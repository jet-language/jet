//! D-BROWSER-AUTO1=A (#772): native std-only WebDriver BiDi protocol proof.
//!
//! The Jet product path compiles and runs against a local mock BiDi endpoint.
//! It never depends on Node, Playwright, or an installed browser.

mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::fs;
use std::thread;
use std::time::Duration;

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h0 = 0x6745_2301u32;
    let mut h1 = 0xefcd_ab89u32;
    let mut h2 = 0x98ba_dcfeu32;
    let mut h3 = 0x1032_5476u32;
    let mut h4 = 0xc3d2_e1f0u32;
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, word) in [h0, h1, h2, h3, h4].iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        out.push(ALPHABET[(a >> 2) as usize] as char);
        out.push(ALPHABET[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn accept_websocket(stream: &mut TcpStream) {
    let mut request = Vec::new();
    let mut buf = [0u8; 1024];
    while !request.windows(4).any(|part| part == b"\r\n\r\n") {
        let n = stream.read(&mut buf).expect("read handshake");
        assert_ne!(n, 0, "client closed during handshake");
        request.extend_from_slice(&buf[..n]);
        assert!(request.len() <= 16 * 1024, "oversized handshake");
    }
    let request = String::from_utf8(request).expect("ASCII handshake");
    let key = request
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("sec-websocket-key"))
                .map(|(_, value)| value.trim())
        })
        .expect("websocket key");
    let digest = sha1(format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes());
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        base64(&digest)
    );
    stream.write_all(response.as_bytes()).unwrap();
}

fn read_text_frame(stream: &mut TcpStream) -> String {
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).unwrap();
    assert_eq!(head[0] & 0x0f, 1, "expected text frame");
    assert_ne!(head[1] & 0x80, 0, "client frame must be masked");
    let mut len = (head[1] & 0x7f) as usize;
    if len == 126 {
        let mut ext = [0u8; 2];
        stream.read_exact(&mut ext).unwrap();
        len = u16::from_be_bytes(ext) as usize;
    } else if len == 127 {
        let mut ext = [0u8; 8];
        stream.read_exact(&mut ext).unwrap();
        len = usize::try_from(u64::from_be_bytes(ext)).unwrap();
    }
    let mut mask = [0u8; 4];
    stream.read_exact(&mut mask).unwrap();
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).unwrap();
    for (i, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[i % 4];
    }
    String::from_utf8(payload).unwrap()
}

fn write_text_frame(stream: &mut TcpStream, text: &str) {
    let bytes = text.as_bytes();
    let mut frame = vec![0x81];
    match bytes.len() {
        0..=125 => frame.push(bytes.len() as u8),
        126..=65535 => {
            frame.push(126);
            frame.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        }
        _ => panic!("mock response too large"),
    }
    frame.extend_from_slice(bytes);
    stream.write_all(&frame).unwrap();
}

fn field(text: &str, name: &str) -> String {
    let marker = format!("\"{name}\":");
    let rest = text.split_once(&marker).unwrap_or_else(|| panic!("missing {name}: {text}")).1;
    if let Some(rest) = rest.strip_prefix('"') {
        return rest.split_once('"').unwrap().0.to_string();
    }
    rest.chars().take_while(char::is_ascii_digit).collect()
}

