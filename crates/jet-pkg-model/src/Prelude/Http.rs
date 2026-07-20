// core.http.client bridge runtime (D-NETDEP1=A, D-HTTPLIB4=B, D-TLS1=A).
//
// Emitted into the hidden FFI bridge crate when a Jet program uses `core.http.client`.
// Cargo enables ureq's rustls + native-certs features for default HTTPS.
// All public functions use ONLY primitive types (String, i64, Vec<String>) so they are
// compatible with the main generated program without cross-crate struct sharing.
// JetHttpClientReq / JetHttpClientResp are defined in CoreLib.rs (embedded in the
// generated program) and never appear here.

use std::io::Read;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const HTTP_CLIENT_DEFAULT_REDIRECTS: u32 = 10;

/// Private, typed transport failures. Generated code exhaustively projects these
/// to the public closed HttpError without carrying backend prose across the seam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetHttpBridgeError {
    InvalidUrl,
    InvalidHeader,
    InvalidFraming,
    Resolve,
    Connect,
    Tls,
    Timeout,
    Proxy,
    Redirect,
    Protocol,
    Io,
    ResourceUnavailable,
    Cancelled,
    Internal,
}

type BodyReader = Arc<Mutex<Box<dyn Read + Send>>>;

fn body_readers() -> &'static Mutex<std::collections::HashMap<i64, BodyReader>> {
    static READERS: OnceLock<Mutex<std::collections::HashMap<i64, BodyReader>>> = OnceLock::new();
    READERS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn register_body(reader: Box<dyn Read + Send>) -> i64 {
    static NEXT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);
    let handle = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    body_readers()
        .lock()
        .expect("HTTP body registry lock")
        .insert(handle, Arc::new(Mutex::new(reader)));
    handle
}

pub fn jet_http_client_body_read_impl(
    handle: i64,
    max_chunk: usize,
) -> Result<Option<Vec<u8>>, JetHttpBridgeError> {
    let reader = body_readers()
        .lock()
        .map_err(|_| JetHttpBridgeError::Internal)?
        .get(&handle)
        .cloned()
        .ok_or(JetHttpBridgeError::Internal)?;
    let mut chunk = vec![0; max_chunk];
    let read = reader
        .lock()
        .map_err(|_| JetHttpBridgeError::Internal)?
        .read(&mut chunk)
        .map_err(|_| JetHttpBridgeError::Io)?;
    if read == 0 {
        let _ = body_readers().lock().map(|mut readers| readers.remove(&handle));
        return Ok(None);
    }
    chunk.truncate(read);
    Ok(Some(chunk))
}

pub fn jet_http_client_body_close_impl(handle: i64) {
    let _ = body_readers().lock().map(|mut readers| readers.remove(&handle));
}

fn validated_timeout(name: &str, milliseconds: i64) -> Result<Duration, JetHttpBridgeError> {
    let milliseconds = u64::try_from(milliseconds)
        .map_err(|_| {
            let _ = name;
            JetHttpBridgeError::Timeout
        })?;
    Ok(Duration::from_millis(milliseconds))
}

/// Perform an HTTP GET. Returns (status_code, body, headers_flat) where headers_flat
/// is alternating [key, value, key, value, ...].
pub fn jet_http_client_get_impl(url: &String) -> Result<(i64, i64, Option<i64>, Vec<String>), JetHttpBridgeError> {
    jet_http_client_send_impl("GET", url, &[], None, None, None, None, None, None, None, &[], &[], &[])
}

/// Perform an HTTP POST with a string body.
pub fn jet_http_client_post_impl(
    url: &String,
    body: &String,
) -> Result<(i64, i64, Option<i64>, Vec<String>), JetHttpBridgeError> {
    jet_http_client_send_impl(
        "POST",
        url,
        &[],
        Some(body.as_bytes()),
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
        &[],
        &[],
    )
}

