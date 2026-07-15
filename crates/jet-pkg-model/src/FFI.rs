//! M7 hidden cargo FFI bridge (S50).
//!
//! When a program declares `extern rust` blocks with crate dependencies, the
//! driver materializes a cached cargo project under `~/.cache/jet/ffi/` and
//! links the built rlib into the user's generated program.

use crate::Diagnostics::Diagnostic;
use crate::AST::{AccessConvention, ExternFn, ExternRustBlock, Item, ProgramBundle, Type};
use std::collections::{hash_map::DefaultHasher, BTreeMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;

// FfiLink struct lives in AST for cross-seam sharing; re-export here.
pub use crate::AST::FfiLink;

/// One foreign function collected from the import graph.
#[derive(Debug, Clone)]
pub struct ExternEntry {
    pub jet_name: String,
    pub rust_path: String,
    pub wrapper_name: String,
    pub params: Vec<(AccessConvention, Type)>,
    pub return_type: Option<Type>,
    pub crate_spec: String,
    /// Human-facing hint for E0705 (`extern` line context).
    pub line_hint: String,
}

/// Gather every `extern rust` function across all modules.
pub fn collect_externs(bundle: &ProgramBundle) -> Vec<ExternEntry> {
    let mut out = Vec::new();
    for module in &bundle.modules {
        for item in &module.items {
            let Item::ExternRust(block) = item else {
                continue;
            };
            for ef in &block.functions {
                out.push(extern_entry(ef, block, &module.display));
            }
        }
    }
    out
}

fn extern_entry(ef: &ExternFn, block: &ExternRustBlock, _file: &str) -> ExternEntry {
    ExternEntry {
        jet_name: ef.name.clone(),
        rust_path: ef.rust_path.clone(),
        wrapper_name: format!("jet_ffi_{}", ef.name),
        params: ef
            .params
            .iter()
            .map(|p| (p.convention, p.ty.clone()))
            .collect(),
        return_type: ef.return_type.clone(),
        crate_spec: block.crate_spec.clone(),
        line_hint: format!("`{}` in `extern rust \"{}\"`", ef.name, block.crate_spec),
    }
}

/// Build (or reuse) the hidden wrapper crate. Returns `Ok(None)` when the
/// program has no `extern rust` declarations and does not use `core.archive`,
/// `jet.db`, or `core.compress.{gzip,zstd}`.
///
/// `core.archive` (zip/tar; D-CORE-COMPRESS1), `jet.db` (D-DEP-DB1), and
/// `core.compress` (gzip/zstd; D-CORE-COMPRESS1) are delivered through this
/// same hidden-cargo bridge: when a program imports any of them, the bridge
/// crate gains the matching dependency and a hand-written runtime. Archive
/// embeds the canonical vendored package source; the other bridges live under
/// `crates/jet-pkg-model/src/Prelude/`. The compiler crate
/// (`Source/`) stays zero-dependency (I6). These are the owner-approved I6
/// bootstrap exceptions, to be native-ized before the end of Epoch 3.
pub fn prepare(bundle: &ProgramBundle) -> Result<Option<FfiLink>, Vec<Diagnostic>> {
    let entries = collect_externs(bundle);
    // D-REGEXENGINE1=A: core.regex is std-only in the generated prelude now, so
    // it never asks for a hidden bridge crate.
    let needs_regex = false;
    let needs_archive = bundle
        .used_core
        .iter()
        .any(|u| u == "core.archive" || u.starts_with("core.archive::"));
    let needs_db = bundle
        .used_core
        .iter()
        .any(|u| u == "jet.db" || u.starts_with("jet.db::"));
    // D-CODECS1: standalone `core.compress.gzip` / `core.compress.zstd` codecs.
    let needs_compress = bundle.used_core.iter().any(|u| {
        u == "core.compress.gzip"
            || u.starts_with("core.compress.gzip::")
            || u == "core.compress.zstd"
            || u.starts_with("core.compress.zstd::")
    });
    // D-NETDEP1=A / D-HTTPLIB4=B / D-TLS1=A: ureq + rustls with system trust
    // roots for core.http.client TLS support.
    let needs_http_client = bundle
        .used_core
        .iter()
        .any(|u| u == "core.http.client" || u.starts_with("core.http.client::"));
    // D-TLSSERVE1=A: server-side TLS uses rustls through the hidden bridge
    // only when the named `tls:` option is constructed.
    let needs_http_server_tls = bundle
        .used_core
        .iter()
        .any(|u| u == "core.http.server::tls");
    // D-NETSOCKET1=A / D-TLS1=A: `core.net.tls_connect` upgrades an existing
    // TcpStream through the same hidden rustls bridge family as HTTP TLS.
    let needs_net_tls = bundle
        .used_core
        .iter()
        .any(|u| u == "core.net::tls_connect" || u == "core.tls" || u.starts_with("core.tls::")
            || u == "core.email" || u.starts_with("core.email::"));
    // D-DEP-CRYPTO1=A: RustCrypto AEAD + Ed25519 for core.crypto envelope APIs.
    let needs_crypto = bundle.used_core.iter().any(|u| {
        u == "jet.crypto"
            || u.starts_with("jet.crypto::")
            || u == "core.crypto.expert"
            || u.starts_with("core.crypto.expert::")
            || u == "core.email"
            || u.starts_with("core.email::")
    });
    // D-DEP-WASM1=A (c81): `core.plugin` — the sandboxed WASM Component Model
    // plugin loader (`Plugin.load`/`.call`).
    let needs_plugin = bundle
        .used_core
        .iter()
        .any(|u| u == "jet.plugin" || u.starts_with("jet.plugin::"));
    // U13 (D-JPK-SECRETCRYPTO1): `core.vault.get` — decrypted-repo-secret read,
    // age-style crypto FFI bridge.
    let needs_secrets = bundle
        .used_core
        .iter()
        .any(|u| u == "core.vault" || u.starts_with("core.vault::"));
    if entries.is_empty()
        && !needs_regex
        && !needs_archive
        && !needs_db
        && !needs_http_client
        && !needs_http_server_tls
        && !needs_net_tls
        && !needs_crypto
        && !needs_compress
        && !needs_plugin
        && !needs_secrets
    {
        return Ok(None);
    }

    build_bridge_full(
        &entries,
        needs_regex,
        needs_archive,
        needs_db,
        needs_http_client,
        needs_http_server_tls,
        needs_net_tls,
        needs_crypto,
        needs_compress,
        needs_plugin,
        needs_secrets,
    )
    .map(Some)
}

/// The `zip` crate version that backs `core.archive` zip (D-DEP-ARCHIVE1).
/// Lives only here — never in the compiler's Cargo.toml (I6).
pub const ZIP_CRATE_SPEC: (&str, &str) = ("zip", "2");

/// The `tar` crate version that backs `core.archive` tar (D-DEP-ARCHIVE1).
/// Lives only here — never in the compiler's Cargo.toml (I6).
pub const TAR_CRATE_SPEC: (&str, &str) = ("tar", "0");

/// The `rusqlite` crate version that backs `jet.db` (D-DEP-DB1).
/// Lives only here — never in the compiler's Cargo.toml (I6).
pub const DB_CRATE_SPEC: (&str, &str) = ("rusqlite", "0.31");

/// The `ureq` crate version that backs `core.http.client`
/// (D-NETDEP1=A, D-HTTPLIB4=B, D-TLS1=A).
/// rustls is pulled in by `tls`; system roots by `native-certs`.
/// Lives only here — never in the compiler's Cargo.toml (I6).
pub const HTTP_CLIENT_CRATE_SPEC: (&str, &str) = ("ureq", "2");

/// Hand-written HTTP client runtime emitted into the bridge crate when `core.http.client` is used.
const HTTP_CLIENT_RUNTIME: &str = include_str!("Prelude/Http.rs");

/// The `rustls` crate version that backs `core.http.server` TLS
/// (D-TLSSERVE1=A). Lives only here — never in the compiler's Cargo.toml (I6).
pub const HTTP_SERVER_TLS_CRATE_SPEC: (&str, &str) = ("rustls", "0.23");

/// PEM parser used by the server TLS bridge.
pub const HTTP_SERVER_TLS_PEMFILE_CRATE_SPEC: (&str, &str) = ("rustls-pemfile", "2");

/// System root loader used by `core.net.tls_connect` (D-NETSOCKET1=A / D-TLS1=A).
pub const RUSTLS_NATIVE_CERTS_CRATE_SPEC: (&str, &str) = ("rustls-native-certs", "0.7");

/// Hand-written HTTP server TLS runtime emitted into the bridge crate when
/// `core.http.server.tls` is used.
const HTTP_SERVER_TLS_RUNTIME: &str = include_str!("Prelude/HttpServerTls.rs");

/// Hand-written client TLS stream runtime emitted when `core.net.tls_connect` is used.
const NET_TLS_RUNTIME: &str = include_str!("Prelude/NetTls.rs");

#[cfg(test)]
mod net_tls_close_tests {
    #![allow(dead_code)]

    include!("Prelude/NetTls.rs");

    struct BrokenTransport;

    impl std::io::Read for BrokenTransport {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Ok(0)
        }
    }

    impl std::io::Write for BrokenTransport {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "hostile peer reset",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "hostile peer reset",
            ))
        }
    }

    #[test]
    fn close_notify_reports_hostile_transport_flush_failure() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let config = std::sync::Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth(),
        );
        let name = rustls::pki_types::ServerName::try_from("localhost".to_string()).unwrap();
        let conn = rustls::ClientConnection::new(config, name).unwrap();
        let mut stream = rustls::StreamOwned::new(conn, BrokenTransport);

        let error = jet_net_tls_flush_close_notify(&mut stream).unwrap_err();

        assert!(error.contains("TLS close-notify flush failed"), "{error}");
        assert!(error.contains("hostile peer reset"), "{error}");
    }

    fn tls_fixture() -> (std::net::SocketAddr, std::thread::JoinHandle<()>) {
        let cert_pem = include_bytes!("../../../tests/fixtures/tls/smtp.server.cert.pem");
        let key_pem = include_bytes!("../../../tests/fixtures/tls/smtp.server.key.pem");
        let certs = jet_net_tls_pem_certificates(cert_pem).unwrap();
        let key_text = std::str::from_utf8(key_pem).unwrap();
        let body = key_text
            .strip_prefix("-----BEGIN PRIVATE KEY-----")
            .unwrap()
            .strip_suffix("-----END PRIVATE KEY-----\n")
            .unwrap();
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(jet_net_tls_pem_base64(body).unwrap()),
        );
        let config = std::sync::Arc::new(
            rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certs, key)
                .unwrap(),
        );
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            let conn = rustls::ServerConnection::new(config).unwrap();
            let mut tls = rustls::StreamOwned::new(conn, socket);
            while tls.conn.is_handshaking() {
                if tls.conn.complete_io(&mut tls.sock).is_err() {
                    return;
                }
            }
        });
        (address, server)
    }

    fn connect_with_ca_for_test(
        socket: std::net::TcpStream,
        server_name: &str,
        pem: &Vec<u8>,
    ) -> Result<i64, String> {
        let id = jet_net_tls_begin_with_ca_impl(socket, &server_name.to_string(), pem)?;
        loop {
            if jet_net_tls_handshake_step_impl(id)? {
                return Ok(id);
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    #[test]
    fn system_plus_ca_extends_roots_and_keeps_hostname_verification() {
        let pem = include_bytes!("../../../tests/fixtures/tls/smtp.ca.cert.pem").to_vec();
        let (address, server) = tls_fixture();
        let socket = std::net::TcpStream::connect(address).unwrap();
        let id = connect_with_ca_for_test(socket, "localhost", &pem)
            .expect("fixture CA should verify localhost");
        jet_net_tls_close_impl(id).unwrap();
        server.join().unwrap();

        let (address, server) = tls_fixture();
        let socket = std::net::TcpStream::connect(address).unwrap();
        let error = connect_with_ca_for_test(socket, "example.com", &pem)
            .expect_err("custom CA must not disable DNS-name verification");
        assert!(error.contains("TLS handshake with `example.com` failed"), "{error}");
        assert!(error.to_ascii_lowercase().contains("not valid for name"), "{error}");
        server.join().unwrap();
    }

    mod smtp_adapter {
        fn jet_sha256_raw(data: &[u8]) -> [u8; 32] {
            let mut out = [0u8; 32];
            for (index, byte) in data.iter().enumerate() {
                out[index % 32] = out[index % 32].wrapping_mul(31).wrapping_add(*byte);
            }
            out
        }
        include!("../../jet-codegen/src/Prelude/CoreLib/Email.rs");
    }

    static SMTP_CANCELLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static SMTP_DEADLINE_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(i64::MAX);
    static SMTP_WIPES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    static SMTP_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn smtp_now_ms() -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .unwrap().as_millis().min(i64::MAX as u128) as i64
    }
    fn smtp_cancelled() -> bool { SMTP_CANCELLED.load(std::sync::atomic::Ordering::SeqCst) }
    fn smtp_remaining() -> Option<i64> {
        let deadline = SMTP_DEADLINE_MS.load(std::sync::atomic::Ordering::SeqCst);
        (deadline != i64::MAX).then(|| deadline.saturating_sub(smtp_now_ms()))
    }
    fn smtp_wipe(bytes: &mut Vec<u8>) {
        bytes.fill(0);
        bytes.clear();
    }
    fn smtp_counting_wipe(bytes: &mut Vec<u8>) {
        bytes.fill(0);
        bytes.clear();
        SMTP_WIPES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    fn smtp_sha256(bytes: &[u8]) -> [u8; 32] {
        use sha2::Digest;
        sha2::Sha256::digest(bytes).into()
    }
    fn smtp_dkim_sign(key: &Vec<u8>, message: &[u8]) -> Result<Vec<u8>, String> {
        use ed25519_dalek::Signer;
        let seed: [u8; 32] = key.as_slice().try_into()
            .map_err(|_| "DKIM test key length".to_string())?;
        Ok(ed25519_dalek::SigningKey::from_bytes(&seed).sign(message).to_bytes().to_vec())
    }

    fn smtp_runtime() -> smtp_adapter::jet_email::RuntimeFns {
        smtp_adapter::jet_email::RuntimeFns {
            tls_begin: jet_net_tls_begin_impl,
            tls_begin_ca: jet_net_tls_begin_with_ca_impl,
            tls_handshake_step: jet_net_tls_handshake_step_impl,
            tls_set_poll_timeout: jet_net_tls_set_poll_timeout_impl,
            tls_read: jet_net_tls_read_bytes_impl,
            tls_write_all: jet_net_tls_write_all_bytes_impl,
            tls_close: jet_net_tls_close_impl,
            wipe: smtp_wipe,
            sha256: smtp_sha256,
            ed25519_sign: smtp_dkim_sign,
            cancelled: smtp_cancelled,
            remaining_ms: smtp_remaining,
            accepted_at: smtp_adapter::jet_email::runtime_now,
        }
    }
    fn smtp_counting_runtime() -> smtp_adapter::jet_email::RuntimeFns {
        let mut runtime = smtp_runtime();
        runtime.wipe = smtp_counting_wipe;
        runtime
    }

    fn smtp_server_config() -> std::sync::Arc<rustls::ServerConfig> {
        let certs = jet_net_tls_pem_certificates(include_bytes!("../../../tests/fixtures/tls/smtp.server.cert.pem")).unwrap();
        let key_text = std::str::from_utf8(include_bytes!("../../../tests/fixtures/tls/smtp.server.key.pem")).unwrap();
        let body = key_text.strip_prefix("-----BEGIN PRIVATE KEY-----").unwrap()
            .strip_suffix("-----END PRIVATE KEY-----\n").unwrap();
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(jet_net_tls_pem_base64(body).unwrap()),
        );
        std::sync::Arc::new(rustls::ServerConfig::builder().with_no_client_auth()
            .with_single_cert(certs, key).unwrap())
    }

    fn smtp_read_line<T: std::io::Read>(io: &mut T) -> String {
        let mut bytes = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            if io.read(&mut byte).unwrap_or(0) == 0 { break; }
            bytes.push(byte[0]);
            if bytes.ends_with(b"\r\n") { break; }
        }
        String::from_utf8(bytes).unwrap()
    }

    #[derive(Clone, Copy)]
    enum SmtpFixture { Success, CloseAfterData, RejectAuth, StallGreeting }

    fn smtp_tls_session<T: std::io::Read + std::io::Write>(
        io: &mut T,
        greeting: bool,
        fixture: SmtpFixture,
        transcript: &std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        if greeting {
            if matches!(fixture, SmtpFixture::StallGreeting) {
                let mut byte = [0u8; 1];
                let _ = io.read(&mut byte);
                return;
            }
            io.write_all(b"220 relay ready\r\n").unwrap(); io.flush().unwrap();
        }
        let ehlo = smtp_read_line(io); transcript.lock().unwrap().push(ehlo.clone());
        assert_eq!(ehlo, "EHLO localhost\r\n");
        io.write_all(b"250-relay\r\n250 AUTH PLAIN LOGIN\r\n").unwrap(); io.flush().unwrap();
        let auth = smtp_read_line(io); transcript.lock().unwrap().push(auth.clone());
        assert!(auth.starts_with("AUTH PLAIN ")); assert!(!auth.contains("swordfish"));
        if matches!(fixture, SmtpFixture::RejectAuth) {
            io.write_all(b"535 bad credentials\r\n").unwrap(); io.flush().unwrap();
            return;
        }
        io.write_all(b"235 authenticated\r\n").unwrap(); io.flush().unwrap();
        let mail = smtp_read_line(io); transcript.lock().unwrap().push(mail.clone());
        assert!(mail.starts_with("MAIL FROM:<sender@example.com>"));
        io.write_all(b"250 sender ok\r\n").unwrap(); io.flush().unwrap();
        let rcpt = smtp_read_line(io); transcript.lock().unwrap().push(rcpt.clone());
        assert!(rcpt.starts_with("RCPT TO:<recipient@example.net>"));
        io.write_all(b"250 recipient ok\r\n").unwrap(); io.flush().unwrap();
        let data = smtp_read_line(io); transcript.lock().unwrap().push(data.clone());
        assert_eq!(data, "DATA\r\n");
        io.write_all(b"354 continue\r\n").unwrap(); io.flush().unwrap();
        let mut body = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            if io.read(&mut byte).unwrap_or(0) == 0 { return; }
            body.push(byte[0]);
            if body.ends_with(b"\r\n.\r\n") { break; }
        }
        transcript.lock().unwrap().push(String::from_utf8_lossy(&body).into_owned());
        if matches!(fixture, SmtpFixture::CloseAfterData) { return; }
        io.write_all(b"250 queued q-1\r\n").unwrap(); io.flush().unwrap();
        let quit = smtp_read_line(io); transcript.lock().unwrap().push(quit);
        let _ = io.write_all(b"221 bye\r\n"); let _ = io.flush();
    }

    fn spawn_smtp_server(
        starttls: bool,
        fixture: SmtpFixture,
    ) -> (std::net::SocketAddr, std::sync::Arc<std::sync::Mutex<Vec<String>>>, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let transcript = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let server_transcript = transcript.clone();
        let config = smtp_server_config();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            if starttls {
                socket.write_all(b"220 relay ready\r\n").unwrap(); socket.flush().unwrap();
                let ehlo = smtp_read_line(&mut socket); server_transcript.lock().unwrap().push(ehlo);
                socket.write_all(b"250-relay\r\n250-STARTTLS\r\n250 AUTH PLAIN LOGIN\r\n").unwrap(); socket.flush().unwrap();
                let command = smtp_read_line(&mut socket); server_transcript.lock().unwrap().push(command.clone());
                assert_eq!(command, "STARTTLS\r\n");
                socket.write_all(b"220 begin TLS\r\n").unwrap(); socket.flush().unwrap();
            }
            let conn = rustls::ServerConnection::new(config).unwrap();
            let mut tls = rustls::StreamOwned::new(conn, socket);
            while tls.conn.is_handshaking() {
                if tls.conn.complete_io(&mut tls.sock).is_err() { return; }
            }
            smtp_tls_session(&mut tls, !starttls, fixture, &server_transcript);
        });
        (address, transcript, server)
    }

    fn smtp_message() -> smtp_adapter::jet_email::Message {
        use smtp_adapter::jet_email as email;
        let from = email::address(&"sender@example.com".to_string()).unwrap();
        let to = email::address(&"recipient@example.net".to_string()).unwrap();
        email::message(&from, &vec![to], &vec![], &"subject".to_string(),
            &"first\r\n.second".to_string(), &String::new(), &vec![]).unwrap()
    }

    fn smtp_config(port: u16, starttls: bool) -> smtp_adapter::jet_email::SmtpConfig<Vec<u8>> {
        use smtp_adapter::jet_email as email;
        email::SmtpConfig {
            host: "localhost".to_string(), port: port as i64,
            security: if starttls { email::SmtpSecurity::StartTls } else { email::SmtpSecurity::Tls },
            auth: email::SmtpAuth::Password { username: "mailer".to_string(), password: b"swordfish".to_vec() },
            recipient_policy: email::RecipientPolicy::RequireAll,
            trust: email::TlsTrust::SystemPlusCa {
                pem: include_bytes!("../../../tests/fixtures/tls/smtp.ca.cert.pem").to_vec(),
            },
            limits: email::Limits::safe(),
            dkim: None,
        }
    }

    fn test_base64_decode(text: &str) -> Vec<u8> {
        let value = |byte| match byte {
            b'A'..=b'Z' => byte - b'A', b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52, b'+' => 62, b'/' => 63,
            _ => 0,
        };
        let bytes: Vec<u8> = text.bytes().filter(|byte| !byte.is_ascii_whitespace()).collect();
        let mut out = Vec::new();
        for chunk in bytes.chunks_exact(4) {
            let values = [value(chunk[0]), value(chunk[1]), value(chunk[2]), value(chunk[3])];
            out.push(values[0] << 2 | values[1] >> 4);
            if chunk[2] != b'=' { out.push(values[1] << 4 | values[2] >> 2); }
            if chunk[3] != b'=' { out.push(values[2] << 6 | values[3]); }
        }
        out
    }

    fn test_relaxed_header(header: &str) -> String {
        let (name, value) = header.split_once(':').unwrap();
        let unfolded = value.replace("\r\n ", " ").replace("\r\n\t", " ");
        let value = unfolded.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
        format!("{}:{}\r\n", name.to_ascii_lowercase(), value)
    }

    fn verify_dkim_wire(wire: &str, seed: [u8; 32]) {
        use sha2::Digest;
        let boundary = wire.find("\r\n\r\n").unwrap();
        let headers = &wire[..boundary];
        let body = &wire[boundary + 4..];
        let mut first_end = 0usize;
        for (index, line) in headers.split("\r\n").enumerate() {
            if index != 0 && !line.starts_with([' ', '\t']) { break; }
            first_end += line.len() + 2;
        }
        let dkim_header = &headers[..first_end - 2];
        let unfolded = dkim_header.replace("\r\n ", " ");
        let tags = unfolded.split_once(':').unwrap().1.split(';')
            .filter_map(|tag| tag.trim().split_once('='))
            .map(|(name, value)| (name, value)).collect::<std::collections::HashMap<_, _>>();
        assert_eq!(tags["a"], "ed25519-sha256");
        assert_eq!(tags["c"], "relaxed/relaxed");
        assert_eq!(tags["d"], "example.com");
        assert_eq!(tags["s"], "login-2026");

        let mut body_lines: Vec<&str> = body.split("\r\n").collect();
        while body_lines.last().is_some_and(|line| line.trim_end_matches([' ', '\t']).is_empty()) {
            body_lines.pop();
        }
        let canonical_body = if body_lines.is_empty() { "\r\n".to_string() } else {
            body_lines.into_iter().map(|line| line.trim_end_matches([' ', '\t']))
                .collect::<Vec<_>>().join("\r\n") + "\r\n"
        };
        let body_hash: [u8; 32] = sha2::Sha256::digest(canonical_body.as_bytes()).into();
        assert_eq!(test_base64_decode(tags["bh"]), body_hash);

        let mut signing_input = String::new();
        for wanted in tags["h"].split(':') {
            let header = headers.rsplit("\r\n")
                .find(|line| line.split_once(':').is_some_and(|(name, _)| name.eq_ignore_ascii_case(wanted)))
                .unwrap();
            signing_input.push_str(&test_relaxed_header(header));
        }
        let empty_b = format!("{}b=", unfolded.split("b=").next().unwrap());
        signing_input.push_str(&test_relaxed_header(&empty_b));
        let signature: [u8; 64] = test_base64_decode(tags["b"]).try_into().unwrap();
        let header_hash: [u8; 32] = sha2::Sha256::digest(signing_input.as_bytes()).into();
        ed25519_dalek::SigningKey::from_bytes(&seed).verifying_key()
            .verify_strict(&header_hash, &ed25519_dalek::Signature::from_bytes(&signature))
            .unwrap();
    }

    #[test]
    fn mailer_real_tls_starttls_cancellation_deadline_and_unknown_boundaries() {
        use smtp_adapter::jet_email as email;
        SMTP_CANCELLED.store(false, std::sync::atomic::Ordering::SeqCst);
        SMTP_DEADLINE_MS.store(i64::MAX, std::sync::atomic::Ordering::SeqCst);
        for starttls in [false, true] {
            let (address, transcript, server) = spawn_smtp_server(starttls, SmtpFixture::Success);
            let config = smtp_config(address.port(), starttls);
            let mut mailer = email::smtp(&config, |secret| secret.clone(), smtp_runtime()).unwrap();
            let report = mailer.send(smtp_message()).unwrap();
            assert_eq!((report.response_code, report.accepted.len(), report.rejected.len()), (250, 1, 0));
            server.join().unwrap();
            let transcript = transcript.lock().unwrap().join("");
            assert_eq!(transcript.matches("DATA\r\n").count(), 1);
            assert!(!transcript.contains("swordfish"));
            if starttls { assert!(transcript.contains("STARTTLS\r\nEHLO localhost\r\n")); }
        }

        let (address, transcript, server) = spawn_smtp_server(false, SmtpFixture::CloseAfterData);
        let config = smtp_config(address.port(), false);
        let mut mailer = email::smtp(&config, |secret| secret.clone(), smtp_runtime()).unwrap();
        assert!(matches!(mailer.send(smtp_message()), Err(email::Error::DeliveryUnknown { .. })));
        server.join().unwrap();
        assert_eq!(transcript.lock().unwrap().join("").matches("DATA\r\n").count(), 1);

        let (address, _, server) = spawn_smtp_server(false, SmtpFixture::RejectAuth);
        let config = smtp_config(address.port(), false);
        let mut mailer = email::smtp(&config, |secret| secret.clone(), smtp_runtime()).unwrap();
        let auth_error = mailer.send(smtp_message()).unwrap_err();
        assert!(matches!(auth_error, email::Error::Auth { .. }));
        assert!(!format!("{auth_error:?}").contains("swordfish"));
        server.join().unwrap();

        let (address, _, server) = spawn_smtp_server(false, SmtpFixture::StallGreeting);
        let config = smtp_config(address.port(), false);
        let mut mailer = email::smtp(&config, |secret| secret.clone(), smtp_runtime()).unwrap();
        let cancel = std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(60));
            SMTP_CANCELLED.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        assert!(matches!(mailer.send(smtp_message()), Err(email::Error::Cancelled { .. })));
        cancel.join().unwrap(); server.join().unwrap();
        SMTP_CANCELLED.store(false, std::sync::atomic::Ordering::SeqCst);

        let (address, _, server) = spawn_smtp_server(false, SmtpFixture::StallGreeting);
        SMTP_DEADLINE_MS.store(smtp_now_ms() + 60, std::sync::atomic::Ordering::SeqCst);
        let config = smtp_config(address.port(), false);
        let mut mailer = email::smtp(&config, |secret| secret.clone(), smtp_runtime()).unwrap();
        assert!(matches!(mailer.send(smtp_message()), Err(email::Error::TimedOut { .. })));
        server.join().unwrap();
        SMTP_DEADLINE_MS.store(i64::MAX, std::sync::atomic::Ordering::SeqCst);
    }

    #[test]
    fn mailer_dkim_signs_final_wire_with_real_ed25519_verifier() {
        use smtp_adapter::jet_email as email;
        let seed = [7u8; 32];
        let mut wires = Vec::new();
        for _ in 0..2 {
            let (address, transcript, server) = spawn_smtp_server(false, SmtpFixture::Success);
            let mut config = smtp_config(address.port(), false);
            config.dkim = Some(email::DkimConfig {
                domain: "example.com".to_string(), selector: "login-2026".to_string(),
                private_key: seed.to_vec(),
                signed_headers: ["from", "to", "subject", "mime-version", "content-type",
                    "content-transfer-encoding"].iter().map(|value| value.to_string()).collect(),
            });
            let mut mailer = email::smtp(&config, |secret| secret.clone(), smtp_runtime()).unwrap();
            mailer.send(smtp_message()).unwrap();
            server.join().unwrap();
            let transcript = transcript.lock().unwrap();
            let stuffed = transcript.iter().find(|part| part.starts_with("DKIM-Signature:")).unwrap();
            wires.push(stuffed.strip_suffix(".\r\n").unwrap().replace("\r\n..", "\r\n."));
        }
        assert_eq!(wires[0], wires[1], "DKIM ordering and folding must be deterministic");
        verify_dkim_wire(&wires[0], seed);
        let mut relaxed_variant = wires[0].replace("\r\nFrom: ", "\r\nfRoM:\t  ");
        let body = relaxed_variant.find("\r\n\r\n").unwrap() + 4;
        let first_body_line = relaxed_variant[body..].find("\r\n").unwrap() + body;
        relaxed_variant.insert_str(first_body_line, " \t");
        verify_dkim_wire(&relaxed_variant, seed);
        assert!(wires[0].starts_with("DKIM-Signature: v=1; a=ed25519-sha256; c=relaxed/relaxed;\r\n"));
    }

    #[test]
    fn mailer_dkim_rejects_before_connect_and_wipes_every_key_copy() {
        use smtp_adapter::jet_email as email;
        SMTP_WIPES.store(0, std::sync::atomic::Ordering::SeqCst);
        let mut config = smtp_config(465, false);
        config.dkim = Some(email::DkimConfig {
            domain: "example.com".to_string(), selector: "login-2026".to_string(),
            private_key: vec![0x5a; 31], signed_headers: vec!["from".to_string()],
        });
        let error = match email::smtp(&config, |secret| secret.clone(), smtp_counting_runtime()) {
            Err(error) => error,
            Ok(_) => panic!("31-byte DKIM seed was accepted"),
        };
        assert!(matches!(&error, email::Error::Configuration { operation, reason, .. }
            if operation == "dkim" && reason == "private_key must contain exactly 32 bytes"));
        assert!(!format!("{error:?}").contains("5a5a"));
        assert_eq!(SMTP_WIPES.load(std::sync::atomic::Ordering::SeqCst), 2,
            "construction failure must wipe copied SMTP password and DKIM seed");

        config.dkim.as_mut().unwrap().private_key.push(0x5a);
        let mailer = email::smtp(&config, |secret| secret.clone(), smtp_counting_runtime()).unwrap();
        drop(mailer);
        assert_eq!(SMTP_WIPES.load(std::sync::atomic::Ordering::SeqCst), 4,
            "Mailer drop must wipe owned SMTP password and DKIM seed");

        config.dkim.as_mut().unwrap().signed_headers.push("x-absent".to_string());
        let mut mailer = email::smtp(&config, |secret| secret.clone(), smtp_counting_runtime()).unwrap();
        let error = mailer.send(smtp_message()).unwrap_err();
        assert!(matches!(error, email::Error::Configuration { operation, reason, .. }
            if operation == "dkim" && reason.contains("x-absent") && reason.contains("absent")));
        drop(mailer);
        assert_eq!(SMTP_WIPES.load(std::sync::atomic::Ordering::SeqCst), 6);
    }

    #[test]
    fn smtp_from_env_dkim_is_all_or_none_and_redacted() {
        use smtp_adapter::jet_email as email;
        let _guard = SMTP_ENV_LOCK.lock().unwrap();
        for name in ["SMTP_DKIM_DOMAIN", "SMTP_DKIM_SELECTOR",
            "SMTP_DKIM_PRIVATE_KEY_BASE64", "SMTP_DKIM_SIGNED_HEADERS"]
        {
            std::env::remove_var(name);
        }
        std::env::set_var("SMTP_HOST", "localhost");
        std::env::set_var("SMTP_SECURITY", "tls");
        std::env::set_var("SMTP_PORT", "465");
        std::env::set_var("SMTP_DKIM_DOMAIN", "example.com");
        let partial = match email::smtp_from_env(smtp_runtime()) {
            Err(error) => error,
            Ok(_) => panic!("partial DKIM environment was accepted"),
        };
        assert!(matches!(partial, email::Error::Configuration { operation, reason, .. }
            if operation == "smtp_from_env" && reason.contains("must be set together")));

        std::env::set_var("SMTP_DKIM_SELECTOR", "login-2026");
        std::env::set_var("SMTP_DKIM_PRIVATE_KEY_BASE64", "private-key-not-base64");
        let malformed = match email::smtp_from_env(smtp_runtime()) {
            Err(error) => error,
            Ok(_) => panic!("malformed DKIM environment was accepted"),
        };
        assert!(!format!("{malformed:?}").contains("private-key-not-base64"));

        std::env::set_var("SMTP_DKIM_PRIVATE_KEY_BASE64", "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=");
        let mailer = email::smtp_from_env(smtp_runtime()).unwrap();
        drop(mailer);
        for name in ["SMTP_HOST", "SMTP_SECURITY", "SMTP_PORT", "SMTP_DKIM_DOMAIN",
            "SMTP_DKIM_SELECTOR", "SMTP_DKIM_PRIVATE_KEY_BASE64", "SMTP_DKIM_SIGNED_HEADERS"]
        {
            std::env::remove_var(name);
        }
    }
}

