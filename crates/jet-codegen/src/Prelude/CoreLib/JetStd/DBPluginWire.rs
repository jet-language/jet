// ── core.db: the tagged SQL parameter/column value (D-DBDRIVER1) ───────────
// `DBValue` mirrors `JSON`'s dynamic-value construction mechanism
// (`DBValue.Int(n)` / `.Float(f)` / `.Text(s)` / `.Bool(b)` / `.Blob(bytes)` /
// `.Null`) but is SQL-shaped: `Int` keeps the full 64-bit width SQLite integers
// carry (never routed through `f64`, which would lose precision above 2^53).
// `Blob` carries an exact `[U8]` sequence. A `Row` is `Map<String, DBValue>` —
// the built-in `Map` type already gives `.get`/`.keys`/`.values`, so no separate
// nominal `Row` type is needed (I8). Keep the Rust carrier behind one adapter
// alias so generated callers never depend on the backing map implementation.
pub(super) type JetDBRow = super::JetMap<String, DBValue>;

#[derive(Clone, Debug, PartialEq)]
pub enum DBValue {
    Null,
    Int(i64),
    Float(f64),
    Text(String),
    Bool(bool),
    Blob(Vec<u8>),
}

/// D-TYPEDSQL-SINK1=A: checked SQL is one value containing template text and
/// ordered database bindings. Sinks receive this alias, not separate text and
/// parameter arguments.
pub type SQL = (String, Vec<DBValue>);

impl super::JetShow for DBValue {
    fn jet_show(&self) -> String {
        render_db_value(self)
    }
}

impl super::JetDisplay for DBValue {
    fn jet_display(&self) -> String {
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
        DBValue::Blob(bytes) => format!("{bytes:?}"),
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
            // SQLite BOOLEAN is INTEGER 0/1; query results keep that affinity.
            DBValue::Int(0) => Ok(false),
            DBValue::Int(1) => Ok(true),
            _ => Err(format!("expected a bool, got {}", render_db_value(self))),
        }
    }
    pub fn blob(&self) -> Result<Vec<u8>, String> {
        match self {
            DBValue::Blob(bytes) => Ok(bytes.clone()),
            _ => Err(format!("expected a blob, got {}", render_db_value(self))),
        }
    }
}

pub fn jet_db_row_value(row: &JetDBRow, key: &String) -> Result<DBValue, String> {
    row.get(key)
        .cloned()
        .ok_or_else(|| format!("missing column `{}`", key))
}

pub fn jet_db_row_int(row: &JetDBRow, key: &String) -> Result<i64, String> {
    jet_db_row_value(row, key).and_then(|v| v.int())
}

pub fn jet_db_row_float(row: &JetDBRow, key: &String) -> Result<f64, String> {
    jet_db_row_value(row, key).and_then(|v| v.float())
}

pub fn jet_db_row_text(row: &JetDBRow, key: &String) -> Result<String, String> {
    jet_db_row_value(row, key).and_then(|v| v.text())
}

pub fn jet_db_row_bool(row: &JetDBRow, key: &String) -> Result<bool, String> {
    jet_db_row_value(row, key).and_then(|v| v.bool())
}
/// D-SHAPE-ONE1=A: project checked DB column names into ordered entries for
/// the canonical DataTree adapter. The callback is supplied by each typed host
/// so this carrier remains usable by AOT, JIT, and interpreter modules without
/// defining a second decoder trait here.
pub fn jet_db_row_project<T, F>(
    row: &JetDBRow,
    names: &[(&str, &str)],
    mut convert: F,
) -> Vec<(String, T)>
where
    F: FnMut(&DBValue) -> T,
{
    names
        .iter()
        .filter_map(|(db_name, json_name)| {
            row.get(*db_name)
                .map(|value| ((*json_name).to_string(), convert(value)))
        })
        .collect()
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

impl super::JetDebug for DBError {
    fn jet_debug(&self) -> String {
        <Self as super::JetShow>::jet_show(self)
    }
}

// D-FAIL-CONV2=A: one display hook backs both the shipped impl DBError -> Err
// and user interpolation.
impl super::JetDisplay for DBError {
    fn jet_display(&self) -> String {
        <Self as super::JetShow>::jet_show(self)
    }
}

/// Static relation facts checked from the SQL literal.  Runtime adapters
/// append observed counters and plans; they never infer a table from row
/// counts or result shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDbTableFact {
    pub table_id: String,
    pub read: bool,
    pub write: bool,
}

/// Source identity and checked relation projection carried to a DB driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDbQueryMetadata {
    pub source_id: String,
    pub source_file: String,
    pub source_start: u64,
    pub source_end: u64,
    pub statement_identity: String,
    pub table_facts: Vec<JetDbTableFact>,
}

/// One observed SQLite query-plan row, reduced to stable diagnostic facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDbQueryPlanFact {
    pub ordinal: u64,
    pub parent: u64,
    pub operation: String,
    pub relation: Option<String>,
    pub index: Option<String>,
}

/// Runtime-only facts returned by the database driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDbQueryRuntimeFacts {
    pub rows_returned: Option<u64>,
    pub rows_affected: Option<u64>,
    pub elapsed_ms: u64,
    pub plan: Vec<JetDbQueryPlanFact>,
}

/// Runtime facts returned by a bounded, read-only EXPLAIN operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDbExplainRuntimeFacts {
    pub elapsed_ms: u64,
    pub plan: Vec<JetDbQueryPlanFact>,
    pub timed_out: bool,
    pub truncated: bool,
}

const DB_MAX_METADATA_BYTES: usize = 64 * 1024;
const DB_MAX_METADATA_TABLES: usize = 256;
const DB_MAX_PLAN_ROWS: usize = 256;

fn db_read_metadata_field(bytes: &[u8], pos: &mut usize) -> Result<String, DBError> {
    let len_start = *pos;
    while let Some(byte) = bytes.get(*pos) {
        if *byte == b':' {
            break;
        }
        if !byte.is_ascii_digit() {
            return Err(DBError {
                message: "database metadata length is not decimal".to_string(),
            });
        }
        *pos += 1;
    }
    if *pos == len_start || bytes.get(*pos) != Some(&b':') {
        return Err(DBError {
            message: "database metadata has no field delimiter".to_string(),
        });
    }
    let length = std::str::from_utf8(&bytes[len_start..*pos])
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| DBError {
            message: "database metadata field length is invalid".to_string(),
        })?;
    *pos += 1;
    let end = (*pos).checked_add(length).ok_or_else(|| DBError {
        message: "database metadata field length overflow".to_string(),
    })?;
    let value = bytes.get(*pos..end).ok_or_else(|| DBError {
        message: "database metadata field is truncated".to_string(),
    })?;
    *pos = end;
    String::from_utf8(value.to_vec()).map_err(|_| DBError {
        message: "database metadata field is not UTF-8".to_string(),
    })
}

fn db_metadata_number(value: String, what: &str) -> Result<u64, DBError> {
    value.parse::<u64>().map_err(|_| DBError {
        message: format!("database metadata {what} is invalid"),
    })
}

