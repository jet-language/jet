//! `core.db` hosts (#729). `include!` canonical SQLite runtime + wire codec —
//! no third algorithm.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use super::Concurrency;
use crate::Marshal::{clone_string, result_err_msg, result_ok};
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

trait JetShow {
    fn jet_show(&self) -> String;
}

trait JetDebug {
    fn jet_debug(&self) -> String;
}

/// D-FAIL-CONV2=A: included error fragments render failure text through this seam.
trait JetDisplay {
    fn jet_display(&self) -> String;
}

// The shared DB wire fragment receives the host's row carrier through this
// name. AOT supplies its JetMap; JIT keeps rows in the native map until heap
// marshalling.
type JetMap<K, V> = std::collections::BTreeMap<K, V>;

/// Canonical `core.db` FFI runtime (rusqlite).
mod runtime {
    include!("../../jet-pkg-model/src/Prelude/DB.rs");
}

/// Canonical wire encode/decode (`jet_std` DBPluginWire fragment) plus the one
/// closed row-policy language (`RowPolicy.rs`) it compiles through — the same
/// two fragments AOT splices into `mod jet_std` (I9).
mod wire {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/RowPolicy.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/DBPluginWire.rs");
}

// DBValue heap record ABI (same 2-slot shape as DataTree): [disc:i64, payload].
const DV_NULL: i64 = 0;
const DV_INT: i64 = 1;
const DV_FLOAT: i64 = 2;
const DV_TEXT: i64 = 3;
const DV_BOOL: i64 = 4;

thread_local! {
    /// JIT-local policy capabilities. The token is passed through Cranelift
    /// as the DBScope value; the policy and user never become mutable heap
    /// fields visible to Jet code.
    static DB_SCOPES: std::cell::RefCell<HashMap<u64, (u64, String, wire::JetRowPolicyExpr, String)>> =
        std::cell::RefCell::new(HashMap::new());
}

static NEXT_DB_SCOPE: AtomicU64 = AtomicU64::new(1_000_000_000);

fn scope_parts(handle: u64) -> Option<(u64, String, wire::JetRowPolicyExpr, String)> {
    DB_SCOPES.with(|scopes| scopes.borrow().get(&handle).cloned())
}

fn base_handle(handle: u64) -> u64 {
    scope_parts(handle)
        .map(|(base, _, _, _)| base)
        .unwrap_or(handle)
}

fn alloc_policy_record(table: &str, compiled: wire::JetRowPolicyExpr) -> i64 {
    let table_id = table.to_string();
    let expression_id = compiled.canonical().to_string();
    let compiled_id = match compiled {
        wire::JetRowPolicyExpr::AllowAll => 0,
        wire::JetRowPolicyExpr::OwnerEqualsUser => 1,
    };
    Concurrency::with_runtime_mut(|rt| {
        let table_id = rt.heap.alloc_string(table_id);
        let expression_id = rt.heap.alloc_string(expression_id);
        let record = rt.heap.alloc_record(3);
        let _ = rt.heap.record_set_int(record, 0, table_id);
        let _ = rt.heap.record_set_int(record, 1, expression_id);
        let _ = rt.heap.record_set_int(record, 2, compiled_id);
        record
    })
}

fn policy_record_parts(policy: i64) -> Option<(String, wire::JetRowPolicyExpr)> {
    Concurrency::with_runtime_mut(|rt| {
        let table = rt
            .heap
            .record_get_int(policy, 0)
            .and_then(|id| rt.heap.clone_string(id))?;
        let compiled = match rt.heap.record_get_int(policy, 2)? {
            0 => wire::JetRowPolicyExpr::AllowAll,
            1 => wire::JetRowPolicyExpr::OwnerEqualsUser,
            _ => return None,
        };
        Some((table, compiled))
    })
}

fn new_scope(
    connection: u64,
    table: String,
    compiled: wire::JetRowPolicyExpr,
    user: String,
) -> i64 {
    if connection == 0 {
        return 0;
    }
    let id = NEXT_DB_SCOPE.fetch_add(1, Ordering::Relaxed);
    let base = base_handle(connection);
    DB_SCOPES.with(|scopes| {
        scopes
            .borrow_mut()
            .insert(id, (base, table, compiled, user));
    });
    id as i64
}

fn alloc_dbvalue_record(disc: i64, payload: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(h, 0, disc);
        let _ = rt.heap.record_set_int(h, 1, payload);
        h
    })
}

fn alloc_dbvalue_float(f: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(h, 0, DV_FLOAT);
        let _ = rt.heap.record_set_float(h, 1, f);
        h
    })
}

