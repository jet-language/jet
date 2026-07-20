// ── D-HTTPLIB1=A / D-HTTPLIB2=B: core.http.server — function-first mux ───────
// Plain HTTP is pure std. D-TLSSERVE1=A routes server TLS through the hidden
// rustls bridge only when the named `tls:` option is used.

const JET_HTTP_KEEPALIVE_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const JET_HTTP_MAX_REQUESTS_PER_CONNECTION: usize = 1000;
const JET_HTTP_MAX_BODY_BYTES: usize = 1024 * 1024;
const JET_HTTP_MAX_CHUNK_FRAMING_BYTES: usize = 32 * 1024;

#[derive(Clone)]
struct JetHttpSrvResp {
    status: i64,
    body: String,
    headers: JetHttpHeaders,
}

#[derive(Clone)]
struct JetHttpSrvReq {
    method: String,
    path: String,
    params: std::collections::BTreeMap<String, String>,
    body: String,
    headers: JetHttpHeaders,
    route_template: Option<String>,
}

type JetHttpMuxHandlerFn = std::sync::Arc<dyn Fn(JetHttpSrvReq) -> JetHttpSrvResp + Send + Sync>;
type JetHttpMuxMiddlewareFn = std::sync::Arc<dyn Fn(JetHttpMuxHandlerFn) -> JetHttpMuxHandlerFn + Send + Sync>;

#[derive(Clone)]
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

#[derive(Debug)]
struct JetHttpReadError {
    status: i64,
    message: &'static str,
}

#[derive(Clone, Copy, Debug)]
enum JetHttpRequestFraming {
    ContentLength(usize),
    Chunked,
}

#[derive(Clone, Copy)]
enum JetHttpChunkPhase {
    Size,
    Data(usize),
    DataCrlf,
    FinalCrlf,
}

struct JetHttpChunkState {
    cursor: usize,
    decoded_len: usize,
    framing_len: usize,
    chunks: Vec<(usize, usize)>,
    phase: JetHttpChunkPhase,
}

impl JetHttpChunkState {
    fn new() -> Self {
        Self {
            cursor: 0,
            decoded_len: 0,
            framing_len: 0,
            chunks: Vec::new(),
            phase: JetHttpChunkPhase::Size,
        }
    }

    fn add_framing(&mut self, amount: usize) -> Result<(), JetHttpReadError> {
        self.framing_len = self.framing_len.saturating_add(amount);
        if self.framing_len > JET_HTTP_MAX_CHUNK_FRAMING_BYTES {
            return Err(JetHttpReadError {
                status: 413,
                message: "chunked request framing is too large",
            });
        }
        Ok(())
    }

    fn advance(&mut self, body: &[u8]) -> Result<Option<usize>, JetHttpReadError> {
        loop {
            match self.phase {
                JetHttpChunkPhase::Size => {
                    let Some(line_len) = body[self.cursor..]
                        .windows(2)
                        .position(|bytes| bytes == b"\r\n")
                    else {
                        if body.len().saturating_sub(self.cursor)
                            > JET_HTTP_MAX_CHUNK_FRAMING_BYTES.saturating_sub(self.framing_len)
                        {
                            return Err(JetHttpReadError {
                                status: 413,
                                message: "chunked request framing is too large",
                            });
                        }
                        return Ok(None);
                    };
                    let line_end = self.cursor + line_len;
                    let size = jet_http_chunk_size(&body[self.cursor..line_end])?;
                    self.add_framing(line_len + 2)?;
                    self.cursor = line_end + 2;
                    if size == 0 {
                        self.phase = JetHttpChunkPhase::FinalCrlf;
                    } else {
                        self.decoded_len = self.decoded_len.checked_add(size).ok_or(JetHttpReadError {
                            status: 413,
                            message: "request body is too large",
                        })?;
                        if self.decoded_len > JET_HTTP_MAX_BODY_BYTES {
                            return Err(JetHttpReadError {
                                status: 413,
                                message: "request body is too large",
                            });
                        }
                        self.chunks.push((self.cursor, size));
                        self.phase = JetHttpChunkPhase::Data(size);
                    }
                }
                JetHttpChunkPhase::Data(remaining) => {
                    let available = body.len().saturating_sub(self.cursor).min(remaining);
                    self.cursor += available;
                    if available < remaining {
                        self.phase = JetHttpChunkPhase::Data(remaining - available);
                        return Ok(None);
                    }
                    self.phase = JetHttpChunkPhase::DataCrlf;
                }
                JetHttpChunkPhase::DataCrlf => {
                    if body.len().saturating_sub(self.cursor) < 2 {
                        return Ok(None);
                    }
                    if &body[self.cursor..self.cursor + 2] != b"\r\n" {
                        return Err(JetHttpReadError {
                            status: 400,
                            message: "chunk data is not followed by CRLF",
                        });
                    }
                    self.cursor += 2;
                    self.add_framing(2)?;
                    self.phase = JetHttpChunkPhase::Size;
                }
                JetHttpChunkPhase::FinalCrlf => {
                    if body.len().saturating_sub(self.cursor) < 2 {
                        return Ok(None);
                    }
                    if &body[self.cursor..self.cursor + 2] != b"\r\n" {
                        return Err(JetHttpReadError {
                            status: 400,
                            message: "request trailers are not supported",
                        });
                    }
                    self.cursor += 2;
                    self.add_framing(2)?;
                    return Ok(Some(self.cursor));
                }
            }
        }
    }
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
        headers: JetHttpHeaders::new(),
    }
}

