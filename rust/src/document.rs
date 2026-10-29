//! Definition registration is separate from the per-record codec. Keep the
//! original document for forwarding; compiled fields are only its wire layout.
use crate::{Definition, Error};
use ciborium::Value;

#[derive(Debug, Clone)]
pub struct DefinitionDocument {
    encoded: Vec<u8>,
    value: Value,
    definition: Definition,
    #[cfg(feature = "schema-runtime")]
    schema: Option<crate::schema::CompiledSchema>,
}

/// Application and transport profiles validate at registration, not during
/// each codec call. Implementations must not mutate the registered document.
pub trait DefinitionValidator {
    fn validate(&self, document: &DefinitionDocument) -> Result<(), Error>;
}

impl DefinitionDocument {
    pub fn from_pil(bytes: &[u8]) -> Result<Self, Error> {
        let value = crate::object::decode_value(bytes)?;
        let definition = Definition::from_pil(bytes)?;
        #[cfg(not(feature = "schema-runtime"))]
        if value
            .as_map()
            .is_some_and(|m| m.iter().any(|(k, _)| k.as_text() == Some("cddl")))
        {
            return Err(Error::Definition(
                "CDDL definitions require schema-runtime support".into(),
            ));
        }

        #[cfg(feature = "schema-runtime")]
        let schema = if value
            .as_map()
            .is_some_and(|m| m.iter().any(|(k, _)| k.as_text() == Some("cddl")))
        {
            Some(crate::schema::SchemaDocument::from_value(&value, &definition)?.compile()?)
        } else {
            None
        };
        Ok(Self {
            encoded: bytes.to_vec(),
            value,
            definition,
            #[cfg(feature = "schema-runtime")]
            schema,
        })
    }

    /// JSON is an authoring format. Convert once at registration; forwarding
    /// subsequently uses the exact returned PIL bytes, including annotations.
    #[cfg(feature = "json")]
    pub fn from_json(bytes: &[u8]) -> Result<Self, Error> {
        let value: Value =
            serde_json::from_slice(bytes).map_err(|e| Error::Definition(format!("JSON: {e}")))?;
        validate_json_maps(&value)?;
        let encoded = crate::object::encode_value(&value)?;
        Self::from_pil(&encoded)
    }

    /// Validate the complete schema after indexing the outer frame. Trusted callers
    /// may use FrameIndex directly once ingress has accepted the record.
    pub fn scan(
        &self,
        index: &mut crate::FrameIndex,
        bytes: &[u8],
        maximum: usize,
    ) -> Result<usize, Error> {
        let n = index.scan(&self.definition, bytes, maximum)?;
        #[cfg(feature = "schema-runtime")]
        if let Some(schema) = &self.schema {
            schema.decode_json(&bytes[..n])?;
        }
        Ok(n)
    }
    #[cfg(feature = "schema-runtime")]
    pub fn encode_json(&self, bytes: &[u8]) -> Result<Vec<u8>, Error> {
        let schema = self
            .schema
            .as_ref()
            .ok_or(Error::Invalid("definition has no CDDL schema"))?;
        let value = serde_json::from_slice(bytes).map_err(|e| Error::Definition(e.to_string()))?;
        schema.encode_json(&value)
    }
    #[cfg(feature = "schema-runtime")]
    pub fn decode_json(&self, bytes: &[u8]) -> Result<Vec<u8>, Error> {
        let schema = self
            .schema
            .as_ref()
            .ok_or(Error::Invalid("definition has no CDDL schema"))?;
        serde_json::to_vec(&schema.decode_json(bytes)?)
            .map_err(|e| Error::Definition(e.to_string()))
    }
    pub fn validate_pac(&self, index: &crate::pac::PacIndex, bytes: &[u8]) -> Result<(), Error> {
        #[cfg(feature = "schema-runtime")]
        if let Some(schema) = &self.schema {
            let [s, e] = index.common();
            let common = if e > s {
                Some(crate::decode_exact(
                    &self.definition,
                    &bytes[s as usize..e as usize],
                )?)
            } else {
                None
            };
            for &[s, e] in index.members() {
                let frame = crate::decode_exact(&self.definition, &bytes[s as usize..e as usize])?;
                let mut values = frame.values().to_vec();
                if let Some(common) = &common {
                    for (i, value) in common.values().iter().enumerate() {
                        if *value != crate::Value::Absent {
                            if values[i] != crate::Value::Absent {
                                return Err(Error::Invalid("PAC override"));
                            };
                            values[i] = *value
                        }
                    }
                }
                schema.decode_json(&crate::encode(&self.definition, &values)?)?;
            }
        }
        Ok(())
    }
    pub fn definition(&self) -> &Definition {
        &self.definition
    }
    pub fn value(&self) -> &Value {
        &self.value
    }
    pub fn as_pil(&self) -> &[u8] {
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
