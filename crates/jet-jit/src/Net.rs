//! Native JIT adapters for `core.url` / `core.mime` / `core.browser` (and later
//! net/http/email/ws). Algorithms come from the same prelude sources AOT emits.

#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use crate::Marshal::{alloc_string, clone_string, result_err_msg, result_ok};

pub(crate) mod runtime {
    use crate::JetShow;

    pub mod jet_std {
        #[derive(Clone, Debug)]
        pub struct JetURL {
            pub scheme: String,
            pub username: Option<String>,
            pub password: Option<String>,
            pub host: Option<String>,
            pub port: Option<i64>,
            pub path: String,
            pub query: Vec<(String, String)>,
            pub fragment: Option<String>,
            pub typed_host: Option<Vec<(String, bool)>>,
            pub typed_path: Option<Vec<(String, bool)>>,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub struct JetMIME {
            pub top: String,
            pub sub: String,
            pub params: Vec<(String, String)>,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub struct JSONError {
            pub line: i64,
            pub message: String,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub enum JSON {
            Null,
            Boolean(bool),
            Number(f64),
            Integer(i64),
            Text(String),
            Array(Vec<JSON>),
            Object(std::collections::BTreeMap<String, JSON>),
        }

        #[allow(unused_imports)]
        pub use jet_foundation::Outcome::*;
        include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/UrlMime.rs");
        #[allow(unused_imports)]
        pub use jet_foundation::Outcome::*;
        include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/JSONCodec.rs");
    }

    fn jet_deadline_remaining_ms() -> Option<i64> {
        None
    }

    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/Browser.rs");

    include!("../../jet-codegen/src/Prelude/CoreLib/Top/SHA256Raw.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Email.rs");

    #[allow(dead_code)]
    pub(crate) mod tls {
        include!("../../jet-pkg-model/src/Prelude/NetTls.rs");
    }

    pub use jet_std::{JetMIME, JetURL};

    pub(crate) fn email_cancelled() -> bool {
        jet_codegen::scheduler::jet_scheduler_task_cancelled()
    }
    pub(crate) fn email_remaining_ms() -> Option<i64> {
        jet_codegen::scheduler::jet_ctx_deadline_ms()
            .map(|deadline| deadline.saturating_sub(jet_codegen::scheduler::jet_std_time_now()))
    }

    pub fn email_runtime() -> jet_email::RuntimeFns {
        jet_email::RuntimeFns {
            tls_begin: tls::jet_net_tls_begin_impl,
            tls_begin_ca: tls::jet_net_tls_begin_with_ca_impl,
            tls_handshake_step: tls::jet_net_tls_handshake_step_impl,
            tls_set_poll_timeout: tls::jet_net_tls_set_poll_timeout_impl,
            tls_read: tls::jet_net_tls_read_bytes_impl,
            tls_write_all: tls::jet_net_tls_write_all_bytes_impl,
            tls_close: tls::jet_net_tls_close_impl,
            wipe: crate::Crypto::runtime::jet_crypto_zeroize_email_impl,
            sha256: crate::Crypto::runtime::jet_crypto_email_sha256_impl,
            ed25519_sign: crate::Crypto::runtime::jet_crypto_email_ed25519_sign_impl,
            cancelled: email_cancelled,
            remaining_ms: email_remaining_ms,
            accepted_at: jet_email::runtime_now,
        }
    }

    pub fn url_parse(s: &String) -> Result<JetURL, String> {
        JetURL::parse(s)
    }
    pub fn url_typed_literal(
        literals: &Vec<String>,
        holes: &Vec<String>,
    ) -> JetURL {
        let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
        jet_std::jet_typed_url_literal(&literal_refs, holes.clone())
    }
    pub fn url_file(path: &String) -> JetURL {
        JetURL::file(path)
    }
    pub fn url_data(mime: &JetMIME, text: &String) -> JetURL {
        JetURL::data(mime, text)
    }
    pub fn url_query(pairs: &Vec<Vec<String>>) -> String {
        let rows: Vec<(String, String)> = pairs
            .iter()
            .filter(|r| !r.is_empty())
            .map(|r| {
                (
                    r.first().cloned().unwrap_or_default(),
                    r.get(1).cloned().unwrap_or_default(),
                )
            })
            .collect();
        rows.iter()
            .map(|(k, v)| {
                format!(
                    "{}={}",
                    jet_std::jet_url_percent_encode(k, false),
                    jet_std::jet_url_percent_encode(v, false)
                )
            })
            .collect::<Vec<_>>()
            .join("&")
    }
    pub fn url_percent_encode(s: &String) -> String {
        jet_std::jet_url_percent_encode(s, false)
    }
    pub fn url_percent_decode(s: &String) -> Result<String, String> {
        jet_std::jet_url_percent_decode_str(s)
    }
    pub fn mime_parse(s: &String) -> Result<JetMIME, String> {
        JetMIME::parse(s)
    }
    pub fn mime_from_extension(ext: &String) -> Option<String> {
        jet_std::jet_mime_from_extension(ext).map(|s| s.to_string())
    }
    pub fn mime_extension(mime: &String) -> Option<String> {
        jet_std::jet_extension_from_mime(mime).map(|s| s.to_string())
    }

    pub fn browser_profile(name: &String) -> Result<(String, String), String> {
        match jet_browser_profile(name) {
            Ok(profile) => Ok((profile.name.clone(), profile.version.clone())),
            Err(err) => Err(err.jet_show()),
        }
    }

    pub fn browser_timeout(ms: i64) -> Result<i64, String> {
        match jet_browser_timeout(ms) {
            Ok(timeout) => Ok(timeout.milliseconds),
            Err(err) => Err(err.jet_show()),
        }
    }
}

pub(crate) fn email_runtime_fns() -> jet_codegen::Comptime::EmailAdapter::RuntimeFns {
    jet_codegen::Comptime::EmailAdapter::RuntimeFns {
        tls_begin: runtime::tls::jet_net_tls_begin_impl,
        tls_begin_ca: runtime::tls::jet_net_tls_begin_with_ca_impl,
        tls_handshake_step: runtime::tls::jet_net_tls_handshake_step_impl,
        tls_set_poll_timeout: runtime::tls::jet_net_tls_set_poll_timeout_impl,
        tls_read: runtime::tls::jet_net_tls_read_bytes_impl,
        tls_write_all: runtime::tls::jet_net_tls_write_all_bytes_impl,
        tls_close: runtime::tls::jet_net_tls_close_impl,
        wipe: crate::Crypto::runtime::jet_crypto_zeroize_email_impl,
        sha256: crate::Crypto::runtime::jet_crypto_email_sha256_impl,
        ed25519_sign: crate::Crypto::runtime::jet_crypto_email_ed25519_sign_impl,
        cancelled: runtime::email_cancelled,
        remaining_ms: runtime::email_remaining_ms,
        accepted_at: runtime::jet_email::runtime_now,
    }
}

pub(crate) enum NetValue {
    Url(runtime::JetURL),
    Mime(runtime::JetMIME),
    EmailAddress(runtime::jet_email::Address),
    EmailAttachment(runtime::jet_email::Attachment),
    EmailMessage(runtime::jet_email::Message),
    EmailMailer(runtime::jet_email::Mailer),
}

fn push(value: NetValue) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.net_values.push(Some(value));
        rt.net_values.len() as i64
    })
}

