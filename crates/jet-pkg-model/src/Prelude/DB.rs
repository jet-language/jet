// core.db runtime (D-DBDRIVER1) — SQLite via rusqlite (bundled).
//
// This file is emitted verbatim into the hidden FFI bridge crate (see
// Source/FFI.rs) when a Jet program uses `core.db`. The compiler crate
// (`Source/`) never depends on `rusqlite`; it only ships this text.
// Owner-approved I6 bootstrap exception: bundled SQLite is compiled by
// rusqlite's build.rs from the C source it vendors.
//
// Connection handles are u64 keys into a thread-local HashMap. Handle 0
// is the error sentinel (never a live connection).
//
// D-TYPEDSQL-SINK1=A: the public driver contract carries one `SQL` value,
// pairing its checked template with its ordered `[DBValue]` binds. This hidden
// bridge is the final marshalling seam: only here do the pair's fields split
// into SQLite's `sql` text and bound values. Values never get concatenated into
// SQL text.
//
// The always-compiled prelude (Source/Prelude/CoreLib.rs, `jet_std::DBValue`)
// and this bridge crate are two independently built crates linked at the
// program's final `rustc` invocation, so they can't share Rust types. They
// exchange the final bound values and result rows as tagged-length wire text
// (`encode`/`decode` below). Blob bytes use an ASCII hexadecimal envelope at
// this String boundary, so arbitrary binary values remain lossless.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// Check that a database handle is live and can execute the pool health
/// probe. This stays an adapter fact; Jet code sees only the pool lifecycle.
pub fn jet_db_health(handle: u64) -> bool {
    DB_CONNS.with(|m| {
        let map = m.borrow();
        map.get(&handle).is_some_and(|conn| {
            conn.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
                .is_ok()
        })
    })
}

/// Return a leased connection to its clean transaction state. A connection
/// already in autocommit mode is clean; otherwise rollback is the shared
/// adapter reset operation.
pub fn jet_db_reset(handle: u64) -> bool {
    DB_CONNS.with(|m| {
        let mut map = m.borrow_mut();
        let Some(conn) = map.get_mut(&handle) else {
            return false;
        };
        conn.is_autocommit() || conn.execute_batch("ROLLBACK").is_ok()
    })
}

fn run_txn_stmt(handle: u64, stmt: &str) -> bool {
    DB_CONNS.with(|m| {
        let map = m.borrow();
        let Some(conn) = map.get(&handle) else { return false };
        conn.execute_batch(stmt).is_ok()
    })
}

/// Begin a transaction with the migration lock requested by the shared
/// Prelude kernel.  SQLite's RESERVED write lock is the authoritative
/// exclusive migration lock; a plain deferred transaction is the shared read
/// mode.  Process lock files remain advisory diagnostics only.
pub fn jet_db_begin_mode(handle: u64, mode: i64) -> bool {
    match mode {
        0 => run_txn_stmt(handle, "BEGIN"),
        1 => run_txn_stmt(handle, "BEGIN IMMEDIATE"),
        _ => false,
    }
}

pub fn jet_db_begin(handle: u64) -> bool {
    jet_db_begin_mode(handle, 0)
}

pub fn jet_db_commit(handle: u64) -> bool {
    run_txn_stmt(handle, "COMMIT")
}

pub fn jet_db_rollback(handle: u64) -> bool {
    run_txn_stmt(handle, "ROLLBACK")
}

/// Run a SELECT with bound parameters. `params_wire` is the tagged-value-list
/// encoding of a `[DBValue]` (see `encode_value_list`/`decode_value_list` in
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
        let mut stmt = match conn.prepare_cached(sql) {
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
                    ValueRef::Blob(bytes) => rusqlite::types::Value::Blob(bytes.to_vec()),
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
        let mut stmt = match conn.prepare_cached(sql) {
            Ok(s) => s,
            Err(e) => return format!("E:{}", e),
        };
        match stmt.execute(param_refs.as_slice()) {
            Ok(n) => format!("O:{}", n),
            Err(e) => format!("E:{}", e),
        }
    })
}
#[derive(Clone, Debug)]
struct DbPlanFact {
    ordinal: u64,
    parent: u64,
    operation: String,
    relation: Option<String>,
    index: Option<String>,
}

