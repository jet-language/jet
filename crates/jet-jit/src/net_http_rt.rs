//! Canonical NetHttp/HTTPMessage/HTTPRoute/HTTPServer/Ws substrate for JIT hosts.
#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    non_snake_case,
    clippy::all
)]

use crate::Crypto::runtime::{
    jet_crypto_entropy_fill_for_host as jet_crypto_entropy_fill, JetCryptoSecretBytes,
};
use crate::Reactive::{jet_app_ws_register, jet_app_ws_unregister};
use crate::{JetDebug, JetDisplay, JetShow};
use jet_codegen::scheduler::{
    jet_ctx_deadline_ms, jet_ctx_push_deadline, jet_deadline_remaining_ms,
    jet_scheduler_blocking_wait_enter, jet_scheduler_blocking_wait_leave, jet_scheduler_io_wait,
    jet_scheduler_park_ms, jet_scheduler_shielded, jet_scheduler_spawn,
    jet_scheduler_spawn_blocking_with_control, jet_scheduler_spawn_blocking_with_control_at,
    jet_scheduler_task_cancelled, jet_scheduler_task_group_wait,
    jet_scheduler_tcp_listener_io_wait, jet_scheduler_wait_point_cancelled,
    jet_scheduler_wait_point_interrupted, jet_scheduler_wait_without_unwind, jet_scheduler_wake,
    jet_scheduler_world_reject_uncontrolled, jet_scheduler_with_root_control,
    jet_scheduler_yield, jet_std_time_now,
    jet_task_deliver_cancel, jet_task_join_deadline_check, JetDeadlineGuard, JetSchedulerJoin,
    JetSchedulerResult, JetSchedulerTaskPoll, JetSchedulerWait, JetTaskControl,
    JetTypedDeadlineBoundary, ParkSlot,
};

/// `Prelude/Core/RuntimeControl.rs`'s `jet_stm` spelling over the one Shared
/// transaction protocol; the task/Shared kernel in `jet_std` names it as a
/// root sibling.
mod jet_stm {
    pub(crate) type Guard = crate::Memory::shared_protocol::JetSharedTransaction;

    pub(crate) fn begin() -> Guard {
        crate::Memory::shared_protocol::jet_shared_transaction_begin()
    }
}
// D-DX-DEVTOOLS1: the HTTP server publishes typed request-panel facts; AOT
// emits this producer-only panel source beside HTTPServer.rs
// (`push_runtime_devtools_panel_preludes`), so the resident tier includes the
// same file.
use jet_foundation::Devtools::*;
include!("../../jet-codegen/src/Prelude/Core/DevtoolsRequestPanel.rs");
#[cfg(unix)]
use jet_codegen::scheduler::{
    jet_scheduler_raw_io_handle, jet_scheduler_tcp_stream_ready_wait, jet_scheduler_udp_io_wait,
    jet_scheduler_udp_ready_wait, jet_scheduler_unix_listener_io_wait,
    jet_scheduler_unix_stream_io_wait, jet_scheduler_unix_stream_ready_wait,
    JetSchedulerRawIoHandle,
};

fn jet_runtime_stop(code: &'static str, _file: &str, line: u32, message: &str) -> ! {
    crate::runtime_host::runtime_stop_unwind(code, line, message)
}

/// AOT `Core.rs`'s context-carrying stop; the resident host records the same
/// code/file/line and unwinds to its invocation boundary.
fn jet_runtime_stop_with_context(
    code: &'static str,
    file: &str,
    line: u32,
    _fn_name: &str,
    _src_line: &str,
    message: &str,
) -> ! {
    crate::runtime_host::runtime_stop_unwind_at(code, file, line, message)
}

type JetDataTree = jet_foundation::DataTree::DataTree;

