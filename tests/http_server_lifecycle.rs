#![allow(dead_code, non_camel_case_types, unexpected_cfgs)]

struct JetTcpListener {
    inner: std::net::TcpListener,
}

trait JetShow {
    fn jet_show(&self) -> String;
}

trait user_Encode {}
impl<T> user_Encode for T {}

trait user_Decode: Sized {}

fn jet_enc_json_to_string<T: user_Encode>(_value: &T) -> String {
    String::new()
}

fn jet_enc_json_decode<T: user_Decode>(_text: &str) -> Result<T, String> {
    Err("unused test decoder".to_string())
}

struct JetFileReader {
    inner: Box<dyn std::io::Read + Send>,
}

struct JetFileWriter {
    inner: Box<dyn std::io::Write + Send>,
}

mod jet_std {
    #[derive(Clone, Copy)]
    pub struct Duration {
        pub ms: i64,
    }

    pub struct JetMime(pub String);

    impl JetMime {
        pub fn to_string_value(&self) -> String {
            self.0.clone()
        }
    }
}

enum JetParaRuntimeFailure {
    SchedulerFatal { msg: String },
}

thread_local! {
    static JET_PARA_DEFER_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static JET_IN_SCHEDULER_TASK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static JET_OBSERVE_TASK_ID: std::cell::Cell<usize> = const { std::cell::Cell::new(1) };
    static TEST_DEADLINE_EXCEEDED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn jet_scheduler_task_panic_enter() { JET_IN_SCHEDULER_TASK.with(|task| task.set(true)); }
fn jet_scheduler_task_panic_leave() { JET_IN_SCHEDULER_TASK.with(|task| task.set(false)); }
fn jet_scheduler_panic_should_unwind() -> bool { JET_IN_SCHEDULER_TASK.with(|task| task.get()) }
fn jet_deadline_remaining_ms() -> Option<i64> {
    TEST_DEADLINE_EXCEEDED.with(|deadline| deadline.get().then_some(0))
}
fn jet_deadline_exceeded(wait_kind: &str) -> ! {
    std::panic::resume_unwind(Box::new(JetDeadlineUnwind {
        rendered: format!("deadline exceeded: {wait_kind}"),
    }))
}

#[derive(Clone)]
struct JetObserveTask {
    parent: usize,
    state: &'static str,
    wait: String,
    deadline_ms: Option<i64>,
    cancelled: bool,
}

#[derive(Clone)]
struct JetObserveChannel {
    depth: usize,
    capacity: Option<usize>,
    send_waiters: usize,
    recv_waiters: usize,
    closed: bool,
}

struct JetObserveRegistry {
    next_task: std::sync::atomic::AtomicUsize,
    next_channel: std::sync::atomic::AtomicUsize,
    tasks: std::sync::Mutex<std::collections::HashMap<usize, JetObserveTask>>,
    channels: std::sync::Mutex<std::collections::HashMap<usize, JetObserveChannel>>,
}

static JET_OBSERVE_WORKERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static JET_OBSERVE_QUEUED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn jet_observe_registry() -> Option<&'static std::sync::Arc<JetObserveRegistry>> { None }
fn jet_observe_task_update(_state: &'static str, _wait: &str, _deadline_ms: Option<i64>) {}

include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/HttpMessage.rs");
include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/HttpRoute.rs");
include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/HttpClient.rs");
include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/HttpServer.rs");
include!("../crates/jet-codegen/src/Prelude/Scheduler.rs");

static HTTP_BODY_CLOSES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static HTTP_BODY_READS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static HTTP_H2_BRIDGE_CLOSES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

fn unread_bridge_body(
    _handle: i64,
    _max_chunk: usize,
) -> Result<Option<Vec<u8>>, JetHttpError> {
    HTTP_BODY_READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Ok(Some(vec![1]))
}

fn close_unread_bridge_body(_handle: i64) {
    HTTP_BODY_CLOSES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

fn close_h2_bridge_body(_handle: i64) {
    HTTP_H2_BRIDGE_CLOSES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

#[test]
fn lazy_bridge_body_closes_once_after_concurrent_final_drops() {
    HTTP_BODY_CLOSES.store(0, std::sync::atomic::Ordering::SeqCst);
    for round in 0..64 {
        let body = JetHttpBody::bridge(
            round,
            Some(1),
            unread_bridge_body,
            close_unread_bridge_body,
        );
        let owners = (0..8).map(|_| body.clone()).collect::<Vec<_>>();
        drop(body);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(owners.len() + 1));
        let threads = owners
            .into_iter()
            .map(|owner| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    drop(owner);
                })
            })
            .collect::<Vec<_>>();

        assert_eq!(
            HTTP_BODY_CLOSES.load(std::sync::atomic::Ordering::SeqCst),
            round as usize,
            "body closed before its final owners were released",
        );
        barrier.wait();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(
            HTTP_BODY_CLOSES.load(std::sync::atomic::Ordering::SeqCst),
            round as usize + 1,
            "concurrent final drops must close exactly once",
        );
    }
}

fn read_response(stream: &mut std::net::TcpStream) -> String {
    use std::io::Read;

    let mut response = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let read = stream.read(&mut chunk).expect("response read");
        if read == 0 {
            break;
        }
        response.extend_from_slice(&chunk[..read]);
        let Some(header_end) = jet_http_header_end(&response) else { continue };
        let header = std::str::from_utf8(&response[..header_end]).expect("response header UTF-8");
        let content_length = header
            .split("\r\n")
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("response Content-Length"))
            })
            .expect("response Content-Length");
        if response.len() >= header_end + 4 + content_length {
            response.truncate(header_end + 4 + content_length);
            break;
        }
    }
    String::from_utf8(response).expect("response UTF-8")
}

fn request(addr: std::net::SocketAddr, text: &'static [u8]) -> String {
    use std::io::Write;
    let mut stream = std::net::TcpStream::connect(addr).expect("connect");
    stream.write_all(text).expect("request write");
    read_response(&mut stream)
}

fn h2_frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let len = payload.len();
    let mut out = vec![(len >> 16) as u8, (len >> 8) as u8, len as u8, kind, flags];
    out.extend_from_slice(&(stream & 0x7fff_ffff).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

fn h2_request_headers(method: u8, path: &str) -> Vec<u8> {
    let mut headers = vec![method, 0x86, 0x04, path.len() as u8];
    headers.extend_from_slice(path.as_bytes());
    headers.extend_from_slice(&[0x41, 0x05]);
    headers.extend_from_slice(b"local");
    headers
}

fn h2_read_frame(stream: &mut std::net::TcpStream) -> (u8, u8, u32, Vec<u8>) {
    use std::io::Read;
    let mut header = [0u8; 9];
    stream.read_exact(&mut header).unwrap();
    let length = (usize::from(header[0]) << 16) | (usize::from(header[1]) << 8) | usize::from(header[2]);
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload).unwrap();
    (header[3], header[4], u32::from_be_bytes(header[5..9].try_into().unwrap()) & 0x7fff_ffff, payload)
}

#[test]
fn shared_headers_preserve_repeats_validate_and_redact() {
    let mut headers = JetHttpHeaders::new();
    headers.append("X-Trace", "one").unwrap();
    headers.append("x-trace", "two").unwrap();
    headers.append("Authorization", "Bearer secret").unwrap();
    assert_eq!(headers.first("X-TRACE"), Some("one"));
    assert_eq!(headers.all("x-trace"), vec!["one", "two"]);
    assert!(headers.append("bad name", "x").is_err());
    assert!(headers.append("x-safe", "bad\r\nvalue").is_err());
    for invalid in ["vertical\u{000b}tab", "form\u{000c}feed", "nonbreaking\u{00a0}space", "delete\u{007f}"] {
        assert!(headers.append("x-safe", invalid).is_err(), "accepted {invalid:?}");
    }
    assert_eq!(
        jet_http_validate_headers(b"GET / HTTP/1.1\r\nBad Name: value")
            .unwrap_err()
            .status,
        400
    );
    assert_eq!(
        jet_http_validate_headers(b"GET / HTTP/1.1\r\nX-Safe: bad\0value")
            .unwrap_err()
            .status,
        400
    );
    let shown = format!("{headers:?}");
    assert!(!shown.contains("Bearer secret"), "secret header leaked: {shown}");
}

#[test]
fn shared_http_body_streams_bytes_once_and_uses_closed_errors() {
    let body = JetHttpBody::reader(std::io::Cursor::new(vec![0, 255, 1, 2]), Some(4));
    let alias = body.clone();
    let chunks = body
        .chunks(2)
        .expect("first body consumer")
        .collect::<Result<Vec<_>, _>>()
        .expect("stream chunks");
    assert_eq!(chunks, vec![vec![0, 255], vec![1, 2]]);
    assert_eq!(
        alias.bytes(8),
        Err(JetHttpError::BodyConsumed),
        "a cloned message must not make a move-only body reusable",
    );

    let oversized = JetHttpBody::from_bytes(vec![1, 2, 3]);
    assert_eq!(
        oversized.bytes(2),
        Err(JetHttpError::BodyTooLarge { limit: 2 }),
    );
    assert_eq!(
        JetHttpBody::from_bytes(vec![0xff]).text(8),
        Err(JetHttpError::UnsupportedEncoding),
    );

    let mut streamed = JetHttpBody::reader(std::io::Cursor::new(b"abcdef".to_vec()), None)
        .chunks(2)
        .unwrap();
    let mut observed = Vec::new();
    while let Some(chunk) = streamed.next() {
        observed.extend(chunk.unwrap());
    }
    assert_eq!(observed, b"abcdef");
}

#[test]
fn shared_http_body_multipart_boundary_is_bounded_and_collision_free() {
    let values = std::collections::BTreeMap::from([
        (
            "jet-http-boundary-0000000000000001\r\nname".to_string(),
            "jet-http-boundary-0000000000000000".to_string(),
        ),
    ]);
    let body = JetHttpBody::from_multipart(values);
    assert_eq!(
        body.content_type().as_deref(),
        Some("multipart/form-data; boundary=jet-http-boundary-0000000000000002"),
    );
    let encoded = body.text(1024).unwrap();
    assert!(encoded.starts_with("--jet-http-boundary-0000000000000002\r\n"));
    assert!(encoded.contains("name=\"jet-http-boundary-0000000000000001%0D%0Aname\""));
    assert!(encoded.contains("jet-http-boundary-0000000000000000"));
}

#[test]
fn client_request_and_response_use_the_shared_headers() {
    let req = jet_http_client_request_header(
        jet_http_client_request_header(
            jet_http_client_request_new(&"GET".to_string(), &"http://local/".to_string()),
            &"X-Trace".to_string(),
            &"one".to_string(),
        ),
        &"x-trace".to_string(),
        &"two".to_string(),
    );
    assert_eq!(req.headers.all("X-TRACE"), vec!["one", "two"]);
    let invalid = jet_http_client_request_header(
        req,
        &"bad name".to_string(),
        &"value".to_string(),
    );
    assert!(invalid.header_error.is_some());

    let headers = JetHttpHeaders::from_flat(vec![
        "Set-Cookie".to_string(), "a=1".to_string(),
        "set-cookie".to_string(), "b=2".to_string(),
    ]).unwrap();
    let response = jet_http_srv_response_with_headers(200, "", headers);
    assert_eq!(jet_http_response_cookies(&response), vec!["a=1", "b=2"]);

    let invalid_response = jet_http_srv_response_header(
        jet_http_srv_response(200, &"secret".to_string()),
        &"bad name".to_string(),
        &"value".to_string(),
    );
    assert_eq!(invalid_response.status, 500);
    assert_eq!(invalid_response.body, "500 Internal Server Error");
    assert!(invalid_response.headers.to_flat().is_empty());
}

#[test]
fn live_server_round_trips_repeated_headers_in_order() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let addr = listener.local_addr().expect("address");
    let mux = jet_http_mux_new();
    jet_http_mux_add(&mux, "GET", "/headers", |req| {
        assert_eq!(req.headers.all("x-tag"), vec!["one", "two"]);
        let mut headers = JetHttpHeaders::new();
        headers.append("Set-Cookie", "a=1").unwrap();
        headers.append("set-cookie", "b=2").unwrap();
        jet_http_srv_response_with_headers(200, "ok", headers)
    });
    let client = std::thread::spawn(move || {
        request(addr, b"GET /headers HTTP/1.1\r\nHost: local\r\nX-Tag: one\r\nx-tag: two\r\n\r\n")
    });
    jet_http_mux_serve_once_listener(&JetTcpListener { inner: listener }, &mux).expect("serve once");
    let response = client.join().expect("client");
    let first = response.find("Set-Cookie: a=1\r\n").expect("first repeated header");
    let second = response.find("set-cookie: b=2\r\n").expect("second repeated header");
    assert!(first < second, "repeated header order changed: {response}");
}

#[test]
fn live_shared_messages_round_trip_binary_and_stream_unknown_length() {
    use std::io::{Read, Write};

    fn takes_canonical_request(_: JetHttpRequest) {}
    fn takes_canonical_response(_: JetHttpResponse) {}

    takes_canonical_request(jet_http_client_request_new(
        &"POST".to_string(),
        &"http://local/binary".to_string(),
    ));
    takes_canonical_response(jet_http_srv_response(200, &String::new()));

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let mux = jet_http_mux_new();
    jet_http_mux_add(&mux, "POST", "/binary", |req| {
        assert_eq!(req.headers.all("x-repeat"), vec!["one", "two"]);
        assert_eq!(req.body.bytes(8).unwrap(), vec![0, 255]);
        let mut response = jet_http_srv_response_with_headers(
            200,
            "",
            [
                ("X-Repeat".to_string(), "alpha".to_string()),
                ("x-repeat".to_string(), "beta".to_string()),
            ]
            .into_iter()
            .collect(),
        );
        response.body = JetHttpBody::reader(
            std::io::Cursor::new(vec![255, 0, 1]),
            None,
        );
        response
    });
    let client = std::thread::spawn(move || {
        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        stream
            .write_all(
                b"POST /binary HTTP/1.1\r\nHost: local\r\nX-Repeat: one\r\nx-repeat: two\r\nContent-Length: 2\r\nConnection: close\r\n\r\n\0\xff",
            )
            .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        response
    });
    jet_http_mux_serve_once_listener(&JetTcpListener { inner: listener }, &mux).unwrap();
    let response = client.join().unwrap();
    let header_end = response.windows(4).position(|bytes| bytes == b"\r\n\r\n").unwrap();
    let headers = std::str::from_utf8(&response[..header_end]).unwrap();
    assert!(headers.contains("Transfer-Encoding: chunked\r\n"), "{headers}");
    let first = headers.find("X-Repeat: alpha").unwrap_or_else(|| panic!("{headers}"));
    let second = headers.find("x-repeat: beta").unwrap_or_else(|| panic!("{headers}"));
    assert!(first < second, "{headers}");
    assert_eq!(&response[header_end + 4..], b"3\r\n\xff\0\x01\r\n0\r\n\r\n");
}

