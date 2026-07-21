mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{fs, process::Command};

fn request_head(stream: &mut TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut out = Vec::new();
    let mut chunk = [0; 512];
    while !out.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut chunk).unwrap();
        assert_ne!(read, 0, "client closed before request head");
        out.extend_from_slice(&chunk[..read]);
        assert!(out.len() <= 64 * 1024, "request head exceeded bound");
    }
    out
}

fn read_h2_frame(stream: &mut TcpStream) -> (u8, u8, u32, Vec<u8>) {
    let mut head = [0; 9];
    stream.read_exact(&mut head).unwrap();
    let length = usize::from(head[0]) << 16 | usize::from(head[1]) << 8 | usize::from(head[2]);
    let id = u32::from_be_bytes(head[5..9].try_into().unwrap()) & 0x7fff_ffff;
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload).unwrap();
    (head[3], head[4], id, payload)
}

fn write_h2_frame(stream: &mut TcpStream, kind: u8, flags: u8, id: u32, payload: &[u8]) {
    let length = payload.len() as u32;
    let bytes = length.to_be_bytes();
    let mut head = [0; 9];
    head[..3].copy_from_slice(&bytes[1..]);
    head[3] = kind;
    head[4] = flags;
    head[5..].copy_from_slice(&id.to_be_bytes());
    stream.write_all(&head).unwrap();
    stream.write_all(payload).unwrap();
}

#[test]
fn pooled_h1_reuses_only_after_drain_and_decodes_gzip_natively() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let accepts = Arc::new(AtomicUsize::new(0));
    let observed = accepts.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        observed.fetch_add(1, Ordering::Relaxed);
        let first = request_head(&mut stream);
        assert!(first.starts_with(b"GET /first HTTP/1.1\r\n"));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: keep-alive\r\n\r\none")
            .unwrap();
        let second = request_head(&mut stream);
        assert!(second.starts_with(b"GET /second HTTP/1.1\r\n"));
        assert!(String::from_utf8_lossy(&second)
            .to_ascii_lowercase()
            .contains("accept-encoding: gzip"));
        const GZIP_HELLO: &[u8] = &[
            0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x03, 0xcb, 0x48, 0xcd, 0xc9,
            0xc9, 0x07, 0x00, 0x86, 0xa6, 0x10, 0x36, 0x05, 0x00, 0x00, 0x00,
        ];
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", GZIP_HELLO.len()).unwrap();
        stream.write_all(GZIP_HELLO).unwrap();
    });
    let src = format!(
        r#"
use core.http.client as http
fn run() {{
    first :: http.get("http://{addr}/first") ?? panic("first")
    print(first.body().text(16) ?? panic("first body"))
    second :: http.get("http://{addr}/second") ?? panic("second")
    print(second.body().text(16) ?? panic("gzip body"))
    print(second.header("content-encoding") ?? "missing")
    print(second.header("content-length") ?? "missing")
}}
"#
    );
    let (code, stdout, stderr) = common::build_and_run("jet_http_client_law", "pool_gzip", &src);
    server.join().unwrap();
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "one\nhello\nmissing\nmissing\n");
    assert_eq!(
        accepts.load(Ordering::Relaxed),
        1,
        "drained response did not reuse one connection"
    );
}

