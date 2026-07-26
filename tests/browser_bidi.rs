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
    for _ in 0..14 {
        let request = read_text_frame(&mut stream);
        let id = field(&request, "id");
        let method = field(&request, "method");
        methods.push(method.clone());
        let result = match method.as_str() {
            "session.status" => Some(r#"{"ready":true,"message":"ready"}"#),
            "session.new" => Some(r#"{"sessionId":"lifecycle","capabilities":{}}"#),
            "browser.createUserContext" => Some(if methods.len() < 9 {
                r#"{"userContext":"retry-context"}"#
            } else {
                r#"{"userContext":"drop-context"}"#
            }),
            "browsingContext.create" => Some(if methods.len() < 10 {
                r#"{"context":"retry-page"}"#
            } else {
                r#"{"context":"drop-page"}"#
            }),
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
    protocol :: session.protocol("bidi") ?? return

    session.subscribe(1) ?? return
    session.next_event("soon") ?? return
    session.protocol(1) ?? return
    page.goto(1) ?? return
    wrong_locator :: page.get_by_role(1, 2)
    locator.wait(1) ?? return
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
        "protocol",
        "goto",
        "get_by_role",
        "wait",
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
    let source = r#"
fn context(session: Browser) --[Fs]-> Unit { session.context() ?? return }
fn subscribe(session: Browser) --[Fs]-> Unit { session.subscribe("log.entryAdded") ?? return }
fn next(session: Browser, timeout: BrowserTimeout) --[Fs]-> Unit { session.next_event(timeout) ?? return }
fn protocol(session: Browser) --[Fs]-> Unit { session.protocol("bidi") ?? return }
fn close(session: Browser) --[Fs]-> Unit { session.close() ?? return }
fn page(context: BrowserContext) --[Fs]-> Unit { context.page() ?? return }
fn goto(page: BrowserPage) --[Fs]-> Unit { page.goto("https://example.test") ?? return }
fn wait(locator: BrowserLocator, timeout: BrowserTimeout) --[Fs]-> Unit { locator.wait(timeout) ?? return }
fn click(locator: BrowserLocator) --[Fs]-> Unit { locator.click() ?? return }
fn send(protocol: BrowserProtocol) --[Fs]-> Unit { protocol.send("session.status", "{{}}") ?? return }
fn run() {}
"#;
    let diags = jet::compile(source).expect_err("Browser I/O methods must infer Net");
    assert!(
        diags.iter().filter(|diag| diag.code == "E0740").count() >= 10,
        "every Browser I/O method must violate an Fs-only bound: {diags:?}"
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
            "session.end",
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
    let source = r#"
use core.browser as browser

fn connect_outcome(endpoint: String) -> String {
    profile :: browser.profile("bidi-2025.5") ?? return "unexpected-profile"
    timeout :: browser.timeout(250) ?? return "unexpected-timeout"
    session :: browser.connect_profile(endpoint, profile, timeout) ?? return "caught"
    session.close() ?? return "close-error"
    return "unexpected-success"
}

fn profile_outcome() -> String {
    profile :: browser.profile("rolling") ?? return "caught"
    return "unexpected-success"
}

fn timeout_outcome() -> String {
    timeout :: browser.timeout(0) ?? return "caught"
    return "unexpected-success"
}

fn cdp_outcome(endpoint: String) -> String {
    profile :: browser.profile("bidi-2025.5") ?? return "unexpected-profile"
    timeout :: browser.timeout(250) ?? return "unexpected-timeout"
    session :: browser.connect_profile(endpoint, profile, timeout) ?? return "unexpected-connect"
    cdp :: session.protocol("cdp") ?? return "caught"
    return "unexpected-success"
}

fn run() {
    print(connect_outcome("__MALFORMED__"))
    print(connect_outcome("__MISMATCHED__"))
    print(connect_outcome("__PROTOCOL__"))
    print(connect_outcome("__TIMEOUT__"))
    print(profile_outcome())
    print(timeout_outcome())
    print(cdp_outcome("__NO_CDP__"))
}
"#
    .replace("__MALFORMED__", &malformed)
    .replace("__MISMATCHED__", &mismatched)
    .replace("__PROTOCOL__", &protocol)
    .replace("__TIMEOUT__", &timeout)
    .replace("__NO_CDP__", &no_cdp);

    let (code, stdout, stderr) =
        common::build_and_run("jet_browser_bidi_hostile", "browser_bidi_hostile", &source);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "caught\ncaught\ncaught\ncaught\ncaught\ncaught\ncaught\n"
    );
    assert!(!stdout.contains("SECRET"), "{stdout}");
    assert!(!stderr.contains("SECRET"), "{stderr}");

    malformed_server.join().unwrap();
    mismatched_server.join().unwrap();
    protocol_server.join().unwrap();
    timeout_server.join().unwrap();
    no_cdp_server.join().unwrap();
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

    let (forced_stdout, forced_methods, _) = run_once(true);
    assert_eq!(forced_stdout, "bidi-2024.11\nblocked\n");
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
}
