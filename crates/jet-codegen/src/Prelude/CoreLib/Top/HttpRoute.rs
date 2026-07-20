// D-HTTP-ROUTE-SYNTAX2=A: one route grammar for every HTTP entry point.

#[derive(Clone, Debug, PartialEq, Eq)]
enum JetHttpRouteSegment {
    Static(String),
    Param(String),
    CatchAll(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetHttpRoutePattern {
    segments: Vec<JetHttpRouteSegment>,
}

fn jet_http_route_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn jet_http_route_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn jet_http_route_decode_segment(segment: &str) -> Result<String, String> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let Some(high) = bytes.get(index + 1).and_then(|byte| jet_http_route_hex(*byte)) else {
            return Err("invalid percent escape".to_string());
        };
        let Some(low) = bytes.get(index + 2).and_then(|byte| jet_http_route_hex(*byte)) else {
            return Err("invalid percent escape".to_string());
        };
        let byte = high * 16 + low;
        if byte == b'/' {
            return Err("encoded slash is ambiguous".to_string());
        }
        decoded.push(byte);
        index += 3;
    }
    let decoded = String::from_utf8(decoded).map_err(|_| "route segment is not valid UTF-8".to_string())?;
    if decoded == "." || decoded == ".." {
        return Err("dot traversal segment is not allowed".to_string());
    }
    Ok(decoded)
}

fn jet_http_route_parse(pattern: &str) -> Result<JetHttpRoutePattern, String> {
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
            JetHttpRouteSegment::Param(name.to_string())
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
            JetHttpRouteSegment::CatchAll(name.to_string())
        } else {
            JetHttpRouteSegment::Static(jet_http_route_decode_segment(segment).map_err(|reason| {
                format!("E2805: invalid HTTP route `{pattern}`: {reason}")
            })?)
        };
        segments.push(parsed);
    }
    Ok(JetHttpRoutePattern { segments })
}

fn jet_http_route_path(path: &str) -> Result<Vec<String>, String> {
    let path = path.split('?').next().unwrap_or(path);
    if !path.starts_with('/') {
        return Err("request path must start with `/`".to_string());
    }
    let raw: Vec<&str> = path.split('/').skip(1).collect();
    if raw.len() == 1 && raw[0].is_empty() {
        return Ok(Vec::new());
    }
    raw.into_iter().map(jet_http_route_decode_segment).collect()
}

fn jet_http_route_match(
    pattern: &JetHttpRoutePattern,
    path: &[String],
) -> Option<std::collections::BTreeMap<String, String>> {
    let has_catch_all = matches!(pattern.segments.last(), Some(JetHttpRouteSegment::CatchAll(_)));
    let required = pattern.segments.len() - usize::from(has_catch_all);
    if path.len() < required || !has_catch_all && path.len() != required {
        return None;
    }
    let mut params = std::collections::BTreeMap::new();
    for (index, segment) in pattern.segments.iter().enumerate() {
        match segment {
            JetHttpRouteSegment::Static(expected) if path.get(index) == Some(expected) => {}
            JetHttpRouteSegment::Static(_) => return None,
            JetHttpRouteSegment::Param(name) => {
                params.insert(name.clone(), path[index].clone());
            }
            JetHttpRouteSegment::CatchAll(name) => {
                params.insert(name.clone(), path[index..].join("/"));
                break;
            }
        }
    }
    Some(params)
}

fn jet_http_route_rank(segment: &JetHttpRouteSegment) -> u8 {
    match segment {
        JetHttpRouteSegment::Static(_) => 2,
        JetHttpRouteSegment::Param(_) => 1,
        JetHttpRouteSegment::CatchAll(_) => 0,
    }
}

fn jet_http_route_selection_cmp(
    left: &JetHttpRoutePattern,
    left_order: usize,
    right: &JetHttpRoutePattern,
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

fn jet_http_route_shape(pattern: &JetHttpRoutePattern) -> String {
    pattern.segments.iter().map(|segment| match segment {
        JetHttpRouteSegment::Static(value) => format!("s{}:{value}", value.len()),
        JetHttpRouteSegment::Param(_) => "p".to_string(),
        JetHttpRouteSegment::CatchAll(_) => "w".to_string(),
    }).collect::<Vec<_>>().join("/")
}