fn run_mock(listener: TcpListener) -> Vec<String> {
    let (mut stream, _) = listener.accept().unwrap();
    accept_websocket(&mut stream);
    let mut methods = Vec::new();
    for _ in 0..14 {
        let request = read_text_frame(&mut stream);
        let id = field(&request, "id");
        let method = field(&request, "method");
        methods.push(method.clone());
        let result = match method.as_str() {
            "session.status" => r#"{"ready":true,"message":"ready"}"#,
            "session.new" => r#"{"sessionId":"mock-session","capabilities":{"browserName":"mock","browserVersion":"1","goog:cdp":true}}"#,
            "browser.createUserContext" => r#"{"userContext":"user-1"}"#,
            "browsingContext.create" => r#"{"context":"page-1"}"#,
            "browsingContext.navigate" => r#"{"url":"https://example.test/app","navigation":"nav-1"}"#,
            "browsingContext.locateNodes" => r#"{"nodes":[{"type":"node","sharedId":"node-1"}]}"#,
            "input.performActions"
            | "session.subscribe"
            | "goog:cdp.sendCommand"
            | "browsingContext.close"
            | "browser.removeUserContext"
            | "session.end" => "{}",
            other => panic!("unexpected BiDi method {other}: {request}"),
        };
        write_text_frame(&mut stream, &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#));
        if method == "session.subscribe" {
            write_text_frame(
                &mut stream,
                r#"{"type":"event","method":"log.entryAdded","params":{"text":"SECRET PAGE DATA"}}"#,
            );
        }
    }
    methods
}

#[derive(Clone, Copy)]
enum HostileReply {
    Malformed,
    MismatchedId,
    ProtocolError,
    Timeout,
    NoCdp,
}

fn run_hostile(listener: TcpListener, reply: HostileReply) {
    let (mut stream, _) = listener.accept().unwrap();
    accept_websocket(&mut stream);
    let request = read_text_frame(&mut stream);
    let id: i64 = field(&request, "id").parse().unwrap();
    match reply {
        HostileReply::Malformed => write_text_frame(&mut stream, "{not-json"),
        HostileReply::MismatchedId => write_text_frame(
            &mut stream,
            &format!(r#"{{"type":"success","id":{},"result":{{}}}}"#, id + 1),
        ),
        HostileReply::ProtocolError => write_text_frame(
            &mut stream,
            &format!(
                r#"{{"type":"error","id":{id},"error":"invalid argument","message":"SECRET SERVER DATA","stacktrace":"SECRET STACK"}}"#
            ),
        ),
        HostileReply::Timeout => thread::sleep(Duration::from_millis(500)),
        HostileReply::NoCdp => {
            write_text_frame(
                &mut stream,
                &format!(
                    r#"{{"type":"success","id":{id},"result":{{"ready":true,"message":"ready"}}}}"#
                ),
            );
            let request = read_text_frame(&mut stream);
            assert_eq!(field(&request, "method"), "session.new");
            assert!(request.contains(r#""capabilities":{"alwaysMatch":{}}"#));
            let id = field(&request, "id");
            write_text_frame(
                &mut stream,
                &format!(
                    r#"{{"type":"success","id":{id},"result":{{"sessionId":"no-cdp","capabilities":{{"goog:cdp":false}}}}}}"#
                ),
            );
        }
    }
}

fn run_lifecycle(listener: TcpListener) -> Vec<String> {
    let (mut stream, _) = listener.accept().unwrap();
    accept_websocket(&mut stream);
    let mut methods = Vec::new();
    let mut page_closes = 0;
    let mut context_closes = 0;
    let mut session_closes = 0;
    for _ in 0..17 {
        let request = read_text_frame(&mut stream);
        let id = field(&request, "id");
        let method = field(&request, "method");
        methods.push(method.clone());
        let result = match method.as_str() {
            "session.status" => Some(r#"{"ready":true,"message":"ready"}"#),
            "session.new" => Some(r#"{"sessionId":"lifecycle","capabilities":{}}"#),
            "browser.createUserContext" => Some(r#"{"userContext":"context"}"#),
            "browsingContext.create" => Some(r#"{"context":"page"}"#),
            "browsingContext.close" => {
                page_closes += 1;
                (page_closes != 1).then_some("{}")
            }
            "browser.removeUserContext" => {
                context_closes += 1;
                (context_closes != 1).then_some("{}")
            }
            "session.end" => {
                session_closes += 1;
                (session_closes != 1).then_some("{}")
            }
            other => panic!("unexpected lifecycle method {other}: {request}"),
        };
        if let Some(result) = result {
            write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
            );
        } else {
            write_text_frame(
                &mut stream,
                &format!(
                    r#"{{"type":"error","id":{id},"error":"unknown error","message":"SECRET CLOSE FAILURE"}}"#
                ),
            );
        }
    }
    methods
}

fn run_tab_frame_lifecycle(listener: TcpListener) -> Vec<String> {
    let (mut stream, _) = listener.accept().unwrap();
    accept_websocket(&mut stream);
    let mut methods = Vec::new();
    let mut creates = 0;
    for _ in 0..11 {
        let request = read_text_frame(&mut stream);
        let id = field(&request, "id");
        let method = field(&request, "method");
        methods.push(method.clone());
        let result = match method.as_str() {
            "session.status" => r#"{"ready":true,"message":"ready"}"#.to_string(),
            "session.new" => r#"{"sessionId":"tabs","capabilities":{}}"#.to_string(),
            "browser.createUserContext" => r#"{"userContext":"ctx"}"#.to_string(),
            "browsingContext.create" => {
                creates += 1;
                assert!(
                    request.contains(r#""type":"tab""#),
                    "tab create must request type=tab: {request}"
                );
                format!(r#"{{"context":"tab-{creates}"}}"#)
            }
            "browsingContext.getTree" => {
                r#"{"contexts":[{"context":"tab-1","children":[{"context":"frame-a","children":[]},{"context":"frame-b","children":[]}]}]}"#
                    .to_string()
            }
            "browsingContext.close" | "browser.removeUserContext" | "session.end" => {
                "{}".to_string()
            }
            other => panic!("unexpected tab/frame method {other}: {request}"),
        };
        write_text_frame(
            &mut stream,
            &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
        );
    }
    methods
}

fn run_closed_frames_hostile(listener: TcpListener) -> Vec<String> {
    let (mut stream, _) = listener.accept().unwrap();
    accept_websocket(&mut stream);
    let mut methods = Vec::new();
    for _ in 0..7 {
        let request = read_text_frame(&mut stream);
        let id = field(&request, "id");
        let method = field(&request, "method");
        methods.push(method.clone());
        let result = match method.as_str() {
            "session.status" => r#"{"ready":true,"message":"ready"}"#,
            "session.new" => r#"{"sessionId":"closed","capabilities":{}}"#,
            "browser.createUserContext" => r#"{"userContext":"ctx"}"#,
            "browsingContext.create" => r#"{"context":"tab"}"#,
            "browsingContext.close" | "browser.removeUserContext" | "session.end" => "{}",
            other => panic!("unexpected closed-frame method {other}: {request}"),
        };
        write_text_frame(
            &mut stream,
            &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
        );
    }
    methods
}

fn run_smoke(listener: TcpListener) -> Vec<String> {
    let (mut stream, _) = listener.accept().unwrap();
    accept_websocket(&mut stream);
    let mut methods = Vec::new();
    for _ in 0..3 {
        let request = read_text_frame(&mut stream);
        let id = field(&request, "id");
        let method = field(&request, "method");
        methods.push(method.clone());
        let result = match method.as_str() {
            "session.status" => r#"{"ready":true,"message":"ready"}"#,
            "session.new" => {
                assert!(request.contains(r#""capabilities":{"alwaysMatch":{}}"#));
                r#"{"sessionId":"dev-session","capabilities":{"browserName":"mock"}}"#
            }
            "session.end" => "{}",
            other => panic!("unexpected smoke method {other}: {request}"),
        };
        write_text_frame(
            &mut stream,
            &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
        );
    }
    methods
}

fn run_profile_smoke(listener: TcpListener) -> Vec<String> {
    let (mut stream, _) = listener.accept().unwrap();
    accept_websocket(&mut stream);
    let mut methods = Vec::new();
    for _ in 0..4 {
        let request = read_text_frame(&mut stream);
        let id = field(&request, "id");
        let method = field(&request, "method");
        methods.push(method.clone());
        let result = match method.as_str() {
            "session.status" => r#"{"ready":true,"message":"ready"}"#,
            "session.new" => r#"{"sessionId":"profile","capabilities":{"goog:cdp":false}}"#,
            "webExtension.install" | "session.end" => "{}",
            other => panic!("unexpected profile method {other}: {request}"),
        };
        write_text_frame(
            &mut stream,
            &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
        );
    }
    methods
}

fn run_handshake(
    listener: TcpListener,
    status_result: &'static str,
    expect_session: bool,
) -> Vec<String> {
    let (mut stream, _) = listener.accept().unwrap();
    accept_websocket(&mut stream);
    let request = read_text_frame(&mut stream);
    let id = field(&request, "id");
    let mut methods = vec![field(&request, "method")];
    write_text_frame(
        &mut stream,
        &format!(r#"{{"type":"success","id":{id},"result":{status_result}}}"#),
    );
    if expect_session {
        let request = read_text_frame(&mut stream);
        let id = field(&request, "id");
        methods.push(field(&request, "method"));
        write_text_frame(
            &mut stream,
            &format!(
                r#"{{"type":"success","id":{id},"result":{{"sessionId":"codec","capabilities":{{}}}}}}"#
            ),
        );
        let request = read_text_frame(&mut stream);
        let id = field(&request, "id");
        methods.push(field(&request, "method"));
        write_text_frame(
            &mut stream,
            &format!(r#"{{"type":"success","id":{id},"result":{{}}}}"#),
        );
    } else {
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut pending = [0u8; 1];
        if stream.peek(&mut pending).unwrap_or(0) != 0 {
            methods.push(field(&read_text_frame(&mut stream), "method"));
        }
    }
    methods
}

fn run_malformed_session(listener: TcpListener) -> Vec<String> {
    let (mut stream, _) = listener.accept().unwrap();
    accept_websocket(&mut stream);
    let mut methods = Vec::new();
    for result in [
        r#"{"ready":true,"message":"ready"}"#,
        r#"{"sessionId":1,"capabilities":[]}"#,
        "{}",
    ] {
        let request = read_text_frame(&mut stream);
        let id = field(&request, "id");
        methods.push(field(&request, "method"));
        write_text_frame(
            &mut stream,
            &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
        );
    }
    methods
}

fn run_event_storm(listener: TcpListener) -> Vec<String> {
    let (mut stream, _) = listener.accept().unwrap();
    accept_websocket(&mut stream);
    let mut methods = Vec::new();
    for command in 0..4 {
        let request = read_text_frame(&mut stream);
        let id = field(&request, "id");
        let method = field(&request, "method");
        methods.push(method.clone());
        let result = match command {
            0 => r#"{"ready":true,"message":"ready"}"#,
            1 => r#"{"sessionId":"storm","capabilities":{}}"#,
            2 => {
                for index in 0..300 {
                    let method = if index == 100 {
                        format!("SECRET_REMOTE_METHOD_{}", "x".repeat(4_000))
                    } else {
                        format!("event.{index}")
                    };
                    write_text_frame(
                        &mut stream,
                        &format!(r#"{{"type":"event","method":"{method}","params":{{}}}}"#),
                    );
                }
                r#"{"ready":true,"message":"ready"}"#
            }
            3 => "{}",
            _ => unreachable!(),
        };
        write_text_frame(
            &mut stream,
            &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
        );
    }
    methods
}

fn run_continuous_events(listener: TcpListener) -> Vec<String> {
    let (mut stream, _) = listener.accept().unwrap();
    accept_websocket(&mut stream);
    let mut methods = Vec::new();
    for command in 0..2 {
        let request = read_text_frame(&mut stream);
        let id = field(&request, "id");
        let method = field(&request, "method");
        methods.push(method);
        let result = match command {
            0 => r#"{"ready":true,"message":"ready"}"#,
            1 => r#"{"sessionId":"deadline","capabilities":{}}"#,
            _ => unreachable!(),
        };
        write_text_frame(
            &mut stream,
            &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
        );
    }
    let request = read_text_frame(&mut stream);
    methods.push(field(&request, "method"));
    for index in 0..60 {
        write_text_frame(
            &mut stream,
            &format!(
                r#"{{"type":"event","method":"network.tick{index}","params":{{}}}}"#
            ),
        );
        thread::sleep(Duration::from_millis(5));
    }
    let request = read_text_frame(&mut stream);
    let id = field(&request, "id");
    methods.push(field(&request, "method"));
    write_text_frame(
        &mut stream,
        &format!(r#"{{"type":"success","id":{id},"result":{{}}}}"#),
    );
    methods
}

fn hostile_listener(reply: HostileReply) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!("ws://{}/session?token=HOSTILE_SECRET", listener.local_addr().unwrap());
    let server = thread::spawn(move || run_hostile(listener, reply));
    (endpoint, server)
}

fn handshake_listener(
    status_result: &'static str,
    expect_session: bool,
) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!("ws://{}/session", listener.local_addr().unwrap());
    let server = thread::spawn(move || run_handshake(listener, status_result, expect_session));
    (endpoint, server)
}

fn malformed_session_listener() -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!("ws://{}/session", listener.local_addr().unwrap());
    let server = thread::spawn(move || run_malformed_session(listener));
    (endpoint, server)
}

#[test]
fn native_bidi_bad_method_arguments_stop_in_sema() {
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? return
    timeout :: browser.timeout(500) ?? return
    session :: browser.connect_profile("ws://127.0.0.1:1", profile, timeout) ?? return
    context :: session.context() ?? return
    page :: context.page() ?? return
    locator :: page.get_by_role("button", "Save")
    text :: page.get_by_text("Save")
    label :: page.get_by_label("Name")
    placeholder :: page.get_by_placeholder("email")
    test_id :: page.get_by_test_id("save")
    css :: page.get_by_css("button.save")
    protocol :: session.protocol("bidi") ?? return

    session.subscribe(1) ?? return
    session.next_event("soon") ?? return
    session.add_intercept(1) ?? return
    session.add_intercept_url(1, 2) ?? return
    session.continue_request(1) ?? return
    session.fail_request(1) ?? return
    session.fulfill_request(1, "200", 3) ?? return
    session.protocol(1) ?? return
    page.goto(1) ?? return
    wrong_locator :: page.get_by_role(1, 2)
    page.get_by_text(1)
    page.get_by_label(1)
    page.get_by_placeholder(1)
    page.get_by_test_id(1)
    page.get_by_css(1)
    locator.wait(1) ?? return
    locator.wait_gone(1) ?? return
    locator.fill(1) ?? return
    locator.press(1) ?? return
    protocol.send(1, 2) ?? return
    event_name :: "log.entryAdded"
    session.subscribe(^event_name) ?? return
    session.close(1) ?? return
}
"#;
    let diags = jet::compile(source).expect_err("Browser bad arguments must fail in sema");
    for method in [
        "subscribe",
        "next_event",
        "add_intercept",
        "add_intercept_url",
        "continue_request",
        "fail_request",
        "fulfill_request",
        "protocol",
        "goto",
        "get_by_role",
        "get_by_text",
        "get_by_label",
        "get_by_placeholder",
        "get_by_test_id",
        "get_by_css",
        "wait",
        "wait_gone",
        "fill",
        "press",
        "send",
    ] {
        assert!(
            diags
                .iter()
                .any(|diag| diag.code == "E0112" && diag.what.contains(method)),
            "missing typed `{method}` diagnostic: {diags:?}"
        );
    }
    assert!(
        diags
            .iter()
            .any(|diag| diag.code == "E0203" && diag.what.contains("does not consume")),
        "Browser read-only arguments accepted an ownership move: {diags:?}"
    );
    assert!(
        diags
            .iter()
            .any(|diag| diag.code == "E0104" && diag.what.contains("`close`")),
        "Browser wrong arity did not use the standard diagnostic: {diags:?}"
    );
}

#[test]
fn native_bidi_session_handles_cannot_cross_tasks_or_channels() {
    let source = r#"
use core.browser as browser
use core.tasks

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? return
    timeout :: browser.timeout(10) ?? return
    session :: browser.connect_profile("ws://127.0.0.1:1", profile, timeout) ?? return
    task :: tasks.spawn(() => session.close())
    (sender, channel) :: tasks.channel<Browser>()
    sender.send(session)
}
"#;
    let diags = jet::compile(source).expect_err("Browser handles must stay thread-confined");
    assert!(
        diags.iter().filter(|diag| diag.code == "E1102").count() >= 2,
        "task and channel crossings must stop in sema without rustc: {diags:?}"
    );
}

#[test]
fn native_bidi_io_handle_methods_record_net_effect() {
    let validators = r#"
use core.browser as browser

fn validators() =[]=> {
    profile :: browser.profile("bidi-2025.5") ?? return
    timeout :: browser.timeout(500) ?? return
}
fn run() { validators() }
"#;
    jet::compile(validators).expect("profile and timeout validation must stay pure");

    let source = r#"
use core.browser as browser

fn connect() =[FS]=> Unit { browser.connect("ws://127.0.0.1:1") ?? return }
fn context(session: Browser) =[FS]=> Unit { session.context() ?? return }
fn subscribe(session: Browser) =[FS]=> Unit { session.subscribe("log.entryAdded") ?? return }
fn next(session: Browser, timeout: BrowserTimeout) =[FS]=> Unit { session.next_event(timeout) ?? return }
fn add_intercept(session: Browser) =[FS]=> Unit { session.add_intercept("beforeRequestSent") ?? return }
fn add_intercept_url(session: Browser) =[FS]=> Unit {
    session.add_intercept_url("beforeRequestSent", "https://example.test/*") ?? return
}
fn continue_request(session: Browser) =[FS]=> Unit { session.continue_request("req-1") ?? return }
fn fail_request(session: Browser) =[FS]=> Unit { session.fail_request("req-1") ?? return }
fn fulfill_request(session: Browser) =[FS]=> Unit {
    session.fulfill_request("req-1", 200, "blocked") ?? return
}
fn protocol(session: Browser) =[FS]=> Unit { session.protocol("bidi") ?? return }
fn close(session: Browser) =[FS]=> Unit { session.close() ?? return }
fn page(context: BrowserContext) =[FS]=> Unit { context.page() ?? return }
fn tab(context: BrowserContext) =[FS]=> Unit { context.tab() ?? return }
fn goto(page: BrowserPage) =[FS]=> Unit { page.goto("https://example.test") ?? return }
fn frames(page: BrowserPage) =[FS]=> Unit { page.frames() ?? return }
fn frame_close(frame: BrowserFrame) =[FS]=> Unit { frame.close() ?? return }
fn wait(locator: BrowserLocator, timeout: BrowserTimeout) =[FS]=> Unit { locator.wait(timeout) ?? return }
fn wait_gone(locator: BrowserLocator, timeout: BrowserTimeout) =[FS]=> Unit { locator.wait_gone(timeout) ?? return }
fn click(locator: BrowserLocator) =[FS]=> Unit { locator.click() ?? return }
fn hover(locator: BrowserLocator) =[FS]=> Unit { locator.hover() ?? return }
fn fill(locator: BrowserLocator) =[FS]=> Unit { locator.fill("value") ?? return }
fn press(locator: BrowserLocator) =[FS]=> Unit { locator.press("Enter") ?? return }
fn remove(intercept: BrowserIntercept) =[FS]=> Unit { intercept.remove() ?? return }
fn send(protocol: BrowserProtocol) =[FS]=> Unit { protocol.send("session.status", "{{}}") ?? return }
fn run() {}
"#;
    let diags = jet::compile(source).expect_err("Browser I/O methods must infer Net");
    assert!(
        diags.iter().filter(|diag| diag.code == "E0740").count() >= 24,
        "Browser connect and every I/O method must violate an FS-only bound: {diags:?}"
    );
}

#[test]
fn native_bidi_cleanup_retries_failures_and_drops_last_owners_once() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!("ws://{}/session?token=LIFECYCLE_SECRET", listener.local_addr().unwrap());
    let server = thread::spawn(move || run_lifecycle(listener));
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(500) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")

    retry_context :: session.context() ?? panic("retry context")
    retry_page :: retry_context.page() ?? panic("retry page")
    retry_locator :: retry_page.get_by_role("button", "Save")
    loop attempt; [1, 2] {
        retry_page.close() ?? next
    }
    loop attempt; [1] {
        retry_locator.click() ?? next
        print("unexpected click")
    }
    loop attempt; [1, 2] {
        retry_context.close() ?? next
    }

    local_context :: session.context() ?? panic("local context")
    local_page :: local_context.page() ?? panic("local page")
    local_context.close() ?? panic("local context close")
    local_page.close() ?? panic("local page close")

    drop_context :: session.context() ?? panic("drop context")
    drop_page :: drop_context.page() ?? panic("drop page")
    session.close() ?? return
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) =
        common::build_and_run("jet_browser_bidi_lifecycle", "browser_bidi_lifecycle", &source);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "");
    assert!(!stdout.contains("SECRET"), "{stdout}");
    assert!(!stderr.contains("SECRET"), "{stderr}");
    assert_eq!(
        server.join().unwrap(),
        [
            "session.status",
            "session.new",
            "browser.createUserContext",
            "browsingContext.create",
            "browsingContext.close",
            "browsingContext.close",
            "browser.removeUserContext",
            "browser.removeUserContext",
            "browser.createUserContext",
            "browsingContext.create",
            "browser.removeUserContext",
            "browser.createUserContext",
            "browsingContext.create",
            "session.end",
            "browsingContext.close",
            "browser.removeUserContext",
            "session.end",
        ]
    );
}

#[test]
fn native_bidi_tab_and_frame_lifecycle_is_explicit_and_idempotent() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!(
        "ws://{}/session?token=TAB_FRAME_SECRET",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || run_tab_frame_lifecycle(listener));
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(500) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    context :: session.context() ?? panic("context")

    tab_one :: context.tab() ?? panic("tab one")
    tab_two :: context.page() ?? panic("tab two")
    main :: tab_one.main_frame() ?? panic("main")
    frames :: tab_one.frames() ?? panic("frames")
    print("frames:{frames.len()}")

    child :: frames[1]
    child.close() ?? panic("child close")
    child.close() ?? panic("child close idempotent")

    tab_two.close() ?? panic("tab two close")
    main.close() ?? panic("main close")
    context.close() ?? panic("context close")
    session.close() ?? panic("session close")
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_tab_frame",
        "browser_bidi_tab_frame",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "frames:3\n");
    assert!(!stdout.contains("SECRET"), "{stdout}");
    assert!(!stderr.contains("SECRET"), "{stderr}");
    assert_eq!(
        server.join().unwrap(),
        [
            "session.status",
            "session.new",
            "browser.createUserContext",
            "browsingContext.create",
            "browsingContext.create",
            "browsingContext.getTree",
            "browsingContext.close",
            "browsingContext.close",
            "browsingContext.close",
            "browser.removeUserContext",
            "session.end",
        ]
    );
}

#[test]
fn native_bidi_frames_on_closed_page_fail_without_get_tree() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!(
        "ws://{}/session?token=CLOSED_FRAME_SECRET",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || run_closed_frames_hostile(listener));
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(500) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    context :: session.context() ?? panic("context")
    page :: context.tab() ?? panic("tab")
    page.close() ?? panic("close")
    loop attempt; [1] {
        page.frames() ?? next
        print("unexpected")
    }
    print("caught")
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_closed_frames",
        "browser_bidi_closed_frames",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "caught\n");
    assert!(!stdout.contains("SECRET"), "{stdout}");
    assert!(!stderr.contains("SECRET"), "{stderr}");
    assert_eq!(
        server.join().unwrap(),
        [
            "session.status",
            "session.new",
            "browser.createUserContext",
            "browsingContext.create",
            "browsingContext.close",
            "browser.removeUserContext",
            "session.end",
        ]
    );
}

#[test]
fn native_bidi_profile_drives_isolated_session_and_redacts_trace() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!("ws://{}/session?token=TOP_SECRET", listener.local_addr().unwrap());
    let server = thread::spawn(move || run_mock(listener));
    let source = include_str!("fixtures/browser_bidi.jet").replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) =
        common::build_and_run("jet_browser_bidi", "browser_bidi", &source);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("caps:true:true:bidi-2025.5"), "{stdout}");
    assert!(stdout.contains("event:log.entryAdded"), "{stdout}");
    assert!(
        stdout.contains(r#"raw:{"message":"ready","ready":true}"#),
        "{stdout}"
    );
    assert!(stdout.contains("cdp:{}"), "{stdout}");
    assert!(stdout.contains("trace:"), "{stdout}");
    assert!(stdout.contains(":true:"), "{stdout}");
    assert!(!stdout.contains("TOP_SECRET"), "{stdout}");
    assert!(!stdout.contains("SECRET PAGE DATA"), "{stdout}");

    assert_eq!(
        server.join().unwrap(),
        [
            "session.status",
            "session.new",
            "browser.createUserContext",
            "browsingContext.create",
            "browsingContext.navigate",
            "browsingContext.locateNodes",
            "browsingContext.locateNodes",
            "input.performActions",
            "session.subscribe",
            "session.status",
            "goog:cdp.sendCommand",
            "browsingContext.close",
            "browser.removeUserContext",
            "session.end",
        ]
    );
}

#[test]
fn native_bidi_rejects_hostile_frames_profiles_and_timeouts_without_leaks() {
    let (malformed, malformed_server) = hostile_listener(HostileReply::Malformed);
    let (mismatched, mismatched_server) = hostile_listener(HostileReply::MismatchedId);
    let (protocol, protocol_server) = hostile_listener(HostileReply::ProtocolError);
    let (timeout, timeout_server) = hostile_listener(HostileReply::Timeout);
    let (no_cdp, no_cdp_server) = hostile_listener(HostileReply::NoCdp);
    let (invalid_number, invalid_number_server) =
        handshake_listener(r#"{"ready":01,"message":"bad"}"#, false);
    let (not_ready, not_ready_server) =
        handshake_listener(r#"{"ready":false,"message":"busy"}"#, false);
    let (surrogate, surrogate_server) =
        handshake_listener(r#"{"ready":true,"message":"\uD83D\uDE80"}"#, true);
    let (closed_protocol, closed_protocol_server) =
        handshake_listener(r#"{"ready":true,"message":"ready"}"#, true);
    let (malformed_session, malformed_session_server) = malformed_session_listener();
    let source = r#"
use core.browser as browser

fn connect_outcome(endpoint: String) => String {
    profile :: browser.profile("bidi-2025.5") ?? return "unexpected-profile"
    timeout :: browser.timeout(250) ?? return "unexpected-timeout"
    session :: browser.connect_profile(endpoint, profile, timeout) ?? return "caught"
    session.close() ?? return "close-error"
    return "unexpected-success"
}

fn profile_outcome() => String {
    profile :: browser.profile("rolling") ?? return "caught"
    return "unexpected-success"
}

fn timeout_outcome() => String {
    timeout :: browser.timeout(0) ?? return "caught"
    return "unexpected-success"
}

fn cdp_outcome(endpoint: String) => String {
    profile :: browser.profile("bidi-2025.5") ?? return "unexpected-profile"
    timeout :: browser.timeout(250) ?? return "unexpected-timeout"
    session :: browser.connect_profile(endpoint, profile, timeout) ?? return "unexpected-connect"
    cdp :: session.protocol("cdp") ?? return "caught"
    return "unexpected-success"
}

fn valid_outcome(endpoint: String) => String {
    profile :: browser.profile("bidi-2025.5") ?? return "unexpected-profile"
    timeout :: browser.timeout(250) ?? return "unexpected-timeout"
    session :: browser.connect_profile(endpoint, profile, timeout) ?? return "caught"
    session.close() ?? return "close-error"
    return "connected"
}

fn closed_protocol_outcome(endpoint: String) => String {
    profile :: browser.profile("bidi-2025.5") ?? return "unexpected-profile"
    timeout :: browser.timeout(250) ?? return "unexpected-timeout"
    session :: browser.connect_profile(endpoint, profile, timeout) ?? return "unexpected-connect"
    session.close() ?? return "close-error"
    loop attempt; [1] {
        session.protocol("bidi") ?? next
        return "unexpected-open"
    }
    return "closed"
}

fn run() {
    print(connect_outcome("__MALFORMED__"))
    print(connect_outcome("__MISMATCHED__"))
    print(connect_outcome("__PROTOCOL__"))
    print(connect_outcome("__TIMEOUT__"))
    print(profile_outcome())
    print(timeout_outcome())
    print(cdp_outcome("__NO_CDP__"))
    print(connect_outcome("__INVALID_NUMBER__"))
    print(connect_outcome("__NOT_READY__"))
    print(valid_outcome("__SURROGATE__"))
    print(closed_protocol_outcome("__CLOSED_PROTOCOL__"))
    print(connect_outcome("__MALFORMED_SESSION__"))
}
"#
    .replace("__MALFORMED__", &malformed)
    .replace("__MISMATCHED__", &mismatched)
    .replace("__PROTOCOL__", &protocol)
    .replace("__TIMEOUT__", &timeout)
    .replace("__NO_CDP__", &no_cdp)
    .replace("__INVALID_NUMBER__", &invalid_number)
    .replace("__NOT_READY__", &not_ready)
    .replace("__SURROGATE__", &surrogate)
    .replace("__CLOSED_PROTOCOL__", &closed_protocol)
    .replace("__MALFORMED_SESSION__", &malformed_session);

    let (code, stdout, stderr) =
        common::build_and_run("jet_browser_bidi_hostile", "browser_bidi_hostile", &source);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "caught\ncaught\ncaught\ncaught\ncaught\ncaught\ncaught\ncaught\ncaught\nconnected\nclosed\ncaught\n"
    );
    assert!(!stdout.contains("SECRET"), "{stdout}");
    assert!(!stderr.contains("SECRET"), "{stderr}");

    malformed_server.join().unwrap();
    mismatched_server.join().unwrap();
    protocol_server.join().unwrap();
    timeout_server.join().unwrap();
    no_cdp_server.join().unwrap();
    assert_eq!(invalid_number_server.join().unwrap(), ["session.status"]);
    assert_eq!(not_ready_server.join().unwrap(), ["session.status"]);
    assert_eq!(
        surrogate_server.join().unwrap(),
        ["session.status", "session.new", "session.end"]
    );
    assert_eq!(
        closed_protocol_server.join().unwrap(),
        ["session.status", "session.new", "session.end"]
    );
    assert_eq!(
        malformed_session_server.join().unwrap(),
        ["session.status", "session.new", "session.end"]
    );
}

#[test]
fn native_bidi_bounds_event_queue_and_hashes_large_remote_method_facts() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!("ws://{}/session", listener.local_addr().unwrap());
    let server = thread::spawn(move || run_event_storm(listener));
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(500) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    bidi :: session.protocol("bidi") ?? panic("protocol")
    bidi.send("session.status", "{{}}") ?? panic("storm")
    first :: session.next_event(timeout) ?? panic("queued event")
    print(first.kind() == "event.44")
    trace :: session.trace()
    print("{trace.redacted()}:{trace.entry_count()}")
    print(trace.summary())
    session.close() ?? panic("close")
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) =
        common::build_and_run("jet_browser_bidi_storm", "browser_bidi_storm", &source);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some("true"), "{stdout}");
    let summary = lines.next().expect("trace facts");
    assert!(summary.starts_with("true:"), "{stdout}");
    let count: usize = summary.trim_start_matches("true:").parse().unwrap();
    assert!(count < 310, "trace must be byte bounded: {stdout}");
    assert!(!stdout.contains("SECRET_REMOTE_METHOD"), "{stdout}");
    assert!(!stdout.contains(&"x".repeat(128)), "{stdout}");
    assert_eq!(
        server.join().unwrap(),
        ["session.status", "session.new", "session.status", "session.end"]
    );
}

#[test]
fn native_bidi_command_deadline_does_not_reset_on_continuous_events() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!("ws://{}/session", listener.local_addr().unwrap());
    let server = thread::spawn(move || run_continuous_events(listener));
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(200) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    bidi :: session.protocol("bidi") ?? panic("protocol")
    outcome :: bidi.send("session.status", "{{}}") ?? "caught"
    print(outcome)
}
"#
    .replace("__ENDPOINT__", &endpoint);
    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_deadline",
        "browser_bidi_deadline",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "caught\n");
    assert!(!stderr.contains("network.tick"), "{stderr}");
    assert_eq!(
        server.join().unwrap(),
        ["session.status", "session.new", "session.status", "session.end"]
    );
}

