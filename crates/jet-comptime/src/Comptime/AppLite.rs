//! D-LIVEQUERY1=A / I9: `app` ambient includes Prelude `LiveQuery.rs`.

use std::cell::Cell;

use super::Diagnostics::unsupported;
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{ClosureData, CtKey, CtOpaque, CtValue, Lambda, LambdaBody, LambdaMeta, Type};

thread_local! {
    static WEB_RUNTIME_POLICY: Cell<(bool, bool)> = const { Cell::new((false, false)) };
}
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
mod jet_app_middleware {
    include!("../../../jet-foundation/src/AppMiddleware.rs");
}

mod web_kernel {
    include!("../../../jet-codegen/src/Prelude/SharedProtocol.rs");

    pub(super) fn jet_web_runtime_devtools_enabled() -> bool {
        super::WEB_RUNTIME_POLICY.with(|policy| policy.get().0)
    }

    pub(super) fn jet_web_runtime_history_enabled() -> bool {
        super::WEB_RUNTIME_POLICY.with(|policy| policy.get().1)
    }
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    use jet_foundation::Devtools::jet_devtools_publish_event;
    use jet_foundation::LiveLifecycle::JetLiveLifecycle;

    pub(super) mod native_scheduler {
        trait JetShow {
            fn jet_show(&self) -> String;
        }

        trait JetDisplay {
            fn jet_display(&self) -> String;
        }

        trait JetDebug {
            fn jet_debug(&self) -> String;
        }
        pub const JET_DEVTOOLS_LOCAL_RAIL_ENABLED: bool = true;
        use jet_foundation::Devtools::{
            jet_devtools_install_event_sink, jet_devtools_install_live_value_sink,
            jet_devtools_publish_event, JET_DEVTOOLS_MAX_LIVE_VALUES, JetDevtoolsEnvelope,
            JetDevtoolsEvent, JetDevtoolsEventSink, JetDevtoolsEventSinkGuard,
            JetDevtoolsSourceIdentityFact, JetLiveValueDecision, JetLiveValueSinkGuard,
        };
        use jet_foundation::Outcome::{
            jet_render_runtime_stop, jet_stream_take_failure_report, JetRuntimeDiagnostic,
            JetTaskFailure,
        };

        pub(crate) fn jet_runtime_stop_with_context(
            code: &'static str,
            file: &str,
            line: u32,
            _fn_name: &str,
            _src_line: &str,
            message: &str,
        ) -> ! {
            super::jet_runtime_stop(code, file, line as i64, message)
        }

        pub(crate) mod jet_std {
            use super::JetShow;
            pub(crate) use jet_foundation::Outcome::{jet_outcome_of, JetTaskFailure};
            pub(crate) use crate::Comptime::UrlLite::url_percent_decode_str
                as jet_url_percent_decode_str;
            pub(crate) use crate::Comptime::UrlLite::url_percent_encode
                as jet_url_percent_encode;
            include!("../../../jet-codegen/src/Prelude/TaskGroup.rs");
            include!("../../../jet-codegen/src/Prelude/LocalCell.rs");
            include!(concat!(env!("OUT_DIR"), "/web_kernel_std.rs"));

            pub use jet_foundation::DataTree::DataTree;

            #[derive(Clone, Debug, PartialEq)]
            pub struct FieldError {
                pub path: String,
                pub reason: String,
            }

            impl FieldError {
                pub fn one(reason: impl Into<String>) -> Vec<Self> {
                    vec![Self {
                        path: String::new(),
                        reason: reason.into(),
                    }]
                }
            }

            // Host rows for the HTTP substrate below. These are the same
            // plain shapes the resident JIT's `net_http_rt::jet_std` supplies;
            // the Prelude sources own every operation on them.
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
            pub struct LogField {
                pub key: String,
                pub value: String,
                pub kind: String,
                pub redacted: bool,
            }

            pub(crate) use crate::Comptime::UrlLite::JetMIME;

            /// Strict JSON validity through the one Foundation parser; the
            /// server-fn kernel only asks whether a body parses.
            pub fn parse_json_typed_datatree(
                text: &str,
            ) -> Result<DataTree, jet_foundation::EncodingJson::Error> {
                jet_foundation::EncodingJson::parse_json_typed(text, false)
            }
        }

        include!(concat!(env!("OUT_DIR"), "/web_kernel_native_scheduler.rs"));
    }

    use self::native_scheduler::jet_std;
    pub use self::native_scheduler::jet_std::{
        DataTree, JetDerived, JetReactiveEffect, JetSignal, jet_reactive_effect,
        jet_reactive_effect_rooted,
    };

    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/LiveQuery.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/HTTPRoute.rs");
    include!("../../../jet-codegen/src/Prelude/Core/WebPending.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/WebRouter.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/WebQuery.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/WebForms.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/WebTable.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/WebVirtual.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/WebStore.rs");

    #[allow(non_camel_case_types)]
    pub trait __jet_Encode {
        fn jet_encode(&self) -> jet_std::DataTree;
    }

    #[allow(non_camel_case_types)]
    pub trait __jet_Decode: Sized {
        fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>>;
    }

    impl __jet_Encode for String {
        fn jet_encode(&self) -> jet_std::DataTree {
            jet_std::DataTree::Text(self.clone())
        }
    }

    impl __jet_Decode for String {
        fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
            match tree {
                jet_std::DataTree::Text(value) | jet_std::DataTree::TypedText(value) => {
                    Ok(value.clone())
                }
                _ => Err(jet_std::FieldError::one("expected Text")),
            }
        }
    }

    pub fn jet_enc_json_to_string<T: __jet_Encode>(value: &T) -> String {
        jet_foundation::EncodingJson::render_json(&value.jet_encode(), false, 0)
    }

    pub fn jet_enc_json_decode<T: __jet_Decode>(
        value: &String,
    ) -> Result<T, Vec<jet_std::FieldError>> {
        let tree = jet_foundation::EncodingJson::parse_json(value, true)
            .map_err(|error| jet_std::FieldError::one(error.message))?;
        T::jet_decode(&tree)
    }


    #[derive(Clone, Debug)]
    pub struct JetErr(pub String);

    impl std::fmt::Display for JetErr {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(&self.0)
        }
    }

    // I9: `App.serve` reaches `jet_app_http_serve` in HTTPServer.rs. AOT emits
    // that file and the resident JIT includes it (`net_http_rt`); the
    // interpreter hosts the same sources in `http_kernel` below so routing,
    // the server-fn CSRF/middleware policy, static files, and live reload
    // have one owner. App.rs names exactly these entries.
    pub(super) use super::http_kernel::{
        jet_app_http_assets, jet_app_http_mount, jet_app_http_mux_new,
        jet_app_http_page_response, jet_app_http_reload, jet_app_http_request_url,
        jet_app_http_serve, jet_web_server_fn_register_http, jet_web_server_fn_typed,
        JetHTTPMux, JetWebServerFnError, JetWebServerFnHandler, JetWebServerFunction,
    };

    pub fn jet_runtime_stop(code: &str, _path: &str, _line: i64, message: &str) -> ! {
        panic!("{code}: {message}")
    }

    use super::jet_app_middleware;
    include!("../../../jet-codegen/src/Prelude/App.rs");
    pub(super) struct AppLiveQuery(JetLiveQuery);

    pub(super) fn app_live_query_parts(
        query: &AppLiveQuery,
    ) -> (u64, String, String, u64, bool, String) {
        let query = &query.0;
        (
            query.id,
            query.footprint.display(),
            query.value.clone(),
            query.lifecycle.generation,
            query.lifecycle.active,
            query.lifecycle.error.clone(),
        )
    }

    pub(super) fn app_live_query_from_parts(
        id: u64,
        footprint: String,
        value: String,
        generation: u64,
        active: bool,
        error: String,
    ) -> Option<AppLiveQuery> {
        let footprint = if footprint.is_empty() {
            JetLiveFootprint { paths: Vec::new() }
        } else {
            JetLiveFootprint::parse(&footprint)?
        };
        Some(AppLiveQuery(JetLiveQuery {
            key: String::new(),
            id,
            footprint,
            value,
            lifecycle: JetLiveLifecycle {
                generation,
                active,
                dirty: false,
                error,
                fresh_at_ms: 0,
                invalidation_cause: String::new(),
                refreshing: false,
                cancelled: false,
            },
            rerun: None,
            sink: None,
        }))
    }

    pub(super) fn app_live(footprint: String, initial: String) -> AppLiveQuery {
        AppLiveQuery(jet_app_live(footprint, initial))
    }

    pub(super) fn app_live_query<F>(
        footprint: String,
        initial: String,
        rerun: F,
    ) -> AppLiveQuery
    where
        F: Fn() -> Result<String, String> + Send + Sync + 'static,
    {
        AppLiveQuery(jet_app_live_query(footprint, initial, rerun))
    }

    pub(super) fn app_subscribe(source: String) -> AppLiveQuery {
        AppLiveQuery(jet_app_subscribe(source))
    }

    pub(super) fn app_invalidate(footprint: String) -> i64 {
        jet_app_invalidate(footprint)
    }

    pub(super) fn app_transact_invalidate(write_set: String) -> i64 {
        jet_app_transact_invalidate(write_set)
    }

    pub(super) fn app_signal_push(query: &AppLiveQuery, payload: String) -> AppLiveQuery {
        AppLiveQuery(jet_app_signal_push(&query.0, payload))
    }

    pub(super) fn app_live_get(query: &AppLiveQuery) -> String {
        jet_app_live_get(&query.0)
    }

    pub(super) fn app_live_show(query: &AppLiveQuery) -> String {
        jet_app_live_show(&query.0)
    }

    pub(super) fn app_live_stats() -> String {
        jet_app_live_stats()
    }

    pub(super) fn app_live_publish_transport(topic: String, event: String) {
        jet_live_publish_transport(topic, event);
    }
    pub(super) fn app_web_query_new(
        key: String,
        live: &AppLiveQuery,
    ) -> JetWebQuery {
        jet_web_query_new(key, &live.0)
    }
}

/// Interpreter host for the canonical HTTP substrate: the same
/// HTTPMessage/NetHTTP/HTTPServer/WebServerFn/Ws sources AOT concatenates and
/// the resident JIT includes in `net_http_rt`. `web_kernel` (App.rs) imports
/// the App mux entries from here, exactly as the JIT's `web_rt` does. Every
/// item defined in this module is a marshalling row; behavior lives in the
/// included files. Dead-code reports here are about other hosts' usage, and
/// `private_interfaces` is the flat AOT namespace's `pub(crate)` grammar
/// landing in a nested module.
#[allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    non_camel_case_types,
    private_interfaces
)]
mod http_kernel {
    use crate::{JetDebug, JetDisplay, JetShow};
    use super::jet_app_middleware;
    use super::web_kernel::native_scheduler::*;
    use super::web_kernel::native_scheduler::jet_std;
    use super::web_kernel::{
        __jet_Decode, __jet_Encode, jet_app_ws_register, jet_app_ws_unregister,
        jet_enc_json_decode, jet_enc_json_to_string, jet_runtime_stop,
    };
    use super::CtValue;
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    use jet_foundation::Devtools::*;
    /// The one request-identity scope the Sync/DB kernel reads; the AOT
    /// Prelude is one flat namespace, so the interpreter shares it too.
    use crate::Comptime::SyncLite::JetDbRequestScope;

    fn jet_panic(file: &str, line: u32, message: &str) -> ! {
        jet_runtime_stop("E3001", file, i64::from(line), message)
    }

    /// Structured log emission is the Log.rs kernel's job; the resident hosts
    /// leave it silent and the App mux never installs the access-log middleware.
    fn jet_log_emit(_level: &str, _message: &str, _fields: &[jet_std::LogField]) {}

    struct JetFileReader {
        inner: std::io::BufReader<std::fs::File>,
        path: String,
    }
    struct JetFileWriter {
        inner: std::io::BufWriter<std::fs::File>,
        path: String,
    }

    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/SHA256Raw.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs");
    use jet_crypto_entropy::{jet_crypto_entropy_fill, JetCryptoEntropyError};
    include!("../../../jet-codegen/src/Prelude/Core/DevtoolsRequestPanel.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/DNSResolverPolicy.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/HTTPMessage.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/HTTPRoute.rs");
    include!("../../../jet-codegen/src/Prelude/Core/NetPure.rs");
    include!("../../../jet-codegen/src/Prelude/Core/NetError.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/NetHTTP.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/WebServerFn.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/Ws.rs");

    fn http_error_to_ct(error: JetHTTPError) -> CtValue {
        let parts = jet_http_error_surface_parts(error);
        let args = match parts.payload {
            JetHTTPErrorSurfacePayload::Unit => Vec::new(),
            JetHTTPErrorSurfacePayload::Int { field, value } => {
                vec![(Some(field.to_string()), CtValue::Int(value))]
            }
            JetHTTPErrorSurfacePayload::Text { field, value } => {
                vec![(Some(field.to_string()), CtValue::Str(value))]
            }
            JetHTTPErrorSurfacePayload::Operation {
                field, variant, ..
            } => vec![(
                Some(field.to_string()),
                CtValue::Enum {
                    type_name: "HTTPOperation".to_string(),
                    variant: variant.to_string(),
                    args: Vec::new(),
                },
            )],
        };
        CtValue::Enum {
            type_name: "HTTPError".to_string(),
            variant: parts.variant.to_string(),
            args,
        }
    }

    pub(super) fn http_text_from_bytes(
        bytes: Vec<u8>,
        limit: Option<i64>,
    ) -> Result<String, CtValue> {
        let body = JetHTTPBody::from_bytes(bytes);
        jet_http_body_json_text_defaulted(&body, limit).map_err(http_error_to_ct)
    }
    pub(super) fn http_project_json_decode_error(result: CtValue) -> CtValue {
        let result = match result {
            CtValue::Present(value) => Ok(*value),
            CtValue::Failed(_) => Err(Vec::<jet_std::FieldError>::new()),
            value => return value,
        };
        match jet_http_project_json_decode_error(result) {
            Ok(value) => CtValue::Present(Box::new(value)),
            Err(error) => CtValue::Failed(crate::AST::CtReport::Told(Box::new(
                http_error_to_ct(error),
            ))),
        }
    }
}

/// Marshal MIR's byte carrier through the canonical HTTPMessage body-text
/// helper; the interpreter does not own HTTP limits or error classification.
pub fn http_text_from_bytes(bytes: Vec<u8>, limit: Option<i64>) -> Result<String, CtValue> {
    http_kernel::http_text_from_bytes(bytes, limit)
}
pub fn http_project_json_decode_error(result: CtValue) -> CtValue {
    http_kernel::http_project_json_decode_error(result)
}

pub(crate) fn jet_live_publish_transport(topic: String, event: String) {
    web_kernel::app_live_publish_transport(topic, event);
}
fn live_to_ct(q: &web_kernel::AppLiveQuery) -> CtValue {
    let (id, footprint, value, generation, active, error) =
        web_kernel::app_live_query_parts(q);
    CtValue::Struct {
        type_name: "LiveQuery".to_string(),
        fields: vec![
            (
                "id".to_string(),
                CtValue::Int(id.min(i64::MAX as u64) as i64),
            ),
            ("footprint".to_string(), CtValue::Str(footprint)),
            ("value".to_string(), CtValue::Str(value)),
            (
                "generation".to_string(),
                CtValue::Int(generation.min(i64::MAX as u64) as i64),
            ),
            ("active".to_string(), CtValue::Bool(active)),
            ("error".to_string(), CtValue::Str(error)),
        ],
    }
}
fn ct_to_live(v: &CtValue, span: Span) -> Result<web_kernel::AppLiveQuery, Diagnostic> {
    let CtValue::Struct { type_name, fields } = v else {
        return Err(unsupported("LiveQuery", span));
    };
    if type_name != "LiveQuery" && type_name != "JetLiveQuery" {
        return Err(unsupported("LiveQuery", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("LiveQuery field", span))
    };
    let footprint = match field("footprint")? {
        CtValue::Str(s) => s.clone(),
        _ => return Err(unsupported("footprint", span)),
    };
    let active = match field("active")? {
        CtValue::Bool(b) => *b,
        _ => return Err(unsupported("active", span)),
    };
    let error = match field("error")? {
        CtValue::Str(s) => s.clone(),
        _ => return Err(unsupported("error", span)),
    };
    if !active && error.is_empty() {
        return Err(unsupported("LiveQuery error", span));
    }
    let id = match field("id")? {
        CtValue::Int(n) if *n >= 0 => *n as u64,
        _ => return Err(unsupported("id", span)),
    };
    let value = match field("value")? {
        CtValue::Str(s) => s.clone(),
        _ => return Err(unsupported("value", span)),
    };
    let generation = match field("generation")? {
        CtValue::Int(n) if *n >= 0 => *n as u64,
        _ => return Err(unsupported("generation", span)),
    };
    web_kernel::app_live_query_from_parts(id, footprint, value, generation, active, error)
        .ok_or_else(|| unsupported("footprint", span))
}

/// JIT/interpreter adapter for a typed live query. The callback remains in
/// the shared Prelude registry; the CtValue is only the ordinary structural
/// handle crossing the engine boundary.
pub fn live_query_with<F>(footprint: String, initial: String, rerun: F) -> CtValue
where
    F: Fn() -> Result<String, String> + Send + Sync + 'static,
{
    live_to_ct(&web_kernel::app_live_query(footprint, initial, rerun))

}
pub fn apply(method: &str, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let one = |i: usize| {
        args.get(i)
            .ok_or_else(|| unsupported(&format!("app.{method} arg {i}"), span))
    };
    match method {
        "live" => {
            let footprint = match one(0)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("footprint", span)),
            };
            let initial = match one(1)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("initial", span)),
            };
            Ok(live_to_ct(&web_kernel::app_live(footprint, initial)))
        }
        "subscribe" => {
            let source = match one(0)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("source", span)),
            };
            Ok(live_to_ct(&web_kernel::app_subscribe(source)))
        }
        "invalidate" => {
            let footprint = match one(0)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("footprint", span)),
            };
            Ok(CtValue::Int(web_kernel::app_invalidate(footprint)))
        }
        "transact_invalidate" => {
            let write_set = match one(0)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("write_set", span)),
            };
            Ok(CtValue::Int(web_kernel::app_transact_invalidate(write_set)))
        }
        "signal_push" => {
            let payload = match one(1)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("payload", span)),
            };
            Ok(live_to_ct(&web_kernel::app_signal_push(
                &ct_to_live(one(0)?, span)?,
                payload,
            )))
        }
        "live_get" => Ok(CtValue::Str(web_kernel::app_live_get(
            &ct_to_live(one(0)?, span)?,
        ))),
        "live_show" => Ok(CtValue::Str(web_kernel::app_live_show(
            &ct_to_live(one(0)?, span)?,
        ))),
        "live_stats" => Ok(CtValue::Str(web_kernel::app_live_stats())),
        _ => Err(unsupported(&format!("`app.{method}()`"), span)),
    }
}

