use crate::Error;
use ciborium::Value;

pub(crate) fn parse(bytes: &[u8]) -> Result<Value, Error> {
    let mut remaining = bytes;
    let value: Value = ciborium::de::from_reader_with_recursion_limit(&mut remaining, 64)
        .map_err(|_| Error::Invalid("malformed CBOR or nesting exceeds 64"))?;
    if !remaining.is_empty() {
        return Err(Error::Invalid("CBOR range contains trailing bytes"));
    }
    Ok(value)
}
