use std::fs;
use std::io::Write;

use jet::Interpreter::dev_iteration;
use jet_foundation::JitBackend::RunOutcome;

mod common;

struct Output {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

fn on_large_stack(work: impl FnOnce() + Send) {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn_scoped(scope, work)
            .unwrap()
            .join()
            .unwrap();
    });
}

fn skip_if_cranelift_host_unsupported() -> bool {
    if jet_jit::cranelift_host_supported() {
        false
    } else if std::env::var("JET_REQUIRE_CRANELIFT_HOST").as_deref() == Ok("1") {
        panic!("Cranelift host required but unavailable");
    } else {
        true
    }
}

fn run(source: &str, name: &str) -> Output {
    run_with_mode(source, name, false)
}

fn run_with_mode(source: &str, name: &str, use_interpreter: bool) -> Output {
    let dir = common::unique_tmp(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"http_i9\"\nversion: \"0.1.0\"\nauthority: .{ holds: { allow: [FS, IO, Mem.Alloc, Net] } }\n",
    )
    .unwrap();
    let file = dir.join("main.jet");
    fs::write(&file, source).unwrap();
    jet_jit::reset_jit_trace_for_test();
    let outcome = dev_iteration(file.to_str().unwrap(), false, use_interpreter);
    let output = match outcome {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Output {
            stdout,
            stderr,
            exit_code,
        },
        RunOutcome::Problems(diags) => panic!("{name} failed: {diags:#?}"),
    };
    let _ = fs::remove_dir_all(dir);
    output
}

const RESIDENT: &str = r#"
use core.http.client as http_client
use core.http.server as http_server

#Codable
struct Reading {
    city: String
    degrees: Int
}

fn run() {
    mux :: http_server.mux()
    mux.get("/health", () -> Ok(http_server.response(204, "")))
    policy :: http_server.cors_policy(["https://app.example"]) ?? panic("cors")
    http_server.cors(mux, policy)
    http_server.static_files(mux, "/assets", "/tmp")
    response :: http_server.json(201, Reading{city: "Reno", degrees: 32})
    response_text :: http_server.response(200, "hello")
    decoded :: response.json<Reading>() ?? panic("response")
    request :: http_client.request("POST", "http://example.test/")
        .json(Reading{city: "Reno", degrees: 31})
    request_decoded :: request.json<Reading>() ?? panic("request")
    request_content_type :: request.header("content-type") ?? "missing"
    print("{decoded.city}|{request_decoded.degrees}|{request_content_type}|{response_text.text() ?? "invalid body"}")
}
"#;

const FORCED_DEOPT: &str = r#"
use core.http.client as http_client
use core.http.server as http_server
use core.text as text

#Codable
struct Reading {
    city: String
    degrees: Int
}

fn any_origins() HTTPCorsOrigins -> {
    return .Any
}

fn run() {
    print(text.casefold("Straße"))
    mux :: http_server.mux()
    mux.get("/health", () -> Ok(http_server.response(204, "")))
    policy :: http_server.cors_policy(["https://app.example"]) ?? panic("cors")
    http_server.cors(mux, policy)
    http_server.static_files(mux, "/assets", "/tmp")
    response :: http_server.json(201, Reading{city: "Reno", degrees: 32})
    response_text :: http_server.response(200, "hello")
    decoded :: response.json<Reading>() ?? panic("response")
    request :: http_client.request("POST", "http://example.test/")
        .json(Reading{city: "Reno", degrees: 31})
    request_decoded :: request.json<Reading>() ?? panic("request")
    request_content_type :: request.header("content-type") ?? "missing"
    print("{decoded.city}|{request_decoded.degrees}|{request_content_type}|{response_text.text() ?? "invalid body"}")
    malformed :: http_client.request("POST", "http://example.test/").body("{{")
    if malformed.json<Reading>() == {
        .Ok(_) -> print("accepted")
        .Err(error) -> print(error)
        else -> print("unexpected")
    }
    if http_server.cors_policy(any_origins(), [], [], true) == {
        .Ok(_) -> print("accepted")
        .Err(error) -> print(error)
        else -> print("unexpected")
    }
}
"#;

const ROUTE_HANDLERS: &str = r#"
use core.http.client as http
use core.http.server as server
use core.net as net

fn route_error(req: HTTPRequest) HTTPResponse !HTTPError -> {
    if req.path() == "/error" -> return Err(.InvalidFraming)
    return Err(.InvalidFraming)
}