fn read_dbvalue(handle: i64) -> Option<wire::DBValue> {
    Concurrency::with_runtime_mut(|rt| {
        let disc = rt.heap.record_get_int(handle, 0)?;
        match disc {
            DV_NULL => Some(wire::DBValue::Null),
            DV_INT => Some(wire::DBValue::Int(
                rt.heap.record_get_int(handle, 1).unwrap_or(0),
            )),
            DV_FLOAT => Some(wire::DBValue::Float(
                rt.heap.record_get_float(handle, 1).unwrap_or(0.0),
            )),
            DV_TEXT => {
                let sid = rt.heap.record_get_int(handle, 1).unwrap_or(0);
                Some(wire::DBValue::Text(
                    rt.heap.clone_string(sid).unwrap_or_default(),
                ))
            }
            DV_BOOL => Some(wire::DBValue::Bool(
                rt.heap.record_get_int(handle, 1).unwrap_or(0) != 0,
            )),
            _ => None,
        }
    })
}

fn value_handles_from_list(list: i64) -> Option<Vec<i64>> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list)?;
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(list, i)?);
        }
        Some(out)
    })
}

pub(crate) fn values_from_list_checked(list: i64) -> Option<Vec<wire::DBValue>> {
    value_handles_from_list(list)?
        .into_iter()
        .map(read_dbvalue)
        .collect()
}

pub(crate) fn values_from_list(list: i64) -> Vec<wire::DBValue> {
    values_from_list_checked(list).unwrap_or_default()
}

pub(crate) fn alloc_dbvalue_list(values: Vec<wire::DBValue>) -> i64 {
    let handles = values
        .into_iter()
        .map(|value| match value {
            wire::DBValue::Null => alloc_dbvalue_record(DV_NULL, 0),
            wire::DBValue::Int(value) => alloc_dbvalue_record(DV_INT, value),
            wire::DBValue::Float(value) => alloc_dbvalue_float(value),
            wire::DBValue::Text(value) => {
                let text = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value));
                alloc_dbvalue_record(DV_TEXT, text)
            }
            wire::DBValue::Bool(value) => alloc_dbvalue_record(DV_BOOL, i64::from(value)),
        })
        .collect::<Vec<_>>();
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for handle in handles {
            let _ = rt.heap.list_push_int(list, handle);
        }
        list
    })
}

pub(crate) fn alloc_sql_value(value: wire::SQL) -> i64 {
    let (template, params) = value;
    let template = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(template));
    let params_list = alloc_dbvalue_list(params);
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(record, 0, template);
        let _ = rt.heap.record_set_int(record, 1, params_list);
        record
    })
}

pub(crate) fn clone_sql_value(value: i64) -> Option<wire::SQL> {
    let (template, params_list) = Concurrency::with_runtime_mut(|rt| {
        let template = rt
            .heap
            .record_get_int(value, 0)
            .and_then(|id| rt.heap.clone_string(id))?;
        let params_list = rt.heap.record_get_int(value, 1)?;
        Some((template, params_list))
    })?;
    Some((template, values_from_list_checked(params_list)?))
}

fn jet_jit_db_policy(table: i64, expression: i64) -> i64 {
    let table = clone_string(table);
    let expression = clone_string(expression);
    match wire::jet_db_policy_compile(&table, &expression) {
        Ok((table, compiled)) => result_ok(alloc_policy_record(&table, compiled) as u64),
        Err(message) => result_err_msg(&message),
    }
}

fn jet_jit_db_with_policy(connection: i64, policy: i64, user: i64) -> i64 {
    let Some((table, compiled)) = policy_record_parts(policy) else {
        return 0;
    };
    let scope = new_scope(connection as u64, table, compiled, clone_string(user));
    scope
}

fn rows_to_list_of_maps(rows: Vec<wire::JetDBRow>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for row in rows {
            let map = rt.heap.alloc_empty_map();
            for (k, v) in row {
                let kid = rt.heap.alloc_string(k);
                let vh = match &v {
                    wire::DBValue::Null => {
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, DV_NULL);
                        let _ = rt.heap.record_set_int(h, 1, 0);
                        h
                    }
                    wire::DBValue::Int(n) => {
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, DV_INT);
                        let _ = rt.heap.record_set_int(h, 1, *n);
                        h
                    }
                    wire::DBValue::Float(f) => {
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, DV_FLOAT);
                        let _ = rt.heap.record_set_float(h, 1, *f);
                        h
                    }
                    wire::DBValue::Text(s) => {
                        let sid = rt.heap.alloc_string(s.clone());
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, DV_TEXT);
                        let _ = rt.heap.record_set_int(h, 1, sid);
                        h
                    }
                    wire::DBValue::Bool(b) => {
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, DV_BOOL);
                        let _ = rt.heap.record_set_int(h, 1, i64::from(*b));
                        h
                    }
                };
                let _ = rt.heap.map_insert(map, kid, vh);
            }
            let _ = rt.heap.list_push_int(list, map);
        }
        list
    })
}

