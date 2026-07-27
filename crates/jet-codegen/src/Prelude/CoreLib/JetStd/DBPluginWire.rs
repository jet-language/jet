    // ── core.db: the tagged SQL parameter/column value (D-DBDRIVER1) ───────────
    // `DBValue` mirrors `JSON`'s dynamic-value construction mechanism
    // (`DBValue.Int(n)` / `.Float(f)` / `.Text(s)` / `.Bool(b)` / `.Null`) but is
    // SQL-shaped: `Int` keeps the full 64-bit width SQLite integers carry (never
    // routed through `f64`, which would lose precision above 2^53). A `Row` is
    // `Map<String, DBValue>` — the built-in `Map` type already gives `.get`/
    // `.keys`/`.values`, so no separate nominal `Row` type is needed (I8).
    #[derive(Clone, Debug, PartialEq)]
    pub enum DBValue {
        Null,
        Int(i64),
        Float(f64),
        Text(String),
        Bool(bool),
    }

    impl super::JetShow for DBValue {
        fn jet_show(&self) -> String {
            render_db_value(self)
        }
    }

    fn render_db_value(v: &DBValue) -> String {
        match v {
            DBValue::Null => "null".to_string(),
            DBValue::Int(n) => n.to_string(),
            DBValue::Float(f) => f.to_string(),
            DBValue::Text(s) => s.clone(),
            DBValue::Bool(b) => b.to_string(),
        }
    }

    impl DBValue {
        pub fn is_null(&self) -> bool {
            matches!(self, DBValue::Null)
        }
        pub fn int(&self) -> Result<i64, String> {
            match self {
                DBValue::Int(n) => Ok(*n),
                _ => Err(format!("expected an int, got {}", render_db_value(self))),
            }
        }
        pub fn float(&self) -> Result<f64, String> {
            match self {
                DBValue::Float(f) => Ok(*f),
                DBValue::Int(n) => Ok(*n as f64),
                _ => Err(format!("expected a float, got {}", render_db_value(self))),
            }
        }
        pub fn text(&self) -> Result<String, String> {
            match self {
                DBValue::Text(s) => Ok(s.clone()),
                _ => Err(format!("expected text, got {}", render_db_value(self))),
            }
        }
        pub fn bool(&self) -> Result<bool, String> {
            match self {
                DBValue::Bool(b) => Ok(*b),
                _ => Err(format!("expected a bool, got {}", render_db_value(self))),
            }
        }
    }

    pub fn jet_db_row_value(
        row: &std::collections::BTreeMap<String, DBValue>,
        key: &String,
    ) -> Result<DBValue, String> {
        row.get(key)
            .cloned()
            .ok_or_else(|| format!("missing column `{}`", key))
    }

    pub fn jet_db_row_int(
        row: &std::collections::BTreeMap<String, DBValue>,
        key: &String,
    ) -> Result<i64, String> {
        jet_db_row_value(row, key).and_then(|v| v.int())
    }

    pub fn jet_db_row_float(
        row: &std::collections::BTreeMap<String, DBValue>,
        key: &String,
    ) -> Result<f64, String> {
        jet_db_row_value(row, key).and_then(|v| v.float())
    }

    pub fn jet_db_row_text(
        row: &std::collections::BTreeMap<String, DBValue>,
        key: &String,
    ) -> Result<String, String> {
        jet_db_row_value(row, key).and_then(|v| v.text())
    }

    pub fn jet_db_row_bool(
        row: &std::collections::BTreeMap<String, DBValue>,
        key: &String,
    ) -> Result<bool, String> {
        jet_db_row_value(row, key).and_then(|v| v.bool())
    }

    /// D-DBDRIVER1: `.query`/`.query_one`/`.execute` fail with a `DBError`
    /// carrying the driver's message (SQLite's error text) — never the raw SQL.
    #[derive(Clone, Debug, PartialEq)]
    pub struct DBError {
        pub message: String,
    }

    impl super::JetShow for DBError {
        fn jet_show(&self) -> String {
            self.message.clone()
        }
    }

    // ── core.db wire codec ──────────────────────────────────────────────────────
    // The FFI bridge crate (built only when a program uses `jet.db`, Source/FFI.rs)
    // and this always-compiled prelude are two independently built Rust crates —
    // they can't share types, so bind params and result rows cross that boundary as
    // plain `String`s in a small tagged-length wire format (mirrored byte-for-byte
    // in Source/Prelude/DB.rs). A value is `<tag><decimal-length>:<payload-bytes>`;
    // a list is a decimal item count + `:` + that many back-to-back items. Every
    // length is a byte count, so arbitrary text — including an "injection-looking"
    // literal — round-trips exactly with no escaping.
    fn db_encode_tagged(tag: char, payload: &str) -> String {
        format!("{tag}{}:{payload}", payload.len())
    }

    pub fn jet_db_encode_params(params: &Vec<DBValue>) -> String {
        let mut out = String::new();
        out.push_str(&params.len().to_string());
        out.push(':');
        for p in params {
            out.push_str(&match p {
                DBValue::Null => db_encode_tagged('N', ""),
                DBValue::Int(n) => db_encode_tagged('I', &n.to_string()),
                DBValue::Float(f) => db_encode_tagged('F', &f.to_string()),
                DBValue::Text(s) => db_encode_tagged('T', s),
                DBValue::Bool(b) => db_encode_tagged('B', if *b { "1" } else { "0" }),
            });
        }
        out
    }

    fn db_read_tagged(bytes: &[u8], pos: &mut usize) -> Option<(char, String)> {
        let tag = *bytes.get(*pos)? as char;
        *pos += 1;
        let len_start = *pos;
        while *bytes.get(*pos)? != b':' {
            *pos += 1;
        }
        let len: usize = std::str::from_utf8(&bytes[len_start..*pos])
            .ok()?
            .parse()
            .ok()?;
        *pos += 1; // skip ':'
        let payload = std::str::from_utf8(bytes.get(*pos..*pos + len)?)
            .ok()?
            .to_string();
        *pos += len;
        Some((tag, payload))
    }

    fn db_decode_value(tag: char, payload: &str) -> DBValue {
        match tag {
            'I' => DBValue::Int(payload.parse().unwrap_or(0)),
            'F' => DBValue::Float(payload.parse().unwrap_or(0.0)),
            'T' => DBValue::Text(payload.to_string()),
            'B' => DBValue::Bool(payload == "1"),
            _ => DBValue::Null,
        }
    }

    /// Decode the `"O:" + rows`/`"E:" + message` wire produced by `jet_db_query`.
    pub fn jet_db_decode_query_result(
        wire: &str,
    ) -> Result<Vec<std::collections::BTreeMap<String, DBValue>>, DBError> {
        let Some(body) = wire.strip_prefix("O:") else {
            let msg = wire.strip_prefix("E:").unwrap_or(wire);
            return Err(DBError {
                message: msg.to_string(),
            });
        };
        let bytes = body.as_bytes();
        let mut pos = 0usize;
        let Some(colon) = bytes.iter().position(|b| *b == b':') else {
            return Ok(Vec::new());
        };
        let row_count: usize = std::str::from_utf8(&bytes[..colon])
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        pos = colon + 1;
        let mut rows = Vec::with_capacity(row_count);
        for _ in 0..row_count {
            let Some(col_colon) = bytes[pos..].iter().position(|b| *b == b':') else {
                break;
            };
            let col_count: usize = std::str::from_utf8(&bytes[pos..pos + col_colon])
                .unwrap_or("0")
                .parse()
                .unwrap_or(0);
            pos += col_colon + 1;
            let mut row = std::collections::BTreeMap::new();
            for _ in 0..col_count {
                let Some((_, name)) = db_read_tagged(bytes, &mut pos) else {
                    break;
                };
                let Some((vtag, vpayload)) = db_read_tagged(bytes, &mut pos) else {
                    break;
                };
                row.insert(name, db_decode_value(vtag, &vpayload));
            }
            rows.push(row);
        }
        Ok(rows)
    }

    /// Decode the `"O:" + count`/`"E:" + message` wire produced by `jet_db_execute`.
    pub fn jet_db_decode_execute_result(wire: &str) -> Result<i64, DBError> {
        if let Some(n) = wire.strip_prefix("O:") {
            return Ok(n.parse().unwrap_or(0));
        }
        let msg = wire.strip_prefix("E:").unwrap_or(wire);
        Err(DBError {
            message: msg.to_string(),
        })
    }

    pub fn jet_db_params_from_sql(sql: &(String, Vec<String>)) -> Vec<DBValue> {
        sql.1.iter().map(|s| DBValue::Text(s.clone())).collect()
    }

    pub fn jet_db_migration_checksum(steps: &Vec<String>) -> String {
        let mut hash: u64 = 0xcbf29ce484222325;
        for step in steps {
            for b in step.as_bytes() {
                hash ^= *b as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
            hash ^= 0xff;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("{hash:016x}")
    }

    // ── D-DEP-WASM1=A / D-PLUGIN1=B (c81): core.plugin wire helpers ────────────
    // `Plugin.call`/`.call_int` cross the sandboxed Component Model boundary as
    // plain wire text — the always-compiled prelude here and the hidden FFI
    // bridge crate (`Prelude/Plugin.rs`, `jet_plugin_call`) are built
    // independently and share no Rust types, only this tagged-length text
    // (same house style as `jet_db_encode_params`/`jet_db_decode_query_result`
    // above; `pluginw_`-prefixed so nothing here collides with `db_*`).
    fn pluginw_encode_tagged(tag: char, payload: &str) -> String {
        format!("{tag}{}:{payload}", payload.len())
    }

    fn pluginw_read_tagged(bytes: &[u8], pos: &mut usize) -> Option<(char, String)> {
        let tag = *bytes.get(*pos)? as char;
        *pos += 1;
        let len_start = *pos;
        while *bytes.get(*pos)? != b':' {
            *pos += 1;
        }
        let len: usize = std::str::from_utf8(&bytes[len_start..*pos])
            .ok()?
            .parse()
            .ok()?;
        *pos += 1; // skip ':'
        let payload = std::str::from_utf8(bytes.get(*pos..*pos + len)?)
            .ok()?
            .to_string();
        *pos += len;
        Some((tag, payload))
    }

    /// Encode a `[Float]` argument list for `plugin.call(name, args)`.
    pub fn jet_plugin_encode_args_float(args: &Vec<f64>) -> String {
        let mut out = String::new();
        out.push_str(&args.len().to_string());
        out.push(':');
        for a in args {
            out.push_str(&pluginw_encode_tagged('F', &a.to_string()));
        }
        out
    }

    /// Encode an `[Int]` argument list for `plugin.call_int(name, args)`.
    pub fn jet_plugin_encode_args_int(args: &Vec<i64>) -> String {
        let mut out = String::new();
        out.push_str(&args.len().to_string());
        out.push(':');
        for a in args {
            out.push_str(&pluginw_encode_tagged('I', &a.to_string()));
        }
        out
    }

    /// Decode the `"O:<handle>"`/`"E:<message>"` wire produced by
    /// `jet_plugin_load`. Returns the handle, or `0` (the invalid-handle
    /// sentinel, mirroring `jet_db_open`'s style) when the load failed — every
    /// later `.call`/`.call_int` on handle `0` reports "no plugin loaded for
    /// this handle" rather than ever panicking (I2).
    pub fn jet_plugin_load_handle(wire: &str) -> u64 {
        wire.strip_prefix("O:")
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0)
    }

    /// Decode the `"O:F<len>:<val>"`/`"E:<message>"` wire produced by
    /// `jet_plugin_call` for a `.call` (Float) invocation.
    pub fn jet_plugin_decode_result_float(wire: &str) -> Result<f64, String> {
        let Some(body) = wire.strip_prefix("O:") else {
            return Err(wire.strip_prefix("E:").unwrap_or(wire).to_string());
        };
        let bytes = body.as_bytes();
        let mut pos = 0usize;
        match pluginw_read_tagged(bytes, &mut pos) {
            Some((_, payload)) => payload
                .parse::<f64>()
                .map_err(|_| "plugin returned a malformed Float result".to_string()),
            None => Err("plugin returned a malformed result".to_string()),
        }
    }

    /// Decode the `"O:I<len>:<val>"`/`"E:<message>"` wire produced by
    /// `jet_plugin_call` for a `.call_int` (Int) invocation.
    pub fn jet_plugin_decode_result_int(wire: &str) -> Result<i64, String> {
        let Some(body) = wire.strip_prefix("O:") else {
            return Err(wire.strip_prefix("E:").unwrap_or(wire).to_string());
        };
        let bytes = body.as_bytes();
        let mut pos = 0usize;
        match pluginw_read_tagged(bytes, &mut pos) {
            Some((_, payload)) => payload
                .parse::<i64>()
                .map_err(|_| "plugin returned a malformed Int result".to_string()),
            None => Err("plugin returned a malformed result".to_string()),
        }
    }