fn jet_http_srv_response_header(
    mut resp: JetHttpSrvResp,
    name: &String,
    value: &String,
) -> JetHttpSrvResp {
    if resp.headers.append(name, value).is_err() {
        resp.status = 500;
        resp.body = "500 Internal Server Error".to_string();
        resp.headers = JetHttpHeaders::new();
    }
    resp
}
fn jet_http_srv_response_status(resp: &JetHttpSrvResp) -> i64 { resp.status }
fn jet_http_srv_response_body(resp: &JetHttpSrvResp) -> String { resp.body.clone() }

fn jet_http_mux_serve(addr: &String, mux: JetHttpMux) -> Result<(), String> {
    jet_http_mux_validate(&mux)?;
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
        let worker_shutdown = shutdown.clone();
        workers.push(std::thread::spawn(move || loop {
            let received = worker_rx.lock().unwrap().recv();
            let Ok(mut stream) = received else { break };
            if worker_force_cancel.load(Ordering::Acquire) {
                let _ = stream.shutdown(std::net::Shutdown::Both);
                continue;
            }
            if let Ok(tracked) = stream.try_clone() { worker_active.lock().unwrap().push(tracked); }
            jet_http_server_handle_stream(&mut stream, &worker_mux, &worker_options, &worker_shutdown);
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

fn jet_http_server_handle_stream(
    stream: &mut std::net::TcpStream,
    mux: &JetHttpMux,
    options: &JetHttpServerOptions,
    shutdown: &std::sync::atomic::AtomicBool,
) {
    use std::io::Write;
    use std::sync::atomic::Ordering;

    let mut pending = Vec::new();
    for request_index in 0..JET_HTTP_MAX_REQUESTS_PER_CONNECTION {
        if request_index > 0 && shutdown.load(Ordering::Acquire) {
            return;
        }
        let raw = match jet_http_srv_read_buffered(
            stream,
            options,
            &mut pending,
            request_index > 0,
            (request_index > 0).then_some(shutdown),
        ) {
            Ok(Some(raw)) => raw,
            Ok(None) => return,
            Err(error) => {
                let _ = stream.write_all(jet_http_srv_read_error_response(&error).as_bytes());
                return;
            }
        };
        if request_index > 0 && shutdown.load(Ordering::Acquire) {
            return;
        }
        let request = match jet_http_srv_parse(&raw) {
            Ok(request) => request,
            Err(error) => {
                let _ = stream.write_all(jet_http_srv_read_error_response(&error).as_bytes());
                return;
            }
        };
        let request_version = jet_http_srv_request_version(&raw);
        let close = !jet_http_srv_request_keep_alive(request_version, &request.headers)
            || request_index + 1 == JET_HTTP_MAX_REQUESTS_PER_CONNECTION;
        if request_index > 0 && shutdown.load(Ordering::Acquire) {
            return;
        }
        let response = jet_http_mux_dispatch(mux, request);
        if stream
            .write_all(jet_http_srv_format_connection(&response, request_version, close).as_bytes())
            .is_err()
            || close
        {
            return;
        }
    }
}

fn jet_http_mux_serve_once(addr: &String, mux: JetHttpMux) -> Result<(), String> {
    jet_http_mux_validate(&mux)?;
    let listener = std::net::TcpListener::bind(addr.as_str())
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))?;
    jet_http_mux_serve_once_listener(&JetTcpListener { inner: listener }, &mux)
}

fn jet_http_mux_serve_once_listener(
    listener: &JetTcpListener,
    mux: &JetHttpMux,
) -> Result<(), String> {
    use std::io::Write;
    jet_http_mux_validate(mux)?;
    let (mut stream, _peer) = jet_http_accept_once(listener, std::time::Duration::from_secs(5))?;
    let raw = match jet_http_srv_read(&mut stream) {
        Ok(raw) => raw,
        Err(error) => {
            stream
                .write_all(jet_http_srv_read_error_response(&error).as_bytes())
                .map_err(|e| format!("http write failed: {}", e))?;
            return Ok(());
        }
    };
    let req = match jet_http_srv_parse(&raw) {
        Ok(req) => req,
        Err(error) => {
            stream
                .write_all(jet_http_srv_read_error_response(&error).as_bytes())
                .map_err(|e| format!("http write failed: {}", e))?;
            return Ok(());
        }
    };
    let resp = jet_http_mux_dispatch(mux, req);
    let text = jet_http_srv_format(&resp);
    stream
        .write_all(text.as_bytes())
        .map_err(|e| format!("http write failed: {}", e))
}

fn jet_http_accept_once(
    listener: &JetTcpListener,
    timeout: std::time::Duration,
) -> Result<(std::net::TcpStream, std::net::SocketAddr), String> {
    let started = std::time::Instant::now();
    loop {
        match listener.inner.accept() {
            Ok(connection) => return Ok(connection),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() >= timeout {
                    return Err("HTTP serve_once accept timed out".to_string());
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(error) => return Err(format!("accept failed: {error}")),
        }
    }
}

fn jet_http_srv_read(stream: &mut std::net::TcpStream) -> Result<Vec<u8>, JetHttpReadError> {
    jet_http_srv_read_with_limits(stream, &JetHttpServerOptions::safe())
}

fn jet_http_srv_read_with_limits(stream: &mut std::net::TcpStream, options: &JetHttpServerOptions) -> Result<Vec<u8>, JetHttpReadError> {
    let mut pending = Vec::new();
    jet_http_srv_read_buffered(stream, options, &mut pending, false, None)?.ok_or(JetHttpReadError {
        status: 400,
        message: "request ended before its declared framing was complete",
    })
}

fn jet_http_srv_read_buffered(
    stream: &mut std::net::TcpStream,
    options: &JetHttpServerOptions,
    pending: &mut Vec<u8>,
    keep_alive: bool,
    shutdown: Option<&std::sync::atomic::AtomicBool>,
) -> Result<Option<Vec<u8>>, JetHttpReadError> {
    use std::io::Read;
    use std::sync::atomic::Ordering;
    const MAX_HEADER_BYTES: usize = 32 * 1024;
    const SHUTDOWN_POLL: std::time::Duration = std::time::Duration::from_millis(20);
    let mut buf = [0u8; 8192];
    let mut reading_body = false;
    let mut chunked = JetHttpChunkState::new();
    let started = std::time::Instant::now();
    let mut header_deadline = (!keep_alive || !pending.is_empty()).then(|| started + options.read_header_timeout);
    let mut idle_deadline = started
        + if header_deadline.is_some() {
            options.read_idle_timeout
        } else {
            JET_HTTP_KEEPALIVE_IDLE_TIMEOUT
        };
    loop {
        if let Some(header_end) = jet_http_header_end(pending) {
            if header_end > MAX_HEADER_BYTES {
                return Err(JetHttpReadError { status: 431, message: "request headers are too large" });
            }
            let framing = jet_http_validate_headers(&pending[..header_end])?;
            if !reading_body {
                reading_body = true;
                idle_deadline = std::time::Instant::now()
                    + options.read_body_timeout.min(options.read_idle_timeout);
            }
            let body_start = header_end + 4;
            let request_end = match framing {
                JetHttpRequestFraming::ContentLength(content_len) => {
                    if content_len > JET_HTTP_MAX_BODY_BYTES {
                        return Err(JetHttpReadError { status: 413, message: "request body is too large" });
                    }
                    let request_end = body_start + content_len;
                    (pending.len() >= request_end).then_some(request_end)
                }
                JetHttpRequestFraming::Chunked => chunked
                    .advance(&pending[body_start..])?
                    .map(|body_end| body_start + body_end),
            };
            if let Some(request_end) = request_end {
                return Ok(Some(pending.drain(..request_end).collect()));
            }
        } else if pending.len() > MAX_HEADER_BYTES {
            return Err(JetHttpReadError { status: 431, message: "request headers are too large" });
        }

        let deadline = if reading_body {
            idle_deadline
        } else if let Some(deadline) = header_deadline {
            deadline.min(idle_deadline)
        } else {
            idle_deadline
        };
        let timeout = deadline.saturating_duration_since(std::time::Instant::now());
        if timeout.is_zero() {
            let between_requests = keep_alive && pending.is_empty() && header_deadline.is_none();
            if between_requests {
                return Ok(None);
            }
            return Err(JetHttpReadError { status: 408, message: "request timed out" });
        }
        let socket_timeout = if shutdown.is_some() { timeout.min(SHUTDOWN_POLL) } else { timeout };
        stream.set_read_timeout(Some(socket_timeout)).map_err(|_| JetHttpReadError { status: 400, message: "request read failed" })?;
        let n = match stream.read(&mut buf) {
            Ok(n) => n,
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                let between_requests = keep_alive && pending.is_empty() && header_deadline.is_none();
                if between_requests && shutdown.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                    return Ok(None);
                }
                if std::time::Instant::now() < deadline {
                    continue;
                }
                if between_requests {
                    return Ok(None);
                }
                return Err(JetHttpReadError { status: 408, message: "request timed out" });
            }
            Err(_) => return Err(JetHttpReadError { status: 400, message: "request read failed" }),
        };
        if n == 0 {
            if pending.is_empty() {
                return Ok(None);
            }
            return Err(JetHttpReadError {
                status: 400,
                message: "request ended before its declared framing was complete",
            });
        }
        pending.extend_from_slice(&buf[..n]);
        let now = std::time::Instant::now();
        if header_deadline.is_none() {
            header_deadline = Some(now + options.read_header_timeout);
        }
        idle_deadline = now
            + if reading_body {
                options.read_body_timeout.min(options.read_idle_timeout)
            } else {
                options.read_idle_timeout
            };
    }
}

fn jet_http_srv_request_version(raw: &[u8]) -> &str {
    raw.windows(2)
        .position(|bytes| bytes == b"\r\n")
        .and_then(|end| std::str::from_utf8(&raw[..end]).ok())
        .and_then(|line| line.split(' ').nth(2))
        .filter(|version| matches!(*version, "HTTP/1.0" | "HTTP/1.1"))
        .unwrap_or("HTTP/1.1")
}

fn jet_http_srv_request_keep_alive(version: &str, headers: &JetHttpHeaders) -> bool {
    let mut close = false;
    let mut keep_alive = false;
    for value in headers.all("connection") {
        for token in value.split(',').map(str::trim) {
            close |= token.eq_ignore_ascii_case("close");
            keep_alive |= token.eq_ignore_ascii_case("keep-alive");
        }
    }
    !close && (version == "HTTP/1.1" || (version == "HTTP/1.0" && keep_alive))
}

fn jet_http_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n")
}