fn jet_jit_db_open_memory() -> i64 {
    runtime::jet_db_open_memory() as i64
}

fn jet_jit_db_open(path: i64) -> i64 {
    runtime::jet_db_open(&clone_string(path)) as i64
}

fn jet_jit_db_close(handle: i64) -> i8 {
    let handle = handle as u64;
    if scope_parts(handle).is_some() {
        let base = base_handle(handle);
        DB_SCOPES.with(|scopes| {
            scopes.borrow_mut().remove(&handle);
        });
        i8::from(runtime::jet_db_close(base))
    } else {
        i8::from(runtime::jet_db_close(handle))
    }
}

fn jet_jit_db_begin(handle: i64) -> i8 {
    i8::from(runtime::jet_db_begin(base_handle(handle as u64)))
}

fn jet_jit_db_commit(handle: i64) -> i8 {
    i8::from(runtime::jet_db_commit(base_handle(handle as u64)))
}

fn jet_jit_db_rollback(handle: i64) -> i8 {
    i8::from(runtime::jet_db_rollback(base_handle(handle as u64)))
}

fn jet_jit_db_execute(handle: i64, sql: i64) -> i64 {
    let Some(sql) = clone_sql_value(sql) else {
        return result_err_msg("malformed SQL value");
    };
    match scoped_execute(handle as u64, &sql, false) {
        Ok(n) => result_ok(n as u64),
        Err(error) => result_err_msg(&error.message),
    }
}

fn jet_jit_db_query(handle: i64, sql: i64) -> i64 {
    let Some(sql) = clone_sql_value(sql) else {
        return result_err_msg("malformed SQL value");
    };
    match scoped_query(handle as u64, &sql, false) {
        Ok(rows) => result_ok(rows_to_list_of_maps(rows) as u64),
        Err(error) => result_err_msg(&error.message),
    }
}

fn jet_jit_db_query_one(handle: i64, sql: i64) -> i64 {
    let Some(sql) = clone_sql_value(sql) else {
        return result_err_msg("malformed SQL value");
    };
    match scoped_query(handle as u64, &sql, false).map(wire::jet_db_first_row) {
        Ok(Ok(row)) => {
            let list = rows_to_list_of_maps(vec![row]);
            let map =
                Concurrency::with_runtime_mut(|rt| rt.heap.list_get_int(list, 0).unwrap_or(0));
            result_ok(map.wrapping_add(1) as u64)
        }
        Ok(Err(_)) => result_ok(0),
        Err(error) => result_err_msg(&error.message),
    }
}

fn list_of_sql(list: i64) -> Option<Vec<wire::SQL>> {
    value_handles_from_list(list)?
        .into_iter()
        .map(clone_sql_value)
        .collect()
}

fn scoped_execute(
    scope: u64,
    sql: &wire::SQL,
    allow_schema: bool,
) -> Result<i64, wire::DBError> {
    let Some((base, table, compiled, user)) = scope_parts(scope) else {
        return Err(wire::DBError {
            message: "database row operations require a policy scope".to_string(),
        });
    };
    let sql = if allow_schema {
        wire::jet_db_apply_compiled_migration_policy_with_proof(sql, &table, compiled, &user)?
            .into_sql()?
    } else {
        wire::jet_db_apply_compiled_policy_with_proof(sql, &table, compiled, &user)?.into_sql()?
    };
    let result = runtime::jet_db_execute(
        base,
        &sql.0,
        &wire::jet_db_encode_params(&sql.1),
    );
    wire::jet_db_decode_execute_result(&result)
}

fn scoped_query(
    scope: u64,
    sql: &wire::SQL,
    allow_schema: bool,
) -> Result<Vec<wire::JetDBRow>, wire::DBError> {
    let Some((base, table, compiled, user)) = scope_parts(scope) else {
        return Err(wire::DBError {
            message: "database row operations require a policy scope".to_string(),
        });
    };
    let sql = if allow_schema {
        wire::jet_db_apply_compiled_migration_policy_with_proof(sql, &table, compiled, &user)?
            .into_sql()?
    } else {
        wire::jet_db_apply_compiled_policy_with_proof(sql, &table, compiled, &user)?.into_sql()?
    };
    let result = runtime::jet_db_query(
        base,
        &sql.0,
        &wire::jet_db_encode_params(&sql.1),
    );
    wire::jet_db_decode_query_result(&result)
}