/// Crate dependency specs that require non-trivial TOML values (e.g. feature flags).
/// These are emitted verbatim as the right-hand side of the `name = …` line.
const FEATURED_DEPS: &[(&str, &str)] = &[
    (
        "rusqlite",
        "{ version = \"0.31\", features = [\"bundled\"] }",
    ),
    (
        "ureq",
        "{ version = \"2\", default-features = false, features = [\"tls\", \"native-certs\"] }",
    ),
    (
        "rustls",
        "{ version = \"0.23\", default-features = false, features = [\"ring\", \"std\", \"tls12\"] }",
    ),
    (
        "wasmtime",
        "{ version = \"26\", features = [\"component-model\"] }",
    ),
];

/// Canonical vendored `core.archive` implementation, compiled both by
/// `CoreProvider` and as the hidden bridge fallback. Keeping one source prevents
/// the offline ring package and direct `jet run` path from drifting apart.
const ARCHIVE_RUNTIME: &str =
    include_str!("../../../corelib/core.archive/pkgs/archive/src/lib.rs");

/// Hand-written database runtime emitted into the bridge crate when `jet.db`
/// is used. This is the only code that touches the `rusqlite` crate.
const DB_RUNTIME: &str = include_str!("Prelude/Db.rs");

/// The `aes-gcm` crate version backing `core.crypto` envelope (D-DEP-CRYPTO1).
pub const AES_GCM_CRATE_SPEC: (&str, &str) = ("aes-gcm", "0.10");