#[test]
fn h1_skips_early_hints_reuses_final_response_and_rejects_upgrade() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        assert!(request_head(&mut stream).starts_with(b"GET /early HTTP/1.1\r\n"));
        stream
            .write_all(
                b"HTTP/1.1 103 Early Hints\r\nLink: </style.css>; rel=preload\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: keep-alive\r\n\r\none",
            )
            .unwrap();
        assert!(request_head(&mut stream).starts_with(b"GET /reuse HTTP/1.1\r\n"));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\ntwo")
            .unwrap();
    });
    let upgrade_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let upgrade_addr = upgrade_listener.local_addr().unwrap();
    let upgrade_server = std::thread::spawn(move || {
        let (mut stream, _) = upgrade_listener.accept().unwrap();
        request_head(&mut stream);
        stream
            .write_all(
                b"HTTP/1.1 101 Switching Protocols\r\nConnection: upgrade\r\nUpgrade: websocket\r\n\r\n",
            )
            .unwrap();
    });
    let src = format!(
        r#"
use core.http.client as http
fn run() {{
    first :: http.get("http://{addr}/early") ?? panic("early")
    print(first.body().text(8) ?? panic("first body"))
    second :: http.get("http://{addr}/reuse") ?? panic("reuse")
    print(second.body().text(8) ?? panic("second body"))
    if http.get("http://{upgrade_addr}/upgrade") == {{
        .Ok(_) -> {{ print("accepted") }}
        .Err(error) -> {{ print(error) }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = common::build_and_run("jet_http_client_law", "interim", &src);
    server.join().unwrap();
    upgrade_server.join().unwrap();
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "one\ntwo\nunsupported HTTP protocol unsupported\n");
}

#[test]
fn gzip_decoding_streams_before_the_response_finishes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let signal = TcpListener::bind("127.0.0.1:0").unwrap();
    let signal_addr = signal.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        request_head(&mut stream);
        let prefix = [
            0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0, 0, // gzip header
            0x00, 0x05, 0x00, 0xfa, 0xff, b'h', b'e', b'l', b'l', b'o',
        ];
        let suffix = [
            0x01, 0x05, 0x00, 0xfa, 0xff, b'w', b'o', b'r', b'l', b'd', // final block
            0xad, 0x20, 0xeb, 0xf9, 0x0a, 0, 0, 0, // CRC32 + size
        ];
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            prefix.len() + suffix.len()
        )
        .unwrap();
        stream.write_all(&prefix).unwrap();
        stream.flush().unwrap();

        signal.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match signal.accept() {
                Ok(_) => break,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "decoder buffered the unfinished gzip response"
                    );
                    std::thread::yield_now();
                }
                Err(error) => panic!("gzip signal accept failed: {error}"),
            }
        }
        stream.write_all(&suffix).unwrap();
        drop(stream);
        let (mut stream, _) = listener.accept().unwrap();
        request_head(&mut stream);
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Encoding: br\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
            )
            .unwrap();
    });

    let dir = std::env::temp_dir().join(format!("jet_http_client_gzip_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let shown = dir.join("seed.jet");
    let source = "use core.http.client as http\nfn run() { req :: http.request(\"GET\", \"http://127.0.0.1/\") }\n";
    fs::write(&shown, source).unwrap();
    let link = jet::compile_with_path(source, shown.to_str().unwrap())
        .unwrap()
        .ffi
        .expect("HTTP client bridge");
    let harness = dir.join("gzip_stream.rs");
    let bin = dir.join("gzip_stream");
    fs::write(
        &harness,
        r#"
fn main() {
    let url = std::env::args().nth(1).unwrap();
    let signal = std::env::args().nth(2).unwrap();
    let root = bridge::jet_http_client_new_impl();
    let response = bridge::jet_http_client_send_with_impl(
        root, "GET", &url, &[], None, None, None, None, None, None, None, &[], &[], &[],
    ).unwrap();
    assert_eq!(bridge::jet_http_client_body_read_impl(response.1, 5).unwrap(), Some(b"hello".to_vec()));
    std::net::TcpStream::connect(signal).unwrap();
    assert_eq!(bridge::jet_http_client_body_read_impl(response.1, 5).unwrap(), Some(b"world".to_vec()));
    assert_eq!(bridge::jet_http_client_body_read_impl(response.1, 5).unwrap(), None);
    assert_eq!(bridge::jet_http_client_send_with_impl(
        root, "GET", &url, &[], None, None, None, None, None, None, None, &[], &[], &[],
    ).unwrap_err(), bridge::JetHttpBridgeError::UnsupportedEncoding);
}
"#,
    )
    .unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args([
        "--edition",
        "2021",
        harness.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    rustc
        .arg("--extern")
        .arg(format!("bridge={}", link.rlib_path.display()));
    for dependency in link.dependency_dirs().filter(|path| path.is_dir()) {
        rustc
            .arg("-L")
            .arg(format!("dependency={}", dependency.display()));
    }
    let built = rustc.output().unwrap();
    assert!(
        built.status.success(),
        "bridge gzip harness compile failed:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let output = Command::new(&bin)
        .arg(format!("http://{addr}/gzip"))
        .arg(signal_addr.to_string())
        .output()
        .unwrap();
    let server_result = server.join();
    assert!(
        output.status.success(),
        "bridge gzip harness failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    server_result.unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unread_body_is_not_reused() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        request_head(&mut first);
        first
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: keep-alive\r\n\r\none")
            .unwrap();
        let (mut second, _) = listener.accept().unwrap();
        request_head(&mut second);
        second
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\ntwo")
            .unwrap();
    });
    let src = format!(
        r#"
use core.http.client as http
fn run() {{
    first :: http.get("http://{addr}/first") ?? panic("first")
    second :: http.get("http://{addr}/second") ?? panic("second")
    print(second.body().text(16) ?? panic("second body"))
    print(first.status())
}}
"#
    );
    let (code, stdout, stderr) = common::build_and_run("jet_http_client_law", "unread", &src);
    server.join().unwrap();
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "two\n200\n");
}

#[test]
fn native_bridge_removes_ureq_from_generated_dependency_graph() {
    let source = std::fs::read_to_string("crates/jet-pkg-model/src/FFI.rs").unwrap();
    assert!(!source.contains("HTTP_CLIENT_CRATE_SPEC"));
    assert!(
        !source.contains("\"ureq\","),
        "HTTP client still emits ureq"
    );
    let runtime = std::fs::read_to_string("crates/jet-pkg-model/src/Prelude/Http.rs").unwrap();
    assert!(
        !runtime.contains("ureq::"),
        "native runtime still calls ureq"
    );
}

#[test]
fn custom_client_clones_share_pool_cookie_jar_and_transport_facts() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let first = request_head(&mut stream);
        assert!(!String::from_utf8_lossy(&first)
            .to_ascii_lowercase()
            .contains("cookie:"));
        stream.write_all(
            b"HTTP/1.1 200 OK\r\nSet-Cookie: session=abc; Path=/; Max-Age=60\r\nSet-Cookie: expired=gone; Expires=Wed, 21 Oct 2015 07:28:00 GMT\r\nSet-Cookie: none=bad; SameSite=None\r\nSet-Cookie: __Host-bad=value; Path=/\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n",
        ).unwrap();
        let second = request_head(&mut stream);
        let second = String::from_utf8_lossy(&second).to_ascii_lowercase();
        assert!(second.contains("cookie: session=abc\r\n"));
        assert!(
            !second.contains("expired=")
                && !second.contains("none=")
                && !second.contains("__host-bad=")
        );
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .unwrap();
    });

    let dir = std::env::temp_dir().join(format!("jet_http_client_policy_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let shown = dir.join("seed.jet");
    let source = "use core.http.client as http\nfn run() { req :: http.request(\"GET\", \"http://127.0.0.1/\") }\n";
    fs::write(&shown, source).unwrap();
    let compiled = jet::compile_with_path(source, shown.to_str().unwrap()).unwrap();
    let link = compiled.ffi.expect("HTTP client bridge");
    let harness = dir.join("policy.rs");
    let bin = dir.join("policy");
    fs::write(
        &harness,
        r#"
fn send(client: i64, url: &String) -> (i64, i64, Option<i64>, Vec<String>) {
    bridge::jet_http_client_send_with_impl(
        client, "GET", url, &[], None, None, None, None, None, None, None,
        &[], &[], &[],
    ).unwrap()
}

fn main() {
    let url = std::env::args().nth(1).unwrap();
    let root = bridge::jet_http_client_new_impl();
    let client = bridge::jet_http_client_cookies_impl(root, true).unwrap();
    bridge::jet_http_client_drop_impl(root);
    let first = send(client, &url);
    assert_eq!(bridge::jet_http_client_body_read_impl(first.1, 64).unwrap(), None);
    let second = send(client, &url);
    assert_eq!(bridge::jet_http_client_body_read_impl(second.1, 64).unwrap(), Some(b"ok".to_vec()));
    assert_eq!(bridge::jet_http_client_body_read_impl(second.1, 64).unwrap(), None);
    assert_eq!(bridge::jet_http_client_response_protocol_impl(second.1), "HTTP/1.1");
    assert!(!bridge::jet_http_client_response_remote_address_impl(second.1).is_empty());
    assert!(bridge::jet_http_client_response_redirect_history_impl(second.1).is_empty());
    assert_eq!(bridge::jet_http_client_response_timings_impl(second.1).len(), 7);
    assert!(bridge::jet_http_client_response_reused_impl(second.1));
    bridge::jet_http_client_response_facts_drop_impl(first.1);
    bridge::jet_http_client_response_facts_drop_impl(second.1);
    bridge::jet_http_client_drop_impl(client);
}
"#,
    )
    .unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args([
        "--edition",
        "2021",
        harness.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    rustc
        .arg("--extern")
        .arg(format!("bridge={}", link.rlib_path.display()));
    for dependency in link.dependency_dirs().filter(|path| path.is_dir()) {
        rustc
            .arg("-L")
            .arg(format!("dependency={}", dependency.display()));
    }
    let built = rustc.output().unwrap();
    assert!(
        built.status.success(),
        "bridge harness compile failed:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let output = Command::new(&bin)
        .arg(format!("http://{addr}/cookie"))
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(
        output.status.success(),
        "bridge harness failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn public_client_policy_exposes_pool_cookie_timeouts_and_facts() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let first = request_head(&mut stream);
        assert!(!String::from_utf8_lossy(&first)
            .to_ascii_lowercase()
            .contains("cookie:"));
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nSet-Cookie: session=abc; Path=/; Max-Age=60\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n",
            )
            .unwrap();
        let second = request_head(&mut stream);
        assert!(String::from_utf8_lossy(&second)
            .to_ascii_lowercase()
            .contains("cookie: session=abc\r\n"));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .unwrap();
    });
    let src = format!(
        r#"
use core.http.client as http
fn run() {{
    client :: http.Client.new()
        .cookies(true)
        .redirects(2)
        .protocols(false, true, false)
        .timeouts(1000, 1000, 1000, 1000, 1000, 1000, 5000)
        .raw_encoding()
    first :: client.send(http.request("GET", "http://{addr}/first")) ?? panic("first")
    _ :: first.body().bytes(8) ?? panic("first body")
    second :: client.send(http.request("GET", "http://{addr}/second")) ?? panic("second")
    print(second.body().text(8) ?? panic("second body"))
    print(second.protocol())
    print(second.redirect_history().len())
    print(second.timings().len())
    print(second.reused_connection())
    print(second.raw_content_encoding() ?? "none")
}}
"#
    );
    let (code, stdout, stderr) =
        common::build_and_run("jet_http_client_law", "public_client", &src);
    server.join().unwrap();
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "ok\nHTTP/1.1\n0\n7\ntrue\nnone\n");
}

#[test]
fn explicit_h2c_negotiates_hpack_and_reuses_one_session() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut preface = [0; 24];
        stream.read_exact(&mut preface).unwrap();
        assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        assert_eq!(read_h2_frame(&mut stream).0, 4);
        write_h2_frame(&mut stream, 4, 0, 0, &[]);
        let request = loop {
            let frame = read_h2_frame(&mut stream);
            if frame.0 == 1 {
                break frame;
            }
        };
        assert_eq!(request.2, 1);
        assert_ne!(request.1 & 4, 0);
        // :status 200 plus content-length: 2, with the value HPACK-Huffman encoded.
        let response = [0x88, 0x0f, 0x0d, 0x81, 0x17];
        write_h2_frame(&mut stream, 1, 4, 1, &response);
        write_h2_frame(&mut stream, 0, 1, 1, b"w0");

        // Do not send either final response HEADERS until both request streams
        // are open. A client that holds its connection mutex while awaiting the
        // first HEADERS deadlocks here instead of multiplexing.
        let mut requests = Vec::new();
        while requests.len() < 2 {
            let frame = read_h2_frame(&mut stream);
            if frame.0 == 6 && frame.1 & 1 == 0 {
                write_h2_frame(&mut stream, 6, 1, 0, &frame.3);
                continue;
            }
            if frame.0 == 1 {
                requests.push(frame);
            }
        }
        requests.sort_by_key(|frame| frame.2);
        assert_eq!(requests.iter().map(|frame| frame.2).collect::<Vec<_>>(), [3, 5]);
        write_h2_frame(&mut stream, 1, 4, 5, &response);
        write_h2_frame(&mut stream, 1, 4, 3, &response);
        write_h2_frame(&mut stream, 0, 1, 5, b"b2");
        write_h2_frame(&mut stream, 0, 1, 3, b"a1");
        drop(stream);

        // A pooled HTTP/2 session may go stale between requests. The client must
        // retire it before writing request bytes and reconnect exactly once.
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "client did not reconnect stale HTTP/2 session"
                    );
                    std::thread::yield_now();
                }
                Err(error) => panic!("second HTTP/2 accept failed: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.read_exact(&mut preface).unwrap();
        assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        assert_eq!(read_h2_frame(&mut stream).0, 4);
        write_h2_frame(&mut stream, 4, 0, 0, &[]);
        let request = loop {
            let frame = read_h2_frame(&mut stream);
            if frame.0 == 1 {
                break frame;
            }
        };
        assert_eq!(request.2, 1);
        write_h2_frame(&mut stream, 1, 4, 1, &response);
        write_h2_frame(&mut stream, 0, 1, 1, b"c3");
        let settings_ack = read_h2_frame(&mut stream);
        assert_eq!((settings_ack.0, settings_ack.1, settings_ack.2), (4, 1, 0));
    });

    let dir = std::env::temp_dir().join(format!("jet_http_client_h2_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let shown = dir.join("seed.jet");
    let source = "use core.http.client as http\nfn run() { req :: http.request(\"GET\", \"http://127.0.0.1/\") }\n";
    fs::write(&shown, source).unwrap();
    let link = jet::compile_with_path(source, shown.to_str().unwrap())
        .unwrap()
        .ffi
        .expect("HTTP client bridge");
    let harness = dir.join("h2.rs");
    let bin = dir.join("h2");
    fs::write(
        &harness,
        r#"
fn main() {
    let url = std::env::args().nth(1).unwrap();
    let root = bridge::jet_http_client_new_impl();
    let client = bridge::jet_http_client_protocols_impl(root, true, false, true).unwrap();
    let warm = bridge::jet_http_client_send_with_impl(
        client, "GET", &url, &[], None, None, None, None, None, None, None, &[], &[], &[],
    ).unwrap();
    assert_eq!(bridge::jet_http_client_body_read_impl(warm.1, 64).unwrap(), Some(b"w0".to_vec()));
    assert_eq!(bridge::jet_http_client_body_read_impl(warm.1, 64).unwrap(), None);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let send = |barrier: std::sync::Arc<std::sync::Barrier>, url: String| std::thread::spawn(move || {
        barrier.wait();
        let response = bridge::jet_http_client_send_with_impl(
            client, "GET", &url, &[], None, None, None, None, None, None, None, &[], &[], &[],
        ).unwrap();
        let body = bridge::jet_http_client_body_read_impl(response.1, 64).unwrap().unwrap();
        assert_eq!(bridge::jet_http_client_body_read_impl(response.1, 64).unwrap(), None);
        (response, body)
    });
    let first = send(barrier.clone(), url.clone());
    let second = send(barrier.clone(), url.clone());
    barrier.wait();
    let first = first.join().unwrap();
    let second = second.join().unwrap();
    let mut bodies = vec![first.1, second.1];
    bodies.sort();
    assert_eq!(bodies, vec![b"a1".to_vec(), b"b2".to_vec()]);
    let third = bridge::jet_http_client_send_with_impl(
        client, "GET", &url, &[], None, None, None, None, None, None, None, &[], &[], &[],
    ).unwrap();
    assert_eq!(bridge::jet_http_client_body_read_impl(third.1, 64).unwrap(), Some(b"c3".to_vec()));
    assert_eq!(bridge::jet_http_client_body_read_impl(third.1, 64).unwrap(), None);
    assert_eq!(bridge::jet_http_client_response_protocol_impl(warm.1), "HTTP/2");
    assert_eq!(bridge::jet_http_client_response_protocol_impl(first.0.1), "HTTP/2");
    assert_eq!(bridge::jet_http_client_response_protocol_impl(second.0.1), "HTTP/2");
    assert_eq!(bridge::jet_http_client_response_protocol_impl(third.1), "HTTP/2");
    assert!(!bridge::jet_http_client_response_reused_impl(warm.1));
    assert!(bridge::jet_http_client_response_reused_impl(first.0.1));
    assert!(bridge::jet_http_client_response_reused_impl(second.0.1));
    assert!(!bridge::jet_http_client_response_reused_impl(third.1));
}

"#,
    )
    .unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args([
        "--edition",
        "2021",
        harness.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    rustc
        .arg("--extern")
        .arg(format!("bridge={}", link.rlib_path.display()));
    for dependency in link.dependency_dirs().filter(|path| path.is_dir()) {
        rustc
            .arg("-L")
            .arg(format!("dependency={}", dependency.display()));
    }
    let built = rustc.output().unwrap();
    assert!(
        built.status.success(),
        "bridge harness compile failed:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let output = Command::new(&bin)
        .arg(format!("http://{addr}/h2"))
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(
        output.status.success(),
        "bridge harness failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn https_negotiates_h2_with_an_explicit_root() {
    let dir = std::env::temp_dir().join(format!("jet_http_client_tls_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let shown = dir.join("seed.jet");
    let source = "use core.http.client as http\nfn run() { req :: http.request(\"GET\", \"https://127.0.0.1/\") }\n";
    fs::write(&shown, source).unwrap();
    let link = jet::compile_with_path(source, shown.to_str().unwrap())
        .unwrap()
        .ffi
        .expect("HTTP client bridge");
    let harness = dir.join("tls_h2.rs");
    let bin = dir.join("tls_h2");
    fs::write(
        &harness,
        r#"
use std::io::{Read, Write};

fn pem(path: &str) -> Vec<u8> {
    let text = std::fs::read_to_string(path).unwrap();
    let encoded: String = text.lines().filter(|line| !line.starts_with("-----")).collect();
    let mut out = Vec::new();
    let mut bits = 0u32;
    let mut count = 0u8;
    for byte in encoded.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => continue,
        };
        bits = bits << 6 | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1u32 << count).wrapping_sub(1);
        }
    }
    out
}

fn read_frame(stream: &mut impl Read) -> (u8, u8, u32, Vec<u8>) {
    let mut head = [0; 9];
    stream.read_exact(&mut head).unwrap();
    let length = usize::from(head[0]) << 16 | usize::from(head[1]) << 8 | usize::from(head[2]);
    let id = u32::from_be_bytes(head[5..9].try_into().unwrap()) & 0x7fff_ffff;
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload).unwrap();
    (head[3], head[4], id, payload)
}

fn write_frame(stream: &mut impl Write, kind: u8, flags: u8, id: u32, payload: &[u8]) {
    let length = (payload.len() as u32).to_be_bytes();
    let mut head = [0; 9];
    head[..3].copy_from_slice(&length[1..]);
    head[3] = kind;
    head[4] = flags;
    head[5..].copy_from_slice(&id.to_be_bytes());
    stream.write_all(&head).unwrap();
    stream.write_all(payload).unwrap();
}

fn main() {
    let cert = pem(&std::env::args().nth(1).unwrap());
    let key = pem(&std::env::args().nth(2).unwrap());
    let root_cert = pem(&std::env::args().nth(3).unwrap());
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut server = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(cert.clone())],
            rustls::pki_types::PrivateKeyDer::Pkcs8(
                rustls::pki_types::PrivatePkcs8KeyDer::from(key),
            ),
        )
        .unwrap();
    server.alpn_protocols = vec![b"h2".to_vec()];
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let connection = rustls::ServerConnection::new(std::sync::Arc::new(server)).unwrap();
        let mut stream = rustls::StreamOwned::new(connection, socket);
        let mut preface = [0; 24];
        stream.read_exact(&mut preface).unwrap();
        assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        assert_eq!(stream.conn.alpn_protocol(), Some(b"h2".as_slice()));
        assert_eq!(read_frame(&mut stream).0, 4);
        write_frame(&mut stream, 4, 0, 0, &[]);
        let request = loop {
            let frame = read_frame(&mut stream);
            if frame.0 == 1 { break frame; }
        };
        write_frame(&mut stream, 1, 4, request.2, &[0x88, 0x0f, 0x0d, 0x01, b'2']);
        write_frame(&mut stream, 0, 1, request.2, b"ok");
        stream.flush().unwrap();
        let settings_ack = read_frame(&mut stream);
        assert_eq!((settings_ack.0, settings_ack.1, settings_ack.2), (4, 1, 0));
    });

    let root = bridge::jet_http_client_new_impl();
    let rooted = bridge::jet_http_client_root_certificate_impl(root, &root_cert, false).unwrap();
    let client = bridge::jet_http_client_protocols_impl(rooted, true, false, false).unwrap();
    let url = format!("https://localhost:{}/tls", address.port());
    let response = bridge::jet_http_client_send_with_impl(
        client, "GET", &url, &[], None, None, None, None, None, None, None, &[], &[], &[],
    ).unwrap();
    assert_eq!(bridge::jet_http_client_response_protocol_impl(response.1), "HTTP/2");
    assert_eq!(bridge::jet_http_client_body_read_impl(response.1, 64).unwrap(), Some(b"ok".to_vec()));
    assert_eq!(bridge::jet_http_client_body_read_impl(response.1, 64).unwrap(), None);
    server.join().unwrap();
}
"#,
    )
    .unwrap();
    let dependency_dirs: Vec<_> = link
        .dependency_dirs()
        .filter(|path| path.is_dir())
        .collect();
    let rustls = dependency_dirs
        .iter()
        .flat_map(|directory| fs::read_dir(directory).unwrap())
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("librustls-") && name.ends_with(".rlib"))
        })
        .expect("generated bridge rustls dependency");
    let mut rustc = Command::new("rustc");
    rustc.args([
        "--edition",
        "2021",
        harness.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    rustc
        .arg("--extern")
        .arg(format!("bridge={}", link.rlib_path.display()))
        .arg("--extern")
        .arg(format!("rustls={}", rustls.display()));
    for dependency in dependency_dirs {
        rustc
            .arg("-L")
            .arg(format!("dependency={}", dependency.display()));
    }
    let built = rustc.output().unwrap();
    assert!(
        built.status.success(),
        "bridge TLS harness compile failed:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let output = Command::new(&bin)
        .arg(fs::canonicalize("tests/fixtures/tls/smtp.server.cert.pem").unwrap())
        .arg(fs::canonicalize("tests/fixtures/tls/smtp.server.key.pem").unwrap())
        .arg(fs::canonicalize("tests/fixtures/tls/smtp.ca.cert.pem").unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "bridge TLS harness failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn h2_rejects_invalid_hpack_names_padding_and_body_lengths() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for case in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut preface = [0; 24];
            stream.read_exact(&mut preface).unwrap();
            assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
            assert_eq!(read_h2_frame(&mut stream).0, 4);
            write_h2_frame(&mut stream, 4, 0, 0, &[]);
            let request = loop {
                let frame = read_h2_frame(&mut stream);
                if frame.0 == 1 {
                    break frame;
                }
            };
            match case {
                0 => write_h2_frame(
                    &mut stream,
                    1,
                    5,
                    request.2,
                    &[0x88, 0x00, 0x01, b'X', 0x01, b'y'],
                ),
                1 => write_h2_frame(
                    &mut stream,
                    1,
                    5,
                    request.2,
                    &[0x88, 0x0f, 0x0d, 0x81, 0xff],
                ),
                _ => {
                    write_h2_frame(
                        &mut stream,
                        1,
                        4,
                        request.2,
                        &[0x88, 0x0f, 0x0d, 0x01, b'2'],
                    );
                    write_h2_frame(&mut stream, 0, 1, request.2, b"x");
                }
            }
            let _settings_ack = read_h2_frame(&mut stream);
        }
    });

    let dir =
        std::env::temp_dir().join(format!("jet_http_client_h2_hostile_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let shown = dir.join("seed.jet");
    let source = "use core.http.client as http\nfn run() { req :: http.request(\"GET\", \"http://127.0.0.1/\") }\n";
    fs::write(&shown, source).unwrap();
    let link = jet::compile_with_path(source, shown.to_str().unwrap())
        .unwrap()
        .ffi
        .expect("HTTP client bridge");
    let harness = dir.join("h2_hostile.rs");
    let bin = dir.join("h2_hostile");
    fs::write(
        &harness,
        r#"
fn client() -> i64 {
    let root = bridge::jet_http_client_new_impl();
    bridge::jet_http_client_protocols_impl(root, true, false, true).unwrap()
}
fn main() {
    let url = std::env::args().nth(1).unwrap();
    println!("{:?}", bridge::jet_http_client_send_with_impl(
        client(), "GET", &url, &[], None, None, None, None, None, None, None, &[], &[], &[],
    ).unwrap_err());
    println!("{:?}", bridge::jet_http_client_send_with_impl(
        client(), "GET", &url, &[], None, None, None, None, None, None, None, &[], &[], &[],
    ).unwrap_err());
    let response = bridge::jet_http_client_send_with_impl(
        client(), "GET", &url, &[], None, None, None, None, None, None, None, &[], &[], &[],
    ).unwrap();
    println!("{:?}", bridge::jet_http_client_body_read_impl(response.1, 64).unwrap_err());
}
"#,
    )
    .unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args([
        "--edition",
        "2021",
        harness.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    rustc
        .arg("--extern")
        .arg(format!("bridge={}", link.rlib_path.display()));
    for dependency in link.dependency_dirs().filter(|path| path.is_dir()) {
        rustc
            .arg("-L")
            .arg(format!("dependency={}", dependency.display()));
    }
    let built = rustc.output().unwrap();
    assert!(
        built.status.success(),
        "bridge harness compile failed:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let output = Command::new(&bin)
        .arg(format!("http://{addr}/h2"))
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(
        output.status.success(),
        "bridge harness failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "InvalidHeader\nProtocol\nInvalidFraming\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn upload_streams_before_the_body_source_finishes() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let dir = std::env::temp_dir().join(format!(
        "jet_http_client_upload_stream_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let fifo = dir.join("upload.fifo");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success(),
        "mkfifo failed"
    );
    let accepted = Arc::new(AtomicBool::new(false));
    let accepted_flag = accepted.clone();
    let seen_first_chunk = Arc::new(AtomicBool::new(false));
    let seen_flag = seen_first_chunk.clone();
    let body_len = 200 * 1024usize;
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        accepted_flag.store(true, Ordering::SeqCst);
        let head = request_head(&mut stream);
        assert!(head.starts_with(b"POST /upload HTTP/1.1\r\n"));
        let head_text = String::from_utf8_lossy(&head);
        assert!(
            head_text
                .to_ascii_lowercase()
                .contains(&format!("content-length: {body_len}")),
            "missing content-length: {head_text}"
        );
        let mut got = 0usize;
        let mut buf = [0u8; 4096];
        while got < body_len {
            let read = stream.read(&mut buf).unwrap();
            assert_ne!(read, 0, "upload ended early at {got}");
            got += read;
            if got >= 64 * 1024 {
                seen_flag.store(true, Ordering::SeqCst);
            }
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .unwrap();
        got
    });
    let fifo_writer = fifo.clone();
    let writer = std::thread::spawn(move || {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(&fifo_writer)
            .unwrap();
        let first = vec![b'a'; 32 * 1024];
        file.write_all(&first).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !accepted.load(Ordering::SeqCst) {
            assert!(
                Instant::now() < deadline,
                "client buffered the upload before connecting"
            );
            std::thread::yield_now();
        }
        let rest = vec![b'b'; body_len - first.len()];
        file.write_all(&rest).unwrap();
    });
    let fifo_path = fifo
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let src = format!(
        r#"
use core.http.client as http
use core.http as message
use core.files as files
fn run() {{
    input :: files.open("{fifo_path}") ?? panic("open")
    body :: message.Body.reader(^input, {body_len}) ?? panic("reader")
    req :: http.request("POST", "http://{addr}/upload").body(body).redirects(0)
    resp :: req.send() ?? panic("send")
    print(resp.body().text(8) ?? panic("body"))
}}
"#
    );
    let (code, stdout, stderr) = common::build_and_run("jet_http_client_law", "upload_stream", &src);
    let got = server.join().unwrap();
    writer.join().unwrap();
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "ok\n");
    assert_eq!(got, body_len);
    assert!(
        seen_first_chunk.load(Ordering::SeqCst),
        "server never saw a 64KiB upload boundary"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn post_307_replays_streamed_body_under_redirect_tee_cap() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let dir = std::env::temp_dir().join(format!(
        "jet_http_client_post_307_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let fifo = dir.join("upload.fifo");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success(),
        "mkfifo failed"
    );
    let body_len = 96 * 1024usize;
    let seen_first = Arc::new(AtomicBool::new(false));
    let seen_first_flag = seen_first.clone();
    let seen_replay = Arc::new(AtomicBool::new(false));
    let seen_replay_flag = seen_replay.clone();
    let server = std::thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        let head = request_head(&mut first);
        assert!(head.starts_with(b"POST /from HTTP/1.1\r\n"));
        let head_text = String::from_utf8_lossy(&head);
        assert!(
            head_text
                .to_ascii_lowercase()
                .contains(&format!("content-length: {body_len}")),
            "missing content-length: {head_text}"
        );
        let mut got = 0usize;
        let mut buf = [0u8; 4096];
        while got < body_len {
            let read = first.read(&mut buf).unwrap();
            assert_ne!(read, 0, "first upload ended early at {got}");
            got += read;
            if got >= 64 * 1024 {
                seen_first_flag.store(true, Ordering::SeqCst);
            }
        }
        first
            .write_all(
                b"HTTP/1.1 307 Temporary Redirect\r\nLocation: /to\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
        drop(first);

        let (mut second, _) = listener.accept().unwrap();
        let head = request_head(&mut second);
        assert!(
            head.starts_with(b"POST /to HTTP/1.1\r\n"),
            "307 must preserve POST: {}",
            String::from_utf8_lossy(&head)
        );
        let head_text = String::from_utf8_lossy(&head).to_ascii_lowercase();
        assert!(
            head_text.contains(&format!("content-length: {body_len}")),
            "replay missing content-length: {head_text}"
        );
        got = 0;
        while got < body_len {
            let read = second.read(&mut buf).unwrap();
            assert_ne!(read, 0, "replayed upload ended early at {got}");
            got += read;
        }
        assert_eq!(got, body_len);
        seen_replay_flag.store(true, Ordering::SeqCst);
        second
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .unwrap();
    });
    let fifo_writer = fifo.clone();
    let writer = std::thread::spawn(move || {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(&fifo_writer)
            .unwrap();
        let payload = vec![b'p'; body_len];
        file.write_all(&payload).unwrap();
    });
    let fifo_path = fifo
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let src = format!(
        r#"
use core.http.client as http
use core.http as message
use core.files as files
fn run() {{
    input :: files.open("{fifo_path}") ?? panic("open")
    body :: message.Body.reader(^input, {body_len}) ?? panic("reader")
    // Default redirect_limit (>0) must tee POST so 307 can replay the body.
    req :: http.request("POST", "http://{addr}/from").body(body)
    resp :: req.send() ?? panic("send")
    print(resp.body().text(8) ?? panic("body"))
    print(resp.redirect_history().len())
}}
"#
    );
    let (code, stdout, stderr) =
        common::build_and_run("jet_http_client_law", "post_307_replay", &src);
    server.join().unwrap();
    writer.join().unwrap();
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "ok\n1\n");
    assert!(
        seen_first.load(Ordering::SeqCst),
        "first POST never streamed a 64KiB boundary before 307"
    );
    assert!(
        seen_replay.load(Ordering::SeqCst),
        "307 follow never received the teed POST body"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unknown_length_upload_uses_h1_chunked_transfer_encoding() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let dir = std::env::temp_dir().join(format!(
        "jet_http_client_upload_chunked_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let fifo = dir.join("upload.fifo");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success(),
        "mkfifo failed"
    );
    let accepted = Arc::new(AtomicBool::new(false));
    let accepted_flag = accepted.clone();
    let seen_chunked = Arc::new(AtomicBool::new(false));
    let seen_flag = seen_chunked.clone();
    let body_len = 96 * 1024usize;
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        accepted_flag.store(true, Ordering::SeqCst);
        let head = request_head(&mut stream);
        assert!(head.starts_with(b"POST /chunked HTTP/1.1\r\n"));
        let head_text = String::from_utf8_lossy(&head).to_ascii_lowercase();
        assert!(
            head_text.contains("transfer-encoding: chunked"),
            "missing chunked transfer: {head_text}"
        );
        assert!(
            !head_text.contains("content-length:"),
            "unexpected content-length: {head_text}"
        );
        let mut got = 0usize;
        let mut buf = [0u8; 4096];
        let mut pending = Vec::new();
        loop {
            let read = stream.read(&mut buf).unwrap();
            assert_ne!(read, 0, "chunked upload ended early at {got}");
            pending.extend_from_slice(&buf[..read]);
            while let Some(line_end) = pending.windows(2).position(|window| window == b"\r\n") {
                let line = String::from_utf8_lossy(&pending[..line_end]).to_string();
                pending.drain(..line_end + 2);
                let size = usize::from_str_radix(line.trim(), 16).expect("chunk size");
                if size == 0 {
                    assert!(pending.starts_with(b"\r\n") || pending.is_empty() || pending == b"\r\n");
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                        .unwrap();
                    return got;
                }
                while pending.len() < size + 2 {
                    let read = stream.read(&mut buf).unwrap();
                    assert_ne!(read, 0, "chunked body truncated at {got}");
                    pending.extend_from_slice(&buf[..read]);
                }
                assert_eq!(&pending[size..size + 2], b"\r\n");
                got += size;
                pending.drain(..size + 2);
                if got >= 64 * 1024 {
                    seen_flag.store(true, Ordering::SeqCst);
                }
            }
        }
    });
    let fifo_writer = fifo.clone();
    let writer = std::thread::spawn(move || {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(&fifo_writer)
            .unwrap();
        let first = vec![b'a'; 32 * 1024];
        file.write_all(&first).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !accepted.load(Ordering::SeqCst) {
            assert!(
                Instant::now() < deadline,
                "client buffered the chunked upload before connecting"
            );
            std::thread::yield_now();
        }
        let rest = vec![b'b'; body_len - first.len()];
        file.write_all(&rest).unwrap();
    });
    let fifo_path = fifo
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let src = format!(
        r#"
use core.http.client as http
use core.http as message
use core.files as files
fn run() {{
    input :: files.open("{fifo_path}") ?? panic("open")
    body :: message.Body.reader(^input) ?? panic("reader")
    req :: http.request("POST", "http://{addr}/chunked").body(body).redirects(0)
    resp :: req.send() ?? panic("send")
    print(resp.body().text(8) ?? panic("body"))
}}
"#
    );
    let (code, stdout, stderr) =
        common::build_and_run("jet_http_client_law", "upload_chunked", &src);
    let got = server.join().unwrap();
    writer.join().unwrap();
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "ok\n");
    assert_eq!(got, body_len);
    assert!(
        seen_chunked.load(Ordering::SeqCst),
        "server never saw a streamed chunked upload boundary"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn h2_upload_streams_data_frames_before_body_eof() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let dir = std::env::temp_dir().join(format!(
        "jet_http_client_upload_h2_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let fifo = dir.join("upload.fifo");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success(),
        "mkfifo failed"
    );
    let accepted = Arc::new(AtomicBool::new(false));
    let accepted_flag = accepted.clone();
    let seen_data = Arc::new(AtomicBool::new(false));
    let seen_flag = seen_data.clone();
    let body_len = 40 * 1024usize;
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        accepted_flag.store(true, Ordering::SeqCst);
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut preface = [0; 24];
        stream.read_exact(&mut preface).unwrap();
        assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        assert_eq!(read_h2_frame(&mut stream).0, 4);
        write_h2_frame(&mut stream, 4, 0, 0, &[]);
        let mut got = 0usize;
        let mut end = false;
        while !end {
            let frame = read_h2_frame(&mut stream);
            if frame.0 == 6 && frame.1 & 1 == 0 {
                write_h2_frame(&mut stream, 6, 1, 0, &frame.3);
                continue;
            }
            if frame.0 == 1 {
                continue;
            }
            if frame.0 == 0 {
                got += frame.3.len();
                if got >= 16 * 1024 {
                    seen_flag.store(true, Ordering::SeqCst);
                }
                if frame.1 & 1 != 0 {
                    end = true;
                }
            }
        }
        assert_eq!(got, body_len);
        write_h2_frame(&mut stream, 1, 4, 1, &[0x88, 0x0f, 0x0d, 0x01, b'2']);
        write_h2_frame(&mut stream, 0, 1, 1, b"ok");
        let _ = read_h2_frame(&mut stream);
        got
    });
    let fifo_writer = fifo.clone();
    let writer = std::thread::spawn(move || {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(&fifo_writer)
            .unwrap();
        let first = vec![b'x'; 8 * 1024];
        file.write_all(&first).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !accepted.load(Ordering::SeqCst) {
            assert!(
                Instant::now() < deadline,
                "H2 client buffered the upload before connecting"
            );
            std::thread::yield_now();
        }
        let rest = vec![b'y'; body_len - first.len()];
        file.write_all(&rest).unwrap();
    });

    let shown = dir.join("seed.jet");
    let source = "use core.http.client as http\nfn run() { req :: http.request(\"GET\", \"http://127.0.0.1/\") }\n";
    fs::write(&shown, source).unwrap();
    let link = jet::compile_with_path(source, shown.to_str().unwrap())
        .unwrap()
        .ffi
        .expect("HTTP client bridge");
    let harness = dir.join("h2_upload.rs");
    let bin = dir.join("h2_upload");
    let fifo_path = fifo.display().to_string();
    fs::write(
        &harness,
        format!(
            r#"
use std::io::Read;
fn main() {{
    let url = std::env::args().nth(1).unwrap();
    let path = std::env::args().nth(2).unwrap();
    let root = bridge::jet_http_client_new_impl();
    let client = bridge::jet_http_client_protocols_impl(root, true, false, true).unwrap();
    let mut file = std::fs::File::open(path).unwrap();
    let mut offset = 0usize;
    let total = {body_len}usize;
    let mut body_read = || -> Result<Option<Vec<u8>>, bridge::JetHttpBridgeError> {{
        if offset >= total {{
            return Ok(None);
        }}
        let want = (64 * 1024).min(total - offset);
        let mut chunk = vec![0; want];
        let mut filled = 0usize;
        while filled < want {{
            let read = file.read(&mut chunk[filled..]).map_err(|_| bridge::JetHttpBridgeError::Io)?;
            if read == 0 {{
                break;
            }}
            filled += read;
        }}
        if filled == 0 {{
            return Ok(None);
        }}
        chunk.truncate(filled);
        offset += filled;
        Ok(Some(chunk))
    }};
    let response = bridge::jet_http_client_send_with_stream_impl(
        client, "POST", &url, &[], Some(total as i64), true, &mut body_read,
        None, None, None, None, Some(0), None, &[], &[], &[],
    ).unwrap();
    assert_eq!(bridge::jet_http_client_body_read_impl(response.1, 8).unwrap(), Some(b"ok".to_vec()));
    assert_eq!(bridge::jet_http_client_response_protocol_impl(response.1), "HTTP/2");
}}
"#,
        ),
    )
    .unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args([
        "--edition",
        "2021",
        harness.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    rustc
        .arg("--extern")
        .arg(format!("bridge={}", link.rlib_path.display()));
    for dependency in link.dependency_dirs().filter(|path| path.is_dir()) {
        rustc
            .arg("-L")
            .arg(format!("dependency={}", dependency.display()));
    }
    let built = rustc.output().unwrap();
    assert!(
        built.status.success(),
        "bridge H2 upload harness compile failed:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let output = Command::new(&bin)
        .arg(format!("http://{addr}/h2"))
        .arg(&fifo_path)
        .output()
        .unwrap();
    let got = server.join().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "bridge H2 upload harness failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(got, body_len);
    assert!(
        seen_data.load(Ordering::SeqCst),
        "server never saw streamed H2 DATA before EOF"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn public_client_proxy_none_ignores_environment_proxy() {
    let origin = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin_addr = origin.local_addr().unwrap();
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_addr = proxy.local_addr().unwrap();
    let origin_server = std::thread::spawn(move || {
        let (mut stream, _) = origin.accept().unwrap();
        let head = request_head(&mut stream);
        assert!(
            String::from_utf8_lossy(&head).starts_with("GET /direct HTTP/1.1\r\n"),
            "expected direct origin request, got {}",
            String::from_utf8_lossy(&head)
        );
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\ndirect")
            .unwrap();
    });
    let proxy_server = std::thread::spawn(move || {
        proxy.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_millis(400);
        loop {
            match proxy.accept() {
                Ok(_) => panic!("environment proxy should not receive traffic"),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::yield_now();
                }
                Err(error) => panic!("proxy accept failed: {error}"),
            }
        }
    });
    let src = format!(
        r#"
use core.http.client as http
fn run() {{
    client :: http.Client.new().proxy(.None)
    resp :: client.send(http.request("GET", "http://{origin_addr}/direct")) ?? panic("send")
    print(resp.body().text(16) ?? panic("body"))
}}
"#
    );
    let dir = common::unique_tmp("jet_http_client_law");
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join("public_proxy_none.jet");
    fs::write(&jet_path, &src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(&src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, &src, &diags)
        )
    });
    let rs = dir.join("public_proxy_none.rs");
    let bin = dir.join("public_proxy_none");
    fs::write(&rs, &out.rust).unwrap();
    let mut rustc_cmd = Command::new("rustc");
    rustc_cmd.args([
        "--edition",
        "2021",
        rs.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    if let Some(link) = &out.ffi {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let rustc = rustc_cmd.output().unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code (I2 violation):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin)
        .env("http_proxy", format!("http://{proxy_addr}"))
        .env("HTTP_PROXY", format!("http://{proxy_addr}"))
        .env("all_proxy", format!("http://{proxy_addr}"))
        .env("ALL_PROXY", format!("http://{proxy_addr}"))
        .env_remove("no_proxy")
        .env_remove("NO_PROXY")
        .output()
        .unwrap();
    origin_server.join().unwrap();
    proxy_server.join().unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(run.status.code().unwrap_or(0), 0, "stderr:\n{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "direct\n");
}

#[test]
fn public_client_proxy_url_sends_absolute_form() {
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_addr = proxy.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = proxy.accept().unwrap();
        let head = request_head(&mut stream);
        let text = String::from_utf8_lossy(&head);
        assert!(
            text.starts_with("GET http://example.invalid/via-proxy HTTP/1.1\r\n"),
            "expected absolute-form proxy request, got {text}"
        );
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nproxy")
            .unwrap();
    });
    let src = format!(
        r#"
use core.http.client as http
fn run() {{
    client :: http.Client.new().proxy(.Url("http://{proxy_addr}"))
    resp :: client.send(http.request("GET", "http://example.invalid/via-proxy")) ?? panic("send")
    print(resp.body().text(16) ?? panic("body"))
}}
"#
    );
    let (code, stdout, stderr) =
        common::build_and_run("jet_http_client_law", "public_proxy_url", &src);
    server.join().unwrap();
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "proxy\n");
}

#[test]
fn public_client_tls_custom_only_trust() {
    use std::process::{Command, Stdio};

    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ca = root.join("tests/fixtures/tls/smtp.ca.cert.pem");
    let wrong_ca = root.join("tests/fixtures/tls/localhost.cert.pem");
    let cert = root.join("tests/fixtures/tls/smtp.server.cert.pem");
    let key = root.join("tests/fixtures/tls/smtp.server.key.pem");
    let mut server = Command::new("openssl")
        .args([
            "s_server",
            "-quiet",
            "-www",
            "-accept",
            &port.to_string(),
            "-cert",
        ])
        .arg(&cert)
        .arg("-key")
        .arg(&key)
        .arg("-CAfile")
        .arg(&ca)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("openssl s_server");
    std::thread::sleep(Duration::from_millis(250));

    let empty_src = r#"
use core.tls as tls
fn run() {
    empty: [U8] :: []
    if tls.RootCertificates.from_pem(empty) == {
        Ok(_) -> print("empty-ok")
        Err(_) -> print("empty-fail")
    }
}
"#;
    let (empty_code, empty_out, empty_err) =
        common::build_and_run("jet_http_client_law", "public_tls_empty_root", empty_src);
    assert_eq!(empty_code, 0, "stderr:\n{empty_err}");
    assert_eq!(empty_out, "empty-fail\n");

    let wrong_src = format!(
        r#"
use core.http.client as http
use core.tls as tls
use core.files as fs
fn run() {{
    pem :: fs.read_bytes("{wrong}") ?? panic("wrong ca")
    roots :: tls.RootCertificates.from_pem(pem) ?? panic("roots")
    cfg :: tls.ClientConfig.default().with_trust(.CustomOnly(roots)) ?? panic("cfg")
    client :: http.Client.new().tls(cfg).protocols(false, true, false)
    if client.send(http.request("GET", "https://localhost:{port}/")) == {{
        Ok(_) -> print("ok")
        Err(_) -> print("tls-fail")
    }}
}}
"#,
        wrong = wrong_ca.display(),
        port = port
    );
    let (wrong_code, wrong_out, wrong_err) =
        common::build_and_run("jet_http_client_law", "public_tls_wrong_root", &wrong_src);
    assert_eq!(wrong_code, 0, "stderr:\n{wrong_err}");
    assert_eq!(wrong_out, "tls-fail\n");

    let pass_src = format!(
        r#"
use core.http.client as http
use core.tls as tls
use core.files as fs
fn run() {{
    pem :: fs.read_bytes("{ca}") ?? panic("ca")
    roots :: tls.RootCertificates.from_pem(pem) ?? panic("roots")
    cfg :: tls.ClientConfig.default().with_trust(.CustomOnly(roots)) ?? panic("cfg")
    client :: http.Client.new().tls(cfg).protocols(false, true, false)
    resp :: client.send(http.request("GET", "https://localhost:{port}/")) ?? panic("send")
    print(resp.status())
}}
"#,
        ca = ca.display(),
        port = port
    );
    let (pass_code, pass_out, pass_err) =
        common::build_and_run("jet_http_client_law", "public_tls_custom_only", &pass_src);
    let _ = server.kill();
    let _ = server.wait();
    assert_eq!(pass_code, 0, "stderr:\n{pass_err}");
    assert_eq!(pass_out, "200\n");
}

#[test]
fn https_to_http_redirect_denied_unless_allow_http_downgrade() {
    let dir = std::env::temp_dir().join(format!(
        "jet_http_client_law_downgrade_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let shown = dir.join("seed.jet");
    let source =
        "use core.http.client as http\nfn run() { req :: http.request(\"GET\", \"https://127.0.0.1/\") }\n";
    fs::write(&shown, source).unwrap();
    let link = jet::compile_with_path(source, shown.to_str().unwrap())
        .unwrap()
        .ffi
        .expect("HTTP client bridge");
    let harness = dir.join("bridge_https_downgrade.rs");
    let bin = dir.join("bridge_https_downgrade");
    fs::write(
        &harness,
        r#"
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn pem(path: &str) -> Vec<u8> {
    let text = std::fs::read_to_string(path).unwrap();
    let encoded: String = text.lines().filter(|line| !line.starts_with("-----")).collect();
    let mut out = Vec::new();
    let mut bits = 0u32;
    let mut count = 0u8;
    for byte in encoded.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => continue,
        };
        bits = bits << 6 | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1u32 << count).wrapping_sub(1);
        }
    }
    out
}

fn request_head(stream: &mut impl Read) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chunk = [0; 512];
    while !out.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut chunk).unwrap();
        assert_ne!(read, 0);
        out.extend_from_slice(&chunk[..read]);
    }
    out
}

fn main() {
    let cert = pem(&std::env::args().nth(1).unwrap());
    let key = pem(&std::env::args().nth(2).unwrap());
    let root_cert = pem(&std::env::args().nth(3).unwrap());
    let _ = rustls::crypto::ring::default_provider().install_default();

    let http = TcpListener::bind("127.0.0.1:0").unwrap();
    let http_addr = http.local_addr().unwrap();
    let http_server = thread::spawn(move || {
        let (mut stream, _) = http.accept().unwrap();
        let _ = request_head(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .unwrap();
    });

    let mut server = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(cert)],
            rustls::pki_types::PrivateKeyDer::Pkcs8(
                rustls::pki_types::PrivatePkcs8KeyDer::from(key),
            ),
        )
        .unwrap();
    server.alpn_protocols = vec![b"http/1.1".to_vec()];
    let server = Arc::new(server);
    let https = TcpListener::bind("127.0.0.1:0").unwrap();
    let https_addr = https.local_addr().unwrap();
    let location = format!("http://{http_addr}/ok");
    let https_server = thread::spawn(move || {
        for _ in 0..2 {
            let (socket, _) = https.accept().unwrap();
            socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let connection = rustls::ServerConnection::new(server.clone()).unwrap();
            let mut stream = rustls::StreamOwned::new(connection, socket);
            let _ = request_head(&mut stream);
            let body = format!(
                "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(body.as_bytes()).unwrap();
            stream.flush().unwrap();
        }
    });

    let root = bridge::jet_http_client_new_impl();
    let rooted = bridge::jet_http_client_root_certificate_impl(root, &root_cert, false).unwrap();
    let client = bridge::jet_http_client_protocols_impl(rooted, false, true, false).unwrap();
    let url = format!("https://localhost:{}/from", https_addr.port());
    let denied = bridge::jet_http_client_send_with_impl(
        client, "GET", &url, &[], None, None, None, None, None, None, None, &[], &[], &[],
    )
    .unwrap_err();
    assert!(matches!(denied, bridge::JetHttpBridgeError::Redirect));

    let allowed_root = bridge::jet_http_client_new_impl();
    let allowed_rooted =
        bridge::jet_http_client_root_certificate_impl(allowed_root, &root_cert, false).unwrap();
    let allowed_proto =
        bridge::jet_http_client_protocols_impl(allowed_rooted, false, true, false).unwrap();
    let allowed =
        bridge::jet_http_client_allow_http_downgrade_impl(allowed_proto, true).unwrap();
    let response = bridge::jet_http_client_send_with_impl(
        allowed, "GET", &url, &[], None, None, None, None, None, None, None, &[], &[], &[],
    )
    .unwrap();
    assert_eq!(response.0, 200);
    assert_eq!(
        bridge::jet_http_client_body_read_impl(response.1, 8).unwrap(),
        Some(b"ok".to_vec())
    );
    https_server.join().unwrap();
    http_server.join().unwrap();
}
"#,
    )
    .unwrap();
    let dependency_dirs: Vec<_> = link
        .dependency_dirs()
        .filter(|path| path.is_dir())
        .collect();
    let rustls = dependency_dirs
        .iter()
        .flat_map(|directory| fs::read_dir(directory).unwrap())
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("librustls-") && name.ends_with(".rlib"))
        })
        .expect("generated bridge rustls dependency");
    let mut rustc = Command::new("rustc");
    rustc.args([
        "--edition",
        "2021",
        harness.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    rustc
        .arg("--extern")
        .arg(format!("bridge={}", link.rlib_path.display()))
        .arg("--extern")
        .arg(format!("rustls={}", rustls.display()));
    for dependency in &dependency_dirs {
        rustc
            .arg("-L")
            .arg(format!("dependency={}", dependency.display()));
    }
    let built = rustc.output().unwrap();
    assert!(
        built.status.success(),
        "bridge downgrade harness compile failed:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let output = Command::new(&bin)
        .arg(fs::canonicalize("tests/fixtures/tls/smtp.server.cert.pem").unwrap())
        .arg(fs::canonicalize("tests/fixtures/tls/smtp.server.key.pem").unwrap())
        .arg(fs::canonicalize("tests/fixtures/tls/smtp.ca.cert.pem").unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "bridge downgrade harness failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn ambient_context_deadline_upper_bounds_client_total_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = request_head(&mut stream);
        std::thread::sleep(Duration::from_millis(250));
        let _ = stream.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nlate",
        );
    });
    let src = format!(
        r#"
use core.http.client as http
use core.time as time
fn run() {{
    client :: http.Client.new().timeouts(5000, 5000, 5000, 5000, 5000, 5000, 5000)
    @Context(deadline: time.now() + 40) {{
        if client.send(http.request("GET", "http://{addr}/slow")) == {{
            Ok(_) -> print("ok")
            Err(_) -> print("timeout")
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) =
        common::build_and_run("jet_http_client_law", "ambient_deadline", &src);
    server.join().unwrap();
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "timeout\n");
}

#[test]
fn public_client_allow_http_downgrade_method_typechecks() {
    let src = r#"
use core.http.client as http
fn run() {
    _ :: http.Client.new().allow_http_downgrade(true).allow_http_downgrade(false)
}
"#;
    let (code, _stdout, stderr) =
        common::build_and_run("jet_http_client_law", "allow_http_downgrade_type", src);
    assert_eq!(code, 0, "stderr:\n{stderr}");
}