#[test]
fn native_bidi_runs_real_network_path_in_forced_and_default_dev_tier_zero() {
    fn run_once(use_interpreter: bool) -> (String, Vec<String>, Vec<jet_jit::TierRow>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = format!("ws://{}/session", listener.local_addr().unwrap());
        let server = thread::spawn(move || run_smoke(listener));
        let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2024.11") ?? panic("profile")
    timeout :: browser.timeout(500) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    print(session.capabilities().profile())
    bidi :: session.protocol("bidi") ?? panic("protocol")
    blocked :: bidi.send("webExtension.install", "{{}}") ?? "blocked"
    print(blocked)
    unknown :: bidi.send("network.futureCommand", "{{}}") ?? "blocked"
    print(unknown)
    raw_cdp :: bidi.send("goog:cdp.sendCommand", "{{}}") ?? "blocked"
    print(raw_cdp)
    session.close() ?? panic("close")
}
"#
        .replace("__ENDPOINT__", &endpoint);
        let dir = common::unique_tmp("jet_browser_bidi_dev");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("browser_bidi_dev.jet");
        fs::write(&path, source).unwrap();

        jet_jit::reset_jit_trace_for_test();
        jet_jit::set_trace_tiers(true);
        let outcome =
            jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, use_interpreter);
        jet_jit::set_trace_tiers(false);
        let trace = jet_jit::take_last_trace();
        let stdout = match outcome {
            jet::Interpreter::RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(exit_code, 0, "dev stderr:\n{stderr}");
                assert_eq!(stderr, "");
                stdout
            }
            jet::Interpreter::RunOutcome::Problems(diags) => {
                panic!("Browser dev tier rejected real network program: {diags:?}")
            }
        };
        (stdout, server.join().unwrap(), trace)
    }

    fn dev_stdout(source: String, label: &str) -> String {
        let dir = common::unique_tmp(label);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("browser_bidi_dev.jet");
        fs::write(&path, source).unwrap();
        match jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, false) {
            jet::Interpreter::RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(exit_code, 0, "dev stderr:\n{stderr}");
                assert_eq!(stderr, "");
                stdout
            }
            jet::Interpreter::RunOutcome::Problems(diags) => {
                panic!("Browser dev tier rejected real network program: {diags:?}")
            }
        }
    }

    let (forced_stdout, forced_methods, _) = run_once(true);
    assert_eq!(
        forced_stdout,
        "bidi-2024.11\nblocked\nblocked\nblocked\n"
    );
    assert_eq!(
        forced_methods,
        ["session.status", "session.new", "session.end"]
    );

    let (default_stdout, default_methods, trace) = run_once(false);
    assert_eq!(default_stdout, forced_stdout);
    assert_eq!(default_methods, forced_methods);
    assert!(
        trace
            .iter()
            .any(|row| row.function == "run" && row.tier == jet_jit::Tier::Interp),
        "Browser must visibly select/deopt to tier-0, never AOT: {trace:?}"
    );
    assert!(
        !jet_jit::fallback_invoked_for_test(),
        "default dev must not hide Browser behind the legacy/AOT fallback"
    );

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!("ws://{}/session", listener.local_addr().unwrap());
    let server = thread::spawn(move || run_profile_smoke(listener));
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(500) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    bidi :: session.protocol("bidi") ?? panic("protocol")
    print(bidi.send("webExtension.install", "{{}}") ?? "failed")
    print(bidi.send("network.futureCommand", "{{}}") ?? "blocked")
    print(bidi.send("goog:cdp.sendCommand", "{{}}") ?? "blocked")
    session.close() ?? panic("close")
}
"#
    .replace("__ENDPOINT__", &endpoint);
    assert_eq!(
        dev_stdout(source, "jet_browser_bidi_profile_2025"),
        "{}\nblocked\nblocked\n"
    );
    assert_eq!(
        server.join().unwrap(),
        [
            "session.status",
            "session.new",
            "webExtension.install",
            "session.end"
        ]
    );

    for iteration in 0..2 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = format!("ws://{}/session", listener.local_addr().unwrap());
        let server = thread::spawn(move || run_smoke(listener));
        let source = r#"
