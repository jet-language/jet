// ── D-HTTPLIB1=A / D-HTTPLIB2=B: core.http.server — function-first mux ───────
// Plain HTTP is pure std. D-TLSSERVE1=A routes server TLS through the hidden
// rustls bridge only when the named `tls:` option is used.

const JET_HTTP_KEEPALIVE_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const JET_HTTP_MAX_REQUESTS_PER_CONNECTION: usize = 1000;
const JET_HTTP_MAX_BODY_BYTES: usize = 1024 * 1024;
const JET_HTTP_MAX_CHUNK_FRAMING_BYTES: usize = 32 * 1024;

type JetHttpMiddleware = std::sync::Arc<dyn Fn(JetHttpHandler) -> JetHttpHandler + Send + Sync>;

#[derive(Clone)]
struct JetHttpMuxRoute {
    method: String,
    pattern: String,
    handler: JetHttpHandler,
}

#[derive(Clone)]
struct JetHttpMux(
    std::sync::Arc<std::sync::Mutex<Vec<JetHttpMuxRoute>>>,
    std::sync::Arc<std::sync::Mutex<Vec<JetHttpMiddleware>>>,
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

#[derive(Clone, Debug)]
struct JetHttpRequestHead {
    framing: JetHttpRequestFraming,
    expect_continue: bool,
    target: String,
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
    limit: usize,
}

impl JetHttpChunkState {
    fn new(limit: usize) -> Self {
        Self {
            cursor: 0,
            decoded_len: 0,
            framing_len: 0,
            chunks: Vec::new(),
            phase: JetHttpChunkPhase::Size,
            limit,
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
                        if self.decoded_len > self.limit {
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
    write_idle_timeout: std::time::Duration,
    shutdown_grace: std::time::Duration,
    max_body_bytes: usize,
    max_connections: usize,
    max_connections_per_ip: usize,
}

impl JetHttpServerOptions {
    fn safe() -> Self {
        Self {
            workers: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            admission_queue: 256,
            read_header_timeout: std::time::Duration::from_secs(5),
            read_idle_timeout: std::time::Duration::from_secs(30),
            read_body_timeout: std::time::Duration::from_secs(30),
            write_idle_timeout: std::time::Duration::from_secs(30),
            shutdown_grace: std::time::Duration::from_secs(30),
            max_body_bytes: JET_HTTP_MAX_BODY_BYTES,
            max_connections: 10_000,
            max_connections_per_ip: 256,
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
        F: Fn(JetHttpRequest) -> Result<JetHttpResponse, JetHttpError> + Send + Sync + 'static,
    {
        self.0.lock().unwrap().push(JetHttpMuxRoute {
            method: method.to_string(),
            pattern: pattern.to_string(),
            handler: std::sync::Arc::new(f) as JetHttpHandler,
        });
    }

    fn add_handler(&self, method: &str, pattern: &str, handler: JetHttpHandler) {
        self.0.lock().unwrap().push(JetHttpMuxRoute {
            method: method.to_string(),
            pattern: pattern.to_string(),
            handler,
        });
    }
}

fn jet_http_mux_middleware<F>(mux: &JetHttpMux, middleware: F)
where
    F: Fn(JetHttpHandler) -> JetHttpHandler + Send + Sync + 'static,
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
    F: Fn(JetHttpRequest) -> JetHttpResponse + Send + Sync + 'static,
{
    mux.add(method, pattern, move |request| Ok(f(request)));
}

fn jet_http_mux_add_handler(mux: &JetHttpMux, method: &str, pattern: &str, handler: JetHttpHandler) {
    mux.add_handler(method, pattern, handler);
}

fn jet_http_srv_response(status: i64, body: &String) -> JetHttpResponse {
    if !(100..=599).contains(&status) {
        return JetHttpResponse {
            status: 500,
            version: "HTTP/1.1".to_string(),
            body: JetHttpBody::from_text("500 Internal Server Error".to_string()),
            headers: JetHttpHeaders::new(),
            trailers: JetHttpHeaders::new(),
            head_content_length: None,
        };
    }
    JetHttpResponse {
        status,
        version: "HTTP/1.1".to_string(),
        body: JetHttpBody::from_text(body.clone()),
        headers: JetHttpHeaders::new(),
        trailers: JetHttpHeaders::new(),
        head_content_length: None,
    }
}

fn jet_http_srv_response_with_headers(
    status: i64,
    body: &str,
    headers: JetHttpHeaders,
) -> JetHttpResponse {
    let mut response = jet_http_srv_response(status, &body.to_string());
    response.headers = headers;
    response
}

fn jet_http_srv_response_header(
    mut resp: JetHttpResponse,
    name: &String,
    value: &String,
) -> JetHttpResponse {
    if resp.headers.append(name, value).is_err() {
        resp.status = 500;
        resp.body = JetHttpBody::from_text("500 Internal Server Error".to_string());
        resp.headers = JetHttpHeaders::new();
    }
    resp
}
fn jet_http_srv_response_status(resp: &JetHttpResponse) -> i64 { resp.status }
fn jet_http_srv_response_body(resp: &JetHttpResponse) -> JetHttpBody {
    resp.body.clone()
}

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
    let (tx, rx): (SyncSender<(std::net::TcpStream, std::net::IpAddr)>, Receiver<(std::net::TcpStream, std::net::IpAddr)>) =
        std::sync::mpsc::sync_channel(options.admission_queue);
    let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
    let active = std::sync::Arc::new(std::sync::Mutex::new(Vec::<std::net::TcpStream>::new()));
    let completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let force_cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let connection_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let per_ip = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::<std::net::IpAddr, usize>::new()));
    let mut workers = Vec::new();
    for _ in 0..options.workers.max(1) {
        let worker_rx = rx.clone();
        let worker_mux = mux.clone();
        let worker_options = options.clone();
        let worker_active = active.clone();
        let worker_completed = completed.clone();
        let worker_force_cancel = force_cancel.clone();
        let worker_shutdown = shutdown.clone();
        let worker_connection_count = connection_count.clone();
        let worker_per_ip = per_ip.clone();
        workers.push(std::thread::spawn(move || loop {
            let received = worker_rx.lock().unwrap().recv();
            let Ok((mut stream, peer_ip)) = received else { break };
            if worker_force_cancel.load(Ordering::Acquire) {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            } else {
                if let Ok(tracked) = stream.try_clone() { worker_active.lock().unwrap().push(tracked); }
                jet_http_server_handle_stream(&mut stream, &worker_mux, &worker_options, &worker_shutdown);
                worker_completed.fetch_add(1, Ordering::Relaxed);
                worker_active.lock().unwrap().retain(|tracked| tracked.peer_addr().ok() != stream.peer_addr().ok());
            }
            worker_connection_count.fetch_sub(1, Ordering::AcqRel);
            let mut counts = worker_per_ip.lock().unwrap();
            if let Some(count) = counts.get_mut(&peer_ip) {
                *count -= 1;
                if *count == 0 { counts.remove(&peer_ip); }
            }
        }));
    }

    let mut report = JetHttpShutdownReport::default();
    while !shutdown.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, peer)) => {
                let peer_ip = peer.ip();
                let globally_full = connection_count.load(Ordering::Acquire) >= options.max_connections;
                let ip_full = per_ip.lock().unwrap().get(&peer_ip).copied().unwrap_or(0) >= options.max_connections_per_ip;
                if globally_full || ip_full {
                    report.user_overloaded += 1;
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(10)));
                    let mut discard = [0u8; 8192];
                    let _ = stream.read(&mut discard);
                    let _ = stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    let _ = stream.flush();
                    let _ = stream.shutdown(std::net::Shutdown::Write);
                    continue;
                }
                connection_count.fetch_add(1, Ordering::AcqRel);
                *per_ip.lock().unwrap().entry(peer_ip).or_insert(0) += 1;
                match tx.try_send((stream, peer_ip)) {
                    Ok(()) => report.user_accepted += 1,
                    Err(TrySendError::Full((mut stream, peer_ip))) => {
                        connection_count.fetch_sub(1, Ordering::AcqRel);
                        let mut counts = per_ip.lock().unwrap();
                        if let Some(count) = counts.get_mut(&peer_ip) {
                            *count -= 1;
                            if *count == 0 { counts.remove(&peer_ip); }
                        }
                        drop(counts);
                        report.user_overloaded += 1;
                        let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(10)));
                        let mut discard = [0u8; 8192];
                        let _ = stream.read(&mut discard);
                        let _ = stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        let _ = stream.flush();
                        let _ = stream.shutdown(std::net::Shutdown::Write);
                    }
                    Err(TrySendError::Disconnected((_stream, peer_ip))) => {
                        connection_count.fetch_sub(1, Ordering::AcqRel);
                        let mut counts = per_ip.lock().unwrap();
                        if let Some(count) = counts.get_mut(&peer_ip) {
                            *count -= 1;
                            if *count == 0 { counts.remove(&peer_ip); }
                        }
                        break;
                    }
                }
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

    let _ = stream.set_write_timeout(Some(options.write_idle_timeout));
    if jet_http2_is_preface(stream, options.read_header_timeout) {
        if jet_http2_serve(stream, mux, options, shutdown).is_err() {
            let _ = jet_http2_write_frame(stream, 7, 0, 0, &[0, 0, 0, 0, 0, 0, 0, 1]);
            let _ = std::io::Write::flush(stream);
        }
        let _ = stream.shutdown(std::net::Shutdown::Both);
        return;
    }
    for request_index in 0..JET_HTTP_MAX_REQUESTS_PER_CONNECTION {
        if request_index > 0 && shutdown.load(Ordering::Acquire) {
            return;
        }
        let (request, request_version) = match jet_http_srv_read_streaming(
            stream,
            options,
            request_index > 0,
            (request_index > 0).then_some(shutdown),
        ) {
            Ok(Some(request)) => request,
            Ok(None) => return,
            Err(error) => {
                let _ = stream.write_all(jet_http_srv_read_error_response(&error).as_bytes());
                jet_http_srv_finish_close(stream);
                return;
            }
        };
        if request_index > 0 && shutdown.load(Ordering::Acquire) {
            return;
        }
        let body = request.body.clone();
        let close = !jet_http_srv_request_keep_alive(&request_version, &request.headers)
            || request_index + 1 == JET_HTTP_MAX_REQUESTS_PER_CONNECTION;
        if request_index > 0 && shutdown.load(Ordering::Acquire) {
            return;
        }
        let response = match jet_http_mux_dispatch(mux, request) {
            Ok(response) => response,
            Err(JetHttpError::BodyTooLarge { .. }) => jet_http_srv_empty_response(413),
            Err(JetHttpError::InvalidFraming) => jet_http_srv_empty_response(400),
            Err(JetHttpError::UnsupportedEncoding) => jet_http_srv_empty_response(415),
            Err(_) => jet_http_srv_response(500, &"500 Internal Server Error".to_string()),
        };
        let close = close || !body.is_drained();
        if jet_http_srv_write_response(stream, &response, &request_version, close).is_err() {
            return;
        }
        if close {
            jet_http_srv_finish_close(stream);
            return;
        }
    }
}

fn jet_http_srv_empty_response(status: i64) -> JetHttpResponse {
    let mut response = jet_http_srv_response(status, &String::new());
    response.body = JetHttpBody::empty();
    response
}

fn jet_http_srv_finish_close(stream: &mut std::net::TcpStream) {
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(10)));
    let mut discarded = 0usize;
    let mut buffer = [0u8; 4096];
    while discarded < 64 * 1024 {
        match std::io::Read::read(stream, &mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => discarded += read,
        }
    }
}

