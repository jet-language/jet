//! Whole-program interpreter hosts for `core.db` / `core.crypto` (#1254).
//!
//! Same bridge runtimes as Cranelift hosts; CtValue at the boundary. Installed
//! only around `run_whole_interp` so comptime/REPL stay pure / native-denied.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use jet_codegen::AST::{CtFloat, CtKey, CtValue, Type};
use jet_codegen::Diagnostics::{Diagnostic, Span};

use crate::Crypto;
use crate::DB;
use crate::IO;
use jet_codegen::Comptime::ServicesLite as service_prelude;

trait JetShow {
    fn jet_show(&self) -> String;
}

mod wire {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/DBPluginWire.rs");
}

fn unsupported(what: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        format!("{what} can't run in the interpreter yet"),
        "this ambient host call is missing an interpreter binding".to_string(),
        "report this as a runtime-tier coverage gap".to_string(),
        Some(span),
    )
}

fn crypto_err(msg: impl Into<String>) -> CtValue {
    CtValue::Struct {
        type_name: "CryptoError".to_string(),
        fields: vec![("message".to_string(), CtValue::Str(msg.into()))],
    }
}

fn db_err(msg: impl Into<String>) -> CtValue {
    CtValue::Struct {
        type_name: "DBError".to_string(),
        fields: vec![("message".to_string(), CtValue::Str(msg.into()))],
    }
}

fn io_error(kind: &str, cause: impl Into<String>) -> CtValue {
    CtValue::Enum {
        type_name: "IOError".to_string(),
        variant: kind.to_string(),
        args: vec![(
            None,
            CtValue::Struct {
                type_name: "IOContext".to_string(),
                fields: vec![
                    (
                        "operation".to_string(),
                        CtValue::Enum {
                            type_name: "IOOperation".to_string(),
                            variant: "Read".to_string(),
                            args: vec![],
                        },
                    ),
                    (
                        "resource".to_string(),
                        CtValue::Present(Box::new(CtValue::Str("stdin".to_string()))),
                    ),
                    ("os_code".to_string(), CtValue::absent(Type::Int)),
                    (
                        "cause".to_string(),
                        CtValue::Present(Box::new(CtValue::Str(cause.into()))),
                    ),
                ],
            },
        )],
    }
}

fn as_bytes(v: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
    match v {
        CtValue::Bytes(b) => Ok(b.clone()),
        CtValue::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    CtValue::Int(n) if (0..=255).contains(n) => out.push(*n as u8),
                    _ => return Err(unsupported("byte list element", span)),
                }
            }
            Ok(out)
        }
        _ => Err(unsupported("bytes argument", span)),
    }
}

fn secret_bytes(v: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
    match v {
        CtValue::Struct { type_name, fields } if type_name == "Secret" => {
            let field = fields.iter().find_map(|(n, val)| match (n.as_str(), val) {
                ("bytes", val) => Some(val),
                _ => None,
            });
            match field {
                Some(val) => as_bytes(val, span),
                None => Err(unsupported("Secret.bytes", span)),
            }
        }
        _ => as_bytes(v, span),
    }
}

fn secret_value(bytes: Vec<u8>) -> CtValue {
    CtValue::Struct {
        type_name: "Secret".to_string(),
        fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))],
    }
}

fn x25519_secret_value(bytes: Vec<u8>) -> CtValue {
    CtValue::Struct {
        type_name: "X25519SecretKey".to_string(),
        fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))],
    }
}

fn x25519_public_value(bytes: Vec<u8>) -> CtValue {
    CtValue::Struct {
        type_name: "X25519PublicKey".to_string(),
        fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))],
    }
}

fn password_hash_value(text: String) -> CtValue {
    CtValue::Struct {
        type_name: "PasswordHash".to_string(),
        fields: vec![("text".to_string(), CtValue::Str(text))],
    }
}

fn path_string(v: &CtValue) -> Option<String> {
    match v {
        CtValue::Str(s) => Some(s.clone()),
        CtValue::Struct { type_name, fields } if type_name == "Path" => fields
            .iter()
            .find_map(|(n, val)| match (n.as_str(), val) {
                ("inner", CtValue::Str(s)) => Some(s.clone()),
                _ => None,
            }),
        _ => None,
    }
}

fn db_conn_value(handle: u64) -> CtValue {
    CtValue::Struct {
        type_name: "DBConnection".to_string(),
        fields: vec![("handle".to_string(), CtValue::Int(handle as i64))],
    }
}

fn db_policy_value(table: String, expression: String) -> CtValue {
    CtValue::Struct {
        type_name: "RowPolicy".to_string(),
        fields: vec![
            ("table".to_string(), CtValue::Str(table)),
            ("expression".to_string(), CtValue::Str(expression)),
        ],
    }
}

fn db_scope_value(handle: u64, table: String, expression: String, user: String) -> CtValue {
    CtValue::Struct {
        type_name: "DBScope".to_string(),
        fields: vec![
            ("handle".to_string(), CtValue::Int(handle as i64)),
            ("policy".to_string(), db_policy_value(table, expression)),
            ("user".to_string(), CtValue::Str(user)),
        ],
    }
}

fn db_handle(recv: &CtValue) -> Option<u64> {
    match recv {
        CtValue::Struct { type_name, fields }
            if matches!(type_name.as_str(), "DBConnection" | "DBScope") => fields
            .iter()
            .find_map(|(n, v)| match (n.as_str(), v) {
                ("handle", CtValue::Int(h)) if *h > 0 => Some(*h as u64),
                _ => None,
            }),
        _ => None,
    }
}

fn db_scope_parts(recv: &CtValue) -> Option<(u64, String, String, String)> {
    let handle = db_handle(recv)?;
    let CtValue::Struct { fields, .. } = recv else {
        return None;
    };
    let policy = fields.iter().find_map(|(name, value)| {
        (name == "policy").then_some(value)
    })?;
    let CtValue::Struct { fields: policy_fields, .. } = policy else {
        return None;
    };
    let table = policy_fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("table", CtValue::Str(value)) => Some(value.clone()),
        _ => None,
    })?;
    let expression = policy_fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("expression", CtValue::Str(value)) => Some(value.clone()),
        _ => None,
    })?;
    let user = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("user", CtValue::Str(value)) => Some(value.clone()),
        _ => None,
    })?;
    Some((handle, table, expression, user))
}

fn service_runtime_value(store: String, retention_ms: i64) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceRuntime".to_string(),
        fields: vec![
            ("store".to_string(), CtValue::Str(store)),
            ("retention_ms".to_string(), CtValue::Int(retention_ms)),
        ],
    }
}

fn service_runtime_parts(recv: &CtValue) -> Option<service_prelude::JetServiceRuntime> {
    let CtValue::Struct { type_name, fields } = recv else {
        return None;
    };
    if type_name != "ServiceRuntime" {
        return None;
    }
    let store = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("store", CtValue::Str(value)) => Some(value.clone()),
        _ => None,
    })?;
    let retention_ms = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("retention_ms", CtValue::Int(value)) => Some(*value),
        _ => None,
    })?;
    Some(service_prelude::JetServiceRuntime { store, retention_ms })
}

