// D-DX-SERVERFN1=A: one checked server-function boundary serves both a typed
// client call and a progressive form POST. App owns registration; this module
// owns the transport adapter, request context, policy facts, and state.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const JET_WEB_SERVER_FN_MAX_NAME: usize = 128;
const JET_WEB_SERVER_FN_MAX_TYPE: usize = 256;
const JET_WEB_SERVER_FN_MAX_BODY: usize = 4 * 1024 * 1024;
const JET_WEB_SERVER_FN_MAX_FORM_FIELDS: usize = 256;
const JET_WEB_SERVER_FN_MAX_FORM_VALUE: usize = 64 * 1024;
const JET_WEB_SERVER_FN_MAX_MIDDLEWARE: usize = 64;

static JET_WEB_SERVER_FN_REQUESTS: AtomicU64 = AtomicU64::new(1);

fn jet_web_server_fn_clean(value: &str, limit: usize) -> String {
    let mut result = String::with_capacity(value.len().min(limit));
    for character in value.chars() {
        if result.len() >= limit {
            break;
        }
        if character.is_control() {
            result.push(' ');
        } else {
            result.push(character);
        }
    }
    result.trim().to_string()
}

fn jet_web_server_fn_json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len().saturating_add(2));
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", character as u32);
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

fn jet_web_server_fn_next_request_id() -> String {
    let number = JET_WEB_SERVER_FN_REQUESTS.fetch_add(1, Ordering::Relaxed);
    format!("jet-sfn-{number:016x}")
}

fn jet_web_server_fn_valid_token(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '/')
        })
}

fn jet_web_server_fn_is_mutating(method: &str) -> bool {
    matches!(method, "POST" | "PUT" | "PATCH" | "DELETE")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetWebServerFnCallStatus {
    Idle,
    Pending,
    Success,
    Error,
    Cancelled,
    TimedOut,
    Uncertain,
}

impl JetWebServerFnCallStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Pending => "pending",
            Self::Success => "success",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Uncertain => "uncertain_completion",
        }
    }
}

