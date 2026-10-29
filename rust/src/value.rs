use crate::{Error, Scalar, array::Array};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value<'a> {
    Absent,
    Null,
    Bool(bool),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U24(u32),
    Ref16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Uuid([u8; 16]),
    TimestampMs(i64),
    TimestampUs(i64),
    TimestampNs(i64),
    Mapped(u64),
    String(&'a str),
    Bytes(&'a [u8]),
    Object(&'a [u8]),
    Array(Array<'a>),
}

impl Value<'_> {
    pub fn scalar(self) -> Option<Scalar> {
        Some(match self {
            Self::Bool(_) => Scalar::Bool,
            Self::I8(_) => Scalar::I8,
            Self::I16(_) => Scalar::I16,
            Self::I32(_) => Scalar::I32,
            Self::I64(_) => Scalar::I64,
            Self::U8(_) => Scalar::U8,
            Self::U16(_) => Scalar::U16,
            Self::U24(_) => Scalar::U24,
            Self::Ref16(_) => Scalar::Ref16,
            Self::U32(_) => Scalar::U32,
            Self::U64(_) => Scalar::U64,
            Self::F32(_) => Scalar::F32,
            Self::F64(_) => Scalar::F64,
            Self::Uuid(_) => Scalar::Uuid,
            Self::TimestampMs(_) => Scalar::TimestampMs,
            Self::TimestampUs(_) => Scalar::TimestampUs,
            Self::TimestampNs(_) => Scalar::TimestampNs,
            Self::String(_) => Scalar::String,
            Self::Bytes(_) => Scalar::Bytes,
            Self::Object(_) => Scalar::Object,
            Self::Absent | Self::Null | Self::Mapped(_) | Self::Array(_) => return None,
        })
    }
}

pub(crate) fn write_as(kind: Scalar, value: Value<'_>, output: &mut Vec<u8>) -> Result<(), Error> {
    if let Scalar::FixedString(capacity) = kind {
        let Value::String(text) = value else {
            return Err(Error::Invalid("fixed STRING requires text"));
        };
        if text.len() > capacity as usize {
            return Err(Error::Invalid("STRING exceeds byte capacity"));
        }
        let end = output
            .len()
            .checked_add(capacity as usize + Scalar::string_length_width(capacity))
            .ok_or(Error::Overflow)?;
        output
            .try_reserve(end - output.len())
            .map_err(|_| Error::Overflow)?;
        output.extend_from_slice(
            &(text.len() as u32).to_le_bytes()[..Scalar::string_length_width(capacity)],
        );
        output.extend_from_slice(text.as_bytes());
        output.resize(end, 0);
        return Ok(());
    }
    write(value, output)
}

pub(crate) fn write(value: Value<'_>, output: &mut Vec<u8>) -> Result<(), Error> {
    macro_rules! fixed {
        ($($kind:ident),* $(,)?) => {
            match value {
                $(Value::$kind(v) => output.extend_from_slice(&v.to_le_bytes()),)*
                Value::U24(v) => { if v > 0xffffff { return Err(Error::Overflow); } output.extend_from_slice(&v.to_le_bytes()[..3]); },
                Value::Bool(v) => output.push(u8::from(v)),
                Value::Uuid(v) => output.extend_from_slice(&v),
                Value::String(v) => output.extend_from_slice(v.as_bytes()),
                Value::Bytes(v) => output.extend_from_slice(v),
                Value::Object(v) => { crate::object::decode_value(v)?; output.extend_from_slice(v); },
                _ => return Err(Error::Invalid("expected a scalar value")),
            }
        };
    }
    fixed!(
        I8,
        I16,
        I32,
        I64,
        U8,
        U16,
        Ref16,
        U32,
        U64,
        F32,
        F64,
        TimestampMs,
        TimestampUs,
        TimestampNs
    );
    Ok(())
}

pub(crate) fn read(kind: Scalar, bytes: &[u8], validate_object: bool) -> Result<Value<'_>, Error> {
    macro_rules! fixed {
        ($($kind:ident : $native:ty),* $(,)?) => {
            match kind {
                $(Scalar::$kind => Value::$kind(<$native>::from_le_bytes(bytes.try_into().map_err(|_| Error::Invalid("wrong scalar width"))?)),)*
                Scalar::U24 => { let [a,b,c] = bytes else { return Err(Error::Invalid("wrong U24 width")); }; Value::U24(u32::from_le_bytes([*a,*b,*c,0])) },
                Scalar::Bool => match bytes {
                    [0] => Value::Bool(false), [1] => Value::Bool(true),
                    _ => return Err(Error::Invalid("invalid BOOL")),
                },
                Scalar::Uuid => Value::Uuid(bytes.try_into().map_err(|_| Error::Invalid("wrong UUID width"))?),
                Scalar::String => Value::String(std::str::from_utf8(bytes).map_err(|_| Error::Invalid("invalid UTF-8"))?),
                Scalar::FixedString(capacity) => {
                    let prefix = Scalar::string_length_width(capacity);
                    if bytes.len() != capacity as usize + prefix { return Err(Error::Invalid("wrong fixed STRING width")); }
                    let mut length = [0u8; 4];
                    length[..prefix].copy_from_slice(&bytes[..prefix]);
                    let length = u32::from_le_bytes(length) as usize;
                    if length > capacity as usize { return Err(Error::Invalid("STRING length exceeds capacity")); }
                    if bytes[prefix + length..].iter().any(|&b| b != 0) { return Err(Error::Invalid("nonzero STRING padding")); }
                    Value::String(std::str::from_utf8(&bytes[prefix..prefix + length]).map_err(|_| Error::Invalid("invalid UTF-8"))?)
                },
                Scalar::Bytes => Value::Bytes(bytes),
                Scalar::Object => {if validate_object {crate::object::decode_value(bytes)?;} Value::Object(bytes)},

            }
        };
    }
    Ok(
        fixed!(I8:i8, I16:i16, I32:i32, I64:i64, U8:u8, U16:u16, Ref16:u16, U32:u32, U64:u64, F32:f32, F64:f64, TimestampMs:i64, TimestampUs:i64, TimestampNs:i64),
    )
}