impl JetDbQueryMetadata {
    pub fn from_wire(wire: &str) -> Result<Self, DBError> {
        if wire.len() > DB_MAX_METADATA_BYTES {
            return Err(DBError {
                message: "database metadata exceeds the wire-size limit".to_string(),
            });
        }
        let bytes = wire.as_bytes();
        if !bytes.starts_with(b"JDB1:") {
            return Err(DBError {
                message: "database metadata has an unknown version".to_string(),
            });
        }
        let mut pos = 5;
        let source_file = db_read_metadata_field(bytes, &mut pos)?;
        let source_id = db_read_metadata_field(bytes, &mut pos)?;
        let source_start = db_metadata_number(
            db_read_metadata_field(bytes, &mut pos)?,
            "source span start",
        )?;
        let source_end =
            db_metadata_number(db_read_metadata_field(bytes, &mut pos)?, "source span end")?;
        let statement_identity = db_read_metadata_field(bytes, &mut pos)?;
        let table_count = usize::try_from(db_metadata_number(
            db_read_metadata_field(bytes, &mut pos)?,
            "table count",
        )?)
        .map_err(|_| DBError {
            message: "database metadata table count overflows usize".to_string(),
        })?;
        if source_file.is_empty() || source_id.is_empty() || statement_identity.is_empty() {
            return Err(DBError {
                message: "database metadata has an empty source or statement identity".to_string(),
            });
        }
        if source_end < source_start {
            return Err(DBError {
                message: "database metadata source span is inverted".to_string(),
            });
        }
        if table_count > DB_MAX_METADATA_TABLES {
            return Err(DBError {
                message: "database metadata table count exceeds the limit".to_string(),
            });
        }
        let mut table_facts = Vec::with_capacity(table_count);
        for _ in 0..table_count {
            let table_id = db_read_metadata_field(bytes, &mut pos)?;
            let read = match db_read_metadata_field(bytes, &mut pos)?.as_str() {
                "1" => true,
                "0" => false,
                _ => {
                    return Err(DBError {
                        message: "database metadata read flag is invalid".to_string(),
                    })
                }
            };
            let write = match db_read_metadata_field(bytes, &mut pos)?.as_str() {
                "1" => true,
                "0" => false,
                _ => {
                    return Err(DBError {
                        message: "database metadata write flag is invalid".to_string(),
                    })
                }
            };
            if table_id.is_empty() || (!read && !write) {
                return Err(DBError {
                    message: "database metadata has an invalid table fact".to_string(),
                });
            }
            if table_facts.iter().any(|fact: &JetDbTableFact| fact.table_id == table_id) {
                return Err(DBError {
                    message: "database metadata repeats a table fact".to_string(),
                });
            }
            table_facts.push(JetDbTableFact {
                table_id,
                read,
                write,
            });
        }
        if pos != bytes.len() {
            return Err(DBError {
                message: "database metadata has trailing bytes".to_string(),
            });
        }
        Ok(Self {
            source_id,
            source_file,
            source_start,
            source_end,
            statement_identity,
            table_facts,
        })
    }
}

/// EXPLAIN is deliberately narrower than the normal DB sink: only one
/// SELECT statement is eligible.  CTEs and transaction/schema/mutation
/// statements are rejected before a driver sees them.
pub fn jet_db_validate_explain_sql(sql: &SQL) -> Result<(), DBError> {
    let text = sql.0.trim();
    if text.is_empty() || text.contains(';') || db_sql_contains_comment(text) {
        return Err(DBError {
            message: "database EXPLAIN accepts one comment-free SELECT".to_string(),
        });
    }
    let tokens = db_sql_tokens(text);
    if tokens.first().map(|(_, _, word)| word.as_str()) != Some("select") {
        return Err(DBError {
            message: "database EXPLAIN accepts read-only SELECT statements only".to_string(),
        });
    }
    Ok(())
}

/// The transformer returns the proof together with the rewritten typed SQL.
/// A backend may marshal the two payload fields only at its final driver
/// boundary; it cannot get policy SQL without first receiving this proof-bearing
/// application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDBPolicyProof {
    table: String,
    operation: String,
    predicate: String,
    user: String,
    user_bind_index: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDBPolicyApplication {
    pub sql: SQL,
    pub proof: JetDBPolicyProof,
}

impl JetDBPolicyApplication {
    pub fn into_sql(self) -> Result<SQL, DBError> {
        let row_filter_proved = self.proof.predicate == "true"
            || self.proof.operation.starts_with("migration-")
            || self.proof.user_bind_index.is_some_and(|index| {
                self.sql
                    .1
                    .get(index)
                    == Some(&DBValue::Text(self.proof.user.clone()))
            });
        if !row_filter_proved {
            return Err(DBError {
                message: "row policy proof does not match the rewritten SQL binds".to_string(),
            });
        }
        if self.proof.predicate == "true"
            || self.proof.operation.starts_with("migration-")
            || self.proof.operation == "insert"
            || self.sql.0.contains("owner = ?")
        {
            Ok(self.sql)
        } else {
            Err(DBError {
                message: "row policy proof does not match the rewritten SQL predicate".to_string(),
            })
        }
    }
}

// The closed policy language lives in `RowPolicy.rs`, included beside this
// fragment on every tier. Public raw-text entry points compile once at the
// boundary; scoped entry points below receive the compiled enum directly.

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
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
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

fn db_sql_target_table(tokens: &[(usize, usize, String)], kind: &str) -> Option<String> {
    let index = db_sql_target_index(tokens, kind)?;
    tokens.get(index).map(|(_, _, word)| word.clone())
}
/// Return the deterministic table write set carried by one transaction.
///
/// The policy checker already restricts ordinary scoped DML to one simple
/// target, so this helper only extracts the target from statements that can
/// change rows or schema.  It intentionally returns relation names rather
/// than effect labels: `app.live` records the same table footprint, while
/// `jet_app_transact_invalidate` performs the shared intersection check after
/// commit.
pub fn jet_db_write_set(steps: &Vec<SQL>) -> String {
    let mut tables = std::collections::BTreeSet::new();
    for sql in steps {
        let tokens = db_sql_tokens(sql.0.trim());
        let Some((_, _, kind)) = tokens.first() else {
            continue;
        };
        let table = match kind.as_str() {
            "insert" | "update" | "delete" => db_sql_target_table(&tokens, kind),
            "create" | "alter" | "drop" => {
                let table_index = tokens
                    .iter()
                    .position(|(_, _, word)| word == "table")
                    .and_then(|index| index.checked_add(1));
                table_index.and_then(|mut index| {
                    if matches!(kind.as_str(), "create" | "drop") {
                        while matches!(
                            tokens.get(index).map(|(_, _, word)| word.as_str()),
                            Some("if" | "not" | "exists")
                        ) {
                            index = index.checked_add(1)?;
                        }
                    }
                    tokens.get(index).map(|(_, _, word)| word.clone())
                })
            }
            _ => None,
        };
        if let Some(table) = table {
            tables.insert(table);
        }
    }
    tables.into_iter().collect::<Vec<_>>().join(",")
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
        || count("from")
            > if matches!(kind, "select" | "delete") {
                1
            } else {
                0
            }
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
                    matches!(
                        word.as_str(),
                        "where" | "order" | "group" | "limit" | "offset" | "returning"
                    )
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
        columns.into_iter().map(str::to_ascii_lowercase).collect(),
        owner,
    ))
}

fn db_sql_clause_start(tokens: &[(usize, usize, String)], after: usize) -> Option<usize> {
    tokens
        .iter()
        .filter(|(_, _, word)| {
            matches!(
                word.as_str(),
                "order" | "group" | "limit" | "offset" | "returning"
            )
        })
        .find(|(start, _, _)| *start > after)
        .map(|(start, _, _)| *start)
}

fn db_sql_placeholder_count(sql: &str) -> usize {
    let bytes = sql.as_bytes();
    let mut count = 0;
    let mut i = 0;
    while i < bytes.len() {
        let quote = match bytes[i] {
            b'\'' | b'"' | b'`' => Some(bytes[i]),
            b'[' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b']' {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
                None
            }
            b'?' => {
                count += 1;
                i += 1;
                None
            }
            _ => {
                i += 1;
                None
            }
        };
        let Some(quote) = quote else {
            continue;
        };
        i += 1;
        while i < bytes.len() {
            if bytes[i] == quote {
                if bytes.get(i + 1) == Some(&quote) {
                    i += 2;
                } else {
                    i += 1;
                    break;
                }
            } else {
                i += 1;
            }
        }
    }
    count
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
            matches!(
                word.as_str(),
                "where" | "order" | "group" | "limit" | "offset" | "returning"
            )
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
        || normalized.starts_with("create table __jet_migrations")
        || normalized.starts_with("pragma table_info(__jet_migrations")
        || normalized.starts_with("select ")
            && normalized.contains(" from __jet_migrations")
        || normalized.starts_with("insert into __jet_migrations")
}

/// Apply the closed owner policy to one typed SQL operation. Unsupported SQL
/// is rejected, never passed through unscoped. The returned SQL still carries
/// the user as a bind, so the policy value cannot become SQL text.
pub fn jet_db_apply_policy(
    sql: &SQL,
    table: &str,
    expression: &str,
    user: &str,
) -> Result<SQL, DBError> {
    let (table, compiled) =
        jet_db_policy_compile(table, expression).map_err(|message| DBError { message })?;
    jet_db_apply_compiled_policy_with_proof(sql, &table, compiled, user)
        .and_then(JetDBPolicyApplication::into_sql)
}