fn run() !(HTTPError | NetError | TaskFailure) {
    listener :: net.tcp_listen("127.0.0.1:0")
    address :: listener.local_addr()
    state :: "captured"
    mux :: server.mux()
    mux.get("/zero", () -> Ok(server.response(200, "zero")))
    mux.get("/items/:id", (req: HTTPRequest) HTTPResponse !HTTPError -> {
        header :: req.header("x-state") ?? "missing"
        id :: req.param("id") ?? "missing"
        path :: req.path()
        return Ok(server.response(200, "{path}|{id}|{header}|{state}"))
    })
    mux.get("/error", route_error)
    t :: task {
        server.serve_once_listener(listener, mux)
        server.serve_once_listener(listener, mux)
        server.serve_once_listener(listener, mux)
    }
    zero :: http.request("GET", "http://{address}/zero").send() ?? panic("zero")
    print(zero.text() ?? "zero body")
    detail :: http.request("GET", "http://{address}/items/42")
        .header("x-state", "captured-header")
        .send() ?? panic("detail")
    print(detail.text() ?? "detail body")
    failed :: http.request("GET", "http://{address}/error").send() ?? panic("error")
    print(failed.status())
    t.join()
}
"#;

const HOSTILE_URLS: &str = r#"
use core.http.client as http_client
use core.net.ws as ws

fn run() {
    if http_client.get("http://127.0.0.1:1/path\nInjected: yes") == {
        .Ok(_) -> print("http accepted")
        .Err(_) -> print("http rejected")
        else -> print("http unexpected")
    }
    if ws.connect("ws://127.0.0.1:1/path\nInjected: yes") == {
        .Ok(_) -> print("ws accepted")
        .Err(error) -> {
            if error == .InvalidUrl -> print("ws rejected")
            else -> print("ws wrong error")
        }
        else -> print("ws unexpected")
    }
}
"#;

#[test]
fn http_web_defaults_stay_resident_in_cranelift() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    on_large_stack(|| {
        let output = run(RESIDENT, "http_i9_resident");
        assert_eq!(output.stdout, "Reno|31|application/json|hello\n");
        assert_eq!(output.stderr, "");
        assert_eq!(output.exit_code, 0);
        assert!(jet_jit::jit_executed_for_test());
        assert!(!jet_jit::deopt_invoked_for_test());
        assert!(!jet_jit::fallback_invoked_for_test());
    });
}

#[test]
fn http_web_defaults_forced_deopt_uses_prelude_ambient() {
    on_large_stack(|| {
        let output = run(FORCED_DEOPT, "http_i9_forced_deopt");
        assert_eq!(output.stderr, "");
        assert_eq!(output.exit_code, 0);
        assert!(output
            .stdout
            .starts_with("strasse\nReno|31|application/json|hello\nInvalidFraming\nPolicy { reason: "));
        assert!(output
            .stdout
            .contains("CORS credentials need named origins."));
        assert!(jet_jit::deopt_invoked_for_test());
        assert!(!jet_jit::fallback_invoked_for_test());
    });
}

#[test]
fn http_route_handlers_preserve_arity_context_and_errors_on_both_dev_tiers() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    on_large_stack(|| {
        for (use_interpreter, name) in [
            (false, "http_i9_routes_jit"),
            (true, "http_i9_routes_interpreter"),
        ] {
            let output = run_with_mode(ROUTE_HANDLERS, name, use_interpreter);
            assert_eq!(output.stdout, "zero\n/items/42|42|captured-header|captured\n400\n");
            assert_eq!(output.stderr, "");
            assert_eq!(output.exit_code, 0);
        }
    });
}

#[test]
fn hostile_http_and_websocket_urls_are_rejected_on_both_dev_tiers() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    on_large_stack(|| {
        for (use_interpreter, name) in [(false, "http_i9_hostile_jit"), (true, "http_i9_hostile_interpreter")] {
            let output = run_with_mode(HOSTILE_URLS, name, use_interpreter);
            assert_eq!(output.stdout, "http rejected\nws rejected\n");
            assert_eq!(output.stderr, "");
            assert_eq!(output.exit_code, 0);
        }
    });
}

#[test]
fn hostile_http_response_lengths_are_rejected_on_both_dev_tiers() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    let server = std::thread::spawn(move || {
        const RESPONSE_BYTES: usize = 64 * 1024 * 1024 + 1;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {RESPONSE_BYTES}\r\nConnection: close\r\n\r\n"
        );
        for _ in 0..4 {
            let (mut stream, _) = listener.accept().expect("client connection");
            let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
            let _ = std::io::Write::flush(&mut stream);
        }
    });
    let url = format!("http://{address}/oversized");
    let source = r#"