fn with_net<R>(handle: i64, f: impl FnOnce(&NetValue) -> Option<R>) -> Option<R> {
    if handle <= 0 {
        return None;
    }
    Concurrency::with_runtime_mut(|rt| {
        let index = handle.saturating_sub(1) as usize;
        rt.net_values
            .get(index)
            .and_then(|slot| slot.as_ref())
            .and_then(f)
    })
}

fn take_net(handle: i64) -> Option<NetValue> {
    if handle <= 0 {
        return None;
    }
    Concurrency::with_runtime_mut(|rt| {
        let index = handle.saturating_sub(1) as usize;
        rt.net_values.get_mut(index).and_then(Option::take)
    })
}

fn put_net(handle: i64, value: NetValue) {
    if handle <= 0 {
        return;
    }
    Concurrency::with_runtime_mut(|rt| {
        let index = handle.saturating_sub(1) as usize;
        if let Some(slot) = rt.net_values.get_mut(index) {
            *slot = Some(value);
        }
    });
}

pub(crate) fn show_value(rt: &crate::JitRuntime, handle: i64) -> String {
    if handle <= 0 {
        return String::new();
    }
    let index = handle.saturating_sub(1) as usize;
    rt.net_values
        .get(index)
        .and_then(|slot| slot.as_ref())
        .map(|value| match value {
            NetValue::Url(url) => url.to_string_value(),
            NetValue::Mime(mime) => mime.to_string_value(),
            _ => String::new(),
        })
        .unwrap_or_default()
}
pub(crate) fn mime_parts(
    handle: i64,
) -> Option<(String, String, Vec<(String, String)>)> {
    with_net(handle, |value| match value {
        NetValue::Mime(mime) => Some((
            mime.top.clone(),
            mime.sub.clone(),
            mime.params.clone(),
        )),
        _ => None,
    })
}

fn result_err(msg: String) -> i64 {
    result_err_msg(&msg)
}

fn option_string(s: Option<String>) -> i64 {
    match s {
        None => 0,
        Some(v) => alloc_string(v).wrapping_add(1),
    }
}

fn list_of_strings(rows: Vec<String>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for row in rows {
            let sid = rt.heap.alloc_string(row);
            let _ = rt.heap.list_push_int(list, sid);
        }
        list
    })
}

fn list_of_string_pairs(rows: Vec<Vec<String>>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let outer = rt.heap.alloc_empty_list();
        for row in rows {
            let inner = rt.heap.alloc_empty_list();
            for cell in row {
                let sid = rt.heap.alloc_string(cell);
                let _ = rt.heap.list_push_int(inner, sid);
            }
            let _ = rt.heap.list_push_int(outer, inner);
        }
        outer
    })
}

fn read_string_pair_list(list: i64) -> Vec<Vec<String>> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let inner = rt.heap.list_get_int(list, i).unwrap_or(0);
            let inner_len = rt.heap.list_len(inner).unwrap_or(0);
            let mut row = Vec::with_capacity(inner_len as usize);
            for j in 0..inner_len {
                let sid = rt.heap.list_get_int(inner, j).unwrap_or(0);
                row.push(rt.heap.clone_string(sid).unwrap_or_default());
            }
            out.push(row);
        }
        out
    })
}

fn profile_record(name: String, version: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(2);
        let name = rt.heap.alloc_string(name);
        let version = rt.heap.alloc_string(version);
        let _ = rt.heap.record_set_string(record, 0, name);
        let _ = rt.heap.record_set_string(record, 1, version);
        record
    })
}

