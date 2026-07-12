#![allow(dead_code)]

struct JetTcpListener {
    inner: std::net::TcpListener,
}

mod jet_std {
    #[derive(Clone, Copy)]
    pub struct Duration {
        pub ms: i64,
    }
}

include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/HttpServer.rs");

fn request(addr: std::net::SocketAddr, text: &'static [u8]) -> String {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(addr).expect("connect");
    stream.write_all(text).expect("request write");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("response read");
    response
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
    use std::io::{Read, Write};
    let mux = jet_http_mux_new();
    jet_http_mux_add(&mux, "GET", "/", |_| jet_http_srv_response(200, &"handle".to_string()));
    let server = jet_http_server_bind(&"127.0.0.1:0".to_string(), mux).expect("bind");
    let addr: std::net::SocketAddr = jet_http_server_local_addr(&server).expect("addr").parse().expect("socket addr");
    let serving = server.clone();
    let serve_thread = std::thread::spawn(move || jet_http_server_serve(&serving).expect("serve"));
    let mut client = std::net::TcpStream::connect(addr).expect("connect");
    client.write_all(b"GET / HTTP/1.1\r\nHost: local\r\n\r\n").expect("write");
    let mut response = String::new();
    client.read_to_string(&mut response).expect("read");
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
    jet_http_mux_add(&mux, "GET", "/files/{*path}", |req| {
        jet_http_srv_response(200, &format!("wild:{}", jet_http_srv_req_param(&req, &"path".to_string()).unwrap()))
    });
    jet_http_mux_add(&mux, "GET", "/files/{id}", |req| {
        jet_http_srv_response(200, &format!("param:{}", jet_http_srv_req_param(&req, &"id".to_string()).unwrap()))
    });
    jet_http_mux_add(&mux, "GET", "/files/static", |_| jet_http_srv_response(200, &"static".to_string()));
    jet_http_mux_add(&mux, "POST", "/files/{id}", |_| jet_http_srv_response(201, &"posted".to_string()));

    let request = |method: &str, path: &str| JetHttpSrvReq {
        method: method.to_string(), path: path.to_string(), params: Default::default(),
        body: String::new(), headers: Default::default(),
    };
    assert_eq!(jet_http_mux_dispatch(&mux, request("GET", "/files/static")).body, "static");
    assert_eq!(jet_http_mux_dispatch(&mux, request("GET", "/files/42")).body, "param:42");
    assert_eq!(jet_http_mux_dispatch(&mux, request("GET", "/files/a/b")).body, "wild:a/b");
    let head = jet_http_mux_dispatch(&mux, request("HEAD", "/files/static"));
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
    assert_eq!(jet_http_mux_dispatch(&mux, request("DELETE", "/files/42")).status, 405);
    let options = jet_http_mux_dispatch(&mux, request("OPTIONS", "/files/42"));
    assert_eq!(options.status, 204);
    assert_eq!(options.headers.get("Allow").unwrap(), "GET, HEAD, OPTIONS, POST");

    let conflict = jet_http_mux_new();
    jet_http_mux_add(&conflict, "GET", "/users/{id}", |_| jet_http_srv_response(200, &String::new()));
    jet_http_mux_add(&conflict, "GET", "/users/{name}", |_| jet_http_srv_response(200, &String::new()));
    assert!(jet_http_server_bind(&"127.0.0.1:0".to_string(), conflict).err().expect("conflict").contains("route conflict"));
    let legacy = jet_http_mux_new();
    jet_http_mux_add(&legacy, "GET", "/users/:id", |_| jet_http_srv_response(200, &String::new()));
    assert!(jet_http_server_bind(&"127.0.0.1:0".to_string(), legacy).err().expect("legacy pattern").contains("use `{name}`"));
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
    jet_http_mux_add(&mux, "GET", "/ok/{id}", |req| {
        jet_http_srv_response(200, &jet_http_srv_req_param(&req, &"id".to_string()).unwrap())
    });
    jet_http_mux_add(&mux, "GET", "/panic", |_| panic!("private failure detail"));
    let request = |path: &str| JetHttpSrvReq {
        method: "GET".to_string(), path: path.to_string(), params: Default::default(),
        body: String::new(), headers: Default::default(),
    };
    assert_eq!(jet_http_mux_dispatch(&mux, request("/ok/one")).body, "one");
    assert_eq!(&*events.lock().unwrap(), &[
        "outer:before:/ok/one", "inner:before:/ok/one",
        "inner:after:/ok/one", "outer:after:/ok/one",
    ]);
    let panic_response = jet_http_mux_dispatch(&mux, request("/panic"));
    assert_eq!(panic_response.status, 500);
    assert_eq!(panic_response.body, "500 Internal Server Error");

    let short = jet_http_mux_new();
    let handler_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    jet_http_mux_middleware(&short, |_| std::sync::Arc::new(|_| jet_http_srv_response(403, &"blocked".to_string())));
    let calls = handler_calls.clone();
    jet_http_mux_add(&short, "GET", "/", move |_| {
        calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        jet_http_srv_response(200, &"wrong".to_string())
    });
    assert_eq!(jet_http_mux_dispatch(&short, request("/")).status, 403);
    assert_eq!(handler_calls.load(std::sync::atomic::Ordering::Relaxed), 0);

    let mut threads = Vec::new();
    for id in 0..16 {
        let mux = mux.clone();
        threads.push(std::thread::spawn(move || {
            let req = JetHttpSrvReq { method: "GET".to_string(), path: format!("/ok/{id}"), params: Default::default(), body: String::new(), headers: Default::default() };
            assert_eq!(jet_http_mux_dispatch(&mux, req).body, id.to_string());
        }));
    }
    for thread in threads { thread.join().unwrap(); }
}
