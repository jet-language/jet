use super::*;

pub(super) fn named_tuple(fields: &[(&str, CtValue)]) -> CtValue {
    CtValue::Struct {
        type_name: format!("({})", fields.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(",")),
        fields: fields.iter().map(|(n, v)| ((*n).to_string(), v.clone())).collect(),
    }
}

pub(super) fn as_string(v: &CtValue, span: Span) -> Result<&str, Diagnostic> {
    match v {
        CtValue::Str(s) => Ok(s.as_str()),
        _ => Err(unsupported("non-string argument to a Core string call", span)),
    }
}

pub(super) fn as_string_rows(v: &CtValue, span: Span) -> Result<Vec<Vec<String>>, Diagnostic> {
    match v {
        CtValue::List(rows) => rows.iter().map(|row| match row {
            CtValue::List(cols) => cols.iter().map(|c| Ok(as_string(c, span)?.to_string())).collect::<Result<Vec<_>, _>>(),
            _ => Err(unsupported("rows that are not `[[String]]`", span)),
        }).collect(),
        _ => Err(unsupported("rows that are not `[[String]]`", span)),
    }
}

pub(super) fn csv_rows_from_records(v: &CtValue) -> Option<Vec<Vec<String>>> {
    let CtValue::List(items) = v else { return None; };
    let field_names = |value: &CtValue| match value {
        CtValue::Struct { fields, .. } => Some(fields.iter().map(|(name, _)| name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX).unwrap_or(name).to_string()).collect::<Vec<_>>()),
        _ => None,
    };
    let header = field_names(items.first()?)?;
    if items.iter().any(|item| field_names(item).is_none()) { return None; }
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
        let CtValue::Struct { fields, .. } = item else { return None; };
        rows.push(header.iter().map(|key| fields.iter().find(|(name, _)| name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX).unwrap_or(name) == key).map(|(_, value)| cell(value)).unwrap_or_default()).collect());
    }
    Some(rows)
}

pub(super) const URL_INTERNAL_PREFIX: &str = "__jet_url_";
const URL_RAW_HOST: &str = "__jet_url_raw_host";
const URL_USERNAME: &str = "__jet_url_username";
const URL_PASSWORD: &str = "__jet_url_password";
const URL_TYPED_HOST: &str = "__jet_url_typed_host";
const URL_TYPED_PATH: &str = "__jet_url_typed_path";

fn url_option_string(value: Option<&String>, ty: Type) -> CtValue {
    match value { Some(value) => CtValue::Present(Box::new(CtValue::Str(value.clone()))), None => CtValue::absent(ty) }
}

fn url_typed_parts_value(parts: &[(String, bool)]) -> CtValue {
    CtValue::List(parts.iter().map(|(part, hole)| CtValue::List(vec![CtValue::Str(part.clone()), CtValue::Bool(*hole)])).collect())
}

fn url_option_string_from_ct(value: Option<&CtValue>, label: &str, span: Span) -> Result<Option<String>, Diagnostic> {
    match value {
        None | Some(CtValue::Failed(CtReport::Clean(_))) => Ok(None),
        Some(CtValue::Present(value)) => match value.as_ref() { CtValue::Str(value) => Ok(Some(value.clone())), _ => Err(unsupported(&format!("malformed URL {label}"), span)) },
        Some(_) => Err(unsupported(&format!("malformed URL {label}"), span)),
    }
}

fn url_option_int_from_ct(value: Option<&CtValue>, label: &str, span: Span) -> Result<Option<i64>, Diagnostic> {
    match value {
        None | Some(CtValue::Failed(CtReport::Clean(_))) => Ok(None),
        Some(CtValue::Present(value)) => match value.as_ref() { CtValue::Int(value) => Ok(Some(*value)), _ => Err(unsupported(&format!("malformed URL {label}"), span)) },
        Some(_) => Err(unsupported(&format!("malformed URL {label}"), span)),
    }
}

