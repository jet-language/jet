// D-HTTP-ROUTE-SYNTAX2=A: one route grammar for every HTTP entry point.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetHTTPRouteSegment {
    Static(String),
    Param(String),
    CatchAll(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetHTTPRoutePattern {
    segments: Vec<JetHTTPRouteSegment>,
}

fn jet_http_route_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}


fn jet_http_route_decode_segment(segment: &str) -> Result<String, String> {
    let decoded = jet_std::jet_url_percent_decode_str(segment)?;
    if decoded.contains('/') {
        return Err("encoded slash is ambiguous".to_string());
    }
    if decoded == "." || decoded == ".." {
        return Err("dot traversal segment is not allowed".to_string());
    }
    Ok(decoded)
}
pub(crate) fn jet_http_route_decode_path_segment<'a>(
    segment: &'a str,
) -> Result<std::borrow::Cow<'a, str>, String> {
    if !segment.as_bytes().contains(&b'%') {
        if segment == "." || segment == ".." {
            return Err("dot traversal segment is not allowed".to_string());
        }
        return Ok(std::borrow::Cow::Borrowed(segment));
    }
    jet_http_route_decode_segment(segment).map(std::borrow::Cow::Owned)
}

fn jet_http_route_parse(pattern: &str) -> Result<JetHTTPRoutePattern, String> {
    if !pattern.starts_with('/') {
        return Err(format!("E2805: invalid HTTP route `{pattern}`: routes must start with `/`"));
    }
    let mut names = std::collections::BTreeSet::new();
    let mut segments = Vec::new();
    let raw_segments: Vec<&str> = pattern.split('/').skip(1).collect();
    for (index, segment) in raw_segments.iter().enumerate() {
        if segment.is_empty() {
            if raw_segments.len() == 1 {
                continue;
            }
            return Err(format!("E2805: invalid HTTP route `{pattern}`: empty path segments are not allowed"));
        }
        if segment.contains('{') || segment.contains('}') {
            return Err(format!("E2805: invalid HTTP route `{pattern}`: use `:name` or final `*name`; braces are not route markers"));
        }
        if *segment == "*" {
            return Err(format!("E2805: invalid HTTP route `{pattern}`: write a named catch-all such as `*wildcard`"));
        }
        let parsed = if let Some(name) = segment.strip_prefix(':') {
            if !jet_http_route_name(name) {
                return Err(format!("E2805: invalid HTTP route `{pattern}`: parameter names must match `[A-Za-z_][A-Za-z0-9_]*`"));
            }
            if !names.insert(name.to_string()) {
                return Err(format!("E2805: invalid HTTP route `{pattern}`: duplicate parameter `{name}`"));
            }
            JetHTTPRouteSegment::Param(name.to_string())
        } else if let Some(name) = segment.strip_prefix('*') {
            if index + 1 != raw_segments.len() {
                return Err(format!("E2805: invalid HTTP route `{pattern}`: `*name` catch-all must be final"));
            }
            if !jet_http_route_name(name) {
                return Err(format!("E2805: invalid HTTP route `{pattern}`: catch-all names must match `[A-Za-z_][A-Za-z0-9_]*`"));
            }
            if !names.insert(name.to_string()) {
                return Err(format!("E2805: invalid HTTP route `{pattern}`: duplicate parameter `{name}`"));
            }
            JetHTTPRouteSegment::CatchAll(name.to_string())
        } else {
            JetHTTPRouteSegment::Static(jet_http_route_decode_segment(segment).map_err(|reason| {
                format!("E2805: invalid HTTP route `{pattern}`: {reason}")
            })?)
        };
        segments.push(parsed);
    }
    Ok(JetHTTPRoutePattern { segments })
}
 
/// Parse one route pattern for every HTTP-facing adapter.  Keeping this
/// constructor beside the grammar lets typed projections carry the parsed
/// pattern instead of reparsing a rendered OpenAPI path.
pub fn jet_http_route_pattern(pattern: &str) -> Result<JetHTTPRoutePattern, String> {
    jet_http_route_parse(pattern)
}

