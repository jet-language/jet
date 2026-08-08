//! `core.db` hosts (#729). `include!` canonical SQLite runtime + wire codec —
//! no third algorithm.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use crate::Marshal::{clone_string, result_ok, result_err_msg};

trait JetShow {
    fn jet_show(&self) -> String;
}

/// Canonical `core.db` FFI runtime (rusqlite).
mod runtime {
    include!("../../jet-pkg-model/src/Prelude/DB.rs");
}

/// Canonical wire encode/decode (`jet_std` DBPluginWire fragment).
mod wire {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
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
    static DB_SCOPES: std::cell::RefCell<HashMap<u64, (u64, String, String, String)>> =
        std::cell::RefCell::new(HashMap::new());
}

static NEXT_DB_SCOPE: AtomicU64 = AtomicU64::new(1_000_000_000);

fn scope_parts(handle: u64) -> Option<(u64, String, String, String)> {
    DB_SCOPES.with(|scopes| scopes.borrow().get(&handle).cloned())
}

fn base_handle(handle: u64) -> u64 {
    scope_parts(handle)
        .map(|(base, _, _, _)| base)
        .unwrap_or(handle)
}

fn alloc_policy_record(table: &str, expression: &str) -> i64 {
    let table_id = table.to_string();
    let expression_id = expression.to_string();
    Concurrency::with_runtime_mut(|rt| {
        let table_id = rt.heap.alloc_string(table_id);
        let expression_id = rt.heap.alloc_string(expression_id);
        let record = rt.heap.alloc_record(3);
        let _ = rt.heap.record_set_int(record, 0, table_id);
        let _ = rt.heap.record_set_int(record, 1, expression_id);
        let _ = rt.heap.record_set_int(record, 2, 0);
        record
    })
}

fn policy_record_parts(policy: i64) -> Option<(String, String)> {
    Concurrency::with_runtime_mut(|rt| {
        let table = rt
            .heap
            .record_get_int(policy, 0)
            .and_then(|id| rt.heap.clone_string(id))?;
        let expression = rt
            .heap
            .record_get_int(policy, 1)
            .and_then(|id| rt.heap.clone_string(id))?;
        Some((table, expression))
    })
}

fn new_scope(connection: u64, table: String, expression: String, user: String) -> i64 {
    if connection == 0 || wire::jet_db_policy_validate(&table, &expression).is_err() {
        return 0;
    }
    let id = NEXT_DB_SCOPE.fetch_add(1, Ordering::Relaxed);
    let base = base_handle(connection);
    DB_SCOPES.with(|scopes| {
        scopes
            .borrow_mut()
            .insert(id, (base, table, expression, user));
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

fn values_from_list(list: i64) -> Vec<wire::DBValue> {
    let handles: Vec<i64> = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(list, i).unwrap_or(0));
        }
        out
    });
    handles
        .into_iter()
        .map(|h| read_dbvalue(h).unwrap_or(wire::DBValue::Null))
        .collect()
}

extern "C" fn jet_jit_db_policy(table: i64, expression: i64) -> i64 {
    let table = clone_string(table);
    let expression = clone_string(expression);
    match wire::jet_db_policy_validate(&table, &expression) {
        Ok(()) => result_ok(alloc_policy_record(&table, &expression) as u64),
        Err(message) => result_err_msg(&message),
    }
}

extern "C" fn jet_jit_db_with_policy(connection: i64, policy: i64, user: i64) -> i64 {
    let Some((table, expression)) = policy_record_parts(policy) else {
        return 0;
    };
    let scope = new_scope(
        connection as u64,
        table,
        expression,
        clone_string(user),
    );
    scope
}

