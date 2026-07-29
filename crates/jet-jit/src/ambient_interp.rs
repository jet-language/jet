//! Whole-program interpreter hosts for `jet.db` / `jet.crypto` (#1254).
//!
//! Same bridge runtimes as Cranelift hosts; CtValue at the boundary. Installed
//! only around `run_whole_interp` so comptime/REPL stay pure / native-denied.

use std::collections::BTreeMap;

use jet_codegen::AST::{CtFloat, CtKey, CtValue, Type};
use jet_codegen::Diagnostics::{Diagnostic, Span};

use crate::Crypto;
use crate::DB;
use crate::IO;

trait JetShow {
    fn jet_show(&self) -> String;
}

mod wire {
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
                        CtValue::Some(Box::new(CtValue::Str("stdin".to_string()))),
                    ),
                    ("os_code".to_string(), CtValue::None(Type::Int)),
                    (
                        "cause".to_string(),
                        CtValue::Some(Box::new(CtValue::Str(cause.into()))),
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

fn db_handle(recv: &CtValue) -> Option<u64> {
    match recv {
        CtValue::Struct { type_name, fields } if type_name == "DBConnection" => fields
            .iter()
            .find_map(|(n, v)| match (n.as_str(), v) {
                ("handle", CtValue::Int(h)) if *h > 0 => Some(*h as u64),
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

fn encode_params(list: &CtValue, span: Span) -> Result<String, Diagnostic> {
    let CtValue::List(items) = list else {
        return Err(unsupported("db params list", span));
    };
    let mut vals = Vec::with_capacity(items.len());
    for item in items {
        vals.push(ct_db_value(item).ok_or_else(|| unsupported("DBValue param", span))?);
    }
    Ok(wire::jet_db_encode_params(&vals))
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
                Ok(item) => CtValue::ResOk(Box::new(CtValue::Str(item))),
                Err(error) => CtValue::ResErr(Box::new(io_error("InvalidInput", error))),
            }))
        }
        ("core.io", "input_secret") => {
            let Some(CtValue::Str(prompt)) = args.first() else {
                return Some(Err(unsupported("core.io.input_secret prompt", span)));
            };
            Some(Ok(match IO::prompt_input_secret(prompt) {
                Ok(secret) => CtValue::ResOk(Box::new(CtValue::Str(secret))),
                Err(error) => {
                    let kind = if error == "secret input needs a terminal" {
                        "InvalidInput"
                    } else {
                        "Other"
                    };
                    CtValue::ResErr(Box::new(io_error(kind, error)))
                }
            }))
        }
        ("jet.db" | "core.db", "open_memory") => Some(Ok(db_conn_value(DB::runtime_open_memory()))),
        ("jet.db" | "core.db", "open") => {
            let path = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("jet.db.open path", span))),
            };
            Some(Ok(db_conn_value(DB::runtime_open(&path))))
        }
        ("jet.crypto", "sha512_bytes") => {
            let data = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Str(Crypto::runtime::jet_crypto_sha512_impl(
                &data,
            ))))
        }
        ("jet.crypto", "blake3_bytes") => {
            let data = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Str(Crypto::runtime::jet_crypto_blake3_impl(
                &data,
            ))))
        }
        ("jet.crypto", "constant_time_equal_bytes") => {
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
        ("jet.crypto", "constant_time_equal") => {
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
        ("jet.crypto", "hkdf_sha256") => {
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
                    Ok(secret) => CtValue::ResOk(Box::new(secret_value(
                        Crypto::runtime::jet_crypto_expert_secret_bytes_impl(&secret),
                    ))),
                    Err(e) => CtValue::ResErr(Box::new(crypto_err(e.to_string()))),
                },
            ))
        }
        ("jet.crypto", "x25519_public") => {
            let secret = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(
                match Crypto::runtime::jet_crypto_x25519_public_impl(&secret) {
                    Ok(pub_bytes) => CtValue::ResOk(Box::new(CtValue::Bytes(pub_bytes))),
                    Err(e) => CtValue::ResErr(Box::new(CtValue::Str(e))),
                },
            ))
        }
        ("jet.crypto", "x25519_shared") => {
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
                    Ok(shared) => CtValue::ResOk(Box::new(CtValue::Bytes(shared))),
                    Err(e) => CtValue::ResErr(Box::new(CtValue::Str(e))),
                },
            ))
        }
        ("jet.crypto", "password_hash") => {
            let password = match to_secret(args.first()?, span) {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(
                match Crypto::runtime::jet_crypto_password_hash_typed_impl(&password) {
                    Ok(ph) => CtValue::ResOk(Box::new(password_hash_value(
                        Crypto::runtime::jet_crypto_password_text_impl(&ph),
                    ))),
                    Err(e) => CtValue::ResErr(Box::new(crypto_err(e.to_string()))),
                },
            ))
        }
        ("jet.crypto", "password_verify") => {
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
                    Ok(b) => CtValue::ResOk(Box::new(CtValue::Bool(b))),
                    Err(e) => CtValue::ResErr(Box::new(crypto_err(e.to_string()))),
                },
            ))
        }
        ("jet.crypto", "__secret_from_text") => {
            let text = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("Secret.from_text", span))),
            };
            let secret = Crypto::runtime::jet_crypto_secret_from_text_impl(text);
            Some(Ok(secret_value(
                Crypto::runtime::jet_crypto_expert_secret_bytes_impl(&secret),
            )))
        }
        ("jet.crypto", "__secret_from_bytes") => {
            let bytes = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let secret = Crypto::runtime::jet_crypto_secret_from_bytes_impl(bytes);
            Some(Ok(secret_value(
                Crypto::runtime::jet_crypto_expert_secret_bytes_impl(&secret),
            )))
        }
        ("jet.crypto", "__x25519_generate") => Some(Ok(
            match Crypto::runtime::jet_crypto_x25519_generate_impl() {
                Ok(key) => CtValue::ResOk(Box::new(x25519_secret_value(
                    Crypto::runtime::jet_crypto_expert_x25519_secret_bytes_impl(&key),
                ))),
                Err(e) => CtValue::ResErr(Box::new(crypto_err(e.to_string()))),
            },
        )),
        ("jet.crypto", "__x25519_public") => {
            let bytes = match struct_bytes(args.first()?, "X25519SecretKey", span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            match Crypto::runtime::jet_crypto_x25519_public_impl(&bytes) {
                Ok(pub_bytes) => Some(Ok(x25519_public_value(pub_bytes))),
                Err(e) => Some(Err(unsupported(&e, span))),
            }
        }
        ("jet.crypto", "__password_text") => {
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
        ("jet.crypto", "file_seal") => {
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
                                return Some(Ok(CtValue::ResErr(Box::new(crypto_err(
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
                    Ok(()) => CtValue::ResOk(Box::new(CtValue::Unit)),
                    Err(e) => CtValue::ResErr(Box::new(crypto_err(e.to_string()))),
                },
            ))
        }
        ("jet.crypto", "file_open") => {
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
                        Ok(()) => CtValue::ResOk(Box::new(CtValue::Unit)),
                        Err(e) => CtValue::ResErr(Box::new(crypto_err(e.to_string()))),
                    }
                }
                Err(e) => CtValue::ResErr(Box::new(crypto_err(e))),
            }))
        }
        _ => None,
    }
}