#[derive(Clone)]
struct WebRouterHandle(web_kernel::JetWebRouter);

#[derive(Clone)]
struct WebQueryHandle(web_kernel::JetWebQuery);

#[derive(Clone)]
struct WebFormHandle(web_kernel::JetWebForm);

#[derive(Clone)]
struct WebTableHandle(web_kernel::JetWebTable<CtValue>);
#[derive(Clone)]
struct WebTableColumnHandle(web_kernel::JetWebTableColumn<CtValue>);

#[derive(Clone)]
struct WebVirtualPlanViewportHandle(web_kernel::JetWebVirtualPlanViewport);

#[derive(Clone)]
struct WebStoreHandle(web_kernel::JetWebStore<CtValue>);

struct WebStorePatchHandle(std::sync::Mutex<Option<web_kernel::JetWebStorePatch<CtValue>>>);

struct WebStoreSubscriptionHandle(web_kernel::JetWebStoreSubscription);

type WebFormValidationHandle =
    std::sync::Mutex<Option<web_kernel::JetWebFormTypedValidation>>;
type WebFormSubmissionHandle =
    std::sync::Mutex<Option<web_kernel::JetWebFormTypedSubmission>>;

fn web_record_field<'a>(
    value: &'a CtValue,
    name: &str,
    span: Span,
) -> Result<&'a CtValue, Diagnostic> {
    let CtValue::Struct { fields, .. } = value else {
        return Err(unsupported("checked Web record", span));
    };
    fields.iter().find(|(field, _)| field == name).map(|(_, value)| value)
        .ok_or_else(|| unsupported(&format!("Web record field `{name}`"), span))
}

fn web_enum_variant<'a>(
    value: &'a CtValue,
    expected: &str,
    span: Span,
) -> Result<&'a str, Diagnostic> {
    match value {
        CtValue::Enum { type_name, variant, .. } if type_name == expected => Ok(variant),
        _ => Err(unsupported(expected, span)),
    }
}

fn web_enum_value(type_name: &str, value: impl std::fmt::Debug) -> CtValue {
    CtValue::Enum {
        type_name: type_name.to_string(),
        variant: format!("{value:?}"),
        args: Vec::new(),
    }
}

fn web_form_field_spec(
    value: &CtValue,
    span: Span,
) -> Result<Result<web_kernel::JetWebFormFieldSpec, String>, Diagnostic> {
    let field = |name| web_record_field(value, name, span);
    let value_type = match web_enum_variant(field("value_type")?, "WebFormValueType", span)? {
        "String" => web_kernel::JetWebFormValueType::String,
        "Int" => web_kernel::JetWebFormValueType::Int,
        "Bool" => web_kernel::JetWebFormValueType::Bool,
        "Float" => web_kernel::JetWebFormValueType::Float,
        _ => return Err(unsupported("WebFormValueType", span)),
    };
    let control = match web_enum_variant(field("control")?, "WebFormControl", span)? {
        "Text" => web_kernel::JetWebFormControl::Text,
        "Email" => web_kernel::JetWebFormControl::Email,
        "Password" => web_kernel::JetWebFormControl::Password,
        "Number" => web_kernel::JetWebFormControl::Number,
        "Checkbox" => web_kernel::JetWebFormControl::Checkbox,
        "Hidden" => web_kernel::JetWebFormControl::Hidden,
        "Url" => web_kernel::JetWebFormControl::Url,
        "Date" => web_kernel::JetWebFormControl::Date,
        _ => return Err(unsupported("WebFormControl", span)),
    };
    let optional = |value: &CtValue| match value {
        CtValue::Present(value) => web_string(value, "form optional text", span).map(Some),
        value if value.is_clean_stop() => Ok(None),
        _ => Err(unsupported("form optional text", span)),
    };
    let default = optional(field("default")?)?;
    let group = optional(field("group")?)?;
    let label = web_string(field("label")?, "form label", span)?;
    let wire_name = web_string(field("wire_name")?, "form wire name", span)?;
    Ok(web_kernel::JetWebFormFieldSpec::new(
        web_string(field("name")?, "form field name", span)?,
        value_type,
        web_bool(field("required")?, "form required field", span)?,
    ).and_then(|spec| {
        let mut spec = spec.with_control(control).with_label(label)?.with_wire_name(wire_name)?;
        if let Some(default) = default {
            spec = spec.default_value(default)?;
        }
        if let Some(group) = group {
            spec = spec.in_group(group)?;
        }
        Ok(spec)
    }))
}

fn web_form_decoded(value: web_kernel::JetWebFormDecodedInput) -> CtValue {
    CtValue::Struct {
        type_name: "WebFormDecodedInput".to_string(),
        fields: vec![
            ("type_name".to_string(), CtValue::Str(value.type_name)),
            ("values".to_string(), web_string_map_value(value.values)),
            ("wire_values".to_string(), web_string_map_value(value.wire_values)),
        ],
    }
}

fn web_error_map_value(value: std::collections::BTreeMap<String, Vec<String>>) -> CtValue {
    CtValue::Map(value.into_iter().map(|(key, messages)| (
        CtKey::Str(key),
        CtValue::List(messages.into_iter().map(CtValue::Str).collect()),
    )).collect())
}

fn web_form_action_error(value: web_kernel::JetWebFormActionError) -> CtValue {
    CtValue::Struct {
        type_name: "WebFormActionError".to_string(),
        fields: vec![
            ("field_errors".to_string(), web_error_map_value(value.field_errors)),
            ("form_errors".to_string(), CtValue::List(
                value.form_errors.into_iter().map(CtValue::Str).collect(),
            )),
        ],
    }
}

fn web_form_action_error_read(
    value: &CtValue,
    span: Span,
) -> Result<web_kernel::JetWebFormActionError, Diagnostic> {
    let CtValue::Map(fields) = web_record_field(value, "field_errors", span)? else {
        return Err(unsupported("form action field errors", span));
    };
    let fields = fields.iter().map(|(key, value)| {
        let CtKey::Str(key) = key else {
            return Err(unsupported("form action field name", span));
        };
        Ok((key.clone(), web_strings(value, "form action field errors", span)?))
    }).collect::<Result<_, Diagnostic>>()?;
    Ok(web_kernel::JetWebFormActionError {
        field_errors: fields,
        form_errors: web_strings(
            web_record_field(value, "form_errors", span)?, "form action errors", span,
        )?,
    })
}

fn web_form_field_state(value: web_kernel::JetWebFormFieldState) -> CtValue {
    CtValue::Struct {
        type_name: "WebFormFieldState".to_string(),
        fields: vec![
            ("name".to_string(), CtValue::Str(value.name)),
            ("value_type".to_string(), web_enum_value("WebFormValueType", value.value_type)),
            ("required".to_string(), CtValue::Bool(value.required)),
            ("value".to_string(), CtValue::Str(value.value)),
            ("touched".to_string(), CtValue::Bool(value.touched)),
            ("validating".to_string(), CtValue::Bool(value.validating)),
            ("errors".to_string(), CtValue::List(
                value.errors.into_iter().map(CtValue::Str).collect(),
            )),
        ],
    }
}

fn web_form_state(value: web_kernel::JetWebFormState) -> CtValue {
    CtValue::Struct {
        type_name: "WebFormState".to_string(),
        fields: vec![
            ("status".to_string(), web_enum_value("WebFormStatus", value.status)),
            ("fields".to_string(), CtValue::Map(value.fields.into_iter().map(|(key, value)| (
                CtKey::Str(key), web_form_field_state(value),
            )).collect())),
            ("action".to_string(), CtValue::Str(value.action)),
            ("method".to_string(), CtValue::Str(value.method)),
            ("result".to_string(), CtValue::Str(value.result)),
            ("error".to_string(), CtValue::Str(value.error)),
            ("submission_count".to_string(), CtValue::Int(value.submission_count as i64)),
        ],
    }
}

fn web_opaque<T: std::any::Any>(
    value: &CtValue,
    span: Span,
) -> Result<&T, Diagnostic> {
    let CtValue::Closure(data) = value else {
        return Err(unsupported(std::any::type_name::<T>(), span));
    };
    data.opaque
        .as_ref()
        .and_then(|value| value.downcast_ref::<T>())
        .ok_or_else(|| unsupported(std::any::type_name::<T>(), span))
}

fn opaque_web<T: std::any::Any + Send + Sync>(value: T) -> CtValue {
    CtValue::Closure(std::sync::Arc::new(ClosureData {
        lambda: Lambda {
            take_names: Vec::new(),
            params: Vec::new(),
            result_type: None,
            error_type: None,
            effects: None,
            body: LambdaBody::Block(Vec::new()),
            span: Span::new(0, 0),
            meta: LambdaMeta::default(),
        },
        captured: std::collections::HashMap::new(),
        return_type: None,
        opaque: Some(CtOpaque::new(value)),
    }))
}
#[derive(Clone)]
pub struct AppHandle(web_kernel::JetApp);

impl std::fmt::Debug for AppHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("AppHandle").finish_non_exhaustive()
    }
}

pub enum AppRuntimeResult {
    App(AppHandle),
    Value(CtValue),
}

pub fn app_new_runtime() -> AppHandle {
    AppHandle(web_kernel::jet_app())
}

fn app_string(value: &CtValue, name: &str, span: Span) -> Result<String, Diagnostic> {
    match value {
        CtValue::Str(value) => Ok(value.clone()),
        _ => Err(unsupported(name, span)),
    }
}

fn app_binding(value: &CtValue, span: Span) -> Result<Vec<web_kernel::JetAppRouteBinding>, Diagnostic> {
    let value = app_string(value, "app route binding", span)?;
    Ok(web_kernel::jet_app_route_binding(&value))
}

fn app_callback(
    value: &CtValue,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    if matches!(value, CtValue::Closure(_)) {
        Ok(value.clone())
    } else {
        Err(unsupported("app callback", span))
    }
}

fn tree_text(tree: &web_kernel::DataTree) -> Option<String> {
    match tree {
        web_kernel::DataTree::Text(value) | web_kernel::DataTree::TypedText(value) => {
            Some(value.clone())
        }
        web_kernel::DataTree::Int(value) => Some(value.to_string()),
        web_kernel::DataTree::Float(value) => Some(value.to_string()),
        web_kernel::DataTree::Bool(value) => Some(value.to_string()),
        web_kernel::DataTree::Number(value) => Some(value.clone()),
        _ => None,
    }
}

fn tree_to_untyped(tree: &web_kernel::DataTree, span: Span) -> Result<CtValue, String> {
    match tree {
        web_kernel::DataTree::Null => Ok(CtValue::Unit),
        web_kernel::DataTree::Bool(value) => Ok(CtValue::Bool(*value)),
        web_kernel::DataTree::Int(value) => Ok(CtValue::Int(*value)),
        web_kernel::DataTree::Float(value) => Ok(CtValue::Float(crate::AST::CtFloat::f64(*value))),
        web_kernel::DataTree::Number(value) => value
            .parse::<i64>()
            .map(CtValue::Int)
            .or_else(|_| value.parse::<f64>().map(|value| CtValue::Float(crate::AST::CtFloat::f64(value))))
            .map_err(|_| format!("invalid JSON number `{value}`")),
        web_kernel::DataTree::TypedText(value) => {
            let tree = parse_app_json_tree(value)?;
            tree_to_untyped(&tree, span)
        }
        web_kernel::DataTree::Text(value) => Ok(CtValue::Str(value.clone())),
        web_kernel::DataTree::Bytes(value) => Ok(CtValue::Bytes(value.clone())),
        web_kernel::DataTree::Array(values) => values
            .iter()
            .map(|value| tree_to_untyped(value, span))
            .collect::<Result<Vec<_>, _>>()
            .map(CtValue::List),
        web_kernel::DataTree::Object(values) => values
            .iter()
            .map(|(name, value)| Ok((name.clone(), tree_to_untyped(value, span)?)))
            .collect::<Result<Vec<_>, String>>()
            .map(|fields| CtValue::Struct {
                type_name: "RouteObject".to_string(),
                fields,
            }),
    }
}

fn binding_type(value: &str) -> (String, bool) {
    match value.strip_suffix('?') {
        Some(value) => (value.to_string(), true),
        None => (value.to_string(), false),
    }
}

fn binding_ast_type(value: &str) -> Type {
    let (value, optional) = binding_type(value);
    let ty = match value.as_str() {
        "Int" => Type::Int,
        "Float" => Type::Float,
        "Bool" => Type::Bool,
        "String" | "Text" => Type::String,
        "Char" => Type::Char,
        value => Type::Named(value.to_string()),
    };
    if optional {
        Type::Option(Box::new(ty))
    } else {
        ty
    }
}

fn decode_binding_tree(
    tree: &web_kernel::DataTree,
    ty: &str,
    span: Span,
) -> Result<CtValue, String> {
    let (base, optional) = binding_type(ty);
    if optional {
        if matches!(tree, web_kernel::DataTree::Null) {
            return Ok(CtValue::absent(binding_ast_type(ty)));
        }
        return decode_binding_tree(tree, &base, span)
            .map(|value| CtValue::Present(Box::new(value)));
    }
    match base.as_str() {
        "Int" => tree_text(tree)
            .and_then(|value| value.parse::<i64>().ok())
            .map(CtValue::Int)
            .ok_or_else(|| format!("route input is not a valid Int: {tree:?}")),
        "Float" => tree_text(tree)
            .and_then(|value| value.parse::<f64>().ok())
            .map(|value| CtValue::Float(crate::AST::CtFloat::f64(value)))
            .ok_or_else(|| format!("route input is not a valid Float: {tree:?}")),
        "Bool" => tree_text(tree)
            .and_then(|value| match value.as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            })
            .map(CtValue::Bool)
            .ok_or_else(|| format!("route input is not a valid Bool: {tree:?}")),
        "String" | "Text" => tree_text(tree)
            .map(CtValue::Str)
            .ok_or_else(|| format!("route input is not valid Text: {tree:?}")),
        "Char" => tree_text(tree)
            .and_then(|value| value.chars().next().filter(|_| value.chars().count() == 1))
            .map(CtValue::Char)
            .ok_or_else(|| format!("route input is not a valid Char: {tree:?}")),
        _ => tree_to_untyped(tree, span).map(|value| match value {
            CtValue::Struct { fields, .. } => CtValue::Struct {
                type_name: base.clone(),
                fields,
            },
            value => value,
        }),
    }
}

fn parse_app_json_tree(value: &str) -> Result<web_kernel::DataTree, String> {
    jet_foundation::EncodingJson::parse_json(value, true)
        .map(|tree| tree)
        .map_err(|error| error.message)
}