fn rows_to_list_of_maps(rows: Vec<std::collections::BTreeMap<String, wire::DBValue>>) -> i64 {
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

extern "C" fn jet_jit_db_open_memory() -> i64 {
    runtime::jet_db_open_memory() as i64
}

extern "C" fn jet_jit_db_open(path: i64) -> i64 {
    runtime::jet_db_open(&clone_string(path)) as i64
}

extern "C" fn jet_jit_db_close(handle: i64) -> i8 {
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

extern "C" fn jet_jit_db_begin(handle: i64) -> i8 {
    i8::from(runtime::jet_db_begin(base_handle(handle as u64)))
}

extern "C" fn jet_jit_db_commit(handle: i64) -> i8 {
    i8::from(runtime::jet_db_commit(base_handle(handle as u64)))
}

extern "C" fn jet_jit_db_rollback(handle: i64) -> i8 {
    i8::from(runtime::jet_db_rollback(base_handle(handle as u64)))
}

extern "C" fn jet_jit_db_execute(handle: i64, sql: i64, params: i64) -> i64 {
    let Some((base, table, expression, user)) = scope_parts(handle as u64) else {
        return result_err_msg("database row operations require a policy scope");
    };
    let values = values_from_list(params);
    let sql = clone_string(sql);
    let (sql, values) = match wire::jet_db_apply_policy(&sql, &values, &table, &expression, &user) {
        Ok(value) => value,
        Err(error) => return result_err_msg(&error.message),
    };
    let wire_s = wire::jet_db_encode_params(&values);
    let out = runtime::jet_db_execute(base, &sql, &wire_s);
    match wire::jet_db_decode_execute_result(&out) {
        Ok(n) => result_ok(n as u64),
        Err(e) => result_err_msg(&e.message),
    }
}

extern "C" fn jet_jit_db_query(handle: i64, sql: i64, params: i64) -> i64 {
    let Some((base, table, expression, user)) = scope_parts(handle as u64) else {
        return result_err_msg("database row operations require a policy scope");
    };
    let values = values_from_list(params);
    let sql = clone_string(sql);
    let (sql, values) = match wire::jet_db_apply_policy(&sql, &values, &table, &expression, &user) {
        Ok(value) => value,
        Err(error) => return result_err_msg(&error.message),
    };
    let wire_s = wire::jet_db_encode_params(&values);
    let out = runtime::jet_db_query(base, &sql, &wire_s);
    match wire::jet_db_decode_query_result(&out) {
        Ok(rows) => result_ok(rows_to_list_of_maps(rows) as u64),
        Err(e) => result_err_msg(&e.message),
    }
}

extern "C" fn jet_jit_db_query_one(handle: i64, sql: i64, params: i64) -> i64 {
    let Some((base, table, expression, user)) = scope_parts(handle as u64) else {
        return result_err_msg("database row operations require a policy scope");
    };
    let values = values_from_list(params);
    let sql = clone_string(sql);
    let (sql, values) = match wire::jet_db_apply_policy(&sql, &values, &table, &expression, &user) {
        Ok(value) => value,
        Err(error) => return result_err_msg(&error.message),
    };
    let wire_s = wire::jet_db_encode_params(&values);
    let out = runtime::jet_db_query(base, &sql, &wire_s);
    match wire::jet_db_decode_query_result(&out) {
        Ok(rows) => {
            let opt = match rows.into_iter().next() {
                None => 0i64,
                Some(row) => {
                    let list = rows_to_list_of_maps(vec![row]);
                    let map = Concurrency::with_runtime_mut(|rt| {
                        rt.heap.list_get_int(list, 0).unwrap_or(0)
                    });
                    map.wrapping_add(1)
                }
            };
            result_ok(opt as u64)
        }
        Err(e) => result_err_msg(&e.message),
    }
}

fn list_of_strings(list: i64) -> Vec<String> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let sid = rt.heap.list_get_int(list, i).unwrap_or(0);
            out.push(rt.heap.clone_string(sid).unwrap_or_default());
        }
        out
    })
}

fn scoped_execute(
    scope: u64,
    sql: &str,
    params: &Vec<wire::DBValue>,
    allow_schema: bool,
) -> Result<i64, wire::DBError> {
    let Some((base, table, expression, user)) = scope_parts(scope) else {
        return Err(wire::DBError {
            message: "database row operations require a policy scope".to_string(),
        });
    };
    let (sql, values) = if allow_schema {
        wire::jet_db_apply_migration_policy(sql, params, &table, &expression, &user)?
    } else {
        wire::jet_db_apply_policy(sql, params, &table, &expression, &user)?
    };
    let result = runtime::jet_db_execute(base, &sql, &wire::jet_db_encode_params(&values));
    wire::jet_db_decode_execute_result(&result)
}

fn scoped_query(
    scope: u64,
    sql: &str,
    params: &Vec<wire::DBValue>,
    allow_schema: bool,
) -> Result<Vec<std::collections::BTreeMap<String, wire::DBValue>>, wire::DBError> {
    let Some((base, table, expression, user)) = scope_parts(scope) else {
        return Err(wire::DBError {
            message: "database row operations require a policy scope".to_string(),
        });
    };
    let (sql, values) = if allow_schema {
        wire::jet_db_apply_migration_policy(sql, params, &table, &expression, &user)?
    } else {
        wire::jet_db_apply_policy(sql, params, &table, &expression, &user)?
    };
    let result = runtime::jet_db_query(base, &sql, &wire::jet_db_encode_params(&values));
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
        sql: &String,
        params: &Vec<wire::DBValue>,
        allow_schema: bool,
    ) -> Result<i64, wire::DBError> {
        scoped_execute(self.scope, sql, params, allow_schema)
    }

    fn query(
        &mut self,
        sql: &String,
        params: &Vec<wire::DBValue>,
        allow_schema: bool,
    ) -> Result<Vec<std::collections::BTreeMap<String, wire::DBValue>>, wire::DBError> {
        scoped_query(self.scope, sql, params, allow_schema)
    }
}

