//! Development conformance check, not part of the record hot path.
use pil_codec::DefinitionDocument;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let schema = std::fs::read_to_string(
        args.next()
            .ok_or("usage: check_definition schema.cddl input.cbor ...")?,
    )?;
    let paths: Vec<_> = args.collect();
    if paths.is_empty() {
        return Err("at least one input is required".into());
    }
    for path in paths {
        let bytes = std::fs::read(&path)?;
        cddl::validate_cbor_from_slice(&schema, &bytes, None)?;
        DefinitionDocument::from_cbor(&bytes)?;
        println!("Valid structure and PIL layout: {path}");
    }
    Ok(())
}
