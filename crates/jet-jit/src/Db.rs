//! `jet.db` hosts (#729). `include!` canonical SQLite runtime + wire codec —
//! no third algorithm.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

trait JetShow {
    fn jet_show(&self) -> String;
}

/// Canonical `jet.db` FFI runtime (rusqlite).
mod runtime {
    include!("../../jet-pkg-model/src/Prelude/Db.rs");
}

/// Canonical wire encode/decode (`jet_std` DbPluginWire fragment).
mod wire {
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/DbPluginWire.rs");
}

// DbValue heap record ABI (same 2-slot shape as DataTree): [disc:i64, payload].
const DV_NULL: i64 = 0;
const DV_INT: i64 = 1;
const DV_FLOAT: i64 = 2;
const DV_TEXT: i64 = 3;
const DV_BOOL: i64 = 4;

fn clone_heap_string(id: i64) -> String {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id).unwrap_or_default())
}

fn result_ok_bits(bits: u64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(super::JitResultValue { ok: true, bits });
        rt.results.len() as i64
    })
}

fn result_err_msg(msg: &str) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let sid = rt.heap.alloc_string(msg.to_string());
        rt.results.push(super::JitResultValue {
            ok: false,
            bits: sid as u64,
        });
        rt.results.len() as i64
    })
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

fn read_dbvalue(handle: i64) -> Option<wire::DbValue> {
    Concurrency::with_runtime_mut(|rt| {
        let disc = rt.heap.record_get_int(handle, 0)?;
        match disc {
            DV_NULL => Some(wire::DbValue::Null),
            DV_INT => Some(wire::DbValue::Int(
                rt.heap.record_get_int(handle, 1).unwrap_or(0),
            )),
            DV_FLOAT => Some(wire::DbValue::Float(
                rt.heap.record_get_float(handle, 1).unwrap_or(0.0),
            )),
            DV_TEXT => {
                let sid = rt.heap.record_get_int(handle, 1).unwrap_or(0);
                Some(wire::DbValue::Text(
                    rt.heap.clone_string(sid).unwrap_or_default(),
                ))
            }
            DV_BOOL => Some(wire::DbValue::Bool(
                rt.heap.record_get_int(handle, 1).unwrap_or(0) != 0,
            )),
            _ => None,
        }
    })
}

fn encode_params_list(list: i64) -> String {
    let handles: Vec<i64> = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(list, i).unwrap_or(0));
        }
        out
    });
    let vals: Vec<wire::DbValue> = handles
        .into_iter()
        .map(|h| read_dbvalue(h).unwrap_or(wire::DbValue::Null))
        .collect();
    wire::jet_db_encode_params(&vals)
}

fn rows_to_list_of_maps(rows: Vec<std::collections::BTreeMap<String, wire::DbValue>>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for row in rows {
            let map = rt.heap.alloc_empty_map();
            for (k, v) in row {
                let kid = rt.heap.alloc_string(k);
                let vh = match &v {
                    wire::DbValue::Null => {
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, DV_NULL);
                        let _ = rt.heap.record_set_int(h, 1, 0);
                        h
                    }
                    wire::DbValue::Int(n) => {
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, DV_INT);
                        let _ = rt.heap.record_set_int(h, 1, *n);
                        h
                    }
                    wire::DbValue::Float(f) => {
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, DV_FLOAT);
                        let _ = rt.heap.record_set_float(h, 1, *f);
                        h
                    }
                    wire::DbValue::Text(s) => {
                        let sid = rt.heap.alloc_string(s.clone());
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, DV_TEXT);
                        let _ = rt.heap.record_set_int(h, 1, sid);
                        h
                    }
                    wire::DbValue::Bool(b) => {
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
    runtime::jet_db_open(&clone_heap_string(path)) as i64
}

extern "C" fn jet_jit_db_close(handle: i64) -> i8 {
    i8::from(runtime::jet_db_close(handle as u64))
}

extern "C" fn jet_jit_db_begin(handle: i64) -> i8 {
    i8::from(runtime::jet_db_begin(handle as u64))
}