fn service_endpoint_value(value: &CtValue) -> Option<service_prelude::JetServiceEndpoint> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "ServiceEndpoint" {
        return None;
    }
    let tree = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("tree", CtValue::Str(value)) => Some(value.clone()),
        _ => None,
    })?;
    let worker = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("worker", CtValue::Str(value)) => Some(value.clone()),
        _ => None,
    })?;
    let generation = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("generation", CtValue::Int(value)) => Some(*value),
        _ => None,
    })?;
    let authority = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("authority", CtValue::Str(value)) => Some(value.clone()),
        _ => None,
    })?;
    service_prelude::jet_services_authority_endpoint(tree, worker, generation, authority).ok()
}

fn service_receipt_value(receipt: service_prelude::JetServiceReceipt) -> CtValue {
    match receipt {
        service_prelude::JetServiceReceipt::Accepted(id) => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "Accepted".to_string(),
            args: vec![(None, CtValue::Str(id))],
        },
        service_prelude::JetServiceReceipt::Duplicate(id) => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "Duplicate".to_string(),
            args: vec![(None, CtValue::Str(id))],
        },
        service_prelude::JetServiceReceipt::Retained { id, until } => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "Retained".to_string(),
            args: vec![
                (Some("id".to_string()), CtValue::Str(id)),
                (Some("until".to_string()), CtValue::Int(until)),
            ],
        },
        service_prelude::JetServiceReceipt::DeadLettered(id) => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "DeadLettered".to_string(),
            args: vec![(None, CtValue::Str(id))],
        },
        service_prelude::JetServiceReceipt::Rejected(reason) => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "Rejected".to_string(),
            args: vec![(None, CtValue::Str(reason))],
        },
        service_prelude::JetServiceReceipt::Unavailable(reason) => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "Unavailable".to_string(),
            args: vec![(None, CtValue::Str(reason))],
        },
    }
}

fn service_error_value(error: service_prelude::JetServiceError) -> CtValue {
    let (variant, message) = match error {
        service_prelude::JetServiceError::Full(message) => ("Full", message),
        service_prelude::JetServiceError::Ambiguous(message) => ("Ambiguous", message),
        service_prelude::JetServiceError::Unknown(message) => ("Unknown", message),
        service_prelude::JetServiceError::NotStarted(message) => ("NotStarted", message),
        service_prelude::JetServiceError::Policy(message) => ("Policy", message),
        service_prelude::JetServiceError::Unavailable(message) => ("Unavailable", message),
        service_prelude::JetServiceError::Partitioned(message) => ("Partitioned", message),
        service_prelude::JetServiceError::Revoked(message) => ("Revoked", message),
        service_prelude::JetServiceError::Stale(message) => ("Stale", message),
        service_prelude::JetServiceError::Expired(message) => ("Expired", message),
    };
    CtValue::Enum {
        type_name: "ServiceError".to_string(),
        variant: variant.to_string(),
        args: vec![(None, CtValue::Str(message))],
    }
}

fn service_duration_ms(value: &CtValue) -> Option<i64> {
    match value {
        // The `Duration` carrier's one field is `ns` (see eval/handles.rs
        // `duration_new`); this host wants milliseconds, so convert.
        CtValue::Struct { type_name, fields } if type_name == "Duration" => fields
            .iter()
            .find_map(|(name, value)| (name == "ns").then_some(value))
            .and_then(|value| match value {
                CtValue::Int(ns) => Some(ns / 1_000_000),
                _ => None,
            }),
        _ => None,
    }
}

fn ct_db_value(v: &CtValue) -> Option<wire::DBValue> {
    match v {
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if type_name == "DBValue" => match (variant.as_str(), args.as_slice()) {
            ("Null", _) => Some(wire::DBValue::Null),
            ("Int", [(_, CtValue::Int(n))]) => Some(wire::DBValue::Int(*n)),
            ("Float", [(_, CtValue::Float(f))]) => Some(wire::DBValue::Float(f.as_f64())),
            ("Text", [(_, CtValue::Str(s))]) => Some(wire::DBValue::Text(s.clone())),
            ("Bool", [(_, CtValue::Bool(b))]) => Some(wire::DBValue::Bool(*b)),
            _ => None,
        },
        _ => None,
    }
}

fn wire_db_value(v: wire::DBValue) -> CtValue {
    match v {
        wire::DBValue::Null => CtValue::Enum {
            type_name: "DBValue".into(),
            variant: "Null".into(),
            args: vec![],
        },
        wire::DBValue::Int(n) => CtValue::Enum {
            type_name: "DBValue".into(),
            variant: "Int".into(),
            args: vec![(None, CtValue::Int(n))],
        },
        wire::DBValue::Float(f) => CtValue::Enum {
            type_name: "DBValue".into(),
            variant: "Float".into(),
            args: vec![(None, CtValue::Float(CtFloat::f64(f)))],
        },
        wire::DBValue::Text(s) => CtValue::Enum {
            type_name: "DBValue".into(),
            variant: "Text".into(),
            args: vec![(None, CtValue::Str(s))],
        },
        wire::DBValue::Bool(b) => CtValue::Enum {
            type_name: "DBValue".into(),
            variant: "Bool".into(),
            args: vec![(None, CtValue::Bool(b))],
        },
    }
}

fn row_map(row: BTreeMap<String, wire::DBValue>) -> CtValue {
    let mut m = BTreeMap::new();
    for (k, v) in row {
        m.insert(CtKey::Str(k), wire_db_value(v));
    }
    CtValue::Map(m)
}

fn db_params(list: &CtValue, span: Span) -> Result<Vec<wire::DBValue>, Diagnostic> {
    let CtValue::List(items) = list else {
        return Err(unsupported("db params list", span));
    };
    let mut vals = Vec::with_capacity(items.len());
    for item in items {
        vals.push(ct_db_value(item).ok_or_else(|| unsupported("DBValue param", span))?);
    }
    Ok(vals)
}

fn ambient_db_scope_execute(
    scope: &(u64, String, String, String),
    sql: &str,
    params: &Vec<wire::DBValue>,
    allow_schema: bool,
) -> Result<i64, wire::DBError> {
    let (handle, table, expression, user) = scope;
    let (sql, values) = if allow_schema {
        wire::jet_db_apply_migration_policy(sql, params, table, expression, user)?
    } else {
        wire::jet_db_apply_policy(sql, params, table, expression, user)?
    };
    let result = DB::runtime_execute(*handle, &sql, &wire::jet_db_encode_params(&values));
    wire::jet_db_decode_execute_result(&result)
}

fn ambient_db_scope_query(
    scope: &(u64, String, String, String),
    sql: &str,
    params: &Vec<wire::DBValue>,
    allow_schema: bool,
) -> Result<Vec<BTreeMap<String, wire::DBValue>>, wire::DBError> {
    let (handle, table, expression, user) = scope;
    let (sql, values) = if allow_schema {
        wire::jet_db_apply_migration_policy(sql, params, table, expression, user)?
    } else {
        wire::jet_db_apply_policy(sql, params, table, expression, user)?
    };
    let result = DB::runtime_query(*handle, &sql, &wire::jet_db_encode_params(&values));
    wire::jet_db_decode_query_result(&result)
}

