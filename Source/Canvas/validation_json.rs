fn validate_ident(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(edit_error("bad_request", "empty identifier"));
    };
    if (!first.is_ascii_alphabetic() && first != '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(edit_error("bad_request", "identifier is not a Jet name"));
    }
    Ok(())
}

fn validate_query_ident(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(query_error("bad_request", "empty identifier"));
    };
    if (!first.is_ascii_alphabetic() && first != '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(query_error("bad_request", "identifier is not a Jet name"));
    }
    Ok(())
}

fn validate_ident_for_project(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(project_edit_error("bad_request", "empty identifier"));
    };
    if (!first.is_ascii_alphabetic() && first != '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(project_edit_error(
            "bad_request",
            "identifier is not a Jet package name",
        ));
    }
    Ok(())
}

fn validate_qualified_name(name: &str) -> Result<(), String> {
    for part in name.split('.') {
        validate_ident(part)?;
    }
    Ok(())
}

fn validate_signature_fragment(fragment: &str) -> Result<(), String> {
    if fragment.contains('{') || fragment.contains('}') || fragment.contains('\n') {
        return Err(edit_error(
            "bad_request",
            "function parameter text must stay inside the signature",
        ));
    }
    Ok(())
}

fn validate_type_fragment(fragment: &str) -> Result<(), String> {
    if fragment.trim().is_empty()
        || fragment.contains('{')
        || fragment.contains('}')
        || fragment.contains('\n')
    {
        return Err(edit_error("bad_request", "return type is not a Jet type"));
    }
    Ok(())
}

fn validate_function_signature(signature: &str) -> Result<(), String> {
    if signature.contains('{') || signature.contains('}') || signature.contains('\n') {
        return Err(edit_error(
            "bad_request",
            "function signature must not include a body",
        ));
    }
    if !signature.split_whitespace().any(|part| part == "fn") {
        return Err(edit_error(
            "bad_request",
            "function signature must include fn",
        ));
    }
    Ok(())
}

fn validate_single_line_fragment(fragment: &str, label: &str) -> Result<(), String> {
    if fragment.trim().is_empty() || fragment.contains('\n') || fragment.contains('\r') {
        return Err(edit_error("bad_request", label));
    }
    Ok(())
}

fn validate_comment_color(color: &str) -> Result<(), String> {
    let ok = color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|c| c.is_ascii_hexdigit());
    if ok {
        Ok(())
    } else {
        Err(edit_error(
            "bad_request",
            "Canvas comment color must be #RRGGBB",
        ))
    }
}

fn validate_comment_alpha(alpha: &str) -> Result<(), String> {
    let Ok(value) = alpha.parse::<f32>() else {
        return Err(edit_error(
            "bad_request",
            "Canvas comment alpha must be a number",
        ));
    };
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(edit_error(
            "bad_request",
            "Canvas comment alpha must be between 0 and 1",
        ))
    }
}

fn normalize_bounds(bounds: &str) -> Result<String, String> {
    let nums = bounds
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .map(|part| part.trim().parse::<i32>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| edit_error("bad_request", "Canvas comment bounds must be x,y,w,h"))?;
    if nums.len() != 4 || nums[2] <= 0 || nums[3] <= 0 {
        return Err(edit_error(
            "bad_request",
            "Canvas comment bounds must be x,y,w,h with positive size",
        ));
    }
    Ok(format!("{},{},{},{}", nums[0], nums[1], nums[2], nums[3]))
}

fn quoted_attr(value: &str) -> String {
    json_str(value)
}

fn find_comment_hint(src: &str, graph_json: &str, region_id: &str) -> Option<CommentHint> {
    for chunk in graph_json.split("\"region_id\":").skip(1) {
        let (id, _) = parse_json_string(chunk.trim_start())?;
        if id != region_id {
            continue;
        }
        let start = json_usize_field(chunk, "start")?;
        let end = json_usize_field(chunk, "end")?;
        return canvas_comment_hints(src)
            .into_iter()
            .find(|hint| hint.anchor.start == start && hint.anchor.end == end);
    }
    None
}

fn find_hint_region(
    src: &str,
    graph_json: &str,
    region_id: &str,
    kind: &str,
) -> Option<CommentHint> {
    for chunk in graph_json.split("\"region_id\":").skip(1) {
        let (id, _) = parse_json_string(chunk.trim_start())?;
        if id != region_id {
            continue;
        }
        if json_string_field(chunk, "kind").as_deref() != Some(kind) {
            continue;
        }
        let start = json_usize_field(chunk, "start")?;
        let end = json_usize_field(chunk, "end")?;
        let hints = match kind {
            "comment" => canvas_comment_hints(src),
            "collapse" => canvas_collapse_hints(src),
            _ => return None,
        };
        return hints
            .into_iter()
            .find(|hint| hint.anchor.start == start && hint.anchor.end == end);
    }
    None
}

