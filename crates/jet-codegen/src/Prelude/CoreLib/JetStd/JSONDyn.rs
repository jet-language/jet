    // JSON/Dyn bridges layered on the canonical JSON parser.
    pub fn io_error_at(operation: IOOperation, path: &str, e: std::io::Error) -> IOError {
        let context = IOContext::new(
            operation,
            Some(path.to_string()),
            e.raw_os_error().map(i64::from),
            Some(e.to_string()),
        );
        match e.kind() {
            std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => {
                IOError::InvalidInput(context)
            }
            std::io::ErrorKind::NotFound => IOError::NotFound(context),
            std::io::ErrorKind::PermissionDenied => IOError::PermissionDenied(context),
            std::io::ErrorKind::TimedOut => IOError::TimedOut(context),
            std::io::ErrorKind::WouldBlock => IOError::Other(context),
            std::io::ErrorKind::NotConnected | std::io::ErrorKind::BrokenPipe => {
                IOError::Closed(context)
            }
            _ => IOError::Other(context),
        }
    }

    // Render a Dyn as JSON, preserving Object field order. Int uses the
    // exact carrier renderer; Float keeps its decimal (`5.0`); Number keeps
    // its exact lexeme (`1.50`); Bytes and DateTime render as canonical strings.
    pub fn render_dyn_json(t: &Dyn, pretty: bool, depth: usize) -> String {
        match t {
            Dyn::Null => "null".to_string(),
            Dyn::Bool(b) => b.to_string(),
            Dyn::Int(n) => n.to_string_rep(),
            Dyn::Float(f) => {
                assert!(f.is_finite(), "JSON cannot encode a non-finite Float");
                format!("{:?}", f)
            }
            Dyn::Number(text) => text.clone(),
            Dyn::Text(s) => quote_json(s),
            Dyn::Bytes(bs) => quote_json(&Dyn::bytes_text(bs)),
            Dyn::DateTime(value) => quote_json(&value.to_rfc3339()),
            Dyn::Array(items) => {
                if items.is_empty() {
                    return "[]".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = items
                        .iter()
                        .map(|x| render_dyn_json(x, false, depth))
                        .collect();
                    return format!("[{}]", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = items
                    .iter()
                    .map(|x| format!("{}{}", pad, render_dyn_json(x, true, depth + 1)))
                    .collect();
                format!("[\n{}\n{}]", parts.join(",\n"), end)
            }
            Dyn::Object(entries) => {
                if entries.is_empty() {
                    return "{}".to_string();
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                if !pretty {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, v)| {
                            format!(
                                "{}:{}",
                                quote_json(k),
                                render_dyn_json(v, false, depth)
                            )
                        })
                        .collect();
                    return format!("{{{}}}", parts.join(","));
                }
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{}{}: {}",
                            pad,
                            quote_json(k),
                            render_dyn_json(v, true, depth + 1)
                        )
                    })
                    .collect();
                format!("{{\n{}\n{}}}", parts.join(",\n"), end)
            }
        }
    }


    /// Parse JSON into the ordered tree with the Jet `json.parse` meaning:
    /// every number token keeps its exact lexeme as `Number` until a typed
    /// read range-checks it (#4602). The parser and malformed-input
    /// vocabulary are shared with comptime.
    pub fn parse_json_dyn(text: &str) -> Result<Dyn, EncodingError> {
        crate::jet_encoding_json::parse_json_exact_numbers(text, true)
            .map_err(json_error_from_dyn)
    }

    fn json_error_from_dyn(error: crate::jet_encoding_json::Error) -> EncodingError {
        EncodingError::new(
            EncodingFormat::JSON,
            EncodingErrorKind::Syntax,
            0,
            Ok(error.line),
            Err(JetAbsent),
            "",
            error.message,
        )
    }

    // Look up a key in an ordered Object.
    pub fn dyn_get<'a>(t: &'a Dyn, key: &str) -> Option<&'a Dyn> {
        match t {
            Dyn::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