fn jet_http_trim_ows(value: &str) -> &str {
    value.trim_matches(|character| matches!(character, ' ' | '\t'))
}

fn jet_http_trim_ows_start(value: &str) -> &str {
    value.trim_start_matches(|character| matches!(character, ' ' | '\t'))
}

fn jet_http_chunk_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.'
                | b'^' | b'_' | b'`' | b'|' | b'~'
        )
}

fn jet_http_chunk_extensions_valid(mut input: &[u8]) -> bool {
    while !input.is_empty() {
        while input.first().is_some_and(|byte| matches!(byte, b' ' | b'\t')) {
            input = &input[1..];
        }
        if input.first() != Some(&b';') {
            return false;
        }
        input = &input[1..];
        while input.first().is_some_and(|byte| matches!(byte, b' ' | b'\t')) {
            input = &input[1..];
        }
        let name_len = input.iter().take_while(|byte| jet_http_chunk_token_byte(**byte)).count();
        if name_len == 0 {
            return false;
        }
        input = &input[name_len..];
        while input.first().is_some_and(|byte| matches!(byte, b' ' | b'\t')) {
            input = &input[1..];
        }
        if input.first() != Some(&b'=') {
            continue;
        }
        input = &input[1..];
        while input.first().is_some_and(|byte| matches!(byte, b' ' | b'\t')) {
            input = &input[1..];
        }
        if input.first() == Some(&b'"') {
            input = &input[1..];
            let mut closed = false;
            while let Some((&byte, rest)) = input.split_first() {
                input = rest;
                if byte == b'"' {
                    closed = true;
                    break;
                }
                if byte == b'\\' {
                    let Some((&escaped, rest)) = input.split_first() else { return false };
                    if !(escaped == b'\t' || escaped == b' ' || escaped.is_ascii_graphic()) {
                        return false;
                    }
                    input = rest;
                } else if !(byte == b'\t' || byte == b' ' || matches!(byte, 0x21 | 0x23..=0x5b | 0x5d..=0x7e)) {
                    return false;
                }
            }
            if !closed {
                return false;
            }
        } else {
            let value_len = input.iter().take_while(|byte| jet_http_chunk_token_byte(**byte)).count();
            if value_len == 0 {
                return false;
            }
            input = &input[value_len..];
        }
    }
    true
}

