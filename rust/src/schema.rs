//! CDDL is compiled once into a JSON-editable schema and positional PIL layouts.
//! Unsupported constructs are errors, never silently relaxed validation.
use crate::{Array, Definition, Error, Field, Scalar, Type, Value, decode_exact, encode};
#[cfg(feature = "schema")]
use cddl::ast;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SchemaType {
    Bool,
    Uint,
    Float,
    Text,
    Bytes,
    Null,
    Integer {
        #[serde(deserialize_with = "bound")]
        min: i128,
        #[serde(deserialize_with = "bound")]
        max: i128,
    },
    Literal {
        value: Json,
    },
    Reference {
        name: String,
    },
    Record {
        fields: Vec<Member>,
    },
    OpenRecord {
        fields: Vec<Member>,
    },
    Array {
        items: Box<SchemaType>,
        min: usize,
        max: Option<usize>,
    },
    Choice {
        choices: Vec<SchemaType>,
    },
    Size {
        value: Box<SchemaType>,
        min: usize,
        max: usize,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub name: String,
    #[serde(default)]
    pub optional: bool,
    pub value: SchemaType,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SchemaDocument {
    pub root: String,
    pub types: BTreeMap<String, SchemaType>,
    /// Qualified field paths, e.g. purchase.customer.name. Units are UTF-8 bytes.
    #[serde(default)]
    pub fixed_strings: BTreeMap<String, u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dictionaries: BTreeMap<String, Vec<String>>,
}
fn error(message: impl Into<String>) -> Error {
    Error::Definition(message.into())
}
impl SchemaDocument {
    #[cfg(feature = "schema")]
    pub fn from_cddl(source: &str, root: &str) -> Result<Self, Error> {
        let parsed = cddl::parser::cddl_from_str(source, false).map_err(error)?;
        let mut types = BTreeMap::new();
        for rule in &parsed.rules {
            let ast::Rule::Type { rule, .. } = rule else {
                return Err(error("CDDL group rules are not yet supported"));
            };
            if rule.generic_params.is_some() || rule.is_type_choice_alternate {
                return Err(error(
                    "CDDL generics and rule extensions are not yet supported",
                ));
            }
            let name = rule.name.ident.to_owned();
            if types
                .insert(name.clone(), lower_type(&rule.value)?)
                .is_some()
            {
                return Err(error(format!("duplicate CDDL rule {name}")));
            }
        }
        let document = Self {
            root: root.into(),
            types,
            fixed_strings: BTreeMap::new(),
            dictionaries: BTreeMap::new(),
        };
        document.compile()?;
        Ok(document)
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self, Error> {
        let document: Self = serde_json::from_slice(bytes).map_err(|e| error(e.to_string()))?;
        document.compile()?;
        Ok(document)
    }
    pub fn to_json(&self) -> Result<Vec<u8>, Error> {
        serde_json::to_vec_pretty(self).map_err(|e| error(e.to_string()))
    }
    pub fn to_pil(&self) -> Result<Vec<u8>, Error> {
        let compiled = self.compile()?;
        let value = ciborium::Value::serialized(self).map_err(|e| error(e.to_string()))?;
        let ciborium::Value::Map(mut document) =
            crate::object::decode_value(&compiled.layout().to_pil()?)?
        else {
            return Err(error("invalid compiled Definition"));
        };
        document.push((ciborium::Value::from("cddl"), value));
        crate::object::encode_value(&ciborium::Value::Map(document))
    }
    pub fn from_pil(bytes: &[u8]) -> Result<Self, Error> {
        let value = crate::object::decode_value(bytes)?;
        let schema = Self::from_value(&value, &Definition::from_pil(bytes)?)?;
        Ok(schema)
    }
    pub(crate) fn from_value(
        value: &ciborium::Value,
        definition: &Definition,
    ) -> Result<Self, Error> {
        let value = value
            .as_map()
            .and_then(|m| {
                m.iter()
                    .find(|(k, _)| k.as_text() == Some("cddl"))
                    .map(|(_, v)| v)
            })
            .ok_or_else(|| error("missing CDDL schema annotation"))?;
        let schema: Self = value.deserialized().map_err(|e| error(e.to_string()))?;
        if schema.compile()?.layout() != definition {
            return Err(error("CDDL schema disagrees with its compiled PIL layout"));
        }
        Ok(schema)
    }
    pub fn compile(&self) -> Result<CompiledSchema, Error> {
        let root = self
            .types
            .get(&self.root)
            .ok_or_else(|| error("missing root rule"))?;
        let mut used = HashSet::new();
        let plan = compile_node(self, root, &self.root, &mut Vec::new(), &mut used, 0)?;
        if let Some(path) = self
            .fixed_strings
            .keys()
            .chain(self.dictionaries.keys())
            .find(|p| !used.contains(p.as_str()))
        {
            return Err(error(format!("unused storage binding {path}")));
        }
        Ok(CompiledSchema { plan })
    }
}

#[cfg(feature = "schema")]
fn lower_type(t: &ast::Type<'_>) -> Result<SchemaType, Error> {
    let choices: Vec<_> = t
        .type_choices
        .iter()
        .map(|c| lower_one(&c.type1))
        .collect::<Result<_, _>>()?;
    if choices.len() == 1 {
        Ok(choices.into_iter().next().unwrap())
    } else {
        Ok(SchemaType::Choice { choices })
    }
}
#[cfg(feature = "schema")]
fn number(t: &ast::Type2<'_>) -> Option<i128> {
    match t {
        ast::Type2::IntValue { value, .. } => Some(*value as i128),
        ast::Type2::UintValue { value, .. } => Some(*value as i128),
        _ => None,
    }
}
#[cfg(feature = "schema")]
fn lower_one(t: &ast::Type1<'_>) -> Result<SchemaType, Error> {
    if let Some(op) = &t.operator {
        match &op.operator {
            ast::RangeCtlOp::RangeOp { is_inclusive, .. } => {
                let min = number(&t.type2)
                    .ok_or_else(|| error("integer range requires literal bounds"))?;
                let mut max = number(&op.type2)
                    .ok_or_else(|| error("integer range requires literal bounds"))?;
                if !is_inclusive {
                    max -= 1;
                }
                return Ok(SchemaType::Integer { min, max });
            }
            ast::RangeCtlOp::CtlOp { ctrl, .. } if ctrl.to_string() == ".size" => {
                let value = Box::new(lower_atom(&t.type2)?);
                let (min, max) = if let Some(n) = number(&op.type2) {
                    (n, n)
                } else if let ast::Type2::ParenthesizedType { pt, .. } = &op.type2 {
                    match lower_type(pt)? {
                        SchemaType::Integer { min, max } => (min, max),
                        _ => return Err(error(".size requires an integer or bounded range")),
                    }
                } else {
                    return Err(error(".size requires literal bounds"));
                };
                let min = usize::try_from(min).map_err(|_| error("negative size"))?;
                let max = usize::try_from(max).map_err(|_| error("negative size"))?;
                return Ok(SchemaType::Size { value, min, max });
            }
            _ => return Err(error(format!("unsupported CDDL control {}", op.operator))),
        }
    }
    lower_atom(&t.type2)
}
#[cfg(feature = "schema")]
fn occurrence(o: Option<&ast::Occurrence<'_>>) -> (usize, Option<usize>) {
    match o.map(|o| &o.occur) {
        None => (1, Some(1)),
        Some(ast::Occur::Optional { .. }) => (0, Some(1)),
        Some(ast::Occur::ZeroOrMore { .. }) => (0, None),
        Some(ast::Occur::OneOrMore { .. }) => (1, None),
        Some(ast::Occur::Exact { lower, upper, .. }) => (lower.unwrap_or(0), *upper),
    }
}
#[cfg(feature = "schema")]
fn lower_atom(t: &ast::Type2<'_>) -> Result<SchemaType, Error> {
    Ok(match t {
        ast::Type2::Typename {
            ident,
            generic_args,
            ..
        } => {
            if generic_args.is_some() {
                return Err(error("generic references are not yet supported"));
            }
            match ident.ident {
                "uint" => SchemaType::Uint,
                "bool" => SchemaType::Bool,
                "float" | "float64" => SchemaType::Float,
                "tstr" | "text" => SchemaType::Text,
                "bstr" | "bytes" => SchemaType::Bytes,
                "nil" | "null" => SchemaType::Null,
                "true" => SchemaType::Literal {
                    value: Json::Bool(true),
                },
                "false" => SchemaType::Literal {
                    value: Json::Bool(false),
                },
                "int" | "nint" => {
                    return Err(error(
                        "unbounded int/nint requires a wider integer profile; use explicit 64-bit bounds",
                    ));
                }
                "any" => {
                    return Err(error(
                        "unconstrained any is not yet supported by the CDDL compiler",
                    ));
                }
                name => SchemaType::Reference { name: name.into() },
            }
        }
        ast::Type2::IntValue { value, .. } => SchemaType::Literal {
            value: Json::from(*value as i64),
        },
        ast::Type2::UintValue { value, .. } => SchemaType::Literal {
            value: Json::from(*value as u64),
        },
        ast::Type2::FloatValue { value, .. } => SchemaType::Literal {
            value: Json::from(*value),
        },
        ast::Type2::TextValue { value, .. } => SchemaType::Literal {
            value: Json::String(value.to_string()),
        },
        ast::Type2::ParenthesizedType { pt, .. } => lower_type(pt)?,
        ast::Type2::Map { group, .. } => {
            if group.group_choices.len() != 1 {
                return Err(error("map group choices are not yet supported"));
            }
            let mut fields = Vec::new();
            let mut open = false;
            for (entry, _) in &group.group_choices[0].group_entries {
                let ast::GroupEntry::ValueMemberKey { ge, .. } = entry else {
                    return Err(error("map group inclusion is not yet supported"));
                };
                if let Some(ast::MemberKey::Type1 { t1, .. }) = &ge.member_key {
                    let key = t1.to_string();
                    if matches!(key.trim(), "tstr" | "text")
                        && ge.entry_type.to_string().trim() == "any"
                        && occurrence(ge.occur.as_ref()) == (0, None)
                        && !open
                    {
                        open = true;
                        continue;
                    }
                    return Err(error(
                        "only * tstr => any is supported for additional fields",
                    ));
                }
                let name = match &ge.member_key {
                    Some(ast::MemberKey::Bareword { ident, .. }) => ident.ident.to_string(),
                    Some(ast::MemberKey::Value {
                        value: cddl::token::Value::TEXT(text),
                        ..
                    }) => text.to_string(),
                    _ => {
                        return Err(error(
                            "record fields require literal text keys; dynamic maps are not yet supported",
                        ));
                    }
                };
                let (min, max) = occurrence(ge.occur.as_ref());
                if max != Some(1) || min > 1 {
                    return Err(error(
                        "record field occurrence must be required or optional",
                    ));
                }
                fields.push(Member {
                    name,
                    optional: min == 0,
                    value: lower_type(&ge.entry_type)?,
                });
            }
            if open {
                SchemaType::OpenRecord { fields }
            } else {
                SchemaType::Record { fields }
            }
        }
        ast::Type2::Array { group, .. } => {
            if group.group_choices.len() != 1 || group.group_choices[0].group_entries.len() != 1 {
                return Err(error(
                    "only homogeneous CDDL arrays are supported initially",
                ));
            }
            let (entry, _) = &group.group_choices[0].group_entries[0];
            let (items, min, max) = match entry {
                ast::GroupEntry::ValueMemberKey { ge, .. } if ge.member_key.is_none() => {
                    let (min, max) = occurrence(ge.occur.as_ref());
                    (lower_type(&ge.entry_type)?, min, max)
                }
                ast::GroupEntry::TypeGroupname { ge, .. } if ge.generic_args.is_none() => {
                    let (min, max) = occurrence(ge.occur.as_ref());
                    (
                        SchemaType::Reference {
                            name: ge.name.ident.to_string(),
                        },
                        min,
                        max,
                    )
                }
                _ => return Err(error("unsupported array group")),
            };
            SchemaType::Array {
                items: Box::new(items),
                min,
                max,
            }
        }
        _ => return Err(error(format!("unsupported CDDL expression {t}"))),
    })
}

#[derive(Debug, Clone)]
enum Shape {
    Scalar(Scalar),
    Record(Vec<(Member, Plan)>, bool),
    Array(Box<Plan>),
    Choice(Vec<Plan>),
}
#[derive(Debug, Clone)]
struct Plan {
    schema: SchemaType,
    layout: Definition,
    shape: Shape,
    dictionary: Vec<String>,
    dictionary_ids: HashMap<String, u64>,
}
#[derive(Debug, Clone)]
pub struct CompiledSchema {
    plan: Plan,
}
fn scalar_for(t: &SchemaType) -> Result<Scalar, Error> {
    Ok(match t {
        SchemaType::Bool => Scalar::Bool,
        SchemaType::Uint => Scalar::U64,
        SchemaType::Float => Scalar::F64,
        SchemaType::Text => Scalar::String,
        SchemaType::Bytes | SchemaType::Null => Scalar::Bytes,
        SchemaType::Integer { min, max } => {
            if min > max
                || *min < i64::MIN as i128
                || *max > u64::MAX as i128
                || (*min < 0 && *max > i64::MAX as i128)
            {
                return Err(error("integer bounds do not fit a PIL scalar"));
            }
            if *min >= 0 {
                if *max <= u8::MAX as i128 {
                    Scalar::U8
                } else if *max <= u16::MAX as i128 {
                    Scalar::U16
                } else if *max <= 0xffffff {
                    Scalar::U24
                } else if *max <= u32::MAX as i128 {
                    Scalar::U32
                } else {
                    Scalar::U64
                }
            } else if *min >= i8::MIN as i128 && *max <= i8::MAX as i128 {
                Scalar::I8
            } else if *min >= i16::MIN as i128 && *max <= i16::MAX as i128 {
                Scalar::I16
            } else if *min >= i32::MIN as i128 && *max <= i32::MAX as i128 {
                Scalar::I32
            } else {
                Scalar::I64
            }
        }
        SchemaType::Size { value, min, max } => {
            if min > max {
                return Err(error("invalid size bounds"));
            }
            match value.as_ref() {
                SchemaType::Text => Scalar::String,
                SchemaType::Bytes => Scalar::Bytes,
                SchemaType::Uint if *max <= 8 => {
                    let max = if *max == 8 {
                        u64::MAX as i128
                    } else {
                        (1i128 << (max * 8)) - 1
                    };
                    scalar_for(&SchemaType::Integer { min: 0, max })?
                }
                _ => {
                    return Err(error(
                        ".size currently supports text, bytes, and uint with up to 8 bytes",
                    ));
                }
            }
        }
        SchemaType::Literal { value } => {
            if value.is_string() {
                Scalar::String
            } else if value.is_boolean() {
                Scalar::Bool
            } else if value.is_u64() {
                Scalar::U64
            } else if value.is_i64() {
                Scalar::I64
            } else if value.is_f64() {
                Scalar::F64
            } else if value.is_null() {
                Scalar::Bytes
            } else {
                return Err(error("unsupported literal"));
            }
        }
        _ => return Err(error("expected scalar schema")),
    })
}
fn compile_node(
    doc: &SchemaDocument,
    t: &SchemaType,
    path: &str,
    refs: &mut Vec<String>,
    used: &mut HashSet<String>,
    depth: usize,
) -> Result<Plan, Error> {
    if depth > 64 {
        return Err(error("schema nesting limit"));
    }
    if let SchemaType::Reference { name } = t {
        if refs.contains(name) {
            return Err(error(format!("recursive rule {name} is not yet supported")));
        }
        let target = doc
            .types
            .get(name)
            .ok_or_else(|| error(format!("undefined CDDL rule {name}")))?;
        refs.push(name.clone());
        let result = compile_node(doc, target, path, refs, used, depth + 1);
        refs.pop();
        return result;
    }
    let shape = match t {
        SchemaType::Record { fields } | SchemaType::OpenRecord { fields } => {
            let open = matches!(t, SchemaType::OpenRecord { .. });
            if fields.is_empty() && !open {
                return Err(error("empty records are not yet supported"));
            }
            let mut names = HashSet::new();
            let mut plans = Vec::new();
            for field in fields {
                if open && field.name == "@fields" {
                    return Err(error("@fields is reserved for additional field storage"));
                }
                if !names.insert(&field.name) {
                    return Err(error("duplicate record member"));
                }
                plans.push((
                    field.clone(),
                    compile_node(
                        doc,
                        &field.value,
                        &format!("{path}.{}", field.name),
                        refs,
                        used,
                        depth + 1,
                    )?,
                ));
            }
            Shape::Record(plans, open)
        }
        SchemaType::Array { items, min, max } => {
            if max.is_some_and(|max| max < *min) {
                return Err(error("invalid array occurrence bounds"));
            }
            Shape::Array(Box::new(compile_node(
                doc,
                items,
                &format!("{path}[]"),
                refs,
                used,
                depth + 1,
            )?))
        }
        SchemaType::Choice { choices } => {
            if choices.len() < 2 {
                return Err(error("choice needs at least two alternatives"));
            }
            Shape::Choice(
                choices
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        compile_node(doc, t, &format!("{path}/{i}"), refs, used, depth + 1)
                    })
                    .collect::<Result<_, _>>()?,
            )
        }
        _ => {
            let mut scalar = scalar_for(t)?;
            if let Some(&capacity) = doc.fixed_strings.get(path) {
                if scalar != Scalar::String || capacity == 0 {
                    return Err(error(
                        "fixed-string binding requires text and positive capacity",
                    ));
                }
                let max = match t {
                    SchemaType::Size { max, .. } => Some(*max),
                    SchemaType::Literal {
                        value: Json::String(s),
                    } => Some(s.len()),
                    _ => None,
                };
                if max.is_none_or(|max| max > capacity as usize) {
                    return Err(error("fixed capacity must cover a bounded CDDL text size"));
                }
                scalar = Scalar::FixedString(capacity);
                used.insert(path.to_string());
            }
            Shape::Scalar(scalar)
        }
    };
    let dictionary = doc.dictionaries.get(path).cloned().unwrap_or_default();
    if doc.dictionaries.contains_key(path) {
        if dictionary.is_empty() || !matches!(shape, Shape::Scalar(Scalar::String)) {
            return Err(error(
                "dictionary binding requires variable text and at least one value",
            ));
        }
        if !used.insert(path.to_string()) {
            return Err(error("ambiguous storage binding path"));
        }
    }
    let fields = match &shape {
        Shape::Record(fields, open) => {
            let mut result: Vec<_> = fields.iter().map(|(f, p)| p.field(&f.name)).collect();
            if *open {
                result.push(Field::new("@fields", Scalar::Object));
            }
            result
        }
        Shape::Array(_) => vec![Field::new("items", Type::Array(Scalar::Bytes))],
        Shape::Choice(_) => vec![
            Field::new("choice", Scalar::U32),
            Field::new("value", Scalar::Bytes),
        ],
        Shape::Scalar(kind) => vec![if dictionary.is_empty() {
            Field::new("value", *kind)
        } else {
            Field::mapped("value", dictionary.clone())
        }],
    };
    Ok(Plan {
        schema: t.clone(),
        layout: Definition::new(path, 0, fields)?,
        shape,
        dictionary_ids: dictionary
            .iter()
            .enumerate()
            .map(|(i, s)| (s.clone(), i as u64))
            .collect(),
        dictionary,
    })
}
impl Plan {
    fn field(&self, name: &str) -> Field {
        if self.dictionary.is_empty() {
            Field::new(name, self.scalar())
        } else {
            Field::mapped(name, self.dictionary.clone())
        }
    }
    fn value<'a>(
        &self,
        kind: Scalar,
        value: &'a Json,
        bytes: &'a [u8],
    ) -> Result<Value<'a>, Error> {
        if let Some(s) = value.as_str() {
            if let Some(&i) = self.dictionary_ids.get(s) {
                return Ok(Value::Mapped(i));
            }
        }
        scalar_value(kind, value, bytes)
    }
    fn json(&self, value: Value<'_>) -> Result<Json, Error> {
        if let Value::Mapped(i) = value {
            return self
                .dictionary
                .get(i as usize)
                .cloned()
                .map(Json::String)
                .ok_or_else(|| error("dictionary ID outside definition"));
        }
        scalar_json(value)
    }
    fn scalar(&self) -> Scalar {
        match self.shape {
            Shape::Scalar(s) => s,
            _ => Scalar::Bytes,
        }
    }
    fn validate(&self, value: &Json) -> Result<(), Error> {
        let valid = match (&self.schema, &self.shape) {
            (_, Shape::Record(fields, open)) => {
                let map = value.as_object().ok_or_else(|| error("expected object"))?;
                if !open
                    && map
                        .keys()
                        .any(|k| !fields.iter().any(|(f, _)| f.name == *k))
                {
                    return Err(error("unknown record member"));
                }
                for (f, p) in fields {
                    match map.get(&f.name) {
                        Some(v) => p.validate(v)?,
                        None if f.optional => (),
                        None => return Err(error(format!("missing {}", f.name))),
                    }
                }
                true
            }
            (SchemaType::Array { min, max, .. }, Shape::Array(item)) => {
                let a = value.as_array().ok_or_else(|| error("expected array"))?;
                if a.len() < *min || max.is_some_and(|n| a.len() > n) {
                    return Err(error("array occurrence constraint"));
                }
                for v in a {
                    item.validate(v)?;
                }
                true
            }
            (_, Shape::Choice(choices)) => choices.iter().any(|p| p.validate(value).is_ok()),
            (SchemaType::Bool, _) => value.is_boolean(),
            (SchemaType::Uint, _) => value.is_u64(),
            (SchemaType::Float, _) => value.is_number(),
            (SchemaType::Text, _) => value.is_string(),
            (SchemaType::Bytes, _) => bytes_json(value).is_ok(),
            (SchemaType::Null, _) => value.is_null(),
            (SchemaType::Integer { min, max }, _) => {
                integer_json(value).is_some_and(|n| n >= *min && n <= *max)
            }
            (SchemaType::Literal { value: expected }, _) => value == expected,
            (
                SchemaType::Size {
                    value: base,
                    min,
                    max,
                },
                _,
            ) => match base.as_ref() {
                SchemaType::Text => value
                    .as_str()
                    .is_some_and(|s| s.len() >= *min && s.len() <= *max),
                SchemaType::Bytes => {
                    bytes_json(value).is_ok_and(|b| b.len() >= *min && b.len() <= *max)
                }
                SchemaType::Uint => value
                    .as_u64()
                    .is_some_and(|n| *max == 8 || n < (1u64 << (max * 8))),
                _ => false,
            },
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(error(format!(
                "value violates schema {}",
                self.layout.identity()
            )))
        }
    }
    fn encode(&self, value: &Json) -> Result<Vec<u8>, Error> {
        match &self.shape {
            Shape::Scalar(kind) => {
                let b = if *kind == Scalar::Bytes && !value.is_null() {
                    bytes_json(value)?
                } else {
                    Vec::new()
                };
                encode(&self.layout, &[self.value(*kind, value, &b)?])
            }
            Shape::Record(fields, open) => {
                let map = value.as_object().ok_or_else(|| error("expected object"))?;
                let mut storage = Vec::with_capacity(fields.len());
                for (f, p) in fields {
                    storage.push(match map.get(&f.name) {
                        Some(v)
                            if matches!(p.shape, Shape::Scalar(Scalar::Bytes)) && !v.is_null() =>
                        {
                            bytes_json(v)?
                        }
                        Some(v) if !matches!(p.shape, Shape::Scalar(_)) => p.encode(v)?,
                        _ => Vec::new(),
                    });
                }
                let additional = if *open {
                    let extras: serde_json::Map<_, _> = map
                        .iter()
                        .filter(|(k, _)| !fields.iter().any(|(f, _)| &f.name == *k))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    if extras.is_empty() {
                        Vec::new()
                    } else {
                        crate::object::encode_value(
                            &ciborium::Value::serialized(&Json::Object(extras))
                                .map_err(|e| error(e.to_string()))?,
                        )?
                    }
                } else {
                    Vec::new()
                };
                let mut values = Vec::with_capacity(fields.len() + usize::from(*open));
                for (i, (f, p)) in fields.iter().enumerate() {
                    values.push(match map.get(&f.name) {
                        None => Value::Absent,
                        Some(v) => match p.shape {
                            Shape::Scalar(k) => p.value(k, v, &storage[i])?,
                            _ => Value::Bytes(&storage[i]),
                        },
                    });
                }
                if *open {
                    values.push(if additional.is_empty() {
                        Value::Absent
                    } else {
                        Value::Object(&additional)
                    });
                }
                encode(&self.layout, &values)
            }
            Shape::Array(item) => {
                let storage: Vec<_> = value
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| item.encode(v))
                    .collect::<Result<_, _>>()?;
                let values: Vec<_> = storage.iter().map(|b| Value::Bytes(b)).collect();
                encode(&self.layout, &[Value::Array(Array::Items(&values))])
            }
            Shape::Choice(choices) => {
                let i = choices
                    .iter()
                    .position(|p| p.validate(value).is_ok())
                    .ok_or_else(|| error("no matching choice"))?;
                let b = choices[i].encode(value)?;
                encode(&self.layout, &[Value::U32(i as u32), Value::Bytes(&b)])
            }
        }
    }
    fn decode(&self, bytes: &[u8]) -> Result<Json, Error> {
        let frame = decode_exact(&self.layout, bytes)?;
        let values = frame.values();
        match &self.shape {
            Shape::Scalar(_) => self.json(values[0]),
            Shape::Record(fields, open) => {
                let mut map = serde_json::Map::new();
                for (i, (f, p)) in fields.iter().enumerate() {
                    if values[i] == Value::Absent {
                        continue;
                    }
                    let value = if matches!(p.shape, Shape::Scalar(_)) {
                        p.json(values[i])?
                    } else {
                        let Value::Bytes(b) = values[i] else {
                            return Err(error("invalid nested record payload"));
                        };
                        p.decode(b)?
                    };
                    map.insert(f.name.clone(), value);
                }
                if *open && values[fields.len()] != Value::Absent {
                    let Value::Object(raw) = values[fields.len()] else {
                        return Err(error("invalid additional fields"));
                    };
                    let extra: Json = crate::object::decode_value(raw)?
                        .deserialized()
                        .map_err(|e| error(e.to_string()))?;
                    let extra = extra
                        .as_object()
                        .ok_or_else(|| error("additional fields must be an object"))?;
                    for (key, value) in extra {
                        if fields.iter().any(|(f, _)| &f.name == key) {
                            return Err(error("additional fields override a declared field"));
                        }
                        map.insert(key.clone(), value.clone());
                    }
                }
                Ok(Json::Object(map))
            }
            Shape::Array(item) => {
                let Value::Array(Array::Encoded(array)) = values[0] else {
                    return Err(error("invalid array payload"));
                };
                let mut result = Vec::new();
                for value in array.iter() {
                    let Value::Bytes(b) = value? else {
                        return Err(error("invalid array item"));
                    };
                    result.push(item.decode(b)?);
                }
                Ok(Json::Array(result))
            }
            Shape::Choice(choices) => {
                let (Value::U32(i), Value::Bytes(b)) = (values[0], values[1]) else {
                    return Err(error("invalid choice payload"));
                };
                choices
                    .get(i as usize)
                    .ok_or_else(|| error("choice index out of bounds"))?
                    .decode(b)
            }
        }
    }
}
impl CompiledSchema {
    pub fn layout(&self) -> &Definition {
        &self.plan.layout
    }
    pub fn validate_json(&self, value: &Json) -> Result<(), Error> {
        self.plan.validate(value)
    }
    pub fn encode_json(&self, value: &Json) -> Result<Vec<u8>, Error> {
        self.plan.validate(value)?;
        self.plan.encode(value)
    }
    pub fn decode_json(&self, bytes: &[u8]) -> Result<Json, Error> {
        let value = self.plan.decode(bytes)?;
        self.plan.validate(&value)?;
        Ok(value)
    }
    /// Trusted PAC payload: structural decoding only; schema constraints were checked at ingress.
    pub fn decode_trusted_json(&self, bytes: &[u8]) -> Result<Json, Error> {
        self.plan.decode(bytes)
    }
}
fn integer_json(value: &Json) -> Option<i128> {
    value
        .as_u64()
        .map(|n| n as i128)
        .or_else(|| value.as_i64().map(|n| n as i128))
}
fn bytes_json(value: &Json) -> Result<Vec<u8>, Error> {
    value
        .as_array()
        .ok_or_else(|| error("bytes JSON view must be an array of octets"))?
        .iter()
        .map(|v| {
            v.as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or_else(|| error("invalid octet"))
        })
        .collect()
}
fn scalar_value<'a>(kind: Scalar, value: &'a Json, bytes: &'a [u8]) -> Result<Value<'a>, Error> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    let n = integer_json(value).unwrap_or(0);
    Ok(match kind {
        Scalar::Bool => Value::Bool(value.as_bool().ok_or_else(|| error("expected bool"))?),
        Scalar::U8 => Value::U8(u8::try_from(n).map_err(|_| Error::Overflow)?),
        Scalar::U16 => Value::U16(u16::try_from(n).map_err(|_| Error::Overflow)?),
        Scalar::U24 => Value::U24(
            u32::try_from(n)
                .ok()
                .filter(|n| *n <= 0xffffff)
                .ok_or(Error::Overflow)?,
        ),
        Scalar::U32 => Value::U32(u32::try_from(n).map_err(|_| Error::Overflow)?),
        Scalar::U64 => Value::U64(u64::try_from(n).map_err(|_| Error::Overflow)?),
        Scalar::I8 => Value::I8(i8::try_from(n).map_err(|_| Error::Overflow)?),
        Scalar::I16 => Value::I16(i16::try_from(n).map_err(|_| Error::Overflow)?),
        Scalar::I32 => Value::I32(i32::try_from(n).map_err(|_| Error::Overflow)?),
        Scalar::I64 => Value::I64(i64::try_from(n).map_err(|_| Error::Overflow)?),
        Scalar::F64 => Value::F64(value.as_f64().ok_or_else(|| error("expected float"))?),
        Scalar::String | Scalar::FixedString(_) => {
            Value::String(value.as_str().ok_or_else(|| error("expected text"))?)
        }
        Scalar::Bytes => Value::Bytes(bytes),
        _ => return Err(error("unsupported compiled scalar")),
    })
}
fn scalar_json(value: Value<'_>) -> Result<Json, Error> {
    Ok(match value {
        Value::Null => Json::Null,
        Value::Bool(v) => Json::from(v),
        Value::U8(v) => Json::from(v),
        Value::U16(v) => Json::from(v),
        Value::U24(v) | Value::U32(v) => Json::from(v),
        Value::U64(v) => Json::from(v),
        Value::I8(v) => Json::from(v),
        Value::I16(v) => Json::from(v),
        Value::I32(v) => Json::from(v),
        Value::I64(v) => Json::from(v),
        Value::F64(v) if v.is_finite() => Json::from(v),
        Value::String(s) => Json::String(s.into()),
        Value::Bytes(b) => Json::Array(b.iter().map(|v| Json::from(*v)).collect()),
        _ => return Err(error("value has no JSON representation")),
    })
}

fn bound<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i128, D::Error> {
    struct Bound;
    impl<'de> serde::de::Visitor<'de> for Bound {
        type Value = i128;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an exact signed or unsigned integer bound")
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<i128, E> {
            Ok(v as i128)
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<i128, E> {
            Ok(v as i128)
        }
    }
    d.deserialize_any(Bound)
}
