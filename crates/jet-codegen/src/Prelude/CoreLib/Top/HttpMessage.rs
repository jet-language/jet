// D-HTTP-CORE2=A: one ordered, repeat-preserving header value shared by the
// client and server runtime paths.

#[derive(Clone, Debug, PartialEq, Eq)]
enum JetHttpOperation {
    ClientConnect,
    ServerBind,
    ServeListener,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum JetHttpError {
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
    Tls { stage: String },
    Timeout { phase: String },
    Proxy { stage: String },
    Redirect { reason: String },
    Protocol { version: String },
    Io { operation: String },
    Cancelled,
    ResourceUnavailable { resource: String },
    UnsupportedTarget { operation: JetHttpOperation },
    Internal { incident_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetHttpMethod(String);

impl JetHttpMethod {
    fn custom(token: String) -> Result<Self, JetHttpError> {
        if token.is_empty()
            || !token.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*'
                        | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~')
            })
        {
            return Err(JetHttpError::InvalidMethod);
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

impl JetShow for JetHttpMethod { fn jet_show(&self) -> String { self.0.clone() } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct JetHttpStatus(i64);

impl JetHttpStatus {
    fn new(code: i64) -> Result<Self, JetHttpError> {
        (100..=599).contains(&code).then_some(Self(code)).ok_or(JetHttpError::InvalidStatus)
    }
}

impl JetShow for JetHttpStatus { fn jet_show(&self) -> String { self.0.to_string() } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JetHttpVersion { Http10, Http11, Http2 }

impl JetHttpVersion {
    fn http_1_0() -> Self { Self::Http10 }
    fn http_1_1() -> Self { Self::Http11 }
    fn http_2() -> Self { Self::Http2 }
}

impl JetShow for JetHttpVersion {
    fn jet_show(&self) -> String {
        match self { Self::Http10 => "HTTP/1.0", Self::Http11 => "HTTP/1.1", Self::Http2 => "HTTP/2" }.to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetHttpHeaderName(String);

impl JetHttpHeaderName {
    fn new(name: String) -> Result<Self, JetHttpError> {
        JetHttpHeaders::valid_name(&name).then_some(Self(name)).ok_or(JetHttpError::InvalidHeader)
    }
}

impl JetShow for JetHttpHeaderName { fn jet_show(&self) -> String { self.0.clone() } }

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetHttpHeaderValue(String);

impl JetHttpHeaderValue {
    fn new(value: String) -> Result<Self, JetHttpError> {
        JetHttpHeaders::valid_value(&value).then_some(Self(value)).ok_or(JetHttpError::InvalidHeader)
    }
}

impl JetShow for JetHttpHeaderValue { fn jet_show(&self) -> String { self.0.clone() } }

impl std::fmt::Display for JetHttpError {
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
            Self::Tls { stage } => write!(formatter, "HTTP TLS failed during {stage}"),
            Self::Timeout { phase } => write!(formatter, "HTTP timeout during {phase}"),
            Self::Proxy { stage } => write!(formatter, "HTTP proxy failed during {stage}"),
            Self::Redirect { reason } => write!(formatter, "HTTP redirect failed: {reason}"),
            Self::Protocol { version } => write!(formatter, "unsupported HTTP protocol {version}"),
            Self::Io { operation } => write!(formatter, "HTTP I/O failed during {operation}"),
            Self::Cancelled => formatter.write_str("HTTP operation cancelled"),
            Self::ResourceUnavailable { resource } => write!(formatter, "HTTP resource unavailable: {resource}"),
            Self::UnsupportedTarget { operation } => {
                let operation = match operation {
                    JetHttpOperation::ClientConnect => "client connect",
                    JetHttpOperation::ServerBind => "server bind",
                    JetHttpOperation::ServeListener => "serve listener",
                };
                write!(formatter, "HTTP {operation} is unavailable on this build target")
            }
            Self::Internal { incident_id } => write!(formatter, "internal HTTP failure; incident {incident_id}"),
        }
    }
}

impl JetShow for JetHttpError {
    fn jet_show(&self) -> String {
        self.to_string()
    }
}

struct JetHttpBodyCloser {
    closed: std::sync::atomic::AtomicBool,
    close: Box<dyn Fn() + Send + Sync>,
}

impl JetHttpBodyCloser {
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

enum JetHttpBodySource {
    Bytes(std::io::Cursor<Vec<u8>>),
    File(std::io::Take<std::fs::File>),
    Reader {
        reader: Box<dyn std::io::Read + Send>,
        closer: Option<std::sync::Arc<JetHttpBodyCloser>>,
    },
    Bridge {
        handle: i64,
        read: fn(i64, usize) -> Result<Option<Vec<u8>>, JetHttpError>,
        closer: std::sync::Arc<JetHttpBodyCloser>,
    },
}

impl JetHttpBodySource {
    fn closer(&self) -> Option<std::sync::Arc<JetHttpBodyCloser>> {
        match self {
            Self::Reader { closer, .. } => closer.clone(),
            Self::Bridge { closer, .. } => Some(closer.clone()),
            Self::Bytes(_) | Self::File(_) => None,
        }
    }

    fn h2_cancellable(&self) -> bool {
        matches!(self, Self::Bytes(_) | Self::File(_)) || self.closer().is_some()
    }

    fn close(&self) {
        if let Some(closer) = self.closer() { closer.close(); }
    }
}

struct JetHttpBodyState {
    source: Option<JetHttpBodySource>,
    length: Option<usize>,
    content_type: Option<String>,
    drained: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for JetHttpBodyState {
    fn drop(&mut self) {
        if let Some(source) = self.source.take() { source.close(); }
    }
}

#[derive(Clone)]
struct JetHttpBody {
    state: std::sync::Arc<std::sync::Mutex<JetHttpBodyState>>,
}

impl JetHttpBody {
    fn empty() -> Self {
        Self::from_bytes(Vec::new())
    }

    fn from_bytes(bytes: Vec<u8>) -> Self {
        Self::from_bytes_with_content_type(bytes, None)
    }

    fn from_bytes_with_content_type(bytes: Vec<u8>, content_type: Option<String>) -> Self {
        let length = bytes.len();
        let drained = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(length == 0));
        Self { state: std::sync::Arc::new(std::sync::Mutex::new(JetHttpBodyState {
            source: Some(JetHttpBodySource::Bytes(std::io::Cursor::new(bytes))),
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

    fn from_text_with_mime(text: String, content_type: jet_std::JetMime) -> Self {
        Self::from_bytes_with_content_type(
            text.into_bytes(),
            Some(content_type.to_string_value()),
        )
    }

    fn from_json<T: user_Encode>(value: T) -> Self {
        Self::from_bytes_with_content_type(
            jet_enc_json_to_string(&value).into_bytes(),
            Some("application/json".to_string()),
        )
    }

    fn from_form(values: std::collections::BTreeMap<String, String>) -> Self {
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
        let body = values.into_iter()
            .map(|(name, value)| format!("{}={}", encode(&name), encode(&value)))
            .collect::<Vec<_>>()
            .join("&");
        Self::from_bytes_with_content_type(
            body.into_bytes(),
            Some("application/x-www-form-urlencoded".to_string()),
        )
    }

    fn from_multipart(values: std::collections::BTreeMap<String, String>) -> Self {
        const PREFIX: &str = "jet-http-boundary-";
        const LENGTH: usize = PREFIX.len() + 16;
        let mut used = std::collections::HashSet::new();
        for text in values.iter().flat_map(|(name, value)| [name.as_str(), value.as_str()]) {
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
        for (name, value) in values {
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
            state: std::sync::Arc::new(std::sync::Mutex::new(JetHttpBodyState {
                source: Some(JetHttpBodySource::Reader { reader: Box::new(reader), closer: None }),
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
        Self { state: std::sync::Arc::new(std::sync::Mutex::new(JetHttpBodyState {
            source: Some(JetHttpBodySource::Reader {
                reader: Box::new(reader),
                closer: Some(JetHttpBodyCloser::new(close)),
            }),
            length,
            content_type: None,
            drained,
        })) }
    }

    fn file(file: std::fs::File, length: usize) -> Self {
        let drained = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(length == 0));
        Self { state: std::sync::Arc::new(std::sync::Mutex::new(JetHttpBodyState {
            source: Some(JetHttpBodySource::File(std::io::Read::take(file, length as u64))),
            length: Some(length),
            content_type: None,
            drained,
        })) }
    }

    fn from_reader(reader: JetFileReader) -> Result<Self, JetHttpError> {
        Ok(Self::reader(reader.inner, None))
    }

    fn from_reader_with_length(reader: JetFileReader, length: i64) -> Result<Self, JetHttpError> {
        let length = usize::try_from(length).ok().filter(|length| *length <= 1024 * 1024 * 1024)
            .ok_or(JetHttpError::BodyTooLarge { limit: length })?;
        Ok(Self::reader(reader.inner, Some(length)))
    }

    fn bridge(
        handle: i64,
        length: Option<usize>,
        read: fn(i64, usize) -> Result<Option<Vec<u8>>, JetHttpError>,
        close: fn(i64),
    ) -> Self {
        let drained = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(length == Some(0)));
        let closer = JetHttpBodyCloser::new(move || close(handle));
        Self {
            state: std::sync::Arc::new(std::sync::Mutex::new(JetHttpBodyState {
                source: Some(JetHttpBodySource::Bridge {
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

    fn chunks(&self, max_chunk: usize) -> Result<JetHttpBodyChunks, JetHttpError> {
        if max_chunk == 0 {
            return Err(JetHttpError::BodyTooLarge { limit: 0 });
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| JetHttpError::Internal {
                incident_id: "http-body-lock".to_string(),
            })?;
        let source = state
            .source
            .take()
            .ok_or(JetHttpError::BodyConsumed)?;
        let drained = state.drained.clone();
        Ok(JetHttpBodyChunks {
            source,
            max_chunk,
            done: false,
            initial_error: None,
            drained,
        })
    }

    fn bytes(&self, limit: usize) -> Result<Vec<u8>, JetHttpError> {
        if self.length().is_some_and(|length| length > limit) {
            let _ = self.chunks(64 * 1024)?;
            return Err(JetHttpError::BodyTooLarge { limit: limit as i64 });
        }
        let mut bytes = Vec::new();
        for chunk in self.chunks(64 * 1024)? {
            let chunk = chunk?;
            if bytes.len().saturating_add(chunk.len()) > limit {
                return Err(JetHttpError::BodyTooLarge { limit: limit as i64 });
            }
            bytes.extend(chunk);
        }
        Ok(bytes)
    }

    fn text(&self, limit: usize) -> Result<String, JetHttpError> {
        String::from_utf8(self.bytes(limit)?).map_err(|_| JetHttpError::UnsupportedEncoding)
    }
}

struct JetHttpBodyChunks {
    source: JetHttpBodySource,
    max_chunk: usize,
    done: bool,
    initial_error: Option<JetHttpError>,
    drained: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl JetHttpBodyChunks {
    fn failed(error: JetHttpError) -> Self {
        Self {
            source: JetHttpBodySource::Bytes(std::io::Cursor::new(Vec::new())),
            max_chunk: 1,
            done: false,
            initial_error: Some(error),
            drained: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn h2_cancellable(&self) -> bool { self.source.h2_cancellable() }

    fn closer(&self) -> Option<std::sync::Arc<JetHttpBodyCloser>> { self.source.closer() }
}

impl Iterator for JetHttpBodyChunks {
    type Item = Result<Vec<u8>, JetHttpError>;

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
            JetHttpBodySource::Bytes(reader) => std::io::Read::read(reader, &mut chunk),
            JetHttpBodySource::File(file) => std::io::Read::read(file, &mut chunk),
            JetHttpBodySource::Reader { reader, .. } => reader.read(&mut chunk),
            JetHttpBodySource::Bridge { handle, read, .. } => match read(*handle, self.max_chunk) {
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
                    std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof => JetHttpError::InvalidFraming,
                    std::io::ErrorKind::OutOfMemory => JetHttpError::BodyTooLarge { limit: 32 * 1024 },
                    _ => JetHttpError::Io { operation: "read body".to_string() },
                }))
            }
        }
    }
}

impl Drop for JetHttpBodyChunks {
    fn drop(&mut self) { self.source.close(); }
}

impl std::fmt::Debug for JetHttpBody {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpBody")
            .field("length", &self.length())
            .finish_non_exhaustive()
    }
}

fn jet_http_body_bytes(body: &JetHttpBody, limit: i64) -> Result<Vec<u8>, JetHttpError> {
    let limit = jet_http_consume_limit(limit)?;
    body.bytes(limit)
}

fn jet_http_body_text(body: &JetHttpBody, limit: i64) -> Result<String, JetHttpError> {
    let limit = jet_http_consume_limit(limit)?;
    body.text(limit)
}

fn jet_http_body_json<T: user_Decode>(body: &JetHttpBody, limit: i64) -> Result<T, JetHttpError> {
    let text = jet_http_body_text(body, limit)?;
    jet_enc_json_decode(&text).map_err(|_| JetHttpError::InvalidFraming)
}

fn jet_http_body_copy_to(
    body: &JetHttpBody,
    writer: &mut JetFileWriter,
    limit: i64,
) -> Result<i64, JetHttpError> {
    use std::io::Write;
    let limit = jet_http_consume_limit(limit)?;
    if body.length().is_some_and(|length| length > limit) {
        let _ = body.chunks(64 * 1024)?;
        return Err(JetHttpError::BodyTooLarge { limit: limit as i64 });
    }
    let mut total = 0usize;
    for chunk in body.chunks(64 * 1024)? {
        let chunk = chunk?;
        total = total.checked_add(chunk.len()).ok_or(JetHttpError::BodyTooLarge { limit: limit as i64 })?;
        if total > limit {
            return Err(JetHttpError::BodyTooLarge { limit: limit as i64 });
        }
        writer.inner.write_all(&chunk).map_err(|_| JetHttpError::Io { operation: "copy body".to_string() })?;
    }
    i64::try_from(total).map_err(|_| JetHttpError::BodyTooLarge { limit: limit as i64 })
}

fn jet_http_consume_limit(limit: i64) -> Result<usize, JetHttpError> {
    if !(0..=1024 * 1024 * 1024).contains(&limit) {
        return Err(JetHttpError::BodyTooLarge { limit });
    }
    Ok(limit as usize)
}

fn jet_http_body_chunks(body: &JetHttpBody, max_chunk: i64) -> JetHttpBodyChunks {
    let max_chunk = usize::try_from(max_chunk)
        .ok()
        .filter(|limit| (1..=1024 * 1024 * 1024).contains(limit));
    match max_chunk {
        Some(max_chunk) => body.chunks(max_chunk).unwrap_or_else(JetHttpBodyChunks::failed),
        None => JetHttpBodyChunks::failed(JetHttpError::BodyTooLarge { limit: 0 }),
    }
}

impl PartialEq<&str> for JetHttpBody {
    fn eq(&self, expected: &&str) -> bool {
        self.text(1024 * 1024).as_deref() == Ok(*expected)
    }
}

impl PartialEq<String> for JetHttpBody {
    fn eq(&self, expected: &String) -> bool {
        self.text(1024 * 1024).as_ref() == Ok(expected)
    }
}

#[derive(Clone)]
struct JetHttpRequest {
    method: String,
    url: String,
    path: String,
    version: String,
    headers: JetHttpHeaders,
    body: JetHttpBody,
    body_set: bool,
    params: std::collections::BTreeMap<String, String>,
    route_template: Option<String>,
    header_error: Option<JetHttpError>,
    timeout_ms: Option<i64>,
    connect_timeout_ms: Option<i64>,
    read_timeout_ms: Option<i64>,
    total_timeout_ms: Option<i64>,
    redirects: Option<i64>,
    proxy: Option<String>,
    cookies: Vec<String>,
    form: Vec<String>,
    multipart: Vec<String>,
}

#[derive(Clone)]
struct JetHttpResponse {
    status: i64,
    version: String,
    headers: JetHttpHeaders,
    body: JetHttpBody,
    trailers: JetHttpHeaders,
    head_content_length: Option<usize>,
}

type JetHttpHandler = std::sync::Arc<
    dyn Fn(JetHttpRequest) -> Result<JetHttpResponse, JetHttpError> + Send + Sync,
>;

impl JetHttpRequest {
    fn server(method: &str, path: String, body: Vec<u8>, headers: JetHttpHeaders) -> Self {
        Self::server_body(method, path, JetHttpBody::from_bytes(body), headers)
    }

    fn server_body(
        method: &str,
        path: String,
        body: JetHttpBody,
        headers: JetHttpHeaders,
    ) -> Self {
        Self {
            method: method.to_string(),
            url: String::new(),
            path,
            version: "HTTP/1.1".to_string(),
            headers,
            body,
            body_set: true,
            params: std::collections::BTreeMap::new(),
            route_template: None,
            header_error: None,
            timeout_ms: None,
            connect_timeout_ms: None,
            read_timeout_ms: None,
            total_timeout_ms: None,
            redirects: None,
            proxy: None,
            cookies: Vec::new(),
            form: Vec::new(),
            multipart: Vec::new(),
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
struct JetHttpHeaders {
    entries: Vec<(String, String)>,
}

impl JetHttpHeaders {
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

fn jet_http_headers_first(headers: &JetHttpHeaders, name: &String) -> Option<String> {
    headers.get(name).cloned()
}

fn jet_http_headers_all(headers: &JetHttpHeaders, name: &String) -> Vec<String> {
    headers.all(name).into_iter().map(str::to_string).collect()
}

fn jet_http_headers_append(
    mut headers: JetHttpHeaders,
    name: &String,
    value: &String,
) -> Result<JetHttpHeaders, JetHttpError> {
    headers.append(name, value).map_err(|_| JetHttpError::InvalidHeader)?;
    Ok(headers)
}

fn jet_http_headers_set(
    mut headers: JetHttpHeaders,
    name: &String,
    value: &String,
) -> Result<JetHttpHeaders, JetHttpError> {
    headers.set(name, value).map_err(|_| JetHttpError::InvalidHeader)?;
    Ok(headers)
}

fn jet_http_headers_remove(mut headers: JetHttpHeaders, name: &String) -> JetHttpHeaders {
    headers.remove(name);
    headers
}

impl std::fmt::Debug for JetHttpHeaders {
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

impl FromIterator<(String, String)> for JetHttpHeaders {
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

impl<'a> IntoIterator for &'a JetHttpHeaders {
    type Item = &'a (String, String);
    type IntoIter = std::slice::Iter<'a, (String, String)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}