fn timeout_record(milliseconds: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(1);
        let _ = rt.heap.record_set_int(record, 0, milliseconds);
        record
    })
}

extern "C" fn jet_jit_url_parse(s: i64) -> i64 {
    let text = clone_string(s);
    match runtime::url_parse(&text) {
        Ok(url) => result_ok(push(NetValue::Url(url)) as u64),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_url_typed_literal(literals: i64, holes: i64) -> i64 {
    let literals = list_strings(literals).unwrap_or_default();
    let holes = list_strings(holes).unwrap_or_default();
    push(NetValue::Url(runtime::url_typed_literal(&literals, &holes)))
}

extern "C" fn jet_jit_url_file(path: i64) -> i64 {
    let path = clone_string(path);
    push(NetValue::Url(runtime::url_file(&path)))
}

extern "C" fn jet_jit_url_data(mime: i64, text: i64) -> i64 {
    let text = clone_string(text);
    let Some(mime) = with_net(mime, |v| match v {
        NetValue::Mime(m) => Some(m.clone()),
        _ => None,
    }) else {
        return 0;
    };
    push(NetValue::Url(runtime::url_data(&mime, &text)))
}

extern "C" fn jet_jit_url_query(pairs: i64) -> i64 {
    let pairs = read_string_pair_list(pairs);
    alloc_string(runtime::url_query(&pairs))
}

extern "C" fn jet_jit_url_percent_encode(s: i64) -> i64 {
    let s = clone_string(s);
    alloc_string(runtime::url_percent_encode(&s))
}

extern "C" fn jet_jit_url_percent_decode(s: i64) -> i64 {
    let s = clone_string(s);
    match runtime::url_percent_decode(&s) {
        Ok(text) => result_ok(alloc_string(text) as u64),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_mime_parse(s: i64) -> i64 {
    let text = clone_string(s);
    match runtime::mime_parse(&text) {
        Ok(mime) => result_ok(push(NetValue::Mime(mime)) as u64),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_mime_from_extension(ext: i64) -> i64 {
    let ext = clone_string(ext);
    option_string(runtime::mime_from_extension(&ext))
}

extern "C" fn jet_jit_mime_extension(mime: i64) -> i64 {
    let mime = clone_string(mime);
    option_string(runtime::mime_extension(&mime))
}

extern "C" fn jet_jit_url_to_string(recv: i64) -> i64 {
    let Some(text) = with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.to_string_value()),
        NetValue::Mime(m) => Some(m.to_string_value()),
        _ => None,
    }) else {
        return alloc_string(String::new());
    };
    alloc_string(text)
}

extern "C" fn jet_jit_url_scheme(recv: i64) -> i64 {
    let Some(text) = with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.scheme()),
        _ => None,
    }) else {
        return alloc_string(String::new());
    };
    alloc_string(text)
}

extern "C" fn jet_jit_url_host(recv: i64) -> i64 {
    option_string(with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.host()),
        _ => None,
    }).and_then(|r| r.ok()))
}

extern "C" fn jet_jit_url_path(recv: i64) -> i64 {
    let Some(text) = with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.path()),
        _ => None,
    }) else {
        return alloc_string(String::new());
    };
    alloc_string(text)
}

extern "C" fn jet_jit_url_query_value(recv: i64) -> i64 {
    let Some(text) = with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.query()),
        _ => None,
    }) else {
        return alloc_string(String::new());
    };
    alloc_string(text)
}

extern "C" fn jet_jit_url_query_pairs(recv: i64) -> i64 {
    let Some(pairs) = with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.query_pairs()),
        _ => None,
    }) else {
        return list_of_string_pairs(Vec::new());
    };
    list_of_string_pairs(pairs)
}

extern "C" fn jet_jit_url_path_segments(recv: i64) -> i64 {
    let Some(segs) = with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.path_segments()),
        _ => None,
    }) else {
        return list_of_strings(Vec::new());
    };
    list_of_strings(segs)
}

extern "C" fn jet_jit_url_fragment(recv: i64) -> i64 {
    option_string(with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.fragment()),
        _ => None,
    }).and_then(|r| r.ok()))
}

extern "C" fn jet_jit_url_username(recv: i64) -> i64 {
    let Some(text) = with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.username()),
        _ => None,
    }) else {
        return alloc_string(String::new());
    };
    alloc_string(text)
}

extern "C" fn jet_jit_url_password(recv: i64) -> i64 {
    let Some(text) = with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.password()),
        _ => None,
    }) else {
        return alloc_string(String::new());
    };
    alloc_string(text)
}

extern "C" fn jet_jit_url_userinfo(recv: i64) -> i64 {
    let Some(text) = with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.userinfo()),
        _ => None,
    }) else {
        return alloc_string(String::new());
    };
    alloc_string(text)
}

extern "C" fn jet_jit_url_authority(recv: i64) -> i64 {
    let Some(text) = with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.authority()),
        _ => None,
    }) else {
        return alloc_string(String::new());
    };
    alloc_string(text)
}

extern "C" fn jet_jit_url_default_port(recv: i64) -> i64 {
    match with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.default_port()),
        _ => None,
    })
    .and_then(|r| r.ok())
    {
        Some(v) => v.wrapping_add(1),
        None => 0,
    }
}

extern "C" fn jet_jit_url_port(recv: i64) -> i64 {
    match with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.port()),
        _ => None,
    })
    .and_then(|r| r.ok())
    {
        Some(v) => v.wrapping_add(1),
        None => 0,
    }
}

