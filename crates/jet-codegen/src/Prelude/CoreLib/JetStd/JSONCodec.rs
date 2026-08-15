    // Canonical std-only JSON codec shared by AOT, JIT encoding, and tier 0.
    pub fn parse_json(text: &str) -> Result<JSON, JSONError> {
        crate::jet_encoding_json::parse_json(text, false)
            .map(json_from_shared)
            .map_err(json_error_from_shared)
    }

    pub fn parse_json_strict(text: &str) -> Result<JSON, JSONError> {
        crate::jet_encoding_json::parse_json(text, true)
            .map(json_from_shared)
            .map_err(json_error_from_shared)
    }

    fn json_from_shared(value: crate::jet_encoding_json::Value) -> JSON {
        match value {
            crate::jet_encoding_json::Value::Null => JSON::Null,
            crate::jet_encoding_json::Value::Bool(value) => JSON::Boolean(value),
            crate::jet_encoding_json::Value::Int(value) => JSON::Integer(value),
            crate::jet_encoding_json::Value::Float(value) => JSON::Number(value),
            crate::jet_encoding_json::Value::Number(_) => {
                unreachable!("lossless JSON number leaked into dynamic projection")
            }
            crate::jet_encoding_json::Value::Text(value) => JSON::Text(value),
            crate::jet_encoding_json::Value::Array(values) => {
                JSON::Array(values.into_iter().map(json_from_shared).collect())
            }
            crate::jet_encoding_json::Value::Object(entries) => JSON::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, json_from_shared(value)))
                    .collect(),
            ),
        }
    }

    fn json_error_from_shared(error: crate::jet_encoding_json::Error) -> JSONError {
        JSONError {
            line: error.line,
            message: error.message,
        }
    }

    pub fn is_json_structural_whitespace(c: char) -> bool {
        crate::jet_encoding_json::is_json_structural_whitespace(c)
    }

    pub fn render_json(j: &JSON, pretty: bool, depth: usize) -> String {
        match j {
            JSON::Null => "null".to_string(),
            JSON::Boolean(b) => b.to_string(),
            JSON::Integer(n) => n.to_string(),
            JSON::Number(n) => format!("{:?}", n),
            JSON::Text(s) => quote_json(s),
            JSON::Array(items) => {
                if items.is_empty() {
                    return "[]".to_string();
                }
                if !pretty {
                    let parts: Vec<String> =
                        items.iter().map(|x| render_json(x, false, depth)).collect();
                    return format!("[{}]", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = items
                    .iter()
                    .map(|x| format!("{}{}", pad, render_json(x, true, depth + 1)))
                    .collect();
                format!("[\n{}\n{}]", parts.join(",\n"), end)
            }
            JSON::Object(entries) => {
                if entries.is_empty() {
                    return "{}".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, v)| format!("{}:{}", quote_json(k), render_json(v, false, depth)))
                        .collect();
                    return format!("{{{}}}", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{}{}: {}",
                            pad,
                            quote_json(k),
                            render_json(v, true, depth + 1)
                        )
                    })
                    .collect();
                format!("{{\n{}\n{}}}", parts.join(",\n"), end)
            }
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