fn mir_tree_to_ct(
    tree: &web_kernel::DataTree,
    ty: &jet_foundation::MIR::MirType,
    span: Span,
) -> Result<CtValue, String> {
    use jet_foundation::MIR::MirTypeKind;
    match ty.kind() {
        MirTypeKind::Option(inner) => {
            if matches!(tree, web_kernel::DataTree::Null) {
                Ok(CtValue::absent(crate::Comptime::MirBridge::mir_to_ast_type(inner)))
            } else {
                mir_tree_to_ct(tree, inner, span).map(|value| CtValue::Present(Box::new(value)))
            }
        }
        MirTypeKind::Shared(inner)
        | MirTypeKind::Tagged { inner, .. }
        | MirTypeKind::Quantity { base: inner, .. }
        | MirTypeKind::InlineRange { base: inner, .. } => mir_tree_to_ct(tree, inner, span),
        MirTypeKind::Int | MirTypeKind::IntN { .. } => decode_binding_tree(tree, "Int", span),
        MirTypeKind::Float | MirTypeKind::Float32 => decode_binding_tree(tree, "Float", span),
        MirTypeKind::Bool => decode_binding_tree(tree, "Bool", span),
        MirTypeKind::String => decode_binding_tree(tree, "String", span),
        MirTypeKind::Char => decode_binding_tree(tree, "Char", span),
        MirTypeKind::List(inner) | MirTypeKind::FixedList { elem: inner, .. } => {
            let web_kernel::DataTree::Array(values) = tree else {
                return Err(format!("route input is not a list: {tree:?}"));
            };
            values
                .iter()
                .map(|value| mir_tree_to_ct(value, inner, span))
                .collect::<Result<Vec<_>, _>>()
                .map(CtValue::List)
        }
        MirTypeKind::Map { value: value_ty, .. } => {
            let web_kernel::DataTree::Object(values) = tree else {
                return Err(format!("route input is not an object: {tree:?}"));
            };
            values
                .iter()
                .map(|(key, value)| {
                    Ok((
                        CtKey::Str(key.clone()),
                        mir_tree_to_ct(value, value_ty, span)?,
                    ))
                })
                .collect::<Result<std::collections::BTreeMap<_, _>, String>>()
                .map(CtValue::Map)
        }
        MirTypeKind::Tuple(fields) => {
            let web_kernel::DataTree::Object(values) = tree else {
                return Err(format!("route input is not a tuple object: {tree:?}"));
            };
            fields
                .iter()
                .map(|(name, field_ty)| {
                    let value = values
                        .iter()
                        .find(|(key, _)| key == name)
                        .map(|(_, value)| value)
                        .ok_or_else(|| format!("route input is missing `{name}`"))?;
                    Ok((name.clone(), mir_tree_to_ct(value, field_ty, span)?))
                })
                .collect::<Result<Vec<_>, String>>()
                .map(|fields| CtValue::Struct {
                    type_name: "RouteTuple".to_string(),
                    fields,
                })
        }
        MirTypeKind::Apply { name, .. } => {
            let value = tree_to_untyped(tree, span)?;
            Ok(match value {
                CtValue::Struct { fields, .. } => CtValue::Struct {
                    type_name: name.name.clone(),
                    fields,
                },
                value => value,
            })
        }
        _ => tree_to_untyped(tree, span),
    }
}

fn decode_route_inputs(
    trees: &[web_kernel::DataTree],
    bindings: &[web_kernel::JetAppRouteBinding],
    types: &[jet_foundation::MIR::MirType],
    span: Span,
) -> Result<Vec<CtValue>, String> {
    if trees.len() != bindings.len() || trees.len() != types.len() {
        return Err(format!(
            "app callback received {} inputs for {} checked bindings",
            trees.len(),
            bindings.len()
        ));
    }
    trees
        .iter()
        .zip(bindings)
        .zip(types)
        .map(|((tree, binding), ty)| match binding {
            web_kernel::JetAppRouteBinding::Search {
                scalar: None,
                fields,
                ..
            } => {
                let web_kernel::DataTree::Object(values) = tree else {
                    return Err("route search input is not an object".to_string());
                };
                fields
                    .iter()
                    .map(|field| {
                        let value = values.iter().find(|(name, _)| name == &field.name);
                        match value {
                            Some((_, value)) => decode_binding_tree(value, &field.ty, span)
                                .map(|value| (field.name.clone(), value)),
                            None if !field.required => Ok((
                                field.name.clone(),
                                CtValue::absent(binding_ast_type(&field.ty)),
                            )),
                            None => Err(format!("route search input is missing `{}`", field.name)),
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map(|fields| CtValue::Struct {
                        type_name: ty.nominal_name().unwrap_or("RouteSearch").to_string(),
                        fields,
                    })
            }
            web_kernel::JetAppRouteBinding::Data => {
                let tree = match tree {
                    web_kernel::DataTree::TypedText(value) => parse_app_json_tree(value)?,
                    tree => tree.clone(),
                };
                mir_tree_to_ct(&tree, ty, span)
            }
            _ => mir_tree_to_ct(tree, ty, span),
        })
        .collect()
}

fn callback_error(value: CtValue) -> String {
    match value {
        CtValue::Failed(report) => format!("{report:?}"),
        value => format!("{value:?}"),
    }
}

fn callback_page(value: CtValue) -> Result<web_kernel::JetWebPage, String> {
    let value = match value {
        CtValue::Present(value) => *value,
        value => value,
    };
    let CtValue::Struct { fields, .. } = value else {
        return Err("app page callback did not return WebPage".to_string());
    };
    let title = fields
        .iter()
        .find(|(name, _)| name == "title")
        .and_then(|(_, value)| match value {
            CtValue::Str(value) => Some(value.clone()),
            _ => None,
        })
        .ok_or_else(|| "app page callback returned WebPage without title".to_string())?;
    let body = fields
        .iter()
        .find(|(name, _)| name == "body")
        .and_then(|(_, value)| match value {
            CtValue::Str(value) => Some(value.clone()),
            _ => None,
        })
        .ok_or_else(|| "app page callback returned WebPage without body".to_string())?;
    Ok(web_kernel::JetWebPage { title, body })
}

fn app_page_callback(
    callback: CtValue,
    types: Vec<jet_foundation::MIR::MirType>,
    binding: Vec<web_kernel::JetAppRouteBinding>,
    span: Span,
) -> web_kernel::JetAppTreeRouteCallback<Result<web_kernel::JetWebPage, String>> {
    let arity = types.len();
    web_kernel::JetAppTreeRouteCallback::new(
        arity,
        std::sync::Arc::new(move |trees| {
            let values = decode_route_inputs(trees, &binding, &types, span)?;
            let value = super::Methods::invoke_standalone_closure(&callback, values, span)
                .map_err(|error| format!("{error:?}"))?;
            match value {
                CtValue::Failed(report) => Err(format!("{report:?}")),
                value => Ok(callback_page(value)),
            }
        }),
    )
}

fn app_boundary_callback(
    callback: CtValue,
    span: Span,
) -> web_kernel::JetAppTreeRouteCallback<Result<web_kernel::JetWebPage, String>> {
    web_kernel::JetAppTreeRouteCallback::new(
        0,
        std::sync::Arc::new(move |_| {
            let value = super::Methods::invoke_standalone_closure(&callback, Vec::new(), span)
                .map_err(|error| format!("{error:?}"))?;
            match value {
                CtValue::Failed(report) => Err(format!("{report:?}")),
                value => Ok(callback_page(value)),
            }
        }),
    )
}

fn app_loader_callback(
    callback: CtValue,
    types: Vec<jet_foundation::MIR::MirType>,
    binding: Vec<web_kernel::JetAppRouteBinding>,
    span: Span,
) -> web_kernel::JetAppTreeRouteCallback<Result<String, String>> {
    let arity = types.len();
    web_kernel::JetAppTreeRouteCallback::new(
        arity,
        std::sync::Arc::new(move |trees| {
            let values = decode_route_inputs(trees, &binding, &types, span)?;
            let value = super::Methods::invoke_standalone_closure(&callback, values, span)
                .map_err(|error| format!("{error:?}"))?;
            match value {
                CtValue::Failed(report) => Err(format!("{report:?}")),
                CtValue::Present(value) => Ok(Ok(ct_to_json_string(value.as_ref()))),
                value => Ok(Ok(ct_to_json_string(&value))),
            }
        }),
    )
}

fn ct_to_json_string(value: &CtValue) -> String {
    fn tree(value: &CtValue) -> web_kernel::DataTree {
        match value {
            CtValue::Int(value) => web_kernel::DataTree::Int(*value),
            CtValue::Float(value) => web_kernel::DataTree::Float(value.as_f64()),
            CtValue::Bool(value) => web_kernel::DataTree::Bool(*value),
            CtValue::Char(value) => web_kernel::DataTree::Text(value.to_string()),
            CtValue::Str(value) => web_kernel::DataTree::Text(value.clone()),
            CtValue::Bytes(value) => web_kernel::DataTree::Bytes(value.clone()),
            CtValue::List(values) => {
                web_kernel::DataTree::Array(values.iter().map(tree).collect())
            }
            CtValue::Map(values) => web_kernel::DataTree::Object(
                values
                    .iter()
                    .filter_map(|(key, value)| match key {
                        CtKey::Str(key) => Some((key.clone(), tree(value))),
                        _ => None,
                    })
                    .collect(),
            ),
            CtValue::Struct { fields, .. } => web_kernel::DataTree::Object(
                fields.iter().map(|(key, value)| (key.clone(), tree(value))).collect(),
            ),
            CtValue::Enum { variant, .. } if variant == "Null" => {
                web_kernel::DataTree::Null
            }
            CtValue::Enum { args, .. } => web_kernel::DataTree::Object(
                args.iter()
                    .filter_map(|(key, value)| {
                        key.as_ref().map(|key| (key.clone(), tree(value)))
                    })
                    .collect(),
            ),
            CtValue::Present(value) => tree(value),
            CtValue::Failed(_) | CtValue::Unit => web_kernel::DataTree::Null,
            CtValue::BigInt(value) => web_kernel::DataTree::Number(value.to_string_rep()),
            CtValue::Closure(_) => web_kernel::DataTree::Null,
        }
    }
    fn render(tree: &web_kernel::DataTree) -> String {
        match tree {
            web_kernel::DataTree::Null => "null".to_string(),
            web_kernel::DataTree::Bool(value) => value.to_string(),
            web_kernel::DataTree::Int(value) => value.to_string(),
            web_kernel::DataTree::Float(value) => format!("{value:?}"),
            web_kernel::DataTree::Number(value) => value.clone(),
            web_kernel::DataTree::TypedText(value) | web_kernel::DataTree::Text(value) => {
                format!("{value:?}")
            }
            web_kernel::DataTree::Bytes(values) => format!(
                "[{}]",
                values.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
            ),
            web_kernel::DataTree::Array(values) => {
                format!("[{}]", values.iter().map(render).collect::<Vec<_>>().join(","))
            }
            web_kernel::DataTree::Object(values) => format!(
                "{{{}}}",
                values
                    .iter()
                    .map(|(key, value)| format!("{:?}:{}", key, render(value)))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
    render(&tree(value))
}

fn app_output_type(value: Option<&jet_foundation::MIR::MirType>) -> String {
    let Some(value) = value else {
        return "Unit".to_string();
    };
    match value.kind() {
        jet_foundation::MIR::MirTypeKind::Result { ok, .. } => ok.canonical_key(),
        _ => value.canonical_key(),
    }
}

pub fn app_method_runtime(
    handle: &AppHandle,
    method: &str,
    args: &[CtValue],
    callback_types: &[jet_foundation::MIR::MirType],
    callback_return_type: Option<&jet_foundation::MIR::MirType>,
    span: Span,
) -> Result<AppRuntimeResult, Diagnostic> {
    let one = |index: usize| {
        args.get(index)
            .ok_or_else(|| unsupported(&format!("app.{method} argument {index}"), span))
    };
    let wrap_app = |value: web_kernel::JetApp| AppRuntimeResult::App(AppHandle(value));
    match method {
        "route" | "page" | "layout" => {
            let path = app_string(one(0)?, "app route path", span)?;
            let callback = app_callback(one(1)?, span)?;
            let binding_text = app_string(one(2)?, "app route binding", span)?;
            let binding = web_kernel::jet_app_route_binding(&binding_text);
            let callback = app_page_callback(callback, callback_types.to_vec(), binding, span);
            let app = match method {
                "route" => web_kernel::jet_app_route(&handle.0, path, callback, binding_text),
                "page" => web_kernel::jet_app_page(&handle.0, path, callback, binding_text),
                _ => web_kernel::jet_app_layout(&handle.0, path, callback, binding_text),
            };
            Ok(wrap_app(app))
        }
        "loader" => {
            let callback = app_callback(one(0)?, span)?;
            let binding_text = app_string(
                args.last()
                    .ok_or_else(|| unsupported("app loader binding", span))?,
                "app loader binding",
                span,
            )?;
            let binding = web_kernel::jet_app_route_binding(&binding_text);
            let callback =
                app_loader_callback(callback, callback_types.to_vec(), binding.clone(), span);
            let preload = if args.len() == 4 {
                match one(2)? {
                    CtValue::Bool(value) => *value,
                    _ => return Err(unsupported("app loader preload", span)),
                }
            } else {
                false
            };
            let data_type = app_output_type(callback_return_type);
            let app = handle.0.loader_encoded(callback, binding, data_type, preload);
            Ok(wrap_app(app))
        }
        "pending" | "not_found" | "error" => {
            let callback = app_boundary_callback(app_callback(one(0)?, span)?, span);
            let app = match method {
                "pending" => web_kernel::jet_app_pending(&handle.0, callback),
                "not_found" => web_kernel::jet_app_not_found(&handle.0, callback),
                _ => web_kernel::jet_app_error(&handle.0, callback),
            };
            Ok(wrap_app(app))
        }
        "action" | "form" | "data" => {
            let name = app_string(one(0)?, "app server function name", span)?;
            let callback = app_callback(one(1)?, span)?;
            let invoke = move |args| {
                let value = super::Methods::invoke_standalone_closure(&callback, args, span)
                    .map_err(|error| format!("{error:?}"))?;
                match value {
                    CtValue::Failed(report) => Err(format!("{report:?}")),
                    value => Ok(ct_to_json_string(&value)),
                }
            };
            let app = if callback_types.is_empty() {
                let action: std::sync::Arc<dyn Fn() -> Result<String, String> + Send + Sync> =
                    std::sync::Arc::new(move || invoke(Vec::new()));
                match method {
                    "action" => web_kernel::jet_app_action(&handle.0, name, action),
                    "form" => web_kernel::jet_app_form(&handle.0, name, action),
                    _ => web_kernel::jet_app_data(&handle.0, name, action),
                }
            } else {
                let action: std::sync::Arc<
                    dyn Fn(&String) -> Result<String, String> + Send + Sync,
                > = std::sync::Arc::new(move |body| {
                    invoke(vec![CtValue::Str(body.clone())])
                });
                match method {
                    "action" => web_kernel::jet_app_action(&handle.0, name, action),
                    "form" => web_kernel::jet_app_form(&handle.0, name, action),
                    _ => web_kernel::jet_app_data(&handle.0, name, action),
                }
            };
            Ok(wrap_app(app))
        }
        "mount" => {
            let prefix = app_string(one(0)?, "app mount prefix", span)?;
            let callback = app_callback(one(1)?, span)?;
            let app = handle.0.mount(prefix, move |path| {
                let _ = super::Methods::invoke_standalone_closure(
                    &callback,
                    vec![CtValue::Str(path.clone())],
                    span,
                );
            });
            Ok(wrap_app(app))
        }
        "routes" | "security" | "assets" | "split" | "code_split" | "cache" | "a11y"
        | "adapter" => {
            let value = app_string(one(0)?, "app setting", span)?;
            let app = match method {
                "routes" => web_kernel::jet_app_routes(&handle.0, value),
                "security" => web_kernel::jet_app_security(&handle.0, value),
                "assets" => web_kernel::jet_app_assets(&handle.0, value),
                "split" => web_kernel::jet_app_split(&handle.0, value),
                "code_split" => web_kernel::jet_app_code_split(&handle.0, value),
                "cache" => web_kernel::jet_app_cache(&handle.0, value),
                "a11y" => web_kernel::jet_app_a11y(&handle.0, value),
                _ => web_kernel::jet_app_adapter(&handle.0, value),
            };
            Ok(wrap_app(app))
        }
        "csr" | "ssr" | "ssg" | "stream" | "streaming" | "island" | "hydration_dev"
        | "hydration_release" => {
            let app = match method {
                "csr" => web_kernel::jet_app_csr(&handle.0),
                "ssr" => web_kernel::jet_app_ssr(&handle.0),
                "ssg" => web_kernel::jet_app_ssg(&handle.0),
                "stream" => web_kernel::jet_app_stream(&handle.0),
                "streaming" => web_kernel::jet_app_streaming(&handle.0),
                "island" => web_kernel::jet_app_island(&handle.0),
                "hydration_dev" => web_kernel::jet_app_hydration_dev(&handle.0),
                _ => web_kernel::jet_app_hydration_release(&handle.0),
            };
            Ok(wrap_app(app))
        }
        "facts_json" => Ok(AppRuntimeResult::Value(CtValue::Str(
            web_kernel::jet_app_facts_json(&handle.0),
        ))),
        "serve" | "serve_on" => {
            if let Some(value) = args.first() {
                let port = match value {
                    CtValue::Int(value) if *value >= 0 && *value <= i64::from(u16::MAX) => {
                        *value as i64
                    }
                    _ => return Err(unsupported("app serve port", span)),
                };
                web_kernel::jet_app_serve_on(&handle.0, port);
            } else {
                web_kernel::jet_app_serve(&handle.0);
            }
            Ok(AppRuntimeResult::Value(CtValue::Unit))
        }
        _ => Err(unsupported(&format!("`App.{method}()`"), span)),
    }
}

fn web_router(value: &CtValue, span: Span) -> Result<&web_kernel::JetWebRouter, Diagnostic> {
    let CtValue::Closure(data) = value else {
        return Err(unsupported("WebRouter", span));
    };
    data.opaque
        .as_ref()
        .and_then(|value| value.downcast_ref::<WebRouterHandle>())
        .map(|value| &value.0)
        .ok_or_else(|| unsupported("WebRouter", span))
}

fn web_query(value: &CtValue, span: Span) -> Result<&web_kernel::JetWebQuery, Diagnostic> {
    let CtValue::Closure(data) = value else {
        return Err(unsupported("WebQuery", span));
    };
    data.opaque
        .as_ref()
        .and_then(|value| value.downcast_ref::<WebQueryHandle>())
        .map(|value| &value.0)
        .ok_or_else(|| unsupported("WebQuery", span))
}

fn web_form(value: &CtValue, span: Span) -> Result<&web_kernel::JetWebForm, Diagnostic> {
    let CtValue::Closure(data) = value else {
        return Err(unsupported("WebForm", span));
    };
    data.opaque
        .as_ref()
        .and_then(|value| value.downcast_ref::<WebFormHandle>())
        .map(|value| &value.0)
        .ok_or_else(|| unsupported("WebForm", span))
}

fn web_table(value: &CtValue, span: Span) -> Result<&web_kernel::JetWebTable<CtValue>, Diagnostic> {
    let CtValue::Closure(data) = value else {
        return Err(unsupported("WebTable", span));
    };
    data.opaque
        .as_ref()
        .and_then(|value| value.downcast_ref::<WebTableHandle>())
        .map(|value| &value.0)
        .ok_or_else(|| unsupported("WebTable", span))
}

fn web_table_column(
    value: &CtValue,
    span: Span,
) -> Result<&web_kernel::JetWebTableColumn<CtValue>, Diagnostic> {
    let CtValue::Closure(data) = value else {
        return Err(unsupported("WebTableColumn", span));
    };
    data.opaque
        .as_ref()
        .and_then(|value| value.downcast_ref::<WebTableColumnHandle>())
        .map(|value| &value.0)
        .ok_or_else(|| unsupported("WebTableColumn", span))
}

fn web_virtual_plan_viewport(
    value: &CtValue,
    span: Span,
) -> Result<&web_kernel::JetWebVirtualPlanViewport, Diagnostic> {
    let CtValue::Closure(data) = value else {
        return Err(unsupported("WebVirtualPlanViewport", span));
    };
    data.opaque
        .as_ref()
        .and_then(|value| value.downcast_ref::<WebVirtualPlanViewportHandle>())
        .map(|value| &value.0)
        .ok_or_else(|| unsupported("WebVirtualPlanViewport", span))
}

fn web_store(value: &CtValue, span: Span) -> Result<&web_kernel::JetWebStore<CtValue>, Diagnostic> {
    let CtValue::Closure(data) = value else {
        return Err(unsupported("WebStore", span));
    };
    data.opaque
        .as_ref()
        .and_then(|value| value.downcast_ref::<WebStoreHandle>())
        .map(|value| &value.0)
        .ok_or_else(|| unsupported("WebStore", span))
}

fn web_string(value: &CtValue, what: &str, span: Span) -> Result<String, Diagnostic> {
    match value {
        CtValue::Str(value) => Ok(value.clone()),
        _ => Err(unsupported(what, span)),
    }
}

fn web_bool(value: &CtValue, what: &str, span: Span) -> Result<bool, Diagnostic> {
    match value {
        CtValue::Bool(value) => Ok(*value),
        _ => Err(unsupported(what, span)),
    }
}

fn web_int(value: &CtValue, what: &str, span: Span) -> Result<i64, Diagnostic> {
    match value {
        CtValue::Int(value) => Ok(*value),
        CtValue::BigInt(value) => value
            .try_i64()
            .ok_or_else(|| unsupported(what, span)),
        _ => Err(unsupported(what, span)),
    }
}

fn web_strings(value: &CtValue, what: &str, span: Span) -> Result<Vec<String>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(unsupported(what, span));
    };
    values
        .iter()
        .map(|value| web_string(value, what, span))
        .collect()
}

fn web_int_pairs(
    value: &CtValue,
    what: &str,
    span: Span,
) -> Result<Vec<(i64, i64)>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(unsupported(what, span));
    };
    values
        .iter()
        .map(|value| {
            let CtValue::Struct { fields, .. } = value else {
                return Err(unsupported(what, span));
            };
            let field = |name: &str| {
                fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, value)| value)
            };
            let index = field("index")
                .or_else(|| fields.first().map(|(_, value)| value))
                .ok_or_else(|| unsupported(what, span))?;
            let size = field("size")
                .or_else(|| fields.get(1).map(|(_, value)| value))
                .ok_or_else(|| unsupported(what, span))?;
            Ok((
                web_int(index, "measurement index", span)?,
                web_int(size, "measurement size", span)?,
            ))
        })
        .collect()
}