fn jet_http_chunk_size(line: &[u8]) -> Result<usize, JetHttpReadError> {
    let digits = line.iter().take_while(|byte| byte.is_ascii_hexdigit()).count();
    if digits == 0 || !jet_http_chunk_extensions_valid(&line[digits..]) {
        return Err(JetHttpReadError {
            status: 400,
            message: "chunk size is malformed",
        });
    }
    let mut size = 0usize;
    for byte in &line[..digits] {
        let digit = (*byte as char).to_digit(16).unwrap() as usize;
        size = size.checked_mul(16).and_then(|value| value.checked_add(digit)).ok_or(
            JetHttpReadError {
                status: 413,
                message: "request body is too large",
            },
        )?;
    }
    Ok(size)
}

fn jet_http_decode_chunked_body(body: &[u8]) -> Result<String, JetHttpReadError> {
    let mut state = JetHttpChunkState::new();
    let end = state.advance(body)?.ok_or(JetHttpReadError {
        status: 400,
        message: "request ended before its chunked framing was complete",
    })?;
    if end != body.len() {
        return Err(JetHttpReadError {
            status: 400,
            message: "request body exceeds its chunked framing",
        });
    }
    let mut decoded = Vec::with_capacity(state.decoded_len);
    for (start, len) in state.chunks {
        decoded.extend_from_slice(&body[start..start + len]);
    }
    String::from_utf8(decoded).map_err(|_| JetHttpReadError {
        status: 400,
        message: "request body is not valid UTF-8",
    })
}