fn jet_http_route_encode_component(value: &str, query: bool, allow_slash: bool) -> String {
    let encoded = jet_std::jet_url_percent_encode(value, allow_slash);
    if query {
        encoded.replace("%20", "+")
    } else {
        encoded
    }
}

pub fn jet_http_route_encode_segment(value: &str, allow_slash: bool) -> String {
    jet_http_route_encode_component(value, false, allow_slash)
}

pub fn jet_http_route_openapi_path(pattern: &JetHTTPRoutePattern) -> String {
    let mut path = String::from("/");
    for (index, segment) in pattern.segments.iter().enumerate() {
        if index > 0 {
            path.push('/');
        }
        match segment {
            JetHTTPRouteSegment::Static(value) => {
                path.push_str(&jet_http_route_encode_segment(value, false));
            }
            JetHTTPRouteSegment::Param(name) | JetHTTPRouteSegment::CatchAll(name) => {
                path.push('{');
                path.push_str(name);
                path.push('}');
            }
        }
    }
    path
}

fn jet_http_route_path<'a>(path: &'a str) -> Result<Vec<std::borrow::Cow<'a, str>>, String> {
    let path = path.split('?').next().unwrap_or(path);
    if !path.starts_with('/') {
        return Err("request path must start with `/`".to_string());
    }
    let mut segments = path.split('/').skip(1);
    let Some(first) = segments.next() else {
        return Ok(Vec::new());
    };
    if first.is_empty() && segments.next().is_none() {
        return Ok(Vec::new());
    }
    let mut decoded = Vec::new();
    decoded.push(jet_http_route_decode_path_segment(first)?);
    for segment in segments {
        decoded.push(jet_http_route_decode_path_segment(segment)?);
    }
    Ok(decoded)
}
pub(crate) fn jet_http_route_validate_path(path: &str) -> Result<(), String> {
    let path = path.split('?').next().unwrap_or(path);
    if !path.starts_with('/') {
        return Err("request path must start with `/`".to_string());
    }
    if path == "/" {
        return Ok(());
    }
    for segment in path.split('/').skip(1) {
        jet_http_route_decode_path_segment(segment)?;
    }
    Ok(())
}

pub(crate) fn jet_http_route_matches_path(pattern: &JetHTTPRoutePattern, path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path);
    let path_len = if path == "/" {
        0
    } else {
        path.split('/').skip(1).count()
    };
    let has_catch_all = matches!(pattern.segments.last(), Some(JetHTTPRouteSegment::CatchAll(_)));
    let required = pattern.segments.len() - usize::from(has_catch_all);
    if path_len < required || !has_catch_all && path_len != required {
        return false;
    }
    for (index, candidate) in path.split('/').skip(1).enumerate() {
        if index >= required {
            continue;
        }
        let Some(segment) = pattern.segments.get(index) else {
            return false;
        };
        if let JetHTTPRouteSegment::Static(expected) = segment {
            let equal = if candidate.as_bytes().contains(&b'%') {
                jet_http_route_decode_path_segment(candidate)
                    .is_ok_and(|decoded| decoded.as_ref() == expected)
            } else {
                candidate == expected
            };
            if !equal {
                return false;
            }
        }
    }
    true
}

pub(crate) fn jet_http_route_params_path(
    pattern: &JetHTTPRoutePattern,
    path: &str,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let path = path.split('?').next().unwrap_or(path);
    let mut params = std::collections::BTreeMap::new();
    let mut segments = path.split('/').skip(1);
    for segment in &pattern.segments {
        match segment {
            JetHTTPRouteSegment::Param(name) => {
                if let Some(value) = segments.next() {
                    params.insert(
                        name.clone(),
                        jet_http_route_decode_path_segment(value)?.into_owned(),
                    );
                }
            }
            JetHTTPRouteSegment::CatchAll(name) => {
                let mut value = String::new();
                for (offset, segment) in segments.enumerate() {
                    if offset > 0 {
                        value.push('/');
                    }
                    value.push_str(
                        jet_http_route_decode_path_segment(segment)?.as_ref(),
                    );
                }
                params.insert(name.clone(), value);
                break;
            }
            JetHTTPRouteSegment::Static(_) => {
                let _ = segments.next();
            }
        }
    }
    Ok(params)
}