struct AmbientDbBackend {
    scope: (u64, String, String, String),
}

impl wire::JetDBBackend for AmbientDbBackend {
    fn begin(&mut self) -> bool {
        DB::runtime_begin(self.scope.0)
    }

    fn commit(&mut self) -> bool {
        DB::runtime_commit(self.scope.0)
    }

    fn rollback(&mut self) {
        let _ = DB::runtime_rollback(self.scope.0);
    }

    fn execute(
        &mut self,
        sql: &String,
        params: &Vec<wire::DBValue>,
        allow_schema: bool,
    ) -> Result<i64, wire::DBError> {
        ambient_db_scope_execute(&self.scope, sql, params, allow_schema)
    }

    fn query(
        &mut self,
        sql: &String,
        params: &Vec<wire::DBValue>,
        allow_schema: bool,
    ) -> Result<Vec<BTreeMap<String, wire::DBValue>>, wire::DBError> {
        ambient_db_scope_query(&self.scope, sql, params, allow_schema)
    }
}

fn ambient_db_steps(value: &CtValue, span: Span) -> Result<Vec<String>, Diagnostic> {
    ct_string_list(value).ok_or_else(|| unsupported("database steps list", span))
}

fn to_secret(v: &CtValue, span: Span) -> Result<Crypto::runtime::Secret, Diagnostic> {
    let bytes = secret_bytes(v, span)?;
    Ok(Crypto::runtime::jet_crypto_secret_from_bytes_impl(bytes))
}

fn struct_bytes(v: &CtValue, type_name: &str, span: Span) -> Result<Vec<u8>, Diagnostic> {
    match v {
        CtValue::Struct {
            type_name: tn,
            fields,
        } if tn == type_name => {
            let field = fields.iter().find_map(|(n, val)| match (n.as_str(), val) {
                ("bytes", val) => Some(val),
                _ => None,
            });
            match field {
                Some(val) => as_bytes(val, span),
                None => Err(unsupported(&format!("{type_name}.bytes"), span)),
            }
        }
        _ => as_bytes(v, span),
    }
}

