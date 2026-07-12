// ── D-HTTPLIB1=A / D-HTTPLIB2=B: core.http.server — function-first mux ───────
// Plain HTTP is pure std. D-TLSSERVE1=A routes server TLS through the hidden
// rustls bridge only when the named `tls:` option is used.

#[derive(Clone)]
struct JetHttpSrvResp {
    status: i64,
    body: String,
    headers: std::collections::BTreeMap<String, String>,
}

#[derive(Clone)]
struct JetHttpSrvReq {
    method: String,
    path: String,
    params: std::collections::BTreeMap<String, String>,
    body: String,
    headers: std::collections::BTreeMap<String, String>,
}

type JetHttpMuxHandlerFn = std::sync::Arc<dyn Fn(JetHttpSrvReq) -> JetHttpSrvResp + Send + Sync>;

struct JetHttpMuxRoute {
    method: String,
    pattern: String,
    handler: JetHttpMuxHandlerFn,
}

#[derive(Clone)]
struct JetHttpMux(std::sync::Arc<std::sync::Mutex<Vec<JetHttpMuxRoute>>>);

#[derive(Clone)]
struct JetHttpServerTls {
    cert_pem: String,
    key_pem: String,
}

struct JetHttpReadError {
    status: i64,
    message: &'static str,
}

impl std::fmt::Display for JetHttpReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl JetHttpMux {
    fn new() -> Self {
        JetHttpMux(std::sync::Arc::new(std::sync::Mutex::new(Vec::new())))
    }
    fn add<F>(&self, method: &str, pattern: &str, f: F)
    where
        F: Fn(JetHttpSrvReq) -> JetHttpSrvResp + Send + Sync + 'static,
    {
        self.0.lock().unwrap().push(JetHttpMuxRoute {
            method: method.to_uppercase(),
            pattern: pattern.to_string(),
            handler: std::sync::Arc::new(f) as JetHttpMuxHandlerFn,
        });
    }
}

fn jet_http_mux_new() -> JetHttpMux {
    JetHttpMux::new()
}

fn jet_http_srv_tls(cert_pem: &String, key_pem: &String) -> JetHttpServerTls {
    JetHttpServerTls {
        cert_pem: cert_pem.clone(),
        key_pem: key_pem.clone(),
    }
}

fn jet_http_mux_add<F>(mux: &JetHttpMux, method: &str, pattern: &str, f: F)
where
    F: Fn(JetHttpSrvReq) -> JetHttpSrvResp + Send + Sync + 'static,
{
    mux.add(method, pattern, f);
}

fn jet_http_srv_response(status: i64, body: &String) -> JetHttpSrvResp {
    JetHttpSrvResp {
        status,
        body: body.clone(),
        headers: std::collections::BTreeMap::new(),
    }
}

fn jet_http_srv_response_header(
    mut resp: JetHttpSrvResp,
    name: &String,
    value: &String,
) -> JetHttpSrvResp {
    resp.headers.insert(name.clone(), value.clone());
    resp
}

fn jet_http_mux_serve(addr: &String, mux: JetHttpMux) -> Result<(), String> {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(addr.as_str())
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))?;
    let mux = std::sync::Arc::new(mux);
    loop {
        let (mut stream, _peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => {
                eprintln!("http accept failed: {}", e);
                continue;
            }
        };
        let m = mux.clone();
        std::thread::spawn(move || {
            let raw = match jet_http_srv_read(&mut stream) {
                Ok(raw) => raw,
                Err(e) => {
                    let _ = stream.write_all(jet_http_srv_read_error_response(&e).as_bytes());
                    return;
                }
            };
            let req = jet_http_srv_parse(&raw);
            let resp = jet_http_mux_dispatch(&m, req);
            let text = jet_http_srv_format(&resp);
            let _ = stream.write_all(text.as_bytes());
        });
    }
}

fn jet_http_mux_serve_once(addr: &String, mux: JetHttpMux) -> Result<(), String> {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(addr.as_str())
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))?;
    jet_http_mux_serve_once_listener(&JetTcpListener { inner: listener }, &mux)
}

