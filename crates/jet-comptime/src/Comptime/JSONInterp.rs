//! JSON rendering and value conversion for the comptime/REPL interpreter.
//! Parsing uses the same foundation-backed Prelude kernel as AOT and JIT.

use crate::Comptime::Builtins::exact_int_value;
use crate::AST::{CtFloat, CtKey, CtValue};
use jet_foundation::EncodingJson::{json_unsupported, EncodingError};

fn from_json_int(value: i64) -> CtValue {
    exact_int_value(crate::Numeric::CtBigInt::from_int(value))
}

fn from_json_number(value: String) -> CtValue {
    let number = crate::Numeric::CtBigInt::from_str(&value)
        .expect("dynamic JSON integer token must be valid exact integer text");
    exact_int_value(number)
}

fn from_json(value: jet_foundation::DataTree::DataTree) -> CtValue {
    match value {
        jet_foundation::DataTree::DataTree::Null => json_variant("Null", None),
        jet_foundation::DataTree::DataTree::Bool(value) => {
            json_variant("Bool", Some(CtValue::Bool(value)))
        }
        jet_foundation::DataTree::DataTree::Int(value) => {
            json_variant("Int", Some(from_json_int(value)))
        }
        jet_foundation::DataTree::DataTree::Float(value) => {
            json_variant("Float", Some(CtValue::Float(CtFloat::f64(value))))
        }
        jet_foundation::DataTree::DataTree::Number(value) => {
            json_variant("Int", Some(from_json_number(value)))
        }
        jet_foundation::DataTree::DataTree::TypedText(value)
        | jet_foundation::DataTree::DataTree::Text(value) => {
            json_variant("Text", Some(CtValue::Str(value)))
        }
        jet_foundation::DataTree::DataTree::Array(values) => json_variant(
            "Array",
            Some(CtValue::List(values.into_iter().map(from_json).collect())),
        ),
        jet_foundation::DataTree::DataTree::Object(fields) => json_object(
            fields
                .into_iter()
                .map(|(key, value)| (key, from_json(value)))
                .collect(),
        ),
        jet_foundation::DataTree::DataTree::Bytes(_) => {
            unreachable!("JSON parser does not produce byte nodes")
        }
    }
}

fn from_ordered_json(value: jet_foundation::DataTree::DataTree) -> CtValue {
    match value {
        jet_foundation::DataTree::DataTree::Object(fields) => json_object(
            fields
                .into_iter()
                .map(|(key, value)| (key, from_ordered_json(value)))
                .collect(),
        ),
        jet_foundation::DataTree::DataTree::Null => json_variant("Null", None),
        jet_foundation::DataTree::DataTree::Bool(value) => {
            json_variant("Bool", Some(CtValue::Bool(value)))
        }
        jet_foundation::DataTree::DataTree::Int(value) => {
            json_variant("Int", Some(from_json_int(value)))
        }
        jet_foundation::DataTree::DataTree::Float(value) => {
            json_variant("Float", Some(CtValue::Float(CtFloat::f64(value))))
        }
        jet_foundation::DataTree::DataTree::Number(value) => {
            json_variant("Int", Some(from_json_number(value)))
        }
        jet_foundation::DataTree::DataTree::TypedText(value)
        | jet_foundation::DataTree::DataTree::Text(value) => {
            json_variant("Text", Some(CtValue::Str(value)))
        }
        jet_foundation::DataTree::DataTree::Array(values) => json_variant(
            "Array",
            Some(CtValue::List(
                values.into_iter().map(from_ordered_json).collect(),
            )),
        ),
        jet_foundation::DataTree::DataTree::Bytes(_) => {
            unreachable!("JSON parser does not produce byte nodes")
        }
    }
}

