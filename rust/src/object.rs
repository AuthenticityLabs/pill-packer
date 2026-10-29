//! Native, bounded PIL values for open objects and Definition annotations.
//! The layout is a codec bootstrap, not a remotely fetched Definition.
use crate::{Array, Definition, Error, Field, Scalar, Type, Value, decode_exact, encode};
use ciborium::Value as Data;
use std::sync::OnceLock;

const LIMIT: usize = 16 << 20;
const DEPTH: usize = 64;
/// Fixed bootstrap layout for native values and `.rx` Definition documents.
/// Its own `.rx` file is encoded with this same layout, without schema lookup.
pub fn definition() -> &'static Definition {
    static LAYOUT: OnceLock<Definition> = OnceLock::new();
    LAYOUT.get_or_init(|| {
        Definition::new(
            "pil/value",
            0,
            vec![
                Field::new("kind", Scalar::U8),
                Field::new("unsigned", Scalar::U64),
                Field::new("negative", Scalar::I64),
                Field::new("float", Scalar::F64),
                Field::new("text", Scalar::String),
                Field::new("bytes", Scalar::Bytes),
                Field::new("children", Type::Array(Scalar::Bytes)),
            ],
        )
        .expect("built-in value layout")
    })
}

/// Encode an open value as PIL. CBOR is not used on the wire.
pub fn encode_value(value: &Data) -> Result<Vec<u8>, Error> {
    encode_at(value, 0)
}
fn encode_at(value: &Data, depth: usize) -> Result<Vec<u8>, Error> {
    if depth > DEPTH {
        return Err(Error::Invalid("PIL value nesting limit"));
    }
    let mut fields = [Value::Absent; 7];
    let mut children = Vec::new();
    fields[0] = Value::U8(match value {
        Data::Null => 0,
        Data::Bool(false) => 1,
        Data::Bool(true) => 2,
        Data::Integer(n) => {
            let n: i128 = (*n).into();
            if n >= 0 {
                fields[1] = Value::U64(u64::try_from(n).map_err(|_| Error::Overflow)?);
                3
            } else {
                fields[2] = Value::I64(i64::try_from(n).map_err(|_| Error::Overflow)?);
                4
            }
        }
        Data::Float(n) => {
            fields[3] = Value::F64(*n);
            5
        }
        Data::Text(s) => {
            fields[4] = Value::String(s);
            6
        }
        Data::Bytes(b) => {
            fields[5] = Value::Bytes(b);
            7
        }
        Data::Array(items) => {
            let mut size = 0usize;
            for item in items {
                let b = encode_at(item, depth + 1)?;
                size = size.checked_add(b.len()).ok_or(Error::Overflow)?;
                if size > LIMIT {
                    return Err(Error::Overflow);
                }
                children.push(b);
            }
            8
        }
        Data::Map(entries) => {
            let mut keys = std::collections::HashSet::new();
            let mut pairs = Vec::new();
            let mut size = 0usize;
            for (key, value) in entries {
                if !matches!(key, Data::Text(_) | Data::Integer(_)) {
                    return Err(Error::Invalid("PIL object key must be text or integer"));
                }
                let key = encode_at(key, depth + 1)?;
                if !keys.insert(key.clone()) {
                    return Err(Error::Invalid("duplicate PIL object key"));
                }
                let value = encode_at(value, depth + 1)?;
                size = size
                    .checked_add(key.len())
                    .and_then(|n| n.checked_add(value.len()))
                    .ok_or(Error::Overflow)?;
                if size > LIMIT {
                    return Err(Error::Overflow);
                }
                pairs.push((key, value));
            }
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            for (key, value) in pairs {
                children.push(key);
                children.push(value);
            }
            9
        }
        _ => {
            return Err(Error::Invalid(
                "tagged values are not part of the PIL value profile",
            ));
        }
    });
    let items: Vec<_> = children.iter().map(|b| Value::Bytes(b)).collect();
    if matches!(value, Data::Array(_) | Data::Map(_)) {
        fields[6] = Value::Array(Array::Items(&items));
    }
    let bytes = encode(definition(), &fields)?;
    if bytes.len() > LIMIT {
        return Err(Error::Overflow);
    }
    Ok(bytes)
}

/// Decode the codec's bootstrap value layout. No schema lookup is needed.
pub fn decode_value(bytes: &[u8]) -> Result<Data, Error> {
    if bytes.len() > LIMIT {
        return Err(Error::Invalid("PIL value exceeds byte limit"));
    }
    decode_at(bytes, 0)
}
fn decode_at(bytes: &[u8], depth: usize) -> Result<Data, Error> {
    if depth > DEPTH {
        return Err(Error::Invalid("PIL value nesting limit"));
    }
    let frame = decode_exact(definition(), bytes)?;
    let f = frame.values();
    let Value::U8(kind) = f[0] else {
        return Err(Error::Invalid("missing PIL value kind"));
    };
    let active = match kind {
        0..=2 => 0,
        3 => 1,
        4 => 2,
        5 => 3,
        6 => 4,
        7 => 5,
        8..=9 => 6,
        _ => return Err(Error::Invalid("unknown PIL value kind")),
    };
    for (i, value) in f.iter().enumerate().skip(1) {
        if i != active && *value != Value::Absent {
            return Err(Error::Invalid("unexpected PIL value payload"));
        }
    }
    Ok(match (kind, f[active]) {
        (0, _) => Data::Null,
        (1, _) => Data::Bool(false),
        (2, _) => Data::Bool(true),
        (3, Value::U64(n)) => Data::Integer(n.into()),
        (4, Value::I64(n)) if n < 0 => Data::Integer(n.into()),
        (5, Value::F64(n)) => Data::Float(n),
        (6, Value::String(s)) => Data::Text(s.into()),
        (7, Value::Bytes(b)) => Data::Bytes(b.into()),
        (8..=9, Value::Array(Array::Encoded(array))) => {
            let mut items = Vec::with_capacity(array.len());
            for item in array.iter() {
                let Value::Bytes(b) = item? else {
                    unreachable!()
                };
                items.push(decode_at(b, depth + 1)?);
            }
            if kind == 8 {
                Data::Array(items)
            } else {
                if items.len() % 2 != 0 {
                    return Err(Error::Invalid("unpaired PIL object member"));
                }
                let mut pairs = Vec::with_capacity(items.len() / 2);
                let mut keys = std::collections::HashSet::new();
                let mut items = items.into_iter();
                while let Some(key) = items.next() {
                    if !matches!(key, Data::Text(_) | Data::Integer(_)) {
                        return Err(Error::Invalid("PIL object key must be text or integer"));
                    }
                    if !keys.insert(encode_value(&key)?) {
                        return Err(Error::Invalid("duplicate PIL object key"));
                    }
                    pairs.push((key, items.next().unwrap()));
                }
                Data::Map(pairs)
            }
        }
        _ => return Err(Error::Invalid("PIL value payload does not match its kind")),
    })
}

pub(crate) fn dynamic_fields(bytes: &[u8], definition: &Definition) -> Result<(), Error> {
    let Data::Map(entries) = decode_value(bytes)? else {
        return Err(Error::Invalid("@fields must be a PIL object"));
    };
    for (key, _) in entries {
        let Data::Text(name) = key else {
            return Err(Error::Invalid("@fields keys must be text"));
        };
        if name.starts_with('@') || definition.fields().iter().any(|f| f.name == name) {
            return Err(Error::Invalid(
                "dynamic attribute collides with defined or reserved field",
            ));
        }
    }
    Ok(())
}