extern "C" fn jet_jit_url_normalize(recv: i64) -> i64 {
    let Some(url) = with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.normalize()),
        _ => None,
    }) else {
        return 0;
    };
    push(NetValue::Url(url))
}

extern "C" fn jet_jit_url_join(recv: i64, rel: i64) -> i64 {
    let rel = clone_string(rel);
    let Some(url) = with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.clone()),
        _ => None,
    }) else {
        return result_err("bad url".into());
    };
    match url.join(&rel) {
        Ok(joined) => result_ok(push(NetValue::Url(joined)) as u64),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_url_set_query(recv: i64, key: i64, value: i64) -> i64 {
    let key = clone_string(key);
    let value = clone_string(value);
    let Some(url) = with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.set_query(&key, &value)),
        _ => None,
    }) else {
        return 0;
    };
    push(NetValue::Url(url))
}

extern "C" fn jet_jit_url_add_query(recv: i64, key: i64, value: i64) -> i64 {
    let key = clone_string(key);
    let value = clone_string(value);
    let Some(url) = with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.add_query(&key, &value)),
        _ => None,
    }) else {
        return 0;
    };
    push(NetValue::Url(url))
}

extern "C" fn jet_jit_mime_essence(recv: i64) -> i64 {
    let Some(text) = with_net(recv, |v| match v {
        NetValue::Mime(m) => Some(m.essence()),
        _ => None,
    }) else {
        return alloc_string(String::new());
    };
    alloc_string(text)
}

extern "C" fn jet_jit_mime_param(recv: i64, name: i64) -> i64 {
    let name = clone_string(name);
    option_string(with_net(recv, |v| match v {
        NetValue::Mime(m) => Some(m.param(&name)),
        _ => None,
    }).and_then(|r| r.ok()))
}

extern "C" fn jet_jit_browser_profile(name: i64) -> i64 {
    let name = clone_string(name);
    match runtime::browser_profile(&name) {
        Ok((name, version)) => result_ok(profile_record(name, version) as u64),
        Err(err) => result_err(err),
    }
}

extern "C" fn jet_jit_browser_timeout(ms: i64) -> i64 {
    match runtime::browser_timeout(ms) {
        Ok(milliseconds) => result_ok(timeout_record(milliseconds) as u64),
        Err(err) => result_err(err),
    }
}

fn record_get_i64(record: i64, idx: i64) -> Option<i64> {
    Concurrency::with_runtime_mut(|rt| rt.heap.record_get_int(record, idx))
}

fn record_get_heap_string(record: i64, idx: i64) -> Option<String> {
    Concurrency::with_runtime_mut(|rt| {
        let sid = rt.heap.record_get_string(record, idx)?;
        rt.heap.clone_string(sid)
    })
}

fn record_get_packed_string(record: i64, idx: i64) -> Option<String> {
    let sid = record_get_i64(record, idx)?;
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(sid))
}

fn list_strings(list: i64) -> Option<Vec<String>> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list)?;
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let sid = rt.heap.list_get_int(list, i)?;
            out.push(rt.heap.clone_string(sid)?);
        }
        Some(out)
    })
}

fn list_bytes(list: i64) -> Option<Vec<u8>> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list)?;
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let value = rt.heap.list_get_int(list, i)?;
            if !(0..=255).contains(&value) {
                return None;
            }
            out.push(value as u8);
        }
        Some(out)
    })
}

fn email_err(err: runtime::jet_email::Error) -> i64 {
    let (_variant, disc, operation, server, code, reason) = runtime::jet_email::error_parts(err);
    Concurrency::with_runtime_mut(|rt| {
        let payload = rt.heap.alloc_record(5);
        let _ = rt.heap.record_set_int(payload, 0, disc);
        let operation = rt.heap.alloc_string(operation);
        let _ = rt.heap.record_set_int(payload, 1, operation);
        let server = server.map_or(0, |server| rt.heap.alloc_string(server) + 1);
        let _ = rt.heap.record_set_int(payload, 2, server);
        let code = code.map_or(0, |code| code + 1);
        let _ = rt.heap.record_set_int(payload, 3, code);
        let reason = rt.heap.alloc_string(reason);
        let _ = rt.heap.record_set_int(payload, 4, reason);
        crate::runtime_host::alloc_jit_result(rt, false, payload as u64)
    })
}

fn email_config_error(operation: &str, reason: &str) -> i64 {
    email_err(runtime::jet_email::configuration_error(operation, reason))
}

fn unpack_limits(handle: i64) -> Option<runtime::jet_email::Limits> {
    Some(runtime::jet_email::Limits {
        max_reply_line_bytes: record_get_i64(handle, 0)?,
        max_reply_lines: record_get_i64(handle, 1)?,
        max_capabilities: record_get_i64(handle, 2)?,
        max_recipients: record_get_i64(handle, 3)?,
        max_message_bytes: record_get_i64(handle, 4)?,
        max_auth_challenge_bytes: record_get_i64(handle, 5)?,
    })
}

fn unpack_dkim(handle: i64) -> Option<runtime::jet_email::DkimConfig<Vec<u8>>> {
    let domain = record_get_heap_string(handle, 0)?;
    let selector = record_get_heap_string(handle, 1)?;
    let key_handle = record_get_i64(handle, 2)?;
    let private_key = crate::Crypto::secret_copy_for_smtp(key_handle)?;
    let headers = list_strings(record_get_i64(handle, 3)?)?;
    Some(runtime::jet_email::DkimConfig {
        domain,
        selector,
        private_key,
        signed_headers: headers,
    })
}