const JET_HTTP2_PREFACE: &[u8; 24] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
const JET_HTTP2_MAX_FRAME: usize = 16 * 1024;
const JET_HTTP2_MAX_HEADER_LIST: usize = 32 * 1024;
static JET_HTTP2_HUFFMAN: std::sync::OnceLock<std::collections::HashMap<(u8, u32), u8>> = std::sync::OnceLock::new();
const JET_HTTP2_HUFFMAN_LENGTHS: [u8; 256] = [
    13, 23, 28, 28, 28, 28, 28, 28, 28, 24, 30, 28, 28, 30, 28, 28,
    28, 28, 28, 28, 28, 28, 30, 28, 28, 28, 28, 28, 28, 28, 28, 28,
    6, 10, 10, 12, 13, 6, 8, 11, 10, 10, 8, 11, 8, 6, 6, 6,
    5, 5, 5, 6, 6, 6, 6, 6, 6, 6, 7, 8, 15, 6, 12, 10,
    13, 6, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
    7, 7, 7, 7, 7, 7, 7, 7, 8, 7, 8, 13, 19, 13, 14, 6,
    15, 5, 6, 5, 6, 5, 6, 6, 6, 5, 7, 7, 6, 6, 6, 5,
    6, 7, 6, 5, 5, 6, 7, 7, 7, 7, 7, 15, 11, 14, 13, 28,
    20, 22, 20, 20, 22, 22, 22, 23, 22, 23, 23, 23, 23, 23, 24, 23,
    24, 24, 22, 23, 24, 23, 23, 23, 23, 21, 22, 23, 22, 23, 23, 24,
    22, 21, 20, 22, 22, 23, 23, 21, 23, 22, 22, 24, 21, 22, 23, 23,
    21, 21, 22, 21, 23, 22, 23, 23, 20, 22, 22, 22, 23, 22, 22, 23,
    26, 26, 20, 19, 22, 23, 22, 25, 26, 26, 26, 27, 27, 26, 24, 25,
    19, 21, 26, 27, 27, 26, 27, 24, 21, 21, 26, 26, 28, 27, 27, 27,
    20, 24, 20, 21, 22, 21, 21, 23, 22, 22, 25, 25, 24, 24, 26, 23,
    26, 27, 26, 26, 27, 27, 27, 27, 27, 28, 27, 27, 27, 27, 27, 26,
];

struct JetHttp2Frame {
    kind: u8,
    flags: u8,
    stream: u32,
    payload: Vec<u8>,
}

fn jet_http2_is_preface(stream: &std::net::TcpStream, timeout: std::time::Duration) -> bool {
    let started = std::time::Instant::now();
    let _ = stream.set_read_timeout(Some(timeout));
    let mut bytes = [0u8; 24];
    while started.elapsed() < timeout {
        match stream.peek(&mut bytes) {
            Ok(read) if read > 0 => {
                if bytes[..read] != JET_HTTP2_PREFACE[..read] { return false; }
                if read == bytes.len() { return true; }
            }
            Ok(_) => return false,
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return false,
            Err(_) => return false,
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    false
}

fn jet_http2_read_frame(reader: &mut impl std::io::Read) -> Result<JetHttp2Frame, String> {
    let mut header = [0u8; 9];
    reader.read_exact(&mut header).map_err(|error| {
        if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) {
            "HTTP/2 read timed out".to_string()
        } else { "HTTP/2 frame header ended early".to_string() }
    })?;
    let length = (usize::from(header[0]) << 16) | (usize::from(header[1]) << 8) | usize::from(header[2]);
    if length > JET_HTTP2_MAX_FRAME { return Err("HTTP/2 frame exceeds the advertised maximum".to_string()); }
    if header[5] & 0x80 != 0 { return Err("HTTP/2 reserved stream bit is set".to_string()); }
    let stream = u32::from_be_bytes([header[5], header[6], header[7], header[8]]);
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload).map_err(|error| {
        if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) {
            "HTTP/2 frame payload timed out".to_string()
        } else { "HTTP/2 frame payload ended early".to_string() }
    })?;
    Ok(JetHttp2Frame { kind: header[3], flags: header[4], stream, payload })
}

fn jet_http2_write_frame(
    writer: &mut impl std::io::Write,
    kind: u8,
    flags: u8,
    stream: u32,
    payload: &[u8],
) -> Result<(), String> {
    if payload.len() > 0x00ff_ffff { return Err("HTTP/2 frame payload is too large".to_string()); }
    let length = payload.len();
    let stream = (stream & 0x7fff_ffff).to_be_bytes();
    let header = [
        (length >> 16) as u8, (length >> 8) as u8, length as u8, kind, flags,
        stream[0], stream[1], stream[2], stream[3],
    ];
    writer.write_all(&header).and_then(|()| writer.write_all(payload))
        .map_err(|_| "HTTP/2 write failed".to_string())
}

fn jet_http2_integer(input: &[u8], cursor: &mut usize, prefix: u8) -> Result<usize, String> {
    let first = *input.get(*cursor).ok_or_else(|| "HPACK integer ended early".to_string())?;
    *cursor += 1;
    let mask = (1u16 << prefix) as u8 - 1;
    let mut value = usize::from(first & mask);
    if value < usize::from(mask) { return Ok(value); }
    let mut shift = 0;
    loop {
        let byte = *input.get(*cursor).ok_or_else(|| "HPACK integer ended early".to_string())?;
        *cursor += 1;
        if shift >= usize::BITS as usize || usize::from(byte & 0x7f) > (usize::MAX - value) >> shift {
            return Err("HPACK integer overflow".to_string());
        }
        value += usize::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 { return Ok(value); }
        shift += 7;
    }
}

fn jet_http2_huffman(input: &[u8]) -> Result<String, String> {
    let table = JET_HTTP2_HUFFMAN.get_or_init(|| {
        let mut entries = std::collections::HashMap::with_capacity(256);
        let mut symbols = (0u16..256).collect::<Vec<_>>();
        symbols.sort_by_key(|symbol| (JET_HTTP2_HUFFMAN_LENGTHS[*symbol as usize], *symbol));
        let mut code = 0u32;
        let mut prior = 0u8;
        for symbol in symbols {
            let length = JET_HTTP2_HUFFMAN_LENGTHS[symbol as usize];
            code <<= u32::from(length - prior);
            entries.insert((length, code), symbol as u8);
            code += 1;
            prior = length;
        }
        entries
    });
    let mut output = Vec::new();
    let mut code = 0u32;
    let mut length = 0u8;
    for byte in input {
        for shift in (0..8).rev() {
            code = (code << 1) | u32::from((byte >> shift) & 1);
            length += 1;
            if let Some(symbol) = table.get(&(length, code)) {
                output.push(*symbol);
                code = 0;
                length = 0;
            } else if length == 30 {
                return Err("HPACK Huffman code is invalid".to_string());
            }
        }
    }
    if length > 7 || code != (1u32 << length) - 1 { return Err("HPACK Huffman padding is invalid".to_string()); }
    String::from_utf8(output).map_err(|_| "HPACK string is not UTF-8".to_string())
}

fn jet_http2_string(input: &[u8], cursor: &mut usize) -> Result<String, String> {
    let huffman = input.get(*cursor).is_some_and(|byte| byte & 0x80 != 0);
    let length = jet_http2_integer(input, cursor, 7)?;
    let end = cursor.checked_add(length).ok_or_else(|| "HPACK string length overflow".to_string())?;
    let bytes = input.get(*cursor..end).ok_or_else(|| "HPACK string ended early".to_string())?;
    *cursor = end;
    if huffman { jet_http2_huffman(bytes) } else {
        std::str::from_utf8(bytes).map(str::to_string).map_err(|_| "HPACK string is not UTF-8".to_string())
    }
}

fn jet_http2_static(index: usize) -> Option<(&'static str, &'static str)> {
    const NAMES: [&str; 47] = [
        "accept-charset", "accept-encoding", "accept-language", "accept-ranges", "accept",
        "access-control-allow-origin", "age", "allow", "authorization", "cache-control",
        "content-disposition", "content-encoding", "content-language", "content-length",
        "content-location", "content-range", "content-type", "cookie", "date", "etag", "expect",
        "expires", "from", "host", "if-match", "if-modified-since", "if-none-match", "if-range",
        "if-unmodified-since", "last-modified", "link", "location", "max-forwards",
        "proxy-authenticate", "proxy-authorization", "range", "referer", "refresh", "retry-after",
        "server", "set-cookie", "strict-transport-security", "transfer-encoding", "user-agent",
        "vary", "via", "www-authenticate",
    ];
    match index {
        1 => Some((":authority", "")), 2 => Some((":method", "GET")), 3 => Some((":method", "POST")),
        4 => Some((":path", "/")), 5 => Some((":path", "/index.html")),
        6 => Some((":scheme", "http")), 7 => Some((":scheme", "https")),
        8 => Some((":status", "200")), 9 => Some((":status", "204")), 10 => Some((":status", "206")),
        11 => Some((":status", "304")), 12 => Some((":status", "400")), 13 => Some((":status", "404")),
        14 => Some((":status", "500")),
        15..=61 => Some((NAMES[index - 15], if index == 16 { "gzip, deflate" } else { "" })),
        _ => None,
    }
}

struct JetHttp2Hpack {
    dynamic: Vec<(String, String)>,
    dynamic_size: usize,
    max_size: usize,
}

impl JetHttp2Hpack {
    fn new() -> Self { Self { dynamic: Vec::new(), dynamic_size: 0, max_size: 4096 } }

    fn field(&self, index: usize) -> Option<(String, String)> {
        jet_http2_static(index).map(|(name, value)| (name.to_string(), value.to_string()))
            .or_else(|| self.dynamic.get(index.checked_sub(62)?).cloned())
    }

    fn resize(&mut self, size: usize) -> Result<(), String> {
        if size > 4096 { return Err("HPACK dynamic table size exceeds server limit".to_string()); }
        self.max_size = size;
        while self.dynamic_size > self.max_size {
            let Some((name, value)) = self.dynamic.pop() else { break };
            self.dynamic_size -= name.len() + value.len() + 32;
        }
        Ok(())
    }

    fn insert(&mut self, name: String, value: String) {
        let size = name.len() + value.len() + 32;
        if size > self.max_size {
            self.dynamic.clear();
            self.dynamic_size = 0;
            return;
        }
        while self.dynamic_size + size > self.max_size {
            let Some((old_name, old_value)) = self.dynamic.pop() else { break };
            self.dynamic_size -= old_name.len() + old_value.len() + 32;
        }
        self.dynamic.insert(0, (name, value));
        self.dynamic_size += size;
    }
}

fn jet_http2_decode_headers(decoder: &mut JetHttp2Hpack, block: &[u8]) -> Result<Vec<(String, String)>, String> {
    let mut headers = Vec::new();
    let mut cursor = 0;
    let mut allow_size_update = true;
    let mut list_size = 0usize;
    while cursor < block.len() {
        let byte = block[cursor];
        if byte & 0x80 != 0 {
            let index = jet_http2_integer(block, &mut cursor, 7)?;
            let (name, value) = decoder.field(index).ok_or_else(|| "HPACK index is invalid".to_string())?;
            list_size = list_size.saturating_add(name.len() + value.len() + 32);
            headers.push((name, value));
            allow_size_update = false;
        } else if byte & 0xe0 == 0x20 {
            if !allow_size_update { return Err("HPACK table size update follows a header".to_string()); }
            let size = jet_http2_integer(block, &mut cursor, 5)?;
            decoder.resize(size)?;
        } else {
            let indexed = byte & 0x40 != 0;
            let prefix = if indexed { 6 } else { 4 };
            let name_index = jet_http2_integer(block, &mut cursor, prefix)?;
            let name = if name_index == 0 { jet_http2_string(block, &mut cursor)? }
                else { decoder.field(name_index).map(|field| field.0).ok_or_else(|| "HPACK name index is invalid".to_string())? };
            let value = jet_http2_string(block, &mut cursor)?;
            list_size = list_size.saturating_add(name.len() + value.len() + 32);
            if indexed { decoder.insert(name.clone(), value.clone()); }
            headers.push((name, value));
            allow_size_update = false;
        }
        if list_size > JET_HTTP2_MAX_HEADER_LIST {
            return Err("HTTP/2 header list is too large".to_string());
        }
        if headers.len() > 100 { return Err("HTTP/2 request has too many headers".to_string()); }
    }
    Ok(headers)
}

