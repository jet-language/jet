// jet.db runtime (D-DBDRIVER1) — SQLite via rusqlite (bundled).
//
// This file is emitted verbatim into the hidden FFI bridge crate (see
// Source/FFI.rs) when a Jet program uses `jet.db`. The compiler crate
// (`Source/`) never depends on `rusqlite`; it only ships this text.
// Owner-approved I6 bootstrap exception: bundled SQLite is compiled by
// rusqlite's build.rs from the C source it vendors.
//
// Connection handles are u64 keys into a thread-local HashMap. Handle 0
// is the error sentinel (never a live connection).
//
// D-DBDRIVER1: the generic driver interface is parameterized-query only — no
// raw-string execute escape. `query`/`execute` take SQL text plus a separate
// `[DbValue]` bind list; values never get concatenated into the SQL string.
// The always-compiled prelude (Source/Prelude/CoreLib.rs, `jet_std::DbValue`)
// and this bridge crate are two independently built crates linked at the
// program's final `rustc` invocation, so they can't share Rust types — they
// exchange bind params and result rows as a small tagged-length wire text
// (`encode`/`decode` below), never full SQL text.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

thread_local! {
    static DB_CONNS: RefCell<HashMap<u64, rusqlite::Connection>> =
        RefCell::new(HashMap::new());
}

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

/// Open a SQLite database file at `path`. Returns a handle (> 0) on success,
/// or 0 on failure.
pub fn jet_db_open(path: &str) -> u64 {
    match rusqlite::Connection::open(path) {
        Ok(conn) => {
            let h = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
            DB_CONNS.with(|m| m.borrow_mut().insert(h, conn));
            h
        }
        Err(_) => 0,
    }
}

/// Open an in-memory SQLite database. Returns a handle (> 0) on success,
/// or 0 on failure.
pub fn jet_db_open_memory() -> u64 {
    match rusqlite::Connection::open_in_memory() {
        Ok(conn) => {
            let h = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
            DB_CONNS.with(|m| m.borrow_mut().insert(h, conn));
            h
        }
        Err(_) => 0,
    }
}

/// Close a database connection. Returns `true` if the handle was valid,
/// `false` if the handle was not found (already closed or never opened).
pub fn jet_db_close(handle: u64) -> bool {
    DB_CONNS.with(|m| m.borrow_mut().remove(&handle).is_some())
}

/// Run a `BEGIN`/`COMMIT`/`ROLLBACK` on `handle`. Returns `true` on success,
/// `false` on error or invalid handle. Shared by `.begin()`/`.commit()`/`.rollback()`.
fn run_txn_stmt(handle: u64, stmt: &str) -> bool {
    DB_CONNS.with(|m| {
        let map = m.borrow();
        let Some(conn) = map.get(&handle) else { return false };
        conn.execute_batch(stmt).is_ok()
    })
}

pub fn jet_db_begin(handle: u64) -> bool {
    run_txn_stmt(handle, "BEGIN")
}

pub fn jet_db_commit(handle: u64) -> bool {
    run_txn_stmt(handle, "COMMIT")
}

pub fn jet_db_rollback(handle: u64) -> bool {
    run_txn_stmt(handle, "ROLLBACK")
}

/// Run a SELECT with bound parameters. `params_wire` is the tagged-value-list
/// encoding of a `[DbValue]` (see `encode_value_list`/`decode_value_list` in
/// `jet_std`, mirrored here as `decode_params`). Returns `"O:"` + the
/// tagged-rows wire encoding on success, or `"E:"` + a plain error message.
pub fn jet_db_query(handle: u64, sql: &str, params_wire: &str) -> String {
    DB_CONNS.with(|m| {
        let map = m.borrow();
        let Some(conn) = map.get(&handle) else {
            return "E:no connection for this handle".to_string();
        };
        let params = decode_params(params_wire);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(e) => return format!("E:{}", e),
        };
        let col_count = stmt.column_count();
        let col_names: Vec<String> = (0..col_count)
            .map(|i| stmt.column_name(i).unwrap_or("").to_string())
            .collect();
        let mut rows = match stmt.query(param_refs.as_slice()) {
            Ok(r) => r,
            Err(e) => return format!("E:{}", e),
        };
        let mut out = String::from("O:");
        let mut row_wires: Vec<String> = Vec::new();
        loop {
            let row_opt = match rows.next() {
                Ok(r) => r,
                Err(e) => return format!("E:{}", e),
            };
            let Some(row) = row_opt else { break };
            let mut cols: Vec<(String, rusqlite::types::Value)> = Vec::with_capacity(col_count);
            for (i, name) in col_names.iter().enumerate() {
                use rusqlite::types::ValueRef;
                let v = match row.get_ref(i).unwrap_or(ValueRef::Null) {
                    ValueRef::Null => rusqlite::types::Value::Null,
                    ValueRef::Integer(n) => rusqlite::types::Value::Integer(n),
                    ValueRef::Real(f) => rusqlite::types::Value::Real(f),
                    ValueRef::Text(b) => rusqlite::types::Value::Text(
                        std::str::from_utf8(b).unwrap_or("").to_string(),
                    ),
                    // Blobs have no `DbValue` shape yet — surface as NULL (same
                    // posture as the old `jet_db_query_json`).
                    ValueRef::Blob(_) => rusqlite::types::Value::Null,
                };
                cols.push((name.clone(), v));
            }
            row_wires.push(encode_row(&cols));
        }
        out.push_str(&encode_count_prefixed(&row_wires));
        out
    })
}

