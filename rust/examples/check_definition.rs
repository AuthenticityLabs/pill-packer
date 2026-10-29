//! Development conformance check, not part of the record hot path.
use pil_codec::DefinitionDocument;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let schema = std::fs::read_to_string(
        args.next()
            .ok_or("usage: check_definition schema.cddl input.rx ...")?,
    )?;
    let paths: Vec<_> = args.collect();
    if paths.is_empty() {
        return Err("at least one input is required".into());
    }
    for path in paths {
        let bytes = std::fs::read(&path)?;
        let value = pil_codec::object::decode_value(&bytes)?;
        let mut oracle = Vec::new();
        ciborium::ser::into_writer(&value, &mut oracle)?;
        cddl::validate_cbor_from_slice(&schema, &oracle, None)?;
        DefinitionDocument::from_pil(&bytes)?;
        println!("Valid structure and PIL layout: {path}");
    }
    Ok(())
}
