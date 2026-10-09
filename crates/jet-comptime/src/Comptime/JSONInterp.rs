//! JSON rendering and value conversion for the comptime/REPL interpreter.
//! Parsing uses the same foundation-backed Prelude kernel as AOT and JIT.

use crate::Comptime::Builtins::exact_int_value;
use crate::AST::{CtFloat, CtKey, CtReport, CtValue};
use jet_foundation::EncodingJson::{json_unsupported, EncodingError};
use std::fmt::Write;

fn from_json_int(value: crate::Numeric::JetInt) -> CtValue {
    exact_int_value(value.to_big())
}

/// One carrier node as the interpreter's `Dyn` enum value. JSON parsing
/// keeps every number token as `Number` (#4602); `Int`/`Float`/`Bytes` arrive
/// from other formats and typed encoders.
pub(super) fn from_json(value: jet_foundation::Dyn::Dyn) -> CtValue {
    match value {
        jet_foundation::Dyn::Dyn::Null => json_variant("Null", None),
        jet_foundation::Dyn::Dyn::Bool(value) => {
            json_variant("Bool", Some(CtValue::Bool(value)))
        }
        jet_foundation::Dyn::Dyn::Int(value) => {
            json_variant("Int", Some(from_json_int(value)))
        }
        jet_foundation::Dyn::Dyn::Float(value) => {
            json_variant("Float", Some(CtValue::Float(CtFloat::f64(value))))
        }
        jet_foundation::Dyn::Dyn::Number(value) => {
            json_variant("Number", Some(CtValue::Str(value)))
        }
        jet_foundation::Dyn::Dyn::Text(value) => {
            json_variant("Text", Some(CtValue::Str(value)))
        }
        jet_foundation::Dyn::Dyn::Bytes(value) => {
            json_variant("Bytes", Some(CtValue::Bytes(value)))
        }
        jet_foundation::Dyn::Dyn::Array(values) => json_variant(
            "Array",
            Some(CtValue::List(values.into_iter().map(from_json).collect())),
        ),
        jet_foundation::Dyn::Dyn::Object(fields) => json_object(
            fields
                .into_iter()
                .map(|(key, value)| (key, from_json(value)))
                .collect(),
        ),
        jet_foundation::Dyn::Dyn::DateTime(value) => json_variant(
            "DateTime",
            Some(super::Methods::dyn_datetime_value(value)),
        ),
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

/// D-SERDE-ACCESS=B / D-DYNAMIC-TYPE1=A: build one node of the canonical
/// `Dyn` dynamic-value tree — a `CtValue::Enum` so it round-trips through
/// the exact same pattern-matching (`data == .Object(entries)`, S31) and
/// explicit construction (`Dyn.Text("jet")`) machinery a user enum already
/// gets, with no interpreter-specific special case needed on either path.
pub(super) fn json_variant(variant: &str, payload: Option<CtValue>) -> CtValue {
    CtValue::Enum {
        type_name: "Dyn".to_string(),
        variant: variant.to_string(),
        args: match payload {
            Some(v) => vec![(None, v)],
            None => Vec::new(),
        },
    }
}

/// The payload of a `Dyn`-tagged `CtValue` with the given variant name, or
/// `None` if `v` isn't that shape (used by the `.field`/`.at`/`.int`/`.text`/
/// `.bool`/`.float` accessor methods in `Builtins.rs`).
pub(super) fn json_payload<'a>(v: &'a CtValue, variant: &str) -> Option<&'a CtValue> {
    match v {
        CtValue::Enum {
            type_name,
            variant: vname,
            args,
        } if type_name == "Dyn" && vname == variant => args.first().map(|(_, v)| v),
        _ => None,
    }
}

pub(super) fn parse_json(text: &str) -> Result<CtValue, jet_foundation::EncodingJson::Error> {
    jet_foundation::EncodingJson::parse_json_exact_numbers(text, true).map(from_json)
}

