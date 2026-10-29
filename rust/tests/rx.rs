#![cfg(feature = "cli")]
use pil_codec::{DefinitionDocument, Value, encode, schema::SchemaDocument};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "pill-rx-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> String {
        let p = self.0.join(name);
        fs::write(&p, bytes).unwrap();
        p.to_str().unwrap().into()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn rx(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rx"))
        .args(args)
        .output()
        .unwrap()
}
fn success(args: &[&str]) -> Vec<u8> {
    let result = rx(args);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}
fn lines(output: &[u8]) -> Vec<Json> {
    String::from_utf8_lossy(output)
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}

#[test]
fn schema_conversion_nested_sequences_pretty_and_stdin() {
    let scratch = Scratch::new();
    let schema = SchemaDocument::from_cddl(
        "record = { id: uint, customer: { name: tstr }, ? note: tstr / nil }",
        "record",
    )
    .unwrap();
    let source = scratch.file("record.json", &schema.to_json().unwrap());
    let definition = scratch.0.join("record.rx").to_str().unwrap().to_string();
    success(&["write", &source, &definition]);
    let rendered = success(&["read", &definition]);
    let edited = scratch.file("editable.json", &rendered);
    let rewritten = scratch.0.join("rewritten.rx").to_str().unwrap().to_string();
    success(&["write", &edited, &rewritten]);
    assert_eq!(
        fs::read(&rewritten).unwrap(),
        fs::read(&definition).unwrap()
    );
    assert_eq!(
        serde_json::from_slice::<Json>(&success(&["read", &definition])).unwrap()["cddl"],
        serde_json::from_slice::<Json>(&schema.to_json().unwrap()).unwrap()
    );
    let pretty = success(&[&definition, "--pretty"]);
    assert!(pretty.windows(3).any(|s| s == b"\n  "));
    let codec = schema.compile().unwrap();
    let a = json!({"id":u64::MAX,"customer":{"name":"Zoë\u{0} "},"note":null});
    let b = json!({"id":9,"customer":{"name":"東京"}});
    let bytes = [
        codec.encode_json(&a).unwrap(),
        codec.encode_json(&b).unwrap(),
    ]
    .concat();
    let data = scratch.file("records.pil", &bytes);
    assert_eq!(
        lines(&success(&["read", &definition, &data])),
        [a.clone(), b.clone()]
    );
    let pretty = success(&["--pretty", "read", &definition, &data]);
    let records: Vec<Json> = serde_json::Deserializer::from_slice(&pretty)
        .into_iter()
        .map(Result::unwrap)
        .collect();
    assert_eq!(records, [a.clone(), b.clone()]);
    let mut child = Command::new(env!("CARGO_BIN_EXE_rx"))
        .args(["read", &definition, "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&bytes).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(lines(&out.stdout), [a, b]);
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gzip.write_all(&bytes).unwrap();
    let data = scratch.file("records.pil.gz", &gzip.finish().unwrap());
    assert_eq!(lines(&success(&["read", &definition, &data])).len(), 2);
}

#[test]
fn flat_definitions_keep_annotations_dictionaries_and_exact_integers() {
    let scratch = Scratch::new();
    let json = br#"{"pil":0,"definition":"test","version":0,"fields":[{"name":"label","type":"STRING(8)"},{"name":"status","type":"STRING","values":["ok","fail"]},{"name":"count","type":"U64"},{"name":"maybe","type":"STRING"}],"annotation":{"maximum":18446744073709551615}}"#;
    let source = scratch.file("flat.json", json);
    let document = DefinitionDocument::from_json(json).unwrap();
    let definition = scratch.file("flat.rx", document.as_pil());
    assert_eq!(
        serde_json::from_slice::<Json>(&success(&["read", &definition])).unwrap(),
        serde_json::from_slice::<Json>(json).unwrap()
    );
    assert_eq!(success(&["write", &source, "-"]), document.as_pil());
    let bytes = encode(
        document.definition(),
        &[
            Value::String("é\0 "),
            Value::Mapped(1),
            Value::U64(u64::MAX),
            Value::Absent,
        ],
    )
    .unwrap();
    let data = scratch.file("flat-data.pil", &bytes);
    assert_eq!(
        lines(&success(&["read", &definition, &data])),
        [json!({"label":"é\u{0} ","status":"fail","count":u64::MAX})]
    );
    let mut bad = bytes.clone();
    bad[6] = 1;
    let data = scratch.file("bad.pil", &bad);
    assert!(!rx(&["read", &definition, &data]).status.success());
}

#[test]
fn pac_common_values_binding_gzip_and_truncation() {
    let scratch = Scratch::new();
    let schema = SchemaDocument::from_cddl("record = { label: tstr, id: uint }", "record").unwrap();
    let raw = schema.to_pil().unwrap();
    let codec = schema.compile().unwrap();
    let definition = scratch.file("record.rx", &raw);
    let common = encode(codec.layout(), &[Value::String("same"), Value::Absent]).unwrap();
    let members: Vec<_> = [7, 8]
        .into_iter()
        .map(|n| encode(codec.layout(), &[Value::Absent, Value::U64(n)]).unwrap())
        .collect();
    let mut header = vec![0u8; 64];
    header[4] = 5;
    header[24..32].copy_from_slice(&2u64.to_le_bytes());
    header[32..64].copy_from_slice(&Sha256::digest(&raw));
    pil_codec::encode_extended(0, &mut header);
    header.extend_from_slice(&[0; 5]); // variable members, no directory
    pil_codec::encode_extended(0, &mut header); // named headers
    pil_codec::encode_extended(common.len() as u64, &mut header);
    header.extend_from_slice(&common);
    let header_size = header.len() as u64;
    header[16..24].copy_from_slice(&header_size.to_le_bytes());
    let mut bytes = header;
    for m in members {
        bytes.extend(m);
    }
    let size = bytes.len();
    bytes[..4].copy_from_slice(&(size as u32).to_le_bytes());
    bytes[8..16].copy_from_slice(&(size as u64).to_le_bytes());
    let data = scratch.file("data.pac", &bytes);
    let expected = vec![
        json!({"label":"same","id":7}),
        json!({"label":"same","id":8}),
    ];
    assert_eq!(lines(&success(&["read", &definition, &data])), expected);
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gzip.write_all(&bytes).unwrap();
    let gzip = gzip.finish().unwrap();
    let mut compressed = Vec::new();
    compressed.extend_from_slice(&((16 + gzip.len()) as u32).to_le_bytes());
    compressed.extend_from_slice(&2u32.to_le_bytes());
    compressed.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    compressed.extend_from_slice(&2u32.to_le_bytes());
    compressed.extend_from_slice(&gzip);
    let data = scratch.file("go-compressed.pac", &compressed);
    assert_eq!(lines(&success(&["read", &definition, &data])), expected);
    compressed[12] = 3;
    let data = scratch.file("bad-compressed.pac", &compressed);
    assert!(!rx(&["read", &definition, &data]).status.success());
    let data = scratch.file("data.pac.gz", &[gzip.clone(), gzip].concat());
    assert_eq!(
        lines(&success(&["read", &definition, &data])),
        [expected.clone(), expected].concat()
    );
    let data = scratch.file("truncated.pac", &bytes[..bytes.len() - 1]);
    assert!(!rx(&["read", &definition, &data]).status.success());
    bytes[32] ^= 1;
    let data = scratch.file("wrong-binding.pac", &bytes);
    assert!(!rx(&["read", &definition, &data]).status.success());
}

#[test]
fn reads_go_pac_fixture_and_rejects_bad_commands_without_writing() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pac");
    let definition = base.join("definition.rx");
    let data = base.join("packed.pac");
    assert_eq!(
        lines(&success(&[
            "read",
            definition.to_str().unwrap(),
            data.to_str().unwrap()
        ]))
        .len(),
        16
    );
    let scratch = Scratch::new();
    let invalid = scratch.file("bad.json", b"{\"root\":\"absent\",\"types\":{}}");
    let target = scratch.file("output.rx", b"keep");
    assert!(!rx(&["write", &invalid, &target]).status.success());
    assert_eq!(fs::read(&target).unwrap(), b"keep");
    for args in [
        &[][..],
        &["read"][..],
        &["write", "one"][..],
        &["--unknown"][..],
    ] {
        assert!(!rx(args).status.success());
    }
    assert!(rx(&["--help"]).status.success());
    let cddl = scratch.file("record.cddl", b"record = { id: uint }");
    success(&["write", &cddl, &target, "--root", "record"]);
    assert_eq!(
        serde_json::from_slice::<Json>(&success(&["read", &target])).unwrap()["cddl"]["root"],
        "record"
    );
}

#[test]
fn exports_and_inspects_the_pil_bootstrap_as_rx() {
    let scratch = Scratch::new();
    let path = scratch.0.join("pil.rx").to_str().unwrap().to_string();
    success(&["bootstrap", &path]);
    let expected = pil_codec::object::definition().to_pil().unwrap();
    assert_eq!(expected, include_bytes!("../../schemas/pil.rx"));
    assert_eq!(fs::read(&path).unwrap(), expected);
    assert_eq!(success(&["bootstrap", "-"]), expected);
    let definition: Json = serde_json::from_slice(&success(&["read", &path])).unwrap();
    assert_eq!(definition["definition"], "pil/value");
    assert_eq!(definition["fields"].as_array().unwrap().len(), 7);
    let encoded: Json = serde_json::from_slice(&success(&["read", &path, &path])).unwrap();
    assert_eq!(encoded["kind"], 9);
    assert!(!rx(&["bootstrap", &path, "--pretty"]).status.success());
}