fn jet_http2_encode_integer(output: &mut Vec<u8>, value: usize, prefix: u8, bits: u8) {
    let mask = (1usize << prefix) - 1;
    if value < mask { output.push(bits | value as u8); return; }
    output.push(bits | mask as u8);
    let mut rest = value - mask;
    while rest >= 128 { output.push((rest as u8 & 0x7f) | 0x80); rest >>= 7; }
    output.push(rest as u8);
}

fn jet_http2_encode_string(output: &mut Vec<u8>, value: &str) {
    jet_http2_encode_integer(output, value.len(), 7, 0);
    output.extend_from_slice(value.as_bytes());
}

fn jet_http2_encode_response_headers(response: &JetHttpResponse, length: Option<usize>) -> Vec<u8> {
    let mut output = Vec::new();
    let status_index = match response.status { 200 => Some(8), 204 => Some(9), 206 => Some(10), 304 => Some(11), 400 => Some(12), 404 => Some(13), 500 => Some(14), _ => None };
    if let Some(index) = status_index { jet_http2_encode_integer(&mut output, index, 7, 0x80); }
    else {
        jet_http2_encode_integer(&mut output, 8, 4, 0);
        jet_http2_encode_string(&mut output, &response.status.to_string());
    }
    if let Some(length) = length {
        jet_http2_encode_integer(&mut output, 28, 4, 0);
        jet_http2_encode_string(&mut output, &length.to_string());
    }
    let connection_headers = response.headers.all("connection").into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    for (name, value) in &response.headers {
        if matches!(name.to_ascii_lowercase().as_str(), "connection" | "content-length" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade")
            || connection_headers.iter().any(|candidate| name.eq_ignore_ascii_case(candidate))
        { continue; }
        jet_http2_encode_integer(&mut output, 0, 4, 0);
        jet_http2_encode_string(&mut output, &name.to_ascii_lowercase());
        jet_http2_encode_string(&mut output, value);
    }
    output
}

fn jet_http2_request(
    headers: Vec<(String, String)>,
    body: JetHttpBody,
) -> Result<(JetHttpRequest, Option<usize>), String> {
    let mut method = None;
    let mut path = None;
    let mut scheme = None;
    let mut authority = None;
    let mut regular = JetHttpHeaders::new();
    let mut saw_regular = false;
    for (name, value) in headers {
        if name.bytes().any(|byte| byte.is_ascii_uppercase()) { return Err("HTTP/2 header name contains uppercase".to_string()); }
        if name.starts_with(':') {
            if saw_regular { return Err("HTTP/2 pseudo-header follows regular headers".to_string()); }
            let slot = match name.as_str() { ":method" => &mut method, ":path" => &mut path, ":scheme" => &mut scheme, ":authority" => &mut authority, _ => return Err("HTTP/2 pseudo-header is invalid".to_string()) };
            if slot.replace(value).is_some() { return Err("HTTP/2 pseudo-header is duplicated".to_string()); }
        } else {
            saw_regular = true;
            if matches!(name.as_str(), "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade") { return Err("HTTP/2 connection-specific header is forbidden".to_string()); }
            if name == "te" && !value.eq_ignore_ascii_case("trailers") { return Err("HTTP/2 TE value is invalid".to_string()); }
            regular.append(&name, &value).map_err(|_| "HTTP/2 header is invalid".to_string())?;
        }
    }
    let method = method.ok_or_else(|| "HTTP/2 method is missing".to_string())?;
    let path = path.ok_or_else(|| "HTTP/2 path is missing".to_string())?;
    if !matches!(scheme.as_deref(), Some("http" | "https")) { return Err("HTTP/2 scheme is invalid".to_string()); }
    if let Some(authority) = authority {
        jet_http_parse_authority(&authority).ok_or_else(|| "HTTP/2 authority is invalid".to_string())?;
        if regular.get("host").is_some_and(|host| !host.eq_ignore_ascii_case(&authority)) {
            return Err("HTTP/2 authority does not match host".to_string());
        }
        if regular.get("host").is_none() { regular.append("host", &authority).map_err(|_| "HTTP/2 authority is invalid".to_string())?; }
    }
    if !JetHttpHeaders::valid_name(&method)
        || !(jet_http_path_query_valid(&path) || method == "OPTIONS" && path == "*")
    { return Err("HTTP/2 request target is invalid".to_string()); }
    let mut content_length = None;
    for value in regular.all("content-length") {
        content_length = Some(jet_http_parse_content_length(value, content_length).map_err(|error| error.to_string())?);
    }
    if let Ok(mut state) = body.state.lock() {
        state.length = content_length;
        state.drained.store(content_length == Some(0), std::sync::atomic::Ordering::Release);
    }
    Ok((JetHttpRequest::server_body(&method, path, body, regular), content_length))
}

enum JetHttp2BodyPart {
    Data { bytes: Vec<u8>, flow_bytes: usize },
    End,
}

struct JetHttpSchedulerBlockingWait;

impl JetHttpSchedulerBlockingWait {
    fn enter() -> Self {
        jet_scheduler_blocking_wait_enter();
        Self
    }
}

impl Drop for JetHttpSchedulerBlockingWait {
    fn drop(&mut self) { jet_scheduler_blocking_wait_leave(); }
}

struct JetHttp2BodyReader {
    receiver: std::sync::mpsc::Receiver<JetHttp2BodyPart>,
    consumed: std::sync::mpsc::Sender<(u32, usize)>,
    stream_id: u32,
    current: Option<(std::io::Cursor<Vec<u8>>, usize)>,
    ended: bool,
}

impl std::io::Read for JetHttp2BodyReader {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if let Some((current, flow_bytes)) = &mut self.current {
                let read = std::io::Read::read(current, output)?;
                let done = current.position() == current.get_ref().len() as u64;
                if done {
                    let flow_bytes = *flow_bytes;
                    self.current = None;
                    let _ = self.consumed.send((self.stream_id, flow_bytes));
                }
                if read != 0 { return Ok(read); }
            }
            if self.ended { return Ok(0); }
            let part = match self.receiver.try_recv() {
                Ok(part) => Ok(part),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(std::sync::mpsc::RecvError),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    let _wait = JetHttpSchedulerBlockingWait::enter();
                    self.receiver.recv()
                }
            };
            if jet_scheduler_wait_point_cancelled() { jet_task_deliver_cancel(); }
            match part {
                Ok(JetHttp2BodyPart::Data { bytes, flow_bytes }) => {
                    self.current = Some((std::io::Cursor::new(bytes), flow_bytes));
                }
                Ok(JetHttp2BodyPart::End) | Err(_) => self.ended = true,
            }
        }
    }
}

struct JetHttp2RequestStream {
    sender: std::sync::mpsc::SyncSender<JetHttp2BodyPart>,
    pending: std::collections::VecDeque<JetHttp2BodyPart>,
    received: usize,
    unconsumed_flow: usize,
    expected: Option<usize>,
    inbound_closed: bool,
    response_done: bool,
    control: Option<std::sync::Arc<JetTaskControl>>,
    last_body: std::time::Instant,
}

impl JetHttp2RequestStream {
    fn pump(&mut self) {
        while let Some(part) = self.pending.pop_front() {
            match self.sender.try_send(part) {
                Ok(()) => {}
                Err(std::sync::mpsc::TrySendError::Full(part)) => {
                    self.pending.push_front(part);
                    break;
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    self.pending.clear();
                    break;
                }
            }
        }
    }
}

impl Drop for JetHttp2RequestStream {
    fn drop(&mut self) {
        if let Some(control) = &self.control { control.cancel(); }
    }
}

struct JetHttp2Outgoing {
    receiver: std::sync::mpsc::Receiver<JetHttp2ResponsePart>,
    chunk: Vec<u8>,
    offset: usize,
    expected: Option<usize>,
    sent: usize,
    control: std::sync::Arc<JetTaskControl>,
}

enum JetHttp2ResponsePart {
    Chunk(Vec<u8>),
    Error,
    End,
}

impl Drop for JetHttp2Outgoing {
    fn drop(&mut self) { self.control.cancel(); }
}

fn jet_http2_write_header_block(
    stream: &mut std::net::TcpStream,
    stream_id: u32,
    flags: u8,
    block: &[u8],
    max_frame: usize,
) -> Result<(), String> {
    if block.len() <= max_frame { return jet_http2_write_frame(stream, 1, flags | 0x4, stream_id, block); }
    let mut chunks = block.chunks(max_frame).peekable();
    let first = chunks.next().expect("non-empty HPACK block");
    jet_http2_write_frame(stream, 1, flags & !0x4, stream_id, first)?;
    while let Some(chunk) = chunks.next() {
        jet_http2_write_frame(stream, 9, if chunks.peek().is_none() { 0x4 } else { 0 }, stream_id, chunk)?;
    }
    Ok(())
}

fn jet_http2_start_response(
    stream: &mut std::net::TcpStream,
    stream_id: u32,
    response: JetHttpResponse,
    max_frame: usize,
) -> Result<Option<JetHttp2Outgoing>, String> {
    let body_forbidden = (100..200).contains(&response.status) || matches!(response.status, 204 | 304);
    let reset_content = response.status == 205;
    let head = response.head_content_length.is_some();
    let length = if body_forbidden { None } else if reset_content { Some(0) }
        else { response.head_content_length.or_else(|| response.body.length()) };
    let empty = body_forbidden || reset_content || head || length == Some(0);
    let headers = jet_http2_encode_response_headers(&response, length);
    if headers.len() > JET_HTTP2_MAX_HEADER_LIST { return Err("HTTP/2 response header list is too large".to_string()); }
    jet_http2_write_header_block(stream, stream_id, if empty { 0x1 } else { 0 }, &headers, max_frame)?;
    std::io::Write::flush(stream).map_err(|_| "HTTP/2 flush failed".to_string())
        .and_then(|()| if empty { Ok(None) } else {
            let mut chunks = response.body.chunks(JET_HTTP2_MAX_FRAME).map_err(|error| error.to_string())?;
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let control = JetTaskControl::new();
            let task_control = control.clone();
            let _task = jet_scheduler_spawn_blocking_with_control(move || loop {
                let part = {
                    let _wait = JetHttpSchedulerBlockingWait::enter();
                    match chunks.next() {
                        Some(Ok(chunk)) => JetHttp2ResponsePart::Chunk(chunk),
                        Some(Err(_)) => JetHttp2ResponsePart::Error,
                        None => JetHttp2ResponsePart::End,
                    }
                };
                if jet_scheduler_wait_point_cancelled() { break; }
                let done = matches!(part, JetHttp2ResponsePart::Error | JetHttp2ResponsePart::End);
                let sent = match sender.try_send(part) {
                    Ok(()) => true,
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => false,
                    Err(std::sync::mpsc::TrySendError::Full(part)) => {
                        let _wait = JetHttpSchedulerBlockingWait::enter();
                        sender.send(part).is_ok()
                    }
                };
                if done || !sent { break; }
            }, task_control);
            Ok(Some(JetHttp2Outgoing {
                receiver,
                chunk: Vec::new(),
                offset: 0,
                expected: length,
                sent: 0,
                control,
            }))
        })
}