extern "C" fn jet_jit_db_migrate(conn: i64, name: i64, steps: i64) -> i64 {
    let scope = conn as u64;
    if scope_parts(scope).is_none() {
        return result_err_msg("database migration requires a policy scope");
    }
    let name_s = clone_string(name);
    let steps_v = list_of_strings(steps);
    let mut backend = JitDbBackend { scope };
    match wire::jet_db_migrate(&mut backend, &name_s, &steps_v) {
        Ok(done) => result_ok(done as u64),
        Err(error) => result_err_msg(&error.message),
    }
}

extern "C" fn jet_jit_db_transaction(conn: i64, label: i64, steps: i64) -> i64 {
    let scope = conn as u64;
    if scope_parts(scope).is_none() {
        return result_err_msg("database transaction requires a policy scope");
    }
    let label_s = clone_string(label);
    let steps_v = list_of_strings(steps);
    let mut backend = JitDbBackend { scope };
    match wire::jet_db_transaction(&mut backend, &label_s, &steps_v) {
        Ok(done) => result_ok(done as u64),
        Err(error) => result_err_msg(&error.message),
    }
}

/// `db.params(sql)` — SQL is a 2-slot record `(template, params_list)`.
extern "C" fn jet_jit_db_params(sql: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let params_list = rt.heap.record_get_int(sql, 1).unwrap_or(0);
        let len = rt.heap.list_len(params_list).unwrap_or(0);
        let list = rt.heap.alloc_empty_list();
        for i in 0..len {
            let sid = rt.heap.list_get_int(params_list, i).unwrap_or(0);
            let h = rt.heap.alloc_record(2);
            let _ = rt.heap.record_set_int(h, 0, DV_TEXT);
            let _ = rt.heap.record_set_int(h, 1, sid);
            let _ = rt.heap.list_push_int(list, h);
        }
        list
    })
}

extern "C" fn jet_jit_db_row_int(row: i64, key: i64) -> i64 {
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

extern "C" fn jet_jit_db_row_text(row: i64, key: i64) -> i64 {
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

extern "C" fn jet_jit_dbvalue_pack(disc: i64, payload: i64) -> i64 {
    if disc == DV_FLOAT {
        alloc_dbvalue_float(f64::from_bits(payload as u64))
    } else {
        alloc_dbvalue_record(disc, payload)
    }
}

extern "C" fn jet_jit_dbvalue_int(handle: i64) -> i64 {
    match read_dbvalue(handle) {
        Some(v) => match v.int() {
            Ok(n) => result_ok(n as u64),
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DBValue"),
    }
}

extern "C" fn jet_jit_dbvalue_float(handle: i64) -> i64 {
    match read_dbvalue(handle) {
        Some(v) => match v.float() {
            Ok(f) => result_ok(f.to_bits()),
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DBValue"),
    }
}

extern "C" fn jet_jit_dbvalue_text(handle: i64) -> i64 {
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

extern "C" fn jet_jit_dbvalue_bool(handle: i64) -> i64 {
    match read_dbvalue(handle) {
        Some(v) => match v.bool() {
            Ok(b) => result_ok(u64::from(b)),
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DBValue"),
    }
}

extern "C" fn jet_jit_dbvalue_is_null(handle: i64) -> i8 {
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
    execute: "jet_jit_db_execute" => jet_jit_db_execute: ternary;
    query: "jet_jit_db_query" => jet_jit_db_query: ternary;
    query_one: "jet_jit_db_query_one" => jet_jit_db_query_one: ternary;
    migrate: "jet_jit_db_migrate" => jet_jit_db_migrate: ternary;
    transaction: "jet_jit_db_transaction" => jet_jit_db_transaction: ternary;
    params: "jet_jit_db_params" => jet_jit_db_params: unary;
    row_int: "jet_jit_db_row_int" => jet_jit_db_row_int: binary;
    row_text: "jet_jit_db_row_text" => jet_jit_db_row_text: binary;
    dbvalue_pack: "jet_jit_dbvalue_pack" => jet_jit_dbvalue_pack: binary;
    dbvalue_int: "jet_jit_dbvalue_int" => jet_jit_dbvalue_int: unary;
    dbvalue_float: "jet_jit_dbvalue_float" => jet_jit_dbvalue_float: unary;
    dbvalue_text: "jet_jit_dbvalue_text" => jet_jit_dbvalue_text: unary;
    dbvalue_bool: "jet_jit_dbvalue_bool" => jet_jit_dbvalue_bool: unary;
    dbvalue_is_null: "jet_jit_dbvalue_is_null" => jet_jit_dbvalue_is_null: unary_i8;
}





