//! Reviewed schema authoring. Sampling never establishes a contract automatically.
use crate::{
    DefinitionDocument, Error,
    schema::{Member, SchemaDocument, SchemaType as Ty},
};
use serde_json::Value as Json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
fn err(s: impl Into<String>) -> Error {
    Error::Definition(s.into())
}
pub const MAX_INPUT: usize = 64 << 20;
pub const MAX_RECORDS: usize = 10_000;
#[derive(Clone, Debug, Default)]
pub struct Observation {
    pub present: usize,
    pub total: usize,
    pub nulls: usize,
    pub kinds: BTreeSet<String>,
    pub strings: BTreeMap<String, usize>,
    pub truncated: bool,
    pub max_bytes: usize,
}
#[derive(Clone, Debug)]
pub enum Step {
    Field(usize),
    Item,
    Choice(usize),
}
#[derive(Clone, Debug)]
pub struct FieldView {
    pub name: String,
    pub path: String,
    pub evidence_path: String,
    pub rule: String,
    pub steps: Vec<Step>,
    pub value: Ty,
    pub optional: bool,
}
#[derive(Clone)]
pub struct Draft {
    pub source: String,
    pub root: String,
    pub native: bool,
    pub original: Option<(String, Vec<u8>)>,
    pub fixed_strings: BTreeMap<String, u32>,
    pub dictionaries: BTreeMap<String, Vec<String>>,
    pub samples: Vec<Json>,
    pub observed: BTreeMap<String, Observation>,
    pub pending: BTreeSet<String>,
    pub annotations: Vec<(ciborium::Value, ciborium::Value)>,
}
impl Default for Draft {
    fn default() -> Self {
        Self {
            native: false,
            original: None,
            source: "record = {\n  * tstr => any\n}\n".into(),
            root: "record".into(),
            fixed_strings: BTreeMap::new(),
            dictionaries: BTreeMap::new(),
            samples: vec![],
            observed: BTreeMap::new(),
            pending: BTreeSet::new(),
            annotations: vec![],
        }
    }
}
impl Draft {
    pub fn from_cddl(source: String, root: Option<String>) -> Result<Self, Error> {
        let root = root.unwrap_or_else(|| first_rule(&source).unwrap_or_else(|| "record".into()));
        let result = Self {
            source,
            root,
            ..Self::default()
        };
        result.schema()?;
        Ok(result)
    }
    pub fn from_json(input: &str) -> Result<Self, Error> {
        let samples = parse_samples(input)?;
        let mut draft = Self::default();
        draft.samples = samples;
        let values: Vec<_> = draft.samples.iter().collect();
        let ty = infer(
            &values,
            "record",
            0,
            &mut draft.observed,
            &mut draft.pending,
        )?;
        draft.source = format!("record = {}\n", type_cddl(&ty, 0));
        Ok(draft)
    }
    pub fn from_rx(bytes: &[u8]) -> Result<Self, Error> {
        let doc = DefinitionDocument::from_pil(bytes)?;
        if doc.annotation("cddl").is_none() {
            let source =
                serde_json::to_string_pretty(doc.value()).map_err(|e| err(e.to_string()))?;
            // Native layouts retain their exact source bytes until explicitly edited.
            return Ok(Self {
                native: true,
                root: doc.definition().identity().into(),
                original: Some((source.clone(), bytes.to_vec())),
                source,
                ..Self::default()
            });
        }
        let schema = SchemaDocument::from_pil(bytes)?;
        let mut draft = Self {
            root: schema.root.clone(),
            source: schema_cddl(&schema),
            fixed_strings: schema.fixed_strings.clone(),
            dictionaries: schema.dictionaries.clone(),
            ..Self::default()
        };
        if let Some(source) = doc.annotation("doc_source").and_then(|v| v.as_text()) {
            if let Ok(mut candidate) = SchemaDocument::from_cddl(source, &schema.root) {
                candidate.fixed_strings = schema.fixed_strings.clone();
                candidate.dictionaries = schema.dictionaries.clone();
                if candidate.to_pil()? == schema.to_pil()? {
                    draft.source = source.into();
                }
            }
        }
        if let Some(entries) = doc.value().as_map() {
            draft.annotations = entries
                .iter()
                .filter(|(k, _)| {
                    !matches!(
                        k.as_text(),
                        Some(
                            "pil"
                                | "definition"
                                | "version"
                                | "fields"
                                | "adaptive_integers"
                                | "cddl"
                                | "doc_source"
                        )
                    )
                })
                .cloned()
                .collect();
        }
        Ok(draft)
    }
    pub fn native_document(&self) -> Result<DefinitionDocument, Error> {
        if let Some((source, bytes)) = &self.original {
            if source == &self.source {
                return DefinitionDocument::from_pil(bytes);
            }
        }
        let doc = DefinitionDocument::from_json(self.source.as_bytes())?;
        // Keep application annotations in their original native types when unchanged in JSON.
        if let Some((old_source, bytes)) = &self.original {
            let old: Json = serde_json::from_str(old_source).map_err(|e| err(e.to_string()))?;
            let new: Json = serde_json::from_str(&self.source).map_err(|e| err(e.to_string()))?;
            let original = DefinitionDocument::from_pil(bytes)?;
            let mut value = doc.value().clone();
            if let Some(entries) = value.as_map_mut() {
                for (k, v) in entries {
                    if let Some(name) = k.as_text() {
                        if !matches!(
                            name,
                            "definition"
                                | "version"
                                | "pil"
                                | "fields"
                                | "adaptive_integers"
                                | "cddl"
                        ) && old.get(name) == new.get(name)
                        {
                            if let Some(saved) = original.annotation(name) {
                                *v = saved.clone();
                            }
                        }
                    }
                }
            }
            return DefinitionDocument::from_pil(&crate::object::encode_value(&value)?);
        }
        Ok(doc)
    }
    pub fn schema(&self) -> Result<SchemaDocument, Error> {
        let mut s = SchemaDocument::from_cddl(&self.source, &self.root)?;
        s.fixed_strings = self.fixed_strings.clone();
        s.dictionaries = self.dictionaries.clone();
        s.compile()?;
        Ok(s)
    }
    pub fn fields(&self) -> Result<Vec<FieldView>, Error> {
        if self.native {
            self.native_document()?;
            return Ok(vec![]);
        }
        let s = self.schema()?;
        let mut out = vec![];
        walk(
            &s,
            &s.types[&s.root],
            &s.root,
            &s.root,
            &s.root,
            &[],
            &mut out,
            0,
        );
        Ok(out)
    }
    pub fn apply(&mut self, field: &FieldView, ty: Ty, optional: bool) -> Result<(), Error> {
        let mut s = self.schema()?;
        let target = s
            .types
            .get_mut(&field.rule)
            .ok_or_else(|| err("missing rule"))?;
        let parent = parent_type(target, &field.steps[..field.steps.len() - 1])?;
        let Step::Field(i) = field.steps.last().unwrap() else {
            return Err(err("not a field"));
        };
        let fields = match parent {
            Ty::Record { fields } | Ty::OpenRecord { fields } => fields,
            _ => return Err(err("not a record")),
        };
        let same_type = field.value == ty;
        fields[*i].value = ty;
        fields[*i].optional = optional;
        // Changing a field invalidates old physical options at that path and its children.
        if !same_type {
            s.fixed_strings.retain(|k, _| !under(k, &field.path));
            s.dictionaries.retain(|k, _| !under(k, &field.path));
        }
        s.compile()?;
        self.fixed_strings = s.fixed_strings.clone();
        self.dictionaries = s.dictionaries.clone();
        self.source = schema_cddl(&s);
        if same_type {
            self.pending.remove(&field.evidence_path);
        } else {
            self.pending.retain(|p| !under(p, &field.evidence_path));
        }
        Ok(())
    }
    pub fn add(&mut self, name: &str) -> Result<(), Error> {
        if name.is_empty() {
            return Err(err("Enter a field name"));
        }
        let mut s = self.schema()?;
        let fields = match s.types.get_mut(&s.root).unwrap() {
            Ty::Record { fields } | Ty::OpenRecord { fields } => fields,
            _ => {
                return Err(err(
                    "Add Field requires a record root; use CDDL for other roots",
                ));
            }
        };
        if fields.iter().any(|f| f.name == name) {
            return Err(err("Field already exists"));
        }
        fields.push(Member {
            name: name.into(),
            optional: true,
            value: Ty::Text,
        });
        s.compile()?;
        self.source = schema_cddl(&s);
        Ok(())
    }
    pub fn remove(&mut self, field: &FieldView) -> Result<(), Error> {
        let mut s = self.schema()?;
        let target = s.types.get_mut(&field.rule).unwrap();
        let parent = parent_type(target, &field.steps[..field.steps.len() - 1])?;
        let Step::Field(i) = field.steps.last().unwrap() else {
            return Err(err("not a field"));
        };
        match parent {
            Ty::Record { fields } | Ty::OpenRecord { fields } => {
                fields.remove(*i);
            }
            _ => return Err(err("not a record")),
        }
        s.fixed_strings.retain(|k, _| !under(k, &field.path));
        s.dictionaries.retain(|k, _| !under(k, &field.path));
        s.compile()?;
        self.fixed_strings = s.fixed_strings.clone();
        self.dictionaries = s.dictionaries.clone();
        self.source = schema_cddl(&s);
        self.pending.retain(|p| !under(p, &field.evidence_path));
        Ok(())
    }
    pub fn set_open(&mut self, open: bool) -> Result<(), Error> {
        let mut s = self.schema()?;
        let root = s.types.get_mut(&s.root).unwrap();
        let fields = match root {
            Ty::Record { fields } | Ty::OpenRecord { fields } => fields.clone(),
            _ => return Err(err("Additional Fields applies to a record root")),
        };
        *root = if open {
            Ty::OpenRecord { fields }
        } else {
            Ty::Record { fields }
        };
        s.compile()?;
        self.source = schema_cddl(&s);
        Ok(())
    }
    pub fn dictionary(&mut self, field: &FieldView, values: Vec<String>) -> Result<(), Error> {
        let path = text_path(&field.path, &field.value)
            .ok_or_else(|| err("Dictionary requires a text field"))?;
        let previous = self.dictionaries.clone();
        if values.is_empty() {
            self.dictionaries.remove(&path);
        } else {
            self.dictionaries.insert(path, values);
        }
        if let Err(e) = self.schema() {
            self.dictionaries = previous;
            return Err(e);
        }
        Ok(())
    }
    pub fn fixed_capacity(
        &mut self,
        field: &FieldView,
        capacity: Option<u32>,
    ) -> Result<(), Error> {
        let path = text_path(&field.path, &field.value)
            .ok_or_else(|| err("Fixed capacity requires bounded text"))?;
        let previous = self.fixed_strings.clone();
        if let Some(n) = capacity {
            self.fixed_strings.insert(path, n);
        } else {
            self.fixed_strings.remove(&path);
        }
        if let Err(e) = self.schema() {
            self.fixed_strings = previous;
            return Err(e);
        }
        Ok(())
    }
    pub fn validate_samples(&self) -> Result<usize, Error> {
        if self.native {
            self.native_document()?;
            return Ok(0);
        }
        let c = self.schema()?.compile()?;
        let mut size = 0;
        for (i, v) in self.samples.iter().enumerate() {
            let bytes = c
                .encode_json(v)
                .map_err(|e| err(format!("Sample {}: {e}", i + 1)))?;
            let decoded = c.decode_json(&bytes)?;
            if !json_equivalent(v, &decoded) {
                return Err(err(format!(
                    "Sample {} would lose information after encoding",
                    i + 1
                )));
            }
            size += bytes.len();
        }
        Ok(size)
    }
    pub fn bytes(&self) -> Result<Vec<u8>, Error> {
        if self.native {
            return Ok(self.native_document()?.as_pil().to_vec());
        }
        if !self.pending.is_empty() {
            return Err(err(format!(
                "Review {} inferred field decisions before building RX",
                self.pending.len()
            )));
        }
        self.validate_samples()?;
        let bytes = self.schema()?.to_pil()?;
        let ciborium::Value::Map(mut entries) = crate::object::decode_value(&bytes)? else {
            unreachable!()
        };
        entries.extend(self.annotations.clone());
        entries.push(("doc_source".into(), self.source.clone().into()));
        let result = crate::object::encode_value(&ciborium::Value::Map(entries))?;
        DefinitionDocument::from_pil(&result)?;
        Ok(result)
    }
    pub fn save(&self, path: &Path, overwrite: bool) -> Result<PathBuf, Error> {
        if path.extension().and_then(|v| v.to_str()) != Some("rx") {
            return Err(err("Output must use the .rx extension"));
        }
        let bytes = self.bytes()?;
        if path.exists() && !overwrite {
            return Err(err("File exists; confirm replacement"));
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let tmp = parent.join(format!(
            ".doc-{}-{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        use std::io::Write;
        let result = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            if overwrite {
                std::fs::rename(&tmp, path)?;
            } else {
                std::fs::hard_link(&tmp, path)?;
                std::fs::remove_file(&tmp)?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            let _ = std::fs::remove_file(&tmp);
            return Err(err(e.to_string()));
        }
        Ok(path.into())
    }
}
fn under(path: &str, parent: &str) -> bool {
    path == parent
        || path.starts_with(&format!("{parent}."))
        || path.starts_with(&format!("{parent}[]"))
        || path.starts_with(&format!("{parent}/"))
}
pub fn text_path(path: &str, t: &Ty) -> Option<String> {
    match t {
        Ty::Text | Ty::Size { value: _, .. }
            if matches!(t, Ty::Text)
                || matches!(t,Ty::Size{value,..} if matches!(value.as_ref(),Ty::Text)) =>
        {
            Some(path.into())
        }
        Ty::Choice { choices } => choices
            .iter()
            .enumerate()
            .find_map(|(i, t)| text_path(&format!("{path}/{i}"), t)),
        _ => None,
    }
}
fn parent_type<'a>(t: &'a mut Ty, steps: &[Step]) -> Result<&'a mut Ty, Error> {
    if steps.is_empty() {
        return Ok(t);
    }
    let next = match (&steps[0], t) {
        (Step::Field(i), Ty::Record { fields } | Ty::OpenRecord { fields }) => {
            &mut fields[*i].value
        }
        (Step::Item, Ty::Array { items, .. }) => items,
        (Step::Choice(i), Ty::Choice { choices }) => &mut choices[*i],
        _ => return Err(err("Field path changed; select the field again")),
    };
    parent_type(next, &steps[1..])
}
fn walk(
    s: &SchemaDocument,
    t: &Ty,
    rule: &str,
    path: &str,
    evidence: &str,
    steps: &[Step],
    out: &mut Vec<FieldView>,
    depth: usize,
) {
    if depth > 64 {
        return;
    }
    match t {
        Ty::Record { fields } | Ty::OpenRecord { fields } => {
            for (i, f) in fields.iter().enumerate() {
                let mut next = steps.to_vec();
                next.push(Step::Field(i));
                let path = format!("{path}.{}", f.name);
                let evidence = format!("{evidence}.{}", f.name);
                out.push(FieldView {
                    name: f.name.clone(),
                    path: path.clone(),
                    evidence_path: evidence.clone(),
                    rule: rule.into(),
                    steps: next.clone(),
                    value: f.value.clone(),
                    optional: f.optional,
                });
                walk(s, &f.value, rule, &path, &evidence, &next, out, depth + 1);
            }
        }
        Ty::Array { items, .. } => {
            let mut next = steps.to_vec();
            next.push(Step::Item);
            walk(
                s,
                items,
                rule,
                &format!("{path}[]"),
                &format!("{evidence}[]"),
                &next,
                out,
                depth + 1,
            )
        }
        Ty::Choice { choices } => {
            for (i, t) in choices.iter().enumerate() {
                let mut next = steps.to_vec();
                next.push(Step::Choice(i));
                walk(
                    s,
                    t,
                    rule,
                    &format!("{path}/{i}"),
                    evidence,
                    &next,
                    out,
                    depth + 1,
                )
            }
        }
        Ty::Reference { name } => {
            if let Some(t) = s.types.get(name) {
                walk(s, t, name, path, evidence, &[], out, depth + 1)
            }
        }
        _ => (),
    }
}
fn first_rule(source: &str) -> Option<String> {
    source
        .lines()
        .filter_map(|l| l.split(';').next())
        .find_map(|l| l.split_once('=').map(|(name, _)| name.trim().to_string()))
}
pub fn schema_cddl(s: &SchemaDocument) -> String {
    s.types
        .iter()
        .map(|(name, t)| format!("{name} = {}\n", type_cddl(t, 0)))
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn type_cddl(t: &Ty, depth: usize) -> String {
    match t {
        Ty::Bool => "bool".into(),
        Ty::Uint => "uint".into(),
        Ty::Float => "float64".into(),
        Ty::Text => "tstr".into(),
        Ty::Bytes => "bstr".into(),
        Ty::Null => "null".into(),
        Ty::Integer { min, max } => format!("{min}..{max}"),
        Ty::Literal { value } => value.to_string(),
        Ty::Reference { name } => name.clone(),
        Ty::Size { value, min, max } => format!("{} .size ({min}..{max})", type_cddl(value, depth)),
        Ty::Choice { choices } => format!(
            "({})",
            choices
                .iter()
                .map(|t| type_cddl(t, depth))
                .collect::<Vec<_>>()
                .join(" / ")
        ),
        Ty::Array { items, min, max } => format!(
            "[ {}{} {} ]",
            if *min == 0 {
                "".into()
            } else {
                min.to_string()
            },
            max.map(|n| format!("*{n}")).unwrap_or("*".into()),
            type_cddl(items, depth)
        ),
        Ty::Record { fields } | Ty::OpenRecord { fields } => {
            let indent = "  ".repeat(depth + 1);
            let mut lines: Vec<_> = fields
                .iter()
                .map(|f| {
                    format!(
                        "{indent}{}{}: {}",
                        if f.optional { "? " } else { "" },
                        serde_json::to_string(&f.name).unwrap(),
                        type_cddl(&f.value, depth + 1)
                    )
                })
                .collect();
            if matches!(t, Ty::OpenRecord { .. }) {
                lines.push(format!("{indent}* tstr => any"));
            }
            format!("{{\n{}\n{}}}", lines.join(",\n"), "  ".repeat(depth))
        }
    }
}
pub fn semantic_type(t: &Ty) -> &'static str {
    match t {
        Ty::Bool => "Boolean",
        Ty::Uint | Ty::Float | Ty::Integer { .. } => "Number",
        Ty::Text => "String",
        Ty::Bytes => "Bytes",
        Ty::Null => "Null",
        Ty::Record { .. } | Ty::OpenRecord { .. } => "Object",
        Ty::Array { .. } => "Array",
        Ty::Size { value, .. } => semantic_type(value),
        Ty::Choice { .. } => "Choice",
        Ty::Reference { .. } => "Reference",
        Ty::Literal { .. } => "Literal",
    }
}
pub fn nullable(t: &Ty) -> bool {
    matches!(t, Ty::Null) || matches!(t,Ty::Choice{choices} if choices.iter().any(nullable))
}
pub fn with_nullable(t: Ty, yes: bool) -> Ty {
    let choices = match t {
        Ty::Choice { choices } => choices,
        other => vec![other],
    };
    let mut choices: Vec<_> = choices
        .into_iter()
        .filter(|t| !matches!(t, Ty::Null))
        .collect();
    if yes {
        choices.push(Ty::Null);
    }
    if choices.len() == 1 {
        choices.remove(0)
    } else if choices.is_empty() {
        Ty::Text
    } else {
        Ty::Choice { choices }
    }
}
fn infer(
    values: &[&Json],
    path: &str,
    depth: usize,
    observed: &mut BTreeMap<String, Observation>,
    pending: &mut BTreeSet<String>,
) -> Result<Ty, Error> {
    if depth > 48 {
        return Err(err("Sample nesting exceeds 48 levels"));
    }
    let mut observation = Observation {
        present: values.len(),
        total: values.len(),
        ..Observation::default()
    };
    let mut groups: BTreeMap<&str, Vec<&Json>> = BTreeMap::new();
    for v in values {
        let kind = match v {
            Json::Null => "null",
            Json::Bool(_) => "boolean",
            Json::Number(n) if n.is_f64() => "decimal",
            Json::Number(_) => "integer",
            Json::String(_) => "string",
            Json::Array(_) => "array",
            Json::Object(_) => "object",
        };
        observation.kinds.insert(kind.into());
        groups.entry(kind).or_default().push(v);
        if v.is_null() {
            observation.nulls += 1;
        }
        if let Some(s) = v.as_str() {
            observation.max_bytes = observation.max_bytes.max(s.len());
            if observation.strings.contains_key(s) || observation.strings.len() < 512 {
                *observation.strings.entry(s.into()).or_default() += 1;
            } else {
                observation.truncated = true;
            }
        }
    }
    observed.insert(path.into(), observation);
    let mut types = vec![];
    for (kind, items) in groups {
        types.push(match kind {
            "null" => Ty::Null,
            "boolean" => Ty::Bool,
            "string" => Ty::Text,
            "decimal" => Ty::Float,
            "integer" => {
                if items
                    .iter()
                    .any(|v| v.as_u64().is_some_and(|n| n > i64::MAX as u64))
                {
                    if items.iter().any(|v| v.as_i64().is_some_and(|n| n < 0)) {
                        Ty::Choice {
                            choices: vec![
                                Ty::Integer {
                                    min: i64::MIN as i128,
                                    max: i64::MAX as i128,
                                },
                                Ty::Uint,
                            ],
                        }
                    } else {
                        Ty::Uint
                    }
                } else {
                    Ty::Integer {
                        min: i64::MIN as i128,
                        max: i64::MAX as i128,
                    }
                }
            }
            "array" => {
                let all: Vec<_> = items
                    .iter()
                    .flat_map(|v| v.as_array().unwrap().iter())
                    .collect();
                let item = infer(&all, &format!("{path}[]"), depth + 1, observed, pending)?;
                Ty::Array {
                    items: Box::new(item),
                    min: 0,
                    max: None,
                }
            }
            "object" => {
                let names: BTreeSet<_> = items
                    .iter()
                    .flat_map(|v| v.as_object().unwrap().keys().cloned())
                    .collect();
                let mut fields = vec![];
                for name in names {
                    let sub: Vec<_> = items.iter().filter_map(|v| v.get(&name)).collect();
                    let key = format!("{path}.{name}");
                    let ty = infer(&sub, &key, depth + 1, observed, pending)?;
                    observed.get_mut(&key).unwrap().total = items.len();
                    pending.insert(key);
                    fields.push(Member {
                        name,
                        optional: true,
                        value: ty,
                    });
                }
                Ty::OpenRecord { fields }
            }
            _ => unreachable!(),
        });
    }
    if types.is_empty() || types.iter().all(|v| matches!(v, Ty::Null)) {
        pending.insert(path.into());
        return Ok(Ty::Choice {
            choices: vec![Ty::Text, Ty::Null],
        });
    }
    // Exact integer alternatives precede float64 when samples mix numeric kinds.
    types.sort_by_key(|t| matches!(t, Ty::Float));
    Ok(if types.len() == 1 {
        types.remove(0)
    } else {
        Ty::Choice { choices: types }
    })
}
// Reject out-of-domain integer literals before serde_json can round them into f64.
fn check_numbers(input: &str) -> Result<(), Error> {
    let b = input.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                } else if b[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else if b[i] == b'-' || b[i].is_ascii_digit() {
            let start = i;
            i += 1;
            while i < b.len() && (b[i].is_ascii_digit() || b".eE+-".contains(&b[i])) {
                i += 1;
            }
            let n = &input[start..i];
            if !n.contains(['.', 'e', 'E']) {
                let value = n.parse::<i128>().map_err(|_| {
                    err("Integer exceeds supported 64-bit precision; supply it as text")
                })?;
                if value < i64::MIN as i128 || value > u64::MAX as i128 {
                    return Err(err(
                        "Integer exceeds supported 64-bit precision; supply it as text",
                    ));
                }
            }
        } else {
            i += 1;
        }
    }
    Ok(())
}
struct UniqueJson(Json);
impl<'de> serde::Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Json;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON value")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Json, E> {
                Ok(v.into())
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Json, E> {
                Ok(v.into())
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Json, E> {
                Ok(v.into())
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Json, E> {
                serde_json::Number::from_f64(v)
                    .map(Json::Number)
                    .ok_or_else(|| E::custom("Non-finite number"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Json, E> {
                Ok(v.into())
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Json, E> {
                Ok(v.into())
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Json, E> {
                Ok(Json::Null)
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Json, E> {
                Ok(Json::Null)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> Result<Json, A::Error> {
                let mut v = vec![];
                while let Some(x) = a.next_element::<UniqueJson>()? {
                    v.push(x.0);
                }
                Ok(Json::Array(v))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut a: A) -> Result<Json, A::Error> {
                let mut m = serde_json::Map::new();
                while let Some((k, v)) = a.next_entry::<String, UniqueJson>()? {
                    if m.insert(k, v.0).is_some() {
                        return Err(serde::de::Error::custom("Duplicate JSON key"));
                    }
                }
                Ok(Json::Object(m))
            }
        }
        d.deserialize_any(Visitor).map(Self)
    }
}
pub fn parse_samples(input: &str) -> Result<Vec<Json>, Error> {
    if input.len() > MAX_INPUT {
        return Err(err("Sample input exceeds 64 MiB"));
    }
    check_numbers(input)?;
    let stream = serde_json::Deserializer::from_str(input).into_iter::<UniqueJson>();
    let mut out = vec![];
    for v in stream {
        match v.map_err(|e| err(e.to_string()))?.0 {
            Json::Object(m) => out.push(Json::Object(m)),
            Json::Array(a) => {
                if !a.iter().all(Json::is_object) {
                    return Err(err("A sample array must contain records (objects)"));
                }
                out.extend(a)
            }
            _ => {
                return Err(err(
                    "Supply an object, an array of records, or JSONL records",
                ));
            }
        };
        if out.len() > MAX_RECORDS {
            return Err(err(
                "More than 10,000 records; supply a representative sample",
            ));
        }
    }
    if out.is_empty() {
        return Err(err("No sample records"));
    }
    Ok(out)
}
fn json_equivalent(a: &Json, b: &Json) -> bool {
    match (a, b) {
        (Json::Number(a), Json::Number(b)) if a.is_f64() || b.is_f64() => {
            if (a.is_u64() && a.as_u64().unwrap() > 1 << 53)
                || (a.is_i64() && a.as_i64().unwrap().unsigned_abs() > 1 << 53)
            {
                return false;
            }
            a.as_f64() == b.as_f64()
        }
        (Json::Object(a), Json::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.get(k).is_some_and(|w| json_equivalent(v, w)))
        }
        (Json::Array(a), Json::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(v, w)| json_equivalent(v, w))
        }
        _ => a == b,
    }
}