use core.browser as browser

fn run() {
    session :: browser.connect("__ENDPOINT__") ?? panic("connect")
    print(session.capabilities().profile())
}
"#
        .replace("__ENDPOINT__", &endpoint);
        assert_eq!(
            dev_stdout(source, &format!("jet_browser_bidi_guard_{iteration}")),
            "bidi-2025.5\n"
        );
        assert_eq!(
            server.join().unwrap(),
            ["session.status", "session.new", "session.end"],
            "runtime entry guard must drain Browser owners on iteration {iteration}"
        );
    }

    let (invalid, invalid_server) =
        handshake_listener(r#"{"ready":01,"message":"bad"}"#, false);
    let (surrogate, surrogate_server) =
        handshake_listener(r#"{"ready":true,"message":"\uD83D\uDE80"}"#, true);
    let source = r#"
use core.browser as browser

fn outcome(endpoint: String) => String {
    session :: browser.connect(endpoint) ?? return "caught"
    session.close() ?? return "close-error"
    return "connected"
}

fn run() {
    print(outcome("__INVALID__"))
    print(outcome("__SURROGATE__"))
}
"#
    .replace("__INVALID__", &invalid)
    .replace("__SURROGATE__", &surrogate);
    assert_eq!(
        dev_stdout(source, "jet_browser_bidi_json_codec"),
        "caught\nconnected\n"
    );
    assert_eq!(invalid_server.join().unwrap(), ["session.status"]);
    assert_eq!(
        surrogate_server.join().unwrap(),
        ["session.status", "session.new", "session.end"]
    );
}

fn run_locator_actions(listener: TcpListener) -> Vec<String> {
    let (mut stream, _) = listener.accept().unwrap();
    accept_websocket(&mut stream);
    let mut methods = Vec::new();
    let mut locate_calls = 0usize;
    // Handshake + context/page + navigate + locator ops + cleanup.
    for _ in 0..24 {
        let request = read_text_frame(&mut stream);
        let id = field(&request, "id");
        let method = field(&request, "method");
        methods.push(method.clone());
        let result = match method.as_str() {
            "session.status" => r#"{"ready":true,"message":"ready"}"#,
            "session.new" => {
                r#"{"sessionId":"mock-session","capabilities":{"browserName":"mock","browserVersion":"1"}}"#
            }
            "browser.createUserContext" => r#"{"userContext":"user-1"}"#,
            "browsingContext.create" => r#"{"context":"page-1"}"#,
            "browsingContext.navigate" => {
                r#"{"url":"https://example.test/form","navigation":"nav-1"}"#
            }
            "browsingContext.locateNodes" => {
                locate_calls += 1;
                // First waits/actions see the node; wait_gone then sees absence.
                if locate_calls <= 10 {
                    r#"{"nodes":[{"type":"node","sharedId":"node-1"}]}"#
                } else {
                    r#"{"nodes":[]}"#
                }
            }
            "input.performActions" | "script.callFunction" | "browsingContext.close"
            | "browser.removeUserContext" | "session.end" => "{}",
            other => panic!("unexpected BiDi method {other}: {request}"),
        };
        write_text_frame(
            &mut stream,
            &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
        );
        if method == "session.end" {
            break;
        }
    }
    methods
}

#[test]
fn native_bidi_semantic_locators_actions_and_waits() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!(
        "ws://{}/session?token=LOCATOR_SECRET",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || run_locator_actions(listener));
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(500) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    context :: session.context() ?? panic("context")
    page :: context.page() ?? panic("page")
    page.goto("https://example.test/form") ?? panic("goto")

    save :: page.get_by_role("button", "Save")
    save.wait(timeout) ?? panic("wait")
    save.hover() ?? panic("hover")
    save.click() ?? panic("click")

    name :: page.get_by_label("Name")
    name.fill("Ada") ?? panic("fill")
    name.press("Tab") ?? panic("press")

    page.get_by_text("Save").wait(timeout) ?? panic("text wait")
    page.get_by_placeholder("email").wait(timeout) ?? panic("placeholder wait")
    page.get_by_test_id("save").wait(timeout) ?? panic("test id wait")
    page.get_by_css("button.save").wait(timeout) ?? panic("css wait")

    gone :: page.get_by_role("status", "Saving")
    gone.wait_gone(timeout) ?? panic("wait_gone")

    print("locators:ok")
    page.close() ?? panic("page close")
    context.close() ?? panic("context close")
    session.close() ?? panic("session close")
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_locators",
        "browser_bidi_locators",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "locators:ok\n");
    assert!(!stdout.contains("SECRET"), "{stdout}");
    assert!(!stderr.contains("SECRET"), "{stderr}");
    let methods = server.join().unwrap();
    assert!(
        methods.iter().filter(|m| *m == "browsingContext.locateNodes").count() >= 11,
        "expected repeated locateNodes for waits/actions: {methods:?}"
    );
    assert!(
        methods.iter().any(|m| m == "script.callFunction"),
        "fill must use script.callFunction: {methods:?}"
    );
    assert!(
        methods.iter().filter(|m| *m == "input.performActions").count() >= 3,
        "hover/click/press need performActions: {methods:?}"
    );
    assert_eq!(methods.last().map(String::as_str), Some("session.end"));
}

