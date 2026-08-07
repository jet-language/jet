//! Canonical NetHttp/HTTPMessage/HTTPRoute/HTTPServer/Ws substrate for JIT hosts.
#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    non_snake_case,
    clippy::all
)]

use crate::JetShow;
use jet_codegen::scheduler::{
    jet_scheduler_blocking_wait_enter, jet_scheduler_blocking_wait_leave,
    jet_scheduler_ctx_deadline_ms, jet_scheduler_park_ms, jet_scheduler_push_deadline,
    jet_scheduler_sleep_ms, jet_scheduler_spawn, jet_scheduler_spawn_blocking_with_control,
    jet_scheduler_task_cancelled, jet_scheduler_wait_point_cancelled,
    jet_scheduler_wait_without_unwind, JetSchedulerDeadlineGuard, JetSchedulerJoin,
    JetSchedulerResult, JetSchedulerWait, JetTaskControl,
};
use std::sync::Arc;

type JetDataTree = crate::Encoding::json_rt::DataTree;

trait user_Encode {
    fn jet_encode(&self) -> JetDataTree;
}
trait user_Decode: Sized {
    fn jet_decode_traced(tree: &JetDataTree) -> Result<(Self, ()), Vec<jet_std::FieldError>>;
}
fn jet_enc_json_to_string<T: user_Encode>(v: &T) -> String {
    crate::Encoding::json_rt::render_datatree_json(&v.jet_encode(), false, 0)
}
fn jet_enc_json_decode<T: user_Decode>(text: &String) -> Result<T, Vec<jet_std::FieldError>> {
    let tree = crate::Encoding::json_rt::parse_datatree(text).map_err(|error| {
        jet_std::FieldError::one(format!(
            "invalid JSON (line {}): {}",
            error.line, error.message
        ))
    })?;
    T::jet_decode_traced(&tree).map(|(value, _)| value)
}
struct JetFileReader {
    inner: std::io::BufReader<std::fs::File>,
    path: String,
}
struct JetFileWriter {
    inner: std::io::BufWriter<std::fs::File>,
    path: String,
}
#[derive(Clone)]
struct JetCryptoSecretBytes(Vec<u8>);
impl JetCryptoSecretBytes {
    fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
    fn as_vec(&self) -> &Vec<u8> {
        &self.0
    }
}
impl Drop for JetCryptoSecretBytes {
    fn drop(&mut self) {
        for b in &mut self.0 {
            *b = 0;
        }
    }
}
fn jet_sha256_raw(data: &[u8]) -> [u8; 32] {
    crate::Crypto::runtime::jet_crypto_email_sha256_impl(data)
}
fn jet_crypto_entropy_fill(buf: &mut [u8]) -> Result<(), String> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(buf))
        .map_err(|e| e.to_string())
}
fn jet_crypto_entropy_zeroize(buf: &mut [u8]) {
    for b in buf {
        *b = 0;
    }
}
fn jet_scheduler_io_wait(
    _stream: &std::net::TcpStream,
    _read: bool,
    _write: bool,
    _wait_kind: &str,
) {
    std::thread::sleep(std::time::Duration::from_millis(5));
}
fn jet_scheduler_shielded() -> bool {
    false
}
fn jet_panic(_file: &str, _line: u32, msg: &str) -> ! {
    // RUNTIME_PANIC (exit 70): user-program panic path for include!d net host, not I2 ICE.
    eprintln!("panic: {msg}");
    std::process::exit(70);
}
fn jet_task_deliver_cancel() {
    // Host-side cancel delivery is a no-op outside a live scheduler task frame.
}
fn jet_log_emit(_level: &str, _msg: &str, _fields: &[jet_std::LogField]) {}

pub mod jet_std {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum IOOperation {
        Read,
        Write,
        Flush,
        Connect,
        Accept,
        Close,
        Resolve,
        Codec,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct IOContext {
        pub operation: IOOperation,
        pub resource: Option<String>,
        pub os_code: Option<i64>,
        pub cause: Option<String>,
    }

