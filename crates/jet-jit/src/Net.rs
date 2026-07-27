//! Native JIT adapters for `core.url` / `core.mime` / `core.browser` (and later
//! net/http/email/ws). Algorithms come from the same prelude sources AOT emits.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

mod runtime {
    use crate::JetShow;

    pub mod jet_std {
        #[derive(Clone, Debug, PartialEq)]
        pub struct JetURL {
            pub scheme: String,
            pub host: Option<String>,
            pub port: Option<i64>,
            pub path: String,
            pub query: Vec<(String, String)>,
            pub fragment: Option<String>,
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
            Text(String),
            Array(Vec<JSON>),
            Object(std::collections::BTreeMap<String, JSON>),
        }

        include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/UrlMime.rs");
        include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/JSONCodec.rs");
    }

    fn jet_deadline_remaining_ms() -> Option<i64> {
        None
    }

    include!("../../jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/Browser.rs");

    fn jet_sha256_raw(data: &[u8]) -> [u8; 32] {
        crate::Crypto::runtime::jet_crypto_email_sha256_impl(data)
    }
    include!("../../jet-codegen/src/Prelude/CoreLib/Email.rs");

    pub use jet_std::{JetMIME, JetURL};

    fn email_tls_begin(
        _: std::net::TcpStream,
        _: &String,
    ) -> Result<i64, String> {
        Err("email TLS begin unused during smtp construction".into())
    }
    fn email_tls_begin_ca(
        _: std::net::TcpStream,
        _: &String,
        _: &Vec<u8>,
    ) -> Result<i64, String> {
        Err("email TLS begin unused during smtp construction".into())
    }
    fn email_tls_handshake_step(_: i64) -> Result<bool, String> {
        Err("email TLS unused during smtp construction".into())
    }
    fn email_tls_set_poll_timeout(_: i64, _: i64) -> Result<(), String> {
        Err("email TLS unused during smtp construction".into())
    }
    fn email_tls_read(_: i64, _: i64) -> Result<Vec<u8>, String> {
        Err("email TLS unused during smtp construction".into())
    }
    fn email_tls_write_all(_: i64, _: &Vec<u8>) -> Result<(), String> {
        Err("email TLS unused during smtp construction".into())
    }
    fn email_tls_close(_: i64) -> Result<(), String> {
        Err("email TLS unused during smtp construction".into())
    }
    fn email_cancelled() -> bool {
        false
    }
    fn email_remaining_ms() -> Option<i64> {
        None
    }

    pub fn email_runtime() -> jet_email::RuntimeFns {
        jet_email::RuntimeFns {
            tls_begin: email_tls_begin,
            tls_begin_ca: email_tls_begin_ca,
            tls_handshake_step: email_tls_handshake_step,
            tls_set_poll_timeout: email_tls_set_poll_timeout,
            tls_read: email_tls_read,
            tls_write_all: email_tls_write_all,
            tls_close: email_tls_close,
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
    Concurrency::with_runtime_mut(|rt| {
        let index = handle.saturating_sub(1) as usize;
        rt.net_values
            .get(index)
            .and_then(|slot| slot.as_ref())
            .and_then(f)
    })
}

fn clone_string(handle: i64) -> String {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(handle).unwrap_or_default())
}

fn alloc_string(s: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
}

fn result_ok(bits: u64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(super::JitResultValue { ok: true, bits });
        rt.results.len() as i64
    })
}

fn result_err(message: String) -> i64 {
    let handle = alloc_string(message);
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(super::JitResultValue {
            ok: false,
            bits: handle as u64,
        });
        rt.results.len() as i64
    })
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

extern "C" fn jet_jit_url_host(recv: i64) -> i64 {
    option_string(with_net(recv, |v| match v {
        NetValue::Url(u) => Some(u.host()),
        _ => None,
    }).flatten())
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
    }).flatten())
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
    }).flatten())
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

fn record_get_i64(record: i64, idx: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.record_get_int(record, idx).unwrap_or(0))
}

fn record_get_string(record: i64, idx: i64) -> String {
    Concurrency::with_runtime_mut(|rt| {
        // StructLit strings are JetVal::String; named-enum payloads store string handles as Int.
        if let Some(sid) = rt.heap.record_get_string(record, idx) {
            return rt.heap.clone_string(sid).unwrap_or_default();
        }
        let sid = rt.heap.record_get_int(record, idx).unwrap_or(0);
        rt.heap.clone_string(sid).unwrap_or_default()
    })
}

