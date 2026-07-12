// core.http.client bridge runtime (D-NETDEP1=A, D-HTTPLIB4=B, D-TLS1=A).
//
// Emitted into the hidden FFI bridge crate when a Jet program uses `core.http.client`.
// Cargo enables ureq's rustls + native-certs features for default HTTPS.
// All public functions use ONLY primitive types (String, i64, Vec<String>) so they are
// compatible with the main generated program without cross-crate struct sharing.
// JetHttpClientReq / JetHttpClientResp are defined in CoreLib.rs (embedded in the
// generated program) and never appear here.

use std::time::Duration;

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
    let default_timeout = timeout_ms.unwrap_or(30_000);
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(
            connect_timeout_ms.unwrap_or(default_timeout) as u64,
        ))
        .timeout_read(Duration::from_millis(
            read_timeout_ms.unwrap_or(default_timeout) as u64,
        ))
        .timeout_write(Duration::from_millis(default_timeout as u64))
        .try_proxy_from_env(true);
    if let Some(ms) = total_timeout_ms {
        builder = builder.timeout(Duration::from_millis(ms as u64));
    }
    if let Some(n) = redirects {
        builder = builder.redirects(n.max(0) as u32);
    }
    if let Some(p) = proxy {
        builder = builder.proxy(ureq::Proxy::new(p).map_err(|e| e.to_string())?);
    }
    let agent = builder.build();
    let mut req = agent.request(method, url.as_str());
    let mut i = 0;
    while i + 1 < headers_flat.len() {
        req = req.set(&headers_flat[i], &headers_flat[i + 1]);
        i += 2;
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
        let boundary = "jet-http-boundary";
        multipart_body = encode_multipart(multipart_flat, boundary);
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
            let known = [
                "content-type",
                "content-length",
                "location",
                "server",
                "date",
                "cache-control",
                "etag",
                "last-modified",
                "set-cookie",
                "vary",
                "access-control-allow-origin",
                "x-request-id",
                "authorization",
                "content-encoding",
            ];
            let mut flat: Vec<String> = Vec::new();
            for name in &known {
                if let Some(v) = resp.header(name) {
                    flat.push(name.to_string());
                    flat.push(v.to_string());
                }
            }
            let body = resp.into_string().unwrap_or_default();
            Ok((status, body, flat))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            Ok((code as i64, body, vec![]))
        }
        Err(e) => Err(e.to_string()),
    }
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

fn encode_multipart(fields: &[String], boundary: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i + 1 < fields.len() {
        out.push_str("--");
        out.push_str(boundary);
        out.push_str("\r\nContent-Disposition: form-data; name=\"");
        out.push_str(&fields[i].replace('"', "%22"));
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