fn extract_params(graph_json: &str, expr: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for ident in identifiers(expr) {
        if let Some(ty) = graph_type_for_name(graph_json, &ident) {
            if !out.iter().any(|(name, _)| name == &ident) {
                out.push((ident, ty));
            }
        }
    }
    out
}

fn graph_type_for_name(graph_json: &str, name: &str) -> Option<String> {
    for chunk in graph_json.split("\"name\":").skip(1) {
        let (found, _) = parse_json_string(chunk.trim_start())?;
        if found != name || !chunk.contains("\"direction\":\"output\"") {
            continue;
        }
        let pos = chunk.find("\"type\"")?;
        let rest = &chunk[pos + "\"type\"".len()..];
        let colon = rest.find(':')?;
        return parse_json_string(rest[colon + 1..].trim_start()).map(|(s, _)| s);
    }
    None
}

fn identifiers(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in expr.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphanumeric() || c == '_' {
            current.push(c);
        } else if !current.is_empty() {
            if current
                .chars()
                .next()
                .map(|c| c.is_ascii_alphabetic() || c == '_')
                .unwrap_or(false)
                && !matches!(current.as_str(), "true" | "false" | "ok" | "err")
            {
                out.push(current.clone());
            }
            current.clear();
        }
    }
    out
}

fn parse_simple_call(call: &str) -> Option<(String, Vec<String>)> {
    let open = call.find('(')?;
    let close = call.rfind(')')?;
    let name = call[..open].trim();
    validate_ident(name).ok()?;
    let args = if call[open + 1..close].trim().is_empty() {
        Vec::new()
    } else {
        call[open + 1..close]
            .split(',')
            .map(|arg| arg.trim().to_string())
            .collect()
    };
    Some((name.to_string(), args))
}

fn find_simple_helper(src: &str, name: &str) -> Option<(Vec<String>, String)> {
    let needle = format!("fn {name}(");
    let start = src.find(&needle)?;
    let params_start = start + needle.len();
    let params_end = src[params_start..].find(')')? + params_start;
    let params = src[params_start..params_end]
        .split(',')
        .filter_map(|param| {
            param
                .trim()
                .split_once(':')
                .map(|(name, _)| name.trim().to_string())
        })
        .collect::<Vec<_>>();
    let body_start = src[params_end..].find('{')? + params_end + 1;
    let body_end = src[body_start..].find('}')? + body_start;
    let body = src[body_start..body_end].trim();
    let returned = body.strip_prefix("return ")?;
    Some((params, returned.trim().to_string()))
}

fn replace_ident(expr: &str, ident: &str, replacement: &str) -> String {
    let mut out = String::new();
    let mut current = String::new();
    for c in expr.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphanumeric() || c == '_' {
            current.push(c);
        } else {
            if !current.is_empty() {
                if current == ident {
                    out.push_str(replacement);
                } else {
                    out.push_str(&current);
                }
                current.clear();
            }
            out.push(c);
        }
    }
    out.trim_end().to_string()
}

fn attr_string(text: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=");
    let pos = text.find(&needle)?;
    let rest = text[pos + needle.len()..].trim_start();
    if rest.starts_with('"') {
        return parse_json_string(rest).map(|(s, _)| s);
    }
    Some(
        rest.chars()
            .take_while(|c| !c.is_whitespace())
            .collect::<String>(),
    )
}

fn attr_span(text: &str, key: &str) -> Option<SourceSpan> {
    let raw = attr_string(text, key)?;
    let (start, end) = raw.split_once("..")?;
    Some(SourceSpan {
        start: start.parse().ok()?,
        end: end.parse().ok()?,
    })
}

fn attr_bounds(text: &str, key: &str) -> Option<(i32, i32, i32, i32)> {
    let needle = format!("{key}=");
    let pos = text.find(&needle)?;
    let rest = text[pos + needle.len()..].trim_start();
    let rest = rest.strip_prefix('(')?;
    let close = rest.find(')')?;
    let nums = rest[..close]
        .split(',')
        .map(|part| part.trim().parse::<i32>())
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if nums.len() == 4 {
        Some((nums[0], nums[1], nums[2], nums[3]))
    } else {
        None
    }
}

fn required_string(text: &str, key: &str) -> Result<String, String> {
    json_string_field(text, key)
        .ok_or_else(|| edit_error("bad_request", &format!("missing `{key}`")))
}

fn required_query_string(text: &str, key: &str) -> Result<String, String> {
    json_string_field(text, key)
        .ok_or_else(|| query_error("bad_request", &format!("missing `{key}`")))
}