fn unpack_auth(handle: i64) -> Option<runtime::jet_email::SMTPAuth<Vec<u8>>> {
    let disc = record_get_i64(handle, 0)?;
    if disc == 0 {
        return Some(runtime::jet_email::SMTPAuth::None);
    }
    if disc == 1 {
        let username = record_get_packed_string(handle, 1)?;
        let password_handle = record_get_i64(handle, 2)?;
        let password = crate::Crypto::secret_copy_for_smtp(password_handle)?;
        return Some(runtime::jet_email::SMTPAuth::Password { username, password });
    }
    None
}

fn unpack_smtp_config(config: i64) -> Option<runtime::jet_email::SMTPConfig<Vec<u8>>> {
    let host = record_get_heap_string(config, 0)?;
    let port = record_get_i64(config, 1)?;
    let security = match record_get_i64(config, 2)? {
        0 => runtime::jet_email::SMTPSecurity::StartTls,
        1 => runtime::jet_email::SMTPSecurity::TLS,
        _ => return None,
    };
    let auth_raw = record_get_i64(config, 3)?;
    let auth = if auth_raw == 0 {
        runtime::jet_email::SMTPAuth::None
    } else {
        unpack_auth(auth_raw)?
    };
    let recipient_policy = match record_get_i64(config, 4)? {
        0 => runtime::jet_email::RecipientPolicy::RequireAll,
        1 => runtime::jet_email::RecipientPolicy::DeliverAccepted,
        _ => return None,
    };
    let trust_raw = record_get_i64(config, 5)?;
    let trust = if trust_raw == 0 {
        runtime::jet_email::TLSTrust::System
    } else {
        if record_get_i64(trust_raw, 0)? != 1 {
            return None;
        }
        runtime::jet_email::TLSTrust::SystemPlusCa {
            pem: list_bytes(record_get_i64(trust_raw, 1)?)?,
        }
    };
    let limits = unpack_limits(record_get_i64(config, 6)?)?;
    let dkim_opt = record_get_i64(config, 7)?;
    let dkim = if dkim_opt == 0 {
        Err(JetAbsent)
    } else {
        jet_outcome_of(unpack_dkim(dkim_opt - 1))
    };
    Some(runtime::jet_email::SMTPConfig {
        host,
        port,
        security,
        auth,
        recipient_policy,
        trust,
        limits,
        dkim,
    })
}

fn email_address_list_handle(addresses: &[runtime::jet_email::Address]) -> i64 {
    let list = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_list());
    for address in addresses {
        let handle = push(NetValue::EmailAddress(address.clone()));
        Concurrency::with_runtime_mut(|rt| {
            let _ = rt.heap.list_push_int(list, handle);
        });
    }
    list
}

fn email_envelope_handle(envelope: &runtime::jet_email::Envelope) -> i64 {
    let handle = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_record(2));
    let from = push(NetValue::EmailAddress(envelope.from.clone()));
    let recipients = email_address_list_handle(&envelope.recipients);
    Concurrency::with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_int(handle, 0, from);
        let _ = rt.heap.record_set_int(handle, 1, recipients);
    });
    handle
}

fn email_address_from_handle(handle: i64) -> Option<runtime::jet_email::Address> {
    with_net(handle, |value| match value {
        NetValue::EmailAddress(address) => Some(address.clone()),
        _ => None,
    })
}

fn email_address_list_from_handle(
    list: i64,
) -> Option<Vec<runtime::jet_email::Address>> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list)?;
        let mut addresses = Vec::with_capacity(len as usize);
        for index in 0..len {
            let handle = rt.heap.list_get_int(list, index)?;
            if handle <= 0 {
                return None;
            }
            let value = rt
                .net_values
                .get(handle.saturating_sub(1) as usize)
                .and_then(|slot| slot.as_ref())?;
            let NetValue::EmailAddress(address) = value else {
                return None;
            };
            addresses.push(address.clone());
        }
        Some(addresses)
    })
}

fn email_envelope_from_handle(handle: i64) -> Option<runtime::jet_email::Envelope> {
    let from = record_get_i64(handle, 0).and_then(email_address_from_handle)?;
    let recipients = record_get_i64(handle, 1).and_then(email_address_list_from_handle)?;
    Some(runtime::jet_email::Envelope { from, recipients })
}

fn email_attachment_list_from_handle(
    list: i64,
) -> Option<Vec<runtime::jet_email::Attachment>> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list)?;
        let mut attachments = Vec::with_capacity(len as usize);
        for index in 0..len {
            let handle = rt.heap.list_get_int(list, index)?;
            if handle <= 0 {
                return None;
            }
            let value = rt
                .net_values
                .get(handle.saturating_sub(1) as usize)
                .and_then(|slot| slot.as_ref())?;
            let NetValue::EmailAttachment(attachment) = value else {
                return None;
            };
            attachments.push(attachment.clone());
        }
        Some(attachments)
    })
}

fn email_string(handle: i64) -> Option<String> {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(handle))
}

fn email_message_from_handle(handle: i64) -> Option<runtime::jet_email::Message> {
    with_net(handle, |value| match value {
        NetValue::EmailMessage(message) => Some(message.clone()),
        _ => None,
    })
}

