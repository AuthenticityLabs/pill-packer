//! Definition registration is separate from the per-record codec. Keep the
//! original document for forwarding; compiled fields are only its wire layout.
use crate::{Definition, Error, cbor};
use ciborium::Value;

#[derive(Debug, Clone)]
pub struct DefinitionDocument {
    encoded: Vec<u8>,
    value: Value,
    definition: Definition,
}

/// Application and transport profiles validate at registration, not during
/// each codec call. Implementations must not mutate the registered document.
pub trait DefinitionValidator {
    fn validate(&self, document: &DefinitionDocument) -> Result<(), Error>;
}

impl DefinitionDocument {
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, Error> {
        let value = cbor::parse(bytes)?;
        let definition = Definition::from_cbor(bytes)?;
        Ok(Self {
            encoded: bytes.to_vec(),
            value,
            definition,
        })
    }

    /// JSON is an authoring format. Convert once at registration; forwarding
    /// subsequently uses the exact returned CBOR bytes, including annotations.
    #[cfg(feature = "json")]
    pub fn from_json(bytes: &[u8]) -> Result<Self, Error> {
        let value: Value =
            serde_json::from_slice(bytes).map_err(|e| Error::Definition(format!("JSON: {e}")))?;
        validate_json_maps(&value)?;
        let mut encoded = Vec::new();
        ciborium::ser::into_writer(&value, &mut encoded)
            .map_err(|_| Error::Invalid("cannot encode Definition CBOR"))?;
        Self::from_cbor(&encoded)
    }

    pub fn definition(&self) -> &Definition {
        &self.definition
    }
    pub fn value(&self) -> &Value {
        &self.value
    }
    pub fn as_cbor(&self) -> &[u8] {
        &self.encoded
    }
    pub fn annotation(&self, key: &str) -> Option<&Value> {
        self.value
            .as_map()?
            .iter()
            .find_map(|(k, v)| (k.as_text() == Some(key)).then_some(v))
    }
    pub fn validate_with(&self, validators: &[&dyn DefinitionValidator]) -> Result<(), Error> {
        for validator in validators {
            validator.validate(self)?;
        }
        Ok(())
    }
}

#[cfg(feature = "json")]
fn validate_json_maps(value: &Value) -> Result<(), Error> {
    match value {
        Value::Map(entries) => {
            let mut names = std::collections::HashSet::new();
            for (key, value) in entries {
                if !names.insert(
                    key.as_text()
                        .ok_or(Error::Invalid("JSON key must be text"))?,
                ) {
                    return Err(Error::Definition("duplicate JSON object key".into()));
                }
                validate_json_maps(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_json_maps(value)?;
            }
        }
        _ => (),
    }
    Ok(())
}