pub(super) fn encoding_error_value(e: jet_foundation::EncodingJson::Error) -> CtValue {
    super::EncodingLite::encoding_error_value_at("JSON", "Syntax", e.line, 1, e.message)
}

pub(super) fn text_encoding_error_value(error: EncodingError) -> CtValue {
    super::EncodingLite::encoding_error_value_fields(
        error.format.as_str(),
        &format!("{:?}", error.kind),
        error.byte_offset,
        error.line.ok(),
        error.column.ok(),
        error.path,
        error.reason,
    )
}

/// `core.encoding.jsonl.parse` (`EncodingLite.rs`) reports a JSON parse error
/// on line `idx` of the JSONL document at `idx + e.line` — mirrors AOT's
/// `jet_std_jsonl_parse` (`MathRandomTime.rs`), which adds the 0-based JSONL
/// line index to the per-line JSON parser's own line number.
/// parity: guard tests/encoding_parity.rs::jsonl_csv_xml_cbor_streams_match_aot_and_default_dev
pub(super) fn encoding_error_value_at_line(
    e: jet_foundation::EncodingJson::Error,
    line_offset: i64,
) -> CtValue {
    super::EncodingLite::encoding_error_value_at(
        "JSONL",
        "Syntax",
        line_offset + e.line,
        1,
        e.message,
    )
}

/// Diagnostic display is not a JSON wire projection. Float leaves use exactly
/// the same formatter as print(Float), including NaN and infinities.
pub(super) fn display_dyn(value: &CtValue) -> String {
    display_diagnostic::<false, true>(value)
}

/// Preserve CtValue's finite wire-shaped bytes for diagnostic CSV cells,
/// including nominal tags nested inside generic values.
pub(super) fn display_ct_json(value: &CtValue) -> String {
    display_diagnostic::<false, false>(value)
}

/// Query cells historically used the sorted dynamic JSON-shaped projection,
/// not CtValue's wire representation. Preserve those finite bytes while
/// allowing the same canonical Float display leaves as other diagnostics.
pub(super) fn display_query_cell(value: &CtValue) -> String {
    display_diagnostic::<true, false>(value)
}