use core.http.client as http

fn run() {
    if http.get("__URL__") == {
        .Ok(_) -> print("simple accepted")
        .Err(_) -> print("simple rejected")
        else -> print("simple unexpected")
    }
    request :: http.request("GET", "__URL__")
    if request.send() == {
        .Ok(_) -> print("request accepted")
        .Err(_) -> print("request rejected")
        else -> print("request unexpected")
    }
}
"#
    .replace("__URL__", &url);

    on_large_stack(|| {
        for (use_interpreter, name) in [
            (false, "http_i9_oversized_response_jit"),
            (true, "http_i9_oversized_response_interpreter"),
        ] {
            let output = run_with_mode(&source, name, use_interpreter);
            assert_eq!(output.stdout, "simple rejected\nrequest rejected\n");
            assert_eq!(output.stderr, "");
            assert_eq!(output.exit_code, 0);
        }
    });
    server.join().expect("server");
}

#[test]
fn hostile_chunked_body_deadline_ends_request_on_both_dev_tiers() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }

    on_large_stack(|| {
        for (use_interpreter, name) in [
            (false, "http_i9_chunked_deadline_jit"),
            (true, "http_i9_chunked_deadline_interpreter"),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind port");
            let port = listener.local_addr().expect("port address").port();
            drop(listener);
            let source = format!(
                r#"
use core.http.server as server
use core.net as net
use core.tasks as tasks

fn run() !(HTTPError | NetError | TaskFailure) {{
    listener :: net.tcp_listen("127.0.0.1:{port}") ?? panic("listen")
    mux :: server.mux()
    mux.post("/", (req: HTTPRequest) -> {{
        body :: req.body().text(1024) ?? "rejected"
        return Ok(server.response(200, body))
    }})
    server_task :: task {{
        server.serve_once_listener(listener, mux) ?? panic("serve")
    }}
    server_task.join() ?? panic("serve")
}}
"#
            );
            let tier_name = name.to_string();
            let runner = std::thread::spawn(move || run_with_mode(&source, &tier_name, use_interpreter));
            let started = std::time::Instant::now();
            let mut client = loop {
                match std::net::TcpStream::connect(("127.0.0.1", port)) {
                    Ok(stream) => break stream,
                    Err(error) if started.elapsed() < std::time::Duration::from_secs(30) => {
                        let _ = error;
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(error) => {
                        let _ = runner.join();
                        panic!("chunked server did not start: {error}");
                    }
                }
            };
            client
                .write_all(
                    b"POST / HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\nTrailer: X-Test\r\nConnection: close\r\n\r\n1\r\nx\r\n0\r\n",
                )
                .expect("write chunked prefix");
            client.flush().expect("flush chunked prefix");
            client
                .set_read_timeout(Some(std::time::Duration::from_secs(35)))
                .expect("set response timeout");
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let writer_stop = stop.clone();
            let mut trickle = client.try_clone().expect("clone client");
            let writer = std::thread::spawn(move || {
                let trailer = b"X-Test: value\r\n";
                for (index, byte) in trailer.iter().copied().enumerate() {
                    if writer_stop.load(std::sync::atomic::Ordering::Acquire)
                        || trickle.write_all(&[byte]).is_err()
                    {
                        return;
                    }
                    let _ = trickle.flush();
                    if index + 1 < trailer.len() {
                        for _ in 0..80 {
                            if writer_stop.load(std::sync::atomic::Ordering::Acquire) {
                                return;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                    }
                }
            });
            let mut response = Vec::new();
            let read_result = std::io::Read::read_to_end(&mut client, &mut response);
            stop.store(true, std::sync::atomic::Ordering::Release);
            drop(client);
            writer.join().expect("trickle writer");
            let output = runner.join().expect("chunked server");
            assert!(
                read_result.is_ok() && response.starts_with(b"HTTP/1.1 "),
                "chunked request was not ended by the body deadline after {:?}: read={read_result:?}, response={response:?}, stderr={}",
                started.elapsed(),
                output.stderr
            );
            assert!(started.elapsed() < std::time::Duration::from_secs(35));
            assert_eq!(output.stderr, "");
            assert_eq!(output.exit_code, 0);
        }
    });
}
