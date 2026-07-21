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
    ).unwrap_err(), bridge::JetHttpBridgeError::Protocol);
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
        let request = loop {
            let frame = read_h2_frame(&mut stream);
            if frame.0 == 6 && frame.1 & 1 == 0 {
                write_h2_frame(&mut stream, 6, 1, 0, &frame.3);
                continue;
            }
            if frame.0 == 1 {
                break frame;
            }
        };
        assert_eq!(request.2, 3);
        assert_ne!(request.1 & 4, 0);
        write_h2_frame(&mut stream, 1, 4, 3, &response);
        // Deliver stream 3 first. The client reading stream 1 must queue it, not block multiplexing.
        write_h2_frame(&mut stream, 0, 1, 3, b"b2");
        write_h2_frame(&mut stream, 0, 1, 1, b"a1");
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
    let first = bridge::jet_http_client_send_with_impl(
        client, "GET", &url, &[], None, None, None, None, None, None, None, &[], &[], &[],
    ).unwrap();
    let second = bridge::jet_http_client_send_with_impl(
        client, "GET", &url, &[], None, None, None, None, None, None, None, &[], &[], &[],
    ).unwrap();
    assert_eq!(bridge::jet_http_client_body_read_impl(first.1, 64).unwrap(), Some(b"a1".to_vec()));
    assert_eq!(bridge::jet_http_client_body_read_impl(second.1, 64).unwrap(), Some(b"b2".to_vec()));
    assert_eq!(bridge::jet_http_client_body_read_impl(first.1, 64).unwrap(), None);
    assert_eq!(bridge::jet_http_client_body_read_impl(second.1, 64).unwrap(), None);
    let third = bridge::jet_http_client_send_with_impl(
        client, "GET", &url, &[], None, None, None, None, None, None, None, &[], &[], &[],
    ).unwrap();
    assert_eq!(bridge::jet_http_client_body_read_impl(third.1, 64).unwrap(), Some(b"c3".to_vec()));
    assert_eq!(bridge::jet_http_client_body_read_impl(third.1, 64).unwrap(), None);
    assert_eq!(bridge::jet_http_client_response_protocol_impl(first.1), "HTTP/2");
    assert_eq!(bridge::jet_http_client_response_protocol_impl(second.1), "HTTP/2");
    assert_eq!(bridge::jet_http_client_response_protocol_impl(third.1), "HTTP/2");
    assert!(!bridge::jet_http_client_response_reused_impl(first.1));
    assert!(bridge::jet_http_client_response_reused_impl(second.1));
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
