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
use crate::Crypto::runtime::{
    jet_crypto_entropy_fill_for_host as jet_crypto_entropy_fill, JetCryptoSecretBytes,
};
use jet_codegen::scheduler::{
    jet_ctx_deadline_ms, jet_ctx_push_deadline, jet_deadline_remaining_ms,
    jet_scheduler_blocking_wait_enter,
    jet_scheduler_blocking_wait_leave, jet_scheduler_io_wait, jet_scheduler_park_ms,
    jet_scheduler_shielded, jet_scheduler_spawn, jet_scheduler_spawn_blocking_with_control,
    jet_scheduler_task_cancelled, jet_scheduler_tcp_listener_io_wait,
    jet_scheduler_wait_point_cancelled, jet_scheduler_wait_without_unwind, jet_std_time_now,
    jet_task_deliver_cancel,
    JetDeadlineGuard, JetSchedulerJoin, JetSchedulerResult, JetSchedulerWait, JetTaskControl,
};
#[cfg(unix)]
use jet_codegen::scheduler::{
    jet_scheduler_raw_io_handle, jet_scheduler_tcp_stream_ready_wait, jet_scheduler_udp_io_wait,
    jet_scheduler_udp_ready_wait, jet_scheduler_unix_listener_io_wait,
    jet_scheduler_unix_stream_io_wait, jet_scheduler_unix_stream_ready_wait,
    JetSchedulerRawIoHandle,
};
use std::sync::Arc;

type JetDataTree = crate::Encoding::json_rt::DataTree;

trait __jet_Encode {
    fn jet_encode(&self) -> JetDataTree;
}
impl __jet_Encode for i64 {
    fn jet_encode(&self) -> JetDataTree {
        JetDataTree::Int(*self)
    }
}
trait __jet_Decode: Sized {
    fn jet_decode_traced(tree: &JetDataTree) -> Result<(Self, ()), Vec<jet_std::FieldError>>;
}
fn jet_enc_json_to_string<T: __jet_Encode>(v: &T) -> String {
    crate::Encoding::json_rt::render_datatree_json(&v.jet_encode(), false, 0)
}
fn jet_enc_json_decode<T: __jet_Decode>(text: &String) -> Result<T, Vec<jet_std::FieldError>> {
    let tree = crate::Encoding::json_rt::parse_datatree_ordered(text).map_err(|error| {
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
fn jet_sha256_raw(data: &[u8]) -> [u8; 32] {
    crate::Crypto::runtime::jet_crypto_email_sha256_impl(data)
}
fn jet_panic(_file: &str, line: u32, msg: &str) -> ! {
    // The shared Prelude owns report text. This bridge records it, then lets
    // the resident boundary own cleanup and the final exit status.
    crate::runtime_host::runtime_stop_unwind("E3001", line, msg)
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
include!("../../jet-codegen/src/Prelude/Core/NetPure.rs");
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