fn jet_http_validate_headers(header: &[u8]) -> Result<JetHttpRequestFraming, JetHttpReadError> {
    let text = std::str::from_utf8(header)
        .map_err(|_| JetHttpReadError { status: 400, message: "request headers are not valid UTF-8" })?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut request_parts = request_line.split(' ');
    let request_shape = (request_parts.next(), request_parts.next(), request_parts.next(), request_parts.next());
    let (Some(method), Some(target), Some(version), None) = request_shape else {
        return Err(JetHttpReadError { status: 400, message: "request line is malformed" });
    };
    if request_line.len() > 8 * 1024 || method.is_empty() || target.is_empty() {
        return Err(JetHttpReadError { status: 400, message: "request line is malformed" });
    }
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(JetHttpReadError { status: 505, message: "HTTP version is not supported" });
    }
    let mut count = 0usize;
    let mut content_length = None;
    let mut transfer_encoding = None;
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
        if !JetHttpHeaders::valid_name(name) {
            return Err(JetHttpReadError { status: 400, message: "request header name is malformed" });
        }
        if !JetHttpHeaders::valid_value(value) {
            return Err(JetHttpReadError { status: 400, message: "request header value is malformed" });
        }
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = jet_http_trim_ows(value).parse::<usize>()
                .map_err(|_| JetHttpReadError { status: 400, message: "content-length is malformed" })?;
            if content_length.replace(parsed).is_some_and(|old| old != parsed) {
                return Err(JetHttpReadError { status: 400, message: "conflicting content-length headers" });
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            if transfer_encoding.replace(jet_http_trim_ows(value)).is_some() {
                return Err(JetHttpReadError {
                    status: 400,
                    message: "multiple transfer-encoding headers are not allowed",
                });
            }
        }
    }
    if transfer_encoding.is_some() && content_length.is_some() {
        return Err(JetHttpReadError { status: 400, message: "content-length and transfer-encoding cannot be combined" });
    }
    if let Some(encoding) = transfer_encoding {
        if version != "HTTP/1.1" || !encoding.eq_ignore_ascii_case("chunked") {
            return Err(JetHttpReadError { status: 400, message: "transfer-encoding is not supported" });
        }
        return Ok(JetHttpRequestFraming::Chunked);
    }
    Ok(JetHttpRequestFraming::ContentLength(content_length.unwrap_or(0)))
}

