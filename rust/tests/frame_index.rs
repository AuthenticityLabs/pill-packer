use pil_codec::{
    ABSENT, Array, Definition, Error, Field, FrameIndex, NULL, Scalar, Type, Value, decode_exact,
    encode,
};
#[test]
fn borrowed_index_matches_codec_and_rejects_every_truncation() {
    let d = Definition::new(
        "index",
        1,
        vec![
            Field::new("missing", Scalar::String),
            Field::new("null", Scalar::U16),
            Field::mapped("service", ["vault", "billing"]),
            Field::new("id", Scalar::Uuid),
            Field::new("text", Scalar::String),
            Field::new("bytes", Scalar::Bytes),
            Field::new("array", Type::Array(Scalar::U16)),
            Field::new("object", Scalar::Object),
        ],
    )
    .unwrap();
    let data = encode(
        &d,
        &[
            Value::Absent,
            Value::Null,
            Value::Mapped(1),
            Value::Uuid([7; 16]),
            Value::String("aλ"),
            Value::Bytes(&[]),
            Value::Array(Array::Items(&[Value::U16(4), Value::U16(6)])),
            Value::Object(&native(&[0xa1, 0x00, 0x18, 0x2a])),
        ],
    )
    .unwrap();
    let mut index = FrameIndex::default();
    for n in 0..data.len() {
        assert_eq!(index.scan(&d, &data[..n], 1024), Err(Error::Incomplete));
    }
    assert_eq!(
        index.scan(&d, &data, 1024).unwrap(),
        decode_exact(&d, &data).unwrap().consumed()
    );
    assert_eq!(index.spans()[0], [ABSENT, ABSENT]);
    assert_eq!(index.spans()[1], [NULL, NULL]);
    let text = index.spans()[4];
    assert_eq!(&data[text[0] as usize..text[1] as usize], "aλ".as_bytes());
    assert!(index.scan(&d, &data, data.len() - 1).is_err());
    let mut bad = data.clone();
    bad[2] = 3;
    assert!(index.scan(&d, &bad, 1024).is_err());
}
#[test]
fn extended_offsets_empty_arrays_and_cbormaps_are_validated() {
    let d = Definition::new(
        "long",
        1,
        vec![
            Field::new("payload", Scalar::Bytes),
            Field::new("end", Scalar::Object),
        ],
    )
    .unwrap();
    let payload = vec![42; 65535];
    let data = encode(
        &d,
        &[Value::Bytes(&payload), Value::Object(&native(&[0xf6]))],
    )
    .unwrap();
    let mut index = FrameIndex::default();
    assert_eq!(index.scan(&d, &data, 1 << 20).unwrap(), data.len());
    let mut corrupt = data.clone();
    *corrupt.last_mut().unwrap() = 0xff;
    assert!(index.scan(&d, &corrupt, 1 << 20).is_err());
}

#[test]
fn structural_scan_skips_values_but_preserves_safe_boundaries() {
    let d = Definition::new(
        "structural",
        0,
        vec![
            Field::mapped("code", ["one"]),
            Field::new("text", Scalar::String),
            Field::new("object", Scalar::Object),
        ],
    )
    .unwrap();
    let good = encode(
        &d,
        &[
            Value::Mapped(0),
            Value::String("text"),
            Value::Object(&native(&[0xf6])),
        ],
    )
    .unwrap();
    let mut full = FrameIndex::default();
    full.scan(&d, &good, 1024).unwrap();
    let spans = full.spans().to_vec();
    let mut fast = FrameIndex::default();
    assert_eq!(fast.scan_structural(&d, &good, 1024).unwrap(), good.len());
    assert_eq!(fast.spans(), spans);
    for end in 0..good.len() {
        assert!(fast.scan_structural(&d, &good[..end], 1024).is_err());
    }
    for (field, value) in [(0, 4), (1, 255), (2, 255)] {
        let mut invalid = good.clone();
        invalid[spans[field][0] as usize] = value;
        assert!(full.scan(&d, &invalid, 1024).is_err());
        assert_eq!(
            fast.scan_structural(&d, &invalid, 1024).unwrap(),
            good.len()
        );
        assert_eq!(fast.spans(), spans);
    }
    assert!(fast.scan_structural(&d, &good, good.len() - 1).is_err());
}

fn native(raw: &[u8]) -> Vec<u8> {
    let value: ciborium::Value = ciborium::from_reader(raw).unwrap();
    pil_codec::object::encode_value(&value).unwrap()
}
