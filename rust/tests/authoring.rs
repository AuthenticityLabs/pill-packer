#![cfg(feature = "schema")]
use pil_codec::{
    DefinitionDocument,
    authoring::{Draft, parse_samples},
    schema::{SchemaDocument, SchemaType as Ty},
};
use serde_json::json;
fn reviewed(mut d: Draft) -> Draft {
    d.pending.clear();
    d
}
#[test]
fn samples_do_not_create_contracts() {
    let d = Draft::from_json(r#"[{"x":1,"name":"a"},{"x":2}]"#).unwrap();
    assert!(d.bytes().is_err());
    let s = d.schema().unwrap();
    let Ty::OpenRecord { fields } = &s.types["record"] else {
        panic!()
    };
    assert!(fields.iter().all(|f| f.optional));
    assert!(matches!(fields[0].value, Ty::Text));
    assert!(
        matches!(fields[1].value,Ty::Integer{min,max} if min==i64::MIN as i128&&max==i64::MAX as i128)
    );
    assert!(d.dictionaries.is_empty());
    let c = s.compile().unwrap();
    let v = json!({"x":-999999,"name":"never seen","extra":{"other":[true,1,"é"]}});
    assert_eq!(c.decode_json(&c.encode_json(&v).unwrap()).unwrap(), v);
    reviewed(d).bytes().unwrap();
}
#[test]
fn open_maps_preserve_nested_extras_without_overriding_declared_types() {
    let s = SchemaDocument::from_cddl(
        "root = {x: 0..255, nested: { y: tstr, * tstr => any }, * tstr => any}",
        "root",
    )
    .unwrap();
    let c = s.compile().unwrap();
    let v =
        json!({"x":42,"nested":{"y":"a","z":[1,null]},"extra": {"count":18446744073709551615u64}});
    assert_eq!(c.decode_json(&c.encode_json(&v).unwrap()).unwrap(), v);
    assert!(
        c.encode_json(&json!({"x":"wrong","nested":{"y":"a"}}))
            .is_err()
    );
    let closed = SchemaDocument::from_cddl("root = {x: uint}", "root")
        .unwrap()
        .compile()
        .unwrap();
    assert!(closed.encode_json(&json!({"x":1,"extra":2})).is_err());
}
#[test]
fn dictionary_accepts_unseen_strings_and_rx_roundtrips() {
    let mut d = Draft::from_cddl(
        "; author comment\nrecord = {name: tstr, ? absent: (tstr / null)}\n".into(),
        None,
    )
    .unwrap();
    let f = d.fields().unwrap()[0].clone();
    d.dictionary(&f, vec!["repeat".into()]).unwrap();
    let bytes = d.bytes().unwrap();
    let loaded = Draft::from_rx(&bytes).unwrap();
    assert_eq!(loaded.source, d.source);
    assert_eq!(loaded.dictionaries, d.dictionaries);
    let c = loaded.schema().unwrap().compile().unwrap();
    for name in ["repeat", "unseen"] {
        let v = json!({"name":name});
        assert_eq!(c.decode_json(&c.encode_json(&v).unwrap()).unwrap(), v);
    }
    let a = c.encode_json(&json!({"name":"repeat"})).unwrap();
    let b = c.encode_json(&json!({"name":"unseen"})).unwrap();
    assert!(a.len() < b.len());
    DefinitionDocument::from_pil(&bytes).unwrap();
}
#[test]
fn fixed_capacity_is_utf8_bytes_and_preserves_nul_and_spaces() {
    let mut d = Draft::from_cddl("r = {s: tstr .size (0..8)}".into(), None).unwrap();
    let f = d.fields().unwrap()[0].clone();
    d.fixed_capacity(&f, Some(8)).unwrap();
    let c = d.schema().unwrap().compile().unwrap();
    for s in ["éééé", "a\u{0} ", ""] {
        let v = json!({"s":s});
        assert_eq!(c.decode_json(&c.encode_json(&v).unwrap()).unwrap(), v);
    }
    assert!(c.encode_json(&json!({"s":"ééééé"})).is_err());
    assert_eq!(
        Draft::from_rx(&d.bytes().unwrap()).unwrap().fixed_strings,
        d.fixed_strings
    );
}
#[test]
fn missing_and_null_are_independent() {
    let d = Draft::from_json(r#"[{"x":null},{"x":1},{}]"#).unwrap();
    let c = d.schema().unwrap().compile().unwrap();
    for v in [json!({}), json!({"x":null}), json!({"x":-3})] {
        assert_eq!(c.decode_json(&c.encode_json(&v).unwrap()).unwrap(), v);
    }
    d.validate_samples().unwrap();
}
#[test]
fn rejects_rounded_integers_and_duplicate_keys() {
    assert!(parse_samples(r#"{"x":18446744073709551616}"#).is_err());
    assert!(parse_samples(r#"{"x":1,"x":2}"#).is_err());
    let d = Draft::from_json(r#"{"x":18446744073709551615}"#).unwrap();
    d.validate_samples().unwrap();
}
#[test]
fn ranges_choose_width_and_reject_overflow() {
    for (max, width) in [(255, 1), (65535, 2), (16777215, 3), (4294967295i128, 4)] {
        let c = SchemaDocument::from_cddl(&format!("r = {{n: 0..{max}}}"), "r")
            .unwrap()
            .compile()
            .unwrap();
        let raw = c.encode_json(&json!({"n":max as u64})).unwrap();
        assert_eq!(raw.len(), 1 + width);
        assert!(c.encode_json(&json!({"n":max as u64+1})).is_err());
    }
}
#[test]
fn inference_complex_and_explicit_cddl_reopen() {
    let d =
        Draft::from_json(r#"[{"obj":{"one":true},"arr":[{"a":1}]},{"obj":{"two":"x"},"arr":[]} ]"#)
            .unwrap();
    d.validate_samples().unwrap();
    let d = reviewed(d);
    let loaded = Draft::from_rx(&d.bytes().unwrap()).unwrap();
    assert_eq!(
        loaded.schema().unwrap().to_pil().unwrap(),
        d.schema().unwrap().to_pil().unwrap()
    );
}
#[test]
fn output_is_rx_and_overwrite_is_explicit() {
    let dir = std::env::temp_dir().join(format!("doc-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let d = Draft::default();
    assert!(d.save(&dir.join("x.json"), false).is_err());
    let p = dir.join("x.rx");
    d.save(&p, false).unwrap();
    assert!(d.save(&p, false).is_err());
    d.save(&p, true).unwrap();
    Draft::from_rx(&std::fs::read(&p).unwrap()).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn failed_edits_are_transactional() {
    let mut d = Draft::from_cddl("r = {name: tstr}".into(), None).unwrap();
    let f = d.fields().unwrap()[0].clone();
    d.dictionary(&f, vec!["x".into()]).unwrap();
    let before = d.bytes().unwrap();
    assert!(d.apply(&f, Ty::Integer { min: 4, max: 1 }, true).is_err());
    assert_eq!(d.bytes().unwrap(), before);
}
#[test]
fn nested_optional_object_review_has_reachable_evidence() {
    let mut d = Draft::from_json(r#"[{"obj":null},{"obj":{"name":"a"}}]"#).unwrap();
    let fields = d.fields().unwrap();
    let inner = fields.iter().find(|f| f.name == "name").unwrap();
    assert_eq!(inner.evidence_path, "record.obj.name");
    assert!(d.observed.contains_key(&inner.evidence_path));
    d.apply(inner, inner.value.clone(), inner.optional).unwrap();
    let outer = d
        .fields()
        .unwrap()
        .into_iter()
        .find(|f| f.name == "obj")
        .unwrap();
    d.apply(&outer, outer.value.clone(), outer.optional)
        .unwrap();
    assert!(d.pending.is_empty());
    d.bytes().unwrap();
}
#[test]
fn native_rx_retains_annotation_bytes_when_fields_are_edited() {
    use ciborium::Value as V;
    let raw = pil_codec::object::encode_value(&V::Map(vec![
        ("pil".into(), 0.into()),
        ("definition".into(), "native".into()),
        ("version".into(), 0.into()),
        (
            "fields".into(),
            V::Array(vec![V::Map(vec![
                ("name".into(), "x".into()),
                ("type".into(), "U8".into()),
            ])]),
        ),
        ("vendor".into(), V::Bytes(vec![0, 255])),
    ]))
    .unwrap();
    let mut d = Draft::from_rx(&raw).unwrap();
    assert_eq!(d.bytes().unwrap(), raw);
    d.source = d.source.replace("U8", "U16");
    let doc = DefinitionDocument::from_pil(&d.bytes().unwrap()).unwrap();
    assert_eq!(doc.annotation("vendor"), Some(&V::Bytes(vec![0, 255])));
}
#[test]
fn shared_open_dictionary_fixture() {
    let mut d = Draft::from_cddl(
        "record = {state: tstr, count: 0..255, * tstr => any}\n".into(),
        None,
    )
    .unwrap();
    let f = d.fields().unwrap()[0].clone();
    d.dictionary(&f, vec!["ok".into(), "error".into()]).unwrap();
    let raw = d.bytes().unwrap();
    let json =
        r#"{"state":"ok","count":42,"extra":{"text":"café","tags":[1,true,null]}}"#.as_bytes();
    let value: serde_json::Value = serde_json::from_slice(json).unwrap();
    let c = d.schema().unwrap().compile().unwrap();
    let frame = c.encode_json(&value).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/authoring");
    let files = [
        ("record.rx", raw.as_slice()),
        ("record.json", json),
        ("record.pil", frame.as_slice()),
    ];
    if std::env::var_os("PILL_REFRESH_DOC_FIXTURE").is_some() {
        std::fs::create_dir_all(&path).unwrap();
        for (name, bytes) in files {
            std::fs::write(path.join(name), bytes).unwrap();
        }
    }
    for (name, bytes) in files {
        assert_eq!(std::fs::read(path.join(name)).unwrap(), bytes);
    }
    assert_eq!(c.decode_json(&frame).unwrap(), value);
}

#[test]
fn mixed_integer_float_samples_keep_exact_integers() {
    let d = Draft::from_json(r#"[{"n":18446744073709551615},{"n":1.5},{"n":-42}]"#).unwrap();
    d.validate_samples().unwrap();
}