fn email_recipient_report_handle(report: runtime::jet_email::RecipientReport) -> i64 {
    let runtime::jet_email::RecipientReport {
        address,
        accepted,
        code,
        message,
    } = report;
    let handle = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_record(4));
    let address = push(NetValue::EmailAddress(address));
    let message = alloc_string(message);
    Concurrency::with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_int(handle, 0, address);
        let _ = rt.heap.record_set_bool(handle, 1, accepted);
        let _ = rt.heap.record_set_int(handle, 2, code);
        let _ = rt.heap.record_set_string(handle, 3, message);
    });
    handle
}

fn email_recipient_reports_handle(reports: Vec<runtime::jet_email::RecipientReport>) -> i64 {
    let list = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_list());
    for report in reports {
        let handle = email_recipient_report_handle(report);
        Concurrency::with_runtime_mut(|rt| {
            let _ = rt.heap.list_push_int(list, handle);
        });
    }
    list
}

fn email_send_report_handle(report: runtime::jet_email::SendReport) -> i64 {
    let runtime::jet_email::SendReport {
        server,
        accepted,
        rejected,
        response_code,
        response,
        accepted_at,
    } = report;
    let handle = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_record(6));
    let server = alloc_string(server);
    let accepted = email_recipient_reports_handle(accepted);
    let rejected = email_recipient_reports_handle(rejected);
    let response = alloc_string(response);
    let accepted_at = alloc_string(accepted_at);
    Concurrency::with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_string(handle, 0, server);
        let _ = rt.heap.record_set_int(handle, 1, accepted);
        let _ = rt.heap.record_set_int(handle, 2, rejected);
        let _ = rt.heap.record_set_int(handle, 3, response_code);
        let _ = rt.heap.record_set_string(handle, 4, response);
        let _ = rt.heap.record_set_string(handle, 5, accepted_at);
    });
    handle
}

extern "C" fn jet_jit_email_address(text: i64) -> i64 {
    let Some(text) = email_string(text) else {
        return email_config_error("address", "invalid address text");
    };
    match runtime::jet_email::address(&text) {
        Ok(addr) => result_ok(push(NetValue::EmailAddress(addr)) as u64),
        Err(err) => {
            email_err(err)
        }
    }
}

extern "C" fn jet_jit_email_attachment(filename: i64, mime: i64, bytes: i64) -> i64 {
    let Some(filename) = email_string(filename) else {
        return email_config_error("attachment", "invalid attachment filename");
    };
    let Some(mime) = email_string(mime) else {
        return email_config_error("attachment", "invalid attachment content type");
    };
    let Some(bytes) = list_bytes(bytes) else {
        return email_config_error("attachment", "invalid attachment bytes");
    };
    match runtime::jet_email::attachment(&filename, &mime, &bytes) {
        Ok(att) => {
            let h = push(NetValue::EmailAttachment(att));
            result_ok(h as u64)
        }
        Err(err) => {
            email_err(err)
        }
    }
}

extern "C" fn jet_jit_email_message(
    from: i64,
    to: i64,
    bcc: i64,
    subject: i64,
    text: i64,
    html: i64,
    attachments: i64,
) -> i64 {
    let from = match email_address_from_handle(from) {
        Some(a) => a,
        None => {
            return email_config_error("message", "invalid sender address");
        }
    };
    let to = email_address_list_from_handle(to);
    let Some(to) = to else {
        return email_config_error("message", "invalid visible recipient list");
    };
    let bcc = email_address_list_from_handle(bcc);
    let Some(bcc) = bcc else {
        return email_config_error("message", "invalid blind-copy recipient list");
    };
    let Some(subject) = email_string(subject) else {
        return email_config_error("message", "invalid subject text");
    };
    let Some(text) = email_string(text) else {
        return email_config_error("message", "invalid plain-text body");
    };
    let Some(html) = email_string(html) else {
        return email_config_error("message", "invalid HTML body");
    };
    let attachments = email_attachment_list_from_handle(attachments);
    let Some(attachments) = attachments else {
        return email_config_error("message", "invalid attachment list");
    };
    match runtime::jet_email::message(
        &from, &to, &bcc, &subject, &text, &html, &attachments,
    ) {
        Ok(msg) => {
            result_ok(push(NetValue::EmailMessage(msg)) as u64)
        }
        Err(err) => {
            email_err(err)
        }
    }
}

extern "C" fn jet_jit_email_envelope(from: i64, recipients: i64) -> i64 {
    let Some(from) = email_address_from_handle(from) else {
        return email_config_error("envelope", "invalid sender address");
    };
    let Some(recipients) = email_address_list_from_handle(recipients) else {
        return email_config_error("envelope", "invalid recipient list");
    };
    match runtime::jet_email::envelope(&from, &recipients) {
        Ok(envelope) => result_ok(email_envelope_handle(&envelope) as u64),
        Err(error) => email_err(error),
    }
}

extern "C" fn jet_jit_email_serialize(message: i64) -> i64 {
    let msg = match email_message_from_handle(message) {
        Some(m) => m,
        None => {
            return email_config_error("serialize", "invalid message");
        }
    };
    match runtime::jet_email::serialize(&msg) {
        Ok(bytes) => {
            let list = Concurrency::with_runtime_mut(|rt| {
                let list = rt.heap.alloc_empty_list();
                for b in bytes {
                    let _ = rt.heap.list_push_int(list, i64::from(b));
                }
                list
            });
            result_ok(list as u64)
        }
        Err(err) => {
            email_err(err)
        }
    }
}