fn jet_http2_flush_body(
    stream: &mut std::net::TcpStream,
    stream_id: u32,
    outgoing: &mut JetHttp2Outgoing,
    connection_window: &mut i64,
    stream_window: &mut i64,
    max_frame: usize,
) -> Result<bool, String> {
    loop {
        if outgoing.offset == outgoing.chunk.len() {
            match outgoing.receiver.try_recv() {
                Ok(JetHttp2ResponsePart::Chunk(chunk)) => {
                    outgoing.chunk = chunk;
                    outgoing.offset = 0;
                    if outgoing.chunk.is_empty() { continue; }
                }
                Ok(JetHttp2ResponsePart::Error) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    jet_http2_write_frame(stream, 3, 0, stream_id, &2u32.to_be_bytes())?;
                    std::io::Write::flush(stream).map_err(|_| "HTTP/2 flush failed".to_string())?;
                    return Ok(true);
                }
                Ok(JetHttp2ResponsePart::End) => {
                    if outgoing.expected.is_some_and(|expected| expected != outgoing.sent) {
                        jet_http2_write_frame(stream, 3, 0, stream_id, &2u32.to_be_bytes())?;
                        std::io::Write::flush(stream).map_err(|_| "HTTP/2 flush failed".to_string())?;
                        return Ok(true);
                    }
                    jet_http2_write_frame(stream, 0, 0x1, stream_id, &[])?;
                    std::io::Write::flush(stream).map_err(|_| "HTTP/2 flush failed".to_string())?;
                    return Ok(true);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(false),
            }
        }
        if *connection_window <= 0 || *stream_window <= 0 { return Ok(false); }
        let remaining = outgoing.expected
            .map(|expected| expected.saturating_sub(outgoing.sent))
            .unwrap_or(usize::MAX);
        if remaining == 0 {
            jet_http2_write_frame(stream, 3, 0, stream_id, &2u32.to_be_bytes())?;
            std::io::Write::flush(stream).map_err(|_| "HTTP/2 flush failed".to_string())?;
            return Ok(true);
        }
        let length = (outgoing.chunk.len() - outgoing.offset)
            .min(max_frame).min(*connection_window as usize).min(*stream_window as usize).min(remaining);
        jet_http2_write_frame(stream, 0, 0, stream_id,
            &outgoing.chunk[outgoing.offset..outgoing.offset + length])?;
        outgoing.offset += length;
        outgoing.sent += length;
        *connection_window -= length as i64;
        *stream_window -= length as i64;
    }
}

fn jet_http2_dispatch(
    mux: &JetHttpMux,
    request: JetHttpRequest,
) -> Result<JetHttpResponse, String> {
    Ok(jet_http_mux_dispatch(mux, request)
        .unwrap_or_else(|_| jet_http_srv_response(500, &"500 Internal Server Error".to_string())))
}

fn jet_http2_queue_response(
    stream: &mut std::net::TcpStream,
    stream_id: u32,
    response: JetHttpResponse,
    outgoing: &mut std::collections::BTreeMap<u32, JetHttp2Outgoing>,
    stream_windows: &mut std::collections::BTreeMap<u32, i64>,
    connection_window: &mut i64,
    initial_window: i64,
    max_frame: usize,
) -> Result<bool, String> {
    let Some(mut response) = jet_http2_start_response(stream, stream_id, response, max_frame)? else { return Ok(true) };
    let window = stream_windows.entry(stream_id).or_insert(initial_window);
    if !jet_http2_flush_body(stream, stream_id, &mut response, connection_window, window, max_frame)? {
        outgoing.insert(stream_id, response);
        return Ok(false);
    }
    Ok(true)
}

fn jet_http2_serve(
    stream: &mut std::net::TcpStream,
    mux: &JetHttpMux,
    options: &JetHttpServerOptions,
    shutdown: &std::sync::atomic::AtomicBool,
) -> Result<(), String> {
    use std::io::{Read, Write};
    use std::sync::atomic::Ordering;
    stream.set_read_timeout(Some(options.read_header_timeout))
        .map_err(|_| "HTTP/2 header timeout setup failed".to_string())?;
    let mut preface = [0u8; 24];
    stream.read_exact(&mut preface).map_err(|_| "HTTP/2 preface ended early".to_string())?;
    if &preface != JET_HTTP2_PREFACE { return Err("HTTP/2 preface is invalid".to_string()); }
    let max_streams = options.workers.saturating_add(options.admission_queue).max(1).min(u32::MAX as usize);
    let mut settings = Vec::with_capacity(18);
    settings.extend_from_slice(&3u16.to_be_bytes());
    settings.extend_from_slice(&(max_streams as u32).to_be_bytes());
    settings.extend_from_slice(&4u16.to_be_bytes());
    settings.extend_from_slice(&65_535u32.to_be_bytes());
    settings.extend_from_slice(&6u16.to_be_bytes());
    settings.extend_from_slice(&(JET_HTTP2_MAX_HEADER_LIST as u32).to_be_bytes());
    jet_http2_write_frame(stream, 4, 0, 0, &settings)?;
    stream.flush().map_err(|_| "HTTP/2 settings write failed".to_string())?;
    let poll_timeout = options.read_idle_timeout.min(std::time::Duration::from_millis(10));
    stream.set_read_timeout(Some(poll_timeout.max(std::time::Duration::from_millis(1))))
        .map_err(|_| "HTTP/2 read timeout setup failed".to_string())?;
    let mut requests = std::collections::BTreeMap::<u32, JetHttp2RequestStream>::new();
    let mut outgoing = std::collections::BTreeMap::<u32, JetHttp2Outgoing>::new();
    let mut stream_windows = std::collections::BTreeMap::<u32, i64>::new();
    let (completed_tx, completed_rx) = std::sync::mpsc::channel::<(u32, Result<JetHttpResponse, String>)>();
    let (consumed_tx, consumed_rx) = std::sync::mpsc::channel::<(u32, usize)>();
    let mut decoder = JetHttp2Hpack::new();
    let mut last_stream = 0u32;
    let mut last_activity = std::time::Instant::now();
    let mut connection_send_window = 65_535i64;
    let mut initial_send_window = 65_535i64;
    let mut peer_max_frame = JET_HTTP2_MAX_FRAME;
    let mut connection_receive_window = 65_535i64;
    let mut stream_receive_windows = std::collections::BTreeMap::<u32, i64>::new();
    let mut saw_client_settings = false;
    while !shutdown.load(Ordering::Acquire) {
        while let Ok((stream_id, flow_bytes)) = consumed_rx.try_recv() {
            let Some(request) = requests.get_mut(&stream_id) else { continue };
            let increment = flow_bytes.min(request.unconsumed_flow);
            if increment == 0 { continue; }
            request.unconsumed_flow -= increment;
            connection_receive_window += increment as i64;
            *stream_receive_windows.entry(stream_id).or_insert(0) += increment as i64;
            let increment = (increment as u32).to_be_bytes();
            jet_http2_write_frame(stream, 8, 0, 0, &increment)?;
            jet_http2_write_frame(stream, 8, 0, stream_id, &increment)?;
            request.pump();
        }
        while let Ok((stream_id, response)) = completed_rx.try_recv() {
            let Some(request) = requests.get_mut(&stream_id) else { continue };
            request.control = None;
            let response = response.unwrap_or_else(|_| jet_http_srv_response(500, &"500 Internal Server Error".to_string()));
            request.response_done = jet_http2_queue_response(
                stream, stream_id, response, &mut outgoing, &mut stream_windows,
                &mut connection_send_window, initial_send_window, peer_max_frame,
            )?;
        }
        for id in outgoing.keys().copied().collect::<Vec<_>>() {
            let Some(mut body) = outgoing.remove(&id) else { continue };
            let stream_window = stream_windows.get_mut(&id)
                .ok_or_else(|| "HTTP/2 response stream lost its window".to_string())?;
            if jet_http2_flush_body(
                stream, id, &mut body, &mut connection_send_window, stream_window, peer_max_frame,
            )? {
                if let Some(request) = requests.get_mut(&id) { request.response_done = true; }
            } else {
                outgoing.insert(id, body);
            }
        }
        let closed = requests.iter()
            .filter_map(|(id, request)| (request.inbound_closed && request.response_done).then_some(*id))
            .collect::<Vec<_>>();
        for id in closed {
            if let Some(request) = requests.remove(&id) {
                if request.unconsumed_flow > 0 {
                    connection_receive_window += request.unconsumed_flow as i64;
                    jet_http2_write_frame(stream, 8, 0, 0, &(request.unconsumed_flow as u32).to_be_bytes())?;
                }
            }
            outgoing.remove(&id);
            stream_windows.remove(&id);
            stream_receive_windows.remove(&id);
        }
        let now = std::time::Instant::now();
        let expired = requests.iter()
            .filter_map(|(id, request)| (!request.inbound_closed
                && now.duration_since(request.last_body) >= options.read_body_timeout).then_some(*id))
            .collect::<Vec<_>>();
        for id in expired {
            jet_http2_write_frame(stream, 3, 0, id, &8u32.to_be_bytes())?;
            if let Some(request) = requests.remove(&id) {
                if request.unconsumed_flow > 0 {
                    connection_receive_window += request.unconsumed_flow as i64;
                    jet_http2_write_frame(stream, 8, 0, 0, &(request.unconsumed_flow as u32).to_be_bytes())?;
                }
            }
            outgoing.remove(&id);
            stream_windows.remove(&id);
            stream_receive_windows.remove(&id);
        }
        let frame = match jet_http2_read_frame(stream) {
            Ok(frame) => { last_activity = std::time::Instant::now(); frame }
            Err(error) if error == "HTTP/2 read timed out" => {
                let now = std::time::Instant::now();
                if now.duration_since(last_activity) >= options.read_idle_timeout { return Ok(()); }
                continue;
            }
            Err(error) if error.contains("ended early") => return Ok(()),
            Err(error) => return Err(error),
        };
        if !saw_client_settings && !(frame.kind == 4 && frame.stream == 0 && frame.flags & 0x1 == 0) {
            return Err("HTTP/2 client SETTINGS must be the first frame".to_string());
        }
        match frame.kind {
            0 => {
                if frame.stream == 0 { return Err("HTTP/2 DATA uses stream zero".to_string()); }
                let request = requests.get_mut(&frame.stream).ok_or_else(|| "HTTP/2 DATA has no open request".to_string())?;
                if request.inbound_closed { return Err("HTTP/2 DATA follows end of stream".to_string()); }
                let (start, padding) = if frame.flags & 0x8 != 0 {
                    (1, usize::from(*frame.payload.first().ok_or_else(|| "HTTP/2 padded DATA is empty".to_string())?))
                } else { (0, 0) };
                if start + padding > frame.payload.len() { return Err("HTTP/2 DATA padding is invalid".to_string()); }
                let data = &frame.payload[start..frame.payload.len() - padding];
                let flow_bytes = frame.payload.len();
                connection_receive_window -= flow_bytes as i64;
                let receive_window = stream_receive_windows.entry(frame.stream).or_insert(65_535);
                *receive_window -= flow_bytes as i64;
                if connection_receive_window < 0 || *receive_window < 0 { return Err("HTTP/2 receive flow-control window exceeded".to_string()); }
                request.received = request.received.saturating_add(data.len());
                request.unconsumed_flow = request.unconsumed_flow.saturating_add(flow_bytes);
                request.last_body = std::time::Instant::now();
                if request.received > options.max_body_bytes { return Err("HTTP/2 request body is too large".to_string()); }
                if flow_bytes > 0 { request.pending.push_back(JetHttp2BodyPart::Data { bytes: data.to_vec(), flow_bytes }); }
                if frame.flags & 0x1 != 0 {
                    if request.expected.is_some_and(|expected| expected != request.received) {
                        return Err("HTTP/2 body does not match content-length".to_string());
                    }
                    request.inbound_closed = true;
                    request.pending.push_back(JetHttp2BodyPart::End);
                }
                request.pump();
            }
            1 => {
                if frame.stream == 0 || frame.stream % 2 == 0 || frame.stream <= last_stream { return Err("HTTP/2 HEADERS stream id is invalid".to_string()); }
                if requests.len() >= max_streams { return Err("HTTP/2 concurrent stream limit exceeded".to_string()); }
                last_stream = frame.stream;
                let mut offset = 0usize;
                let padding = if frame.flags & 0x8 != 0 { offset = 1; usize::from(*frame.payload.first().ok_or_else(|| "HTTP/2 padded HEADERS is empty".to_string())?) } else { 0 };
                if frame.flags & 0x20 != 0 { offset += 5; }
                if offset + padding > frame.payload.len() { return Err("HTTP/2 HEADERS padding is invalid".to_string()); }
                let mut block = frame.payload[offset..frame.payload.len() - padding].to_vec();
                if frame.flags & 0x4 == 0 {
                    stream.set_read_timeout(Some(options.read_header_timeout))
                        .map_err(|_| "HTTP/2 header timeout setup failed".to_string())?;
                    loop {
                        let continuation = jet_http2_read_frame(stream)?;
                        if continuation.kind != 9 || continuation.stream != frame.stream { return Err("HTTP/2 header block was interrupted".to_string()); }
                        block.extend_from_slice(&continuation.payload);
                        if block.len() > JET_HTTP2_MAX_HEADER_LIST { return Err("HTTP/2 header block is too large".to_string()); }
                        if continuation.flags & 0x4 != 0 { break; }
                    }
                    stream.set_read_timeout(Some(poll_timeout.max(std::time::Duration::from_millis(1))))
                        .map_err(|_| "HTTP/2 read timeout setup failed".to_string())?;
                }
                let headers = jet_http2_decode_headers(&mut decoder, &block)?;
                stream_receive_windows.insert(frame.stream, 65_535);
                stream_windows.insert(frame.stream, initial_send_window);
                let (body_tx, body_rx) = std::sync::mpsc::sync_channel(1);
                let body = JetHttpBody::reader(JetHttp2BodyReader {
                    receiver: body_rx,
                    consumed: consumed_tx.clone(),
                    stream_id: frame.stream,
                    current: None,
                    ended: false,
                }, None);
                let (request, expected) = jet_http2_request(headers, body)?;
                if expected.is_some_and(|length| length > options.max_body_bytes) {
                    return Err("HTTP/2 request body is too large".to_string());
                }
                let inbound_closed = frame.flags & 0x1 != 0;
                if inbound_closed && expected.is_some_and(|length| length != 0) {
                    return Err("HTTP/2 body does not match content-length".to_string());
                }
                let control = JetTaskControl::new();
                let task_control = control.clone();
                let task_mux = mux.clone();
                let task_completed = completed_tx.clone();
                let stream_id = frame.stream;
                let _task = jet_scheduler_spawn_blocking_with_control(move || {
                    let result = jet_http2_dispatch(&task_mux, request);
                    let _ = task_completed.send((stream_id, result));
                }, task_control);
                let mut request = JetHttp2RequestStream {
                    sender: body_tx,
                    pending: std::collections::VecDeque::new(),
                    received: 0,
                    unconsumed_flow: 0,
                    expected,
                    inbound_closed,
                    response_done: false,
                    control: Some(control),
                    last_body: std::time::Instant::now(),
                };
                if inbound_closed { request.pending.push_back(JetHttp2BodyPart::End); }
                request.pump();
                requests.insert(frame.stream, request);
            }
            2 if frame.stream != 0 && frame.payload.len() == 5 => {}
            3 if frame.stream != 0 && frame.payload.len() == 4 => {
                if let Some(request) = requests.remove(&frame.stream) {
                    if request.unconsumed_flow > 0 {
                        connection_receive_window += request.unconsumed_flow as i64;
                        jet_http2_write_frame(stream, 8, 0, 0, &(request.unconsumed_flow as u32).to_be_bytes())?;
                    }
                }
                outgoing.remove(&frame.stream);
                stream_windows.remove(&frame.stream);
                stream_receive_windows.remove(&frame.stream);
            }
            4 => {
                if frame.stream != 0 || frame.payload.len() % 6 != 0 || frame.flags & 0x1 != 0 && !frame.payload.is_empty() { return Err("HTTP/2 SETTINGS is malformed".to_string()); }
                if frame.flags & 0x1 == 0 {
                    saw_client_settings = true;
                    for setting in frame.payload.chunks_exact(6) {
                        let id = u16::from_be_bytes([setting[0], setting[1]]);
                        let value = u32::from_be_bytes([setting[2], setting[3], setting[4], setting[5]]);
                        match id {
                            2 if value > 1 => return Err("HTTP/2 ENABLE_PUSH setting is invalid".to_string()),
                            4 if value > 0x7fff_ffff => return Err("HTTP/2 initial window is invalid".to_string()),
                            4 => {
                                let change = i64::from(value) - initial_send_window;
                                initial_send_window = i64::from(value);
                                for window in stream_windows.values_mut() { *window += change; }
                            }
                            5 if !(16_384..=16_777_215).contains(&value) => return Err("HTTP/2 maximum frame size is invalid".to_string()),
                            5 => peer_max_frame = value as usize,
                            _ => {}
                        }
                    }
                    jet_http2_write_frame(stream, 4, 0x1, 0, &[])?;
                }
            }
            6 => {
                if frame.stream != 0 || frame.payload.len() != 8 { return Err("HTTP/2 PING is malformed".to_string()); }
                if frame.flags & 0x1 == 0 { jet_http2_write_frame(stream, 6, 0x1, 0, &frame.payload)?; }
            }
            7 => {
                if frame.stream != 0 || frame.payload.len() < 8 { return Err("HTTP/2 GOAWAY is malformed".to_string()); }
                return Ok(());
            }
            8 => {
                if frame.payload.len() != 4 { return Err("HTTP/2 WINDOW_UPDATE is malformed".to_string()); }
                let increment = u32::from_be_bytes(frame.payload[..4].try_into().unwrap()) & 0x7fff_ffff;
                if increment == 0 { return Err("HTTP/2 WINDOW_UPDATE is malformed".to_string()); }
                if frame.stream != 0 && !stream_windows.contains_key(&frame.stream) { continue; }
                let window = if frame.stream == 0 { &mut connection_send_window }
                    else { stream_windows.get_mut(&frame.stream).ok_or_else(|| "HTTP/2 WINDOW_UPDATE uses an idle stream".to_string())? };
                *window = window.checked_add(i64::from(increment)).filter(|value| *value <= 0x7fff_ffff)
                    .ok_or_else(|| "HTTP/2 flow-control window overflow".to_string())?;
                let ids = if frame.stream == 0 { outgoing.keys().copied().collect::<Vec<_>>() } else { vec![frame.stream] };
                for id in ids {
                    let Some(mut body) = outgoing.remove(&id) else { continue };
                    let stream_window = stream_windows.get_mut(&id).ok_or_else(|| "HTTP/2 response stream lost its window".to_string())?;
                    if jet_http2_flush_body(stream, id, &mut body, &mut connection_send_window, stream_window, peer_max_frame)? {
                        if let Some(request) = requests.get_mut(&id) { request.response_done = true; }
                    } else {
                        outgoing.insert(id, body);
                    }
                }
            }
            9 => return Err("HTTP/2 CONTINUATION has no open header block".to_string()),
            _ => {}
        }
    }
    for request in requests.values() {
        if let Some(control) = &request.control { control.cancel(); }
    }
    jet_http2_write_frame(stream, 7, 0, 0, &[0, 0, 0, 0, 0, 0, 0, 0])
}

