// ── D-HTTPLIB1=A / D-HTTPLIB2=B: core.http.server — function-first mux ───────
// Plain HTTP is pure std. D-TLSSERVE1=A routes server TLS through the hidden
// rustls bridge only when the named `tls:` option is used.

const JET_HTTP_KEEPALIVE_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const JET_HTTP_MAX_REQUESTS_PER_CONNECTION: usize = 1000;
const JET_HTTP_MAX_BODY_BYTES: usize = 1024 * 1024 * 1024;
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
            if length > JET_HTTP_MAX_BODY_BYTES {
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
            let head = jet_http_validate_headers(&pending[..header_end])?;
            if !reading_body {
                reading_body = true;
                idle_deadline = std::time::Instant::now()
                    + options.read_body_timeout.min(options.read_idle_timeout);
            }
            let body_start = header_end + 4;
            let request_end = match head.framing {
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
    req: &JetHttpRequest,
    path: &String,
    mime: &String,
) -> Result<JetHttpResponse, String> {
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

fn jet_http_srv_access_log(req: &JetHttpRequest, status: i64) -> String {
    let route = req.route_template.as_deref().unwrap_or_else(|| req.path.split('?').next().unwrap_or(&req.path));
    format!("{} {} {}", req.method, route, status)
}