fn jet_http_mux_serve_once_listener(
    listener: &JetTcpListener,
    mux: &JetHttpMux,
) -> Result<(), String> {
    use std::io::{Read, Write};
    let (mut stream, _peer) = listener
        .inner
        .accept()
        .map_err(|e| format!("accept failed: {}", e))?;
    let raw = match jet_http_srv_read(&mut stream) {
        Ok(raw) => raw,
        Err(error) => {
            stream
                .write_all(jet_http_srv_read_error_response(&error).as_bytes())
                .map_err(|e| format!("http write failed: {}", e))?;
            return Ok(());
        }
    };
    let req = jet_http_srv_parse(&raw);
    let resp = jet_http_mux_dispatch(mux, req);
    let text = jet_http_srv_format(&resp);
    stream
        .write_all(text.as_bytes())
        .map_err(|e| format!("http write failed: {}", e))
}

fn jet_http_srv_read(stream: &mut std::net::TcpStream) -> Result<String, JetHttpReadError> {
    use std::io::Read;
    const MAX_HEADER_BYTES: usize = 32 * 1024;
    const MAX_BODY_BYTES: usize = 1024 * 1024;
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    let mut complete = false;
    loop {
        let n = stream
            .read(&mut buf)
            .map_err(|_| JetHttpReadError { status: 400, message: "request read failed" })?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        if let Some(header_end) = jet_http_header_end(&raw) {
            if header_end > MAX_HEADER_BYTES {
                return Err(JetHttpReadError { status: 431, message: "request headers are too large" });
            }
            let content_len = jet_http_validate_headers(&raw[..header_end])?;
            if content_len > MAX_BODY_BYTES {
                return Err(JetHttpReadError { status: 413, message: "request body is too large" });
            }
            let body_start = header_end + 4;
            if raw.len().saturating_sub(body_start) >= content_len {
                raw.truncate(body_start + content_len);
                complete = true;
                break;
            }
        } else if raw.len() > MAX_HEADER_BYTES {
            return Err(JetHttpReadError { status: 431, message: "request headers are too large" });
        }
    }
    if !complete {
        return Err(JetHttpReadError {
            status: 400,
            message: "request ended before its declared framing was complete",
        });
    }
    String::from_utf8(raw).map_err(|_| JetHttpReadError { status: 400, message: "request is not valid UTF-8" })
}

fn jet_http_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n")
}

fn jet_http_validate_headers(header: &[u8]) -> Result<usize, JetHttpReadError> {
    let text = std::str::from_utf8(header)
        .map_err(|_| JetHttpReadError { status: 400, message: "request headers are not valid UTF-8" })?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    if request_line.len() > 8 * 1024 || request_line.split(' ').count() != 3 {
        return Err(JetHttpReadError { status: 400, message: "request line is malformed" });
    }
    let mut count = 0usize;
    let mut content_length = None;
    let mut has_transfer_encoding = false;
    for line in lines {
        count += 1;
        if count > 100 {
            return Err(JetHttpReadError { status: 431, message: "request has too many headers" });
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            return Err(JetHttpReadError { status: 400, message: "folded request headers are not allowed" });
        }
        let (name, value) = line.split_once(':')
            .ok_or(JetHttpReadError { status: 400, message: "request header is malformed" })?;
        if name.is_empty() || name.ends_with(' ') || name.ends_with('\t') {
            return Err(JetHttpReadError { status: 400, message: "request header name is malformed" });
        }
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = value.trim().parse::<usize>()
                .map_err(|_| JetHttpReadError { status: 400, message: "content-length is malformed" })?;
            if content_length.replace(parsed).is_some_and(|old| old != parsed) {
                return Err(JetHttpReadError { status: 400, message: "conflicting content-length headers" });
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            has_transfer_encoding = true;
        }
    }
    if has_transfer_encoding && content_length.is_some() {
        return Err(JetHttpReadError { status: 400, message: "content-length and transfer-encoding cannot be combined" });
    }
    if has_transfer_encoding {
        return Err(JetHttpReadError { status: 400, message: "transfer-encoding is not supported" });
    }
    Ok(content_length.unwrap_or(0))
}