struct JetHttpContinueReader<R> {
    inner: R,
    stream: Option<std::net::TcpStream>,
}

impl<R: std::io::Read> std::io::Read for JetHttpContinueReader<R> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if let Some(mut stream) = self.stream.take() {
            std::io::Write::write_all(&mut stream, b"HTTP/1.1 100 Continue\r\n\r\n")?;
            std::io::Write::flush(&mut stream)?;
        }
        std::io::Read::read(&mut self.inner, output)
    }
}

struct JetHttpChunkedSocketReader {
    stream: std::net::TcpStream,
    remaining: usize,
    need_crlf: bool,
    done: bool,
    framing: usize,
    decoded: usize,
    limit: usize,
}

impl JetHttpChunkedSocketReader {
    fn invalid(message: &'static str) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidData, message)
    }

    fn read_exact_framing(&mut self, bytes: &mut [u8]) -> std::io::Result<()> {
        std::io::Read::read_exact(&mut self.stream, bytes)?;
        self.framing = self.framing.saturating_add(bytes.len());
        if self.framing > JET_HTTP_MAX_CHUNK_FRAMING_BYTES {
            return Err(std::io::Error::new(std::io::ErrorKind::OutOfMemory, "chunk framing is too large"));
        }
        Ok(())
    }

    fn next_size(&mut self) -> std::io::Result<usize> {
        let mut line = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            self.read_exact_framing(&mut byte)?;
            line.push(byte[0]);
            if line.ends_with(b"\r\n") {
                line.truncate(line.len() - 2);
                return jet_http_chunk_size(&line).map_err(|error| {
                    if error.status == 413 {
                        std::io::Error::new(std::io::ErrorKind::OutOfMemory, error.message)
                    } else {
                        Self::invalid("chunk size is malformed")
                    }
                });
            }
        }
    }
}

impl std::io::Read for JetHttpChunkedSocketReader {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if self.done || output.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            if self.need_crlf {
                let mut crlf = [0u8; 2];
                self.read_exact_framing(&mut crlf)?;
                if crlf != *b"\r\n" {
                    return Err(Self::invalid("chunk data is not followed by CRLF"));
                }
                self.need_crlf = false;
            }
            self.remaining = self.next_size()?;
            self.decoded = self.decoded.checked_add(self.remaining)
                .filter(|decoded| *decoded <= self.limit)
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::OutOfMemory, "request body is too large"))?;
            if self.remaining == 0 {
                let mut final_crlf = [0u8; 2];
                self.read_exact_framing(&mut final_crlf)?;
                if final_crlf != *b"\r\n" {
                    return Err(Self::invalid("request trailers are not supported"));
                }
                self.done = true;
                return Ok(0);
            }
        }
        let wanted = output.len().min(self.remaining);
        let read = self.stream.read(&mut output[..wanted])?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "chunk ended early",
            ));
        }
        self.remaining -= read;
        self.need_crlf = self.remaining == 0;
        Ok(read)
    }
}

