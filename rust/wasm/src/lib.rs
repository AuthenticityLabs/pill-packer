//! Small synchronous ABI. Input and index buffers are reusable; JS copies bytes
//! once per network batch, never once per field. All handles belong to this instance.
use pil_codec::{DefinitionDocument, Error, FrameIndex};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
const MAX: usize = 64 * 1024 * 1024;
#[derive(Default)]
struct Codec {
    input: Vec<u8>,
    inflated: Vec<u8>,
    definitions: Vec<Option<(DefinitionDocument, [u8; 32])>>,
    supported: Vec<u32>,
    document: Vec<u8>,
    index: FrameIndex,
    pac: pil_codec::pac::PacIndex,
    error: String,
}
thread_local! { static CODEC: RefCell<Codec> = RefCell::new(Codec::default()); }
#[unsafe(no_mangle)]
pub extern "C" fn input(length: usize) -> *mut u8 {
    CODEC.with_borrow_mut(|c| {
        if length >= u32::MAX as usize {
            c.error = "PIL input exceeds address range".into();
            return std::ptr::null_mut();
        }
        if c.input
            .try_reserve(length.saturating_sub(c.input.len()))
            .is_err()
        {
            c.error = "PIL input allocation failed".into();
            return std::ptr::null_mut();
        }
        c.input.resize(length, 0);
        c.input.as_mut_ptr()
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn definition() -> u32 {
    CODEC.with_borrow_mut(|c| match DefinitionDocument::from_pil(&c.input) {
        Ok(d) => {
            let digest: [u8; 32] = Sha256::digest(d.as_pil()).into();
            let d = (d, digest);
            if let Some(i) = c.definitions.iter().position(Option::is_none) {
                c.definitions[i] = Some(d);
                i as u32
            } else {
                c.definitions.push(Some(d));
                (c.definitions.len() - 1) as u32
            }
        }
        Err(e) => {
            c.error = e.to_string();
            u32::MAX
        }
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn release(handle: usize) {
    CODEC.with_borrow_mut(|c| {
        if let Some(d) = c.definitions.get_mut(handle) {
            *d = None;
        }
    });
}
#[unsafe(no_mangle)]
pub extern "C" fn scan(handle: usize, start: usize, length: usize) -> u32 {
    scan_mode(handle, start, length, false)
}
#[unsafe(no_mangle)]
pub extern "C" fn scan_structural(handle: usize, start: usize, length: usize) -> u32 {
    scan_mode(handle, start, length, true)
}
fn scan_mode(handle: usize, start: usize, length: usize, structural: bool) -> u32 {
    CODEC.with_borrow_mut(|c| {
        let Codec {
            input,
            definitions,
            index,
            error,
            ..
        } = c;
        let result = definitions
            .get(handle)
            .and_then(Option::as_ref)
            .zip(
                start
                    .checked_add(length)
                    .and_then(|end| input.get(start..end)),
            )
            .ok_or(Error::Invalid("invalid WASM handle or range"))
            .and_then(|(d, b)| {
                if structural {
                    index.scan_structural(d.0.definition(), b, MAX)
                } else {
                    d.0.scan(index, b, MAX)
                }
            });
        match result {
            Ok(n) => n as u32,
            Err(Error::Incomplete) => 0,
            Err(e) => {
                *error = e.to_string();
                u32::MAX
            }
        }
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn spans() -> *const [u32; 2] {
    CODEC.with_borrow(|c| c.index.spans().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn scan_pac(
    handle: usize,
    start: usize,
    length: usize,
    additional_extension: u32,
) -> u32 {
    scan_pac_mode(handle, start, length, additional_extension, false)
}
#[unsafe(no_mangle)]
pub extern "C" fn scan_pac_structural(
    handle: usize,
    start: usize,
    length: usize,
    additional_extension: u32,
) -> u32 {
    scan_pac_mode(handle, start, length, additional_extension, true)
}
fn scan_pac_mode(
    handle: usize,
    start: usize,
    length: usize,
    additional_extension: u32,
    structural: bool,
) -> u32 {
    CODEC.with_borrow_mut(|c| {
        let Codec {
            input,
            definitions,
            pac,
            supported,
            error,
            ..
        } = c;
        let result = (|| {
            let d = definitions
                .get(handle)
                .and_then(Option::as_ref)
                .ok_or(Error::Invalid("PAC definition handle"))?;
            let b = start
                .checked_add(length)
                .and_then(|end| input.get(start..end))
                .ok_or(Error::Invalid("PAC input bounds"))?;
            let digest = d.1;
            let extra = additional_extension != 0 && !supported.contains(&additional_extension);
            if extra {
                supported.push(additional_extension);
            }
            let result = if structural {
                pac.scan_structural(
                    d.0.definition(),
                    b,
                    &digest,
                    supported,
                    u32::MAX as usize - 1,
                )
            } else {
                pac.scan(
                    d.0.definition(),
                    b,
                    &digest,
                    supported,
                    u32::MAX as usize - 1,
                )
            };
            if extra {
                supported.pop();
            }
            let n = result?;
            if !structural {
                d.0.validate_pac(pac, b)?;
            }
            Ok(n)
        })();
        match result {
            Ok(n) => n as u32,
            Err(Error::Incomplete) => 0,
            Err(e) => {
                *error = e.to_string();
                u32::MAX
            }
        }
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pac_members() -> *const [u32; 2] {
    CODEC.with_borrow(|c| c.pac.members().as_ptr())
}
#[unsafe(no_mangle)]
pub extern "C" fn error_ptr() -> *const u8 {
    CODEC.with_borrow(|c| c.error.as_ptr())
}
#[unsafe(no_mangle)]
pub extern "C" fn error_len() -> usize {
    CODEC.with_borrow(|c| c.error.len())
}

#[unsafe(no_mangle)]
pub extern "C" fn inflate(length: usize) -> u32 {
    CODEC.with_borrow_mut(|c| {
        match pil_codec::compression::inflate(&c.input, length, &mut c.inflated) {
            Ok(()) => length as u32,
            Err(e) => {
                c.error = e.to_string();
                u32::MAX
            }
        }
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn inflated_ptr() -> *const u8 {
    CODEC.with_borrow(|c| c.inflated.as_ptr())
}

/// Register extension identifiers understood by the host. No application IDs
/// are part of the codec ABI. Call after input() contains little-endian U32s.
#[unsafe(no_mangle)]
pub extern "C" fn extensions() -> u32 {
    CODEC.with_borrow_mut(|c| {
        if c.input.len() % 4 != 0 {
            c.error = "invalid extension list".into();
            return u32::MAX;
        }
        c.supported.clear();
        c.supported.extend(
            c.input
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap())),
        );
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn json_definition() -> u32 {
    CODEC.with_borrow_mut(|c| match DefinitionDocument::from_json(&c.input) {
        Ok(d) => {
            c.document.clear();
            c.document.extend_from_slice(d.as_pil());
            c.document.len() as u32
        }
        Err(e) => {
            c.error = e.to_string();
            u32::MAX
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn document_ptr() -> *const u8 {
    CODEC.with_borrow(|c| c.document.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn pac_count() -> u32 {
    CODEC.with_borrow(|c| c.pac.members().len() as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn verify_crc32c(start: usize, length: usize, expected: u32) -> u32 {
    CODEC.with_borrow_mut(|c| {
        let valid = start
            .checked_add(length)
            .and_then(|end| c.input.get(start..end))
            .map(|b| pil_codec::checksum::crc32c(b) == expected)
            .unwrap_or(false);
        if valid {
            1
        } else {
            c.error = "PAC checksum mismatch".into();
            u32::MAX
        }
    })
}

/// Convert JSON to nested PIL (mode 0) or a complete nested PIL record to JSON
/// (mode 1). Input and output use the existing reusable ABI buffers.
#[unsafe(no_mangle)]
pub extern "C" fn schema_json(handle: usize, mode: u32) -> u32 {
    CODEC.with_borrow_mut(|c| {
        let result = c
            .definitions
            .get(handle)
            .and_then(Option::as_ref)
            .ok_or(Error::Invalid("definition handle"))
            .and_then(|d| match mode {
                0 => d.0.encode_json(&c.input),
                1 => d.0.decode_json(&c.input),
                _ => Err(Error::Invalid("JSON mode")),
            });
        match result {
            Ok(b) => {
                c.document = b;
                c.document.len() as u32
            }
            Err(e) => {
                c.error = e.to_string();
                u32::MAX
            }
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn deflate(level:u32)->u32 { CODEC.with_borrow_mut(|c| {match pil_codec::compression::deflate(&c.input,level,&mut c.inflated) {Ok(())=>c.inflated.len() as u32,Err(e)=>{c.error=e.to_string();u32::MAX}}}) }
