// D-HTTP-CORE2=A: one ordered, repeat-preserving header value shared by the
// client and server runtime paths.

const JET_HTTP_MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
enum JetHTTPOperation {
    ClientConnect,
    ServerBind,
    ServeListener,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum JetHTTPError {
    InvalidMethod,
    InvalidUrl,
    InvalidHeader,
    InvalidStatus,
    BodyConsumed,
    BodyTooLarge { limit: i64 },
    InvalidFraming,
    UnsupportedEncoding,
    Resolve { host: String },
    Connect { address: String },
    TLS { stage: String },
    Timeout { phase: String },
    Proxy { stage: String },
    Redirect { reason: String },
    Protocol { version: String },
    IO { operation: String },
    /// D-HTTP-CORS1=A: a policy value was refused when it was built. `reason`
    /// carries the user-facing copy that says what to change.
    Policy { reason: String },
    Cancelled,
    ResourceUnavailable { resource: String },
    UnsupportedTarget { operation: JetHTTPOperation },
    Internal { incident_id: String },
}

enum JetHTTPErrorSurfacePayload {
    Unit,
    Int {
        field: &'static str,
        value: i64,
    },
    Text {
        field: &'static str,
        value: String,
    },
    Operation {
        field: &'static str,
        variant: &'static str,
        ordinal: i64,
    },
}

struct JetHTTPErrorSurfaceParts {
    variant: &'static str,
    ordinal: i64,
    payload: JetHTTPErrorSurfacePayload,
}

/// Canonical CoreLib shape used by engine adapters to marshal `HTTPError`.
/// Ordinals follow the ratified surface enum order, not Rust declaration order.
fn jet_http_error_surface_parts(error: JetHTTPError) -> JetHTTPErrorSurfaceParts {
    let unit = |variant, ordinal| JetHTTPErrorSurfaceParts {
        variant,
        ordinal,
        payload: JetHTTPErrorSurfacePayload::Unit,
    };
    let int = |variant, ordinal, field, value| JetHTTPErrorSurfaceParts {
        variant,
        ordinal,
        payload: JetHTTPErrorSurfacePayload::Int { field, value },
    };
    let text = |variant, ordinal, field, value| JetHTTPErrorSurfaceParts {
        variant,
        ordinal,
        payload: JetHTTPErrorSurfacePayload::Text { field, value },
    };
    match error {
        JetHTTPError::InvalidMethod => unit("InvalidMethod", 0),
        JetHTTPError::InvalidUrl => unit("InvalidUrl", 1),
        JetHTTPError::InvalidHeader => unit("InvalidHeader", 2),
        JetHTTPError::InvalidStatus => unit("InvalidStatus", 3),
        JetHTTPError::BodyConsumed => unit("BodyConsumed", 4),
        JetHTTPError::InvalidFraming => unit("InvalidFraming", 5),
        JetHTTPError::UnsupportedEncoding => unit("UnsupportedEncoding", 6),
        JetHTTPError::Cancelled => unit("Cancelled", 7),
        JetHTTPError::BodyTooLarge { limit } => int("BodyTooLarge", 8, "limit", limit),
        JetHTTPError::Resolve { host } => text("Resolve", 9, "host", host),
        JetHTTPError::Connect { address } => text("Connect", 10, "address", address),
        JetHTTPError::TLS { stage } => text("TLS", 11, "stage", stage),
        JetHTTPError::Timeout { phase } => text("Timeout", 12, "phase", phase),
        JetHTTPError::Proxy { stage } => text("Proxy", 13, "stage", stage),
        JetHTTPError::Redirect { reason } => text("Redirect", 14, "reason", reason),
        JetHTTPError::Protocol { version } => text("Protocol", 15, "version", version),
        JetHTTPError::IO { operation } => text("IO", 16, "operation", operation),
        JetHTTPError::Policy { reason } => text("Policy", 17, "reason", reason),
        JetHTTPError::ResourceUnavailable { resource } => {
            text("ResourceUnavailable", 18, "resource", resource)
        }
        JetHTTPError::Internal { incident_id } => {
            text("Internal", 19, "incident_id", incident_id)
        }
        JetHTTPError::UnsupportedTarget { operation } => {
            let (variant, ordinal) = match operation {
                JetHTTPOperation::ClientConnect => ("ClientConnect", 0),
                JetHTTPOperation::ServerBind => ("ServerBind", 1),
                JetHTTPOperation::ServeListener => ("ServeListener", 2),
            };
            JetHTTPErrorSurfaceParts {
                variant: "UnsupportedTarget",
                ordinal: 20,
                payload: JetHTTPErrorSurfacePayload::Operation {
                    field: "operation",
                    variant,
                    ordinal,
                },
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetHTTPMethod(String);

impl JetHTTPMethod {
    fn custom(token: String) -> Result<Self, JetHTTPError> {
        if token.is_empty()
            || !token.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*'
                        | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~')
            })
        {
            return Err(JetHTTPError::InvalidMethod);
        }
        Ok(Self(token))
    }

    fn get() -> Self { Self("GET".to_string()) }
    fn head() -> Self { Self("HEAD".to_string()) }
    fn post() -> Self { Self("POST".to_string()) }
    fn put() -> Self { Self("PUT".to_string()) }
    fn delete() -> Self { Self("DELETE".to_string()) }
    fn connect() -> Self { Self("CONNECT".to_string()) }
    fn options() -> Self { Self("OPTIONS".to_string()) }
    fn trace() -> Self { Self("TRACE".to_string()) }
    fn patch() -> Self { Self("PATCH".to_string()) }
}

impl JetShow for JetHTTPMethod { fn jet_show(&self) -> String { self.0.clone() } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct JetHTTPStatus(i64);

impl JetHTTPStatus {
    fn new(code: i64) -> Result<Self, JetHTTPError> {
        (100..=599).contains(&code).then_some(Self(code)).ok_or(JetHTTPError::InvalidStatus)
    }
}

impl JetShow for JetHTTPStatus { fn jet_show(&self) -> String { self.0.to_string() } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JetHTTPVersion { HTTP10, HTTP11, HTTP2 }

impl JetHTTPVersion {
    fn http_1_0() -> Self { Self::HTTP10 }
    fn http_1_1() -> Self { Self::HTTP11 }
    fn http_2() -> Self { Self::HTTP2 }
}

impl JetShow for JetHTTPVersion {
    fn jet_show(&self) -> String {
        match self { Self::HTTP10 => "HTTP/1.0", Self::HTTP11 => "HTTP/1.1", Self::HTTP2 => "HTTP/2" }.to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetHTTPHeaderName(String);

impl JetHTTPHeaderName {
    fn new(name: String) -> Result<Self, JetHTTPError> {
        JetHTTPHeaders::valid_name(&name).then_some(Self(name)).ok_or(JetHTTPError::InvalidHeader)
    }
}

impl JetShow for JetHTTPHeaderName { fn jet_show(&self) -> String { self.0.clone() } }

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetHTTPHeaderValue(String);

impl JetHTTPHeaderValue {
    fn new(value: String) -> Result<Self, JetHTTPError> {
        JetHTTPHeaders::valid_value(&value).then_some(Self(value)).ok_or(JetHTTPError::InvalidHeader)
    }
}

impl JetShow for JetHTTPHeaderValue { fn jet_show(&self) -> String { self.0.clone() } }

impl std::fmt::Display for JetHTTPError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMethod => formatter.write_str("invalid HTTP method"),
            Self::InvalidUrl => formatter.write_str("invalid HTTP URL"),
            Self::InvalidHeader => formatter.write_str("invalid HTTP header"),
            Self::InvalidStatus => formatter.write_str("invalid HTTP status"),
            Self::BodyConsumed => formatter.write_str("HTTP body was already consumed"),
            Self::BodyTooLarge { limit } => write!(formatter, "HTTP body exceeds {limit}-byte limit"),
            Self::InvalidFraming => formatter.write_str("invalid HTTP framing"),
            Self::UnsupportedEncoding => formatter.write_str("unsupported HTTP body encoding"),
            Self::Resolve { host } => write!(formatter, "HTTP name resolution failed for {host}"),
            Self::Connect { address } => write!(formatter, "HTTP connection failed for {address}"),
            Self::TLS { stage } => write!(formatter, "HTTP TLS failed during {stage}"),
            Self::Timeout { phase } => write!(formatter, "HTTP timeout during {phase}"),
            Self::Proxy { stage } => write!(formatter, "HTTP proxy failed during {stage}"),
            Self::Redirect { reason } => write!(formatter, "HTTP redirect failed: {reason}"),
            Self::Protocol { version } => write!(formatter, "unsupported HTTP protocol {version}"),
            Self::IO { operation } => write!(formatter, "HTTP I/O failed during {operation}"),
            Self::Policy { reason } => write!(formatter, "{reason}"),
            Self::Cancelled => formatter.write_str("HTTP operation cancelled"),
            Self::ResourceUnavailable { resource } => write!(formatter, "HTTP resource unavailable: {resource}"),
            Self::UnsupportedTarget { operation } => {
                let operation = match operation {
                    JetHTTPOperation::ClientConnect => "client connect",
                    JetHTTPOperation::ServerBind => "server bind",
                    JetHTTPOperation::ServeListener => "serve listener",
                };
                write!(formatter, "HTTP {operation} is unavailable on this build target")
            }
            Self::Internal { incident_id } => write!(formatter, "internal HTTP failure; incident {incident_id}"),
        }
    }
}

impl JetShow for JetHTTPError {
    fn jet_show(&self) -> String {
        self.to_string()
    }
}

struct JetHTTPBodyCloser {
    closed: std::sync::atomic::AtomicBool,
    close: Box<dyn Fn() + Send + Sync>,
}

impl JetHTTPBodyCloser {
    fn new(close: impl Fn() + Send + Sync + 'static) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            closed: std::sync::atomic::AtomicBool::new(false),
            close: Box::new(close),
        })
    }

    fn close(&self) {
        if !self.closed.swap(true, std::sync::atomic::Ordering::AcqRel) { (self.close)(); }
    }
}

enum JetHTTPBodySource {
    Bytes(std::io::Cursor<Vec<u8>>),
    File(std::io::Take<std::fs::File>),
    Reader {
        reader: Box<dyn std::io::Read + Send>,
        closer: Option<std::sync::Arc<JetHTTPBodyCloser>>,
    },
    Bridge {
        handle: i64,
        read: fn(i64, usize) -> Result<Option<Vec<u8>>, JetHTTPError>,
        closer: std::sync::Arc<JetHTTPBodyCloser>,
    },
}

impl JetHTTPBodySource {
    fn closer(&self) -> Option<std::sync::Arc<JetHTTPBodyCloser>> {
        match self {
            Self::Reader { closer, .. } => closer.clone(),
            Self::Bridge { closer, .. } => Some(closer.clone()),
            Self::Bytes(_) | Self::File(_) => None,
        }
    }

    fn h2_cancellable(&self) -> bool {
        matches!(self, Self::Bytes(_) | Self::File(_) | Self::Reader { closer: Some(_), .. })
    }

    fn close(&self) {
        if let Some(closer) = self.closer() { closer.close(); }
    }
}

struct JetHTTPBodyState {
    source: Option<JetHTTPBodySource>,
    length: Option<usize>,
    content_type: Option<String>,
    drained: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for JetHTTPBodyState {
    fn drop(&mut self) {
        if let Some(source) = self.source.take() { source.close(); }
    }
}

#[derive(Clone)]
struct JetHTTPBody {
    state: std::sync::Arc<std::sync::Mutex<JetHTTPBodyState>>,
}

impl JetHTTPBody {
    fn empty() -> Self {
        Self::from_bytes(Vec::new())
    }

    fn from_bytes(bytes: Vec<u8>) -> Self {
        Self::from_bytes_with_content_type(bytes, None)
    }

    fn from_bytes_with_content_type(bytes: Vec<u8>, content_type: Option<String>) -> Self {
        let length = bytes.len();
        let drained = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(length == 0));
        Self { state: std::sync::Arc::new(std::sync::Mutex::new(JetHTTPBodyState {
            source: Some(JetHTTPBodySource::Bytes(std::io::Cursor::new(bytes))),
            length: Some(length),
            content_type,
            drained,
        })) }
    }

    fn from_text(text: String) -> Self {
        Self::from_bytes_with_content_type(
            text.into_bytes(),
            Some("text/plain; charset=utf-8".to_string()),
        )
    }

    fn from_text_with_mime(text: String, content_type: jet_std::JetMIME) -> Self {
        Self::from_bytes_with_content_type(
            text.into_bytes(),
            Some(content_type.to_string_value()),
        )
    }

    fn from_json<T: __jet_Encode>(value: T) -> Self {
        Self::from_bytes_with_content_type(
            jet_enc_json_to_string(&value).into_bytes(),
            Some("application/json".to_string()),
        )
    }

    fn from_form<I>(values: I) -> Self
    where
        for<'a> &'a I: IntoIterator<Item = (&'a String, &'a String)>,
    {
        fn encode(text: &str) -> String {
            let mut encoded = String::new();
            for byte in text.bytes() {
                if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                    encoded.push(char::from(byte));
                } else if byte == b' ' {
                    encoded.push('+');
                } else {
                    encoded.push_str(&format!("%{byte:02X}"));
                }
            }
            encoded
        }
        let body = (&values).into_iter()
            .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
            .collect::<Vec<_>>()
            .join("&");
        Self::from_bytes_with_content_type(
            body.into_bytes(),
            Some("application/x-www-form-urlencoded".to_string()),
        )
    }

    fn from_multipart<I>(values: I) -> Self
    where
        for<'a> &'a I: IntoIterator<Item = (&'a String, &'a String)>,
    {
        const PREFIX: &str = "jet-http-boundary-";
        const LENGTH: usize = PREFIX.len() + 16;
        let mut used = std::collections::HashSet::new();
        for text in (&values).into_iter().flat_map(|(name, value)| [name.as_str(), value.as_str()]) {
            for window in text.as_bytes().windows(LENGTH) {
                let Some(suffix) = window.strip_prefix(PREFIX.as_bytes()) else {
                    continue;
                };
                let Ok(suffix) = std::str::from_utf8(suffix) else {
                    continue;
                };
                if let Ok(suffix) = u64::from_str_radix(suffix, 16) {
                    used.insert(suffix);
                }
            }
        }
        let mut suffix = 0u64;
        while used.contains(&suffix) {
            suffix = suffix
                .checked_add(1)
                .expect("in-memory multipart fields cannot contain every u64 boundary suffix");
        }
        let boundary = format!("{PREFIX}{suffix:016x}");
        let mut body = Vec::new();
        for (name, value) in (&values).into_iter() {
            let name = name.replace('\"', "%22").replace('\r', "%0D").replace('\n', "%0A");
            body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        Self::from_bytes_with_content_type(
            body,
            Some(format!("multipart/form-data; boundary={boundary}")),
        )
    }

    fn reader(reader: impl std::io::Read + Send + 'static, length: Option<usize>) -> Self {
        Self::reader_with_content_type(reader, length, None)
    }

    fn reader_with_content_type(
        reader: impl std::io::Read + Send + 'static,
        length: Option<usize>,
        content_type: Option<String>,
    ) -> Self {
        let drained = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(length == Some(0)));
        Self {
            state: std::sync::Arc::new(std::sync::Mutex::new(JetHTTPBodyState {
                source: Some(JetHTTPBodySource::Reader { reader: Box::new(reader), closer: None }),
                length,
                content_type,
                drained,
            })),
        }
    }

    fn reader_cancellable(
        reader: impl std::io::Read + Send + 'static,
        length: Option<usize>,
        close: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let drained = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(length == Some(0)));
        Self { state: std::sync::Arc::new(std::sync::Mutex::new(JetHTTPBodyState {
            source: Some(JetHTTPBodySource::Reader {
                reader: Box::new(reader),
                closer: Some(JetHTTPBodyCloser::new(close)),
            }),
            length,
            content_type: None,
            drained,
        })) }
    }