fn jet_http_srv_read_streaming(
    stream: &mut std::net::TcpStream,
    options: &JetHttpServerOptions,
    keep_alive: bool,
    shutdown: Option<&std::sync::atomic::AtomicBool>,
) -> Result<Option<(JetHttpRequest, String)>, JetHttpReadError> {
    use std::io::Read;
    use std::sync::atomic::Ordering;
    const MAX_HEADER_BYTES: usize = 32 * 1024;
    let started = std::time::Instant::now();
    let timeout = if keep_alive {
        JET_HTTP_KEEPALIVE_IDLE_TIMEOUT
    } else {
        options.read_header_timeout
    };
    let read_timeout = if keep_alive && shutdown.is_some() {
        std::time::Duration::from_millis(20)
    } else {
        timeout
    };
    stream.set_read_timeout(Some(read_timeout)).map_err(|_| JetHttpReadError {
        status: 400,
        message: "request read failed",
    })?;
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        if shutdown.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Ok(None);
        }
        let deadline = if keep_alive && !header.is_empty() {
            options.read_idle_timeout
        } else {
            timeout
        };
        if started.elapsed() >= deadline {
            return if keep_alive && header.is_empty() {
                Ok(None)
            } else {
                Err(JetHttpReadError { status: 408, message: "request timed out" })
            };
        }
        let mut byte = [0u8; 1];
        match stream.read(&mut byte) {
            Ok(0) if header.is_empty() => return Ok(None),
            Ok(0) => return Err(JetHttpReadError {
                status: 400,
                message: "request headers ended early",
            }),
            Ok(_) => header.push(byte[0]),
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            Err(_) => return Err(JetHttpReadError { status: 400, message: "request read failed" }),
        }
        if header.len() > MAX_HEADER_BYTES {
            return Err(JetHttpReadError { status: 431, message: "request headers are too large" });
        }
    }
    let header_end = header.len() - 4;
    let head = jet_http_validate_headers(&header[..header_end])?;
    let body_already_arrived = if head.expect_continue {
        let _ = stream.set_nonblocking(true);
        let mut byte = [0u8; 1];
        let arrived = stream.peek(&mut byte).is_ok_and(|read| read > 0);
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(options.read_body_timeout));
        arrived
    } else {
        false
    };
    let text = std::str::from_utf8(&header[..header_end]).map_err(|_| JetHttpReadError {
        status: 400,
        message: "request headers are not valid UTF-8",
    })?;
    let mut lines = text.lines();
    let line = lines.next().unwrap_or("");
    let mut parts = line.splitn(3, ' ');
    let method = parts.next().unwrap_or("");
    let _target = parts.next().unwrap_or("");
    let version = parts.next().unwrap_or("HTTP/1.1").to_string();
    let mut headers = JetHttpHeaders::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(JetHttpReadError {
            status: 400,
            message: "request header is malformed",
        })?;
        headers.append(name, jet_http_trim_ows_start(value)).map_err(|_| JetHttpReadError {
            status: 400,
            message: "request header is malformed",
        })?;
    }
    stream.set_read_timeout(Some(options.read_body_timeout)).map_err(|_| JetHttpReadError {
        status: 400,
        message: "request read failed",
    })?;
    let body_stream = stream.try_clone().map_err(|_| JetHttpReadError {
        status: 500,
        message: "request stream could not be cloned",
    })?;
    let continue_stream = if head.expect_continue && !body_already_arrived {
        Some(stream.try_clone().map_err(|_| JetHttpReadError {
            status: 500,
            message: "continue response stream could not be cloned",
        })?)
    } else {
        None
    };
    let body = match head.framing {
        JetHttpRequestFraming::ContentLength(length) => {
            if length > options.max_body_bytes {
                return Err(JetHttpReadError { status: 413, message: "request body is too large" });
            }
            JetHttpBody::reader(JetHttpContinueReader { inner: body_stream.take(length as u64), stream: continue_stream }, Some(length))
        }
        JetHttpRequestFraming::Chunked => JetHttpBody::reader(
            JetHttpContinueReader {
                inner: JetHttpChunkedSocketReader {
                    stream: body_stream,
                    remaining: 0,
                    need_crlf: false,
                    done: false,
                    framing: 0,
                    decoded: 0,
                    limit: options.max_body_bytes,
                },
                stream: continue_stream,
            },
            None,
        ),
    };
    Ok(Some((
        JetHttpRequest::server_body(method, head.target, body, headers),
        version,
    )))
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
    let (req, version) = match jet_http_srv_read_streaming(
        &mut stream,
        &JetHttpServerOptions::safe(),
        false,
        None,
    ) {
        Ok(Some(request)) => request,
        Ok(None) => return Ok(()),
        Err(error) => {
            stream
                .write_all(jet_http_srv_read_error_response(&error).as_bytes())
                .map_err(|e| format!("http write failed: {}", e))?;
            return Ok(());
        }
    };
    let resp = jet_http_mux_dispatch(mux, req)
        .unwrap_or_else(|_| jet_http_srv_response(500, &"500 Internal Server Error".to_string()));
    jet_http_srv_write_response(&mut stream, &resp, &version, true)
        .map_err(|error| error.to_string())
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
    use std::io::{Read, Write};
    use std::sync::atomic::Ordering;
    const MAX_HEADER_BYTES: usize = 32 * 1024;
    const SHUTDOWN_POLL: std::time::Duration = std::time::Duration::from_millis(20);
    let mut buf = [0u8; 8192];
    let mut reading_body = false;
    let mut continue_sent = false;
    let mut chunked = JetHttpChunkState::new(options.max_body_bytes);
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
            let head = jet_http_validate_headers(&pending[..header_end])?;
            if !reading_body {
                reading_body = true;
                idle_deadline = std::time::Instant::now()
                    + options.read_body_timeout.min(options.read_idle_timeout);
            }
            let body_start = header_end + 4;
            let request_end = match head.framing {
                JetHttpRequestFraming::ContentLength(content_len) => {
                    if content_len > options.max_body_bytes {
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
            if head.expect_continue && !continue_sent {
                stream
                    .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
                    .map_err(|_| JetHttpReadError {
                        status: 400,
                        message: "continue response write failed",
                    })?;
                continue_sent = true;
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

fn jet_http_connection_options(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').map(jet_http_trim_ows).filter(|token| !token.is_empty())
}

fn jet_http_parse_content_length(
    value: &str,
    mut expected: Option<usize>,
) -> Result<usize, JetHttpReadError> {
    for member in value.split(',').map(jet_http_trim_ows) {
        if member.is_empty() || !member.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(JetHttpReadError { status: 400, message: "content-length is malformed" });
        }
        let parsed = member.parse::<usize>()
            .map_err(|_| JetHttpReadError { status: 400, message: "content-length is malformed" })?;
        if expected.is_some_and(|old| old != parsed) {
            return Err(JetHttpReadError { status: 400, message: "conflicting content-length headers" });
        }
        expected = Some(parsed);
    }
    expected.ok_or(JetHttpReadError { status: 400, message: "content-length is malformed" })
}

fn jet_http_srv_request_keep_alive(version: &str, headers: &JetHttpHeaders) -> bool {
    let mut close = false;
    let mut keep_alive = false;
    for value in headers.all("connection") {
        for token in jet_http_connection_options(value) {
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

fn jet_http_host_port_valid(port: &str) -> bool {
    !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok()
}

fn jet_http_reg_name_valid(host: &str) -> bool {
    if host.is_empty() {
        return false;
    }
    let bytes = host.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b';' | b'=')
        {
            index += 1;
        } else if byte == b'%'
            && bytes.get(index + 1).is_some_and(u8::is_ascii_hexdigit)
            && bytes.get(index + 2).is_some_and(u8::is_ascii_hexdigit)
        {
            index += 3;
        } else {
            return false;
        }
    }
    true
}

fn jet_http_ipv_future_valid(host: &str) -> bool {
    let Some((version, address)) = host.get(1..).and_then(|rest| rest.split_once('.')) else {
        return false;
    };
    (host.starts_with('v') || host.starts_with('V'))
        && !version.is_empty()
        && version.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !address.is_empty()
        && address.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' | b':')
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetHttpAuthority {
    host: String,
    port: Option<u16>,
}

fn jet_http_normalize_reg_name(host: &str) -> String {
    let mut normalized = String::with_capacity(host.len());
    let bytes = host.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            normalized.push('%');
            normalized.push((bytes[index + 1] as char).to_ascii_uppercase());
            normalized.push((bytes[index + 2] as char).to_ascii_uppercase());
            index += 3;
        } else {
            normalized.push((bytes[index] as char).to_ascii_lowercase());
            index += 1;
        }
    }
    normalized
}

fn jet_http_parse_authority(value: &str) -> Option<JetHttpAuthority> {
    let authority = jet_http_trim_ows(value);
    if let Some(bracketed) = authority.strip_prefix('[') {
        let (host, suffix) = bracketed.split_once(']')?;
        let host = if let Ok(address) = host.parse::<std::net::Ipv6Addr>() {
            address.to_string()
        } else if jet_http_ipv_future_valid(host) {
            host.to_ascii_lowercase()
        } else {
            return None;
        };
        let port = if suffix.is_empty() {
            None
        } else {
            let port = suffix.strip_prefix(':')?;
            if !jet_http_host_port_valid(port) {
                return None;
            }
            Some(port.parse().unwrap())
        };
        return Some(JetHttpAuthority { host: format!("[{host}]"), port });
    }
    let mut parts = authority.split(':');
    let host = parts.next().unwrap_or("");
    let port = parts.next();
    if parts.next().is_some() || !jet_http_reg_name_valid(host) {
        return None;
    }
    let port = match port {
        Some(port) if jet_http_host_port_valid(port) => Some(port.parse().unwrap()),
        Some(_) => return None,
        None => None,
    };
    Some(JetHttpAuthority {
        host: jet_http_normalize_reg_name(host),
        port,
    })
}

fn jet_http_path_query_valid(target: &str) -> bool {
    let bytes = target.as_bytes();
    if bytes.first() != Some(&b'/') {
        return false;
    }
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            if !bytes.get(index + 1).is_some_and(u8::is_ascii_hexdigit)
                || !bytes.get(index + 2).is_some_and(u8::is_ascii_hexdigit)
            {
                return false;
            }
            index += 3;
            continue;
        }
        if byte == b'?' {
            index += 1;
            continue;
        }
        if !(byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'.' | b'_' | b'~' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')'
                    | b'*' | b'+' | b',' | b';' | b'=' | b':' | b'@' | b'/'
            ))
        {
            return false;
        }
        index += 1;
    }
    true
}

fn jet_http_absolute_target(
    method: &str,
    target: &str,
    host: Option<&JetHttpAuthority>,
    host_required: bool,
) -> Result<String, JetHttpReadError> {
    if target == "*" {
        return if method == "OPTIONS" {
            Ok(target.to_string())
        } else {
            Err(JetHttpReadError { status: 400, message: "asterisk request target requires OPTIONS" })
        };
    }
    let path_query = if target.starts_with('/') {
        target.to_string()
    } else {
        let Some(scheme_end) = target.find("://") else {
            return Err(JetHttpReadError { status: 400, message: "request target form is not supported" });
        };
        let scheme = &target[..scheme_end];
        if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
            return Err(JetHttpReadError { status: 400, message: "absolute request target is malformed" });
        }
        let remainder = &target[scheme_end + 3..];
        let authority_end = remainder.find(['/', '?']).unwrap_or(remainder.len());
        let raw_authority = &remainder[..authority_end];
        if jet_http_trim_ows(raw_authority) != raw_authority {
            return Err(JetHttpReadError { status: 400, message: "absolute request authority is malformed" });
        }
        let authority = jet_http_parse_authority(raw_authority).ok_or(JetHttpReadError {
            status: 400,
            message: "absolute request authority is malformed",
        })?;
        let default_port = if scheme.eq_ignore_ascii_case("http") { 80 } else { 443 };
        if let Some(host) = host {
            if authority.host != host.host
                || authority.port.unwrap_or(default_port) != host.port.unwrap_or(default_port)
            {
                return Err(JetHttpReadError { status: 400, message: "absolute request authority does not match host" });
            }
        } else if host_required {
            return Err(JetHttpReadError { status: 400, message: "absolute request target requires a host header" });
        }
        let suffix = &remainder[authority_end..];
        if suffix.is_empty() {
            "/".to_string()
        } else if suffix.starts_with('?') {
            format!("/{suffix}")
        } else {
            suffix.to_string()
        }
    };
    if !jet_http_path_query_valid(&path_query) {
        return Err(JetHttpReadError { status: 400, message: "request target path or query is malformed" });
    }
    Ok(path_query)
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

fn jet_http_decode_chunked_body(body: &[u8]) -> Result<Vec<u8>, JetHttpReadError> {
    let mut state = JetHttpChunkState::new(JET_HTTP_MAX_BODY_BYTES);
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
    Ok(decoded)
}

