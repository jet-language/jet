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
    // exact carrier renderer; Float keeps its decimal (`5.0`); Bytes render as
    // a number array.
    pub fn render_dyn_json(t: &Dyn, pretty: bool, depth: usize) -> String {
        match t {
            Dyn::Null => "null".to_string(),
            Dyn::Bool(b) => b.to_string(),
            Dyn::Int(n) => jet_int_to_string(*n),
            Dyn::Float(f) => {
                assert!(f.is_finite(), "JSON cannot encode a non-finite Float");
                format!("{:?}", f)
            }
            Dyn::Number(text) => text.clone(),
            Dyn::TypedText(text) => quote_json(text),
            Dyn::Text(s) => quote_json(s),
            Dyn::Bytes(bs) => {
                let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                format!("[{}]", parts.join(","))
            }
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


    /// Parse typed wire data directly into the ordered tree. The parser and
    /// malformed-input vocabulary are shared with dynamic JSON and comptime.
    pub fn parse_json_dyn(text: &str) -> Result<Dyn, EncodingError> {
        crate::jet_encoding_json::parse_json(text, true).map_err(json_error_from_dyn)
    }

    /// Parse typed JSON through the same tokenizer while preserving each
    /// number token until a destination decoder chooses Int, a sized integer,
    /// Decimal, or Float.
    pub fn parse_json_typed_dyn(text: &str) -> Result<Dyn, EncodingError> {
        crate::jet_encoding_json::parse_json_typed(text, true)
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

    // ONE lenient-decode policy — the walk, the coercion message, and
    // the audit-line shape. A tier supplies only the sink it can reach (AOT
    // `eprintln!`, the resident JIT's `JitRuntime.stderr`), which is real
    // marshalling. This lived as two byte-equivalent copies: AOT's
    // `jet_std_json_coerce_walk` (`CoreLib/Top/MathRandomTime.rs`) and the JIT
    // host's `coerce_walk` (`jet-jit/src/Encoding.rs`), the second
    // self-documented as "same as" the first. Both tiers include this file, so
    // the copy is deleted rather than kept in sync (I8/I9).
    pub fn jet_std_json_coerce_line(path: &str, from: &str, to: &str) -> String {
        let field_label = if path.is_empty() { "<root>" } else { path };
        let msg = format!(
            "json coerce: field \"{}\" {} \u{2192} {}",
            field_label, from, to
        );
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        format!("{{\"level\":\"info\",\"body\":\"{}\",\"ts\":{}}}", msg, ts)
    }

    // Any JSON string that looks like a boolean or a number becomes that type,
    // and one audit line is emitted per coercion naming the field and from→to.
    pub fn jet_std_json_coerce_walk(
        value: &Dyn,
        path: &str,
        emit: &mut dyn FnMut(String),
    ) -> Dyn {
        match value {
            Dyn::Text(s) | Dyn::TypedText(s) => {
                if s == "true" {
                    emit(jet_std_json_coerce_line(path, "string", "boolean"));
                    return Dyn::Bool(true);
                }
                if s == "false" {
                    emit(jet_std_json_coerce_line(path, "string", "boolean"));
                    return Dyn::Bool(false);
                }
                if let Ok(n) = s.parse::<f64>() {
                    if n.is_finite() {
                        emit(jet_std_json_coerce_line(path, "string", "number"));
                        return Dyn::Float(n);
                    }
                }
                value.clone()
            }
            Dyn::Object(entries) => Dyn::Object(
                entries
                    .iter()
                    .map(|(key, value)| {
                        let child_path = if path.is_empty() {
                            key.clone()
                        } else {
                            format!("{}.{}", path, key)
                        };
                        (
                            key.clone(),
                            jet_std_json_coerce_walk(value, &child_path, emit),
                        )
                    })
                    .collect(),
            ),
            Dyn::Array(items) => Dyn::Array(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        let child_path = if path.is_empty() {
                            format!("[{}]", index)
                        } else {
                            format!("{}[{}]", path, index)
                        };
                        jet_std_json_coerce_walk(value, &child_path, emit)
                    })
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    // The lenient decode value stays on the canonical ordered Dyn.
    pub fn jet_std_json_decode_lenient(
        text: &str,
        emit: &mut dyn FnMut(String),
    ) -> Result<Dyn, EncodingError> {
        let parsed = parse_json_dyn(text)?;
        Ok(jet_std_json_coerce_walk(&parsed, "", emit))
    }