struct JitDbBackend {
    scope: u64,
}

impl wire::JetDBBackend for JitDbBackend {
    fn begin(&mut self) -> bool {
        runtime::jet_db_begin(base_handle(self.scope))
    }

    fn commit(&mut self) -> bool {
        runtime::jet_db_commit(base_handle(self.scope))
    }

    fn rollback(&mut self) {
        let _ = runtime::jet_db_rollback(base_handle(self.scope));
    }

    fn execute(
        &mut self,
        sql: &wire::SQL,
        allow_schema: bool,
    ) -> Result<i64, wire::DBError> {
        scoped_execute(self.scope, sql, allow_schema)
    }

    fn query(
        &mut self,
        sql: &wire::SQL,
        allow_schema: bool,
    ) -> Result<Vec<wire::JetDBRow>, wire::DBError> {
        scoped_query(self.scope, sql, allow_schema)
    }
}

fn jet_jit_db_migrate(conn: i64, name: i64, steps: i64) -> i64 {
    let scope = conn as u64;
    if scope_parts(scope).is_none() {
        return result_err_msg("database migration requires a policy scope");
    }
    let name_s = clone_string(name);
    let Some(steps_v) = list_of_sql(steps) else {
        return result_err_msg("database migration steps must be SQL values");
    };
    let mut backend = JitDbBackend { scope };
    match wire::jet_db_migrate(&mut backend, &name_s, &steps_v) {
        Ok(done) => result_ok(done as u64),
        Err(error) => result_err_msg(&error.message),
    }
}

fn jet_jit_db_transaction(conn: i64, label: i64, steps: i64) -> i64 {
    let scope = conn as u64;
    if scope_parts(scope).is_none() {
        return result_err_msg("database transaction requires a policy scope");
    }
    let label_s = clone_string(label);
    let Some(steps_v) = list_of_sql(steps) else {
        return result_err_msg("database transaction steps must be SQL values");
    };
    let mut backend = JitDbBackend { scope };
    match wire::jet_db_transaction(&mut backend, &label_s, &steps_v) {
        Ok(done) => result_ok(done as u64),
        Err(error) => result_err_msg(&error.message),
    }
}

