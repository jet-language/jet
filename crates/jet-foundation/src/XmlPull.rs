//! Shared std-only encoding engines compiled by generated and comptime tiers.
//! The primary engine here is the XML 1.0 pull tokenizer.

use std::collections::{BTreeMap, BTreeSet};

/// D-ENCBASE-STRICT1=A edition-2026 compatibility engine. This module lives in
/// the existing std-only generated/comptime seam so both tiers compile exactly
/// one decoder source. Edition 2027 strict defaults remain toolchain-gated.
pub mod base_encoding_2026 {
    fn base64_digit(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    fn historical_aot(text: &str) -> Option<Vec<u8>> {
        let input: Vec<u8> = text.bytes().filter(|&b| !b.is_ascii_whitespace()).collect();
        if input.len() % 4 != 0 {
            return None;
        }
        let mut out = Vec::with_capacity(input.len() / 4 * 3);
        for chunk in input.chunks(4) {
            let a = base64_digit(chunk[0])?;
            let b = base64_digit(chunk[1])?;
            out.push((a << 2) | (b >> 4));
            if chunk[2] != b'=' {
                let c = base64_digit(chunk[2])?;
                out.push((b << 4) | (c >> 2));
                if chunk[3] != b'=' {
                    let d = base64_digit(chunk[3])?;
                    out.push((c << 6) | d);
                }
            }
        }
        Some(out)
    }

    fn historical_comptime(text: &str) -> Option<Vec<u8>> {
        let input = text.trim_end_matches('=').as_bytes();
        if input.len() % 4 == 1 {
            return None;
        }
        let mut out = Vec::with_capacity(input.len() / 4 * 3);
        for chunk in input.chunks(4) {
            let digits: Vec<u8> = chunk
                .iter()
                .map(|&byte| base64_digit(byte))
                .collect::<Option<_>>()?;
            out.push((digits[0] << 2) | (digits.get(1).copied().unwrap_or(0) >> 4));
            if digits.len() > 2 {
                out.push((digits[1] << 4) | (digits[2] >> 2));
            }
            if digits.len() > 3 {
                out.push((digits[2] << 6) | digits[3]);
            }
        }
        Some(out)
    }

    fn error(text: &str, url: bool) -> String {
        let label = if url { "base64url" } else { "base64" };
        let alphabet = if url {
            "URL-safe base64"
        } else {
            "standard base64"
        };
        for (offset, &byte) in text.as_bytes().iter().enumerate() {
            let accepted = byte.is_ascii_alphanumeric()
                || matches!(byte, b'=' | b'+' | b'/')
                || (url && matches!(byte, b'-' | b'_'))
                || byte.is_ascii_whitespace();
            if !accepted {
                return format!(
                    "invalid {label} at byte {offset}: byte 0x{byte:02X} is not in the {alphabet} alphabet"
                );
            }
        }
        if let Some(offset) = text.as_bytes().iter().position(|&byte| byte == b'=') {
            if text.as_bytes()[offset + 1..]
                .iter()
                .any(|&byte| byte != b'=' && !byte.is_ascii_whitespace())
            {
                return format!(
                    "invalid {label} at byte {offset}: padding may appear only at the end"
                );
            }
        }
        format!(
            "invalid {label} at byte {}: encoded length cannot represent whole bytes",
            text.len()
        )
    }

    fn decode_base64_inner(text: &str, url: bool) -> Result<Vec<u8>, String> {
        let prepared = if url {
            let mut value = text.trim().replace('-', "+").replace('_', "/");
            while value.len() % 4 != 0 {
                value.push('=');
            }
            value
        } else {
            text.to_string()
        };
        match (historical_aot(&prepared), historical_comptime(&prepared)) {
            (Some(aot), Some(comptime)) if aot != comptime => Err(format!(
                "invalid {} at byte {}: historical decoders disagree",
                if url { "base64url" } else { "base64" },
                text.len()
            )),
            (Some(bytes), _) | (_, Some(bytes)) => Ok(bytes),
            (None, None) => Err(error(text, url)),
        }
    }

    pub fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
        decode_base64_inner(text, false)
    }

    pub fn decode_base64url(text: &str) -> Result<Vec<u8>, String> {
        decode_base64_inner(text, true)
    }

    pub fn decode_base32(text: &str) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let mut buffer = 0u32;
        let mut bits = 0u8;
        for (offset, byte) in text.bytes().enumerate() {
            if byte.is_ascii_whitespace() || byte == b'=' {
                continue;
            }
            let value = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a',
                b'2'..=b'7' => byte - b'2' + 26,
                _ => {
                    return Err(format!(
                        "invalid base32 at byte {offset}: byte 0x{byte:02X} is not in the base32 alphabet"
                    ));
                }
            };
            buffer = (buffer << 5) | value as u32;
            bits += 5;
            if bits >= 8 {
                out.push(((buffer >> (bits - 8)) & 0xff) as u8);
                bits -= 8;
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    pub raw: String,
    pub prefix: Option<String>,
    pub local: String,
    pub namespace_uri: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Namespace {
    pub prefix: Option<String>,
    pub namespace_uri: String,
    pub quote: char,
    pub raw: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Text {
        value: String,
        raw: String,
    },
    Entity {
        name: String,
        resolved: Option<String>,
        raw: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute {
    pub name: Name,
    pub parts: Vec<Part>,
    pub normalized: Option<String>,
    pub quote: char,
    pub raw: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    DocumentStart,
    Declaration {
        version: String,
        encoding: Option<String>,
        standalone: Option<bool>,
        raw: String,
    },
    DocumentWhitespace {
        value: String,
        raw: String,
    },
    Doctype {
        name: String,
        public_id: Option<String>,
        system_id: Option<String>,
        internal_subset: Option<String>,
        raw: String,
    },
    ElementStart {
        name: Name,
        namespaces: Vec<Namespace>,
        attributes: Vec<Attribute>,
        empty: bool,
        raw: String,
    },
    Text {
        value: String,
        raw: String,
    },
    Cdata {
        value: String,
        raw: String,
    },
    EntityRef {
        name: String,
        resolved: Option<String>,
        raw: String,
    },
    Comment {
        value: String,
        raw: String,
    },
    ProcessingInstruction {
        target: String,
        value: String,
        raw: String,
    },
    ElementEnd {
        name: Name,
        raw: String,
    },
    DocumentEnd,
}

/// DataTree-compatible value used by both runtime and comptime adapters.
/// Keeping the fold here prevents either adapter from inventing a second XML
/// tree or subtly changing event ordering.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Text(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

fn text(value: impl Into<String>) -> Value {
    Value::Text(value.into())
}

fn optional_text(value: Option<String>) -> Value {
    value.map(Value::Text).unwrap_or(Value::Null)
}

fn object(entries: Vec<(&str, Value)>) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let Value::Object(entries) = value else {
        return None;
    };
    entries
        .iter()
        .find(|(candidate, _)| candidate == key)
        .map(|(_, value)| value)
}

fn name_value(name: Name) -> Value {
    object(vec![
        ("raw", text(name.raw)),
        ("prefix", optional_text(name.prefix)),
        ("local", text(name.local)),
        ("namespace_uri", optional_text(name.namespace_uri)),
    ])
}

fn lexical(raw: String, semantic: Value) -> Value {
    object(vec![
        ("raw_text", text(raw)),
        ("raw_bytes", Value::Null),
        ("semantic", semantic),
    ])
}

fn strip_lexical(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(strip_lexical).collect()),
        Value::Object(entries) => Value::Object(
            entries
                .iter()
                .filter(|(key, _)| {
                    key != "lexical" && key != "open_lexical" && key != "close_lexical"
                })
                .map(|(key, value)| (key.clone(), strip_lexical(value)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn has_exact_xml_keys(entries: &[(String, Value)], keys: &[&str]) -> bool {
    entries.len() == keys.len()
        && keys
            .iter()
            .all(|key| entries.iter().any(|(candidate, _)| candidate == key))
}

fn xml_entry<'a>(entries: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    entries
        .iter()
        .find(|(candidate, _)| candidate == key)
        .map(|(_, value)| value)
}

/// Exact field order for every closed D-ENCXML1 object shape.
pub fn xml_schema_order(entries: &[(String, Value)]) -> Option<&'static [&'static str]> {
    let text_field = |key: &str| {
        entries.iter().find_map(|(candidate, value)| {
            (candidate == key).then_some(value).and_then(|value| match value {
                Value::Text(value) => Some(value.as_str()),
                _ => None,
            })
        })
    };
    let has = |key: &str| entries.iter().any(|(candidate, _)| candidate == key);
    let order: &[&str] = if let Some(tag) = text_field("$xml") {
        match tag {
            "document" => &["$xml", "encoding", "bom", "children"],
            "document_whitespace" if has("lexical") => &["$xml", "value", "lexical"],
            "document_whitespace" => &["$xml", "value"],
            "declaration" if has("lexical") => {
                &["$xml", "version", "encoding", "standalone", "lexical"]
            }
            "declaration" => &["$xml", "version", "encoding", "standalone"],
            "doctype" if has("lexical") => &[
                "$xml",
                "name",
                "public_id",
                "system_id",
                "internal_subset",
                "lexical",
            ],
            "doctype" => &["$xml", "name", "public_id", "system_id", "internal_subset"],
            "element" if has("open_lexical") => &[
                "$xml",
                "name",
                "namespaces",
                "attributes",
                "children",
                "empty_style",
                "open_lexical",
                "close_lexical",
            ],
            "element" => &[
                "$xml",
                "name",
                "namespaces",
                "attributes",
                "children",
                "empty_style",
            ],
            "namespace" if has("lexical") => {
                &["$xml", "prefix", "namespace_uri", "quote", "lexical"]
            }
            "namespace" => &["$xml", "prefix", "namespace_uri", "quote"],
            "attribute" if has("lexical") => &[
                "$xml",
                "name",
                "parts",
                "normalized_value",
                "quote",
                "lexical",
            ],
            "attribute" => &["$xml", "name", "parts", "normalized_value", "quote"],
            "text" | "cdata" | "comment" if has("lexical") => {
                &["$xml", "value", "lexical"]
            }
            "text" | "cdata" | "comment" => &["$xml", "value"],
            "entity_ref" if has("lexical") => {
                &["$xml", "name", "resolved_value", "lexical"]
            }
            "entity_ref" => &["$xml", "name", "resolved_value"],
            "processing_instruction" if has("lexical") => {
                &["$xml", "target", "value", "lexical"]
            }
            "processing_instruction" => &["$xml", "target", "value"],
            _ => return None,
        }
    } else if let Some(tag) = text_field("$xml_event") {
        match tag {
            "document_start" => &["$xml_event", "encoding", "bom"],
            "document_whitespace" => &["$xml_event", "value", "lexical"],
            "declaration" => {
                &["$xml_event", "version", "encoding", "standalone", "lexical"]
            }
            "doctype" => &[
                "$xml_event",
                "name",
                "public_id",
                "system_id",
                "internal_subset",
                "lexical",
            ],
            "element_start" => &[
                "$xml_event",
                "name",
                "namespaces",
                "attributes",
                "empty_style",
                "open_lexical",
            ],
            "text" | "cdata" | "comment" => &["$xml_event", "value", "lexical"],
            "entity_ref" => &["$xml_event", "name", "resolved_value", "lexical"],
            "processing_instruction" => &["$xml_event", "target", "value", "lexical"],
            "element_end" => &["$xml_event", "name", "close_lexical"],
            "document_end" => &["$xml_event"],
            _ => return None,
        }
    } else {
        [
            &["raw", "prefix", "local", "namespace_uri"][..],
            &["raw_text", "raw_bytes", "semantic"],
            &["name", "namespaces", "attributes", "empty_style"],
            &["encoding", "bom"],
            &["version", "encoding", "standalone"],
            &["name", "public_id", "system_id", "internal_subset"],
            &["name", "resolved_value"],
            &["target", "value"],
            &["value"],
            &["name"],
        ]
        .into_iter()
        .find(|keys| has_exact_xml_keys(entries, keys))?
    };
    has_exact_xml_keys(entries, order).then_some(order)
}

fn xml_schema_types_are_valid(entries: &[(String, Value)]) -> bool {
    let text = |key| matches!(xml_entry(entries, key), Some(Value::Text(_)));
    let optional_text = |key| matches!(xml_entry(entries, key), Some(Value::Text(_) | Value::Null));
    let array = |key| matches!(xml_entry(entries, key), Some(Value::Array(_)));
    let object = |key| matches!(xml_entry(entries, key), Some(Value::Object(_)));
    let object_or_null = |key| matches!(xml_entry(entries, key), Some(Value::Object(_) | Value::Null));
    let bool_or_null = |key| matches!(xml_entry(entries, key), Some(Value::Bool(_) | Value::Null));
    let has = |key| xml_entry(entries, key).is_some();
    let bytes = |key| match xml_entry(entries, key) {
        Some(Value::Array(values)) => values
            .iter()
            .all(|value| matches!(value, Value::Int(byte) if (0..=255).contains(byte))),
        _ => false,
    };
    let tag = |key| match xml_entry(entries, key) {
        Some(Value::Text(value)) => Some(value.as_str()),
        _ => None,
    };

    if let Some(tag) = tag("$xml") {
        return match tag {
            "document" => optional_text("encoding") && bytes("bom") && array("children"),
            "document_whitespace" => text("value") && (!has("lexical") || object("lexical")),
            "declaration" => text("version") && optional_text("encoding") && bool_or_null("standalone") && (!has("lexical") || object("lexical")),
            "doctype" => text("name") && optional_text("public_id") && optional_text("system_id") && optional_text("internal_subset") && (!has("lexical") || object("lexical")),
            "element" => object("name") && array("namespaces") && array("attributes") && array("children") && text("empty_style") && (!has("open_lexical") || (object("open_lexical") && object_or_null("close_lexical"))),
            "namespace" => optional_text("prefix") && text("namespace_uri") && text("quote") && (!has("lexical") || object("lexical")),
            "attribute" => object("name") && array("parts") && optional_text("normalized_value") && text("quote") && (!has("lexical") || object("lexical")),
            "text" | "cdata" | "comment" => text("value") && (!has("lexical") || object("lexical")),
            "entity_ref" => text("name") && optional_text("resolved_value") && (!has("lexical") || object("lexical")),
            "processing_instruction" => text("target") && text("value") && (!has("lexical") || object("lexical")),
            _ => false,
        };
    }
    if let Some(tag) = tag("$xml_event") {
        return match tag {
            "document_start" => optional_text("encoding") && bytes("bom"),
            "document_whitespace" => text("value") && object("lexical"),
            "declaration" => text("version") && optional_text("encoding") && bool_or_null("standalone") && object("lexical"),
            "doctype" => text("name") && optional_text("public_id") && optional_text("system_id") && optional_text("internal_subset") && object("lexical"),
            "element_start" => object("name") && array("namespaces") && array("attributes") && text("empty_style") && object("open_lexical"),
            "text" | "cdata" | "comment" => text("value") && object("lexical"),
            "entity_ref" => text("name") && optional_text("resolved_value") && object("lexical"),
            "processing_instruction" => text("target") && text("value") && object("lexical"),
            "element_end" => object("name") && object("close_lexical"),
            "document_end" => true,
            _ => false,
        };
    }
    match entries.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>().as_slice() {
        ["raw", "prefix", "local", "namespace_uri"] => text("raw") && optional_text("prefix") && text("local") && optional_text("namespace_uri"),
        ["raw_text", "raw_bytes", "semantic"] => true,
        ["name", "namespaces", "attributes", "empty_style"] => object("name") && array("namespaces") && array("attributes") && text("empty_style"),
        ["encoding", "bom"] => optional_text("encoding") && bytes("bom"),
        ["version", "encoding", "standalone"] => text("version") && optional_text("encoding") && bool_or_null("standalone"),
        ["name", "public_id", "system_id", "internal_subset"] => text("name") && optional_text("public_id") && optional_text("system_id") && optional_text("internal_subset"),
        ["name", "resolved_value"] => text("name") && optional_text("resolved_value"),
        ["target", "value"] => text("target") && text("value"),
        ["value"] => text("value"),
        ["name"] => object("name"),
        _ => false,
    }
}

fn has_canonical_xml_object(value: &Value) -> bool {
    let Value::Object(entries) = value else { return false };
    xml_schema_order(entries).is_some_and(|order| {
        entries
            .iter()
            .map(|(key, _)| key.as_str())
            .eq(order.iter().copied())
    }) && xml_schema_types_are_valid(entries)
}

fn has_canonical_xml_shape(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().all(has_canonical_xml_shape),
        Value::Object(entries) => has_canonical_xml_object(value)
            && entries
                .iter()
                .all(|(_, value)| has_canonical_xml_shape(value)),
        _ => true,
    }
}

fn lexical_evidence_matches(lexical: &Value, expected: &Value) -> bool {
    let Value::Object(entries) = lexical else { return false };
    if !entries
        .iter()
        .map(|(key, _)| key.as_str())
        .eq(["raw_text", "raw_bytes", "semantic"])
        || !has_canonical_xml_shape(expected)
    {
        return false;
    }
    let Some(semantic) = field(lexical, "semantic") else { return false };
    if !has_canonical_xml_shape(semantic) || semantic != expected {
        return false;
    }
    match (field(lexical, "raw_text"), field(lexical, "raw_bytes")) {
        (Some(Value::Text(_)), Some(Value::Null)) | (Some(Value::Null), Some(Value::Null)) => true,
        (Some(Value::Null), Some(Value::Array(bytes))) => bytes
            .iter()
            .all(|byte| matches!(byte, Value::Int(value) if (0..=255).contains(value))),
        _ => false,
    }
}

fn lexical_semantic(value: &Value, key: &str) -> Option<Value> {
    if !has_canonical_xml_object(value) {
        return None;
    }
    let whole_tag = match field(value, "$xml") {
        Some(Value::Text(tag)) => Some(tag.as_str()),
        _ => None,
    };
    let event_tag = match field(value, "$xml_event") {
        Some(Value::Text(tag)) => Some(tag.as_str()),
        _ => None,
    };
    match (whole_tag, event_tag, key) {
        (Some("element"), _, "open_lexical") | (_, Some("element_start"), "open_lexical") => {
            Some(strip_lexical(&object(vec![
                ("name", field(value, "name")?.clone()),
                ("namespaces", field(value, "namespaces")?.clone()),
                ("attributes", field(value, "attributes")?.clone()),
                ("empty_style", field(value, "empty_style")?.clone()),
            ])))
        }
        (Some("element"), _, "close_lexical") | (_, Some("element_end"), "close_lexical") => {
            Some(object(vec![("name", field(value, "name")?.clone())]))
        }
        (Some(tag), _, "lexical")
            if matches!(tag, "document_whitespace" | "declaration" | "doctype" | "namespace" | "attribute" | "text" | "cdata" | "entity_ref" | "comment" | "processing_instruction") =>
        {
            let mut semantic = strip_lexical(value);
            if matches!(
                tag,
                "document_whitespace"
                    | "declaration"
                    | "doctype"
                    | "text"
                    | "cdata"
                    | "entity_ref"
                    | "comment"
                    | "processing_instruction"
            ) && matches!(
                field(field(value, key)?, "raw_bytes"),
                Some(Value::Array(_))
            ) {
                let Value::Object(entries) = &mut semantic else {
                    return None;
                };
                entries.retain(|(name, _)| name != "$xml");
            }
            Some(semantic)
        }
        (_, Some(tag), "lexical")
            if matches!(tag, "document_whitespace" | "declaration" | "doctype" | "text" | "cdata" | "entity_ref" | "comment" | "processing_instruction") =>
        {
            let Value::Object(entries) = value else { return None };
            Some(Value::Object(entries.iter().filter(|(name, _)| name != "$xml_event" && name != "lexical" && name != "open_lexical" && name != "close_lexical").map(|(name, value)| (name.clone(), strip_lexical(value))).collect()))
        }
        _ => None,
    }
}

/// Remove raw forms that would become falsely trusted after ordered Objects
/// cross comptime's sorted Map representation.
pub fn invalidate_untrusted_lexical_evidence(value: &mut Value) {
    for key in ["lexical", "open_lexical", "close_lexical"] {
        let Some(lexical) = field(value, key) else { continue };
        let trusted = lexical_semantic(value, key)
            .as_ref()
            .is_some_and(|expected| lexical_evidence_matches(lexical, expected));
        if !trusted {
            if let Value::Object(entries) = value {
                if let Some((_, Value::Object(lexical))) =
                    entries.iter_mut().find(|(candidate, _)| candidate == key)
                {
                    for (field, value) in lexical {
                        if matches!(field.as_str(), "raw_text" | "raw_bytes") {
                            *value = Value::Null;
                        }
                    }
                }
            }
        }
    }
    match value {
        Value::Array(values) => {
            for value in values {
                invalidate_untrusted_lexical_evidence(value);
            }
        }
        Value::Object(entries) => {
            for (_, value) in entries {
                invalidate_untrusted_lexical_evidence(value);
            }
        }
        _ => {}
    }
}

fn leaf(tag: &str, fields: Vec<(&str, Value)>, raw: String) -> Value {
    let mut semantic_fields = vec![("$xml", text(tag))];
    semantic_fields.extend(fields.clone());
    let semantic = strip_lexical(&object(semantic_fields));
    let mut node_fields = vec![("$xml", text(tag))];
    node_fields.extend(fields);
    node_fields.push(("lexical", lexical(raw, semantic)));
    object(node_fields)
}

fn part_value(part: Part) -> Value {
    match part {
        Part::Text { value, raw } => leaf("text", vec![("value", text(value))], raw),
        Part::Entity {
            name,
            resolved,
            raw,
        } => leaf(
            "entity_ref",
            vec![
                ("name", text(name)),
                ("resolved_value", optional_text(resolved)),
            ],
            raw,
        ),
    }
}

fn namespace_value(namespace: Namespace) -> Value {
    leaf(
        "namespace",
        vec![
            ("prefix", optional_text(namespace.prefix)),
            ("namespace_uri", text(namespace.namespace_uri)),
            (
                "quote",
                text(if namespace.quote == '\'' {
                    "single"
                } else {
                    "double"
                }),
            ),
        ],
        namespace.raw,
    )
}

fn attribute_value(attribute: Attribute) -> Value {
    leaf(
        "attribute",
        vec![
            ("name", name_value(attribute.name)),
            (
                "parts",
                Value::Array(attribute.parts.into_iter().map(part_value).collect()),
            ),
            ("normalized_value", optional_text(attribute.normalized)),
            (
                "quote",
                text(if attribute.quote == '\'' {
                    "single"
                } else {
                    "double"
                }),
            ),
        ],
        attribute.raw,
    )
}

fn wire_bytes(text: &str, encoding: WireEncoding) -> Vec<u8> {
    match encoding {
        WireEncoding::UTF8 => text.as_bytes().to_vec(),
        WireEncoding::Utf16Le => text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        WireEncoding::Utf16Be => text.encode_utf16().flat_map(u16::to_be_bytes).collect(),
    }
}

fn byte_lexical(raw: Vec<u8>, semantic: Value) -> Value {
    object(vec![("raw_text", Value::Null), ("raw_bytes", Value::Array(raw.into_iter().map(|b| Value::Int(i64::from(b))).collect())), ("semantic", semantic)])
}
fn lexical_value_to_wire(mut value:Value,encoding:WireEncoding)->Value{if let Value::Object(entries)=&mut value{if let Some((_,Value::Object(lex)))=entries.iter_mut().find(|(k,_)|k=="lexical"){if let Some(index)=lex.iter().position(|(k,_)|k=="raw_text"){if let Value::Text(raw)=&lex[index].1{let bytes=wire_bytes(raw,encoding);lex[index].1=Value::Null;if let Some((_,slot))=lex.iter_mut().find(|(k,_)|k=="raw_bytes"){*slot=Value::Array(bytes.into_iter().map(|b|Value::Int(i64::from(b))).collect())}}}}}value}

/// Exact public `$xml_event` DataTree projection for a byte-backed stream item.
pub fn stream_event_value(item: StreamEvent) -> Value {
    let encoding = item.encoding;
    let lex = |raw: Vec<u8>, semantic: Value| byte_lexical(raw, semantic);
    match item.event {
        Event::DocumentStart => object(vec![("$xml_event", text("document_start")), ("encoding", text(match encoding { WireEncoding::UTF8=>"UTF-8", WireEncoding::Utf16Le=>"UTF-16LE", WireEncoding::Utf16Be=>"UTF-16BE" })), ("bom", Value::Array(item.bom.into_iter().map(|b|Value::Int(i64::from(b))).collect()))]),
        Event::DocumentEnd => object(vec![("$xml_event",text("document_end"))]),
        Event::Declaration{version,encoding:declared,standalone,raw:_} => { let fields=vec![("version",text(version)),("encoding",optional_text(declared)),("standalone",standalone.map(Value::Bool).unwrap_or(Value::Null))]; let mut out=vec![("$xml_event",text("declaration"))];out.extend(fields.clone());out.push(("lexical",lex(item.raw_bytes,strip_lexical(&object(fields)))));object(out) }
        Event::DocumentWhitespace{value,raw:_} => leaf_event("document_whitespace",vec![("value",text(value))],item.raw_bytes),
        Event::Text{value,raw:_} => leaf_event("text",vec![("value",text(value))],item.raw_bytes),
        Event::Cdata{value,raw:_} => leaf_event("cdata",vec![("value",text(value))],item.raw_bytes),
        Event::Comment{value,raw:_} => leaf_event("comment",vec![("value",text(value))],item.raw_bytes),
        Event::EntityRef{name,resolved,raw:_} => leaf_event("entity_ref",vec![("name",text(name)),("resolved_value",optional_text(resolved))],item.raw_bytes),
        Event::ProcessingInstruction{target,value,raw:_} => leaf_event("processing_instruction",vec![("target",text(target)),("value",text(value))],item.raw_bytes),
        Event::Doctype{name,public_id,system_id,internal_subset,raw:_} => leaf_event("doctype",vec![("name",text(name)),("public_id",optional_text(public_id)),("system_id",optional_text(system_id)),("internal_subset",optional_text(internal_subset))],item.raw_bytes),
        Event::ElementEnd{name,raw:_} => { let semantic=object(vec![("name",name_value(name.clone()))]);object(vec![("$xml_event",text("element_end")),("name",name_value(name)),("close_lexical",lex(item.raw_bytes,semantic))]) }
        Event::ElementStart{name,namespaces,attributes,empty,raw:_} => { let ns=Value::Array(namespaces.into_iter().map(|n|lexical_value_to_wire(namespace_value(n),encoding)).collect());let attrs=Value::Array(attributes.into_iter().map(|a|lexical_value_to_wire(attribute_value(a),encoding)).collect());let style=text(if empty{"empty"}else{"explicit"});let semantic=strip_lexical(&object(vec![("name",name_value(name.clone())),("namespaces",ns.clone()),("attributes",attrs.clone()),("empty_style",style.clone())]));object(vec![("$xml_event",text("element_start")),("name",name_value(name)),("namespaces",ns),("attributes",attrs),("empty_style",style),("open_lexical",lex(item.raw_bytes,semantic))]) }
    }
}

fn leaf_event(tag:&str,fields:Vec<(&str,Value)>,raw:Vec<u8>)->Value{let semantic=strip_lexical(&object(fields.clone()));let mut out=vec![("$xml_event",text(tag))];out.extend(fields);out.push(("lexical",byte_lexical(raw,semantic)));object(out)}

struct ElementFrame {
    name: Name,
    namespaces: Vec<Value>,
    attributes: Vec<Value>,
    children: Vec<Value>,
    open_raw: String,
}

struct ByteElementFrame {
    name: Value,
    namespaces: Value,
    attributes: Value,
    children: Vec<Value>,
    empty_style: Value,
    open_lexical: Value,
}

fn finish_element(frame: ElementFrame, close_raw: Option<String>) -> Value {
    let empty = close_raw.is_none();
    let name = name_value(frame.name.clone());
    let empty_style = text(if empty { "empty" } else { "explicit" });
    let open_semantic = strip_lexical(&object(vec![
        ("name", name.clone()),
        ("namespaces", Value::Array(frame.namespaces.clone())),
        ("attributes", Value::Array(frame.attributes.clone())),
        ("empty_style", empty_style.clone()),
    ]));
    let close_lexical =
        close_raw.map(|raw| lexical(raw, object(vec![("name", name_value(frame.name.clone()))])));
    object(vec![
        ("$xml", text("element")),
        ("name", name),
        ("namespaces", Value::Array(frame.namespaces)),
        ("attributes", Value::Array(frame.attributes)),
        ("children", Value::Array(frame.children)),
        ("empty_style", empty_style),
        ("open_lexical", lexical(frame.open_raw, open_semantic)),
        ("close_lexical", close_lexical.unwrap_or(Value::Null)),
    ])
}

fn finish_byte_element(frame: ByteElementFrame, close_lexical: Value) -> Value {
    object(vec![
        ("$xml", text("element")),
        ("name", frame.name),
        ("namespaces", frame.namespaces),
        ("attributes", frame.attributes),
        ("children", Value::Array(frame.children)),
        ("empty_style", frame.empty_style),
        ("open_lexical", frame.open_lexical),
        ("close_lexical", close_lexical),
    ])
}

/// Parse and fold the pull stream into the ratified ordered tagged XML tree.
pub fn parse_document(source: &str) -> Result<Value, Error> {
    parse_document_with(source, &ParseOptions::safe())
}

/// Parse a complete XML value with the ratified whole-value policy and limits.
pub fn parse_document_with(source: &str, options: &ParseOptions) -> Result<Value, Error> {
    options.limits.validate()?;
    let mut scanner = Scanner::with_options(source, options.clone())?;
    let mut document_children = Vec::new();
    let mut stack: Vec<ElementFrame> = Vec::new();
    while let Some(event) = scanner.next()? {
        let node = match event {
            Event::DocumentStart | Event::DocumentEnd => continue,
            Event::Declaration {
                version,
                encoding,
                standalone,
                raw,
            } => Some(leaf(
                "declaration",
                vec![
                    ("version", text(version)),
                    ("encoding", optional_text(encoding)),
                    (
                        "standalone",
                        standalone.map(Value::Bool).unwrap_or(Value::Null),
                    ),
                ],
                raw,
            )),
            Event::DocumentWhitespace { value, raw } => Some(leaf(
                "document_whitespace",
                vec![("value", text(value))],
                raw,
            )),
            Event::Doctype {
                name,
                public_id,
                system_id,
                internal_subset,
                raw,
            } => Some(leaf(
                "doctype",
                vec![
                    ("name", text(name)),
                    ("public_id", optional_text(public_id)),
                    ("system_id", optional_text(system_id)),
                    ("internal_subset", optional_text(internal_subset)),
                ],
                raw,
            )),
            Event::ElementStart {
                name,
                namespaces,
                attributes,
                empty,
                raw,
            } => {
                let frame = ElementFrame {
                    name,
                    namespaces: namespaces.into_iter().map(namespace_value).collect(),
                    attributes: attributes.into_iter().map(attribute_value).collect(),
                    children: Vec::new(),
                    open_raw: raw,
                };
                if empty {
                    Some(finish_element(frame, None))
                } else {
                    stack.push(frame);
                    None
                }
            }
            Event::Text { value, raw } => Some(leaf("text", vec![("value", text(value))], raw)),
            Event::Cdata { value, raw } => Some(leaf("cdata", vec![("value", text(value))], raw)),
            Event::EntityRef {
                name,
                resolved,
                raw,
            } => Some(leaf(
                "entity_ref",
                vec![
                    ("name", text(name)),
                    ("resolved_value", optional_text(resolved)),
                ],
                raw,
            )),
            Event::Comment { value, raw } => {
                Some(leaf("comment", vec![("value", text(value))], raw))
            }
            Event::ProcessingInstruction { target, value, raw } => Some(leaf(
                "processing_instruction",
                vec![("target", text(target)), ("value", text(value))],
                raw,
            )),
            Event::ElementEnd { name: _, raw } => {
                let frame = stack
                    .pop()
                    .ok_or_else(|| Error::at(scanner.offset, "element stack underflow"))?;
                Some(finish_element(frame, Some(raw)))
            }
        };
        if let Some(node) = node {
            if let Some(parent) = stack.last_mut() {
                parent.children.push(node);
            } else {
                document_children.push(node);
            }
        }
    }
    Ok(object(vec![
        ("$xml", text("document")),
        ("encoding", Value::Null),
        ("bom", Value::Array(Vec::new())),
        ("children", Value::Array(document_children)),
    ]))
}

/// Parse bytes through the same pull scanner used by XMLReader, then fold its
/// exact event algebra into the ratified whole-value tree.
pub fn parse_document_bytes(bytes: &[u8]) -> Result<Value, Error> {
    parse_document_bytes_with(bytes, &ParseOptions::safe())
}

pub fn parse_document_bytes_with(bytes: &[u8], options: &ParseOptions) -> Result<Value, Error> {
    options.limits.validate()?;
    let mut scanner = StreamScanner::new(bytes.len().max(4), options.clone())?;
    scanner.push(bytes)?;
    scanner.finish_input()?;
    let mut events = Vec::new();
    while let Some(item) = scanner.next()? {
        events.push(stream_event_value(item));
    }
    fold_events(&events)
}

/// Fold a complete `$xml_event` stream into the ratified whole-value `$xml` document.
/// Inverse of [`unfold_events`]. Incomplete closure, mismatched ends, or missing
/// document_start/document_end reject with Shape (never a partial tree).
pub fn fold_events(events: &[Value]) -> Result<Value, Error> {
    let mut document_start = None;
    let mut document_children = Vec::new();
    let mut stack: Vec<ByteElementFrame> = Vec::new();
    let mut ended = false;
    for event in events {
        if ended {
            return Err(Error::shape("XML event after document_end"));
        }
        let tag = required_text(event, "$xml_event").map_err(Error::shape)?;
        let node = match tag {
            "document_start" => {
                exact_keys(event, &["$xml_event", "encoding", "bom"])?;
                if document_start.is_some() {
                    return Err(Error::shape("duplicate XML document_start"));
                }
                document_start = Some((
                    field(event, "encoding")
                        .cloned()
                        .ok_or_else(|| Error::shape("document_start lacks encoding"))?,
                    field(event, "bom")
                        .cloned()
                        .ok_or_else(|| Error::shape("document_start lacks bom"))?,
                ));
                None
            }
            "document_end" => {
                exact_keys(event, &["$xml_event"])?;
                if document_start.is_none() || !stack.is_empty() {
                    return Err(Error::shape("document_end requires one closed root element"));
                }
                ended = true;
                None
            }
            "element_start" => {
                if document_start.is_none() {
                    return Err(Error::shape("XML event before document_start"));
                }
                exact_keys(
                    event,
                    &[
                        "$xml_event",
                        "name",
                        "namespaces",
                        "attributes",
                        "empty_style",
                        "open_lexical",
                    ],
                )?;
                let empty = match required_text(event, "empty_style").map_err(Error::shape)? {
                    "empty" => true,
                    "explicit" => false,
                    _ => return Err(Error::shape("empty_style must be empty or explicit")),
                };
                let frame = ByteElementFrame {
                    name: field(event, "name")
                        .cloned()
                        .ok_or_else(|| Error::shape("element_start lacks name"))?,
                    namespaces: field(event, "namespaces")
                        .cloned()
                        .ok_or_else(|| Error::shape("element_start lacks namespaces"))?,
                    attributes: field(event, "attributes")
                        .cloned()
                        .ok_or_else(|| Error::shape("element_start lacks attributes"))?,
                    children: Vec::new(),
                    empty_style: field(event, "empty_style")
                        .cloned()
                        .ok_or_else(|| Error::shape("element_start lacks empty_style"))?,
                    open_lexical: field(event, "open_lexical")
                        .cloned()
                        .ok_or_else(|| Error::shape("element_start lacks open_lexical"))?,
                };
                if empty {
                    Some(finish_byte_element(frame, Value::Null))
                } else {
                    stack.push(frame);
                    None
                }
            }
            "element_end" => {
                exact_keys(event, &["$xml_event", "name", "close_lexical"])?;
                let end_name = parse_name(
                    field(event, "name").ok_or_else(|| Error::shape("element_end lacks name"))?,
                )?;
                let frame = stack
                    .pop()
                    .ok_or_else(|| Error::shape("element stack underflow"))?;
                let open_name = parse_name(&frame.name)?;
                if open_name.local != end_name.local
                    || open_name.namespace_uri != end_name.namespace_uri
                {
                    return Err(Error::at_kind(
                        0,
                        Reason::MismatchedTag,
                        "element_end does not match the open expanded name",
                    ));
                }
                Some(finish_byte_element(
                    frame,
                    field(event, "close_lexical")
                        .cloned()
                        .ok_or_else(|| Error::shape("element_end lacks close_lexical"))?,
                ))
            }
            _ => {
                if document_start.is_none() {
                    return Err(Error::shape("XML event before document_start"));
                }
                let Value::Object(mut entries) = event.clone() else {
                    return Err(Error::shape("XML event must be an Object"));
                };
                let Some((key, _)) = entries.iter_mut().find(|(key, _)| key == "$xml_event") else {
                    return Err(Error::shape("XML event lacks $xml_event"));
                };
                *key = "$xml".to_string();
                Some(Value::Object(entries))
            }
        };
        if let Some(node) = node {
            if let Some(parent) = stack.last_mut() {
                parent.children.push(node);
            } else {
                document_children.push(node);
            }
        }
    }
    if !ended || !stack.is_empty() {
        return Err(Error::shape("XML event stream did not form a complete document"));
    }
    let (encoding, bom) =
        document_start.ok_or_else(|| Error::shape("XML event stream lacks document_start"))?;
    let has_root = document_children
        .iter()
        .filter(|node| required_text(node, "$xml").ok() == Some("element"))
        .count()
        == 1;
    if !has_root {
        return Err(Error::shape("XML document must contain exactly one root element"));
    }
    Ok(object(vec![
        ("$xml", text("document")),
        ("encoding", encoding),
        ("bom", bom),
        ("children", Value::Array(document_children)),
    ]))
}

/// Unfold a whole-value `$xml` document into the exact `$xml_event` algebra.
/// Inverse of [`fold_events`].
pub fn unfold_events(value: &Value) -> Result<Vec<Value>, Error> {
    fn event_from_node(value: &Value) -> Result<Value, Error> {
        let Value::Object(mut entries) = value.clone() else {
            return Err(Error::shape("XML node must be an Object"));
        };
        let Some((key, _)) = entries.iter_mut().find(|(key, _)| key == "$xml") else {
            return Err(Error::shape("XML node lacks $xml"));
        };
        *key = "$xml_event".to_string();
        Ok(Value::Object(entries))
    }

    fn push_node(value: &Value, output: &mut Vec<Value>) -> Result<(), Error> {
        let tag = required_text(value, "$xml").map_err(Error::shape)?;
        if tag == "element" {
            exact_keys(
                value,
                &[
                    "$xml",
                    "name",
                    "namespaces",
                    "attributes",
                    "children",
                    "empty_style",
                    "open_lexical",
                    "close_lexical",
                ],
            )?;
            let children = match field(value, "children") {
                Some(Value::Array(children)) => children,
                _ => return Err(Error::shape("XML element children must be an Array")),
            };
            let style = required_text(value, "empty_style").map_err(Error::shape)?;
            match (style, field(value, "close_lexical")) {
                ("empty", Some(Value::Null)) if children.is_empty() => {}
                ("explicit", Some(Value::Object(_))) => {}
                ("empty", _) => {
                    return Err(Error::shape(
                        "empty element requires no children and null close_lexical",
                    ))
                }
                ("explicit", _) => {
                    return Err(Error::shape("explicit element requires close_lexical"))
                }
                _ => return Err(Error::shape("empty_style must be empty or explicit")),
            }
            output.push(object(vec![
                ("$xml_event", text("element_start")),
                (
                    "name",
                    field(value, "name")
                        .cloned()
                        .ok_or_else(|| Error::shape("element lacks name"))?,
                ),
                (
                    "namespaces",
                    field(value, "namespaces")
                        .cloned()
                        .ok_or_else(|| Error::shape("element lacks namespaces"))?,
                ),
                (
                    "attributes",
                    field(value, "attributes")
                        .cloned()
                        .ok_or_else(|| Error::shape("element lacks attributes"))?,
                ),
                ("empty_style", text(style)),
                (
                    "open_lexical",
                    field(value, "open_lexical")
                        .cloned()
                        .ok_or_else(|| Error::shape("element lacks open_lexical"))?,
                ),
            ]));
            for child in children {
                push_node(child, output)?;
            }
            if style == "explicit" {
                output.push(object(vec![
                    ("$xml_event", text("element_end")),
                    (
                        "name",
                        field(value, "name")
                            .cloned()
                            .ok_or_else(|| Error::shape("element lacks name"))?,
                    ),
                    (
                        "close_lexical",
                        field(value, "close_lexical")
                            .cloned()
                            .ok_or_else(|| Error::shape("element lacks close_lexical"))?,
                    ),
                ]));
            }
            return Ok(());
        }
        if !matches!(
            tag,
            "declaration"
                | "document_whitespace"
                | "doctype"
                | "text"
                | "cdata"
                | "entity_ref"
                | "comment"
                | "processing_instruction"
        ) {
            return Err(Error::shape(format!(
                "{tag} is not legal as an XML document or element child"
            )));
        }
        output.push(event_from_node(value)?);
        Ok(())
    }

    exact_keys(value, &["$xml", "encoding", "bom", "children"])?;
    if required_text(value, "$xml").map_err(Error::shape)? != "document" {
        return Err(Error::shape("XML whole value must be a document"));
    }
    let children = match field(value, "children") {
        Some(Value::Array(children)) => children,
        _ => return Err(Error::shape("XML document children must be an Array")),
    };
    let mut output = vec![object(vec![
        ("$xml_event", text("document_start")),
        (
            "encoding",
            field(value, "encoding")
                .cloned()
                .ok_or_else(|| Error::shape("document lacks encoding"))?,
        ),
        (
            "bom",
            field(value, "bom")
                .cloned()
                .ok_or_else(|| Error::shape("document lacks bom"))?,
        ),
    ])];
    for child in children {
        push_node(child, &mut output)?;
    }
    output.push(object(vec![("$xml_event", text("document_end"))]));
    Ok(output)
}

fn lexical_raw<'a>(value: &'a Value, key: &str, semantic: &Value) -> Option<&'a str> {
    let lexical_value = field(value, key)?;
    if !has_canonical_xml_object(value) || !lexical_evidence_matches(lexical_value, semantic) {
        return None;
    }
    match field(lexical_value, "raw_text")? {
        Value::Text(raw) => Some(raw),
        _ => None,
    }
}

fn required_text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    match field(value, key) {
        Some(Value::Text(text)) => Ok(text),
        _ => Err(format!("XML {key} must be text")),
    }
}

