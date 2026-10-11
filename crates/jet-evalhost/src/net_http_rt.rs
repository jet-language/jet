//! Canonical NetHttp/HTTPMessage/HTTPRoute/HTTPServer/Ws substrate for JIT hosts.
#![allow(unused_imports, unused_variables, unused_mut, non_snake_case, clippy::all)]

use crate::Crypto::runtime::{
    jet_crypto_entropy_fill_for_host as jet_crypto_entropy_fill, JetCryptoSecretBytes
};
use crate::Reactive::{jet_app_ws_register, jet_app_ws_unregister};
use crate::{JetDebug, JetDisplay, JetShow};

impl JetShow for Vec<String> {
    fn jet_show(&self) -> String {
        jet_foundation::StructuralDebug::jet_text_list(self)
    }
}

impl JetDebug for Vec<String> {
    fn jet_debug(&self) -> String {
        jet_foundation::StructuralDebug::jet_text_list(
            self.iter().map(|origin| format!("{origin:?}")),
        )
    }
}
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
    JetTypedDeadlineBoundary, ParkSlot
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
    JetSchedulerRawIoHandle
};

fn jet_runtime_stop(_code: &'static str, _file: &str, _line: u32, message: &str) -> ! {
    panic!("{message}");
}

fn jet_runtime_stop_with_context(
    _code: &'static str,
    _file: &str,
    _line: u32,
    _fn_name: &str,
    _src_line: &str,
    message: &str,
) -> ! {
    panic!("{message}");
}

fn jet_panic(_file: &str, _line: u32, message: &str) -> ! {
    panic!("{message}");
}

// HTTP and app dispatch use the same canonical codec traits and fallible JSON
// helpers; no resident-only renderer bypasses the wire validation.
use crate::Web::web_rt::{
    __jet_Decode, __jet_Encode, jet_enc_json_decode, jet_enc_json_error_text,
    jet_enc_json_to_string,
};
struct JetFileReader {
    inner: std::sync::Mutex<std::io::BufReader<std::fs::File>>,
    path: String
}
struct JetFileWriter {
    inner: std::sync::Mutex<std::io::BufWriter<std::fs::File>>,
    path: String
}
fn jet_sha256_raw(data: &[u8]) -> [u8; 32] {
    crate::Crypto::runtime::jet_crypto_email_sha256_impl(data)
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
        Codec
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct IOContext {
        pub operation: IOOperation,
        pub resource: Option<String>,
        pub os_code: Option<i64>,
        pub cause: Option<String>
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
                cause
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
        Other(IOContext)
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Duration {
        pub ns: i64
    }

    impl Duration {
        #[inline]
        pub fn as_millis(self) -> i64 {
            self.ns / 1_000_000
        }
    }

    #[derive(Clone, Debug)]
    pub struct JetURLParts {
        pub scheme: String,
        pub username: Option<String>,
        pub password: Option<String>,
        pub host: Option<String>,
        pub port: Option<i64>,
        pub path: String,
        pub query: Vec<(String, String)>,
        pub fragment: Option<String>,
        pub typed_host: Option<Vec<(String, bool)>>,
        pub typed_path: Option<Vec<(String, bool)>>
    }

    pub(crate) use crate::Encoding::json_rt::{
        dyn_kind_for, decode_bool, decode_f32, decode_float, decode_int, decode_int_with, decode_string,
        jet_dyn_encode_i64, jet_dyn_encode_u64, jet_dyn_decode_fixed_integer,
        jet_dyn_project, jet_decode_text_cells, jet_text_cells_active, jet_enc_csv_decode_rows, jet_int_to_string,
        parse_json_dyn, Dyn, EncodingCause, EncodingError, EncodingErrorKind,
        EncodingFormat, EncodingLimits, FieldError, JetDynAccess
    };

    pub(crate) use crate::Encoding::codec_rt::jet_std::JetDecimal;
    pub(crate) use crate::Encoding::codec_rt::{
        jet_codec_date_decode, jet_codec_date_encode, jet_codec_datetime_decode,
        jet_codec_datetime_encode, jet_codec_decimal_decode_int, jet_codec_decimal_decode_text,
        jet_codec_decimal_encode, jet_codec_duration_decode, jet_codec_duration_encode,
        jet_codec_local_time_decode, jet_codec_local_time_encode
    };
    pub(crate) use crate::Time::time_rt::{JetDate, JetDateTime, JetLocalTime};
    pub(crate) use crate::Reactive::reactive_rt::{
        jet_reactive_effect, jet_reactive_transaction_nullary, JetDerived, JetReactiveEffect, JetSignal
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
        pub redacted: bool
    }

    pub fn render_dyn_json(tree: &Dyn, pretty: bool, indent: i64) -> String {
        crate::Encoding::json_rt::render_dyn_json(tree, pretty, indent as usize)
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
include!("../../jet-codegen/src/Prelude/Core/HttpRequestTarget.rs");
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

pub(crate) fn jet_net_io_error_mir_value(
    operation: &str,
    address: Option<String>,
    error: std::io::Error,
) -> jet_foundation::MIR::MirRuntimeValue {
    use jet_foundation::MIR::{MirRuntimeValue as Value, MirType, MirTypeKind};

    fn optional_string(value: Option<String>) -> Value {
        match value {
            Some(value) => Value::Present(Box::new(Value::String(value))),
            None => Value::Absent {
                element: MirType::from_kind(MirTypeKind::String)
            }
        }
    }

    fn optional_int(value: Option<i64>) -> Value {
        match value {
            Some(value) => Value::Present(Box::new(Value::Int(value))),
            None => Value::Absent {
                element: MirType::from_kind(MirTypeKind::Int)
            }
        }
    }

    let parts = jet_net_error_surface_parts(jet_net_io_error(operation, address, error));
    let payload = match parts.payload {
        JetNetErrorSurfacePayload::Detail(detail) => Value::Struct {
            type_name: "NetErrorDetail".to_string(),
            fields: vec![
                ("operation".to_string(), Value::String(detail.operation)),
                ("address".to_string(), optional_string(detail.address)),
                ("name".to_string(), optional_string(detail.name)),
                ("message".to_string(), Value::String(detail.message)),
                ("os_code".to_string(), optional_int(detail.os_code)),
            ]
        },
        JetNetErrorSurfacePayload::DNS { variant, value, .. } => Value::Enum {
            type_name: "NetDNSError".to_string(),
            variant: variant.to_string(),
            args: vec![(None, Value::String(value))]
        }
    };
    Value::Enum {
        type_name: "NetError".to_string(),
        variant: parts.variant.to_string(),
        args: vec![(None, payload)]
    }
}
thread_local! {
    static JET_DB_REQUEST_ID: std::cell::RefCell<Option<String>> =
        std::cell::RefCell::new(None);
}

pub(crate) fn jet_db_current_request_id() -> Option<String> {
    JET_DB_REQUEST_ID.with(|request_id| request_id.borrow().clone())
}

pub(crate) struct JetDbRequestScope {
    previous: Option<String>
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
    #![allow(unused_imports, unused_variables, unused_mut, non_snake_case, clippy::all)]
    const HTTP_PUBLIC_SUFFIX_LIST: &str =
        include_str!("../../jet-pkg-model/src/Prelude/public_suffix_list.dat");
    include!("../../jet-pkg-model/src/Prelude/HTTP.rs");
}

include!("net_http_hosts.rs");