pub fn ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    // I9: core.http.server adapters call the same Prelude helpers as AOT/JIT.
    if module == "core.http.server" {
        return Some(ambient_http_server_call(method, &args, span));
    }
    if module == "core.http.client" && method == "request" {
        let result = match args.as_slice() {
            [CtValue::Str(method), CtValue::Str(url)] => Ok(http_handle_value(
                "HTTPRequest",
                crate::net_http_rt::runtime_http_request_new(method.clone(), url.clone()),
            )),
            _ => Err(unsupported("core.http.client.request arguments", span)),
        };
        return Some(result);
    }
    match (module, method) {
        ("core.testing", "temp_dir") => {
            let Some(CtValue::Str(prefix)) = args.first() else {
                return Some(Err(unsupported("core.testing.temp_dir arguments", span)));
            };
            Some(Ok(CtValue::Str(
                crate::testing_shared::jet_testing_temp_dir_path(prefix),
            )))
        }
        ("core.services", "runtime") => {
            let (Some(CtValue::Str(store)), Some(retention)) = (args.first(), args.get(1)) else {
                return Some(Err(unsupported("core.services.runtime arguments", span)));
            };
            let Some(retention_ms) = service_duration_ms(retention) else {
                return Some(Err(unsupported("core.services.runtime duration", span)));
            };
            Some(Ok(service_runtime_value(store.clone(), retention_ms)))
        }
        ("core.db", "policy") => {
            let (Some(CtValue::Str(table)), Some(CtValue::Str(expression))) =
                (args.first(), args.get(1))
            else {
                return Some(Err(unsupported("core.db.policy arguments", span)));
            };
            Some(Ok(match wire::jet_db_policy_validate(table, expression) {
                Ok(()) => CtValue::Present(Box::new(db_policy_value(
                    table.clone(),
                    expression.clone(),
                ))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            }))
        }
        ("core.db", "transaction" | "migrate") => {
            let Some(scope_value) = args.first() else {
                return Some(Err(unsupported("database scope", span)));
            };
            let Some(scope) = db_scope_parts(scope_value) else {
                return Some(Ok(CtValue::failed(Box::new(db_err(
                    "database transaction requires a policy scope",
                )))));
            };
            let Some(CtValue::Str(label)) = args.get(1) else {
                return Some(Err(unsupported("database transaction label", span)));
            };
            let steps = match ambient_db_steps(args.get(2)?, span) {
                Ok(steps) => steps,
                Err(error) => return Some(Err(error)),
            };
            let mut backend = AmbientDbBackend { scope };
            let result = if method == "migrate" {
                wire::jet_db_migrate(&mut backend, label, &steps)
            } else {
                wire::jet_db_transaction(&mut backend, label, &steps)
            };
            Some(Ok(match result {
                Ok(done) => CtValue::Present(Box::new(CtValue::Int(done))),
                Err(error) => CtValue::failed(Box::new(db_err(error.message))),
            }))
        }
        ("core.io", "confirm") => {
            let Some(CtValue::Str(prompt)) = args.first() else {
                return Some(Err(unsupported("core.io.confirm prompt", span)));
            };
            Some(Ok(CtValue::Bool(IO::prompt_confirm(prompt))))
        }
        ("core.io", "choose") => {
            let Some(CtValue::Str(prompt)) = args.first() else {
                return Some(Err(unsupported("core.io.choose prompt", span)));
            };
            let Some(CtValue::List(items)) = args.get(1) else {
                return Some(Err(unsupported("core.io.choose items", span)));
            };
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                let CtValue::Str(item) = item else {
                    return Some(Err(unsupported("core.io.choose item", span)));
                };
                values.push(item.clone());
            }
            Some(Ok(match IO::prompt_choose(prompt, &values) {
                Ok(item) => CtValue::Present(Box::new(CtValue::Str(item))),
                Err(error) => CtValue::failed(Box::new(io_error("InvalidInput", error))),
            }))
        }
        ("core.io", "input_secret") => {
            let Some(CtValue::Str(prompt)) = args.first() else {
                return Some(Err(unsupported("core.io.input_secret prompt", span)));
            };
            Some(Ok(match IO::prompt_input_secret(prompt) {
                Ok(secret) => CtValue::Present(Box::new(CtValue::Str(secret))),
                Err(error) => {
                    let kind = if error == "secret input needs a terminal" {
                        "InvalidInput"
                    } else {
                        "Other"
                    };
                    CtValue::failed(Box::new(io_error(kind, error)))
                }
            }))
        }
        ("core.db", "open_memory") => Some(Ok(db_conn_value(DB::runtime_open_memory()))),
        ("core.db", "open") => {
            let path = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("core.db.open path", span))),
            };
            Some(Ok(db_conn_value(DB::runtime_open(&path))))
        }
        ("core.crypto", "sha512_bytes") => {
            let data = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Str(Crypto::runtime::jet_crypto_sha512_impl(
                &data,
            ))))
        }
        ("core.crypto", "blake3_bytes") => {
            let data = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Str(Crypto::runtime::jet_crypto_blake3_impl(
                &data,
            ))))
        }
        ("core.crypto", "constant_time_equal_bytes") => {
            let a = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let b = match as_bytes(args.get(1)?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Bool(
                Crypto::runtime::jet_crypto_constant_time_equal_bytes_impl(&a, &b),
            )))
        }
        ("core.crypto", "constant_time_equal") => {
            let a = match to_secret(args.first()?, span) {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            let b = match to_secret(args.get(1)?, span) {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Bool(
                Crypto::runtime::jet_crypto_constant_time_secret_impl(&a, &b),
            )))
        }
        ("core.crypto", "hkdf_sha256") => {
            let ikm = match to_secret(args.first()?, span) {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            let salt = match as_bytes(args.get(1)?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let info = match as_bytes(args.get(2)?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let len = match args.get(3) {
                Some(CtValue::Int(n)) => *n,
                _ => return Some(Err(unsupported("hkdf length", span))),
            };
            Some(Ok(
                match Crypto::runtime::jet_crypto_hkdf_typed_impl(&ikm, &salt, &info, len) {
                    Ok(secret) => CtValue::Present(Box::new(secret_value(
                        Crypto::runtime::jet_crypto_expert_secret_bytes_impl(&secret),
                    ))),
                    Err(e) => CtValue::failed(Box::new(crypto_err(e.to_string()))),
                },
            ))
        }
        ("core.crypto", "x25519_public") => {
            let secret = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(
                match Crypto::runtime::jet_crypto_x25519_public_impl(&secret) {
                    Ok(pub_bytes) => CtValue::Present(Box::new(CtValue::Bytes(pub_bytes))),
                    Err(e) => CtValue::failed(Box::new(CtValue::Str(e))),
                },
            ))
        }
        ("core.crypto", "x25519_shared") => {
            let secret = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let public = match as_bytes(args.get(1)?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(
                match Crypto::runtime::jet_crypto_x25519_shared_impl(&secret, &public) {
                    Ok(shared) => CtValue::Present(Box::new(CtValue::Bytes(shared))),
                    Err(e) => CtValue::failed(Box::new(CtValue::Str(e))),
                },
            ))
        }
        ("core.crypto", "password_hash") => {
            let password = match to_secret(args.first()?, span) {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(
                match Crypto::runtime::jet_crypto_password_hash_typed_impl(&password) {
                    Ok(ph) => CtValue::Present(Box::new(password_hash_value(
                        Crypto::runtime::jet_crypto_password_text_impl(&ph),
                    ))),
                    Err(e) => CtValue::failed(Box::new(crypto_err(e.to_string()))),
                },
            ))
        }
        ("core.crypto", "password_verify") => {
            let password = match to_secret(args.first()?, span) {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            let stored = match args.get(1) {
                Some(CtValue::Struct { type_name, fields }) if type_name == "PasswordHash" => {
                    fields
                        .iter()
                        .find_map(|(n, v)| match (n.as_str(), v) {
                            ("text", CtValue::Str(s)) => Some(s.clone()),
                            _ => None,
                        })
                        .ok_or_else(|| unsupported("PasswordHash.text", span))
                }
                _ => Err(unsupported("password_verify stored hash", span)),
            };
            let stored = match stored {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            let ph = Crypto::runtime::password_hash_from_text(stored);
            Some(Ok(
                match Crypto::runtime::jet_crypto_password_verify_typed_impl(&password, &ph) {
                    Ok(b) => CtValue::Present(Box::new(CtValue::Bool(b))),
                    Err(e) => CtValue::failed(Box::new(crypto_err(e.to_string()))),
                },
            ))
        }
        ("core.crypto", "__secret_from_text") => {
            let text = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("Secret.from_text", span))),
            };
            let secret = Crypto::runtime::jet_crypto_secret_from_text_impl(text);
            Some(Ok(secret_value(
                Crypto::runtime::jet_crypto_expert_secret_bytes_impl(&secret),
            )))
        }
        ("core.crypto", "__secret_from_bytes") => {
            let bytes = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let secret = Crypto::runtime::jet_crypto_secret_from_bytes_impl(bytes);
            Some(Ok(secret_value(
                Crypto::runtime::jet_crypto_expert_secret_bytes_impl(&secret),
            )))
        }
        ("core.crypto", "__x25519_generate") => Some(Ok(
            match Crypto::runtime::jet_crypto_x25519_generate_impl() {
                Ok(key) => CtValue::Present(Box::new(x25519_secret_value(
                    Crypto::runtime::jet_crypto_expert_x25519_secret_bytes_impl(&key),
                ))),
                Err(e) => CtValue::failed(Box::new(crypto_err(e.to_string()))),
            },
        )),
        ("core.crypto", "__x25519_public") => {
            let bytes = match struct_bytes(args.first()?, "X25519SecretKey", span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            match Crypto::runtime::jet_crypto_x25519_public_impl(&bytes) {
                Ok(pub_bytes) => Some(Ok(x25519_public_value(pub_bytes))),
                Err(e) => Some(Err(unsupported(&e, span))),
            }
        }
        ("core.crypto", "__password_text") => {
            let text = match args.first() {
                Some(CtValue::Struct { type_name, fields }) if type_name == "PasswordHash" => {
                    fields.iter().find_map(|(n, v)| match (n.as_str(), v) {
                        ("text", CtValue::Str(s)) => Some(s.clone()),
                        _ => None,
                    })
                }
                _ => None,
            };
            match text {
                Some(s) => Some(Ok(CtValue::Str(s))),
                None => Some(Err(unsupported("PasswordHash.text", span))),
            }
        }
        ("core.crypto", "file_seal") => {
            let recipients = match args.first() {
                Some(CtValue::List(items)) => {
                    let mut out = Vec::new();
                    for item in items {
                        let bytes = match struct_bytes(item, "X25519PublicKey", span) {
                            Ok(b) => b,
                            Err(e) => return Some(Err(e)),
                        };
                        match Crypto::runtime::jet_crypto_x25519_public_from_bytes_impl(bytes) {
                            Ok(pk) => out.push(pk),
                            Err(e) => {
                                return Some(Ok(CtValue::failed(Box::new(crypto_err(
                                    e.to_string(),
                                )))))
                            }
                        }
                    }
                    out
                }
                _ => return Some(Err(unsupported("file_seal recipients", span))),
            };
            let source = match args.get(1).and_then(path_string) {
                Some(s) => s,
                None => return Some(Err(unsupported("file_seal source", span))),
            };
            let dest = match args.get(2).and_then(path_string) {
                Some(s) => s,
                None => return Some(Err(unsupported("file_seal destination", span))),
            };
            Some(Ok(
                match Crypto::runtime::jet_crypto_file_seal_impl(
                    recipients,
                    &source,
                    &dest,
                    || false,
                ) {
                    Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
                    Err(e) => CtValue::failed(Box::new(crypto_err(e.to_string()))),
                },
            ))
        }
        ("core.crypto", "file_open") => {
            let key_bytes = match struct_bytes(args.first()?, "X25519SecretKey", span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let source = match args.get(1).and_then(path_string) {
                Some(s) => s,
                None => return Some(Err(unsupported("file_open source", span))),
            };
            let dest = match args.get(2).and_then(path_string) {
                Some(s) => s,
                None => return Some(Err(unsupported("file_open destination", span))),
            };
            Some(Ok(match Crypto::x25519_secret_from_vec(key_bytes) {
                Ok(recipient) => {
                    match Crypto::runtime::jet_crypto_file_open_impl(
                        &recipient,
                        &source,
                        &dest,
                        || false,
                    ) {
                        Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
                        Err(e) => CtValue::failed(Box::new(crypto_err(e.to_string()))),
                    }
                }
                Err(e) => CtValue::failed(Box::new(crypto_err(e))),
            }))
        }
        _ => None,
    }
}

struct InterpWebCallback {
    id: i64,
    callable: CtValue,
    args: Vec<CtValue>,
    reply: mpsc::SyncSender<CtValue>,
}

struct InterpWebServer {
    requests: Mutex<mpsc::Receiver<InterpWebCallback>>,
    replies: Mutex<HashMap<i64, mpsc::SyncSender<CtValue>>>,
}

static INTERP_WEB_SERVERS: OnceLock<Mutex<Vec<Arc<InterpWebServer>>>> = OnceLock::new();
static INTERP_WEB_CALLBACK_ID: AtomicI64 = AtomicI64::new(1);

fn interp_web_servers() -> &'static Mutex<Vec<Arc<InterpWebServer>>> {
    INTERP_WEB_SERVERS.get_or_init(|| Mutex::new(Vec::new()))
}