fn required_project_string(text: &str, key: &str) -> Result<String, String> {
    json_string_field(text, key)
        .ok_or_else(|| project_edit_error("bad_request", &format!("missing `{key}`")))
}

fn json_string_field(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let pos = text.find(&needle)?;
    let rest = &text[pos + needle.len()..];
    let colon = rest.find(':')?;
    let rest = &rest[colon + 1..];
    parse_json_string(rest.trim_start()).map(|(s, _)| s)
}

fn json_bool_field(text: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{key}\"");
    let pos = text.find(&needle)?;
    let rest = &text[pos + needle.len()..];
    let colon = rest.find(':')?;
    let rest = rest[colon + 1..].trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn json_usize_field(text: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    let pos = text.find(&needle)?;
    let rest = &text[pos + needle.len()..];
    let colon = rest.find(':')?;
    rest[colon + 1..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

fn json_usize_array(text: &str, key: &str) -> Vec<usize> {
    let needle = format!("\"{key}\"");
    let Some(pos) = text.find(&needle) else {
        return Vec::new();
    };
    let rest = &text[pos + needle.len()..];
    let Some(colon) = rest.find(':') else {
        return Vec::new();
    };
    let rest = rest[colon + 1..].trim_start();
    let Some(mut rest) = rest.strip_prefix('[') else {
        return Vec::new();
    };
    let mut out = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.starts_with(']') {
            break;
        }
        let digits = rest
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>();
        if digits.is_empty() {
            break;
        }
        if let Ok(n) = digits.parse::<usize>() {
            out.push(n);
        }
        rest = &rest[digits.len()..];
        rest = rest.trim_start();
        if rest.starts_with(',') {
            rest = &rest[1..];
        }
    }
    out
}

fn json_array_body<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let pos = text.find(&needle)?;
    let rest = &text[pos + needle.len()..];
    let colon = rest.find(':')?;
    let rest = rest[colon + 1..].trim_start();
    let bytes = rest.as_bytes();
    if bytes.first().copied()? != b'[' {
        return None;
    }
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (i, b) in bytes.iter().copied().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return rest.get(1..i);
                }
            }
            _ => {}
        }
    }
    None
}

fn json_object_bodies(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = None;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (i, b) in bytes.iter().copied().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => {
                if depth == 0 {
                    start = Some(i + 1);
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start.take() {
                        if let Some(body) = text.get(s..i) {
                            out.push(body);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn wire_span_from_json_chunk(chunk: &str, wire_id: &str) -> Option<SourceSpan> {
    let (id, _) = parse_json_string(chunk.trim_start())?;
    if id != wire_id {
        return None;
    }
    let pos = chunk.find("\"source_span\"")?;
    let rest = &chunk[pos + "\"source_span\"".len()..];
    let colon = rest.find(':')?;
    let value = rest[colon + 1..].trim_start();
    if value.starts_with("null") {
        return None;
    }
    Some(SourceSpan {
        start: json_usize_field(value, "start")?,
        end: json_usize_field(value, "end")?,
    })
}

fn json_string_array(text: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{key}\"");
    let Some(pos) = text.find(&needle) else {
        return Vec::new();
    };
    let rest = &text[pos + needle.len()..];
    let Some(colon) = rest.find(':') else {
        return Vec::new();
    };
    let rest = rest[colon + 1..].trim_start();
    let Some(mut rest) = rest.strip_prefix('[') else {
        return Vec::new();
    };
    let mut out = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.starts_with(']') {
            break;
        }
        let Some((value, consumed)) = parse_json_string(rest) else {
            break;
        };
        out.push(value);
        rest = &rest[consumed..];
        rest = rest.trim_start();
        if rest.starts_with(',') {
            rest = &rest[1..];
        }
    }
    out
}

fn parse_json_string(text: &str) -> Option<(String, usize)> {
    let mut chars = text.char_indices();
    if chars.next()?.1 != '"' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for (i, c) in chars {
        if escaped {
            out.push(match c {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '"' => '"',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some((out, i + 1));
        } else {
            out.push(c);
        }
    }
    None
}

fn span_json(span: SourceSpan) -> String {
    format!("{{\"start\":{},\"end\":{}}}", span.start, span.end)
}

fn json_strs(values: &[String]) -> String {
    values
        .iter()
        .map(|s| json_str(s))
        .collect::<Vec<_>>()
        .join(",")
}

fn json_str(s: &str) -> String {
    format!("\"{}\"", json_escape(s))
}

fn json_optional_str(value: Option<&str>) -> String {
    value.map(json_str).unwrap_or_else(|| "null".to_string())
}

fn json_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