fn parse_plan_detail(detail: &str) -> (String, Option<String>, Option<String>) {
    let tokens: Vec<&str> = detail.split_whitespace().collect();
    let operation = match tokens.first().copied() {
        Some("USE") if tokens.get(1).copied() == Some("TEMP") => "USE TEMP".to_string(),
        Some(value) => value.to_string(),
        None => "UNKNOWN".to_string(),
    };
    let relation = match tokens.first().copied() {
        Some("SCAN" | "SEARCH") => tokens.get(1).and_then(|value| {
            let value = value.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '_' && character != '.'
            });
            (!value.is_empty() && !value.eq_ignore_ascii_case("SUBQUERY")).then(|| value.to_string())
        }),
        _ => None,
    };
    let index = tokens
        .windows(2)
        .position(|pair| pair[0].eq_ignore_ascii_case("INDEX"))
        .and_then(|position| tokens.get(position + 1))
        .map(|value| {
            value
                .trim_matches(|character: char| {
                    !character.is_ascii_alphanumeric() && character != '_' && character != '.'
                })
                .to_string()
        })
        .filter(|value| !value.is_empty());
    (operation, relation, index)
}

fn query_plan(
    conn: &rusqlite::Connection,
    sql: &str,
    param_refs: &[&dyn rusqlite::types::ToSql],
) -> Vec<DbPlanFact> {
    let explain_sql = format!("EXPLAIN QUERY PLAN {}", sql.trim().trim_end_matches(';'));
    let Ok(mut statement) = conn.prepare(&explain_sql) else {
        return Vec::new();
    };
    let Ok(mut rows) = statement.query(param_refs) else {
        return Vec::new();
    };
    let mut plan = Vec::new();
    while let Ok(Some(row)) = rows.next() {
        let Ok(ordinal) = row.get::<_, i64>(0) else {
            continue;
        };
        let Ok(parent) = row.get::<_, i64>(1) else {
            continue;
        };
        let Ok(detail) = row.get::<_, String>(3) else {
            continue;
        };
        if ordinal < 0 || parent < 0 {
            continue;
        }
        let (operation, relation, index) = parse_plan_detail(&detail);
        plan.push(DbPlanFact {
            ordinal: ordinal as u64,
            parent: parent as u64,
            operation,
            relation,
            index,
        });
    }
    plan
}

fn query_plan_limited(
    conn: &rusqlite::Connection,
    sql: &str,
    param_refs: &[&dyn rusqlite::types::ToSql],
    max_rows: usize,
    timeout_ms: u64,
) -> Result<(Vec<DbPlanFact>, bool, bool), String> {
    let timeout_flag = Arc::new(AtomicBool::new(false));
    let callback_flag = Arc::clone(&timeout_flag);
    let timeout = Duration::from_millis(timeout_ms);
    let started = Instant::now();
    conn.progress_handler(
        1_000,
        Some(move || {
            if started.elapsed() >= timeout {
                callback_flag.store(true, Ordering::Relaxed);
                true
            } else {
                false
            }
        }),
    );
    let result = (|| {
        let explain_sql = format!("EXPLAIN QUERY PLAN {}", sql.trim());
        let mut statement = conn.prepare(&explain_sql).map_err(|error| error.to_string())?;
        let mut rows = statement
            .query(param_refs)
            .map_err(|error| error.to_string())?;
        let mut plan = Vec::new();
        let mut truncated = false;
        let max_rows = max_rows.min(256);
        loop {
            let row = match rows.next() {
                Ok(row) => row,
                Err(_error) if timeout_flag.load(Ordering::Relaxed) => {
                    return Ok((plan, true, truncated));
                }
                Err(error) => return Err(error.to_string()),
            };
            let Some(row) = row else {
                break;
            };
            if plan.len() >= max_rows {
                truncated = true;
                break;
            }
            let Ok(ordinal) = row.get::<_, i64>(0) else {
                continue;
            };
            let Ok(parent) = row.get::<_, i64>(1) else {
                continue;
            };
            let Ok(detail) = row.get::<_, String>(3) else {
                continue;
            };
            if ordinal < 0 || parent < 0 {
                continue;
            }
            let (operation, relation, index) = parse_plan_detail(&detail);
            plan.push(DbPlanFact {
                ordinal: ordinal as u64,
                parent: parent as u64,
                operation,
                relation,
                index,
            });
        }
        Ok((plan, timeout_flag.load(Ordering::Relaxed), truncated))
    })();
    conn.progress_handler(0, None::<fn() -> bool>);
    result
}