/// Perform a generic HTTP request.
/// headers_flat: alternating [key, value, key, value, ...]
pub fn jet_http_client_send_impl(
    method: &str,
    url: &String,
    headers_flat: &[String],
    body: Option<&[u8]>,
    timeout_ms: Option<i64>,
    connect_timeout_ms: Option<i64>,
    read_timeout_ms: Option<i64>,
    total_timeout_ms: Option<i64>,
    redirects: Option<i64>,
    proxy: Option<&str>,
    cookies_flat: &[String],
    form_flat: &[String],
    multipart_flat: &[String],
) -> Result<(i64, i64, Option<i64>, Vec<String>), JetHttpBridgeError> {
    let default_timeout = validated_timeout("timeout", timeout_ms.unwrap_or(30_000))?;
    let connect_timeout = connect_timeout_ms
        .map(|milliseconds| validated_timeout("connect timeout", milliseconds))
        .transpose()?
        .unwrap_or(default_timeout);
    let read_timeout = read_timeout_ms
        .map(|milliseconds| validated_timeout("read timeout", milliseconds))
        .transpose()?
        .unwrap_or(default_timeout);
    let total_timeout = total_timeout_ms
        .map(|milliseconds| validated_timeout("total timeout", milliseconds))
        .transpose()?;
    let redirects = redirects
        .map(|limit| {
            u32::try_from(limit).map_err(|_| JetHttpBridgeError::Redirect)
        })
        .transpose()?;
    let redirect_limit = redirects.unwrap_or(HTTP_CLIENT_DEFAULT_REDIRECTS);
    let backend_redirect_limit = redirects.unwrap_or(HTTP_CLIENT_DEFAULT_REDIRECTS + 1);
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(connect_timeout)
        .timeout_read(read_timeout)
        .timeout_write(default_timeout)
        .try_proxy_from_env(true)
        // ureq errors when its count reaches this value, so Jet's ten follows need eleven.
        .redirects(backend_redirect_limit);
    if let Some(timeout) = total_timeout {
        builder = builder.timeout(timeout);
    }
    if let Some(p) = proxy {
        builder = builder.proxy(
            ureq::Proxy::new(p).map_err(|_| JetHttpBridgeError::Proxy)?,
        );
    }
    let agent = builder.build();
    let mut req = agent.request(method, url.as_str());
    for (name, value) in coalesce_request_headers(headers_flat) {
        req = req.set(&name, &value);
    }
    if !cookies_flat.is_empty() {
        let mut cookie = String::new();
        let mut i = 0;
        while i + 1 < cookies_flat.len() {
            if !cookie.is_empty() {
                cookie.push_str("; ");
            }
            cookie.push_str(&cookies_flat[i]);
            cookie.push('=');
            cookie.push_str(&cookies_flat[i + 1]);
            i += 2;
        }
        req = req.set("cookie", &cookie);
    }
    let form_body;
    let multipart_body;
    let result = if let Some(b) = body {
        req.send_bytes(b)
    } else if !multipart_flat.is_empty() {
        let boundary = multipart_boundary(multipart_flat);
        multipart_body = encode_multipart(multipart_flat, &boundary);
        req.set(
            "content-type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send_string(&multipart_body)
    } else if !form_flat.is_empty() {
        form_body = encode_form(form_flat);
        req.set("content-type", "application/x-www-form-urlencoded")
            .send_string(&form_body)
    } else {
        req.call()
    };
    match result {
        Ok(resp) => {
            let status = resp.status() as i64;
            let flat = flatten_response_headers(&resp);
            Ok(response_parts(status, resp, flat))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let flat = flatten_response_headers(&resp);
            Ok(response_parts(code as i64, resp, flat))
        }
        Err(ureq::Error::Transport(error))
            if matches!(
                error.kind(),
                ureq::ErrorKind::InvalidUrl | ureq::ErrorKind::UnknownScheme
            ) =>
        {
            Err(JetHttpBridgeError::InvalidUrl)
        }
        Err(ureq::Error::Transport(error)) if error.kind() == ureq::ErrorKind::ProxyConnect => {
            Err(JetHttpBridgeError::Proxy)
        }
        Err(ureq::Error::Transport(error))
            if error.kind() == ureq::ErrorKind::ProxyUnauthorized =>
        {
            Err(JetHttpBridgeError::Proxy)
        }
        Err(ureq::Error::Transport(error))
            if error.kind() == ureq::ErrorKind::ConnectionFailed =>
        {
            Err(JetHttpBridgeError::Connect)
        }
        Err(ureq::Error::Transport(error)) if error.kind() == ureq::ErrorKind::Io => {
            Err(JetHttpBridgeError::Io)
        }
        Err(ureq::Error::Transport(error))
            if matches!(
                error.kind(),
                ureq::ErrorKind::BadStatus | ureq::ErrorKind::BadHeader
            ) =>
        {
            Err(if error.kind() == ureq::ErrorKind::BadHeader {
                JetHttpBridgeError::InvalidHeader
            } else {
                JetHttpBridgeError::InvalidFraming
            })
        }
        Err(ureq::Error::Transport(error))
            if error.kind() == ureq::ErrorKind::TooManyRedirects =>
        {
            let _ = redirect_limit;
            Err(JetHttpBridgeError::Redirect)
        }
        Err(ureq::Error::Transport(error)) => Err(match error.kind() {
            ureq::ErrorKind::Dns => JetHttpBridgeError::Resolve,
            ureq::ErrorKind::ConnectionFailed => JetHttpBridgeError::Connect,
            ureq::ErrorKind::Io => JetHttpBridgeError::Io,
            ureq::ErrorKind::InvalidUrl | ureq::ErrorKind::UnknownScheme => {
                JetHttpBridgeError::InvalidUrl
            }
            ureq::ErrorKind::ProxyConnect | ureq::ErrorKind::ProxyUnauthorized => {
                JetHttpBridgeError::Proxy
            }
            ureq::ErrorKind::TooManyRedirects => JetHttpBridgeError::Redirect,
            ureq::ErrorKind::BadStatus => JetHttpBridgeError::InvalidFraming,
            ureq::ErrorKind::BadHeader => JetHttpBridgeError::InvalidHeader,
            _ => JetHttpBridgeError::Internal,
        }),
        Err(ureq::Error::Status(_, _)) => unreachable!("status responses are handled above"),
    }
}

fn response_parts(
    status: i64,
    response: ureq::Response,
    headers: Vec<String>,
) -> (i64, i64, Option<i64>, Vec<String>) {
    let length = response
        .header("content-length")
        .and_then(|value| value.parse::<i64>().ok());
    let handle = register_body(response.into_reader());
    (status, handle, length, headers)
}

fn flatten_response_headers(response: &ureq::Response) -> Vec<String> {
    let mut flat = Vec::new();
    let mut seen = std::collections::HashMap::<String, usize>::new();
    for name in response.headers_names() {
        let index = seen.entry(name.clone()).or_default();
        if let Some(value) = response.all(&name).get(*index) {
            flat.push(name.clone());
            flat.push(value.to_string());
        }
        *index += 1;
    }
    flat
}

fn coalesce_request_headers(flat: &[String]) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    for pair in flat.chunks_exact(2) {
        if let Some((_, value)) = headers
            .iter_mut()
            .find(|(name, _)| name.eq_ignore_ascii_case(&pair[0]))
        {
            value.push_str(if pair[0].eq_ignore_ascii_case("cookie") {
                "; "
            } else {
                ", "
            });
            value.push_str(&pair[1]);
        } else {
            headers.push((pair[0].clone(), pair[1].clone()));
        }
    }
    headers
}

fn encode_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn encode_form(fields: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i + 1 < fields.len() {
        if !out.is_empty() {
            out.push('&');
        }
        out.push_str(&encode_component(&fields[i]));
        out.push('=');
        out.push_str(&encode_component(&fields[i + 1]));
        i += 2;
    }
    out
}

fn multipart_boundary(fields: &[String]) -> String {
    const PREFIX: &str = "jet-http-boundary-";
    const LENGTH: usize = PREFIX.len() + 16;

    let mut used = std::collections::HashSet::new();
    for field in fields {
        for window in field.as_bytes().windows(LENGTH) {
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
    format!("{PREFIX}{suffix:016x}")
}

fn encode_multipart(fields: &[String], boundary: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i + 1 < fields.len() {
        out.push_str("--");
        out.push_str(boundary);
        out.push_str("\r\nContent-Disposition: form-data; name=\"");
        out.push_str(
            &fields[i]
                .replace('"', "%22")
                .replace('\r', "%0D")
                .replace('\n', "%0A"),
        );
        out.push_str("\"\r\n\r\n");
        out.push_str(&fields[i + 1]);
        out.push_str("\r\n");
        i += 2;
    }
    out.push_str("--");
    out.push_str(boundary);
    out.push_str("--\r\n");
    out
}