fn web_string_map(
    value: &CtValue,
    what: &str,
    span: Span,
) -> Result<std::collections::BTreeMap<String, String>, Diagnostic> {
    let CtValue::Map(values) = value else {
        return Err(unsupported(what, span));
    };
    values
        .iter()
        .map(|(key, value)| {
            let CtKey::Str(key) = key else {
                return Err(unsupported(what, span));
            };
            Ok((key.clone(), web_string(value, what, span)?))
        })
        .collect()
}

fn web_string_map_value(values: std::collections::BTreeMap<String, String>) -> CtValue {
    CtValue::Map(
        values
            .into_iter()
            .map(|(key, value)| (CtKey::Str(key), CtValue::Str(value)))
            .collect(),
    )
}

fn web_navigation(value: web_kernel::JetWebNavigation) -> CtValue {
    CtValue::Struct {
        type_name: "WebNavigation".to_string(),
        fields: vec![
            ("url".to_string(), CtValue::Str(value.url)),
            ("route".to_string(), CtValue::Str(value.route)),
            ("params".to_string(), web_string_map_value(value.params)),
            ("search".to_string(), web_string_map_value(value.search)),
        ],
    }
}

fn web_navigation_status(value: web_kernel::JetWebNavigationStatus) -> CtValue {
    let variant = match value {
        web_kernel::JetWebNavigationStatus::Idle => "Idle",
        web_kernel::JetWebNavigationStatus::Preloading => "Preloading",
        web_kernel::JetWebNavigationStatus::Pending => "Pending",
        web_kernel::JetWebNavigationStatus::Ready => "Ready",
        web_kernel::JetWebNavigationStatus::Error => "Error",
        web_kernel::JetWebNavigationStatus::Aborted => "Aborted",
    };
    CtValue::Enum {
        type_name: "WebNavigationStatus".to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn web_optional_string(value: Option<String>) -> CtValue {
    value
        .map(CtValue::Str)
        .map(|value| CtValue::Present(Box::new(value)))
        .unwrap_or_else(|| CtValue::absent(Type::String))
}

fn web_navigation_state(value: web_kernel::JetWebNavigationState) -> CtValue {
    let pending_boundary_id = value
        .pending_boundary_id
        .map(|boundary| boundary.as_str().to_string());
    let error_boundary_id = value
        .error_boundary_id
        .map(|boundary| boundary.as_str().to_string());
    let island_identity = value.island_identity.map(|identity| identity.compact());
    let hydration_trigger = value
        .hydration_trigger
        .map(|trigger| trigger.as_str().to_string());
    CtValue::Struct {
        type_name: "WebNavigationState".to_string(),
        fields: vec![
            ("status".to_string(), web_navigation_status(value.status)),
            (
                "current".to_string(),
                value
                    .current
                    .map(web_navigation)
                    .map(|value| CtValue::Present(Box::new(value)))
                    .unwrap_or_else(|| CtValue::absent(Type::Named("WebNavigation".to_string()))),
            ),
            ("data".to_string(), CtValue::Str(value.data)),
            ("error".to_string(), CtValue::Str(value.error)),
            ("pending_boundary_id".to_string(), web_optional_string(pending_boundary_id)),
            ("error_boundary_id".to_string(), web_optional_string(error_boundary_id)),
            ("island_identity".to_string(), web_optional_string(island_identity)),
            ("hydration_trigger".to_string(), web_optional_string(hydration_trigger)),
        ],
    }
}

fn web_query_status(value: web_kernel::JetWebQueryStatus) -> CtValue {
    let variant = match value {
        web_kernel::JetWebQueryStatus::Pending => "Pending",
        web_kernel::JetWebQueryStatus::Fresh => "Fresh",
        web_kernel::JetWebQueryStatus::Stale => "Stale",
        web_kernel::JetWebQueryStatus::Fetching => "Fetching",
        web_kernel::JetWebQueryStatus::Error => "Error",
        web_kernel::JetWebQueryStatus::Offline => "Offline",
    };
    CtValue::Enum {
        type_name: "WebQueryStatus".to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn web_query_state(value: web_kernel::JetWebQueryState) -> CtValue {
    CtValue::Struct {
        type_name: "WebQueryState".to_string(),
        fields: vec![
            ("status".to_string(), web_query_status(value.status)),
            ("value".to_string(), CtValue::Str(value.value)),
            ("error".to_string(), CtValue::Str(value.error)),
            ("generation".to_string(), CtValue::Int(value.generation as i64)),
            ("queued".to_string(), CtValue::Int(value.queued as i64)),
            ("offline".to_string(), CtValue::Bool(value.offline)),
        ],
    }
}

fn web_router_value_type(
    value: &CtValue,
    span: Span,
) -> Result<web_kernel::JetWebRouterValueType, Diagnostic> {
    let CtValue::Enum { type_name, variant, .. } = value else {
        return Err(unsupported("WebRouterValueType", span));
    };
    if type_name != "WebRouterValueType" {
        return Err(unsupported("WebRouterValueType", span));
    }
    match variant.as_str() {
        "String" => Ok(web_kernel::JetWebRouterValueType::String),
        "Int" => Ok(web_kernel::JetWebRouterValueType::Int),
        "Bool" => Ok(web_kernel::JetWebRouterValueType::Bool),
        "Float" => Ok(web_kernel::JetWebRouterValueType::Float),
        "JSON" | "Json" => Ok(web_kernel::JetWebRouterValueType::Json),
        _ => Err(unsupported("WebRouterValueType", span)),
    }
}

fn web_router_codec(
    value: &CtValue,
    span: Span,
) -> Result<web_kernel::JetWebRouterSearchCodec, Diagnostic> {
    let CtValue::Enum { type_name, variant, .. } = value else {
        return Err(unsupported("WebRouterSearchCodec", span));
    };
    if type_name != "WebRouterSearchCodec" {
        return Err(unsupported("WebRouterSearchCodec", span));
    }
    match variant.as_str() {
        "Query" => Ok(web_kernel::JetWebRouterSearchCodec::Query),
        "JSON" | "Json" => Ok(web_kernel::JetWebRouterSearchCodec::Json),
        _ => Err(unsupported("WebRouterSearchCodec", span)),
    }
}

fn web_optional_string_value(
    value: &CtValue,
    span: Span,
) -> Result<Option<String>, Diagnostic> {
    match value {
        CtValue::Present(value) => Ok(Some(web_string(value, "router field default", span)?)),
        CtValue::Failed(jet_foundation::AST::CtReport::Clean(_)) => Ok(None),
        _ => Err(unsupported("router field default", span)),
    }
}

fn web_router_fields(
    value: &CtValue,
    span: Span,
) -> Result<Vec<web_kernel::JetWebRouterField>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(unsupported("WebRouterField list", span));
    };
    values
        .iter()
        .map(|value| {
            let CtValue::Struct { type_name, fields } = value else {
                return Err(unsupported("WebRouterField", span));
            };
            if type_name != "WebRouterField" {
                return Err(unsupported("WebRouterField", span));
            }
            let field = |name: &str| {
                fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, value)| value)
            };
            let name = web_string(
                field("name").ok_or_else(|| unsupported("WebRouterField.name", span))?,
                "WebRouterField.name",
                span,
            )?;
            let value_type = web_router_value_type(
                field("value_type")
                    .ok_or_else(|| unsupported("WebRouterField.value_type", span))?,
                span,
            )?;
            let required = web_bool(
                field("required")
                    .ok_or_else(|| unsupported("WebRouterField.required", span))?,
                "WebRouterField.required",
                span,
            )?;
            let default = web_optional_string_value(
                field("default")
                    .ok_or_else(|| unsupported("WebRouterField.default", span))?,
                span,
            )?;
            Ok(web_kernel::JetWebRouterField {
                name,
                value_type,
                required,
                default,
            })
        })
        .collect()
}

fn web_mutation_state(value: web_kernel::JetWebMutationState) -> CtValue {
    CtValue::Struct {
        type_name: "WebMutationState".to_string(),
        fields: vec![
            (
                "status".to_string(),
                CtValue::Enum {
                    type_name: "WebMutationStatus".to_string(),
                    variant: value.status.name().to_string(),
                    args: Vec::new(),
                },
            ),
            ("generation".to_string(), CtValue::Int(value.generation as i64)),
            ("optimistic".to_string(), CtValue::Bool(value.optimistic)),
            ("value".to_string(), CtValue::Str(value.value)),
            ("result".to_string(), CtValue::Str(value.result)),
            ("error".to_string(), CtValue::Str(value.error)),
            ("context".to_string(), CtValue::Str(value.context)),
            ("rollback".to_string(), CtValue::Bool(value.rollback)),
            ("paused".to_string(), CtValue::Bool(value.paused)),
            ("queued".to_string(), CtValue::Int(value.queued as i64)),
            ("replayed".to_string(), CtValue::Int(value.replayed as i64)),
        ],
    }
}
fn web_query_mode(
    value: &CtValue,
    span: Span,
) -> Result<web_kernel::JetWebQueryNetworkMode, Diagnostic> {
    let CtValue::Enum {
        type_name,
        variant,
        ..
    } = value
    else {
        return Err(unsupported("WebQueryNetworkMode", span));
    };
    if type_name != "WebQueryNetworkMode" {
        return Err(unsupported("WebQueryNetworkMode", span));
    }
    match variant.as_str() {
        "Online" => Ok(web_kernel::JetWebQueryNetworkMode::Online),
        "Always" => Ok(web_kernel::JetWebQueryNetworkMode::Always),
        "OfflineFirst" => Ok(web_kernel::JetWebQueryNetworkMode::OfflineFirst),
        _ => Err(unsupported("WebQueryNetworkMode", span)),
    }
}


fn web_virtual_window(value: web_kernel::JetWebVirtualWindow) -> CtValue {
    CtValue::Struct {
        type_name: "WebVirtualWindow".to_string(),
        fields: vec![
            ("total_count".to_string(), CtValue::Int(value.total_count)),
            ("scroll_offset".to_string(), CtValue::Int(value.scroll_offset)),
            ("viewport_size".to_string(), CtValue::Int(value.viewport_size)),
            (
                "estimated_item_size".to_string(),
                CtValue::Int(value.estimated_item_size),
            ),
            ("overscan".to_string(), CtValue::Int(value.overscan)),
            ("start".to_string(), CtValue::Int(value.start)),
            ("end".to_string(), CtValue::Int(value.end)),
            ("total_size".to_string(), CtValue::Int(value.total_size)),
            ("measured_count".to_string(), CtValue::Int(value.measured_count)),
        ],
    }
}

