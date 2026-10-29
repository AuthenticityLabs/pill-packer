use pil_codec::{Definition, DefinitionDocument, DefinitionValidator, Error, Field, Scalar};

const SCHEMA: &str = include_str!("../../schemas/definition.cddl");

#[test]
fn emitted_definition_conforms_to_cddl() {
    let d = Definition::new(
        "example",
        0,
        vec![
            Field::new("id", Scalar::Uuid),
            Field::mapped("status", ["ready", "done"]),
        ],
    )
    .unwrap();
    validate(SCHEMA, &d.to_pil().unwrap()).unwrap();
    let adaptive = d.with_adaptive_integers(pil_codec::AdaptiveIntegers::From16);
    validate(SCHEMA, &adaptive.to_pil().unwrap()).unwrap();
}

#[cfg(feature = "json")]
#[test]
fn annotations_survive_and_profiles_are_explicit() {
    let json = br#"{"pil":0,"definition":"test","version":0,"fields":[{"name":"id","type":"UUID","ui":{"label":"Identity"}}],"profile":{"identity":"id"},"vendor":{"transport":"jms"}}"#;
    cddl::validate_json_from_str(SCHEMA, std::str::from_utf8(json).unwrap(), None).unwrap();
    let doc = DefinitionDocument::from_json(json).unwrap();
    validate(SCHEMA, doc.as_pil()).unwrap();
    let forwarded = DefinitionDocument::from_pil(doc.as_pil()).unwrap();
    assert_eq!(forwarded.as_pil(), doc.as_pil());
    assert_eq!(forwarded.value(), doc.value());
    assert!(forwarded.annotation("profile").is_some());
    struct NeedsTimezone;
    impl DefinitionValidator for NeedsTimezone {
        fn validate(&self, doc: &DefinitionDocument) -> Result<(), Error> {
            if doc.annotation("timezone").is_none() {
                return Err(Error::Definition("profile requires timezone".into()));
            }
            Ok(())
        }
    }
    assert!(doc.validate_with(&[&NeedsTimezone]).is_err());
    assert!(doc.validate_with(&[]).is_ok());
}

#[cfg(feature = "json")]
#[test]
fn invalid_core_cannot_hide_in_extension_keys() {
    for json in [
        r#"{"pil":9,"definition":"test","version":0,"fields":[{"name":"x","type":"U16"}]}"#,
        r#"{"pil":0,"definition":"","version":0,"fields":[{"name":"x","type":"U16"}]}"#,
        r#"{"pil":0,"definition":"test","version":0,"fields":[{"name":"x","type":"U16","values":["bad"]}]}"#,
        r#"{"pil":0,"definition":"test","version":0,"fields":[{"name":"x","type":"UNKNOWN"}]}"#,
    ] {
        if !json.contains("values") {
            assert!(
                cddl::validate_json_from_str(SCHEMA, json, None).is_err(),
                "{json}"
            );
        }
        assert!(
            DefinitionDocument::from_json(json.as_bytes()).is_err(),
            "{json}"
        );
    }
    for json in [
        r#"{"pil":0,"definition":"test","version":0,"fields":[{"name":"x","type":"U16"}],"vendor":{"a":1,"a":2}}"#,
        r#"{"pil":0,"definition":"test","version":0,"fields":[{"name":"x","type":"U16"}],"version":1}"#,
    ] {
        assert!(DefinitionDocument::from_json(json.as_bytes()).is_err());
    }
}

#[cfg(feature = "json")]
#[test]
fn json_preserves_u64_without_javascript_rounding() {
    let doc = DefinitionDocument::from_json(br#"{"pil":0,"definition":"test","version":18446744073709551615,"fields":[{"name":"x","type":"U64"}]}"#).unwrap();
    assert_eq!(doc.definition().version(), u64::MAX);
}

fn validate(schema: &str, pil: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let value = pil_codec::object::decode_value(pil)?;
    let mut oracle = Vec::new();
    ciborium::ser::into_writer(&value, &mut oracle)?;
    cddl::validate_cbor_from_slice(schema, &oracle, None)?;
    Ok(())
}
