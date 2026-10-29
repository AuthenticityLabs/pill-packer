#![cfg(feature = "schema")]
use pil_codec::{
    Scalar,
    object::{decode_value, encode_value},
    schema::SchemaDocument,
};
use serde_json::json;
#[test]
fn cddl_nested_pil_and_json_editing_roundtrip() {
    let source = include_str!("../examples/nested_objects.cddl");
    let schema = SchemaDocument::from_cddl(source, "purchase").unwrap();
    let schema = SchemaDocument::from_json(&schema.to_json().unwrap()).unwrap();
    let schema = SchemaDocument::from_pil(&schema.to_pil().unwrap()).unwrap();
    let codec = schema.compile().unwrap();
    let data = json!({"id":18446744073709551615u64,"customer":{"id":7,"name":"Zoë","address":{"city":"東京","postal":"100-0001"}},"items":[{"sku":"a","quantity":2,"unit_price":42}],"note":null});
    let bytes = codec.encode_json(&data).unwrap();
    assert_eq!(codec.decode_json(&bytes).unwrap(), data);
    assert_eq!(codec.decode_trusted_json(&bytes).unwrap(), data);
    let mut invalid = data.clone();
    invalid["items"][0]["quantity"] = json!(65536);
    assert!(codec.encode_json(&invalid).is_err());
    invalid = data.clone();
    invalid["customer"]["extra"] = json!(true);
    assert!(codec.encode_json(&invalid).is_err());
    invalid = data.clone();
    invalid.as_object_mut().unwrap().remove("customer");
    assert!(codec.encode_json(&invalid).is_err());
    let mut partial = data.clone();
    partial.as_object_mut().unwrap().remove("note");
    assert_eq!(
        codec
            .decode_json(&codec.encode_json(&partial).unwrap())
            .unwrap(),
        partial
    );
    for n in 0..bytes.len() {
        assert!(codec.decode_json(&bytes[..n]).is_err());
    }
}
#[test]
fn byte_capacity_is_distinct_from_cddl_size() {
    let mut schema = SchemaDocument::from_cddl(
        "record = { label: tstr .size (0..300), exact: tstr .size 4 }",
        "record",
    )
    .unwrap();
    schema.fixed_strings.insert("record.label".into(), 300);
    schema.fixed_strings.insert("record.exact".into(), 8);
    let codec = schema.compile().unwrap();
    assert_eq!(
        codec.layout().fields()[0].kind,
        Scalar::FixedString(300).into()
    );
    let value = json!({"label":"é\u{0} ","exact":"éé"});
    let bytes = codec.encode_json(&value).unwrap();
    assert_eq!(codec.decode_json(&bytes).unwrap(), value);
    assert!(
        codec
            .encode_json(&json!({"label":"ok","exact":"four!"}))
            .is_err()
    );
    assert!(
        codec
            .encode_json(&json!({"label":"é".repeat(151),"exact":"four"}))
            .is_err()
    );
    schema.fixed_strings.insert("record.label".into(), 255);
    assert!(schema.compile().is_err());
}
#[test]
fn rejects_unsupported_or_invalid_schema_without_relaxing_it() {
    for source in [
        "root = bstr .cbor uint",
        "root = tstr .regexp \"x\"",
        "root = { x: unknown }",
        "root = { next: root }",
        "root = { * tstr => uint }",
        "root = int",
    ] {
        assert!(
            SchemaDocument::from_cddl(source, "root").is_err(),
            "{source}"
        );
    }
}
#[test]
fn native_values_preserve_annotation_types_without_cbor_wire() {
    use ciborium::Value as V;
    let value = V::Map(vec![(
        V::Text("extension".into()),
        V::Array(vec![
            V::from(u64::MAX),
            V::from(i64::MIN),
            V::Text("a\0 ".into()),
            V::Bytes(vec![0, 255]),
            V::Null,
            V::Bool(false),
        ]),
    )]);
    let bytes = encode_value(&value).unwrap();
    assert_eq!(decode_value(&bytes).unwrap(), value);
    for n in 0..bytes.len() {
        assert!(decode_value(&bytes[..n]).is_err());
    }
    assert!(
        encode_value(&V::Map(vec![
            (V::from("x"), V::Null),
            (V::from("x"), V::Null)
        ]))
        .is_err()
    );
}

#[test]
fn native_value_wire_matches_go_fixture() {
    let text = include_str!("../../npm/fixtures/object.hex").trim();
    let bytes: Vec<u8> = text
        .as_bytes()
        .chunks_exact(2)
        .map(|s| u8::from_str_radix(std::str::from_utf8(s).unwrap(), 16).unwrap())
        .collect();
    let value = decode_value(&bytes).unwrap();
    assert_eq!(encode_value(&value).unwrap(), bytes);
    let ciborium::Value::Map(fields) = value else {
        panic!("expected object")
    };
    let max = fields
        .iter()
        .find(|(k, _)| k.as_text() == Some("max"))
        .unwrap()
        .1
        .as_integer()
        .unwrap();
    assert_eq!(u64::try_from(max).unwrap(), u64::MAX);
}

