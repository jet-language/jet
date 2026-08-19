#![allow(dead_code, non_camel_case_types, unexpected_cfgs)]

mod common;

struct JetTCPListener {
    inner: std::net::TcpListener,
}

trait JetShow {
    fn jet_show(&self) -> String;
}


/// D-FAIL-CONV2=A: included error fragments render failure text through this seam.
trait JetDisplay {
    fn jet_display(&self) -> String;
}

trait __jet_Encode {}
impl<T> __jet_Encode for T {}

trait __jet_Decode: Sized {}

fn jet_enc_json_to_string<T: __jet_Encode>(_value: &T) -> String {
    String::new()
}

fn jet_enc_json_decode<T: __jet_Decode>(_text: &str) -> Result<T, String> {
    Err("unused test decoder".to_string())
}

struct JetFileReader {
    inner: Box<dyn std::io::Read + Send>,
}

struct JetFileWriter {
    inner: Box<dyn std::io::Write + Send>,
}

mod jet_std {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../crates/jet-codegen/src/Prelude/TaskGroup.rs");

    #[derive(Clone, Copy)]
    pub struct Duration {
        pub ms: i64,
    }

    impl Duration {
        // D-TIMERES1=A: mirrors CoreLib/JetStd/CommonTypes.rs Duration::as_millis
        // (this test stub stores milliseconds directly, so it's the identity read).
        pub fn as_millis(self) -> i64 {
            self.ms
        }
    }

    pub struct JetMIME(pub String);