fn jet_http_validate_headers(header: &[u8]) -> Result<JetHttpRequestHead, JetHttpReadError> {
    let text = std::str::from_utf8(header)
        .map_err(|_| JetHttpReadError { status: 400, message: "request headers are not valid UTF-8" })?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut request_parts = request_line.split(' ');
    let request_shape = (request_parts.next(), request_parts.next(), request_parts.next(), request_parts.next());
    let (Some(method), Some(target), Some(version), None) = request_shape else {
        return Err(JetHttpReadError { status: 400, message: "request line is malformed" });
    };
    if request_line.len() > 8 * 1024 || target.is_empty() {
        return Err(JetHttpReadError { status: 400, message: "request line is malformed" });
    }
    if !JetHttpHeaders::valid_name(method) {
        return Err(JetHttpReadError { status: 400, message: "request method is malformed" });
    }
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(JetHttpReadError { status: 505, message: "HTTP version is not supported" });
    }
    let mut count = 0usize;
    let mut content_length = None;
    let mut transfer_encoding = None;
    let mut expectation = None;
    let mut host = None;
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
        if name.eq_ignore_ascii_case("connection") {
            if !jet_http_connection_options(value).all(JetHttpHeaders::valid_name) {
                return Err(JetHttpReadError {
                    status: 400,
                    message: "connection option is malformed",
                });
            }
        } else if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(jet_http_parse_content_length(value, content_length)?);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            if transfer_encoding.replace(jet_http_trim_ows(value)).is_some() {
                return Err(JetHttpReadError {
                    status: 400,
                    message: "multiple transfer-encoding headers are not allowed",
                });
            }
        } else if name.eq_ignore_ascii_case("expect") {
            if expectation.replace(jet_http_trim_ows(value)).is_some() {
                return Err(JetHttpReadError {
                    status: 417,
                    message: "multiple expect headers are not supported",
                });
            }
        } else if name.eq_ignore_ascii_case("host") {
            if host.replace(value).is_some() {
                return Err(JetHttpReadError {
                    status: 400,
                    message: "multiple host headers are not allowed",
                });
            }
        }
    }
    let host = match host {
        Some(value) => Some(jet_http_parse_authority(value).ok_or(JetHttpReadError {
            status: 400,
            message: "host authority is malformed",
        })?),
        None if version == "HTTP/1.1" => {
            return Err(JetHttpReadError {
                status: 400,
                message: "HTTP/1.1 requires one host header",
            });
        }
        None => None,
    };
    let target = jet_http_absolute_target(method, target, host.as_ref(), version == "HTTP/1.1")?;
    if transfer_encoding.is_some() && content_length.is_some() {
        return Err(JetHttpReadError { status: 400, message: "content-length and transfer-encoding cannot be combined" });
    }
    let framing = if let Some(encoding) = transfer_encoding {
        if version != "HTTP/1.1" || !encoding.eq_ignore_ascii_case("chunked") {
            return Err(JetHttpReadError { status: 400, message: "transfer-encoding is not supported" });
        }
        JetHttpRequestFraming::Chunked
    } else {
        JetHttpRequestFraming::ContentLength(content_length.unwrap_or(0))
    };
    let expect_continue = match expectation {
        None => false,
        Some(value) if version == "HTTP/1.1" && value.eq_ignore_ascii_case("100-continue") => true,
        Some(_) => {
            return Err(JetHttpReadError {
                status: 417,
                message: "request expectation is not supported",
            });
        }
    };
    Ok(JetHttpRequestHead { framing, expect_continue, target })
}

fn jet_http_srv_read_error_response(error: &JetHttpReadError) -> String {
    let reason = match error.status {
        413 => "Payload Too Large",
        417 => "Expectation Failed",
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
                    Ok(req) => match jet_http_mux_dispatch(&m, req) {
                        Ok(response) => jet_http_srv_format(&response),
                        Err(_) => jet_http_srv_format(&jet_http_srv_response(
                            500,
                            &"500 Internal Server Error".to_string(),
                        )),
                    },
                    Err(error) => jet_http_srv_read_error_response(&error),
                }
            });
            if let Err(e) = handle_one(&tls_cfg.cert_pem, &tls_cfg.key_pem, stream, dispatch) {
                eprintln!("http TLS connection failed: {}", e);
            }
        });
    }
}