/// The `chacha20poly1305` crate version backing `core.crypto` envelope (D-DEP-CRYPTO1).
pub const CHACHA_POLY_CRATE_SPEC: (&str, &str) = ("chacha20poly1305", "0.10");

/// The `ed25519-dalek` crate version backing `core.crypto.sign/verify` (D-DEP-CRYPTO1).
pub const ED25519_CRATE_SPEC: (&str, &str) = ("ed25519-dalek", "2");

/// The `argon2` crate version backing `core.crypto.password_hash` (D-PWHASH1).
pub const ARGON2_CRATE_SPEC: (&str, &str) = ("argon2", "0.5");

/// The `sha2` crate version backing SHA-512 + HKDF-SHA256 (D-CRYPTO-SUITE1).
pub const SHA2_CRATE_SPEC: (&str, &str) = ("sha2", "0.10");

/// The `blake3` crate version backing `core.crypto.blake3_bytes` (D-CRYPTO-SUITE1).
pub const BLAKE3_CRATE_SPEC: (&str, &str) = ("blake3", "1");

/// The `hkdf` crate version backing `core.crypto.hkdf_sha256` (D-CRYPTO-SUITE1).
pub const HKDF_CRATE_SPEC: (&str, &str) = ("hkdf", "0.12");

/// The `x25519-dalek` crate version backing key agreement (D-CRYPTO-SUITE1).
pub const X25519_CRATE_SPEC: (&str, &str) = ("x25519-dalek", "2");

