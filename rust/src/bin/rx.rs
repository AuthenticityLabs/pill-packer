use pil_codec::{
    Array, Definition, DefinitionDocument, Value, decode_exact, encode,
    object::decode_value,
    pac::{HEADER_SIZE, PacIndex, SOURCE_EXTENSION},
    read_frame_bytes,
    schema::{CompiledSchema, SchemaDocument},
};
use serde_json::{Map, Value as Json};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
    path::Path,
    process::ExitCode,
};

const MAXIMUM: usize = 64 << 20;
const HELP: &str = "Usage:
  rx read <definition> [<pil/pac>|-] [--pretty]
  rx <definition> [--pretty]
  rx write <definition.json> <definition.rx>
  rx write <schema.cddl> <definition.rx> --root <rule>
  rx bootstrap <pil.rx>|-

Definitions use the .rx extension and contain PIL, including compiled CDDL schemas.
Read also accepts JSON authoring input. With no data file, it prints the definition.
Records are JSONL by default; --pretty prints indented JSON records.
PIL sequences, PACs (including common fields), and gzip inputs are supported.
Use - for record input on stdin or for binary definition output on stdout.
Bytes render as octet arrays; UUIDs as dashed hex; timestamps retain their units.";

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
fn codec(error: impl std::fmt::Display) -> io::Error {
    invalid(error.to_string())
}

