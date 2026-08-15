//! JSON rendering and value conversion for the comptime/REPL interpreter.
//! Parsing uses the same foundation-backed Prelude kernel as AOT and JIT.

use crate::AST::{CtFloat, CtKey, CtValue};
use crate::Comptime::Builtins::exact_int_value;

fn from_json_int(value: i64) -> CtValue {
    exact_int_value(crate::Numeric::CtBigInt::from_int(value))
}

fn from_json(value: jet_foundation::EncodingJson::Value) -> CtValue {
    match value {
        jet_foundation::EncodingJson::Value::Null => json_variant("Null", None),
        jet_foundation::EncodingJson::Value::Bool(value) => {
            json_variant("Bool", Some(CtValue::Bool(value)))
        }
        jet_foundation::EncodingJson::Value::Int(value) => {
            json_variant("Int", Some(from_json_int(value)))
        }
        jet_foundation::EncodingJson::Value::Float(value) => {
            json_variant("Float", Some(CtValue::Float(CtFloat::f64(value))))
        }
        jet_foundation::EncodingJson::Value::Number(_) => {
            unreachable!("lossless JSON number leaked into dynamic projection")
        }
        jet_foundation::EncodingJson::Value::Text(value) => {
            json_variant("Text", Some(CtValue::Str(value)))
        }
        jet_foundation::EncodingJson::Value::Array(values) => json_variant(
            "Array",
            Some(CtValue::List(values.into_iter().map(from_json).collect())),
        ),
        jet_foundation::EncodingJson::Value::Object(fields) => {
            let map = fields
                .into_iter()
                .map(|(key, value)| (CtKey::Str(key), from_json(value)))
                .collect();
            json_variant("Object", Some(CtValue::Map(map)))
        }
    }
}

fn from_ordered_json(value: jet_foundation::EncodingJson::Value) -> CtValue {
    match value {
        jet_foundation::EncodingJson::Value::Object(fields) => json_object(
            fields
                .into_iter()
                .map(|(key, value)| (key, from_ordered_json(value)))
                .collect(),
        ),
        jet_foundation::EncodingJson::Value::Null => json_variant("Null", None),
        jet_foundation::EncodingJson::Value::Bool(value) => {
            json_variant("Bool", Some(CtValue::Bool(value)))
        }
        jet_foundation::EncodingJson::Value::Int(value) => {
            json_variant("Int", Some(from_json_int(value)))
        }
        jet_foundation::EncodingJson::Value::Float(value) => {
            json_variant("Float", Some(CtValue::Float(CtFloat::f64(value))))
        }
        jet_foundation::EncodingJson::Value::Number(_) => {
            unreachable!("lossless JSON number leaked into dynamic projection")
        }
        jet_foundation::EncodingJson::Value::Text(value) => {
            json_variant("Text", Some(CtValue::Str(value)))
        }
        jet_foundation::EncodingJson::Value::Array(values) => json_variant(
            "Array",
            Some(CtValue::List(
                values.into_iter().map(from_ordered_json).collect(),
            )),
        ),
    }
}

fn from_typed_ordered_json(value: jet_foundation::EncodingJson::Value) -> CtValue {
    match value {
        jet_foundation::EncodingJson::Value::Object(fields) => json_object(
            fields
                .into_iter()
                .map(|(key, value)| (key, from_typed_ordered_json(value)))
                .collect(),
        ),
        jet_foundation::EncodingJson::Value::Null => json_variant("Null", None),
        jet_foundation::EncodingJson::Value::Bool(value) => {
            json_variant("Bool", Some(CtValue::Bool(value)))
        }
        jet_foundation::EncodingJson::Value::Number(value) => json_variant(
            "Text",
            Some(CtValue::Str(
                jet_foundation::JSONNumber::json_typed_number(&value),
            )),
        ),
        jet_foundation::EncodingJson::Value::Text(value) => json_variant(
            "Text",
            Some(CtValue::Str(
                jet_foundation::JSONNumber::json_typed_text(&value),
            )),
        ),
        jet_foundation::EncodingJson::Value::Array(values) => json_variant(
            "Array",
            Some(CtValue::List(
                values
                    .into_iter()
                    .map(from_typed_ordered_json)
                    .collect(),
            )),
        ),
        jet_foundation::EncodingJson::Value::Int(_)
        | jet_foundation::EncodingJson::Value::Float(_) => {
            unreachable!("lossless JSON parsing projected a number early")
        }
    }
}

fn json_object(fields: Vec<(String, CtValue)>) -> CtValue {
    json_variant(
        "Object",
        Some(CtValue::Struct {
            type_name: "JSONObject".to_string(),
            fields,
        }),
    )
}