fn jet_web_server_fn_origin_matches_host(origin: &str, host: Option<&str>) -> bool {
    if origin == "same-origin" {
        return true;
    }
    let Some(host) = host else {
        return false;
    };
    let authority = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
        .unwrap_or(origin)
        .split('/')
        .next()
        .unwrap_or_default();
    !authority.is_empty() && authority.eq_ignore_ascii_case(host)
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetWebServerFnRetryPolicy {
    None,
    Safe { max_attempts: u32 },
    Idempotent { max_attempts: u32 },
}

impl JetWebServerFnRetryPolicy {
    fn bounded_attempts(self) -> u32 {
        match self {
            Self::None => 1,
            Self::Safe { max_attempts } | Self::Idempotent { max_attempts } => {
                max_attempts.max(1).min(8)
            }
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Safe { .. } => "safe",
            Self::Idempotent { .. } => "idempotent",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetWebServerFnCsrfPolicy {
    SameOrigin {
        expected_origin: Option<String>,
        token: Option<String>,
    },
    Disabled,
}

impl Default for JetWebServerFnCsrfPolicy {
    fn default() -> Self {
        Self::SameOrigin {
            expected_origin: None,
            token: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebServerFnSpec {
    pub name: String,
    pub endpoint: String,
    pub source_id: String,
    pub method: String,
    pub input_type: String,
    pub output_type: String,
    pub error_type: String,
    pub csrf: JetWebServerFnCsrfPolicy,
    pub capability: Option<String>,
    pub effects: Vec<String>,
    pub revalidate: Vec<String>,
    pub timeout_ms: Option<u64>,
    pub retry: JetWebServerFnRetryPolicy,
    pub idempotency_declared: bool,
}

impl JetWebServerFnSpec {
    pub fn mutating(&self) -> bool {
        jet_web_server_fn_is_mutating(&self.method)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebServerFnState {
    pub status: JetWebServerFnCallStatus,
    pub request_id: String,
    pub endpoint: String,
    pub source_id: String,
    pub method: String,
    pub attempts: u32,
    pub retry_policy: String,
    pub idempotency_key: Option<String>,
    pub result: String,
    pub error: String,
    pub middleware: Vec<String>,
    pub revalidate: Vec<String>,
}

impl JetWebServerFnState {
    fn idle(spec: &JetWebServerFnSpec) -> Self {
        Self {
            status: JetWebServerFnCallStatus::Idle,
            request_id: String::new(),
            endpoint: spec.endpoint.clone(),
            source_id: spec.source_id.clone(),
            method: spec.method.clone(),
            attempts: 0,
            retry_policy: spec.retry.name().to_string(),
            idempotency_key: None,
            result: String::new(),
            error: String::new(),
            middleware: Vec::new(),
            revalidate: spec.revalidate.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct JetWebServerFnRequestContext {
    pub request_id: String,
    pub source_id: String,
    pub endpoint: String,
    pub method: String,
    pub origin: Option<String>,
    pub host: Option<String>,
    pub auth_subject: Option<String>,
    pub capabilities: BTreeSet<String>,
    pub csrf_token: Option<String>,
    pub idempotency_key: Option<String>,
    pub cancellation: Arc<AtomicBool>,
    pub deadline: Option<Instant>,
}

impl JetWebServerFnRequestContext {
    pub fn checked(
        request_id: String,
        source_id: String,
        endpoint: String,
        method: String,
        origin: Option<String>,
        host: Option<String>,
        auth_subject: Option<String>,
        capabilities: BTreeSet<String>,
        csrf_token: Option<String>,
        idempotency_key: Option<String>,
        timeout_ms: Option<u64>,
    ) -> Result<Self, JetWebServerFnError> {
        if !jet_web_server_fn_valid_token(&request_id, 128)
            || !jet_web_server_fn_valid_token(&source_id, 256)
            || endpoint.is_empty()
            || method.is_empty()
            || idempotency_key
                .as_deref()
                .is_some_and(|key| !jet_web_server_fn_valid_token(key, 512))
        {
            return Err(JetWebServerFnError::InvalidContext);
        }
        Ok(Self {
            request_id,
            source_id,
            endpoint,
            method,
            origin,
            host,
            auth_subject,
            capabilities,
            csrf_token,
            idempotency_key,
            cancellation: Arc::new(AtomicBool::new(false)),
            deadline: timeout_ms
                .map(|milliseconds| Instant::now() + Duration::from_millis(milliseconds)),
        })
    }

    pub fn cancelled(&self) -> bool {
        self.cancellation.load(Ordering::Acquire)
    }

    pub fn cancel(&self) {
        self.cancellation.store(true, Ordering::Release);
    }

    fn check_live(&self) -> Result<(), JetWebServerFnError> {
        if self.cancelled() {
            return Err(JetWebServerFnError::Cancelled);
        }
        if self.deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(JetWebServerFnError::TimedOut);
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct JetWebServerFnRequest {
    pub method: String,
    pub path: String,
    pub content_type: String,
    pub body: String,
    pub headers: BTreeMap<String, String>,
    pub context: JetWebServerFnRequestContext,
}

impl JetWebServerFnRequest {
    pub fn json(
        method: String,
        path: String,
        body: String,
        context: JetWebServerFnRequestContext,
    ) -> Self {
        let mut headers = BTreeMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        Self {
            method,
            path,
            content_type: "application/json".to_string(),
            body,
            headers,
            context,
        }
    }

    pub fn progressive(
        method: String,
        path: String,
        body: String,
        context: JetWebServerFnRequestContext,
    ) -> Self {
        let mut headers = BTreeMap::new();
        headers.insert(
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        );
        Self {
            method,
            path,
            content_type: "application/x-www-form-urlencoded".to_string(),
            body,
            headers,
            context,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebServerFnResponse {
    pub status: u16,
    pub body: String,
    pub content_type: String,
    pub request_id: String,
    pub middleware: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebServerFnFormSubmission {
    pub method: String,
    pub action: String,
    pub content_type: String,
    pub body: String,
    pub pending: bool,
    pub revalidate: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetWebServerFnError {
    InvalidDefinition(String),
    InvalidContext,
    InvalidInput(String),
    InvalidOutput,
    MethodNotAllowed { expected: String, actual: String },
    EndpointNotFound,
    UnsupportedMediaType,
    Csrf(String),
    Unauthorized,
    Forbidden(String),
    Cancelled,
    TimedOut,
    RetryExhausted,
    UncertainCompletion { request_id: String },
    DuplicateRequest,
    Handler { code: String, message: String },
    Transport {
        code: String,
        message: String,
        dispatched: bool,
    },
}

impl JetWebServerFnError {
    fn code(&self) -> &'static str {
        match self {
            Self::InvalidDefinition(_) => "invalid_definition",
            Self::InvalidContext => "invalid_context",
            Self::InvalidInput(_) => "invalid_input",
            Self::InvalidOutput => "invalid_output",
            Self::MethodNotAllowed { .. } => "method_not_allowed",
            Self::EndpointNotFound => "endpoint_not_found",
            Self::UnsupportedMediaType => "unsupported_media_type",
            Self::Csrf(_) => "csrf_rejected",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::RetryExhausted => "retry_exhausted",
            Self::UncertainCompletion { .. } => "uncertain_completion",
            Self::DuplicateRequest => "duplicate_request",
            Self::Handler { .. } => "handler_error",
            Self::Transport { .. } => "transport_error",
        }
    }

    fn status(&self) -> u16 {
        match self {
            Self::InvalidDefinition(_) | Self::InvalidContext => 500,
            Self::InvalidInput(_) | Self::UnsupportedMediaType => 400,
            Self::InvalidOutput => 500,
            Self::MethodNotAllowed { .. } => 405,
            Self::EndpointNotFound => 404,
            Self::Csrf(_) => 403,
            Self::Unauthorized => 401,
            Self::Forbidden(_) => 403,
            Self::Cancelled => 499,
            Self::TimedOut => 504,
            Self::RetryExhausted => 503,
            Self::UncertainCompletion { .. } => 504,
            Self::DuplicateRequest => 409,
            Self::Handler { .. } => 422,
            Self::Transport { .. } => 502,
        }
    }

    fn public_message(&self) -> String {
        match self {
            Self::InvalidDefinition(message)
            | Self::InvalidInput(message)
            | Self::Csrf(message)
            | Self::Forbidden(message) => jet_web_server_fn_clean(message, 512),
            Self::MethodNotAllowed { .. } => "the server function does not accept this method".to_string(),
            Self::EndpointNotFound => "the server function endpoint was not found".to_string(),
            Self::UnsupportedMediaType => "the server function body format is not supported".to_string(),
            Self::InvalidOutput => "the server function returned an invalid wire value".to_string(),
            Self::InvalidContext => "the request context is invalid".to_string(),
            Self::Unauthorized => "authentication is required".to_string(),
            Self::Cancelled => "the server function call was cancelled".to_string(),
            Self::TimedOut => "the server function call timed out".to_string(),
            Self::RetryExhausted => "the server function retry policy was exhausted".to_string(),
            Self::UncertainCompletion { .. } => "the server may have completed the call".to_string(),
            Self::DuplicateRequest => "the idempotency key was already used".to_string(),
            Self::Handler { message, .. } => jet_web_server_fn_clean(message, 512),
            Self::Transport { message, .. } => jet_web_server_fn_clean(message, 512),
        }
    }

    fn dispatched(&self) -> bool {
        matches!(self, Self::Transport { dispatched: true, .. })
    }

    fn structured(&self, request_id: &str) -> String {
        let mut fields = vec![
            format!("\"schema\":\"jet.web.server-fn/v1\""),
            format!("\"code\":{}", jet_web_server_fn_json_string(self.code())),
            format!("\"message\":{}", jet_web_server_fn_json_string(&self.public_message())),
            format!("\"request_id\":{}", jet_web_server_fn_json_string(request_id)),
        ];
        if let Self::MethodNotAllowed { expected, actual } = self {
            fields.push(format!(
                "\"expected_method\":{}",
                jet_web_server_fn_json_string(expected)
            ));
            fields.push(format!(
                "\"actual_method\":{}",
                jet_web_server_fn_json_string(actual)
            ));
        }
        format!("{{{}}}", fields.join(","))
    }
}

impl fmt::Display for JetWebServerFnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

pub type JetWebServerFnHandler =
    Arc<dyn Fn(&JetWebServerFnRequestContext, &str) -> Result<String, JetWebServerFnError> + Send + Sync>;
pub type JetWebServerFnValidator = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;
pub type JetWebServerFnMiddleware =
    Arc<dyn Fn(&mut JetWebServerFnRequestContext) -> Result<(), JetWebServerFnError> + Send + Sync>;

#[derive(Clone)]
struct JetWebServerFnMiddlewareEntry {
    name: String,
    callback: JetWebServerFnMiddleware,
}

#[derive(Clone)]
pub struct JetWebServerFunction {
    spec: JetWebServerFnSpec,
    handler: JetWebServerFnHandler,
    validator: Option<JetWebServerFnValidator>,
    middleware: Vec<JetWebServerFnMiddlewareEntry>,
    state: Arc<Mutex<JetWebServerFnState>>,
    idempotency_results: Arc<Mutex<BTreeMap<String, String>>>,
}

impl JetWebServerFunction {
    pub fn new(
        name: String,
        source_id: String,
        handler: JetWebServerFnHandler,
    ) -> Result<Self, JetWebServerFnError> {
        Self::typed(
            name,
            source_id,
            "POST".to_string(),
            "Input".to_string(),
            "Output".to_string(),
            "Error".to_string(),
            handler,
        )
    }

    pub fn typed(
        name: String,
        source_id: String,
        method: String,
        input_type: String,
        output_type: String,
        error_type: String,
        handler: JetWebServerFnHandler,
    ) -> Result<Self, JetWebServerFnError> {
        let name = jet_web_server_fn_clean(&name, JET_WEB_SERVER_FN_MAX_NAME);
        let source_id = jet_web_server_fn_clean(&source_id, JET_WEB_SERVER_FN_MAX_TYPE);
        let method = method.trim().to_ascii_uppercase();
        let input_type = jet_web_server_fn_clean(&input_type, JET_WEB_SERVER_FN_MAX_TYPE);
        let output_type = jet_web_server_fn_clean(&output_type, JET_WEB_SERVER_FN_MAX_TYPE);
        let error_type = jet_web_server_fn_clean(&error_type, JET_WEB_SERVER_FN_MAX_TYPE);
        if name.is_empty()
            || name.len() > JET_WEB_SERVER_FN_MAX_NAME
            || !name
                .chars()
                .enumerate()
                .all(|(index, character)| {
                    character.is_ascii_alphanumeric() || character == '_' || (index > 0 && character == '-')
                })
            || source_id.is_empty()
            || input_type.is_empty()
            || output_type.is_empty()
            || error_type.is_empty()
            || !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE")
        {
            return Err(JetWebServerFnError::InvalidDefinition(
                "server function metadata is incomplete or invalid".to_string(),
            ));
        }
        let endpoint = format!("/actions/{name}");
        let spec = JetWebServerFnSpec {
            name,
            endpoint,
            source_id,
            method,
            input_type,
            output_type,
            error_type,
            csrf: JetWebServerFnCsrfPolicy::default(),
            capability: None,
            effects: Vec::new(),
            revalidate: Vec::new(),
            timeout_ms: None,
            retry: JetWebServerFnRetryPolicy::None,
            idempotency_declared: false,
        };
        let mut function = Self {
            state: Arc::new(Mutex::new(JetWebServerFnState::idle(&spec))),
            spec,
            handler,
            validator: None,
            middleware: Vec::new(),
            idempotency_results: Arc::new(Mutex::new(BTreeMap::new())),
        };
        for kind in jet_app_middleware::APP_SERVER_FUNCTION_MIDDLEWARE {
            function = function.with_middleware(kind.as_str().to_string(), |_context| Ok(()));
        }
        Ok(function)
    }

    pub fn spec(&self) -> JetWebServerFnSpec {
        self.spec.clone()
    }

    pub fn with_csrf(mut self, policy: JetWebServerFnCsrfPolicy) -> Self {
        self.spec.csrf = policy;
        self
    }

    pub fn with_capability(mut self, capability: String) -> Self {
        if !capability.is_empty() {
            self.spec.capability = Some(capability);
        }
        self
    }

    pub fn with_effects(mut self, effects: Vec<String>) -> Self {
        self.spec.effects = effects
            .into_iter()
            .map(|effect| jet_web_server_fn_clean(&effect, 64))
            .filter(|effect| !effect.is_empty())
            .collect();
        self
    }

    pub fn with_revalidate(mut self, keys: Vec<String>) -> Self {
        self.spec.revalidate = keys
            .into_iter()
            .map(|key| jet_web_server_fn_clean(&key, 256))
            .filter(|key| !key.is_empty())
            .collect();
        if let Ok(mut state) = self.state.lock() {
            state.revalidate = self.spec.revalidate.clone();
        }
        self
    }

    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.spec.timeout_ms = Some(timeout_ms.clamp(1, 86_400_000));
        self
    }

    pub fn with_retry(mut self, retry: JetWebServerFnRetryPolicy) -> Self {
        self.spec.retry = retry;
        if let Ok(mut state) = self.state.lock() {
            state.retry_policy = retry.name().to_string();
        }
        self
    }

    pub fn with_idempotency(mut self, declared: bool) -> Self {
        self.spec.idempotency_declared = declared;
        self
    }

    pub fn with_validator<F>(mut self, validator: F) -> Self
    where
        F: Fn(&str) -> Result<(), String> + Send + Sync + 'static,
    {
        self.validator = Some(Arc::new(validator));
        self
    }

    pub fn with_middleware<F>(mut self, name: String, middleware: F) -> Self
    where
        F: Fn(&mut JetWebServerFnRequestContext) -> Result<(), JetWebServerFnError>
            + Send
            + Sync
            + 'static,
    {
        if self.middleware.len() < JET_WEB_SERVER_FN_MAX_MIDDLEWARE {
            self.middleware.push(JetWebServerFnMiddlewareEntry {
                name: jet_web_server_fn_clean(&name, 128),
                callback: Arc::new(middleware),
            });
        }
        self
    }

    pub fn state(&self) -> JetWebServerFnState {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_else(|_| JetWebServerFnState::idle(&self.spec))
    }

    pub fn facts_json(&self) -> String {
        let state = self.state();
        let middleware = self
            .middleware
            .iter()
            .map(|entry| jet_web_server_fn_json_string(&entry.name))
            .collect::<Vec<_>>()
            .join(",");
        let revalidate = self
            .spec
            .revalidate
            .iter()
            .map(|key| jet_web_server_fn_json_string(key))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"name\":{},\"endpoint\":{},\"source_id\":{},\"method\":{},\"input\":{},\"output\":{},\"error\":{},\"mutating\":{},\"csrf\":{},\"capability\":{},\"effects\":{},\"retry\":{},\"idempotency\":{},\"state\":{},\"attempts\":{},\"middleware\":[{}],\"revalidate\":[{}]}}",
            jet_web_server_fn_json_string(&self.spec.name),
            jet_web_server_fn_json_string(&self.spec.endpoint),
            jet_web_server_fn_json_string(&self.spec.source_id),
            jet_web_server_fn_json_string(&self.spec.method),
            jet_web_server_fn_json_string(&self.spec.input_type),
            jet_web_server_fn_json_string(&self.spec.output_type),
            jet_web_server_fn_json_string(&self.spec.error_type),
            self.spec.mutating(),
            match &self.spec.csrf {
                JetWebServerFnCsrfPolicy::SameOrigin { .. } => "\"same-origin\"",
                JetWebServerFnCsrfPolicy::Disabled => "\"disabled\"",
            },
            self.spec
                .capability
                .as_deref()
                .map(jet_web_server_fn_json_string)
                .unwrap_or_else(|| "null".to_string()),
            format!(
                "[{}]",
                self.spec
                    .effects
                    .iter()
                    .map(|effect| jet_web_server_fn_json_string(effect))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            jet_web_server_fn_json_string(self.spec.retry.name()),
            self.spec.idempotency_declared,
            jet_web_server_fn_json_string(state.status.name()),
            state.attempts,
            middleware,
            revalidate,
        )
    }

    fn set_state(&self, update: impl FnOnce(&mut JetWebServerFnState)) {
        if let Ok(mut state) = self.state.lock() {
            update(&mut state);
        }
    }

    fn context_from_http(
        &self,
        request: &JetHTTPRequest,
        idempotency_key: Option<String>,
    ) -> Result<JetWebServerFnRequestContext, JetWebServerFnError> {
        let request_id = request
            .headers
            .first("x-request-id")
            .filter(|value| jet_web_server_fn_valid_token(value, 128))
            .map(str::to_string)
            .unwrap_or_else(jet_web_server_fn_next_request_id);
        let origin = request.headers.first("origin").map(str::to_string);
        let host = request.headers.first("host").map(str::to_string);
        let auth_subject = request
            .headers
            .first("authorization")
            .filter(|value| !value.trim().is_empty())
            .map(|_| "authorization".to_string())
            .or_else(|| {
                request
                    .headers
                    .first("cookie")
                    .filter(|value| !value.trim().is_empty())
                    .map(|_| "cookie".to_string())
            });
        let capabilities = request
            .headers
            .first("x-jet-capability")
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|capability| !capability.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let csrf_token = request.headers.first("x-csrf-token").map(str::to_string);
        JetWebServerFnRequestContext::checked(
            request_id,
            self.spec.source_id.clone(),
            self.spec.endpoint.clone(),
            request.method.clone(),
            origin,
            host,
            auth_subject,
            capabilities,
            csrf_token,
            idempotency_key,
            self.spec.timeout_ms,
        )
    }

    fn check_csrf(
        &self,
        context: &JetWebServerFnRequestContext,
    ) -> Result<(), JetWebServerFnError> {
        if !self.spec.mutating() {
            return Ok(());
        }
        match &self.spec.csrf {
            JetWebServerFnCsrfPolicy::Disabled => Ok(()),
            JetWebServerFnCsrfPolicy::SameOrigin {
                expected_origin,
                token,
            } => {
                let same_origin = match (expected_origin.as_deref(), context.origin.as_deref()) {
                    (Some(expected), Some(actual)) => expected == actual,
                    (Some(_), None) => false,
                    (None, Some(actual)) => {
                        jet_web_server_fn_origin_matches_host(actual, context.host.as_deref())
                    }
                    (None, None) => true,
                };
                if !same_origin {
                    return Err(JetWebServerFnError::Csrf(
                        "the request origin is not same-origin".to_string(),
                    ));
                }
                if let Some(expected) = token {
                    if context.csrf_token.as_deref() != Some(expected.as_str()) {
                        return Err(JetWebServerFnError::Csrf(
                            "the request CSRF token is invalid".to_string(),
                        ));
                    }
                }
                Ok(())
            }
        }
    }

    fn normalize_input(
        &self,
        content_type: &str,
        body: &str,
    ) -> Result<String, JetWebServerFnError> {
        if body.len() > JET_WEB_SERVER_FN_MAX_BODY {
            return Err(JetWebServerFnError::InvalidInput(
                "server function input exceeds the payload limit".to_string(),
            ));
        }
        if body.trim().is_empty() {
            if self.spec.input_type == "Unit" {
                // Native forms legitimately omit a body for a Unit input.
                // The declared boundary type, not an endpoint or test route,
                // decides whether the empty wire representation is valid.
                return Ok("null".to_string());
            }
            return Err(JetWebServerFnError::InvalidInput(
                "server function input is missing".to_string(),
            ));
        }
        if content_type.eq_ignore_ascii_case("application/json")
            || content_type
                .to_ascii_lowercase()
                .starts_with("application/json;")
        {
            if jet_std::parse_json_typed_datatree(&body.to_string()).is_err() {
                return Err(JetWebServerFnError::InvalidInput(
                    "server function input is not valid JSON".to_string(),
                ));
            }
            return Ok(body.to_string());
        }
        if content_type
            .to_ascii_lowercase()
            .starts_with("application/x-www-form-urlencoded")
        {
            return jet_web_server_fn_form_json(body);
        }
        Err(JetWebServerFnError::UnsupportedMediaType)
    }

    fn authorize(&self, context: &JetWebServerFnRequestContext) -> Result<(), JetWebServerFnError> {
        if self.spec.capability.is_some() && context.auth_subject.is_none() {
            return Err(JetWebServerFnError::Unauthorized);
        }
        if let Some(required) = &self.spec.capability {
            if !context.capabilities.contains(required) {
                return Err(JetWebServerFnError::Forbidden(
                    "the request lacks the required capability".to_string(),
                ));
            }
        }
        Ok(())
    }

    fn dispatch_checked(
        &self,
        mut request: JetWebServerFnRequest,
    ) -> Result<(String, Vec<String>), JetWebServerFnError> {
        if request.method != self.spec.method {
            return Err(JetWebServerFnError::MethodNotAllowed {
                expected: self.spec.method.clone(),
                actual: request.method,
            });
        }
        if request.path != self.spec.endpoint {
            return Err(JetWebServerFnError::EndpointNotFound);
        }
        if request.context.source_id != self.spec.source_id
            || request.context.endpoint != self.spec.endpoint
        {
            return Err(JetWebServerFnError::InvalidContext);
        }
        request.context.check_live()?;
        self.check_csrf(&request.context)?;
        self.authorize(&request.context)?;
        let body = self.normalize_input(&request.content_type, &request.body)?;
        if let Some(validator) = &self.validator {
            validator(&body).map_err(|message| JetWebServerFnError::InvalidInput(
                jet_web_server_fn_clean(message.as_str(), 512),
            ))?;
        }
        let mut middleware = Vec::with_capacity(self.middleware.len());
        for entry in &self.middleware {
            request.context.check_live()?;
            (entry.callback)(&mut request.context)?;
            middleware.push(entry.name.clone());
        }
        if self.spec.idempotency_declared {
            if let Some(key) = request.context.idempotency_key.as_deref() {
                let cached = self
                    .idempotency_results
                    .lock()
                    .ok()
                    .and_then(|results| results.get(key).cloned());
                if let Some(output) = cached {
                    return Ok((output, middleware));
                }
            }
        }
        request.context.check_live()?;
        let output = (self.handler)(&request.context, &body)?;
        if output.len() > JET_WEB_SERVER_FN_MAX_BODY
            || jet_std::parse_json_typed_datatree(&output).is_err()
        {
            return Err(JetWebServerFnError::InvalidOutput);
        }
        if self.spec.idempotency_declared {
            if let Some(key) = request.context.idempotency_key.as_deref() {
                if let Ok(mut results) = self.idempotency_results.lock() {
                    if results.len() >= 1024 {
                        if let Some(first) = results.keys().next().cloned() {
                            results.remove(&first);
                        }
                    }
                    results.insert(key.to_string(), output.clone());
                }
            }
        }
        Ok((output, middleware))
    }

    pub fn dispatch(&self, request: JetWebServerFnRequest) -> JetWebServerFnResponse {
        let request_id = request.context.request_id.clone();
        self.set_state(|state| {
            state.status = JetWebServerFnCallStatus::Pending;
            state.request_id = request_id.clone();
            state.attempts = state.attempts.saturating_add(1);
            state.error.clear();
            state.result.clear();
        });
        match self.dispatch_checked(request) {
            Ok((body, middleware)) => {
                self.set_state(|state| {
                    state.status = JetWebServerFnCallStatus::Success;
                    state.result = body.clone();
                    state.middleware = middleware.clone();
                });
                JetWebServerFnResponse {
                    status: 200,
                    body,
                    content_type: "application/json".to_string(),
                    request_id,
                    middleware,
                }
            }
            Err(error) => {
                let body = error.structured(&request_id);
                self.set_state(|state| {
                    state.status = match &error {
                        JetWebServerFnError::Cancelled => JetWebServerFnCallStatus::Cancelled,
                        JetWebServerFnError::TimedOut => JetWebServerFnCallStatus::TimedOut,
                        JetWebServerFnError::UncertainCompletion { .. } => {
                            JetWebServerFnCallStatus::Uncertain
                        }
                        _ => JetWebServerFnCallStatus::Error,
                    };
                    state.error = body.clone();
                });
                JetWebServerFnResponse {
                    status: error.status(),
                    body,
                    content_type: "application/json".to_string(),
                    request_id,
                    middleware: Vec::new(),
                }
            }
        }
    }

    pub fn progressive_form(&self, body: String) -> Result<JetWebServerFnResponse, JetWebServerFnError> {
        let request_id = jet_web_server_fn_next_request_id();
        let mut context = JetWebServerFnRequestContext::checked(
            request_id.clone(),
            self.spec.source_id.clone(),
            self.spec.endpoint.clone(),
            self.spec.method.clone(),
            None,
            None,
            None,
            BTreeSet::new(),
            None,
            None,
            self.spec.timeout_ms,
        )?;
        context.origin = Some("same-origin".to_string());
        let request = JetWebServerFnRequest::progressive(
            self.spec.method.clone(),
            self.spec.endpoint.clone(),
            body,
            context,
        );
        let response = self.dispatch(request);
        if response.status >= 400 {
            let message = response
                .body
                .as_str()
                .to_string();
            return Err(JetWebServerFnError::Handler {
                code: "progressive_submission".to_string(),
                message,
            });
        }
        Ok(response)
    }

    pub fn progressive_submission(&self, body: String) -> JetWebServerFnFormSubmission {
        JetWebServerFnFormSubmission {
            method: self.spec.method.clone(),
            action: self.spec.endpoint.clone(),
            content_type: "application/x-www-form-urlencoded".to_string(),
            body,
            pending: true,
            revalidate: self.spec.revalidate.clone(),
        }
    }

    fn synthetic_client_request(
        &self,
        body: String,
        idempotency_key: Option<String>,
    ) -> Result<JetWebServerFnRequest, JetWebServerFnError> {
        let request_id = jet_web_server_fn_next_request_id();
        let mut context = JetWebServerFnRequestContext::checked(
            request_id,
            self.spec.source_id.clone(),
            self.spec.endpoint.clone(),
            self.spec.method.clone(),
            self.spec
                .csrf
                .expected_origin()
                .map(str::to_string),
            None,
            None,
            BTreeSet::new(),
            self.spec.csrf.token().map(str::to_string),
            idempotency_key.clone(),
            self.spec.timeout_ms,
        )?;
        context.origin = self
            .spec
            .csrf
            .expected_origin()
            .map(str::to_string)
            .or_else(|| Some("same-origin".to_string()));
        let mut request = JetWebServerFnRequest::json(
            self.spec.method.clone(),
            self.spec.endpoint.clone(),
            body,
            context,
        );
        if let Some(key) = idempotency_key {
            request
                .headers
                .insert("idempotency-key".to_string(), key);
        }
        if let Some(origin) = self.spec.csrf.expected_origin() {
            request
                .headers
                .insert("origin".to_string(), origin.to_string());
        }
        if let Some(token) = self.spec.csrf.token() {
            request
                .headers
                .insert("x-csrf-token".to_string(), token.to_string());
        }
        Ok(request)
    }

    fn can_retry(&self, attempt: u32, key: Option<&str>, error: &JetWebServerFnError) -> bool {
        if attempt >= self.spec.retry.bounded_attempts() || !matches!(error, JetWebServerFnError::Transport { .. } | JetWebServerFnError::TimedOut) {
            return false;
        }
        match self.spec.retry {
            JetWebServerFnRetryPolicy::None => false,
            JetWebServerFnRetryPolicy::Safe { .. } => !self.spec.mutating(),
            JetWebServerFnRetryPolicy::Idempotent { .. } => {
                self.spec.idempotency_declared && key.is_some_and(|value| !value.is_empty())
            }
        }
    }

    pub fn call_encoded<F>(
        &self,
        body: String,
        idempotency_key: Option<String>,
        transport: F,
    ) -> Result<String, JetWebServerFnError>
    where
        F: Fn(JetWebServerFnRequest) -> Result<JetWebServerFnResponse, JetWebServerFnError>,
    {
        if body.len() > JET_WEB_SERVER_FN_MAX_BODY {
            return Err(JetWebServerFnError::InvalidInput(
                "server function input exceeds the payload limit".to_string(),
            ));
        }
        if self.spec.mutating()
            && matches!(self.spec.retry, JetWebServerFnRetryPolicy::Idempotent { .. })
            && (idempotency_key.is_none() || !self.spec.idempotency_declared)
        {
            return Err(JetWebServerFnError::InvalidDefinition(
                "idempotent retries require a declared idempotency key".to_string(),
            ));
        }
        let max_attempts = self.spec.retry.bounded_attempts();
        let mut last_error = None;
        for attempt in 1..=max_attempts {
            let request = self.synthetic_client_request(body.clone(), idempotency_key.clone())?;
            let request_id = request.context.request_id.clone();
            self.set_state(|state| {
                state.status = JetWebServerFnCallStatus::Pending;
                state.request_id = request_id.clone();
                state.attempts = attempt;
                state.idempotency_key = idempotency_key.clone();
                state.error.clear();
                state.result.clear();
            });
            if let Err(error) = request.context.check_live() {
                self.set_state(|state| {
                    state.status = match &error {
                        JetWebServerFnError::Cancelled => JetWebServerFnCallStatus::Cancelled,
                        _ => JetWebServerFnCallStatus::TimedOut,
                    };
                    state.error = error.to_string();
                });
                return Err(error);
            }
            let started = Instant::now();
            let result = transport(request);
            let elapsed = started.elapsed();
            let outcome = match result {
                Ok(response) if (200..300).contains(&response.status) => {
                    if self
                        .spec
                        .timeout_ms
                        .is_some_and(|timeout| elapsed > Duration::from_millis(timeout))
                    {
                        Err(JetWebServerFnError::UncertainCompletion { request_id })
                    } else if jet_std::parse_json_typed_datatree(&response.body).is_err() {
                        Err(JetWebServerFnError::InvalidOutput)
                    } else {
                        Ok(response.body)
                    }
                }
                Ok(response) => Err(JetWebServerFnError::Handler {
                    code: format!("http_{}", response.status),
                    message: "the server function returned an error".to_string(),
                }),
                Err(error) => Err(if error.dispatched() {
                    JetWebServerFnError::UncertainCompletion { request_id }
                } else {
                    error
                }),
            };
            match outcome {
                Ok(body) => {
                    self.set_state(|state| {
                        state.status = JetWebServerFnCallStatus::Success;
                        state.result = body.clone();
                    });
                    return Ok(body);
                }
                Err(error) => {
                    let retry = self.can_retry(attempt, idempotency_key.as_deref(), &error);
                    self.set_state(|state| {
                        state.status = match &error {
                            JetWebServerFnError::Cancelled => JetWebServerFnCallStatus::Cancelled,
                            JetWebServerFnError::TimedOut => JetWebServerFnCallStatus::TimedOut,
                            JetWebServerFnError::UncertainCompletion { .. } => {
                                JetWebServerFnCallStatus::Uncertain
                            }
                            _ => JetWebServerFnCallStatus::Error,
                        };
                        state.error = error.to_string();
                    });
                    if !retry {
                        return Err(error);
                    }
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap_or(JetWebServerFnError::RetryExhausted))
    }

    pub fn call<T, U, F>(
        &self,
        input: &T,
        idempotency_key: Option<String>,
        transport: F,
    ) -> Result<U, JetWebServerFnError>
    where
        T: __jet_Encode,
        U: __jet_Decode,
        F: Fn(JetWebServerFnRequest) -> Result<JetWebServerFnResponse, JetWebServerFnError>,
    {
        let body = jet_enc_json_to_string(input);
        let result = self.call_encoded(body, idempotency_key, transport)?;
        jet_enc_json_decode(&result).map_err(|_| JetWebServerFnError::InvalidOutput)
    }

    pub(crate) fn http_handler(&self) -> JetHTTPHandler {
        let function = self.clone();
        Arc::new(move |request| Ok(function.http_dispatch(request)))
    }

    fn http_dispatch(&self, request: JetHTTPRequest) -> JetHTTPResponse {
        let idempotency_key = request.headers.first("idempotency-key").map(str::to_string);
        let mut context = match self.context_from_http(&request, idempotency_key) {
            Ok(context) => context,
            Err(error) => {
                let mut response = jet_http_srv_response_owned(
                    error.status() as i64,
                    error.structured(&jet_web_server_fn_next_request_id()),
                );
                let _ = response
                    .headers
                    .set("content-type", "application/json");
                let _ = response
                    .headers
                    .set("content-security-policy", "default-src 'self'");
                return response;
            }
        };
        let body = match jet_http_request_text_with_limit(&request, JET_WEB_SERVER_FN_MAX_BODY as i64) {
            Ok(body) => body,
            Err(_) => {
                let error = JetWebServerFnError::InvalidInput(
                    "server function request body is unreadable".to_string(),
                );
                let mut response = jet_http_srv_response_owned(
                    error.status() as i64,
                    error.structured(&context.request_id),
                );
                let _ = response.headers.set("content-type", "application/json");
                let _ = response
                    .headers
                    .set("content-security-policy", "default-src 'self'");
                return response;
            }
        };
        let content_type = request
            .headers
            .first("content-type")
            .unwrap_or("application/json")
            .to_string();
        if context.csrf_token.is_none()
            && content_type
                .to_ascii_lowercase()
                .starts_with("application/x-www-form-urlencoded")
        {
            context.csrf_token = jet_web_server_fn_form_field(&body, "__jet_csrf");
        }
        let checked = JetWebServerFnRequest {
            method: request.method,
            path: request.path,
            content_type,
            body,
            headers: BTreeMap::new(),
            context,
        };
        let response = self.dispatch(checked);
        let mut http = jet_http_srv_response_owned(response.status as i64, response.body);
        let _ = http.headers.set("content-type", "application/json");
        let _ = http.headers.set("cache-control", "no-store");
        let _ = http.headers.set("x-request-id", &response.request_id);
        let _ = http
            .headers
            .set("content-security-policy", "default-src 'self'");
        http
    }
}

impl JetWebServerFnCsrfPolicy {
    fn expected_origin(&self) -> Option<&str> {
        match self {
            Self::SameOrigin {
                expected_origin: Some(origin),
                ..
            } => Some(origin.as_str()),
            _ => None,
        }
    }

    fn token(&self) -> Option<&str> {
        match self {
            Self::SameOrigin { token, .. } => token.as_deref(),
            Self::Disabled => None,
        }
    }
}

fn jet_web_server_fn_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn jet_web_server_fn_form_decode(value: &str) -> Result<String, JetWebServerFnError> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => out.push(b' '),
            b'%' if index + 2 < bytes.len() => {
                let Some(high) = jet_web_server_fn_hex(bytes[index + 1]) else {
                    return Err(JetWebServerFnError::InvalidInput(
                        "form input contains an invalid escape".to_string(),
                    ));
                };
                let Some(low) = jet_web_server_fn_hex(bytes[index + 2]) else {
                    return Err(JetWebServerFnError::InvalidInput(
                        "form input contains an invalid escape".to_string(),
                    ));
                };
                out.push(high << 4 | low);
                index += 2;
            }
            b'%' => {
                return Err(JetWebServerFnError::InvalidInput(
                    "form input contains an incomplete escape".to_string(),
                ));
            }
            byte => out.push(byte),
        }
        index += 1;
    }
    String::from_utf8(out).map_err(|_| {
        JetWebServerFnError::InvalidInput("form input is not valid UTF-8".to_string())
    })
}

fn jet_web_server_fn_form_json(body: &str) -> Result<String, JetWebServerFnError> {
    let mut fields: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if body.is_empty() {
        return Ok("{}".to_string());
    }
    for pair in body.split('&') {
        if fields.len() >= JET_WEB_SERVER_FN_MAX_FORM_FIELDS {
            return Err(JetWebServerFnError::InvalidInput(
                "form input has too many fields".to_string(),
            ));
        }
        let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = jet_web_server_fn_form_decode(raw_key)?;
        let value = jet_web_server_fn_form_decode(raw_value)?;
        if key.is_empty() || key.len() > 256 || value.len() > JET_WEB_SERVER_FN_MAX_FORM_VALUE {
            return Err(JetWebServerFnError::InvalidInput(
                "form input contains an invalid field".to_string(),
            ));
        }
        fields.entry(key).or_default().push(value);
    }

    let mut entries = Vec::with_capacity(fields.len());
    for (key, values) in fields {
        let value = if values.len() == 1 {
            jet_web_server_fn_json_string(&values[0])
        } else {
            format!(
                "[{}]",
                values
                    .iter()
                    .map(|value| jet_web_server_fn_json_string(value))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        entries.push(format!("{}:{value}", jet_web_server_fn_json_string(&key)));
    }
    Ok(format!("{{{}}}", entries.join(",")))
}
fn jet_web_server_fn_form_field(body: &str, wanted: &str) -> Option<String> {
    body.split('&').find_map(|pair| {
        let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = jet_web_server_fn_form_decode(raw_key).ok()?;
        if key == wanted {
            jet_web_server_fn_form_decode(raw_value).ok()
        } else {
            None
        }
    })
}

pub fn jet_web_server_fn_new(
    name: String,
    source_id: String,
    handler: JetWebServerFnHandler,
) -> Result<JetWebServerFunction, JetWebServerFnError> {
    JetWebServerFunction::new(name, source_id, handler)
}

pub fn jet_web_server_fn_typed(
    name: String,
    source_id: String,
    method: String,
    input_type: String,
    output_type: String,
    error_type: String,
    handler: JetWebServerFnHandler,
) -> Result<JetWebServerFunction, JetWebServerFnError> {
    JetWebServerFunction::typed(
        name,
        source_id,
        method,
        input_type,
        output_type,
        error_type,
        handler,
    )
}

pub(crate) fn jet_web_server_fn_register_http(
    mux: &JetHTTPMux,
    function: JetWebServerFunction,
) {
    let handler = function.http_handler();
    jet_http_mux_add_handler(mux, &function.spec.method, &function.spec.endpoint, handler);
}