fn optional_text_field<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match field(value, key) {
        Some(Value::Text(text)) => Ok(Some(text)),
        Some(Value::Null) => Ok(None),
        _ => Err(format!("XML {key} must be text or null")),
    }
}

fn escape_text(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;")
}

fn escape_attribute(value: &str, quote: char) -> String {
    let escaped = escape_text(value);
    if quote == '\'' {
        escaped.replace('\'', "&apos;")
    } else {
        escaped.replace('"', "&quot;")
    }
}

fn deterministic_leaf(value: &Value, tag: &str) -> Result<String, String> {
    match tag {
        "document_whitespace" => Ok(required_text(value, "value")?.to_string()),
        "declaration" => {
            let mut output = format!("<?xml version=\"{}\"", required_text(value, "version")?);
            if let Some(encoding) = optional_text_field(value, "encoding")? {
                output.push_str(&format!(" encoding=\"{encoding}\""));
            }
            match field(value, "standalone") {
                Some(Value::Bool(true)) => output.push_str(" standalone=\"yes\""),
                Some(Value::Bool(false)) => output.push_str(" standalone=\"no\""),
                Some(Value::Null) => {}
                _ => return Err("XML standalone must be bool or null".to_string()),
            }
            output.push_str("?>");
            Ok(output)
        }
        "doctype" => {
            let mut output = format!("<!DOCTYPE {}", required_text(value, "name")?);
            match (
                optional_text_field(value, "public_id")?,
                optional_text_field(value, "system_id")?,
            ) {
                (Some(public), Some(system)) => {
                    output.push_str(&format!(" PUBLIC \"{public}\" \"{system}\""));
                }
                (None, Some(system)) => output.push_str(&format!(" SYSTEM \"{system}\"")),
                (None, None) => {}
                (Some(_), None) => {
                    return Err("XML PUBLIC identifier requires SYSTEM identifier".to_string());
                }
            }
            if let Some(subset) = optional_text_field(value, "internal_subset")? {
                output.push_str(&format!(" [{subset}]"));
            }
            output.push('>');
            Ok(output)
        }
        "text" => Ok(escape_text(required_text(value, "value")?)),
        "cdata" => {
            let body = required_text(value, "value")?;
            if body.contains("]]>") {
                return Err("CDATA value contains ]]>".to_string());
            }
            Ok(format!("<![CDATA[{body}]]>"))
        }
        "entity_ref" => Ok(format!("&{};", required_text(value, "name")?)),
        "comment" => {
            let body = required_text(value, "value")?;
            if body.contains("--") || body.ends_with('-') {
                return Err("XML comment contains forbidden --".to_string());
            }
            Ok(format!("<!--{body}-->"))
        }
        "processing_instruction" => {
            let body = required_text(value, "value")?;
            let separator = if body.is_empty() { "" } else { " " };
            Ok(format!(
                "<?{}{separator}{body}?>",
                required_text(value, "target")?
            ))
        }
        _ => Err(format!("unsupported XML node tag {tag}")),
    }
}

fn render_element_open(value: &Value, output: &mut String) -> Result<(), String> {
    let name = field(value, "name").ok_or("XML element lacks name")?;
    output.push('<');
    output.push_str(required_text(name, "raw")?);
    let Some(Value::Array(namespaces)) = field(value, "namespaces") else {
        return Err("XML namespaces must be an array".to_string());
    };
    for namespace in namespaces {
        output.push_str(" xmlns");
        if let Some(prefix) = optional_text_field(namespace, "prefix")? {
            output.push(':');
            output.push_str(prefix);
        }
        let quote = if required_text(namespace, "quote")? == "single" {
            '\''
        } else {
            '"'
        };
        output.push('=');
        output.push(quote);
        output.push_str(&escape_attribute(
            required_text(namespace, "namespace_uri")?,
            quote,
        ));
        output.push(quote);
    }
    let Some(Value::Array(attributes)) = field(value, "attributes") else {
        return Err("XML attributes must be an array".to_string());
    };
    for attribute in attributes {
        let attribute_name = field(attribute, "name").ok_or("XML attribute lacks name")?;
        let quote = if required_text(attribute, "quote")? == "single" {
            '\''
        } else {
            '"'
        };
        output.push(' ');
        output.push_str(required_text(attribute_name, "raw")?);
        output.push('=');
        output.push(quote);
        if let Some(normalized) = optional_text_field(attribute, "normalized_value")? {
            output.push_str(&escape_attribute(normalized, quote));
        } else {
            let Some(Value::Array(parts)) = field(attribute, "parts") else {
                return Err("XML attribute parts must be an array".to_string());
            };
            for part in parts {
                let tag = required_text(part, "$xml")?;
                output.push_str(&deterministic_leaf(part, tag)?);
            }
        }
        output.push(quote);
    }
    if required_text(value, "empty_style")? == "empty" {
        output.push_str("/>");
    } else {
        output.push('>');
    }
    Ok(())
}