fn display_diagnostic<const QUERY: bool, const TREE: bool>(value: &CtValue) -> String {
    fn append<const TREE: bool>(out: &mut String, value: &CtValue) {
        match value {
            CtValue::Enum {
                type_name,
                variant,
                args,
            } if TREE && type_name == "Dyn" => match (variant.as_str(), args.first()) {
                ("Null", _) | (_, None) => out.push_str("null"),
                ("Number", Some((_, CtValue::Str(text)))) => out.push_str(text),
                ("Int", Some((_, CtValue::BigInt(number)))) => {
                    out.push_str(&number.to_string_rep());
                }
                ("Object", Some((_, CtValue::List(entries)))) => {
                    out.push('{');
                    for (index, entry) in entries.iter().enumerate() {
                        if index != 0 {
                            out.push(',');
                        }
                        if let CtValue::Struct { fields, .. } = entry {
                            if let (Some((_, key)), Some((_, value))) = (
                                fields.iter().find(|(name, _)| name == "key"),
                                fields.iter().find(|(name, _)| name == "value"),
                            ) {
                                jet_foundation::EncodingJson::append_quote_json(
                                    out,
                                    &key.jet_show(),
                                );
                                out.push(':');
                                append::<true>(out, value);
                            }
                        }
                    }
                    out.push('}');
                }
                (_, Some((_, payload))) => append::<true>(out, payload),
            },
            CtValue::List(items) => {
                out.push('[');
                for (index, value) in items.iter().enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    append::<TREE>(out, value);
                }
                out.push(']');
            }
            CtValue::Map(entries) => {
                out.push('{');
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    match key {
                        CtKey::Str(text) => {
                            jet_foundation::EncodingJson::append_quote_json(out, text)
                        }
                        other => append::<false>(out, &other.to_value()),
                    }
                    out.push(':');
                    append::<TREE>(out, value);
                }
                out.push('}');
            }
            CtValue::Struct { type_name, fields } => {
                out.push('{');
                let mut first = true;
                for (key, value) in fields {
                    if crate::Syntax::is_memo_storage_name(key) {
                        continue;
                    }
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    jet_foundation::EncodingJson::append_quote_json(out, key);
                    out.push(':');
                    if TREE && type_name == "JSONObject" {
                        append::<true>(out, value);
                    } else {
                        append::<false>(out, value);
                    }
                }
                out.push('}');
            }
            CtValue::Enum { variant, args, .. } => {
                if args.is_empty() {
                    jet_foundation::EncodingJson::append_quote_json(out, variant);
                } else {
                    let labeled = args.iter().all(|(label, _)| label.is_some());
                    out.push('{');
                    jet_foundation::EncodingJson::append_quote_json(out, variant);
                    out.push(':');
                    out.push(if labeled { '{' } else { '[' });
                    for (index, (label, value)) in args.iter().enumerate() {
                        if index != 0 {
                            out.push(',');
                        }
                        if labeled {
                            jet_foundation::EncodingJson::append_quote_json(
                                out,
                                label.as_deref().unwrap(),
                            );
                            out.push(':');
                        }
                        append::<false>(out, value);
                    }
                    out.push(if labeled { '}' } else { ']' });
                    out.push('}');
                }
            }
            CtValue::Present(value) => append::<false>(out, value),
            CtValue::Failed(CtReport::Told(error)) => {
                out.push_str("{\"err\":");
                append::<false>(out, error);
                out.push('}');
            }
            CtValue::Unit | CtValue::Failed(CtReport::Clean(_)) => out.push_str("null"),
            CtValue::Float(value) => write!(out, "{value:?}").unwrap(),
            // All remaining leaves are finite scalar/Bytes values; use the
            // canonical Ct wire kernel to preserve their established bytes.
            scalar @ (CtValue::Int(_)
            | CtValue::Bool(_)
            | CtValue::Char(_)
            | CtValue::Str(_)
            | CtValue::BigInt(_)
            | CtValue::Bytes(_)
            | CtValue::Closure(_)) => scalar.append_json(out).unwrap(),
        }
    }
    fn append_query(out: &mut String, value: &CtValue) {
        match value {
            CtValue::Enum {
                type_name,
                variant,
                args,
            } if matches!(type_name.as_str(), "Dyn" | "TOML" | "YAML" | "CSV") => {
                match (variant.as_str(), args.first()) {
                    ("Null", _) | (_, None) => out.push_str("null"),
                    (_, Some((_, payload))) => append_query(out, payload),
                }
            }
            CtValue::BigInt(number) => out.push_str(&number.to_string_rep()),
            CtValue::List(items) => {
                out.push('[');
                for (index, value) in items.iter().enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    append_query(out, value);
                }
                out.push(']');
            }
            CtValue::Map(entries) => {
                out.push('{');
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    match key {
                        CtKey::Str(text) => {
                            jet_foundation::EncodingJson::append_quote_json(out, text)
                        }
                        other => jet_foundation::EncodingJson::append_quote_json(
                            out,
                            &other.to_value().jet_show(),
                        ),
                    }
                    out.push(':');
                    append_query(out, value);
                }
                out.push('}');
            }
            CtValue::Struct { type_name, fields } if type_name == "JSONObject" => {
                let mut sorted: Vec<_> = fields.iter().collect();
                sorted.sort_by(|a, b| a.0.cmp(&b.0));
                out.push('{');
                for (index, (key, value)) in sorted.into_iter().enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    jet_foundation::EncodingJson::append_quote_json(out, key);
                    out.push(':');
                    append_query(out, value);
                }
                out.push('}');
            }
            other => append::<false>(out, other),
        }
    }
    let mut out = String::new();
    if QUERY {
        append_query(&mut out, value);
    } else {
        append::<TREE>(&mut out, value);
    }
    out
}

