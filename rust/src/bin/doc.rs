use pil_codec::authoring::{Draft, MAX_INPUT};
use std::{
    env,
    io::{self, IsTerminal},
    path::PathBuf,
};
#[path = "../doc_ui/mod.rs"]
mod ui;
const HELP: &str = "DOC — Definition Editor
Usage:
  doc
  doc --json [samples.json|samples.jsonl]
  doc --cddl [schema.cddl]
  doc --rx [definition.rx]
Options:
  --root <rule>       CDDL root rule (default: first rule)
  --output <file.rx>  Suggested output path; all output is .rx
  --check            Validate an input without opening the editor; writes nothing
  --help             Show help

Without a filename, an input mode opens a paste/file prompt.
Samples propose types; review fields before building. No inferred size limits.
";
#[derive(Clone, Copy, Debug)]
pub enum Mode {
    Json,
    Cddl,
    Rx,
}
pub fn load(mode: Mode, path: &str, root: Option<String>) -> Result<Draft, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_INPUT as u64 {
        return Err("Input exceeds 64 MiB".into());
    }
    use std::io::Read;
    let mut raw = vec![];
    file.take(MAX_INPUT as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    if raw.len() > MAX_INPUT {
        return Err("Input exceeds 64 MiB".into());
    }
    match mode {
        Mode::Rx => Draft::from_rx(&raw),
        Mode::Json => Draft::from_json(std::str::from_utf8(&raw).map_err(|e| e.to_string())?),
        Mode::Cddl => Draft::from_cddl(String::from_utf8(raw).map_err(|e| e.to_string())?, root),
    }
    .map_err(|e| e.to_string())
}
fn run() -> Result<(), String> {
    let args: Vec<_> = env::args().skip(1).collect();
    let (mut mode, mut input, mut root, mut output, mut check) = (None, None, None, None, false);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print!("{HELP}");
                return Ok(());
            }
            "--json" | "--cddl" | "--rx" => {
                if mode.is_some() {
                    return Err("Choose one input mode: --json, --cddl, or --rx".into());
                }
                mode = Some(match args[i].as_str() {
                    "--json" => Mode::Json,
                    "--cddl" => Mode::Cddl,
                    _ => Mode::Rx,
                });
                if args.get(i + 1).is_some_and(|s| !s.starts_with('-')) {
                    i += 1;
                    input = Some(args[i].clone());
                }
            }
            "--root" | "--output" => {
                let flag = args[i].clone();
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| format!("Missing value for {flag}"))?
                    .clone();
                if flag == "--root" {
                    root = Some(value)
                } else {
                    output = Some(PathBuf::from(value));
                }
            }
            "--check" => check = true,
            other => return Err(format!("Unknown argument: {other}. Use --help.")),
        };
        i += 1;
    }
    if let Some(p) = &output {
        if p.extension().and_then(|v| v.to_str()) != Some("rx") {
            return Err("Output must use .rx".into());
        }
    }
    let draft = if let Some(path) = &input {
        load(mode.unwrap(), path, root.clone())?
    } else {
        Draft::default()
    };
    if check {
        if input.is_none() {
            return Err("--check requires an input file".into());
        }
        let bytes = draft.validate_samples().map_err(|e| e.to_string())?;
        println!(
            "Valid schema; {} samples; {} encoded sample bytes; {} decisions awaiting review. No output written.",
            draft.samples.len(),
            bytes,
            draft.pending.len()
        );
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("DOC needs an interactive terminal. Use --check for input validation.".into());
    }
    let output = output.or_else(|| {
        input
            .as_ref()
            .map(|p| PathBuf::from(p).with_extension("rx"))
    });
    ui::run(draft, if input.is_none() { mode } else { None }, output)
}
fn main() {
    if let Err(e) = run() {
        eprintln!("doc: {e}");
        std::process::exit(1);
    }
}