    impl JetMIME {
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

fn jet_runtime_diagnostic(rendered: String) -> ! {
    eprintln!("{rendered}");
    std::process::exit(70);
}

fn jet_scheduler_runtime_stop(msg: &str) -> ! {
    let report = jet_foundation::Outcome::jet_render_runtime_stop(
        "E3001", "", 0, "", "", 1, 1, msg, "",
    );
    if jet_scheduler_panic_should_unwind() {
        panic!("{}", report.rendered);
    }
    jet_runtime_diagnostic(report.rendered);
}

/// Mirrors `Prelude/Core.rs::jet_runtime_stop`, which the included HTTPServer
/// fragment calls. That definition lives in a fragment this test does not
/// include, so the boundary is stubbed here exactly like
/// `jet_scheduler_runtime_stop` above: render through the shared report
/// boundary, then stop. An abort is never the outcome.
fn jet_runtime_stop(code: &'static str, file: &str, line: u32, msg: &str) -> ! {
    let report =
        jet_foundation::Outcome::jet_render_runtime_stop(code, file, line, "", "", 1, 1, msg, "");
    jet_runtime_diagnostic(report.rendered);
}

fn jet_runtime_caught_stop(message: &str) {
    if message.starts_with("Stop [E") {
        eprint!("{message}");
        if !message.ends_with('\n') {
            eprintln!();
        }
        return;
    }
    let report = jet_foundation::Outcome::jet_render_runtime_stop(
        "E3001", "", 0, "", "", 1, 1, message, "",
    );
    eprint!("{}", report.rendered);
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

fn jet_panic(file: &str, line: u32, msg: &str) -> ! {
    panic!("{msg} (at {file}:{line})");
}

// D-TASK-PAUSE-TIER1: mirrors jet_foundation::StructuralDebug::jet_task_control_trace
// (the canonical AOT crate root embeds StructuralDebug.rs alongside Scheduler.rs;
// this test harness includes Scheduler.rs alone, so it needs the same symbol locally).
fn jet_task_control_trace(paused: bool, cancel: bool) -> String {
    format!("paused={paused},cancel={cancel}")
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

#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPMessage.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPRoute.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPClient.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/LiveQuery.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/Ws.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../crates/jet-codegen/src/Prelude/Scheduler.rs");

/// D-LIVEQUERY1/D-WS1: the native transport is the shared Prelude seam. A
/// typed rerun updates its canonical sink and serialized topic, while a later
/// connection receives only the latest event for that topic.
#[test]
fn live_query_rerun_publishes_and_reconnect_replays_latest() {
    let footprint = format!("law_live_{}.rows", std::process::id());
    let frames = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let frame_sink = frames.clone();
    let transport_id = jet_app_ws_register(std::sync::Arc::new(move |frame| {
        frame_sink.lock().unwrap().push(frame);
    }));
    assert_ne!(transport_id, 0, "live transport registration must succeed");

    let reruns = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rerun_count = reruns.clone();
    let query = jet_app_live_query(footprint.clone(), "v1".to_string(), move || {
        let call = rerun_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        Ok(format!("v{}", call + 1))
    });
    assert!(query.error.is_empty(), "query registration failed: {:?}", query.error);

    let delivered = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let delivered_sink = delivered.clone();
    let query = jet_app_live_bind_sink(
        &query,
        std::sync::Arc::new(move |value| delivered_sink.lock().unwrap().push(value)),
    );
    assert!(query.error.is_empty(), "sink binding failed: {:?}", query.error);
    assert_eq!(jet_app_live_get(&query), "v1");

    assert_eq!(jet_app_invalidate(footprint.clone()), 1);
    assert_eq!(reruns.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(jet_app_live_get(&query), "v2");
    assert!(jet_app_live_show(&query).contains("generation=2"));
    assert_eq!(*delivered.lock().unwrap(), vec!["v2".to_string()]);

    let topic_prefix = format!("live:{}:", query.id);
    let published = frames
        .lock()
        .unwrap()
        .iter()
        .filter(|frame| frame.starts_with(&topic_prefix))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(published.len(), 2, "initial and rerun events must publish");
    assert!(published[0].ends_with(&format!(":{}:v1", footprint)));
    assert!(published[1].ends_with(&format!(":{}:v2", footprint)));

    let replayed = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let replay_sink = replayed.clone();
    let reconnect_id = jet_app_ws_register(std::sync::Arc::new(move |frame| {
        replay_sink.lock().unwrap().push(frame);
    }));
    assert_ne!(reconnect_id, 0, "reconnect registration must succeed");
    assert_eq!(
        replayed
            .lock()
            .unwrap()
            .iter()
            .filter(|frame| frame.starts_with(&topic_prefix))
            .cloned()
            .collect::<Vec<_>>(),
        vec![format!("live:{}:2:{}:v2", query.id, footprint)]
    );

    jet_app_ws_unregister(reconnect_id);
    jet_app_ws_unregister(transport_id);
}

#[test]
fn direct_ws_consumers_include_client_core_first() {
    fn raw_string_start(bytes: &[u8], index: usize) -> Option<(usize, usize)> {
        let mut cursor = index;
        if bytes.get(cursor) == Some(&b'b') {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'r') {
            return None;
        }
        cursor += 1;
        let start = cursor;
        while bytes.get(cursor) == Some(&b'#') {
            cursor += 1;
        }
        (bytes.get(cursor) == Some(&b'"')).then_some((cursor + 1, cursor - start))
    }

    fn include_at(bytes: &[u8], index: usize) -> Option<(usize, String)> {
        if bytes.get(index..index + "include".len()) != Some(b"include") {
            return None;
        }
        if index != 0
            && bytes
                .get(index - 1)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            return None;
        }
        let mut cursor = index + "include".len();
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'!') {
            return None;
        }
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'(') {
            return None;
        }
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'"') {
            return None;
        }
        cursor += 1;
        let target_start = cursor;
        while let Some(byte) = bytes.get(cursor) {
            match byte {
                b'\\' => cursor += 2,
                b'"' => break,
                b'\n' | b'\r' => return None,
                _ => cursor += 1,
            }
        }
        if bytes.get(cursor) != Some(&b'"') {
            return None;
        }
        let target = std::str::from_utf8(bytes.get(target_start..cursor)?)
            .ok()?
            .replace('\\', "/");
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b')') {
            return None;
        }
        cursor += 1;
        if bytes.get(cursor) == Some(&b';') {
            cursor += 1;
        }
        Some((cursor, target))
    }

    fn char_literal_end(bytes: &[u8], quote: usize) -> Option<usize> {
        if bytes.get(quote) != Some(&b'\'') {
            return None;
        }
        let mut cursor = quote + 1;
        if bytes.get(cursor) == Some(&b'\\') {
            cursor += 1;
            match *bytes.get(cursor)? {
                b'u' if bytes.get(cursor + 1) == Some(&b'{') => {
                    cursor += 2;
                    while bytes.get(cursor).is_some_and(|byte| *byte != b'}') {
                        cursor += 1;
                    }
                    if bytes.get(cursor) != Some(&b'}') {
                        return None;
                    }
                    cursor += 1;
                }
                b'x' => cursor += 3,
                _ => cursor += 1,
            }
        } else {
            let text = std::str::from_utf8(bytes.get(cursor..)?).ok()?;
            cursor += text.chars().next()?.len_utf8();
        }
        (bytes.get(cursor) == Some(&b'\'')).then_some(cursor + 1)
    }