/// Render the ordered `Dyn` representation used by typed codecs. Dynamic
/// values use the same canonical tree; published-schema output retains the
/// wire order stored in `JSONObject`.
pub(super) fn render_ordered_dyn(v: &CtValue, pretty: bool, depth: usize) -> Result<String, EncodingError> {
    Ok(match v {
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if matches!(type_name.as_str(), "Dyn" | "TOML" | "YAML" | "CSV") =>
        {
            match variant.as_str() {
                "Null" => "null".to_string(),
                // Typed JSON keeps a lexical Number payload so published-schema
                // unknown fields can round-trip without turning `2` into `"2"`.
                "Number" => args
                    .first()
                    .and_then(|(_, payload)| match payload {
                        CtValue::Str(value) => Some(value.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "null".to_string()),
                "Object" => args
                    .first()
                    .map(|(_, payload)| render_ordered_object_payload(payload, pretty, depth)).transpose()?
                    .unwrap_or_else(|| "{}".to_string()),
                _ => args
                    .first()
                    .map(|(_, payload)| render_ordered_dyn(payload, pretty, depth)).transpose()?
                    .unwrap_or_else(|| "null".to_string()),
            }
        }
        CtValue::List(items) => {
            if items.is_empty() {
                return Ok("[]".to_string());
            }
            if !pretty {
                return Ok(format!(
                    "[{}]",
                    items
                        .iter()
                        .map(|item| render_ordered_dyn(item, false, depth))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(",")
                ));
            }
            let pad = "  ".repeat(depth + 1);
            let end = "  ".repeat(depth);
            let parts = items
                .iter()
                .map(|item| Ok(format!("{}{}", pad, render_ordered_dyn(item, true, depth + 1)?)))
                .collect::<Result<Vec<_>, EncodingError>>()?;
            format!("[\n{}\n{}]", parts.join(",\n"), end)
        }
        CtValue::Struct { type_name, fields } if type_name == "JSONObject" => {
            render_ordered_object(fields, pretty, depth)?
        }
        _ => render_json_pretty(v, pretty, depth)?,
    })
}

fn render_ordered_object_payload(payload: &CtValue, pretty: bool, depth: usize) -> Result<String, EncodingError> {
    let fields = match payload {
        CtValue::Map(entries) => entries
            .iter()
            .map(|(key, value)| match key {
                CtKey::Str(key) => Some((key.clone(), value.clone())),
                _ => None,
            })
            .collect::<Option<Vec<_>>>(),
        CtValue::Struct { type_name, fields } if type_name == "JSONObject" => {
            return render_ordered_object(fields, pretty, depth);
        }
        CtValue::List(values) => values
            .iter()
            .map(|value| {
                let CtValue::Struct { fields, .. } = value else {
                    return None;
                };
                let key = fields
                    .iter()
                    .find(|(name, _)| name == "key")
                    .map(|(_, value)| value)?;
                let value = fields
                    .iter()
                    .find(|(name, _)| name == "value")
                    .map(|(_, value)| value)?;
                let CtValue::Str(key) = key else {
                    return None;
                };
                Some((key.clone(), value.clone()))
            })
            .collect::<Option<Vec<_>>>(),
        _ => None,
    };
    match fields {
        Some(fields) => render_ordered_object(&fields, pretty, depth),
        None => render_json_pretty(payload, pretty, depth),
    }
}

fn render_ordered_object(fields: &[(String, CtValue)], pretty: bool, depth: usize) -> Result<String, EncodingError> {
    if fields.is_empty() {
        return Ok("{}".to_string());
    }
    if !pretty {
        return Ok(format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(key, value)| {
                    Ok(format!(
                        "{}:{}",
                        quote_json(key),
                        render_ordered_dyn(value, false, depth)?
                    ))
                })
                .collect::<Result<Vec<_>, EncodingError>>()?
                .join(",")
        ));
    }
    let pad = "  ".repeat(depth + 1);
    let end = "  ".repeat(depth);
    let parts = fields
        .iter()
        .map(|(key, value)| {
            Ok(format!(
                "{}{}: {}",
                pad,
                quote_json(key),
                render_ordered_dyn(value, true, depth + 1)?
            ))
        })
        .collect::<Result<Vec<_>, EncodingError>>()?;
    Ok(format!("{{\n{}\n{}}}", parts.join(",\n"), end))
}