/// D-SERDE-ACCESS=B / D-DYNAMIC-TYPE1=A: build one node of the `JSON`/`Data`
/// dynamic-value tree — a `CtValue::Enum` so it round-trips through the exact
/// same pattern-matching (`data == .Object(entries)`, S31) and explicit
/// construction (`JSON.Text("jet")`) machinery a user enum already gets, with
/// no interpreter-specific special case needed on either of those paths.
pub(super) fn json_variant(variant: &str, payload: Option<CtValue>) -> CtValue {
    CtValue::Enum {
        type_name: "JSON".to_string(),
        variant: variant.to_string(),
        args: match payload {
            Some(v) => vec![(None, v)],
            None => Vec::new(),
        },
    }
}

/// The payload of a `JSON`-tagged `CtValue` with the given variant name, or
/// `None` if `v` isn't that shape (used by the `.field`/`.at`/`.int`/`.text`/
/// `.bool`/`.float` accessor methods in `Builtins.rs`).
pub(super) fn json_payload<'a>(v: &'a CtValue, variant: &str) -> Option<&'a CtValue> {
    match v {
        CtValue::Enum {
            type_name,
            variant: vname,
            args,
        } if type_name == "JSON" && vname == variant => args.first().map(|(_, v)| v),
        _ => None,
    }
}

pub(super) fn parse_json(
    text: &str,
) -> Result<CtValue, jet_foundation::EncodingJson::Error> {
    jet_foundation::EncodingJson::parse_json(text, false).map(from_json)
}

pub(super) fn parse_json_ordered(
    text: &str,
) -> Result<CtValue, jet_foundation::EncodingJson::Error> {
    jet_foundation::EncodingJson::parse_json(text, false).map(from_ordered_json)
}

pub(super) fn parse_json_typed_ordered(
    text: &str,
) -> Result<CtValue, jet_foundation::EncodingJson::Error> {
    jet_foundation::EncodingJson::parse_json_exact_numbers(text, false)
        .map(from_typed_ordered_json)
}

pub(super) fn json_error_value(e: jet_foundation::EncodingJson::Error) -> CtValue {
    CtValue::Struct {
        type_name: "JSONError".to_string(),
        fields: vec![
            ("line".to_string(), CtValue::Int(e.line)),
            ("message".to_string(), CtValue::Str(e.message)),
        ],
    }
}

/// `core.encoding.jsonl.parse` (`EncodingLite.rs`) reports a JSON parse error
/// on line `idx` of the JSONL document at `idx + e.line` — mirrors AOT's
/// `jet_std_jsonl_parse` (`MathRandomTime.rs`), which adds the 0-based JSONL
/// line index to the per-line JSON parser's own line number.
/// parity: guard tests/encoding_parity.rs::jsonl_csv_xml_cbor_streams_match_aot_and_default_dev
pub(super) fn json_error_value_at_line(
    e: jet_foundation::EncodingJson::Error,
    line_offset: i64,
) -> CtValue {
    json_error_value(jet_foundation::EncodingJson::Error {
        line: line_offset + e.line,
        message: e.message,
    })
}

/// Render the ordered `DataTree` representation used by typed codecs. Dynamic
/// JSON keeps the canonical BTreeMap renderer below; published-schema output
/// must retain the wire order stored in `JSONObject`.
pub(super) fn render_ordered_datatree(v: &CtValue, pretty: bool, depth: usize) -> String {
    match v {
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if matches!(
            type_name.as_str(),
            "DataTree" | "JSON" | "TOML" | "YAML" | "CSV"
        ) => match variant.as_str() {
            "Null" => "null".to_string(),
            _ => args
                .first()
                .map(|(_, payload)| render_ordered_datatree(payload, pretty, depth))
                .unwrap_or_else(|| "null".to_string()),
        },
        CtValue::List(items) => {
            if items.is_empty() {
                return "[]".to_string();
            }
            if !pretty {
                return format!(
                    "[{}]",
                    items
                        .iter()
                        .map(|item| render_ordered_datatree(item, false, depth))
                        .collect::<Vec<_>>()
                        .join(",")
                );
            }
            let pad = "  ".repeat(depth + 1);
            let end = "  ".repeat(depth);
            let parts = items
                .iter()
                .map(|item| {
                    format!(
                        "{}{}",
                        pad,
                        render_ordered_datatree(item, true, depth + 1)
                    )
                })
                .collect::<Vec<_>>();
            format!("[\n{}\n{}]", parts.join(",\n"), end)
        }
        CtValue::Struct {
            type_name,
            fields,
        } if type_name == "JSONObject" => render_ordered_object(fields, pretty, depth),
        _ => render_json_pretty(v, pretty, depth),
    }
}