struct Document {
    bytes: Vec<u8>,
    json: Json,
    layout: Definition,
    schema: Option<CompiledSchema>,
}
impl Document {
    fn load(path: &str, root: Option<&str>) -> io::Result<Self> {
        let raw = fs::read(path)?;
        if raw.len() > MAXIMUM {
            return Err(invalid("definition exceeds byte limit"));
        }
        if let Some(root) = root {
            let source = std::str::from_utf8(&raw).map_err(codec)?;
            return Self::from_schema(SchemaDocument::from_cddl(source, root).map_err(codec)?);
        }
        // JSON and PIL are distinct authoring/wire inputs, with no CBOR fallback.
        let json_input = raw.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{');
        let json: Json = if json_input {
            serde_json::from_slice(&raw).map_err(codec)?
        } else {
            data_json(decode_value(&raw).map_err(codec)?)?
        };
        if json.get("root").is_some() {
            let schema = if json_input {
                SchemaDocument::from_json(&raw)
            } else {
                SchemaDocument::from_pil(&raw)
            }
            .map_err(codec)?;
            let mut document = Self::from_schema(schema)?;
            if !json_input {
                document.bytes = raw;
            }
            return Ok(document);
        }
        let document = if json_input {
            DefinitionDocument::from_json(&raw)
        } else {
            DefinitionDocument::from_pil(&raw)
        }
        .map_err(codec)?;
        let schema = if document.annotation("cddl").is_some() {
            Some(
                SchemaDocument::from_pil(document.as_pil())
                    .map_err(codec)?
                    .compile()
                    .map_err(codec)?,
            )
        } else {
            None
        };
        Ok(Self {
            bytes: document.as_pil().to_vec(),
            json: data_json(document.value().clone())?,
            layout: document.definition().clone(),
            schema,
        })
    }
    fn from_schema(document: SchemaDocument) -> io::Result<Self> {
        let schema = document.compile().map_err(codec)?;
        let bytes = document.to_pil().map_err(codec)?;
        Ok(Self {
            json: data_json(decode_value(&bytes).map_err(codec)?)?,
            bytes,
            layout: schema.layout().clone(),
            schema: Some(schema),
        })
    }
    fn render(&self, bytes: &[u8], common: Option<&[u8]>) -> io::Result<Json> {
        let member = decode_exact(&self.layout, bytes).map_err(codec)?;
        let mut values = member.values().to_vec();
        if let Some(common) = common {
            let frame = decode_exact(&self.layout, common).map_err(codec)?;
            for (value, shared) in values.iter_mut().zip(frame.values()) {
                if *shared != Value::Absent {
                    if *value != Value::Absent {
                        return Err(invalid("PAC common override"));
                    }
                    *value = *shared;
                }
            }
        }
        if let Some(schema) = &self.schema {
            return schema
                .decode_json(&encode(&self.layout, &values).map_err(codec)?)
                .map_err(codec);
        }
        let mut result = Map::new();
        for (field, value) in self.layout.fields().iter().zip(values) {
            if value != Value::Absent {
                result.insert(field.name.clone(), value_json(value, &field.values)?);
            }
        }
        Ok(Json::Object(result))
    }
}

fn value_json(value: Value<'_>, dictionary: &[String]) -> io::Result<Json> {
    Ok(match value {
        Value::Null => Json::Null,
        Value::Bool(v) => v.into(),
        Value::I8(v) => v.into(),
        Value::I16(v) => v.into(),
        Value::I32(v) => v.into(),
        Value::I64(v) => v.into(),
        Value::U8(v) => v.into(),
        Value::U16(v) | Value::Ref16(v) => v.into(),
        Value::U24(v) | Value::U32(v) => v.into(),
        Value::U64(v) => v.into(),
        Value::TimestampMs(v) | Value::TimestampUs(v) | Value::TimestampNs(v) => v.into(),
        Value::F32(v) => finite(v as f64)?,
        Value::F64(v) => finite(v)?,
        Value::Mapped(v) => dictionary
            .get(v as usize)
            .ok_or_else(|| invalid("invalid dictionary reference"))?
            .clone()
            .into(),
        Value::String(v) => v.into(),
        Value::Bytes(v) => Json::Array(v.iter().copied().map(Json::from).collect()),
        Value::Uuid(v) => {
            let text: String = v
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    format!(
                        "{}{b:02x}",
                        if [4, 6, 8, 10].contains(&i) { "-" } else { "" }
                    )
                })
                .collect();
            text.into()
        }
        Value::Object(v) => data_json(decode_value(v).map_err(codec)?)?,
        Value::Array(array) => {
            let values = match array {
                Array::Items(items) => items
                    .iter()
                    .copied()
                    .map(Ok)
                    .collect::<Result<Vec<_>, pil_codec::Error>>(),
                Array::Encoded(view) => view.iter().collect(),
            }
            .map_err(codec)?;
            Json::Array(
                values
                    .into_iter()
                    .map(|v| value_json(v, &[]))
                    .collect::<io::Result<_>>()?,
            )
        }
        Value::Absent => return Err(invalid("absent value has no JSON representation")),
    })
}
fn finite(value: f64) -> io::Result<Json> {
    serde_json::Number::from_f64(value)
        .map(Json::Number)
        .ok_or_else(|| invalid("non-finite number has no JSON representation"))
}
fn data_json(value: ciborium::Value) -> io::Result<Json> {
    use ciborium::Value as Data;
    Ok(match value {
        Data::Null => Json::Null,
        Data::Bool(v) => v.into(),
        Data::Text(v) => v.into(),
        Data::Integer(v) => {
            let n: i128 = v.into();
            if n < 0 {
                i64::try_from(n).map_err(codec)?.into()
            } else {
                u64::try_from(n).map_err(codec)?.into()
            }
        }
        Data::Float(v) => finite(v)?,
        Data::Bytes(v) => Json::Array(v.into_iter().map(Json::from).collect()),
        Data::Array(v) => Json::Array(v.into_iter().map(data_json).collect::<io::Result<_>>()?),
        Data::Map(v) => {
            let mut map = Map::new();
            for (key, value) in v {
                let key = match key {
                    Data::Text(s) => s,
                    Data::Integer(n) => i128::from(n).to_string(),
                    _ => return Err(invalid("object key has no JSON representation")),
                };
                if map.insert(key, data_json(value)?).is_some() {
                    return Err(invalid("object keys collide in JSON"));
                }
            }
            Json::Object(map)
        }
        _ => return Err(invalid("tagged value has no JSON representation")),
    })
}
fn print_json(output: &mut impl Write, value: &Json, pretty: bool) -> io::Result<()> {
    if pretty {
        serde_json::to_writer_pretty(&mut *output, value)?;
    } else {
        serde_json::to_writer(&mut *output, value)?;
    }
    output.write_all(b"\n")
}