fn from_typed_ordered_json(value: jet_foundation::DataTree::DataTree) -> CtValue {
    match value {
        jet_foundation::DataTree::DataTree::Object(fields) => json_object(
            fields
                .into_iter()
                .map(|(key, value)| (key, from_typed_ordered_json(value)))
                .collect(),
        ),
        jet_foundation::DataTree::DataTree::Null => json_variant("Null", None),
        jet_foundation::DataTree::DataTree::Bool(value) => {
            json_variant("Bool", Some(CtValue::Bool(value)))
        }
        jet_foundation::DataTree::DataTree::Number(value) => {
            json_variant("Number", Some(CtValue::Str(value)))
        }
        jet_foundation::DataTree::DataTree::TypedText(value)
        | jet_foundation::DataTree::DataTree::Text(value) => {
            json_variant("TypedText", Some(CtValue::Str(value)))
        }
        jet_foundation::DataTree::DataTree::Array(values) => json_variant(
            "Array",
            Some(CtValue::List(
                values.into_iter().map(from_typed_ordered_json).collect(),
            )),
        ),
        jet_foundation::DataTree::DataTree::Int(_)
        | jet_foundation::DataTree::DataTree::Float(_)
        | jet_foundation::DataTree::DataTree::Bytes(_) => {
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

/// D-SERDE-ACCESS=B / D-DYNAMIC-TYPE1=A: build one node of the canonical
/// `DataTree` dynamic-value tree — a `CtValue::Enum` so it round-trips through
/// the exact same pattern-matching (`data == .Object(entries)`, S31) and
/// explicit construction (`DataTree.Text("jet")`) machinery a user enum already
/// gets, with no interpreter-specific special case needed on either path.
pub(super) fn json_variant(variant: &str, payload: Option<CtValue>) -> CtValue {
    CtValue::Enum {
        type_name: "DataTree".to_string(),
        variant: variant.to_string(),
        args: match payload {
            Some(v) => vec![(None, v)],
            None => Vec::new(),
        },
    }
}

/// The payload of a `DataTree`-tagged `CtValue` with the given variant name, or
/// `None` if `v` isn't that shape (used by the `.field`/`.at`/`.int`/`.text`/
/// `.bool`/`.float` accessor methods in `Builtins.rs`).
pub(super) fn json_payload<'a>(v: &'a CtValue, variant: &str) -> Option<&'a CtValue> {
    match v {
        CtValue::Enum {
            type_name,
            variant: vname,
            args,
        } if type_name == "DataTree" && vname == variant => args.first().map(|(_, v)| v),
        _ => None,
    }
}

pub(super) fn parse_json(text: &str) -> Result<CtValue, jet_foundation::EncodingJson::Error> {
    jet_foundation::EncodingJson::parse_json(text, true).map(from_json)
}

pub(super) fn parse_json_ordered(
    text: &str,
) -> Result<CtValue, jet_foundation::EncodingJson::Error> {
    jet_foundation::EncodingJson::parse_json(text, true).map(from_ordered_json)
}

pub(super) fn parse_json_typed_ordered(
    text: &str,
) -> Result<CtValue, jet_foundation::EncodingJson::Error> {
    jet_foundation::EncodingJson::parse_json_typed(text, true).map(from_typed_ordered_json)
}

pub(super) fn encoding_error_value(e: jet_foundation::EncodingJson::Error) -> CtValue {
    super::EncodingLite::encoding_error_value_at(
        "JSON",
        "Syntax",
        e.line,
        1,
        e.message,
    )
}

pub(super) fn text_encoding_error_value(error: EncodingError) -> CtValue {
    super::EncodingLite::encoding_error_value_fields(
        error.format.as_str(), &format!("{:?}", error.kind), error.byte_offset,
        error.line.ok(), error.column.ok(), error.path, error.reason,
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
pub(super) fn display_datatree(value: &CtValue) -> String {
    fn append(out: &mut String, value: &CtValue) {
        match value {
            CtValue::Enum { type_name, variant, args } if type_name == "DataTree" => {
                match (variant.as_str(), args.first()) {
                    ("Null", _) | (_, None) => out.push_str("null"),
                    ("Number", Some((_, CtValue::Str(text)))) => out.push_str(text),
                    ("Object", Some((_, CtValue::List(entries)))) => {
                        out.push('{');
                        for (index, entry) in entries.iter().enumerate() {
                            if index != 0 { out.push(','); }
                            if let CtValue::Struct { fields, .. } = entry {
                                if let (Some((_, key)), Some((_, value))) = (
                                    fields.iter().find(|(name, _)| name == "key"),
                                    fields.iter().find(|(name, _)| name == "value"),
                                ) {
                                    jet_foundation::EncodingJson::append_quote_json(out, &key.jet_show());
                                    out.push(':');
                                    append(out, value);
                                }
                            }
                        }
                        out.push('}');
                    }
                    (_, Some((_, payload))) => append(out, payload),
                }
            }
            CtValue::List(items) => {
                out.push('[');
                for (index, value) in items.iter().enumerate() {
                    if index != 0 { out.push(','); }
                    append(out, value);
                }
                out.push(']');
            }
            CtValue::Map(entries) => {
                out.push('{');
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index != 0 { out.push(','); }
                    jet_foundation::EncodingJson::append_quote_json(out, &key.to_value().jet_show());
                    out.push(':');
                    append(out, value);
                }
                out.push('}');
            }
            CtValue::Struct { fields, .. } => {
                out.push('{');
                for (index, (key, value)) in fields.iter().enumerate() {
                    if index != 0 { out.push(','); }
                    jet_foundation::EncodingJson::append_quote_json(out, key);
                    out.push(':');
                    append(out, value);
                }
                out.push('}');
            }
            CtValue::Str(text) => jet_foundation::EncodingJson::append_quote_json(out, text),
            CtValue::Char(character) => {
                let mut bytes = [0; 4];
                jet_foundation::EncodingJson::append_quote_json(out, character.encode_utf8(&mut bytes));
            }
            CtValue::Unit => out.push_str("null"),
            other => out.push_str(&other.jet_show()),
        }
    }
    let mut out = String::new();
    append(&mut out, value);
    out
}

/// Render the ordered `DataTree` representation used by typed codecs. Dynamic
/// values use the same canonical tree; published-schema output retains the
/// wire order stored in `JSONObject`.
pub(super) fn render_ordered_datatree(
    value: &CtValue, pretty: bool, depth: usize,
) -> Result<String, EncodingError> {
    render_json(value, pretty, depth, false)
}

pub(super) fn render_json_pretty(
    value: &CtValue, pretty: bool, depth: usize,
) -> Result<String, EncodingError> {
    render_json(value, pretty, depth, true)
}

fn render_json(
    value: &CtValue, pretty: bool, depth: usize, sort_keys: bool,
) -> Result<String, EncodingError> {
    let mut out = String::new();
    append_json(&mut out, value, pretty, depth, sort_keys)?;
    Ok(out)
}

fn append_indent(out: &mut String, depth: usize) {
    for _ in 0..depth { out.push_str("  "); }
}

fn append_object<'a>(
    out: &mut String, mut fields: Vec<(&'a str, &'a CtValue)>,
    pretty: bool, depth: usize, sort_keys: bool,
) -> Result<(), EncodingError> {
    if sort_keys { fields.sort_by(|a, b| a.0.cmp(b.0)); }
    out.push('{');
    for (index, (key, value)) in fields.iter().enumerate() {
        if index != 0 { out.push(','); }
        if pretty { out.push('\n'); append_indent(out, depth + 1); }
        jet_foundation::EncodingJson::append_quote_json(out, key);
        out.push(':');
        if pretty { out.push(' '); }
        append_json(out, value, pretty, depth + 1, sort_keys)?;
    }
    if pretty && !fields.is_empty() { out.push('\n'); append_indent(out, depth); }
    out.push('}');
    Ok(())
}

fn append_json(
    out: &mut String, value: &CtValue, pretty: bool, depth: usize, sort_keys: bool,
) -> Result<(), EncodingError> {
    match value {
        CtValue::Enum { type_name, variant, args }
            if matches!(type_name.as_str(), "DataTree" | "TOML" | "YAML" | "CSV") => {
            match (variant.as_str(), args.first()) {
                ("Null", _) | (_, None) => out.push_str("null"),
                ("Number", Some((_, CtValue::Str(number)))) => out.push_str(number),
                ("Object", Some((_, CtValue::List(entries)))) => {
                    let mut fields = Vec::with_capacity(entries.len());
                    for entry in entries {
                        let CtValue::Struct { fields: row, .. } = entry else {
                            return Err(json_unsupported("JSON object entries require text keys"));
                        };
                        let key = row.iter().find(|(name, _)| name == "key");
                        let value = row.iter().find(|(name, _)| name == "value");
                        match (key, value) {
                            (Some((_, CtValue::Str(key))), Some((_, value))) => {
                                fields.push((key.as_str(), value));
                            }
                            _ => return Err(json_unsupported("JSON cannot encode non-text map keys")),
                        }
                    }
                    append_object(out, fields, pretty, depth, sort_keys)?;
                }
                (_, Some((_, payload))) => append_json(out, payload, pretty, depth, sort_keys)?,
            }
        }
        CtValue::List(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index != 0 { out.push(','); }
                if pretty { out.push('\n'); append_indent(out, depth + 1); }
                append_json(out, item, pretty, depth + 1, sort_keys)?;
            }
            if pretty && !items.is_empty() { out.push('\n'); append_indent(out, depth); }
            out.push(']');
        }
        CtValue::Map(entries) => {
            let mut fields = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                let CtKey::Str(key) = key else {
                    return Err(json_unsupported("JSON cannot encode non-text map keys"));
                };
                fields.push((key.as_str(), value));
            }
            append_object(out, fields, pretty, depth, sort_keys)?;
        }
        CtValue::Struct { type_name, fields } if type_name == "JSONObject" => {
            append_object(out, fields.iter().map(|(key, value)| (key.as_str(), value)).collect(),
                pretty, depth, sort_keys)?;
        }
        // DataTree's exact Int may spill into BigInt; keep its JSON number exact.
        CtValue::BigInt(number) => out.push_str(&number.to_string_rep()),
        other => other.append_json(out)?,
    }
    Ok(())
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
        let routes: &[(&str, ParseRoute)] = &[
            ("parse_json", parse_json),
            ("parse_json_ordered", parse_json_ordered),
            ("parse_json_typed_ordered", parse_json_typed_ordered),
        ];

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
                let error = render_ordered_datatree(&tree, pretty, 0).unwrap_err();
                assert_eq!(error.kind, EncodingErrorKind::Unsupported);
                assert_eq!(error.reason, "JSON cannot encode a non-finite Float");
            }
            assert_eq!(display_datatree(&tree), format!("[{}]", leaf.jet_show()));
        }
        let map = CtValue::Map([(CtKey::Int(1), CtValue::Int(2))].into_iter().collect());
        for pretty in [false, true] {
            let error = render_ordered_datatree(&map, pretty, 0).unwrap_err();
            assert_eq!(error.reason, "JSON cannot encode non-text map keys");
        }
        let ordered = json_object(vec![
            ("z".into(), CtValue::Int(1)), ("a".into(), CtValue::Char('\n')),
        ]);
        assert_eq!(render_ordered_datatree(&ordered, false, 0).unwrap(),
            "{\"z\":1,\"a\":\"\\n\"}");
        assert_eq!(display_datatree(&ordered), "{\"z\":1,\"a\":\"\\n\"}");
    }
}