extern "C" fn jet_jit_email_limits_safe() -> i64 {
    let limits = runtime::jet_email::Limits::safe();
    let handle = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_record(6));
    let fields = [
        limits.max_reply_line_bytes,
        limits.max_reply_lines,
        limits.max_capabilities,
        limits.max_recipients,
        limits.max_message_bytes,
        limits.max_auth_challenge_bytes,
    ];
    Concurrency::with_runtime_mut(|rt| {
        for (index, value) in fields.into_iter().enumerate() {
            let _ = rt.heap.record_set_int(handle, index as i64, value);
        }
    });
    handle
}

extern "C" fn jet_jit_email_smtp_from_env() -> i64 {
    match runtime::jet_email::smtp_from_env(runtime::email_runtime()) {
        Ok(mailer) => result_ok(push(NetValue::EmailMailer(mailer)) as u64),
        Err(error) => email_err(error),
    }
}

extern "C" fn jet_jit_email_message_envelope(message: i64) -> i64 {
    let Some(message) = email_message_from_handle(message) else {
        return email_config_error("envelope", "invalid message");
    };
    email_envelope_handle(message.envelope())
}

extern "C" fn jet_jit_email_message_with_envelope(message: i64, envelope: i64) -> i64 {
    let Some(message) = email_message_from_handle(message) else {
        return email_config_error("with_envelope", "invalid message");
    };
    let Some(envelope) = email_envelope_from_handle(envelope) else {
        return email_config_error("with_envelope", "invalid envelope");
    };
    match message.with_envelope(&envelope) {
        Ok(message) => result_ok(push(NetValue::EmailMessage(message)) as u64),
        Err(error) => email_err(error),
    }
}

extern "C" fn jet_jit_email_mailer_send(mailer: i64, message: i64) -> i64 {
    let Some(message) = email_message_from_handle(message) else {
        return email_config_error("send", "invalid message");
    };
    let Some(NetValue::EmailMailer(mut mailer_value)) = take_net(mailer) else {
        return email_config_error("send", "invalid mailer");
    };
    let result = mailer_value.send(message);
    put_net(mailer, NetValue::EmailMailer(mailer_value));
    match result {
        Ok(report) => result_ok(email_send_report_handle(report) as u64),
        Err(error) => email_err(error),
    }
}

extern "C" fn jet_jit_email_smtp(config: i64) -> i64 {
    let Some(mut config) = unpack_smtp_config(config) else {
        return email_config_error("smtp", "invalid SMTP configuration value");
    };
    let extract = |s: &Vec<u8>| s.clone();
    let email_runtime = runtime::email_runtime();
    let smtp_result = runtime::jet_email::smtp(&config, extract, email_runtime);
    runtime::jet_email::wipe_config_secrets(&mut config, email_runtime);
    match smtp_result {
        Ok(mailer) => result_ok(push(NetValue::EmailMailer(mailer)) as u64),
        Err(err) => email_err(err),
    }
}


// Minimal TCP listen/local-addr/port for watcher demos (#1219) and http loopbacks.
use std::cell::RefCell;
use std::net::{SocketAddr, TcpListener};

thread_local! {
    static LISTENERS: RefCell<Vec<Option<TcpListener>>> = const { RefCell::new(Vec::new()) };
    static ADDRS: RefCell<Vec<Option<SocketAddr>>> = const { RefCell::new(Vec::new()) };
}

fn push_listener(listener: TcpListener) -> i64 {
    LISTENERS.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(listener));
        v.len() as i64
    })
}

fn push_addr(addr: SocketAddr) -> i64 {
    ADDRS.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(addr));
        v.len() as i64
    })
}

pub(crate) fn clear_net_state() {
    LISTENERS.with(|s| s.borrow_mut().clear());
    ADDRS.with(|s| s.borrow_mut().clear());
}

extern "C" fn jet_jit_net_tcp_listen(addr: i64) -> i64 {
    let addr = clone_string(addr);
    match TcpListener::bind(addr.as_str()) {
        Ok(listener) => {
            if let Err(e) = listener.set_nonblocking(true) {
                return result_err(e.to_string());
            }
            result_ok(push_listener(listener) as u64)
        }
        Err(e) => result_err(e.to_string()),
    }
}

extern "C" fn jet_jit_net_listener_local_socket_addr(listener: i64) -> i64 {
    if listener <= 0 {
        return result_err("invalid TcpListener".into());
    }
    let idx = (listener as usize).saturating_sub(1);
    let addr = LISTENERS.with(|slot| {
        slot.borrow()
            .get(idx)
            .and_then(|l| l.as_ref())
            .and_then(|l| l.local_addr().ok())
    });
    match addr {
        Some(addr) => result_ok(push_addr(addr) as u64),
        None => result_err("tcp listener local address failed".into()),
    }
}

extern "C" fn jet_jit_net_socket_port(addr: i64) -> i64 {
    if addr <= 0 {
        return 0;
    }
    let idx = (addr as usize).saturating_sub(1);
    ADDRS.with(|slot| {
        slot.borrow()
            .get(idx)
            .and_then(|a| a.as_ref())
            .map(|a| i64::from(a.port()))
            .unwrap_or(0)
    })
}

