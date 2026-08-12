#[test]
fn core_email_policy_envelope_and_reports_are_real_jet_values() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_email_policy_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.email as email

fn error_text(problem: email.EmailError) => String {
    if problem == {
        .Configuration(_, _, _, _) -> { return "matched" }
        .TLS(_, _, _, _) -> { return "tls-error" }
    }
    return "unknown"
}

fn run() {
    sender :: email.address("sender@example.com") ?? panic("sender")
    visible :: email.address("visible@example.net") ?? panic("visible")
    hidden :: email.address("hidden@example.org") ?? panic("hidden")
    message :: email.message(~sender, [~visible], [~hidden], "subject", "body", "", []) ?? panic("message")
    original_bytes :: email.serialize(~message) ?? panic("serialize original")
    default_envelope :: message.envelope()
    envelope :: email.envelope(sender, [~hidden]) ?? panic("envelope")
    replaced :: message.with_envelope(envelope) ?? panic("replace")
    bytes :: email.serialize(replaced) ?? panic("serialize")
    start_tls :: email.SMTPSecurity.StartTls
    transport_tls :: email.SMTPSecurity.TLS
    require_all :: email.RecipientPolicy.RequireAll
    recipient :: email.RecipientReport.{
        address: hidden,
        accepted: true,
        code: 250,
        message: "accepted",
    }
    report :: email.SendReport.{
        server: "smtp.example.com",
        accepted: [recipient],
        rejected: [],
        response_code: 250,
        response: "queued",
        accepted_at: "2026-07-13T17:00:00Z",
    }
    problem :: EmailError.{ .Configuration.{
        operation: "send",
        server: Val("smtp.example.com"),
        code: Val(451),
        reason: "stopped",
    } }
    tls_problem :: EmailError.{ .TLS.{
        operation: "handshake",
        server: Val("smtp.example.com"),
        code: Val(525),
        reason: "certificate",
    } }
    print(start_tls == .StartTls)
    print(transport_tls == .TLS)
    print(require_all == .RequireAll)
    print(default_envelope.recipients.len())
    print(original_bytes == bytes)
    print(report.server)
    print(report.accepted.len())
    print(error_text(problem))
    print(error_text(tls_problem))
    print(bytes.len() > 0)
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "email_policy", src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "true\ntrue\ntrue\n2\ntrue\nsmtp.example.com\n1\nmatched\ntls-error\ntrue\n");
    let file = dir.join("email_policy.jet");
    fs::write(&file, src).unwrap();
    match jet::Interpreter::dev_iteration(file.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("email policy default-dev failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_client_accepts_typed_url_in_codegen() {
    let out = compile_temp(
        "http_url.jet",
        r#"
use core.http.client as http
use core.url as url

fn run() {
    u :: url.parse("https://example.com/a") ?? panic("bad url")
    req :: http.request("GET", u).timeout(1)
}
"#,
    );
    assert!(
        out.rust.contains(".to_string_value()"),
        "typed Url should render to String at HTTP boundary:\n{}",
        out.rust
    );
}

#[test]
fn core_http_client_preserves_repeated_headers_over_a_socket() {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = [0; 4096];
        let read = stream.read(&mut bytes).unwrap();
        let request = String::from_utf8_lossy(&bytes[..read]);
        let warnings = request
            .lines()
            .filter(|line| line.to_ascii_lowercase().starts_with("warning:"))
            .collect::<Vec<_>>()
            .join("\n");
        let first = warnings.find("one").expect("first Warning value");
        let second = warnings.find("two").expect("second Warning value");
        assert!(first < second, "repeated Warning values changed: {request}");
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nX-A: one\r\nX-B: middle\r\nX-A: two\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\nConnection: close\r\n\r\n\xff\0",
            )
            .unwrap();
    });

    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_headers_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let compiled = compile_temp(
        "http_bridge_seed.jet",
        "use core.http.client as http\nfn run() { req :: http.request(\"GET\", \"http://127.0.0.1/\") }\n",
    );
    let link = compiled.ffi.expect("HTTP client bridge");
    let harness = dir.join("bridge_headers.rs");
    let bin = dir.join("bridge_headers");
    fs::write(
        &harness,
        r#"
fn main() {
    let url = std::env::args().nth(1).unwrap();
    let request_headers = vec![
        "Warning".to_string(), "one".to_string(),
        "Warning".to_string(), "two".to_string(),
    ];
    let (_, body, _, headers) = bridge::jet_http_client_send_impl(
        "GET", &url, &request_headers, None, None, None, None, None, None, None, None, None, None, None,
        &[], &[], &[],
    ).unwrap();
    assert_eq!(
        bridge::jet_http_client_body_read_impl(body, 8).unwrap(),
        Some(vec![255, 0]),
    );
    assert_eq!(bridge::jet_http_client_body_read_impl(body, 8).unwrap(), None);
    let selected = headers.chunks_exact(2)
        .filter(|pair| matches!(pair[0].as_str(), "x-a" | "x-b" | "set-cookie"))
        .flat_map(|pair| [pair[0].clone(), pair[1].clone()])
        .collect::<Vec<_>>();
    assert_eq!(selected, vec![
        "x-a", "one", "x-b", "middle", "x-a", "two",
        "set-cookie", "a=1", "set-cookie", "b=2",
    ]);
}
"#,
    )
    .unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args(["--edition", "2021", harness.to_str().unwrap(), "-o", bin.to_str().unwrap()]);
    rustc.arg("--extern").arg(format!("bridge={}", link.rlib_path.display()));
    for dependency in link.dependency_dirs().filter(|path| path.is_dir()) {
        rustc.arg("-L").arg(format!("dependency={}", dependency.display()));
    }
    let built = rustc.output().unwrap();
    assert!(built.status.success(), "bridge harness compile failed:\n{}", String::from_utf8_lossy(&built.stderr));
    let output = Command::new(&bin).arg(format!("http://{addr}/")).output().unwrap();
    server.join().unwrap();
    assert!(output.status.success(), "bridge harness failed:\n{}", String::from_utf8_lossy(&output.stderr));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_client_exposes_binary_body_once() {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        stream.read(&mut request).unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nX-A: one\r\nX-A: two\r\nConnection: close\r\n\r\n\0\xff\x01")
            .unwrap();
    });
    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_binary_body_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = format!(
        r#"
use core.http.client as client

fn run() {{
    response :: client.get("http://{addr}/") ?? panic("request")
    values :: response.headers.all("x-a")
    print(values.len())
    print(values[0])
    print(values[1])
    body :: response.body()
    bytes :: body.bytes(8) ?? panic("body")
    print(bytes.len())
    print(bytes[0])
    print(bytes[1])
    second :: body.bytes(8)
    if second == {{
        .Ok(_) -> {{ print("reused") }}
        .Err(error) -> {{
            if error == {{
                .BodyConsumed -> {{ print("consumed") }}
                else -> {{ print("wrong-error") }}
            }}
        }}
    }}
}}
"#,
    );
    let (code, stdout, stderr) = build_and_run(&dir, "http_binary_body", &source, &[], None);
    server.join().unwrap();
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "2\none\ntwo\n3\n0\n255\nconsumed\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_nominal_message_and_body_surface_is_executable() {
    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_nominal_surface_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("input");
    let output = dir.join("output");
    fs::write(&input, b"reader").unwrap();
    let source = format!(
        r#"
use core.http as http
use core.mime as mime
use core.files as files

fn run() {{
    print(http.Method.custom("PURGE") ?? panic("method"))
    print(http.Status.new(299) ?? panic("status"))
    print(http.Version.http_1_1())
    print(http.HeaderName.new("X-Test") ?? panic("name"))
    print(http.HeaderValue.new("ok") ?? panic("value"))
    print(http.Body.empty().bytes(1) ?? panic("empty"))
    print(http.Body.bytes([0, 255]).bytes(2) ?? panic("bytes"))
    print(http.Body.text("hello").text(5) ?? panic("text"))
    print(http.Body.text("hello", mime.parse("text/custom") ?? panic("mime")).text(5) ?? panic("custom"))
    print(http.Body.form(["a": "b"]).text(16) ?? panic("form"))
    print(http.Body.json(42).json<Int>(16) ?? panic("json"))
    input :: files.open("{input}") ?? panic("open")
    body :: http.Body.reader(^input, 6) ?? panic("reader")
    output :: files.create("{output}") ?? panic("create")
    print(body.copy_to(^output, 6) ?? panic("copy"))
}}
"#,
        input = jet_string_path(&input),
        output = jet_string_path(&output),
    );
    let (code, stdout, stderr) = build_and_run(&dir, "http_nominal_surface", &source, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "PURGE\n299\nHTTP/1.1\nX-Test\nok\n[]\n[0, 255]\nhello\nhello\na=b\n42\n6\n"
    );
    assert_eq!(fs::read(output).unwrap(), b"reader");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_client_multipart_boundary_does_not_collide_with_fields() {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        loop {
            let mut chunk = [0; 4096];
            let read = stream.read(&mut chunk).unwrap();
            assert_ne!(read, 0, "multipart request ended before its body");
            request.extend_from_slice(&chunk[..read]);
            let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
            else {
                continue;
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
            )
            .unwrap();
        String::from_utf8(request).unwrap()
    });

    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_multipart_boundary_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let compiled = compile_temp(
        "http_multipart_seed.jet",
        "use core.http.client as http\nfn run() { req :: http.request(\"POST\", \"http://127.0.0.1/\") }\n",
    );
    let link = compiled.ffi.expect("HTTP client bridge");
    let harness = dir.join("bridge_multipart_boundary.rs");
    let bin = dir.join("bridge_multipart_boundary");
    fs::write(
        &harness,
        r#"
fn main() {
    let url = std::env::args().nth(1).unwrap();
    let long_candidate = format!("jet-http-boundary{}", "-".repeat(53));
    let candidates = (0u64..300)
        .map(|suffix| format!("jet-http-boundary-{suffix:016x}"))
        .collect::<String>();
    let line_break_name =
        format!("safe\"\r\nX-Extra: yes\r\n{long_candidate}{candidates}");
    let multipart = vec![
        line_break_name,
        format!("before\r\n--{long_candidate}\r\n{candidates}\r\nafter"),
    ];
    let response = bridge::jet_http_client_send_impl(
        "POST", &url, &[], None, None, None, None, None, None, None, None, None, None, None,
        &[], &[], &multipart,
    ).unwrap();
    let body = bridge::jet_http_client_body_read_impl(response.1, 8).unwrap().unwrap();
    assert_eq!((response.0, body.as_slice()), (200, b"ok".as_slice()));
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
        .arg(format!("http://{addr}/"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "bridge harness failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = server.join().unwrap();
    let (headers, body) = request.split_once("\r\n\r\n").unwrap();
    let boundary = headers
        .lines()
        .find_map(|line| line.strip_prefix("content-type: multipart/form-data; boundary="))
        .unwrap();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    let long_candidate = format!("jet-http-boundary{}", "-".repeat(53));
    let candidates = (0u64..300)
        .map(|suffix| format!("jet-http-boundary-{suffix:016x}"))
        .collect::<String>();
    let raw_field_name = format!("safe\"\r\nX-Extra: yes\r\n{long_candidate}{candidates}");
    let field_name = format!(
        "safe%22%0D%0AX-Extra: yes%0D%0A{long_candidate}{candidates}"
    );
    let field_value = format!("before\r\n--{long_candidate}\r\n{candidates}\r\nafter");
    assert!((1..=70).contains(&boundary.len()), "invalid boundary length: {boundary}");
    assert!(boundary.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'));
    assert_eq!(boundary, "jet-http-boundary-000000000000012c");
    assert!(!raw_field_name.contains(boundary), "multipart name collided");
    assert!(!field_value.contains(boundary), "multipart value collided");
    let (part_headers, _) = body
        .strip_prefix(&format!("--{boundary}\r\n"))
        .unwrap()
        .split_once("\r\n\r\n")
        .unwrap();
    assert_eq!(
        part_headers,
        format!("Content-Disposition: form-data; name=\"{field_name}\"")
    );
    assert_eq!(
        part_headers.lines().count(),
        1,
        "multipart field name produced extra header lines"
    );
    assert_eq!(content_length, body.len());
    assert_eq!(
        body,
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{field_name}\"\r\n\r\n{field_value}\r\n--{boundary}--\r\n"
        )
    );
    assert_eq!(body.matches(&format!("--{boundary}")).count(), 2);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_client_owns_pre_response_errors() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let accepted = std::sync::Arc::new(AtomicUsize::new(0));
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let server_stop = stop.clone();
    let server_accepted = accepted.clone();
    let server_captured = captured.clone();
    let server = std::thread::spawn(move || {
        while !server_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let request_index = server_accepted.fetch_add(1, Ordering::AcqRel);
                    stream.set_read_timeout(Some(std::time::Duration::from_secs(1))).unwrap();
                    let mut request = [0; 4096];
                    let read = stream.read(&mut request).unwrap();
                    server_captured.lock().unwrap().push(request[..read].to_vec());
                    let response: Option<&[u8]> = match request_index {
                        0 => Some(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
                        1 => Some(b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"jet\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
                        _ => None,
                    };
                    if let Some(response) = response {
                        stream.write_all(response).unwrap();
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        }
    });

    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_timeout_range_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let compiled = compile_temp(
        "http_timeout_seed.jet",
        "use core.http.client as http\nfn run() { req :: http.request(\"GET\", \"http://127.0.0.1/\") }\n",
    );
    let link = compiled.ffi.expect("HTTP client bridge");
    let harness = dir.join("bridge_timeout_range.rs");
    let bin = dir.join("bridge_timeout_range");
    fs::write(
        &harness,
        r#"
fn main() {
    std::env::set_var("NO_PROXY", "127.0.0.1");
    std::env::set_var("no_proxy", "127.0.0.1");
    let url = std::env::args().nth(1).unwrap();
    let cases = [
        (Some(-1), None, None, None),
        (None, Some(-1), None, None),
        (None, None, Some(-1), None),
        (None, None, None, Some(-1)),
    ];
    let errors = cases.into_iter().map(|(timeout, connect, read, total)| {
        bridge::jet_http_client_send_impl(
            "GET", &url, &[], None, timeout, connect, read, total, None, None, None, None, None, None,
            &[], &[], &[],
        ).err()
    }).collect::<Vec<_>>();
    assert!(errors.into_iter().all(|error| matches!(error, Some(bridge::JetHTTPBridgeError::Timeout))));
    let unsupported_url = url.replacen("http://", "ftp://", 1);
    let url_errors = ["http://[".to_string(), unsupported_url].map(|url| {
        bridge::jet_http_client_send_impl(
            "GET", &url, &[], None, None, None, None, None, None, None, None, None, None, None,
            &[], &[], &[],
        ).unwrap_err()
    });
    assert!(url_errors.into_iter().all(|error| matches!(error, bridge::JetHTTPBridgeError::InvalidUrl)));
    let refused_url = "http://127.0.0.1:0/".to_string();
    let connection_error = bridge::jet_http_client_send_impl(
        "GET", &refused_url, &[], None, None, None, None, None, None, None, None, None, None, None,
        &[], &[], &[],
    ).unwrap_err();
    assert!(matches!(connection_error, bridge::JetHTTPBridgeError::Connect));
    let proxy_error = bridge::jet_http_client_send_impl(
        "GET", &url, &[], None, None, None, None, None, None, None, None, None, None, Some("ftp://proxy.invalid"),
        &[], &[], &[],
    ).unwrap_err();
    assert!(matches!(proxy_error, bridge::JetHTTPBridgeError::Proxy));
    let proxy_connection_error = bridge::jet_http_client_send_impl(
        "GET", &"https://example.invalid/".to_string(), &[], None, None, None, None, None, None, None, None, None, None,
        Some(url.as_str()),
        &[], &[], &[],
    ).unwrap_err();
    assert!(matches!(proxy_connection_error, bridge::JetHTTPBridgeError::Proxy));
    let proxy_auth_error = bridge::jet_http_client_send_impl(
        "GET", &"https://auth.invalid/".to_string(), &[], None, None, None, None, None, None, None, None, None, None,
        Some(url.as_str()),
        &[], &[], &[],
    ).unwrap_err();
    assert!(matches!(proxy_auth_error, bridge::JetHTTPBridgeError::Proxy));
    let io_error = bridge::jet_http_client_send_impl(
        "GET", &format!("{url}io"), &[], None, None, None, None, None, None, None, None, None, None, None,
        &[], &[], &[],
    ).unwrap_err();
    assert!(matches!(io_error, bridge::JetHTTPBridgeError::IO));
}
"#,
    )
    .unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args(["--edition", "2021", harness.to_str().unwrap(), "-o", bin.to_str().unwrap()]);
    rustc.arg("--extern").arg(format!("bridge={}", link.rlib_path.display()));
    for dependency in link.dependency_dirs().filter(|path| path.is_dir()) {
        rustc.arg("-L").arg(format!("dependency={}", dependency.display()));
    }
    let built = rustc.output().unwrap();
    assert!(built.status.success(), "bridge harness compile failed:\n{}", String::from_utf8_lossy(&built.stderr));
    let output = Command::new(&bin).arg(format!("http://{addr}/")).output().unwrap();
    stop.store(true, Ordering::Release);
    server.join().unwrap();
    assert!(output.status.success(), "bridge harness failed:\n{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(accepted.load(Ordering::Acquire), 3, "pre-response transport count changed");
    let requests = captured.lock().unwrap();
    assert!(
        requests[0].starts_with(b"CONNECT example.invalid:443 HTTP/1.1\r\n")
            && requests[1].starts_with(b"CONNECT auth.invalid:443 HTTP/1.1\r\n")
            && requests[2].starts_with(b"GET /io HTTP/1.1\r\n"),
        "unexpected requests: {:?}",
        requests
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_client_rejects_invalid_redirect_limits_before_transport() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let accepted = std::sync::Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let server_stop = stop.clone();
    let server_accepted = accepted.clone();
    let server_requests = requests.clone();
    let server = std::thread::spawn(move || {
        while !server_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    server_accepted.fetch_add(1, Ordering::AcqRel);
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
                        .unwrap();
                    let mut request = [0; 4096];
                    let read = stream.read(&mut request).unwrap();
                    let request = String::from_utf8_lossy(&request[..read]);
                    let target = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_ascii_whitespace().nth(1))
                        .unwrap();
                    server_requests.lock().unwrap().push(target.to_string());
                    let response = match target {
                        "/redirect" => "HTTP/1.1 302 Found\r\nLocation: /target\r\nContent-Length: 8\r\nConnection: close\r\n\r\nredirect".to_string(),
                        "/target" => "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_string(),
                        _ => {
                            let (chain, step) = target.rsplit_once('/').unwrap();
                            let step = step.parse::<usize>().unwrap();
                            let final_step = match chain {
                                "/within" => 10,
                                "/over" => 11,
                                _ => panic!("unexpected target {target}"),
                            };
                            if step == final_step {
                                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_string()
                            } else {
                                format!(
                                    "HTTP/1.1 302 Found\r\nLocation: {chain}/{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                                    step + 1
                                )
                            }
                        }
                    };
                    stream.write_all(response.as_bytes()).unwrap();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        }
    });

    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_redirect_range_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let compiled = compile_temp(
        "http_redirect_seed.jet",
        "use core.http.client as http\nfn run() { req :: http.request(\"GET\", \"http://127.0.0.1/\") }\n",
    );
    let link = compiled.ffi.expect("HTTP client bridge");
    let harness = dir.join("bridge_redirect_range.rs");
    let bin = dir.join("bridge_redirect_range");
    fs::write(
        &harness,
        r#"
fn main() {
    let base = std::env::args().nth(1).unwrap();
    let url = format!("{base}/redirect");
    let errors = [-1, i64::from(u32::MAX) + 1].into_iter().map(|redirects| {
        bridge::jet_http_client_send_impl(
            "GET", &url, &[], None, None, None, None, None, None, None, None, None, Some(redirects), None,
            &[], &[], &[],
        ).err()
    }).collect::<Vec<_>>();
    assert!(errors.into_iter().all(|error| matches!(error, Some(bridge::JetHTTPBridgeError::Redirect))));
    let stopped = bridge::jet_http_client_send_impl(
        "GET", &url, &[], None, None, None, None, None, None, None, None, None, Some(0), None,
        &[], &[], &[],
    ).unwrap();
    let stopped_body = bridge::jet_http_client_body_read_impl(stopped.1, 16).unwrap().unwrap();
    assert_eq!((stopped.0, stopped_body.as_slice()), (302, b"redirect".as_slice()));
    let followed = bridge::jet_http_client_send_impl(
        "GET", &url, &[], None, None, None, None, None, None, None, None, None,
        Some(i64::from(u32::MAX)), None, &[], &[], &[],
    ).unwrap();
    let followed_body = bridge::jet_http_client_body_read_impl(followed.1, 8).unwrap().unwrap();
    assert_eq!((followed.0, followed_body.as_slice()), (200, b"ok".as_slice()));
    let explicit = bridge::jet_http_client_send_impl(
        "GET", &url, &[], None, None, None, None, None, None, None, None, None, Some(1), None,
        &[], &[], &[],
    ).unwrap_err();
    assert!(matches!(explicit, bridge::JetHTTPBridgeError::Redirect));
    let within = bridge::jet_http_client_send_impl(
        "GET", &format!("{base}/within/0"), &[], None, None, None, None, None, None, None, None, None,
        None, None, &[], &[], &[],
    ).unwrap();
    let within_body = bridge::jet_http_client_body_read_impl(within.1, 8).unwrap().unwrap();
    assert_eq!((within.0, within_body.as_slice()), (200, b"ok".as_slice()));
    let over = bridge::jet_http_client_send_impl(
        "GET", &format!("{base}/over/0"), &[], None, None, None, None, None, None, None, None, None,
        None, None, &[], &[], &[],
    ).unwrap_err();
    assert!(matches!(over, bridge::JetHTTPBridgeError::Redirect));
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
        .arg(format!("http://{addr}"))
        .output()
        .unwrap();
    stop.store(true, Ordering::Release);
    server.join().unwrap();
    assert!(
        output.status.success(),
        "bridge harness failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        accepted.load(Ordering::Acquire),
        26,
        "invalid redirect limit reached transport or redirect boundary behavior changed"
    );
    let expected = ["/redirect", "/redirect", "/target", "/redirect"]
        .into_iter()
        .map(str::to_string)
        .chain((0..=10).map(|step| format!("/within/{step}")))
        .chain((0..=10).map(|step| format!("/over/{step}")))
        .collect::<Vec<_>>();
    assert_eq!(
        *requests.lock().unwrap(),
        expected,
        "redirect boundaries sent an unexpected request sequence"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_client_bounds_and_strictly_decodes_response_bodies() {
    use std::io::{Read, Write};

    const LIMIT: usize = 8 * 1024 * 1024;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let cases = [
            ("200 OK", vec![b'a'; LIMIT], false, None),
            ("200 OK", vec![0xff], false, None),
            ("404 Not Found", b"missing".to_vec(), false, None),
            ("413 Payload Too Large", vec![b'b'; LIMIT + 1], false, None),
            ("200 OK", vec![b'c'; LIMIT], true, None),
            ("413 Payload Too Large", vec![b'd'; LIMIT + 1], true, None),
            ("200 OK", b"no".to_vec(), false, Some(5)),
            ("502 Bad Gateway", b"no".to_vec(), true, Some(2)),
        ];
        for (status, body, chunked, claimed_len) in cases {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            stream.read(&mut request).unwrap();
            if chunked {
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",
                    claimed_len.unwrap_or(body.len())
                )
                .unwrap();
                let _ = stream.write_all(&body);
                if claimed_len.is_none() {
                    let _ = stream.write_all(b"\r\n0\r\n\r\n");
                } else {
                    let _ = stream.write_all(b"\r\n");
                }
            } else {
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    claimed_len.unwrap_or(body.len())
                )
                .unwrap();
                let _ = stream.write_all(&body);
            }
        }
        for response in [
            "NOT HTTP\r\nConnection: close\r\n\r\n".to_string(),
            format!("HTTP/1.1 200 OK\r\n{}\r\n", "X: y\r\n".repeat(102)),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            stream.read(&mut request).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
        }
    });

    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_body_bounds_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.http.client as client

fn run() {
    first :: client.get("http://__ADDR__/")
    if first == {
        .Ok(response) -> {
            if response.body().bytes(8388608) == {
                .Ok(bytes) -> { print(bytes.len()) }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    second :: client.get("http://__ADDR__/")
    if second == {
        .Ok(response) -> {
            if response.body().text(8388608) == {
                .Ok(text) -> { print("unexpected utf8 success: {text}") }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    third :: client.get("http://__ADDR__/")
    if third == {
        .Ok(response) -> {
            if response.body().text(8388608) == {
                .Ok(text) -> { print("{response.status()}:{text}") }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    fourth :: client.get("http://__ADDR__/")
    if fourth == {
        .Ok(response) -> {
            if response.body().bytes(8388608) == {
                .Ok(bytes) -> { print("unexpected oversized success: {bytes.len()}") }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    fifth :: client.get("http://__ADDR__/")
    if fifth == {
        .Ok(response) -> {
            if response.body().bytes(8388608) == {
                .Ok(bytes) -> { print(bytes.len()) }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    sixth :: client.get("http://__ADDR__/")
    if sixth == {
        .Ok(response) -> {
            if response.body().bytes(8388608) == {
                .Ok(bytes) -> { print("unexpected chunked oversized success: {bytes.len()}") }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    seventh :: client.get("http://__ADDR__/")
    if seventh == {
        .Ok(response) -> {
            if response.body().text(8388608) == {
                .Ok(text) -> { print("unexpected partial content-length success: {text}") }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    eighth :: client.get("http://__ADDR__/")
    if eighth == {
        .Ok(response) -> {
            if response.body().text(8388608) == {
                .Ok(text) -> { print("unexpected partial chunked success: {response.status()}:{text}") }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    ninth :: client.get("http://__ADDR__/")
    if ninth == {
        .Ok(response) -> { print("unexpected malformed status success: {response.status()}") }
        .Err(error) -> { print(error) }
    }
    tenth :: client.get("http://__ADDR__/")
    if tenth == {
        .Ok(response) -> { print("unexpected malformed header success: {response.status()}") }
        .Err(error) -> { print(error) }
    }
}
"#
    .replace("__ADDR__", &addr.to_string());
    let (code, stdout, stderr) = build_and_run(&dir, "http_body_bounds", &src, &[], None);
    server.join().unwrap();
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "8388608\nunsupported HTTP body encoding\n404:missing\nHTTP body exceeds 8388608-byte limit\n8388608\nHTTP body exceeds 8388608-byte limit\nHTTP I/O failed during transport\nHTTP I/O failed during transport\ninvalid HTTP framing\ninvalid HTTP header\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_server_public_response_appends_repeated_headers() {
    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_server_headers_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.http.client as client
use core.http.server as server
use core.net as net
use core.tasks as tasks

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("bind")
    addr :: listener.local_addr() ?? panic("address")
    mux :: server.mux()
    mux.get("/", (req: HTTPRequest) =>
        .Ok(server.response(200, "ok")
            .header("Set-Cookie", "a=1")
            .header("Set-Cookie", "b=2"))
    )
    serving :: tasks.spawn(() =>
        server.serve_once_listener(listener, mux) ?? panic("serve")
    )
    response :: client.get("http://{addr}/") ?? panic("get")
    cookies :: response.cookies()
    print(cookies.len())
    print(cookies[0])
    print(cookies[1])
    serving.join()
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "http_server_headers", src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "2\na=1\nb=2\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_data_typed_csv_group_stats_status_and_plot() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.data runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_data_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "data_core",
        r#"
use core.data as data

#Codable
struct Ticket {
    team: String
    minutes: Float
}

#Codable
struct Budget {
    team: String
    owner: String
}

fn must_stay_deferred(ticket: Ticket) => Bool {
    panic("lazy filter ran before collect")
    return false
}

fn missing_minutes() => Float? = None

fn run() {
    raw :: "team,minutes\nCore,4.0\nTools,5.0\nCore,8.0\nTools,7.0"
    rows :: data.csv<Ticket>(raw) ?? panic("bad csv")
    budget_raw :: "team,owner\nCore,Ada\nCore,Lin\nTools,Grace"
    budgets :: data.csv<Budget>(budget_raw) ?? panic("bad budget")
    print(data.count(rows))
    table :: data.table(rows)
    lazy :: data.lazy(table)
    deferred :: data.lazy_filter(lazy, (t) => must_stay_deferred(t))
    print(data.plan(deferred)[1])
    planned :: data.lazy_sort_by(data.lazy_filter(lazy, (t) => t.minutes >= 6.0), (t) => t.team)
    collected :: data.collect(planned) ?? panic("collect")
    print(data.count(table))
    print(data.count(planned))
    print(data.count(data.rows(collected)))
    print(data.plan(planned)[2])
    loop ticket, data.rows(collected) {
        print("planned:{ticket.team}:{ticket.minutes}")
    }
    maybe_minutes :: [ Val(2.0), missing_minutes(), Val(6.0), missing_minutes() ]
    series :: data.series(maybe_minutes)
    print(data.count(series))
    print(data.missing_count(series))
    groups :: data.group_mean(rows, (t) => t.team, (t) => t.minutes) ?? panic("group")
    loop g, groups {
        print("{g.key}:{g.count}:{g.sum}:{g.mean}")
    }
    values :: [2.0, 4.0, 6.0]
    print(data.sum(values) ?? panic("sum"))
    print(data.mean(values) ?? panic("mean"))
    joined :: data.inner_join(rows, budgets, (t) => t.team, (b) => b.team) ?? panic("join")
    loop pair, joined {
        print("{pair.left.team}:{pair.right.owner}")
    }
    left :: data.left_join(rows, [budgets[0]], (t) => t.team, (b) => b.team) ?? panic("left")
    loop pair, left {
        if pair.right == {
            Val(budget) -> print("{pair.left.team}:{budget.owner}")
            None -> print("{pair.left.team}:none")
        }
    }
    pivot :: data.pivot_sum(rows, (t) => t.team, (t) => if t.minutes >= 6.0 -> "long" else -> "short", (t) => t.minutes) ?? panic("pivot")
    loop cell, pivot {
        print("{cell.row_key}|{cell.column_key}:{cell.count}")
    }
    rolling :: data.rolling_mean([2.0, 4.0, 6.0], 2) ?? panic("rolling")
    print(rolling[2])
    counts :: data.group_count(rows, (t) => t.team) ?? panic("count")
    print(data.bar_text(counts) ?? panic("bar"))
    print((data.bar_svg(counts) ?? panic("svg")).len())
    status :: data.status()
    print("{status[0].step}:{status[0].path}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.data program failed: {stderr}");
    assert_eq!(
        stdout,
        "4\nfilter\n4\n2\n2\nsort_by\nplanned:Core:8.0\nplanned:Tools:7.0\n4\n2\nCore:2:12.0:6.0\nTools:2:12.0:6.0\n12.0\n4.0\nCore:Ada\nCore:Lin\nTools:Grace\nCore:Ada\nCore:Lin\nTools:Grace\nCore:Ada\nTools:none\nCore:Ada\nTools:none\nCore|long:1\nCore|short:1\nTools|long:1\nTools|short:1\n5.0\nCore | ## 2\nTools | ## 2\n531\ncore.data.csv:native\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_data_stream_limits_and_typed_errors() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.data stream test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_data_stream_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let csv_path = dir.join("events.csv");
    fs::write(&csv_path, "service,latency_ms\napi,10.0\napi,20.0\ndb,5.0\napi,30.0\n").unwrap();
    let path_lit = csv_path.to_string_lossy().replace('\\', "\\\\");
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "data_stream",
        &format!(
            r#"
use core.data as data
use core.files as files

#Codable
struct Event {{
    service: String
    latency_ms: Float
}}

fn run() {{
    input :: files.open("{path_lit}") ?? panic("open")
    limits := data.DataLimits.safe()
    limits.max_groups = 1
    reader :: data.csv_reader<Event>(input, limits) ?? panic("reader")
    first :: reader.next() ?? panic("next")
    if first == {{
        Val(row) -> print("first:{{row.service}}")
        None -> panic("eof")
    }}
    groups := data.group_mean(reader, (e) => e.service, (e) => e.latency_ms)
    if groups == {{
        .Ok(_) -> print("unexpected ok")
        .Err(error) -> print("{{error.kind}} {{error.operation}}")
    }}
    empty := data.mean([Float].{{}})
    if empty == {{
        .Ok(_) -> print("unexpected mean")
        .Err(error) -> print("{{error.kind}} {{error.operation}}")
    }}
    bad := data.quantile([1.0, 2.0], 1.5)
    if bad == {{
        .Ok(_) -> print("unexpected q")
        .Err(error) -> print("{{error.kind}} {{error.operation}}")
    }}
}}
"#
        ),
        &[],
        None,
    );
    assert_eq!(code, 0, "core.data stream program failed: {stderr}");
    assert_eq!(
        stdout,
        "first:api\nLimit group_mean\nEmpty mean\nInvalidArgument quantile\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_data_schema_ingest_and_select() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.data schema test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_data_schema_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "data_schema",
        r#"
use core.data as data

#Codable
struct Ticket {
    team: String
    minutes: Float
}

fn run() {
    raw :: "team,minutes\nCore,4.0\nTools,5.0\nCore,8.0"
    rows :: data.csv<Ticket>(raw) ?? panic("bad csv")
    table :: data.table(rows)
    cols :: data.schema(table)
    loop c, cols {
        print("{c.name}:{c.type_name}")
    }
    selected :: data.filter(data.rows(table), (t) => t.minutes >= 5.0)
    print("selected:{data.count(selected)}")
    loop t, selected {
        print("{t.team}:{t.minutes}")
    }
    print("{data.status()[5].step}:{data.status()[5].path}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.data schema program failed: {stderr}");
    assert_eq!(
        stdout,
        "team:String\nminutes:Float\nselected:2\nTools:5.0\nCore:8.0\ncore.data.schema:native\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_data_json_ingest_and_select() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.data json test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_data_json_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "data_json",
        r#"
use core.data as data

#Codable
struct Ticket {
    team: String
    minutes: Float
}

fn run() {
    raw :: "[{{\"team\":\"Core\",\"minutes\":4.0}},{{\"team\":\"Tools\",\"minutes\":5.0}},{{\"team\":\"Core\",\"minutes\":8.0}}]"
    rows :: data.json<Ticket>(raw) ?? panic("bad json")
    table :: data.table(rows)
    cols :: data.schema(table)
    loop c, cols {
        print("{c.name}:{c.type_name}")
    }
    selected :: data.filter(data.rows(table), (t) => t.minutes >= 5.0)
    print("selected:{data.count(selected)}")
    loop t, selected {
        print("{t.team}:{t.minutes}")
    }
    print("{data.status()[6].step}:{data.status()[6].path}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.data json program failed: {stderr}");
    assert_eq!(
        stdout,
        "team:String\nminutes:Float\nselected:2\nTools:5.0\nCore:8.0\ncore.data.json:native\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_data_schema_empty_table_and_series_law() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.data empty schema test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_data_schema_empty_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "data_schema_empty",
        r#"
use core.data as data

#Codable
struct Ticket {
    team: String
    minutes: Float
}

struct Empty {}

struct Box<T> {
    value: T
}

fn run() {
    empty_rows := [Ticket].{}
    empty_table :: data.table(empty_rows)
    loop c, data.schema(empty_table) {
        print("empty:{c.name}:{c.type_name}")
    }

    nums :: data.series([1.0, 2.0])
    loop c, data.schema(nums) {
        print("float:{c.name}:{c.type_name}")
    }

    tickets :: data.series([Ticket.{team: "Core", minutes: 4.0}])
    loop c, data.schema(tickets) {
        print("struct:{c.name}:{c.type_name}")
    }

    empty_tickets := [Ticket].{}
    empty_series :: data.series(empty_tickets)
    loop c, data.schema(empty_series) {
        print("empty_series:{c.name}:{c.type_name}")
    }

    empty_units := [Empty].{}
    print("empty_struct:{data.count(data.schema(data.table(empty_units)))}")

    boxed := [Box<Int>].{}
    loop c, data.schema(data.table(boxed)) {
        print("generic:{c.name}:{c.type_name}")
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.data empty schema program failed: {stderr}");
    assert_eq!(
        stdout,
        "empty:team:String\nempty:minutes:Float\nfloat:value:Float\nstruct:value:Ticket\nempty_series:value:Ticket\nempty_struct:0\ngeneric:value:Int\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn io_input_reads_a_line_from_stdin() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping io.input test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_input_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "input_demo",
        r#"
use core.io as io

fn run() {
    name :: io.input("name? ") ?? panic("read failed")
    print("hello, {name}")
}
"#,
        &[],
        Some("Ada\n"),
    );
    assert_eq!(code, 0, "stdin demo failed");
    assert!(
        stdout.contains("hello, Ada"),
        "expected greeting on stdout, got stdout={stdout:?} stderr={stderr:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn io_prompt_helpers_validate_choices_and_refuse_non_tty_secrets() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.io prompt test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_prompts_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = include_str!("../../examples/features/io/terminal_parity.jet");
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "terminal_parity",
        source,
        &[],
        Some("\nnot-a-number\n3\n2\n"),
    );
    assert_eq!(code, 0, "prompt fixture failed: {stderr}");
    assert_eq!(
        stdout,
        include_str!("../../examples/features/expected/io/terminal_parity.out")
    );
    assert_eq!(
        stderr,
        include_str!("../../examples/features/expected/io/terminal_parity.stderr.out")
    );

    #[cfg(unix)]
    {
        let shell = r#"
{
  sleep 0.2
  printf '\r'
  sleep 0.1
  printf 'bad\r3\r2\r'
  sleep 0.2
  printf 'swordfish\r'
} | timeout 8s script -qec '"$JET_PROMPT_BIN"' /dev/null
"#;
        let output = Command::new("sh")
            .args(["-c", shell])
            .env("JET_PROMPT_BIN", dir.join("terminal_parity"))
            .env("NO_COLOR", "1")
            .output()
            .expect("run prompt fixture under PTY");
        let shown = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "PTY prompt failed:\n{shown}");
        assert!(shown.contains("secret length: 9"), "{shown}");
        assert!(!shown.contains("swordfish"), "secret was echoed:\n{shown}");
    }

    let _ = fs::remove_dir_all(&dir);
}


