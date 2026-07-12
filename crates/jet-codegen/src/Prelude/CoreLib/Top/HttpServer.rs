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
type JetHttpMuxMiddlewareFn = std::sync::Arc<dyn Fn(JetHttpMuxHandlerFn) -> JetHttpMuxHandlerFn + Send + Sync>;

struct JetHttpMuxRoute {
    method: String,
    pattern: String,
    handler: JetHttpMuxHandlerFn,
}

#[derive(Clone)]
struct JetHttpMux(
    std::sync::Arc<std::sync::Mutex<Vec<JetHttpMuxRoute>>>,
    std::sync::Arc<std::sync::Mutex<Vec<JetHttpMuxMiddlewareFn>>>,
);

#[derive(Clone)]
struct JetHttpServerTls {
    cert_pem: String,
    key_pem: String,
}

struct JetHttpReadError {
    status: i64,
    message: &'static str,
}

#[derive(Clone)]
struct JetHttpServerOptions {
    workers: usize,
    admission_queue: usize,
    read_header_timeout: std::time::Duration,
    read_idle_timeout: std::time::Duration,
    read_body_timeout: std::time::Duration,
    shutdown_grace: std::time::Duration,
}

impl JetHttpServerOptions {
    fn safe() -> Self {
        Self {
            workers: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            admission_queue: 256,
            read_header_timeout: std::time::Duration::from_secs(5),
            read_idle_timeout: std::time::Duration::from_secs(30),
            read_body_timeout: std::time::Duration::from_secs(30),
            shutdown_grace: std::time::Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct JetHttpShutdownReport {
    user_accepted: i64,
    user_overloaded: i64,
    user_completed: i64,
    user_cancelled: i64,
}

#[derive(Clone)]
struct JetHttpServer {
    inner: std::sync::Arc<JetHttpServerState>,
}

struct JetHttpServerState {
    listener: std::sync::Mutex<Option<std::net::TcpListener>>,
    mux: JetHttpMux,
    local_addr: String,
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    shutdown_called: std::sync::atomic::AtomicBool,
    grace_ms: std::sync::Arc<std::sync::atomic::AtomicU64>,
    lifecycle: std::sync::atomic::AtomicU8,
    report: std::sync::Mutex<Option<JetHttpShutdownReport>>,
    report_ready: std::sync::Condvar,
}

impl std::fmt::Display for JetHttpReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl JetHttpMux {
    fn new() -> Self {
        JetHttpMux(
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        )
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

fn jet_http_mux_middleware<F>(mux: &JetHttpMux, middleware: F)
where
    F: Fn(JetHttpMuxHandlerFn) -> JetHttpMuxHandlerFn + Send + Sync + 'static,
{
    mux.1.lock().unwrap().push(std::sync::Arc::new(middleware));
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

fn jet_http_mux_add_handler(mux: &JetHttpMux, method: &str, pattern: &str, handler: JetHttpMuxHandlerFn) {
    mux.0.lock().unwrap().push(JetHttpMuxRoute {
        method: method.to_uppercase(),
        pattern: pattern.to_string(),
        handler,
    });
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
fn jet_http_srv_response_status(resp: &JetHttpSrvResp) -> i64 { resp.status }
fn jet_http_srv_response_body(resp: &JetHttpSrvResp) -> String { resp.body.clone() }

fn jet_http_mux_serve(addr: &String, mux: JetHttpMux) -> Result<(), String> {
    let listener = std::net::TcpListener::bind(addr.as_str())
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))?;
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    jet_http_server_run_listener(listener, mux, JetHttpServerOptions::safe(), shutdown, None).map(|_| ())
}

fn jet_http_server_bind(addr: &String, mux: JetHttpMux) -> Result<JetHttpServer, String> {
    jet_http_mux_validate(&mux)?;
    let listener = std::net::TcpListener::bind(addr.as_str())
        .map_err(|error| format!("bind on `{addr}` failed: {error}"))?;
    let local_addr = listener.local_addr().map_err(|error| format!("local address failed: {error}"))?.to_string();
    Ok(JetHttpServer { inner: std::sync::Arc::new(JetHttpServerState {
        listener: std::sync::Mutex::new(Some(listener)), mux, local_addr,
        shutdown: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        shutdown_called: std::sync::atomic::AtomicBool::new(false),
        grace_ms: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(30_000)),
        lifecycle: std::sync::atomic::AtomicU8::new(0), report: std::sync::Mutex::new(None),
        report_ready: std::sync::Condvar::new(),
    }) })
}

fn jet_http_server_local_addr(server: &JetHttpServer) -> Result<String, String> { Ok(server.inner.local_addr.clone()) }

fn jet_http_server_serve(server: &JetHttpServer) -> Result<JetHttpShutdownReport, String> {
    use std::sync::atomic::Ordering;
    server.inner.lifecycle.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "HTTP server can only be served once".to_string())?;
    let listener = server.inner.listener.lock().unwrap().take()
        .ok_or_else(|| "HTTP server listener was already consumed".to_string())?;
    let result = jet_http_server_run_listener(listener, server.inner.mux.clone(), JetHttpServerOptions::safe(),
        server.inner.shutdown.clone(), Some(server.inner.grace_ms.clone()));
    server.inner.lifecycle.store(2, Ordering::Release);
    if let Ok(report) = result {
        *server.inner.report.lock().unwrap() = Some(report);
        server.inner.report_ready.notify_all();
        Ok(report)
    } else { server.inner.report_ready.notify_all(); result }
}

fn jet_http_server_shutdown(server: &JetHttpServer, grace: &jet_std::Duration) -> Result<JetHttpShutdownReport, String> {
    use std::sync::atomic::Ordering;
    if server.inner.shutdown_called.swap(true, Ordering::AcqRel) { return Err("HTTP server shutdown was already requested".to_string()); }
    if server.inner.lifecycle.load(Ordering::Acquire) != 1 { return Err("HTTP server is not serving".to_string()); }
    server.inner.grace_ms.store(grace.ms.max(0) as u64, Ordering::Release);
    server.inner.shutdown.store(true, Ordering::Release);
    let mut report = server.inner.report.lock().unwrap();
    while report.is_none() && server.inner.lifecycle.load(Ordering::Acquire) == 1 { report = server.inner.report_ready.wait(report).unwrap(); }
    (*report).ok_or_else(|| "HTTP server stopped without a shutdown report".to_string())
}

fn jet_http_server_run_listener(
    listener: std::net::TcpListener,
    mux: JetHttpMux,
    options: JetHttpServerOptions,
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    dynamic_grace_ms: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
) -> Result<JetHttpShutdownReport, String> {
    use std::io::{Read, Write};
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::{Receiver, SyncSender, TrySendError};

    listener.set_nonblocking(true).map_err(|error| format!("http listener setup failed: {error}"))?;
    let (tx, rx): (SyncSender<std::net::TcpStream>, Receiver<std::net::TcpStream>) =
        std::sync::mpsc::sync_channel(options.admission_queue);
    let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
    let active = std::sync::Arc::new(std::sync::Mutex::new(Vec::<std::net::TcpStream>::new()));
    let completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let force_cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut workers = Vec::new();
    for _ in 0..options.workers.max(1) {
        let worker_rx = rx.clone();
        let worker_mux = mux.clone();
        let worker_options = options.clone();
        let worker_active = active.clone();
        let worker_completed = completed.clone();
        let worker_force_cancel = force_cancel.clone();
        workers.push(std::thread::spawn(move || loop {
            let received = worker_rx.lock().unwrap().recv();
            let Ok(mut stream) = received else { break };
            if worker_force_cancel.load(Ordering::Acquire) {
                let _ = stream.shutdown(std::net::Shutdown::Both);
                continue;
            }
            if let Ok(tracked) = stream.try_clone() { worker_active.lock().unwrap().push(tracked); }
            jet_http_server_handle_stream(&mut stream, &worker_mux, &worker_options);
            worker_completed.fetch_add(1, Ordering::Relaxed);
            worker_active.lock().unwrap().retain(|tracked| tracked.peer_addr().ok() != stream.peer_addr().ok());
        }));
    }

    let mut report = JetHttpShutdownReport::default();
    while !shutdown.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, _)) => match tx.try_send(stream) {
                Ok(()) => report.user_accepted += 1,
                Err(TrySendError::Full(returned)) => {
                    stream = returned;
                    report.user_overloaded += 1;
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(10)));
                    let mut discard = [0u8; 8192];
                    let _ = stream.read(&mut discard);
                    let _ = stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    let _ = stream.flush();
                    let _ = stream.shutdown(std::net::Shutdown::Write);
                }
                Err(TrySendError::Disconnected(_)) => break,
            },
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(2)),
            Err(error) => return Err(format!("http accept failed: {error}")),
        }
    }
    drop(tx);
    let grace = dynamic_grace_ms.as_ref()
        .map(|value| std::time::Duration::from_millis(value.load(Ordering::Acquire)))
        .unwrap_or(options.shutdown_grace);
    let deadline = std::time::Instant::now() + grace;
    while completed.load(Ordering::Acquire) < report.user_accepted as usize
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    report.user_completed = completed
        .load(Ordering::Acquire)
        .min(report.user_accepted as usize) as i64;
    report.user_cancelled = report.user_accepted.saturating_sub(report.user_completed);
    if report.user_cancelled > 0 {
        force_cancel.store(true, Ordering::Release);
        for stream in active.lock().unwrap().iter() { let _ = stream.shutdown(std::net::Shutdown::Both); }
    }
    for worker in workers {
        if worker.is_finished() { let _ = worker.join(); }
    }
    Ok(report)
}