#[test]
fn native_bidi_missing_locator_action_times_out_without_leaks() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!(
        "ws://{}/session?token=MISSING_LOCATOR_SECRET",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        accept_websocket(&mut stream);
        let mut methods = Vec::new();
        for _ in 0..12 {
            let request = read_text_frame(&mut stream);
            let id = field(&request, "id");
            let method = field(&request, "method");
            methods.push(method.clone());
            let result = match method.as_str() {
                "session.status" => r#"{"ready":true,"message":"ready"}"#,
                "session.new" => {
                    r#"{"sessionId":"mock-session","capabilities":{"browserName":"mock","browserVersion":"1"}}"#
                }
                "browser.createUserContext" => r#"{"userContext":"user-1"}"#,
                "browsingContext.create" => r#"{"context":"page-1"}"#,
                "browsingContext.locateNodes" => r#"{"nodes":[]}"#,
                "browsingContext.close" | "browser.removeUserContext" | "session.end" => "{}",
                other => panic!("unexpected BiDi method {other}: {request}"),
            };
            write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
            );
            if method == "session.end" {
                break;
            }
        }
        methods
    });
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(80) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    context :: session.context() ?? panic("context")
    page :: context.page() ?? panic("page")
    missing :: page.get_by_role("button", "Missing")
    loop attempt; [1] {
        missing.click() ?? next
        print("unexpected")
    }
    print("caught")
    page.close() ?? panic("page close")
    context.close() ?? panic("context close")
    session.close() ?? panic("session close")
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_missing_locator",
        "browser_bidi_missing_locator",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "caught\n");
    assert!(!stdout.contains("SECRET"), "{stdout}");
    assert!(!stderr.contains("SECRET"), "{stderr}");
    let methods = server.join().unwrap();
    assert!(
        methods.iter().any(|m| m == "browsingContext.locateNodes"),
        "{methods:?}"
    );
    assert!(
        !methods.iter().any(|m| m == "input.performActions"),
        "missing locator must not click: {methods:?}"
    );
}

fn run_network_events(listener: TcpListener) -> Vec<String> {
    let (mut stream, _) = listener.accept().unwrap();
    accept_websocket(&mut stream);
    let mut methods = Vec::new();
    let mut add_intercepts = 0usize;
    for _ in 0..20 {
        let request = read_text_frame(&mut stream);
        let id = field(&request, "id");
        let method = field(&request, "method");
        methods.push(method.clone());
        match method.as_str() {
            "session.status" => write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{{"ready":true,"message":"ready"}}}}"#),
            ),
            "session.new" => write_text_frame(
                &mut stream,
                &format!(
                    r#"{{"type":"success","id":{id},"result":{{"sessionId":"mock-session","capabilities":{{"browserName":"mock","browserVersion":"1"}}}}}}"#
                ),
            ),
            "session.subscribe" => write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{{}}}}"#),
            ),
            "network.addIntercept" => {
                add_intercepts += 1;
                write_text_frame(
                    &mut stream,
                    &format!(
                        r#"{{"type":"success","id":{id},"result":{{"intercept":"intercept-{add_intercepts}"}}}}"#
                    ),
                );
                if add_intercepts == 1 {
                    // Deliver a blocked request event with a secret URL that must stay redacted.
                    write_text_frame(
                        &mut stream,
                        r#"{"type":"event","method":"network.beforeRequestSent","params":{"isBlocked":true,"request":{"request":"req-secret-1","method":"GET","url":"https://leak.example/TOP_SECRET_TOKEN"},"response":null}}"#,
                    );
                }
            }
            "network.continueRequest" | "network.failRequest" | "network.provideResponse"
            | "network.removeIntercept" | "session.end" => write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{{}}}}"#),
            ),
            other => panic!("unexpected BiDi method {other}: {request}"),
        }
        if method == "session.end" {
            break;
        }
    }
    methods
}

#[test]
fn native_bidi_network_events_inspection_and_interception() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!(
        "ws://{}/session?token=NETWORK_SECRET",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || run_network_events(listener));
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(500) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    session.subscribe("network.beforeRequestSent") ?? panic("subscribe")

    intercept :: session.add_intercept("beforeRequestSent") ?? panic("add_intercept")
    event :: session.next_event(timeout) ?? panic("next_event")
    print(event.kind())
    print(event.request_id())
    print(event.request_method())
    print(event.is_blocked())
    hash :: event.url_hash()
    print(hash.len() == 16)
    print(hash.contains("SECRET") == false)

    session.continue_request(event.request_id()) ?? panic("continue")
    session.fail_request("req-secret-1") ?? panic("fail")
    session.fulfill_request("req-secret-1", 200, "ok") ?? panic("fulfill")

    url_intercept :: session.add_intercept_url("beforeRequestSent", "https://api.test/*")
        ?? panic("add_intercept_url")
    url_intercept.remove() ?? panic("remove url")
    intercept.remove() ?? panic("remove")
    intercept.remove() ?? panic("remove idempotent")

    print("network:ok")
    session.close() ?? panic("session close")
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_network",
        "browser_bidi_network",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("network.beforeRequestSent\n"), "{stdout}");
    assert!(stdout.contains("req-secret-1\n"), "{stdout}");
    assert!(stdout.contains("GET\n"), "{stdout}");
    assert!(stdout.contains("true\n"), "{stdout}");
    assert!(stdout.contains("network:ok\n"), "{stdout}");
    assert!(!stdout.contains("TOP_SECRET"), "{stdout}");
    assert!(!stdout.contains("NETWORK_SECRET"), "{stdout}");
    assert!(!stderr.contains("TOP_SECRET"), "{stderr}");
    assert!(!stderr.contains("NETWORK_SECRET"), "{stderr}");
    let methods = server.join().unwrap();
    assert!(
        methods.iter().any(|m| m == "session.subscribe"),
        "{methods:?}"
    );
    assert_eq!(
        methods.iter().filter(|m| *m == "network.addIntercept").count(),
        2,
        "{methods:?}"
    );
    assert!(
        methods.iter().any(|m| m == "network.continueRequest"),
        "{methods:?}"
    );
    assert!(
        methods.iter().any(|m| m == "network.failRequest"),
        "{methods:?}"
    );
    assert!(
        methods.iter().any(|m| m == "network.provideResponse"),
        "{methods:?}"
    );
    assert_eq!(
        methods.iter().filter(|m| *m == "network.removeIntercept").count(),
        2,
        "second remove must be local-idempotent: {methods:?}"
    );
    assert_eq!(methods.last().map(String::as_str), Some("session.end"));
}

#[test]
fn native_bidi_network_intercept_rejects_bad_phase_and_empty_request() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!(
        "ws://{}/session?token=HOSTILE_NETWORK_SECRET",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        accept_websocket(&mut stream);
        let mut methods = Vec::new();
        for _ in 0..10 {
            let request = read_text_frame(&mut stream);
            let id = field(&request, "id");
            let method = field(&request, "method");
            methods.push(method.clone());
            let result = match method.as_str() {
                "session.status" => r#"{"ready":true,"message":"ready"}"#,
                "session.new" => {
                    r#"{"sessionId":"mock-session","capabilities":{"browserName":"mock","browserVersion":"1"}}"#
                }
                "session.end" => "{}",
                other => panic!("unexpected BiDi method {other}: {request}"),
            };
            write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
            );
            if method == "session.end" {
                break;
            }
        }
        methods
    });
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(200) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")

    loop attempt; [1] {
        session.add_intercept("not-a-phase") ?? next
        print("unexpected phase")
    }
    loop attempt; [1] {
        session.continue_request("") ?? next
        print("unexpected empty")
    }
    loop attempt; [1] {
        session.fulfill_request("req", 99, "nope") ?? next
        print("unexpected status")
    }
    print("caught")
    session.close() ?? panic("close")
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_network_hostile",
        "browser_bidi_network_hostile",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "caught\n");
    assert!(!stdout.contains("SECRET"), "{stdout}");
    assert!(!stderr.contains("SECRET"), "{stderr}");
    let methods = server.join().unwrap();
    assert!(
        !methods.iter().any(|m| m.starts_with("network.")),
        "invalid client calls must not hit the wire: {methods:?}"
    );
}

#[test]
fn native_bidi_network_response_event_exposes_status_without_payload_leak() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!(
        "ws://{}/session?token=RESPONSE_SECRET",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        accept_websocket(&mut stream);
        let mut methods = Vec::new();
        for _ in 0..12 {
            let request = read_text_frame(&mut stream);
            let id = field(&request, "id");
            let method = field(&request, "method");
            methods.push(method.clone());
            match method.as_str() {
                "session.status" => write_text_frame(
                    &mut stream,
                    &format!(
                        r#"{{"type":"success","id":{id},"result":{{"ready":true,"message":"ready"}}}}"#
                    ),
                ),
                "session.new" => write_text_frame(
                    &mut stream,
                    &format!(
                        r#"{{"type":"success","id":{id},"result":{{"sessionId":"mock-session","capabilities":{{"browserName":"mock","browserVersion":"1"}}}}}}"#
                    ),
                ),
                "session.subscribe" => {
                    write_text_frame(
                        &mut stream,
                        &format!(r#"{{"type":"success","id":{id},"result":{{}}}}"#),
                    );
                    write_text_frame(
                        &mut stream,
                        r#"{"type":"event","method":"network.responseCompleted","params":{"isBlocked":false,"request":{"request":"req-2","method":"POST","url":"https://api.example/SECRET_PATH"},"response":{"status":201,"headers":[{"name":"set-cookie","value":"session=SECRET"}]}}}"#,
                    );
                }
                "session.end" => write_text_frame(
                    &mut stream,
                    &format!(r#"{{"type":"success","id":{id},"result":{{}}}}"#),
                ),
                other => panic!("unexpected BiDi method {other}: {request}"),
            }
            if method == "session.end" {
                break;
            }
        }
        methods
    });
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(500) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    session.subscribe("network.responseCompleted") ?? panic("subscribe")
    event :: session.next_event(timeout) ?? panic("next_event")
    print(event.kind())
    print(event.request_method())
    print(event.status_code())
    print(event.is_blocked())
    print(event.url_hash().contains("SECRET") == false)
    trace :: session.trace()
    print(trace.redacted())
    print(trace.summary().contains("SECRET") == false)
    print("response:ok")
    session.close() ?? panic("close")
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_network_response",
        "browser_bidi_network_response",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("network.responseCompleted\n"), "{stdout}");
    assert!(stdout.contains("POST\n"), "{stdout}");
    assert!(stdout.contains("201\n"), "{stdout}");
    assert!(stdout.contains("response:ok\n"), "{stdout}");
    assert!(!stdout.contains("SECRET"), "{stdout}");
    assert!(!stderr.contains("SECRET"), "{stderr}");
    let methods = server.join().unwrap();
    assert_eq!(methods.last().map(String::as_str), Some("session.end"));
}