/// Constant-time byte equality helper (D-CRYPTO-SUITE1).
pub const SUBTLE_CRATE_SPEC: (&str, &str) = ("subtle", "2");

/// Hand-written crypto runtime emitted into the bridge crate when `core.crypto`
/// seal/open/sign/verify is used (D-CRYPTOENV1, D-DEP-CRYPTO1).
const CRYPTO_RUNTIME: &str = include_str!("Prelude/Crypto.rs");
const CRYPTO_ENTROPY_RUNTIME: &str =
    include_str!("../../jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs");

/// The `wasmtime` crate version that backs `core.plugin` (D-DEP-WASM1=A, c81).
/// Lives only here — never in the compiler's Cargo.toml (I6). Reuses the
/// already-approved Cranelift backend internally (D-JITDEP1).
pub const WASMTIME_CRATE_SPEC: (&str, &str) = ("wasmtime", "26");

/// Hand-written plugin-loader runtime emitted into the bridge crate when
/// `core.plugin` is used. This is the only place the `wasmtime` crate is
/// touched.
const PLUGIN_RUNTIME: &str = include_str!("Prelude/Plugin.rs");

/// The `age` crate version backing `core.vault` (U13, D-JPK-SECRETCRYPTO1=A) —
/// the age-style crypto bridge: X25519 recipients, ChaCha20-Poly1305 payload.
/// Lives only here — never in the compiler's Cargo.toml (I6).
pub const AGE_CRATE_SPEC: (&str, &str) = ("age", "0.10");

/// Hand-written age-style crypto runtime emitted into the bridge crate when
/// `core.vault` is used (U13). This is the only place the `age` crate is
/// touched.
const SECRETS_RUNTIME: &str = include_str!("Prelude/SecretsCrypto.rs");

/// The `flate2` crate version that backs canonical `core.compress.gzip`
/// (D-CORE-COMPRESS1=A / D-CODECS1). Lives only here — never in the compiler's
/// Cargo.toml (I6).
pub const COMPRESS_GZIP_CRATE_SPEC: (&str, &str) = ("flate2", "1");

/// The `zstd` crate version that backs `core.compress.zstd` (D-CODECS1). Pure
/// bootstrap dep: the `zstd` crate is a Rust binding that vendors/builds the C
/// zstd source via `zstd-sys` at compile time (same I6 bootstrap-exception
/// posture as `rusqlite`'s bundled SQLite, `DB_CRATE_SPEC`). Lives only here —
/// never in the compiler's Cargo.toml (I6).
pub const COMPRESS_ZSTD_CRATE_SPEC: (&str, &str) = ("zstd", "0.13");

/// Hand-written compression runtime emitted into the bridge crate when
/// `core.compress.gzip` or `core.compress.zstd` is used (D-CODECS1). This is
/// the only place the standalone codec paths touch `flate2` / `zstd`.
const COMPRESS_RUNTIME: &str = include_str!("Prelude/Compress.rs");

pub fn build_bridge(
    entries: &[ExternEntry],
    needs_regex: bool,
    needs_archive: bool,
    needs_db: bool,
    needs_http_client: bool,
    needs_crypto: bool,
    needs_compress: bool,
    needs_plugin: bool,
    needs_secrets: bool,
) -> Result<FfiLink, Vec<Diagnostic>> {
    build_bridge_full(
        entries,
        needs_regex,
        needs_archive,
        needs_db,
        needs_http_client,
        false,
        false,
        needs_crypto,
        needs_compress,
        needs_plugin,
        needs_secrets,
    )
}