    fn file(file: std::fs::File, length: usize) -> Self {
        let drained = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(length == 0));
        Self { state: std::sync::Arc::new(std::sync::Mutex::new(JetHTTPBodyState {
            source: Some(JetHTTPBodySource::File(std::io::Read::take(file, length as u64))),
            length: Some(length),
            content_type: None,
            drained,
        })) }
    }

    fn from_reader(reader: JetFileReader) -> Result<Self, JetHTTPError> {
        Ok(Self::reader(reader.inner, None))
    }

    fn from_reader_with_length(reader: JetFileReader, length: i64) -> Result<Self, JetHTTPError> {
        let length = usize::try_from(length).ok().filter(|length| *length <= 1024 * 1024 * 1024)
            .ok_or(JetHTTPError::BodyTooLarge { limit: length })?;
        Ok(Self::reader(reader.inner, Some(length)))
    }

    fn bridge(
        handle: i64,
        length: Option<usize>,
        read: fn(i64, usize) -> Result<Option<Vec<u8>>, JetHTTPError>,
        close: fn(i64),
    ) -> Self {
        let drained = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(length == Some(0)));
        let closer = JetHTTPBodyCloser::new(move || close(handle));
        Self {
            state: std::sync::Arc::new(std::sync::Mutex::new(JetHTTPBodyState {
                source: Some(JetHTTPBodySource::Bridge {
                    handle,
                    read,
                    closer,
                }),
                length,
                content_type: None,
                drained,
            })),
        }
    }

    fn length(&self) -> Option<usize> {
        self.state.lock().ok().and_then(|state| state.length)
    }

    fn content_type(&self) -> Option<String> {
        self.state.lock().ok().and_then(|state| state.content_type.clone())
    }

    fn len(&self) -> usize {
        self.length().unwrap_or(0)
    }

    fn is_drained(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.drained.load(std::sync::atomic::Ordering::Acquire))
            .unwrap_or(false)
    }

    fn is_empty(&self) -> bool {
        self.length() == Some(0)
    }

    fn chunks(&self, max_chunk: usize) -> Result<JetHTTPBodyChunks, JetHTTPError> {
        if max_chunk == 0 {
            return Err(JetHTTPError::BodyTooLarge { limit: 0 });
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| JetHTTPError::Internal {
                incident_id: "http-body-lock".to_string(),
            })?;
        let source = state
            .source
            .take()
            .ok_or(JetHTTPError::BodyConsumed)?;
        let drained = state.drained.clone();
        Ok(JetHTTPBodyChunks {
            source,
            max_chunk,
            done: false,
            initial_error: None,
            drained,
        })
    }

    fn bytes(&self, limit: usize) -> Result<Vec<u8>, JetHTTPError> {
        if self.length().is_some_and(|length| length > limit) {
            let _ = self.chunks(64 * 1024)?;
            return Err(JetHTTPError::BodyTooLarge { limit: limit as i64 });
        }
        let mut bytes = Vec::new();
        for chunk in self.chunks(64 * 1024)? {
            let chunk = chunk?;
            if bytes.len().saturating_add(chunk.len()) > limit {
                return Err(JetHTTPError::BodyTooLarge { limit: limit as i64 });
            }
            bytes.extend(chunk);
        }
        Ok(bytes)
    }

    fn text(&self, limit: usize) -> Result<String, JetHTTPError> {
        String::from_utf8(self.bytes(limit)?).map_err(|_| JetHTTPError::UnsupportedEncoding)
    }
}