fn render_ordered_object(fields: &[(String, CtValue)], pretty: bool, depth: usize) -> String {
    if fields.is_empty() {
        return "{}".to_string();
    }
    if !pretty {
        return format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{}:{}",
                        quote_json(key),
                        render_ordered_datatree(value, false, depth)
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    let pad = "  ".repeat(depth + 1);
    let end = "  ".repeat(depth);
    let parts = fields
        .iter()
        .map(|(key, value)| {
            format!(
                "{}{}: {}",
                pad,
                quote_json(key),
                render_ordered_datatree(value, true, depth + 1)
            )
        })
        .collect::<Vec<_>>();
    format!("{{\n{}\n{}}}", parts.join(",\n"), end)
}

pub(super) fn render_json_pretty(v: &CtValue, pretty: bool, depth: usize) -> String {
    match v {
        // D-SERDE-ACCESS=B: a `JSON`-tagged dynamic value (from `.parse()`, or
        // built by hand with `JSON.Text(…)`/`JSON.Object(…)`) — unwrap the tag
        // and render its payload the same way the untagged shapes below do.
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if matches!(
            type_name.as_str(),
            "DataTree" | "JSON" | "TOML" | "YAML" | "CSV"
        ) =>
        match variant.as_str() {
            "Null" => "null".to_string(),
            _ => match args.first() {
                Some((_, payload)) => render_json_pretty(payload, pretty, depth),
                None => "null".to_string(),
            },
        },
        CtValue::Unit => "null".to_string(),
        CtValue::Bool(b) => b.to_string(),
        CtValue::Int(n) => n.to_string(),
        CtValue::Float(f) => format!("{:?}", f),
        CtValue::Str(s) => quote_json(s),
        CtValue::List(xs) => {
            if xs.is_empty() {
                return "[]".to_string();
            }
            if !pretty {
                let parts: Vec<String> = xs
                    .iter()
                    .map(|x| render_json_pretty(x, false, depth))
                    .collect();
                return format!("[{}]", parts.join(","));
            }
            let pad = "  ".repeat(depth + 1);
            let end = "  ".repeat(depth);
            let parts: Vec<String> = xs
                .iter()
                .map(|x| format!("{}{}", pad, render_json_pretty(x, true, depth + 1)))
                .collect();
            format!("[\n{}\n{}]", parts.join(",\n"), end)
        }
        CtValue::Map(m) => {
            if m.is_empty() {
                return "{}".to_string();
            }
            if !pretty {
                let parts: Vec<String> = m
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{}:{}",
                            quote_json(&match k {
                                CtKey::Str(s) => s.clone(),
                                other => other.to_value().jet_show(),
                            }),
                            render_json_pretty(v, false, depth)
                        )
                    })
                    .collect();
                return format!("{{{}}}", parts.join(","));
            }
            let pad = "  ".repeat(depth + 1);
            let end = "  ".repeat(depth);
            let parts: Vec<String> = m
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        CtKey::Str(s) => quote_json(s),
                        other => quote_json(&other.to_value().jet_show()),
                    };
                    format!("{}{}: {}", pad, key, render_json_pretty(v, true, depth + 1))
                })
                .collect();
            format!("{{\n{}\n{}}}", parts.join(",\n"), end)
        }
        // EncodingLite Object payload: insertion-ordered `Struct` (AOT DataTree).
        // Sort keys so `json.canonical` matches BTreeMap / AOT sorted form.
        CtValue::Struct {
            type_name,
            fields,
        } if type_name == "JSONObject" => {
            let mut fields = fields.clone();
            fields.sort_by(|a, b| a.0.cmp(&b.0));
            if fields.is_empty() {
                return "{}".to_string();
            }
            if !pretty {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(k, v)| {
                        format!("{}:{}", quote_json(k), render_json_pretty(v, false, depth))
                    })
                    .collect();
                return format!("{{{}}}", parts.join(","));
            }
            let pad = "  ".repeat(depth + 1);
            let end = "  ".repeat(depth);
            let parts: Vec<String> = fields
                .iter()
                .map(|(k, v)| {
                    format!("{}{}: {}", pad, quote_json(k), render_json_pretty(v, true, depth + 1))
                })
                .collect();
            format!("{{\n{}\n{}}}", parts.join(",\n"), end)
        }
        other => other.to_json(),
    }
}

fn quote_json(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