extern "C" fn jet_jit_db_commit(handle: i64) -> i8 {
    i8::from(runtime::jet_db_commit(handle as u64))
}

extern "C" fn jet_jit_db_rollback(handle: i64) -> i8 {
    i8::from(runtime::jet_db_rollback(handle as u64))
}

extern "C" fn jet_jit_db_execute(handle: i64, sql: i64, params: i64) -> i64 {
    let wire_s = encode_params_list(params);
    let out = runtime::jet_db_execute(handle as u64, &clone_heap_string(sql), &wire_s);
    match wire::jet_db_decode_execute_result(&out) {
        Ok(n) => result_ok_bits(n as u64),
        Err(e) => result_err_msg(&e.message),
    }
}

extern "C" fn jet_jit_db_query(handle: i64, sql: i64, params: i64) -> i64 {
    let wire_s = encode_params_list(params);
    let out = runtime::jet_db_query(handle as u64, &clone_heap_string(sql), &wire_s);
    match wire::jet_db_decode_query_result(&out) {
        Ok(rows) => result_ok_bits(rows_to_list_of_maps(rows) as u64),
        Err(e) => result_err_msg(&e.message),
    }
}

extern "C" fn jet_jit_db_query_one(handle: i64, sql: i64, params: i64) -> i64 {
    let wire_s = encode_params_list(params);
    let out = runtime::jet_db_query(handle as u64, &clone_heap_string(sql), &wire_s);
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
            result_ok_bits(opt as u64)
        }
        Err(e) => result_err_msg(&e.message),
    }
}