fn jet_http_server_handle_stream(stream: &mut std::net::TcpStream, mux: &JetHttpMux, options: &JetHttpServerOptions) {
    use std::io::Write;
    let raw = match jet_http_srv_read_with_limits(stream, options) {
        Ok(raw) => raw,
        Err(error) => { let _ = stream.write_all(jet_http_srv_read_error_response(&error).as_bytes()); return; }
    };
    let response = jet_http_mux_dispatch(mux, jet_http_srv_parse(&raw));
    let _ = stream.write_all(jet_http_srv_format(&response).as_bytes());
}

fn jet_http_mux_serve_once(addr: &String, mux: JetHttpMux) -> Result<(), String> {
    let listener = std::net::TcpListener::bind(addr.as_str())
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))?;
    jet_http_mux_serve_once_listener(&JetTcpListener { inner: listener }, &mux)
}

fn jet_http_mux_serve_once_listener(
    listener: &JetTcpListener,
    mux: &JetHttpMux,
) -> Result<(), String> {
    use std::io::Write;
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
    jet_http_srv_read_with_limits(stream, &JetHttpServerOptions::safe())
}

fn jet_http_srv_read_with_limits(stream: &mut std::net::TcpStream, options: &JetHttpServerOptions) -> Result<String, JetHttpReadError> {
    use std::io::Read;
    const MAX_HEADER_BYTES: usize = 32 * 1024;
    const MAX_BODY_BYTES: usize = 1024 * 1024;
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    let mut complete = false;
    let header_deadline = std::time::Instant::now() + options.read_header_timeout;
    let mut reading_body = false;
    loop {
        let timeout = if reading_body {
            options.read_body_timeout.min(options.read_idle_timeout)
        } else {
            header_deadline.saturating_duration_since(std::time::Instant::now()).min(options.read_idle_timeout)
        };
        if timeout.is_zero() { return Err(JetHttpReadError { status: 408, message: "request timed out" }); }
        stream.set_read_timeout(Some(timeout)).map_err(|_| JetHttpReadError { status: 400, message: "request read failed" })?;
        let n = stream
            .read(&mut buf)
            .map_err(|error| if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) { JetHttpReadError { status: 408, message: "request timed out" } } else { JetHttpReadError { status: 400, message: "request read failed" } })?;
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
            reading_body = true;
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
        408 => "Request Timeout",
        503 => "Service Unavailable",
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
    let path_matches: Vec<(&JetHttpMuxRoute, std::collections::BTreeMap<String, String>, (usize, usize, usize))> = routes
        .iter()
        .filter_map(|route| jet_http_match_path(&route.pattern, &req.path).map(|(params, score)| (route, params, score)))
        .collect();
    let requested_method = req.method.to_uppercase();
    let effective_method = if requested_method == "HEAD"
        && !path_matches.iter().any(|(route, _, _)| route.method == "HEAD")
    { "GET" } else { requested_method.as_str() };
    if requested_method == "OPTIONS" && !path_matches.iter().any(|(route, _, _)| route.method == "OPTIONS") {
        let allow = jet_http_allowed_methods(&path_matches);
        return JetHttpSrvResp { status: 204, body: String::new(), headers: [("Allow".to_string(), allow)].into_iter().collect() };
    }
    if let Some((route, params, _)) = path_matches.iter()
        .filter(|(route, _, _)| route.method == effective_method)
        .max_by_key(|(_, _, score)| *score)
    {
        let mut r2 = req.clone();
        r2.params = params.clone();
        let middlewares = mux.1.lock().unwrap().clone();
        let mut handler = route.handler.clone();
        for middleware in middlewares.iter().rev() { handler = middleware(handler); }
        let mut response = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(r2))) {
            Ok(response) => response,
            Err(_) => JetHttpSrvResp {
                status: 500,
                body: "500 Internal Server Error".to_string(),
                headers: std::collections::BTreeMap::new(),
            },
        };
        if requested_method == "HEAD" { response.body.clear(); }
        return response;
    }
    if !path_matches.is_empty() {
        return JetHttpSrvResp {
            status: 405,
            body: "405 Method Not Allowed".to_string(),
            headers: [("Allow".to_string(), jet_http_allowed_methods(&path_matches))].into_iter().collect(),
        };
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
) -> Option<(std::collections::BTreeMap<String, String>, (usize, usize, usize))> {
    let p_segs: Vec<&str> = pattern.split('/').collect();
    let r_segs: Vec<&str> = path.split('?').next().unwrap_or(path).split('/').collect();
    let mut params = std::collections::BTreeMap::new();
    let mut literals = 0usize;
    let mut singles = 0usize;
    let mut wildcard = false;
    let mut pi = 0usize;
    let mut ri = 0usize;
    while pi < p_segs.len() {
        let p = p_segs[pi];
        if let Some(name) = p.strip_prefix("{*").and_then(|s| s.strip_suffix('}')) {
            wildcard = true;
            params.insert(name.to_string(), r_segs[ri..].join("/"));
            ri = r_segs.len();
            pi += 1;
            break;
        }
        let r = *r_segs.get(ri)?;
        if let Some(name) = p.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            params.insert(name.to_string(), r.to_string());
            singles += 1;
        } else if p == r {
            literals += 1;
        } else {
            return None;
        }
        pi += 1;
        ri += 1;
    }
    if pi != p_segs.len() || ri != r_segs.len() { return None; }
    Some((params, (literals, usize::from(!wildcard), usize::MAX - singles)))
}