fn web_virtual_window_value(
    value: &CtValue,
    span: Span,
) -> Result<web_kernel::JetWebVirtualWindow, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("WebVirtualWindow", span));
    };
    if type_name != "WebVirtualWindow" {
        return Err(unsupported("WebVirtualWindow", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
            .ok_or_else(|| unsupported("WebVirtualWindow field", span))
    };
    Ok(web_kernel::JetWebVirtualWindow {
        total_count: web_int(field("total_count")?, "total_count", span)?,
        scroll_offset: web_int(field("scroll_offset")?, "scroll_offset", span)?,
        viewport_size: web_int(field("viewport_size")?, "viewport_size", span)?,
        estimated_item_size: web_int(field("estimated_item_size")?, "estimated_item_size", span)?,
        overscan: web_int(field("overscan")?, "overscan", span)?,
        start: web_int(field("start")?, "start", span)?,
        end: web_int(field("end")?, "end", span)?,
        total_size: web_int(field("total_size")?, "total_size", span)?,
        measured_count: web_int(field("measured_count")?, "measured_count", span)?,
    })
}

fn web_virtual_plan(value: web_kernel::JetWebVirtualPlan) -> CtValue {
    CtValue::Struct {
        type_name: "WebVirtualPlan".to_string(),
        fields: vec![
            ("total_count".to_string(), CtValue::Int(value.total_count)),
            ("scroll_offset".to_string(), CtValue::Int(value.scroll_offset)),
            ("viewport_width".to_string(), CtValue::Int(value.viewport_width)),
            ("viewport_height".to_string(), CtValue::Int(value.viewport_height)),
            (
                "estimated_item_size".to_string(),
                CtValue::Int(value.estimated_item_size),
            ),
            ("overscan".to_string(), CtValue::Int(value.overscan)),
            ("start".to_string(), CtValue::Int(value.start)),
            ("end".to_string(), CtValue::Int(value.end)),
            ("total_size".to_string(), CtValue::Int(value.total_size)),
            ("measured_count".to_string(), CtValue::Int(value.measured_count)),
            ("anchor_index".to_string(), CtValue::Int(value.anchor_index)),
            ("anchor_offset".to_string(), CtValue::Int(value.anchor_offset)),
            (
                "measurements".to_string(),
                CtValue::List(
                    value
                        .measurements
                        .into_iter()
                        .map(|(index, size)| CtValue::Struct {
                            type_name: "WebVirtualMeasurement".to_string(),
                            fields: vec![
                                ("index".to_string(), CtValue::Int(index)),
                                ("size".to_string(), CtValue::Int(size)),
                            ],
                        })
                        .collect(),
                ),
            ),
        ],
    }
}

fn web_virtual_plan_value(
    value: &CtValue,
    span: Span,
) -> Result<web_kernel::JetWebVirtualPlan, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("WebVirtualPlan", span));
    };
    if type_name != "WebVirtualPlan" {
        return Err(unsupported("WebVirtualPlan", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
            .ok_or_else(|| unsupported("WebVirtualPlan field", span))
    };
    let CtValue::List(values) = field("measurements")? else {
        return Err(unsupported("WebVirtualPlan measurements", span));
    };
    let mut measurements = Vec::with_capacity(values.len());
    for value in values {
        let CtValue::Struct { type_name, fields } = value else {
            return Err(unsupported("WebVirtualMeasurement", span));
        };
        if type_name != "WebVirtualMeasurement" && type_name != "tuple" {
            return Err(unsupported("WebVirtualMeasurement", span));
        }
        let field_value = |name: &str| {
            fields
                .iter()
                .find(|(field, _)| field == name)
                .map(|(_, value)| value)
        };
        let index = field_value("index")
            .or_else(|| fields.first().map(|(_, value)| value))
            .ok_or_else(|| unsupported("WebVirtualMeasurement index", span))?;
        let size = field_value("size")
            .or_else(|| fields.get(1).map(|(_, value)| value))
            .ok_or_else(|| unsupported("WebVirtualMeasurement size", span))?;
        measurements.push((
            web_int(index, "measurement index", span)?,
            web_int(size, "measurement size", span)?,
        ));
    }
    Ok(web_kernel::JetWebVirtualPlan {
        total_count: web_int(field("total_count")?, "total_count", span)?,
        scroll_offset: web_int(field("scroll_offset")?, "scroll_offset", span)?,
        viewport_width: web_int(field("viewport_width")?, "viewport_width", span)?,
        viewport_height: web_int(field("viewport_height")?, "viewport_height", span)?,
        estimated_item_size: web_int(
            field("estimated_item_size")?,
            "estimated_item_size",
            span,
        )?,
        overscan: web_int(field("overscan")?, "overscan", span)?,
        start: web_int(field("start")?, "start", span)?,
        end: web_int(field("end")?, "end", span)?,
        total_size: web_int(field("total_size")?, "total_size", span)?,
        measured_count: web_int(field("measured_count")?, "measured_count", span)?,
        anchor_index: web_int(field("anchor_index")?, "anchor_index", span)?,
        anchor_offset: web_int(field("anchor_offset")?, "anchor_offset", span)?,
        measurements,
    })
}

fn web_table_direction(value: web_kernel::JetWebTableSortDirection) -> CtValue {
    CtValue::Enum {
        type_name: "WebTableSortDirection".to_string(),
        variant: if matches!(
            value,
            web_kernel::JetWebTableSortDirection::Descending
        ) {
            "Descending".to_string()
        } else {
            "Ascending".to_string()
        },
        args: Vec::new(),
    }
}

fn web_table_page_mode(value: web_kernel::JetWebTablePageMode) -> CtValue {
    CtValue::Enum {
        type_name: "WebTablePageMode".to_string(),
        variant: if matches!(value, web_kernel::JetWebTablePageMode::Server) {
            "Server".to_string()
        } else {
            "Client".to_string()
        },
        args: Vec::new(),
    }
}

fn web_table_state(value: web_kernel::JetWebTableState) -> CtValue {
    let sort = value
        .sort
        .map(|sort| {
            CtValue::Present(Box::new(CtValue::Struct {
                type_name: "WebTableSort".to_string(),
                fields: vec![
                    ("column".to_string(), CtValue::Str(sort.column)),
                    ("direction".to_string(), web_table_direction(sort.direction)),
                ],
            }))
        })
        .unwrap_or_else(|| CtValue::absent(Type::Named("WebTableSort".to_string())));
    let filter = value
        .filter
        .map(|filter| {
            CtValue::Present(Box::new(CtValue::Struct {
                type_name: "WebTableFilter".to_string(),
                fields: vec![
                    ("column".to_string(), CtValue::Str(filter.column)),
                    ("value".to_string(), CtValue::Str(filter.value)),
                ],
            }))
        })
        .unwrap_or_else(|| CtValue::absent(Type::Named("WebTableFilter".to_string())));
    let selection_anchor = value
        .selection_anchor
        .map(CtValue::Str)
        .map(|value| CtValue::Present(Box::new(value)))
        .unwrap_or_else(|| CtValue::absent(Type::String));
    CtValue::Struct {
        type_name: "WebTableState".to_string(),
        fields: vec![
            ("sort".to_string(), sort),
            ("filter".to_string(), filter),
            ("page_index".to_string(), CtValue::Int(value.page_index)),
            ("page_size".to_string(), CtValue::Int(value.page_size)),
            (
                "selected_keys".to_string(),
                CtValue::List(value.selected_keys.into_iter().map(CtValue::Str).collect()),
            ),
            ("selection_anchor".to_string(), selection_anchor),
            ("page_mode".to_string(), web_table_page_mode(value.page_mode)),
        ],
    }
}
fn web_table_page(value: web_kernel::JetWebTablePage<CtValue>) -> CtValue {
    CtValue::Struct {
        type_name: "WebTablePage".to_string(),
        fields: vec![
            ("rows".to_string(), CtValue::List(value.rows)),
            (
                "row_keys".to_string(),
                CtValue::List(value.row_keys.into_iter().map(CtValue::Str).collect()),
            ),
            ("total_rows".to_string(), CtValue::Int(value.total_rows)),
            ("page_index".to_string(), CtValue::Int(value.page_index)),
            ("page_size".to_string(), CtValue::Int(value.page_size)),
            ("page_count".to_string(), CtValue::Int(value.page_count)),
        ],
    }
}



fn web_table_direction_value(
    value: &CtValue,
    span: Span,
) -> Result<web_kernel::JetWebTableSortDirection, Diagnostic> {
    match value {
        CtValue::Enum { variant, .. } if variant == "Descending" => {
            Ok(web_kernel::JetWebTableSortDirection::Descending)
        }
        CtValue::Enum { variant, .. } if variant == "Ascending" => {
            Ok(web_kernel::JetWebTableSortDirection::Ascending)
        }
        _ => Err(unsupported("WebTableSortDirection", span)),
    }
}

fn web_table_page_value(
    value: &CtValue,
    span: Span,
) -> Result<web_kernel::JetWebTablePage<CtValue>, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("WebTablePage", span));
    };
    if type_name != "WebTablePage" {
        return Err(unsupported("WebTablePage", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
            .ok_or_else(|| unsupported("WebTablePage field", span))
    };
    let CtValue::List(rows) = field("rows")? else {
        return Err(unsupported("WebTablePage rows", span));
    };
    let row_keys = web_strings(field("row_keys")?, "WebTablePage row_keys", span)?;
    Ok(web_kernel::JetWebTablePage {
        rows: rows.clone(),
        row_keys,
        total_rows: web_int(field("total_rows")?, "total_rows", span)?,
        page_index: web_int(field("page_index")?, "page_index", span)?,
        page_size: web_int(field("page_size")?, "page_size", span)?,
        page_count: web_int(field("page_count")?, "page_count", span)?,
    })
}

fn web_table_rows(value: Vec<web_kernel::JetWebTableRow<CtValue>>) -> CtValue {
    CtValue::List(
        value
            .into_iter()
            .map(|row| CtValue::Struct {
                type_name: "WebTableRow".to_string(),
                fields: vec![
                    ("key".to_string(), CtValue::Str(row.key)),
                    ("value".to_string(), row.value),
                ],
            })
            .collect(),
    )
}
fn web_store_transaction(value: web_kernel::JetWebStoreTransaction<CtValue>) -> CtValue {
    CtValue::Struct {
        type_name: "WebStoreTransaction".to_string(),
        fields: vec![
            ("generation".to_string(), CtValue::Int(value.generation as i64)),
            ("action".to_string(), CtValue::Str(value.action)),
            (
                "changed_fields".to_string(),
                CtValue::List(value.changed_fields.into_iter().map(CtValue::Str).collect()),
            ),
            ("before".to_string(), value.before),
            ("after".to_string(), value.after),
        ],
    }
}

fn web_store_event(value: web_kernel::JetWebStoreEvent) -> CtValue {
    CtValue::Struct {
        type_name: "WebStoreEvent".to_string(),
        fields: vec![
            ("sequence".to_string(), CtValue::Int(value.sequence as i64)),
            ("generation".to_string(), CtValue::Int(value.generation as i64)),
            ("action".to_string(), CtValue::Str(value.action)),
            ("changed_fields".to_string(), CtValue::List(
                value.changed_fields.into_iter().map(CtValue::Str).collect(),
            )),
            ("cursor".to_string(), CtValue::Int(value.cursor as i64)),
            ("kind".to_string(), CtValue::Enum {
                type_name: "WebStoreEventKind".to_string(),
                variant: match value.kind {
                    web_kernel::JetWebStoreEventKind::Transaction => "Transaction",
                    web_kernel::JetWebStoreEventKind::Navigation => "Navigation",
                    web_kernel::JetWebStoreEventKind::Rollback => "Rollback",
                }.to_string(),
                args: Vec::new(),
            }),
        ],
    }
}

fn web_store_inspection(value: web_kernel::JetWebStoreInspection<CtValue>) -> CtValue {
    CtValue::Struct {
        type_name: "WebStoreInspection".to_string(),
        fields: vec![
            ("value".to_string(), value.value),
            ("generation".to_string(), CtValue::Int(value.generation as i64)),
            ("cursor".to_string(), CtValue::Int(value.cursor as i64)),
            ("history".to_string(), CtValue::List(
                value.history.into_iter().map(web_store_transaction).collect(),
            )),
            ("events".to_string(), CtValue::List(
                value.events.into_iter().map(web_store_event).collect(),
            )),
            ("history_limit".to_string(), CtValue::Int(value.history_limit as i64)),
            ("history_enabled".to_string(), CtValue::Bool(value.history_enabled)),
        ],
    }
}

fn web_optional_value(
    value: Option<CtValue>,
    result_type: Option<&Type>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    match value {
        Some(value) => Ok(CtValue::Present(Box::new(value))),
        None => match result_type {
            Some(Type::Option(element)) => Ok(CtValue::absent((**element).clone())),
            _ => Err(unsupported("Web optional result requires its checked element type", span)),
        },
    }
}
/// Install the folded release-policy projection for the current interpreter
/// invocation. Kernel callbacks consult this shared context instead of the
/// compiler's build profile. Dropping the guard restores the enclosing
/// invocation's policy.
pub struct WebRuntimePolicyGuard {
    previous: (bool, bool),
}

impl Drop for WebRuntimePolicyGuard {
    fn drop(&mut self) {
        WEB_RUNTIME_POLICY.with(|policy| policy.set(self.previous));
    }
}

pub fn set_web_runtime_policy(
    devtools_enabled: bool,
    history_enabled: bool,
) -> WebRuntimePolicyGuard {
    let previous = WEB_RUNTIME_POLICY.with(|policy| {
        policy.replace((devtools_enabled, history_enabled))
    });
    WebRuntimePolicyGuard { previous }
}

pub fn apply_suite_with_policy(
    module: &str,
    method: &str,
    args: &[CtValue],
    span: Span,
    devtools_enabled: bool,
    history_enabled: bool,
) -> Result<CtValue, Diagnostic> {
    let _policy_guard = set_web_runtime_policy(devtools_enabled, history_enabled);
    apply_suite(module, method, args, span, None)
}


fn web_result(
    value: Result<CtValue, String>,
) -> Result<CtValue, Diagnostic> {
    Ok(match value {
        Ok(value) => CtValue::Present(Box::new(value)),
        Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
    })
}


fn callback_result(value: CtValue, span: Span) -> Result<CtValue, CtValue> {
    match value {
        CtValue::Present(value) => Ok(*value),
        CtValue::Failed(jet_foundation::AST::CtReport::Told(value)) => Err(*value),
        _ => std::panic::panic_any(unsupported("checked web callback outcome", span)),
    }
}

/// The Web kernels currently store callback failures as text. Keep the
/// canonical default `Err` carrier intact until this one documented boundary.
fn callback_result_for_kernel(value: CtValue, span: Span) -> Result<CtValue, String> {
    callback_result(value, span).map_err(|error| {
        let error = error
            .to_jet_err()
            .unwrap_or_else(|| std::panic::panic_any(unsupported("web callback default Err report", span)));
        jet_foundation::Outcome::jet_err_message(&error)
    })
}

fn callback_result_string(value: CtValue, span: Span) -> Result<String, String> {
    callback_result_for_kernel(value, span).map(|value| match value {
        CtValue::Str(value) => value,
        _ => std::panic::panic_any(unsupported("web callback String payload", span)),
    })
}

fn callback_result_unit(value: CtValue, span: Span) -> Result<(), String> {
    callback_result_for_kernel(value, span).map(|value| match value {
        CtValue::Unit => (),
        _ => std::panic::panic_any(unsupported("web callback Unit payload", span)),
    })
}

/// Interpreter/comptime adapter for the six headless first-party web modules.
/// The kernel files above are the only owners of their state transitions;
/// these branches only marshal CtValue handles and records.
pub fn apply_suite(
    module: &str,
    method: &str,
    args: &[CtValue],
    span: Span,
    result_type: Option<&Type>,
) -> Result<CtValue, Diagnostic> {
    let one = |index: usize| {
        args.get(index)
            .ok_or_else(|| unsupported(&format!("{module}.{method} argument {index}"), span))
    };
    match (module, method) {
        ("core.reactive", "effect") => {
            let callback = one(0)?.clone();
            Ok(opaque_web(web_kernel::jet_reactive_effect(move || {
                super::Methods::invoke_standalone_closure(&callback, Vec::new(), span)
                    .unwrap_or_else(|error| std::panic::panic_any(error));
            })))
        }
        ("core.ui", "reactive_render") => {
            let callback = one(0)?.clone();
            web_kernel::jet_reactive_effect_rooted(move || {
                super::Methods::invoke_standalone_closure(&callback, Vec::new(), span)
                    .unwrap_or_else(|error| std::panic::panic_any(error));
            });
            Ok(CtValue::Unit)
        }
        ("core.reactive", "signal") => Ok(opaque_web(
            web_kernel::JetSignal::new(one(0)?.clone()),
        )),
        ("core.reactive", "derived" | "computed") => {
            let callback = one(0)?.clone();
            Ok(opaque_web(web_kernel::JetDerived::new(move || {
                super::Methods::invoke_standalone_closure(&callback, Vec::new(), span)
                    .unwrap_or_else(|error| std::panic::panic_any(error))
            })))
        }
        // D-WEBAPP1=D: the same Prelude page constructor AOT and the JIT call;
        // the record shape is what `callback_page` reads back at the route edge.
        ("core.web", "page") => {
            let title = web_string(one(0)?, "page title", span)?;
            let body = web_string(one(1)?, "page body", span)?;
            let page = web_kernel::jet_web_page(title, body);
            Ok(CtValue::Struct {
                type_name: "WebPage".to_string(),
                fields: vec![
                    ("title".to_string(), CtValue::Str(page.title)),
                    ("body".to_string(), CtValue::Str(page.body)),
                ],
            })
        }
        ("core.web.router", "new") => {
            Ok(opaque_web(WebRouterHandle(web_kernel::jet_web_router_new())))
        }
        ("core.web.router", "route") => {
            let router = web_router(one(0)?, span)?;
            let pattern = web_string(one(1)?, "router pattern", span)?;
            let params = web_router_fields(one(2)?, span)?;
            let search = web_router_fields(one(3)?, span)?;
            let callback = one(4)?.clone();
            web_result(
                web_kernel::jet_web_router_route(
                    router,
                    pattern,
                    params,
                    search,
                    move |navigation| {
                        let value = super::Methods::invoke_standalone_closure(
                            &callback,
                            vec![web_navigation(navigation.clone())],
                            span,
                        )
                        .unwrap_or_else(|error| std::panic::panic_any(error));
                        callback_result_string(value, span)
                    },
                )
                .map(|router| opaque_web(WebRouterHandle(router))),
            )
        }
        ("core.web.router", "route_with_search_codec") => {
            let router = web_router(one(0)?, span)?;
            let pattern = web_string(one(1)?, "router pattern", span)?;
            let params = web_router_fields(one(2)?, span)?;
            let search = web_router_fields(one(3)?, span)?;
            let codec = web_router_codec(one(4)?, span)?;
            let callback = one(5)?.clone();
            web_result(
                web_kernel::jet_web_router_route_with_search_codec(
                    router,
                    pattern,
                    params,
                    search,
                    codec,
                    move |navigation| {
                        let value = super::Methods::invoke_standalone_closure(
                            &callback,
                            vec![web_navigation(navigation.clone())],
                            span,
                        )
                        .unwrap_or_else(|error| std::panic::panic_any(error));
                        callback_result_string(value, span)
                    },
                )
                .map(|router| opaque_web(WebRouterHandle(router))),
            )
        }
        ("core.web.router", "not_found") => {
            let router = web_router(one(0)?, span)?;
            let callback = one(1)?.clone();
            web_result(
                web_kernel::jet_web_router_not_found(router, move |path| {
                    super::Methods::invoke_standalone_closure(
                        &callback,
                        vec![CtValue::Str(path.to_string())],
                        span,
                    )
                    .map_err(|_| "router not_found callback failed".to_string())
                    .and_then(|value| callback_result_string(value, span))
                    .unwrap_or_else(|error| {
                        format!("router not_found callback failed: {error}")
                    })
                })
                .map(|router| opaque_web(WebRouterHandle(router))),
            )
        }
        ("core.web.router", "navigate" | "preload") => {
            let router = web_router(one(0)?, span)?;
            let url = web_string(one(1)?, "router URL", span)?;
            let result = if method == "navigate" {
                web_kernel::jet_web_router_navigate(router, url)
            } else {
                web_kernel::jet_web_router_preload(router, url)
            };
            web_result(result.map(web_navigation))
        }
        ("core.web.router", "current") => {
            Ok(web_navigation_state(web_kernel::jet_web_router_current(
                web_router(one(0)?, span)?,
            )))
        }
        ("core.web.router", "show") => Ok(CtValue::Str(
            web_kernel::jet_web_router_show(web_router(one(0)?, span)?),
        )),
        ("core.web.router", "link") => {
            let router = web_router(one(0)?, span)?;
            let pattern = web_string(one(1)?, "router pattern", span)?;
            let params = web_string_map(one(2)?, "router params", span)?;
            let search = web_string_map(one(3)?, "router search", span)?;
            web_result(web_kernel::jet_web_router_link(
                router, pattern, params, search,
            ).map(CtValue::Str))
        }
        ("core.web.query", "new") => {
            let key = web_string(one(0)?, "query key", span)?;
            let live = ct_to_live(one(1)?, span)?;
            Ok(opaque_web(WebQueryHandle(web_kernel::app_web_query_new(
                key, &live,
            ))))
        }
        ("core.web.query", "mutate") => {
            let query = web_query(one(0)?, span)?;
            let payload = web_string(one(1)?, "query mutation payload", span)?;
            let optimistic = web_optional_string_value(one(2)?, span)?;
            let callback = one(3)?.clone();
            Ok(web_mutation_state(web_kernel::jet_web_query_mutate(
                query,
                payload,
                optimistic,
                move |payload| {
                    let value = super::Methods::invoke_standalone_closure(
                        &callback,
                        vec![CtValue::Str(payload)],
                        span,
                    )
                    .unwrap_or_else(|error| std::panic::panic_any(error));
                    callback_result_string(value, span)
                },
            )))
        }
        ("core.web.query", "mutate_with_invalidations") => {
            let query = web_query(one(0)?, span)?;
            let payload = web_string(one(1)?, "query mutation payload", span)?;
            let optimistic = web_optional_string_value(one(2)?, span)?;
            let invalidations = web_strings(one(3)?, "query invalidations", span)?;
            let callback = one(4)?.clone();
            Ok(web_mutation_state(
                web_kernel::jet_web_query_mutate_with_invalidations(
                    query,
                    payload,
                    optimistic,
                    invalidations,
                    move |payload| {
                        let value = super::Methods::invoke_standalone_closure(
                            &callback,
                            vec![CtValue::Str(payload)],
                            span,
                        )
                        .unwrap_or_else(|error| std::panic::panic_any(error));
                        callback_result_string(value, span)
                    },
                ),
            ))
        }
        ("core.web.query", "retry") => {
            let query = web_query(one(0)?, span)?;
            let callback = one(1)?.clone();
            web_result(
                web_kernel::jet_web_query_retry(query, move |payload| {
                    let value = super::Methods::invoke_standalone_closure(
                        &callback,
                        vec![CtValue::Str(payload)],
                        span,
                    )
                    .unwrap_or_else(|error| std::panic::panic_any(error));
                    callback_result_unit(value, span)
                })
                .map(CtValue::Int),
            )
        }
        ("core.web.query", "facts") => Ok(CtValue::Str(web_kernel::jet_web_query_facts(
            web_query(one(0)?, span)?,
        ))),
        ("core.web.query", "live") => {
            let key = web_string(one(0)?, "query key", span)?;
            let footprint = web_string(one(1)?, "query footprint", span)?;
            let initial = web_string(one(2)?, "query initial", span)?;
            let fetch = one(3)?.clone();
            Ok(opaque_web(WebQueryHandle(web_kernel::jet_web_query_live(
                key, footprint, initial, move || {
                    let value = super::Methods::invoke_standalone_closure(
                        &fetch, vec![], span,
                    ).unwrap_or_else(|error| std::panic::panic_any(error));
                    callback_result_string(value, span)
                },
            ))))
        }
        ("core.web.query", "subscribe") => Ok(opaque_web(WebQueryHandle(
            web_kernel::jet_web_query_subscribe(web_string(one(0)?, "query source", span)?),
        ))),
        ("core.web.query", "invalidate") => Ok(CtValue::Int(
            web_kernel::jet_web_query_invalidate(web_query(one(0)?, span)?),
        )),
        ("core.web.query", "cancel") => Ok(CtValue::Bool(
            web_kernel::jet_web_query_cancel(web_query(one(0)?, span)?),
        )),
        ("core.web.query", "get") => Ok(CtValue::Str(web_kernel::jet_web_query_get(
            web_query(one(0)?, span)?,
        ))),
        ("core.web.query", "show") => Ok(CtValue::Str(web_kernel::jet_web_query_show(
            web_query(one(0)?, span)?,
        ))),
        ("core.web.query", "state") => Ok(web_query_state(web_kernel::jet_web_query_state(
            web_query(one(0)?, span)?,
        ))),
        ("core.web.query", "mutation_state") => Ok(web_mutation_state(
            web_kernel::jet_web_query_mutation_state(web_query(one(0)?, span)?),
        )),
        ("core.web.query", "state_signal") => Ok(opaque_web(
            web_kernel::jet_web_query_state_signal(web_query(one(0)?, span)?),
        )),
        ("core.web.query", "mutation_signal") => Ok(opaque_web(
            web_kernel::jet_web_query_mutation_signal(web_query(one(0)?, span)?),
        )),
        ("core.web.query", "queue") => web_result(
            web_kernel::jet_web_query_queue(
                web_query(one(0)?, span)?,
                web_string(one(1)?, "query payload", span)?,
            )
            .map(|value| CtValue::Int(value)),
        ),
        ("core.web.query", "refresh") => web_result(
            web_kernel::jet_web_query_refresh(web_query(one(0)?, span)?)
                .map(|()| CtValue::Unit),
        ),
        ("core.web.query", "set_online") => {
            let query = web_query(one(0)?, span)?;
            let online = web_bool(one(1)?, "query online", span)?;
            web_kernel::jet_web_query_set_online(query, online);
            Ok(CtValue::Unit)
        }
        ("core.web.query", "set_mode") => {
            let query = web_query(one(0)?, span)?;
            let mode = web_query_mode(one(1)?, span)?;
            web_kernel::jet_web_query_set_mode(query, mode);
            Ok(CtValue::Unit)
        },
        ("core.web.forms", "new") => Ok(opaque_web(WebFormHandle(
            web_kernel::jet_web_forms_new(web_string(one(0)?, "form action", span)?),
        ))),
        ("core.web.forms", "field") => web_result(
            web_kernel::jet_web_forms_field(
                web_form(one(0)?, span)?,
                web_string(one(1)?, "form field name", span)?,
                web_string(one(2)?, "form field type", span)?,
                web_bool(one(3)?, "form required", span)?,
            )
            .map(|form| opaque_web(WebFormHandle(form))),
        ),
        ("core.web.forms", "set") => web_result(
            web_kernel::jet_web_forms_set(
                web_form(one(0)?, span)?,
                web_string(one(1)?, "form field name", span)?,
                web_string(one(2)?, "form value", span)?,
            )
            .map(|()| CtValue::Unit),
        ),
        ("core.web.forms", "blur") => web_result(
            web_kernel::jet_web_forms_blur(
                web_form(one(0)?, span)?,
                web_string(one(1)?, "form field name", span)?,
            )
            .map(|()| CtValue::Unit),
        ),
        ("core.web.forms", "validate") => web_result(
            web_kernel::jet_web_forms_validate(web_form(one(0)?, span)?)
                .map(|()| CtValue::Unit),
        ),
        ("core.web.forms", "validate_async") => Ok(opaque_web(WebFormHandle(
            web_kernel::jet_web_forms_validate_async(web_form(one(0)?, span)?),
        ))),
        ("core.web.forms", "submit" | "no_script") => {
            let result = if method == "submit" {
                web_kernel::jet_web_forms_submit(web_form(one(0)?, span)?)
            } else {
                web_kernel::jet_web_forms_no_script(web_form(one(0)?, span)?)
            };
            web_result(result.map(CtValue::Str))
        }
        ("core.web.forms", "html") => Ok(CtValue::Str(web_kernel::jet_web_forms_html(
            web_form(one(0)?, span)?,
        ))),
        ("core.web.forms", "show") => Ok(CtValue::Str(web_kernel::jet_web_forms_show(
            web_form(one(0)?, span)?,
        ))),
        ("core.web.forms", "input") => {
            let CtValue::List(fields) = one(1)? else {
                return Err(unsupported("form input fields", span));
            };
            let mut specs = Vec::with_capacity(fields.len());
            for field in fields {
                match web_form_field_spec(field, span)? {
                    Ok(spec) => specs.push(spec),
                    Err(error) => return web_result(Err(error)),
                }
            }
            web_result(web_kernel::jet_web_forms_input(
                web_string(one(0)?, "form input type name", span)?, specs,
            ).map(opaque_web))
        }
        ("core.web.forms", "input_rename" | "input_exclude" | "input_group" | "input_replace") => {
            let input = web_opaque::<web_kernel::JetWebFormInput>(one(0)?, span)?.clone();
            let name = web_string(one(1)?, "form input field", span)?;
            let input = match method {
                "input_rename" => web_kernel::jet_web_forms_input_rename(
                    input, name, web_string(one(2)?, "form wire name", span)?,
                ),
                "input_exclude" => web_kernel::jet_web_forms_input_exclude(input, name),
                "input_group" => web_kernel::jet_web_forms_input_group(
                    input, name, web_string(one(2)?, "form group", span)?,
                ),
                _ => match web_form_field_spec(one(2)?, span)? {
                    Ok(field) => web_kernel::jet_web_forms_input_replace(input, name, field),
                    Err(error) => Err(error),
                },
            };
            web_result(input.map(opaque_web))
        }
        ("core.web.forms", "typed") => web_result(web_kernel::jet_web_forms_typed(
            web_opaque::<web_kernel::JetWebFormInput>(one(0)?, span)?,
            web_string(one(1)?, "form action", span)?,
        ).map(opaque_web)),
        ("core.web.forms", "typed_set" | "typed_blur" | "typed_validate" | "typed_focus") => {
            let form = web_opaque::<web_kernel::JetWebFormTyped>(one(0)?, span)?;
            let result = match method {
                "typed_set" => web_kernel::jet_web_forms_typed_set(
                    form, web_string(one(1)?, "form field", span)?,
                    web_string(one(2)?, "form field value", span)?,
                ),
                "typed_blur" => web_kernel::jet_web_forms_typed_blur(
                    form, web_string(one(1)?, "form field", span)?,
                ),
                "typed_focus" => web_kernel::jet_web_forms_typed_focus(
                    form, web_string(one(1)?, "form field", span)?,
                ),
                _ => web_kernel::jet_web_forms_typed_validate(form),
            };
            web_result(result.map(|()| CtValue::Unit))
        }
        ("core.web.forms", "typed_validate_field") => {
            let timing = match web_enum_variant(one(2)?, "WebFormValidationTiming", span)? {
                "Blur" => web_kernel::JetWebFormValidationTiming::Blur,
                "Submit" => web_kernel::JetWebFormValidationTiming::Submit,
                "Change" => web_kernel::JetWebFormValidationTiming::Change,
                _ => return Err(unsupported("WebFormValidationTiming", span)),
            };
            let callback = one(3)?.clone();
            Ok(opaque_web(web_kernel::jet_web_forms_typed_validate_field(
                web_opaque::<web_kernel::JetWebFormTyped>(one(0)?, span)?,
                web_string(one(1)?, "form validated field", span)?,
                timing,
                move |value| callback_result_unit(
                    super::Methods::invoke_standalone_closure(
                        &callback,
                        vec![CtValue::Str(value)],
                        span,
                    )
                    .unwrap_or_else(|error| std::panic::panic_any(error)),
                    span,
                ),
            )))
        }
        ("core.web.forms", "typed_submit" | "typed_no_script" | "typed_post") => {
            let form = web_opaque::<web_kernel::JetWebFormTyped>(one(0)?, span)?;
            let result = match method {
                "typed_submit" => web_kernel::jet_web_forms_typed_submit(form),
                "typed_no_script" => web_kernel::jet_web_forms_typed_no_script(form),
                _ => web_kernel::jet_web_forms_typed_post(
                    form, web_string(one(1)?, "form POST body", span)?,
                ),
            };
            web_result(result.map(CtValue::Str))
        }
        ("core.web.forms", "typed_decode_post") => web_result(
            web_kernel::jet_web_forms_typed_decode_post(
                web_opaque::<web_kernel::JetWebFormTyped>(one(0)?, span)?,
                web_string(one(1)?, "form POST body", span)?,
            ).map(web_form_decoded),
        ),
        ("core.web.forms", "typed_html" | "typed_show") => {
            let form = web_opaque::<web_kernel::JetWebFormTyped>(one(0)?, span)?;
            Ok(CtValue::Str(if method == "typed_html" {
                web_kernel::jet_web_forms_typed_html(form)
            } else {
                web_kernel::jet_web_forms_typed_show(form)
            }))
        }
        ("core.web.forms", "typed_validation_render") => Ok(CtValue::Str(
            web_kernel::jet_web_forms_typed_validation_render(
                web_opaque::<web_kernel::JetWebFormValidationChain>(one(0)?, span)?,
            ),
        )),
        ("core.web.forms", "typed_state") => Ok(web_form_state(
            web_kernel::jet_web_forms_typed_state(
                web_opaque::<web_kernel::JetWebFormTyped>(one(0)?, span)?,
            ),
        )),
        ("core.web.forms", "typed_lifecycle") => {
            let value = web_kernel::jet_web_forms_typed_lifecycle(
                web_opaque::<web_kernel::JetWebFormTyped>(one(0)?, span)?,
            );
            Ok(CtValue::Struct {
                type_name: "WebFormLifecycle".to_string(),
                fields: vec![
                    ("status".to_string(), web_enum_value("WebFormLifecycleStatus", value.status)),
                    ("result".to_string(), CtValue::Str(value.result)),
                    ("error".to_string(), CtValue::Str(value.error)),
                    ("generation".to_string(), CtValue::Int(value.generation as i64)),
                ],
            })
        }
        ("core.web.forms", "typed_errors") => {
            let value = web_kernel::jet_web_forms_typed_errors(
                web_opaque::<web_kernel::JetWebFormTyped>(one(0)?, span)?,
            );
            Ok(CtValue::Struct {
                type_name: "WebFormErrorState".to_string(),
                fields: vec![
                    ("fields".to_string(), web_error_map_value(value.fields)),
                    ("form".to_string(), CtValue::List(
                        value.form.into_iter().map(CtValue::Str).collect(),
                    )),
                ],
            })
        }
        ("core.web.forms", "typed_cancel") => {
            web_kernel::jet_web_forms_typed_cancel(
                web_opaque::<web_kernel::JetWebFormTyped>(one(0)?, span)?,
            );
            Ok(CtValue::Unit)
        }
        ("core.web.forms", "typed_select_field") => web_result(
            web_kernel::jet_web_forms_typed_select_field(
                web_opaque::<web_kernel::JetWebFormTyped>(one(0)?, span)?,
                web_string(one(1)?, "form selected field", span)?,
            ).map(opaque_web),
        ),
        ("core.web.forms", "typed_validate_async") => Ok(opaque_web(
            WebFormValidationHandle::new(Some(web_kernel::jet_web_forms_typed_validate_async(
                web_opaque::<web_kernel::JetWebFormTyped>(one(0)?, span)?,
            ))),
        )),
        ("core.web.forms", "typed_submit_async") => Ok(opaque_web(
            WebFormSubmissionHandle::new(Some(web_kernel::jet_web_forms_typed_submit_async(
                web_opaque::<web_kernel::JetWebFormTyped>(one(0)?, span)?,
            ))),
        )),
        ("core.web.forms", "typed_validation_wait" | "typed_validation_cancel") => {
            let handle = web_opaque::<WebFormValidationHandle>(one(0)?, span)?;
            let mut slot = handle.lock().unwrap_or_else(|error| error.into_inner());
            if method == "typed_validation_cancel" {
                web_kernel::jet_web_forms_typed_validation_cancel(
                    slot.as_ref().ok_or_else(|| unsupported("consumed form validation", span))?,
                );
                Ok(CtValue::Unit)
            } else {
                let validation = slot.take()
                    .ok_or_else(|| unsupported("consumed form validation", span))?;
                drop(slot);
                web_result(web_kernel::jet_web_forms_typed_validation_wait(validation)
                    .map(|()| CtValue::Unit))
            }
        }
        ("core.web.forms", "typed_submission_wait" | "typed_submission_cancel") => {
            let handle = web_opaque::<WebFormSubmissionHandle>(one(0)?, span)?;
            let mut slot = handle.lock().unwrap_or_else(|error| error.into_inner());
            if method == "typed_submission_cancel" {
                web_kernel::jet_web_forms_typed_submission_cancel(
                    slot.as_ref().ok_or_else(|| unsupported("consumed form submission", span))?,
                );
                Ok(CtValue::Unit)
            } else {
                let submission = slot.take()
                    .ok_or_else(|| unsupported("consumed form submission", span))?;
                drop(slot);
                web_result(web_kernel::jet_web_forms_typed_submission_wait(submission)
                    .map(CtValue::Str))
            }
        }
        ("core.web.forms", "typed_set_async_validator") => {
            let timing = match web_enum_variant(one(2)?, "WebFormValidationTiming", span)? {
                "Blur" => web_kernel::JetWebFormValidationTiming::Blur,
                "Submit" => web_kernel::JetWebFormValidationTiming::Submit,
                "Change" => web_kernel::JetWebFormValidationTiming::Change,
                _ => return Err(unsupported("WebFormValidationTiming", span)),
            };
            let debounce = web_int(one(3)?, "form validation debounce", span)? as u64;
            let callback = one(4)?.clone();
            web_result(web_kernel::jet_web_forms_typed_set_async_validator(
                web_opaque::<web_kernel::JetWebFormTyped>(one(0)?, span)?,
                web_string(one(1)?, "form validated field", span)?, timing, debounce,
                move |value| callback_result_unit(
                    super::Methods::invoke_standalone_closure(
                        &callback, vec![CtValue::Str(value)], span,
                    ).unwrap_or_else(|error| std::panic::panic_any(error)),
                    span,
                ),
            ).map(|()| CtValue::Unit))
        }
        ("core.web.forms", "action_error") => Ok(web_form_action_error(
            web_kernel::jet_web_forms_action_error(),
        )),
        ("core.web.forms", "action_field_error" | "action_form_error") => {
            let error = web_form_action_error_read(one(0)?, span)?;
            Ok(web_form_action_error(if method == "action_field_error" {
                web_kernel::jet_web_forms_action_field_error(
                    error, web_string(one(1)?, "form error field", span)?,
                    web_string(one(2)?, "form field error", span)?,
                )
            } else {
                web_kernel::jet_web_forms_action_form_error(
                    error, web_string(one(1)?, "form error", span)?,
                )
            }))
        }
        ("core.web.forms", "typed_set_action") => {
            let callback = one(1)?.clone();
            web_kernel::jet_web_forms_typed_set_action(
                web_opaque::<web_kernel::JetWebFormTyped>(one(0)?, span)?,
                move |input| {
                    let result = super::Methods::invoke_standalone_closure(
                        &callback, vec![web_form_decoded(input)], span,
                    ).unwrap_or_else(|error| std::panic::panic_any(error));
                    match result {
                        CtValue::Present(value) => Ok(web_string(
                            &value, "form action result", span,
                        ).unwrap_or_else(|error| std::panic::panic_any(error))),
                        value => Err(web_form_action_error_read(
                            value.told_report().unwrap_or_else(|| {
                                std::panic::panic_any(unsupported("form action result", span))
                            }),
                            span,
                        ).unwrap_or_else(|error| std::panic::panic_any(error))),
                    }
                },
            );
            Ok(CtValue::Unit)
        }
        ("core.web.table", "new" | "new_keyed") => {
            let name = web_string(one(0)?, "table name", span)?;
            let CtValue::List(rows) = one(1)? else {
                return Err(unsupported("table rows", span));
            };
            let callback = if method == "new_keyed" {
                Some(one(2)?.clone())
            } else {
                args.get(2).cloned()
            };
            if let Some(callback) = callback {
                let table = web_kernel::jet_web_table_with_key(
                    name,
                    rows.clone(),
                    move |row: &CtValue| {
                        super::Methods::invoke_standalone_closure(
                            &callback,
                            vec![row.clone()],
                            span,
                        )
                        .map_err(|_| "table key callback failed".to_string())
                        .and_then(|value| callback_result_string(value, span))
                        .unwrap_or_else(|error| format!("table key callback failed: {error}"))
                    },
                );
                Ok(opaque_web(WebTableHandle(table)))
            } else {
                Ok(opaque_web(WebTableHandle(web_kernel::jet_web_table(
                    name,
                    rows.clone(),
                ))))
            }
        }
        ("core.web.table", "with_column") => {
            let table = web_table(one(0)?, span)?.clone();
            let column = web_table_column(one(1)?, span)?.clone();
            Ok(opaque_web(WebTableHandle(table.with_column(column))))
        }
        ("core.web.table", "with_server_page") => {
            let table = web_table(one(0)?, span)?.clone();
            let callback = one(1)?.clone();
            let table = table.with_server_page(move |state| {
                let state = web_table_state(state);
                let value = super::Methods::invoke_standalone_closure(
                    &callback,
                    vec![state],
                    span,
                )
                .unwrap_or_else(|error| std::panic::panic_any(error));
                callback_result_for_kernel(value, span).map(|page| {
                    web_table_page_value(&page, span)
                        .unwrap_or_else(|error| std::panic::panic_any(error))
                })
            });
            Ok(opaque_web(WebTableHandle(table)))
        }
        ("core.web.table", "state") => Ok(web_table_state(
            web_table(one(0)?, span)?.state(),
        )),
        ("core.web.table", "facts") => Ok(CtValue::Str(
            web_table(one(0)?, span)?.facts_json(),
        )),
        ("core.web.table", "keys") => web_result(
            web_table(one(0)?, span)?
                .row_keys()
                .map(|keys| CtValue::List(keys.into_iter().map(CtValue::Str).collect())),
        ),
        ("core.web.table", "page_state") => web_result(
            web_table(one(0)?, span)?
                .page()
                .map(web_table_page),
        ),
        ("core.web.table", "sort_by") => {
            let table = web_table(one(0)?, span)?;
            let column = web_string(one(1)?, "table sort column", span)?;
            let direction = web_table_direction_value(one(2)?, span)?;
            Ok(opaque_web(WebTableHandle(table.sort_by(column, direction))))
        }
        ("core.web.table", "filter_by") => {
            let table = web_table(one(0)?, span)?;
            let column = web_string(one(1)?, "table filter column", span)?;
            let value = web_string(one(2)?, "table filter value", span)?;
            Ok(opaque_web(WebTableHandle(table.filter_by(column, value))))
        }
        ("core.web.table", "paginate") => {
            let table = web_table(one(0)?, span)?;
            Ok(opaque_web(WebTableHandle(table.paginate(
                web_int(one(1)?, "table page index", span)?,
                web_int(one(2)?, "table page size", span)?,
            ))))
        }
        ("core.web.table", "set_rows") => {
            let table = web_table(one(0)?, span)?;
            let CtValue::List(rows) = one(1)? else {
                return Err(unsupported("table rows", span));
            };
            web_result(table.replace_rows(rows.clone()).map(|()| CtValue::Unit))
        }
        ("core.web.table", "set_selected") => {
            let table = web_table(one(0)?, span)?;
            let key = web_string(one(1)?, "table selection key", span)?;
            let selected = web_bool(one(2)?, "table selection state", span)?;
            web_result(
                table
                    .set_selected(key, selected)
                    .map(|table| opaque_web(WebTableHandle(table))),
            )
        }
        ("core.web.table", "toggle_selection") => {
            let table = web_table(one(0)?, span)?;
            let key = web_string(one(1)?, "table selection key", span)?;
            web_result(table.toggle_selection(key).map(|table| opaque_web(WebTableHandle(table))))
        }
        ("core.web.table", "clear_selection") => {
            let table = web_table(one(0)?, span)?;
            Ok(opaque_web(WebTableHandle(table.clear_selection())))
        }
        ("core.web.table", "selected_keys") => Ok(CtValue::List(
            web_table(one(0)?, span)?
                .selected_keys()
                .into_iter()
                .map(CtValue::Str)
                .collect(),
        )),
        ("core.web.table", "selected_rows") => web_result(
            web_table(one(0)?, span)?
                .selected_rows()
                .map(web_table_rows),
        ),
        ("core.web.table", "insert_row") => {
            let table = web_table(one(0)?, span)?;
            web_result(
                table
                    .insert_row(one(1)?.clone())
                    .map(CtValue::Str),
            )
        }
        ("core.web.table", "replace_row") => {
            let table = web_table(one(0)?, span)?;
            let key = web_string(one(1)?, "table row key", span)?;
            web_result(
                table
                    .replace_row(&key, one(2)?.clone())
                    .map(|()| CtValue::Unit),
            )
        }
        ("core.web.table", "update_row") => {
            let table = web_table(one(0)?, span)?;
            let key = web_string(one(1)?, "table row key", span)?;
            let callback = one(2)?.clone();
            let mut callback_failed = false;
            let result = table.update_row(&key, |row| {
                match super::Methods::invoke_standalone_closure(
                    &callback,
                    vec![row.clone()],
                    span,
                ) {
                    Ok(value) => *row = value,
                    Err(_) => callback_failed = true,
                }
            });
            if callback_failed {
                web_result(Err("table row update callback failed".to_string()))
            } else {
                web_result(result.map(|()| CtValue::Unit))
            }
        }
        ("core.web.table", "remove_row") => {
            let table = web_table(one(0)?, span)?;
            let key = web_string(one(1)?, "table row key", span)?;
            web_result(table.remove_row(&key))
        }
        ("core.web.table", "first_page" | "next_page") => {
            let table = web_table(one(0)?, span)?;
            let table = if method == "first_page" {
                table.first_page()
            } else {
                table.next_page()
            };
            Ok(opaque_web(WebTableHandle(table)))
        }
        ("core.web.table", "last_page") => {
            web_result(web_table(one(0)?, span)?.last_page()
                .map(|table| opaque_web(WebTableHandle(table))))
        }
        ("core.web.table", "visible_rows") => {
            let table = web_table(one(0)?, span)?;
            let plan = web_virtual_plan_value(one(1)?, span)?;
            web_result(table.visible_rows(&plan).map(web_table_rows))
        }
        ("core.web.table", "page") => {
            let CtValue::List(rows) = one(0)? else {
                return Err(unsupported("table rows", span));
            };
            Ok(web_table_page(web_kernel::jet_web_table_page(
                rows,
                web_int(one(1)?, "table page index", span)?,
                web_int(one(2)?, "table page size", span)?,
            )))
        }
        ("core.web.table", "column") => {
            let name = web_string(one(0)?, "table column name", span)?;
            let cell_type = web_string(one(1)?, "table column type", span)?;
            let callback = one(2)?.clone();
            let column =
                web_kernel::jet_web_table_column(name, cell_type, move |row: &CtValue| {
                    super::Methods::invoke_standalone_closure(
                        &callback,
                        vec![row.clone()],
                        span,
                    )
                    .map_err(|_| "table column callback failed".to_string())
                    .and_then(|value| callback_result_string(value, span))
                    .unwrap_or_else(|error| {
                        format!("table column callback failed: {error}")
                    })
                });
            Ok(opaque_web(WebTableColumnHandle(column)))
        }
        ("core.web.table", "sort") => {
            let CtValue::List(rows) = one(0)? else {
                return Err(unsupported("table rows", span));
            };
            let descending = web_bool(one(1)?, "table descending", span)?;
            let callback = one(2)?.clone();
            let mut keys = Vec::with_capacity(rows.len());
            for row in rows {
                let value = super::Methods::invoke_standalone_closure(
                    &callback,
                    vec![row.clone()],
                    span,
                )?;
                keys.push((row.clone(), web_string(&value, "table sort key", span)?));
            }
            let sorted = web_kernel::jet_web_table_sort(rows, descending, move |row| {
                keys.iter()
                    .find(|(candidate, _)| candidate == row)
                    .map(|(_, key)| key.clone())
                    .unwrap_or_default()
            });
            Ok(CtValue::List(sorted))
        }
        ("core.web.table", "filter") => {
            let CtValue::List(rows) = one(0)? else {
                return Err(unsupported("table rows", span));
            };
            let callback = one(1)?.clone();
            let mut keep = Vec::with_capacity(rows.len());
            for row in rows {
                let value = super::Methods::invoke_standalone_closure(
                    &callback,
                    vec![row.clone()],
                    span,
                )?;
                keep.push((row.clone(), web_bool(&value, "table predicate", span)?));
            }
            let filtered = web_kernel::jet_web_table_filter(rows, move |row| {
                keep.iter()
                    .find(|(candidate, _)| candidate == row)
                    .is_some_and(|(_, keep)| *keep)
            });
            Ok(CtValue::List(filtered))
        }
        ("core.web.virtual", "window") => Ok(web_virtual_window(
            web_kernel::jet_web_virtual_window(
                web_int(one(0)?, "virtual total count", span)?,
                web_int(one(1)?, "virtual scroll offset", span)?,
                web_int(one(2)?, "virtual viewport size", span)?,
                web_int(one(3)?, "virtual item size", span)?,
                web_int(one(4)?, "virtual overscan", span)?,
            ),
        )),
        ("core.web.virtual", "window_measured") => {
            let measurements = web_ints(one(5)?, "virtual measurements", span)?;
            Ok(web_virtual_window(web_kernel::jet_web_virtual_window_measured(
                web_int(one(0)?, "virtual total count", span)?,
                web_int(one(1)?, "virtual scroll offset", span)?,
                web_int(one(2)?, "virtual viewport size", span)?,
                web_int(one(3)?, "virtual item size", span)?,
                web_int(one(4)?, "virtual overscan", span)?,
                &measurements,
            )))
        }
        ("core.web.virtual", "slice") => {
            let CtValue::List(rows) = one(0)? else {
                return Err(unsupported("virtual rows", span));
            };
            let window = web_virtual_window_value(one(1)?, span)?;
            Ok(CtValue::List(web_kernel::jet_web_virtual_slice(rows, &window)))
        }
        ("core.web.virtual", "indices") => {
            let window = web_virtual_window_value(one(0)?, span)?;
            Ok(CtValue::List(
                web_kernel::jet_web_virtual_indices(&window)
                    .into_iter()
                    .map(CtValue::Int)
                    .collect(),
            ))
        }
        ("core.web.virtual", "plan") => Ok(web_virtual_plan(
            web_kernel::jet_web_virtual_plan(
                web_int(one(0)?, "virtual total count", span)?,
                web_int(one(1)?, "virtual scroll offset", span)?,
                web_int(one(2)?, "virtual viewport width", span)?,
                web_int(one(3)?, "virtual viewport height", span)?,
                web_int(one(4)?, "virtual item size", span)?,
                web_int(one(5)?, "virtual overscan", span)?,
            ),
        )),
        ("core.web.virtual", "plan_measured") => {
            let measurements = web_int_pairs(one(6)?, "virtual measurements", span)?;
            Ok(web_virtual_plan(web_kernel::jet_web_virtual_plan_measured(
                web_int(one(0)?, "virtual total count", span)?,
                web_int(one(1)?, "virtual scroll offset", span)?,
                web_int(one(2)?, "virtual viewport width", span)?,
                web_int(one(3)?, "virtual viewport height", span)?,
                web_int(one(4)?, "virtual item size", span)?,
                web_int(one(5)?, "virtual overscan", span)?,
                &measurements,
            )))
        }
        ("core.web.virtual", "plan_from_sizes") => {
            let measurements = web_ints(one(6)?, "virtual measurements", span)?;
            Ok(web_virtual_plan(web_kernel::jet_web_virtual_plan_from_sizes(
                web_int(one(0)?, "virtual total count", span)?,
                web_int(one(1)?, "virtual scroll offset", span)?,
                web_int(one(2)?, "virtual viewport width", span)?,
                web_int(one(3)?, "virtual viewport height", span)?,
                web_int(one(4)?, "virtual item size", span)?,
                web_int(one(5)?, "virtual overscan", span)?,
                &measurements,
            )))
        }
        ("core.web.virtual", "plan_measure") => {
            let plan = web_virtual_plan_value(one(0)?, span)?;
            Ok(web_virtual_plan(plan.with_measurement(
                web_int(one(1)?, "measurement index", span)?,
                web_int(one(2)?, "measurement size", span)?,
            )))
        }
        ("core.web.virtual", "plan_slice") => {
            let CtValue::List(rows) = one(0)? else {
                return Err(unsupported("virtual rows", span));
            };
            let plan = web_virtual_plan_value(one(1)?, span)?;
            Ok(CtValue::List(web_kernel::jet_web_virtual_plan_slice(
                rows, &plan,
            )))
        }
        ("core.web.virtual", "plan_indices") => {
            let plan = web_virtual_plan_value(one(0)?, span)?;
            Ok(CtValue::List(
                web_kernel::jet_web_virtual_plan_indices(&plan)
                    .into_iter()
                    .map(CtValue::Int)
                    .collect(),
            ))
        }
        ("core.web.virtual", "plan_facts") => {
            let plan = web_virtual_plan_value(one(0)?, span)?;
            Ok(CtValue::Str(web_kernel::jet_web_virtual_plan_facts(&plan)))
        }
        ("core.web.virtual", "plan_viewport") => Ok(opaque_web(
            WebVirtualPlanViewportHandle(web_kernel::jet_web_virtual_plan_viewport(
                web_virtual_plan_value(one(0)?, span)?,
            )),
        )),
        ("core.web.virtual", "plan_viewport_state") => Ok(web_virtual_plan(
            web_virtual_plan_viewport(one(0)?, span)?.plan(),
        )),
        ("core.web.virtual", "plan_scroll_to") => {
            let viewport = web_virtual_plan_viewport(one(0)?, span)?;
            viewport.scroll_to(web_int(one(1)?, "virtual scroll offset", span)?);
            Ok(CtValue::Unit)
        }
        ("core.web.virtual", "plan_resize") => {
            let viewport = web_virtual_plan_viewport(one(0)?, span)?;
            viewport.resize(
                web_int(one(1)?, "virtual viewport width", span)?,
                web_int(one(2)?, "virtual viewport height", span)?,
            );
            Ok(CtValue::Unit)
        }
        ("core.web.virtual", "plan_viewport_measure") => {
            let viewport = web_virtual_plan_viewport(one(0)?, span)?;
            viewport.measure(
                web_int(one(1)?, "measurement index", span)?,
                web_int(one(2)?, "measurement size", span)?,
            );
            Ok(CtValue::Unit)
        }
        ("core.web.store", "new") => Ok(opaque_web(WebStoreHandle(web_kernel::jet_web_store(
            web_string(one(0)?, "store name", span)?,
            one(1)?.clone(),
        )))),
        ("core.web.store", "with_history") => Ok(opaque_web(WebStoreHandle(
            web_kernel::jet_web_store_with_history(
                web_string(one(0)?, "store name", span)?,
                one(1)?.clone(),
                web_int(one(2)?, "store history limit", span)?,
            ),
        ))),
        ("core.web.store", "signal" | "state_signal") => {
            let store = web_store(one(0)?, span)?;
            Ok(opaque_web(if method == "signal" {
                web_kernel::jet_web_store_signal(store)
            } else {
                web_kernel::jet_web_store_state_signal(store)
            }))
        }
        ("core.web.store", "derived" | "selector") => {
            let store = web_store(one(0)?, span)?;
            let callback = one(1)?.clone();
            let compute = move |value| super::Methods::invoke_standalone_closure(
                &callback, vec![value], span,
            ).unwrap_or_else(|error| std::panic::panic_any(error));
            Ok(opaque_web(if method == "derived" {
                web_kernel::jet_web_store_derived(store, compute)
            } else {
                web_kernel::jet_web_store_selector(store, compute)
            }))
        }
        ("core.web.store", "value") => Ok(web_kernel::jet_web_store_value(
            web_store(one(0)?, span)?,
        )),
        ("core.web.store", "set" | "set_state") => {
            let store = web_store(one(0)?, span)?;
            let value = one(1)?.clone();
            Ok(web_store_transaction(if method == "set" {
                web_kernel::jet_web_store_set(store, value)
            } else {
                web_kernel::jet_web_store_set_state(store, value)
            }))
        }
        ("core.web.store", "transaction" | "update" | "batch" | "optimistic" | "patch") => {
            let store = web_store(one(0)?, span)?;
            let action = web_string(one(1)?, "store action", span)?;
            let fields = web_strings(one(2)?, "store changed fields", span)?;
            let callback = one(3)?.clone();
            let update = move |state: &mut CtValue| {
                super::Methods::invoke_standalone_closure_mut(&callback, state, span)
                    .unwrap_or_else(|error| std::panic::panic_any(error));
            };
            match method {
                "optimistic" | "patch" => {
                    let patch = if method == "optimistic" {
                        web_kernel::jet_web_store_optimistic(store, action, fields, update)
                    } else {
                        web_kernel::jet_web_store_patch(store, action, fields, update)
                    };
                    Ok(opaque_web(WebStorePatchHandle(std::sync::Mutex::new(Some(patch)))))
                }
                "transaction" => Ok(web_store_transaction(web_kernel::jet_web_store_transaction(
                    store, action, fields, update,
                ))),
                "batch" => Ok(web_store_transaction(web_kernel::jet_web_store_batch(
                    store, action, fields, update,
                ))),
                _ => Ok(web_store_transaction(web_kernel::jet_web_store_update(
                    store, action, fields, update,
                ))),
            }
        }
        ("core.web.store", "back" | "forward" | "jump" | "scrub" | "restore") => {
            let store = web_store(one(0)?, span)?;
            let value = match method {
                "back" => web_kernel::jet_web_store_back(store),
                "forward" => web_kernel::jet_web_store_forward(store),
                "jump" => web_kernel::jet_web_store_jump(
                    store, web_int(one(1)?, "store generation", span)?,
                ),
                "scrub" => web_kernel::jet_web_store_scrub(
                    store, web_int(one(1)?, "store generation", span)?,
                ),
                _ => web_kernel::jet_web_store_restore(
                    store, web_int(one(1)?, "store generation", span)?,
                ),
            };
            web_optional_value(value, result_type, span)
        }
        ("core.web.store", "history") => Ok(CtValue::List(
            web_kernel::jet_web_store_history(web_store(one(0)?, span)?)
                .into_iter().map(web_store_transaction).collect(),
        )),
        ("core.web.store", "history_at") => web_optional_value(
            web_kernel::jet_web_store_history_at(
                web_store(one(0)?, span)?, web_int(one(1)?, "store history index", span)?,
            ).map(web_store_transaction),
            result_type,
            span,
        ),
        ("core.web.store", "events" | "events_since") => {
            let store = web_store(one(0)?, span)?;
            let events = if method == "events" {
                web_kernel::jet_web_store_events(store)
            } else {
                web_kernel::jet_web_store_events_since(
                    store, web_int(one(1)?, "store event sequence", span)?,
                )
            };
            Ok(CtValue::List(events.into_iter().map(web_store_event).collect()))
        }
        ("core.web.store", "clear_history") => {
            web_kernel::jet_web_store_clear_history(web_store(one(0)?, span)?);
            Ok(CtValue::Unit)
        }
        ("core.web.store", "set_history_limit") => {
            web_kernel::jet_web_store_set_history_limit(
                web_store(one(0)?, span)?, web_int(one(1)?, "store history limit", span)?,
            );
            Ok(CtValue::Unit)
        }
        ("core.web.store", "history_enabled") => Ok(CtValue::Bool(
            web_kernel::jet_web_store_history_enabled(web_store(one(0)?, span)?),
        )),
        ("core.web.store", "history_limit" | "cursor" | "current_generation") => {
            let store = web_store(one(0)?, span)?;
            Ok(CtValue::Int(match method {
                "history_limit" => web_kernel::jet_web_store_history_limit(store),
                "cursor" => web_kernel::jet_web_store_cursor(store),
                _ => web_kernel::jet_web_store_current_generation(store),
            }))
        }
        ("core.web.store", "facts_json" | "event_json") => {
            let store = web_store(one(0)?, span)?;
            Ok(CtValue::Str(if method == "facts_json" {
                web_kernel::jet_web_store_facts_json(store)
            } else {
                web_kernel::jet_web_store_event_json(store)
            }))
        }
        ("core.web.store", "inspect") => Ok(web_store_inspection(
            web_kernel::jet_web_store_inspect(web_store(one(0)?, span)?),
        )),
        ("core.web.store", "patch_generation" | "patch_transaction" | "patch_active"
            | "patch_commit" | "patch_rollback") => {
            let handle = web_opaque::<WebStorePatchHandle>(one(0)?, span)?;
            let mut slot = handle.0.lock().unwrap_or_else(|error| error.into_inner());
            let patch = slot.as_ref().ok_or_else(|| unsupported("consumed WebStorePatch", span))?;
            match method {
                "patch_generation" => Ok(CtValue::Int(web_kernel::jet_web_store_patch_generation(patch))),
                "patch_transaction" => Ok(web_store_transaction(
                    web_kernel::jet_web_store_patch_transaction(patch),
                )),
                "patch_active" => Ok(CtValue::Bool(web_kernel::jet_web_store_patch_active(patch))),
                "patch_commit" => {
                    let patch = slot.take().expect("checked live WebStorePatch");
                    drop(slot);
                    Ok(web_store_transaction(web_kernel::jet_web_store_patch_commit(patch)))
                }
                _ => {
                    let patch = slot.take().expect("checked live WebStorePatch");
                    drop(slot);
                    web_optional_value(
                        web_kernel::jet_web_store_patch_rollback(patch), result_type, span,
                    )
                }
            }
        }
        ("core.web.store", "subscribe") => {
            let callback = one(1)?.clone();
            Ok(opaque_web(WebStoreSubscriptionHandle(
                web_kernel::jet_web_store_subscribe(web_store(one(0)?, span)?, move |value| {
                    super::Methods::invoke_standalone_closure(&callback, vec![value], span)
                        .unwrap_or_else(|error| std::panic::panic_any(error));
                }),
            )))
        }
        ("core.web.store", "subscribe_selector") => {
            let selector = one(1)?.clone();
            let callback = one(2)?.clone();
            Ok(opaque_web(WebStoreSubscriptionHandle(
                web_kernel::jet_web_store_subscribe_selector(
                    web_store(one(0)?, span)?,
                    move |value| super::Methods::invoke_standalone_closure(
                        &selector, vec![value], span,
                    ).unwrap_or_else(|error| std::panic::panic_any(error)),
                    move |value| {
                        super::Methods::invoke_standalone_closure(&callback, vec![value], span)
                            .unwrap_or_else(|error| std::panic::panic_any(error));
                    },
                ),
            )))
        }
        ("core.web.store", "subscription_unsubscribe" | "subscription_active") => {
            let subscription = &web_opaque::<WebStoreSubscriptionHandle>(one(0)?, span)?.0;
            if method == "subscription_active" {
                Ok(CtValue::Bool(web_kernel::jet_web_store_subscription_active(subscription)))
            } else {
                web_kernel::jet_web_store_subscription_unsubscribe(subscription);
                Ok(CtValue::Unit)
            }
        }
        _ => Err(unsupported(&format!("`{module}.{method}()`"), span)),
    }
}

fn web_query_state_read(
    value: &CtValue,
    span: Span,
) -> Result<web_kernel::JetWebQueryState, Diagnostic> {
    let field = |name| web_record_field(value, name, span);
    let status = match web_enum_variant(field("status")?, "WebQueryStatus", span)? {
        "Pending" => web_kernel::JetWebQueryStatus::Pending,
        "Fresh" => web_kernel::JetWebQueryStatus::Fresh,
        "Stale" => web_kernel::JetWebQueryStatus::Stale,
        "Fetching" => web_kernel::JetWebQueryStatus::Fetching,
        "Error" => web_kernel::JetWebQueryStatus::Error,
        "Offline" => web_kernel::JetWebQueryStatus::Offline,
        _ => return Err(unsupported("WebQueryStatus", span)),
    };
    Ok(web_kernel::JetWebQueryState {
        status,
        value: web_string(field("value")?, "query value", span)?,
        error: web_string(field("error")?, "query error", span)?,
        generation: web_int(field("generation")?, "query generation", span)? as u64,
        queued: web_int(field("queued")?, "query queued count", span)? as usize,
        offline: web_bool(field("offline")?, "query offline state", span)?,
    })
}

fn web_mutation_state_read(
    value: &CtValue,
    span: Span,
) -> Result<web_kernel::JetWebMutationState, Diagnostic> {
    let field = |name| web_record_field(value, name, span);
    let status = match web_enum_variant(field("status")?, "WebMutationStatus", span)? {
        "Idle" => web_kernel::JetWebMutationStatus::Idle,
        "Pending" => web_kernel::JetWebMutationStatus::Pending,
        "Success" => web_kernel::JetWebMutationStatus::Success,
        "Error" => web_kernel::JetWebMutationStatus::Error,
        "Settled" => web_kernel::JetWebMutationStatus::Settled,
        _ => return Err(unsupported("WebMutationStatus", span)),
    };
    Ok(web_kernel::JetWebMutationState {
        status,
        generation: web_int(field("generation")?, "mutation generation", span)? as u64,
        optimistic: web_bool(field("optimistic")?, "optimistic mutation", span)?,
        value: web_string(field("value")?, "mutation value", span)?,
        result: web_string(field("result")?, "mutation result", span)?,
        error: web_string(field("error")?, "mutation error", span)?,
        context: web_string(field("context")?, "mutation context", span)?,
        rollback: web_bool(field("rollback")?, "mutation rollback", span)?,
        paused: web_bool(field("paused")?, "mutation paused state", span)?,
        queued: web_int(field("queued")?, "queued mutation count", span)? as usize,
        replayed: web_int(field("replayed")?, "replayed mutation count", span)? as u64,
    })
}

fn web_signal_method<T: Clone + Send + Sync + 'static>(
    signal: &web_kernel::JetSignal<T>,
    method: &str,
    args: &[CtValue],
    encode: fn(T) -> CtValue,
    decode: fn(&CtValue, Span) -> Result<T, Diagnostic>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    match method {
        "get" => Ok(encode(signal.get())),
        "set" => {
            let value = args.first().ok_or_else(|| unsupported("Signal.set value", span))?;
            signal.set(decode(value, span)?);
            Ok(CtValue::Unit)
        }
        _ => Err(unsupported(&format!("Signal.{method}"), span)),
    }
}