host_fns! {
    struct NetHostFns;
    register: register_net_symbols;
    declare: declare_net_host_fns(module) {
        let cc = module.target_config().default_call_conv;
        let mut sig0 = Signature::new(cc);
        sig0.returns.push(AbiParam::new(types::I64));
        let mut sig1 = Signature::new(cc);
        sig1.params.push(AbiParam::new(types::I64));
        sig1.returns.push(AbiParam::new(types::I64));
        let mut sig2 = Signature::new(cc);
        sig2.params.push(AbiParam::new(types::I64));
        sig2.params.push(AbiParam::new(types::I64));
        sig2.returns.push(AbiParam::new(types::I64));
        let mut sig3 = Signature::new(cc);
        for _ in 0..3 {
            sig3.params.push(AbiParam::new(types::I64));
        }
        sig3.returns.push(AbiParam::new(types::I64));
        let mut sig7 = Signature::new(cc);
        for _ in 0..7 {
            sig7.params.push(AbiParam::new(types::I64));
        }
        sig7.returns.push(AbiParam::new(types::I64));


    }
    tcp_listen: "jet_jit_net_tcp_listen" => jet_jit_net_tcp_listen: sig1;
    listener_local_socket_addr: "jet_jit_net_listener_local_socket_addr" => jet_jit_net_listener_local_socket_addr: sig1;
    socket_port: "jet_jit_net_socket_port" => jet_jit_net_socket_port: sig1;
    url_parse: "jet_jit_url_parse" => jet_jit_url_parse: sig1;
    url_typed_literal: "jet_jit_url_typed_literal" => jet_jit_url_typed_literal: sig2;
    url_file: "jet_jit_url_file" => jet_jit_url_file: sig1;
    url_data: "jet_jit_url_data" => jet_jit_url_data: sig2;
    url_query_value: "jet_jit_url_query_value" => jet_jit_url_query_value: sig1;
    url_percent_encode: "jet_jit_url_percent_encode" => jet_jit_url_percent_encode: sig1;
    url_percent_decode: "jet_jit_url_percent_decode" => jet_jit_url_percent_decode: sig1;
    mime_parse: "jet_jit_mime_parse" => jet_jit_mime_parse: sig1;
    mime_from_extension: "jet_jit_mime_from_extension" => jet_jit_mime_from_extension: sig1;
    mime_extension: "jet_jit_mime_extension" => jet_jit_mime_extension: sig1;
    url_to_string: "jet_jit_url_to_string" => jet_jit_url_to_string: sig1;
    url_scheme: "jet_jit_url_scheme" => jet_jit_url_scheme: sig1;
    url_host: "jet_jit_url_host" => jet_jit_url_host: sig1;
    url_path: "jet_jit_url_path" => jet_jit_url_path: sig1;
    url_query: "jet_jit_url_query" => jet_jit_url_query: sig1;
    url_query_pairs: "jet_jit_url_query_pairs" => jet_jit_url_query_pairs: sig1;
    url_path_segments: "jet_jit_url_path_segments" => jet_jit_url_path_segments: sig1;
    url_fragment: "jet_jit_url_fragment" => jet_jit_url_fragment: sig1;
    url_username: "jet_jit_url_username" => jet_jit_url_username: sig1;
    url_password: "jet_jit_url_password" => jet_jit_url_password: sig1;
    url_userinfo: "jet_jit_url_userinfo" => jet_jit_url_userinfo: sig1;
    url_authority: "jet_jit_url_authority" => jet_jit_url_authority: sig1;
    url_port: "jet_jit_url_port" => jet_jit_url_port: sig1;
    url_default_port: "jet_jit_url_default_port" => jet_jit_url_default_port: sig1;
    url_normalize: "jet_jit_url_normalize" => jet_jit_url_normalize: sig1;
    url_join: "jet_jit_url_join" => jet_jit_url_join: sig2;
    url_set_query: "jet_jit_url_set_query" => jet_jit_url_set_query: sig3;
    url_add_query: "jet_jit_url_add_query" => jet_jit_url_add_query: sig3;
    mime_essence: "jet_jit_mime_essence" => jet_jit_mime_essence: sig1;
    mime_param: "jet_jit_mime_param" => jet_jit_mime_param: sig2;
    browser_profile: "jet_jit_browser_profile" => jet_jit_browser_profile: sig1;
    browser_timeout: "jet_jit_browser_timeout" => jet_jit_browser_timeout: sig1;
    email_address: "jet_jit_email_address" => jet_jit_email_address: sig1;
    email_attachment: "jet_jit_email_attachment" => jet_jit_email_attachment: sig3;
    email_message: "jet_jit_email_message" => jet_jit_email_message: sig7;
    email_envelope: "jet_jit_email_envelope" => jet_jit_email_envelope: sig2;
    email_serialize: "jet_jit_email_serialize" => jet_jit_email_serialize: sig1;
    email_limits_safe: "jet_jit_email_limits_safe" => jet_jit_email_limits_safe: sig0;
    email_smtp: "jet_jit_email_smtp" => jet_jit_email_smtp: sig1;
    email_smtp_from_env: "jet_jit_email_smtp_from_env" => jet_jit_email_smtp_from_env: sig0;
    email_message_envelope: "jet_jit_email_message_envelope" => jet_jit_email_message_envelope: sig1;
    email_message_with_envelope: "jet_jit_email_message_with_envelope" => jet_jit_email_message_with_envelope: sig2;
    email_mailer_send: "jet_jit_email_mailer_send" => jet_jit_email_mailer_send: sig2;
}