#[test]
fn live_server_body_starts_before_the_large_slow_upload_finishes() {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let mux = jet_http_mux_new();
    let (first_chunk_tx, first_chunk_rx) = std::sync::mpsc::channel();
    jet_http_mux_add(&mux, "POST", "/slow", move |req| {
        let mut chunks = req.body.chunks(4096).unwrap();
        let first = chunks.next().unwrap().unwrap();
        first_chunk_tx.send(first.len()).unwrap();
        let mut total = first.len();
        for chunk in chunks {
            total += chunk.unwrap().len();
        }
        jet_http_srv_response(200, &total.to_string())
    });
    let server = std::thread::spawn(move || {
        jet_http_mux_serve_once_listener(&JetTcpListener { inner: listener }, &mux).unwrap();
    });

    let length = 512 * 1024;
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    write!(
        stream,
        "POST /slow HTTP/1.1\r\nHost: local\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    stream.write_all(&vec![7; 4096]).unwrap();
    assert_eq!(
        first_chunk_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("handler must pull before the upload finishes"),
        4096,
    );
    for _ in 1..length / 4096 {
        stream.write_all(&vec![7; 4096]).unwrap();
    }
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    server.join().unwrap();
    assert!(response.ends_with(&length.to_string()), "{response}");
}

#[test]
fn parser_and_framing_share_validated_headers() {
    let request = jet_http_srv_parse(b"GET / HTTP/1.1\r\nHost:local\r\nX-Tag:\tvalue\r\n\r\n")
        .expect("optional header whitespace");
    assert_eq!(request.headers.first("host"), Some("local"));
    assert_eq!(request.headers.first("x-tag"), Some("value"));
    assert_eq!(
        jet_http_srv_parse(b"GET / HTTP/1.1\r\nBad Name:value\r\n\r\n")
            .err()
            .expect("invalid header name")
            .status,
        400
    );
    assert_eq!(
        jet_http_srv_parse(b"GET / HTTP/1.1\r\nX-Safe:bad\0value\r\n\r\n")
            .err()
            .expect("invalid header value")
            .status,
        400
    );

    let response = jet_http_srv_response_header(
        jet_http_srv_response_header(
            jet_http_srv_response_header(
                jet_http_srv_response_header(
                    jet_http_srv_response(200, &"ok".to_string()),
                    &"Content-Length".to_string(),
                    &"999".to_string(),
                ),
                &"Transfer-Encoding".to_string(),
                &"chunked".to_string(),
            ),
            &"Connection".to_string(),
            &"X-Smuggle".to_string(),
        ),
        &"X-Smuggle".to_string(),
        &"leak".to_string(),
    );
    let wire = jet_http_srv_format(&response);
    assert_eq!(wire.matches("Content-Length:").count(), 1, "{wire}");
    assert!(wire.contains("Content-Length: 2\r\n"), "{wire}");
    assert_eq!(wire.matches("Connection:").count(), 1, "{wire}");
    assert!(wire.contains("Connection: close\r\n"), "{wire}");
    assert!(!wire.to_ascii_lowercase().contains("transfer-encoding:"), "{wire}");
    assert!(!wire.to_ascii_lowercase().contains("x-smuggle:"), "{wire}");

    for status in [100, 199, 204, 304] {
        let forbidden = jet_http_srv_format_connection(
            &jet_http_srv_response(status, &"must-not-publish".to_string()),
            "HTTP/1.1",
            false,
        );
        let (headers, body) = forbidden.split_once("\r\n\r\n").expect("response framing");
        assert!(!headers.to_ascii_lowercase().contains("content-length:"), "{forbidden}");
        assert!(body.is_empty(), "status {status} published a body: {forbidden}");
    }
}

#[test]
fn serve_once_waits_for_nonblocking_listener_readiness() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let addr = listener.local_addr().expect("address");
    let mux = jet_http_mux_new();
    jet_http_mux_add(&mux, "GET", "/ready", |_| jet_http_srv_response(200, &"ready".to_string()));
    let client = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(20));
        request(addr, b"GET /ready HTTP/1.1\r\nHost: local\r\n\r\n")
    });
    jet_http_mux_serve_once_listener(&JetTcpListener { inner: listener }, &mux).expect("serve once");
    assert!(client.join().expect("client").contains("ready"));
}

#[test]
fn serve_once_accept_has_a_deadline() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let started = std::time::Instant::now();
    let error = jet_http_accept_once(
        &JetTcpListener { inner: listener },
        std::time::Duration::from_millis(20),
    )
    .expect_err("accept should time out");
    assert!(error.contains("timed out"), "{error}");
    assert!(started.elapsed() < std::time::Duration::from_millis(250));
}