fn list_strings(list: i64) -> Vec<String> {
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

fn list_bytes(list: i64) -> Vec<u8> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(list, i).unwrap_or(0) as u8);
        }
        out
    })
}

fn email_err(err: runtime::jet_email::Error) -> i64 {
    result_err(format!("{err:?}"))
}

fn unpack_limits(handle: i64) -> runtime::jet_email::Limits {
    runtime::jet_email::Limits {
        max_reply_line_bytes: record_get_i64(handle, 0),
        max_reply_lines: record_get_i64(handle, 1),
        max_capabilities: record_get_i64(handle, 2),
        max_recipients: record_get_i64(handle, 3),
        max_message_bytes: record_get_i64(handle, 4),
        max_auth_challenge_bytes: record_get_i64(handle, 5),
    }
}

fn unpack_dkim(handle: i64) -> Option<runtime::jet_email::DkimConfig<Vec<u8>>> {
    let domain = record_get_string(handle, 0);
    let selector = record_get_string(handle, 1);
    let key_handle = record_get_i64(handle, 2);
    let private_key = crate::Crypto::secret_copy_for_smtp(key_handle)?;
    let headers = list_strings(record_get_i64(handle, 3));
    Some(runtime::jet_email::DkimConfig {
        domain,
        selector,
        private_key,
        signed_headers: headers,
    })
}

fn unpack_auth(handle: i64) -> Option<runtime::jet_email::SMTPAuth<Vec<u8>>> {
    let disc = record_get_i64(handle, 0);
    if disc == 0 {
        return Some(runtime::jet_email::SMTPAuth::None);
    }
    if disc == 1 {
        let username = record_get_string(handle, 1);
        let password_handle = record_get_i64(handle, 2);
        let password = crate::Crypto::secret_copy_for_smtp(password_handle)?;
        return Some(runtime::jet_email::SMTPAuth::Password { username, password });
    }
    None
}