/// Apply an already compiled policy. Scope carriers use this path so a later
/// operation cannot re-parse or replace the policy's source text.
pub fn jet_db_apply_compiled_policy(
    sql: &SQL,
    table: &str,
    compiled: JetRowPolicyExpr,
    user: &str,
) -> Result<SQL, DBError> {
    jet_db_apply_compiled_policy_with_proof(sql, table, compiled, user)
        .and_then(JetDBPolicyApplication::into_sql)
}

/// Apply a compiled policy and retain the enforcement proof until the caller
/// has selected its execution path. All scoped query, mutation, transaction,
/// and live adapters use this entry point.
pub fn jet_db_apply_compiled_policy_with_proof(
    sql: &SQL,
    table: &str,
    compiled: JetRowPolicyExpr,
    user: &str,
) -> Result<JetDBPolicyApplication, DBError> {
    jet_db_apply_policy_inner(sql, table, compiled, user, false)
}

/// Apply a policy while executing an explicit `db.migrate` step. Schema
/// statements are an authority of migration, but row statements still use the
/// same table/predicate/bind transformation as ordinary scope calls.
pub fn jet_db_apply_migration_policy(
    sql: &SQL,
    table: &str,
    expression: &str,
    user: &str,
) -> Result<SQL, DBError> {
    let (table, compiled) =
        jet_db_policy_compile(table, expression).map_err(|message| DBError { message })?;
    jet_db_apply_compiled_migration_policy_with_proof(sql, &table, compiled, user)
        .and_then(JetDBPolicyApplication::into_sql)
}

/// Migration form of [`jet_db_apply_compiled_policy`]. Schema statements
/// remain the only extra authority granted by migration.
pub fn jet_db_apply_compiled_migration_policy(
    sql: &SQL,
    table: &str,
    compiled: JetRowPolicyExpr,
    user: &str,
) -> Result<SQL, DBError> {
    jet_db_apply_compiled_migration_policy_with_proof(sql, table, compiled, user)
        .and_then(JetDBPolicyApplication::into_sql)
}

pub fn jet_db_apply_compiled_migration_policy_with_proof(
    sql: &SQL,
    table: &str,
    compiled: JetRowPolicyExpr,
    user: &str,
) -> Result<JetDBPolicyApplication, DBError> {
    jet_db_apply_policy_inner(sql, table, compiled, user, true)
}

fn jet_db_policy_application(
    sql: SQL,
    table: &str,
    compiled: JetRowPolicyExpr,
    operation: &str,
    user: &str,
    user_bind_index: Option<usize>,
) -> JetDBPolicyApplication {
    JetDBPolicyApplication {
        sql,
        proof: JetDBPolicyProof {
            table: table.to_string(),
            operation: operation.to_string(),
            predicate: compiled.sql_predicate().to_string(),
            user: user.to_string(),
            user_bind_index,
        },
    }
}

fn jet_db_apply_policy_inner(
    sql: &SQL,
    table: &str,
    compiled: JetRowPolicyExpr,
    user: &str,
    allow_schema: bool,
) -> Result<JetDBPolicyApplication, DBError> {
    let policy_table = table;
    let sql_text = &sql.0;
    let params = &sql.1;
    if sql_text.len() > 1024 * 1024
        || sql_text
            .chars()
            .any(|c| c == ';' || c == '\0' || (c.is_control() && !c.is_whitespace()))
    {
        return Err(DBError {
            message: "policy-scoped SQL must be one statement without control characters"
                .to_string(),
        });
    }
    if db_sql_contains_comment(sql_text) {
        return Err(DBError {
            message: "policy-scoped SQL cannot contain comments".to_string(),
        });
    }
    if user.trim().is_empty() || user.len() > 1024 * 1024 || user.chars().any(char::is_control) {
        return Err(DBError {
            message: "policy user identity is empty, too long, or contains control characters"
                .to_string(),
        });
    }
    let tokens = db_sql_tokens(sql_text);
    let Some((_, _, first)) = tokens.first() else {
        return Err(DBError {
            message: "policy-scoped SQL is empty".to_string(),
        });
    };
    if db_sql_is_migration_metadata(sql_text) {
        if !allow_schema {
            return Err(DBError {
                message: "migration metadata is managed only by db.migrate".to_string(),
            });
        }
        return Ok(jet_db_policy_application(
            sql.clone(),
            policy_table,
            compiled,
            "migration-metadata",
            user,
            None,
        ));
    }
    let kind = first.as_str();
    if !matches!(kind, "select" | "update" | "delete" | "insert") {
        if matches!(
            kind,
            "create" | "alter" | "drop" | "pragma" | "begin" | "commit" | "rollback"
        ) {
            if allow_schema {
                return Ok(jet_db_policy_application(
                    sql.clone(),
                    policy_table,
                    compiled,
                    "migration-schema",
                    user,
                    None,
                ));
            }
            return Err(DBError {
                message: "schema and transaction-control SQL is only allowed in db.migrate or through the scope transaction controls".to_string(),
            });
        }
        return Err(DBError {
            message: "row policy supports SELECT, INSERT, UPDATE, and DELETE only".to_string(),
        });
    }
    let expected_table = policy_table.to_ascii_lowercase();
    if db_sql_target_table(&tokens, kind).as_deref() != Some(expected_table.as_str()) {
        return Err(DBError {
            message: format!("policy scope targets table `{policy_table}`"),
        });
    }
    if !db_sql_simple_target(sql_text, &tokens, kind) {
        return Err(DBError {
            message:
                "policy-scoped SQL must name one simple target table without joins or subqueries"
                    .to_string(),
        });
    }
    if !compiled.requires_owner_filter() {
        return Ok(jet_db_policy_application(
            sql.clone(),
            policy_table,
            compiled,
            kind,
            user,
            None,
        ));
    }
    if kind == "insert" {
        let Some((columns, owner_index)) = db_sql_insert_columns_and_values(sql_text, &tokens)
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
        return Ok(jet_db_policy_application(
            (sql_text.to_string(), scoped_params),
            policy_table,
            compiled,
            kind,
            user,
            Some(owner_index),
        ));
    }
    if kind == "update" && db_sql_update_mutates_owner(&tokens) {
        return Err(DBError {
            message: "owner policy does not allow changing the owner column".to_string(),
        });
    }
    let where_token = tokens
        .iter()
        .find(|(_, _, word)| word == "where")
        .map(|(start, end, _)| (*start, *end));
    let insertion = where_token
        .and_then(|(_, end)| db_sql_clause_start(&tokens, end))
        .or_else(|| db_sql_clause_start(&tokens, 0))
        .unwrap_or_else(|| sql_text.len());
    let user_index = db_sql_placeholder_count(&sql_text[..insertion]);
    if user_index > params.len() {
        return Err(DBError {
            message: "policy-scoped SQL bind count does not match its placeholders".to_string(),
        });
    }
    let mut scoped_params = params.clone();
    scoped_params.insert(user_index, DBValue::Text(user.to_string()));
    let (head, tail) = sql_text.split_at(insertion);
    let suffix = if tail.trim().is_empty() {
        String::new()
    } else {
        format!(" {}", tail.trim_start())
    };
    let scoped_sql = if let Some((where_start, where_end)) = where_token {
        let condition = sql_text[where_end..insertion].trim();
        if condition.is_empty() {
            return Err(DBError {
                message: "policy-scoped SQL has an empty WHERE clause".to_string(),
            });
        }
        format!(
            "{}WHERE ({}) AND owner = ?{}",
            &sql_text[..where_start],
            condition,
            suffix,
        )
    } else {
        format!("{} WHERE owner = ?{}", head.trim_end(), suffix)
    };
    Ok(jet_db_policy_application(
        (scoped_sql, scoped_params),
        policy_table,
        compiled,
        kind,
        user,
        Some(user_index),
    ))
}

