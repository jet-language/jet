// core.http.client bridge runtime (D-NETDEP1=A, D-HTTPLIB4=B).
//
// Emitted into the hidden FFI bridge crate when a Jet program uses `core.http.client`.
// All public functions use ONLY primitive types (String, i64, Vec<String>) so they are
// compatible with the main generated program without cross-crate struct sharing.
// JetHttpClientReq / JetHttpClientResp are defined in CoreLib.rs (embedded in the
// generated program) and never appear here.

use std::time::Duration;

/// Perform an HTTP GET. Returns (status_code, body, headers_flat) where headers_flat
/// is alternating [key, value, key, value, ...].
pub fn jet_http_client_get_impl(url: &String) -> Result<(i64, String, Vec<String>), String> {
    jet_http_client_send_impl("GET", url, &[], None, None)
}

/// Perform an HTTP POST with a string body.
pub fn jet_http_client_post_impl(url: &String, body: &String) -> Result<(i64, String, Vec<String>), String> {
    jet_http_client_send_impl("POST", url, &[], Some(body.as_str()), None)
}

/// Perform a generic HTTP request.
/// headers_flat: alternating [key, value, key, value, ...]
pub fn jet_http_client_send_impl(
    method: &str,
    url: &String,
    headers_flat: &[String],
    body: Option<&str>,
    timeout_ms: Option<i64>,
) -> Result<(i64, String, Vec<String>), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(timeout_ms.unwrap_or(30_000) as u64))
        .timeout_read(Duration::from_millis(timeout_ms.unwrap_or(30_000) as u64))
        .build();
    let mut req = agent.request(method, url.as_str());
    let mut i = 0;
    while i + 1 < headers_flat.len() {
        req = req.set(&headers_flat[i], &headers_flat[i + 1]);
        i += 2;
    }
    let result = if let Some(b) = body {
        req.send_string(b)
    } else {
        req.call()
    };
    match result {
        Ok(resp) => {
            let status = resp.status() as i64;
            let known = ["content-type", "content-length", "location", "server", "date",
                         "cache-control", "etag", "last-modified", "set-cookie", "vary",
                         "access-control-allow-origin", "x-request-id", "authorization"];
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