fn jet_http_srv_read_error_response(error: &JetHttpReadError) -> String {
    let reason = match error.status {
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        _ => "Bad Request",
    };
    format!(
        "HTTP/1.1 {} {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        error.status, reason
    )
}

fn jet_http_mux_serve_tls<V, H>(
    addr: &String,
    mux: JetHttpMux,
    tls: JetHttpServerTls,
    validate: V,
    handle: H,
) -> Result<(), String>
where
    V: Fn(&String, &String) -> Result<(), String>,
    H: Fn(
            &String,
            &String,
            std::net::TcpStream,
            Box<dyn FnOnce(String) -> String + Send>,
        ) -> Result<(), String>
        + Clone
        + Send
        + Sync
        + 'static,
{
    validate(&tls.cert_pem, &tls.key_pem)?;
    let listener = std::net::TcpListener::bind(addr.as_str())
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))?;
    let mux = std::sync::Arc::new(mux);
    loop {
        let (stream, _peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => {
                eprintln!("http TLS accept failed: {}", e);
                continue;
            }
        };
        let m = mux.clone();
        let tls_cfg = tls.clone();
        let handle_one = handle.clone();
        std::thread::spawn(move || {
            let dispatch = Box::new(move |raw: String| {
                let req = jet_http_srv_parse(&raw);
                let resp = jet_http_mux_dispatch(&m, req);
                jet_http_srv_format(&resp)
            });
            if let Err(e) = handle_one(&tls_cfg.cert_pem, &tls_cfg.key_pem, stream, dispatch) {
                eprintln!("http TLS connection failed: {}", e);
            }
        });
    }
}

fn jet_http_srv_parse(raw: &str) -> JetHttpSrvReq {
    let sep = raw.find("\r\n\r\n").unwrap_or(raw.len());
    let header_part = &raw[..sep];
    let body = if sep + 4 <= raw.len() {
        raw[sep + 4..].to_string()
    } else {
        String::new()
    };
    let mut lines = header_part.lines();
    let req_line = lines.next().unwrap_or("GET / HTTP/1.1");
    let mut parts = req_line.splitn(3, ' ');
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(": ") {
            headers.insert(k.to_lowercase(), v.to_string());
        }
    }
    JetHttpSrvReq {
        method,
        path,
        params: std::collections::BTreeMap::new(),
        body,
        headers,
    }
}

fn jet_http_mux_dispatch(mux: &JetHttpMux, req: JetHttpSrvReq) -> JetHttpSrvResp {
    let routes = mux.0.lock().unwrap();
    for route in routes.iter() {
        if route.method != req.method {
            continue;
        }
        if let Some(params) = jet_http_match_path(&route.pattern, &req.path) {
            let mut r2 = req.clone();
            r2.params = params;
            return (route.handler)(r2);
        }
    }
    JetHttpSrvResp {
        status: 404,
        body: "404 Not Found".to_string(),
        headers: std::collections::BTreeMap::new(),
    }
}

fn jet_http_match_path(
    pattern: &str,
    path: &str,
) -> Option<std::collections::BTreeMap<String, String>> {
    let p_segs: Vec<&str> = pattern.split('/').collect();
    let r_segs: Vec<&str> = path.split('?').next().unwrap_or(path).split('/').collect();
    if p_segs.last() == Some(&"*") && r_segs.len() >= p_segs.len() {
        let mut params = std::collections::BTreeMap::new();
        for (p, r) in p_segs[..p_segs.len() - 1]
            .iter()
            .zip(r_segs[..p_segs.len() - 1].iter())
        {
            if let Some(key) = p.strip_prefix(':') {
                params.insert(key.to_string(), r.to_string());
            } else if *p != *r {
                return None;
            }
        }
        params.insert("wildcard".to_string(), r_segs[p_segs.len() - 1..].join("/"));
        return Some(params);
    }
    if p_segs.len() != r_segs.len() {
        return None;
    }
    let mut params = std::collections::BTreeMap::new();
    for (p, r) in p_segs.iter().zip(r_segs.iter()) {
        if let Some(key) = p.strip_prefix(':') {
            params.insert(key.to_string(), r.to_string());
        } else if *p != *r {
            return None;
        }
    }
    Some(params)
}