fn explain_sql_is_select(sql: &str) -> bool {
    let text = sql.trim();
    if text.is_empty()
        || text.contains(';')
        || text.contains("--")
        || text.contains("/*")
        || text.contains("*/")
    {
        return false;
    }
    text.split_whitespace()
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("select"))
}

fn encode_plan(facts: &[DbPlanFact]) -> String {
    let mut out = String::new();
    out.push_str(&facts.len().to_string());
    out.push(':');
    for fact in facts {
        out.push_str(&encode_tagged('I', &fact.ordinal.to_string()));
        out.push_str(&encode_tagged('P', &fact.parent.to_string()));
        out.push_str(&encode_tagged('O', &fact.operation));
        out.push_str(&encode_tagged('T', fact.relation.as_deref().unwrap_or("")));
        out.push_str(&encode_tagged('X', fact.index.as_deref().unwrap_or("")));
    }
    out
}

fn query_rows(
    conn: &rusqlite::Connection,
    sql: &str,
    param_refs: &[&dyn rusqlite::types::ToSql],
) -> Result<Vec<String>, String> {
    let mut statement = conn.prepare_cached(sql).map_err(|error| error.to_string())?;
    let column_count = statement.column_count();
    let column_names: Vec<String> = (0..column_count)
        .map(|index| statement.column_name(index).unwrap_or("").to_string())
        .collect();
    let mut rows = statement
        .query(param_refs)
        .map_err(|error| error.to_string())?;
    let mut row_wires = Vec::new();
    loop {
        let row_opt = rows.next().map_err(|error| error.to_string())?;
        let Some(row) = row_opt else { break };
        let mut columns: Vec<(String, rusqlite::types::Value)> =
            Vec::with_capacity(column_count);
        for (index, name) in column_names.iter().enumerate() {
            use rusqlite::types::ValueRef;
            let value = match row.get_ref(index).unwrap_or(ValueRef::Null) {
                ValueRef::Null => rusqlite::types::Value::Null,
                ValueRef::Integer(number) => rusqlite::types::Value::Integer(number),
                ValueRef::Real(number) => rusqlite::types::Value::Real(number),
                ValueRef::Text(bytes) => rusqlite::types::Value::Text(
                    std::str::from_utf8(bytes).unwrap_or("").to_string(),
                ),
                ValueRef::Blob(bytes) => rusqlite::types::Value::Blob(bytes.to_vec()),
            };
            columns.push((name.clone(), value));
        }
        row_wires.push(encode_row(&columns));
    }
    Ok(row_wires)
}

/// Run a SELECT and return rows plus the actual execution duration and
/// driver-provided query-plan facts.
pub fn jet_db_query_observed(handle: u64, sql: &str, params_wire: &str) -> String {
    DB_CONNS.with(|connections| {
        let map = connections.borrow();
        let Some(conn) = map.get(&handle) else {
            return "E:no connection for this handle".to_string();
        };
        let params = decode_params(params_wire);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|value| value as &dyn rusqlite::types::ToSql).collect();
        let started = Instant::now();
        let row_wires = match query_rows(conn, sql, param_refs.as_slice()) {
            Ok(rows) => rows,
            Err(error) => return format!("E:{error}"),
        };
        let elapsed_ms = started.elapsed().as_millis() as u64;
        let plan = query_plan(conn, sql, param_refs.as_slice());
        let rows_wire = encode_count_prefixed(&row_wires);
        format!(
            "Q:{}{}{}{}",
            encode_tagged('R', &rows_wire),
            encode_tagged('N', &row_wires.len().to_string()),
            encode_tagged('D', &elapsed_ms.to_string()),
            encode_tagged('P', &encode_plan(&plan)),
        )
    })
}