// Keep the generated Rust codec trait spelling stable; Codegen emits `__jet_Encode`.
#[allow(non_camel_case_types)]
trait __jet_Encode {
    fn jet_encode(&self) -> JetDataTree;
}
impl __jet_Encode for i64 {
    fn jet_encode(&self) -> JetDataTree {
        JetDataTree::Int(*self)
    }
}
// Keep the generated Rust codec trait spelling stable; Codegen emits `__jet_Decode`.
#[allow(non_camel_case_types)]
trait __jet_Decode: Sized {
    fn jet_decode(tree: &JetDataTree) -> Result<Self, Vec<jet_std::FieldError>>;
}
fn jet_enc_json_to_string<T: __jet_Encode>(v: &T) -> String {
    crate::Encoding::json_rt::render_datatree_json(&v.jet_encode(), false, 0)
}
fn jet_enc_json_decode<T: __jet_Decode>(text: &String) -> Result<T, Vec<jet_std::FieldError>> {
    let tree = crate::Encoding::json_rt::parse_datatree_typed_ordered(text).map_err(|error| {
        let line = error.line.ok().unwrap_or(0);
        jet_std::FieldError::one(format!(
            "invalid JSON (line {}): {}",
            line, error.reason
        ))
    })?;
    T::jet_decode(&tree)
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

    pub(crate) use crate::Encoding::json_rt::{
        datatree_kind_for, decode_bool, decode_f32, decode_float, decode_int, decode_int_with, decode_string,
        jet_datatree_encode_i64, jet_datatree_encode_u64, jet_datatree_decode_fixed_integer,
        jet_datatree_project, jet_enc_csv_decode_rows, jet_int_to_i128, jet_int_to_i64, jet_int_to_string,
        parse_json_typed_datatree, DataTree, EncodingCause, EncodingError, EncodingErrorKind,
        EncodingFormat, EncodingLimits, FieldError, JetDataTreeAccess,
    };

    pub(crate) use crate::Encoding::codec_rt::jet_std::JetDecimal;
    pub(crate) use crate::Encoding::codec_rt::{
        jet_codec_date_decode, jet_codec_date_encode, jet_codec_datetime_decode,
        jet_codec_datetime_encode, jet_codec_decimal_decode_int, jet_codec_decimal_decode_text,
        jet_codec_decimal_encode, jet_codec_duration_decode, jet_codec_duration_encode,
        jet_codec_local_time_decode, jet_codec_local_time_encode,
    };
    pub(crate) use crate::Time::time_rt::{JetDate, JetDateTime, JetLocalTime};
    pub(crate) use crate::Reactive::reactive_rt::{
        jet_reactive_effect, jet_reactive_transaction, JetDerived, JetReactiveEffect, JetSignal,
    };
    // `JetTask` / `JetShared` / `JetSharedSnapshot` for WebForms.rs and
    // WebStore.rs: the one Prelude kernel, cut by build.rs (see
    // `write_web_kernel_std`), running on the shared scheduler host.
    use crate::{JetDebug, JetDisplay, JetShow};
    use jet_codegen::local_cell::JetCell;
    use jet_codegen::task_group::{jet_task_deadline_pending, JetTaskFailure, JetTaskGroupRuntime};
    include!(concat!(env!("OUT_DIR"), "/web_kernel_std.rs"));

    #[derive(Clone, Debug, PartialEq)]
    pub struct LogField {
        pub key: String,
        pub value: String,
        pub kind: String,
        pub redacted: bool,
    }

    pub fn render_datatree_json(tree: &DataTree, pretty: bool, indent: i64) -> String {
        crate::Encoding::json_rt::render_datatree_json(tree, pretty, indent as usize)
    }

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
include!("../../jet-codegen/src/Prelude/Core/NetError.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/NetHTTP.rs");
thread_local! {
    static JET_DB_REQUEST_ID: std::cell::RefCell<Option<String>> =
        std::cell::RefCell::new(None);
}

pub(crate) fn jet_db_current_request_id() -> Option<String> {
    JET_DB_REQUEST_ID.with(|request_id| request_id.borrow().clone())
}

pub(crate) struct JetDbRequestScope {
    previous: Option<String>,
}

impl JetDbRequestScope {
    pub(crate) fn enter(request_id: Option<String>) -> Self {
        let previous = JET_DB_REQUEST_ID.with(|current| current.replace(request_id));
        Self { previous }
    }
}

impl Drop for JetDbRequestScope {
    fn drop(&mut self) {
        JET_DB_REQUEST_ID.with(|current| {
            let _ = current.replace(self.previous.take());
        });
    }
}

mod jet_app_middleware {
    include!("../../jet-foundation/src/AppMiddleware.rs");
}
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/HTTPClient.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/WebServerFn.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/Ws.rs");

pub(crate) mod native_http {
    #![allow(
        dead_code,
        unused_imports,
        unused_variables,
        unused_mut,
        non_snake_case,
        clippy::all
    )]
    const HTTP_PUBLIC_SUFFIX_LIST: &str =
        include_str!("../../jet-pkg-model/src/Prelude/public_suffix_list.dat");
    include!("../../jet-pkg-model/src/Prelude/HTTP.rs");
}