fn run_artifacts(listener: TcpListener) -> Vec<String> {
    let (mut stream, _) = listener.accept().unwrap();
    accept_websocket(&mut stream);
    let mut methods = Vec::new();
    let mut locate_calls = 0usize;
    for _ in 0..40 {
        let request = read_text_frame(&mut stream);
        let id = field(&request, "id");
        let method = field(&request, "method");
        methods.push(method.clone());
        match method.as_str() {
            "session.status" => write_text_frame(
                &mut stream,
                &format!(
                    r#"{{"type":"success","id":{id},"result":{{"ready":true,"message":"ready"}}}}"#
                ),
            ),
            "session.new" => write_text_frame(
                &mut stream,
                &format!(
                    r#"{{"type":"success","id":{id},"result":{{"sessionId":"mock-session","capabilities":{{"browserName":"mock","browserVersion":"1"}}}}}}"#
                ),
            ),
            "browser.createUserContext" => write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{{"userContext":"user-1"}}}}"#),
            ),
            "browsingContext.create" => write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{{"context":"page-1"}}}}"#),
            ),
            "browsingContext.navigate" => write_text_frame(
                &mut stream,
                &format!(
                    r#"{{"type":"success","id":{id},"result":{{"url":"https://example.test/app","navigation":"nav-1"}}}}"#
                ),
            ),
            "storage.setCookie" | "storage.deleteCookies" | "browser.setDownloadBehavior"
            | "browsingContext.close" | "browser.removeUserContext" | "session.end"
            | "input.setFiles" => write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{{}}}}"#),
            ),
            "storage.getCookies" => write_text_frame(
                &mut stream,
                &format!(
                    r#"{{"type":"success","id":{id},"result":{{"cookies":[{{"name":"session","value":{{"type":"string","value":"cookie-SECRET"}},"domain":"example.test","path":"/"}}],"partitionKey":{{"userContext":"user-1"}}}}}}"#
                ),
            ),
            "script.callFunction" => {
                // storage_get returns a string; set/clear return undefined-shaped success.
                let result = if request.contains("getItem") {
                    r#"{"result":{"type":"string","value":"stored-SECRET"}}"#
                } else {
                    r#"{"result":{"type":"undefined"}}"#
                };
                write_text_frame(
                    &mut stream,
                    &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
                );
            }
            "browsingContext.captureScreenshot" => write_text_frame(
                &mut stream,
                &format!(
                    r#"{{"type":"success","id":{id},"result":{{"data":"iVBORw0KGgo="}}}}"#
                ),
            ),
            "browsingContext.print" => write_text_frame(
                &mut stream,
                &format!(
                    r#"{{"type":"success","id":{id},"result":{{"data":"JVBERi0xLjQ="}}}}"#
                ),
            ),
            "browsingContext.locateNodes" => {
                locate_calls += 1;
                write_text_frame(
                    &mut stream,
                    &format!(
                        r#"{{"type":"success","id":{id},"result":{{"nodes":[{{"type":"node","sharedId":"file-node-{locate_calls}"}}]}}}}"#
                    ),
                );
            }
            "session.subscribe" => {
                write_text_frame(
                    &mut stream,
                    &format!(r#"{{"type":"success","id":{id},"result":{{}}}}"#),
                );
                write_text_frame(
                    &mut stream,
                    r#"{"type":"event","method":"browsingContext.downloadWillBegin","params":{"context":"page-1","navigation":"nav-dl","timestamp":1.0,"url":"https://cdn.example/SECRET_REPORT.pdf","download":"dl-1","suggestedFilename":"SECRET_REPORT.pdf"}}"#,
                );
            }
            other => panic!("unexpected BiDi method {other}: {request}"),
        }
        if method == "session.end" {
            break;
        }
    }
    methods
}

#[test]
fn native_bidi_artifacts_storage_files_screenshots_and_pdf() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!(
        "ws://{}/session?token=ARTIFACT_SECRET",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || run_artifacts(listener));
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(500) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    session.allow_downloads("/tmp/jet-downloads") ?? panic("allow_downloads")
    context :: session.context() ?? panic("context")
    page :: context.page() ?? panic("page")
    page.goto("https://example.test/app") ?? panic("goto")

    page.set_cookie("session", "cookie-SECRET", "example.test") ?? panic("set_cookie")
    maybe_cookie :: page.cookie("session") ?? panic("cookie")
    cookie :: maybe_cookie ?? panic("missing cookie")
    print(cookie == "cookie-SECRET")
    page.clear_cookies() ?? panic("clear_cookies")

    page.storage_set("local", "token", "stored-SECRET") ?? panic("storage_set")
    maybe_stored :: page.storage_get("local", "token") ?? panic("storage_get")
    stored :: maybe_stored ?? panic("missing storage")
    print(stored == "stored-SECRET")
    page.storage_clear("local") ?? panic("storage_clear")
    page.storage_set("session", "draft", "x") ?? panic("session set")
    page.storage_clear("session") ?? panic("session clear")

    upload :: page.get_by_css("input[type=file]")
    upload.set_files("/tmp/upload-SECRET.txt") ?? panic("set_files")

    png :: page.screenshot() ?? panic("screenshot")
    print(png.len() > 0)
    pdf :: page.pdf() ?? panic("pdf")
    print(pdf.len() > 0)

    session.subscribe("browsingContext.downloadWillBegin") ?? panic("subscribe")
    event :: session.next_event(timeout) ?? panic("next_event")
    print(event.kind())
    print(event.download_id())
    hash :: event.suggested_filename_hash()
    print(hash.len() == 16)
    print(hash.contains("SECRET") == false)
    print(event.url_hash().contains("SECRET") == false)

    trace :: session.trace()
    print(trace.redacted())
    print(trace.summary().contains("SECRET") == false)
    print("artifacts:ok")
    page.close() ?? panic("page close")
    context.close() ?? panic("context close")
    session.close() ?? panic("session close")
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_artifacts",
        "browser_bidi_artifacts",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("true\n"), "{stdout}");
    assert!(stdout.contains("browsingContext.downloadWillBegin\n"), "{stdout}");
    assert!(stdout.contains("dl-1\n"), "{stdout}");
    assert!(stdout.contains("artifacts:ok\n"), "{stdout}");
    assert!(!stdout.contains("ARTIFACT_SECRET"), "{stdout}");
    assert!(!stdout.contains("SECRET_REPORT"), "{stdout}");
    assert!(!stderr.contains("ARTIFACT_SECRET"), "{stderr}");
    assert!(!stderr.contains("SECRET_REPORT"), "{stderr}");
    let methods = server.join().unwrap();
    for expected in [
        "browser.setDownloadBehavior",
        "storage.setCookie",
        "storage.getCookies",
        "storage.deleteCookies",
        "script.callFunction",
        "input.setFiles",
        "browsingContext.captureScreenshot",
        "browsingContext.print",
        "session.subscribe",
    ] {
        assert!(
            methods.iter().any(|m| m == expected),
            "missing {expected}: {methods:?}"
        );
    }
    assert_eq!(methods.last().map(String::as_str), Some("session.end"));
}

#[test]
fn native_bidi_artifacts_reject_hostile_inputs_without_wire() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!(
        "ws://{}/session?token=HOSTILE_ARTIFACT_SECRET",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        accept_websocket(&mut stream);
        let mut methods = Vec::new();
        for _ in 0..16 {
            let request = read_text_frame(&mut stream);
            let id = field(&request, "id");
            let method = field(&request, "method");
            methods.push(method.clone());
            let result = match method.as_str() {
                "session.status" => r#"{"ready":true,"message":"ready"}"#,
                "session.new" => {
                    r#"{"sessionId":"mock-session","capabilities":{"browserName":"mock","browserVersion":"1"}}"#
                }
                "browser.createUserContext" => r#"{"userContext":"user-1"}"#,
                "browsingContext.create" => r#"{"context":"page-1"}"#,
                "browsingContext.close" | "browser.removeUserContext" | "session.end" => "{}",
                other => panic!("unexpected BiDi method {other}: {request}"),
            };
            write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
            );
            if method == "session.end" {
                break;
            }
        }
        methods
    });
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(200) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    context :: session.context() ?? panic("context")
    page :: context.page() ?? panic("page")

    loop attempt; [1] {
        session.allow_downloads("") ?? next
        print("unexpected empty folder")
    }
    loop attempt; [1] {
        page.set_cookie("", "v", "example.test") ?? next
        print("unexpected empty cookie")
    }
    loop attempt; [1] {
        value :: page.cookie("") ?? next
        print("unexpected empty cookie name {value}")
    }
    loop attempt; [1] {
        value :: page.storage_get("memory", "k") ?? next
        print("unexpected storage kind {value}")
    }
    loop attempt; [1] {
        page.storage_set("local", "", "v") ?? next
        print("unexpected empty key")
    }
    loop attempt; [1] {
        page.get_by_css("input").set_files("") ?? next
        print("unexpected empty path")
    }
    print("caught")
    page.close() ?? panic("page close")
    context.close() ?? panic("context close")
    session.close() ?? panic("session close")
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_artifacts_hostile",
        "browser_bidi_artifacts_hostile",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "caught\n");
    assert!(!stdout.contains("SECRET"), "{stdout}");
    assert!(!stderr.contains("SECRET"), "{stderr}");
    let methods = server.join().unwrap();
    assert!(
        !methods.iter().any(|m| {
            m.starts_with("storage.")
                || m.starts_with("input.")
                || *m == "browser.setDownloadBehavior"
                || *m == "browsingContext.captureScreenshot"
                || *m == "browsingContext.print"
                || *m == "script.callFunction"
        }),
        "invalid client calls must not hit the wire: {methods:?}"
    );
}

#[test]
fn native_bidi_closed_page_rejects_artifacts() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!(
        "ws://{}/session?token=CLOSED_ARTIFACT_SECRET",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        accept_websocket(&mut stream);
        let mut methods = Vec::new();
        for _ in 0..12 {
            let request = read_text_frame(&mut stream);
            let id = field(&request, "id");
            let method = field(&request, "method");
            methods.push(method.clone());
            let result = match method.as_str() {
                "session.status" => r#"{"ready":true,"message":"ready"}"#,
                "session.new" => {
                    r#"{"sessionId":"mock-session","capabilities":{"browserName":"mock","browserVersion":"1"}}"#
                }
                "browser.createUserContext" => r#"{"userContext":"user-1"}"#,
                "browsingContext.create" => r#"{"context":"page-1"}"#,
                "browsingContext.close" | "browser.removeUserContext" | "session.end" => "{}",
                other => panic!("unexpected BiDi method {other}: {request}"),
            };
            write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
            );
            if method == "session.end" {
                break;
            }
        }
        methods
    });
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(200) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    context :: session.context() ?? panic("context")
    page :: context.page() ?? panic("page")
    page.close() ?? panic("page close")
    loop attempt; [1] {
        page.screenshot() ?? next
        print("unexpected screenshot")
    }
    loop attempt; [1] {
        page.pdf() ?? next
        print("unexpected pdf")
    }
    loop attempt; [1] {
        page.clear_cookies() ?? next
        print("unexpected cookies")
    }
    print("caught")
    context.close() ?? panic("context close")
    session.close() ?? panic("session close")
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_artifacts_closed",
        "browser_bidi_artifacts_closed",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "caught\n");
    assert!(!stdout.contains("SECRET"), "{stdout}");
    assert!(!stderr.contains("SECRET"), "{stderr}");
    let methods = server.join().unwrap();
    assert!(
        !methods.iter().any(|m| {
            *m == "browsingContext.captureScreenshot"
                || *m == "browsingContext.print"
                || m.starts_with("storage.")
        }),
        "closed page must not call artifact commands: {methods:?}"
    );
}

