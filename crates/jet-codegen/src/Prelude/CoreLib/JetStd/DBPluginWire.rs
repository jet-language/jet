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

    /// Validate the small, closed policy language before a scope is created.
    /// Keeping this check next to the SQL transformer lets the JIT and AOT
    /// adapters call the same policy semantics.
    pub fn jet_db_policy_validate(table: &str, expression: &str) -> Result<(), String> {
        if table.trim().is_empty()
            || expression.trim().is_empty()
            || table.len() > 1024 * 1024
            || expression.len() > 1024 * 1024
            || table.chars().any(char::is_control)
            || expression.chars().any(char::is_control)
        {
            return Err("row policy needs a table and expression".to_string());
        }
        if !table
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err("row policy table must be a simple identifier".to_string());
        }
        match expression.trim() {
            "true" | "owner == user" => Ok(()),
            other => Err(format!(
                "unsupported row policy expression `{other}`; supported forms are `true` and `owner == user`"
            )),
        }
    }

    fn db_sql_tokens(sql: &str) -> Vec<(usize, usize, String)> {
        let bytes = sql.as_bytes();
        let mut tokens = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\'' {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\'' {
                        if bytes.get(i + 1) == Some(&b'\'') {
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            if bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
                {
                    i += 1;
                }
                tokens.push((start, i, sql[start..i].to_ascii_lowercase()));
            } else {
                i += 1;
            }
        }
        tokens
    }

    /// Policy rewriting only accepts the small, closed SQL shape that the
    /// transformer can prove safe. Comments are rejected instead of being
    /// copied through: a trailing `--` can otherwise hide the predicate that
    /// this function appends.
    fn db_sql_contains_comment(sql: &str) -> bool {
        let bytes = sql.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\'' {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\'' {
                        if bytes.get(i + 1) == Some(&b'\'') {
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            if bytes.get(i..i + 2) == Some(b"--")
                || bytes.get(i..i + 2) == Some(b"/*")
                || bytes.get(i..i + 2) == Some(b"*/")
            {
                return true;
            }
            i += 1;
        }
        false
    }

    fn db_sql_target_index(tokens: &[(usize, usize, String)], kind: &str) -> Option<usize> {
        let keyword = match kind {
            "select" | "delete" => "from",
            "insert" => "into",
            _ => kind,
        };
        tokens
            .iter()
            .position(|(_, _, word)| word == keyword)
            .and_then(|index| index.checked_add(1))
    }

    fn db_sql_target_table(
        tokens: &[(usize, usize, String)],
        kind: &str,
    ) -> Option<String> {
        let index = db_sql_target_index(tokens, kind)?;
        tokens.get(index).map(|(_, _, word)| word.clone())
    }

    /// Reject joins, aliases, subqueries, and other shapes for which adding a
    /// bare `owner = ?` predicate is not a proof of row isolation. A policy
    /// scope fails closed rather than guessing which relation an unqualified
    /// owner column belongs to.
    fn db_sql_simple_target(sql: &str, tokens: &[(usize, usize, String)], kind: &str) -> bool {
        let count = |word: &str| tokens.iter().filter(|(_, _, token)| token == word).count();
        if count("join") > 0
            || count("union") > 0
            || count("with") > 0
            || count("except") > 0
            || count("intersect") > 0
            || count("conflict") > 0
            || count("upsert") > 0
            || count("replace") > 0
            || count("select") > if kind == "select" { 1 } else { 0 }
            || count("from") > if matches!(kind, "select" | "delete") { 1 } else { 0 }
        {
            return false;
        }
        let Some(target) = db_sql_target_index(tokens, kind) else {
            return false;
        };
        let Some((_, target_end, _)) = tokens.get(target) else {
            return false;
        };
        match kind {
            "select" | "delete" => {
                let clause = tokens
                    .iter()
                    .skip(target + 1)
                    .find(|(_, _, word)| {
                        matches!(word.as_str(), "where" | "order" | "group" | "limit" | "offset" | "returning")
                    })
                    .map(|(start, _, _)| *start)
                    .unwrap_or(sql.len());
                sql[*target_end..clause].trim().is_empty()
            }
            "update" => {
                let Some((set_index, (set_start, _, _))) = tokens
                    .iter()
                    .enumerate()
                    .skip(target + 1)
                    .find(|(_, (_, _, word))| word == "set")
                else {
                    return false;
                };
                let _ = set_index;
                sql[*target_end..*set_start].trim().is_empty()
            }
            "insert" => count("values") == 1 && count("select") == 0,
            _ => false,
        }
    }

    fn db_sql_insert_columns_and_values(
        sql: &str,
        tokens: &[(usize, usize, String)],
    ) -> Option<(Vec<String>, usize)> {
        let into = tokens.iter().position(|(_, _, word)| word == "into")?;
        let table_end = tokens.get(into.checked_add(1)?)?.1;
        let values = tokens.iter().position(|(_, _, word)| word == "values")?;
        let column_start = sql[table_end..].find('(')?.checked_add(table_end)?;
        let column_end = sql[column_start..].find(')')?.checked_add(column_start)?;
        let value_start = sql[tokens.get(values)?.1..]
            .find('(')?
            .checked_add(tokens.get(values)?.1)?;
        let value_end = sql[value_start..].find(')')?.checked_add(value_start)?;
        let columns = sql[column_start + 1..column_end]
            .split(',')
            .map(str::trim)
            .collect::<Vec<_>>();
        let values = sql[value_start + 1..value_end]
            .split(',')
            .map(str::trim)
            .collect::<Vec<_>>();
        if columns.is_empty()
            || columns.len() != values.len()
            || columns.iter().any(|column| {
                column.is_empty()
                    || !column
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
            || values.iter().any(|value| *value != "?")
        {
            return None;
        }
        let owner = columns
            .iter()
            .position(|column| column.eq_ignore_ascii_case("owner"))?;
        Some((
            columns
                .into_iter()
                .map(str::to_ascii_lowercase)
                .collect(),
            owner,
        ))
    }

    fn db_sql_clause_start(tokens: &[(usize, usize, String)], after: usize) -> Option<usize> {
        tokens
            .iter()
            .filter(|(_, _, word)| {
                matches!(word.as_str(), "order" | "group" | "limit" | "offset" | "returning")
            })
            .find(|(start, _, _)| *start > after)
            .map(|(start, _, _)| *start)
    }

    fn db_sql_update_mutates_owner(tokens: &[(usize, usize, String)]) -> bool {
        let Some(set_index) = tokens.iter().position(|(_, _, word)| word == "set") else {
            return false;
        };
        let end_index = tokens
            .iter()
            .enumerate()
            .skip(set_index + 1)
            .find(|(_, (_, _, word))| {
                matches!(word.as_str(), "where" | "order" | "group" | "limit" | "offset" | "returning")
            })
            .map(|(index, _)| index)
            .unwrap_or(tokens.len());
        tokens[set_index + 1..end_index]
            .iter()
            .any(|(_, _, word)| word == "owner")
    }

    fn db_sql_is_migration_metadata(sql: &str) -> bool {
        let normalized = sql
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase();
        normalized.starts_with("create table if not exists __jet_migrations")
            || normalized.starts_with("select checksum from __jet_migrations where name = ?")
            || normalized.starts_with(
                "insert into __jet_migrations (name, checksum) values (?, ?)",
            )
    }

    /// Apply the closed owner policy to one SQL operation. Unsupported SQL is
    /// rejected, never passed through unscoped. The returned bind list carries
    /// the user as a parameter, so the policy value cannot become SQL text.
    pub fn jet_db_apply_policy(
        sql: &str,
        params: &Vec<DBValue>,
        table: &str,
        expression: &str,
        user: &str,
    ) -> Result<(String, Vec<DBValue>), DBError> {
        jet_db_apply_policy_inner(sql, params, table, expression, user, false)
    }

    /// Apply a policy while executing an explicit `db.migrate` step. Schema
    /// statements are an authority of migration, but row statements still use
    /// the same table/predicate/bind transformation as ordinary scope calls.
    pub fn jet_db_apply_migration_policy(
        sql: &str,
        params: &Vec<DBValue>,
        table: &str,
        expression: &str,
        user: &str,
    ) -> Result<(String, Vec<DBValue>), DBError> {
        jet_db_apply_policy_inner(sql, params, table, expression, user, true)
    }

    fn jet_db_apply_policy_inner(
        sql: &str,
        params: &Vec<DBValue>,
        table: &str,
        expression: &str,
        user: &str,
        allow_schema: bool,
    ) -> Result<(String, Vec<DBValue>), DBError> {
        jet_db_policy_validate(table, expression).map_err(|message| DBError { message })?;
        if sql.len() > 1024 * 1024
            || sql
                .chars()
                .any(|c| c == ';' || c == '\0' || (c.is_control() && !c.is_whitespace()))
        {
            return Err(DBError {
                message: "policy-scoped SQL must be one statement without control characters".to_string(),
            });
        }
        if db_sql_contains_comment(sql) {
            return Err(DBError {
                message: "policy-scoped SQL cannot contain comments".to_string(),
            });
        }
        if user.is_empty() || user.len() > 1024 * 1024 || user.chars().any(char::is_control) {
            return Err(DBError {
                message: "policy user identity is empty, too long, or contains control characters"
                    .to_string(),
            });
        }
        let tokens = db_sql_tokens(sql);
        let Some((_, _, first)) = tokens.first() else {
            return Err(DBError { message: "policy-scoped SQL is empty".to_string() });
        };
        if db_sql_is_migration_metadata(sql) {
            if !allow_schema {
                return Err(DBError {
                    message: "migration metadata is managed only by db.migrate".to_string(),
                });
            }
            return Ok((sql.to_string(), params.clone()));
        }
        let kind = first.as_str();
        if !matches!(kind, "select" | "update" | "delete" | "insert") {
            if matches!(kind, "create" | "alter" | "drop" | "pragma" | "begin" | "commit" | "rollback") {
                if allow_schema {
                    return Ok((sql.to_string(), params.clone()));
                }
                return Err(DBError {
                    message: "schema and transaction-control SQL is only allowed in db.migrate or through the scope transaction controls".to_string(),
                });
            }
            return Err(DBError {
                message: "row policy supports SELECT, INSERT, UPDATE, and DELETE only".to_string(),
            });
        }
        let expected_table = table.to_ascii_lowercase();
        if db_sql_target_table(&tokens, kind).as_deref() != Some(expected_table.as_str()) {
            return Err(DBError {
                message: format!("policy scope targets table `{table}`"),
            });
        }
        if !db_sql_simple_target(sql, &tokens, kind) {
            return Err(DBError {
                message: "policy-scoped SQL must name one simple target table without joins or subqueries".to_string(),
            });
        }
        if expression.trim() == "true" {
            return Ok((sql.to_string(), params.clone()));
        }
        if kind == "insert" {
            let Some((columns, owner_index)) = db_sql_insert_columns_and_values(sql, &tokens)
            else {
                return Err(DBError {
                    message: "owner policy requires INSERT columns and `?` values with an owner column"
                        .to_string(),
                });
            };
            if columns.len() != params.len() {
                return Err(DBError {
                    message: "owner policy INSERT bind count does not match its columns".to_string(),
                });
            }
            let mut scoped_params = params.clone();
            scoped_params[owner_index] = DBValue::Text(user.to_string());
            return Ok((sql.to_string(), scoped_params));
        }
        if kind == "update" && db_sql_update_mutates_owner(&tokens) {
            return Err(DBError {
                message: "owner policy does not allow changing the owner column".to_string(),
            });
        }
        let mut scoped_params = params.clone();
        scoped_params.push(DBValue::Text(user.to_string()));
        let where_token = tokens
            .iter()
            .find(|(_, _, word)| word == "where")
            .map(|(start, end, _)| (*start, *end));
        let insertion = where_token
            .and_then(|(_, end)| db_sql_clause_start(&tokens, end))
            .or_else(|| db_sql_clause_start(&tokens, 0))
            .unwrap_or_else(|| sql.len());
        let (head, tail) = sql.split_at(insertion);
        let suffix = if tail.trim().is_empty() {
            String::new()
        } else {
            format!(" {}", tail.trim_start())
        };
        let scoped_sql = if let Some((where_start, where_end)) = where_token {
            let condition = sql[where_end..insertion].trim();
            if condition.is_empty() {
                return Err(DBError {
                    message: "policy-scoped SQL has an empty WHERE clause".to_string(),
                });
            }
            format!(
                "{}WHERE ({}) AND owner = ?{}",
                &sql[..where_start],
                condition,
                suffix,
            )
        } else {
            format!("{} WHERE owner = ?{}", head.trim_end(), suffix)
        };
        Ok((scoped_sql, scoped_params))
    }

    // ── core.db wire codec ──────────────────────────────────────────────────────
    // The FFI bridge crate (built only when a program uses `core.db`, Source/FFI.rs)
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

    const DB_MAX_WIRE_BYTES: usize = 64 * 1024 * 1024;
    const DB_MAX_ROWS: usize = 1_000_000;
    const DB_MAX_COLUMNS: usize = 100_000;

    fn db_read_tagged(bytes: &[u8], pos: &mut usize) -> Result<(char, String), String> {
        let tag = *bytes
            .get(*pos)
            .ok_or_else(|| "database wire ended before a value tag".to_string())?
            as char;
        *pos += 1;
        let len_start = *pos;
        while let Some(byte) = bytes.get(*pos) {
            if *byte == b':' {
                break;
            }
            if !byte.is_ascii_digit() {
                return Err("database wire length is not decimal".to_string());
            }
            *pos += 1;
        }
        if *pos == len_start || bytes.get(*pos) != Some(&b':') {
            return Err("database wire has no value length delimiter".to_string());
        }
        let len: usize = std::str::from_utf8(&bytes[len_start..*pos])
            .map_err(|_| "database wire length is not UTF-8".to_string())?
            .parse()
            .map_err(|_| "database wire value length overflows usize".to_string())?;
        *pos += 1; // skip ':'
        let end = (*pos)
            .checked_add(len)
            .ok_or_else(|| "database wire value length overflows the input".to_string())?;
        let payload = std::str::from_utf8(
            bytes
                .get(*pos..end)
                .ok_or_else(|| "database wire value is truncated".to_string())?,
        )
        .map_err(|_| "database wire value is not UTF-8".to_string())?
        .to_string();
        *pos = end;
        Ok((tag, payload))
    }

    fn db_decode_value(tag: char, payload: &str) -> Result<DBValue, String> {
        match tag {
            'N' if payload.is_empty() => Ok(DBValue::Null),
            'I' => payload
                .parse()
                .map(DBValue::Int)
                .map_err(|_| "database integer value is invalid".to_string()),
            'F' => {
                let value: f64 = payload
                    .parse()
                    .map_err(|_| "database float value is invalid".to_string())?;
                if !value.is_finite() {
                    return Err("database float value is not finite".to_string());
                }
                Ok(DBValue::Float(value))
            }
            'T' => Ok(DBValue::Text(payload.to_string())),
            'B' => match payload {
                "0" => Ok(DBValue::Bool(false)),
                "1" => Ok(DBValue::Bool(true)),
                _ => Err("database boolean value is invalid".to_string()),
            },
            'N' => Err("database null value has a non-empty payload".to_string()),
            _ => Err("database wire contains an unknown value tag".to_string()),
        }
    }

    /// Decode the `"O:" + rows`/`"E:" + message` wire produced by `jet_db_query`.
    pub fn jet_db_decode_query_result(
        wire: &str,
    ) -> Result<Vec<std::collections::BTreeMap<String, DBValue>>, DBError> {
        if wire.len() > DB_MAX_WIRE_BYTES {
            return Err(DBError {
                message: "database result exceeds the wire-size limit".to_string(),
            });
        }
        let Some(body) = wire.strip_prefix("O:") else {
            let msg = wire.strip_prefix("E:").unwrap_or(wire);
            return Err(DBError {
                message: msg.to_string(),
            });
        };
        let bytes = body.as_bytes();
        let mut pos = 0usize;
        let Some(colon) = bytes.iter().position(|b| *b == b':') else {
            return Err(DBError {
                message: "database result is missing its row-count delimiter".to_string(),
            });
        };
        let row_count: usize = std::str::from_utf8(&bytes[..colon])
            .map_err(|_| DBError {
                message: "database row count is not UTF-8".to_string(),
            })?
            .parse()
            .map_err(|_| DBError {
                message: "database row count is invalid".to_string(),
            })?;
        if row_count > DB_MAX_ROWS {
            return Err(DBError {
                message: "database row count exceeds the wire limit".to_string(),
            });
        }
        pos = colon + 1;
        let mut rows = Vec::with_capacity(row_count);
        for _ in 0..row_count {
            let row_start = pos;
            let Some(col_colon) = bytes
                .get(pos..)
                .and_then(|rest| rest.iter().position(|b| *b == b':'))
            else {
                return Err(DBError {
                    message: "database row is missing its column-count delimiter".to_string(),
                });
            };
            let col_count: usize = std::str::from_utf8(&bytes[pos..pos + col_colon])
                .map_err(|_| DBError {
                    message: "database column count is not UTF-8".to_string(),
                })?
                .parse()
                .map_err(|_| DBError {
                    message: "database column count is invalid".to_string(),
                })?;
            if col_count > DB_MAX_COLUMNS {
                return Err(DBError {
                    message: "database column count exceeds the wire limit".to_string(),
                });
            }
            pos += col_colon + 1;
            let mut row = std::collections::BTreeMap::new();
            for _ in 0..col_count {
                let (tag, name) = db_read_tagged(bytes, &mut pos).map_err(|message| DBError {
                    message,
                })?;
                if tag != 'C' {
                    return Err(DBError {
                        message: "database row column has an invalid tag".to_string(),
                    });
                }
                let (vtag, vpayload) = db_read_tagged(bytes, &mut pos).map_err(|message| DBError {
                    message,
                })?;
                let value = db_decode_value(vtag, &vpayload).map_err(|message| DBError {
                    message,
                })?;
                if row.insert(name, value).is_some() {
                    return Err(DBError {
                        message: "database row contains a duplicate column".to_string(),
                    });
                }
            }
            rows.push(row);
            if pos <= row_start {
                return Err(DBError {
                    message: "database row decoder made no progress".to_string(),
                });
            }
        }
        if pos != bytes.len() {
            return Err(DBError {
                message: "database result contains trailing wire bytes".to_string(),
            });
        }
        Ok(rows)
    }

    /// Decode the `"O:" + count`/`"E:" + message` wire produced by `jet_db_execute`.
    pub fn jet_db_decode_execute_result(wire: &str) -> Result<i64, DBError> {
        if wire.len() > DB_MAX_WIRE_BYTES {
            return Err(DBError {
                message: "database execute result exceeds the wire-size limit".to_string(),
            });
        }
        if let Some(n) = wire.strip_prefix("O:") {
            let count = n.parse::<i64>().map_err(|_| DBError {
                message: "database affected-row count is invalid".to_string(),
            })?;
            if count < 0 {
                return Err(DBError {
                    message: "database affected-row count is negative".to_string(),
                });
            }
            return Ok(count);
        }
        let msg = wire.strip_prefix("E:").unwrap_or(wire);
        if msg.is_empty() {
            return Err(DBError {
                message: "database execute result has no error message".to_string(),
            });
        }
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

    /// One transaction/migration state machine for every execution tier. The
    /// backend only supplies begin/commit/rollback and policy-bound SQL I/O;
    /// ordering, checksums, and rollback meaning stay here (I9).
    pub trait JetDBBackend {
        fn begin(&mut self) -> bool;
        fn commit(&mut self) -> bool;
        fn rollback(&mut self);
        fn execute(
            &mut self,
            sql: &String,
            params: &Vec<DBValue>,
            allow_schema: bool,
        ) -> Result<i64, DBError>;
        fn query(
            &mut self,
            sql: &String,
            params: &Vec<DBValue>,
            allow_schema: bool,
        ) -> Result<Vec<std::collections::BTreeMap<String, DBValue>>, DBError>;
    }

    pub fn jet_db_transaction<B: JetDBBackend>(
        backend: &mut B,
        label: &String,
        steps: &Vec<String>,
    ) -> Result<i64, DBError> {
        if !backend.begin() {
            return Err(DBError {
                message: format!("could not begin transaction: {label}"),
            });
        }
        let empty = Vec::new();
        let mut done = 0;
        for sql in steps {
            match backend.execute(sql, &empty, false) {
                Ok(_) => done += 1,
                Err(error) => {
                    backend.rollback();
                    return Err(error);
                }
            }
        }
        if backend.commit() {
            Ok(done)
        } else {
            Err(DBError {
                message: "could not commit transaction".to_string(),
            })
        }
    }

    pub fn jet_db_migrate<B: JetDBBackend>(
        backend: &mut B,
        name: &String,
        steps: &Vec<String>,
    ) -> Result<i64, DBError> {
        if !backend.begin() {
            return Err(DBError {
                message: format!("could not begin migration `{name}`"),
            });
        }
        let empty = Vec::new();
        let create_sql =
            "CREATE TABLE IF NOT EXISTS __jet_migrations (name TEXT PRIMARY KEY, checksum TEXT NOT NULL)"
                .to_string();
        if let Err(error) = backend.execute(&create_sql, &empty, true) {
            backend.rollback();
            return Err(error);
        }
        let checksum = jet_db_migration_checksum(steps);
        let check_sql = "SELECT checksum FROM __jet_migrations WHERE name = ?".to_string();
        let check_params = vec![DBValue::Text(name.clone())];
        let existing = match backend.query(&check_sql, &check_params, true) {
            Ok(rows) => rows,
            Err(error) => {
                backend.rollback();
                return Err(error);
            }
        };
        if let Some(row) = existing.into_iter().next() {
            let old = row
                .get("checksum")
                .and_then(|value| value.text().ok())
                .unwrap_or_default();
            if old == checksum {
                if backend.commit() {
                    return Ok(0);
                }
                return Err(DBError {
                    message: format!("could not commit migration `{name}`"),
                });
            }
            backend.rollback();
            return Err(DBError {
                message: format!("migration `{name}` checksum changed"),
            });
        }
        let mut done = 0;
        for sql in steps {
            match backend.execute(sql, &empty, true) {
                Ok(_) => done += 1,
                Err(error) => {
                    backend.rollback();
                    return Err(error);
                }
            }
        }
        let insert_sql = "INSERT INTO __jet_migrations (name, checksum) VALUES (?, ?)".to_string();
        let insert_params = vec![
            DBValue::Text(name.clone()),
            DBValue::Text(checksum),
        ];
        if let Err(error) = backend.execute(&insert_sql, &insert_params, true) {
            backend.rollback();
            return Err(error);
        }
        if backend.commit() {
            Ok(done)
        } else {
            Err(DBError {
                message: format!("could not commit migration `{name}`"),
            })
        }
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