pub(crate) fn jet_http_route_matches<'a>(
    pattern: &JetHTTPRoutePattern,
    path: &[std::borrow::Cow<'a, str>],
) -> bool {
    let has_catch_all = matches!(pattern.segments.last(), Some(JetHTTPRouteSegment::CatchAll(_)));
    let required = pattern.segments.len() - usize::from(has_catch_all);
    if path.len() < required || !has_catch_all && path.len() != required {
        return false;
    }
    pattern.segments.iter().enumerate().all(|(index, segment)| {
        match segment {
            JetHTTPRouteSegment::Static(expected) => {
                path.get(index).is_some_and(|candidate| candidate.as_ref() == expected)
            }
            JetHTTPRouteSegment::Param(_) | JetHTTPRouteSegment::CatchAll(_) => true,
        }
    })
}

pub(crate) fn jet_http_route_params<'a>(
    pattern: &JetHTTPRoutePattern,
    path: &[std::borrow::Cow<'a, str>],
) -> std::collections::BTreeMap<String, String> {
    let mut params = std::collections::BTreeMap::new();
    for (index, segment) in pattern.segments.iter().enumerate() {
        match segment {
            JetHTTPRouteSegment::Param(name) => {
                if let Some(value) = path.get(index) {
                    params.insert(name.clone(), value.as_ref().to_string());
                }
            }
            JetHTTPRouteSegment::CatchAll(name) => {
                let mut value = String::new();
                for (offset, segment) in path[index..].iter().enumerate() {
                    if offset > 0 {
                        value.push('/');
                    }
                    value.push_str(segment.as_ref());
                }
                params.insert(name.clone(), value);
                break;
            }
            JetHTTPRouteSegment::Static(_) => {}
        }
    }
    params
}

pub(crate) fn jet_http_route_match<'a>(
    pattern: &JetHTTPRoutePattern,
    path: &[std::borrow::Cow<'a, str>],
) -> Option<std::collections::BTreeMap<String, String>> {
    jet_http_route_matches(pattern, path).then(|| jet_http_route_params(pattern, path))
}


fn jet_http_route_rank(segment: &JetHTTPRouteSegment) -> u8 {
    match segment {
        JetHTTPRouteSegment::Static(_) => 2,
        JetHTTPRouteSegment::Param(_) => 1,
        JetHTTPRouteSegment::CatchAll(_) => 0,
    }
}

pub(crate) fn jet_http_route_selection_cmp(
    left: &JetHTTPRoutePattern,
    left_order: usize,
    right: &JetHTTPRoutePattern,
    right_order: usize,
) -> std::cmp::Ordering {
    for (left, right) in left.segments.iter().zip(&right.segments) {
        let order = jet_http_route_rank(left).cmp(&jet_http_route_rank(right));
        if order != std::cmp::Ordering::Equal {
            return order;
        }
    }
    // If both matched and one pattern ends first, it is the exact route while
    // the longer pattern can only add an empty catch-all. Exact wins.
    right.segments.len().cmp(&left.segments.len())
        // Equivalent shapes use first registration, identically in both routers.
        .then_with(|| right_order.cmp(&left_order))
}

pub(crate) fn jet_http_route_shape(pattern: &JetHTTPRoutePattern) -> String {
    pattern.segments.iter().map(|segment| match segment {
        JetHTTPRouteSegment::Static(value) => format!("s{}:{value}", value.len()),
        JetHTTPRouteSegment::Param(_) => "p".to_string(),
        JetHTTPRouteSegment::CatchAll(_) => "w".to_string(),
    }).collect::<Vec<_>>().join("/")
}