fn interp_web_server_value(index: usize) -> CtValue {
    CtValue::Struct {
        type_name: "__JetInterpWebServer".to_string(),
        fields: vec![("index".to_string(), CtValue::Int(index as i64))],
    }
}

fn interp_web_server(value: &CtValue) -> Option<Arc<InterpWebServer>> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "__JetInterpWebServer" {
        return None;
    }
    let index = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("index", CtValue::Int(index)) => usize::try_from(*index).ok(),
        _ => None,
    })?;
    interp_web_servers().lock().ok()?.get(index).cloned()
}

fn interp_web_field<'a>(
    fields: &'a [(String, CtValue)],
    name: &str,
) -> Option<&'a CtValue> {
    fields
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
}

fn interp_web_steps(value: &CtValue, span: Span) -> Result<Vec<(String, Vec<CtValue>)>, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("WebApp state", span));
    };
    if type_name != "__JetTirWebAppState" {
        return Err(unsupported("WebApp state", span));
    }
    let Some(CtValue::List(steps)) = interp_web_field(fields, "steps") else {
        return Err(unsupported("WebApp steps", span));
    };
    steps
        .iter()
        .map(|step| {
            let CtValue::Struct { type_name, fields } = step else {
                return Err(unsupported("WebApp step", span));
            };
            if type_name != "__JetTirWebAppStep" {
                return Err(unsupported("WebApp step", span));
            }
            let method = match interp_web_field(fields, "method") {
                Some(CtValue::Str(method)) => method.clone(),
                _ => return Err(unsupported("WebApp step method", span)),
            };
            let args = match interp_web_field(fields, "args") {
                Some(CtValue::List(args)) => args.clone(),
                _ => return Err(unsupported("WebApp step arguments", span)),
            };
            Ok((method, args))
        })
        .collect()
}

fn interp_web_string(args: &[CtValue], index: usize, span: Span) -> Result<String, Diagnostic> {
    match args.get(index) {
        Some(CtValue::Str(value)) => Ok(value.clone()),
        _ => Err(unsupported("WebApp text argument", span)),
    }
}

fn interp_web_callback(
    sender: &mpsc::Sender<InterpWebCallback>,
    callable: CtValue,
    args: Vec<CtValue>,
) -> CtValue {
    let id = INTERP_WEB_CALLBACK_ID.fetch_add(1, Ordering::Relaxed);
    let (reply, receive) = mpsc::sync_channel(1);
    if sender
        .send(InterpWebCallback {
            id,
            callable,
            args,
            reply,
        })
        .is_err()
    {
        return CtValue::Unit;
    }
    receive.recv().unwrap_or(CtValue::Unit)
}

fn interp_web_page(value: CtValue) -> crate::Web::web_rt::JetWebPage {
    let CtValue::Struct { type_name, fields } = value else {
        return crate::Web::web_rt::jet_web_page(String::new(), String::new());
    };
    if type_name != "__JetTirWebPage" {
        return crate::Web::web_rt::jet_web_page(String::new(), String::new());
    }
    let text = |name| match interp_web_field(&fields, name) {
        Some(CtValue::Str(value)) => value.clone(),
        _ => String::new(),
    };
    crate::Web::web_rt::jet_web_page(text("title"), text("body"))
}

fn materialize_interp_web_app(
    state: &CtValue,
    sender: Option<&mpsc::Sender<InterpWebCallback>>,
    span: Span,
) -> Result<crate::Web::web_rt::JetWebApp, Diagnostic> {
    let mut app = crate::Web::web_rt::jet_web_app();
    for (method, args) in interp_web_steps(state, span)? {
        app = match method.as_str() {
            "route" | "page" | "layout" => {
                let path = interp_web_string(&args, 0, span)?;
                let callable = args
                    .get(1)
                    .cloned()
                    .ok_or_else(|| unsupported("WebApp page callback", span))?;
                let callback_sender = sender.cloned();
                let handler = move || {
                    callback_sender
                        .as_ref()
                        .map(|sender| {
                            interp_web_page(interp_web_callback(
                                sender,
                                callable.clone(),
                                Vec::new(),
                            ))
                        })
                        .unwrap_or_default()
                };
                match method.as_str() {
                    "route" => app.route(path, handler),
                    "page" => app.page(path, handler),
                    _ => app.layout(path, handler),
                }
            }
            "action" | "form" | "data" => {
                let name = interp_web_string(&args, 0, span)?;
                let callable = args
                    .get(1)
                    .cloned()
                    .ok_or_else(|| unsupported("WebApp action callback", span))?;
                let callback_sender = sender.cloned();
                let handler = move || {
                    if let Some(sender) = &callback_sender {
                        let _ = interp_web_callback(sender, callable.clone(), Vec::new());
                    }
                };
                match method.as_str() {
                    "action" => app.action(name, handler),
                    "form" => app.form(name, handler),
                    _ => app.data(name, handler),
                }
            }
            "mount" => {
                let prefix = interp_web_string(&args, 0, span)?;
                let callable = args
                    .get(1)
                    .cloned()
                    .ok_or_else(|| unsupported("WebApp mount callback", span))?;
                let callback_sender = sender.cloned();
                app.mount(prefix, move |path| {
                    if let Some(sender) = &callback_sender {
                        let _ = interp_web_callback(
                            sender,
                            callable.clone(),
                            vec![CtValue::Str(path.clone())],
                        );
                    }
                })
            }
            "routes" => app.routes(interp_web_string(&args, 0, span)?),
            "security" => app.security(interp_web_string(&args, 0, span)?),
            "assets" => app.assets(interp_web_string(&args, 0, span)?),
            "split" => app.split(interp_web_string(&args, 0, span)?),
            "code_split" => app.code_split(interp_web_string(&args, 0, span)?),
            "cache" => app.cache(interp_web_string(&args, 0, span)?),
            "a11y" => app.a11y(interp_web_string(&args, 0, span)?),
            "adapter" => app.adapter(interp_web_string(&args, 0, span)?),
            "csr" => app.csr(),
            "ssr" => app.ssr(),
            "ssg" => app.ssg(),
            "stream" => app.stream(),
            "streaming" => app.streaming(),
            "island" => app.island(),
            "hydration_dev" => app.hydration_dev(),
            "hydration_release" => app.hydration_release(),
            _ => return Err(unsupported(&format!("WebApp.{method}"), span)),
        };
    }
    Ok(app)
}