fn jet_http_srv_read_error_response(error: &JetHttpReadError) -> String {
    let reason = match error.status {
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        408 => "Request Timeout",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
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
                match jet_http_srv_parse(raw.as_bytes()) {
                    Ok(req) => jet_http_srv_format(&jet_http_mux_dispatch(&m, req)),
                    Err(error) => jet_http_srv_read_error_response(&error),
                }
            });
            if let Err(e) = handle_one(&tls_cfg.cert_pem, &tls_cfg.key_pem, stream, dispatch) {
                eprintln!("http TLS connection failed: {}", e);
            }
        });
    }
}

fn jet_http_srv_parse(raw: &[u8]) -> Result<JetHttpSrvReq, JetHttpReadError> {
    let sep = jet_http_header_end(raw).ok_or(JetHttpReadError {
        status: 400,
        message: "request headers are incomplete",
    })?;
    let header_part = &raw[..sep];
    let framing = jet_http_validate_headers(header_part)?;
    let encoded_body = &raw[sep + 4..];
    let body = match framing {
        JetHttpRequestFraming::ContentLength(content_length) => {
            if encoded_body.len() != content_length {
                return Err(JetHttpReadError {
                    status: 400,
                    message: "request body does not match content-length",
                });
            }
            String::from_utf8(encoded_body.to_vec()).map_err(|_| JetHttpReadError {
                status: 400,
                message: "request body is not valid UTF-8",
            })?
        }
        JetHttpRequestFraming::Chunked => jet_http_decode_chunked_body(encoded_body)?,
    };
    let header_part = std::str::from_utf8(header_part).map_err(|_| JetHttpReadError {
        status: 400,
        message: "request headers are not valid UTF-8",
    })?;
    let mut lines = header_part.lines();
    let req_line = lines.next().unwrap_or("");
    let mut parts = req_line.splitn(3, ' ');
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut headers = JetHttpHeaders::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(JetHttpReadError {
            status: 400,
            message: "request header is malformed",
        })?;
        let value = jet_http_trim_ows_start(value);
        headers.append(name, value).map_err(|_| JetHttpReadError {
            status: 400,
            message: "request header is malformed",
        })?;
    }
    Ok(JetHttpSrvReq {
        method,
        path,
        params: std::collections::BTreeMap::new(),
        body,
        headers,
        route_template: None,
    })
}