fn normalize_web_handle_method(method: &str) -> &str {
    match method {
        "Signal.get" | "Derived.get" => "get",
        "Signal.set" => "set",
        "Effect.active" => "active",
        "Effect.unsubscribe" => "unsubscribe",
        _ => method,
    }
}

/// Preserve live Prelude owners across the interpreter's private handle boundary.
/// This is method-name/value marshalling, not a second reactive graph.
pub fn apply_web_handle(
    receiver: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
    result_type: Option<&Type>,
) -> Option<Result<CtValue, Diagnostic>> {
    let CtValue::Closure(data) = receiver else {
        return None;
    };
    let opaque = data.opaque.as_ref()?;
    if let Some(file_result) =
        crate::Comptime::TextLite::apply_file_handle(receiver, method, args, span)
    {
        return Some(file_result);
    }
    let method = normalize_web_handle_method(method);
    if let Some(effect) = opaque.downcast_ref::<web_kernel::JetReactiveEffect>() {
        return Some(match method {
            "active" => Ok(CtValue::Bool(effect.active())),
            "unsubscribe" => {
                effect.unsubscribe();
                Ok(CtValue::Unit)
            }
            _ => Err(unsupported(&format!("Effect.{method}"), span)),
        });
    }
    if let Some(signal) = opaque.downcast_ref::<web_kernel::JetSignal<CtValue>>() {
        return Some(web_signal_method(
            signal, method, args, |value| value, |value, _| Ok(value.clone()), span,
        ));
    }
    if let Some(signal) = opaque.downcast_ref::<web_kernel::JetSignal<web_kernel::JetWebQueryState>>() {
        return Some(web_signal_method(
            signal, method, args, web_query_state, web_query_state_read, span,
        ));
    }
    if let Some(signal) = opaque.downcast_ref::<web_kernel::JetSignal<web_kernel::JetWebMutationState>>() {
        return Some(web_signal_method(
            signal, method, args, web_mutation_state, web_mutation_state_read, span,
        ));
    }
    if let Some(derived) = opaque.downcast_ref::<web_kernel::JetDerived<CtValue>>() {
        return Some(match method {
            "get" => Ok(derived.get()),
            _ => Err(unsupported(&format!("Derived.{method}"), span)),
        });
    }
    if let Some(derived) = opaque.downcast_ref::<web_kernel::JetDerived<web_kernel::JetWebFormFieldState>>() {
        return Some(match method {
            "get" => Ok(web_form_field_state(derived.get())),
            _ => Err(unsupported(&format!("Derived.{method}"), span)),
        });
    }
    let (module, prefix) = if opaque.downcast_ref::<WebRouterHandle>().is_some() {
        ("core.web.router", "")
    } else if opaque.downcast_ref::<WebQueryHandle>().is_some() {
        ("core.web.query", "")
    } else if opaque.downcast_ref::<WebFormHandle>().is_some() {
        ("core.web.forms", "")
    } else if opaque.downcast_ref::<web_kernel::JetWebFormInput>().is_some() {
        ("core.web.forms", "input_")
    } else if opaque.downcast_ref::<web_kernel::JetWebFormTyped>().is_some() {
        ("core.web.forms", "typed_")
    } else if opaque.downcast_ref::<web_kernel::JetWebFormValidationChain>().is_some() {
        ("core.web.forms", "typed_validation_")
    } else if opaque.downcast_ref::<WebFormValidationHandle>().is_some() {
        ("core.web.forms", "typed_validation_")
    } else if opaque.downcast_ref::<WebFormSubmissionHandle>().is_some() {
        ("core.web.forms", "typed_submission_")
    } else if opaque.downcast_ref::<WebTableHandle>().is_some() {
        ("core.web.table", "")
    } else if opaque.downcast_ref::<WebVirtualPlanViewportHandle>().is_some() {
        ("core.web.virtual", "plan_viewport_")
    } else if opaque.downcast_ref::<WebStoreHandle>().is_some() {
        ("core.web.store", "")
    } else if opaque.downcast_ref::<WebStorePatchHandle>().is_some() {
        ("core.web.store", "patch_")
    } else if opaque.downcast_ref::<WebStoreSubscriptionHandle>().is_some() {
        ("core.web.store", "subscription_")
    } else {
        return None;
    };
    let projected;
    let method = if prefix.is_empty() {
        method
    } else {
        projected = format!("{prefix}{method}");
        &projected
    };
    if args.is_empty() {
        return Some(apply_suite(module, method, std::slice::from_ref(receiver), span, result_type));
    }
    let mut values = Vec::with_capacity(args.len() + 1);
    values.push(receiver.clone());
    values.extend_from_slice(args);
    Some(apply_suite(module, method, &values, span, result_type))
}

fn web_ints(value: &CtValue, what: &str, span: Span) -> Result<Vec<i64>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(unsupported(what, span));
    };
    values.iter().map(|value| web_int(value, what, span)).collect()
}

#[allow(dead_code)]
fn _type_anchor() -> Type {
    Type::Named("LiveQuery".to_string())
}