fn ambient_webapp_handle(
    op: &str,
    recv: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let result = match op {
        "WebAppFacts" => materialize_interp_web_app(recv, None, span)
            .map(|app| CtValue::Str(app.facts_json())),
        "WebAppServe" => {
            let (requests, receiver) = mpsc::channel();
            let app = match materialize_interp_web_app(recv, Some(&requests), span) {
                Ok(app) => app,
                Err(error) => return Some(Err(error)),
            };
            let port = match args.first() {
                Some(CtValue::Int(port)) => Some(*port),
                None => None,
                _ => return Some(Err(unsupported("WebApp serve port", span))),
            };
            std::thread::spawn(move || match port {
                Some(port) => app.serve_on(port),
                None => app.serve(),
            });
            let server = Arc::new(InterpWebServer {
                requests: Mutex::new(receiver),
                replies: Mutex::new(HashMap::new()),
            });
            let mut servers = interp_web_servers()
                .lock()
                .expect("interpreter WebApp registry poisoned");
            let index = servers.len();
            servers.push(server);
            Ok(interp_web_server_value(index))
        }
        "WebAppNext" => {
            let server = match interp_web_server(recv) {
                Some(server) => server,
                None => return Some(Err(unsupported("WebApp server handle", span))),
            };
            let request = match server
                .requests
                .lock()
                .expect("interpreter WebApp request queue poisoned")
                .recv()
            {
                Ok(request) => request,
                Err(_) => return Some(Err(unsupported("WebApp request queue", span))),
            };
            server
                .replies
                .lock()
                .expect("interpreter WebApp reply queue poisoned")
                .insert(request.id, request.reply);
            Ok(CtValue::Struct {
                type_name: "__JetInterpWebCallback".to_string(),
                fields: vec![
                    ("id".to_string(), CtValue::Int(request.id)),
                    ("callable".to_string(), request.callable),
                    ("args".to_string(), CtValue::List(request.args)),
                ],
            })
        }
        "WebAppReply" => {
            let server = match interp_web_server(recv) {
                Some(server) => server,
                None => return Some(Err(unsupported("WebApp server handle", span))),
            };
            let id = match args.first() {
                Some(CtValue::Int(id)) => *id,
                _ => return Some(Err(unsupported("WebApp callback id", span))),
            };
            let value = args.get(1).cloned().unwrap_or(CtValue::Unit);
            let reply = server
                .replies
                .lock()
                .expect("interpreter WebApp reply queue poisoned")
                .remove(&id);
            match reply {
                Some(reply) => reply
                    .send(value)
                    .map(|_| CtValue::Unit)
                    .map_err(|_| unsupported("WebApp callback reply", span)),
                None => Err(unsupported("WebApp callback reply id", span)),
            }
        }
        _ => return None,
    };
    Some(result)
}

