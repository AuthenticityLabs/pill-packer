//! C callers own handle lifetime and serialize calls on a handle. Input is
//! borrowed only for the call; definitions and reusable scan buffers are Rust-owned.
use pil_codec::{DefinitionDocument, Error, FrameIndex};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[derive(Default)]
pub struct Context {
    document: Option<DefinitionDocument>,
    index: FrameIndex,
    error: String,
    output: Vec<u8>,
}

#[unsafe(no_mangle)]
pub extern "C" fn pp_new() -> *mut Context {
    Box::into_raw(Box::default())
}

/// # Safety
/// ctx must be a live handle from pp_new, freed exactly once with no active calls.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pp_free(ctx: *mut Context) {
    if !ctx.is_null() {
        drop(unsafe { Box::from_raw(ctx) });
    }
}

fn invoke(ctx: &mut Context, f: impl FnOnce(&mut Context) -> Result<i64, Error>) -> i64 {
    match catch_unwind(AssertUnwindSafe(|| f(ctx))) {
        Ok(Ok(n)) => n,
        Ok(Err(Error::Incomplete)) => -2,
        Ok(Err(e)) => {
            ctx.error = e.to_string();
            -1
        }
        Err(_) => {
            ctx.error = "PIL operation panicked".into();
            -1
        }
    }
}

unsafe fn input<'a>(bytes: *const u8, length: usize) -> Result<&'a [u8], Error> {
    if length == 0 {
        return Ok(&[]);
    }
    if bytes.is_null() || length > isize::MAX as usize {
        return Err(Error::Invalid("invalid C input"));
    }
    Ok(unsafe { std::slice::from_raw_parts(bytes, length) })
}

/// # Safety
/// ctx is live and exclusively borrowed; bytes points to length readable bytes.
/// format: 0 = PIL, 1 = JSON. No pointer into input survives the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pp_define(
    ctx: *mut Context,
    bytes: *const u8,
    length: usize,
    format: u8,
) -> i64 {
    let Some(ctx) = (unsafe { ctx.as_mut() }) else {
        return -1;
    };
    invoke(ctx, |ctx| {
        let bytes = unsafe { input(bytes, length)? };
        let document = match format {
            0 => DefinitionDocument::from_pil(bytes)?,
            1 => DefinitionDocument::from_json(bytes)?,
            _ => return Err(Error::Invalid("unknown definition format")),
        };
        ctx.document = Some(document);
        Ok(0)
    })
}

/// # Safety
/// Same handle and slice requirements as pp_define. Returns consumed bytes,
/// -2 for incomplete input, -1 for invalid data. Reuses field-span allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pp_scan(
    ctx: *mut Context,
    bytes: *const u8,
    length: usize,
    maximum: usize,
) -> i64 {
    let Some(ctx) = (unsafe { ctx.as_mut() }) else {
        return -1;
    };
    invoke(ctx, |ctx| {
        let bytes = unsafe { input(bytes, length)? };
        let d = ctx
            .document
            .as_ref()
            .ok_or(Error::Invalid("definition not loaded"))?;
        Ok(d.scan(&mut ctx.index, bytes, maximum)? as i64)
    })
}

/// # Safety
/// A live shared handle is required. Result valid until define/free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pp_document(ctx: *const Context, length: *mut usize) -> *const u8 {
    if length.is_null() {
        return std::ptr::null();
    }
    unsafe {
        *length = 0;
    }
    let Some(ctx) = (unsafe { ctx.as_ref() }) else {
        return std::ptr::null();
    };
    let Some(d) = ctx.document.as_ref() else {
        return std::ptr::null();
    };
    unsafe {
        *length = d.as_pil().len();
    }
    d.as_pil().as_ptr()
}

/// # Safety
/// A live shared handle and writable length pointer are required. Copy the
/// returned bytes before the next mutating call or pp_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pp_error(ctx: *const Context, length: *mut usize) -> *const u8 {
    if length.is_null() {
        return std::ptr::null();
    }
    unsafe {
        *length = 0;
    }
    let Some(ctx) = (unsafe { ctx.as_ref() }) else {
        return std::ptr::null();
    };
    unsafe {
        *length = ctx.error.len();
    }
    ctx.error.as_ptr()
}

/// # Safety
/// Same input and handle requirements as pp_scan. Output is borrowed until the
/// next mutation. mode 0 encodes JSON, mode 1 decodes a complete PIL record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pp_json(
    ctx: *mut Context,
    bytes: *const u8,
    length: usize,
    mode: u8,
) -> i64 {
    let Some(ctx) = (unsafe { ctx.as_mut() }) else {
        return -1;
    };
    invoke(ctx, |ctx| {
        let input = unsafe { input(bytes, length)? };
        let d = ctx
            .document
            .as_ref()
            .ok_or(Error::Invalid("definition not loaded"))?;
        ctx.output = match mode {
            0 => d.encode_json(input)?,
            1 => d.decode_json(input)?,
            _ => return Err(Error::Invalid("unknown JSON operation")),
        };
        Ok(ctx.output.len() as i64)
    })
}
/// # Safety
/// A live handle and writable length pointer are required. Copy before mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pp_output(ctx: *const Context, length: *mut usize) -> *const u8 {
    if length.is_null() {
        return std::ptr::null();
    }
    unsafe {
        *length = 0;
    }
    let Some(ctx) = (unsafe { ctx.as_ref() }) else {
        return std::ptr::null();
    };
    unsafe {
        *length = ctx.output.len();
    }
    ctx.output.as_ptr()
}