pub fn ambient_handle(
    op: &str,
    recv: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if let Some(result) = ambient_http_handle(op, recv, args, span) {
        return Some(result);
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
                _ => return Some(Err(unsupported("DBConnection.execute sql", span))),
            };
            let params = match encode_params(args.get(1).unwrap_or(&CtValue::List(vec![])), span) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            let out = DB::runtime_execute(handle, &sql, &params);
            Some(Ok(match wire::jet_db_decode_execute_result(&out) {
                Ok(n) => CtValue::ResOk(Box::new(CtValue::Int(n))),
                Err(e) => CtValue::ResErr(Box::new(db_err(e.message))),
            }))
        }
        "DBQuery" => {
            let sql = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("DBConnection.query sql", span))),
            };
            let params = match encode_params(args.get(1).unwrap_or(&CtValue::List(vec![])), span) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            let out = DB::runtime_query(handle, &sql, &params);
            Some(Ok(match wire::jet_db_decode_query_result(&out) {
                Ok(rows) => CtValue::ResOk(Box::new(CtValue::List(
                    rows.into_iter().map(row_map).collect(),
                ))),
                Err(e) => CtValue::ResErr(Box::new(db_err(e.message))),
            }))
        }
        "DBQueryOne" => {
            let sql = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("DBConnection.query_one sql", span))),
            };
            let params = match encode_params(args.get(1).unwrap_or(&CtValue::List(vec![])), span) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            let out = DB::runtime_query(handle, &sql, &params);
            Some(Ok(match wire::jet_db_decode_query_result(&out) {
                Ok(rows) => {
                    let opt = match rows.into_iter().next() {
                        Some(row) => CtValue::Some(Box::new(row_map(row))),
                        None => CtValue::None(Type::Map {
                            key: Box::new(Type::String),
                            key_span: None,
                            value: Box::new(Type::Named("DBValue".into())),
                        }),
                    };
                    CtValue::ResOk(Box::new(opt))
                }
                Err(e) => CtValue::ResErr(Box::new(db_err(e.message))),
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
                Ok(h) => Ok(CtValue::ResOk(Box::new(http_handle_value(
                    "HTTPCorsPolicy",
                    h,
                )))),
                Err(error) => Ok(CtValue::ResErr(Box::new(error.value))),
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
        return Some(Ok(CtValue::ResErr(Box::new(
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
        Ok(text) => CtValue::ResOk(Box::new(CtValue::Str(text))),
        Err(error) => CtValue::ResErr(Box::new(error)),
    }
}