// ── core.db wire codec ──────────────────────────────────────────────────────
// The FFI bridge crate (built only when a program uses `core.db`, Source/FFI.rs)
// and this always-compiled prelude are two independently built Rust crates —
// they can't share types, so bind params and result rows cross that boundary as
// plain `String`s in a small tagged-length wire format (mirrored byte-for-byte
// in Source/Prelude/DB.rs). A value is `<tag><decimal-length>:<payload-bytes>`;
// a list is a decimal item count + `:` + that many back-to-back items. Every
// length is a byte count, so arbitrary text — including an "injection-looking"
// literal — round-trips exactly with no escaping. `Blob` payloads use an ASCII
// hexadecimal envelope so arbitrary bytes remain lossless across the String
// boundary.
// parity: guard tests/corelib.rs::core_db_implements_driver_trait
fn db_encode_tagged(tag: char, payload: &str) -> String {
    format!("{tag}{}:{payload}", payload.len())
}

fn db_hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
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
            DBValue::Blob(bytes) => db_encode_tagged('X', &db_hex_encode(bytes)),
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
        .ok_or_else(|| "database wire ended before a value tag".to_string())? as char;
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
fn db_runtime_number(payload: &str, what: &str) -> Result<u64, DBError> {
    payload.parse::<u64>().map_err(|_| DBError {
        message: format!("database runtime {what} is invalid"),
    })
}

fn db_runtime_bool(payload: &str, what: &str) -> Result<bool, DBError> {
    match payload {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(DBError {
            message: format!("database runtime {what} is invalid"),
        }),
    }
}

fn db_decode_plan(payload: &str) -> Result<Vec<JetDbQueryPlanFact>, DBError> {
    let bytes = payload.as_bytes();
    let Some(colon) = bytes.iter().position(|byte| *byte == b':') else {
        return Err(DBError {
            message: "database runtime plan is missing its row-count delimiter".to_string(),
        });
    };
    let count = std::str::from_utf8(&bytes[..colon])
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| DBError {
            message: "database runtime plan row count is invalid".to_string(),
        })?;
    if count > DB_MAX_PLAN_ROWS {
        return Err(DBError {
            message: "database runtime plan row count exceeds the limit".to_string(),
        });
    }
    let mut pos = colon + 1;
    let mut plan = Vec::with_capacity(count);
    for _ in 0..count {
        let mut ordinal = None;
        let mut parent = None;
        let mut operation = None;
        let mut relation = None;
        let mut index = None;
        for _ in 0..5 {
            let (tag, value) =
                db_read_tagged(bytes, &mut pos).map_err(|message| DBError { message })?;
            match tag {
                'I' if ordinal.is_none() => {
                    ordinal = Some(db_runtime_number(&value, "plan ordinal")?)
                }
                'P' if parent.is_none() => {
                    parent = Some(db_runtime_number(&value, "plan parent")?)
                }
                'O' if operation.is_none() && !value.is_empty() => operation = Some(value),
                'T' if relation.is_none() => relation = (!value.is_empty()).then_some(value),
                'X' if index.is_none() => index = (!value.is_empty()).then_some(value),
                _ => {
                    return Err(DBError {
                        message:
                            "database runtime plan has an invalid or duplicate field".to_string(),
                    })
                }
            }
        }
        plan.push(JetDbQueryPlanFact {
            ordinal: ordinal.ok_or_else(|| DBError {
                message: "database runtime plan has no ordinal".to_string(),
            })?,
            parent: parent.ok_or_else(|| DBError {
                message: "database runtime plan has no parent".to_string(),
            })?,
            operation: operation.ok_or_else(|| DBError {
                message: "database runtime plan has no operation".to_string(),
            })?,
            relation,
            index,
        });
    }
    if pos != bytes.len() {
        return Err(DBError {
            message: "database runtime plan has trailing bytes".to_string(),
        });
    }
    Ok(plan)
}

fn db_observed_error(wire: &str) -> Option<DBError> {
    wire.strip_prefix("E:").map(|message| DBError {
        message: message.to_string(),
    })
}

/// Decode the observed query envelope returned by a database driver.
pub fn jet_db_decode_observed_query_result(
    wire: &str,
) -> Result<(Vec<JetDBRow>, JetDbQueryRuntimeFacts), DBError> {
    if wire.len() > DB_MAX_WIRE_BYTES {
        return Err(DBError {
            message: "database observed query exceeds the wire-size limit".to_string(),
        });
    }
    if let Some(error) = db_observed_error(wire) {
        return Err(error);
    }
    let Some(body) = wire.strip_prefix("Q:") else {
        return Err(DBError {
            message: "database observed query has an unknown envelope".to_string(),
        });
    };
    let bytes = body.as_bytes();
    let mut pos = 0;
    let mut rows_wire = None;
    let mut row_count = None;
    let mut elapsed_ms = None;
    let mut plan_wire = None;
    while pos < bytes.len() {
        let (tag, payload) =
            db_read_tagged(bytes, &mut pos).map_err(|message| DBError { message })?;
        match tag {
            'R' if rows_wire.is_none() => rows_wire = Some(payload),
            'N' if row_count.is_none() => {
                row_count = Some(db_runtime_number(&payload, "row count")?)
            }
            'D' if elapsed_ms.is_none() => {
                elapsed_ms = Some(db_runtime_number(&payload, "elapsed time")?)
            }
            'P' if plan_wire.is_none() => plan_wire = Some(payload),
            _ => {
                return Err(DBError {
                    message:
                        "database observed query has an invalid or duplicate field".to_string(),
                })
            }
        }
    }
    let rows_wire = rows_wire.ok_or_else(|| DBError {
        message: "database observed query has no rows".to_string(),
    })?;
    let rows = jet_db_decode_query_result(&format!("O:{rows_wire}"))?;
    let actual_count = row_count.ok_or_else(|| DBError {
        message: "database observed query has no row count".to_string(),
    })?;
    if actual_count != rows.len() as u64 {
        return Err(DBError {
            message: "database observed query row count does not match its rows".to_string(),
        });
    }
    let elapsed_ms = elapsed_ms.ok_or_else(|| DBError {
        message: "database observed query has no elapsed time".to_string(),
    })?;
    let plan_wire = plan_wire.ok_or_else(|| DBError {
        message: "database observed query has no plan".to_string(),
    })?;
    Ok((
        rows,
        JetDbQueryRuntimeFacts {
            rows_returned: Some(actual_count),
            rows_affected: None,
            elapsed_ms,
            plan: db_decode_plan(&plan_wire)?,
        },
    ))
}