pub(super) fn render_json_pretty(v: &CtValue, pretty: bool, depth: usize) -> Result<String, EncodingError> {
    Ok(match v {
        // D-SERDE-ACCESS=B: a `Dyn` value (from `.parse()`, or built by
        // hand with `Dyn.Text(…)`/`Dyn.Object(…)`) — unwrap the tag
        // and render its payload the same way the untagged shapes below do.
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if matches!(type_name.as_str(), "Dyn" | "TOML" | "YAML" | "CSV") =>
        {
            match variant.as_str() {
                "Null" => "null".to_string(),
                _ => match args.first() {
                    Some((_, payload)) => render_json_pretty(payload, pretty, depth)?,
                    None => "null".to_string(),
                },
            }
        }
        CtValue::Unit => "null".to_string(),
        CtValue::Bool(b) => b.to_string(),
        CtValue::Int(n) => n.to_string(),
        CtValue::Float(f) => f.to_json()?,
        CtValue::BigInt(value) => value.to_string_rep(),
        CtValue::Str(s) => quote_json(s),
        CtValue::List(xs) => {
            if xs.is_empty() {
                return Ok("[]".to_string());
            }
            if !pretty {
                let parts: Vec<String> = xs
                    .iter()
                    .map(|x| render_json_pretty(x, false, depth))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(format!("[{}]", parts.join(",")));
            }
            let pad = "  ".repeat(depth + 1);
            let end = "  ".repeat(depth);
            let parts: Vec<String> = xs
                .iter()
                .map(|x| Ok(format!("{}{}", pad, render_json_pretty(x, true, depth + 1)?)))
                .collect::<Result<Vec<_>, EncodingError>>()?;
            format!("[\n{}\n{}]", parts.join(",\n"), end)
        }
        CtValue::Map(m) => {
            if m.is_empty() {
                return Ok("{}".to_string());
            }
            if !pretty {
                let parts: Vec<String> = m
                    .iter()
                    .map(|(k, v)| {
                        let CtKey::Str(key) = k else {
                            return Err(json_unsupported("JSON cannot encode non-text map keys"));
                        };
                        Ok(format!("{}:{}", quote_json(key), render_json_pretty(v, false, depth)?))
                    })
                    .collect::<Result<Vec<_>, EncodingError>>()?;
                return Ok(format!("{{{}}}", parts.join(",")));
            }
            let pad = "  ".repeat(depth + 1);
            let end = "  ".repeat(depth);
            let parts: Vec<String> = m
                .iter()
                .map(|(k, v)| {
                    let CtKey::Str(key) = k else {
                        return Err(json_unsupported("JSON cannot encode non-text map keys"));
                    };
                    Ok(format!("{}{}: {}", pad, quote_json(key), render_json_pretty(v, true, depth + 1)?))
                })
                .collect::<Result<Vec<_>, EncodingError>>()?;
            format!("{{\n{}\n{}}}", parts.join(",\n"), end)
        }
        // EncodingLite Object payload: insertion-ordered `Struct` (AOT Dyn).
        // Sort keys so `json.canonical` matches BTreeMap / AOT sorted form.
        CtValue::Struct { type_name, fields } if type_name == "JSONObject" => {
            let mut fields = fields.iter().collect::<Vec<_>>();
            fields.sort_by(|a, b| a.0.cmp(&b.0));
            if fields.is_empty() {
                return Ok("{}".to_string());
            }
            if !pretty {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(k, v)| {
                        Ok(format!("{}:{}", quote_json(k), render_json_pretty(v, false, depth)?))
                    })
                    .collect::<Result<Vec<_>, EncodingError>>()?;
                return Ok(format!("{{{}}}", parts.join(",")));
            }
            let pad = "  ".repeat(depth + 1);
            let end = "  ".repeat(depth);
            let parts: Vec<String> = fields
                .iter()
                .map(|(k, v)| {
                    Ok(format!(
                        "{}{}: {}",
                        pad,
                        quote_json(k),
                        render_json_pretty(v, true, depth + 1)?
                    ))
                })
                .collect::<Result<Vec<_>, EncodingError>>()?;
            format!("{{\n{}\n{}}}", parts.join(",\n"), end)
        }
        other => other.to_json()?,
    })
}

