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
    BodyTooLarge { limit: usize },
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

impl JetHttpError {
    fn from_bridge(message: String) -> Self {
        if message.contains("URL is invalid") {
            Self::InvalidUrl
        } else if message.contains("header") || message.contains("framing") {
            Self::InvalidFraming
        } else if message.contains("UTF-8") {
            Self::UnsupportedEncoding
        } else if let Some(limit) = message
            .strip_prefix("HTTP response body exceeds ")
            .and_then(|rest| rest.strip_suffix("-byte limit"))
            .and_then(|limit| limit.parse().ok())
        {
            Self::BodyTooLarge { limit }
        } else if message.contains("proxy") {
            Self::Proxy { stage: message }
        } else if message.contains("redirect") {
            Self::Redirect { reason: message }
        } else if message.contains("timeout") {
            Self::Timeout { phase: message }
        } else if message.contains("connection") {
            Self::Connect { address: "<redacted>".to_string() }
        } else {
            Self::Io { operation: "HTTP transport".to_string() }
        }
    }
}

enum JetHttpBodySource {
    Reader(Box<dyn std::io::Read + Send>),
}

struct JetHttpBodyState {
    source: Option<JetHttpBodySource>,
    length: Option<usize>,
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
        let length = bytes.len();
        Self::reader(std::io::Cursor::new(bytes), Some(length))
    }

    fn from_text(text: String) -> Self {
        Self::from_bytes(text.into_bytes())
    }

    fn reader(reader: impl std::io::Read + Send + 'static, length: Option<usize>) -> Self {
        Self {
            state: std::sync::Arc::new(std::sync::Mutex::new(JetHttpBodyState {
                source: Some(JetHttpBodySource::Reader(Box::new(reader))),
                length,
            })),
        }
    }

    fn length(&self) -> Option<usize> {
        self.state.lock().ok().and_then(|state| state.length)
    }

    fn len(&self) -> usize {
        self.length().unwrap_or(0)
    }

    fn is_empty(&self) -> bool {
        self.length() == Some(0)
    }

    fn chunks(&self, max_chunk: usize) -> Result<JetHttpBodyChunks, JetHttpError> {
        if max_chunk == 0 {
            return Err(JetHttpError::BodyTooLarge { limit: 0 });
        }
        let source = self
            .state
            .lock()
            .map_err(|_| JetHttpError::Internal {
                incident_id: "http-body-lock".to_string(),
            })?
            .source
            .take()
            .ok_or(JetHttpError::BodyConsumed)?;
        Ok(JetHttpBodyChunks { source, max_chunk, done: false })
    }

    fn bytes(&self, limit: usize) -> Result<Vec<u8>, JetHttpError> {
        if self.length().is_some_and(|length| length > limit) {
            let _ = self.chunks(64 * 1024)?;
            return Err(JetHttpError::BodyTooLarge { limit });
        }
        let mut bytes = Vec::new();
        for chunk in self.chunks(64 * 1024)? {
            let chunk = chunk?;
            if bytes.len().saturating_add(chunk.len()) > limit {
                return Err(JetHttpError::BodyTooLarge { limit });
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
}

impl Iterator for JetHttpBodyChunks {
    type Item = Result<Vec<u8>, JetHttpError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let mut chunk = vec![0; self.max_chunk];
        let read = match &mut self.source {
            JetHttpBodySource::Reader(reader) => reader.read(&mut chunk),
        };
        match read {
            Ok(0) => {
                self.done = true;
                None
            }
            Ok(read) => {
                chunk.truncate(read);
                Some(Ok(chunk))
            }
            Err(_) => {
                self.done = true;
                Some(Err(JetHttpError::Io {
                    operation: "read body".to_string(),
                }))
            }
        }
    }
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
    let limit = usize::try_from(limit).map_err(|_| JetHttpError::BodyTooLarge { limit: 0 })?;
    body.bytes(limit)
}

fn jet_http_body_text(body: &JetHttpBody, limit: i64) -> Result<String, JetHttpError> {
    let limit = usize::try_from(limit).map_err(|_| JetHttpError::BodyTooLarge { limit: 0 })?;
    body.text(limit)
}

fn jet_http_body_chunks(body: &JetHttpBody, max_chunk: i64) -> Result<Vec<Vec<u8>>, JetHttpError> {
    let max_chunk = usize::try_from(max_chunk).map_err(|_| JetHttpError::BodyTooLarge { limit: 0 })?;
    body.chunks(max_chunk)?.collect()
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

impl JetHttpRequest {
    fn server(method: &str, path: String, body: Vec<u8>, headers: JetHttpHeaders) -> Self {
        Self {
            method: method.to_string(),
            url: String::new(),
            path,
            version: "HTTP/1.1".to_string(),
            headers,
            body: JetHttpBody::from_bytes(body),
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

type JetHttpClientReq = JetHttpRequest;
type JetHttpSrvReq = JetHttpRequest;
type JetHttpClientResp = JetHttpResponse;
type JetHttpSrvResp = JetHttpResponse;

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