fn jet_http_mux_dispatch(mux: &JetHttpMux, req: JetHttpSrvReq) -> JetHttpSrvResp {
    let path = match jet_http_route_path(&req.path) {
        Ok(path) => path,
        Err(_) => return JetHttpSrvResp {
            status: 400,
            body: "400 Bad Request".to_string(),
            headers: JetHttpHeaders::new(),
        },
    };
    // Route lookup is a short snapshot operation. Never retain the registry
    // lock while composing middleware or running user code: handlers may
    // overlap and may register another route on this same mux.
    let path_matches: Vec<(usize, JetHttpMuxRoute, std::collections::BTreeMap<String, String>, JetHttpRoutePattern)> = {
        let routes = mux.0.lock().unwrap();
        routes
            .iter()
            .enumerate()
            .filter_map(|(order, route)| {
                let pattern = jet_http_route_parse(&route.pattern).ok()?;
                jet_http_route_match(&pattern, &path).map(|params| (order, route.clone(), params, pattern))
            })
            .collect()
    };
    let requested_method = req.method.to_uppercase();
    let effective_method = if requested_method == "HEAD"
        && !path_matches.iter().any(|(_, route, _, _)| route.method == "HEAD")
    { "GET" } else { requested_method.as_str() };
    if requested_method == "OPTIONS" && !path_matches.iter().any(|(_, route, _, _)| route.method == "OPTIONS") {
        let allow = jet_http_allowed_methods(&path_matches);
        return JetHttpSrvResp { status: 204, body: String::new(), headers: [("Allow".to_string(), allow)].into_iter().collect() };
    }
    if let Some((_, route, params, _)) = path_matches.iter()
        .filter(|(_, route, _, _)| route.method == effective_method)
        .max_by(|(left_order, _, _, left), (right_order, _, _, right)| {
            jet_http_route_selection_cmp(left, *left_order, right, *right_order)
        })
    {
        let mut r2 = req.clone();
        r2.params = params.clone();
        r2.route_template = Some(route.pattern.clone());
        let middlewares = mux.1.lock().unwrap().clone();
        let mut handler = route.handler.clone();
        for middleware in middlewares.iter().rev() { handler = middleware(handler); }
        let mut response = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(r2))) {
            Ok(response) => response,
            Err(_) => JetHttpSrvResp {
                status: 500,
                body: "500 Internal Server Error".to_string(),
                headers: JetHttpHeaders::new(),
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
        headers: JetHttpHeaders::new(),
    }
}

fn jet_http_allowed_methods(
    matches: &[(usize, JetHttpMuxRoute, std::collections::BTreeMap<String, String>, JetHttpRoutePattern)],
) -> String {
    let mut methods = std::collections::BTreeSet::new();
    for (_, route, _, _) in matches { methods.insert(route.method.clone()); }
    if methods.contains("GET") { methods.insert("HEAD".to_string()); }
    methods.insert("OPTIONS".to_string());
    methods.into_iter().collect::<Vec<_>>().join(", ")
}

fn jet_http_mux_validate(mux: &JetHttpMux) -> Result<(), String> {
    let routes = mux.0.lock().unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for route in routes.iter() {
        let pattern = jet_http_route_parse(&route.pattern)?;
        let key = (route.method.clone(), jet_http_route_shape(&pattern));
        if !seen.insert(key) { return Err(format!("E2804: HTTP route conflict for {} `{}`", route.method, route.pattern)); }
    }
    Ok(())
}

fn jet_http_srv_format(resp: &JetHttpSrvResp) -> String {
    jet_http_srv_format_connection(resp, "HTTP/1.1", true)
}

fn jet_http_srv_format_connection(resp: &JetHttpSrvResp, version: &str, close: bool) -> String {
    let reason = match resp.status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        500 => "Internal Server Error",
        505 => "HTTP Version Not Supported",
        _ => "OK",
    };
    let body_forbidden = (100..200).contains(&resp.status) || matches!(resp.status, 204 | 304);
    let mut out = format!("{} {} {}\r\n", version, resp.status, reason);
    if !body_forbidden {
        out.push_str(&format!("Content-Length: {}\r\n", resp.body.len()));
    }
    out.push_str(&format!("Connection: {}\r\n", if close { "close" } else { "keep-alive" }));
    let connection_headers = resp
        .headers
        .all("connection")
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    for (name, value) in &resp.headers {
        let framing = name.eq_ignore_ascii_case("content-length")
            || name.eq_ignore_ascii_case("transfer-encoding")
            || name.eq_ignore_ascii_case("connection");
        let nominated = connection_headers
            .iter()
            .any(|candidate| name.eq_ignore_ascii_case(candidate));
        if !framing && !nominated {
            out.push_str(&format!("{}: {}\r\n", name, value));
        }
    }
    out.push_str("\r\n");
    if !body_forbidden {
        out.push_str(&resp.body);
    }
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
    req.headers.get(name).cloned()
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
    let route = req.route_template.as_deref().unwrap_or_else(|| req.path.split('?').next().unwrap_or(&req.path));
    format!("{} {} {}", req.method, route, status)
}