fn jet_http_srv_parse(raw: &[u8]) -> Result<JetHttpRequest, JetHttpReadError> {
    let sep = jet_http_header_end(raw).ok_or(JetHttpReadError {
        status: 400,
        message: "request headers are incomplete",
    })?;
    let header_part = &raw[..sep];
    let head = jet_http_validate_headers(header_part)?;
    let encoded_body = &raw[sep + 4..];
    let body = match head.framing {
        JetHttpRequestFraming::ContentLength(content_length) => {
            if encoded_body.len() != content_length {
                return Err(JetHttpReadError {
                    status: 400,
                    message: "request body does not match content-length",
                });
            }
            encoded_body.to_vec()
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
    let path = head.target;
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
    Ok(JetHttpRequest::server(&method, path, body, headers))
}

fn jet_http_mux_dispatch(
    mux: &JetHttpMux,
    req: JetHttpRequest,
) -> Result<JetHttpResponse, JetHttpError> {
    let requested_method = req.method.as_str();
    let is_head = requested_method == "HEAD";
    if requested_method == "OPTIONS" && req.path == "*" {
        let routes = mux.0.clone();
        let handler: JetHttpHandler = std::sync::Arc::new(move |_| {
            let allow = {
                let routes = routes.lock().unwrap();
                jet_http_allowed_methods(routes.iter().map(|route| route.method.as_str()))
            };
            Ok(jet_http_srv_response_with_headers(
                204,
                "",
                [("Allow".to_string(), allow)].into_iter().collect(),
            ))
        });
        return jet_http_mux_run_handler(mux, req, handler);
    }
    let path = match jet_http_route_path(&req.path) {
        Ok(path) => path,
        Err(_) => {
            return Ok(jet_http_srv_head_response(
                jet_http_srv_response(400, &"400 Bad Request".to_string()),
                is_head,
            ));
        }
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
    let effective_method = if requested_method == "HEAD"
        && !path_matches.iter().any(|(_, route, _, _)| route.method == "HEAD")
    { "GET" } else { requested_method };
    if requested_method == "OPTIONS" && !path_matches.iter().any(|(_, route, _, _)| route.method == "OPTIONS") {
        let allow = jet_http_allowed_methods(path_matches.iter().map(|(_, route, _, _)| route.method.as_str()));
        let handler: JetHttpHandler = std::sync::Arc::new(move |_| {
            Ok(jet_http_srv_response_with_headers(
                204,
                "",
                [("Allow".to_string(), allow.clone())].into_iter().collect(),
            ))
        });
        return jet_http_mux_run_handler(mux, req, handler);
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
        let response = jet_http_mux_run_handler(mux, r2, route.handler.clone())?;
        return Ok(jet_http_srv_head_response(response, is_head));
    }
    if !path_matches.is_empty() {
        return Ok(jet_http_srv_head_response(
            jet_http_srv_response_with_headers(
                405,
                "405 Method Not Allowed",
                [("Allow".to_string(), jet_http_allowed_methods(
                    path_matches.iter().map(|(_, route, _, _)| route.method.as_str()),
                ))].into_iter().collect(),
            ),
            is_head,
        ));
    }
    Ok(jet_http_srv_head_response(
        jet_http_srv_response(404, &"404 Not Found".to_string()),
        is_head,
    ))
}

fn jet_http_mux_run_handler(
    mux: &JetHttpMux,
    req: JetHttpRequest,
    mut handler: JetHttpHandler,
) -> Result<JetHttpResponse, JetHttpError> {
    let middlewares = mux.1.lock().unwrap().clone();
    for middleware in middlewares.iter().rev() { handler = middleware(handler); }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(req))) {
        Ok(response) => response,
        Err(_) => Err(JetHttpError::Internal {
            incident_id: "http-handler-panic".to_string(),
        }),
    }
}

fn jet_http_srv_head_response(mut response: JetHttpResponse, is_head: bool) -> JetHttpResponse {
    if is_head {
        response.head_content_length = response.body.length();
        response.body = JetHttpBody::empty();
    }
    response
}

fn jet_http_allowed_methods<'a>(registered: impl Iterator<Item = &'a str>) -> String {
    let mut methods = std::collections::BTreeSet::new();
    methods.extend(registered.map(str::to_string));
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

fn jet_http_srv_format(resp: &JetHttpResponse) -> String {
    jet_http_srv_format_connection(resp, "HTTP/1.1", true)
}

fn jet_http_srv_format_connection(resp: &JetHttpResponse, version: &str, close: bool) -> String {
    let mut bytes = Vec::new();
    jet_http_srv_write_response(&mut bytes, resp, version, close)
        .expect("in-memory HTTP response formatting cannot fail");
    String::from_utf8(bytes).expect("text compatibility response contains UTF-8")
}

fn jet_http_srv_write_response(
    writer: &mut impl std::io::Write,
    resp: &JetHttpResponse,
    version: &str,
    close: bool,
) -> Result<(), JetHttpError> {
    let reason = match resp.status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        500 => "Internal Server Error",
        505 => "HTTP Version Not Supported",
        _ => "OK",
    };
    let body_forbidden = (100..200).contains(&resp.status) || matches!(resp.status, 204 | 304);
    let reset_content = resp.status == 205;
    let known_length = resp.head_content_length.or_else(|| resp.body.length());
    let chunked = !body_forbidden && !reset_content && known_length.is_none();
    let mut out = format!("{} {} {}\r\n", version, resp.status, reason);
    if !body_forbidden {
        if chunked {
            out.push_str("Transfer-Encoding: chunked\r\n");
        } else {
            out.push_str(&format!("Content-Length: {}\r\n", if reset_content { 0 } else { known_length.unwrap_or(0) }));
        }
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
    writer.write_all(out.as_bytes()).map_err(|_| JetHttpError::Io {
        operation: "write response headers".to_string(),
    })?;
    if !body_forbidden && !reset_content && resp.head_content_length.is_none() {
        let mut written = 0usize;
        for chunk in resp.body.chunks(64 * 1024)? {
            let chunk = chunk?;
            written = written.saturating_add(chunk.len());
            if chunked {
                write!(writer, "{:x}\r\n", chunk.len()).map_err(|_| JetHttpError::Io {
                    operation: "write response chunk framing".to_string(),
                })?;
            }
            writer.write_all(&chunk).map_err(|_| JetHttpError::Io {
                operation: "write response body".to_string(),
            })?;
            if chunked {
                writer.write_all(b"\r\n").map_err(|_| JetHttpError::Io {
                    operation: "write response chunk framing".to_string(),
                })?;
            }
        }
        if known_length.is_some_and(|length| length != written) {
            return Err(JetHttpError::InvalidFraming);
        }
        if chunked {
            writer.write_all(b"0\r\n\r\n").map_err(|_| JetHttpError::Io {
                operation: "write response chunk terminator".to_string(),
            })?;
        }
    }
    Ok(())
}

fn jet_http_srv_req_method(req: &JetHttpRequest) -> String {
    req.method.clone()
}
fn jet_http_srv_req_path(req: &JetHttpRequest) -> String {
    req.path.clone()
}
fn jet_http_srv_req_param(req: &JetHttpRequest, name: &String) -> Option<String> {
    req.params.get(name).cloned()
}
fn jet_http_srv_req_body(req: &JetHttpRequest) -> JetHttpBody {
    req.body.clone()
}
fn jet_http_srv_req_header(req: &JetHttpRequest, name: &String) -> Option<String> {
    req.headers.get(name).cloned()
}

fn jet_http_srv_req_body_len(req: &JetHttpRequest) -> i64 {
    req.body.length().unwrap_or(0) as i64
}

fn jet_http_srv_req_under_limit(req: &JetHttpRequest, max_bytes: i64) -> bool {
    max_bytes >= 0 && req.body.length().is_some_and(|length| length as i64 <= max_bytes)
}

fn jet_http_srv_sse(data: &String) -> JetHttpResponse {
    let resp = jet_http_srv_response(200, &format!("data: {}\n\n", data));
    let resp = jet_http_srv_response_header(
        resp,
        &"content-type".to_string(),
        &"text/event-stream".to_string(),
    );
    jet_http_srv_response_header(resp, &"cache-control".to_string(), &"no-cache".to_string())
}

fn jet_http_srv_static_file(path: &String, mime: &String) -> Result<JetHttpResponse, String> {
    let candidate = std::path::Path::new(path);
    let parent = candidate.parent().filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let root = std::fs::canonicalize(parent).map_err(|error| format!("static file `{path}` failed: {error}"))?;
    let Some((file, metadata, _)) = jet_http_static_open(&root, candidate) else {
        return Err(format!("static file `{path}` could not be opened with held identity"));
    };
    let length = usize::try_from(metadata.len()).map_err(|_| format!("static file `{path}` is too large"))?;
    let mut response = jet_http_srv_response(200, &String::new());
    response.body = JetHttpBody::reader(file, Some(length));
    Ok(jet_http_srv_response_header(response, &"content-type".to_string(), mime))
}

fn jet_http_srv_static_file_range(
    req: &JetHttpRequest,
    path: &String,
    mime: &String,
) -> Result<JetHttpResponse, String> {
    let candidate = std::path::Path::new(path);
    let parent = candidate.parent().filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let root = std::fs::canonicalize(parent).map_err(|error| format!("static file `{path}` failed: {error}"))?;
    let Some((mut file, metadata, _)) = jet_http_static_open(&root, candidate) else {
        return Err(format!("static file `{path}` could not be opened with held identity"));
    };
    let file_len = usize::try_from(metadata.len()).map_err(|_| format!("static file `{path}` is too large"))?;
    let Some(range) = jet_http_srv_req_header(req, &"range".to_string()) else {
        let mut response = jet_http_srv_response(200, &String::new());
        response.body = JetHttpBody::reader(file, Some(file_len));
        return Ok(jet_http_srv_response_header(response, &"content-type".to_string(), mime));
    };
    let Some((start, end)) = jet_http_static_range(&range, file_len) else {
        return Ok(jet_http_srv_response(416, &"range not satisfiable".to_string()));
    };
    use std::io::Seek;
    file.seek(std::io::SeekFrom::Start(start as u64))
        .map_err(|error| format!("static file `{path}` seek failed: {error}"))?;
    let length = end - start + 1;
    let mut response = jet_http_srv_response(206, &String::new());
    response.body = JetHttpBody::reader(std::io::Read::take(file, length as u64), Some(length));
    let resp = jet_http_srv_response_header(
        response,
        &"content-type".to_string(),
        mime,
    );
    Ok(jet_http_srv_response_header(
        resp,
        &"content-range".to_string(),
        &format!("bytes {start}-{end}/{file_len}"),
    ))
}

fn jet_http_static_range(value: &str, len: usize) -> Option<(usize, usize)> {
    let spec = value.strip_prefix("bytes=")?;
    if len == 0 || spec.contains(',') { return None; }
    let (first, last) = spec.split_once('-')?;
    match (first.is_empty(), last.is_empty()) {
        (true, true) => None,
        (true, false) => {
            let suffix = last.parse::<usize>().ok()?;
            (suffix > 0).then_some((len.saturating_sub(suffix), len - 1))
        }
        (false, true) => {
            let start = first.parse::<usize>().ok()?;
            (start < len).then_some((start, len - 1))
        }
        (false, false) => {
            let start = first.parse::<usize>().ok()?;
            let end = last.parse::<usize>().ok()?.min(len - 1);
            (start < len && start <= end).then_some((start, end))
        }
    }
}

fn jet_http_static_mime(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(std::ffi::OsStr::to_str).unwrap_or("").to_ascii_lowercase().as_str() {
        "css" => "text/css; charset=utf-8",
        "gif" => "image/gif",
        "html" | "htm" => "text/html; charset=utf-8",
        "jpeg" | "jpg" => "image/jpeg",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn jet_http_days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * shifted_month + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn jet_http_civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn jet_http_date(seconds: i64) -> String {
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let days = seconds.div_euclid(86_400);
    let within = seconds.rem_euclid(86_400);
    let (year, month, day) = jet_http_civil_from_days(days);
    format!("{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        WEEKDAYS[(days + 4).rem_euclid(7) as usize], day, MONTHS[(month - 1) as usize], year,
        within / 3600, within / 60 % 60, within % 60)
}

fn jet_http_date_parse(value: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let parts = value.split_ascii_whitespace().collect::<Vec<_>>();
    if parts.len() != 6 || !parts[0].ends_with(',') || parts[5] != "GMT" { return None; }
    let day = parts[1].parse::<i64>().ok()?;
    let month = MONTHS.iter().position(|month| *month == parts[2])? as i64 + 1;
    let year = parts[3].parse::<i64>().ok()?;
    let time = parts[4].split(':').map(str::parse::<i64>).collect::<Result<Vec<_>, _>>().ok()?;
    if time.len() != 3 || !(0..24).contains(&time[0]) || !(0..60).contains(&time[1]) || !(0..60).contains(&time[2]) { return None; }
    let seconds = jet_http_days_from_civil(year, month, day).checked_mul(86_400)?
        .checked_add(time[0] * 3600 + time[1] * 60 + time[2])?;
    (jet_http_date(seconds) == value).then_some(seconds)
}

fn jet_http_static_open(
    root: &std::path::Path,
    candidate: &std::path::Path,
) -> Option<(std::fs::File, std::fs::Metadata, std::path::PathBuf)> {
    // std exposes neither final-path-by-handle nor an openat-style no-reparse
    // walk on Windows. Pathname revalidation is not an identity guarantee, so
    // static serving fails closed there until the native bridge owns this open.
    #[cfg(windows)]
    {
        let _ = (root, candidate);
        return None;
    }
    #[cfg(not(windows))]
    {
    let canonical = std::fs::canonicalize(candidate).ok()?;
    if !canonical.starts_with(root) { return None; }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        const O_NOFOLLOW: i32 = 0o400000;
        options.custom_flags(O_NOFOLLOW);
    }
    let file = options.open(&canonical).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() { return None; }
    if std::fs::canonicalize(candidate).ok()? != canonical { return None; }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::fd::AsRawFd;
        let held = std::fs::canonicalize(format!("/proc/self/fd/{}", file.as_raw_fd())).ok()?;
        if !held.starts_with(root) { return None; }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let current = std::fs::metadata(&canonical).ok()?;
        if current.dev() != metadata.dev() || current.ino() != metadata.ino() { return None; }
    }
        Some((file, metadata, canonical))
    }
}

fn jet_http_srv_static_files(
    req: &JetHttpRequest,
    root: &std::path::Path,
) -> Result<JetHttpResponse, String> {
    let not_found = || Ok(jet_http_srv_empty_response(404));
    if !matches!(req.method.as_str(), "GET" | "HEAD") { return Ok(jet_http_srv_empty_response(405)); }
    let root = match std::fs::canonicalize(root) { Ok(root) => root, Err(_) => return not_found() };
    let path = req.path.split('?').next().unwrap_or(&req.path);
    let mut candidate = root.clone();
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        let Ok(segment) = jet_http_route_decode_segment(segment) else { return not_found() };
        if segment == "." || segment == ".." || segment.contains(std::path::MAIN_SEPARATOR) { return not_found(); }
        candidate.push(segment);
        let Ok(metadata) = std::fs::symlink_metadata(&candidate) else { return not_found() };
        if metadata.file_type().is_symlink() { return not_found(); }
    }
    if candidate.is_dir() {
        candidate.push("index.html");
        let Ok(metadata) = std::fs::symlink_metadata(&candidate) else { return not_found() };
        if metadata.file_type().is_symlink() { return not_found(); }
    }
    let Some((mut file, metadata, canonical)) = jet_http_static_open(&root, &candidate) else { return not_found() };
    let file_len = usize::try_from(metadata.len()).map_err(|_| "static file is too large".to_string())?;
    let modified_seconds = metadata.modified().ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64).unwrap_or(0);
    let last_modified = jet_http_date(modified_seconds);
    let etag = format!("\"{:x}-{:x}\"", file_len, modified_seconds);
    if req.headers.all("if-none-match").iter().any(|value| *value == &etag || *value == "*") {
        let mut response = jet_http_srv_empty_response(304);
        response.headers.append("etag", &etag).expect("generated ETag is valid");
        response.headers.append("last-modified", &last_modified).expect("generated date is valid");
        return Ok(response);
    }
    if req.headers.get("if-none-match").is_none()
        && req.headers.get("if-modified-since").and_then(|value| jet_http_date_parse(value)).is_some_and(|since| modified_seconds <= since)
    {
        let mut response = jet_http_srv_empty_response(304);
        response.headers.append("etag", &etag).expect("generated ETag is valid");
        response.headers.append("last-modified", &last_modified).expect("generated date is valid");
        return Ok(response);
    }
    let range = req.headers.get("range").cloned().filter(|_| {
        req.headers.get("if-range").is_none_or(|value| value == &etag || value == &last_modified)
    });
    let (status, start, length, content_range) = if let Some(range) = range {
        let Some((start, end)) = jet_http_static_range(&range, file_len) else {
            let mut response = jet_http_srv_empty_response(416);
            response.headers.append("content-range", &format!("bytes */{file_len}")).expect("generated range is valid");
            return Ok(response);
        };
        (206, start, end - start + 1, Some(format!("bytes {start}-{end}/{file_len}")))
    } else {
        (200, 0, file_len, None)
    };
    use std::io::Seek;
    file.seek(std::io::SeekFrom::Start(start as u64))
        .map_err(|_| "static file seek failed".to_string())?;
    let mut response = jet_http_srv_empty_response(status);
    response.body = JetHttpBody::reader(std::io::Read::take(file, length as u64), Some(length));
    response.headers.append("content-type", jet_http_static_mime(&canonical)).expect("static MIME is valid");
    response.headers.append("accept-ranges", "bytes").expect("static header is valid");
    response.headers.append("etag", &etag).expect("generated ETag is valid");
    response.headers.append("last-modified", &last_modified).expect("generated date is valid");
    if let Some(content_range) = content_range {
        response.headers.append("content-range", &content_range).expect("generated range is valid");
    }
    Ok(response)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetHttpAccessEvent {
    request_id: String,
    method: String,
    path: String,
    route_template: String,
    status: i64,
    bytes: i64,
    duration_ms: i64,
    peer: String,
    protocol: String,
    tls: bool,
}

impl std::fmt::Display for JetHttpAccessEvent {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(output, "request_id={} method={} path={} route={} status={} bytes={} duration_ms={} peer={} protocol={} tls={}",
            self.request_id, self.method, self.path, self.route_template, self.status,
            self.bytes, self.duration_ms, self.peer, self.protocol, self.tls)
    }
}

fn jet_http_srv_access_event(
    req: &JetHttpRequest,
    status: i64,
    bytes: i64,
    duration_ms: i64,
    peer: &str,
    protocol: &str,
    tls: bool,
) -> JetHttpAccessEvent {
    let path = req.path.split('?').next().unwrap_or(&req.path).to_string();
    JetHttpAccessEvent {
        request_id: req.headers.get("x-request-id").cloned().unwrap_or_default(),
        method: req.method.clone(),
        route_template: req.route_template.clone().unwrap_or_else(|| path.clone()),
        path,
        status,
        bytes,
        duration_ms,
        peer: peer.to_string(),
        protocol: protocol.to_string(),
        tls,
    }
}

fn jet_http_srv_access_log(req: &JetHttpRequest, status: i64) -> String {
    let route = req.route_template.as_deref().unwrap_or_else(|| req.path.split('?').next().unwrap_or(&req.path));
    format!("{} {} {}", req.method, route, status)
}
