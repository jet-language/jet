use super::*;

pub(super) fn named_tuple(fields: &[(&str, CtValue)]) -> CtValue {
    CtValue::Struct {
        type_name: format!(
            "({})",
            fields.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(",")
        ),
        fields: fields
            .iter()
            .map(|(n, v)| ((*n).to_string(), v.clone()))
            .collect(),
    }
}

pub(crate) fn as_string(v: &CtValue, span: Span) -> Result<&str, Diagnostic> {
    match v {
        CtValue::Str(s) => Ok(s.as_str()),
        _ => Err(unsupported(
            "non-string argument to a Core string call",
            span,
        )),
    }
}

pub(super) fn as_string_rows(v: &CtValue, span: Span) -> Result<Vec<Vec<String>>, Diagnostic> {
    match v {
        CtValue::List(rows) => rows
            .iter()
            .map(|row| match row {
                CtValue::List(cols) => cols
                    .iter()
                    .map(|c| Ok(as_string(c, span)?.to_string()))
                    .collect::<Result<Vec<_>, _>>(),
                _ => Err(unsupported("rows that are not `[[String]]`", span)),
            })
            .collect(),
        _ => Err(unsupported("rows that are not `[[String]]`", span)),
    }
}

pub(super) fn csv_rows_from_records(v: &CtValue) -> Option<Vec<Vec<String>>> {
    let CtValue::List(items) = v else {
        return None;
    };
    let field_names = |value: &CtValue| match value {
        CtValue::Struct { fields, .. } => Some(
            fields
                .iter()
                .map(|(name, _)| {
                    name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                        .unwrap_or(name)
                        .to_string()
                })
                .collect::<Vec<_>>(),
        ),
        _ => None,
    };
    let header = field_names(items.first()?)?;
    if items.iter().any(|item| field_names(item).is_none()) {
        return None;
    }
    let cell = |value: &CtValue| match value {
        CtValue::Str(s) => s.clone(),
        CtValue::Int(n) => n.to_string(),
        CtValue::Float(f) => f.render(),
        CtValue::Bool(b) => b.to_string(),
        CtValue::Unit | CtValue::Failed(CtReport::Clean(_)) => String::new(),
        other => other.to_json(),
    };
    let mut rows = vec![header.clone()];
    for item in items {
        let CtValue::Struct { fields, .. } = item else {
            return None;
        };
        rows.push(
            header
                .iter()
                .map(|key| {
                    fields
                        .iter()
                        .find(|(name, _)| {
                            name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                                .unwrap_or(name)
                                == key
                        })
                        .map(|(_, value)| cell(value))
                        .unwrap_or_default()
                })
                .collect(),
        );
    }
    Some(rows)
}

/// D-BOUND-HEAD1=A: the `core.net.url.URL` record in `Core/net/url.jet` field
/// order, projected by the URL kernel exactly as AOT's `JetURL::from_url_parts`
/// and the JIT typed head, so every tier carries one URL layout.
pub(crate) fn url_parts_to_ct(u: &super::super::super::UrlLite::UrlParts) -> CtValue {
    let (port, port_explicit) = u.core_port();
    let text = |value: &Option<String>| CtValue::Str(value.clone().unwrap_or_default());
    CtValue::Struct {
        type_name: "URL".to_string(),
        fields: vec![
            ("scheme".to_string(), CtValue::Str(u.scheme.clone())),
            ("user_text".to_string(), text(&u.username)),
            ("password_text".to_string(), text(&u.password)),
            ("host".to_string(), text(&u.host)),
            ("port".to_string(), CtValue::Int(port)),
            ("port_explicit".to_string(), CtValue::Bool(port_explicit)),
            ("path".to_string(), CtValue::Str(u.path.clone())),
            ("query".to_string(), CtValue::Str(u.query())),
            ("fragment".to_string(), text(&u.fragment)),
            ("raw".to_string(), CtValue::Str(u.to_string_value())),
        ],
    }
}

fn url_record_field<'a>(
    fields: &'a [(String, CtValue)],
    name: &str,
    span: Span,
) -> Result<&'a CtValue, Diagnostic> {
    fields
        .iter()
        .find(|(field, _)| field == name)
        .map(|(_, value)| value)
        .ok_or_else(|| unsupported(&format!("malformed URL {name}"), span))
}

fn url_record_text<'a>(
    fields: &'a [(String, CtValue)],
    name: &str,
    span: Span,
) -> Result<&'a str, Diagnostic> {
    match url_record_field(fields, name, span)? {
        CtValue::Str(value) => Ok(value.as_str()),
        _ => Err(unsupported(&format!("malformed URL {name}"), span)),
    }
}

/// Re-enter the URL kernel from the `core.net.url.URL` record fields.
pub(crate) fn url_parts_from_ct(
    value: &CtValue,
    span: Span,
) -> Result<super::super::super::UrlLite::UrlParts, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("malformed URL value", span));
    };
    let type_name = type_name
        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
        .unwrap_or(type_name);
    if type_name != "URL" {
        return Err(unsupported("malformed URL value", span));
    }
    let CtValue::Int(port) = url_record_field(fields, "port", span)? else {
        return Err(unsupported("malformed URL port", span));
    };
    let CtValue::Bool(port_explicit) = url_record_field(fields, "port_explicit", span)? else {
        return Err(unsupported("malformed URL port_explicit", span));
    };
    Ok(super::super::super::UrlLite::from_core_record(
        url_record_text(fields, "scheme", span)?,
        url_record_text(fields, "user_text", span)?,
        url_record_text(fields, "password_text", span)?,
        url_record_text(fields, "host", span)?,
        *port,
        *port_explicit,
        url_record_text(fields, "path", span)?,
        url_record_text(fields, "query", span)?,
        url_record_text(fields, "fragment", span)?,
    ))
}
