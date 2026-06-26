// jet.db runtime (D-DEP-DB1) — SQLite via rusqlite (bundled).
//
// This file is emitted verbatim into the hidden FFI bridge crate (see
// Source/FFI.rs) when a Jet program uses `jet.db`. The compiler crate
// (`Source/`) never depends on `rusqlite`; it only ships this text.
// Owner-approved I6 bootstrap exception: bundled SQLite is compiled by
// rusqlite's build.rs from the C source it vendors.
//
// Connection handles are u64 keys into a thread-local HashMap. Handle 0
// is the error sentinel (never a live connection).

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

/// Execute a SQL statement that returns no rows (DDL, INSERT, UPDATE, DELETE).
/// Returns `true` on success, `false` on error or invalid handle.
pub fn jet_db_exec(handle: u64, sql: &str) -> bool {
    DB_CONNS.with(|m| {
        let map = m.borrow();
        let Some(conn) = map.get(&handle) else { return false };
        conn.execute_batch(sql).is_ok()
    })
}

/// Execute a SELECT statement and return the result rows as a JSON array.
/// Each row is a JSON object mapping column names to values.
/// NULL → `null`, integers → bare numbers, reals → bare numbers,
/// text → quoted strings, blobs → `null`.
/// Returns `"[]"` on an empty result or invalid handle; `"[]"` on error.
pub fn jet_db_query_json(handle: u64, sql: &str) -> String {
    DB_CONNS.with(|m| {
        let map = m.borrow();
        let Some(conn) = map.get(&handle) else { return "[]".to_string() };
        let Ok(mut stmt) = conn.prepare(sql) else { return "[]".to_string() };
        let col_count = stmt.column_count();
        let col_names: Vec<String> = (0..col_count)
            .map(|i| stmt.column_name(i).unwrap_or("").to_string())
            .collect();
        let mut rows_json = String::from("[");
        let mut first_row = true;
        let Ok(rows) = stmt.query([]) else { return "[]".to_string() };
        let mut rows = rows;
        loop {
            let Ok(row_opt) = rows.next() else { break };
            let Some(row) = row_opt else { break };
            if !first_row {
                rows_json.push(',');
            }
            first_row = false;
            rows_json.push('{');
            for (i, col) in col_names.iter().enumerate() {
                if i > 0 {
                    rows_json.push(',');
                }
                // Key
                rows_json.push('"');
                json_escape_into(&mut rows_json, col);
                rows_json.push_str("\":");
                // Value
                use rusqlite::types::ValueRef;
                match row.get_ref(i).unwrap_or(ValueRef::Null) {
                    ValueRef::Null => rows_json.push_str("null"),
                    ValueRef::Integer(n) => rows_json.push_str(&n.to_string()),
                    ValueRef::Real(f) => {
                        // Emit a JSON number; use integer form when lossless.
                        if f.fract() == 0.0 && f.abs() < 1e15 {
                            rows_json.push_str(&(f as i64).to_string());
                        } else {
                            rows_json.push_str(&format!("{}", f));
                        }
                    }
                    ValueRef::Text(b) => {
                        rows_json.push('"');
                        let s = std::str::from_utf8(b).unwrap_or("");
                        json_escape_into(&mut rows_json, s);
                        rows_json.push('"');
                    }
                    ValueRef::Blob(_) => rows_json.push_str("null"),
                }
            }
            rows_json.push('}');
        }
        rows_json.push(']');
        rows_json
    })
}

fn json_escape_into(out: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
}