/// Decode the observed execute envelope returned by a database driver.
pub fn jet_db_decode_observed_execute_result(
    wire: &str,
) -> Result<(i64, JetDbQueryRuntimeFacts), DBError> {
    if wire.len() > DB_MAX_WIRE_BYTES {
        return Err(DBError {
            message: "database observed execute exceeds the wire-size limit".to_string(),
        });
    }
    if let Some(error) = db_observed_error(wire) {
        return Err(error);
    }
    let Some(body) = wire.strip_prefix("X:") else {
        return Err(DBError {
            message: "database observed execute has an unknown envelope".to_string(),
        });
    };
    let bytes = body.as_bytes();
    let mut pos = 0;
    let mut affected = None;
    let mut elapsed_ms = None;
    let mut plan_wire = None;
    while pos < bytes.len() {
        let (tag, payload) =
            db_read_tagged(bytes, &mut pos).map_err(|message| DBError { message })?;
        match tag {
            'A' if affected.is_none() => {
                affected = Some(payload.parse::<i64>().map_err(|_| DBError {
                    message: "database runtime affected row count is invalid".to_string(),
                })?);
            }
            'D' if elapsed_ms.is_none() => {
                elapsed_ms = Some(db_runtime_number(&payload, "elapsed time")?)
            }
            'P' if plan_wire.is_none() => plan_wire = Some(payload),
            _ => {
                return Err(DBError {
                    message:
                        "database observed execute has an invalid or duplicate field".to_string(),
                })
            }
        }
    }
    let affected = affected.ok_or_else(|| DBError {
        message: "database observed execute has no affected row count".to_string(),
    })?;
    let elapsed_ms = elapsed_ms.ok_or_else(|| DBError {
        message: "database observed execute has no elapsed time".to_string(),
    })?;
    let plan_wire = plan_wire.ok_or_else(|| DBError {
        message: "database observed execute has no plan".to_string(),
    })?;
    let affected = u64::try_from(affected).map_err(|_| DBError {
        message: "database runtime affected row count is negative".to_string(),
    })?;
    Ok((
        i64::try_from(affected).unwrap_or(i64::MAX),
        JetDbQueryRuntimeFacts {
            rows_returned: None,
            rows_affected: Some(affected),
            elapsed_ms,
            plan: db_decode_plan(&plan_wire)?,
        },
    ))
}
/// Decode the bounded EXPLAIN envelope returned by a database driver.
pub fn jet_db_decode_explain_result(
    wire: &str,
) -> Result<JetDbExplainRuntimeFacts, DBError> {
    if wire.len() > DB_MAX_WIRE_BYTES {
        return Err(DBError {
            message: "database EXPLAIN exceeds the wire-size limit".to_string(),
        });
    }
    if let Some(error) = db_observed_error(wire) {
        return Err(error);
    }
    let Some(body) = wire.strip_prefix("Y:") else {
        return Err(DBError {
            message: "database EXPLAIN has an unknown envelope".to_string(),
        });
    };
    let bytes = body.as_bytes();
    let mut pos = 0;
    let mut elapsed_ms = None;
    let mut timed_out = None;
    let mut truncated = None;
    let mut plan_wire = None;
    while pos < bytes.len() {
        let (tag, payload) =
            db_read_tagged(bytes, &mut pos).map_err(|message| DBError { message })?;
        match tag {
            'D' if elapsed_ms.is_none() => {
                elapsed_ms = Some(db_runtime_number(&payload, "elapsed time")?)
            }
            'T' if timed_out.is_none() => {
                timed_out = Some(db_runtime_bool(&payload, "timeout flag")?)
            }
            'L' if truncated.is_none() => {
                truncated = Some(db_runtime_bool(&payload, "truncation flag")?)
            }
            'P' if plan_wire.is_none() => plan_wire = Some(payload),
            _ => {
                return Err(DBError {
                    message: "database EXPLAIN has an invalid or duplicate field".to_string(),
                })
            }
        }
    }
    let elapsed_ms = elapsed_ms.ok_or_else(|| DBError {
        message: "database EXPLAIN has no elapsed time".to_string(),
    })?;
    let timed_out = timed_out.ok_or_else(|| DBError {
        message: "database EXPLAIN has no timeout flag".to_string(),
    })?;
    let truncated = truncated.ok_or_else(|| DBError {
        message: "database EXPLAIN has no truncation flag".to_string(),
    })?;
    let plan_wire = plan_wire.ok_or_else(|| DBError {
        message: "database EXPLAIN has no plan".to_string(),
    })?;
    Ok(JetDbExplainRuntimeFacts {
        elapsed_ms,
        plan: db_decode_plan(&plan_wire)?,
        timed_out,
        truncated,
    })
}


fn db_hex_decode(payload: &str) -> Result<Vec<u8>, String> {
    if payload.len() % 2 != 0 {
        return Err("database blob value has odd hexadecimal length".to_string());
    }
    let mut bytes = Vec::with_capacity(payload.len() / 2);
    for pair in payload.as_bytes().chunks_exact(2) {
        let high = (pair[0] as char)
            .to_digit(16)
            .ok_or_else(|| "database blob value is not hexadecimal".to_string())?;
        let low = (pair[1] as char)
            .to_digit(16)
            .ok_or_else(|| "database blob value is not hexadecimal".to_string())?;
        bytes.push(((high << 4) | low) as u8);
    }
    Ok(bytes)
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
        'X' => db_hex_decode(payload).map(DBValue::Blob),
        'N' => Err("database null value has a non-empty payload".to_string()),
        _ => Err("database wire contains an unknown value tag".to_string()),
    }
}