fn unpack_smtp_config(config: i64) -> Option<runtime::jet_email::SMTPConfig<Vec<u8>>> {
    let host = record_get_string(config, 0);
    let port = record_get_i64(config, 1);
    let security = match record_get_i64(config, 2) {
        1 => runtime::jet_email::SMTPSecurity::TLS,
        _ => runtime::jet_email::SMTPSecurity::StartTls,
    };
    let auth_raw = record_get_i64(config, 3);
    let auth = if auth_raw == 0 {
        runtime::jet_email::SMTPAuth::None
    } else {
        unpack_auth(auth_raw)?
    };
    let recipient_policy = match record_get_i64(config, 4) {
        1 => runtime::jet_email::RecipientPolicy::DeliverAccepted,
        _ => runtime::jet_email::RecipientPolicy::RequireAll,
    };
    let trust = runtime::jet_email::TLSTrust::System;
    let _ = record_get_i64(config, 5);
    let limits = unpack_limits(record_get_i64(config, 6));
    let dkim_opt = record_get_i64(config, 7);
    let dkim = if dkim_opt == 0 {
        None
    } else {
        unpack_dkim(dkim_opt - 1)
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

extern "C" fn jet_jit_email_address(text: i64) -> i64 {
    let text = clone_string(text);
    match runtime::jet_email::address(&text) {
        Ok(addr) => result_ok(push(NetValue::EmailAddress(addr)) as u64),
        Err(err) => {
            email_err(err)
        }
    }
}

extern "C" fn jet_jit_email_attachment(filename: i64, mime: i64, bytes: i64) -> i64 {
    let filename = clone_string(filename);
    let mime = clone_string(mime);
    let bytes = list_bytes(bytes);
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
    let from = match with_net(from, |v| match v {
        NetValue::EmailAddress(a) => Some(a.clone()),
        _ => None,
    }) {
        Some(a) => a,
        None => {
            return result_err("invalid email from address".into());
        }
    };
    let to = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(to).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let h = rt.heap.list_get_int(to, i).unwrap_or(0);
            let idx = h.saturating_sub(1) as usize;
            match rt.net_values.get(idx).and_then(|s| s.as_ref()) {
                Some(NetValue::EmailAddress(a)) => out.push(a.clone()),
                _ => return None,
            }
        }
        Some(out)
    });
    let Some(to) = to else {
        return result_err("invalid email to list".into());
    };
    let bcc = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(bcc).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let h = rt.heap.list_get_int(bcc, i).unwrap_or(0);
            let idx = h.saturating_sub(1) as usize;
            match rt.net_values.get(idx).and_then(|s| s.as_ref()) {
                Some(NetValue::EmailAddress(a)) => out.push(a.clone()),
                _ => return None,
            }
        }
        Some(out)
    });
    let Some(bcc) = bcc else {
        return result_err("invalid email bcc list".into());
    };
    let subject = clone_string(subject);
    let text = clone_string(text);
    let html = clone_string(html);
    let attachments = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(attachments).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let h = rt.heap.list_get_int(attachments, i).unwrap_or(0);
            let idx = h.saturating_sub(1) as usize;
            match rt.net_values.get(idx).and_then(|s| s.as_ref()) {
                Some(NetValue::EmailAttachment(a)) => out.push(a.clone()),
                _ => return None,
            }
        }
        Some(out)
    });
    let Some(attachments) = attachments else {
        return result_err("invalid email attachment list".into());
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

extern "C" fn jet_jit_email_serialize(message: i64) -> i64 {
    let msg = match with_net(message, |v| match v {
        NetValue::EmailMessage(m) => Some(m.clone()),
        _ => None,
    }) {
        Some(m) => m,
        None => {
            return result_err("invalid email message".into());
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

extern "C" fn jet_jit_email_smtp(config: i64) -> i64 {
    let Some(mut config) = unpack_smtp_config(config) else {
        return result_err("invalid SMTP config".into());
    };
    let extract = |s: &Vec<u8>| s.clone();
    match runtime::jet_email::smtp(&config, extract, runtime::email_runtime()) {
        Ok(mailer) => {
            if let runtime::jet_email::SMTPAuth::Password { password, .. } = &mut config.auth {
                crate::Crypto::runtime::jet_crypto_zeroize_email_impl(password);
            }
            if let Some(dkim) = &mut config.dkim {
                crate::Crypto::runtime::jet_crypto_zeroize_email_impl(&mut dkim.private_key);
            }
            result_ok(push(NetValue::EmailMailer(mailer)) as u64)
        }
        Err(err) => {
            if let runtime::jet_email::SMTPAuth::Password { password, .. } = &mut config.auth {
                crate::Crypto::runtime::jet_crypto_zeroize_email_impl(password);
            }
            if let Some(dkim) = &mut config.dkim {
                crate::Crypto::runtime::jet_crypto_zeroize_email_impl(&mut dkim.private_key);
            }
            email_err(err)
        }
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

pub(crate) struct NetHostFns {
    pub tcp_listen: FuncId,
    pub listener_local_socket_addr: FuncId,
    pub socket_port: FuncId,
    pub url_parse: FuncId,
    pub url_file: FuncId,
    pub url_data: FuncId,
    pub url_query: FuncId,
    pub url_percent_encode: FuncId,
    pub url_percent_decode: FuncId,
    pub mime_parse: FuncId,
    pub mime_from_extension: FuncId,
    pub mime_extension: FuncId,
    pub url_to_string: FuncId,
    pub url_host: FuncId,
    pub url_path: FuncId,
    pub url_query_pairs: FuncId,
    pub url_path_segments: FuncId,
    pub url_fragment: FuncId,
    pub url_join: FuncId,
    pub mime_essence: FuncId,
    pub mime_param: FuncId,
    pub browser_profile: FuncId,
    pub browser_timeout: FuncId,
    pub email_address: FuncId,
    pub email_attachment: FuncId,
    pub email_message: FuncId,
    pub email_serialize: FuncId,
    pub email_smtp: FuncId,
}

pub(crate) fn register_net_symbols(builder: &mut JITBuilder) {
    builder.symbol("jet_jit_net_tcp_listen", jet_jit_net_tcp_listen as *const u8);
    builder.symbol(
        "jet_jit_net_listener_local_socket_addr",
        jet_jit_net_listener_local_socket_addr as *const u8,
    );
    builder.symbol("jet_jit_net_socket_port", jet_jit_net_socket_port as *const u8);
    builder.symbol("jet_jit_url_parse", jet_jit_url_parse as *const u8);
    builder.symbol("jet_jit_url_file", jet_jit_url_file as *const u8);
    builder.symbol("jet_jit_url_data", jet_jit_url_data as *const u8);
    builder.symbol("jet_jit_url_query", jet_jit_url_query as *const u8);
    builder.symbol(
        "jet_jit_url_percent_encode",
        jet_jit_url_percent_encode as *const u8,
    );
    builder.symbol(
        "jet_jit_url_percent_decode",
        jet_jit_url_percent_decode as *const u8,
    );
    builder.symbol("jet_jit_mime_parse", jet_jit_mime_parse as *const u8);
    builder.symbol(
        "jet_jit_mime_from_extension",
        jet_jit_mime_from_extension as *const u8,
    );
    builder.symbol("jet_jit_mime_extension", jet_jit_mime_extension as *const u8);
    builder.symbol("jet_jit_url_to_string", jet_jit_url_to_string as *const u8);
    builder.symbol("jet_jit_url_host", jet_jit_url_host as *const u8);
    builder.symbol("jet_jit_url_path", jet_jit_url_path as *const u8);
    builder.symbol("jet_jit_url_query_pairs", jet_jit_url_query_pairs as *const u8);
    builder.symbol(
        "jet_jit_url_path_segments",
        jet_jit_url_path_segments as *const u8,
    );
    builder.symbol("jet_jit_url_fragment", jet_jit_url_fragment as *const u8);
    builder.symbol("jet_jit_url_join", jet_jit_url_join as *const u8);
    builder.symbol("jet_jit_mime_essence", jet_jit_mime_essence as *const u8);
    builder.symbol("jet_jit_mime_param", jet_jit_mime_param as *const u8);
    builder.symbol(
        "jet_jit_browser_profile",
        jet_jit_browser_profile as *const u8,
    );
    builder.symbol(
        "jet_jit_browser_timeout",
        jet_jit_browser_timeout as *const u8,
    );
    builder.symbol("jet_jit_email_address", jet_jit_email_address as *const u8);
    builder.symbol(
        "jet_jit_email_attachment",
        jet_jit_email_attachment as *const u8,
    );
    builder.symbol("jet_jit_email_message", jet_jit_email_message as *const u8);
    builder.symbol(
        "jet_jit_email_serialize",
        jet_jit_email_serialize as *const u8,
    );
    builder.symbol("jet_jit_email_smtp", jet_jit_email_smtp as *const u8);
}

pub(crate) fn declare_net_host_fns(module: &mut JITModule) -> Result<NetHostFns, String> {
    let cc = module.target_config().default_call_conv;
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
    let import = |module: &mut JITModule, name: &str, sig: &Signature| {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    Ok(NetHostFns {
        tcp_listen: import(module, "jet_jit_net_tcp_listen", &sig1)?,
        listener_local_socket_addr: import(module, "jet_jit_net_listener_local_socket_addr", &sig1)?,
        socket_port: import(module, "jet_jit_net_socket_port", &sig1)?,
        url_parse: import(module, "jet_jit_url_parse", &sig1)?,
        url_file: import(module, "jet_jit_url_file", &sig1)?,
        url_data: import(module, "jet_jit_url_data", &sig2)?,
        url_query: import(module, "jet_jit_url_query", &sig1)?,
        url_percent_encode: import(module, "jet_jit_url_percent_encode", &sig1)?,
        url_percent_decode: import(module, "jet_jit_url_percent_decode", &sig1)?,
        mime_parse: import(module, "jet_jit_mime_parse", &sig1)?,
        mime_from_extension: import(module, "jet_jit_mime_from_extension", &sig1)?,
        mime_extension: import(module, "jet_jit_mime_extension", &sig1)?,
        url_to_string: import(module, "jet_jit_url_to_string", &sig1)?,
        url_host: import(module, "jet_jit_url_host", &sig1)?,
        url_path: import(module, "jet_jit_url_path", &sig1)?,
        url_query_pairs: import(module, "jet_jit_url_query_pairs", &sig1)?,
        url_path_segments: import(module, "jet_jit_url_path_segments", &sig1)?,
        url_fragment: import(module, "jet_jit_url_fragment", &sig1)?,
        url_join: import(module, "jet_jit_url_join", &sig2)?,
        mime_essence: import(module, "jet_jit_mime_essence", &sig1)?,
        mime_param: import(module, "jet_jit_mime_param", &sig2)?,
        browser_profile: import(module, "jet_jit_browser_profile", &sig1)?,
        browser_timeout: import(module, "jet_jit_browser_timeout", &sig1)?,
        email_address: import(module, "jet_jit_email_address", &sig1)?,
        email_attachment: import(module, "jet_jit_email_attachment", &sig3)?,
        email_message: import(module, "jet_jit_email_message", &sig7)?,
        email_serialize: import(module, "jet_jit_email_serialize", &sig1)?,
        email_smtp: import(module, "jet_jit_email_smtp", &sig1)?,
    })
}