/// Render the ratified tree. Valid unchanged token evidence is reused; edited
/// or constructed tokens render deterministically without trusting stale raw.
pub fn render_document(value: &Value) -> Result<String, String> {
    fn render_node(value: &Value, output: &mut String) -> Result<(), String> {
        let tag = required_text(value, "$xml")?;
        if tag == "document" {
            let Some(Value::Array(children)) = field(value, "children") else {
                return Err("XML document children must be an array".to_string());
            };
            for child in children {
                render_node(child, output)?;
            }
            return Ok(());
        }
        if tag == "element" {
            let open_semantic = strip_lexical(&object(vec![
                (
                    "name",
                    field(value, "name")
                        .cloned()
                        .ok_or("XML element lacks name")?,
                ),
                (
                    "namespaces",
                    field(value, "namespaces")
                        .cloned()
                        .ok_or("XML element lacks namespaces")?,
                ),
                (
                    "attributes",
                    field(value, "attributes")
                        .cloned()
                        .ok_or("XML element lacks attributes")?,
                ),
                (
                    "empty_style",
                    field(value, "empty_style")
                        .cloned()
                        .ok_or("XML element lacks empty_style")?,
                ),
            ]));
            if let Some(raw) = lexical_raw(value, "open_lexical", &open_semantic) {
                output.push_str(raw);
            } else {
                render_element_open(value, output)?;
            }
            let Some(Value::Array(children)) = field(value, "children") else {
                return Err("XML element children must be an array".to_string());
            };
            for child in children {
                render_node(child, output)?;
            }
            let close_semantic = object(vec![(
                "name",
                field(value, "name")
                    .cloned()
                    .ok_or("XML element lacks name")?,
            )]);
            if let Some(raw) = lexical_raw(value, "close_lexical", &close_semantic) {
                output.push_str(raw);
            } else if required_text(value, "empty_style")? == "explicit" {
                let name = field(value, "name").ok_or("XML element lacks name")?;
                output.push_str("</");
                output.push_str(required_text(name, "raw")?);
                output.push('>');
            }
            return Ok(());
        }
        let semantic = strip_lexical(value);
        if let Some(raw) = lexical_raw(value, "lexical", &semantic) {
            output.push_str(raw);
        } else {
            output.push_str(&deterministic_leaf(value, tag)?);
        }
        Ok(())
    }

    let mut output = String::new();
    render_node(value, &mut output)?;
    Ok(output)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderEncoding { UTF8, UTF8Bom, Utf16Le, Utf16Be }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexicalPolicy { PreserveValid, Deterministic }

#[derive(Clone)]
pub struct StreamWriter {
    encoding: RenderEncoding,
    lexical: LexicalPolicy,
    started: bool,
    declaration: bool,
    doctype: bool,
    root_seen: bool,
    root_closed: bool,
    ended: bool,
    source_matches: bool,
    stack: Vec<Name>,
    namespaces: Vec<BTreeMap<Option<String>, String>>,
}

impl StreamWriter {
    pub fn new(encoding: RenderEncoding, lexical: LexicalPolicy) -> Self {
        Self { encoding, lexical, started: false, declaration: false, doctype: false, root_seen: false, root_closed: false, ended: false, source_matches: false, stack: Vec::new(), namespaces: Vec::new() }
    }

    pub fn is_finished(&self) -> bool { self.ended }

    pub fn write(&mut self, event: &Value) -> Result<Vec<u8>, Error> {
        if self.ended { return Err(Error::state("XML writer received an event after document_end")); }
        let mut next = self.clone();
        let bytes = next.write_inner(event)?;
        *self = next;
        Ok(bytes)
    }

    fn write_inner(&mut self, event: &Value) -> Result<Vec<u8>, Error> {
        let tag = required_text(event, "$xml_event").map_err(Error::shape)?;
        let deterministic = match tag {
            "document_start" => {
                exact_keys(event, &["$xml_event", "encoding", "bom"])?;
                if self.started { return Err(Error::state("duplicate XML document_start")); }
                let source = optional_text_field(event, "encoding").map_err(Error::shape)?;
                let bom = byte_array(field(event, "bom").ok_or_else(|| Error::shape("document_start lacks bom"))?)?;
                self.source_matches = source_encoding_matches(source, &bom, self.encoding);
                self.started = true;
                return Ok(target_bom(self.encoding));
            }
            _ if !self.started => return Err(Error::state("XML writer expects document_start first")),
            "declaration" => {
                exact_keys(event, &["$xml_event", "version", "encoding", "standalone", "lexical"])?;
                if self.declaration || self.doctype || self.root_seen { return Err(Error::state("XML declaration is out of order")); }
                let version = required_text(event, "version").map_err(Error::shape)?;
                if version != "1.0" { return Err(Error::at_kind(0, Reason::Unsupported, "only XML version 1.0 is supported")); }
                let declared = optional_text_field(event, "encoding").map_err(Error::shape)?;
                if let Some(name) = declared { validate_declared_encoding(name, self.encoding)?; }
                let standalone = match field(event, "standalone") { Some(Value::Null) => None, Some(Value::Bool(value)) => Some(*value), _ => return Err(Error::shape("declaration standalone must be Bool or Null")) };
                self.declaration = true;
                let mut text = format!("<?xml version=\"{version}\"");
                if declared.is_some() { text.push_str(&format!(" encoding=\"{}\"", encoding_declaration(self.encoding))); }
                if let Some(value) = standalone { text.push_str(if value { " standalone=\"yes\"" } else { " standalone=\"no\"" }); }
                text.push_str("?>");
                text
            }
            "document_whitespace" => {
                exact_keys(event, &["$xml_event", "value", "lexical"])?;
                if self.root_seen && !self.root_closed { return Err(Error::state("document_whitespace is not legal inside an element")); }
                let value = required_text(event, "value").map_err(Error::shape)?;
                if value.is_empty() || !value.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')) { return Err(Error::shape("document_whitespace contains a non-XML-S character")); }
                value.to_string()
            }
            "doctype" => {
                exact_keys(event, &["$xml_event", "name", "public_id", "system_id", "internal_subset", "lexical"])?;
                if self.doctype || self.root_seen { return Err(Error::state("XML DOCTYPE is out of order")); }
                self.doctype = true;
                deterministic_event_node(event, "doctype")?
            }
            "element_start" => {
                exact_keys(event, &["$xml_event", "name", "namespaces", "attributes", "empty_style", "open_lexical"])?;
                if self.root_closed { return Err(Error::state("XML document already has its root element")); }
                let name = parse_name(field(event, "name").ok_or_else(|| Error::shape("element_start lacks name"))?)?;
                let empty = match required_text(event, "empty_style").map_err(Error::shape)? { "empty" => true, "explicit" => false, _ => return Err(Error::shape("empty_style must be empty or explicit")) };
                let parent = self.namespaces.last().cloned().unwrap_or_else(base_namespaces);
                let context = validate_start(event, &name, parent)?;
                if self.stack.is_empty() { self.root_seen = true; }
                let node = event_element_node(event)?;
                let mut text = String::new();
                render_element_open(&node, &mut text).map_err(Error::shape)?;
                if empty {
                    if self.stack.is_empty() { self.root_closed = true; }
                } else {
                    self.stack.push(name);
                    self.namespaces.push(context);
                }
                text
            }
            "element_end" => {
                exact_keys(event, &["$xml_event", "name", "close_lexical"])?;
                let name = parse_name(field(event, "name").ok_or_else(|| Error::shape("element_end lacks name"))?)?;
                let Some(open) = self.stack.last() else { return Err(Error::state("element_end has no open element")); };
                if open.local != name.local || open.namespace_uri != name.namespace_uri { return Err(Error::at_kind(0, Reason::MismatchedTag, "element_end does not match the open expanded name")); }
                self.stack.pop(); self.namespaces.pop();
                if self.stack.is_empty() { self.root_closed = true; }
                format!("</{}>", name.raw)
            }
            "document_end" => {
                exact_keys(event, &["$xml_event"])?;
                if !self.root_seen || !self.root_closed || !self.stack.is_empty() { return Err(Error::state("document_end requires one closed root element")); }
                self.ended = true;
                String::new()
            }
            "text" | "cdata" | "entity_ref" => {
                if self.stack.is_empty() {
                    return Err(Error::state("XML child event requires an open element"));
                }
                exact_leaf_keys(event, tag)?;
                deterministic_event_node(event, tag)?
            }
            "comment" | "processing_instruction" => {
                // Document-level Misc (prolog/epilog) and element children are both legal.
                exact_leaf_keys(event, tag)?;
                deterministic_event_node(event, tag)?
            }
            _ => return Err(Error::shape(format!("unknown XML event `{tag}`"))),
        };
        if self.lexical == LexicalPolicy::PreserveValid && self.source_matches {
            if let Some(raw) = valid_event_raw_bytes(event, tag) { return Ok(raw); }
        }
        Ok(encode_text(&deterministic, self.encoding))
    }
}

/// Render a whole tree by unfolding it into the same validated events consumed
/// by XMLWriter. This keeps encoding selection and token-local byte reuse in one
/// writer engine.
pub fn render_document_bytes(
    value: &Value,
    encoding: RenderEncoding,
    lexical: LexicalPolicy,
) -> Result<Vec<u8>, Error> {
    let events = unfold_events(value)?;
    let mut writer = StreamWriter::new(encoding, lexical);
    let mut output = Vec::new();
    for event in &events {
        output.extend(writer.write(event)?);
    }
    Ok(output)
}

impl Error {
    fn shape(reason: impl Into<String>) -> Self { Self::at_kind(0, Reason::Shape, reason) }
    fn state(reason: impl Into<String>) -> Self { Self::at_kind(0, Reason::Shape, format!("[state] {}", reason.into())) }
}

fn exact_keys(value: &Value, expected: &[&str]) -> Result<(), Error> {
    let Value::Object(entries) = value else { return Err(Error::shape("XML event must be an Object")); };
    if entries.len() != expected.len() || expected.iter().any(|key| entries.iter().filter(|(candidate, _)| candidate == key).count() != 1) {
        return Err(Error::shape("XML event has missing, duplicate, or unknown keys"));
    }
    Ok(())
}

fn exact_leaf_keys(value: &Value, tag: &str) -> Result<(), Error> {
    match tag {
        "text" | "cdata" | "comment" => exact_keys(value, &["$xml_event", "value", "lexical"]),
        "entity_ref" => exact_keys(value, &["$xml_event", "name", "resolved_value", "lexical"]),
        "processing_instruction" => exact_keys(value, &["$xml_event", "target", "value", "lexical"]),
        _ => Err(Error::shape("unknown XML leaf event")),
    }
}

fn byte_array(value: &Value) -> Result<Vec<u8>, Error> {
    let Value::Array(values) = value else { return Err(Error::shape("XML byte value must be an Array<Int>")); };
    values.iter().map(|value| match value { Value::Int(byte) => u8::try_from(*byte).map_err(|_| Error::shape("XML byte is outside 0..255")), _ => Err(Error::shape("XML byte value must contain only Int")) }).collect()
}

fn base_namespaces() -> BTreeMap<Option<String>, String> {
    BTreeMap::from([(Some("xml".to_string()), "http://www.w3.org/XML/1998/namespace".to_string())])
}

fn parse_name(value: &Value) -> Result<Name, Error> {
    exact_keys(value, &["raw", "prefix", "local", "namespace_uri"])?;
    let raw = required_text(value, "raw").map_err(Error::shape)?.to_string();
    let prefix = optional_text_field(value, "prefix").map_err(Error::shape)?.map(str::to_string);
    let local = required_text(value, "local").map_err(Error::shape)?.to_string();
    let namespace_uri = optional_text_field(value, "namespace_uri").map_err(Error::shape)?.map(str::to_string);
    if !valid_name(&raw) || !valid_name(&local) || prefix.as_deref().is_some_and(|p| !valid_name(p)) { return Err(Error::at_kind(0, Reason::InvalidName, "invalid XML expanded name")); }
    let expected = prefix.as_ref().map(|p| format!("{p}:{local}")).unwrap_or_else(|| local.clone());
    if raw != expected { return Err(Error::at_kind(0, Reason::InvalidName, "XML name raw/prefix/local fields disagree")); }
    Ok(Name { raw, prefix, local, namespace_uri })
}

fn validate_start(event: &Value, name: &Name, mut context: BTreeMap<Option<String>, String>) -> Result<BTreeMap<Option<String>, String>, Error> {
    let Some(Value::Array(namespaces)) = field(event, "namespaces") else { return Err(Error::shape("element namespaces must be an Array")); };
    for namespace in namespaces {
        exact_keys(namespace, &["$xml", "prefix", "namespace_uri", "quote", "lexical"])?;
        if required_text(namespace, "$xml").map_err(Error::shape)? != "namespace" { return Err(Error::shape("namespaces contains a non-namespace node")); }
        let prefix = optional_text_field(namespace, "prefix").map_err(Error::shape)?.map(str::to_string);
        let uri = required_text(namespace, "namespace_uri").map_err(Error::shape)?.to_string();
        let quote = required_text(namespace, "quote").map_err(Error::shape)?;
        if !matches!(quote, "single" | "double") { return Err(Error::shape("namespace quote must be single or double")); }
        if context.insert(prefix.clone(), uri.clone()).is_some() && namespaces.iter().filter(|item| optional_text_field(item, "prefix").ok().flatten() == prefix.as_deref()).count() > 1 { return Err(Error::at_kind(0, Reason::Namespace, "duplicate namespace prefix declaration")); }
        if prefix.as_deref() == Some("xmlns") || (prefix.as_deref() == Some("xml") && uri != "http://www.w3.org/XML/1998/namespace") { return Err(Error::at_kind(0, Reason::Namespace, "reserved XML namespace binding")); }
    }
    let resolved = name.prefix.as_ref().and_then(|prefix| context.get(&Some(prefix.clone()))).cloned().or_else(|| if name.prefix.is_none() { context.get(&None).cloned() } else { None });
    if resolved != name.namespace_uri { return Err(Error::at_kind(0, Reason::Namespace, "element expanded name disagrees with namespace bindings")); }
    let Some(Value::Array(attributes)) = field(event, "attributes") else { return Err(Error::shape("element attributes must be an Array")); };
    let mut expanded = BTreeSet::new();
    for attribute in attributes {
        exact_keys(attribute, &["$xml", "name", "parts", "normalized_value", "quote", "lexical"])?;
        if required_text(attribute, "$xml").map_err(Error::shape)? != "attribute" { return Err(Error::shape("attributes contains a non-attribute node")); }
        let attr = parse_name(field(attribute, "name").ok_or_else(|| Error::shape("attribute lacks name"))?)?;
        let resolved = attr.prefix.as_ref().and_then(|prefix| context.get(&Some(prefix.clone()))).cloned();
        if resolved != attr.namespace_uri { return Err(Error::at_kind(0, Reason::Namespace, "attribute expanded name disagrees with namespace bindings")); }
        if !expanded.insert((attr.namespace_uri.clone(), attr.local.clone())) { return Err(Error::at_kind(0, Reason::DuplicateAttribute, "duplicate expanded attribute name")); }
        let quote = required_text(attribute, "quote").map_err(Error::shape)?;
        if !matches!(quote, "single" | "double") { return Err(Error::shape("attribute quote must be single or double")); }
        let Some(Value::Array(parts)) = field(attribute, "parts") else { return Err(Error::shape("attribute parts must be an Array")); };
        for part in parts { let tag = required_text(part, "$xml").map_err(Error::shape)?; if !matches!(tag, "text" | "entity_ref") { return Err(Error::shape("attribute parts contains an illegal node")); } }
    }
    Ok(context)
}

fn event_element_node(event: &Value) -> Result<Value, Error> {
    Ok(object(vec![("$xml", text("element")), ("name", field(event, "name").cloned().ok_or_else(|| Error::shape("element lacks name"))?), ("namespaces", field(event, "namespaces").cloned().ok_or_else(|| Error::shape("element lacks namespaces"))?), ("attributes", field(event, "attributes").cloned().ok_or_else(|| Error::shape("element lacks attributes"))?), ("empty_style", field(event, "empty_style").cloned().ok_or_else(|| Error::shape("element lacks empty_style"))?)]))
}

fn deterministic_event_node(event: &Value, tag: &str) -> Result<String, Error> {
    let Value::Object(entries) = event else { return Err(Error::shape("XML event must be an Object")); };
    let mut fields = vec![("$xml".to_string(), text(tag))];
    fields.extend(entries.iter().filter(|(key, _)| key != "$xml_event" && key != "lexical" && key != "open_lexical" && key != "close_lexical").cloned());
    deterministic_leaf(&Value::Object(fields), tag).map_err(Error::shape)
}

fn valid_event_raw_bytes(event: &Value, tag: &str) -> Option<Vec<u8>> {
    let key = match tag { "element_start" => "open_lexical", "element_end" => "close_lexical", "document_start" | "document_end" => return None, _ => "lexical" };
    let lexical = field(event, key)?;
    let expected = lexical_semantic(event, key)?;
    if !lexical_evidence_matches(lexical, &expected) { return None; }
    byte_array(field(lexical, "raw_bytes")?).ok()
}

fn source_encoding_matches(source: Option<&str>, bom: &[u8], target: RenderEncoding) -> bool {
    matches!((source, bom, target), (Some("UTF-8"), [], RenderEncoding::UTF8) | (Some("UTF-8"), [0xef,0xbb,0xbf], RenderEncoding::UTF8Bom) | (Some("UTF-16LE"), [0xff,0xfe], RenderEncoding::Utf16Le) | (Some("UTF-16BE"), [0xfe,0xff], RenderEncoding::Utf16Be))
}
fn target_bom(target: RenderEncoding) -> Vec<u8> { match target { RenderEncoding::UTF8 => vec![], RenderEncoding::UTF8Bom => vec![0xef,0xbb,0xbf], RenderEncoding::Utf16Le => vec![0xff,0xfe], RenderEncoding::Utf16Be => vec![0xfe,0xff] } }
fn encoding_declaration(target: RenderEncoding) -> &'static str { match target { RenderEncoding::UTF8 | RenderEncoding::UTF8Bom => "UTF-8", RenderEncoding::Utf16Le | RenderEncoding::Utf16Be => "UTF-16" } }
fn validate_declared_encoding(name: &str, target: RenderEncoding) -> Result<(), Error> { let ok = match target { RenderEncoding::UTF8 | RenderEncoding::UTF8Bom => name.eq_ignore_ascii_case("UTF-8"), RenderEncoding::Utf16Le | RenderEncoding::Utf16Be => name.eq_ignore_ascii_case("UTF-16") || name.eq_ignore_ascii_case("UTF-16LE") || name.eq_ignore_ascii_case("UTF-16BE") }; if ok { Ok(()) } else { Err(Error::at_kind(0, Reason::InvalidEncoding, "XML declaration conflicts with selected output encoding")) } }
fn encode_text(text: &str, target: RenderEncoding) -> Vec<u8> { match target { RenderEncoding::UTF8 | RenderEncoding::UTF8Bom => text.as_bytes().to_vec(), RenderEncoding::Utf16Le => text.encode_utf16().flat_map(u16::to_le_bytes).collect(), RenderEncoding::Utf16Be => text.encode_utf16().flat_map(u16::to_be_bytes).collect() } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalMode { Inclusive11, Exclusive10 }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalOptions {
    pub mode: CanonicalMode,
    pub comments: bool,
    pub inclusive_prefixes: Vec<String>,
}

/// Canonicalize the resolved XML infoset. Lexical evidence is deliberately
/// ignored: canonical output is a semantic operation, never a byte replay.
pub fn canonical_document(value: &Value, options: &CanonicalOptions) -> Result<String, Error> {
    if options.mode == CanonicalMode::Inclusive11 && !options.inclusive_prefixes.is_empty() {
        return Err(Error::shape("inclusive_prefixes is legal only for Exclusive10"));
    }
    if required_text(value, "$xml").map_err(Error::shape)? != "document" {
        return Err(Error::shape("XML canonical input must be a document"));
    }
    let Some(Value::Array(children)) = field(value, "children") else {
        return Err(Error::shape("XML document children must be an array"));
    };
    if children.iter().filter(|node| required_text(node, "$xml").ok() == Some("element")).count() != 1 {
        return Err(Error::shape("XML canonical input must contain exactly one root element"));
    }
    let mut output = String::new();
    let mut root_seen = false;
    let base = base_namespaces();
    let rendered = BTreeMap::new();
    for child in children {
        let tag = required_text(child, "$xml").map_err(Error::shape)?;
        match tag {
            "declaration" | "doctype" | "document_whitespace" => {}
            "element" => {
                root_seen = true;
                canonical_node(child, options, &base, &rendered, &mut output)?;
            }
            "comment" if options.comments => {
                if root_seen { output.push('\n'); }
                canonical_leaf(child, tag, &mut output)?;
                if !root_seen { output.push('\n'); }
            }
            "processing_instruction" => {
                if root_seen { output.push('\n'); }
                canonical_leaf(child, tag, &mut output)?;
                if !root_seen { output.push('\n'); }
            }
            "comment" => {}
            _ => return Err(Error::shape("illegal node outside XML root element")),
        }
    }
    Ok(output)
}

fn canonical_node(value: &Value, options: &CanonicalOptions, parent_scope: &BTreeMap<Option<String>, String>, parent_rendered: &BTreeMap<Option<String>, String>, output: &mut String) -> Result<(), Error> {
    let tag = required_text(value, "$xml").map_err(Error::shape)?;
    if tag != "element" { return canonical_leaf(value, tag, output); }
    let name = parse_name(field(value, "name").ok_or_else(|| Error::shape("element lacks name"))?)?;
    let mut scope = parent_scope.clone();
    let Some(Value::Array(namespaces)) = field(value, "namespaces") else { return Err(Error::shape("element namespaces must be an array")); };
    for namespace in namespaces {
        let prefix = optional_text_field(namespace, "prefix").map_err(Error::shape)?.map(str::to_string);
        let uri = required_text(namespace, "namespace_uri").map_err(Error::shape)?.to_string();
        if !uri.is_empty() && !uri.contains(':') { return Err(Error::at_kind(0, Reason::Namespace, "relative namespace URI cannot be canonicalized")); }
        scope.insert(prefix, uri);
    }
    let resolved = name.prefix.as_ref().and_then(|p| scope.get(&Some(p.clone()))).cloned().or_else(|| name.prefix.is_none().then(|| scope.get(&None).cloned()).flatten());
    if resolved != name.namespace_uri { return Err(Error::at_kind(0, Reason::Namespace, "element expanded name disagrees with namespace bindings")); }
    let Some(Value::Array(attributes)) = field(value, "attributes") else { return Err(Error::shape("element attributes must be an array")); };
    let mut attrs = Vec::new();
    let mut visible = BTreeSet::new();
    if let Some(prefix) = &name.prefix { visible.insert(Some(prefix.clone())); } else if name.namespace_uri.is_some() { visible.insert(None); }
    for attribute in attributes {
        let attr_name = parse_name(field(attribute, "name").ok_or_else(|| Error::shape("attribute lacks name"))?)?;
        if let Some(prefix) = &attr_name.prefix { visible.insert(Some(prefix.clone())); }
        let resolved = attr_name.prefix.as_ref().and_then(|p| scope.get(&Some(p.clone()))).cloned();
        if resolved != attr_name.namespace_uri { return Err(Error::at_kind(0, Reason::Namespace, "attribute expanded name disagrees with namespace bindings")); }
        let value = canonical_attribute_value(attribute)?;
        attrs.push((attr_name.namespace_uri.clone().unwrap_or_default(), attr_name.local.clone(), attr_name.raw, value));
    }
    for prefix in &options.inclusive_prefixes { visible.insert(if prefix == "#default" { None } else { Some(prefix.clone()) }); }
    let mut declarations: Vec<_> = scope.iter().filter(|(prefix, uri)| {
        if prefix.as_deref() == Some("xml") { return false; }
        let selected = options.mode == CanonicalMode::Inclusive11 || visible.contains(*prefix);
        selected && parent_rendered.get(*prefix) != Some(*uri)
    }).map(|(prefix, uri)| (prefix.clone(), uri.clone())).collect();
    declarations.sort_by(|a, b| match (&a.0, &b.0) { (None, None) => std::cmp::Ordering::Equal, (None, _) => std::cmp::Ordering::Less, (_, None) => std::cmp::Ordering::Greater, (Some(a), Some(b)) => a.cmp(b) });
    attrs.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    output.push('<'); output.push_str(&name.raw);
    let mut rendered = parent_rendered.clone();
    for (prefix, uri) in declarations {
        output.push_str(" xmlns"); if let Some(prefix) = &prefix { output.push(':'); output.push_str(prefix); }
        output.push_str("=\""); output.push_str(&canonical_attr_escape(&uri)); output.push('"');
        rendered.insert(prefix, uri);
    }
    for (_, _, raw, value) in attrs { output.push(' '); output.push_str(&raw); output.push_str("=\""); output.push_str(&canonical_attr_escape(&value)); output.push('"'); }
    output.push('>');
    let Some(Value::Array(children)) = field(value, "children") else { return Err(Error::shape("element children must be an array")); };
    for child in children {
        if required_text(child, "$xml").map_err(Error::shape)? == "comment" && !options.comments { continue; }
        canonical_node(child, options, &scope, &rendered, output)?;
    }
    output.push_str("</"); output.push_str(&name.raw); output.push('>');
    Ok(())
}

fn canonical_attribute_value(attribute: &Value) -> Result<String, Error> {
    if let Some(value) = optional_text_field(attribute, "normalized_value").map_err(Error::shape)? { return Ok(value.to_string()); }
    let Some(Value::Array(parts)) = field(attribute, "parts") else { return Err(Error::shape("attribute parts must be an array")); };
    let mut output = String::new();
    for part in parts {
        match required_text(part, "$xml").map_err(Error::shape)? {
            "text" => output.push_str(required_text(part, "value").map_err(Error::shape)?),
            "entity_ref" => output.push_str(optional_text_field(part, "resolved_value").map_err(Error::shape)?.ok_or_else(|| Error::at_kind(0, Reason::Canonicalization, "unresolved entity cannot be canonicalized"))?),
            _ => return Err(Error::shape("illegal XML attribute part")),
        }
    }
    Ok(output)
}

fn canonical_leaf(value: &Value, tag: &str, output: &mut String) -> Result<(), Error> {
    match tag {
        "text" | "cdata" => output.push_str(&canonical_text_escape(required_text(value, "value").map_err(Error::shape)?)),
        "entity_ref" => output.push_str(&canonical_text_escape(optional_text_field(value, "resolved_value").map_err(Error::shape)?.ok_or_else(|| Error::at_kind(0, Reason::Canonicalization, "unresolved entity cannot be canonicalized"))?)),
        "comment" => { output.push_str("<!--"); output.push_str(&normalize_lines(required_text(value, "value").map_err(Error::shape)?)); output.push_str("-->"); }
        "processing_instruction" => { output.push_str("<?"); output.push_str(required_text(value, "target").map_err(Error::shape)?); let body = normalize_lines(required_text(value, "value").map_err(Error::shape)?); if !body.is_empty() { output.push(' '); output.push_str(&body); } output.push_str("?>"); }
        _ => return Err(Error::shape(format!("unsupported canonical XML node {tag}"))),
    }
    Ok(())
}

fn normalize_lines(value: &str) -> String { value.replace("\r\n", "\n").replace('\r', "\n") }
fn canonical_text_escape(value: &str) -> String { normalize_lines(value).replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;") }
fn canonical_attr_escape(value: &str) -> String { value.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;").replace('\t', "&#x9;").replace('\n', "&#xA;").replace('\r', "&#xD;") }

/// D-ENCXML-PROJECTION1=A: focused helpers over the closed `$xml` tree.
fn xml_tag_of(value: &Value) -> Option<&str> {
    match field(value, "$xml") {
        Some(Value::Text(tag)) => Some(tag.as_str()),
        _ => None,
    }
}

fn projection_err(path: &str, reason: impl Into<String>) -> Error {
    Error {
        kind: Reason::Shape,
        offset: 0,
        line: None,
        column: None,
        path: path.to_string(),
        reason: reason.into(),
    }
}

fn entity_err(path: &str, reason: impl Into<String>) -> Error {
    Error {
        kind: Reason::Entity,
        offset: 0,
        line: None,
        column: None,
        path: path.to_string(),
        reason: reason.into(),
    }
}

fn clark_key(local: &str, namespace_uri: Option<&str>) -> String {
    match namespace_uri {
        Some(uri) if !uri.is_empty() => format!("{{{uri}}}{local}"),
        _ => local.to_string(),
    }
}

fn read_name_parts(name: &Value, path: &str) -> Result<(String, Option<String>, String, Option<String>), Error> {
    let raw = required_text(name, "raw").map_err(|reason| projection_err(path, reason))?.to_string();
    let prefix = optional_text_field(name, "prefix")
        .map_err(|reason| projection_err(path, reason))?
        .map(str::to_string);
    let local = required_text(name, "local").map_err(|reason| projection_err(path, reason))?.to_string();
    let namespace_uri = optional_text_field(name, "namespace_uri")
        .map_err(|reason| projection_err(path, reason))?
        .map(str::to_string);
    Ok((raw, prefix, local, namespace_uri))
}

fn selector_matches(local: &str, namespace_uri: Option<&str>, selector: &str) -> bool {
    if let Some(rest) = selector.strip_prefix('{') {
        let Some((uri, sel_local)) = rest.split_once('}') else {
            return false;
        };
        return local == sel_local && namespace_uri == Some(uri);
    }
    // Local or Clark only — never treat `prefix:local` as a prefix selector.
    local == selector && namespace_uri.map(|uri| uri.is_empty()).unwrap_or(true)
}

fn text_like_chars(node: &Value) -> Result<Option<String>, Error> {
    match xml_tag_of(node) {
        Some("text") | Some("cdata") => {
            Ok(Some(required_text(node, "value").map_err(|reason| projection_err("$", reason))?.to_string()))
        }
        Some("entity_ref") => match optional_text_field(node, "resolved_value").map_err(|reason| projection_err("$", reason))? {
            Some(value) => Ok(Some(value.to_string())),
            None => Ok(None),
        },
        _ => Ok(None),
    }
}

fn is_simple_content(children: &[Value]) -> bool {
    children.iter().all(|child| matches!(xml_tag_of(child), Some("text" | "cdata" | "entity_ref")))
        && children.iter().all(|child| {
            !matches!(xml_tag_of(child), Some("entity_ref"))
                || matches!(field(child, "resolved_value"), Some(Value::Text(_)))
        })
}

/// Root element of a document `$xml:"document"` tree.
pub fn document_root(document: &Value) -> Result<Value, Error> {
    if xml_tag_of(document) != Some("document") {
        return Err(projection_err("$", "XML root expects a document node"));
    }
    let Value::Array(children) = field(document, "children").ok_or_else(|| projection_err("$", "XML document children missing"))? else {
        return Err(projection_err("$", "XML document children must be an array"));
    };
    let mut root = None;
    for child in children {
        if xml_tag_of(child) == Some("element") {
            if root.is_some() {
                return Err(projection_err("$", "XML document must contain exactly one root element"));
            }
            root = Some(child.clone());
        }
    }
    root.ok_or_else(|| projection_err("$", "XML document must contain exactly one root element"))
}

/// Expanded-name view: `(raw, prefix, local, namespace_uri)`.
pub fn expanded_name_parts(node: &Value) -> Result<(String, Option<String>, String, Option<String>), Error> {
    match xml_tag_of(node) {
        Some("element") | Some("attribute") | Some("namespace") => {}
        Some(other) => {
            return Err(projection_err("$", format!("expanded_name requires an element, attribute, or namespace node; found {other}")));
        }
        None => return Err(projection_err("$", "expanded_name requires a tagged XML node")),
    }
    let name = field(node, "name").ok_or_else(|| projection_err("$", "XML name object missing"))?;
    read_name_parts(name, "$")
}

/// Attribute selector is a local name or Clark name; returns normalized_value.
pub fn lookup_attribute(element: &Value, name: &str) -> Result<Option<String>, Error> {
    if xml_tag_of(element) != Some("element") {
        return Err(projection_err("$", "attribute helper requires an element node"));
    }
    let Value::Array(attributes) = field(element, "attributes").ok_or_else(|| projection_err("$", "XML element attributes missing"))? else {
        return Err(projection_err("$", "XML element attributes must be an array"));
    };
    for attribute in attributes {
        if xml_tag_of(attribute) != Some("attribute") {
            return Err(projection_err("$", "XML attributes array must contain attribute nodes"));
        }
        let attr_name = field(attribute, "name").ok_or_else(|| projection_err("$", "XML attribute name missing"))?;
        let (_, _, local, namespace_uri) = read_name_parts(attr_name, "$")?;
        if !selector_matches(&local, namespace_uri.as_deref(), name) {
            continue;
        }
        match optional_text_field(attribute, "normalized_value").map_err(|reason| projection_err("$", reason))? {
            Some(value) => return Ok(Some(value.to_string())),
            None => {
                return Err(entity_err(
                    &format!("@{}", clark_key(&local, namespace_uri.as_deref())),
                    "attribute has an unresolved entity",
                ));
            }
        }
    }
    Ok(None)
}

/// Exact child nodes in source order.
pub fn element_content(element: &Value) -> Result<Vec<Value>, Error> {
    if xml_tag_of(element) != Some("element") {
        return Err(projection_err("$", "content helper requires an element node"));
    }
    let Value::Array(children) = field(element, "children").ok_or_else(|| projection_err("$", "XML element children missing"))? else {
        return Err(projection_err("$", "XML element children must be an array"));
    };
    Ok(children.clone())
}

fn project_element_value(element: &Value, path: &str) -> Result<Value, Error> {
    if xml_tag_of(element) != Some("element") {
        return Err(projection_err(path, "Codable projection requires an element node"));
    }
    let Value::Array(attributes) = field(element, "attributes").ok_or_else(|| projection_err(path, "XML element attributes missing"))? else {
        return Err(projection_err(path, "XML element attributes must be an array"));
    };
    let Value::Array(children) = field(element, "children").ok_or_else(|| projection_err(path, "XML element children missing"))? else {
        return Err(projection_err(path, "XML element children must be an array"));
    };

    let mut entries: Vec<(String, Value)> = Vec::new();
    for attribute in attributes {
        if xml_tag_of(attribute) != Some("attribute") {
            return Err(projection_err(path, "XML attributes array must contain attribute nodes"));
        }
        let attr_name = field(attribute, "name").ok_or_else(|| projection_err(path, "XML attribute name missing"))?;
        let (_, _, local, namespace_uri) = read_name_parts(attr_name, path)?;
        let key = format!("@{}", clark_key(&local, namespace_uri.as_deref()));
        let child_path = if path == "$" { key.clone() } else { format!("{path}.{key}") };
        match optional_text_field(attribute, "normalized_value").map_err(|reason| projection_err(&child_path, reason))? {
            Some(value) => entries.push((key, Value::Text(value.to_string()))),
            None => {
                return Err(entity_err(&child_path, "attribute has an unresolved entity"));
            }
        }
    }

    if is_simple_content(children) {
        let mut text = String::new();
        for child in children {
            match text_like_chars(child)? {
                Some(piece) => text.push_str(&piece),
                None => {
                    return Err(entity_err(path, "simple content has an unresolved entity"));
                }
            }
        }
        // Scalar-friendly: attribute-less simple elements project as Text so a
        // parent field `title: String` decodes the child directly from $text.
        // Attributes keep the object form with `$text` (e.g. Price).
        if entries.is_empty() {
            return Ok(Value::Text(text));
        }
        entries.push(("$text".to_string(), Value::Text(text)));
        return Ok(Value::Object(entries));
    } else {
        let mut child_keys: Vec<(String, Value)> = Vec::new();
        let mut content: Vec<Value> = Vec::new();
        for (index, child) in children.iter().enumerate() {
            content.push(child.clone());
            if xml_tag_of(child) != Some("element") {
                continue;
            }
            let name = field(child, "name").ok_or_else(|| projection_err(path, "XML child element name missing"))?;
            let (_, _, local, namespace_uri) = read_name_parts(name, path)?;
            let key = clark_key(&local, namespace_uri.as_deref());
            let child_path = if path == "$" {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            let projected = project_element_value(child, &child_path)?;
            if let Some((_, existing)) = child_keys.iter_mut().find(|(candidate, _)| candidate == &key) {
                let prior = existing.clone();
                match prior {
                    Value::Array(mut items) => {
                        items.push(projected);
                        *existing = Value::Array(items);
                    }
                    other => *existing = Value::Array(vec![other, projected]),
                }
            } else {
                let _ = index;
                child_keys.push((key, projected));
            }
        }
        entries.extend(child_keys);
        entries.push(("$content".to_string(), Value::Array(content)));
    }

    Ok(Value::Object(entries))
}

/// Project the document root element into the Codable view (`@`, Clark children, `$text`/`$content`).
pub fn project_document_for_decode(document: &Value) -> Result<Value, Error> {
    let root = document_root(document)?;
    project_element_value(&root, "$")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use super::{
        lookup_attribute, canonical_document, document_root, element_content, expanded_name_parts, field,
        fold_events, parse_document, parse_document_bytes, parse_document_with, project_document_for_decode,
        render_document, render_document_bytes, required_text, stream_event_value, strip_lexical, text,
        unfold_events, wire_bytes, ByteLexer, CanonicalMode, CanonicalOptions, EntityPolicy, Error, Event,
        LexicalPolicy, Limits, ParseOptions, Part, Reason, RenderEncoding, Scanner, StreamEvent,
        StreamScanner, StreamWriter, TokenKind, Value, WireEncoding, object,
    };

    fn scan(source: &str) -> Result<Vec<Event>, String> {
        let mut scanner = Scanner::new(source);
        let mut events = Vec::new();
        loop {
            match scanner
                .next()
                .map_err(|error| format!("{}:{}", error.offset, error.reason))?
            {
                Some(event) => events.push(event),
                None => return Ok(events),
            }
        }
    }

    #[test]
    fn preserves_order_namespaces_and_lexical_tokens() {
        let source = "<?xml version=\"1.0\"?><r xmlns='u' xmlns:p='v' a='x&amp;y'>a<!--c--><![CDATA[<x>]]><?go now?><p:c/>z</r>\n";
        let events = scan(source).expect("valid XML");
        assert_eq!(events.len(), 12);
        let Event::ElementStart {
            name,
            namespaces,
            attributes,
            raw,
            ..
        } = &events[2]
        else {
            panic!("root start")
        };
        assert_eq!(
            (
                name.local.as_str(),
                name.namespace_uri.as_deref(),
                raw.as_str()
            ),
            ("r", Some("u"), "<r xmlns='u' xmlns:p='v' a='x&amp;y'>")
        );
        assert_eq!(namespaces.len(), 2);
        assert_eq!(attributes[0].normalized.as_deref(), Some("x&y"));
        assert!(
            matches!(&attributes[0].parts[1], Part::Entity { name, resolved: Some(value), .. } if name == "amp" && value == "&")
        );
        assert!(matches!(&events[4], Event::Comment { value, .. } if value == "c"));
        assert!(matches!(&events[5], Event::Cdata { value, .. } if value == "<x>"));
        assert!(
            matches!(&events[6], Event::ProcessingInstruction { target, value, .. } if target == "go" && value == "now")
        );
        assert!(
            matches!(&events[7], Event::ElementStart { name, empty: true, .. } if name.namespace_uri.as_deref() == Some("v"))
        );
        assert!(matches!(&events[10], Event::DocumentWhitespace { value, .. } if value == "\n"));
    }

    #[test]
    fn rejects_namespace_and_structure_corruption() {
        for source in [
            "<p:r/>",
            "<r xmlns:p='u' xmlns:q='u' p:a='1' q:a='2'/>",
            "<r xmlns:p='u' xmlns:p='v'/>",
            "<r><a></r>",
            "<r/><s/>",
            "<r><!-- a--b --></r>",
            "<r>&missing;</r>",
            "<r>&#x110000;</r>",
            "<r a='bad<value'/>",
            "<r>bad]]>text</r>",
        ] {
            assert!(scan(source).is_err(), "accepted {source}");
        }
    }

    #[test]
    fn xml_10_fifth_edition_char_production_is_exact_and_chunk_invariant() {
        fn stream_error(bytes: &[u8], split: usize) -> Error {
            let mut scanner = StreamScanner::new(4096, ParseOptions::safe()).expect("scanner");
            let error = 'failed: {
                if let Err(error) = scanner.push(&bytes[..split]) {
                    break 'failed error;
                }
                loop {
                    match scanner.next() {
                        Ok(Some(_)) => {}
                        Ok(None) => break,
                        Err(error) => break 'failed error,
                    }
                }
                if let Err(error) = scanner.push(&bytes[split..]) {
                    break 'failed error;
                }
                if let Err(error) = scanner.finish_input() {
                    break 'failed error;
                }
                loop {
                    match scanner.next() {
                        Ok(Some(_)) => {}
                        Ok(None) => panic!("stream accepted forbidden XML character at split {split}"),
                        Err(error) => break 'failed error,
                    }
                }
            };
            assert_eq!(scanner.next(), Err(error.clone()), "terminal split {split}");
            error
        }

        for character in [
            '\u{9}', '\u{a}', '\u{d}', '\u{20}', '\u{d7ff}', '\u{e000}', '\u{fffd}',
            '\u{10000}', '\u{10ffff}',
        ] {
            parse_document(&format!("<r>{character}</r>")).unwrap_or_else(|error| {
                panic!("rejected XML Char U+{:X}: {error:?}", character as u32)
            });
        }
        for reference in [
            "&#9;", "&#xA;", "&#13;", "&#x20;", "&#xD7FF;", "&#xE000;", "&#xFFFD;",
            "&#x10000;", "&#x10FFFF;",
        ] {
            parse_document(&format!("<r>{reference}</r>")).unwrap_or_else(|error| {
                panic!("rejected XML character reference {reference}: {error:?}")
            });
        }

        for character in [
            '\u{0}', '\u{1}', '\u{8}', '\u{b}', '\u{c}', '\u{e}', '\u{1f}', '\u{fffe}',
            '\u{ffff}',
        ] {
            let source = format!("<r>{character}</r>");
            let expected = parse_document(&source).expect_err("forbidden XML character");
            assert_eq!(
                (expected.kind, expected.offset, expected.line, expected.column),
                (Reason::Malformed, 3, Some(1), Some(4)),
                "U+{:X}",
                character as u32
            );
            for split in 0..=source.len() {
                let actual = stream_error(source.as_bytes(), split);
                assert_eq!(actual, expected, "U+{:X} split {split}", character as u32);
            }
        }

        for source in [
            "<r a='\u{1}'/>",
            "<r><!--\u{1}--></r>",
            "<r><![CDATA[\u{1}]]></r>",
            "<r><?p \u{1}?></r>",
            "<!DOCTYPE r [\u{1}]><r/>",
            "<r\u{1}/>",
            "<r></r\u{1}>",
            "<r>&\u{1};</r>",
            "<r/>\u{1}",
        ] {
            let expected = parse_document(source).expect_err("forbidden XML token character");
            assert_eq!(expected.kind, Reason::Malformed, "{source:?}");
            for split in 0..=source.len() {
                assert_eq!(
                    stream_error(source.as_bytes(), split),
                    expected,
                    "{source:?} split {split}"
                );
            }
        }

        for reference in [
            "&#0;", "&#x1;", "&#xB;", "&#xC;", "&#x1F;", "&#xFFFE;", "&#xFFFF;",
        ] {
            let error = parse_document(&format!("<r>{reference}</r>"))
                .expect_err("forbidden XML character reference");
            assert_eq!(error.kind, Reason::Entity, "{reference}");
        }

        for (source, offset, column, path) in [
            ("<r a='&#0;'/>", 6, 7, "$"),
            ("<r xmlns='&#0;'/>", 10, 11, "$"),
            ("<r>&#0;</r>", 3, 4, "$/r"),
        ] {
            let expected = parse_document(source).expect_err("forbidden numeric reference");
            assert_eq!(
                (
                    expected.kind,
                    expected.offset,
                    expected.line,
                    expected.column,
                    expected.path.as_str(),
                ),
                (Reason::Entity, offset, Some(1), Some(column), path),
                "{source}"
            );
            for split in 0..=source.len() {
                assert_eq!(
                    stream_error(source.as_bytes(), split),
                    expected,
                    "{source} split {split}"
                );
            }
        }
    }

    #[test]
    fn xml_10_attribute_whitespace_normalizes_without_losing_lexical_identity() {
        let source = "<r xmlns='urn:\tfoo\r\nbar' a='x\t y\r\nz\ru\nv' b='&#xD;&#xA;&#x9;'/>";
        let events = scan(source).expect("valid XML");
        let Event::ElementStart {
            namespaces,
            attributes,
            ..
        } = &events[1]
        else {
            panic!("root start")
        };
        assert_eq!(namespaces[0].namespace_uri, "urn: foo bar");
        assert_eq!(attributes[0].normalized.as_deref(), Some("x  y z u v"));
        assert!(matches!(
            &attributes[0].parts[..],
            [Part::Text { value, raw }]
                if value == "x  y z u v" && raw == "x\t y\r\nz\ru\nv"
        ));
        assert_eq!(attributes[1].normalized.as_deref(), Some("\r\n\t"));

        let resolved_source =
            "<!DOCTYPE r [<!ENTITY ws 'ignored'>]><r xmlns='&ws;' a='&ws;' b='&#xD;&#xA;&#x9;'/>";
        let mut replacements = BTreeMap::new();
        replacements.insert(
            "ws".to_string(),
            "urn:\talpha\r\nbeta\rgamma\ndelta".to_string(),
        );
        let options = ParseOptions {
            entities: EntityPolicy::Resolve(replacements),
            limits: Limits::safe(),
        };
        let mut scanner =
            Scanner::with_options(resolved_source, options.clone()).expect("options");
        let (namespace, attribute, numeric) = loop {
            match scanner.next().expect("resolved XML") {
                Some(Event::ElementStart {
                    namespaces,
                    attributes,
                    ..
                }) => break (
                    namespaces[0].clone(),
                    attributes[0].clone(),
                    attributes[1].clone(),
                ),
                Some(_) => {}
                None => panic!("root start"),
            }
        };
        let expected = "urn: alpha beta gamma delta";
        assert_eq!(namespace.namespace_uri, expected);
        assert_eq!(attribute.normalized.as_deref(), Some(expected));
        assert_eq!(numeric.normalized.as_deref(), Some("\r\n\t"));
        assert!(matches!(
            &attribute.parts[..],
            [Part::Entity { resolved: Some(value), .. }] if value == expected
        ));
        let tree = parse_document_with(resolved_source, &options).expect("resolved tree");
        assert_eq!(render_document(&tree), Ok(resolved_source.to_string()));

        let tree = parse_document(source).expect("whole tree");
        assert_eq!(render_document(&tree), Ok(source.to_string()));
        for split in 0..=source.len() {
            let mut writer = StreamWriter::new(RenderEncoding::UTF8, LexicalPolicy::PreserveValid);
            let mut output = Vec::new();
            for event in stream_values(source.as_bytes(), split) {
                output.extend(writer.write(&event).expect("write event"));
            }
            assert_eq!(output, source.as_bytes(), "split {split}");
        }
    }

    #[test]
    fn folds_lossless_tagged_tree_and_round_trips() {
        let source = "<?xml version='1.0'?>\n<!DOCTYPE r PUBLIC 'pub' 'sys' [<!ENTITY e 'v'>]><r xmlns='u' xmlns:p='v' p:a='x&amp;y'>a&amp;<!--c--><![CDATA[<x>]]><?go now?><p:c/></r>\n";
        let tree = parse_document(source).expect("valid XML");
        assert_eq!(field(&tree, "$xml"), Some(&Value::Text("document".into())));
        let Some(Value::Array(document_children)) = field(&tree, "children") else {
            panic!("document children")
        };
        assert_eq!(document_children.len(), 5);
        assert_eq!(
            field(&document_children[2], "public_id"),
            Some(&Value::Text("pub".into()))
        );
        assert_eq!(
            field(&document_children[2], "system_id"),
            Some(&Value::Text("sys".into()))
        );
        assert_eq!(
            field(&document_children[2], "internal_subset"),
            Some(&Value::Text("<!ENTITY e 'v'>".into()))
        );
        let Some(Value::Array(root_children)) = field(&document_children[3], "children") else {
            panic!("root children")
        };
        let tags: Vec<_> = root_children
            .iter()
            .map(|node| field(node, "$xml"))
            .collect();
        assert_eq!(
            tags,
            vec![
                Some(&Value::Text("text".into())),
                Some(&Value::Text("entity_ref".into())),
                Some(&Value::Text("comment".into())),
                Some(&Value::Text("cdata".into())),
                Some(&Value::Text("processing_instruction".into())),
                Some(&Value::Text("element".into())),
            ]
        );
        assert_eq!(render_document(&tree), Ok(source.to_string()));
    }

    #[test]
    fn reports_stable_kind_coordinates_and_path() {
        let error = parse_document("<r>\n <a></b></r>").expect_err("mismatch");
        assert_eq!(error.kind, Reason::MismatchedTag);
        assert_eq!(error.offset, 8);
        assert_eq!(error.line, Some(2));
        assert_eq!(error.column, Some(5));
        assert_eq!(error.path, "$/r/a");
        assert_eq!(error.reason, "mismatched XML closing tag");
    }

    #[test]
    fn enforces_whole_value_limits_prospectively() {
        let mut limits = Limits::safe();
        limits.max_depth = 2;
        limits.max_entity_depth = 2;
        let error = parse_document_with(
            "<a><b><c/></b></a>",
            &ParseOptions { entities: EntityPolicy::Preserve, limits: limits.clone() },
        ).expect_err("depth budget");
        assert_eq!(error.kind, Reason::Limit);
        assert_eq!((error.offset, error.line, error.column, error.path.as_str()), (10, Some(1), Some(11), "$/a/b"));
        assert!(error.reason.contains("max_depth (2)"));

        limits = Limits::safe();
        limits.max_nodes = 2;
        let error = parse_document_with(
            "<a>x</a>",
            &ParseOptions { entities: EntityPolicy::Preserve, limits: limits.clone() },
        ).expect_err("node budget");
        assert_eq!(error.kind, Reason::Limit);
        assert!(error.reason.contains("max_nodes (2)"));

        limits = Limits::safe();
        limits.max_attributes_per_element = 1;
        let error = parse_document_with(
            "<a x='1' y='2'/>",
            &ParseOptions { entities: EntityPolicy::Preserve, limits },
        ).expect_err("attribute budget");
        assert_eq!(error.kind, Reason::Limit);
        assert!(error.reason.contains("max_attributes_per_element (1)"));

        limits = Limits::safe();
        limits.max_name_bytes = 1;
        let error = parse_document_with(
            "<ab/>",
            &ParseOptions { entities: EntityPolicy::Preserve, limits },
        ).expect_err("name budget");
        assert_eq!(error.kind, Reason::Limit);
        assert!(error.reason.contains("max_name_bytes (1)"));

        limits = Limits::safe();
        limits.max_text_bytes = 1;
        limits.max_entity_replacement_bytes = 1;
        let error = parse_document_with(
            "<a>xx</a>",
            &ParseOptions { entities: EntityPolicy::Preserve, limits },
        ).expect_err("text budget");
        assert_eq!(error.kind, Reason::Limit);
        assert!(error.reason.contains("max_text_bytes (1)"));

        limits = Limits::safe();
        limits.max_entity_declarations = 1;
        let error = parse_document_with(
            "<!DOCTYPE r [<!ENTITY a 'a'><!ENTITY b 'b'>]><r/>",
            &ParseOptions { entities: EntityPolicy::Preserve, limits },
        ).expect_err("entity declaration budget");
        assert_eq!(error.kind, Reason::Limit);
        assert!(error.reason.contains("max_entity_declarations (1)"));
    }

    #[test]
    fn xml_limits_validate_in_ratified_field_order() {
        let mut limits = Limits::safe();
        limits.max_depth = 0;
        limits.max_nodes = 0;
        let error = limits.validate().expect_err("max_depth first");
        assert_eq!(error.kind, Reason::Limit);
        assert_eq!((error.offset, error.line, error.column, error.path.as_str()), (0, None, None, ""));
        assert!(error.reason.contains("`max_depth`"));
        assert!(!error.reason.contains("`max_nodes`"));

        limits = Limits::safe();
        limits.max_nodes = 0;
        limits.max_attributes_per_element = 1_000_001;
        let error = limits.validate().expect_err("max_nodes before attributes");
        assert!(error.reason.contains("`max_nodes`"));
        assert!(!error.reason.contains("`max_attributes_per_element`"));

        limits = Limits::safe();
        limits.max_attributes_per_element = 1_000_001;
        limits.max_name_bytes = 0;
        let error = limits.validate().expect_err("attributes before name");
        assert!(error.reason.contains("`max_attributes_per_element`"));

        limits = Limits::safe();
        limits.max_name_bytes = 0;
        limits.max_text_bytes = 1_073_741_825;
        let error = limits.validate().expect_err("name before text");
        assert!(error.reason.contains("`max_name_bytes`"));

        limits = Limits::safe();
        limits.max_text_bytes = 1_073_741_825;
        limits.max_entity_declarations = 1_000_001;
        let error = limits.validate().expect_err("text before entity declarations");
        assert!(error.reason.contains("`max_text_bytes`"));

        limits = Limits::safe();
        limits.max_entity_declarations = 1_000_001;
        limits.max_entity_depth = 257;
        let error = limits.validate().expect_err("entity declarations before depth");
        assert!(error.reason.contains("`max_entity_declarations`"));

        limits = Limits::safe();
        limits.max_entity_depth = 257;
        limits.max_entity_replacement_bytes = 1_073_741_825;
        let error = limits.validate().expect_err("entity depth before replacement");
        assert!(error.reason.contains("`max_entity_depth`"));

        limits = Limits::safe();
        limits.max_entity_replacement_bytes = 1_073_741_825;
        let error = limits.validate().expect_err("replacement range");
        assert!(error.reason.contains("`max_entity_replacement_bytes`"));

        limits = Limits::safe();
        limits.max_depth = 2;
        limits.max_entity_depth = 3;
        let error = limits.validate().expect_err("cross-field depth");
        assert_eq!(error.path, "");
        assert!(error.reason.contains("`max_entity_depth` exceeds `max_depth`"));

        limits = Limits::safe();
        limits.max_text_bytes = 8;
        limits.max_entity_replacement_bytes = 9;
        let error = limits.validate().expect_err("cross-field replacement");
        assert!(error.reason.contains("`max_entity_replacement_bytes` exceeds `max_text_bytes`"));

        assert!(Limits::safe().validate().is_ok());
    }

    #[test]
    fn dual_limit_fusion_keeps_stronger_bound_and_names_it() {
        // Mirrors EncodingStream: effective ceilings are min(encoding, xml).
        let mut xml = Limits::safe();
        xml.max_depth = 8;
        xml.max_entity_depth = 8;
        xml.max_entity_replacement_bytes = 64;
        let encoding_depth = 2usize;
        let encoding_expansion_depth = 1usize;
        let encoding_expansion_bytes = 4usize;
        xml.max_depth = xml.max_depth.min(encoding_depth);
        xml.max_entity_depth = xml
            .max_entity_depth
            .min(encoding_expansion_depth)
            .min(xml.max_depth);
        xml.max_entity_replacement_bytes = xml
            .max_entity_replacement_bytes
            .min(encoding_expansion_bytes)
            .min(xml.max_text_bytes);
        assert_eq!(xml.max_depth, 2);
        assert_eq!(xml.max_entity_depth, 1);
        assert_eq!(xml.max_entity_replacement_bytes, 4);

        let error = parse_document_with(
            "<a><b><c/></b></a>",
            &ParseOptions {
                entities: EntityPolicy::Preserve,
                limits: xml.clone(),
            },
        )
        .expect_err("encoding depth wins");
        assert_eq!(error.kind, Reason::Limit);
        assert!(error.reason.contains("max_depth (2)"));

        let mut values = BTreeMap::new();
        values.insert("e".to_string(), "value".to_string());
        let error = parse_document_with(
            "<!DOCTYPE r [<!ENTITY e 'ignored'>]><r>&e;</r>",
            &ParseOptions {
                entities: EntityPolicy::Resolve(values),
                limits: xml,
            },
        )
        .expect_err("encoding expansion wins");
        assert_eq!(error.kind, Reason::Limit);
        assert!(
            error.reason.contains("max_entity_replacement_bytes (4)"),
            "{}",
            error.reason
        );

        let mut xml_tight = Limits::safe();
        xml_tight.max_depth = 1;
        let encoding_loose = 256usize;
        xml_tight.max_depth = xml_tight.max_depth.min(encoding_loose);
        xml_tight.max_entity_depth = xml_tight.max_entity_depth.min(xml_tight.max_depth);
        let error = parse_document_with(
            "<a><b/></a>",
            &ParseOptions {
                entities: EntityPolicy::Preserve,
                limits: xml_tight,
            },
        )
        .expect_err("xml depth wins");
        assert!(error.reason.contains("max_depth (1)"));
    }

    #[test]
    fn max_name_bytes_checks_element_raw_before_retention() {
        let mut limits = Limits::safe();
        limits.max_name_bytes = 3;
        // Prefixed raw `p:ab` is 4 UTF-8 bytes even though local/prefix alone fit.
        let error = parse_document_with(
            "<r xmlns:p='urn:p'><p:ab/></r>",
            &ParseOptions {
                entities: EntityPolicy::Preserve,
                limits,
            },
        )
        .expect_err("raw name budget");
        assert_eq!(error.kind, Reason::Limit);
        assert!(error.reason.contains("max_name_bytes (3)"));
        assert!(error.reason.contains("element name") || error.reason.contains("name"));
    }

    #[test]
    fn applies_entity_policy_and_replacement_budget() {
        let source = "<!DOCTYPE r [<!ENTITY e 'ignored'>]><r>&e;</r>";
        let mut values = BTreeMap::new();
        values.insert("e".to_string(), "value".to_string());
        let resolved = parse_document_with(source, &ParseOptions {
            entities: EntityPolicy::Resolve(values.clone()),
            limits: Limits::safe(),
        }).expect("explicit inert replacement");
        let rendered = render_document(&resolved).expect("render");
        assert_eq!(rendered, source);

        let rejected = parse_document_with(source, &ParseOptions {
            entities: EntityPolicy::Reject,
            limits: Limits::safe(),
        }).expect_err("reject policy");
        assert_eq!(rejected.kind, Reason::Entity);

        let mut limits = Limits::safe();
        limits.max_entity_replacement_bytes = 4;
        let limited = parse_document_with(source, &ParseOptions {
            entities: EntityPolicy::Resolve(values.clone()),
            limits,
        }).expect_err("replacement budget");
        assert_eq!(limited.kind, Reason::Limit);
        assert!(limited.reason.contains("max_entity_replacement_bytes (4)"));

        let mut limits = Limits::safe();
        limits.max_entity_depth = 0;
        let limited = parse_document_with(source, &ParseOptions {
            entities: EntityPolicy::Resolve(values),
            limits,
        }).expect_err("entity depth budget");
        assert_eq!(limited.kind, Reason::Limit);
        assert!(limited.reason.contains("max_entity_depth (0)"));
    }

    #[test]
    fn entity_policy_rejects_external_parameter_cycles_and_keeps_resolve_inert() {
        let external = parse_document(
            "<!DOCTYPE r [<!ENTITY e SYSTEM 'file:///tmp/jet_xml_entity_probe'>]><r>&e;</r>",
        )
        .expect_err("external entity");
        assert_eq!(external.kind, Reason::Unsupported);
        assert!(external.reason.contains("external entity"));

        let parameter = parse_document("<!DOCTYPE r [<!ENTITY % pe 'x'>]><r/>").expect_err("parameter");
        assert_eq!(parameter.kind, Reason::Unsupported);
        assert!(parameter.reason.contains("parameter entity"));

        let pe_ref = parse_document("<!DOCTYPE r [<!ENTITY e '%pe;'>]><r/>").expect_err("pe ref");
        assert_eq!(pe_ref.kind, Reason::Unsupported);
        assert!(pe_ref.reason.contains("parameter entity"));

        let cycle = parse_document(
            "<!DOCTYPE r [<!ENTITY a '&b;'><!ENTITY b '&a;'>]><r>&a;</r>",
        )
        .expect_err("cycle");
        assert_eq!(cycle.kind, Reason::EntityCycle);

        let mut values = BTreeMap::new();
        values.insert("e".to_string(), "<x/>&y;".to_string());
        let tree = parse_document_with(
            "<!DOCTYPE r [<!ENTITY e 'ignored'>]><r>&e;</r>",
            &ParseOptions {
                entities: EntityPolicy::Resolve(values),
                limits: Limits::safe(),
            },
        )
        .expect("inert resolve");
        let Some(Value::Array(document_children)) = field(&tree, "children") else {
            panic!("document children")
        };
        let root = document_children
            .iter()
            .find(|node| matches!(field(node, "$xml"), Some(Value::Text(tag)) if tag == "element"))
            .expect("root");
        let Some(Value::Array(children)) = field(root, "children") else {
            panic!("root children")
        };
        assert_eq!(children.len(), 1);
        assert_eq!(field(&children[0], "$xml"), Some(&Value::Text("entity_ref".into())));
        assert_eq!(
            field(&children[0], "resolved_value"),
            Some(&Value::Text("<x/>&y;".into()))
        );
        assert!(
            !children.iter().any(|node| matches!(field(node, "$xml"), Some(Value::Text(tag)) if tag == "element")),
            "Resolve must not reparse replacement markup"
        );

        let with_system = parse_document(
            "<!DOCTYPE r SYSTEM 'file:///tmp/jet_xml_entity_probe'><r/>",
        )
        .expect("external subset id stays inert data");
        let Some(Value::Array(children)) = field(&with_system, "children") else {
            panic!("document children")
        };
        assert!(children.iter().any(|node| {
            matches!(field(node, "$xml"), Some(Value::Text(tag)) if tag == "doctype")
                && matches!(field(node, "system_id"), Some(Value::Text(id)) if id == "file:///tmp/jet_xml_entity_probe")
        }));
    }

    fn lex_chunks(bytes: &[u8], split: usize) -> Vec<(TokenKind, String, Vec<u8>)> {
        let mut lexer = ByteLexer::new(1024);
        lexer.push(&bytes[..split]).expect("first chunk");
        let mut tokens = Vec::new();
        while let Some(token) = lexer.next_token().expect("first tokens") {
            tokens.push((token.kind, token.text, token.raw_bytes));
        }
        lexer.push(&bytes[split..]).expect("second chunk");
        lexer.finish_input().expect("finish input");
        while let Some(token) = lexer.next_token().expect("remaining tokens") {
            tokens.push((token.kind, token.text, token.raw_bytes));
        }
        tokens
    }

    #[test]
    fn byte_lexer_is_chunk_invariant_and_preserves_utf8_bytes() {
        let bytes = b"\xef\xbb\xbf<r a='>'>h\xc3\xa9&amp;<!--x>y--><![CDATA[z>]]></r>";
        let expected = lex_chunks(bytes, bytes.len());
        for split in 0..=bytes.len() {
            assert_eq!(lex_chunks(bytes, split), expected, "split {split}");
        }
        let raw: Vec<u8> = expected.iter().flat_map(|(_, _, raw)| raw.clone()).collect();
        assert_eq!(raw, bytes[3..]);
    }

    #[test]
    fn byte_lexer_decodes_utf16_at_every_byte_boundary() {
        let source = "<?xml version='1.0'?><r>\u{1f642}&amp;</r>";
        for (encoding, bom, bytes) in [
            (WireEncoding::Utf16Le, vec![0xff, 0xfe], source.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>()),
            (WireEncoding::Utf16Be, vec![0xfe, 0xff], source.encode_utf16().flat_map(u16::to_be_bytes).collect::<Vec<_>>()),
        ] {
            let mut wire = bom.clone(); wire.extend(bytes);
            let expected = lex_chunks(&wire, wire.len());
            for split in 0..=wire.len() { assert_eq!(lex_chunks(&wire, split), expected, "{encoding:?} split {split}"); }
            assert_eq!(expected.iter().map(|(_, text, _)| text.as_str()).collect::<String>(), source);
        }
    }

    #[test]
    fn stream_bytes_validate_declaration_and_preserve_encoding_identity() {
        for (wire, bom, declared) in [
            (WireEncoding::UTF8, vec![0xef, 0xbb, 0xbf], "UTF-16"),
            (WireEncoding::Utf16Le, vec![0xff, 0xfe], "UTF-8"),
            (WireEncoding::Utf16Be, vec![0xfe, 0xff], "UTF-8"),
        ] {
            let source = format!("<?xml version='1.0' encoding='{declared}'?><r/>");
            let mut bytes = bom.clone();
            bytes.extend(wire_bytes(&source, wire));
            let mut scanner = StreamScanner::new(4096, ParseOptions::safe()).expect("scanner");
            scanner.push(&bytes).expect("input bytes");
            scanner.finish_input().expect("finish input");
            assert!(matches!(scanner.next().expect("document start"), Some(StreamEvent { event: Event::DocumentStart, .. })));
            let error = scanner.next().expect_err("conflicting XML declaration");
            assert_eq!(error.kind, Reason::InvalidEncoding, "{wire:?}");
            assert_eq!(error.offset, bom.len(), "{wire:?}");
        }

        for (wire, render, bom, declared) in [
            (WireEncoding::UTF8, RenderEncoding::UTF8Bom, vec![0xef, 0xbb, 0xbf], "UTF-8"),
            (WireEncoding::Utf16Le, RenderEncoding::Utf16Le, vec![0xff, 0xfe], "UTF-16"),
            (WireEncoding::Utf16Be, RenderEncoding::Utf16Be, vec![0xfe, 0xff], "UTF-16BE"),
        ] {
            let source = format!("<?xml version='1.0' encoding='{declared}'?><r>\u{1f642}</r>");
            let mut bytes = bom;
            bytes.extend(wire_bytes(&source, wire));
            let events = stream_values(&bytes, bytes.len());
            let mut writer = StreamWriter::new(render, LexicalPolicy::PreserveValid);
            let mut output = Vec::new();
            for event in &events {
                output.extend(writer.write(event).expect("write matching encoded event"));
            }
            assert_eq!(output, bytes, "{wire:?} byte identity");
        }
    }

    #[test]
    fn whole_bytes_share_stream_encoding_and_identity() {
        for (wire, render, bom, declared) in [
            (WireEncoding::UTF8, RenderEncoding::UTF8Bom, vec![0xef, 0xbb, 0xbf], "UTF-8"),
            (WireEncoding::Utf16Le, RenderEncoding::Utf16Le, vec![0xff, 0xfe], "UTF-16"),
            (WireEncoding::Utf16Be, RenderEncoding::Utf16Be, vec![0xfe, 0xff], "UTF-16BE"),
        ] {
            let source = format!("<?xml version='1.0' encoding='{declared}'?><r>\u{e9}\u{1f642}</r>");
            let mut bytes = bom;
            bytes.extend(wire_bytes(&source, wire));
            let value = parse_document_bytes(&bytes).expect("whole byte parse");
            assert_eq!(
                render_document_bytes(&value, render, LexicalPolicy::PreserveValid).expect("whole byte render"),
                bytes,
                "{wire:?} byte identity",
            );
        }

        let source = "<?xml version='1.0' encoding='UTF-8'?><r/>";
        let mut conflict = vec![0xff, 0xfe];
        conflict.extend(wire_bytes(source, WireEncoding::Utf16Le));
        let error = parse_document_bytes(&conflict).expect_err("conflicting declaration");
        assert_eq!(error.kind, Reason::InvalidEncoding);
        assert_eq!(error.offset, 2);
    }

    #[test]
    fn byte_lexer_fuses_encoding_and_item_limit_errors() {
        let mut invalid = ByteLexer::new(32);
        invalid.push(&[0xef, 0xbb, 0xbf, 0xff]).expect_err("invalid UTF-8");
        let first = invalid.next_token().expect_err("terminal error");
        assert_eq!(invalid.push(b"<r/>").expect_err("fused"), first);

        let mut continuation = ByteLexer::new(32);
        let first = continuation.push(&[0xc2, 0x20, b'<']).expect_err("invalid continuation");
        assert_eq!(continuation.finish_input().expect_err("fused"), first);

        let mut limited = ByteLexer::new(3);
        let first = limited.push(b"<root>").expect_err("item limit");
        assert_eq!(first.kind, Reason::Limit);
        assert_eq!(limited.finish_input().expect_err("fused"), first);
    }

    fn stream_events(bytes: &[u8], split: usize) -> Vec<Event> {
        let mut scanner=StreamScanner::new(4096,ParseOptions::safe()).expect("scanner");
        scanner.push(&bytes[..split]).expect("first");
        let mut events=Vec::new();while let Some(event)=scanner.next().expect("events"){events.push(event.event)}
        scanner.push(&bytes[split..]).expect("second");scanner.finish_input().expect("finish");
        while let Some(event)=scanner.next().expect("events"){events.push(event.event)}events
    }

    #[test]
    fn stream_scanner_preserves_semantics_across_every_byte_split() {
        let bytes=b"\xef\xbb\xbf<?xml version='1.0'?><r xmlns:p='u'>x&amp;<p:c/></r>\n";
        let expected=stream_events(bytes,bytes.len());
        for split in 0..=bytes.len(){assert_eq!(stream_events(bytes,split),expected,"split {split}")}
        assert!(matches!(&expected[2],Event::ElementStart{name,..} if name.local=="r"));
        assert!(matches!(&expected[5],Event::ElementStart{name,..} if name.namespace_uri.as_deref()==Some("u")));
        assert!(matches!(expected.last(),Some(Event::DocumentEnd)));
    }

    fn stream_values(bytes: &[u8], split: usize) -> Vec<Value> {
        let mut scanner = StreamScanner::new(4096, ParseOptions::safe()).expect("scanner");
        let mut values = Vec::new();
        scanner.push(&bytes[..split]).expect("first chunk");
        while let Some(event) = scanner.next().expect("first events") { values.push(stream_event_value(event)); }
        scanner.push(&bytes[split..]).expect("second chunk");
        scanner.finish_input().expect("finish input");
        while let Some(event) = scanner.next().expect("last events") { values.push(stream_event_value(event)); }
        values
    }

    #[test]
    fn stream_writer_preserves_every_event_across_every_input_split() {
        let source = b"\xef\xbb\xbf<?xml version='1.0' encoding='UTF-8'?><r xmlns:p='urn:p' p:z='1'>x&amp;<![CDATA[<y>]]><!--c--><?go now?><p:e/></r>";
        for split in 0..=source.len() {
            let mut writer = StreamWriter::new(RenderEncoding::UTF8Bom, LexicalPolicy::PreserveValid);
            let mut output = Vec::new();
            for event in stream_values(source, split) { output.extend(writer.write(&event).expect("write event")); }
            assert!(writer.is_finished());
            assert_eq!(output, source, "split {split}");
        }
    }

    #[test]
    fn stream_writer_rejects_hostile_raw_byte_evidence() {
        let source = b"<r  a='x'/>";
        for case in ["negative byte", "oversized byte", "extra key", "both raw forms"] {
            let mut events = stream_values(source, source.len());
            let Value::Object(event) = &mut events[1] else { panic!("element_start event") };
            let Some((_, Value::Object(lexical))) =
                event.iter_mut().find(|(key, _)| key == "open_lexical")
            else {
                panic!("open_lexical object")
            };
            match case {
                "negative byte" | "oversized byte" => {
                    let value = if case == "negative byte" { -1 } else { 256 };
                    lexical.iter_mut().find(|(key, _)| key == "raw_bytes").unwrap().1 =
                        Value::Array(vec![Value::Int(value)]);
                }
                "extra key" => lexical.push(("extra".to_string(), Value::Null)),
                "both raw forms" => {
                    lexical.iter_mut().find(|(key, _)| key == "raw_text").unwrap().1 =
                        Value::Text("<forged/>".to_string());
                }
                _ => unreachable!(),
            }

            let mut writer =
                StreamWriter::new(RenderEncoding::UTF8, LexicalPolicy::PreserveValid);
            let mut output = Vec::new();
            for event in &events {
                output.extend(writer.write(event).expect("write hostile event deterministically"));
            }
            assert_eq!(output, b"<r a='x'/>", "{case}");
            assert_ne!(output, source, "{case} replayed forged raw bytes");
        }
    }

    #[test]
    fn stream_writer_emits_selected_encoding_and_rejects_state_without_output() {
        let source = b"<r a='x'>z</r>";
        let events = stream_values(source, source.len());
        for (encoding, bom, body) in [
            (RenderEncoding::UTF8, Vec::new(), source.to_vec()),
            (RenderEncoding::Utf16Le, vec![0xff, 0xfe], String::from_utf8(source.to_vec()).unwrap().encode_utf16().flat_map(u16::to_le_bytes).collect()),
            (RenderEncoding::Utf16Be, vec![0xfe, 0xff], String::from_utf8(source.to_vec()).unwrap().encode_utf16().flat_map(u16::to_be_bytes).collect()),
        ] {
            let mut writer = StreamWriter::new(encoding, LexicalPolicy::Deterministic);
            let mut output = Vec::new();
            for event in &events { output.extend(writer.write(event).expect("encoded event")); }
            let mut expected = bom; expected.extend(body);
            assert_eq!(output, expected);
        }
        let mut writer = StreamWriter::new(RenderEncoding::UTF8, LexicalPolicy::Deterministic);
        let before = writer.clone();
        let error = writer.write(&events[1]).expect_err("missing document start");
        assert!(error.reason.contains("document_start"));
        assert_eq!(writer.started, before.started);
        assert_eq!(writer.stack, before.stack);
    }

    #[test]
    fn xml_10_namespaces_closed_tree_algebra_for_parse_surfaces() {
        // Criterion 1 corpus: XML 1.0 Fifth Edition + Namespaces 1.0 into the
        // closed DataTree algebra across parse / parse_with / parse_bytes.
        let source = "<?xml version='1.0' encoding='UTF-8'?>\n<!--prolog-->\n<root xmlns='urn:r' xmlns:p='urn:p' a='1' p:b='2'>text<!--c--><![CDATA[<x>]]><?go now?><p:child/>&amp;tail</root>\n";
        let tree = parse_document(source).expect("parse");
        let with = parse_document_with(source, &ParseOptions::safe()).expect("parse_with");
        let bytes = parse_document_bytes(source.as_bytes()).expect("parse_bytes");
        assert_eq!(field(&tree, "$xml"), Some(&Value::Text("document".into())));
        assert_eq!(strip_lexical(&tree), strip_lexical(&with));
        assert_eq!(
            strip_lexical(field(&tree, "children").expect("string children")),
            strip_lexical(field(&bytes, "children").expect("byte children"))
        );
        assert_eq!(field(&bytes, "encoding"), Some(&Value::Text("UTF-8".into())));
        assert_eq!(field(&bytes, "bom"), Some(&Value::Array(Vec::new())));

        let Value::Array(children) = field(&tree, "children").cloned().expect("children") else {
            panic!("children array");
        };
        assert_eq!(
            children
                .iter()
                .map(|node| required_text(node, "$xml").expect("tag"))
                .collect::<Vec<_>>(),
            vec![
                "declaration",
                "document_whitespace",
                "comment",
                "document_whitespace",
                "element",
                "document_whitespace",
            ]
        );
        assert_eq!(
            field(&children[1], "value"),
            Some(&Value::Text("\n".into()))
        );

        let root = children
            .iter()
            .find(|node| matches!(field(node, "$xml"), Some(Value::Text(tag)) if tag == "element"))
            .expect("root");
        let (raw, prefix, local, uri) = expanded_name_parts(root).expect("expanded");
        assert_eq!((raw.as_str(), prefix, local.as_str(), uri.as_deref()), ("root", None, "root", Some("urn:r")));
        let Value::Array(namespaces) = field(root, "namespaces").cloned().expect("ns") else {
            panic!("namespaces");
        };
        assert_eq!(namespaces.len(), 2);
        let Value::Array(attributes) = field(root, "attributes").cloned().expect("attrs") else {
            panic!("attributes");
        };
        assert_eq!(attributes.len(), 2);
        assert_eq!(
            (
                expanded_name_parts(&attributes[0]).expect("a").2.as_str(),
                field(&attributes[0], "normalized_value"),
            ),
            ("a", Some(&Value::Text("1".into())))
        );
        assert_eq!(
            expanded_name_parts(&attributes[1]).expect("b").3.as_deref(),
            Some("urn:p")
        );

        let Value::Array(content) = field(root, "children").cloned().expect("mixed") else {
            panic!("mixed content");
        };
        assert_eq!(
            content
                .iter()
                .map(|node| required_text(node, "$xml").expect("child tag"))
                .collect::<Vec<_>>(),
            vec![
                "text",
                "comment",
                "cdata",
                "processing_instruction",
                "element",
                "entity_ref",
                "text",
            ]
        );

        let dup = parse_document("<r xmlns:p='u' xmlns:q='u' p:a='1' q:a='2'/>")
            .expect_err("duplicate expanded attribute");
        assert_eq!(dup.kind, Reason::DuplicateAttribute);
        assert_eq!(dup.line, Some(1));
        assert!(dup.column.is_some());
        // Open-tag attribute errors locate before the element is stacked.
        assert_eq!(dup.path, "$");

        let mismatch = parse_document("<r>\n <a></b></r>").expect_err("mismatch");
        assert_eq!(
            (
                mismatch.kind,
                mismatch.offset,
                mismatch.line,
                mismatch.column,
                mismatch.path.as_str()
            ),
            (Reason::MismatchedTag, 8, Some(2), Some(5), "$/r/a")
        );

        assert_eq!(render_document(&tree).expect("round-trip"), source);
    }

    #[test]
    fn parse_bytes_identity_and_token_local_lexical_reuse() {
        let source = "<?xml version='1.0' encoding='UTF-8'?><r><!--keep--><![CDATA[stay]]>x</r>";
        let mut utf8_bom = vec![0xef, 0xbb, 0xbf];
        utf8_bom.extend(source.as_bytes());
        let mut tree = parse_document_bytes(&utf8_bom).expect("parse bom");
        let identity = render_document_bytes(&tree, RenderEncoding::UTF8Bom, LexicalPolicy::PreserveValid)
            .expect("identity");
        assert_eq!(identity, utf8_bom);

        // Edit one text token; unchanged sibling tokens must reuse raw_bytes.
        let Value::Object(ref mut doc_entries) = tree else {
            panic!("document object");
        };
        let children_slot = doc_entries
            .iter_mut()
            .find(|(key, _)| key == "children")
            .map(|(_, value)| value)
            .expect("children");
        let Value::Array(children) = children_slot else {
            panic!("children array");
        };
        let root = children
            .iter_mut()
            .find(|node| matches!(field(node, "$xml"), Some(Value::Text(tag)) if tag == "element"))
            .expect("root");
        let Value::Object(ref mut root_entries) = root else {
            panic!("root object");
        };
        let content_slot = root_entries
            .iter_mut()
            .find(|(key, _)| key == "children")
            .map(|(_, value)| value)
            .expect("content");
        let Value::Array(content) = content_slot else {
            panic!("content array");
        };
        let text_node = content
            .iter_mut()
            .find(|node| matches!(field(node, "$xml"), Some(Value::Text(tag)) if tag == "text"))
            .expect("text");
        let Value::Object(text_entries) = text_node else {
            panic!("text object");
        };
        text_entries
            .iter_mut()
            .find(|(key, _)| key == "value")
            .expect("value")
            .1 = text("edited");
        let lexical = text_entries
            .iter_mut()
            .find(|(key, _)| key == "lexical")
            .map(|(_, value)| value)
            .expect("lexical");
        let Value::Object(lex_entries) = lexical else {
            panic!("lexical object");
        };
        // Drop raw_bytes evidence so only this token re-renders.
        lex_entries
            .iter_mut()
            .find(|(key, _)| key == "raw_bytes")
            .expect("raw_bytes")
            .1 = Value::Null;
        lex_entries
            .iter_mut()
            .find(|(key, _)| key == "semantic")
            .expect("semantic")
            .1 = object(vec![("value", text("edited"))]);

        let out = render_document_bytes(&tree, RenderEncoding::UTF8Bom, LexicalPolicy::PreserveValid)
            .expect("token edit");
        assert_ne!(out, utf8_bom);
        let out_text = String::from_utf8(out[3..].to_vec()).expect("utf8 body");
        assert!(out_text.contains("<!--keep-->"));
        assert!(out_text.contains("<![CDATA[stay]]>"));
        assert!(out_text.contains(">edited</r>"));
        assert!(!out_text.contains(">x</r>"));

        // Encoding family conflict rejects before identity reuse.
        let mut conflict = vec![0xff, 0xfe];
        conflict.extend(wire_bytes(
            "<?xml version='1.0' encoding='UTF-8'?><r/>",
            WireEncoding::Utf16Le,
        ));
        let error = parse_document_bytes(&conflict).expect_err("conflict");
        assert_eq!(error.kind, Reason::InvalidEncoding);

        // Unsupported encoding signature without BOM stays UTF-8 only when ASCII XML.
        let latin = b"<?xml version='1.0' encoding='ISO-8859-1'?><r/>";
        let error = parse_document_bytes(latin).expect_err("latin1");
        assert_eq!(error.kind, Reason::InvalidEncoding);

        // UTF-16LE/BE identity under matching render encoding.
        for (bom, wire, render, declared) in [
            (
                vec![0xff, 0xfe],
                WireEncoding::Utf16Le,
                RenderEncoding::Utf16Le,
                "UTF-16",
            ),
            (
                vec![0xfe, 0xff],
                WireEncoding::Utf16Be,
                RenderEncoding::Utf16Be,
                "UTF-16BE",
            ),
        ] {
            let text_src = format!("<?xml version='1.0' encoding='{declared}'?><r/>");
            let mut bytes = bom.clone();
            bytes.extend(wire_bytes(&text_src, wire));
            let doc = parse_document_bytes(&bytes).expect("utf16 parse");
            assert_eq!(
                render_document_bytes(&doc, render, LexicalPolicy::PreserveValid).expect("utf16 render"),
                bytes
            );
        }
    }

    #[test]
    fn canonical_xml_w3c_whole_document_corpus() {
        // Provenance: tests/data/xml/c14n/PROVENANCE.md — hand-built from
        // W3C C14N 1.1 + Exclusive C14N 1.0 for whole-document node-sets.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/xml/c14n");
        let cases = [
            (
                "incl11_sort_escape",
                CanonicalOptions {
                    mode: CanonicalMode::Inclusive11,
                    comments: false,
                    inclusive_prefixes: vec![],
                },
            ),
            (
                "incl11_with_comments",
                CanonicalOptions {
                    mode: CanonicalMode::Inclusive11,
                    comments: true,
                    inclusive_prefixes: vec![],
                },
            ),
            (
                "excl10_omit_unused",
                CanonicalOptions {
                    mode: CanonicalMode::Exclusive10,
                    comments: false,
                    inclusive_prefixes: vec![],
                },
            ),
            (
                "excl10_inclusive_prefix",
                CanonicalOptions {
                    mode: CanonicalMode::Exclusive10,
                    comments: false,
                    inclusive_prefixes: vec!["b".to_string()],
                },
            ),
            (
                "excl10_with_comments",
                CanonicalOptions {
                    mode: CanonicalMode::Exclusive10,
                    comments: true,
                    inclusive_prefixes: vec![],
                },
            ),
        ];
        for (stem, options) in cases {
            let input = fs::read_to_string(root.join(format!("{stem}.in.xml"))).expect(stem);
            let expected = fs::read_to_string(root.join(format!("{stem}.out.xml"))).expect(stem);
            let tree = parse_document(input.trim_end_matches('\n')).unwrap_or_else(|e| {
                panic!("{stem} parse: {e:?}")
            });
            let got = canonical_document(&tree, &options).unwrap_or_else(|e| {
                panic!("{stem} canonical: {e:?}")
            });
            assert_eq!(got, expected.trim_end_matches('\n'), "{stem}");
        }

        let tree = parse_document("<r xmlns:q='urn:q' xmlns:p='urn:p' q:z='2' a='1'><e/></r>")
            .expect("reject options tree");
        let err = canonical_document(
            &tree,
            &CanonicalOptions {
                mode: CanonicalMode::Inclusive11,
                comments: false,
                inclusive_prefixes: vec!["p".to_string()],
            },
        )
        .expect_err("inclusive_prefixes illegal for Inclusive11");
        assert_eq!(err.kind, Reason::Shape);

        let relative = parse_document("<r xmlns:p='relative'><p:e/></r>").expect("relative");
        let err = canonical_document(
            &relative,
            &CanonicalOptions {
                mode: CanonicalMode::Exclusive10,
                comments: false,
                inclusive_prefixes: vec![],
            },
        )
        .expect_err("relative NS");
        assert_eq!(err.kind, Reason::Namespace);

        let unresolved = parse_document("<!DOCTYPE r [<!ENTITY e 'x'>]><r>&e;</r>").expect("entity");
        // Preserve leaves unresolved_value Null → Canonicalization reject.
        let err = canonical_document(
            &unresolved,
            &CanonicalOptions {
                mode: CanonicalMode::Inclusive11,
                comments: false,
                inclusive_prefixes: vec![],
            },
        )
        .expect_err("unresolved entity");
        assert_eq!(err.kind, Reason::Canonicalization);

        let not_doc = object(vec![
            ("$xml", text("element")),
            (
                "name",
                object(vec![
                    ("raw", text("r")),
                    ("prefix", Value::Null),
                    ("local", text("r")),
                    ("namespace_uri", Value::Null),
                ]),
            ),
            ("namespaces", Value::Array(vec![])),
            ("attributes", Value::Array(vec![])),
            ("children", Value::Array(vec![])),
            ("empty_style", text("empty")),
            ("open_lexical", Value::Null),
            ("close_lexical", Value::Null),
        ]);
        let err = canonical_document(
            &not_doc,
            &CanonicalOptions {
                mode: CanonicalMode::Inclusive11,
                comments: false,
                inclusive_prefixes: vec![],
            },
        )
        .expect_err("non-document");
        assert_eq!(err.kind, Reason::Shape);
    }

    #[test]
    fn fold_unfold_inverses_and_hostile_event_algebra() {
        let source = "<?xml version='1.0' encoding='UTF-8'?>\n<!--c-->\n<root xmlns:p='urn:p' a='1'>x&amp;<![CDATA[<y>]]><?go now?><p:e/></root>\n";
        let tree = parse_document_bytes(source.as_bytes()).expect("parse bytes");
        let events = unfold_events(&tree).expect("unfold");
        assert_eq!(
            required_text(&events[0], "$xml_event").expect("start"),
            "document_start"
        );
        assert_eq!(
            required_text(events.last().expect("end"), "$xml_event").expect("end tag"),
            "document_end"
        );
        let folded = fold_events(&events).expect("fold");
        assert_eq!(folded, tree);
        let re_unfolded = unfold_events(&folded).expect("re-unfold");
        assert_eq!(re_unfolded, events);

        // Hostile chunking: every split yields identical event algebra, and
        // fold(events) matches whole-byte parse.
        let bytes = source.as_bytes();
        let expected = stream_values(bytes, bytes.len());
        for split in 0..=bytes.len() {
            let chunked = stream_values(bytes, split);
            assert_eq!(chunked, expected, "hostile chunk split {split}");
        }
        assert_eq!(fold_events(&expected).expect("fold chunked"), tree);

        // Incomplete stream (no document_end).
        let incomplete: Vec<_> = expected[..expected.len() - 1].to_vec();
        let err = fold_events(&incomplete).expect_err("incomplete");
        assert_eq!(err.kind, Reason::Shape);
        assert!(err.reason.contains("complete document"));

        // Post-end event.
        let mut after_end = expected.clone();
        after_end.push(object(vec![("$xml_event", text("document_end"))]));
        let err = fold_events(&after_end).expect_err("post end");
        assert!(err.reason.contains("after document_end"));

        // Mismatched element_end expanded name.
        let mut mismatched = expected.clone();
        let end = mismatched
            .iter_mut()
            .find(|event| {
                matches!(
                    field(event, "$xml_event"),
                    Some(Value::Text(tag)) if tag == "element_end"
                )
            })
            .expect("element_end");
        let Value::Object(entries) = end else { panic!("object") };
        let name = entries
            .iter_mut()
            .find(|(key, _)| key == "name")
            .map(|(_, value)| value)
            .expect("name");
        *name = object(vec![
            ("raw", text("other")),
            ("prefix", Value::Null),
            ("local", text("other")),
            ("namespace_uri", Value::Null),
        ]);
        let err = fold_events(&mismatched).expect_err("mismatch");
        assert_eq!(err.kind, Reason::MismatchedTag);

        // Writer hostile order / terminal lifecycle.
        let mut writer = StreamWriter::new(RenderEncoding::UTF8, LexicalPolicy::Deterministic);
        let err = writer
            .write(&object(vec![
                ("$xml_event", text("document_end")),
            ]))
            .expect_err("end before start");
        assert!(err.reason.contains("document_start"));

        writer = StreamWriter::new(RenderEncoding::UTF8, LexicalPolicy::Deterministic);
        writer.write(&expected[0]).expect("document_start");
        let err = writer.write(&expected[0]).expect_err("duplicate start");
        assert!(err.reason.contains("duplicate"));

        writer = StreamWriter::new(RenderEncoding::UTF8, LexicalPolicy::Deterministic);
        for event in &expected {
            writer.write(event).expect("write complete");
        }
        assert!(writer.is_finished());
        let err = writer
            .write(&object(vec![("$xml_event", text("document_end"))]))
            .expect_err("post terminal");
        assert!(err.reason.contains("after document_end"));

        // Finish before document_end: writer not finished; incomplete stack.
        writer = StreamWriter::new(RenderEncoding::UTF8, LexicalPolicy::Deterministic);
        writer.write(&expected[0]).expect("start");
        // Skip to first element_start and write an explicit open without close.
        let start = expected
            .iter()
            .find(|event| {
                matches!(
                    field(event, "$xml_event"),
                    Some(Value::Text(tag)) if tag == "element_start"
                        && matches!(field(*event, "empty_style"), Some(Value::Text(style)) if style == "explicit")
                )
            })
            .expect("root start");
        writer.write(start).expect("open root");
        let err = writer
            .write(&object(vec![("$xml_event", text("document_end"))]))
            .expect_err("incomplete closure");
        assert!(err.reason.contains("closed root"));
        assert!(!writer.is_finished());
    }

    #[test]
    fn canonical_xml_sorts_and_normalizes_semantic_infoset() {
        let tree = parse_document("<?xml version='1.0'?><r xmlns:q='urn:q' xmlns:p='urn:p' q:z='2' a='x&#xA;y' p:a='1'><e/><!--c--><![CDATA[<&]]></r>").expect("tree");
        let without = canonical_document(&tree, &CanonicalOptions { mode: CanonicalMode::Inclusive11, comments: false, inclusive_prefixes: vec![] }).expect("canonical");
        assert_eq!(without, "<r xmlns:p=\"urn:p\" xmlns:q=\"urn:q\" a=\"x&#xA;y\" p:a=\"1\" q:z=\"2\"><e></e>&lt;&amp;</r>");
        let with = canonical_document(&tree, &CanonicalOptions { mode: CanonicalMode::Exclusive10, comments: true, inclusive_prefixes: vec!["p".to_string()] }).expect("exclusive");
        assert_eq!(with, "<r xmlns:p=\"urn:p\" xmlns:q=\"urn:q\" a=\"x&#xA;y\" p:a=\"1\" q:z=\"2\"><e></e><!--c-->&lt;&amp;</r>");
    }

    #[test]
    fn projection_helpers_follow_d_encxml_projection1_a() {
        let doc = parse_document(
            r#"<catalog xmlns:s="urn:shop"><s:book id="7"><title>Hi</title><s:price currency="USD">9</s:price><!--note--></s:book></catalog>"#,
        )
        .expect("parse");
        let root = document_root(&doc).expect("root");
        let (raw, prefix, local, uri) = expanded_name_parts(&root).expect("name");
        assert_eq!(raw, "catalog");
        assert_eq!(prefix, None);
        assert_eq!(local, "catalog");
        assert_eq!(uri, None);
        assert_eq!(lookup_attribute(&root, "missing").expect("attr"), None);

        let book = element_content(&root).expect("content")
            .into_iter()
            .find(|node| matches!(field(node, "$xml"), Some(Value::Text(tag)) if tag == "element"))
            .expect("book");
        let (_, _, book_local, book_uri) = expanded_name_parts(&book).expect("book name");
        assert_eq!(book_local, "book");
        assert_eq!(book_uri.as_deref(), Some("urn:shop"));
        assert_eq!(lookup_attribute(&book, "id").expect("id").as_deref(), Some("7"));
        assert_eq!(lookup_attribute(&book, "{urn:shop}id").expect("clark").as_deref(), None);

        let projected = project_document_for_decode(&doc).expect("project");
        let Value::Object(entries) = projected else { panic!("object") };
        assert!(entries.iter().any(|(key, _)| key == "{urn:shop}book"));
        let book_proj = entries.iter().find(|(key, _)| key == "{urn:shop}book").map(|(_, value)| value).expect("book proj");
        let Value::Object(book_entries) = book_proj else { panic!("book object") };
        assert!(book_entries.iter().any(|(key, value)| key == "@id" && matches!(value, Value::Text(text) if text == "7")));
        assert!(book_entries.iter().any(|(key, _)| key == "$content"));
        assert!(!book_entries.iter().any(|(key, _)| key == "$text"));
    }

    #[test]
    fn projection_maps_simple_children_and_price_for_codable() {
        let doc = parse_document(
            r#"<catalog><book id="7"><title>Hi</title><price currency="USD">9</price></book><book id="8"><title>Lo</title><price currency="EUR">3</price></book></catalog>"#,
        )
        .expect("parse");
        let projected = project_document_for_decode(&doc).expect("project");
        let Value::Object(entries) = projected else { panic!("object") };
        let book = match entries.iter().find(|(key, _)| key == "book").map(|(_, value)| value) {
            Some(Value::Array(items)) => &items[0],
            other => panic!("expected book array, got {other:?}"),
        };
        let Value::Object(book_entries) = book else { panic!("book object") };
        assert!(book_entries.iter().any(|(key, value)| key == "@id" && matches!(value, Value::Text(text) if text == "7")));
        assert!(book_entries.iter().any(|(key, value)| key == "title" && matches!(value, Value::Text(text) if text == "Hi")));
        let price = book_entries.iter().find(|(key, _)| key == "price").map(|(_, value)| value).expect("price");
        let Value::Object(price_entries) = price else { panic!("price object: {price:?}") };
        assert!(price_entries.iter().any(|(key, value)| key == "@currency" && matches!(value, Value::Text(text) if text == "USD")));
        assert!(price_entries.iter().any(|(key, value)| key == "$text" && matches!(value, Value::Text(text) if text == "9")));
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: Reason,
    pub offset: usize,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub path: String,
    pub reason: String,
}
impl Error {
    fn at(offset: usize, reason: impl Into<String>) -> Self {
        Self::at_kind(offset, Reason::Malformed, reason)
    }
    fn at_kind(offset: usize, kind: Reason, reason: impl Into<String>) -> Self {
        Self {
            kind,
            offset,
            line: None,
            column: None,
            path: "$".to_string(),
            reason: reason.into(),
        }
    }
    fn limit(reason: impl Into<String>) -> Self {
        // Constructor/source-less Limit errors keep empty path (D-ENCXML1/D-ENCSTREAM).
        Self {
            kind: Reason::Limit,
            offset: 0,
            line: None,
            column: None,
            path: String::new(),
            reason: reason.into(),
        }
    }
    fn located(mut self, source: &str, path: String) -> Self {
        let offset = self.offset.min(source.len());
        let prefix = &source[..offset];
        self.line = Some(prefix.chars().filter(|character| *character == '\n').count() + 1);
        self.column = Some(prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1);
        self.path = path;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    InvalidEncoding,
    Malformed,
    MismatchedTag,
    InvalidName,
    Namespace,
    DuplicateAttribute,
    Entity,
    EntityCycle,
    Limit,
    Canonicalization,
    Shape,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_attributes_per_element: usize,
    pub max_name_bytes: usize,
    pub max_text_bytes: usize,
    pub max_entity_declarations: usize,
    pub max_entity_depth: usize,
    pub max_entity_replacement_bytes: usize,
}

impl Limits {
    pub fn safe() -> Self {
        Self {
            max_depth: 256,
            max_nodes: 1_000_000,
            max_attributes_per_element: 1024,
            max_name_bytes: 4096,
            max_text_bytes: 16_777_216,
            max_entity_declarations: 1024,
            max_entity_depth: 32,
            max_entity_replacement_bytes: 8_388_608,
        }
    }

    pub fn validate(&self) -> Result<(), Error> {
        let checks = [
            ("max_depth", self.max_depth, 1, 4096),
            ("max_nodes", self.max_nodes, 1, 1_000_000_000),
            ("max_attributes_per_element", self.max_attributes_per_element, 0, 1_000_000),
            ("max_name_bytes", self.max_name_bytes, 1, 1_048_576),
            ("max_text_bytes", self.max_text_bytes, 0, 1_073_741_824),
            ("max_entity_declarations", self.max_entity_declarations, 0, 1_000_000),
            ("max_entity_depth", self.max_entity_depth, 0, 256),
            ("max_entity_replacement_bytes", self.max_entity_replacement_bytes, 0, 1_073_741_824),
        ];
        for (name, value, minimum, maximum) in checks {
            if !(minimum..=maximum).contains(&value) {
                return Err(Error::limit(format!(
                    "XML limit `{name}` must be between {minimum} and {maximum}"
                )));
            }
        }
        if self.max_entity_depth > self.max_depth {
            return Err(Error::limit("XML limit `max_entity_depth` exceeds `max_depth`"));
        }
        if self.max_entity_replacement_bytes > self.max_text_bytes {
            return Err(Error::limit(
                "XML limit `max_entity_replacement_bytes` exceeds `max_text_bytes`",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntityPolicy {
    Preserve,
    Reject,
    Resolve(BTreeMap<String, String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseOptions {
    pub entities: EntityPolicy,
    pub limits: Limits,
}

impl ParseOptions {
    pub fn safe() -> Self {
        Self {
            entities: EntityPolicy::Preserve,
            limits: Limits::safe(),
        }
    }
}

/// Wire encoding selected once from BOM/signature bytes. The incremental
/// lexer keeps each decoded scalar paired with its original bytes so stream
/// lexical evidence never requires retaining a document-sized source buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireEncoding { UTF8, Utf16Le, Utf16Be }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind { Text, Entity, Markup }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ByteToken {
    pub kind: TokenKind,
    pub text: String,
    pub raw_bytes: Vec<u8>,
    pub scalar_bytes: Vec<usize>,
}

#[derive(Clone, Debug)]
struct DecodedUnit { scalar: char, raw: Vec<u8> }

/// Owned, bounded, chunk-invariant XML byte lexer. It performs encoding/BOM
/// selection and XML-aware token framing (quotes, comments, CDATA, PI, and
/// DOCTYPE subsets) before the semantic Scanner consumes a token. No token is
/// split because an IO chunk ended; retained memory is one token plus at most
/// four undecoded bytes.
pub struct ByteLexer {
    encoding: Option<WireEncoding>,
    bom: Vec<u8>,
    undecided: Vec<u8>,
    pending: Vec<u8>,
    units: Vec<DecodedUnit>,
    max_item_bytes: usize,
    wire_offset: usize,
    eof: bool,
    terminal: Option<Error>,
}

impl ByteLexer {
    pub fn new(max_item_bytes: usize) -> Self {
        Self { encoding: None, bom: Vec::new(), undecided: Vec::new(), pending: Vec::new(), units: Vec::new(), max_item_bytes, wire_offset: 0, eof: false, terminal: None }
    }
    pub fn encoding(&self) -> Option<WireEncoding> { self.encoding }
    pub fn bom(&self) -> &[u8] { &self.bom }
    fn fail<T>(&mut self, kind: Reason, reason: impl Into<String>) -> Result<T, Error> {
        let error = Error::at_kind(self.wire_offset, kind, reason);
        self.terminal = Some(error.clone());
        Err(error)
    }
    fn choose_encoding(&mut self, final_input: bool) -> Result<(), Error> {
        if self.encoding.is_some() { return Ok(()); }
        if self.undecided.starts_with(&[0xef, 0xbb, 0xbf]) {
            self.encoding = Some(WireEncoding::UTF8); self.bom = self.undecided.drain(..3).collect(); self.wire_offset = 3;
        } else if self.undecided.starts_with(&[0xff, 0xfe]) {
            self.encoding = Some(WireEncoding::Utf16Le); self.bom = self.undecided.drain(..2).collect(); self.wire_offset = 2;
        } else if self.undecided.starts_with(&[0xfe, 0xff]) {
            self.encoding = Some(WireEncoding::Utf16Be); self.bom = self.undecided.drain(..2).collect(); self.wire_offset = 2;
        } else if self.undecided.len() >= 4 {
            self.encoding = Some(if self.undecided[..4] == [0, b'<', 0, b'?'] { WireEncoding::Utf16Be } else if self.undecided[..4] == [b'<', 0, b'?', 0] { WireEncoding::Utf16Le } else { WireEncoding::UTF8 });
        } else if final_input || self.undecided.len() >= 3 {
            self.encoding = Some(WireEncoding::UTF8);
        }
        if self.encoding.is_some() { self.pending.append(&mut self.undecided); }
        Ok(())
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if let Some(error) = &self.terminal { return Err(error.clone()); }
        if self.eof { return self.fail(Reason::Malformed, "XML bytes arrived after end of input"); }
        if self.encoding.is_none() { self.undecided.extend_from_slice(bytes); self.choose_encoding(false)?; } else { self.pending.extend_from_slice(bytes); }
        self.decode(false)
    }
    pub fn finish_input(&mut self) -> Result<(), Error> {
        if let Some(error) = &self.terminal { return Err(error.clone()); }
        self.eof = true;
        self.choose_encoding(true)?;
        self.decode(true)
    }
    fn retain(&mut self, scalar: char, raw: Vec<u8>) -> Result<(), Error> {
        let retained: usize = self.units.iter().map(|unit| unit.raw.len()).sum();
        if retained.saturating_add(raw.len()) > self.max_item_bytes { return self.fail(Reason::Limit, format!("XML token exceeds max_item_bytes ({})", self.max_item_bytes)); }
        self.wire_offset = self.wire_offset.saturating_add(raw.len());
        self.units.push(DecodedUnit { scalar, raw });
        Ok(())
    }
    fn decode(&mut self, final_input: bool) -> Result<(), Error> {
        let Some(encoding) = self.encoding else { return Ok(()); };
        loop {
            // Never append past an already-complete token. finish_input used to
            // glue the next scalar onto a finished Markup and strand Text crumbs.
            if self.boundary().is_some() {
                break;
            }
            match encoding {
                WireEncoding::UTF8 => {
                    let Some(&first) = self.pending.first() else { break };
                    let width = if first < 0x80 { 1 } else if first & 0xe0 == 0xc0 { 2 } else if first & 0xf0 == 0xe0 { 3 } else if first & 0xf8 == 0xf0 { 4 } else { return self.fail(Reason::InvalidEncoding, "invalid UTF-8 leading byte"); };
                    if self.pending.len() < width { break; }
                    let raw: Vec<u8> = self.pending.drain(..width).collect();
                    let text = match std::str::from_utf8(&raw) {
                        Ok(text) => text,
                        Err(_) => return self.fail(Reason::InvalidEncoding, "invalid UTF-8 sequence"),
                    };
                    let Some(scalar) = text.chars().next() else {
                        return self.fail(Reason::InvalidEncoding, "empty UTF-8 sequence");
                    };
                    self.retain(scalar, raw)?;
                }
                WireEncoding::Utf16Le | WireEncoding::Utf16Be => {
                    if self.pending.len() < 2 { break; }
                    let unit = if encoding == WireEncoding::Utf16Le { u16::from_le_bytes([self.pending[0], self.pending[1]]) } else { u16::from_be_bytes([self.pending[0], self.pending[1]]) };
                    let width = if (0xd800..=0xdbff).contains(&unit) { 4 } else { 2 };
                    if self.pending.len() < width { break; }
                    let raw: Vec<u8> = self.pending.drain(..width).collect();
                    let scalar = if width == 2 {
                        if (0xdc00..=0xdfff).contains(&unit) { return self.fail(Reason::InvalidEncoding, "unpaired UTF-16 low surrogate"); }
                        let Some(scalar) = char::from_u32(unit as u32) else { return self.fail(Reason::InvalidEncoding, "invalid UTF-16 scalar"); };
                        scalar
                    } else {
                        let low = if encoding == WireEncoding::Utf16Le { u16::from_le_bytes([raw[2], raw[3]]) } else { u16::from_be_bytes([raw[2], raw[3]]) };
                        if !(0xdc00..=0xdfff).contains(&low) { return self.fail(Reason::InvalidEncoding, "unpaired UTF-16 high surrogate"); }
                        let Some(scalar) = char::from_u32(0x10000 + (((unit - 0xd800) as u32) << 10) + (low - 0xdc00) as u32) else { return self.fail(Reason::InvalidEncoding, "invalid UTF-16 scalar"); };
                        scalar
                    };
                    self.retain(scalar, raw)?;
                }
            }
        }
        if final_input && self.boundary().is_none() && !self.pending.is_empty() { return self.fail(Reason::InvalidEncoding, "truncated encoded XML scalar"); }
        Ok(())
    }
    fn boundary(&self) -> Option<(usize, TokenKind)> {
        if self.units.is_empty() { return None; }
        let chars: Vec<char> = self.units.iter().map(|unit| unit.scalar).collect();
        if chars[0] == '&' { return chars.iter().position(|c| *c == ';').map(|i| (i + 1, TokenKind::Entity)); }
        if chars[0] != '<' {
            if let Some(index) = chars.iter().position(|c| *c == '<' || *c == '&') {
                return Some((index, TokenKind::Text));
            }
            // Close Text at EOF only when every pending wire byte is already a unit.
            // Otherwise decode would freeze one scalar at a time under eof=true.
            if self.eof && self.pending.is_empty() {
                return Some((chars.len(), TokenKind::Text));
            }
            return None;
        }
        let text: String = chars.iter().collect();
        for (lead, end) in [("<!--", "-->"), ("<![CDATA[", "]]>") , ("<?", "?>")] {
            if text.starts_with(lead) { return text.find(end).map(|i| (text[..i + end.len()].chars().count(), TokenKind::Markup)); }
        }
        let mut quote = None; let mut bracket = 0i32;
        for (index, scalar) in chars.iter().enumerate().skip(1) {
            if let Some(expected) = quote { if *scalar == expected { quote = None; } continue; }
            if *scalar == '\'' || *scalar == '"' { quote = Some(*scalar); }
            else if text.starts_with("<!DOCTYPE") && *scalar == '[' { bracket += 1; }
            else if text.starts_with("<!DOCTYPE") && *scalar == ']' { bracket -= 1; }
            else if *scalar == '>' && bracket == 0 { return Some((index + 1, TokenKind::Markup)); }
        }
        None
    }
    pub fn next_token(&mut self) -> Result<Option<ByteToken>, Error> {
        if let Some(error) = &self.terminal { return Err(error.clone()); }
        self.decode(self.eof)?;
        let Some((count, kind)) = self.boundary() else {
            if self.eof && !self.units.is_empty() { return self.fail(Reason::Malformed, "unterminated XML token at end of input"); }
            return Ok(None);
        };
        if count == 0 { return Ok(None); }
        let drained: Vec<_> = self.units.drain(..count).collect();
        let scalar_bytes = drained.iter().map(|unit| unit.raw.len()).collect();
        Ok(Some(ByteToken { kind, text: drained.iter().map(|unit| unit.scalar).collect(), raw_bytes: drained.into_iter().flat_map(|unit| unit.raw).collect(), scalar_bytes }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamEvent {
    pub event: Event,
    pub raw_bytes: Vec<u8>,
    pub encoding: WireEncoding,
    pub bom: Vec<u8>,
}

/// Persistent semantic projection over ByteLexer. Scanner remains the sole XML
/// grammar/namespace/limit engine; this adapter supplies one complete lexical
/// token at a time and transfers its owned semantic state between calls.
pub struct StreamScanner {
    lexer: ByteLexer,
    options: ParseOptions,
    started: bool,
    ended: bool,
    root_seen: bool,
    root_closed: bool,
    declaration_seen: bool,
    doctype_seen: bool,
    declared_entities: BTreeMap<String, DeclaredEntityKind>,
    stack: Vec<Frame>,
    nodes: usize,
    text_bytes: usize,
    entity_replacement_bytes: usize,
    wire_consumed: usize,
    line: usize,
    column: usize,
    terminal: Option<Error>,
}

impl StreamScanner {
    pub fn new(max_item_bytes: usize, options: ParseOptions) -> Result<Self, Error> {
        options.limits.validate()?;
        Ok(Self { lexer: ByteLexer::new(max_item_bytes), options, started: false, ended: false, root_seen: false, root_closed: false, declaration_seen: false, doctype_seen: false, declared_entities: BTreeMap::new(), stack: Vec::new(), nodes: 0, text_bytes: 0, entity_replacement_bytes: 0, wire_consumed: 0, line: 1, column: 1, terminal: None })
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), Error> { if let Some(error)=&self.terminal{return Err(error.clone())} self.lexer.push(bytes).map_err(|error|{self.terminal=Some(error.clone());error}) }
    pub fn finish_input(&mut self) -> Result<(), Error> { if let Some(error)=&self.terminal{return Err(error.clone())} self.lexer.finish_input().map_err(|error|{self.terminal=Some(error.clone());error}) }
    fn fail<T>(&mut self, mut error: Error) -> Result<T, Error> { if error.line.is_none(){error.line=Some(self.line);error.column=Some(self.column)} self.terminal=Some(error.clone());Err(error) }
    pub fn next(&mut self) -> Result<Option<StreamEvent>, Error> {
        if let Some(error)=&self.terminal{return Err(error.clone())}
        let Some(encoding)=self.lexer.encoding() else{return Ok(None)};
        if !self.started { self.started=true; self.nodes=1; self.wire_consumed=self.lexer.bom().len(); return Ok(Some(StreamEvent{event:Event::DocumentStart,raw_bytes:Vec::new(),encoding,bom:self.lexer.bom().to_vec()})); }
        let Some(token)=self.lexer.next_token().map_err(|error|{self.terminal=Some(error.clone());error})? else {
            if !self.lexer.eof{return Ok(None)}
            if self.ended{return Ok(None)}
            if !self.stack.is_empty(){return self.fail(Error::at_kind(self.wire_consumed,Reason::Malformed,"XML document ended before closing element"))}
            if !self.root_seen{return self.fail(Error::at_kind(self.wire_consumed,Reason::Malformed,"empty XML document"))}
            self.ended=true;
            return Ok(Some(StreamEvent{event:Event::DocumentEnd,raw_bytes:Vec::new(),encoding,bom:self.lexer.bom().to_vec()}));
        };
        if token.text.starts_with("<?xml") && self.wire_consumed != self.lexer.bom().len() { return self.fail(Error::at_kind(self.wire_consumed,Reason::Malformed,"XML declaration is out of order")); }
        let mut scanner=Scanner{source:&token.text,wire_encoding:Some(encoding),offset:0,started:true,ended:false,root_seen:self.root_seen,root_closed:self.root_closed,declaration_seen:self.declaration_seen,doctype_seen:self.doctype_seen,declared_entities:self.declared_entities.clone(),stack:self.stack.clone(),options:self.options.clone(),nodes:self.nodes,text_bytes:self.text_bytes,entity_replacement_bytes:self.entity_replacement_bytes};
        let event=match scanner.next(){Ok(Some(event))=>event,Ok(None)=>return self.fail(Error::at_kind(self.wire_consumed,Reason::Malformed,"XML token produced no event")),Err(mut error)=>{
            let scalar_count=token.text[..error.offset.min(token.text.len())].chars().count();
            error.offset=self.wire_consumed+token.scalar_bytes.iter().take(scalar_count).sum::<usize>();
            let local_line=error.line.unwrap_or(1);let local_column=error.column.unwrap_or(1);
            error.line=Some(self.line+local_line-1);
            error.column=Some(if local_line==1{self.column+local_column-1}else{local_column});
            return self.fail(error)
        }};
        self.root_seen=scanner.root_seen;self.root_closed=scanner.root_closed;self.declaration_seen=scanner.declaration_seen;self.doctype_seen=scanner.doctype_seen;self.declared_entities=scanner.declared_entities;self.stack=scanner.stack;self.nodes=scanner.nodes;self.text_bytes=scanner.text_bytes;self.entity_replacement_bytes=scanner.entity_replacement_bytes;
        self.wire_consumed+=token.raw_bytes.len();
        for scalar in token.text.chars(){if scalar=='\n'{self.line+=1;self.column=1}else{self.column+=1}}
        Ok(Some(StreamEvent{event,raw_bytes:token.raw_bytes,encoding,bom:self.lexer.bom().to_vec()}))
    }
}

#[derive(Clone)]
struct Frame {
    name: Name,
    namespaces: Vec<(Option<String>, String)>,
}
pub struct Scanner<'a> {
    source: &'a str,
    wire_encoding: Option<WireEncoding>,
    offset: usize,
    started: bool,
    ended: bool,
    root_seen: bool,
    root_closed: bool,
    declaration_seen: bool,
    doctype_seen: bool,
    declared_entities: BTreeMap<String, DeclaredEntityKind>,
    stack: Vec<Frame>,
    options: ParseOptions,
    nodes: usize,
    text_bytes: usize,
    entity_replacement_bytes: usize,
}

fn name_start(c: char) -> bool {
    matches!(c, ':'|'A'..='Z'|'_'|'a'..='z')
        || matches!(c as u32, 0xC0..=0xD6|0xD8..=0xF6|0xF8..=0x2FF|0x370..=0x37D|0x37F..=0x1FFF|0x200C..=0x200D|0x2070..=0x218F|0x2C00..=0x2FEF|0x3001..=0xD7FF|0xF900..=0xFDCF|0xFDF0..=0xFFFD|0x10000..=0xEFFFF)
}
fn name_char(c: char) -> bool {
    name_start(c)
        || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}')
        || matches!(c as u32, 0x0300..=0x036F|0x203F..=0x2040)
}
fn valid_name(s: &str) -> bool {
    let mut cs = s.chars();
    cs.next().is_some_and(name_start) && cs.all(name_char) && s.matches(':').count() <= 1
}
fn valid_xml_char(character: char) -> bool {
    matches!(
        character as u32,
        0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
    )
}
fn normalize_attribute_text(raw: &str) -> String {
    let mut normalized = String::with_capacity(raw.len());
    let mut characters = raw.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                normalized.push(' ');
            }
            '\n' | '\t' => normalized.push(' '),
            character => normalized.push(character),
        }
    }
    normalized
}
fn split_name(raw: &str) -> (Option<String>, String) {
    match raw.split_once(':') {
        Some((p, l)) => (Some(p.to_string()), l.to_string()),
        None => (None, raw.to_string()),
    }
}
fn predefined(name: &str) -> Option<String> {
    match name {
        "lt" => Some("<".into()),
        "gt" => Some(">".into()),
        "amp" => Some("&".into()),
        "apos" => Some("'".into()),
        "quot" => Some("\"".into()),
        _ if name.starts_with("#x") => u32::from_str_radix(&name[2..], 16)
            .ok()
            .and_then(char::from_u32)
            .filter(|character| valid_xml_char(*character))
            .map(|c| c.to_string()),
        _ if name.starts_with('#') => name[1..]
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .filter(|character| valid_xml_char(*character))
            .map(|c| c.to_string()),
        _ => None,
    }
}

fn take_doctype_literal(source: &str, cursor: &mut usize) -> Result<String, Error> {
    while source
        .as_bytes()
        .get(*cursor)
        .is_some_and(u8::is_ascii_whitespace)
    {
        *cursor += 1;
    }
    let quote = *source
        .as_bytes()
        .get(*cursor)
        .ok_or_else(|| Error::at(*cursor, "DOCTYPE external identifier lacks quoted value"))?;
    if quote != b'\'' && quote != b'"' {
        return Err(Error::at(
            *cursor,
            "DOCTYPE external identifier must be quoted",
        ));
    }
    *cursor += 1;
    let start = *cursor;
    while source
        .as_bytes()
        .get(*cursor)
        .is_some_and(|byte| *byte != quote)
    {
        *cursor += 1;
    }
    if *cursor == source.len() {
        return Err(Error::at(start, "unterminated DOCTYPE external identifier"));
    }
    let value = source[start..*cursor].to_string();
    *cursor += 1;
    Ok(value)
}

fn doctype_fields(
    inside: &str,
) -> Result<(String, Option<String>, Option<String>, Option<String>), Error> {
    let mut subset_start = None;
    let mut quote = None;
    for (offset, character) in inside.char_indices() {
        if let Some(expected) = quote {
            if character == expected {
                quote = None;
            }
        } else if character == '\'' || character == '"' {
            quote = Some(character);
        } else if character == '[' {
            subset_start = Some(offset);
            break;
        }
    }
    let (header, internal_subset) = match subset_start {
        Some(start) => {
            let tail = inside[start + 1..].trim_end();
            let subset = tail
                .strip_suffix(']')
                .ok_or_else(|| Error::at(start, "DOCTYPE internal subset lacks ]"))?;
            (inside[..start].trim(), Some(subset.to_string()))
        }
        None => (inside.trim(), None),
    };
    let name_end = header.find(char::is_whitespace).unwrap_or(header.len());
    let name = header[..name_end].to_string();
    if !valid_name(&name) {
        return Err(Error::at(0, "invalid DOCTYPE name"));
    }
    let external = header[name_end..].trim();
    if external.is_empty() {
        return Ok((name, None, None, internal_subset));
    }
    let mut cursor = if external.starts_with("SYSTEM") {
        "SYSTEM".len()
    } else if external.starts_with("PUBLIC") {
        "PUBLIC".len()
    } else {
        return Err(Error::at(name_end, "invalid DOCTYPE external identifier"));
    };
    let public = if external.starts_with("PUBLIC") {
        Some(take_doctype_literal(external, &mut cursor)?)
    } else {
        None
    };
    let system = Some(take_doctype_literal(external, &mut cursor)?);
    if !external[cursor..].trim().is_empty() {
        return Err(Error::at(
            cursor,
            "unexpected data after DOCTYPE identifier",
        ));
    }
    Ok((name, public, system, internal_subset))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeclaredEntityKind {
    Internal,
    External,
}

fn take_entity_literal(source: &str, cursor: &mut usize) -> Result<String, Error> {
    while source
        .as_bytes()
        .get(*cursor)
        .is_some_and(u8::is_ascii_whitespace)
    {
        *cursor += 1;
    }
    let quote = *source
        .as_bytes()
        .get(*cursor)
        .ok_or_else(|| Error::at(*cursor, "ENTITY declaration lacks quoted value"))?;
    if quote != b'\'' && quote != b'"' {
        return Err(Error::at(
            *cursor,
            "ENTITY declaration value must be quoted",
        ));
    }
    *cursor += 1;
    let start = *cursor;
    while source
        .as_bytes()
        .get(*cursor)
        .is_some_and(|byte| *byte != quote)
    {
        *cursor += 1;
    }
    if *cursor == source.len() {
        return Err(Error::at(start, "unterminated ENTITY declaration value"));
    }
    let value = source[start..*cursor].to_string();
    *cursor += 1;
    Ok(value)
}

fn reject_parameter_entity_markup(subset: &str, base: usize) -> Result<(), Error> {
    let mut index = 0usize;
    while index < subset.len() {
        if subset.as_bytes()[index] == b'%' {
            let name_begin = index + 1;
            let mut name_end = name_begin;
            while name_end < subset.len() {
                let next = subset[name_end..].chars().next().unwrap();
                if name_end == name_begin {
                    if !name_start(next) {
                        break;
                    }
                } else if !name_char(next) {
                    break;
                }
                name_end += next.len_utf8();
            }
            if name_end > name_begin && subset.as_bytes().get(name_end) == Some(&b';') {
                return Err(Error::at_kind(
                    base + index,
                    Reason::Unsupported,
                    "parameter entity references are unsupported",
                ));
            }
        }
        index += subset[index..].chars().next().unwrap().len_utf8();
    }
    Ok(())
}

fn general_entity_refs(value: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let mut at = 0usize;
    while at < value.len() {
        let Some(rel) = value[at..].find('&') else {
            break;
        };
        let start = at + rel;
        let Some(end) = value[start..].find(';') else {
            break;
        };
        let name = &value[start + 1..start + end];
        if !name.is_empty()
            && !name.starts_with('#')
            && predefined(name).is_none()
            && valid_name(name)
        {
            refs.push(name.to_string());
        }
        at = start + end + 1;
    }
    refs
}

fn entity_reference_graph_has_cycle(edges: &BTreeMap<String, Vec<String>>) -> bool {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Color {
        White,
        Gray,
        Black,
    }
    let mut color = BTreeMap::new();
    for name in edges.keys() {
        color.insert(name.clone(), Color::White);
    }
    fn visit(
        node: &str,
        edges: &BTreeMap<String, Vec<String>>,
        color: &mut BTreeMap<String, Color>,
    ) -> bool {
        color.insert(node.to_string(), Color::Gray);
        if let Some(targets) = edges.get(node) {
            for target in targets {
                match color.get(target).copied().unwrap_or(Color::White) {
                    Color::Gray => return true,
                    Color::White => {
                        if visit(target, edges, color) {
                            return true;
                        }
                    }
                    Color::Black => {}
                }
            }
        }
        color.insert(node.to_string(), Color::Black);
        false
    }
    for name in edges.keys() {
        if color.get(name) == Some(&Color::White) && visit(name, edges, &mut color) {
            return true;
        }
    }
    false
}

fn declared_entity_table(
    subset: Option<&str>,
) -> Result<(BTreeMap<String, DeclaredEntityKind>, usize), Error> {
    let mut names = BTreeMap::new();
    let mut replacements = BTreeMap::new();
    let mut count = 0usize;
    let Some(subset) = subset else {
        return Ok((names, count));
    };
    reject_parameter_entity_markup(subset, 0)?;
    let mut search_from = 0usize;
    while let Some(rel) = subset[search_from..].find("<!ENTITY") {
        let start = search_from + rel;
        count = count.saturating_add(1);
        let mut cursor = start + "<!ENTITY".len();
        while subset
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            cursor += 1;
        }
        if subset.as_bytes().get(cursor) == Some(&b'%') {
            return Err(Error::at_kind(
                start,
                Reason::Unsupported,
                "parameter entity declarations are unsupported",
            ));
        }
        let name_begin = cursor;
        while cursor < subset.len() {
            let next = subset[cursor..].chars().next().unwrap();
            if cursor == name_begin {
                if !name_start(next) {
                    break;
                }
            } else if !name_char(next) {
                break;
            }
            cursor += next.len_utf8();
        }
        if cursor == name_begin {
            return Err(Error::at(start, "ENTITY declaration lacks name"));
        }
        let name = &subset[name_begin..cursor];
        if !valid_name(name) {
            return Err(Error::at(start, "invalid ENTITY declaration name"));
        }
        while subset
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            cursor += 1;
        }
        let kind = if subset[cursor..].starts_with("SYSTEM")
            || subset[cursor..].starts_with("PUBLIC")
        {
            let external_start = cursor;
            if subset[cursor..].starts_with("PUBLIC") {
                cursor += "PUBLIC".len();
                take_entity_literal(subset, &mut cursor)?;
            } else {
                cursor += "SYSTEM".len();
            }
            take_entity_literal(subset, &mut cursor)?;
            while subset
                .as_bytes()
                .get(cursor)
                .is_some_and(u8::is_ascii_whitespace)
            {
                cursor += 1;
            }
            if subset[cursor..].starts_with("NDATA") {
                return Err(Error::at_kind(
                    start,
                    Reason::Unsupported,
                    "unparsed external entities are unsupported",
                ));
            }
            let _ = external_start;
            DeclaredEntityKind::External
        } else if subset.as_bytes().get(cursor) == Some(&b'\'')
            || subset.as_bytes().get(cursor) == Some(&b'"')
        {
            let value = take_entity_literal(subset, &mut cursor)?;
            replacements.insert(name.to_string(), value);
            DeclaredEntityKind::Internal
        } else {
            return Err(Error::at(start, "ENTITY declaration lacks value"));
        };
        if names.insert(name.to_string(), kind).is_some() {
            return Err(Error::at(start, "duplicate ENTITY declaration"));
        }
        search_from = start + "<!ENTITY".len();
    }
    let mut edges = BTreeMap::new();
    for (name, value) in &replacements {
        let targets = general_entity_refs(value)
            .into_iter()
            .filter(|target| replacements.contains_key(target))
            .collect::<Vec<_>>();
        edges.insert(name.clone(), targets);
    }
    if entity_reference_graph_has_cycle(&edges) {
        return Err(Error::at_kind(
            0,
            Reason::EntityCycle,
            "XML entity declarations form a replacement cycle",
        ));
    }
    Ok((names, count))
}

fn declaration_fields(data: &str) -> Result<(String, Option<String>, Option<bool>), Error> {
    let mut cursor = 0;
    let mut fields = Vec::new();
    while cursor < data.len() {
        while data.as_bytes()[cursor].is_ascii_whitespace() {
            cursor += 1;
            if cursor == data.len() {
                break;
            }
        }
        if cursor == data.len() {
            break;
        }
        let key_start = cursor;
        while cursor < data.len()
            && !data.as_bytes()[cursor].is_ascii_whitespace()
            && data.as_bytes()[cursor] != b'='
        {
            cursor += 1;
        }
        let key = &data[key_start..cursor];
        while cursor < data.len() && data.as_bytes()[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if data.as_bytes().get(cursor) != Some(&b'=') {
            return Err(Error::at(cursor, "XML declaration field needs ="));
        }
        cursor += 1;
        let value = take_doctype_literal(data, &mut cursor)?;
        fields.push((key, value));
    }
    if fields.first().map(|(key, _)| *key) != Some("version") {
        return Err(Error::at(0, "XML declaration must start with version"));
    }
    let version = fields[0].1.clone();
    if version != "1.0" {
        return Err(Error::at(0, "only XML version 1.0 is supported"));
    }
    let mut encoding = None;
    let mut standalone = None;
    for (key, value) in fields.into_iter().skip(1) {
        match key {
            "encoding" if encoding.is_none() && standalone.is_none() => {
                if !matches!(value.to_ascii_uppercase().as_str(), "UTF-8" | "UTF-16" | "UTF-16LE" | "UTF-16BE") {
                    return Err(Error::at_kind(0, Reason::InvalidEncoding, "unsupported XML declaration encoding"));
                }
                encoding = Some(value);
            }
            "standalone" if standalone.is_none() => match value.as_str() {
                "yes" => standalone = Some(true),
                "no" => standalone = Some(false),
                _ => return Err(Error::at(0, "standalone must be yes or no")),
            },
            _ => return Err(Error::at(0, "invalid or duplicate XML declaration field")),
        }
    }
    Ok((version, encoding, standalone))
}

fn declaration_matches_input(name: &str, encoding: Option<WireEncoding>) -> bool {
    match encoding.unwrap_or(WireEncoding::UTF8) {
        WireEncoding::UTF8 => name.eq_ignore_ascii_case("UTF-8"),
        WireEncoding::Utf16Le | WireEncoding::Utf16Be => matches!(
            name.to_ascii_uppercase().as_str(),
            "UTF-16" | "UTF-16LE" | "UTF-16BE"
        ),
    }
}

impl<'a> Scanner<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            wire_encoding: None,
            offset: 0,
            started: false,
            ended: false,
            root_seen: false,
            root_closed: false,
            declaration_seen: false,
            doctype_seen: false,
            declared_entities: BTreeMap::new(),
            stack: Vec::new(),
            options: ParseOptions::safe(),
            nodes: 0,
            text_bytes: 0,
            entity_replacement_bytes: 0,
        }
    }
    pub fn with_options(source: &'a str, options: ParseOptions) -> Result<Self, Error> {
        options.limits.validate()?;
        Ok(Self {
            options,
            ..Self::new(source)
        })
    }
    fn path(&self) -> String {
        let mut path = String::from("$");
        for frame in &self.stack {
            path.push('/');
            if let Some(uri) = &frame.name.namespace_uri {
                path.push('{');
                path.push_str(uri);
                path.push('}');
            }
            path.push_str(&frame.name.local);
        }
        path
    }
    fn classify(error: &mut Error) {
        if error.kind != Reason::Malformed {
            return;
        }
        let reason = error.reason.as_str();
        error.kind = if reason.contains("encoding") || reason.contains("version 1.0") {
            Reason::InvalidEncoding
        } else if reason.contains("mismatched") {
            Reason::MismatchedTag
        } else if reason.contains("name") || reason.contains("instruction target") {
            Reason::InvalidName
        } else if reason.contains("namespace") || reason.contains("prefix") {
            Reason::Namespace
        } else if reason.contains("duplicate expanded XML attribute") {
            Reason::DuplicateAttribute
        } else if reason.contains("entity") || reason.contains("character reference") {
            Reason::Entity
        } else {
            error.kind
        };
    }
    fn limit<T>(&self, reason: impl Into<String>) -> Result<T, Error> {
        Err(Error::at_kind(self.offset, Reason::Limit, reason))
    }
    fn check_name(&self, value: &str, label: &str) -> Result<(), Error> {
        if value.len() > self.options.limits.max_name_bytes {
            return self.limit(format!(
                "XML {label} exceeds max_name_bytes ({})",
                self.options.limits.max_name_bytes
            ));
        }
        Ok(())
    }
    fn charge_text(&mut self, bytes: usize, label: &str) -> Result<(), Error> {
        if bytes > self.options.limits.max_text_bytes
            || self.text_bytes.saturating_add(bytes) > self.options.limits.max_text_bytes
        {
            return self.limit(format!(
                "XML {label} exceeds max_text_bytes ({})",
                self.options.limits.max_text_bytes
            ));
        }
        self.text_bytes += bytes;
        Ok(())
    }
    fn resolve_entity(&mut self, name: &str) -> Result<Option<String>, Error> {
        if let Some(value) = predefined(name) {
            return Ok(Some(value));
        }
        if name.starts_with('#') {
            return self.fail("invalid numeric character reference");
        }
        match self.declared_entities.get(name).copied() {
            None => self.fail(format!("undeclared entity reference `&{name};`")),
            Some(DeclaredEntityKind::External) => Err(Error::at_kind(
                self.offset,
                Reason::Unsupported,
                format!("external entity reference `&{name};` is unsupported"),
            )),
            Some(DeclaredEntityKind::Internal) => match &self.options.entities {
                EntityPolicy::Preserve => Ok(None),
                EntityPolicy::Reject => Err(Error::at_kind(
                    self.offset,
                    Reason::Entity,
                    format!("entity reference `&{name};` rejected by XML entity policy"),
                )),
                EntityPolicy::Resolve(values) => {
                    let Some(value) = values.get(name).cloned() else {
                        return Ok(None);
                    };
                    if self.options.limits.max_entity_depth < 1 {
                        return self.limit("XML entity replacement exceeds max_entity_depth (0)");
                    }
                    let total = self.entity_replacement_bytes.saturating_add(value.len());
                    if value.len() > self.options.limits.max_entity_replacement_bytes
                        || total > self.options.limits.max_entity_replacement_bytes
                    {
                        return self.limit(format!(
                            "XML entity replacement exceeds max_entity_replacement_bytes ({})",
                            self.options.limits.max_entity_replacement_bytes
                        ));
                    }
                    self.entity_replacement_bytes = total;
                    self.charge_text(value.len(), "entity replacement")?;
                    Ok(Some(value))
                }
            },
        }
    }
    fn fail<T>(&self, reason: impl Into<String>) -> Result<T, Error> {
        Err(Error::at(self.offset, reason))
    }
    fn validate_characters(&self, start: usize, end: usize) -> Result<(), Error> {
        if let Some((offset, character)) = self.source[start..end]
            .char_indices()
            .find(|(_, character)| !valid_xml_char(*character))
        {
            return Err(Error::at(
                start + offset,
                format!("XML contains forbidden character U+{:04X}", character as u32),
            ));
        }
        Ok(())
    }
    fn starts(&self, s: &str) -> bool {
        self.source[self.offset..].starts_with(s)
    }
    fn take_until(&mut self, end: &str, reason: &str) -> Result<(String, String), Error> {
        let start = self.offset;
        let Some(rel) = self.source[self.offset..].find(end) else {
            return self.fail(reason);
        };
        let token_end = self.offset + rel + end.len();
        self.validate_characters(start, token_end)?;
        self.offset = token_end;
        Ok((
            self.source[start..self.offset].to_string(),
            self.source[start..self.offset - end.len()].to_string(),
        ))
    }
    fn scope_uri(
        &self,
        prefix: Option<&str>,
        local: &[(Option<String>, String)],
    ) -> Option<String> {
        for (p, u) in local.iter().rev().chain(
            self.stack
                .iter()
                .rev()
                .flat_map(|f| f.namespaces.iter().rev()),
        ) {
            if p.as_deref() == prefix {
                return Some(u.clone());
            }
        }
        if prefix == Some("xml") {
            Some("http://www.w3.org/XML/1998/namespace".into())
        } else {
            None
        }
    }
    fn resolved_name(
        &self,
        raw: &str,
        local: &[(Option<String>, String)],
        attribute: bool,
    ) -> Result<Name, Error> {
        self.check_name(raw, "name")?;
        if !valid_name(raw) {
            return self.fail(format!("invalid XML name `{raw}`"));
        }
        let (prefix, name) = split_name(raw);
        let uri = if attribute && prefix.is_none() {
            None
        } else {
            self.scope_uri(prefix.as_deref(), local)
        };
        if prefix.is_some() && uri.is_none() {
            return self.fail(format!("unbound namespace prefix in `{raw}`"));
        }
        Ok(Name {
            raw: raw.into(),
            prefix,
            local: name,
            namespace_uri: uri,
        })
    }
    fn reference(&mut self) -> Result<(String, String, Option<String>), Error> {
        let start = self.offset;
        let Some(end) = self.source[start..].find(';') else {
            return self.fail("unterminated entity reference");
        };
        let name = &self.source[start + 1..start + end];
        if name.is_empty() {
            return self.fail("empty entity reference");
        }
        self.validate_characters(start, start + end + 1)?;
        let resolved = self.resolve_entity(name)?;
        self.offset = start + end + 1;
        Ok((
            self.source[start..self.offset].into(),
            name.into(),
            resolved,
        ))
    }
    fn parse_parts(
        &mut self,
        raw: &str,
        source_base: usize,
    ) -> Result<(Vec<Part>, Option<String>), Error> {
        let mut parts = Vec::new();
        let mut normalized = String::new();
        let mut at = 0;
        while at < raw.len() {
            if let Some(rel) = raw[at..].find('&') {
                if rel > 0 {
                    let v = &raw[at..at + rel];
                    let value = normalize_attribute_text(v);
                    normalized.push_str(&value);
                    parts.push(Part::Text {
                        value,
                        raw: v.into(),
                    });
                }
                let start = at + rel;
                let Some(end) = raw[start..].find(';') else {
                    return Err(Error::at(
                        source_base + start,
                        "unterminated entity reference",
                    ));
                };
                let name = &raw[start + 1..start + end];
                let mut resolved = self.resolve_entity(name).map_err(|mut error| {
                    error.offset = source_base + start;
                    error
                })?;
                if !name.starts_with('#')
                    && !matches!(name, "lt" | "gt" | "amp" | "apos" | "quot")
                {
                    if let Some(value) = &mut resolved {
                        *value = normalize_attribute_text(value);
                    }
                }
                if let Some(v) = &resolved {
                    normalized.push_str(v)
                }
                parts.push(Part::Entity {
                    name: name.into(),
                    resolved: resolved.clone(),
                    raw: raw[start..start + end + 1].into(),
                });
                if resolved.is_none() {
                    normalized.clear()
                }
                at = start + end + 1;
            } else {
                let v = &raw[at..];
                let value = normalize_attribute_text(v);
                normalized.push_str(&value);
                parts.push(Part::Text {
                    value,
                    raw: v.into(),
                });
                break;
            }
        }
        let all = parts
            .iter()
            .all(|p| !matches!(p, Part::Entity { resolved: None, .. }));
        Ok((parts, all.then_some(normalized)))
    }
    fn open(&mut self) -> Result<Event, Error> {
        let start = self.offset;
        let mut end = start + 1;
        let mut quote = None;
        while end < self.source.len() {
            let character = self.source.as_bytes()[end] as char;
            if let Some(expected) = quote {
                if character == expected {
                    quote = None;
                }
            } else if character == '\'' || character == '"' {
                quote = Some(character);
            } else if character == '>' {
                break;
            }
            end += 1;
        }
        if end == self.source.len() {
            return self.fail("unterminated opening tag");
        }
        self.validate_characters(start, end + 1)?;
        let mut inner = &self.source[start + 1..end];
        let empty = inner.trim_end().ends_with('/');
        if empty {
            inner = inner.trim_end();
            inner = &inner[..inner.len() - 1]
        }
        let mut cursor = 0;
        while inner[cursor..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            cursor += inner[cursor..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(0)
        }
        let ns = inner[cursor..]
            .find(char::is_whitespace)
            .unwrap_or(inner.len() - cursor);
        let raw_name = &inner[cursor..cursor + ns];
        cursor += ns;
        let mut raw_attrs = Vec::new();
        while cursor < inner.len() {
            while inner[cursor..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
            {
                cursor += inner[cursor..]
                    .chars()
                    .next()
                    .map(char::len_utf8)
                    .unwrap_or(0)
            }
            if cursor >= inner.len() {
                break;
            }
            let astart = cursor;
            while cursor < inner.len()
                && !inner.as_bytes()[cursor].is_ascii_whitespace()
                && inner.as_bytes()[cursor] != b'='
            {
                cursor += 1
            }
            let key = inner[astart..cursor].to_string();
            while cursor < inner.len() && inner.as_bytes()[cursor].is_ascii_whitespace() {
                cursor += 1
            }
            if inner.as_bytes().get(cursor) != Some(&b'=') {
                return self.fail("XML attribute needs =");
            }
            cursor += 1;
            while cursor < inner.len() && inner.as_bytes()[cursor].is_ascii_whitespace() {
                cursor += 1
            }
            let quote = *inner
                .as_bytes()
                .get(cursor)
                .ok_or_else(|| Error::at(start + cursor, "missing attribute value"))?
                as char;
            if quote != '\'' && quote != '\"' {
                return self.fail("XML attribute value must be quoted");
            }
            cursor += 1;
            let vstart = cursor;
            while cursor < inner.len() && inner.as_bytes()[cursor] as char != quote {
                cursor += 1
            }
            if cursor >= inner.len() {
                return self.fail("unterminated attribute value");
            }
            let value = inner[vstart..cursor].to_string();
            let value_source_base = start + 1 + vstart;
            if value.contains('<') {
                return self.fail("XML attribute value contains <");
            }
            cursor += 1;
            raw_attrs.push((
                key,
                value,
                value_source_base,
                quote,
                inner[astart..cursor].to_string(),
            ));
        }
        let mut declarations = Vec::new();
        let mut seen_ns = BTreeSet::new();
        for (k, v, value_source_base, q, r) in &raw_attrs {
            if k == "xmlns" || k.starts_with("xmlns:") {
                let p = if k == "xmlns" {
                    None
                } else {
                    Some(k[6..].to_string())
                };
                if !seen_ns.insert(p.clone()) {
                    return self.fail("duplicate namespace declaration");
                }
                if p.as_deref() == Some("xmlns")
                    || p.as_deref() == Some("xml") && v != "http://www.w3.org/XML/1998/namespace"
                {
                    return self.fail("reserved namespace prefix binding");
                }
                let (_, normalized) = self.parse_parts(v, *value_source_base)?;
                let Some(namespace_uri) = normalized else {
                    return self.fail("namespace URI contains unresolved entity reference");
                };
                declarations.push((p, namespace_uri, *q, r.clone()));
            }
        }
        let local: Vec<_> = declarations
            .iter()
            .map(|(p, u, _, _)| (p.clone(), u.clone()))
            .collect();
        let name = self.resolved_name(raw_name, &local, false)?;
        let namespaces = declarations
            .into_iter()
            .map(|(prefix, namespace_uri, quote, raw)| Namespace {
                prefix,
                namespace_uri,
                quote,
                raw,
            })
            .collect();
        let mut attributes = Vec::new();
        let mut expanded = BTreeSet::new();
        for (k, v, value_source_base, q, r) in raw_attrs {
            if k == "xmlns" || k.starts_with("xmlns:") {
                continue;
            }
            let aname = self.resolved_name(&k, &local, true)?;
            if !expanded.insert((aname.namespace_uri.clone(), aname.local.clone())) {
                return self.fail("duplicate expanded XML attribute");
            }
            let (parts, normalized) = self.parse_parts(&v, value_source_base)?;
            attributes.push(Attribute {
                name: aname,
                parts,
                normalized,
                quote: q,
                raw: r,
            });
        }
        self.offset = end + 1;
        let raw = self.source[start..self.offset].into();
        self.root_seen = true;
        if !empty {
            self.stack.push(Frame {
                name: name.clone(),
                namespaces: local,
            })
        } else if self.stack.is_empty() {
            self.root_closed = true
        }
        Ok(Event::ElementStart {
            name,
            namespaces,
            attributes,
            empty,
            raw,
        })
    }
    fn charge_nodes(&mut self, amount: usize) -> Result<(), Error> {
        if self.nodes.saturating_add(amount) > self.options.limits.max_nodes {
            return self.limit(format!(
                "XML document exceeds max_nodes ({})",
                self.options.limits.max_nodes
            ));
        }
        self.nodes += amount;
        Ok(())
    }
    fn charge_event(&mut self, event: &Event) -> Result<(), Error> {
        match event {
            Event::DocumentStart => self.charge_nodes(1)?,
            Event::DocumentEnd | Event::ElementEnd { .. } => {}
            Event::ElementStart {
                name,
                namespaces,
                attributes,
                empty,
                ..
            } => {
                let depth = if *empty { self.stack.len() + 1 } else { self.stack.len() };
                if depth > self.options.limits.max_depth {
                    return self.limit(format!(
                        "XML element nesting exceeds max_depth ({})",
                        self.options.limits.max_depth
                    ));
                }
                if attributes.len() > self.options.limits.max_attributes_per_element {
                    return self.limit(format!(
                        "XML element exceeds max_attributes_per_element ({})",
                        self.options.limits.max_attributes_per_element
                    ));
                }
                self.check_name(&name.raw, "element name")?;
                self.check_name(&name.local, "local name")?;
                if let Some(prefix) = &name.prefix {
                    self.check_name(prefix, "name prefix")?;
                }
                if let Some(uri) = &name.namespace_uri {
                    self.check_name(uri, "namespace URI")?;
                }
                for namespace in namespaces {
                    if let Some(prefix) = &namespace.prefix {
                        self.check_name(prefix, "namespace prefix")?;
                    }
                    self.check_name(&namespace.namespace_uri, "namespace URI")?;
                }
                for attribute in attributes {
                    self.check_name(&attribute.name.raw, "attribute name")?;
                    self.check_name(&attribute.name.local, "attribute local name")?;
                    if let Some(prefix) = &attribute.name.prefix {
                        self.check_name(prefix, "attribute prefix")?;
                    }
                    if let Some(uri) = &attribute.name.namespace_uri {
                        self.check_name(uri, "attribute namespace URI")?;
                    }
                    for part in &attribute.parts {
                        if let Part::Text { value, .. } = part {
                            self.charge_text(value.len(), "attribute text")?;
                        }
                    }
                }
                self.charge_nodes(1 + namespaces.len() + attributes.len())?;
            }
            Event::Declaration { version, encoding, .. } => {
                self.check_name(version, "XML version")?;
                if let Some(encoding) = encoding {
                    self.check_name(encoding, "XML encoding")?;
                }
                self.charge_nodes(1)?;
            }
            Event::Doctype {
                name,
                public_id,
                system_id,
                internal_subset,
                ..
            } => {
                self.check_name(name, "DOCTYPE name")?;
                for value in [public_id, system_id].into_iter().flatten() {
                    self.check_name(value, "DOCTYPE identifier")?;
                }
                if let Some(value) = internal_subset {
                    self.charge_text(value.len(), "DOCTYPE internal subset")?;
                }
                self.charge_nodes(1)?;
            }
            Event::ProcessingInstruction { target, value, .. } => {
                self.check_name(target, "processing instruction target")?;
                self.charge_text(value.len(), "processing instruction text")?;
                self.charge_nodes(1)?;
            }
            Event::DocumentWhitespace { value, .. }
            | Event::Text { value, .. }
            | Event::Cdata { value, .. }
            | Event::Comment { value, .. } => {
                self.charge_text(value.len(), "text")?;
                self.charge_nodes(1)?;
            }
            Event::EntityRef { name, .. } => {
                self.check_name(name, "entity name")?;
                self.charge_nodes(1)?;
            }
        }
        Ok(())
    }
    fn next_inner(&mut self) -> Result<Option<Event>, Error> {
        if self.ended {
            return Ok(None);
        }
        if !self.started {
            self.started = true;
            return Ok(Some(Event::DocumentStart));
        }
        if self.offset == self.source.len() {
            if !self.stack.is_empty() {
                return self.fail("XML document ended before closing element");
            }
            if !self.root_seen {
                return self.fail("empty XML document");
            }
            self.ended = true;
            return Ok(Some(Event::DocumentEnd));
        }
        if self.starts("<!--") {
            let (raw, body) = self.take_until("-->", "unterminated XML comment")?;
            let value = body[4..].to_string();
            if value.contains("--") {
                return self.fail("XML comment contains --");
            }
            return Ok(Some(Event::Comment { value, raw }));
        }
        if self.starts("<![CDATA[") {
            if self.stack.is_empty() {
                return self.fail("CDATA outside root element");
            }
            let (raw, body) = self.take_until("]]>", "unterminated CDATA")?;
            return Ok(Some(Event::Cdata {
                value: body[9..].into(),
                raw,
            }));
        }
        if self.starts("<?xml") {
            if self.declaration_seen || self.root_seen || self.offset != 0 {
                return self.fail("XML declaration is out of order");
            }
            let (raw, body) = self.take_until("?>", "unterminated XML declaration")?;
            let data = body[5..].trim();
            let (version, encoding, standalone) = declaration_fields(data)?;
            if encoding
                .as_deref()
                .is_some_and(|name| !declaration_matches_input(name, self.wire_encoding))
            {
                return Err(Error::at_kind(
                    0,
                    Reason::InvalidEncoding,
                    "XML declaration conflicts with detected input encoding",
                ));
            }
            self.declaration_seen = true;
            return Ok(Some(Event::Declaration {
                version,
                encoding,
                standalone,
                raw,
            }));
        }
        if self.starts("<?") {
            let (raw, body) = self.take_until("?>", "unterminated processing instruction")?;
            let data = &body[2..];
            let split = data.find(char::is_whitespace).unwrap_or(data.len());
            let target = data[..split].to_string();
            if !valid_name(&target) || target.eq_ignore_ascii_case("xml") {
                return self.fail("invalid processing instruction target");
            }
            return Ok(Some(Event::ProcessingInstruction {
                target,
                value: data[split..].trim_start().into(),
                raw,
            }));
        }
        if self.starts("<!DOCTYPE") {
            if self.doctype_seen || self.root_seen {
                return self.fail("DOCTYPE is out of order");
            }
            let start = self.offset;
            let mut i = start + 9;
            let mut quote = None;
            let mut bracket = 0;
            while i < self.source.len() {
                let c = self.source.as_bytes()[i] as char;
                if let Some(q) = quote {
                    if c == q {
                        quote = None
                    }
                } else if c == '\'' || c == '\"' {
                    quote = Some(c)
                } else if c == '[' {
                    bracket += 1
                } else if c == ']' {
                    bracket -= 1
                } else if c == '>' && bracket == 0 {
                    break;
                }
                i += 1
            }
            if i == self.source.len() {
                return self.fail("unterminated DOCTYPE");
            }
            self.validate_characters(start, i + 1)?;
            self.offset = i + 1;
            let raw = self.source[start..self.offset].to_string();
            let inside = self.source[start + 9..i].trim();
            let (name, public_id, system_id, internal_subset) = doctype_fields(inside)
                .map_err(|error| Error::at(start + 9 + error.offset, error.reason))?;
            let (declared_entities, declaration_count) =
                declared_entity_table(internal_subset.as_deref()).map_err(|error| {
                    let mut mapped = Error::at_kind(
                        start + 9 + error.offset,
                        error.kind,
                        error.reason.clone(),
                    );
                    mapped.path = error.path;
                    mapped
                })?;
            if declaration_count > self.options.limits.max_entity_declarations {
                return self.limit(format!(
                    "XML document exceeds max_entity_declarations ({})",
                    self.options.limits.max_entity_declarations
                ));
            }
            for name in declared_entities.keys() {
                self.check_name(name, "entity name")?;
            }
            self.declared_entities = declared_entities;
            self.doctype_seen = true;
            return Ok(Some(Event::Doctype {
                name,
                public_id,
                system_id,
                internal_subset,
                raw,
            }));
        }
        if self.starts("</") {
            let start = self.offset;
            let Some(rel) = self.source[start..].find('>') else {
                return self.fail("unterminated closing tag");
            };
            self.validate_characters(start, start + rel + 1)?;
            let raw_name = self.source[start + 2..start + rel].trim();
            let frame = self
                .stack
                .last()
                .cloned()
                .ok_or_else(|| Error::at(start, "closing tag without opener"))?;
            let name = self.resolved_name(raw_name, &[], false)?;
            if name.namespace_uri != frame.name.namespace_uri || name.local != frame.name.local {
                return self.fail("mismatched XML closing tag");
            }
            self.offset = start + rel + 1;
            self.stack.pop();
            if self.stack.is_empty() {
                self.root_closed = true
            }
            return Ok(Some(Event::ElementEnd {
                name,
                raw: self.source[start..self.offset].into(),
            }));
        }
        if self.starts("<") {
            if self.root_closed {
                return self.fail("multiple XML root elements");
            }
            return self.open().map(Some);
        }
        if self.starts("&") {
            if self.stack.is_empty() {
                return self.fail("entity reference outside root element");
            }
            let (raw, name, resolved) = self.reference()?;
            return Ok(Some(Event::EntityRef {
                name,
                resolved,
                raw,
            }));
        }
        let start = self.offset;
        let rel = self.source[start..]
            .find(['<', '&'])
            .unwrap_or(self.source.len() - start);
        self.validate_characters(start, start + rel)?;
        self.offset += rel;
        let raw = self.source[start..self.offset].to_string();
        if self.stack.is_empty() {
            if !raw.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')) {
                return self.fail("character data outside root element");
            }
            Ok(Some(Event::DocumentWhitespace {
                value: raw.clone(),
                raw,
            }))
        } else {
            if raw.contains("]]>") {
                return self.fail("character data contains forbidden ]]>");
            }
            Ok(Some(Event::Text {
                value: raw.clone(),
                raw,
            }))
        }
    }

    pub fn next(&mut self) -> Result<Option<Event>, Error> {
        match self.next_inner() {
            Ok(Some(event)) => {
                if let Err(mut error) = self.charge_event(&event) {
                    Self::classify(&mut error);
                    return Err(error.located(self.source, self.path()));
                }
                Ok(Some(event))
            }
            Ok(None) => Ok(None),
            Err(mut error) => {
                Self::classify(&mut error);
                Err(error.located(self.source, self.path()))
            }
        }
    }
}