/// Run a DDL/INSERT/UPDATE/DELETE with bound parameters. Returns `"O:"` + the
/// affected-row count on success, or `"E:"` + a plain error message.
pub fn jet_db_execute(handle: u64, sql: &str, params_wire: &str) -> String {
    DB_CONNS.with(|m| {
        let map = m.borrow();
        let Some(conn) = map.get(&handle) else {
            return "E:no connection for this handle".to_string();
        };
        let params = decode_params(params_wire);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
        match conn.execute(sql, param_refs.as_slice()) {
            Ok(n) => format!("O:{}", n),
            Err(e) => format!("E:{}", e),
        }
    })
}

// ── wire encoding: tagged, length-prefixed, byte-exact (never escaped) ──────
// A single value: `<tag><decimal-length>:<payload-bytes>`. A list: the decimal
// item count, `:`, then that many self-delimiting items back to back. Every
// length is a byte count, so arbitrary text (including an "injection-looking"
// literal like `'; DROP TABLE x; --`) round-trips exactly — no escaping, no
// quoting, nothing for a hostile payload to break out of.

fn encode_count_prefixed(items: &[String]) -> String {
    let mut out = String::new();
    out.push_str(&items.len().to_string());
    out.push(':');
    for item in items {
        out.push_str(item);
    }
    out
}

fn encode_tagged(tag: char, payload: &str) -> String {
    format!("{tag}{}:{payload}", payload.len())
}

fn encode_row(cols: &[(String, rusqlite::types::Value)]) -> String {
    let mut parts = Vec::with_capacity(cols.len());
    for (name, v) in cols {
        let mut s = encode_tagged('C', name);
        s.push_str(&encode_value(v));
        parts.push(s);
    }
    encode_count_prefixed(&parts)
}

fn encode_value(v: &rusqlite::types::Value) -> String {
    use rusqlite::types::Value;
    match v {
        Value::Null => encode_tagged('N', ""),
        Value::Integer(n) => encode_tagged('I', &n.to_string()),
        Value::Real(f) => encode_tagged('F', &f.to_string()),
        Value::Text(s) => encode_tagged('T', s),
        Value::Blob(_) => encode_tagged('N', ""),
    }
}

/// Read one `<tag><len>:<payload>` item starting at `*pos`; advances `*pos`
/// past it. Returns `None` on a malformed/truncated wire (defensive only —
/// the wire is always produced by `jet_std`'s matching encoder).
fn read_tagged(bytes: &[u8], pos: &mut usize) -> Option<(char, String)> {
    let tag = *bytes.get(*pos)? as char;
    *pos += 1;
    let len_start = *pos;
    while *bytes.get(*pos)? != b':' {
        *pos += 1;
    }
    let len: usize = std::str::from_utf8(&bytes[len_start..*pos]).ok()?.parse().ok()?;
    *pos += 1; // skip ':'
    let payload = std::str::from_utf8(bytes.get(*pos..*pos + len)?).ok()?.to_string();
    *pos += len;
    Some((tag, payload))
}

fn decode_params(wire: &str) -> Vec<rusqlite::types::Value> {
    use rusqlite::types::Value;
    let bytes = wire.as_bytes();
    let mut pos = 0usize;
    let Some(colon) = bytes.iter().position(|b| *b == b':') else { return Vec::new() };
    let Ok(count) = std::str::from_utf8(&bytes[..colon]).unwrap_or("0").parse::<usize>() else {
        return Vec::new();
    };
    pos = colon + 1;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let Some((tag, payload)) = read_tagged(bytes, &mut pos) else { break };
        out.push(match tag {
            'N' => Value::Null,
            'I' => Value::Integer(payload.parse().unwrap_or(0)),
            'F' => Value::Real(payload.parse().unwrap_or(0.0)),
            'T' => Value::Text(payload),
            'B' => Value::Integer(if payload == "1" { 1 } else { 0 }),
            _ => Value::Null,
        });
    }
    out
}
