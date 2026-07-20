// core.http.client bridge runtime (D-NETDEP1=A, D-HTTPLIB4=B, D-TLS1=A).
//
// Emitted into the hidden FFI bridge crate when a Jet program uses `core.http.client`.
// Cargo enables ureq's rustls + native-certs features for default HTTPS.
// All public functions use ONLY primitive types (String, i64, Vec<String>) so they are
// compatible with the main generated program without cross-crate struct sharing.
// JetHttpClientReq / JetHttpClientResp are defined in CoreLib.rs (embedded in the
// generated program) and never appear here.

use std::time::Duration;

const HTTP_CLIENT_TEXT_LIMIT: usize = 8 * 1024 * 1024;
const HTTP_CLIENT_READ_CHUNK: usize = 64 * 1024;
const HTTP_CLIENT_DEFAULT_REDIRECTS: u32 = 10;

fn validated_timeout(name: &str, milliseconds: i64) -> Result<Duration, String> {
    let milliseconds = u64::try_from(milliseconds)
        .map_err(|_| format!("HTTP {name} must be non-negative"))?;
    Ok(Duration::from_millis(milliseconds))
}

/// Perform an HTTP GET. Returns (status_code, body, headers_flat) where headers_flat
/// is alternating [key, value, key, value, ...].
pub fn jet_http_client_get_impl(url: &String) -> Result<(i64, String, Vec<String>), String> {
    jet_http_client_send_impl("GET", url, &[], None, None, None, None, None, None, None, &[], &[], &[])
}

/// Perform an HTTP POST with a string body.
pub fn jet_http_client_post_impl(
    url: &String,
    body: &String,
) -> Result<(i64, String, Vec<String>), String> {
    jet_http_client_send_impl(
        "POST",
        url,
        &[],
        Some(body.as_str()),
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
    body: Option<&str>,
    timeout_ms: Option<i64>,
    connect_timeout_ms: Option<i64>,
    read_timeout_ms: Option<i64>,
    total_timeout_ms: Option<i64>,
    redirects: Option<i64>,
    proxy: Option<&str>,
    cookies_flat: &[String],
    form_flat: &[String],
    multipart_flat: &[String],
) -> Result<(i64, String, Vec<String>), String> {
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
            u32::try_from(limit).map_err(|_| {
                "HTTP redirect limit must be between 0 and 4294967295".to_string()
            })
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
            ureq::Proxy::new(p).map_err(|_| "HTTP proxy URL is invalid".to_string())?,
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
        req.send_string(b)
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
            let body = read_response_text(resp)?;
            Ok((status, body, flat))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let flat = flatten_response_headers(&resp);
            let body = read_response_text(resp)?;
            Ok((code as i64, body, flat))
        }
        Err(ureq::Error::Transport(error))
            if matches!(
                error.kind(),
                ureq::ErrorKind::InvalidUrl | ureq::ErrorKind::UnknownScheme
            ) =>
        {
            Err("HTTP URL is invalid".to_string())
        }
        Err(ureq::Error::Transport(error))
            if error.kind() == ureq::ErrorKind::TooManyRedirects =>
        {
            Err(format!("HTTP redirect limit {redirect_limit} exceeded"))
        }
        Err(e) => Err(e.to_string()),
    }
}

fn read_response_text(response: ureq::Response) -> Result<String, String> {
    use std::io::Read;

    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    let mut chunk = [0u8; HTTP_CLIENT_READ_CHUNK];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|_| "HTTP response body read failed".to_string())?;
        if read == 0 {
            break;
        }
        if bytes.len() + read > HTTP_CLIENT_TEXT_LIMIT {
            return Err(format!(
                "HTTP response body exceeds {HTTP_CLIENT_TEXT_LIMIT}-byte limit"
            ));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8(bytes).map_err(|_| "HTTP response body is not valid UTF-8".to_string())
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
