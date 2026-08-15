    // JSON/DataTree bridges layered on the canonical JSON parser.
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

    // Render a DataTree as JSON, preserving Object field order. Int prints with no
    // decimal (`5`), Float keeps its decimal (`5.0`); Bytes render as a number array.
    pub fn render_datatree_json(t: &DataTree, pretty: bool, depth: usize) -> String {
        match t {
            DataTree::Null => "null".to_string(),
            DataTree::Bool(b) => b.to_string(),
            DataTree::Int(n) => format!("{}", n),
            DataTree::Float(f) => format!("{:?}", f),
            DataTree::Number(text) => text.clone(),
            DataTree::TypedText(text) => quote_json(text),
            DataTree::Text(s) => quote_json(s),
            DataTree::Bytes(bs) => {
                let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                format!("[{}]", parts.join(","))
            }
            DataTree::Array(items) => {
                if items.is_empty() {
                    return "[]".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = items
                        .iter()
                        .map(|x| render_datatree_json(x, false, depth))
                        .collect();
                    return format!("[{}]", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = items
                    .iter()
                    .map(|x| format!("{}{}", pad, render_datatree_json(x, true, depth + 1)))
                    .collect();
                format!("[\n{}\n{}]", parts.join(",\n"), end)
            }
            DataTree::Object(entries) => {
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
                                render_datatree_json(v, false, depth)
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
                            render_datatree_json(v, true, depth + 1)
                        )
                    })
                    .collect();
                format!("{{\n{}\n{}}}", parts.join(",\n"), end)
            }
        }
    }

    // JSON (dynamic, BTreeMap-keyed) → DataTree. Numbers that are integral collapse
    // to `Int`, so a round-trip through JSON keeps `5` an Int.
    pub fn datatree_from_json(j: &JSON) -> DataTree {
        match j {
            JSON::Null => DataTree::Null,
            JSON::Boolean(b) => DataTree::Bool(*b),
            JSON::Integer(n) => DataTree::Int(*n),
            JSON::Number(n) => {
                if n.fract() == 0.0
                    && n.is_finite()
                    && *n >= i64::MIN as f64
                    && *n <= i64::MAX as f64
                {
                    DataTree::Int(*n as i64)
                } else {
                    DataTree::Float(*n)
                }
            }
            JSON::Text(s) => DataTree::Text(s.clone()),
            JSON::Array(items) => DataTree::Array(items.iter().map(datatree_from_json).collect()),
            JSON::Object(entries) => DataTree::Object(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), datatree_from_json(value)))
                    .collect(),
            ),
        }
    }

    /// Parse typed wire data directly into the ordered tree. The parser and
    /// malformed-input vocabulary are shared with dynamic JSON and comptime.
    pub fn parse_json_datatree(text: &str) -> Result<DataTree, JSONError> {
        crate::jet_encoding_json::parse_json(text, false)
            .map(datatree_from_shared)
            .map_err(json_error_from_shared)
    }

    /// Parse typed JSON through the same tokenizer while preserving each
    /// number token until a destination decoder chooses Int, a sized integer,
    /// Decimal, or Float.
    pub fn parse_json_typed_datatree(text: &str) -> Result<DataTree, JSONError> {
        crate::jet_encoding_json::parse_json_exact_numbers(text, false)
            .map(typed_datatree_from_shared)
            .map_err(json_error_from_shared)
    }

    fn typed_datatree_from_shared(value: crate::jet_encoding_json::Value) -> DataTree {
        match value {
            crate::jet_encoding_json::Value::Null => DataTree::Null,
            crate::jet_encoding_json::Value::Bool(value) => DataTree::Bool(value),
            crate::jet_encoding_json::Value::Number(value) => DataTree::Number(value),
            crate::jet_encoding_json::Value::Text(value) => DataTree::TypedText(value),
            crate::jet_encoding_json::Value::Array(values) => DataTree::Array(
                values
                    .into_iter()
                    .map(typed_datatree_from_shared)
                    .collect(),
            ),
            crate::jet_encoding_json::Value::Object(entries) => DataTree::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, typed_datatree_from_shared(value)))
                    .collect(),
            ),
            crate::jet_encoding_json::Value::Int(_)
            | crate::jet_encoding_json::Value::Float(_) => {
                unreachable!("lossless JSON parsing projected a number early")
            }
        }
    }

    fn datatree_from_shared(value: crate::jet_encoding_json::Value) -> DataTree {
        match value {
            crate::jet_encoding_json::Value::Null => DataTree::Null,
            crate::jet_encoding_json::Value::Bool(value) => DataTree::Bool(value),
            crate::jet_encoding_json::Value::Int(value) => DataTree::Int(value),
            crate::jet_encoding_json::Value::Float(value) => DataTree::Float(value),
            crate::jet_encoding_json::Value::Number(_) => {
                unreachable!("lossless JSON number leaked into dynamic DataTree projection")
            }
            crate::jet_encoding_json::Value::Text(value) => DataTree::Text(value),
            crate::jet_encoding_json::Value::Array(values) => {
                DataTree::Array(values.into_iter().map(datatree_from_shared).collect())
            }
            crate::jet_encoding_json::Value::Object(entries) => DataTree::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, datatree_from_shared(value)))
                    .collect(),
            ),
        }
    }

    // Look up a key in an ordered Object.
    pub fn datatree_get<'a>(t: &'a DataTree, key: &str) -> Option<&'a DataTree> {
        match t {
            DataTree::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