/// D-BROWSER-AUTO1=A (#1192): checked expert CDP — success, BiDi smuggle block,
/// shape rejection, capability miss, and redacted hostile CDP wire errors.
#[test]
fn native_bidi_checked_expert_cdp_supplement_success_and_gates() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!(
        "ws://{}/session?token=CDP_SUCCESS_SECRET",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        accept_websocket(&mut stream);
        let mut methods = Vec::new();
        for _ in 0..8 {
            let request = read_text_frame(&mut stream);
            let id = field(&request, "id");
            let method = field(&request, "method");
            methods.push(method.clone());
            let result = match method.as_str() {
                "session.status" => r#"{"ready":true,"message":"ready"}"#,
                "session.new" => {
                    assert!(
                        request.contains(r#""capabilities":{"alwaysMatch":{}}"#),
                        "session.new must not request CDP up front: {request}"
                    );
                    r#"{"sessionId":"cdp-ok","capabilities":{"browserName":"mock","goog:cdp":true}}"#
                }
                "goog:cdp.sendCommand" => {
                    assert!(
                        request.contains(r#""method":"Network.setCacheDisabled""#),
                        "CDP wrap must carry Domain.command: {request}"
                    );
                    assert!(
                        request.contains(r#""params":{"cacheDisabled":true}"#),
                        "CDP wrap must carry params object: {request}"
                    );
                    assert!(
                        !request.contains("CDP_SUCCESS_SECRET"),
                        "endpoint secret must not enter CDP params: {request}"
                    );
                    r#"{"result":{"cacheDisabled":true}}"#
                }
                "session.end" => "{}",
                other => panic!("unexpected CDP success method {other}: {request}"),
            };
            write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
            );
            if method == "session.end" {
                break;
            }
        }
        methods
    });
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(500) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    caps :: session.capabilities()
    print("caps:{caps.bidi()}:{caps.cdp()}")

    bidi :: session.protocol("bidi") ?? panic("bidi")
    smuggle :: bidi.send("goog:cdp.sendCommand", "{{\"method\":\"Network.enable\",\"params\":{{}}}}") ?? "blocked-smuggle"
    print(smuggle)

    cdp :: session.protocol("cdp") ?? panic("cdp")
    bad_shape :: cdp.send("session.status", "{{}}") ?? "blocked-shape"
    print(bad_shape)
    empty :: cdp.send("", "{{}}") ?? "blocked-empty"
    print(empty)
    colon :: cdp.send("goog:cdp.sendCommand", "{{}}") ?? "blocked-colon"
    print(colon)

    ok :: cdp.send("Network.setCacheDisabled", "{{\"cacheDisabled\":true}}") ?? panic("cdp send")
    print("cdp:{ok}")

    trace :: session.trace()
    redacted :: trace.redacted()
    no_method :: trace.summary().contains("Network") == false
    no_secret :: trace.summary().contains("CDP_SUCCESS_SECRET") == false
    print("trace:{redacted}:{no_method}:{no_secret}")
    session.close() ?? panic("close")
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_cdp_checked",
        "browser_bidi_cdp_checked",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "caps:true:true\nblocked-smuggle\nblocked-shape\nblocked-empty\nblocked-colon\ncdp:{\"result\":{\"cacheDisabled\":true}}\ntrace:true:true:true\n"
    );
    assert!(!stdout.contains("CDP_SUCCESS_SECRET"), "{stdout}");
    assert!(!stderr.contains("CDP_SUCCESS_SECRET"), "{stderr}");
    assert_eq!(
        server.join().unwrap(),
        [
            "session.status",
            "session.new",
            "goog:cdp.sendCommand",
            "session.end",
        ]
    );
}

#[test]
fn native_bidi_checked_expert_cdp_rejects_missing_capability_and_hostile_wire() {
    let (no_cdp, no_cdp_server) = hostile_listener(HostileReply::NoCdp);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let hostile_endpoint = format!(
        "ws://{}/session?token=CDP_HOSTILE_SECRET",
        listener.local_addr().unwrap()
    );
    let hostile_server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        accept_websocket(&mut stream);
        let mut methods = Vec::new();
        for _ in 0..4 {
            let request = read_text_frame(&mut stream);
            let id = field(&request, "id");
            let method = field(&request, "method");
            methods.push(method.clone());
            match method.as_str() {
                "session.status" => write_text_frame(
                    &mut stream,
                    &format!(
                        r#"{{"type":"success","id":{id},"result":{{"ready":true,"message":"ready"}}}}"#
                    ),
                ),
                "session.new" => write_text_frame(
                    &mut stream,
                    &format!(
                        r#"{{"type":"success","id":{id},"result":{{"sessionId":"cdp-hostile","capabilities":{{"goog:cdp":true}}}}}}"#
                    ),
                ),
                "goog:cdp.sendCommand" => write_text_frame(
                    &mut stream,
                    &format!(
                        r#"{{"type":"error","id":{id},"error":"unknown error","message":"CDP_HOSTILE_SECRET wire leak","stacktrace":"SECRET STACK"}}"#
                    ),
                ),
                "session.end" => {
                    write_text_frame(
                        &mut stream,
                        &format!(r#"{{"type":"success","id":{id},"result":{{}}}}"#),
                    );
                    break;
                }
                other => panic!("unexpected hostile CDP method {other}: {request}"),
            }
        }
        methods
    });

    let source = r#"
use core.browser as browser

fn missing_cdp(endpoint: String) => String {
    profile :: browser.profile("bidi-2025.5") ?? return "unexpected-profile"
    timeout :: browser.timeout(250) ?? return "unexpected-timeout"
    session :: browser.connect_profile(endpoint, profile, timeout) ?? return "unexpected-connect"
    print("caps:{session.capabilities().cdp()}")
    cdp :: session.protocol("cdp") ?? return "caught-capability"
    return "unexpected-open"
}

fn hostile_cdp(endpoint: String) => String {
    profile :: browser.profile("bidi-2025.5") ?? return "unexpected-profile"
    timeout :: browser.timeout(250) ?? return "unexpected-timeout"
    session :: browser.connect_profile(endpoint, profile, timeout) ?? return "unexpected-connect"
    cdp :: session.protocol("cdp") ?? return "unexpected-protocol"
    cdp.send("Runtime.evaluate", "{{\"expression\":\"1\"}}") ?? return "caught-wire"
    return "unexpected-success"
}

fn run() {
    print(missing_cdp("__NO_CDP__"))
    print(hostile_cdp("__HOSTILE__"))
}
"#
    .replace("__NO_CDP__", &no_cdp)
    .replace("__HOSTILE__", &hostile_endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_cdp_hostile",
        "browser_bidi_cdp_hostile",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "caps:false\ncaught-capability\ncaught-wire\n");
    assert!(!stdout.contains("CDP_HOSTILE_SECRET"), "{stdout}");
    assert!(!stderr.contains("CDP_HOSTILE_SECRET"), "{stderr}");
    assert!(!stdout.contains("SECRET STACK"), "{stdout}");
    assert!(!stderr.contains("SECRET STACK"), "{stderr}");
    no_cdp_server.join().unwrap();
    assert_eq!(
        hostile_server.join().unwrap(),
        [
            "session.status",
            "session.new",
            "goog:cdp.sendCommand",
            "session.end",
        ]
    );
}

#[test]
fn native_bidi_checked_expert_cdp_integrates_with_page_lifecycle() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!("ws://{}/session", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        accept_websocket(&mut stream);
        let mut methods = Vec::new();
        for _ in 0..10 {
            let request = read_text_frame(&mut stream);
            let id = field(&request, "id");
            let method = field(&request, "method");
            methods.push(method.clone());
            let result = match method.as_str() {
                "session.status" => r#"{"ready":true,"message":"ready"}"#,
                "session.new" => {
                    r#"{"sessionId":"cdp-life","capabilities":{"goog:cdp":true}}"#
                }
                "browser.createUserContext" => r#"{"userContext":"user-1"}"#,
                "browsingContext.create" => r#"{"context":"page-1"}"#,
                "goog:cdp.sendCommand" => {
                    assert!(
                        request.contains(r#""method":"Page.enable""#),
                        "page-scoped expert CDP must wrap Page.enable: {request}"
                    );
                    "{}"
                }
                "browsingContext.close" | "browser.removeUserContext" | "session.end" => "{}",
                other => panic!("unexpected CDP lifecycle method {other}: {request}"),
            };
            write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
            );
            if method == "session.end" {
                break;
            }
        }
        methods
    });
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(500) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    context :: session.context() ?? panic("context")
    page :: context.page() ?? panic("page")
    cdp :: session.protocol("cdp") ?? panic("cdp")
    print(cdp.send("Page.enable", "{{}}") ?? panic("send"))
    page.close() ?? panic("page close")
    context.close() ?? panic("context close")
    session.close() ?? panic("session close")
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_cdp_lifecycle",
        "browser_bidi_cdp_lifecycle",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "{}\n");
    assert_eq!(
        server.join().unwrap(),
        [
            "session.status",
            "session.new",
            "browser.createUserContext",
            "browsingContext.create",
            "goog:cdp.sendCommand",
            "browsingContext.close",
            "browser.removeUserContext",
            "session.end",
        ]
    );
}