/// Path of the already-built Ed25519 helper, without creating cache state.
/// Health checks use this instead of calling `build_bridge`.
pub fn cached_crypto_helper_path() -> PathBuf {
    let mut deps = BTreeMap::new();
    for (name, version) in [
        AES_GCM_CRATE_SPEC, CHACHA_POLY_CRATE_SPEC, ED25519_CRATE_SPEC,
        ARGON2_CRATE_SPEC, SHA2_CRATE_SPEC, BLAKE3_CRATE_SPEC,
        HKDF_CRATE_SPEC, X25519_CRATE_SPEC, SUBTLE_CRATE_SPEC,
    ] {
        deps.insert(name.to_string(), version.to_string());
    }
    let key = cache_key_full(
        &[], &deps, false, false, false, false, false, false, true, false, false, false,
    );
    cache_dir().join(format!("{key:016x}"))
        .join("target/release/jet-crypto-helper")
}

fn build_bridge_full(
    entries: &[ExternEntry],
    needs_regex: bool,
    needs_archive: bool,
    needs_db: bool,
    needs_http_client: bool,
    needs_http_server_tls: bool,
    needs_net_tls: bool,
    needs_crypto: bool,
    needs_compress: bool,
    needs_plugin: bool,
    needs_secrets: bool,
) -> Result<FfiLink, Vec<Diagnostic>> {
    let mut deps = collect_crate_deps(entries);
    if needs_archive {
        deps.insert(ZIP_CRATE_SPEC.0.to_string(), ZIP_CRATE_SPEC.1.to_string());
        deps.insert(TAR_CRATE_SPEC.0.to_string(), TAR_CRATE_SPEC.1.to_string());
    }
    if needs_db {
        deps.insert(DB_CRATE_SPEC.0.to_string(), DB_CRATE_SPEC.1.to_string());
    }
    if needs_http_client {
        deps.insert(
            HTTP_CLIENT_CRATE_SPEC.0.to_string(),
            HTTP_CLIENT_CRATE_SPEC.1.to_string(),
        );
    }
    if needs_http_server_tls {
        deps.insert(
            HTTP_SERVER_TLS_CRATE_SPEC.0.to_string(),
            HTTP_SERVER_TLS_CRATE_SPEC.1.to_string(),
        );
        deps.insert(
            HTTP_SERVER_TLS_PEMFILE_CRATE_SPEC.0.to_string(),
            HTTP_SERVER_TLS_PEMFILE_CRATE_SPEC.1.to_string(),
        );
    }
    if needs_net_tls {
        deps.insert(
            HTTP_SERVER_TLS_CRATE_SPEC.0.to_string(),
            HTTP_SERVER_TLS_CRATE_SPEC.1.to_string(),
        );
        deps.insert(
            RUSTLS_NATIVE_CERTS_CRATE_SPEC.0.to_string(),
            RUSTLS_NATIVE_CERTS_CRATE_SPEC.1.to_string(),
        );
        deps.insert(
            HTTP_SERVER_TLS_PEMFILE_CRATE_SPEC.0.to_string(),
            HTTP_SERVER_TLS_PEMFILE_CRATE_SPEC.1.to_string(),
        );
    }
    if needs_crypto {
        deps.insert(
            AES_GCM_CRATE_SPEC.0.to_string(),
            AES_GCM_CRATE_SPEC.1.to_string(),
        );
        deps.insert(
            CHACHA_POLY_CRATE_SPEC.0.to_string(),
            CHACHA_POLY_CRATE_SPEC.1.to_string(),
        );
        deps.insert(
            ED25519_CRATE_SPEC.0.to_string(),
            ED25519_CRATE_SPEC.1.to_string(),
        );
        deps.insert(
            ARGON2_CRATE_SPEC.0.to_string(),
            ARGON2_CRATE_SPEC.1.to_string(),
        );
        deps.insert(SHA2_CRATE_SPEC.0.to_string(), SHA2_CRATE_SPEC.1.to_string());
        deps.insert(
            BLAKE3_CRATE_SPEC.0.to_string(),
            BLAKE3_CRATE_SPEC.1.to_string(),
        );
        deps.insert(HKDF_CRATE_SPEC.0.to_string(), HKDF_CRATE_SPEC.1.to_string());
        deps.insert(
            X25519_CRATE_SPEC.0.to_string(),
            X25519_CRATE_SPEC.1.to_string(),
        );
        deps.insert(
            SUBTLE_CRATE_SPEC.0.to_string(),
            SUBTLE_CRATE_SPEC.1.to_string(),
        );
    }
    if needs_compress {
        // D-CORE-COMPRESS1=A: only core.compress pulls stream-codec deps.
        deps.insert(
            COMPRESS_GZIP_CRATE_SPEC.0.to_string(),
            COMPRESS_GZIP_CRATE_SPEC.1.to_string(),
        );
        deps.insert(
            COMPRESS_ZSTD_CRATE_SPEC.0.to_string(),
            COMPRESS_ZSTD_CRATE_SPEC.1.to_string(),
        );
    }
    if needs_plugin {
        deps.insert(
            WASMTIME_CRATE_SPEC.0.to_string(),
            WASMTIME_CRATE_SPEC.1.to_string(),
        );
    }
    if needs_secrets {
        deps.insert(AGE_CRATE_SPEC.0.to_string(), AGE_CRATE_SPEC.1.to_string());
    }
    let key = cache_key_full(
        entries,
        &deps,
        needs_regex,
        needs_archive,
        needs_db,
        needs_http_client,
        needs_http_server_tls,
        needs_net_tls,
        needs_crypto,
        needs_compress,
        needs_plugin,
        needs_secrets,
    );
    let cache_root = cache_dir().join(format!("{:016x}", key));
    let crate_name = format!("jet_ffi_{:016x}", key);
    let target_dir = cache_root.join("target");
    let target = target_dir.join("release");
    let rlib = target.join(format!("lib{}.rlib", crate_name));
    let deps_dir = target.join("deps");

    // c146: when the bridge carries crypto, it also emits a `jet-crypto-helper`
    // binary (a thin stdin wrapper around `jet_crypto_*_impl`) that `jet`'s own
    // publish/keygen path shells out to. Its path is fixed by the bin target name.
    let helper_bin = if needs_crypto {
        Some(target.join("jet-crypto-helper"))
    } else {
        None
    };
    // U13: same shape, a `jet-secrets-helper` binary when the bridge carries the
    // age-style crypto bridge — `jetpack secrets *` shells out to it.
    let secrets_helper_bin = if needs_secrets {
        Some(target.join("jet-secrets-helper"))
    } else {
        None
    };
    // The build is only "cached and ready" when the rlib exists AND — if a helper
    // is expected — the helper binary exists too (a bridge cached before c146
    // landed would have the rlib but no helper, so fall through to rebuild).
    let helper_ready = helper_bin.as_ref().map(|p| p.is_file()).unwrap_or(true)
        && secrets_helper_bin
            .as_ref()
            .map(|p| p.is_file())
            .unwrap_or(true);

    // Fast path (cache hit): the key is content-derived from the exact
    // extern-rust signature / dep set, so an rlib already sitting at this
    // path is a valid build for this call — reuse it without touching
    // `cargo` or rewriting the cached sources. No lock needed: we don't
    // write anything.
    if rlib.is_file() && helper_ready {
        return Ok(FfiLink {
            crate_name,
            rlib_path: rlib,
            deps_dir,
            helper_bin_path: helper_bin,
            secrets_helper_bin_path: secrets_helper_bin,
        });
    }

    if !command_exists("cargo") {
        return Err(vec![Diagnostic::error(
            "E0703",
            "can't call foreign Rust crates without `cargo`".to_string(),
            "Jet builds a small helper crate for each `extern rust` dependency set".to_string(),
            "install Rust from https://rustup.rs (this includes `cargo`), then try again"
                .to_string(),
            None,
        )]);
    }

    fs::create_dir_all(&cache_root)
        .map_err(|e| tool_error(&format!("couldn't create the FFI cache folder: {}", e)))?;

    // Slow path (cache miss): another `jet` process may be building this same
    // key right now. Cargo's `CARGO_TARGET_DIR` lock protects `target/`, not
    // the `Cargo.toml`/`src/lib.rs` sources this function is about to
    // (re)write, so guard the write+build with our own cross-process lock,
    // scoped to this cache key — two processes on *different* keys never
    // block each other.
    let _lock = BuildLock::acquire(&cache_root)?;

    // Re-check under the lock: whoever held it may have just finished
    // building this exact key while we were waiting.
    if rlib.is_file() && helper_ready {
        return Ok(FfiLink {
            crate_name,
            rlib_path: rlib,
            deps_dir,
            helper_bin_path: helper_bin,
            secrets_helper_bin_path: secrets_helper_bin,
        });
    }

    let src_dir = cache_root.join("src");
    fs::create_dir_all(&src_dir)
        .map_err(|e| tool_error(&format!("couldn't create the FFI build folder: {}", e)))?;

    let manifest = cache_root.join("Cargo.toml");
    let lib_rs = src_dir.join("lib.rs");
    fs::write(&manifest, emit_cargo_toml(&crate_name, &deps))
        .map_err(|e| tool_error(&format!("couldn't write the FFI manifest: {}", e)))?;
    fs::write(
        &lib_rs,
        emit_wrapper_lib(
            entries,
            needs_regex,
            needs_archive,
            needs_db,
            needs_http_client,
            needs_http_server_tls,
            needs_net_tls,
            needs_crypto,
            needs_compress,
            needs_plugin,
            needs_secrets,
        ),
    )
    .map_err(|e| tool_error(&format!("couldn't write the FFI wrappers: {}", e)))?;

    // c146: emit the crypto helper binary alongside the lib when crypto is in play.
    if needs_crypto {
        let bin_dir = src_dir.join("bin");
        fs::create_dir_all(&bin_dir)
            .map_err(|e| tool_error(&format!("couldn't create the FFI bin folder: {}", e)))?;
        fs::write(
            bin_dir.join("jet-crypto-helper.rs"),
            emit_crypto_helper_bin(&crate_name),
        )
        .map_err(|e| tool_error(&format!("couldn't write the crypto helper: {}", e)))?;
    }
    // U13: same shape, the secrets helper binary when the age-style crypto
    // bridge is in play.
    if needs_secrets {
        let bin_dir = src_dir.join("bin");
        fs::create_dir_all(&bin_dir)
            .map_err(|e| tool_error(&format!("couldn't create the FFI bin folder: {}", e)))?;
        fs::write(
            bin_dir.join("jet-secrets-helper.rs"),
            emit_secrets_helper_bin(&crate_name),
        )
        .map_err(|e| tool_error(&format!("couldn't write the secrets helper: {}", e)))?;
    }

    let out = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(&manifest)
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .map_err(|e| {
            vec![Diagnostic::error(
                "E0703",
                format!("couldn't run `cargo`: {}", e),
                "Jet needs `cargo` to build foreign crate wrappers".to_string(),
                "install Rust from https://rustup.rs, then try again".to_string(),
                None,
            )]
        })?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if looks_like_signature_mismatch(&stderr) {
            let hint = entries
                .first()
                .map(|e| e.line_hint.clone())
                .unwrap_or_else(|| "an `extern rust` declaration".to_string());
            return Err(vec![Diagnostic::error(
                "E0705",
                format!(
                    "the Rust item doesn't match the Jet signature at {}",
                    hint
                ),
                "the `= \"rust::path\"` on an `extern rust` line must name a safe Rust function whose parameters and return type match what you wrote"
                    .to_string(),
                format!(
                    "check the path and types on {}, or pick a different Rust function",
                    hint
                ),
                None,
            )
            .with_detail(format!("  cargo said:\n{}", stable_cargo_detail(&stderr)))]);
        }
        let dep = deps
            .keys()
            .next()
            .map(|k| format!("{}@{}", k, deps[k]))
            .unwrap_or_else(|| "a foreign crate".to_string());
        return Err(vec![Diagnostic::error(
            "E0704",
            format!("couldn't fetch or build `{}`", dep),
            "pure-Rust crates only — crates that need system libraries or a build script aren't supported yet"
                .to_string(),
            "try a different crate version, check your network, or pick another crate"
                .to_string(),
            None,
        )
        .with_detail(format!("  cargo said:\n{}", stable_cargo_detail(&stderr)))]);
    }

    if !rlib.is_file() {
        return Err(tool_error(&format!(
            "FFI build finished but `{}` is missing",
            rlib.display()
        )));
    }
    if let Some(bin) = &helper_bin {
        if !bin.is_file() {
            return Err(tool_error(&format!(
                "FFI build finished but the crypto helper `{}` is missing",
                bin.display()
            )));
        }
    }
    if let Some(bin) = &secrets_helper_bin {
        if !bin.is_file() {
            return Err(tool_error(&format!(
                "FFI build finished but the secrets helper `{}` is missing",
                bin.display()
            )));
        }
    }
    Ok(FfiLink {
        crate_name,
        rlib_path: rlib,
        deps_dir,
        helper_bin_path: helper_bin,
        secrets_helper_bin_path: secrets_helper_bin,
    })
}