pub fn ambient_handle(
    op: &str,
    recv: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if let Some(result) = ambient_webapp_handle(op, recv, args, span) {
        return Some(result);
    }
    if let Some(result) = ambient_http_handle(op, recv, args, span) {
        return Some(result);
    }
    if op == "DBWithPolicy" {
        let handle = db_handle(recv)?;
        let (CtValue::Struct { fields, .. }, CtValue::Str(user)) =
            (args.first()?, args.get(1)?)
        else {
            return Some(Err(unsupported("DBConnection.with_policy arguments", span)));
        };
        let table = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
            ("table", CtValue::Str(value)) => Some(value.clone()),
            _ => None,
        });
        let expression = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
            ("expression", CtValue::Str(value)) => Some(value.clone()),
            _ => None,
        });
        let (Some(table), Some(expression)) = (table, expression) else {
            return Some(Err(unsupported("DBConnection.with_policy policy", span)));
        };
        return Some(match wire::jet_db_policy_validate(&table, &expression) {
            Ok(()) => Ok(db_scope_value(handle, table, expression, user.clone())),
            Err(error) => Err(unsupported(&format!("row policy: {error}"), span)),
        });
    }
    if matches!(
        op,
        "ServiceRuntimeSend"
            | "ServiceRuntimeRetry"
            | "ServiceRuntimeDeadLetter"
            | "ServiceRuntimeRetain"
            | "ServiceRuntimeCommit"
    ) {
        let Some(runtime) = service_runtime_parts(recv) else {
            return Some(Err(unsupported("ServiceRuntime receiver", span)));
        };
        if op == "ServiceRuntimeCommit" {
            let Some(CtValue::Str(id)) = args.first() else {
                return Some(Err(unsupported("ServiceRuntime.commit id", span)));
            };
            return Some(Ok(match service_prelude::jet_services_runtime_commit(&runtime, id) {
                Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
                Err(error) => CtValue::failed(Box::new(service_error_value(error))),
            }));
        }
        let result = match op {
            "ServiceRuntimeSend" => {
                let Some(endpoint) = args.first().and_then(service_endpoint_value) else {
                    return Some(Err(unsupported("ServiceRuntime.send endpoint", span)));
                };
                let Some(CtValue::Str(message)) = args.get(1) else {
                    return Some(Err(unsupported("ServiceRuntime.send message", span)));
                };
                let Some(CtValue::Str(key)) = args.get(2) else {
                    return Some(Err(unsupported("ServiceRuntime.send key", span)));
                };
                service_prelude::jet_services_runtime_send(&runtime, &endpoint, message, key)
            }
            "ServiceRuntimeRetry" => {
                let Some(CtValue::Str(id)) = args.first() else {
                    return Some(Err(unsupported("ServiceRuntime.retry id", span)));
                };
                service_prelude::jet_services_runtime_retry(&runtime, id)
            }
            "ServiceRuntimeDeadLetter" => {
                let Some(CtValue::Str(id)) = args.first() else {
                    return Some(Err(unsupported("ServiceRuntime.dead_letter id", span)));
                };
                service_prelude::jet_services_runtime_dead_letter(&runtime, id)
            }
            "ServiceRuntimeRetain" => {
                let Some(CtValue::Str(id)) = args.first() else {
                    return Some(Err(unsupported("ServiceRuntime.retain id", span)));
                };
                service_prelude::jet_services_runtime_retain(&runtime, id)
            }
            _ => unreachable!(),
        };
        return Some(Ok(match result {
            Ok(receipt) => CtValue::Present(Box::new(service_receipt_value(receipt))),
            Err(error) => CtValue::failed(Box::new(service_error_value(error))),
        }));
    }
    let handle = db_handle(recv)?;
    match op {
        "DBBegin" => Some(Ok(CtValue::Bool(DB::runtime_begin(handle)))),
        "DBCommit" => Some(Ok(CtValue::Bool(DB::runtime_commit(handle)))),
        "DBRollback" => Some(Ok(CtValue::Bool(DB::runtime_rollback(handle)))),
        "DBClose" => Some(Ok(CtValue::Bool(DB::runtime_close(handle)))),
        "DBExecute" => {
            let sql = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("DBScope.execute sql", span))),
            };
            let values = match db_params(args.get(1).unwrap_or(&CtValue::List(vec![])), span) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            let (handle, sql, values) = match db_scope_parts(recv) {
                Some((handle, table, expression, user)) => match wire::jet_db_apply_policy(
                    &sql, &values, &table, &expression, &user,
                ) {
                    Ok((sql, values)) => (handle, sql, values),
                    Err(error) => return Some(Ok(CtValue::failed(Box::new(db_err(error.message))))),
                },
                None => return Some(Ok(CtValue::failed(Box::new(db_err(
                    "database row operations require a policy scope",
                ))))),
            };
            let params = wire::jet_db_encode_params(&values);
            let out = DB::runtime_execute(handle, &sql, &params);
            Some(Ok(match wire::jet_db_decode_execute_result(&out) {
                Ok(n) => CtValue::Present(Box::new(CtValue::Int(n))),
                Err(e) => CtValue::failed(Box::new(db_err(e.message))),
            }))
        }
        "DBQuery" => {
            let sql = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("DBScope.query sql", span))),
            };
            let values = match db_params(args.get(1).unwrap_or(&CtValue::List(vec![])), span) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            let (handle, sql, values) = match db_scope_parts(recv) {
                Some((handle, table, expression, user)) => match wire::jet_db_apply_policy(
                    &sql, &values, &table, &expression, &user,
                ) {
                    Ok((sql, values)) => (handle, sql, values),
                    Err(error) => return Some(Ok(CtValue::failed(Box::new(db_err(error.message))))),
                },
                None => return Some(Ok(CtValue::failed(Box::new(db_err(
                    "database row operations require a policy scope",
                ))))),
            };
            let params = wire::jet_db_encode_params(&values);
            let out = DB::runtime_query(handle, &sql, &params);
            Some(Ok(match wire::jet_db_decode_query_result(&out) {
                Ok(rows) => CtValue::Present(Box::new(CtValue::List(
                    rows.into_iter().map(row_map).collect(),
                ))),
                Err(e) => CtValue::failed(Box::new(db_err(e.message))),
            }))
        }
        "DBQueryOne" => {
            let sql = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("DBScope.query_one sql", span))),
            };
            let values = match db_params(args.get(1).unwrap_or(&CtValue::List(vec![])), span) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            let (handle, sql, values) = match db_scope_parts(recv) {
                Some((handle, table, expression, user)) => match wire::jet_db_apply_policy(
                    &sql, &values, &table, &expression, &user,
                ) {
                    Ok((sql, values)) => (handle, sql, values),
                    Err(error) => return Some(Ok(CtValue::failed(Box::new(db_err(error.message))))),
                },
                None => return Some(Ok(CtValue::failed(Box::new(db_err(
                    "database row operations require a policy scope",
                ))))),
            };
            let params = wire::jet_db_encode_params(&values);
            let out = DB::runtime_query(handle, &sql, &params);
            Some(Ok(match wire::jet_db_decode_query_result(&out) {
                Ok(rows) => {
                    let opt = match rows.into_iter().next() {
                        Some(row) => CtValue::Present(Box::new(row_map(row))),
                        None => CtValue::absent(Type::Map {
                            key: Box::new(Type::String),
                            key_span: None,
                            value: Box::new(Type::Named("DBValue".into())),
                        }),
                    };
                    CtValue::Present(Box::new(opt))
                }
                Err(e) => CtValue::failed(Box::new(db_err(e.message))),
            }))
        }
        "DBLive" => {
            let sql = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("DBScope.live sql", span))),
            };
            let values = match db_params(args.get(1).unwrap_or(&CtValue::List(vec![])), span) {
                Ok(values) => values,
                Err(error) => return Some(Err(error)),
            };
            let (handle, table, expression, user) = match db_scope_parts(recv) {
                Some(parts) => parts,
                None => {
                    return Some(Ok(CtValue::failed(Box::new(db_err(
                        "database live queries require a policy scope",
                    )))))
                }
            };
            let (sql, values) = match wire::jet_db_apply_policy(
                &sql, &values, &table, &expression, &user,
            ) {
                Ok(value) => value,
                Err(error) => return Some(Ok(CtValue::failed(Box::new(db_err(error.message))))),
            };
            let out = DB::runtime_query(handle, &sql, &wire::jet_db_encode_params(&values));
            Some(Ok(match wire::jet_db_decode_query_result(&out) {
                Ok(rows) => {
                    let footprint = format!("db:{table}:{sql}");
                    let initial = format!("{rows:?}");
                    match jet_codegen::Comptime::AppLite::apply(
                        "live",
                        &[CtValue::Str(footprint), CtValue::Str(initial)],
                        span,
                    ) {
                        Ok(query) => CtValue::Present(Box::new(query)),
                        Err(error) => return Some(Err(error)),
                    }
                }
                Err(error) => CtValue::failed(Box::new(db_err(error.message))),
            }))
        }
        _ => None,
    }
}

// ── I9 HTTP ambient: marshal CtValue ↔ shared runtime_* Prelude adapters ───

fn http_handle_value(type_name: &str, handle: i64) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: vec![("handle".to_string(), CtValue::Int(handle))],
    }
}

fn http_handle_id(recv: &CtValue, type_name: &str) -> Option<i64> {
    match recv {
        CtValue::Struct {
            type_name: tn,
            fields,
        } if tn == type_name => fields.iter().find_map(|(n, v)| match (n.as_str(), v) {
            ("handle", CtValue::Int(h)) if *h > 0 => Some(*h),
            _ => None,
        }),
        _ => None,
    }
}

fn ct_string_list(v: &CtValue) -> Option<Vec<String>> {
    match v {
        CtValue::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    CtValue::Str(s) => out.push(s.clone()),
                    _ => return None,
                }
            }
            Some(out)
        }
        _ => None,
    }
}

fn ct_cors_origins(v: &CtValue) -> Option<(bool, Vec<String>)> {
    match v {
        CtValue::List(_) => Some((false, ct_string_list(v)?)),
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if type_name == "HTTPCorsOrigins" => match (variant.as_str(), args.as_slice()) {
            ("Any", _) => Some((true, Vec::new())),
            ("List", [(_, list)]) => Some((false, ct_string_list(list)?)),
            _ => None,
        },
        _ => None,
    }
}

