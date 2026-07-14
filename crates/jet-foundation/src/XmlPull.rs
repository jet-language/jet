//! Shared XML 1.0 pull tokenizer. Kept std-only so generated and comptime
//! adapters consume identical token, namespace, and error behavior.

use std::collections::{BTreeMap, BTreeSet};

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

struct ElementFrame {
    name: Name,
    namespaces: Vec<Value>,
    attributes: Vec<Value>,
    children: Vec<Value>,
    open_raw: String,
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

fn lexical_raw<'a>(value: &'a Value, key: &str, semantic: &Value) -> Option<&'a str> {
    let lexical_value = field(value, key)?;
    if field(lexical_value, "semantic")? != semantic {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use super::{field, parse_document, parse_document_with, render_document, ByteLexer, EntityPolicy, Event, Limits, ParseOptions, Part, Reason, Scanner, StreamScanner, TokenKind, Value, WireEncoding};

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
        Self::at_kind(0, Reason::Limit, reason)
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
pub enum WireEncoding { Utf8, Utf16Le, Utf16Be }

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
            self.encoding = Some(WireEncoding::Utf8); self.bom = self.undecided.drain(..3).collect(); self.wire_offset = 3;
        } else if self.undecided.starts_with(&[0xff, 0xfe]) {
            self.encoding = Some(WireEncoding::Utf16Le); self.bom = self.undecided.drain(..2).collect(); self.wire_offset = 2;
        } else if self.undecided.starts_with(&[0xfe, 0xff]) {
            self.encoding = Some(WireEncoding::Utf16Be); self.bom = self.undecided.drain(..2).collect(); self.wire_offset = 2;
        } else if self.undecided.len() >= 4 {
            self.encoding = Some(if self.undecided[..4] == [0, b'<', 0, b'?'] { WireEncoding::Utf16Be } else if self.undecided[..4] == [b'<', 0, b'?', 0] { WireEncoding::Utf16Le } else { WireEncoding::Utf8 });
        } else if final_input || self.undecided.len() >= 3 {
            self.encoding = Some(WireEncoding::Utf8);
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
            match encoding {
                WireEncoding::Utf8 => {
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
            if self.boundary().is_some() { break; }
        }
        if final_input && self.boundary().is_none() && !self.pending.is_empty() { return self.fail(Reason::InvalidEncoding, "truncated encoded XML scalar"); }
        Ok(())
    }
    fn boundary(&self) -> Option<(usize, TokenKind)> {
        if self.units.is_empty() { return None; }
        let chars: Vec<char> = self.units.iter().map(|unit| unit.scalar).collect();
        if chars[0] == '&' { return chars.iter().position(|c| *c == ';').map(|i| (i + 1, TokenKind::Entity)); }
        if chars[0] != '<' {
            return chars.iter().position(|c| *c == '<' || *c == '&').map(|i| (i, TokenKind::Text)).or_else(|| self.eof.then_some((chars.len(), TokenKind::Text)));
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
    declared_entities: BTreeSet<String>,
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
        Ok(Self { lexer: ByteLexer::new(max_item_bytes), options, started: false, ended: false, root_seen: false, root_closed: false, declaration_seen: false, doctype_seen: false, declared_entities: BTreeSet::new(), stack: Vec::new(), nodes: 0, text_bytes: 0, entity_replacement_bytes: 0, wire_consumed: 0, line: 1, column: 1, terminal: None })
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
        let mut scanner=Scanner{source:&token.text,offset:0,started:true,ended:false,root_seen:self.root_seen,root_closed:self.root_closed,declaration_seen:self.declaration_seen,doctype_seen:self.doctype_seen,declared_entities:self.declared_entities.clone(),stack:self.stack.clone(),options:self.options.clone(),nodes:self.nodes,text_bytes:self.text_bytes,entity_replacement_bytes:self.entity_replacement_bytes};
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
    offset: usize,
    started: bool,
    ended: bool,
    root_seen: bool,
    root_closed: bool,
    declaration_seen: bool,
    doctype_seen: bool,
    declared_entities: BTreeSet<String>,
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
            .map(|c| c.to_string()),
        _ if name.starts_with('#') => name[1..]
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
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

fn declared_entity_names(subset: Option<&str>) -> Result<(BTreeSet<String>, usize), Error> {
    let mut names = BTreeSet::new();
    let mut count = 0usize;
    let Some(mut remaining) = subset else {
        return Ok((names, count));
    };
    while let Some(start) = remaining.find("<!ENTITY") {
        count = count.saturating_add(1);
        remaining = &remaining[start + "<!ENTITY".len()..];
        let declaration = remaining.trim_start();
        if declaration.starts_with('%') {
            remaining = &declaration[1..];
            continue;
        }
        let end = declaration
            .find(char::is_whitespace)
            .ok_or_else(|| Error::at(start, "ENTITY declaration lacks value"))?;
        let name = &declaration[..end];
        if !valid_name(name) {
            return Err(Error::at(start, "invalid ENTITY declaration name"));
        }
        if !names.insert(name.to_string()) {
            return Err(Error::at(start, "duplicate ENTITY declaration"));
        }
        remaining = &declaration[end..];
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
                if !value.eq_ignore_ascii_case("UTF-8") {
                    return Err(Error::at(0, "String XML input must declare UTF-8"));
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

impl<'a> Scanner<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            offset: 0,
            started: false,
            ended: false,
            root_seen: false,
            root_closed: false,
            declaration_seen: false,
            doctype_seen: false,
            declared_entities: BTreeSet::new(),
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
        if !self.declared_entities.contains(name) {
            return self.fail(format!("undeclared entity reference `&{name};`"));
        }
        match &self.options.entities {
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
        }
    }
    fn fail<T>(&self, reason: impl Into<String>) -> Result<T, Error> {
        Err(Error::at(self.offset, reason))
    }
    fn starts(&self, s: &str) -> bool {
        self.source[self.offset..].starts_with(s)
    }
    fn take_until(&mut self, end: &str, reason: &str) -> Result<(String, String), Error> {
        let start = self.offset;
        let Some(rel) = self.source[self.offset..].find(end) else {
            return self.fail(reason);
        };
        self.offset += rel + end.len();
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
        let resolved = self.resolve_entity(name)?;
        self.offset = start + end + 1;
        Ok((
            self.source[start..self.offset].into(),
            name.into(),
            resolved,
        ))
    }
    fn parse_parts(&mut self, raw: &str) -> Result<(Vec<Part>, Option<String>), Error> {
        let mut parts = Vec::new();
        let mut normalized = String::new();
        let mut at = 0;
        while at < raw.len() {
            if let Some(rel) = raw[at..].find('&') {
                if rel > 0 {
                    let v = &raw[at..at + rel];
                    normalized.push_str(v);
                    parts.push(Part::Text {
                        value: v.into(),
                        raw: v.into(),
                    });
                }
                let start = at + rel;
                let Some(end) = raw[start..].find(';') else {
                    return Err(Error::at(start, "unterminated entity reference"));
                };
                let name = &raw[start + 1..start + end];
                let resolved = self.resolve_entity(name).map_err(|mut error| {
                    error.offset = start;
                    error
                })?;
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
                normalized.push_str(v);
                parts.push(Part::Text {
                    value: v.into(),
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
            if value.contains('<') {
                return self.fail("XML attribute value contains <");
            }
            cursor += 1;
            raw_attrs.push((key, value, quote, inner[astart..cursor].to_string()));
        }
        let mut declarations = Vec::new();
        let mut seen_ns = BTreeSet::new();
        for (k, v, q, r) in &raw_attrs {
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
                let (_, normalized) = self.parse_parts(v)?;
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
        for (k, v, q, r) in raw_attrs {
            if k == "xmlns" || k.starts_with("xmlns:") {
                continue;
            }
            let aname = self.resolved_name(&k, &local, true)?;
            if !expanded.insert((aname.namespace_uri.clone(), aname.local.clone())) {
                return self.fail("duplicate expanded XML attribute");
            }
            let (parts, normalized) = self.parse_parts(&v)?;
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
            let (version, encoding, standalone) =
                declaration_fields(data).map_err(|error| Error::at(error.offset, error.reason))?;
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
            self.offset = i + 1;
            let raw = self.source[start..self.offset].to_string();
            let inside = self.source[start + 9..i].trim();
            let (name, public_id, system_id, internal_subset) = doctype_fields(inside)
                .map_err(|error| Error::at(start + 9 + error.offset, error.reason))?;
            let (declared_entities, declaration_count) = declared_entity_names(internal_subset.as_deref())
                .map_err(|error| Error::at(start + 9 + error.offset, error.reason))?;
            if declaration_count > self.options.limits.max_entity_declarations {
                return self.limit(format!(
                    "XML document exceeds max_entity_declarations ({})",
                    self.options.limits.max_entity_declarations
                ));
            }
            for name in &declared_entities {
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