struct JetHTTPBodyChunks {
    source: JetHTTPBodySource,
    max_chunk: usize,
    done: bool,
    initial_error: Option<JetHTTPError>,
    drained: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl JetHTTPBodyChunks {
    fn failed(error: JetHTTPError) -> Self {
        Self {
            source: JetHTTPBodySource::Bytes(std::io::Cursor::new(Vec::new())),
            max_chunk: 1,
            done: false,
            initial_error: Some(error),
            drained: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn h2_cancellable(&self) -> bool { self.source.h2_cancellable() }

    fn closer(&self) -> Option<std::sync::Arc<JetHTTPBodyCloser>> { self.source.closer() }
}

impl Iterator for JetHTTPBodyChunks {
    type Item = Result<Vec<u8>, JetHTTPError>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(error) = self.initial_error.take() {
            self.done = true;
            return Some(Err(error));
        }
        if self.done {
            return None;
        }
        let mut chunk = vec![0; self.max_chunk];
        let read = match &mut self.source {
            JetHTTPBodySource::Bytes(reader) => std::io::Read::read(reader, &mut chunk),
            JetHTTPBodySource::File(file) => std::io::Read::read(file, &mut chunk),
            JetHTTPBodySource::Reader { reader, .. } => reader.read(&mut chunk),
            JetHTTPBodySource::Bridge { handle, read, .. } => match read(*handle, self.max_chunk) {
                Ok(Some(bytes)) => return Some(Ok(bytes)),
                Ok(None) => Ok(0),
                Err(error) => {
                    self.done = true;
                    return Some(Err(error));
                }
            },
        };
        match read {
            Ok(0) => {
                self.done = true;
                self.drained.store(true, std::sync::atomic::Ordering::Release);
                None
            }
            Ok(read) => {
                chunk.truncate(read);
                Some(Ok(chunk))
            }
            Err(error) => {
                self.done = true;
                Some(Err(match error.kind() {
                    std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof => JetHTTPError::InvalidFraming,
                    std::io::ErrorKind::OutOfMemory => JetHTTPError::BodyTooLarge { limit: 32 * 1024 },
                    _ => JetHTTPError::IO { operation: "read body".to_string() },
                }))
            }
        }
    }
}

impl Drop for JetHTTPBodyChunks {
    fn drop(&mut self) { self.source.close(); }
}

impl std::fmt::Debug for JetHTTPBody {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HTTPBody")
            .field("length", &self.length())
            .finish_non_exhaustive()
    }
}

fn jet_http_body_bytes(body: &JetHTTPBody, limit: i64) -> Result<Vec<u8>, JetHTTPError> {
    let limit = jet_http_consume_limit(limit)?;
    body.bytes(limit)
}

fn jet_http_body_text(body: &JetHTTPBody, limit: i64) -> Result<String, JetHTTPError> {
    let limit = jet_http_consume_limit(limit)?;
    body.text(limit)
}

fn jet_http_body_json_text(body: &JetHTTPBody, limit: i64) -> Result<String, JetHTTPError> {
    jet_http_body_text(body, limit)
}

fn jet_http_body_json_text_defaulted(
    body: &JetHTTPBody,
    limit: Option<i64>,
) -> Result<String, JetHTTPError> {
    jet_http_body_json_text(body, limit.unwrap_or(JET_HTTP_MAX_BODY_BYTES as i64))
}

fn jet_http_json_decode_error() -> JetHTTPError {
    JetHTTPError::InvalidFraming
}

fn jet_http_body_json<T: __jet_Decode>(body: &JetHTTPBody, limit: i64) -> Result<T, JetHTTPError> {
    let text = jet_http_body_json_text(body, limit)?;
    jet_enc_json_decode(&text).map_err(|_| jet_http_json_decode_error())
}

fn jet_http_body_json_defaulted<T: __jet_Decode>(
    body: &JetHTTPBody,
    limit: Option<i64>,
) -> Result<T, JetHTTPError> {
    let text = jet_http_body_json_text_defaulted(body, limit)?;
    jet_enc_json_decode(&text).map_err(|_| jet_http_json_decode_error())
}

fn jet_http_body_copy_to(
    body: &JetHTTPBody,
    writer: &mut JetFileWriter,
    limit: i64,
) -> Result<i64, JetHTTPError> {
    use std::io::Write;
    let limit = jet_http_consume_limit(limit)?;
    if body.length().is_some_and(|length| length > limit) {
        let _ = body.chunks(64 * 1024)?;
        return Err(JetHTTPError::BodyTooLarge { limit: limit as i64 });
    }
    let mut total = 0usize;
    for chunk in body.chunks(64 * 1024)? {
        let chunk = chunk?;
        total = total.checked_add(chunk.len()).ok_or(JetHTTPError::BodyTooLarge { limit: limit as i64 })?;
        if total > limit {
            return Err(JetHTTPError::BodyTooLarge { limit: limit as i64 });
        }
        writer.inner.write_all(&chunk).map_err(|_| JetHTTPError::IO { operation: "copy body".to_string() })?;
    }
    i64::try_from(total).map_err(|_| JetHTTPError::BodyTooLarge { limit: limit as i64 })
}

fn jet_http_consume_limit(limit: i64) -> Result<usize, JetHTTPError> {
    if !(0..=1024 * 1024 * 1024).contains(&limit) {
        return Err(JetHTTPError::BodyTooLarge { limit });
    }
    Ok(limit as usize)
}

fn jet_http_body_chunks(body: &JetHTTPBody, max_chunk: i64) -> JetHTTPBodyChunks {
    let max_chunk = usize::try_from(max_chunk)
        .ok()
        .filter(|limit| (1..=1024 * 1024 * 1024).contains(limit));
    match max_chunk {
        Some(max_chunk) => body.chunks(max_chunk).unwrap_or_else(JetHTTPBodyChunks::failed),
        None => JetHTTPBodyChunks::failed(JetHTTPError::BodyTooLarge { limit: 0 }),
    }
}

impl PartialEq<&str> for JetHTTPBody {
    fn eq(&self, expected: &&str) -> bool {
        self.text(1024 * 1024).as_deref() == Ok(*expected)
    }
}

impl PartialEq<String> for JetHTTPBody {
    fn eq(&self, expected: &String) -> bool {
        self.text(1024 * 1024).as_ref() == Ok(expected)
    }
}

#[derive(Clone)]
struct JetHTTPRequest {
    method: String,
    url: String,
    path: String,
    version: String,
    headers: JetHTTPHeaders,
    trailers: std::sync::Arc<std::sync::Mutex<JetHTTPHeaders>>,
    body: JetHTTPBody,
    body_set: bool,
    params: std::collections::BTreeMap<String, String>,
    route_template: Option<String>,
    header_error: Option<JetHTTPError>,
    timeout_ms: Option<i64>,
    connect_timeout_ms: Option<i64>,
    read_timeout_ms: Option<i64>,
    total_timeout_ms: Option<i64>,
    dns_timeout_ms: Option<i64>,
    tls_timeout_ms: Option<i64>,
    write_timeout_ms: Option<i64>,
    first_byte_timeout_ms: Option<i64>,
    redirects: Option<i64>,
    proxy: Option<String>,
    cookies: Vec<String>,
    form: Vec<String>,
    multipart: Vec<String>,
}

#[derive(Clone)]
struct JetHTTPResponse {
    status: i64,
    version: String,
    headers: JetHTTPHeaders,
    body: JetHTTPBody,
    trailers: JetHTTPHeaders,
    head_content_length: Option<usize>,
    suppress_body: bool,
    protocol: String,
    remote_address: String,
    redirect_history: Vec<String>,
    timings_ms: Vec<i64>,
    reused_connection: bool,
    raw_content_encoding: Option<String>,
}

type JetHTTPHandler = std::sync::Arc<
    dyn Fn(JetHTTPRequest) -> Result<JetHTTPResponse, JetHTTPError> + Send + Sync,
>;

impl JetHTTPRequest {
    fn server(method: &str, path: String, body: Vec<u8>, headers: JetHTTPHeaders) -> Self {
        Self::server_body(method, path, JetHTTPBody::from_bytes(body), headers)
    }