fn ambient_http_server_call(
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    match method {
        "mux" if args.is_empty() => Ok(http_handle_value(
            "HTTPMux",
            crate::net_http_rt::runtime_http_mux(),
        )),
        "json" => {
            let status = match args.first() {
                Some(CtValue::Int(n)) => *n,
                _ => return Err(unsupported("core.http.server.json status", span)),
            };
            let body = match args.get(1) {
                Some(CtValue::Str(s)) => s.clone(),
                _ => {
                    return Err(unsupported(
                        "core.http.server.json body (ambient expects JSON text)",
                        span,
                    ))
                }
            };
            Ok(http_handle_value(
                "HTTPResponse",
                crate::net_http_rt::runtime_json_response(status, body),
            ))
        }
        "static_files" => {
            let mux = args
                .first()
                .ok_or_else(|| unsupported("core.http.server.static_files mux", span))?;
            let prefix = match args.get(1) {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Err(unsupported("core.http.server.static_files prefix", span)),
            };
            let root = match args.get(2) {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Err(unsupported("core.http.server.static_files root", span)),
            };
            let bool_option = |index: usize| match args.get(index) {
                Some(CtValue::Bool(value)) => Ok(Some(*value)),
                None => Ok(None),
                _ => Err(unsupported("core.http.server.static_files option", span)),
            };
            let mux_h = http_handle_id(mux, "HTTPMux")
                .ok_or_else(|| unsupported("core.http.server.static_files mux handle", span))?;
            crate::net_http_rt::runtime_static_files(
                mux_h,
                prefix,
                root,
                bool_option(3)?,
                bool_option(4)?,
                bool_option(5)?,
            )
            .map_err(|e| unsupported(&e, span))?;
            Ok(CtValue::Unit)
        }
        "cors_policy" => {
            let origins = args
                .first()
                .ok_or_else(|| unsupported("core.http.server.cors_policy origins", span))?;
            let (origins_any, origin_list) = ct_cors_origins(origins)
                .ok_or_else(|| unsupported("core.http.server.cors_policy origins form", span))?;
            let list_option = |index: usize| match args.get(index) {
                Some(value) => ct_string_list(value)
                    .map(Some)
                    .ok_or_else(|| unsupported("core.http.server.cors_policy list", span)),
                None => Ok(None),
            };
            let credentials = match args.get(3) {
                Some(CtValue::Bool(value)) => Some(*value),
                None => None,
                _ => return Err(unsupported("core.http.server.cors_policy credentials", span)),
            };
            let max_age = match args.get(4) {
                Some(CtValue::Int(value)) => Some(*value),
                None => None,
                _ => return Err(unsupported("core.http.server.cors_policy max_age", span)),
            };
            match crate::net_http_rt::runtime_cors_policy(
                origins_any,
                origin_list,
                list_option(1)?,
                list_option(2)?,
                credentials,
                max_age,
            ) {
                Ok(h) => Ok(CtValue::Present(Box::new(http_handle_value(
                    "HTTPCorsPolicy",
                    h,
                )))),
                Err(error) => Ok(CtValue::failed(Box::new(error.value))),
            }
        }
        "cors" => {
            let mux = args
                .first()
                .ok_or_else(|| unsupported("core.http.server.cors mux", span))?;
            let policy = args
                .get(1)
                .ok_or_else(|| unsupported("core.http.server.cors policy", span))?;
            let mux_h = http_handle_id(mux, "HTTPMux")
                .ok_or_else(|| unsupported("core.http.server.cors mux handle", span))?;
            let policy_h = http_handle_id(policy, "HTTPCorsPolicy")
                .ok_or_else(|| unsupported("core.http.server.cors policy handle", span))?;
            crate::net_http_rt::runtime_cors(mux_h, policy_h).map_err(|e| unsupported(&e, span))?;
            Ok(CtValue::Unit)
        }
        other => Err(unsupported(
            &format!("core.http.server.{other} ambient"),
            span,
        )),
    }
}

fn ambient_http_handle(
    op: &str,
    recv: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if op == "HTTPJSONDecodeError" {
        return Some(Ok(CtValue::failed(Box::new(
            crate::net_http_rt::runtime_http_json_decode_error(),
        ))));
    }
    if !(op.starts_with("HTTPClient:") || op.starts_with("HTTPServer:")) {
        return None;
    }
    let result = match op {
        "HTTPServer:HTTPRequest:json" if args.is_empty() => {
            let request = http_handle_id(recv, "HTTPRequest")
                .ok_or_else(|| unsupported("HTTPRequest.json receiver", span));
            request.and_then(|request| {
                let body = crate::net_http_rt::runtime_http_req_body(request)
                    .map_err(|error| unsupported(&error, span))?;
                let result = crate::net_http_rt::runtime_http_body_json_text(body, None)
                    .map_err(|error| unsupported(&error, span))?;
                Ok(http_json_text_result(result))
            })
        }
        "HTTPClient:HTTPResponse:json" if args.len() <= 1 => {
            let response = http_handle_id(recv, "HTTPResponse")
                .ok_or_else(|| unsupported("HTTPResponse.json receiver", span));
            response.and_then(|response| {
                let body = crate::net_http_rt::runtime_http_resp_body(response)
                    .map_err(|error| unsupported(&error, span))?;
                let limit = match args.first() {
                    Some(CtValue::Int(limit)) => Some(*limit),
                    None => None,
                    _ => return Err(unsupported("HTTPResponse.json limit", span)),
                };
                let result = crate::net_http_rt::runtime_http_body_json_text(body, limit)
                    .map_err(|error| unsupported(&error, span))?;
                Ok(http_json_text_result(result))
            })
        }
        "HTTPClient:HTTPBody:json" | "HTTPServer:HTTPBody:json" if args.len() == 1 => {
            let body = http_handle_id(recv, "HTTPBody")
                .ok_or_else(|| unsupported("HTTPBody.json receiver", span));
            body.and_then(|body| {
                let Some(CtValue::Int(limit)) = args.first() else {
                    return Err(unsupported("HTTPBody.json limit", span));
                };
                let result =
                    crate::net_http_rt::runtime_http_body_json_text(body, Some(*limit))
                        .map_err(|error| unsupported(&error, span))?;
                Ok(http_json_text_result(result))
            })
        }
        "HTTPClient:HTTPRequest:body" if args.len() == 1 => {
            let request = http_handle_id(recv, "HTTPRequest")
                .ok_or_else(|| unsupported("HTTPRequest.body receiver", span));
            request.and_then(|request| {
                let Some(CtValue::Str(body)) = args.first() else {
                    return Err(unsupported("HTTPRequest.body text", span));
                };
                let handle = crate::net_http_rt::runtime_http_request_body(request, body.clone())
                    .map_err(|error| unsupported(&error, span))?;
                Ok(http_handle_value("HTTPRequest", handle))
            })
        }
        _ => Err(unsupported(&format!("HTTP ambient handle `{op}`"), span)),
    };
    Some(result)
}

fn http_json_text_result(result: Result<String, CtValue>) -> CtValue {
    match result {
        Ok(text) => CtValue::Present(Box::new(CtValue::Str(text))),
        Err(error) => CtValue::failed(Box::new(error)),
    }
}