fn jet_jit_db_row_int(row: i64, key: i64) -> i64 {
    let key_s = clone_string(key);
    let val = Concurrency::with_runtime_mut(|rt| {
        let kid = rt.heap.alloc_string(key_s.clone());
        rt.heap.map_get(row, kid)
    });
    match val.and_then(read_dbvalue) {
        Some(v) => match v.int() {
            Ok(n) => result_ok(n as u64),
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg(&format!("missing column `{key_s}`")),
    }
}

fn jet_jit_db_row_text(row: i64, key: i64) -> i64 {
    let key_s = clone_string(key);
    let val = Concurrency::with_runtime_mut(|rt| {
        let kid = rt.heap.alloc_string(key_s.clone());
        rt.heap.map_get(row, kid)
    });
    match val.and_then(read_dbvalue) {
        Some(v) => match v.text() {
            Ok(s) => {
                let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
                result_ok(sid as u64)
            }
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg(&format!("missing column `{key_s}`")),
    }
}

fn jet_jit_dbvalue_pack(disc: i64, payload: i64) -> i64 {
    if disc == DV_FLOAT {
        alloc_dbvalue_float(f64::from_bits(payload as u64))
    } else {
        alloc_dbvalue_record(disc, payload)
    }
}

fn jet_jit_dbvalue_int(handle: i64) -> i64 {
    match read_dbvalue(handle) {
        Some(v) => match v.int() {
            Ok(n) => result_ok(n as u64),
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DBValue"),
    }
}

fn jet_jit_dbvalue_float(handle: i64) -> i64 {
    match read_dbvalue(handle) {
        Some(v) => match v.float() {
            Ok(f) => result_ok(f.to_bits()),
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DBValue"),
    }
}

fn jet_jit_dbvalue_text(handle: i64) -> i64 {
    match read_dbvalue(handle) {
        Some(v) => match v.text() {
            Ok(s) => {
                let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
                result_ok(sid as u64)
            }
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DBValue"),
    }
}

fn jet_jit_dbvalue_bool(handle: i64) -> i64 {
    match read_dbvalue(handle) {
        Some(v) => match v.bool() {
            Ok(b) => result_ok(u64::from(b)),
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DBValue"),
    }
}

fn jet_jit_dbvalue_is_null(handle: i64) -> i8 {
    match read_dbvalue(handle) {
        Some(v) => i8::from(v.is_null()),
        None => 0,
    }
}

/// Open in-memory SQLite (interpreter ambient host).
pub(crate) fn runtime_open_memory() -> u64 {
    runtime::jet_db_open_memory()
}

/// Open SQLite file (interpreter ambient host).
pub(crate) fn runtime_open(path: &str) -> u64 {
    runtime::jet_db_open(path)
}

pub(crate) fn runtime_close(handle: u64) -> bool {
    runtime::jet_db_close(handle)
}

pub(crate) fn runtime_begin(handle: u64) -> bool {
    runtime::jet_db_begin(handle)
}

pub(crate) fn runtime_commit(handle: u64) -> bool {
    runtime::jet_db_commit(handle)
}

pub(crate) fn runtime_rollback(handle: u64) -> bool {
    runtime::jet_db_rollback(handle)
}

pub(crate) fn runtime_execute(handle: u64, sql: &str, params_wire: &str) -> String {
    runtime::jet_db_execute(handle, sql, params_wire)
}

pub(crate) fn runtime_query(handle: u64, sql: &str, params_wire: &str) -> String {
    runtime::jet_db_query(handle, sql, params_wire)
}

host_fns! {
    struct DBHostFns;
    register: register_db_symbols;
    declare: declare_db_host_fns(module) {
        let cc = module.target_config().default_call_conv;
        let mut nullary = Signature::new(cc);
        nullary.returns.push(AbiParam::new(types::I64));
        let mut unary = Signature::new(cc);
        unary.params.push(AbiParam::new(types::I64));
        unary.returns.push(AbiParam::new(types::I64));
        let mut unary_i8 = Signature::new(cc);
        unary_i8.params.push(AbiParam::new(types::I64));
        unary_i8.returns.push(AbiParam::new(types::I8));
        let mut ternary = Signature::new(cc);
        for _ in 0..3 {
            ternary.params.push(AbiParam::new(types::I64));
        }
        ternary.returns.push(AbiParam::new(types::I64));
        let mut binary = Signature::new(cc);
        binary.params.push(AbiParam::new(types::I64));
        binary.params.push(AbiParam::new(types::I64));
        binary.returns.push(AbiParam::new(types::I64));


    }
    open_memory: "jet_jit_db_open_memory" => jet_jit_db_open_memory: nullary;
    open: "jet_jit_db_open" => jet_jit_db_open: unary;
    policy: "jet_jit_db_policy" => jet_jit_db_policy: binary;
    with_policy: "jet_jit_db_with_policy" => jet_jit_db_with_policy: ternary;
    close: "jet_jit_db_close" => jet_jit_db_close: unary_i8;
    begin: "jet_jit_db_begin" => jet_jit_db_begin: unary_i8;
    commit: "jet_jit_db_commit" => jet_jit_db_commit: unary_i8;
    rollback: "jet_jit_db_rollback" => jet_jit_db_rollback: unary_i8;
    execute: "jet_jit_db_execute" => jet_jit_db_execute: binary;
    query: "jet_jit_db_query" => jet_jit_db_query: binary;
    query_one: "jet_jit_db_query_one" => jet_jit_db_query_one: binary;
    migrate: "jet_jit_db_migrate" => jet_jit_db_migrate: ternary;
    transaction: "jet_jit_db_transaction" => jet_jit_db_transaction: ternary;
    row_int: "jet_jit_db_row_int" => jet_jit_db_row_int: binary;
    row_text: "jet_jit_db_row_text" => jet_jit_db_row_text: binary;
    dbvalue_pack: "jet_jit_dbvalue_pack" => jet_jit_dbvalue_pack: binary;
    dbvalue_int: "jet_jit_dbvalue_int" => jet_jit_dbvalue_int: unary;
    dbvalue_float: "jet_jit_dbvalue_float" => jet_jit_dbvalue_float: unary;
    dbvalue_text: "jet_jit_dbvalue_text" => jet_jit_dbvalue_text: unary;
    dbvalue_bool: "jet_jit_dbvalue_bool" => jet_jit_dbvalue_bool: unary;
    dbvalue_is_null: "jet_jit_dbvalue_is_null" => jet_jit_dbvalue_is_null: unary_i8;
}