    fn server_body(
        method: &str,
        path: String,
        body: JetHTTPBody,
        headers: JetHTTPHeaders,
    ) -> Self {
        Self::server_body_with_trailers(
            method,
            path,
            body,
            headers,
            std::sync::Arc::new(std::sync::Mutex::new(JetHTTPHeaders::new())),
        )
    }

    fn server_body_with_trailers(
        method: &str,
        path: String,
        body: JetHTTPBody,
        headers: JetHTTPHeaders,
        trailers: std::sync::Arc<std::sync::Mutex<JetHTTPHeaders>>,
    ) -> Self {
        Self {
            method: method.to_string(),
            url: String::new(),
            path,
            version: "HTTP/1.1".to_string(),
            headers,
            trailers,
            body,
            body_set: true,
            params: std::collections::BTreeMap::new(),
            route_template: None,
            header_error: None,
            timeout_ms: None,
            connect_timeout_ms: None,
            read_timeout_ms: None,
            total_timeout_ms: None,
            dns_timeout_ms: None,
            tls_timeout_ms: None,
            write_timeout_ms: None,
            first_byte_timeout_ms: None,
            redirects: None,
            proxy: None,
            cookies: Vec::new(),
            form: Vec::new(),
            multipart: Vec::new(),
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
struct JetHTTPHeaders {
    entries: Vec<(String, String)>,
}

impl JetHTTPHeaders {
    fn new() -> Self {
        Self::default()
    }

    fn valid_name(name: &str) -> bool {
        !name.is_empty()
            && name.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(
                        byte,
                        b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+'
                            | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
                    )
            })
    }

    fn valid_value(value: &str) -> bool {
        value.chars().all(|character| {
            (!character.is_control() || character == '\t')
                && (!character.is_whitespace() || matches!(character, ' ' | '\t'))
        })
    }

    fn append(&mut self, name: &str, value: &str) -> Result<(), String> {
        if !Self::valid_name(name) {
            return Err(format!("invalid HTTP header name `{name}`"));
        }
        if !Self::valid_value(value) {
            return Err(format!("invalid value for HTTP header `{name}`"));
        }
        self.entries.push((name.to_string(), value.to_string()));
        Ok(())
    }

    fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        if !Self::valid_name(name) {
            return Err(format!("invalid HTTP header name `{name}`"));
        }
        if !Self::valid_value(value) {
            return Err(format!("invalid value for HTTP header `{name}`"));
        }
        self.remove(name);
        self.entries.push((name.to_string(), value.to_string()));
        Ok(())
    }