    fn actual_includes(text: &str) -> Vec<(usize, String)> {
        let bytes = text.as_bytes();
        let mut includes = Vec::new();
        let mut index = 0;
        let mut line = 1;
        let mut block_depth = 0usize;
        while index < bytes.len() {
            if block_depth != 0 {
                match bytes.get(index..index + 2) {
                    Some(b"/*") => {
                        block_depth += 1;
                        index += 2;
                    }
                    Some(b"*/") => {
                        block_depth -= 1;
                        index += 2;
                    }
                    _ => {
                        if bytes[index] == b'\n' {
                            line += 1;
                        }
                        index += 1;
                    }
                }
                continue;
            }
            match bytes.get(index..index + 2) {
                Some(b"//") => {
                    while bytes.get(index).is_some_and(|byte| *byte != b'\n') {
                        index += 1;
                    }
                    continue;
                }
                Some(b"/*") => {
                    block_depth = 1;
                    index += 2;
                    continue;
                }
                _ => {}
            }
            let char_quote = if bytes[index] == b'\'' {
                Some(index)
            } else if bytes[index] == b'b' && bytes.get(index + 1) == Some(&b'\'') {
                Some(index + 1)
            } else {
                None
            };
            if let Some(end) = char_quote.and_then(|quote| char_literal_end(bytes, quote)) {
                index = end;
                continue;
            }
            if let Some((mut cursor, hashes)) = raw_string_start(bytes, index) {
                loop {
                    let Some(byte) = bytes.get(cursor) else {
                        index = cursor;
                        break;
                    };
                    if *byte == b'\n' {
                        line += 1;
                    }
                    if *byte == b'"'
                        && (0..hashes).all(|offset| {
                            bytes.get(cursor + 1 + offset) == Some(&b'#')
                        })
                    {
                        index = cursor + 1 + hashes;
                        break;
                    }
                    cursor += 1;
                }
                continue;
            }
            let quote = if bytes[index] == b'"' {
                Some(b'"')
            } else if bytes[index] == b'b' && bytes.get(index + 1) == Some(&b'"') {
                index += 1;
                Some(b'"')
            } else {
                None
            };
            if let Some(quote) = quote {
                index += 1;
                while let Some(byte) = bytes.get(index) {
                    if *byte == b'\\' {
                        index += 2;
                    } else {
                        if *byte == b'\n' {
                            line += 1;
                        }
                        index += 1;
                        if *byte == quote {
                            break;
                        }
                    }
                }
                continue;
            }
            if let Some((end, target)) = include_at(bytes, index) {
                includes.push((line, target));
                line += bytes[index..end]
                    .iter()
                    .filter(|byte| **byte == b'\n')
                    .count();
                index = end;
                continue;
            }
            if bytes[index] == b'\n' {
                line += 1;
            }
            index += 1;
        }
        includes
    }

    fn collect_rs(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        let mut entries = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                collect_rs(&path, files);
            } else if path.extension().and_then(|part| part.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }

    let hostile = r###"
// include!("ignored/line/Ws.rs");
/* include!("ignored/block/Ws.rs"); */
const NORMAL: &str = "include!(\"ignored/normal/Ws.rs\");";
const RAW: &str = r#"include!("ignored/raw/Ws.rs");"#;
include!("kept/WsClient.rs");
include!("kept/Ws.rs");
"###;
    assert_eq!(
        actual_includes(hostile),
        [
            (6, "kept/WsClient.rs".to_string()),
            (7, "kept/Ws.rs".to_string()),
        ]
    );

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_rs(&root.join("crates"), &mut files);
    collect_rs(&root.join("tests"), &mut files);
    let mut consumers = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        let includes = actual_includes(&text);
        for (index, (line, target)) in includes.iter().enumerate() {
            if !target.ends_with("/Ws.rs") {
                continue;
            }
            let Some((client_line, client_target)) = index
                .checked_sub(1)
                .and_then(|previous| includes.get(previous))
            else {
                panic!("{} omits WsClient.rs", path.display());
            };
            assert_eq!(
                *client_line + 1,
                *line,
                "{} must include WsClient.rs on the line before Ws.rs",
                path.display()
            );
            assert!(
                client_target.ends_with("/WsClient.rs"),
                "{} must include WsClient.rs immediately before Ws.rs",
                path.display()
            );
            consumers.push(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
    consumers.sort();
    assert_eq!(
        consumers,
        [
            std::path::PathBuf::from("crates/jet-jit/src/net_http_rt.rs"),
            std::path::PathBuf::from("tests/http_server_lifecycle.rs"),
            std::path::PathBuf::from("tests/ws_law.rs"),
        ]
    );
}

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
            &JetHTTPServerOptions::safe(),
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
            &JetHTTPServerOptions::safe(),
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
            &JetHTTPServerOptions::safe(),
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
            &JetHTTPServerOptions::safe(),
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
