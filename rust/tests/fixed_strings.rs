use pil_codec::{
    AdaptiveIntegers, Array, Definition, Field, FrameIndex, Scalar, Type, Value, decode_exact,
    encode, read_frame_bytes,
};
fn layout(capacity: u32) -> Definition {
    Definition::new(
        "fixed",
        0,
        vec![Field::new("text", Scalar::FixedString(capacity))],
    )
    .unwrap()
}
#[test]
fn preserves_utf8_nuls_and_spaces() {
    for capacity in [8, 255, 256, 65535, 65536] {
        for mode in [AdaptiveIntegers::None, AdaptiveIntegers::From16] {
            let d = layout(capacity).with_adaptive_integers(mode);
            for text in ["", "é\0 ", "𐍈"] {
                let bytes = encode(&d, &[Value::String(text)]).unwrap();
                assert_eq!(
                    bytes.len(),
                    1 + capacity as usize + Scalar::string_length_width(capacity)
                );
                assert_eq!(
                    decode_exact(&d, &bytes).unwrap().values(),
                    &[Value::String(text)]
                );
                assert_eq!(
                    FrameIndex::default().scan(&d, &bytes, 100000).unwrap(),
                    bytes.len()
                );
                assert_eq!(
                    read_frame_bytes(&d, &mut &bytes[..], 100000)
                        .unwrap()
                        .unwrap(),
                    bytes
                );
            }
        }
    }
    assert_eq!(
        encode(&layout(4), &[Value::String("a\0 ")]).unwrap(),
        b"\x80\x03a\0 \0"
    );
}
#[test]
fn rejects_overflow_invalid_utf8_and_padding() {
    let d = layout(4);
    assert!(encode(&d, &[Value::String("ééé")]).is_err());
    for bytes in [
        vec![0x80, 5, 0, 0, 0, 0],
        vec![0x80, 1, 0xff, 0, 0, 0],
        vec![0x80, 0, 1, 0, 0, 0],
    ] {
        assert!(decode_exact(&d, &bytes).is_err());
        assert!(FrameIndex::default().scan(&d, &bytes, 100).is_err());
    }
    for n in 0..6 {
        assert!(decode_exact(&d, &[0x80, 3, b'a', 0, b' ', 0][..n]).is_err());
    }
    assert!("STRING(0)".parse::<Scalar>().is_err());
    assert!("STRING(4294967296)".parse::<Scalar>().is_err());
}
#[test]
fn fixed_array_slots_roundtrip() {
    let d = Definition::new(
        "array",
        0,
        vec![Field::new("texts", Type::Array(Scalar::FixedString(8)))],
    )
    .unwrap();
    let values = [Value::String("é"), Value::String("a\0 ")];
    let bytes = encode(&d, &[Value::Array(Array::Items(&values))]).unwrap();
    let frame = decode_exact(&d, &bytes).unwrap();
    let Value::Array(Array::Encoded(array)) = frame.values()[0] else {
        panic!()
    };
    assert_eq!(array.iter().collect::<Result<Vec<_>, _>>().unwrap(), values);
}