/// D-BROWSER-AUTO1=A (#1193): privacy defaults, isolated contexts, redacted
/// receipt, and cleanup after close — success path.
#[test]
fn native_bidi_privacy_isolation_receipt_and_cleanup_success() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!(
        "ws://{}/session?token=PRIVACY_SUCCESS_SECRET",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        accept_websocket(&mut stream);
        let mut methods = Vec::new();
        let mut contexts = 0u32;
        for _ in 0..20 {
            let request = read_text_frame(&mut stream);
            let id = field(&request, "id");
            let method = field(&request, "method");
            methods.push(method.clone());
            let result = match method.as_str() {
                "session.status" => r#"{"ready":true,"message":"ready"}"#,
                "session.new" => {
                    r#"{"sessionId":"privacy-ok","capabilities":{"browserName":"mock"}}"#
                }
                "browser.createUserContext" => {
                    contexts += 1;
                    if contexts == 1 {
                        r#"{"userContext":"user-1"}"#
                    } else {
                        r#"{"userContext":"user-2"}"#
                    }
                }
                "browsingContext.create" => {
                    if request.contains(r#""userContext":"user-1""#) {
                        r#"{"context":"page-1"}"#
                    } else {
                        r#"{"context":"page-2"}"#
                    }
                }
                "storage.setCookie" => {
                    assert!(
                        request.contains(r#""value":"SECRET_COOKIE""#),
                        "setCookie carries caller value (not traced): {request}"
                    );
                    assert!(
                        !request.contains("PRIVACY_SUCCESS_SECRET"),
                        "endpoint secret must not enter cookie params: {request}"
                    );
                    "{}"
                }
                "storage.getCookies" => {
                    if request.contains(r#""context":"page-1""#) {
                        r#"{"cookies":[{"name":"session","value":{"type":"string","value":"SECRET_COOKIE"},"domain":"example.test","path":"/"}]}"#
                    } else {
                        r#"{"cookies":[]}"#
                    }
                }
                "browsingContext.close" | "browser.removeUserContext" | "session.end" => "{}",
                other => panic!("unexpected privacy success method {other}: {request}"),
            };
            write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
            );
            if method == "session.end" {
                break;
            }
        }
        methods
    });
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(500) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")

    privacy :: session.privacy()
    print("privacy:{privacy.isolated_profiles()}:{privacy.redact_receipts()}:{privacy.shared_profiles()}")

    left :: session.context() ?? panic("left")
    right :: session.context() ?? panic("right")
    print("isolated:{left.isolated()}:{right.isolated()}")
    left_hash :: left.user_hash()
    right_hash :: right.user_hash()
    distinct :: left_hash != right_hash
    print("hashes:{distinct}:{left_hash.len()}:{right_hash.len()}")

    page_left :: left.page() ?? panic("page left")
    page_right :: right.page() ?? panic("page right")
    page_left.set_cookie("session", "SECRET_COOKIE", "https://example.test") ?? panic("set")
    left_cookie_opt :: page_left.cookie("session") ?? panic("left cookie")
    left_cookie :: left_cookie_opt ?? panic("missing left")
    right_cookie_opt :: page_right.cookie("session") ?? panic("right cookie")
    right_absent :: right_cookie_opt ?? "absent"
    print("cookies:{left_cookie == "SECRET_COOKIE"}:{right_absent == "absent"}")

    page_left.close() ?? panic("close left page")
    page_right.close() ?? panic("close right page")
    left.close() ?? panic("close left")
    right.close() ?? panic("close right")
    session.close() ?? panic("close session")

    receipt :: session.receipt()
    redacted :: receipt.redacted()
    no_secret :: receipt.summary().contains("PRIVACY_SUCCESS_SECRET") == false
    no_cookie :: receipt.summary().contains("SECRET_COOKIE") == false
    no_endpoint :: receipt.summary().contains("ws://") == false
    print("receipt:{redacted}:{receipt.isolated()}:{receipt.cleaned()}:{no_secret}:{no_cookie}:{no_endpoint}")
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_privacy_success",
        "browser_bidi_privacy_success",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "privacy:true:true:false\nisolated:true:true\nhashes:true:16:16\ncookies:true:true\nreceipt:true:true:true:true:true:true\n"
    );
    assert!(!stdout.contains("PRIVACY_SUCCESS_SECRET"), "{stdout}");
    assert!(!stderr.contains("PRIVACY_SUCCESS_SECRET"), "{stderr}");
    assert_eq!(
        server.join().unwrap(),
        [
            "session.status",
            "session.new",
            "browser.createUserContext",
            "browser.createUserContext",
            "browsingContext.create",
            "browsingContext.create",
            "storage.setCookie",
            "storage.getCookies",
            "storage.getCookies",
            "browsingContext.close",
            "browsingContext.close",
            "browser.removeUserContext",
            "browser.removeUserContext",
            "session.end",
        ]
    );
}

/// D-BROWSER-AUTO1=A (#1193): hostile wire errors and closed-session receipt
/// stay redacted — no endpoint/page secret leaks.
#[test]
fn native_bidi_privacy_receipt_hostile_and_closed_paths() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let hostile_endpoint = format!(
        "ws://{}/session?token=PRIVACY_HOSTILE_SECRET",
        listener.local_addr().unwrap()
    );
    let hostile_server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        accept_websocket(&mut stream);
        let mut methods = Vec::new();
        for _ in 0..6 {
            let request = read_text_frame(&mut stream);
            let id = field(&request, "id");
            let method = field(&request, "method");
            methods.push(method.clone());
            match method.as_str() {
                "session.status" => write_text_frame(
                    &mut stream,
                    &format!(
                        r#"{{"type":"success","id":{id},"result":{{"ready":true,"message":"ready"}}}}"#
                    ),
                ),
                "session.new" => write_text_frame(
                    &mut stream,
                    &format!(
                        r#"{{"type":"success","id":{id},"result":{{"sessionId":"privacy-hostile","capabilities":{{}}}}}}"#
                    ),
                ),
                "browser.createUserContext" => write_text_frame(
                    &mut stream,
                    &format!(
                        r#"{{"type":"error","id":{id},"error":"unknown error","message":"PRIVACY_HOSTILE_SECRET page dump","stacktrace":"SECRET STACK"}}"#
                    ),
                ),
                "session.end" => {
                    write_text_frame(
                        &mut stream,
                        &format!(r#"{{"type":"success","id":{id},"result":{{}}}}"#),
                    );
                    break;
                }
                other => panic!("unexpected privacy hostile method {other}: {request}"),
            }
        }
        methods
    });

    let closed = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let closed_endpoint = format!(
        "ws://{}/session?token=PRIVACY_CLOSED_SECRET",
        closed.local_addr().unwrap()
    );
    let closed_server = thread::spawn(move || {
        let (mut stream, _) = closed.accept().unwrap();
        accept_websocket(&mut stream);
        let mut methods = Vec::new();
        for _ in 0..5 {
            let request = read_text_frame(&mut stream);
            let id = field(&request, "id");
            let method = field(&request, "method");
            methods.push(method.clone());
            let result = match method.as_str() {
                "session.status" => r#"{"ready":true,"message":"ready"}"#,
                "session.new" => r#"{"sessionId":"privacy-closed","capabilities":{}}"#,
                "session.end" => "{}",
                other => panic!("unexpected privacy closed method {other}: {request}"),
            };
            write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
            );
            if method == "session.end" {
                break;
            }
        }
        methods
    });

    let source = r#"
use core.browser as browser

fn hostile(endpoint: String) => String {
    profile :: browser.profile("bidi-2025.5") ?? return "unexpected-profile"
    timeout :: browser.timeout(250) ?? return "unexpected-timeout"
    session :: browser.connect_profile(endpoint, profile, timeout) ?? return "unexpected-connect"
    session.context() ?? return "caught-context"
    return "unexpected-success"
}

fn closed_receipt(endpoint: String) => String {
    profile :: browser.profile("bidi-2025.5") ?? return "unexpected-profile"
    timeout :: browser.timeout(250) ?? return "unexpected-timeout"
    session :: browser.connect_profile(endpoint, profile, timeout) ?? return "unexpected-connect"
    session.close() ?? return "unexpected-close"
    loop attempt; [1] {
        session.context() ?? next
        return "unexpected-open"
    }
    receipt :: session.receipt()
    privacy :: session.privacy()
    ok :: receipt.redacted() && receipt.cleaned() && privacy.redact_receipts()
    leak :: receipt.summary().contains("PRIVACY_CLOSED_SECRET")
    if ok && leak == false {
        return "cleaned"
    }
    return "bad-receipt"
}

fn run() {
    print(hostile("__HOSTILE__"))
    print(closed_receipt("__CLOSED__"))
}
"#
    .replace("__HOSTILE__", &hostile_endpoint)
    .replace("__CLOSED__", &closed_endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_privacy_hostile",
        "browser_bidi_privacy_hostile",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "caught-context\ncleaned\n");
    assert!(!stdout.contains("PRIVACY_HOSTILE_SECRET"), "{stdout}");
    assert!(!stderr.contains("PRIVACY_HOSTILE_SECRET"), "{stderr}");
    assert!(!stdout.contains("PRIVACY_CLOSED_SECRET"), "{stdout}");
    assert!(!stderr.contains("PRIVACY_CLOSED_SECRET"), "{stderr}");
    assert!(!stdout.contains("SECRET STACK"), "{stdout}");
    assert!(!stderr.contains("SECRET STACK"), "{stderr}");
    assert_eq!(
        hostile_server.join().unwrap(),
        [
            "session.status",
            "session.new",
            "browser.createUserContext",
            "session.end",
        ]
    );
    assert_eq!(
        closed_server.join().unwrap(),
        ["session.status", "session.new", "session.end"]
    );
}

/// D-BROWSER-AUTO1=A (#1193): privacy/receipt integrate with page lifecycle and
/// leave a cleaned redacted receipt after explicit close.
#[test]
fn native_bidi_privacy_receipt_integrates_with_lifecycle() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!("ws://{}/session", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        accept_websocket(&mut stream);
        let mut methods = Vec::new();
        for _ in 0..10 {
            let request = read_text_frame(&mut stream);
            let id = field(&request, "id");
            let method = field(&request, "method");
            methods.push(method.clone());
            let result = match method.as_str() {
                "session.status" => r#"{"ready":true,"message":"ready"}"#,
                "session.new" => r#"{"sessionId":"privacy-life","capabilities":{}}"#,
                "browser.createUserContext" => r#"{"userContext":"user-life"}"#,
                "browsingContext.create" => r#"{"context":"page-life"}"#,
                "browsingContext.navigate" => "{}",
                "browsingContext.close" | "browser.removeUserContext" | "session.end" => "{}",
                other => panic!("unexpected privacy lifecycle method {other}: {request}"),
            };
            write_text_frame(
                &mut stream,
                &format!(r#"{{"type":"success","id":{id},"result":{result}}}"#),
            );
            if method == "session.end" {
                break;
            }
        }
        methods
    });
    let source = r#"
use core.browser as browser

fn run() {
    profile :: browser.profile("bidi-2025.5") ?? panic("profile")
    timeout :: browser.timeout(500) ?? panic("timeout")
    session :: browser.connect_profile("__ENDPOINT__", profile, timeout) ?? panic("connect")
    context :: session.context() ?? panic("context")
    page :: context.page() ?? panic("page")
    page.goto("https://example.test/app") ?? panic("goto")
    print("hash:{context.user_hash().len()}")
    page.close() ?? panic("page close")
    context.close() ?? panic("context close")
    session.close() ?? panic("session close")
    receipt :: session.receipt()
    print("done:{receipt.cleaned()}:{receipt.redacted()}:{receipt.isolated()}")
}
"#
    .replace("__ENDPOINT__", &endpoint);

    let (code, stdout, stderr) = common::build_and_run(
        "jet_browser_bidi_privacy_lifecycle",
        "browser_bidi_privacy_lifecycle",
        &source,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "hash:16\ndone:true:true:true\n");
    assert_eq!(
        server.join().unwrap(),
        [
            "session.status",
            "session.new",
            "browser.createUserContext",
            "browsingContext.create",
            "browsingContext.navigate",
            "browsingContext.close",
            "browser.removeUserContext",
            "session.end",
        ]
    );
}