    impl IOContext {
        pub fn new(
            operation: IOOperation,
            resource: Option<String>,
            os_code: Option<i64>,
            cause: Option<String>,
        ) -> Self {
            Self {
                operation,
                resource,
                os_code,
                cause,
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum IOError {
        InvalidInput(IOContext),
        NotFound(IOContext),
        PermissionDenied(IOContext),
        TimedOut(IOContext),
        Cancelled(IOContext),
        Closed(IOContext),
        Protocol(IOContext),
        Other(IOContext),
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Duration {
        pub ns: i64,
    }

    impl Duration {
        #[inline]
        pub fn as_millis(self) -> i64 {
            self.ns / 1_000_000
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct JetURL {
        pub scheme: String,
        pub username: Option<String>,
        pub password: Option<String>,
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

    pub use crate::Encoding::json_rt::{JSON, JSONError};

    #[derive(Clone, Debug, PartialEq)]
    pub struct FieldError {
        pub path: String,
        pub reason: String,
    }

    impl FieldError {
        pub fn one(reason: impl Into<String>) -> Vec<FieldError> {
            vec![FieldError {
                path: String::new(),
                reason: reason.into(),
            }]
        }

        pub fn under_errors(seg: &str, errors: Vec<FieldError>) -> Vec<FieldError> {
            errors
                .into_iter()
                .map(|mut error| {
                    error.path = if error.path.is_empty() {
                        seg.to_string()
                    } else if error.path.starts_with('[') {
                        format!("{}{}", seg, error.path)
                    } else {
                        format!("{}.{}", seg, error.path)
                    };
                    error
                })
                .collect()
        }

        pub fn under<T>(seg: &str, result: Result<T, Vec<FieldError>>) -> Result<T, Vec<FieldError>> {
            result.map_err(|errors| Self::under_errors(seg, errors))
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct LogField {
        pub key: String,
        pub value: String,
        pub kind: String,
        pub redacted: bool,
    }

    pub fn render_datatree_json(tree: &super::JetDataTree, pretty: bool, indent: i64) -> String {
        crate::Encoding::json_rt::render_datatree_json(tree, pretty, indent as usize)
    }

    pub fn datatree_from_json(j: &JSON) -> super::JetDataTree {
        crate::Encoding::json_rt::datatree_from_json(j)
    }

    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/UrlMime.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/JSONCodec.rs");
}

type JetDeadlineGuard = JetSchedulerDeadlineGuard;

fn jet_std_time_now() -> i64 {
    if let Ok(s) = std::env::var("JET_PROVE_REPLAY_TIME_MS") {
        if let Ok(n) = s.parse::<i64>() {
            return n;
        }
    }
    if let Ok(s) = std::env::var("LEX_TEST_EPOCH") {
        if let Ok(n) = s.parse::<i64>() {
            return n;
        }
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn jet_ctx_deadline_ms() -> Option<i64> {
    jet_scheduler_ctx_deadline_ms()
}

fn jet_ctx_push_deadline(deadline_ms: i64) -> JetDeadlineGuard {
    jet_scheduler_push_deadline(deadline_ms)
}

fn jet_deadline_remaining_ms() -> Option<i64> {
    jet_ctx_deadline_ms().map(|d| d.saturating_sub(jet_std_time_now()))
}

fn jet_deadline_check(_wait_kind: &str) {}

fn jet_scheduler_tcp_listener_io_wait(_listener: &std::net::TcpListener, _wait_kind: &str) {
    std::thread::sleep(std::time::Duration::from_millis(5));
}

fn jet_scheduler_tcp_stream_io_wait(
    _stream: &std::net::TcpStream,
    _read: bool,
    _write: bool,
    _wait_kind: &str,
) {
    std::thread::sleep(std::time::Duration::from_millis(5));
}

fn jet_scheduler_tcp_stream_ready_wait(
    _stream: &std::net::TcpStream,
    read: bool,
    write: bool,
    _wait_kind: &str,
) -> (bool, bool) {
    (read, write)
}

fn jet_scheduler_udp_io_wait(
    _socket: &std::net::UdpSocket,
    _read: bool,
    _write: bool,
    _wait_kind: &str,
) {
    std::thread::sleep(std::time::Duration::from_millis(5));
}

#[cfg(unix)]
fn jet_scheduler_unix_listener_io_wait(
    _listener: &std::os::unix::net::UnixListener,
    _wait_kind: &str,
) {
    std::thread::sleep(std::time::Duration::from_millis(5));
}

#[cfg(unix)]
fn jet_scheduler_unix_stream_io_wait(
    _stream: &std::os::unix::net::UnixStream,
    _read: bool,
    _write: bool,
    _wait_kind: &str,
) {
    std::thread::sleep(std::time::Duration::from_millis(5));
}

#[cfg(unix)]
fn jet_scheduler_unix_stream_ready_wait(
    _stream: &std::os::unix::net::UnixStream,
    read: bool,
    write: bool,
    _wait_kind: &str,
) -> (bool, bool) {
    (read, write)
}

#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/DNSResolverPolicy.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/HTTPMessage.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/HTTPRoute.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/NetHTTP.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/HTTPClient.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/Ws.rs");

include!("net_http_hosts.rs");