#[test]
fn compiled_cddl_documents_are_portable_definitions_with_checked_layouts() {
    let schema =
        SchemaDocument::from_cddl("root = { name: tstr, nested: { count: uint } }", "root")
            .unwrap();
    let bytes = schema.to_pil().unwrap();
    let document = pil_codec::DefinitionDocument::from_pil(&bytes).unwrap();
    assert_eq!(document.definition(), schema.compile().unwrap().layout());
    assert!(document.annotation("cddl").unwrap().as_map().is_some());
    let mut json: serde_json::Value =
        serde_json::from_slice(&serde_json::to_vec(document.value()).unwrap()).unwrap();
    json["fields"][0]["type"] = json!("U64");
    assert!(pil_codec::DefinitionDocument::from_json(&serde_json::to_vec(&json).unwrap()).is_err());
}

#[test]
fn shared_nested_runtime_fixture() {
    use pil_codec::{
        Array, ArrayView, DefinitionDocument, FrameIndex, Value, decode_exact, encode,
    };
    let schema=SchemaDocument::from_cddl(r#"
      purchase = {id: uint, customer: customer, items: [1*3 item], ? note: tstr / null, kind: "purchase"}
      customer = {name: tstr .size (1..32), address: {city: tstr}}
      item = {sku: tstr, quantity: 0..999}
    "#,"purchase").unwrap();
    let rx = schema.to_pil().unwrap();
    let compiled = schema.compile().unwrap();
    let json=r#"{"id":18446744073709551615,"customer":{"name":"Zoë","address":{"city":"東京"}},"items":[{"sku":"a","quantity":2}],"note":null,"kind":"purchase"}"#.as_bytes();
    let document = DefinitionDocument::from_pil(&rx).unwrap();
    let frame = document.encode_json(json).unwrap();
    // A structurally valid outer frame containing an out-of-range nested value.
    let root = decode_exact(compiled.layout(), &frame).unwrap();
    let Value::Bytes(items) = root.values()[2] else {
        panic!()
    };
    let items_layout = pil_codec::Definition::new(
        "items",
        0,
        vec![pil_codec::Field::new(
            "items",
            pil_codec::Type::Array(Scalar::Bytes),
        )],
    )
    .unwrap();
    let arrayframe = decode_exact(&items_layout, items).unwrap();
    let Value::Array(Array::Encoded(array)) = arrayframe.values()[0] else {
        panic!()
    };
    let Value::Bytes(item) = array.iter().next().unwrap().unwrap() else {
        panic!()
    };
    let item_layout = pil_codec::Definition::new(
        "item",
        0,
        vec![
            pil_codec::Field::new("sku", Scalar::String),
            pil_codec::Field::new("quantity", Scalar::U16),
        ],
    )
    .unwrap();
    let item = decode_exact(&item_layout, item).unwrap();
    let baditem = encode(&item_layout, &[item.values()[0], Value::U16(1000)]).unwrap();
    let badarray = encode(
        &items_layout,
        &[Value::Array(Array::Items(&[Value::Bytes(&baditem)]))],
    )
    .unwrap();
    let mut values = root.values().to_vec();
    values[2] = Value::Bytes(&badarray);
    let invalid = encode(compiled.layout(), &values).unwrap();
    let mut index = FrameIndex::default();
    assert!(index.scan(compiled.layout(), &invalid, 1 << 20).is_ok());
    assert!(document.scan(&mut index, &invalid, 1 << 20).is_err());
    assert_eq!(
        document.scan(&mut index, &frame, 1 << 20).unwrap(),
        frame.len()
    );
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");
    let files = [
        ("purchase.rx", rx),
        ("record.pil", frame),
        ("invalid-child.pil", invalid),
        ("record.json", json.to_vec()),
    ];
    if std::env::var_os("PIL_UPDATE_FIXTURES").is_some() {
        std::fs::create_dir_all(&directory).unwrap();
        for (name, raw) in &files {
            std::fs::write(directory.join(name), raw).unwrap();
        }
    }
    for (name, raw) in files {
        assert_eq!(std::fs::read(directory.join(name)).unwrap(), raw, "{name}");
    }
    let _ = std::mem::size_of::<ArrayView>();
}

#[test]
fn cddl_record_ordinal_uses_u24() {
 let codec=SchemaDocument::from_cddl("record = { ordinal: 0..16777215 }","record").unwrap().compile().unwrap();
 assert_eq!(codec.layout().fields()[0].kind,Scalar::U24.into());
 for n in [0,65536,16777215] { let value=json!({"ordinal":n}); let raw=codec.encode_json(&value).unwrap(); assert_eq!(raw.len(),4); assert_eq!(codec.decode_json(&raw).unwrap(),value); }
 assert!(codec.encode_json(&json!({"ordinal":16777216})).is_err());
}