/// The crypto helper binary source (c146). A thin stdin-protocol wrapper around
/// the crate's own `jet_crypto_*_impl` functions (which is the *only* code that
/// touches `ed25519-dalek`, D-DEP-CRYPTO1). `jet` shells out to this exactly as
/// it already shells out to `cargo`/`rustc`, keeping the compiler crate itself
/// zero-dependency (I6). Protocol (one command line on stdin, hex-encoded args):
///   `keygen`                          → stdout `<seed_hex> <pub_hex>`
///   `sign <seed_hex> <msg_hex>`       → stdout `<sig_hex>`         (exit 0)
///   `verify <pub_hex> <msg_hex> <sig_hex>` → exit 0 valid / 2 invalid / 1 error
fn emit_crypto_helper_bin(crate_name: &str) -> String {
    format!(
        r#"// Auto-generated Ed25519 signing helper (card c146) — do not edit.
#![allow(warnings)]
use std::io::{{Read, Write}};
use std::process::exit;

const ENTROPY_UNAVAILABLE: i32 = 75;

fn volatile_zeroize(bytes: &mut [u8]) {{
    for byte in bytes {{
        unsafe {{ std::ptr::write_volatile(byte, 0) }};
    }}
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}}

fn write_hex(out: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {{
    for byte in bytes {{
        write!(out, "{{:02x}}", byte)?;
    }}
    Ok(())
}}

fn hex_encode(bytes: &[u8]) -> String {{
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {{
        s.push_str(&format!("{{:02x}}", b));
    }}
    s
}}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {{
    let s = s.trim();
    if s.len() % 2 != 0 {{
        return Err("odd-length hex".to_string());
    }}
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {{
        let hi = (bytes[i] as char).to_digit(16).ok_or("bad hex digit")?;
        let lo = (bytes[i + 1] as char).to_digit(16).ok_or("bad hex digit")?;
        out.push(((hi << 4) | lo) as u8);
        i += 2;
    }}
    Ok(out)
}}

fn fail(msg: &str) -> ! {{
    eprintln!("error: {{}}", msg);
    exit(1);
}}

fn main() {{
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {{
        fail("couldn't read stdin");
    }}
    let mut parts = input.split_whitespace();
    let cmd = parts.next().unwrap_or("");
    match cmd {{
        "keygen" => {{
            match {crate_name}::jet_crypto_keygen_impl() {{
                Ok((mut seed, public)) => {{
                    let result = (|| -> std::io::Result<()> {{
                        let stdout = std::io::stdout();
                        let mut out = stdout.lock();
                        write_hex(&mut out, &seed)?;
                        out.write_all(b" ")?;
                        write_hex(&mut out, &public)?;
                        out.write_all(b"\n")?;
                        out.flush()
                    }})();
                    volatile_zeroize(&mut seed);
                    if result.is_err() {{ exit(1); }}
                }}
                Err(_) => exit(ENTROPY_UNAVAILABLE),
            }}
        }}
        "sign" => {{
            let seed = parts.next().unwrap_or_else(|| fail("sign: missing key"));
            let msg = parts.next().unwrap_or_else(|| fail("sign: missing message"));
            let seed = hex_decode(seed).unwrap_or_else(|e| fail(&e));
            let msg = hex_decode(msg).unwrap_or_else(|e| fail(&e));
            match {crate_name}::jet_crypto_sign_impl(&seed, &msg) {{
                Ok(sig) => println!("{{}}", hex_encode(&sig)),
                Err(e) => fail(&e),
            }}
        }}
        "verify" => {{
            let pk = parts.next().unwrap_or_else(|| fail("verify: missing public key"));
            let msg = parts.next().unwrap_or_else(|| fail("verify: missing message"));
            let sig = parts.next().unwrap_or_else(|| fail("verify: missing signature"));
            let pk = hex_decode(pk).unwrap_or_else(|e| fail(&e));
            let msg = hex_decode(msg).unwrap_or_else(|e| fail(&e));
            let sig = hex_decode(sig).unwrap_or_else(|e| fail(&e));
            match {crate_name}::jet_crypto_verify_impl(&pk, &msg, &sig) {{
                Ok(()) => exit(0),
                Err(_) => exit(2),
            }}
        }}
        other => fail(&format!("unknown command `{{}}`", other)),
    }}
}}
"#,
        crate_name = crate_name
    )
}

/// The secrets helper binary source (U13, D-JPK-SECRETCRYPTO1). A thin
/// stdin-protocol wrapper around the crate's own `jet_vault_*_impl`
/// functions (the *only* code that touches the `age` crate). `jetpack secrets
/// set/get/recipients/keygen` shells out to this exactly as `jet` already
/// shells out to `cargo`/`rustc`, keeping `crates/jet-driver` zero-dependency
/// (I6). Protocol (one command line on stdin, hex-encoded byte args, plain
/// age strings for identities/recipients since neither ever contains a space):
///   `keygen`                                       → stdout `<identity> <recipient>`
///   `encrypt <recipients_csv> <plaintext_hex>`      → stdout `<ciphertext_hex>` (exit 0) / exit 1 error
///   `decrypt <identity> <ciphertext_hex>`           → stdout `<plaintext_hex>`  (exit 0) / exit 1 error
fn emit_secrets_helper_bin(crate_name: &str) -> String {
    format!(
        r#"// Auto-generated age-style secrets helper (U13) — do not edit.
#![allow(warnings)]
use std::io::Read;
use std::process::exit;

fn hex_encode(bytes: &[u8]) -> String {{
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {{
        s.push_str(&format!("{{:02x}}", b));
    }}
    s
}}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {{
    let s = s.trim();
    if s.len() % 2 != 0 {{
        return Err("odd-length hex".to_string());
    }}
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {{
        let hi = (bytes[i] as char).to_digit(16).ok_or("bad hex digit")?;
        let lo = (bytes[i + 1] as char).to_digit(16).ok_or("bad hex digit")?;
        out.push(((hi << 4) | lo) as u8);
        i += 2;
    }}
    Ok(out)
}}

fn fail(msg: &str) -> ! {{
    eprintln!("error: {{}}", msg);
    exit(1);
}}

fn main() {{
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {{
        fail("couldn't read stdin");
    }}
    let mut parts = input.split_whitespace();
    let cmd = parts.next().unwrap_or("");
    match cmd {{
        "keygen" => {{
            let (identity, recipient) = {crate_name}::jet_vault_keygen_impl();
            println!("{{}} {{}}", identity, recipient);
        }}
        "encrypt" => {{
            let recipients_csv = parts.next().unwrap_or_else(|| fail("encrypt: missing recipients"));
            let plaintext_hex = parts.next().unwrap_or_else(|| fail("encrypt: missing plaintext"));
            let recipients: Vec<String> = recipients_csv.split(',').map(|s| s.to_string()).collect();
            let plaintext = hex_decode(plaintext_hex).unwrap_or_else(|e| fail(&e));
            match {crate_name}::jet_vault_encrypt_impl(&recipients, &plaintext) {{
                Ok(ciphertext) => println!("{{}}", hex_encode(&ciphertext)),
                Err(e) => fail(&e),
            }}
        }}
        "decrypt" => {{
            let identity = parts.next().unwrap_or_else(|| fail("decrypt: missing identity")).to_string();
            let ciphertext_hex = parts.next().unwrap_or_else(|| fail("decrypt: missing ciphertext"));
            let ciphertext = hex_decode(ciphertext_hex).unwrap_or_else(|e| fail(&e));
            match {crate_name}::jet_vault_decrypt_impl(&identity, &ciphertext) {{
                Ok(plaintext) => println!("{{}}", hex_encode(&plaintext)),
                Err(e) => fail(&e),
            }}
        }}
        other => fail(&format!("unknown command `{{}}`", other)),
    }}
}}
"#,
        crate_name = crate_name
    )
}

/// Cross-process lock guarding the slow path (rewrite + `cargo build`) of the
/// FFI bridge cache for one cache key. Same atomic `create_dir` + stale-steal
/// shape as `tests/common/mod.rs` `FfiBridgeLock`, kept as a *separate* lock
/// (different failure domain: this one guards real concurrent `jet`
/// processes; the test lock also serializes different test *binaries* in the
/// same suite run) — scoped per cache key rather than global, and
/// error-returning instead of panicking, since this runs in the compiler
/// itself and must never crash a build (I2: no path here may surface as an
/// internal panic in place of a diagnostic).
struct BuildLock {
    dir: PathBuf,
}

impl BuildLock {
    /// Blocks until the lock is held. Steals a stale lock (mtime older than 2
    /// minutes — far longer than any single FFI bridge `cargo build` takes)
    /// so a killed/timed-out `jet` process can't wedge every later build on
    /// this key.
    fn acquire(cache_root: &std::path::Path) -> Result<BuildLock, Vec<Diagnostic>> {
        let dir = cache_root.join(".build-lock");
        loop {
            match fs::create_dir(&dir) {
                Ok(()) => return Ok(BuildLock { dir }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if let Ok(meta) = fs::metadata(&dir) {
                        if let Ok(age) = meta.modified().and_then(|m| {
                            m.elapsed()
                                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                        }) {
                            if age > std::time::Duration::from_secs(120) {
                                let _ = fs::remove_dir(&dir);
                                continue;
                            }
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => {
                    return Err(tool_error(&format!(
                        "couldn't lock the FFI cache folder: {}",
                        e
                    )));
                }
            }
        }
    }
}

impl Drop for BuildLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.dir);
    }
}

fn collect_crate_deps(entries: &[ExternEntry]) -> BTreeMap<String, String> {
    let mut deps = BTreeMap::new();
    for e in entries {
        if e.crate_spec == "std" {
            continue;
        }
        if let Some((name, ver)) = parse_crate_spec(&e.crate_spec) {
            deps.insert(name, ver);
        }
    }
    deps
}

/// `"std"` or `Some((name, version))` for `"name@version"`.
pub fn parse_crate_spec(spec: &str) -> Option<(String, String)> {
    if spec == "std" {
        return None;
    }
    let (name, ver) = spec.split_once('@')?;
    if name.is_empty() || ver.is_empty() {
        return None;
    }
    Some((name.to_string(), ver.to_string()))
}

fn cache_key_full(
    entries: &[ExternEntry],
    deps: &BTreeMap<String, String>,
    needs_regex: bool,
    needs_archive: bool,
    needs_db: bool,
    needs_http_client: bool,
    needs_http_server_tls: bool,
    needs_net_tls: bool,
    needs_crypto: bool,
    needs_compress: bool,
    needs_plugin: bool,
    needs_secrets: bool,
) -> u64 {
    let mut h = DefaultHasher::new();
    // Only perturb the key when a ring module is actually needed, so programs
    // without those modules keep their historical cache key. The dep is already
    // in `deps`; the flag guards the (currently impossible) empty-deps case.
    if needs_regex {
        needs_regex.hash(&mut h);
    }
    if needs_archive {
        needs_archive.hash(&mut h);
        ARCHIVE_RUNTIME.hash(&mut h);
    }
    if needs_db {
        needs_db.hash(&mut h);
        DB_RUNTIME.hash(&mut h);
    }
    if needs_http_client {
        needs_http_client.hash(&mut h);
        HTTP_CLIENT_RUNTIME.hash(&mut h);
    }
    if needs_http_server_tls {
        needs_http_server_tls.hash(&mut h);
        HTTP_SERVER_TLS_RUNTIME.hash(&mut h);
    }
    if needs_net_tls {
        needs_net_tls.hash(&mut h);
        NET_TLS_RUNTIME.hash(&mut h);
    }
    if needs_crypto {
        needs_crypto.hash(&mut h);
        CRYPTO_RUNTIME.hash(&mut h);
        CRYPTO_ENTROPY_RUNTIME.hash(&mut h);
        // The helper is a separately cached binary. Its closed status protocol
        // and cleanup behavior must invalidate old cache entries too.
        emit_crypto_helper_bin("jet_ffi_cache_key").hash(&mut h);
    }
    if needs_compress {
        needs_compress.hash(&mut h);
        COMPRESS_RUNTIME.hash(&mut h);
    }
    if needs_plugin {
        needs_plugin.hash(&mut h);
        PLUGIN_RUNTIME.hash(&mut h);
    }
    if needs_secrets {
        needs_secrets.hash(&mut h);
        SECRETS_RUNTIME.hash(&mut h);
    }
    for (k, v) in deps {
        k.hash(&mut h);
        v.hash(&mut h);
    }
    for e in entries {
        e.wrapper_name.hash(&mut h);
        e.rust_path.hash(&mut h);
        e.crate_spec.hash(&mut h);
        for (c, t) in &e.params {
            format!("{:?}", c).hash(&mut h);
            type_key(t).hash(&mut h);
        }
        if let Some(rt) = &e.return_type {
            type_key(rt).hash(&mut h);
        }
    }
    h.finish()
}

fn type_key(ty: &Type) -> String {
    match ty {
        Type::Int => "Int".into(),
        Type::Float => "Float".into(),
        Type::IntN { signed, bits } => crate::AST::int_spelling(*signed, *bits),
        Type::Float32 => "F32".into(),
        Type::Bool => "Bool".into(),
        Type::String => "String".into(),
        Type::Char => "Char".into(),
        Type::List(inner) => format!("List<{}>", type_key(inner)),
        Type::Map { key, value, .. } => format!("Map<{},{}>", type_key(key), type_key(value)),
        Type::Shared(inner) => format!("Shared<{}>", type_key(inner)),
        Type::Option(inner) => format!("{}?", type_key(inner)),
        Type::Result { ok, err } => format!("Result<{},{}>", type_key(ok), type_key(err)),
        Type::Fn { params, ret, .. } => {
            let ps = params.iter().map(type_key).collect::<Vec<_>>().join(",");
            let r = ret.as_ref().map(|t| type_key(t)).unwrap_or_default();
            format!("fn({ps})->{r}")
        }
        Type::Named(n) => n.clone(),
        Type::Apply { name, args } => format!(
            "{name}<{}>",
            args.iter().map(type_key).collect::<Vec<_>>().join(",")
        ),
        Type::TraitObject(t) => format!("dyn {}", t.join(" + ")),
        Type::Tuple(fields) => format!(
            "({})",
            fields
                .iter()
                .map(|(n, t)| format!("{n}:{}", type_key(t)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::FixedList { elem, len, .. } => format!("List<{}#{}>", type_key(elem), len),
        Type::Tagged { marker, inner } => format!("#{marker}:{}", type_key(inner)),
    }
}

fn cache_dir() -> PathBuf {
    dirs_home().join(".cache").join("jet").join("ffi")
}

fn dirs_home() -> PathBuf {
    if let Ok(h) = std::env::var("HOME") {
        return PathBuf::from(h);
    }
    if let Ok(h) = std::env::var("USERPROFILE") {
        return PathBuf::from(h);
    }
    PathBuf::from("/tmp")
}

fn command_exists(cmd: &str) -> bool {
    Command::new(cmd).arg("--version").output().is_ok()
}

fn emit_cargo_toml(crate_name: &str, deps: &BTreeMap<String, String>) -> String {
    let mut s = format!(
        "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"rlib\"]\n\n"
    );
    if !deps.is_empty() {
        s.push_str("[dependencies]\n");
        for (name, ver) in deps {
            // Some crates need feature flags or other TOML table syntax — check
            // the allowlist first; fall back to the plain `name = "version"` form.
            if let Some((_, toml_val)) = FEATURED_DEPS.iter().find(|(n, _)| *n == name) {
                s.push_str(&format!("{name} = {toml_val}\n"));
            } else {
                s.push_str(&format!("{name} = \"{ver}\"\n"));
            }
        }
    }
    s
}

fn emit_wrapper_lib(
    entries: &[ExternEntry],
    _needs_regex: bool,
    needs_archive: bool,
    needs_db: bool,
    needs_http_client: bool,
    needs_http_server_tls: bool,
    needs_net_tls: bool,
    needs_crypto: bool,
    needs_compress: bool,
    needs_plugin: bool,
    needs_secrets: bool,
) -> String {
    let mut out = String::from(
        "// Auto-generated FFI wrappers — do not edit.\n#![allow(warnings)]\n\nfn ffi_panic() -> ! {\n    eprintln!(\"panic: a foreign function panicked\");\n    std::process::exit(70);\n}\n\n",
    );
    if needs_archive {
        // D-CORE-COMPRESS1=A: archive runtime touches only zip/tar containers.
        out.push_str(ARCHIVE_RUNTIME);
        out.push('\n');
    }
    if needs_db {
        // D-DEP-DB1: the database runtime is the only place `rusqlite` is touched.
        out.push_str(DB_RUNTIME);
        out.push('\n');
    }
    if needs_http_client {
        // D-NETDEP1=A: the HTTP client runtime is the only place `ureq` is touched.
        out.push_str(HTTP_CLIENT_RUNTIME);
        out.push('\n');
    }
    if needs_http_server_tls {
        // D-TLSSERVE1=A: the server TLS runtime is the only place serving
        // touches rustls.
        out.push_str(HTTP_SERVER_TLS_RUNTIME);
        out.push('\n');
    }
    if needs_net_tls {
        // D-NETSOCKET1=A / D-TLS1=A: client stream TLS over an existing TcpStream.
        out.push_str(NET_TLS_RUNTIME);
        out.push('\n');
    }
    if needs_crypto {
        // D-DEP-CRYPTO1=A: the crypto runtime is the only place RustCrypto is touched.
        out.push_str(CRYPTO_ENTROPY_RUNTIME);
        out.push('\n');
        out.push_str(CRYPTO_RUNTIME);
        out.push('\n');
    }
    if needs_compress {
        // D-CODECS1: the compress runtime is the only place the standalone
        // `core.compress.gzip` / `core.compress.zstd` codec paths are touched.
        out.push_str(COMPRESS_RUNTIME);
        out.push('\n');
    }
    if needs_plugin {
        // D-DEP-WASM1=A: the plugin runtime is the only place `wasmtime` is touched.
        out.push_str(PLUGIN_RUNTIME);
        out.push('\n');
    }
    if needs_secrets {
        // U13 (D-JPK-SECRETCRYPTO1): the secrets runtime is the only place the
        // `age` crate is touched.
        out.push_str(SECRETS_RUNTIME);
        out.push('\n');
    }
    let mut names: HashSet<String> = HashSet::new();
    for e in entries {
        names.insert(e.jet_name.clone());
    }
    for e in entries {
        out.push_str(&emit_wrapper_fn(e, &names));
        out.push('\n');
    }
    out
}

fn emit_wrapper_fn(entry: &ExternEntry, user_types: &HashSet<String>) -> String {
    let params: Vec<String> = entry
        .params
        .iter()
        .enumerate()
        .map(|(i, (_, ty))| format!("p{i}: {}", rust_type(ty, user_types)))
        .collect();
    let ret = entry
        .return_type
        .as_ref()
        .map(|t| rust_type(t, user_types))
        .unwrap_or_else(|| "()".to_string());
    let call_args: Vec<String> = (0..entry.params.len()).map(|i| format!("p{i}")).collect();
    let rust_call = if ret == "()" {
        format!("{}({});", entry.rust_path, call_args.join(", "))
    } else {
        format!("{}({})", entry.rust_path, call_args.join(", "))
    };
    let body = if ret == "()" {
        format!(
            "match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {{\n        {rust_call}\n    }})) {{\n        Ok(()) => (),\n        Err(_) => ffi_panic(),\n    }}"
        )
    } else {
        format!(
            "match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {rust_call})) {{\n        Ok(v) => v,\n        Err(_) => ffi_panic(),\n    }}"
        )
    };
    format!(
        "pub fn {}({}){} {{\n    {body}\n}}\n",
        entry.wrapper_name,
        params.join(", "),
        if ret == "()" {
            String::new()
        } else {
            format!(" -> {ret}")
        }
    )
}

fn rust_type(ty: &Type, user_types: &HashSet<String>) -> String {
    match ty {
        Type::Int => "i64".to_string(),
        Type::Float => "f64".to_string(),
        Type::IntN { signed, bits } => format!("{}{}", if *signed { 'i' } else { 'u' }, bits),
        Type::Float32 => "f32".to_string(),
        Type::Bool => "bool".to_string(),
        Type::String => "String".to_string(),
        Type::Char => "char".to_string(),
        Type::List(inner) => format!("Vec<{}>", rust_type(inner, user_types)),
        Type::Map { key, value, .. } => format!(
            "std::collections::BTreeMap<{}, {}>",
            rust_type(key, user_types),
            rust_type(value, user_types)
        ),
        // D-MEM1 S6: the main codegen path (Codegen/Context.rs) now renders
        // `Type::Shared` as `jet_std::JetShared<T>` (`Arc<RwLock<T>>`), not a
        // bare `Arc<T>` — this C-FFI bridge type table is untested for
        // `Shared<T>` crossing the boundary (no test exercises it; a
        // concurrency handle in a C-ABI signature is not a realistic shape),
        // left as the pre-S6 mapping rather than guessed at.
        Type::Shared(inner) => format!("std::sync::Arc<{}>", rust_type(inner, user_types)),
        Type::Option(inner) => format!("Option<{}>", rust_type(inner, user_types)),
        Type::Result { ok, err } => format!(
            "Result<{}, {}>",
            rust_type(ok, user_types),
            rust_type(err, user_types)
        ),
        Type::Fn { .. } => "Box<dyn std::any::Any>".to_string(),
        Type::Named(name) if name == "Error" => "String".to_string(),
        Type::Named(name) if user_types.contains(name) => format!("user_{name}"),
        Type::Named(name) => name.clone(),
        Type::Apply { name, args } if user_types.contains(name) => format!(
            "user_{name}<{}>",
            args.iter()
                .map(|a| rust_type(a, user_types))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Apply { .. } | Type::TraitObject(_) | Type::Tuple(_) => {
            "Box<dyn std::any::Any>".to_string()
        }
        // D-FIXARR1: [T#N] lowers to a real Rust array [T; N] in FFI too.
        Type::FixedList { elem, len, .. } => format!("[{}; {}]", rust_type(elem, user_types), len),
        Type::Tagged { inner, .. } => rust_type(inner, user_types),
    }
}

fn looks_like_signature_mismatch(stderr: &str) -> bool {
    stderr.contains("E0308")
        || stderr.contains("E0277")
        || stderr.contains("E0061")
        || stderr.contains("E0425")
        || stderr.contains("cannot find")
        || stderr.contains("mismatched types")
        || stderr.contains("arguments to this function")
}

fn indent_block(s: &str) -> String {
    s.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("    {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Keep cargo output stable for ui snapshots — drop fetch noise and parallel-build chatter.
fn stable_cargo_detail(stderr: &str) -> String {
    let kept: Vec<String> = stderr
        .lines()
        .filter(|line| {
            let t = line.trim();
            !t.is_empty()
                && !t.starts_with("Updating crates.io")
                && !t.starts_with("Locking ")
                && !t.contains("waiting for other jobs")
                && !(t.starts_with("Compiling ") && !t.contains("jet_ffi_"))
        })
        .map(|line| {
            normalize_ffi_generated_source_line(&normalize_ffi_crate_name(
                &normalize_ffi_cache_path(line),
            ))
        })
        .collect();
    indent_block(&kept.join("\n"))
}

fn normalize_ffi_generated_source_line(line: &str) -> String {
    let marker = "match std::panic::catch_unwind(";
    let Some(start) = line.find(marker) else {
        return line.to_string();
    };
    format!("{}{}<generated wrapper>)", &line[..start], marker)
}

fn normalize_ffi_crate_name(line: &str) -> String {
    let marker = "jet_ffi_";
    let mut out = String::new();
    let mut rest = line;
    while let Some(idx) = rest.find(marker) {
        out.push_str(&rest[..idx]);
        let after = &rest[idx + marker.len()..];
        let hash_len = after.chars().take_while(|c| c.is_ascii_hexdigit()).count();
        if hash_len == 16 {
            out.push_str("jet_ffi_<hash>");
            rest = &after[hash_len..];
        } else {
            out.push_str(marker);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Keep ui snapshots stable across machines (`/home/…/.cache/jet/ffi/…` → `~/.cache/jet/ffi/…`).
fn normalize_ffi_cache_path(line: &str) -> String {
    let marker = ".cache/jet/ffi/";
    let Some(idx) = line.find(marker) else {
        return line.to_string();
    };
    let path_start = line[..idx]
        .rfind('/')
        .and_then(|slash| {
            let pre = &line[..slash];
            pre.rfind(|c| c == '(' || c == ' ')
                .map(|j| j + 1)
                .or(Some(0))
        })
        .unwrap_or(idx);
    let rest = &line[idx + marker.len()..];
    let hash_len = rest.chars().take_while(|c| c.is_ascii_hexdigit()).count();
    if hash_len != 16 {
        return line.to_string();
    }
    let suffix = &rest[hash_len..];
    format!("{}~/.cache/jet/ffi/<hash>{}", &line[..path_start], suffix)
}

fn tool_error(msg: &str) -> Vec<Diagnostic> {
    vec![Diagnostic::error(
        "E0704",
        msg.to_string(),
        "building the foreign crate bridge failed".to_string(),
        "check disk permissions and try again".to_string(),
        None,
    )]
}

// c43: U32/IntN FFI type-mapping tests.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::AST::Type;
    use std::collections::HashSet;

    #[test]
    fn generated_wrapper_excerpt_is_width_independent() {
        let full = "    10 |     match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call())) {";
        let short = "    10 |     match std::panic::catch_unwind(s...";
        let expected = "    10 |     match std::panic::catch_unwind(<generated wrapper>)";
        assert_eq!(normalize_ffi_generated_source_line(full), expected);
        assert_eq!(normalize_ffi_generated_source_line(short), expected);
    }

    #[test]
    fn intn_u32_maps_to_rust_u32() {
        // c43: `Type::IntN { signed: false, bits: 32 }` (Jet `U32`) must lower
        // to Rust `u32` in the FFI bridge — not `i64` or any other type.
        let empty = HashSet::new();
        assert_eq!(
            rust_type(
                &Type::IntN {
                    signed: false,
                    bits: 32
                },
                &empty
            ),
            "u32",
            "U32 must map to Rust u32 in FFI"
        );
    }

    #[test]
    fn intn_i32_maps_to_rust_i32() {
        // c43: signed 32-bit maps to Rust i32 (S44 signed-integer subset).
        let empty = HashSet::new();
        assert_eq!(
            rust_type(
                &Type::IntN {
                    signed: true,
                    bits: 32
                },
                &empty
            ),
            "i32",
            "I32 must map to Rust i32 in FFI"
        );
    }

    #[test]
    fn intn_width_family_round_trip() {
        // c43: verify all supported fixed-width integer types map correctly.
        let empty = HashSet::new();
        for &(signed, bits, expected) in &[
            (false, 8_u8, "u8"),
            (true, 8, "i8"),
            (false, 16, "u16"),
            (true, 16, "i16"),
            (false, 32, "u32"),
            (true, 32, "i32"),
            (false, 64, "u64"),
            (true, 64, "i64"),
        ] {
            assert_eq!(
                rust_type(&Type::IntN { signed, bits }, &empty),
                expected,
                "IntN {{ signed:{}, bits:{} }} should map to {}",
                signed,
                bits,
                expected
            );
        }
    }

    #[test]
    fn int_maps_to_i64_and_float_maps_to_f64() {
        // Regression guard: base types haven't drifted.
        let empty = HashSet::new();
        assert_eq!(rust_type(&Type::Int, &empty), "i64");
        assert_eq!(rust_type(&Type::Float, &empty), "f64");
        assert_eq!(rust_type(&Type::Float32, &empty), "f32");
    }
}