/// Run a mutation and return the actual affected-row count and execution
/// duration.  A mutating statement has no EXPLAIN payload.
pub fn jet_db_execute_observed(handle: u64, sql: &str, params_wire: &str) -> String {
    DB_CONNS.with(|connections| {
        let map = connections.borrow();
        let Some(conn) = map.get(&handle) else {
            return "E:no connection for this handle".to_string();
        };
        let params = decode_params(params_wire);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|value| value as &dyn rusqlite::types::ToSql).collect();
        let started = Instant::now();
        let mut statement = match conn.prepare_cached(sql) {
            Ok(statement) => statement,
            Err(error) => return format!("E:{error}"),
        };
        let affected = match statement.execute(param_refs.as_slice()) {
            Ok(count) => count,
            Err(error) => return format!("E:{error}"),
        };
        let elapsed_ms = started.elapsed().as_millis() as u64;
        format!(
            "X:{}{}{}",
            encode_tagged('A', &affected.to_string()),
            encode_tagged('D', &elapsed_ms.to_string()),
            encode_tagged('P', "0:"),
        )
    })
}
/// Run a bounded, read-only EXPLAIN QUERY PLAN.  The driver never executes the
/// original SELECT: it prepares only SQLite's plan statement, interrupts it
/// through the progress callback at the requested deadline, and limits the
/// number of returned plan rows before they cross the FFI boundary.
pub fn jet_db_explain(
    handle: u64,
    sql: &str,
    params_wire: &str,
    max_rows: u64,
    timeout_ms: u64,
) -> String {
    DB_CONNS.with(|connections| {
        let map = connections.borrow();
        let Some(conn) = map.get(&handle) else {
            return "E:no connection for this handle".to_string();
        };
        if !explain_sql_is_select(sql) {
            return "E:database EXPLAIN accepts read-only SELECT statements only".to_string();
        }
        let params = decode_params(params_wire);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|value| value as &dyn rusqlite::types::ToSql).collect();
        let started = Instant::now();
        let max_rows = usize::try_from(max_rows).unwrap_or(256).min(256);
        match query_plan_limited(
            conn,
            sql,
            param_refs.as_slice(),
            max_rows,
            timeout_ms,
        ) {
            Ok((plan, timed_out, truncated)) => {
                let elapsed_ms = started.elapsed().as_millis() as u64;
                format!(
                    "Y:{}{}{}{}",
                    encode_tagged('D', &elapsed_ms.to_string()),
                    encode_tagged('T', if timed_out { "1" } else { "0" }),
                    encode_tagged('L', if truncated { "1" } else { "0" }),
                    encode_tagged('P', &encode_plan(&plan)),
                )
            }
            Err(error) => format!("E:{error}"),
        }
    })
}


// ── wire encoding: tagged, length-prefixed, byte-exact (never escaped) ──────
// A single value: `<tag><decimal-length>:<payload-bytes>`. A list: the decimal
// item count, `:`, then that many self-delimiting items back to back. Every
// length is a byte count, so arbitrary text (including an "injection-looking"
// literal like `'; DROP TABLE x; --`) round-trips exactly — no escaping, no
// quoting, nothing for a hostile payload to break out of. Blob payloads use an
// ASCII hexadecimal envelope because this bridge's wire carrier is `String`.

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

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(payload: &str) -> Option<Vec<u8>> {
    if payload.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(payload.len() / 2);
    for pair in payload.as_bytes().chunks_exact(2) {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        out.push(((high << 4) | low) as u8);
    }
    Some(out)
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
        Value::Blob(bytes) => encode_tagged('X', &hex_encode(bytes)),
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
    let Some(colon) = bytes.iter().position(|b| *b == b':') else { return Vec::new() };
    let Ok(count) = std::str::from_utf8(&bytes[..colon]).unwrap_or("0").parse::<usize>() else {
        return Vec::new();
    };
    let mut pos = colon + 1;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let Some((tag, payload)) = read_tagged(bytes, &mut pos) else { break };
        out.push(match tag {
            'N' => Value::Null,
            'I' => Value::Integer(payload.parse().unwrap_or(0)),
            'F' => Value::Real(payload.parse().unwrap_or(0.0)),
            'T' => Value::Text(payload),
            'B' => Value::Integer(if payload == "1" { 1 } else { 0 }),
            'X' => hex_decode(&payload).map(Value::Blob).unwrap_or(Value::Null),
            _ => Value::Null,
        });
    }
    out
}