fn url_typed_parts_from_ct(value: Option<&CtValue>, label: &str, span: Span) -> Result<Option<Vec<(String, bool)>>, Diagnostic> {
    let Some(CtValue::List(rows)) = value else {
        return if value.is_none() { Ok(None) } else { Err(unsupported(&format!("malformed URL {label}"), span)) };
    };
    rows.iter().map(|row| match row {
        CtValue::List(parts) => match parts.as_slice() {
            [CtValue::Str(part), CtValue::Bool(hole)] => Ok((part.clone(), *hole)),
            _ => Err(unsupported(&format!("malformed URL {label}"), span)),
        },
        _ => Err(unsupported(&format!("malformed URL {label}"), span)),
    }).collect::<Result<Vec<_>, _>>().map(Some)
}

pub(crate) fn url_parts_to_ct(u: &super::super::super::UrlLite::UrlParts) -> CtValue {
    let mut fields = vec![
        ("scheme".to_string(), CtValue::Str(u.scheme.clone())),
        ("host".to_string(), match &u.host { Some(h) if !h.is_empty() => CtValue::Present(Box::new(CtValue::Str(h.clone()))), _ => CtValue::absent(Type::String) }),
        ("port".to_string(), match u.port { Some(p) => CtValue::Present(Box::new(CtValue::Int(p))), None => CtValue::absent(Type::Int) }),
        ("path".to_string(), CtValue::Str(u.path.clone())),
        ("query".to_string(), CtValue::List(u.query.iter().map(|(k, v)| CtValue::List(vec![CtValue::Str(k.clone()), CtValue::Str(v.clone())])).collect())),
        ("fragment".to_string(), match &u.fragment { Some(f) => CtValue::Present(Box::new(CtValue::Str(f.clone()))), None => CtValue::absent(Type::String) }),
        (URL_RAW_HOST.to_string(), url_option_string(u.host.as_ref(), Type::String)),
        (URL_USERNAME.to_string(), url_option_string(u.username.as_ref(), Type::String)),
        (URL_PASSWORD.to_string(), url_option_string(u.password.as_ref(), Type::String)),
    ];
    if let Some(parts) = &u.typed_host { fields.push((URL_TYPED_HOST.to_string(), url_typed_parts_value(parts))); }
    if let Some(parts) = &u.typed_path { fields.push((URL_TYPED_PATH.to_string(), url_typed_parts_value(parts))); }
    CtValue::Struct { type_name: "Url".to_string(), fields }
}

pub(super) fn url_parts_from_ct(value: &CtValue, span: Span) -> Result<super::super::super::UrlLite::UrlParts, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else { return Err(unsupported("malformed URL value", span)); };
    let type_name = type_name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX).unwrap_or(type_name);
    if type_name != "Url" { return Err(unsupported("malformed URL value", span)); }
    let field = |name: &str| fields.iter().find(|(field, _)| field == name).map(|(_, value)| value);
    let string_field = |name: &str| match field(name) { Some(CtValue::Str(value)) => Ok(value.clone()), _ => Err(unsupported(&format!("malformed URL {name}"), span)) };
    let scheme = string_field("scheme")?;
    let hidden_field = |name: &str| field(name).ok_or_else(|| unsupported(&format!("malformed URL {name}"), span));
    let host = url_option_string_from_ct(Some(hidden_field(URL_RAW_HOST)?), "host", span)?;
    let port = url_option_int_from_ct(field("port"), "port", span)?;
    let path = string_field("path")?;
    let query = as_string_rows(field("query").ok_or_else(|| unsupported("malformed URL query", span))?, span)?.into_iter().map(|row| (row.first().cloned().unwrap_or_default(), row.get(1).cloned().unwrap_or_default())).collect();
    let fragment = url_option_string_from_ct(field("fragment"), "fragment", span)?;
    let username = url_option_string_from_ct(Some(hidden_field(URL_USERNAME)?), "username", span)?;
    let password = url_option_string_from_ct(Some(hidden_field(URL_PASSWORD)?), "password", span)?;
    let typed_host = url_typed_parts_from_ct(field(URL_TYPED_HOST), "typed host", span)?;
    let typed_path = url_typed_parts_from_ct(field(URL_TYPED_PATH), "typed path", span)?;
    Ok(super::super::super::UrlLite::from_marshaled(scheme, username, password, host, port, path, query, fragment, typed_host, typed_path))
}
