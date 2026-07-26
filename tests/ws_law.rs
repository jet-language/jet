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

fn jet_scheduler_task_panic_enter() {
    JET_IN_SCHEDULER_TASK.with(|task| task.set(true));
}
fn jet_scheduler_task_panic_leave() {
    JET_IN_SCHEDULER_TASK.with(|task| task.set(false));
}
fn jet_scheduler_panic_should_unwind() -> bool {
    JET_IN_SCHEDULER_TASK.with(|task| task.get())
}
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

fn jet_observe_registry() -> Option<&'static std::sync::Arc<JetObserveRegistry>> {
    None
}
fn jet_observe_task_update(_state: &'static str, _wait: &str, _deadline_ms: Option<i64>) {}

struct LogField {
    key: String,
    value: String,
}

fn jet_log_emit(_level: &str, _msg: &str, _fields: &[LogField]) {}

include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/HttpMessage.rs");
include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/HttpRoute.rs");
include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/HttpClient.rs");
include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/HttpServer.rs");
include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs");
include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/Ws.rs");
include!("../crates/jet-codegen/src/Prelude/Scheduler.rs");

#[test]
fn accept_key_matches_rfc6455_example() {
    // RFC6455 section 1.3 example.
    assert_eq!(
        jet_ws_accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
        "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
    );
}

#[test]
fn client_and_server_echo_text_over_live_sockets() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mux = jet_http_mux_new();
        mux.add("GET", "/live", |req| {
            let mut sock = jet_ws_upgrade(&req).expect("upgrade");
            let message = jet_ws_recv(&sock).expect("recv");
            let text = jet_ws_message_text(&message).expect("text");
            jet_ws_send_text(&sock, &format!("echo:{text}")).expect("send");
            jet_ws_close(&sock, 1000, &String::new()).expect("close");
            Ok(jet_http_srv_response(500, &"unused".to_string()))
        });
        jet_http_server_handle_stream(
            &mut stream,
            &mux,
            &JetHttpServerOptions::safe(),
            &std::sync::atomic::AtomicBool::new(false),
            None,
            None,
        );
    });

    let url = format!("ws://{addr}/live");
    let client = jet_ws_connect(&url).expect("connect");
    jet_ws_send_text(&client, &"ping".to_string()).expect("client send");
    let reply = jet_ws_recv(&client).expect("client recv");
    assert_eq!(jet_ws_message_text(&reply).unwrap(), "echo:ping");
    let closed = jet_ws_recv(&client).expect("client close");
    assert!(jet_ws_message_is_close(&closed));
    server.join().unwrap();
}

#[test]
fn oversized_client_frame_is_rejected() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mux = jet_http_mux_new();
        mux.add("GET", "/live", |req| {
            let mut sock = jet_ws_upgrade(&req).expect("upgrade");
            sock.max_message = 8;
            let err = jet_ws_recv(&sock).expect_err("must reject");
            assert!(matches!(err, JetWsError::MessageTooLarge { limit: 8 }));
            Ok(jet_http_srv_response(500, &"unused".to_string()))
        });
        jet_http_server_handle_stream(
            &mut stream,
            &mux,
            &JetHttpServerOptions::safe(),
            &std::sync::atomic::AtomicBool::new(false),
            None,
            None,
        );
    });

    let url = format!("ws://{addr}/live");
    let client = jet_ws_connect(&url).expect("connect");
    let _ = jet_ws_send_text(&client, &"0123456789".to_string());
    server.join().unwrap();
}

#[test]
fn ambient_deadline_cancels_recv() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mux = jet_http_mux_new();
        mux.add("GET", "/live", |req| {
            let mut sock = jet_ws_upgrade(&req).expect("upgrade");
            TEST_DEADLINE_EXCEEDED.with(|deadline| deadline.set(true));
            let err = jet_ws_recv(&sock).expect_err("cancelled");
            assert!(matches!(err, JetWsError::Cancelled));
            TEST_DEADLINE_EXCEEDED.with(|deadline| deadline.set(false));
            Ok(jet_http_srv_response(500, &"unused".to_string()))
        });
        jet_http_server_handle_stream(
            &mut stream,
            &mux,
            &JetHttpServerOptions::safe(),
            &std::sync::atomic::AtomicBool::new(false),
            None,
            None,
        );
    });

    let url = format!("ws://{addr}/live");
    let _client = jet_ws_connect(&url).expect("connect");
    server.join().unwrap();
}

#[test]
fn hostile_handshake_missing_version_fails_closed() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mux = jet_http_mux_new();
        mux.add("GET", "/live", |req| {
            match jet_ws_upgrade(&req) {
                Ok(_) => panic!("expected invalid handshake"),
                Err(err) => assert!(matches!(err, JetWsError::InvalidHandshake)),
            }
            Ok(jet_http_srv_response(400, &"bad".to_string()))
        });
        jet_http_server_handle_stream(
            &mut stream,
            &mux,
            &JetHttpServerOptions::safe(),
            &std::sync::atomic::AtomicBool::new(false),
            None,
            None,
        );
    });

    use std::io::Write;
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    stream
        .write_all(
            b"GET /live HTTP/1.1\r\nHost: local\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
        )
        .unwrap();
    drop(stream);
    server.join().unwrap();
}