/// Decode the `"O:" + rows`/`"E:" + message` wire produced by `jet_db_query`.
pub fn jet_db_decode_query_result(wire: &str) -> Result<Vec<JetDBRow>, DBError> {
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
    let mut pos = colon + 1;
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
        let mut row = JetDBRow::new();
        for _ in 0..col_count {
            let (tag, name) =
                db_read_tagged(bytes, &mut pos).map_err(|message| DBError { message })?;
            if tag != 'C' {
                return Err(DBError {
                    message: "database row column has an invalid tag".to_string(),
                });
            }
            let (vtag, vpayload) =
                db_read_tagged(bytes, &mut pos).map_err(|message| DBError { message })?;
            let value = db_decode_value(vtag, &vpayload).map_err(|message| DBError { message })?;
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

/// Map a query result to the canonical `query_one` outcome. Adapters only
/// marshal this outcome; first-row and absent-row meaning lives here.
pub fn jet_db_first_row(rows: Vec<JetDBRow>) -> JetOutcome<JetDBRow, JetAbsent> {
    jet_outcome_of(rows.into_iter().next())
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

/// The migration ledger is a database-owned transition log.  These types are
/// deliberately independent of any CLI, filesystem, or tier adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetMigrationLock {
    Shared,
    Exclusive,
}

impl JetMigrationLock {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Exclusive => "exclusive",
        }
    }

    fn parse(value: &str) -> Result<Self, DBError> {
        match value {
            "shared" => Ok(Self::Shared),
            "exclusive" => Ok(Self::Exclusive),
            _ => Err(DBError {
                message: format!("unknown migration lock mode `{value}`"),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetMigrationOperation {
    Apply,
    Rollback,
    Direct,
}

impl JetMigrationOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::Rollback => "rollback",
            Self::Direct => "direct",
        }
    }

    fn parse(value: &str) -> Result<Self, DBError> {
        match value {
            "apply" => Ok(Self::Apply),
            "rollback" => Ok(Self::Rollback),
            "direct" => Ok(Self::Direct),
            _ => Err(DBError {
                message: format!("unknown migration operation `{value}`"),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetMigrationSql {
    pub ordinal: u64,
    pub sql: SQL,
    pub inverse_sql: Option<SQL>,
    pub risk: String,
    pub lock: JetMigrationLock,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetMigrationRequest {
    pub operation: JetMigrationOperation,
    pub migration_key: String,
    pub target: String,
    pub target_identity: String,
    pub database_identity: String,
    pub source_identity: String,
    pub schema_identity: String,
    pub tool_identity: String,
    pub from_version: u64,
    pub to_version: u64,
    pub lock: JetMigrationLock,
    pub risk: String,
    pub steps: Vec<JetMigrationSql>,
}

impl JetMigrationRequest {
    pub fn direct(name: String, steps: Vec<SQL>) -> Self {
        Self {
            operation: JetMigrationOperation::Direct,
            migration_key: name,
            target: "direct".to_string(),
            target_identity: "direct".to_string(),
            database_identity: "runtime".to_string(),
            source_identity: "jet.db.runtime".to_string(),
            schema_identity: "jet.db.migration/v2".to_string(),
            tool_identity: "jet-runtime".to_string(),
            from_version: 0,
            to_version: 0,
            lock: JetMigrationLock::Exclusive,
            risk: "runtime migration".to_string(),
            steps: steps
                .into_iter()
                .enumerate()
                .map(|(index, sql)| JetMigrationSql {
                    ordinal: index as u64 + 1,
                    sql,
                    inverse_sql: None,
                    risk: "runtime migration".to_string(),
                    lock: JetMigrationLock::Exclusive,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetMigrationOutcome {
    pub receipt_id: String,
    pub operation: String,
    pub status: String,
    pub target: String,
    pub target_identity: String,
    pub source_identity: String,
    pub schema_identity: String,
    pub database_identity: String,
    pub tool_identity: String,
    pub from_version: u64,
    pub to_version: u64,
    pub step_count: usize,
    pub step_ids: Vec<String>,
    pub checksum: String,
    pub lock: JetMigrationLock,
    pub risk: String,
    pub started_unix_ms: u128,
    pub finished_unix_ms: u128,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetMigrationStateRequest {
    pub target: String,
    pub target_identity: String,
    pub database_identity: String,
    pub source_identity: String,
    pub schema_identity: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetMigrationState {
    pub target: String,
    pub target_identity: String,
    pub database_identity: String,
    pub source_identity: String,
    pub schema_identity: String,
    pub current_version: u64,
    pub checksum: String,
    pub receipts: Vec<JetMigrationOutcome>,
    pub drift: Vec<String>,
}

fn db_hash_field(hash: &mut u64, value: &str) {
    for byte in (value.len() as u64).to_le_bytes() {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
    for byte in value.as_bytes() {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
    *hash ^= 0xff;
    *hash = hash.wrapping_mul(0x100000001b3);
}

fn db_hash_value(hash: &mut u64, value: &DBValue) {
    match value {
        DBValue::Null => db_hash_field(hash, "N"),
        DBValue::Int(value) => {
            db_hash_field(hash, "I");
            db_hash_field(hash, &value.to_string());
        }
        DBValue::Float(value) => {
            db_hash_field(hash, "F");
            db_hash_field(hash, &value.to_bits().to_string());
        }
        DBValue::Text(value) => {
            db_hash_field(hash, "T");
            db_hash_field(hash, value);
        }
        DBValue::Bool(value) => db_hash_field(hash, if *value { "B1" } else { "B0" }),
        DBValue::Blob(value) => {
            db_hash_field(hash, "X");
            for byte in value {
                *hash ^= u64::from(*byte);
                *hash = hash.wrapping_mul(0x100000001b3);
            }
            *hash ^= 0xff;
            *hash = hash.wrapping_mul(0x100000001b3);
        }
    }
}

fn db_hash_sql(hash: &mut u64, sql: &SQL) {
    db_hash_field(hash, &sql.0);
    db_hash_field(hash, &(sql.1.len() as u64).to_string());
    for value in &sql.1 {
        db_hash_value(hash, value);
    }
}

/// D-TYPEDSQL-SINK1=A: this checksum remains the compatibility identity for
/// the public `db.migrate(name, steps)` operation.  The typed CLI request
/// checksum below additionally covers target, source, lock, and version facts.
pub fn jet_db_migration_checksum(steps: &Vec<SQL>) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for sql in steps {
        for b in sql.0.as_bytes() {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xfe;
        hash = hash.wrapping_mul(0x100000001b3);
        for param in &sql.1 {
            let encoded = match param {
                DBValue::Null => "N".to_string(),
                DBValue::Int(value) => format!("I{value}"),
                DBValue::Float(value) => format!("F{value}"),
                DBValue::Text(value) => format!("T{value}"),
                DBValue::Bool(value) => format!("B{}", if *value { 1 } else { 0 }),
                DBValue::Blob(value) => format!("X{}", db_hex_encode(value)),
            };
            for b in encoded.as_bytes() {
                hash ^= *b as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
            hash ^= 0xfd;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn jet_db_migration_request_checksum(request: &JetMigrationRequest) -> String {
    if matches!(request.operation, JetMigrationOperation::Direct) {
        let steps = request
            .steps
            .iter()
            .map(|step| step.sql.clone())
            .collect::<Vec<_>>();
        return jet_db_migration_checksum(&steps);
    }
    let mut hash = 0xcbf29ce484222325;
    db_hash_field(&mut hash, "jet.db.migration/v2");
    db_hash_field(&mut hash, request.operation.as_str());
    db_hash_field(&mut hash, &request.migration_key);
    db_hash_field(&mut hash, &request.target);
    db_hash_field(&mut hash, &request.target_identity);
    db_hash_field(&mut hash, &request.database_identity);
    db_hash_field(&mut hash, &request.source_identity);
    db_hash_field(&mut hash, &request.schema_identity);
    db_hash_field(&mut hash, &request.tool_identity);
    db_hash_field(&mut hash, &request.from_version.to_string());
    db_hash_field(&mut hash, &request.to_version.to_string());
    db_hash_field(&mut hash, request.lock.as_str());
    db_hash_field(&mut hash, &request.risk);
    db_hash_field(&mut hash, &(request.steps.len() as u64).to_string());
    for step in &request.steps {
        db_hash_field(&mut hash, &step.ordinal.to_string());
        db_hash_field(&mut hash, step.lock.as_str());
        db_hash_field(&mut hash, &step.risk);
        db_hash_sql(&mut hash, &step.sql);
        if let Some(inverse) = &step.inverse_sql {
            db_hash_field(&mut hash, "inverse");
            db_hash_sql(&mut hash, inverse);
        } else {
            db_hash_field(&mut hash, "no-inverse");
        }
    }
    format!("migration-v2:{hash:016x}")
}

fn db_migration_step_id(step: &JetMigrationSql) -> String {
    let mut hash = 0xcbf29ce484222325;
    db_hash_field(&mut hash, &step.ordinal.to_string());
    db_hash_field(&mut hash, step.lock.as_str());
    db_hash_field(&mut hash, &step.risk);
    db_hash_sql(&mut hash, &step.sql);
    format!("step:{hash:016x}")
}

fn db_migration_transition_key(
    request: &JetMigrationRequest,
    checksum: &str,
    predecessor: Option<&str>,
) -> String {
    let mut hash = 0xcbf29ce484222325;
    db_hash_field(&mut hash, "transition");
    db_hash_field(&mut hash, request.operation.as_str());
    db_hash_field(&mut hash, &request.migration_key);
    db_hash_field(&mut hash, &request.target_identity);
    db_hash_field(&mut hash, checksum);
    db_hash_field(&mut hash, predecessor.unwrap_or(""));
    format!("transition:{hash:016x}")
}

fn db_migration_now_ms() -> u128 {
    u128::try_from(super::jet_std_time_now()).unwrap_or(0)
}

fn db_migration_validate_request(request: &JetMigrationRequest) -> Result<(), DBError> {
    for (name, value) in [
        ("migration key", request.migration_key.as_str()),
        ("target", request.target.as_str()),
        ("target identity", request.target_identity.as_str()),
        ("database identity", request.database_identity.as_str()),
        ("source identity", request.source_identity.as_str()),
        ("schema identity", request.schema_identity.as_str()),
        ("tool identity", request.tool_identity.as_str()),
        ("risk", request.risk.as_str()),
    ] {
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(DBError {
                message: format!("migration {name} is empty or contains control characters"),
            });
        }
    }
    if matches!(request.operation, JetMigrationOperation::Apply)
        && request.to_version <= request.from_version
    {
        return Err(DBError {
            message: "migration apply target version must be ahead of its source".to_string(),
        });
    }
    if matches!(request.operation, JetMigrationOperation::Rollback)
        && request.to_version >= request.from_version
    {
        return Err(DBError {
            message: "migration rollback target version must be below its source".to_string(),
        });
    }
    if request.steps.len() > i64::MAX as usize
        || request.from_version > i64::MAX as u64
        || request.to_version > i64::MAX as u64
    {
        return Err(DBError {
            message: "migration request exceeds the SQLite integer range".to_string(),
        });
    }
    let mut previous = 0;
    for step in &request.steps {
        if step.ordinal == 0 || step.ordinal <= previous {
            return Err(DBError {
                message: "migration step ordinals must be positive and increasing".to_string(),
            });
        }
        previous = step.ordinal;
        if step.sql.0.trim().is_empty()
            || step.sql.0.contains('\0')
            || step.sql.0.chars().any(char::is_control)
            || step.risk.is_empty()
            || step.risk.chars().any(char::is_control)
        {
            return Err(DBError {
                message: "migration step contains empty or invalid SQL metadata".to_string(),
            });
        }
    }
    Ok(())
}

const MIGRATION_LEDGER_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS __jet_migrations (sequence INTEGER PRIMARY KEY AUTOINCREMENT, transition_key TEXT NOT NULL UNIQUE, migration_key TEXT NOT NULL, target TEXT NOT NULL, target_identity TEXT NOT NULL, database_identity TEXT NOT NULL, source_identity TEXT NOT NULL, schema_identity TEXT NOT NULL, tool_identity TEXT NOT NULL, operation TEXT NOT NULL, status TEXT NOT NULL, from_version INTEGER NOT NULL, to_version INTEGER NOT NULL, step_count INTEGER NOT NULL, step_ids TEXT NOT NULL, checksum TEXT NOT NULL, lock_mode TEXT NOT NULL, risk TEXT NOT NULL, started_unix_ms INTEGER NOT NULL, finished_unix_ms INTEGER NOT NULL, error TEXT)";

fn db_migration_ensure_schema<B: JetDBBackend>(backend: &mut B) -> Result<(), DBError> {
    backend.execute(&(MIGRATION_LEDGER_SCHEMA.to_string(), Vec::new()), true)?;
    let columns = backend.query(
        &(
            "PRAGMA table_info(__jet_migrations)".to_string(),
            Vec::new(),
        ),
        true,
    )?;
    let canonical = [
        "sequence",
        "transition_key",
        "migration_key",
        "target",
        "target_identity",
        "database_identity",
        "source_identity",
        "schema_identity",
        "tool_identity",
        "operation",
        "status",
        "from_version",
        "to_version",
        "step_count",
        "step_ids",
        "checksum",
        "lock_mode",
        "risk",
        "started_unix_ms",
        "finished_unix_ms",
        "error",
    ];
    if columns.len() != canonical.len()
        || columns.iter().zip(canonical).any(|(row, expected)| {
            !row
                .get("name")
                .and_then(|value| value.text().ok())
                .is_some_and(|name| name == expected)
        })
    {
        return Err(DBError {
            message: "migration ledger uses a non-canonical schema".to_string(),
        });
    }
    Ok(())
}

struct JetMigrationLedgerRow {
    transition_key: String,
    migration_key: String,
    outcome: JetMigrationOutcome,
}

fn db_migration_row_text(row: &JetDBRow, key: &str) -> Result<String, DBError> {
    row.get(key)
        .ok_or_else(|| DBError {
            message: format!("migration ledger row has no `{key}` field"),
        })?
        .text()
        .map_err(|message| DBError { message })
}

fn db_migration_row_u64(row: &JetDBRow, key: &str) -> Result<u64, DBError> {
    let value = row
        .get(key)
        .ok_or_else(|| DBError {
            message: format!("migration ledger row has no `{key}` field"),
        })?
        .int()
        .map_err(|message| DBError { message })?;
    u64::try_from(value).map_err(|_| DBError {
        message: format!("migration ledger `{key}` is negative"),
    })
}

fn db_migration_row_optional_text(
    row: &JetDBRow,
    key: &str,
) -> Result<Option<String>, DBError> {
    let value = row.get(key).ok_or_else(|| DBError {
        message: format!("migration ledger row has no `{key}` field"),
    })?;
    if value.is_null() {
        Ok(None)
    } else {
        value
            .text()
            .map(Some)
            .map_err(|message| DBError { message })
    }
}

fn db_migration_parse_row(row: &JetDBRow) -> Result<JetMigrationLedgerRow, DBError> {
    let transition_key = db_migration_row_text(row, "transition_key")?;
    let migration_key = db_migration_row_text(row, "migration_key")?;
    let operation = db_migration_row_text(row, "operation")?;
    let status = db_migration_row_text(row, "status")?;
    let step_ids = db_migration_row_text(row, "step_ids")?;
    let step_ids = if step_ids.is_empty() {
        Vec::new()
    } else {
        step_ids.split('\n').map(str::to_string).collect()
    };
    let step_count = usize::try_from(db_migration_row_u64(row, "step_count")?).map_err(|_| DBError {
        message: "migration ledger step count is outside the host range".to_string(),
    })?;
    let started_unix_ms = u128::from(db_migration_row_u64(row, "started_unix_ms")?);
    let finished_unix_ms = u128::from(db_migration_row_u64(row, "finished_unix_ms")?);
    let lock = JetMigrationLock::parse(&db_migration_row_text(row, "lock_mode")?)?;
    let outcome = JetMigrationOutcome {
        receipt_id: transition_key.clone(),
        operation,
        status,
        target: db_migration_row_text(row, "target")?,
        target_identity: db_migration_row_text(row, "target_identity")?,
        source_identity: db_migration_row_text(row, "source_identity")?,
        schema_identity: db_migration_row_text(row, "schema_identity")?,
        database_identity: db_migration_row_text(row, "database_identity")?,
        tool_identity: db_migration_row_text(row, "tool_identity")?,
        from_version: db_migration_row_u64(row, "from_version")?,
        to_version: db_migration_row_u64(row, "to_version")?,
        step_count,
        step_ids,
        checksum: db_migration_row_text(row, "checksum")?,
        lock,
        risk: db_migration_row_text(row, "risk")?,
        started_unix_ms,
        finished_unix_ms,
        error: db_migration_row_optional_text(row, "error")?,
    };
    Ok(JetMigrationLedgerRow {
        transition_key,
        migration_key,
        outcome,
    })
}

fn db_migration_rows<B: JetDBBackend>(backend: &mut B) -> Result<Vec<JetMigrationLedgerRow>, DBError> {
    let rows = backend.query(
        &(
            "SELECT transition_key, migration_key, target, target_identity, database_identity, source_identity, schema_identity, tool_identity, operation, status, from_version, to_version, step_count, step_ids, checksum, lock_mode, risk, started_unix_ms, finished_unix_ms, error FROM __jet_migrations ORDER BY sequence"
                .to_string(),
            Vec::new(),
        ),
        true,
    )?;
    rows.iter().map(db_migration_parse_row).collect()
}

fn db_migration_state_from_rows(
    query: &JetMigrationStateRequest,
    rows: &[JetMigrationLedgerRow],
) -> Result<JetMigrationState, DBError> {
    let mut current_version = 0;
    let mut receipts = Vec::new();
    let mut drift = Vec::new();
    let mut hash = 0xcbf29ce484222325;
    db_hash_field(&mut hash, &query.target);
    db_hash_field(&mut hash, &query.target_identity);
    db_hash_field(&mut hash, &query.database_identity);
    db_hash_field(&mut hash, &query.source_identity);
    for row in rows {
        let receipt = &row.outcome;
        if receipt.target != query.target {
            continue;
        }
        receipts.push(receipt.clone());
        db_hash_field(&mut hash, &row.transition_key);
        db_hash_field(&mut hash, &receipt.checksum);
        if receipt.target_identity != query.target_identity {
            drift.push(format!(
                "migration transition `{}` has a foreign target identity",
                row.transition_key
            ));
            continue;
        }
        if receipt.database_identity != query.database_identity {
            drift.push(format!(
                "migration transition `{}` has a foreign database identity",
                row.transition_key
            ));
            continue;
        }
        if receipt.source_identity != query.source_identity {
            drift.push(format!(
                "migration transition `{}` was created from a different migration catalog",
                row.transition_key
            ));
            continue;
        }
        if receipt.schema_identity != query.schema_identity {
            drift.push(format!(
                "migration transition `{}` has a mismatched schema identity",
                row.transition_key
            ));
            continue;
        }
        if receipt.step_count != receipt.step_ids.len() {
            drift.push(format!(
                "migration transition `{}` has an invalid step count",
                row.transition_key
            ));
            continue;
        }
        if receipt.status != "applied" {
            drift.push(format!(
                "migration transition `{}` has unresolved status `{}`",
                row.transition_key, receipt.status
            ));
            continue;
        }
        match JetMigrationOperation::parse(&receipt.operation) {
            Ok(JetMigrationOperation::Direct) => {}
            Ok(JetMigrationOperation::Apply | JetMigrationOperation::Rollback) => {
                if receipt.from_version != current_version {
                    drift.push(format!(
                        "migration transition `{}` starts at {}, but canonical state is at {}",
                        row.transition_key, receipt.from_version, current_version
                    ));
                    continue;
                }
                current_version = receipt.to_version;
            }
            Err(_) => drift.push(format!(
                "migration transition `{}` has unknown operation `{}`",
                row.transition_key, receipt.operation
            )),
        }
    }
    Ok(JetMigrationState {
        target: query.target.clone(),
        target_identity: query.target_identity.clone(),
        database_identity: query.database_identity.clone(),
        source_identity: query.source_identity.clone(),
        schema_identity: query.schema_identity.clone(),
        current_version,
        checksum: format!("migration-state:{hash:016x}"),
        receipts,
        drift,
    })
}

fn db_migration_error(message: impl Into<String>) -> DBError {
    DBError {
        message: message.into(),
    }
}

fn db_migration_insert_values(outcome: &JetMigrationOutcome, migration_key: &str) -> SQL {
    let step_ids = outcome.step_ids.join("\n");
    (
        "INSERT INTO __jet_migrations (transition_key, migration_key, target, target_identity, database_identity, source_identity, schema_identity, tool_identity, operation, status, from_version, to_version, step_count, step_ids, checksum, lock_mode, risk, started_unix_ms, finished_unix_ms, error) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            .to_string(),
        vec![
            DBValue::Text(outcome.receipt_id.clone()),
            DBValue::Text(migration_key.to_string()),
            DBValue::Text(outcome.target.clone()),
            DBValue::Text(outcome.target_identity.clone()),
            DBValue::Text(outcome.database_identity.clone()),
            DBValue::Text(outcome.source_identity.clone()),
            DBValue::Text(outcome.schema_identity.clone()),
            DBValue::Text(outcome.tool_identity.clone()),
            DBValue::Text(outcome.operation.clone()),
            DBValue::Text(outcome.status.clone()),
            DBValue::Int(outcome.from_version as i64),
            DBValue::Int(outcome.to_version as i64),
            DBValue::Int(outcome.step_count as i64),
            DBValue::Text(step_ids),
            DBValue::Text(outcome.checksum.clone()),
            DBValue::Text(outcome.lock.as_str().to_string()),
            DBValue::Text(outcome.risk.clone()),
            DBValue::Int(outcome.started_unix_ms as i64),
            DBValue::Int(outcome.finished_unix_ms as i64),
            outcome
                .error
                .as_ref()
                .map(|value| DBValue::Text(value.clone()))
                .unwrap_or(DBValue::Null),
        ],
    )
}

pub fn jet_db_migration_state<B: JetDBBackend>(
    backend: &mut B,
    query: &JetMigrationStateRequest,
) -> Result<JetMigrationState, DBError> {
    if query.target.is_empty()
        || query.target_identity.is_empty()
        || query.database_identity.is_empty()
        || query.source_identity.is_empty()
        || query.schema_identity.is_empty()
    {
        return Err(db_migration_error("migration state query has incomplete identity"));
    }
    // Schema bootstrap/upgrade writes the ledger, so acquire SQLite's
    // authoritative write lock even for the read-facing state operation.
    if !backend.begin_with_lock(JetMigrationLock::Exclusive) {
        return Err(db_migration_error("could not begin migration state read"));
    }
    if let Err(error) = db_migration_ensure_schema(backend) {
        backend.rollback();
        return Err(error);
    }
    let rows = match db_migration_rows(backend) {
        Ok(rows) => rows,
        Err(error) => {
            backend.rollback();
            return Err(error);
        }
    };
    let state = match db_migration_state_from_rows(query, &rows) {
        Ok(state) => state,
        Err(error) => {
            backend.rollback();
            return Err(error);
        }
    };
    if backend.commit() {
        Ok(state)
    } else {
        backend.rollback();
        Err(db_migration_error("could not commit migration state read"))
    }
}

pub fn jet_db_migration_request<B: JetDBBackend>(
    backend: &mut B,
    request: &JetMigrationRequest,
) -> Result<JetMigrationOutcome, DBError> {
    db_migration_validate_request(request)?;
    if !backend.begin_with_lock(request.lock) {
        return Err(db_migration_error(format!(
            "could not begin migration `{}`",
            request.migration_key
        )));
    }
    if let Err(error) = db_migration_ensure_schema(backend) {
        backend.rollback();
        return Err(error);
    }
    let rows = match db_migration_rows(backend) {
        Ok(rows) => rows,
        Err(error) => {
            backend.rollback();
            return Err(error);
        }
    };
    let query = JetMigrationStateRequest {
        target: request.target.clone(),
        target_identity: request.target_identity.clone(),
        database_identity: request.database_identity.clone(),
        source_identity: request.source_identity.clone(),
        schema_identity: request.schema_identity.clone(),
    };
    let state = match db_migration_state_from_rows(&query, &rows) {
        Ok(state) => state,
        Err(error) => {
            backend.rollback();
            return Err(error);
        }
    };
    if state.drift.len() > 0 {
        backend.rollback();
        return Err(db_migration_error(format!(
            "migration target `{}` has unreconciled state: {}",
            request.target,
            state.drift.join("; ")
        )));
    }
    let checksum = jet_db_migration_request_checksum(request);
    let latest = rows
        .iter()
        .filter(|row| row.migration_key == request.migration_key && row.outcome.target == request.target)
        .next_back();
    if let Some(previous) = latest {
        if previous.outcome.status == "applied"
            && previous.outcome.operation == request.operation.as_str()
        {
            if previous.outcome.checksum == checksum {
                if backend.commit() {
                    let mut outcome = previous.outcome.clone();
                    // Steps applied *this call*. Idempotent replay applies none.
                    outcome.step_count = 0;
                    outcome.step_ids.clear();
                    return Ok(outcome);
                }
                backend.rollback();
                return Err(db_migration_error("could not commit idempotent migration read"));
            }
            backend.rollback();
            return Err(db_migration_error(format!(
                "migration `{}` checksum changed",
                request.migration_key
            )));
        }
    }
    if !matches!(request.operation, JetMigrationOperation::Direct)
        && request.from_version != state.current_version
    {
        backend.rollback();
        return Err(db_migration_error(format!(
            "migration target `{}` is at version {}, not {}",
            request.target, state.current_version, request.from_version
        )));
    }
    let predecessor = latest.map(|row| row.transition_key.as_str());
    let transition_key = db_migration_transition_key(request, &checksum, predecessor);
    let started = db_migration_now_ms();
    for step in &request.steps {
        if let Err(error) = backend.execute(&step.sql, true) {
            backend.rollback();
            return Err(error);
        }
    }
    let finished = db_migration_now_ms();
    let outcome = JetMigrationOutcome {
        receipt_id: transition_key,
        operation: request.operation.as_str().to_string(),
        status: "applied".to_string(),
        target: request.target.clone(),
        target_identity: request.target_identity.clone(),
        source_identity: request.source_identity.clone(),
        schema_identity: request.schema_identity.clone(),
        database_identity: request.database_identity.clone(),
        tool_identity: request.tool_identity.clone(),
        from_version: request.from_version,
        to_version: request.to_version,
        step_count: request.steps.len(),
        step_ids: request.steps.iter().map(db_migration_step_id).collect(),
        checksum,
        lock: request.lock,
        risk: request.risk.clone(),
        started_unix_ms: started,
        finished_unix_ms: finished,
        error: None,
    };
    if let Err(error) = backend.execute(
        &db_migration_insert_values(&outcome, &request.migration_key),
        true,
    ) {
        backend.rollback();
        return Err(error);
    }
    if backend.commit() {
        Ok(outcome)
    } else {
        backend.rollback();
        Err(db_migration_error(format!(
            "could not commit migration `{}`",
            request.migration_key
        )))
    }
}

/// One transaction state machine for ordinary `db.transaction` calls.
pub trait JetDBBackend {
    fn begin(&mut self) -> bool;
    fn begin_with_lock(&mut self, _lock: JetMigrationLock) -> bool {
        self.begin()
    }
    fn commit(&mut self) -> bool;
    fn rollback(&mut self);
    fn execute(&mut self, sql: &SQL, allow_schema: bool) -> Result<i64, DBError>;
    fn query(
        &mut self,
        sql: &SQL,
        allow_schema: bool,
    ) -> Result<Vec<JetDBRow>, DBError>;
}

pub fn jet_db_transaction<B: JetDBBackend>(
    backend: &mut B,
    label: &String,
    steps: &Vec<SQL>,
) -> Result<i64, DBError> {
    if !backend.begin() {
        return Err(DBError {
            message: format!("could not begin transaction: {label}"),
        });
    }
    let mut done = 0;
    for sql in steps {
        match backend.execute(sql, false) {
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
        backend.rollback();
        Err(DBError {
            message: "could not commit transaction".to_string(),
        })
    }
}

pub fn jet_db_migrate<B: JetDBBackend>(
    backend: &mut B,
    name: &String,
    steps: &Vec<SQL>,
) -> Result<i64, DBError> {
    let request = JetMigrationRequest::direct(name.clone(), steps.clone());
    jet_db_migration_request(backend, &request).map(|outcome| outcome.step_count as i64)
}