fn quote_json(s: &str) -> String {
    jet_foundation::JSON::quote(s)
}
#[cfg(test)]
mod tests {
    use super::*;

    fn nested_json(depth: usize) -> String {
        format!("{}{}", "[".repeat(depth), "]".repeat(depth))
    }

    #[test]
    fn comptime_json_routes_share_the_depth_boundary() {
        type ParseRoute = fn(&str) -> Result<CtValue, jet_foundation::EncodingJson::Error>;
        let routes: &[(&str, ParseRoute)] = &[("parse_json", parse_json)];

        for (name, parse) in routes {
            let boundary = nested_json(jet_foundation::EncodingJson::MAX_JSON_DEPTH);
            assert!(
                parse(&boundary).is_ok(),
                "{name} must accept the maximum JSON nesting depth"
            );

            let too_deep = nested_json(jet_foundation::EncodingJson::MAX_JSON_DEPTH + 1);
            let error = parse(&too_deep).expect_err("{name} must reject excessive nesting");
            assert_eq!(error.line, 1, "{name} reported the wrong error line");
            assert_eq!(
                error.message, "JSON value is nested too deeply",
                "{name} changed the shared depth-limit error"
            );
        }
    }
    #[test]
    fn text_renderers_reject_non_finite_and_non_text_keys_but_display_does_not() {
        use jet_foundation::EncodingJson::EncodingErrorKind;
        for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let leaf = CtValue::Float(CtFloat::f64(number));
            let tree = json_variant("Array", Some(CtValue::List(vec![leaf.clone()])));
            for pretty in [false, true] {
                let error = render_ordered_dyn(&tree, pretty, 0).unwrap_err();
                assert_eq!(error.kind, EncodingErrorKind::Unsupported);
                assert_eq!(error.reason, "JSON cannot encode a non-finite Float");
            }
            assert_eq!(display_dyn(&tree), format!("[{}]", leaf.jet_show()));
        }
        let map = CtValue::Map([(CtKey::Int(1), CtValue::Int(2))].into_iter().collect());
        for pretty in [false, true] {
            let error = render_ordered_dyn(&map, pretty, 0).unwrap_err();
            assert_eq!(error.reason, "JSON cannot encode non-text map keys");
        }
        let ordered = json_object(vec![
            ("z".into(), CtValue::Int(1)),
            ("a".into(), CtValue::Char('\n')),
        ]);
        assert_eq!(
            render_ordered_dyn(&ordered, false, 0).unwrap(),
            "{\"z\":1,\"a\":\"\\n\"}"
        );
        assert_eq!(display_dyn(&ordered), "{\"z\":1,\"a\":\"\\n\"}");
    }

    #[test]
    fn parsed_dyn_display_unwraps_nested_ordered_object_carriers() {
        let text = "{\"z\":\"x\",\"a\":[{\"inside\":1.5},null]}";
        let parsed = parse_json(text).unwrap();
        assert_eq!(display_dyn(&parsed), text);
        for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let leaf = CtValue::Float(CtFloat::f64(number));
            let tree = json_object(vec![
                (
                    "z".into(),
                    json_variant("Text", Some(CtValue::Str("x".into()))),
                ),
                (
                    "a".into(),
                    json_variant(
                        "Array",
                        Some(CtValue::List(vec![json_object(vec![(
                            "inside".into(),
                            json_variant("Float", Some(leaf.clone())),
                        )])])),
                    ),
                ),
            ]);
            assert_eq!(
                display_dyn(&tree),
                format!("{{\"z\":\"x\",\"a\":[{{\"inside\":{}}}]}}", leaf.jet_show())
            );
        }
    }

    #[test]
    fn diagnostic_cells_keep_finite_ct_json_bytes() {
        let nominal = CtValue::Enum {
            type_name: "Choice".into(),
            variant: "Value".into(),
            args: vec![(None, CtValue::Bytes(vec![0, 10, 255]))],
        };
        let record = CtValue::Struct {
            type_name: "Row".into(),
            fields: vec![
                ("value".into(), nominal.clone()),
                (
                    crate::Syntax::memo_storage_name("cached"),
                    CtValue::Float(CtFloat::f64(f64::NAN)),
                ),
            ],
        };
        let report = CtValue::failed(Box::new(record.clone()));
        for value in [
            nominal,
            record,
            report,
            CtValue::Present(Box::new(CtValue::Char('\0'))),
            CtValue::Bytes(vec![0, 10, 255]),
            CtValue::BigInt(crate::Numeric::CtBigInt::from_str("9007199254740993").unwrap()),
            CtValue::Present(Box::new(json_variant(
                "Text",
                Some(CtValue::Str("x".into())),
            ))),
            CtValue::Enum {
                type_name: "Choice".into(),
                variant: "Tree".into(),
                args: vec![(None, json_variant("Text", Some(CtValue::Str("x".into()))))],
            },
            CtValue::List(vec![
                json_variant("Text", Some(CtValue::Str("x".into()))),
                CtValue::BigInt(crate::Numeric::CtBigInt::from_str("9007199254740993").unwrap()),
            ]),
            CtValue::Struct {
                type_name: "JSONObject".into(),
                fields: vec![("z".into(), CtValue::Int(1)), ("a".into(), CtValue::Int(2))],
            },
            CtValue::Map(
                [(
                    CtKey::Str("tree".into()),
                    json_variant("Text", Some(CtValue::Str("x".into()))),
                )]
                .into_iter()
                .collect(),
            ),
        ] {
            assert_eq!(display_ct_json(&value), value.to_json().unwrap());
        }
    }

    #[test]
    fn diagnostic_reports_use_canonical_float_leaves_without_wire_fallback() {
        for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let leaf = CtValue::Float(CtFloat::f64(number));
            let report = CtValue::failed(Box::new(CtValue::Enum {
                type_name: "Report".into(),
                variant: "Value".into(),
                args: vec![(Some("float".into()), leaf.clone())],
            }));
            assert_eq!(
                display_ct_json(&report),
                format!(
                    "{{\"err\":{{\"Value\":{{\"float\":{}}}}}}}",
                    leaf.jet_show()
                )
            );
            assert!(report.to_json().is_err());
        }
    }

    #[test]
    fn query_cells_keep_their_distinct_finite_projection() {
        let big = CtValue::BigInt(crate::Numeric::CtBigInt::from_str("9007199254740993").unwrap());
        let object = CtValue::Struct {
            type_name: "JSONObject".into(),
            fields: vec![("z".into(), CtValue::Int(1)), ("a".into(), CtValue::Int(2))],
        };
        let cases = [
            (CtValue::List(vec![big]), "[9007199254740993]"),
            (
                CtValue::Map([(CtKey::Int(1), CtValue::Int(2))].into_iter().collect()),
                "{\"1\":2}",
            ),
            (object, "{\"a\":2,\"z\":1}"),
            (
                CtValue::Present(Box::new(json_variant(
                    "Text",
                    Some(CtValue::Str("x".into())),
                ))),
                "{\"Text\":[\"x\"]}",
            ),
        ];
        for (value, expected) in cases {
            assert_eq!(
                super::super::DataPipeline::query_cell_text(&value),
                expected
            );
        }
        for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let leaf = CtValue::Float(CtFloat::f64(number));
            let list = CtValue::List(vec![leaf.clone()]);
            assert_eq!(
                super::super::DataPipeline::query_cell_text(&list),
                format!("[{}]", leaf.jet_show())
            );
        }
    }
}