#[test]
fn plain_http11_keepalive_pipelines_in_order_and_closes_at_boundaries() {
    use std::io::{Read, Write};

    fn exchange(addr: std::net::SocketAddr, request: &[u8]) -> String {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.write_all(request).expect("request write");
        stream
            .shutdown(std::net::Shutdown::Write)
            .expect("finish request write");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("response read");
        response
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let mux = jet_http_mux_new();
    jet_http_mux_add(&mux, "POST", "/one", |req| {
        jet_http_srv_response(200, &req.body.text(1024 * 1024).unwrap())
    });
    jet_http_mux_add(&mux, "GET", "/two", |_| jet_http_srv_response(200, &"two".to_string()));
    jet_http_mux_add(&mux, "GET", "/empty", |_| jet_http_srv_response(204, &"forbidden".to_string()));
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let mut options = JetHttpServerOptions::safe();
    options.workers = 1;
    options.admission_queue = 5;
    options.read_header_timeout = std::time::Duration::from_millis(40);
    options.read_idle_timeout = std::time::Duration::from_millis(40);
    let server = std::thread::spawn(move || {
        jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server")
    });

    let pipelined = exchange(
        addr,
        b"POST /one HTTP/1.1\r\nHost: local\r\nContent-Length: 3\r\n\r\noneGET /two HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    let one = pipelined.find("\r\n\r\none").expect("first response");
    let two = pipelined.find("\r\n\r\ntwo").expect("second response");
    assert!(one < two, "pipelined responses reordered: {pipelined}");
    assert_eq!(pipelined.matches("HTTP/1.1 200 OK").count(), 2, "{pipelined}");
    assert_eq!(pipelined.matches("Connection: close").count(), 1, "{pipelined}");

    let http10 = exchange(
        addr,
        b"GET /two HTTP/1.0\r\nHost: local\r\n\r\nGET /two HTTP/1.0\r\nHost: local\r\n\r\n",
    );
    assert_eq!(http10.matches("HTTP/1.0 200 OK").count(), 1, "{http10}");
    assert_eq!(http10.matches("Connection: close").count(), 1, "{http10}");

    let http10_keepalive = exchange(
        addr,
        b"GET /two HTTP/1.0\r\nHost: local\r\nConnection: keep-alive\r\n\r\nGET /two HTTP/1.0\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(http10_keepalive.matches("HTTP/1.0 200 OK").count(), 2, "{http10_keepalive}");
    assert_eq!(http10_keepalive.matches("Connection: close").count(), 1, "{http10_keepalive}");

    let capped_request = "GET /two HTTP/1.1\r\nHost: local\r\n\r\n".repeat(1001);
    let capped = exchange(addr, capped_request.as_bytes());
    assert_eq!(capped.matches("HTTP/1.1 200 OK").count(), 1000, "{capped}");
    assert_eq!(capped.matches("Connection: close").count(), 1, "{capped}");

    let mut partial = std::net::TcpStream::connect(addr).expect("partial connect");
    partial
        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
        .expect("partial read timeout");
    partial
        .write_all(b"GET /two HTTP/1.1\r\nHost: local\r\n\r\nGET /two HTTP/1.1\r\nHost:")
        .expect("partial pipeline write");
    let mut partial_response = String::new();
    partial.read_to_string(&mut partial_response).expect("partial pipeline response");
    assert_eq!(partial_response.matches("HTTP/1.1 200 OK").count(), 1, "{partial_response}");
    assert_eq!(partial_response.matches("HTTP/1.1 408 Request Timeout").count(), 1, "{partial_response}");
    assert!(partial_response.ends_with("Connection: close\r\n\r\n"), "{partial_response}");

    let body_forbidden = exchange(
        addr,
        b"GET /empty HTTP/1.1\r\nHost: local\r\n\r\nGET /two HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    let next = body_forbidden.find("HTTP/1.1 200 OK").expect("response after 204");
    let no_content = &body_forbidden[..next];
    assert!(no_content.starts_with("HTTP/1.1 204 No Content\r\n"), "{body_forbidden}");
    assert!(!no_content.to_ascii_lowercase().contains("content-length:"), "{body_forbidden}");
    assert!(!body_forbidden.contains("forbidden"), "{body_forbidden}");
    assert!(body_forbidden.ends_with("\r\n\r\ntwo"), "{body_forbidden}");

    for version in ["HTTP/2.0", "garbage"] {
        let invalid = exchange(
            addr,
            format!("GET /two {version}\r\nHost: local\r\n\r\nGET /two HTTP/1.1\r\nHost: local\r\n\r\n").as_bytes(),
        );
        assert_eq!(invalid.matches("HTTP/1.1 505 HTTP Version Not Supported").count(), 1, "{invalid}");
        assert!(!invalid.contains("200 OK"), "unsupported version persisted: {invalid}");
        assert!(invalid.ends_with("Connection: close\r\n\r\n"), "{invalid}");
    }

    shutdown.store(true, std::sync::atomic::Ordering::Release);
    let report = server.join().expect("server join");
    assert_eq!(report.user_accepted, 8);
    assert_eq!(report.user_completed, 8);
    assert_eq!(JET_HTTP_KEEPALIVE_IDLE_TIMEOUT, std::time::Duration::from_secs(60));
    assert_eq!(JET_HTTP_MAX_REQUESTS_PER_CONNECTION, 1000);
}

#[test]
fn head_preserves_representation_length_without_a_wire_body() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn exchange(addr: std::net::SocketAddr, request: &[u8]) -> String {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.write_all(request).expect("request write");
        stream.shutdown(std::net::Shutdown::Write).expect("finish request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("response read");
        response
    }

    fn assert_head(response: &str, status: &str, length: usize) {
        let (headers, body) = response.split_once("\r\n\r\n").expect("response framing");
        assert!(headers.starts_with(status), "{response}");
        assert!(headers.contains(&format!("Content-Length: {length}\r\n")), "{response}");
        assert!(body.is_empty(), "HEAD emitted representation bytes: {response}");
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("address");
    let mux = jet_http_mux_new();
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let implicit_calls = calls.clone();
    jet_http_mux_add(&mux, "GET", "/implicit", move |_| {
        implicit_calls.fetch_add(1, Ordering::AcqRel);
        jet_http_srv_response_header(
            jet_http_srv_response(200, &"hello".to_string()),
            &"x-origin".to_string(),
            &"get".to_string(),
        )
    });
    let explicit_calls = calls.clone();
    jet_http_mux_add(&mux, "HEAD", "/explicit", move |_| {
        explicit_calls.fetch_add(1, Ordering::AcqRel);
        jet_http_srv_response(200, &"explicit".to_string())
    });
    jet_http_mux_add(&mux, "POST", "/post", |_| jet_http_srv_response(200, &"posted".to_string()));
    let empty_calls = calls.clone();
    jet_http_mux_add(&mux, "GET", "/empty", move |_| {
        empty_calls.fetch_add(1, Ordering::AcqRel);
        jet_http_srv_response(204, &"forbidden".to_string())
    });
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let mut options = JetHttpServerOptions::safe();
    options.workers = 1;
    options.admission_queue = 8;
    let server = std::thread::spawn(move || {
        jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server")
    });

    let implicit = exchange(
        addr,
        b"HEAD /implicit HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert_head(&implicit, "HTTP/1.1 200 OK", 5);
    assert!(implicit.contains("x-origin: get\r\n"), "HEAD lost GET metadata: {implicit}");

    let explicit = exchange(
        addr,
        b"HEAD /explicit HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert_head(&explicit, "HTTP/1.1 200 OK", 8);

    let missing = exchange(
        addr,
        b"HEAD /missing HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert_head(&missing, "HTTP/1.1 404 Not Found", 13);

    let bad_route = exchange(
        addr,
        b"HEAD /%FF HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert_head(&bad_route, "HTTP/1.1 400 Bad Request", 15);

    let not_allowed = exchange(
        addr,
        b"HEAD /post HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert_head(&not_allowed, "HTTP/1.1 405 Method Not Allowed", 22);

    let no_content = exchange(
        addr,
        b"HEAD /empty HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    let (no_content_headers, no_content_body) = no_content.split_once("\r\n\r\n").expect("204 framing");
    assert!(no_content_headers.starts_with("HTTP/1.1 204 No Content"), "{no_content}");
    assert!(!no_content_headers.to_ascii_lowercase().contains("content-length:"), "{no_content}");
    assert!(no_content_body.is_empty(), "{no_content}");

    let pipelined = exchange(
        addr,
        b"HEAD /implicit HTTP/1.1\r\nHost: local\r\n\r\nGET /implicit HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    let second = pipelined[1..].find("HTTP/1.1 200 OK").map(|index| index + 1).expect("second response");
    let first_response = &pipelined[..second];
    assert!(first_response.contains("Content-Length: 5\r\n"), "{pipelined}");
    assert!(first_response.ends_with("\r\n\r\n"), "HEAD body shifted pipeline: {pipelined}");
    assert!(pipelined[second..].ends_with("\r\n\r\nhello"), "GET response missing after HEAD: {pipelined}");

    assert_eq!(calls.load(Ordering::Acquire), 5);
    shutdown.store(true, Ordering::Release);
    let report = server.join().expect("server join");
    assert_eq!(report.user_accepted, 7);
    assert_eq!(report.user_completed, 7);
}

#[test]
fn reset_content_has_zero_length_and_preserves_pipeline_boundaries() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn exchange(addr: std::net::SocketAddr, request: &[u8]) -> String {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.write_all(request).expect("request write");
        stream.shutdown(std::net::Shutdown::Write).expect("finish request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("response read");
        response
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("address");
    let mux = jet_http_mux_new();
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let reset_calls = calls.clone();
    jet_http_mux_add(&mux, "GET", "/reset", move |_| {
        reset_calls.fetch_add(1, Ordering::AcqRel);
        jet_http_srv_response_header(
            jet_http_srv_response_header(
                jet_http_srv_response_header(
                    jet_http_srv_response(205, &"must-not-publish".to_string()),
                    &"Content-Length".to_string(),
                    &"999".to_string(),
                ),
                &"Transfer-Encoding".to_string(),
                &"chunked".to_string(),
            ),
            &"X-Origin".to_string(),
            &"reset".to_string(),
        )
    });
    let next_calls = calls.clone();
    jet_http_mux_add(&mux, "GET", "/next", move |_| {
        next_calls.fetch_add(1, Ordering::AcqRel);
        jet_http_srv_response(200, &"next".to_string())
    });
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let mut options = JetHttpServerOptions::safe();
    options.workers = 1;
    options.admission_queue = 8;
    let server = std::thread::spawn(move || {
        jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server")
    });

    let pipelined = exchange(
        addr,
        b"GET /reset HTTP/1.1\r\nHost: local\r\n\r\nGET /next HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    let second = pipelined.find("HTTP/1.1 200 OK").expect("successor response");
    let reset = &pipelined[..second];
    assert!(reset.starts_with("HTTP/1.1 205 Reset Content"), "wrong 205 status: {pipelined}");
    assert!(reset.contains("Content-Length: 0\r\n"), "205 was not zero-length: {pipelined}");
    assert!(!reset.to_ascii_lowercase().contains("transfer-encoding:"), "205 retained custom framing: {pipelined}");
    assert!(reset.contains("X-Origin: reset\r\n"), "205 lost representation metadata: {pipelined}");
    assert!(reset.ends_with("\r\n\r\n"), "205 body shifted pipeline: {pipelined}");
    assert!(pipelined[second..].ends_with("\r\n\r\nnext"), "successor response misaligned: {pipelined}");

    let head = exchange(
        addr,
        b"HEAD /reset HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    let (head_headers, head_body) = head.split_once("\r\n\r\n").expect("HEAD 205 framing");
    assert!(head_headers.starts_with("HTTP/1.1 205 Reset Content"), "{head}");
    assert!(head_headers.contains("Content-Length: 0\r\n"), "HEAD metadata overrode 205: {head}");
    assert!(head_body.is_empty(), "HEAD 205 emitted body bytes: {head}");

    assert_eq!(calls.load(Ordering::Acquire), 3);
    shutdown.store(true, Ordering::Release);
    let report = server.join().expect("server join");
    assert_eq!(report.user_accepted, 2);
    assert_eq!(report.user_completed, 2);
}

#[test]
fn invalid_response_statuses_fail_closed_without_breaking_pipelines() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn exchange(addr: std::net::SocketAddr, request: &[u8]) -> String {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.write_all(request).expect("request write");
        stream.shutdown(std::net::Shutdown::Write).expect("finish request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("response read");
        response
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("address");
    let mux = jet_http_mux_new();
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    for (path, status) in [("/low", 99), ("/high", 600)] {
        let handler_calls = calls.clone();
        jet_http_mux_add(&mux, "GET", path, move |_| {
            handler_calls.fetch_add(1, Ordering::AcqRel);
            jet_http_srv_response(status, &format!("secret-status-{status}"))
        });
    }
    let next_calls = calls.clone();
    jet_http_mux_add(&mux, "GET", "/next", move |_| {
        next_calls.fetch_add(1, Ordering::AcqRel);
        jet_http_srv_response(200, &"next".to_string())
    });
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let mut options = JetHttpServerOptions::safe();
    options.workers = 1;
    options.admission_queue = 8;
    let server = std::thread::spawn(move || {
        jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server")
    });

    for path in ["/low", "/high"] {
        let response = exchange(
            addr,
            format!(
                "GET {path} HTTP/1.1\r\nHost: local\r\n\r\nGET /next HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        );
        assert!(response.starts_with("HTTP/1.1 500 Internal Server Error"), "invalid status reached wire: {response}");
        assert!(response.contains("Content-Length: 25\r\n"), "invalid status did not use generic 500 framing: {response}");
        assert!(!response.contains("secret-status"), "invalid response body leaked: {response}");
        assert_eq!(response.matches("HTTP/1.1").count(), 2, "invalid status broke response boundary: {response}");
        assert!(response.ends_with("\r\n\r\nnext"), "successor response misaligned: {response}");
    }

    assert_eq!(calls.load(Ordering::Acquire), 4);
    shutdown.store(true, Ordering::Release);
    let report = server.join().expect("server join");
    assert_eq!(report.user_accepted, 2);
    assert_eq!(report.user_completed, 2);

    for status in [100, 205, 299, 599] {
        let response = jet_http_srv_response(status, &"valid".to_string());
        assert_eq!(response.status, status);
        assert_eq!(response.body, "valid");
    }
    for status in [i64::MIN, -1, 0, 99, 600, 1000, i64::MAX] {
        let response = jet_http_srv_response(status, &"private".to_string());
        assert_eq!(response.status, 500, "invalid status {status} survived");
        assert_eq!(response.body, "500 Internal Server Error");
        assert!(response.headers.to_flat().is_empty());
    }
}

#[test]
fn chunked_requests_are_bounded_strict_and_preserve_pipeline_boundaries() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn exchange(addr: std::net::SocketAddr, request: &[u8]) -> String {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.write_all(request).expect("request write");
        stream.shutdown(std::net::Shutdown::Write).expect("finish request");
        let mut response = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => response.extend_from_slice(&buf[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(error) => panic!("response read: {error}"),
            }
        }
        String::from_utf8(response).expect("response UTF-8")
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("address");
    let mux = jet_http_mux_new();
    let post_calls = std::sync::Arc::new(AtomicUsize::new(0));
    let handler_calls = post_calls.clone();
    jet_http_mux_add_handler(&mux, "POST", "/echo", std::sync::Arc::new(move |req| {
        let body = req.body.text(1024 * 1024)?;
        handler_calls.fetch_add(1, Ordering::AcqRel);
        let length = body.len();
        let preview = (length <= 32).then_some(body.as_str()).unwrap_or("");
        Ok(jet_http_srv_response(200, &format!("{length}:{preview}")))
    }));
    jet_http_mux_add(&mux, "GET", "/next", |_| {
        jet_http_srv_response(200, &"next".to_string())
    });
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let mut options = JetHttpServerOptions::safe();
    options.workers = 1;
    options.admission_queue = 16;
    options.read_header_timeout = std::time::Duration::from_secs(1);
    options.read_idle_timeout = std::time::Duration::from_secs(1);
    options.read_body_timeout = std::time::Duration::from_secs(1);
    let server = std::thread::spawn(move || {
        jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server")
    });

    let pipelined = exchange(
        addr,
        b"POST /echo HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: ChUnKeD\r\n\r\n4;name=value\r\nWiki\r\n5;note=\"quoted value\"\r\npedia\r\n0\r\n\r\nGET /next HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(pipelined.matches("HTTP/1.1 200 OK").count(), 2, "{pipelined}");
    let echoed = pipelined.find("\r\n\r\n9:Wikipedia").expect("decoded chunked body");
    let next = pipelined.rfind("\r\n\r\nnext").expect("pipelined next response");
    assert!(echoed < next, "pipeline order changed: {pipelined}");

    let split_codepoint = exchange(
        addr,
        b"POST /echo HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\n\r\n1\r\n\xc3\r\n1\r\n\xa9\r\n1\r\n!\r\n0\r\n\r\nGET /next HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(split_codepoint.matches("HTTP/1.1 200 OK").count(), 2, "{split_codepoint}");
    let split_body = split_codepoint.find("\r\n\r\n3:é!").expect("split UTF-8 codepoint");
    let split_next = split_codepoint.rfind("\r\n\r\nnext").expect("split pipeline tail");
    assert!(split_body < split_next, "split-codepoint pipeline order changed: {split_codepoint}");

    let limit = 1024 * 1024;
    let exact_request = format!(
        "POST /echo HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{limit:x}\r\n{}\r\n0\r\n\r\n",
        "x".repeat(limit),
    );
    let exact = exchange(addr, exact_request.as_bytes());
    assert!(exact.ends_with(&format!("\r\n\r\n{limit}:")), "exact limit failed: {exact}");

    let over_limit_request = format!(
        "POST /echo HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n100001\r\n{}\r\n0\r\n\r\n",
        "x".repeat(1024 * 1024 + 1),
    );
    let over_limit = exchange(addr, over_limit_request.as_bytes());
    assert!(over_limit.starts_with("HTTP/1.1 413 Payload Too Large"), "{over_limit}");

    for invalid in [
        "POST /echo HTTP/1.0\r\nHost: local\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
        "POST /echo HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
        "POST /echo HTTP/1.1\r\nHost: local\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
        "POST /echo HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: gzip, chunked\r\n\r\n0\r\n\r\n",
        "POST /echo HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\n\r\n+1\r\nx\r\n0\r\n\r\n",
        "POST /echo HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\n\r\n1;bad=\"unterminated\r\nx\r\n0\r\n\r\n",
        "POST /echo HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nxX0\r\n\r\n",
        "POST /echo HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nX-Trailer: hidden\r\n\r\n",
        "POST /echo HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nx\r\n",
    ] {
        let response = exchange(addr, invalid.as_bytes());
        assert!(response.starts_with("HTTP/1.1 400 Bad Request"), "{response}");
        assert_eq!(response.matches("HTTP/1.1").count(), 1, "invalid framing persisted: {response}");
        assert!(response.ends_with("Connection: close\r\n\r\n"), "{response}");
    }

    for invalid_whitespace in [
        "Content-Length:\u{000b}0",
        "Content-Length:\u{000c}0",
        "Content-Length:\u{00a0}0",
        "Transfer-Encoding:\u{00a0}chunked",
        "X-Invalid:\u{007f}",
    ] {
        let request = format!(
            "POST /echo HTTP/1.1\r\nHost: local\r\n{invalid_whitespace}\r\n\r\nGET /next HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n"
        );
        let response = exchange(addr, request.as_bytes());
        assert!(response.starts_with("HTTP/1.1 400 Bad Request"), "{response}");
        assert_eq!(response.matches("HTTP/1.1").count(), 1, "invalid whitespace reused connection: {response}");
        assert!(!response.contains("200 OK"), "invalid whitespace reached a handler: {response}");
    }

    let excessive_framing = format!(
        "POST /echo HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\n\r\n{}0\r\n\r\n",
        "1\r\nx\r\n".repeat(7_000),
    );
    let excessive = exchange(addr, excessive_framing.as_bytes());
    assert!(excessive.starts_with("HTTP/1.1 413 Payload Too Large"), "{excessive}");

    assert_eq!(post_calls.load(Ordering::Acquire), 3, "invalid framing reached handler");
    shutdown.store(true, std::sync::atomic::Ordering::Release);
    let report = server.join().expect("server join");
    assert_eq!(report.user_accepted, 19);
    assert_eq!(report.user_completed, 19);
}

#[test]
fn expect_continue_is_ordered_bounded_and_rejects_before_dispatch() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn read_rest(stream: &mut std::net::TcpStream) -> String {
        let mut response = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => response.extend_from_slice(&buf[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(error) => panic!("response read: {error}"),
            }
        }
        String::from_utf8(response).expect("response UTF-8")
    }

    fn exchange(addr: std::net::SocketAddr, request: &[u8]) -> String {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.write_all(request).expect("request write");
        stream.shutdown(std::net::Shutdown::Write).expect("finish request");
        read_rest(&mut stream)
    }

    fn continue_exchange(addr: std::net::SocketAddr, head: &[u8], body_and_tail: &[u8]) -> String {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.set_read_timeout(Some(std::time::Duration::from_secs(1))).expect("timeout");
        stream.write_all(head).expect("request head");
        let expected = b"HTTP/1.1 100 Continue\r\n\r\n";
        let mut interim = vec![0u8; expected.len()];
        stream.read_exact(&mut interim).expect("100 Continue");
        assert_eq!(interim, expected);
        stream.write_all(body_and_tail).expect("request body");
        stream.shutdown(std::net::Shutdown::Write).expect("finish request");
        let response = read_rest(&mut stream);
        assert!(!response.contains("100 Continue"), "server sent a second interim response: {response}");
        response
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("address");
    let mut arrived_content_length = std::net::TcpStream::connect(addr).expect("pre-arrived CL connect");
    arrived_content_length
        .write_all(b"POST /echo HTTP/1.1\r\nHost: local\r\nContent-Length: 5\r\nExpect: 100-continue\r\nConnection: close\r\n\r\nearly")
        .expect("pre-arrived CL write");
    arrived_content_length.shutdown(std::net::Shutdown::Write).expect("pre-arrived CL finish");
    let mut arrived_chunked = std::net::TcpStream::connect(addr).expect("pre-arrived chunked connect");
    arrived_chunked
        .write_all(b"POST /echo HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\nExpect: 100-continue\r\n\r\n5\r\nearly\r\n0\r\n\r\nGET /next HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n")
        .expect("pre-arrived chunked write");
    arrived_chunked.shutdown(std::net::Shutdown::Write).expect("pre-arrived chunked finish");
    let mux = jet_http_mux_new();
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let post_calls = calls.clone();
    jet_http_mux_add_handler(&mux, "POST", "/echo", std::sync::Arc::new(move |req| {
        let body = req.body.text(1024 * 1024)?;
        post_calls.fetch_add(1, Ordering::AcqRel);
        Ok(jet_http_srv_response(200, &body))
    }));
    let get_calls = calls.clone();
    jet_http_mux_add(&mux, "GET", "/next", move |_| {
        get_calls.fetch_add(1, Ordering::AcqRel);
        jet_http_srv_response(200, &"next".to_string())
    });
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let mut options = JetHttpServerOptions::safe();
    options.workers = 1;
    options.admission_queue = 8;
    options.read_header_timeout = std::time::Duration::from_millis(200);
    options.read_idle_timeout = std::time::Duration::from_millis(200);
    options.read_body_timeout = std::time::Duration::from_millis(200);
    let server = std::thread::spawn(move || {
        jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server")
    });

    let arrived_content_length = read_rest(&mut arrived_content_length);
    assert_eq!(arrived_content_length.matches("HTTP/1.1 200 OK").count(), 1, "{arrived_content_length}");
    assert!(!arrived_content_length.contains("100 Continue"), "fully arrived CL received interim: {arrived_content_length}");
    assert!(arrived_content_length.ends_with("\r\n\r\nearly"), "{arrived_content_length}");
    let arrived_chunked = read_rest(&mut arrived_chunked);
    assert_eq!(arrived_chunked.matches("HTTP/1.1 200 OK").count(), 2, "{arrived_chunked}");
    assert!(!arrived_chunked.contains("100 Continue"), "fully arrived chunked request received interim: {arrived_chunked}");
    assert!(arrived_chunked.find("\r\n\r\nearly").unwrap() < arrived_chunked.rfind("\r\n\r\nnext").unwrap());

    let content_length = continue_exchange(
        addr,
        b"POST /echo HTTP/1.1\r\nHost: local\r\nContent-Length: 5\r\nExpect: 100-continue\r\n\r\n",
        b"helloGET /next HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(content_length.matches("HTTP/1.1 200 OK").count(), 2, "{content_length}");
    assert!(content_length.find("\r\n\r\nhello").unwrap() < content_length.rfind("\r\n\r\nnext").unwrap());

    let chunked = continue_exchange(
        addr,
        b"POST /echo HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\nExpect: 100-continue\r\n\r\n",
        b"5\r\nworld\r\n0\r\n\r\nGET /next HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(chunked.matches("HTTP/1.1 200 OK").count(), 2, "{chunked}");
    assert!(chunked.find("\r\n\r\nworld").unwrap() < chunked.rfind("\r\n\r\nnext").unwrap());

    let oversized = exchange(
        addr,
        b"POST /echo HTTP/1.1\r\nHost: local\r\nContent-Length: 1048577\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n",
    );
    assert!(oversized.starts_with("HTTP/1.1 413 Payload Too Large"), "{oversized}");
    assert!(!oversized.contains("100 Continue"), "oversized body was invited: {oversized}");

    for invalid in [
        "POST /echo HTTP/1.1\r\nHost: local\r\nContent-Length: 1\r\nExpect: fancy\r\n\r\nxGET /next HTTP/1.1\r\nHost: local\r\n\r\n",
        "POST /echo HTTP/1.1\r\nHost: local\r\nContent-Length: 1\r\nExpect: 100-continue\r\nExpect: 100-continue\r\n\r\nxGET /next HTTP/1.1\r\nHost: local\r\n\r\n",
        "POST /echo HTTP/1.0\r\nHost: local\r\nContent-Length: 1\r\nExpect: 100-continue\r\n\r\nxGET /next HTTP/1.1\r\nHost: local\r\n\r\n",
    ] {
        let response = exchange(addr, invalid.as_bytes());
        assert!(response.starts_with("HTTP/1.1 417 Expectation Failed"), "{response}");
        assert!(!response.contains("100 Continue"), "invalid expectation was invited: {response}");
        assert!(!response.contains("200 OK"), "invalid expectation dispatched or reused: {response}");
        assert!(response.ends_with("Connection: close\r\n\r\n"), "{response}");
    }

    assert_eq!(calls.load(Ordering::Acquire), 7, "rejected expectation reached a handler");
    shutdown.store(true, std::sync::atomic::Ordering::Release);
    let report = server.join().expect("server join");
    assert_eq!(report.user_accepted, 8);
    assert_eq!(report.user_completed, 8);
}

#[test]
fn host_authority_is_single_valid_and_required_for_http11() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn exchange(addr: std::net::SocketAddr, request: &[u8]) -> String {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.write_all(request).expect("request write");
        stream.shutdown(std::net::Shutdown::Write).expect("finish request");
        let mut response = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => response.extend_from_slice(&buf[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(error) => panic!("response read: {error}"),
            }
        }
        String::from_utf8(response).expect("response UTF-8")
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("address");
    let mux = jet_http_mux_new();
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let handler_calls = calls.clone();
    jet_http_mux_add(&mux, "GET", "/", move |_| {
        handler_calls.fetch_add(1, Ordering::AcqRel);
        jet_http_srv_response(200, &"ok".to_string())
    });
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let mut options = JetHttpServerOptions::safe();
    options.workers = 1;
    options.admission_queue = 24;
    let server = std::thread::spawn(move || {
        jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server")
    });

    for valid in [
        "GET / HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
        "GET / HTTP/1.1\r\nHost: example.com:8080\r\nConnection: close\r\n\r\n",
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:80\r\nConnection: close\r\n\r\n",
        "GET / HTTP/1.1\r\nHost: percent%2Dname.example\r\nConnection: close\r\n\r\n",
        "GET / HTTP/1.1\r\nHost: [::1]:443\r\nConnection: close\r\n\r\n",
        "GET / HTTP/1.1\r\nHost: [v1.fe80::a]:80\r\nConnection: close\r\n\r\n",
        "GET / HTTP/1.0\r\nConnection: close\r\n\r\n",
    ] {
        let response = exchange(addr, valid.as_bytes());
        assert!(response.contains("200 OK"), "valid authority rejected: {response}");
    }

    for invalid_host in [
        "",
        "Host:\r\n",
        "Host: one\r\nHost: two\r\n",
        "Host: one,two\r\n",
        "Host: user@local\r\n",
        "Host: local/path\r\n",
        "Host: ::1\r\n",
        "Host: [::1\r\n",
        "Host: []\r\n",
        "Host: [127.0.0.1]\r\n",
        "Host: [v.fe]\r\n",
        "Host: [v1.]\r\n",
        "Host: local:abc\r\n",
        "Host: local:65536\r\n",
        "Host: bad host\r\n",
        "Host: bad%ZZ\r\n",
    ] {
        let request = format!(
            "GET / HTTP/1.1\r\n{invalid_host}\r\nGET / HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n"
        );
        let response = exchange(addr, request.as_bytes());
        assert!(response.starts_with("HTTP/1.1 400 Bad Request"), "accepted invalid Host {invalid_host:?}: {response}");
        assert!(!response.contains("200 OK"), "invalid Host dispatched or reused: {response}");
        assert_eq!(response.matches("HTTP/1.1").count(), 1, "invalid Host reused connection: {response}");
        assert!(response.ends_with("Connection: close\r\n\r\n"), "{response}");
    }

    for invalid_host in [
        "Host: bad host\r\n",
        "Host: one\r\nHost: two\r\n",
    ] {
        let request = format!(
            "GET / HTTP/1.0\r\n{invalid_host}\r\nGET / HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n"
        );
        let response = exchange(addr, request.as_bytes());
        assert!(response.starts_with("HTTP/1.1 400 Bad Request"), "HTTP/1.0 accepted invalid Host {invalid_host:?}: {response}");
        assert!(!response.contains("200 OK"), "invalid HTTP/1.0 Host dispatched or reused: {response}");
        assert_eq!(response.matches("HTTP/1.1").count(), 1, "invalid HTTP/1.0 Host reused connection: {response}");
        assert!(response.ends_with("Connection: close\r\n\r\n"), "{response}");
    }

    assert_eq!(calls.load(Ordering::Acquire), 7, "invalid Host reached handler");
    shutdown.store(true, Ordering::Release);
    let report = server.join().expect("server join");
    assert_eq!(report.user_accepted, 25);
    assert_eq!(report.user_completed, 25);
}

#[test]
fn absolute_form_target_matches_host_before_routing() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn exchange(addr: std::net::SocketAddr, request: &[u8]) -> String {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.write_all(request).expect("request write");
        stream.shutdown(std::net::Shutdown::Write).expect("finish request");
        let mut response = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => response.extend_from_slice(&buf[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(error) => panic!("response read: {error}"),
            }
        }
        String::from_utf8(response).expect("response UTF-8")
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("address");
    let mux = jet_http_mux_new();
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let handler_calls = calls.clone();
    jet_http_mux_add(&mux, "GET", "/resource", move |req| {
        handler_calls.fetch_add(1, Ordering::AcqRel);
        jet_http_srv_response(200, &req.path)
    });
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let mut options = JetHttpServerOptions::safe();
    options.workers = 1;
    options.admission_queue = 16;
    let server = std::thread::spawn(move || {
        jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server")
    });

    for (request, expected_path) in [
        ("GET http://local/resource?x=1 HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n", "/resource?x=1"),
        ("GET HTTP://EXAMPLE.COM:80/resource HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n", "/resource"),
        ("GET http://local:8080/resource HTTP/1.1\r\nHost: local:8080\r\nConnection: close\r\n\r\n", "/resource"),
        ("GET https://[::1]/resource HTTP/1.1\r\nHost: [::1]:443\r\nConnection: close\r\n\r\n", "/resource"),
        ("GET http://percent%2Dname.example/resource HTTP/1.1\r\nHost: percent%2dname.example:80\r\nConnection: close\r\n\r\n", "/resource"),
        ("GET http://local/resource HTTP/1.0\r\nConnection: close\r\n\r\n", "/resource"),
        ("GET /resource?x=%2F:@!$&'()*+,;=~._-/? HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n", "/resource?x=%2F:@!$&'()*+,;=~._-/?"),
        ("GET http://local/resource?x=%2f:@!$&'()*+,;=~._-/? HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n", "/resource?x=%2f:@!$&'()*+,;=~._-/?"),
    ] {
        let response = exchange(addr, request.as_bytes());
        assert!(response.starts_with("HTTP/1.") && response.contains(" 200 OK\r\n"), "valid absolute-form rejected: {response}");
        assert!(response.ends_with(&format!("\r\n\r\n{expected_path}")), "target was not normalized once: {response}");
    }

    for invalid_target in [
        "GET http://one/resource HTTP/1.1\r\nHost: two\r\n",
        "GET http://local:8080/resource HTTP/1.1\r\nHost: local\r\n",
        "GET http://user@local/resource HTTP/1.1\r\nHost: local\r\n",
        "GET http://local/resource#fragment HTTP/1.1\r\nHost: local\r\n",
        "GET ftp://local/resource HTTP/1.1\r\nHost: local\r\n",
        "GET relative HTTP/1.1\r\nHost: local\r\n",
        "GET http://local\t/resource HTTP/1.1\r\nHost: local\r\n",
        "GET http:///resource HTTP/1.1\r\nHost: local\r\n",
        "GET /resource% HTTP/1.1\r\nHost: local\r\n",
        "GET http://local/resource%zz HTTP/1.1\r\nHost: local\r\n",
        "GET /resource\\evil HTTP/1.1\r\nHost: local\r\n",
        "GET /resource?x=<bad> HTTP/1.1\r\nHost: local\r\n",
        "GET /resource?x=é HTTP/1.1\r\nHost: local\r\n",
        "GET /resource#fragment HTTP/1.1\r\nHost: local\r\n",
        "GET * HTTP/1.1\r\nHost: local\r\n",
    ] {
        let request = format!(
            "{invalid_target}\r\nGET /resource HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n"
        );
        let response = exchange(addr, request.as_bytes());
        assert!(response.starts_with("HTTP/1.1 400 Bad Request"), "accepted invalid target: {response}");
        assert!(!response.contains("200 OK"), "invalid target dispatched or reused: {response}");
        assert_eq!(response.matches("HTTP/1.1").count(), 1, "invalid target reused connection: {response}");
    }

    let mismatched_expect = exchange(
        addr,
        b"POST http://one/resource HTTP/1.1\r\nHost: two\r\nContent-Length: 1\r\nExpect: 100-continue\r\n\r\n",
    );
    assert!(mismatched_expect.starts_with("HTTP/1.1 400 Bad Request"), "{mismatched_expect}");
    assert!(!mismatched_expect.contains("100 Continue"), "mismatch received body permission: {mismatched_expect}");

    let malformed_expect = exchange(
        addr,
        b"POST /resource?x=%zz HTTP/1.1\r\nHost: local\r\nContent-Length: 1\r\nExpect: 100-continue\r\n\r\n",
    );
    assert!(malformed_expect.starts_with("HTTP/1.1 400 Bad Request"), "{malformed_expect}");
    assert!(!malformed_expect.contains("100 Continue"), "malformed target received body permission: {malformed_expect}");

    assert_eq!(calls.load(Ordering::Acquire), 8, "invalid target reached handler");
    shutdown.store(true, Ordering::Release);
    let report = server.join().expect("server join");
    assert_eq!(report.user_accepted, 25);
    assert_eq!(report.user_completed, 25);
}

#[test]
fn request_method_is_one_http_token_before_body_or_dispatch() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn exchange(addr: std::net::SocketAddr, request: &[u8]) -> String {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.write_all(request).expect("request write");
        stream.shutdown(std::net::Shutdown::Write).expect("finish request");
        let mut response = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => response.extend_from_slice(&buf[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(error) => panic!("response read: {error}"),
            }
        }
        String::from_utf8(response).expect("response UTF-8")
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("address");
    let mux = jet_http_mux_new();
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    for method in ["GET", "M-SEARCH", "custom!#$%&'*+-.^_`|~"] {
        let handler_calls = calls.clone();
        jet_http_mux_add(&mux, method, "/resource", move |req| {
            handler_calls.fetch_add(1, Ordering::AcqRel);
            jet_http_srv_response(200, &req.method)
        });
    }
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let mut options = JetHttpServerOptions::safe();
    options.workers = 1;
    options.admission_queue = 16;
    let server = std::thread::spawn(move || {
        jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server")
    });

    for method in ["GET", "M-SEARCH", "custom!#$%&'*+-.^_`|~"] {
        let response = exchange(
            addr,
            format!("{method} /resource HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n").as_bytes(),
        );
        assert!(response.starts_with("HTTP/1.1 200 OK"), "valid method rejected: {response}");
        assert!(response.ends_with(&format!("\r\n\r\n{method}")), "method changed before dispatch: {response}");
    }

    for invalid_method in ["G/ET", "G:ET", "G\\ET", "GÉT", "GET\t", "GE(T", "", "\x7fGET"] {
        let request = format!(
            "{invalid_method} /resource HTTP/1.1\r\nHost: local\r\n\r\nGET /resource HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n"
        );
        let response = exchange(addr, request.as_bytes());
        assert!(response.starts_with("HTTP/1.1 400 Bad Request"), "accepted invalid method: {response}");
        assert!(!response.contains("200 OK"), "invalid method dispatched or reused: {response}");
        assert_eq!(response.matches("HTTP/1.1").count(), 1, "invalid method reused connection: {response}");
    }

    let invalid_expect = exchange(
        addr,
        b"G/ET /resource HTTP/1.1\r\nHost: local\r\nContent-Length: 1\r\nExpect: 100-continue\r\n\r\n",
    );
    assert!(invalid_expect.starts_with("HTTP/1.1 400 Bad Request"), "{invalid_expect}");
    assert!(!invalid_expect.contains("100 Continue"), "invalid method received body permission: {invalid_expect}");

    assert_eq!(calls.load(Ordering::Acquire), 3, "invalid method reached handler");
    shutdown.store(true, Ordering::Release);
    let report = server.join().expect("server join");
    assert_eq!(report.user_accepted, 12);
    assert_eq!(report.user_completed, 12);
}

#[test]
fn request_methods_are_case_sensitive_during_routing() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn exchange(addr: std::net::SocketAddr, request: &[u8]) -> String {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.write_all(request).expect("request write");
        stream.shutdown(std::net::Shutdown::Write).expect("finish request");
        let mut response = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => response.extend_from_slice(&buf[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(error) => panic!("response read: {error}"),
            }
        }
        String::from_utf8(response).expect("response UTF-8")
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("address");
    let mux = jet_http_mux_new();
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let handler_calls = calls.clone();
    jet_http_mux_add(&mux, "GET", "/resource", move |_| {
        handler_calls.fetch_add(1, Ordering::AcqRel);
        jet_http_srv_response(200, &"GET".to_string())
    });
    let handler_calls = calls.clone();
    jet_http_mux_add(&mux, "get", "/resource", move |_| {
        handler_calls.fetch_add(1, Ordering::AcqRel);
        jet_http_srv_response(200, &"get".to_string())
    });
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let mut options = JetHttpServerOptions::safe();
    options.workers = 1;
    options.admission_queue = 16;
    let server = std::thread::spawn(move || {
        jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server")
    });

    for method in ["GET", "get"] {
        let response = exchange(
            addr,
            format!("{method} /resource HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n").as_bytes(),
        );
        assert!(response.starts_with("HTTP/1.1 200 OK"), "case-distinct route rejected: {response}");
        assert!(response.ends_with(&format!("\r\n\r\n{method}")), "wrong case-distinct route dispatched: {response}");
    }

    for method in ["GeT", "head", "options"] {
        let response = exchange(
            addr,
            format!(
                "{method} /resource HTTP/1.1\r\nHost: local\r\n\r\nGET /resource HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        );
        assert!(response.starts_with("HTTP/1.1 405 Method Not Allowed"), "{method} matched uppercase semantics: {response}");
        assert!(response.contains("Allow: GET, HEAD, OPTIONS, get\r\n"), "wrong exact Allow methods: {response}");
        assert_eq!(response.matches("HTTP/1.1 200 OK").count(), 1, "valid unmatched method broke reuse: {response}");
        assert!(response.ends_with("\r\n\r\nGET"), "uppercase successor did not dispatch: {response}");
    }

    assert_eq!(calls.load(Ordering::Acquire), 5, "method case changed before routing");
    shutdown.store(true, Ordering::Release);
    let report = server.join().expect("server join");
    assert_eq!(report.user_accepted, 5);
    assert_eq!(report.user_completed, 5);
}

#[test]
fn options_asterisk_reports_server_methods_without_dispatch() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn exchange(addr: std::net::SocketAddr, request: &[u8]) -> String {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.write_all(request).expect("request write");
        stream.shutdown(std::net::Shutdown::Write).expect("finish request");
        let mut response = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => response.extend_from_slice(&buf[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(error) => panic!("response read: {error}"),
            }
        }
        String::from_utf8(response).expect("response UTF-8")
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("address");
    let mux = jet_http_mux_new();
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    for (method, pattern) in [("GET", "/one"), ("POST", "/two"), ("get", "/three")] {
        let handler_calls = calls.clone();
        jet_http_mux_add(&mux, method, pattern, move |_| {
            handler_calls.fetch_add(1, Ordering::AcqRel);
            jet_http_srv_response(200, &"handled".to_string())
        });
    }
    let middleware_calls = std::sync::Arc::new(AtomicUsize::new(0));
    let middleware_events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let wrapper_calls = middleware_calls.clone();
    let wrapper_events = middleware_events.clone();
    jet_http_mux_middleware(&mux, move |next| {
        let calls = wrapper_calls.clone();
        let events = wrapper_events.clone();
        std::sync::Arc::new(move |req| {
            calls.fetch_add(1, Ordering::AcqRel);
            events.lock().unwrap().push(format!("{}:{}", req.method, req.path));
            next(req).map(|response| {
                jet_http_srv_response_header(
                    response,
                    &"X-Middleware".to_string(),
                    &"observed".to_string(),
                )
            })
        })
    });
    jet_http_mux_middleware(&mux, move |next| {
        std::sync::Arc::new(move |req| {
            if jet_http_srv_req_header(&req, &"X-Block".to_string()).is_some() {
                Ok(jet_http_srv_response(403, &"blocked".to_string()))
            } else {
                next(req)
            }
        })
    });
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let mut options = JetHttpServerOptions::safe();
    options.workers = 1;
    options.admission_queue = 16;
    let server = std::thread::spawn(move || {
        jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server")
    });

    let standalone = exchange(
        addr,
        b"OPTIONS * HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert!(standalone.starts_with("HTTP/1.1 204 No Content"), "server-wide OPTIONS rejected: {standalone}");
    assert!(standalone.contains("Allow: GET, HEAD, OPTIONS, POST, get\r\n"), "wrong server-wide Allow: {standalone}");
    assert_eq!(standalone.matches("X-Middleware: observed\r\n").count(), 1, "middleware did not wrap OPTIONS exactly once: {standalone}");

    let path_local = exchange(
        addr,
        b"OPTIONS /one HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert!(path_local.starts_with("HTTP/1.1 204 No Content"), "path OPTIONS rejected: {path_local}");
    assert!(path_local.contains("Allow: GET, HEAD, OPTIONS\r\n"), "wrong path Allow: {path_local}");
    assert_eq!(path_local.matches("X-Middleware: observed\r\n").count(), 1, "middleware did not wrap path OPTIONS exactly once: {path_local}");

    let blocked = exchange(
        addr,
        b"OPTIONS * HTTP/1.1\r\nHost: local\r\nX-Block: yes\r\nConnection: close\r\n\r\n",
    );
    assert!(blocked.starts_with("HTTP/1.1 403 Forbidden"), "middleware could not reject OPTIONS: {blocked}");
    assert!(!blocked.contains("Allow:"), "rejected OPTIONS leaked method inventory: {blocked}");
    assert_eq!(blocked.matches("X-Middleware: observed\r\n").count(), 1, "rejection was not wrapped exactly once: {blocked}");

    let response = exchange(
        addr,
        b"OPTIONS * HTTP/1.1\r\nHost: local\r\n\r\nGET /one HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert!(response.starts_with("HTTP/1.1 204 No Content"), "server-wide OPTIONS rejected: {response}");
    assert!(response.contains("Allow: GET, HEAD, OPTIONS, POST, get\r\n"), "wrong server-wide Allow: {response}");
    assert_eq!(response.matches("X-Middleware: observed\r\n").count(), 2, "middleware did not wrap each pipelined request once: {response}");
    assert_eq!(response.matches("HTTP/1.1 200 OK").count(), 1, "OPTIONS * broke pipeline reuse: {response}");
    assert!(response.ends_with("\r\n\r\nhandled"), "successor request did not dispatch: {response}");

    for request_target in ["GET *", "GeT *", "OPTIONS **", "OPTIONS *?query"] {
        let request = format!(
            "{request_target} HTTP/1.1\r\nHost: local\r\n\r\nGET /one HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n"
        );
        let response = exchange(addr, request.as_bytes());
        assert!(response.starts_with("HTTP/1.1 400 Bad Request"), "invalid asterisk-form accepted: {response}");
        assert!(!response.contains("200 OK"), "invalid asterisk-form dispatched or reused: {response}");
        assert_eq!(response.matches("HTTP/1.1").count(), 1, "invalid asterisk-form reused connection: {response}");
    }

    assert_eq!(calls.load(Ordering::Acquire), 1, "OPTIONS or invalid asterisk-form invoked a handler");
    assert_eq!(middleware_calls.load(Ordering::Acquire), 5, "OPTIONS middleware count changed");
    assert_eq!(&*middleware_events.lock().unwrap(), &[
        "OPTIONS:*", "OPTIONS:/one", "OPTIONS:*", "OPTIONS:*", "GET:/one",
    ]);
    shutdown.store(true, Ordering::Release);
    let report = server.join().expect("server join");
    assert_eq!(report.user_accepted, 8);
    assert_eq!(report.user_completed, 8);
}

#[test]
fn connection_options_are_tokens_before_reuse_or_body_permission() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn exchange(addr: std::net::SocketAddr, request: &[u8]) -> String {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.write_all(request).expect("request write");
        stream.shutdown(std::net::Shutdown::Write).expect("finish request");
        let mut response = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => response.extend_from_slice(&buf[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(error) => panic!("response read: {error}"),
            }
        }
        String::from_utf8(response).expect("response UTF-8")
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("address");
    let mux = jet_http_mux_new();
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let handler_calls = calls.clone();
    jet_http_mux_add(&mux, "GET", "/resource", move |_| {
        handler_calls.fetch_add(1, Ordering::AcqRel);
        jet_http_srv_response(200, &"ok".to_string())
    });
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let mut options = JetHttpServerOptions::safe();
    options.workers = 1;
    options.admission_queue = 16;
    let server = std::thread::spawn(move || {
        jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server")
    });

    let http10_extensions = exchange(
        addr,
        b"GET /resource HTTP/1.0\r\nHost: local\r\nConnection: custom!#$%&'*+-.^_`|~\r\nConnection:\tKEEP-ALIVE \t\r\n\r\nGET /resource HTTP/1.0\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(http10_extensions.matches("HTTP/1.0 200 OK").count(), 2, "{http10_extensions}");

    let empty_members = exchange(
        addr,
        b"GET /resource HTTP/1.1\r\nHost: local\r\nConnection: , ,\r\n\r\nGET /resource HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(empty_members.matches("HTTP/1.1 200 OK").count(), 2, "{empty_members}");

    let close_dominates = exchange(
        addr,
        b"GET /resource HTTP/1.1\r\nHost: local\r\nConnection: keep-alive, extension, CLOSE\r\n\r\nGET /resource HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(close_dominates.matches("HTTP/1.1 200 OK").count(), 1, "{close_dominates}");
    assert!(close_dominates.contains("Connection: close"), "{close_dominates}");

    for invalid_connection in [
        "Connection: \"close\"",
        "Connection: bad/option",
        "Connection: bad;option",
        "Connection: bad option",
        "Connection: café",
        "Connection: keep-alive\r\nConnection: bad/option",
    ] {
        let request = format!(
            "GET /resource HTTP/1.1\r\nHost: local\r\n{invalid_connection}\r\n\r\nGET /resource HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n"
        );
        let response = exchange(addr, request.as_bytes());
        assert!(response.starts_with("HTTP/1.1 400 Bad Request"), "accepted invalid Connection: {response}");
        assert!(!response.contains("200 OK"), "invalid Connection dispatched or reused: {response}");
        assert_eq!(response.matches("HTTP/1.1").count(), 1, "invalid Connection reused socket: {response}");
    }

    let invalid_expect = exchange(
        addr,
        b"GET /resource HTTP/1.1\r\nHost: local\r\nConnection: bad/option\r\nContent-Length: 1\r\nExpect: 100-continue\r\n\r\n",
    );
    assert!(invalid_expect.starts_with("HTTP/1.1 400 Bad Request"), "{invalid_expect}");
    assert!(!invalid_expect.contains("100 Continue"), "invalid Connection received body permission: {invalid_expect}");

    assert_eq!(calls.load(Ordering::Acquire), 5, "invalid Connection reached handler");
    shutdown.store(true, Ordering::Release);
    let report = server.join().expect("server join");
    assert_eq!(report.user_accepted, 10);
    assert_eq!(report.user_completed, 10);
}

#[test]
fn content_length_is_one_identical_decimal_value_before_body_permission() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn exchange(addr: std::net::SocketAddr, request: &[u8]) -> String {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.write_all(request).expect("request write");
        stream.shutdown(std::net::Shutdown::Write).expect("finish request");
        let mut response = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => response.extend_from_slice(&buf[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(error) => panic!("response read: {error}"),
            }
        }
        String::from_utf8(response).expect("response UTF-8")
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("address");
    let mux = jet_http_mux_new();
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let post_calls = calls.clone();
    jet_http_mux_add(&mux, "POST", "/echo", move |req| {
        post_calls.fetch_add(1, Ordering::AcqRel);
        jet_http_srv_response(200, &req.body.text(1024 * 1024).unwrap())
    });
    let get_calls = calls.clone();
    jet_http_mux_add(&mux, "GET", "/next", move |_| {
        get_calls.fetch_add(1, Ordering::AcqRel);
        jet_http_srv_response(200, &"next".to_string())
    });
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let mut options = JetHttpServerOptions::safe();
    options.workers = 1;
    options.admission_queue = 16;
    let server = std::thread::spawn(move || {
        jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server")
    });

    for content_length in [
        "Content-Length: 03",
        "Content-Length: 3, 003",
        "Content-Length: 3\r\nContent-Length: 003",
    ] {
        let response = exchange(
            addr,
            format!("POST /echo HTTP/1.1\r\nHost: local\r\n{content_length}\r\nConnection: close\r\n\r\nabc").as_bytes(),
        );
        assert!(response.starts_with("HTTP/1.1 200 OK"), "valid Content-Length rejected: {response}");
        assert!(response.ends_with("\r\n\r\nabc"), "body boundary changed: {response}");
    }

    for invalid_content_length in [
        "Content-Length: +3",
        "Content-Length: -3",
        "Content-Length: 3, 4",
        "Content-Length: 3,",
        "Content-Length: ,3",
        "Content-Length: 3,,3",
        "Content-Length: 3 3",
        "Content-Length: 3;",
        "Content-Length: café",
        "Content-Length: 184467440737095516160000000000000000000",
        "Content-Length: 3\r\nContent-Length: 4",
    ] {
        let request = format!(
            "POST /echo HTTP/1.1\r\nHost: local\r\n{invalid_content_length}\r\n\r\nabcGET /next HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n"
        );
        let response = exchange(addr, request.as_bytes());
        assert!(response.starts_with("HTTP/1.1 400 Bad Request"), "accepted invalid Content-Length: {response}");
        assert!(!response.contains("200 OK"), "invalid Content-Length dispatched or reused: {response}");
        assert_eq!(response.matches("HTTP/1.1").count(), 1, "invalid Content-Length reused socket: {response}");
    }

    let invalid_expect = exchange(
        addr,
        b"POST /echo HTTP/1.1\r\nHost: local\r\nContent-Length: +3\r\nExpect: 100-continue\r\n\r\n",
    );
    assert!(invalid_expect.starts_with("HTTP/1.1 400 Bad Request"), "{invalid_expect}");
    assert!(!invalid_expect.contains("100 Continue"), "invalid Content-Length received body permission: {invalid_expect}");

    assert_eq!(calls.load(Ordering::Acquire), 3, "invalid Content-Length reached handler");
    shutdown.store(true, Ordering::Release);
    let report = server.join().expect("server join");
    assert_eq!(report.user_accepted, 15);
    assert_eq!(report.user_completed, 15);
}

#[test]
fn shutdown_closes_idle_keepalive_and_starts_no_new_request() {
    use std::io::Write;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let mux = jet_http_mux_new();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let handler_calls = calls.clone();
    jet_http_mux_add(&mux, "GET", "/", move |_| {
        handler_calls.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        jet_http_srv_response(200, &"ok".to_string())
    });
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let mut options = JetHttpServerOptions::safe();
    options.workers = 2;
    options.admission_queue = 2;
    options.shutdown_grace = std::time::Duration::from_millis(500);
    let server = std::thread::spawn(move || {
        jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server")
    });

    let mut active = std::net::TcpStream::connect(addr).expect("active connect");
    let mut idle = std::net::TcpStream::connect(addr).expect("idle connect");
    active.write_all(b"GET / HTTP/1.1\r\nHost: local\r\n\r\n").expect("active first request");
    idle.write_all(b"GET / HTTP/1.1\r\nHost: local\r\n\r\n").expect("idle first request");
    assert!(read_response(&mut active).ends_with("\r\n\r\nok"));
    assert!(read_response(&mut idle).ends_with("\r\n\r\nok"));
    assert_eq!(calls.load(std::sync::atomic::Ordering::Acquire), 2);
    std::thread::sleep(std::time::Duration::from_millis(40));

    let started = std::time::Instant::now();
    shutdown.store(true, std::sync::atomic::Ordering::Release);
    let _ = active.write_all(b"GET / HTTP/1.1\r\nHost: local\r\n\r\n");
    let report = server.join().expect("server join");
    assert!(started.elapsed() < std::time::Duration::from_millis(250), "idle keepalive consumed grace");
    assert_eq!(calls.load(std::sync::atomic::Ordering::Acquire), 2, "shutdown started a keepalive request");
    assert_eq!(report.user_accepted, 2);
    assert_eq!(report.user_completed, 2);
}

#[test]
fn bounded_admission_returns_503_and_shutdown_drains_accepted_work() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let mux = jet_http_mux_new();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = std::sync::Arc::new(std::sync::Mutex::new(release_rx));
    jet_http_mux_add(&mux, "GET", "/slow", move |_| {
        entered_tx.send(()).expect("entered signal");
        release_rx.lock().unwrap().recv().expect("release");
        jet_http_srv_response(200, &"slow done".to_string())
    });
    jet_http_mux_add(&mux, "GET", "/queued", |_| jet_http_srv_response(200, &"queued done".to_string()));

    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let options = JetHttpServerOptions {
        workers: 1,
        admission_queue: 1,
        read_header_timeout: std::time::Duration::from_secs(1),
        read_idle_timeout: std::time::Duration::from_secs(1),
        read_body_timeout: std::time::Duration::from_secs(1),
        shutdown_grace: std::time::Duration::from_secs(1),
        ..JetHttpServerOptions::safe()
    };
    let server = std::thread::spawn(move || jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server"));
    let slow = std::thread::spawn(move || request(addr, b"GET /slow HTTP/1.1\r\nHost: local\r\n\r\n"));
    entered_rx.recv_timeout(std::time::Duration::from_secs(1)).expect("slow admitted");
    let queued = std::thread::spawn(move || request(addr, b"GET /queued HTTP/1.1\r\nHost: local\r\n\r\n"));
    std::thread::sleep(std::time::Duration::from_millis(30));
    let overloaded = request(addr, b"GET /queued HTTP/1.1\r\nHost: local\r\n\r\n");
    assert!(overloaded.starts_with("HTTP/1.1 503 Service Unavailable"), "{overloaded}");

    shutdown.store(true, std::sync::atomic::Ordering::Release);
    release_tx.send(()).expect("release slow");
    assert!(slow.join().unwrap().contains("slow done"));
    assert!(queued.join().unwrap().contains("queued done"));
    let report = server.join().expect("server join");
    assert_eq!(report.user_accepted, 2);
    assert_eq!(report.user_overloaded, 1);
    assert_eq!(report.user_completed, 2);
    assert_eq!(report.user_cancelled, 0);
}

fn timeout_for(partial: &'static [u8]) -> JetHttpReadError {
    use std::io::Write;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let client = std::thread::spawn(move || {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.write_all(partial).expect("partial write");
        std::thread::sleep(std::time::Duration::from_millis(150));
    });
    let (mut stream, _) = listener.accept().expect("accept");
    let options = JetHttpServerOptions {
        workers: 1,
        admission_queue: 1,
        read_header_timeout: std::time::Duration::from_millis(40),
        read_idle_timeout: std::time::Duration::from_millis(40),
        read_body_timeout: std::time::Duration::from_millis(40),
        shutdown_grace: std::time::Duration::from_millis(40),
        ..JetHttpServerOptions::safe()
    };
    let error = jet_http_srv_read_with_limits(&mut stream, &options).expect_err("timeout");
    client.join().expect("client");
    error
}

#[test]
fn header_and_body_reads_have_bounded_timeouts() {
    assert_eq!(timeout_for(b"GET / HTTP/1.1\r\nHost:").status, 408);
    assert_eq!(timeout_for(b"POST / HTTP/1.1\r\nHost: local\r\nContent-Length: 4\r\n\r\nx").status, 408);
}

#[test]
fn shutdown_grace_cancels_straggler_socket_and_returns_bounded_report() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let mux = jet_http_mux_new();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    jet_http_mux_add(&mux, "GET", "/slow", move |_| {
        entered_tx.send(()).expect("entered");
        std::thread::sleep(std::time::Duration::from_millis(250));
        jet_http_srv_response(200, &"too late".to_string())
    });
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let options = JetHttpServerOptions {
        workers: 1,
        admission_queue: 1,
        read_header_timeout: std::time::Duration::from_secs(1),
        read_idle_timeout: std::time::Duration::from_secs(1),
        read_body_timeout: std::time::Duration::from_secs(1),
        shutdown_grace: std::time::Duration::from_millis(30),
        ..JetHttpServerOptions::safe()
    };
    let server = std::thread::spawn(move || jet_http_server_run_listener(listener, mux, options, server_shutdown, None).expect("server"));
    let mut client = std::net::TcpStream::connect(addr).expect("connect");
    client.write_all(b"GET /slow HTTP/1.1\r\nHost: local\r\n\r\n").expect("write");
    entered_rx.recv_timeout(std::time::Duration::from_secs(1)).expect("handler entered");
    let started = std::time::Instant::now();
    shutdown.store(true, std::sync::atomic::Ordering::Release);
    let report = server.join().expect("server join");
    assert!(started.elapsed() < std::time::Duration::from_millis(150));
    assert_eq!(report.user_accepted, 1);
    assert_eq!(report.user_completed, 0);
    assert_eq!(report.user_cancelled, 1);
    let mut response = String::new();
    let _ = client.read_to_string(&mut response);
    assert!(!response.contains("200 OK"), "straggler published after cancellation: {response}");
}

#[test]
fn server_handle_binds_serves_and_rejects_second_shutdown() {
    use std::io::Write;
    let mux = jet_http_mux_new();
    jet_http_mux_add(&mux, "GET", "/", |_| jet_http_srv_response(200, &"handle".to_string()));
    let server = jet_http_server_bind(&"127.0.0.1:0".to_string(), mux).expect("bind");
    let addr: std::net::SocketAddr = jet_http_server_local_addr(&server).expect("addr").parse().expect("socket addr");
    let serving = server.clone();
    let serve_thread = std::thread::spawn(move || jet_http_server_serve(&serving).expect("serve"));
    let mut client = std::net::TcpStream::connect(addr).expect("connect");
    client.write_all(b"GET / HTTP/1.1\r\nHost: local\r\n\r\n").expect("write");
    let response = read_response(&mut client);
    assert!(response.contains("handle"));
    let report = jet_http_server_shutdown(&server, &jet_std::Duration { ms: 100 }).expect("shutdown");
    assert_eq!(report.user_completed, 1);
    assert!(jet_http_server_shutdown(&server, &jet_std::Duration { ms: 100 }).unwrap_err().contains("already requested"));
    assert_eq!(serve_thread.join().expect("serve join").user_completed, 1);
    assert!(jet_http_server_serve(&server)
        .unwrap_err()
        .contains("only be served once"));
}

#[test]
fn canonical_router_precedence_methods_and_conflicts() {
    let mux = jet_http_mux_new();
    jet_http_mux_add(&mux, "GET", "/files/*path", |req| {
        jet_http_srv_response(200, &format!("wild:{}", jet_http_srv_req_param(&req, &"path".to_string()).unwrap()))
    });
    jet_http_mux_add(&mux, "GET", "/files/:id", |req| {
        jet_http_srv_response(200, &format!("param:{}", jet_http_srv_req_param(&req, &"id".to_string()).unwrap()))
    });
    jet_http_mux_add(&mux, "GET", "/files/static", |_| jet_http_srv_response(200, &"static".to_string()));
    jet_http_mux_add(&mux, "POST", "/files/:id", |_| jet_http_srv_response(201, &"posted".to_string()));

    let request = |method: &str, path: &str| {
        JetHttpRequest::server(method, path.to_string(), Vec::new(), Default::default())
    };
    assert_eq!(jet_http_mux_dispatch(&mux, request("GET", "/files/static")).unwrap().body, "static");
    assert_eq!(jet_http_mux_dispatch(&mux, request("GET", "/files/42")).unwrap().body, "param:42");
    assert_eq!(jet_http_mux_dispatch(&mux, request("GET", "/files/a/b")).unwrap().body, "wild:a/b");
    assert_eq!(jet_http_mux_dispatch(&mux, request("GET", "/files")).unwrap().body, "wild:");
    let head = jet_http_mux_dispatch(&mux, request("HEAD", "/files/static")).unwrap();
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
    assert_eq!(jet_http_mux_dispatch(&mux, request("DELETE", "/files/42")).unwrap().status, 405);
    let options = jet_http_mux_dispatch(&mux, request("OPTIONS", "/files/42")).unwrap();
    assert_eq!(options.status, 204);
    assert_eq!(options.headers.get("Allow").unwrap(), "GET, HEAD, OPTIONS, POST");

    let conflict = jet_http_mux_new();
    jet_http_mux_add(&conflict, "GET", "/users/:id", |_| jet_http_srv_response(200, &String::new()));
    jet_http_mux_add(&conflict, "GET", "/users/:name", |_| jet_http_srv_response(200, &String::new()));
    assert!(jet_http_server_bind(&"127.0.0.1:0".to_string(), conflict).err().expect("conflict").contains("route conflict"));
    let legacy = jet_http_mux_new();
    jet_http_mux_add(&legacy, "GET", "/users/{id}", |_| jet_http_srv_response(200, &String::new()));
    assert!(jet_http_server_bind(&"127.0.0.1:0".to_string(), legacy).err().expect("brace pattern").contains("E2805"));

    let bare = jet_http_mux_new();
    jet_http_mux_add(&bare, "GET", "/files/*", |_| jet_http_srv_response(200, &String::new()));
    assert!(jet_http_server_bind(&"127.0.0.1:0".to_string(), bare).err().expect("bare wildcard").contains("`*wildcard`"));

    let bare_once = jet_http_mux_new();
    jet_http_mux_add(&bare_once, "GET", "/files/*", |_| jet_http_srv_response(200, &String::new()));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind invalid route probe");
    assert!(jet_http_mux_serve_once_listener(&JetTcpListener { inner: listener }, &bare_once)
        .err().expect("serve-once validation").contains("`*wildcard`"));

    let invalid_before_bind = jet_http_mux_new();
    jet_http_mux_add(&invalid_before_bind, "GET", "/files/*", |_| jet_http_srv_response(200, &String::new()));
    assert!(jet_http_mux_serve_once(&"not a socket address".to_string(), invalid_before_bind)
        .expect_err("route validation must precede bind")
        .contains("E2805"));

    let encoded = jet_http_mux_new();
    jet_http_mux_add(&encoded, "GET", "/literal/%3Aadmin/%2Astar", |_| jet_http_srv_response(200, &"encoded".to_string()));
    jet_http_mux_add(&encoded, "GET", "/once/%252F", |_| jet_http_srv_response(200, &"once".to_string()));
    assert_eq!(jet_http_mux_dispatch(&encoded, request("GET", "/literal/%3Aadmin/%2Astar")).unwrap().body, "encoded");
    assert_eq!(jet_http_mux_dispatch(&encoded, request("GET", "/once/%252F")).unwrap().body, "once");
    assert_eq!(jet_http_mux_dispatch(&encoded, request("GET", "/literal/%2F/admin")).unwrap().status, 400);
    assert_eq!(jet_http_mux_dispatch(&encoded, request("GET", "/literal/%FF")).unwrap().status, 400);

    for invalid in ["/x/*rest/more", "/x/:1bad", "/x/:id/:id", "/x/%2F", "/x/%2e%2e", "/x/%ZZ", "/x/%FF"] {
        let invalid_mux = jet_http_mux_new();
        jet_http_mux_add(&invalid_mux, "GET", invalid, |_| jet_http_srv_response(200, &String::new()));
        assert!(jet_http_server_bind(&"127.0.0.1:0".to_string(), invalid_mux).is_err(), "accepted {invalid}");
    }

    let precedence = jet_http_mux_new();
    jet_http_mux_add(&precedence, "GET", "/a/*rest", |_| jet_http_srv_response(200, &"catch".to_string()));
    jet_http_mux_add(&precedence, "GET", "/a/:id/*rest", |_| jet_http_srv_response(200, &"param".to_string()));
    jet_http_mux_add(&precedence, "GET", "/tie/:first/static", |_| jet_http_srv_response(200, &"param-first".to_string()));
    jet_http_mux_add(&precedence, "GET", "/tie/static/:last", |_| jet_http_srv_response(200, &"static-first".to_string()));
    assert_eq!(jet_http_mux_dispatch(&precedence, request("GET", "/a/x/y")).unwrap().body, "param");
    assert_eq!(jet_http_mux_dispatch(&precedence, request("GET", "/tie/static/static")).unwrap().body, "static-first");

    let first = jet_http_route_parse("/same/:first").unwrap();
    let second = jet_http_route_parse("/same/:second").unwrap();
    assert!(jet_http_route_selection_cmp(&first, 0, &second, 1).is_gt());

    let logs = jet_http_mux_new();
    jet_http_mux_add(&logs, "GET", "/logs/:id/*rest", |req| {
        jet_http_srv_response(200, &jet_http_srv_access_log(&req, 200))
    });
    assert_eq!(
        jet_http_mux_dispatch(&logs, request("GET", "/logs/42/a/b?secret=x")).unwrap().body,
        "GET /logs/:id/*rest 200"
    );
}

#[test]
fn middleware_orders_short_circuits_contains_panics_and_isolates_requests() {
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mux = jet_http_mux_new();
    for name in ["outer", "inner"] {
        let events = events.clone();
        jet_http_mux_middleware(&mux, move |next| {
            let events = events.clone();
            let name = name.to_string();
            std::sync::Arc::new(move |req| {
                events.lock().unwrap().push(format!("{name}:before:{}", req.path));
                let response = next(req.clone());
                events.lock().unwrap().push(format!("{name}:after:{}", req.path));
                response
            })
        });
    }
    jet_http_mux_add(&mux, "GET", "/ok/:id", |req| {
        jet_http_srv_response(200, &jet_http_srv_req_param(&req, &"id".to_string()).unwrap())
    });
    jet_http_mux_add(&mux, "GET", "/panic", |_| panic!("private failure detail"));
    let request = |path: &str| {
        JetHttpRequest::server("GET", path.to_string(), Vec::new(), Default::default())
    };
    assert_eq!(jet_http_mux_dispatch(&mux, request("/ok/one")).unwrap().body, "one");
    assert_eq!(&*events.lock().unwrap(), &[
        "outer:before:/ok/one", "inner:before:/ok/one",
        "inner:after:/ok/one", "outer:after:/ok/one",
    ]);
    assert!(matches!(
        jet_http_mux_dispatch(&mux, request("/panic")),
        Err(JetHttpError::Internal { .. })
    ));

    let short = jet_http_mux_new();
    let handler_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    jet_http_mux_middleware(&short, |_| std::sync::Arc::new(|_| Ok(jet_http_srv_response(403, &"blocked".to_string()))));
    let calls = handler_calls.clone();
    jet_http_mux_add(&short, "GET", "/", move |_| {
        calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        jet_http_srv_response(200, &"wrong".to_string())
    });
    assert_eq!(jet_http_mux_dispatch(&short, request("/")).unwrap().status, 403);
    assert_eq!(handler_calls.load(std::sync::atomic::Ordering::Relaxed), 0);

    let mut threads = Vec::new();
    for id in 0..16 {
        let mux = mux.clone();
        threads.push(std::thread::spawn(move || {
            let req = JetHttpRequest::server(
                "GET",
                format!("/ok/{id}"),
                Vec::new(),
                Default::default(),
            );
            assert_eq!(jet_http_mux_dispatch(&mux, req).unwrap().body, id.to_string());
        }));
    }
    for thread in threads { thread.join().unwrap(); }
}

#[test]
fn dispatch_drops_route_lock_before_concurrent_and_reentrant_handlers() {
    let request = |path: &str| {
        JetHttpRequest::server("GET", path.to_string(), Vec::new(), Default::default())
    };

    // Two requests must enter the same handler concurrently. Holding the route
    // registry guard through user code serializes them and strands this barrier.
    let overlap = jet_http_mux_new();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let entered = barrier.clone();
    jet_http_mux_add(&overlap, "GET", "/overlap", move |_| {
        entered.wait();
        jet_http_srv_response(200, &"overlap".to_string())
    });
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    for _ in 0..2 {
        let mux = overlap.clone();
        let done = done_tx.clone();
        std::thread::spawn(move || {
            done.send(jet_http_mux_dispatch(&mux, request("/overlap")).unwrap().status).unwrap();
        });
    }
    for _ in 0..2 {
        assert_eq!(done_rx.recv_timeout(std::time::Duration::from_secs(2)).expect("handlers did not overlap"), 200);
    }

    // User code may register a route on its own mux without deadlocking on the
    // registry lock retained by dispatch.
    let reentrant = jet_http_mux_new();
    let from_handler = reentrant.clone();
    jet_http_mux_add(&reentrant, "GET", "/register", move |_| {
        jet_http_mux_add(&from_handler, "GET", "/added", |_| jet_http_srv_response(201, &"added".to_string()));
        jet_http_srv_response(200, &"registered".to_string())
    });
    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    let dispatch_mux = reentrant.clone();
    std::thread::spawn(move || {
        reply_tx.send(jet_http_mux_dispatch(&dispatch_mux, request("/register"))).unwrap();
    });
    assert_eq!(reply_rx.recv_timeout(std::time::Duration::from_secs(2)).expect("route registration deadlocked").unwrap().status, 200);
    assert_eq!(jet_http_mux_dispatch(&reentrant, request("/added")).unwrap().status, 201);
}

#[test]
fn server_safe_defaults_static_files_ranges_and_access_events_are_bounded() {
    let options = JetHttpServerOptions::safe();
    assert_eq!(options.max_body_bytes, 1024 * 1024);
    assert_eq!(options.max_connections, 10_000);
    assert_eq!(options.max_connections_per_ip, 256);
    assert_eq!(options.write_idle_timeout, std::time::Duration::from_secs(30));

    let root = std::env::temp_dir().join(format!(
        "jet-http-static-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
    ));
    std::fs::create_dir_all(root.join("nested")).unwrap();
    std::fs::write(root.join("nested/data.bin"), [0, 1, 2, 0xff, 4, 5]).unwrap();
    std::fs::write(root.join("index.html"), b"index").unwrap();

    let request = |path: &str, range: Option<&str>| {
        let mut headers = JetHttpHeaders::new();
        if let Some(range) = range { headers.append("range", range).unwrap(); }
        JetHttpRequest::server("GET", path.to_string(), Vec::new(), headers)
    };
    let full = jet_http_srv_static_files(&request("/nested/data.bin", None), &root).unwrap();
    assert_eq!(full.status, 200);
    let etag = full.headers.get("etag").unwrap().clone();
    let last_modified = full.headers.get("last-modified").unwrap().clone();
    assert_eq!(full.body.bytes(64).unwrap(), vec![0, 1, 2, 0xff, 4, 5]);
    assert_eq!(full.headers.get("accept-ranges"), Some(&"bytes".to_string()));
    assert!(full.headers.get("etag").is_some());

    let partial = jet_http_srv_static_files(&request("/nested/data.bin", Some("bytes=2-4")), &root).unwrap();
    assert_eq!(partial.status, 206);
    assert_eq!(partial.body.bytes(64).unwrap(), vec![2, 0xff, 4]);
    assert_eq!(partial.headers.get("content-range"), Some(&"bytes 2-4/6".to_string()));
    assert_eq!(jet_http_srv_static_files(&request("/nested/data.bin", Some("bytes=0-1,4-5")), &root).unwrap().status, 416);
    assert_eq!(jet_http_srv_static_files(&request("/../secret", None), &root).unwrap().status, 404);
    assert_eq!(jet_http_srv_static_files(&request("/nested", None), &root).unwrap().status, 404);
    assert_eq!(jet_http_srv_static_files(&request("/", None), &root).unwrap().body.bytes(64).unwrap(), b"index");
    for (name, value) in [("if-none-match", etag.as_str()), ("if-modified-since", last_modified.as_str())] {
        let mut headers = JetHttpHeaders::new();
        headers.append(name, value).unwrap();
        let conditional = JetHttpRequest::server("GET", "/nested/data.bin".to_string(), Vec::new(), headers);
        assert_eq!(jet_http_srv_static_files(&conditional, &root).unwrap().status, 304);
    }
    let mut headers = JetHttpHeaders::new();
    headers.append("range", "bytes=1-2").unwrap();
    headers.append("if-range", "\"stale\"").unwrap();
    let stale_range = JetHttpRequest::server("GET", "/nested/data.bin".to_string(), Vec::new(), headers);
    let stale_range = jet_http_srv_static_files(&stale_range, &root).unwrap();
    assert_eq!(stale_range.status, 200);
    assert_eq!(stale_range.body.bytes(64).unwrap().len(), 6);

    let mut headers = JetHttpHeaders::new();
    headers.append("authorization", "Bearer private").unwrap();
    headers.append("cookie", "session=private").unwrap();
    headers.append("x-request-id", "req-7").unwrap();
    let mut req = JetHttpRequest::server("GET", "/users/7?token=private".to_string(), Vec::new(), headers);
    req.route_template = Some("/users/:id".to_string());
    let event = jet_http_srv_access_event(&req, 201, 12, 3, "127.0.0.1:9", "HTTP/2", true);
    assert_eq!(event.request_id, "req-7");
    assert_eq!(event.path, "/users/7");
    assert_eq!(event.route_template, "/users/:id");
    assert_eq!(event.status, 201);
    assert_eq!(event.bytes, 12);
    assert_eq!(event.duration_ms, 3);
    assert_eq!(event.protocol, "HTTP/2");
    assert!(event.tls);
    let shown = event.to_string();
    assert!(!shown.contains("private") && !shown.contains("authorization") && !shown.contains("cookie"));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn static_file_response_holds_the_open_identity_and_streams_the_selected_range() {
    let root = std::env::temp_dir().join(format!(
        "jet-http-held-static-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("asset.bin");
    std::fs::write(&path, b"trusted-body").unwrap();
    let mut headers = JetHttpHeaders::new();
    headers.append("range", "bytes=2-6").unwrap();
    let request = JetHttpRequest::server("GET", "/asset.bin".to_string(), Vec::new(), headers);
    let response = jet_http_srv_static_files(&request, &root).unwrap();
    let direct = jet_http_srv_static_file_range(
        &request,
        &path.to_string_lossy().into_owned(),
        &"application/octet-stream".to_string(),
    ).unwrap();

    std::fs::rename(&path, root.join("old.bin")).unwrap();
    std::fs::write(&path, b"attacker-body").unwrap();
    assert_eq!(response.status, 206);
    assert_eq!(response.body.length(), Some(5));
    assert_eq!(response.body.bytes(5).unwrap(), b"usted");
    assert_eq!(direct.body.length(), Some(5));
    assert_eq!(direct.body.bytes(5).unwrap(), b"usted");
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_static_serving_fails_closed_without_held_no_reparse_identity() {
    let root = std::env::temp_dir().join(format!("jet-http-windows-static-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("asset.txt"), b"must not serve by pathname").unwrap();
    let request = JetHttpRequest::server(
        "GET",
        "/asset.txt".to_string(),
        Vec::new(),
        JetHttpHeaders::new(),
    );
    assert_eq!(jet_http_srv_static_files(&request, &root).unwrap().status, 404);
    assert!(jet_http_srv_static_file(
        &root.join("asset.txt").to_string_lossy().into_owned(),
        &"text/plain".to_string(),
    ).is_err());
    assert!(jet_http_srv_static_file_range(
        &request,
        &root.join("asset.txt").to_string_lossy().into_owned(),
        &"text/plain".to_string(),
    ).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn http2_response_framing_filters_handler_claims_and_verifies_known_lengths() {
    use std::io::Read;

    let mut forbidden = jet_http_srv_empty_response(204);
    forbidden.headers.append("content-length", "99").unwrap();
    forbidden.headers.append("transfer-encoding", "chunked").unwrap();
    forbidden.headers.append("connection", "x-hop").unwrap();
    forbidden.headers.append("x-hop", "private").unwrap();
    let encoded = jet_http2_encode_response_headers(&forbidden, None);
    let decoded = jet_http2_decode_headers(&mut JetHttp2Hpack::new(), &encoded).unwrap();
    assert_eq!(decoded, vec![(":status".to_string(), "204".to_string())]);

    let mux = jet_http_mux_new();
    jet_http_mux_add(&mux, "GET", "/", |_| jet_http_srv_response(200, &"representation".to_string()));
    let head = jet_http_mux_dispatch(
        &mux,
        JetHttpRequest::server("HEAD", "/".to_string(), Vec::new(), JetHttpHeaders::new()),
    ).unwrap();
    let length = head.head_content_length.or_else(|| head.body.length());
    let decoded = jet_http2_decode_headers(
        &mut JetHttp2Hpack::new(),
        &jet_http2_encode_response_headers(&head, length),
    ).unwrap();
    assert!(decoded.iter().any(|(name, value)| name == "content-length" && value == "14"));

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let mut peer = std::net::TcpStream::connect(addr).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(1))).unwrap();
    let mut response = jet_http_srv_empty_response(200);
    response.body = JetHttpBody::reader_cancellable(
        std::io::Cursor::new(b"ab".to_vec()),
        Some(1),
        || {},
    );
    let mut outgoing = jet_http2_start_response(&mut server, 1, response, JET_HTTP2_MAX_FRAME).unwrap().unwrap();
    let mut connection_window = 65_535;
    let mut stream_window = 65_535;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        if jet_http2_flush_body(
            &mut server, 1, &mut outgoing, &mut connection_window, &mut stream_window, JET_HTTP2_MAX_FRAME,
        ).unwrap() { break; }
        assert!(std::time::Instant::now() < deadline, "response producer did not complete");
        std::thread::yield_now();
    }
    let mut wire = Vec::new();
    peer.read_to_end(&mut wire).ok();
    assert!(wire.windows(9).any(|header| header[3] == 3 && u32::from_be_bytes(header[5..9].try_into().unwrap()) == 1));
}

#[test]
fn http2_rejects_uncancellable_reader_before_response_headers() {
    struct Uncancellable(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl std::io::Read for Uncancellable {
        fn read(&mut self, _output: &mut [u8]) -> std::io::Result<usize> {
            std::thread::park();
            Ok(0)
        }
    }
    impl Drop for Uncancellable {
        fn drop(&mut self) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let mut peer = std::net::TcpStream::connect(addr).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    peer.set_nonblocking(true).unwrap();
    let dropped = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut response = jet_http_srv_empty_response(200);
    response.body = JetHttpBody::reader(Uncancellable(dropped.clone()), None);
    assert!(jet_http2_start_response(&mut server, 1, response, JET_HTTP2_MAX_FRAME)
        .err().expect("uncancellable reader rejection")
        .contains("bounded or cancellable"));
    assert_eq!(dropped.load(std::sync::atomic::Ordering::Acquire), 1);
    let mut byte = [0u8; 1];
    assert_eq!(std::io::Read::read(&mut peer, &mut byte).unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
}

#[test]
fn http2_rejects_transport_bridge_before_response_headers() {
    HTTP_BODY_READS.store(0, std::sync::atomic::Ordering::SeqCst);
    HTTP_H2_BRIDGE_CLOSES.store(0, std::sync::atomic::Ordering::SeqCst);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let mut peer = std::net::TcpStream::connect(addr).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    peer.set_nonblocking(true).unwrap();
    let mut response = jet_http_srv_empty_response(200);
    response.body = JetHttpBody::bridge(
        1,
        None,
        unread_bridge_body,
        close_h2_bridge_body,
    );
    assert!(jet_http2_start_response(&mut server, 1, response, JET_HTTP2_MAX_FRAME)
        .err().expect("transport bridge rejection")
        .contains("bounded or cancellable"));
    assert_eq!(HTTP_BODY_READS.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(HTTP_H2_BRIDGE_CLOSES.load(std::sync::atomic::Ordering::SeqCst), 1);
    let mut byte = [0u8; 1];
    assert_eq!(std::io::Read::read(&mut peer, &mut byte).unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
}

#[test]
fn server_enforces_and_releases_per_ip_connection_capacity() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let mux = jet_http_mux_new();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = std::sync::Arc::new(std::sync::Mutex::new(release_rx));
    jet_http_mux_add(&mux, "GET", "/hold", move |_| {
        entered_tx.send(()).unwrap();
        release_rx.lock().unwrap().recv().unwrap();
        jet_http_srv_response(200, &"released".to_string())
    });
    jet_http_mux_add(&mux, "GET", "/ok", |_| jet_http_srv_response(200, &"ok".to_string()));
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop = shutdown.clone();
    let options = JetHttpServerOptions {
        workers: 2,
        admission_queue: 2,
        max_connections: 2,
        max_connections_per_ip: 1,
        ..JetHttpServerOptions::safe()
    };
    let server = std::thread::spawn(move || jet_http_server_run_listener(listener, mux, options, stop, None).unwrap());
    let held = std::thread::spawn(move || request(addr, b"GET /hold HTTP/1.1\r\nHost: local\r\n\r\n"));
    entered_rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
    let rejected = request(addr, b"GET /ok HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n");
    assert!(rejected.starts_with("HTTP/1.1 503 Service Unavailable"), "{rejected}");
    release_tx.send(()).unwrap();
    assert!(held.join().unwrap().contains("released"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    let accepted = loop {
        let response = request(addr, b"GET /ok HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n");
        if response.contains("200 OK") || std::time::Instant::now() >= deadline { break response; }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert!(accepted.contains("200 OK"), "capacity was not released: {accepted}");
    shutdown.store(true, std::sync::atomic::Ordering::Release);
    let report = server.join().unwrap();
    assert_eq!(report.user_accepted, 2);
    assert_eq!(report.user_overloaded, 1);
}

#[test]
fn http2_handlers_overlap_reset_drops_queued_data_and_completed_streams_release_capacity() {
    use std::io::Write;

    struct BlockingBody {
        started: Option<std::sync::mpsc::Sender<()>>,
        wake: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
        dropped: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl std::io::Read for BlockingBody {
        fn read(&mut self, _output: &mut [u8]) -> std::io::Result<usize> {
            if let Some(started) = self.started.take() { let _ = started.send(()); }
            let (cancelled, wake) = &*self.wake;
            let mut cancelled = cancelled.lock().unwrap();
            while !*cancelled { cancelled = wake.wait(cancelled).unwrap(); }
            Err(std::io::ErrorKind::Interrupted.into())
        }
    }

    impl Drop for BlockingBody {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
    }

    std::env::set_var("JET_SCHEDULER_THREADS", "1");
    let mux = jet_http_mux_new();
    jet_http_mux_add(&mux, "POST", "/slow-body", |request| {
        let body = request.body.bytes(64).unwrap();
        jet_http_srv_response(200, &body.len().to_string())
    });
    jet_http_mux_add(&mux, "GET", "/fast", |_| jet_http_srv_response(200, &"fast".to_string()));
    jet_http_mux_add(&mux, "GET", "/large", |_| {
        let mut response = jet_http_srv_empty_response(200);
        response.body = JetHttpBody::from_bytes(vec![7; 100_000]);
        response
    });
    jet_http_mux_add(&mux, "POST", "/ignore", |_| jet_http_srv_response(200, &"ignored".to_string()));
    let (response_started_tx, response_started_rx) = std::sync::mpsc::channel();
    let response_wake = std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let response_dropped = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let response_closed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let handler_wake = response_wake.clone();
    let handler_dropped = response_dropped.clone();
    let handler_closed = response_closed.clone();
    jet_http_mux_add(&mux, "GET", "/slow-response", move |_| {
        let mut response = jet_http_srv_empty_response(200);
        let source_wake = handler_wake.clone();
        let close_wake = handler_wake.clone();
        let close_count = handler_closed.clone();
        response.body = JetHttpBody::reader_cancellable(BlockingBody {
            started: Some(response_started_tx.clone()),
            wake: source_wake,
            dropped: handler_dropped.clone(),
        }, Some(4), move || {
            close_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            let (cancelled, wake) = &*close_wake;
            *cancelled.lock().unwrap() = true;
            wake.notify_all();
        });
        response
    });

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let mut client = std::net::TcpStream::connect(addr).unwrap();
    let (mut server_stream, _) = listener.accept().unwrap();
    let options = JetHttpServerOptions {
        workers: 1,
        admission_queue: 1,
        read_idle_timeout: std::time::Duration::from_secs(2),
        ..JetHttpServerOptions::safe()
    };
    let server = std::thread::spawn(move || {
        jet_http2_serve(
            &mut server_stream,
            &mux,
            &options,
            &std::sync::atomic::AtomicBool::new(false),
            None,
        )
    });
    client.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
    client.write_all(JET_HTTP2_PREFACE).unwrap();
    client.write_all(&h2_frame(4, 0, 0, &[])).unwrap();
    client.write_all(&h2_frame(1, 0x4, 1, &h2_request_headers(0x83, "/slow-body"))).unwrap();
    client.write_all(&h2_frame(0, 0, 1, b"part")).unwrap();
    client.flush().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while jet_scheduler_blocking_wait_stats().0 == 0 {
        assert!(std::time::Instant::now() < deadline, "request Body did not enter a scheduler-aware blocking wait");
        std::thread::yield_now();
    }
    assert_eq!(JET_OBSERVE_WORKERS.load(std::sync::atomic::Ordering::Relaxed), 1);
    client.write_all(&h2_frame(1, 0x5, 3, &h2_request_headers(0x82, "/fast"))).unwrap();
    client.write_all(&h2_frame(6, 0, 0, b"12345678")).unwrap();
    client.flush().unwrap();

    let mut fast = false;
    let mut ping = false;
    while !fast || !ping {
        let (kind, flags, stream, payload) = h2_read_frame(&mut client);
        fast |= kind == 0 && stream == 3 && payload == b"fast";
        ping |= kind == 6 && flags & 1 != 0 && payload == b"12345678";
    }
    assert!(jet_scheduler_blocking_wait_stats().2 >= 1, "one blocked worker did not receive bounded compensation");
    client.write_all(&h2_frame(0, 0x1, 1, b"done")).unwrap();
    client.flush().unwrap();
    while {
        let (kind, _, stream, payload) = h2_read_frame(&mut client);
        !(kind == 0 && stream == 1 && payload == b"8")
    } {}

    let blocked_response_id = 5;
    client.write_all(&h2_frame(1, 0x5, blocked_response_id, &h2_request_headers(0x82, "/slow-response"))).unwrap();
    client.flush().unwrap();
    response_started_rx.recv_timeout(std::time::Duration::from_secs(1))
        .expect("response Body producer did not start");
    client.write_all(&h2_frame(1, 0x5, 7, &h2_request_headers(0x82, "/fast"))).unwrap();
    client.write_all(&h2_frame(6, 0, 0, b"bodyping")).unwrap();
    client.flush().unwrap();
    let mut fast = false;
    let mut ping = false;
    while !fast || !ping {
        let (kind, flags, stream, payload) = h2_read_frame(&mut client);
        fast |= kind == 0 && stream == 7 && payload == b"fast";
        ping |= kind == 6 && flags & 1 != 0 && payload == b"bodyping";
        assert!(!(kind == 0 && stream == blocked_response_id), "blocked response produced DATA before release");
    }
    client.write_all(&h2_frame(3, 0, blocked_response_id, &8u32.to_be_bytes())).unwrap();
    client.flush().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        let (waits, threads, _) = jet_scheduler_blocking_wait_stats();
        if response_closed.load(std::sync::atomic::Ordering::Acquire) == 1
            && response_dropped.load(std::sync::atomic::Ordering::Acquire) == 1
            && waits == 0
            && threads == 0
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline,
            "RST did not close and drop blocked response source: close={} drop={} scheduler={:?}",
            response_closed.load(std::sync::atomic::Ordering::Acquire),
            response_dropped.load(std::sync::atomic::Ordering::Acquire),
            jet_scheduler_blocking_wait_stats());
        std::thread::yield_now();
    }

    // With a configured two-stream cap, more than two sequential successes prove
    // every per-stream map releases completed entries rather than accumulating.
    for stream_id in (9..29).step_by(2) {
        client.write_all(&h2_frame(1, 0x5, stream_id, &h2_request_headers(0x82, "/fast"))).unwrap();
        client.flush().unwrap();
        loop {
            let (kind, _, stream, payload) = h2_read_frame(&mut client);
            assert!(!(kind == 0 && stream == blocked_response_id), "cancelled response producer emitted late DATA");
            if kind == 0 && stream == stream_id && payload == b"fast" { break; }
        }
    }

    let large_id = 29;
    client.write_all(&h2_frame(4, 0, 0, &[0, 4, 0, 0, 0, 1])).unwrap();
    client.write_all(&h2_frame(1, 0x5, large_id, &h2_request_headers(0x82, "/large"))).unwrap();
    client.flush().unwrap();
    loop {
        let (kind, _, stream, payload) = h2_read_frame(&mut client);
        if kind == 0 && stream == large_id && !payload.is_empty() { break; }
    }
    client.write_all(&h2_frame(3, 0, large_id, &8u32.to_be_bytes())).unwrap();
    client.write_all(&h2_frame(8, 0, 0, &100_000u32.to_be_bytes())).unwrap();
    client.write_all(&h2_frame(8, 0, large_id, &100_000u32.to_be_bytes())).unwrap();
    client.write_all(&h2_frame(6, 0, 0, b"reset-ok")).unwrap();
    client.flush().unwrap();
    loop {
        let (kind, flags, stream, payload) = h2_read_frame(&mut client);
        assert!(!(kind == 0 && stream == large_id), "RST stream emitted late DATA");
        if kind == 6 && flags & 1 != 0 && payload == b"reset-ok" { break; }
    }

    let ignored_id = 31;
    client.write_all(&h2_frame(4, 0, 0, &[0, 4, 0, 0, 0xff, 0xff])).unwrap();
    client.write_all(&h2_frame(1, 0x4, ignored_id, &h2_request_headers(0x83, "/ignore"))).unwrap();
    client.write_all(&h2_frame(0, 0, ignored_id, b"body")).unwrap();
    client.flush().unwrap();
    loop {
        let (kind, _, stream, payload) = h2_read_frame(&mut client);
        assert!(!(kind == 8 && stream == ignored_id), "receive credit was returned before application consumption");
        if kind == 0 && stream == ignored_id && payload == b"ignored" { break; }
    }
    client.write_all(&h2_frame(3, 0, ignored_id, &8u32.to_be_bytes())).unwrap();
    client.write_all(&h2_frame(7, 0, 0, &[0; 8])).unwrap();
    client.flush().unwrap();
    assert!(server.join().unwrap().is_ok());
}

#[test]
fn http2_uses_configured_header_body_and_size_limits() {
    use std::io::Write;

    fn serve_once(
        options: JetHttpServerOptions,
        write_client: impl FnOnce(&mut std::net::TcpStream),
    ) -> Result<(), String> {
        let mux = jet_http_mux_new();
        jet_http_mux_add(&mux, "POST", "/echo", |request| {
            let size = request.body.bytes(64).map(|body| body.len()).unwrap_or(0);
            jet_http_srv_response(200, &size.to_string())
        });
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let worker = std::thread::spawn(move || {
            jet_http2_serve(&mut server, &mux, &options, &std::sync::atomic::AtomicBool::new(false), None)
        });
        client.write_all(JET_HTTP2_PREFACE).unwrap();
        client.write_all(&h2_frame(4, 0, 0, &[])).unwrap();
        write_client(&mut client);
        client.flush().unwrap();
        worker.join().unwrap()
    }

    let short = JetHttpServerOptions {
        read_header_timeout: std::time::Duration::from_millis(25),
        read_idle_timeout: std::time::Duration::from_secs(1),
        ..JetHttpServerOptions::safe()
    };
    let header_error = serve_once(short, |client| {
        client.write_all(&h2_frame(1, 0, 1, &h2_request_headers(0x83, "/echo"))).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(60));
    }).unwrap_err();
    assert!(header_error.contains("timed out") || header_error.contains("ended early"), "{header_error}");

    let bounded = JetHttpServerOptions { max_body_bytes: 4, ..JetHttpServerOptions::safe() };
    let body_error = serve_once(bounded, |client| {
        client.write_all(&h2_frame(1, 0x4, 1, &h2_request_headers(0x83, "/echo"))).unwrap();
        client.write_all(&h2_frame(0, 0x1, 1, b"12345")).unwrap();
        let _ = client.shutdown(std::net::Shutdown::Write);
    }).unwrap_err();
    assert_eq!(body_error, "HTTP/2 request body is too large");

    let mux = jet_http_mux_new();
    jet_http_mux_add(&mux, "POST", "/echo", |request| {
        let size = request.body.bytes(64).map(|body| body.len()).unwrap_or(0);
        jet_http_srv_response(200, &size.to_string())
    });
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let mut client = std::net::TcpStream::connect(addr).unwrap();
    let (mut server_stream, _) = listener.accept().unwrap();
    let options = JetHttpServerOptions {
        read_body_timeout: std::time::Duration::from_millis(25),
        read_idle_timeout: std::time::Duration::from_secs(1),
        ..JetHttpServerOptions::safe()
    };
    let worker = std::thread::spawn(move || {
        jet_http2_serve(&mut server_stream, &mux, &options, &std::sync::atomic::AtomicBool::new(false), None)
    });
    client.set_read_timeout(Some(std::time::Duration::from_secs(1))).unwrap();
    client.write_all(JET_HTTP2_PREFACE).unwrap();
    client.write_all(&h2_frame(4, 0, 0, &[])).unwrap();
    client.write_all(&h2_frame(1, 0x4, 1, &h2_request_headers(0x83, "/echo"))).unwrap();
    client.flush().unwrap();
    let mut pinger = client.try_clone().unwrap();
    let pings = std::thread::spawn(move || {
        for sequence in 0u64..20 {
            if pinger.write_all(&h2_frame(6, 0, 0, &sequence.to_be_bytes())).is_err() { break; }
            let _ = pinger.flush();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
    let deadline_started = std::time::Instant::now();
    loop {
        let (kind, _, stream, payload) = h2_read_frame(&mut client);
        if kind == 3 && stream == 1 {
            assert_eq!(payload, 8u32.to_be_bytes());
            break;
        }
    }
    assert!(deadline_started.elapsed() < std::time::Duration::from_millis(250),
        "frequent PING traffic extended the incomplete-body deadline");
    pings.join().unwrap();
    client.write_all(&h2_frame(7, 0, 0, &[0; 8])).unwrap();
    client.flush().unwrap();
    assert!(worker.join().unwrap().is_ok());
}

#[test]
fn native_http2_routes_huffman_headers_and_enforces_stream_framing() {
    use std::io::{Read, Write};

    fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
        let len = payload.len();
        let mut out = vec![(len >> 16) as u8, (len >> 8) as u8, len as u8, kind, flags];
        out.extend_from_slice(&(stream & 0x7fff_ffff).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    let mux = jet_http_mux_new();
    jet_http_mux_add(&mux, "GET", "/", |_| jet_http_srv_response(200, &"h2-ok".to_string()));
    jet_http_mux_add(&mux, "GET", "/large", |_| {
        let mut response = jet_http_srv_response(200, &String::new());
        response.body = JetHttpBody::from_bytes(vec![7; 70_000]);
        response
    });
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop = shutdown.clone();
    let server = std::thread::spawn(move || {
        jet_http_server_run_listener(listener, mux, JetHttpServerOptions::safe(), stop, None).unwrap()
    });

    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
    stream.write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n").unwrap();
    stream.write_all(&frame(4, 0, 0, &[])).unwrap();
    // RFC 7541 C.4.1: GET http://www.example.com/ with a Huffman authority.
    stream.write_all(&frame(1, 0x5, 1, &[0x82, 0x86, 0x84, 0x41, 0x8c, 0xf1, 0xe3, 0xc2, 0xe5, 0xf2, 0x3a, 0x6b, 0xa0, 0xab, 0x90, 0xf4, 0xff])).unwrap();
    stream.flush().unwrap();
    let mut wire = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !wire.windows(5).any(|part| part == b"h2-ok") && std::time::Instant::now() < deadline {
        let mut chunk = [0; 4096];
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => wire.extend_from_slice(&chunk[..read]),
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => break,
            Err(error) => panic!("HTTP/2 response read failed: {error}"),
        }
    }
    assert!(wire.windows(5).any(|part| part == b"h2-ok"), "HTTP/2 DATA missing: {wire:02x?}");
    assert!(wire.windows(9).any(|header| header[3] == 4), "server SETTINGS missing");
    assert!(wire.windows(9).any(|header| header[3] == 1 && header[8] == 1), "stream-1 HEADERS missing");

    let large_headers = [
        0x82, 0x86, 0x04, 0x06, b'/', b'l', b'a', b'r', b'g', b'e',
        0x41, 0x05, b'l', b'o', b'c', b'a', b'l',
    ];
    stream.write_all(&frame(1, 0x5, 3, &large_headers)).unwrap();
    stream.flush().unwrap();
    let mut received = 0usize;
    while received < 65_000 {
        let mut header = [0u8; 9];
        stream.read_exact(&mut header).unwrap();
        let length = (usize::from(header[0]) << 16) | (usize::from(header[1]) << 8) | usize::from(header[2]);
        let mut payload = vec![0; length];
        stream.read_exact(&mut payload).unwrap();
        if header[3] == 0 && u32::from_be_bytes(header[5..9].try_into().unwrap()) == 3 {
            assert!(payload.iter().all(|byte| *byte == 7));
            received += payload.len();
        }
    }
    assert!(received < 70_000, "server ignored HTTP/2 flow control");
    stream.write_all(&frame(8, 0, 0, &70_000u32.to_be_bytes())).unwrap();
    stream.write_all(&frame(8, 0, 3, &70_000u32.to_be_bytes())).unwrap();
    stream.flush().unwrap();
    let mut ended = false;
    while !ended {
        let mut header = [0u8; 9];
        stream.read_exact(&mut header).unwrap();
        let length = (usize::from(header[0]) << 16) | (usize::from(header[1]) << 8) | usize::from(header[2]);
        let mut payload = vec![0; length];
        stream.read_exact(&mut payload).unwrap();
        if header[3] == 0 && u32::from_be_bytes(header[5..9].try_into().unwrap()) == 3 {
            received += payload.len();
            ended = header[4] & 0x1 != 0;
        }
    }
    assert_eq!(received, 70_000);
    // Dynamic index 62 reuses stream 3's incrementally indexed :authority.
    stream.write_all(&frame(1, 0x5, 5, &[0x82, 0x86, 0x84, 0xbe])).unwrap();
    stream.flush().unwrap();
    let mut reused = Vec::new();
    while !reused.windows(5).any(|part| part == b"h2-ok") {
        let mut header = [0u8; 9];
        stream.read_exact(&mut header).unwrap();
        let length = (usize::from(header[0]) << 16) | (usize::from(header[1]) << 8) | usize::from(header[2]);
        let mut payload = vec![0; length];
        stream.read_exact(&mut payload).unwrap();
        reused.extend(payload);
    }

    shutdown.store(true, std::sync::atomic::Ordering::Release);
    drop(stream);
    let report = server.join().unwrap();
    assert_eq!(report.user_accepted, 1);

    let mux = jet_http_mux_new();
    let (mut client, mut server) = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = std::net::TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client, server)
    };
    client.write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n").unwrap();
    client.write_all(&frame(0, 0, 0, b"illegal stream-zero data")).unwrap();
    client.flush().unwrap();
    server.set_read_timeout(Some(std::time::Duration::from_secs(1))).unwrap();
    assert!(jet_http2_serve(
        &mut server,
        &mux,
        &JetHttpServerOptions::safe(),
        &std::sync::atomic::AtomicBool::new(false),
        None,
    ).is_err());
}

#[test]
fn http2_shutdown_sends_goaway_with_last_stream_and_drains_active() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};

    let mux = jet_http_mux_new();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let release = std::sync::Arc::new(AtomicBool::new(false));
    let handler_release = release.clone();
    jet_http_mux_add(&mux, "GET", "/slow", move |_| {
        entered_tx.send(()).expect("entered");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !handler_release.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline, "release timed out");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        jet_http_srv_response(200, &"drained".to_string())
    });
    jet_http_mux_add(&mux, "GET", "/late", |_| jet_http_srv_response(200, &"too late".to_string()));

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let _addr = listener.local_addr().unwrap();
    let mut client = std::net::TcpStream::connect(_addr).unwrap();
    let (mut server_stream, _) = listener.accept().unwrap();
    let shutdown = std::sync::Arc::new(AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let options = JetHttpServerOptions {
        workers: 1,
        admission_queue: 1,
        shutdown_grace: std::time::Duration::from_secs(2),
        read_idle_timeout: std::time::Duration::from_secs(2),
        ..JetHttpServerOptions::safe()
    };
    let server = std::thread::spawn(move || {
        jet_http2_serve(&mut server_stream, &mux, &options, &server_shutdown, None)
    });

    client.set_read_timeout(Some(std::time::Duration::from_millis(50))).unwrap();
    client.write_all(JET_HTTP2_PREFACE).unwrap();
    client.write_all(&h2_frame(4, 0, 0, &[])).unwrap();
    client.write_all(&h2_frame(1, 0x5, 1, &h2_request_headers(0x82, "/slow"))).unwrap();
    client.flush().unwrap();

    let read_frame = |stream: &mut std::net::TcpStream| -> Option<(u8, u8, u32, Vec<u8>)> {
        let mut header = [0u8; 9];
        match stream.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                return None;
            }
            Err(error) => panic!("frame header: {error}"),
        }
        let length = (usize::from(header[0]) << 16) | (usize::from(header[1]) << 8) | usize::from(header[2]);
        let mut payload = vec![0; length];
        stream.read_exact(&mut payload).expect("frame payload");
        Some((
            header[3],
            header[4],
            u32::from_be_bytes(header[5..9].try_into().unwrap()) & 0x7fff_ffff,
            payload,
        ))
    };

    let mut saw_server_settings = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while !saw_server_settings {
        assert!(std::time::Instant::now() < deadline, "missing SETTINGS");
        let Some((kind, flags, stream, _payload)) = read_frame(&mut client) else { continue };
        if kind == 4 && stream == 0 {
            saw_server_settings = true;
            if flags & 0x1 == 0 {
                client.write_all(&h2_frame(4, 0x1, 0, &[])).unwrap();
            }
        }
    }
    entered_rx.recv_timeout(std::time::Duration::from_secs(1)).expect("handler entered");
    shutdown.store(true, Ordering::Release);

    let mut goaway_last = None;
    let mut saw_body = false;
    let mut refused_late = false;
    let mut late_headers_sent = false;
    let mut late_data_sent = false;
    let mut released = false;
    let drain_deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < drain_deadline && (goaway_last.is_none() || !saw_body || !refused_late) {
        if goaway_last.is_some() && !late_headers_sent {
            // Refuse path: HEADERS after GOAWAY must RST immediately (flushed).
            client.write_all(&h2_frame(1, 0x5, 3, &h2_request_headers(0x82, "/late"))).unwrap();
            client.flush().unwrap();
            late_headers_sent = true;
        }
        if refused_late && !late_data_sent {
            // P0: DATA on a higher/unknown stream must be discarded, not kill drain.
            client.write_all(&h2_frame(0, 0x1, 5, b"late")).unwrap();
            client.flush().unwrap();
            late_data_sent = true;
        }
        if refused_late && late_data_sent && !released {
            release.store(true, Ordering::Release);
            released = true;
        }
        let Some((kind, _flags, stream, payload)) = read_frame(&mut client) else { continue };
        match kind {
            7 => {
                assert_eq!(stream, 0);
                assert!(payload.len() >= 8, "GOAWAY payload too short");
                let last = u32::from_be_bytes(payload[..4].try_into().unwrap()) & 0x7fff_ffff;
                let error = u32::from_be_bytes(payload[4..8].try_into().unwrap());
                assert_eq!(error, 0, "GOAWAY should be NO_ERROR");
                goaway_last = Some(last);
            }
            0 if stream == 1 => {
                if payload.windows(7).any(|part| part == b"drained") {
                    saw_body = true;
                }
            }
            3 if stream == 3 => {
                assert_eq!(payload.len(), 4);
                assert_eq!(u32::from_be_bytes(payload[..4].try_into().unwrap()), 7);
                assert!(!released, "RST refuse must flush before the active handler finishes");
                refused_late = true;
            }
            _ => {}
        }
    }
    assert_eq!(goaway_last, Some(1), "GOAWAY must advertise the accepted last stream");
    assert!(saw_body, "active request must drain after GOAWAY");
    assert!(refused_late, "streams after GOAWAY must be refused");
    assert!(late_data_sent, "late DATA after GOAWAY must be exercised");
    release.store(true, Ordering::Release);
    server.join().expect("server join").expect("http2 serve must survive discarded late DATA");
}

#[test]
fn http2_serve_honors_dynamic_grace_over_options_shutdown_grace() {
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    let mux = jet_http_mux_new();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let hold = std::sync::Arc::new(AtomicBool::new(true));
    let handler_hold = hold.clone();
    jet_http_mux_add(&mux, "GET", "/slow", move |_| {
        let _ = entered_tx.send(());
        while handler_hold.load(Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        jet_http_srv_response(200, &"late".to_string())
    });

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let mut client = std::net::TcpStream::connect(addr).unwrap();
    let (mut server_stream, _) = listener.accept().unwrap();
    let shutdown = std::sync::Arc::new(AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let grace_ms = std::sync::Arc::new(AtomicU64::new(45));
    let server_grace = grace_ms.clone();
    let options = JetHttpServerOptions {
        workers: 1,
        admission_queue: 1,
        // If H2 ignored dynamic_grace_ms, drain would wait the full five seconds.
        shutdown_grace: std::time::Duration::from_secs(5),
        read_idle_timeout: std::time::Duration::from_secs(2),
        ..JetHttpServerOptions::safe()
    };
    let server = std::thread::spawn(move || {
        jet_http2_serve(
            &mut server_stream,
            &mux,
            &options,
            &server_shutdown,
            Some(server_grace.as_ref()),
        )
    });

    client.set_read_timeout(Some(std::time::Duration::from_millis(40))).unwrap();
    client.write_all(JET_HTTP2_PREFACE).unwrap();
    client.write_all(&h2_frame(4, 0, 0, &[])).unwrap();
    client.write_all(&h2_frame(1, 0x5, 1, &h2_request_headers(0x82, "/slow"))).unwrap();
    client.flush().unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        assert!(std::time::Instant::now() < deadline, "missing SETTINGS");
        let (kind, flags, stream, _) = match h2_try_read_frame(&mut client) {
            Some(frame) => frame,
            None => continue,
        };
        if kind == 4 && stream == 0 {
            if flags & 0x1 == 0 {
                client.write_all(&h2_frame(4, 0x1, 0, &[])).unwrap();
            }
            break;
        }
    }
    entered_rx.recv_timeout(std::time::Duration::from_secs(1)).expect("handler entered");
    let started = std::time::Instant::now();
    shutdown.store(true, Ordering::Release);
    server.join().expect("join").expect("serve ok");
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_millis(800),
        "dynamic grace ignored; waited options.shutdown_grace: {elapsed:?}"
    );
    hold.store(false, Ordering::Release);
}

#[test]
fn http2_server_api_shutdown_grace_reaches_h2_drain() {
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};

    let mux = jet_http_mux_new();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let hold = std::sync::Arc::new(AtomicBool::new(true));
    let handler_hold = hold.clone();
    jet_http_mux_add(&mux, "GET", "/slow", move |_| {
        let _ = entered_tx.send(());
        while handler_hold.load(Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        jet_http_srv_response(200, &"late".to_string())
    });

    let server = jet_http_server_bind(&"127.0.0.1:0".to_string(), mux).expect("bind");
    let addr = jet_http_server_local_addr(&server).expect("local addr");
    let serving = server.clone();
    let serve = std::thread::spawn(move || jet_http_server_serve(&serving));

    let mut client = std::net::TcpStream::connect(&addr).expect("connect");
    client.set_read_timeout(Some(std::time::Duration::from_millis(40))).unwrap();
    client.write_all(JET_HTTP2_PREFACE).unwrap();
    client.write_all(&h2_frame(4, 0, 0, &[])).unwrap();
    client.write_all(&h2_frame(1, 0x5, 1, &h2_request_headers(0x82, "/slow"))).unwrap();
    client.flush().unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    let mut saw_goaway = false;
    loop {
        assert!(std::time::Instant::now() < deadline, "missing SETTINGS before shutdown");
        let Some((kind, flags, stream, payload)) = h2_try_read_frame(&mut client) else { continue };
        if kind == 4 && stream == 0 && flags & 0x1 == 0 {
            client.write_all(&h2_frame(4, 0x1, 0, &[])).unwrap();
            break;
        }
        if kind == 7 {
            saw_goaway = true;
            let last = u32::from_be_bytes(payload[..4].try_into().unwrap()) & 0x7fff_ffff;
            assert_eq!(last, 1);
        }
    }
    entered_rx.recv_timeout(std::time::Duration::from_secs(1)).expect("handler entered");

    let started = std::time::Instant::now();
    let report = jet_http_server_shutdown(&server, &jet_std::Duration { ms: 70 }).expect("shutdown");
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_millis(900),
        "Server.shutdown(grace) did not bound H2 drain: {elapsed:?}"
    );
    assert!(report.user_accepted >= 1, "{report:?}");

    let observe = std::time::Instant::now() + std::time::Duration::from_millis(400);
    while !saw_goaway && std::time::Instant::now() < observe {
        if let Some((kind, _, stream, payload)) = h2_try_read_frame(&mut client) {
            if kind == 7 && stream == 0 {
                let last = u32::from_be_bytes(payload[..4].try_into().unwrap()) & 0x7fff_ffff;
                assert_eq!(last, 1, "Server API GOAWAY must carry last-stream");
                saw_goaway = true;
            }
        }
    }
    assert!(saw_goaway, "Server.shutdown must drive H2 GOAWAY");
    hold.store(false, Ordering::Release);
    let _ = serve.join().expect("serve join");
}

fn h2_try_read_frame(stream: &mut std::net::TcpStream) -> Option<(u8, u8, u32, Vec<u8>)> {
    use std::io::Read;
    let mut header = [0u8; 9];
    match stream.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
            return None;
        }
        Err(error) => panic!("frame header: {error}"),
    }
    let length = (usize::from(header[0]) << 16) | (usize::from(header[1]) << 8) | usize::from(header[2]);
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload).expect("frame payload");
    Some((
        header[3],
        header[4],
        u32::from_be_bytes(header[5..9].try_into().unwrap()) & 0x7fff_ffff,
        payload,
    ))
}