fn jet_http_srv_format(resp: &JetHttpSrvResp) -> String {
    let reason = match resp.status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let mut out = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        resp.status,
        reason,
        resp.body.len()
    );
    for (k, v) in &resp.headers {
        out.push_str(&format!("{}: {}\r\n", k, v));
    }
    out.push_str("\r\n");
    out.push_str(&resp.body);
    out
}

fn jet_http_srv_req_method(req: &JetHttpSrvReq) -> String {
    req.method.clone()
}
fn jet_http_srv_req_path(req: &JetHttpSrvReq) -> String {
    req.path.clone()
}
fn jet_http_srv_req_param(req: &JetHttpSrvReq, name: &String) -> Option<String> {
    req.params.get(name).cloned()
}
fn jet_http_srv_req_body(req: &JetHttpSrvReq) -> String {
    req.body.clone()
}
fn jet_http_srv_req_header(req: &JetHttpSrvReq, name: &String) -> Option<String> {
    req.headers.get(&name.to_lowercase()).cloned()
}

fn jet_http_srv_req_body_len(req: &JetHttpSrvReq) -> i64 {
    req.body.len() as i64
}

fn jet_http_srv_req_under_limit(req: &JetHttpSrvReq, max_bytes: i64) -> bool {
    max_bytes >= 0 && req.body.len() as i64 <= max_bytes
}

fn jet_http_srv_sse(data: &String) -> JetHttpSrvResp {
    let resp = jet_http_srv_response(200, &format!("data: {}\n\n", data));
    let resp = jet_http_srv_response_header(
        resp,
        &"content-type".to_string(),
        &"text/event-stream".to_string(),
    );
    jet_http_srv_response_header(resp, &"cache-control".to_string(), &"no-cache".to_string())
}

fn jet_http_srv_static_file(path: &String, mime: &String) -> Result<JetHttpSrvResp, String> {
    std::fs::read_to_string(path)
        .map(|body| {
            jet_http_srv_response_header(
                jet_http_srv_response(200, &body),
                &"content-type".to_string(),
                mime,
            )
        })
        .map_err(|e| format!("static file `{}` failed: {}", path, e))
}

fn jet_http_srv_static_file_range(
    req: &JetHttpSrvReq,
    path: &String,
    mime: &String,
) -> Result<JetHttpSrvResp, String> {
    let body =
        std::fs::read_to_string(path).map_err(|e| format!("static file `{}` failed: {}", path, e))?;
    let Some(range) = jet_http_srv_req_header(req, &"range".to_string()) else {
        return Ok(jet_http_srv_response_header(
            jet_http_srv_response(200, &body),
            &"content-type".to_string(),
            mime,
        ));
    };
    let Some(spec) = range.strip_prefix("bytes=") else {
        return Ok(jet_http_srv_response(416, &"range not satisfiable".to_string()));
    };
    let (start_s, end_s) = spec.split_once('-').unwrap_or((spec, ""));
    let start = start_s.parse::<usize>().unwrap_or(0);
    let end = if end_s.is_empty() {
        body.len().saturating_sub(1)
    } else {
        end_s.parse::<usize>().unwrap_or(body.len().saturating_sub(1))
    };
    if start >= body.len() || end < start {
        return Ok(jet_http_srv_response(416, &"range not satisfiable".to_string()));
    }
    let capped = std::cmp::min(end + 1, body.len());
    let part = body[start..capped].to_string();
    let resp = jet_http_srv_response_header(
        jet_http_srv_response(206, &part),
        &"content-type".to_string(),
        mime,
    );
    Ok(jet_http_srv_response_header(
        resp,
        &"content-range".to_string(),
        &format!("bytes {}-{}/{}", start, capped - 1, body.len()),
    ))
}

fn jet_http_srv_access_log(req: &JetHttpSrvReq, status: i64) -> String {
    format!("{} {} {}", req.method, req.path, status)
}