fn jet_http_allowed_methods(
    matches: &[(&JetHttpMuxRoute, std::collections::BTreeMap<String, String>, (usize, usize, usize))],
) -> String {
    let mut methods = std::collections::BTreeSet::new();
    for (route, _, _) in matches { methods.insert(route.method.clone()); }
    if methods.contains("GET") { methods.insert("HEAD".to_string()); }
    methods.insert("OPTIONS".to_string());
    methods.into_iter().collect::<Vec<_>>().join(", ")
}

fn jet_http_mux_validate(mux: &JetHttpMux) -> Result<(), String> {
    let routes = mux.0.lock().unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for route in routes.iter() {
        if !route.pattern.starts_with('/') { return Err(format!("invalid HTTP route `{}`: routes must start with `/`", route.pattern)); }
        let segments: Vec<&str> = route.pattern.split('/').collect();
        let mut names = std::collections::BTreeSet::new();
        let mut canonical = Vec::new();
        for (index, segment) in segments.iter().enumerate() {
            if segment.starts_with(':') || *segment == "*" { return Err(format!("invalid HTTP route `{}`: use `{{name}}` or final `{{*name}}` parameters", route.pattern)); }
            if let Some(name) = segment.strip_prefix("{*").and_then(|s| s.strip_suffix('}')) {
                if name.is_empty() || index + 1 != segments.len() { return Err(format!("invalid HTTP route `{}`: catch-all must be named and final", route.pattern)); }
                if !names.insert(name) { return Err(format!("invalid HTTP route `{}`: duplicate parameter `{name}`", route.pattern)); }
                canonical.push("{*}".to_string());
            } else if let Some(name) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                if name.is_empty() || !names.insert(name) { return Err(format!("invalid HTTP route `{}`: parameter names must be non-empty and unique", route.pattern)); }
                canonical.push("{}".to_string());
            } else if segment.contains('{') || segment.contains('}') { return Err(format!("invalid HTTP route `{}`: malformed parameter", route.pattern)); }
            else { canonical.push((*segment).to_string()); }
        }
        let key = (route.method.clone(), canonical.join("/"));
        if !seen.insert(key) { return Err(format!("HTTP route conflict for {} `{}`", route.method, route.pattern)); }
    }
    Ok(())
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