fn records(
    document: &Document,
    path: &str,
    pretty: bool,
    output: &mut impl Write,
) -> io::Result<()> {
    let input: Box<dyn Read> = if path == "-" {
        Box::new(io::stdin())
    } else {
        Box::new(fs::File::open(path)?)
    };
    let mut input = BufReader::new(input);
    let gzip = input.fill_buf()?.starts_with(&[0x1f, 0x8b]);
    let stream: Box<dyn Read> = if gzip {
        Box::new(flate2::read::MultiGzDecoder::new(input))
    } else {
        Box::new(input)
    };
    // read_exact-style peeking also recognizes PACs on fragmented stdin pipes.
    let mut stream = stream;
    let mut head = Vec::new();
    stream
        .by_ref()
        .take(HEADER_SIZE as u64)
        .read_to_end(&mut head)?;
    let header_pac = head.len() >= 16
        && match u32::from_le_bytes(head[4..8].try_into().unwrap()) {
            4 | 5 => {
                head.len() >= HEADER_SIZE
                    && u64::from_le_bytes(head[8..16].try_into().unwrap())
                        == u32::from_le_bytes(head[..4].try_into().unwrap()) as u64
                    && u64::from_le_bytes(head[16..24].try_into().unwrap()) >= HEADER_SIZE as u64
            }
            2 => u32::from_le_bytes(head[8..12].try_into().unwrap()) >= HEADER_SIZE as u32,
            _ => false,
        };
    let mut input = BufReader::new(io::Cursor::new(head).chain(stream));
    let extension_pac = Path::new(path).file_name().is_some_and(|n| {
        n.to_string_lossy().ends_with(".pac") || n.to_string_lossy().ends_with(".pac.gz")
    });
    if extension_pac || header_pac {
        return pac_records(document, &mut input, pretty, output);
    }
    while let Some(bytes) = read_frame_bytes(&document.layout, &mut input, MAXIMUM)? {
        print_json(output, &document.render(&bytes, None)?, pretty)?;
    }
    Ok(())
}
fn pac_records(
    document: &Document,
    input: &mut impl BufRead,
    pretty: bool,
    output: &mut impl Write,
) -> io::Result<()> {
    let digest: [u8; 32] = Sha256::digest(&document.bytes).into();
    while !input.fill_buf()?.is_empty() {
        let mut header = [0; 8];
        input.read_exact(&mut header)?;
        let size = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
        let flags = u32::from_le_bytes(header[4..8].try_into().unwrap());
        let minimum = if flags == 2 { 16 } else { HEADER_SIZE };
        if !matches!(flags, 2 | 4 | 5) || size < minimum || size > MAXIMUM {
            return Err(invalid("PAC exceeds byte bounds"));
        }
        let mut bytes = header.to_vec();
        bytes.resize(size, 0);
        input.read_exact(&mut bytes[8..])?;
        if flags == 2 {
            let decoded = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
            let count = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as u64;
            if decoded < HEADER_SIZE || decoded > MAXIMUM || count > (decoded - HEADER_SIZE) as u64
            {
                return Err(invalid("PAC compression bounds"));
            }
            let mut expanded = Vec::new();
            pil_codec::compression::inflate(&bytes[16..], decoded, &mut expanded)?;
            if expanded.len() < HEADER_SIZE
                || u64::from_le_bytes(expanded[24..32].try_into().unwrap()) != count
            {
                return Err(invalid("PAC compressed/decoded count mismatch"));
            }
            bytes = expanded;
        }
        let mut index = PacIndex::default();
        index
            .scan(
                &document.layout,
                &bytes,
                &digest,
                &[SOURCE_EXTENSION],
                MAXIMUM,
            )
            .map_err(codec)?;
        let [start, end] = index.common();
        let common = (start != end).then_some(&bytes[start as usize..end as usize]);
        for [start, end] in index.members() {
            print_json(
                output,
                &document.render(&bytes[*start as usize..*end as usize], common)?,
                pretty,
            )?;
        }
    }
    Ok(())
}

fn run() -> io::Result<()> {
    let mut positional = Vec::new();
    let mut pretty = false;
    let mut root = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            "--pretty" => pretty = true,
            "--root" => {
                root = Some(
                    args.next()
                        .ok_or_else(|| invalid("--root requires a rule"))?,
                )
            }
            _ if arg.starts_with('-') && arg != "-" => {
                return Err(invalid(format!("unknown option {arg}")));
            }
            _ => positional.push(arg),
        }
    }
    if positional.is_empty() {
        return Err(invalid(HELP));
    }
    let command = if matches!(positional[0].as_str(), "read" | "write" | "bootstrap") {
        positional.remove(0)
    } else {
        "read".into()
    };
    if command == "bootstrap" {
        if positional.len() != 1 || pretty || root.is_some() {
            return Err(invalid("usage: rx bootstrap <pil.rx>|-"));
        }
        let bytes = pil_codec::object::definition().to_pil().map_err(codec)?;
        if positional[0] == "-" {
            io::stdout().lock().write_all(&bytes)?;
        } else {
            fs::write(&positional[0], bytes)?;
        }
        return Ok(());
    }
    if positional.is_empty()
        || positional.len() > 2
        || (command == "write" && positional.len() != 2)
    {
        return Err(invalid(HELP));
    }
    if command == "write" && pretty {
        return Err(invalid(
            "--pretty formats JSON read output; write produces binary PIL",
        ));
    }
    let document = Document::load(&positional[0], root.as_deref())?;
    let mut output = BufWriter::new(io::stdout().lock());
    if command == "write" {
        if positional[1] == "-" {
            output.write_all(&document.bytes)?;
        } else {
            fs::write(&positional[1], &document.bytes)?;
        }
    } else if positional.len() == 1 {
        print_json(&mut output, &document.json, pretty)?;
    } else {
        records(&document, &positional[1], pretty, &mut output)?;
    }
    output.flush()
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rx: {error}");
            ExitCode::FAILURE
        }
    }
}