fn empty_params_wire() -> String {
    let empty: Vec<wire::DbValue> = Vec::new();
    wire::jet_db_encode_params(&empty)
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

extern "C" fn jet_jit_db_migrate(conn: i64, name: i64, steps: i64) -> i64 {
    let name_s = clone_heap_string(name);
    let steps_v = list_of_strings(steps);
    let empty = empty_params_wire();
    if !runtime::jet_db_begin(conn as u64) {
        return result_err_msg(&format!("could not begin migration `{name_s}`"));
    }
    let create_sql =
        "CREATE TABLE IF NOT EXISTS __jet_migrations (name TEXT PRIMARY KEY, checksum TEXT NOT NULL)"
            .to_string();
    match wire::jet_db_decode_execute_result(&runtime::jet_db_execute(
        conn as u64,
        &create_sql,
        &empty,
    )) {
        Err(e) => {
            let _ = runtime::jet_db_rollback(conn as u64);
            return result_err_msg(&e.message);
        }
        Ok(_) => {}
    }
    let checksum = wire::jet_db_migration_checksum(&steps_v);
    let check_sql = "SELECT checksum FROM __jet_migrations WHERE name = ?".to_string();
    let check_vec = vec![wire::DbValue::Text(name_s.clone())];
    let check_params = wire::jet_db_encode_params(&check_vec);
    let existing = match wire::jet_db_decode_query_result(&runtime::jet_db_query(
        conn as u64,
        &check_sql,
        &check_params,
    )) {
        Err(e) => {
            let _ = runtime::jet_db_rollback(conn as u64);
            return result_err_msg(&e.message);
        }
        Ok(rows) => rows,
    };
    if let Some(row) = existing.into_iter().next() {
        let old = row
            .get("checksum")
            .and_then(|v| v.text().ok())
            .unwrap_or_default();
        if old == checksum {
            if runtime::jet_db_commit(conn as u64) {
                return result_ok_bits(0);
            }
            return result_err_msg(&format!("could not commit migration `{name_s}`"));
        }
        let _ = runtime::jet_db_rollback(conn as u64);
        return result_err_msg(&format!("migration `{name_s}` checksum changed"));
    }
    let mut done: i64 = 0;
    for sql in &steps_v {
        match wire::jet_db_decode_execute_result(&runtime::jet_db_execute(
            conn as u64, sql, &empty,
        )) {
            Ok(_) => done += 1,
            Err(e) => {
                let _ = runtime::jet_db_rollback(conn as u64);
                return result_err_msg(&e.message);
            }
        }
    }
    let insert_sql = "INSERT INTO __jet_migrations (name, checksum) VALUES (?, ?)".to_string();
    let insert_vec = vec![
        wire::DbValue::Text(name_s.clone()),
        wire::DbValue::Text(checksum),
    ];
    let insert_params = wire::jet_db_encode_params(&insert_vec);
    match wire::jet_db_decode_execute_result(&runtime::jet_db_execute(
        conn as u64,
        &insert_sql,
        &insert_params,
    )) {
        Err(e) => {
            let _ = runtime::jet_db_rollback(conn as u64);
            return result_err_msg(&e.message);
        }
        Ok(_) => {}
    }
    if runtime::jet_db_commit(conn as u64) {
        result_ok_bits(done as u64)
    } else {
        result_err_msg(&format!("could not commit migration `{name_s}`"))
    }
}

extern "C" fn jet_jit_db_transaction(conn: i64, _label: i64, steps: i64) -> i64 {
    let steps_v = list_of_strings(steps);
    let empty = empty_params_wire();
    if !runtime::jet_db_begin(conn as u64) {
        return result_err_msg("could not begin transaction");
    }
    let mut done: i64 = 0;
    for sql in &steps_v {
        match wire::jet_db_decode_execute_result(&runtime::jet_db_execute(
            conn as u64, sql, &empty,
        )) {
            Ok(_) => done += 1,
            Err(e) => {
                let _ = runtime::jet_db_rollback(conn as u64);
                return result_err_msg(&e.message);
            }
        }
    }
    if runtime::jet_db_commit(conn as u64) {
        result_ok_bits(done as u64)
    } else {
        result_err_msg("could not commit transaction")
    }
}

/// `db.params(sql)` — Sql is a 2-slot record `(template, params_list)`.
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
    let key_s = clone_heap_string(key);
    let val = Concurrency::with_runtime_mut(|rt| {
        let kid = rt.heap.alloc_string(key_s.clone());
        rt.heap.map_get(row, kid)
    });
    match val.and_then(read_dbvalue) {
        Some(v) => match v.int() {
            Ok(n) => result_ok_bits(n as u64),
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg(&format!("missing column `{key_s}`")),
    }
}

extern "C" fn jet_jit_db_row_text(row: i64, key: i64) -> i64 {
    let key_s = clone_heap_string(key);
    let val = Concurrency::with_runtime_mut(|rt| {
        let kid = rt.heap.alloc_string(key_s.clone());
        rt.heap.map_get(row, kid)
    });
    match val.and_then(read_dbvalue) {
        Some(v) => match v.text() {
            Ok(s) => {
                let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
                result_ok_bits(sid as u64)
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
            Ok(n) => result_ok_bits(n as u64),
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DbValue"),
    }
}

extern "C" fn jet_jit_dbvalue_float(handle: i64) -> i64 {
    match read_dbvalue(handle) {
        Some(v) => match v.float() {
            Ok(f) => result_ok_bits(f.to_bits()),
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DbValue"),
    }
}

extern "C" fn jet_jit_dbvalue_text(handle: i64) -> i64 {
    match read_dbvalue(handle) {
        Some(v) => match v.text() {
            Ok(s) => {
                let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
                result_ok_bits(sid as u64)
            }
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DbValue"),
    }
}

extern "C" fn jet_jit_dbvalue_bool(handle: i64) -> i64 {
    match read_dbvalue(handle) {
        Some(v) => match v.bool() {
            Ok(b) => result_ok_bits(u64::from(b)),
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DbValue"),
    }
}

extern "C" fn jet_jit_dbvalue_is_null(handle: i64) -> i8 {
    match read_dbvalue(handle) {
        Some(v) => i8::from(v.is_null()),
        None => 0,
    }
}

pub(crate) struct DbHostFns {
    pub open_memory: FuncId,
    pub open: FuncId,
    pub close: FuncId,
    pub begin: FuncId,
    pub commit: FuncId,
    pub rollback: FuncId,
    pub execute: FuncId,
    pub query: FuncId,
    pub query_one: FuncId,
    pub migrate: FuncId,
    pub transaction: FuncId,
    pub params: FuncId,
    pub row_int: FuncId,
    pub row_text: FuncId,
    pub dbvalue_pack: FuncId,
    pub dbvalue_int: FuncId,
    pub dbvalue_float: FuncId,
    pub dbvalue_text: FuncId,
    pub dbvalue_bool: FuncId,
    pub dbvalue_is_null: FuncId,
}

pub(crate) fn register_db_symbols(builder: &mut JITBuilder) {
    builder.symbol("jet_jit_db_open_memory", jet_jit_db_open_memory as *const u8);
    builder.symbol("jet_jit_db_open", jet_jit_db_open as *const u8);
    builder.symbol("jet_jit_db_close", jet_jit_db_close as *const u8);
    builder.symbol("jet_jit_db_begin", jet_jit_db_begin as *const u8);
    builder.symbol("jet_jit_db_commit", jet_jit_db_commit as *const u8);
    builder.symbol("jet_jit_db_rollback", jet_jit_db_rollback as *const u8);
    builder.symbol("jet_jit_db_execute", jet_jit_db_execute as *const u8);
    builder.symbol("jet_jit_db_query", jet_jit_db_query as *const u8);
    builder.symbol("jet_jit_db_query_one", jet_jit_db_query_one as *const u8);
    builder.symbol("jet_jit_db_migrate", jet_jit_db_migrate as *const u8);
    builder.symbol("jet_jit_db_transaction", jet_jit_db_transaction as *const u8);
    builder.symbol("jet_jit_db_params", jet_jit_db_params as *const u8);
    builder.symbol("jet_jit_db_row_int", jet_jit_db_row_int as *const u8);
    builder.symbol("jet_jit_db_row_text", jet_jit_db_row_text as *const u8);
    builder.symbol("jet_jit_dbvalue_pack", jet_jit_dbvalue_pack as *const u8);
    builder.symbol("jet_jit_dbvalue_int", jet_jit_dbvalue_int as *const u8);
    builder.symbol("jet_jit_dbvalue_float", jet_jit_dbvalue_float as *const u8);
    builder.symbol("jet_jit_dbvalue_text", jet_jit_dbvalue_text as *const u8);
    builder.symbol("jet_jit_dbvalue_bool", jet_jit_dbvalue_bool as *const u8);
    builder.symbol(
        "jet_jit_dbvalue_is_null",
        jet_jit_dbvalue_is_null as *const u8,
    );
}

pub(crate) fn declare_db_host_fns(module: &mut JITModule) -> Result<DbHostFns, String> {
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
    let mut import = |name: &str, sig: &Signature| {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    Ok(DbHostFns {
        open_memory: import("jet_jit_db_open_memory", &nullary)?,
        open: import("jet_jit_db_open", &unary)?,
        close: import("jet_jit_db_close", &unary_i8)?,
        begin: import("jet_jit_db_begin", &unary_i8)?,
        commit: import("jet_jit_db_commit", &unary_i8)?,
        rollback: import("jet_jit_db_rollback", &unary_i8)?,
        execute: import("jet_jit_db_execute", &ternary)?,
        query: import("jet_jit_db_query", &ternary)?,
        query_one: import("jet_jit_db_query_one", &ternary)?,
        migrate: import("jet_jit_db_migrate", &ternary)?,
        transaction: import("jet_jit_db_transaction", &ternary)?,
        params: import("jet_jit_db_params", &unary)?,
        row_int: import("jet_jit_db_row_int", &binary)?,
        row_text: import("jet_jit_db_row_text", &binary)?,
        dbvalue_pack: import("jet_jit_dbvalue_pack", &binary)?,
        dbvalue_int: import("jet_jit_dbvalue_int", &unary)?,
        dbvalue_float: import("jet_jit_dbvalue_float", &unary)?,
        dbvalue_text: import("jet_jit_dbvalue_text", &unary)?,
        dbvalue_bool: import("jet_jit_dbvalue_bool", &unary)?,
        dbvalue_is_null: import("jet_jit_dbvalue_is_null", &unary_i8)?,
    })
}