include!("net_http_hosts.rs");
/// Typed host boundary for the existing application HTTP mux.
///
/// The mux remains the canonical route graph.  This adapter only translates
/// transport-neutral console DTOs; it never opens a listener or HTTP client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleHttpRequest {
    pub request_id: String,
    pub session_id: String,
    pub method: String,
    pub path: String,
    pub origin: String,
    pub headers: std::collections::BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleHttpResponse {
    pub request_id: String,
    pub status: u16,
    pub headers: std::collections::BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Clone)]
pub struct ConsoleHttpRouter {
    mux: std::sync::Arc<JetHTTPMux>,
    cancelled: std::sync::Arc<std::sync::Mutex<std::collections::BTreeSet<String>>>,
}

impl ConsoleHttpRouter {
    /// Build a console adapter from the already assembled application mux.
    /// This constructor is crate-visible because only the app host can own the
    /// opaque mux; callers receive the typed router above.
    pub(crate) fn from_mux(mux: JetHTTPMux) -> Self {
        Self {
            mux: std::sync::Arc::new(mux),
            cancelled: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::BTreeSet::new(),
            )),
        }
    }

    /// Dispatch one validated console request through the canonical mux.
    pub fn dispatch(
        &self,
        request: ConsoleHttpRequest,
    ) -> Result<ConsoleHttpResponse, String> {
        let was_cancelled = self
            .cancelled
            .lock()
            .map_err(|_| "HTTP console cancellation state is poisoned".to_string())?
            .remove(&request.request_id);
        if was_cancelled {
            return Err("HTTP operation cancelled".to_string());
        }

        let mut headers = JetHTTPHeaders::new();
        for (name, value) in request.headers {
            headers.set(&name, &value)?;
        }
        // The console identity is the request identity used by the canonical
        // devtools publication path.  Do not allow an input header to replace
        // it.
        headers.set("x-request-id", &request.request_id)?;
        headers.set("x-jet-session-id", &request.session_id)?;
        headers.set("origin", &request.origin)?;
        let http_request =
            JetHTTPRequest::server(&request.method, request.path, request.body, headers);
        let http_response =
            jet_http_mux_dispatch(&self.mux, http_request).map_err(|error| error.to_string())?;
        let body = http_response
            .body
            .bytes(JET_HTTP_MAX_BODY_BYTES)
            .map_err(|error| error.to_string())?;
        let cancelled = self
            .cancelled
            .lock()
            .map_err(|_| "HTTP console cancellation state is poisoned".to_string())?
            .remove(&request.request_id);
        if cancelled {
            return Err("HTTP operation cancelled".to_string());
        }
        Ok(ConsoleHttpResponse {
            request_id: request.request_id,
            status: u16::try_from(http_response.status)
                .map_err(|_| "HTTP response status is outside the console range".to_string())?,
            headers: http_response.headers.entries.into_iter().collect(),
            body,
        })
    }

    /// Mark an in-flight request cancelled.  The canonical synchronous mux
    /// observes the mark before and after handler execution.
    pub fn cancel(&self, request_id: &str) {
        if let Ok(mut cancelled) = self.cancelled.lock() {
            cancelled.insert(request_id.to_string());
        }
    }
}

/// Convert an existing application mux into the typed console router.
pub(crate) fn console_http_router_from_mux(mux: JetHTTPMux) -> ConsoleHttpRouter {
    ConsoleHttpRouter::from_mux(mux)
}