    fn first(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn get(&self, name: &str) -> Option<&String> {
        self.entries
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    }

    fn all(&self, name: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
            .collect()
    }

    fn remove(&mut self, name: &str) {
        self.entries
            .retain(|(candidate, _)| !candidate.eq_ignore_ascii_case(name));
    }

    fn to_flat(&self) -> Vec<String> {
        self.entries
            .iter()
            .flat_map(|(name, value)| [name.clone(), value.clone()])
            .collect()
    }

    fn from_flat(flat: Vec<String>) -> Result<Self, String> {
        if flat.len() % 2 != 0 {
            return Err("invalid flattened HTTP headers".to_string());
        }
        let mut headers = Self::new();
        for pair in flat.chunks_exact(2) {
            headers.append(&pair[0], &pair[1])?;
        }
        Ok(headers)
    }
}

fn jet_http_headers_first(headers: &JetHTTPHeaders, name: &String) -> Option<String> {
    headers.get(name).cloned()
}

fn jet_http_headers_all(headers: &JetHTTPHeaders, name: &String) -> Vec<String> {
    headers.all(name).into_iter().map(str::to_string).collect()
}

fn jet_http_headers_append(
    mut headers: JetHTTPHeaders,
    name: &String,
    value: &String,
) -> Result<JetHTTPHeaders, JetHTTPError> {
    headers.append(name, value).map_err(|_| JetHTTPError::InvalidHeader)?;
    Ok(headers)
}

fn jet_http_headers_set(
    mut headers: JetHTTPHeaders,
    name: &String,
    value: &String,
) -> Result<JetHTTPHeaders, JetHTTPError> {
    headers.set(name, value).map_err(|_| JetHTTPError::InvalidHeader)?;
    Ok(headers)
}

fn jet_http_headers_remove(mut headers: JetHTTPHeaders, name: &String) -> JetHTTPHeaders {
    headers.remove(name);
    headers
}

impl std::fmt::Debug for JetHTTPHeaders {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut list = formatter.debug_list();
        for (name, value) in &self.entries {
            let secret = matches!(
                name.to_ascii_lowercase().as_str(),
                "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
            );
            list.entry(&(name, if secret { "<redacted>" } else { value.as_str() }));
        }
        list.finish()
    }
}

impl FromIterator<(String, String)> for JetHTTPHeaders {
    fn from_iter<T: IntoIterator<Item = (String, String)>>(iter: T) -> Self {
        let mut headers = Self::new();
        for (name, value) in iter {
            headers
                .append(&name, &value)
                .expect("compiler-generated HTTP header is valid");
        }
        headers
    }
}

impl<'a> IntoIterator for &'a JetHTTPHeaders {
    type Item = &'a (String, String);
    type IntoIter = std::slice::Iter<'a, (String, String)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}
