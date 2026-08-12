#[test]
fn core_net_dns_txt_and_srv_are_real_udp_queries() {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let mut buf = [0u8; 512];
            let (n, peer) = socket.recv_from(&mut buf).unwrap();
            let resp = dns_fixture_response(&buf[..n]);
            socket.send_to(&resp, peer).unwrap();
        }
    });
    let dir = std::env::temp_dir().join(format!("jet_core_net_dns_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    txts :: net.dns_txt_at("{}", "service.example.test", 1000) ?? panic("txt")
    print(txts[0])
    srvs :: net.dns_srv_at("{}", "_jet._tcp.example.test", 1000) ?? panic("srv")
    print("{{net.dns_srv_target(srvs[0])}}:{{net.dns_srv_port(srvs[0])}}")
}}
"#,
        addr, addr
    );
    let (code, stdout, stderr) = build_and_run(&dir, "dns_fixture", &src, &[], None);
    server.join().unwrap();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "jet\nsrv.example.test:443\n");
}

#[test]
fn core_net_dns_udp_truncation_retries_tcp_and_reads_cname_additional() {
    use std::io::{Read, Write};

    let (tcp, udp, addr) = bind_dns_dual_protocol_fixture();
    let server = std::thread::spawn(move || {
        let mut udp_query = [0u8; 512];
        let (n, peer) = udp.recv_from(&mut udp_query).unwrap();
        udp.send_to(&dns_truncated_response(&udp_query[..n]), peer)
            .unwrap();

        let (mut stream, _) = tcp.accept().unwrap();
        let mut prefix = [0u8; 2];
        stream.read_exact(&mut prefix).unwrap();
        let mut tcp_query = vec![0u8; u16::from_be_bytes(prefix) as usize];
        stream.read_exact(&mut tcp_query).unwrap();
        let response = dns_cname_additional_response(&tcp_query);
        stream
            .write_all(&(response.len() as u16).to_be_bytes())
            .unwrap();
        stream.write_all(&response).unwrap();
    });

    let dir = std::env::temp_dir().join(format!("jet_core_net_dns_tcp_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    ips :: net.dns_a_at("{}", "service.example.test", 1000) ?? panic("dns")
    print(net.ip_to_string(ips[0]))
}}
"#,
        addr
    );
    let (code, stdout, stderr) = build_and_run(&dir, "dns_tcp_cname", &src, &[], None);
    server.join().unwrap();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "192.0.2.42\n");
}

#[test]
fn core_net_dns_timeout_is_one_budget_across_udp_and_tcp() {
    use std::io::Read;

    let (tcp, udp, addr) = bind_dns_dual_protocol_fixture();
    let server = std::thread::spawn(move || {
        let mut query = [0u8; 512];
        let (n, peer) = udp.recv_from(&mut query).unwrap();
        let started = std::time::Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(70));
        udp.send_to(&dns_truncated_response(&query[..n]), peer).unwrap();
        let (mut stream, _) = tcp.accept().unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_millis(500))).unwrap();
        let mut prefix = [0u8; 2];
        stream.read_exact(&mut prefix).unwrap();
        let mut request = vec![0u8; u16::from_be_bytes(prefix) as usize];
        stream.read_exact(&mut request).unwrap();
        let mut closed = [0u8; 1];
        let _ = stream.read(&mut closed);
        started.elapsed()
    });
    let dir = std::env::temp_dir().join(format!("jet_core_net_dns_budget_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(r#"
use core.net as net

fn run() {{
    _ :: net.dns_a_at("{}", "service.example.test", 120) ?? panic("dns total timeout")
}}
"#, addr);
    let (code, _stdout, stderr) = build_and_run(&dir, "dns_total_budget", &src, &[], None);
    let elapsed = server.join().unwrap();
    assert_ne!(code, 0, "stalled DNS TCP fallback unexpectedly succeeded");
    assert!(stderr.contains("dns total timeout"), "{stderr}");
    assert!(elapsed < std::time::Duration::from_millis(190), "UDP and TCP each received a fresh timeout: {elapsed:?}");
}

#[test]
fn core_net_dns_platform_resolver_policy_uses_native_sources() {
    let net = include_str!("../../crates/jet-codegen/src/Prelude/CoreLib/Top/NetHTTP.rs");
    assert!(net.contains("#[cfg(target_os = \"linux\")]"));
    assert!(net.contains("read_to_string(\"/etc/resolv.conf\")"));
    assert!(net.contains("#[cfg(target_os = \"macos\")]"));
    assert!(net.contains("Command::new(\"scutil\").arg(\"--dns\")"));
    assert!(net.contains("#[cfg(windows)]"));
    assert!(net.contains("Get-DNSClientServerAddress"));
    assert!(net.contains("$_.ServerAddresses"));
    assert!(!net.contains("Command::new(\"ipconfig\")"));
    assert!(!net.contains("1.1.1.1"));
}

#[test]
fn core_net_dns_platform_resolver_parsers_accept_native_fixtures() {
    assert_eq!(
        dns_resolver_policy::resolv_conf(
            "# generated\nnameserver 192.0.2.53 # vpn\nnameserver 2001:db8::53\nsearch example.test\n"
        ),
        ["192.0.2.53:53", "[2001:db8::53]:53"]
    );
    assert_eq!(
        dns_resolver_policy::scutil(
            "resolver #1\n  nameserver[0] : 192.0.2.54\n  nameserver[1] : 2001:db8::54\n  search domain[0] : example.test\n"
        ),
        ["192.0.2.54:53", "[2001:db8::54]:53"]
    );
    assert_eq!(
        dns_resolver_policy::windows("{192.0.2.55, 2001:db8::55}\r\n\r\n"),
        ["192.0.2.55:53", "[2001:db8::55]:53"]
    );
}

#[test]
fn core_net_dns_platform_resolver_parsers_reject_noise_and_malformed_entries() {
    assert!(dns_resolver_policy::resolv_conf(
        "nameserver nope\nnot-nameserver 192.0.2.1\nnameserver [broken\n"
    )
    .is_empty());
    assert!(dns_resolver_policy::scutil(
        "nameserver[x] : 192.0.2.1\nnameserver[0 : 192.0.2.2\nnameserver[] : 192.0.2.3\n"
    )
    .is_empty());
    assert!(dns_resolver_policy::windows(
        "InterfaceAlias Ethernet\nServerAddresses nope, 999.1.1.1\n"
    )
    .is_empty());
}

#[test]
fn core_net_dns_rejects_wrong_transaction_id() {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut query = [0u8; 512];
        let (n, peer) = socket.recv_from(&mut query).unwrap();
        let mut response = dns_fixture_response(&query[..n]);
        let wrong = u16::from_be_bytes([response[0], response[1]]).wrapping_add(1);
        response[0..2].copy_from_slice(&wrong.to_be_bytes());
        socket.send_to(&response, peer).unwrap();
    });

    let dir = std::env::temp_dir().join(format!("jet_core_net_dns_bad_id_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    _ :: net.dns_txt_at("{}", "service.example.test", 1000) ?? panic("forged DNS accepted")
}}
"#,
        addr
    );
    let (code, _stdout, stderr) = build_and_run(&dir, "dns_bad_id", &src, &[], None);
    server.join().unwrap();
    assert_ne!(code, 0, "forged transaction ID was accepted");
    assert!(stderr.contains("forged DNS accepted"), "{stderr}");
}

#[test]
fn core_net_dns_transaction_ids_are_not_a_fixed_sequence() {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut ids = std::collections::BTreeSet::new();
        for _ in 0..9 {
            let mut query = [0u8; 512];
            let (n, peer) = socket.recv_from(&mut query).unwrap();
            ids.insert(u16::from_be_bytes([query[0], query[1]]));
            socket
                .send_to(&dns_fixture_response(&query[..n]), peer)
                .unwrap();
        }
        ids
    });

    let dir = std::env::temp_dir().join(format!("jet_core_net_dns_ids_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    loop _i, 0..8 {{
        _ :: net.dns_txt_at("{}", "service.example.test", 1000) ?? panic("dns")
    }}
}}
"#,
        addr
    );
    let (code, _stdout, stderr) = build_and_run(&dir, "dns_ids", &src, &[], None);
    let ids = server.join().unwrap();
    assert_eq!(code, 0, "{stderr}");
    assert!(ids.len() > 1, "all nine DNS queries reused one transaction ID");
}

fn run_rejected_dns_response(tag: &str, make_response: fn(&[u8]) -> Vec<u8>) -> String {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut query = [0u8; 512];
        let (n, peer) = socket.recv_from(&mut query).unwrap();
        socket
            .send_to(&make_response(&query[..n]), peer)
            .unwrap();
    });
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_dns_reject_{}_{}",
        tag,
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    _ :: net.dns_a_at("{}", "service.example.test", 1000) ?? panic("invalid DNS accepted")
}}
"#,
        addr
    );
    let (code, _stdout, stderr) = build_and_run(&dir, tag, &src, &[], None);
    server.join().unwrap();
    assert_ne!(code, 0, "invalid DNS response was accepted: {tag}");
    stderr
}

#[test]
fn core_net_dns_rejects_non_response_and_cyclic_compression() {
    fn non_response(query: &[u8]) -> Vec<u8> {
        let mut response = dns_fixture_response(query);
        response[2] &= 0x7f;
        response
    }
    fn cyclic_compression(query: &[u8]) -> Vec<u8> {
        let end = dns_question_end(query);
        let record_start = end;
        let mut response = Vec::new();
        response.extend_from_slice(&query[0..2]);
        response.extend_from_slice(&0x8180u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&query[12..end]);
        response.push(0xc0 | ((record_start >> 8) as u8 & 0x3f));
        response.push(record_start as u8);
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u32.to_be_bytes());
        response.extend_from_slice(&4u16.to_be_bytes());
        response.extend_from_slice(&[192, 0, 2, 1]);
        response
    }

    let qr = run_rejected_dns_response("dns_not_response", non_response);
    assert!(qr.contains("invalid DNS accepted"), "{qr}");
    let cycle = run_rejected_dns_response("dns_pointer_cycle", cyclic_compression);
    assert!(cycle.contains("invalid DNS accepted"), "{cycle}");
}

#[test]
fn core_net_dns_rejects_reserved_header_forward_pointer_and_impossible_counts() {
    fn reserved_header(query: &[u8]) -> Vec<u8> {
        let mut response = dns_cname_additional_response(query);
        response[3] |= 0x40;
        response
    }
    fn forward_pointer(query: &[u8]) -> Vec<u8> {
        let end = dns_question_end(query);
        let record_start = end;
        let pointer_target = record_start + 6;
        let mut response = Vec::new();
        response.extend_from_slice(&query[0..2]);
        response.extend_from_slice(&0x8180u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&query[12..end]);
        response.push(0xc0 | ((pointer_target >> 8) as u8 & 0x3f));
        response.push(pointer_target as u8);
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u32.to_be_bytes());
        response.extend_from_slice(&4u16.to_be_bytes());
        response.extend_from_slice(&[192, 0, 2, 1]);
        response
    }
    fn impossible_counts(query: &[u8]) -> Vec<u8> {
        let end = dns_question_end(query);
        let mut response = Vec::new();
        response.extend_from_slice(&query[0..2]);
        response.extend_from_slice(&0x8180u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&u16::MAX.to_be_bytes());
        response.extend_from_slice(&u16::MAX.to_be_bytes());
        response.extend_from_slice(&u16::MAX.to_be_bytes());
        response.extend_from_slice(&query[12..end]);
        response
    }

    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let responses: [fn(&[u8]) -> Vec<u8>; 3] =
            [reserved_header, forward_pointer, impossible_counts];
        for response in responses {
            let mut query = [0u8; 512];
            let (n, peer) = socket.recv_from(&mut query).unwrap();
            socket.send_to(&response(&query[..n]), peer).unwrap();
        }
    });
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_dns_hostile_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    if net.dns_a_at("{0}", "service.example.test", 1000) == {{
        .Ok(_) -> panic("reserved DNS header accepted")
        .Err(_) -> print("rejected")
    }}
    if net.dns_a_at("{0}", "service.example.test", 1000) == {{
        .Ok(_) -> panic("forward DNS pointer accepted")
        .Err(_) -> print("rejected")
    }}
    if net.dns_a_at("{0}", "service.example.test", 1000) == {{
        .Ok(_) -> panic("impossible DNS counts accepted")
        .Err(_) -> print("rejected")
    }}
}}
"#,
        addr
    );
    let (code, stdout, stderr) = build_and_run(&dir, "dns_hostile_bounds", &src, &[], None);
    server.join().unwrap();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "rejected\nrejected\nrejected\n");
}

#[test]
fn core_net_dns_wire_lookup_observes_task_cancellation() {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_dns_cancel_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {{
    (ready_tx, ready_rx) :: tasks.channel<Int>()
    lookup :: task {{
        ready_tx.send(1)
        if net.dns_a_at("{}", "service.example.test", 5000) == {{
            .Ok(_) -> print("unexpected DNS response")
            .Err(error) -> print(net.error_message(error))
        }}
    }}
    _ready :: ready_rx.receive() ?? panic("ready")
    time.sleep(50)
    lookup.cancel()
    lookup.join() ?? 0
}}
"#,
        addr
    );
    let (code, stdout, stderr) = build_and_run(&dir, "dns_task_cancel", &src, &[], None);
    drop(socket);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "network operation cancelled during DNS lookup for `service.example.test`\n");
}

#[test]
fn core_net_dns_nxdomain_is_an_error() {
    fn nxdomain(query: &[u8]) -> Vec<u8> {
        let end = dns_question_end(query);
        let mut response = Vec::new();
        response.extend_from_slice(&query[0..2]);
        response.extend_from_slice(&0x8183u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&query[12..end]);
        response
    }

    let stderr = run_rejected_dns_response("dns_nxdomain", nxdomain);
    assert!(stderr.contains("invalid DNS accepted"), "{stderr}");
}

#[test]
fn core_net_ratified_named_forms_require_exact_labels() {
    let cases = [
        ("tcp accept", "fn check(listener: TcpListener, d: Duration) { result :: listener.accept(d) }", "deadline:"),
        ("tcp read", "fn check(stream: TcpStream, d: Duration) { result :: stream.read(1, banana: d) }", "deadline:"),
        ("tcp read text", "fn check(stream: TcpStream, d: Duration) { result :: stream.read_text(1, d) }", "deadline:"),
        ("tcp write", "fn check(stream: TcpStream, d: Duration) { result :: stream.write([1], potato: d) }", "deadline:"),
        ("tcp write all", "fn check(stream: TcpStream, d: Duration) { result :: stream.write_all([1], d) }", "deadline:"),
        ("tcp write text", "fn check(stream: TcpStream, d: Duration) { result :: stream.write_text(\"x\", turnip: d) }", "deadline:"),
        ("tcp ready", "fn check(stream: TcpStream, d: Duration) { result :: stream.ready(.Read, d) }", "deadline:"),
        ("udp send", "fn check(socket: UdpSocket, address: SocketAddr, d: Duration) { result :: socket.send_to([1], address, banana: d) }", "deadline:"),
        ("udp receive", "fn check(socket: UdpSocket, d: Duration) { result :: socket.receive(1, d) }", "deadline:"),
        ("udp ready", "fn check(socket: UdpSocket, d: Duration) { result :: socket.ready(.Read, potato: d) }", "deadline:"),
        ("unix connect", "fn check(d: Duration) { result :: net.unix_connect(\"/tmp/jet-label-test\", d) }", "deadline:"),
        ("unix accept", "fn check(listener: UnixListener, d: Duration) { result :: listener.accept(banana: d) }", "deadline:"),
        ("unix read", "fn check(stream: UnixStream, d: Duration) { result :: stream.read(1, d) }", "deadline:"),
        ("unix write", "fn check(stream: UnixStream, d: Duration) { result :: stream.write_all([1], potato: d) }", "deadline:"),
        ("unix ready", "fn check(stream: UnixStream, d: Duration) { result :: stream.ready(.Write, d) }", "deadline:"),
        ("tls read", "fn check(stream: TLSStream, d: Duration) { result :: stream.read(1, banana: d) }", "deadline:"),
        ("tls write", "fn check(stream: TLSStream, d: Duration) { result :: stream.write_all([1], d) }", "deadline:"),
        ("tls ready", "fn check(stream: TLSStream, d: Duration) { result :: stream.ready(.Read, potato: d) }", "deadline:"),
        ("tls close write", "fn check(stream: TLSStream, d: Duration) { result :: stream.close_write(d) }", "deadline:"),
        ("tls version bounds", "fn check() { result :: tls.ClientConfig.default().with_version_bounds(.Tls12, .Tls13) }", "min:"),
        ("tls client identity", "fn check() { result :: tls.ClientIdentity.from_pem([], []) }", "cert_chain:"),
        (
            "tls client",
            "fn check(stream: TcpStream, d: Duration) { cfg :: tls.ClientConfig.default(); result :: tls.client(^stream, banana: \"localhost\", potato: cfg, turnip: d) }",
            "server_name:",
        ),
    ];
    for (name, body, expected_fix) in cases {
        let source = format!("use core.net as net\nuse core.tls as tls\n{body}\n");
        let diags = jet::compile(&source).expect_err(name);
        assert!(
            diags.iter().any(|diag| matches!(diag.code.as_str(), "E0764" | "E0769") && diag.fix.contains(expected_fix)),
            "{name} did not reject its missing/wrong label precisely: {diags:?}",
        );
        if name == "tls client" {
            for label in ["server_name:", "config:", "deadline:"] {
                assert!(
                    diags.iter().any(|diag| matches!(diag.code.as_str(), "E0764" | "E0769") && diag.fix.contains(label)),
                    "tls.client accepted or misreported `{label}`: {diags:?}",
                );
            }
        }
        if name == "tls version bounds" {
            for label in ["min:", "max:"] {
                assert!(
                    diags.iter().any(|diag| matches!(diag.code.as_str(), "E0764" | "E0769") && diag.fix.contains(label)),
                    "with_version_bounds accepted or misreported `{label}`: {diags:?}",
                );
            }
        }
        if name == "tls client identity" {
            for label in ["cert_chain:", "private_key:"] {
                assert!(
                    diags.iter().any(|diag| matches!(diag.code.as_str(), "E0764" | "E0769") && diag.fix.contains(label)),
                    "ClientIdentity.from_pem accepted or misreported `{label}`: {diags:?}",
                );
            }
        }
    }
}

#[test]
fn core_net_tcp_read_uses_scheduler_and_returns_typed_cancellation() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_task_cancel_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_task_cancel",
        r#"
use core.net as net
use core.tasks as tasks

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    typed_address :: net.listener_local_socket_addr(listener) ?? panic("address")
    address :: net.socket_to_string(typed_address)
    (ready_tx, ready_rx) :: tasks.channel<Int>()
    server :: task {
        stream := net.tcp_accept(listener) ?? panic("accept")
        ready_tx.send(1)
        if stream.read(1) == {
            .Ok(_) -> print("unexpected read")
            .Err(error) -> print(net.error_message(error))
        }
    }
    _client :: net.tcp_connect(address) ?? panic("connect")
    _ready :: ready_rx.receive() ?? panic("ready")
    server.cancel()
    server.join() ?? 0
}

"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "tcp read cancelled\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_net_tcp_accept_and_ready_are_scheduler_interrupt_points() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_accept_ready_interrupts_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_accept_ready_interrupts",
        r#"
use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {
    cancelled_listener :: net.tcp_listen("127.0.0.1:0") ?? panic("cancel listen")
    cancelled_address :: net.socket_to_string(net.listener_local_socket_addr(cancelled_listener) ?? panic("cancel address"))
    (accept_tx, accept_rx) :: tasks.channel<Int>()
    cancelled_accept :: task {
        accept_tx.send(1)
        if cancelled_listener.accept() == {
            .Ok(_) -> print("accept unexpectedly succeeded")
            .Err(error) -> print(net.error_message(error))
        }
    }
    _accept_ready :: accept_rx.receive() ?? panic("accept ready")
    time.sleep(10)
    cancelled_accept.cancel()
    time.sleep(10)
    release_accept :: net.tcp_connect(cancelled_address) ?? panic("release accept")
    release_accept.close() ?? panic("release close")
    cancelled_accept.join() ?? 0

    ready_listener :: net.tcp_listen("127.0.0.1:0") ?? panic("ready listen")
    ready_address :: net.socket_to_string(net.listener_local_socket_addr(ready_listener) ?? panic("ready address"))
    ready_client :: net.tcp_connect(ready_address) ?? panic("ready connect")
    ready_server := net.tcp_accept(ready_listener) ?? panic("ready accept")
    write_interest :: NetReadyInterest.Write
    write_ready :: ready_server.ready(write_interest, deadline: Duration.milliseconds(1000) ?? panic("write ready deadline")) ?? panic("write ready")
    print(net.ready_readable(write_ready))
    print(net.ready_writable(write_ready))
    interest :: NetReadyInterest.Read
    (wait_tx, wait_rx) :: tasks.channel<Int>()
    ready_wait :: task {
        wait_tx.send(1)
        if ready_server.ready(interest, deadline: Duration.milliseconds(1000) ?? panic("ready deadline")) == {
            .Ok(_) -> print("ready unexpectedly succeeded")
            .Err(error) -> print(net.error_message(error))
        }
    }
    _wait_ready :: wait_rx.receive() ?? panic("wait ready")
    time.sleep(10)
    ready_wait.cancel()
    ready_wait.join() ?? 0
    ready_client.close() ?? panic("ready client close")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "tcp accept cancelled\nfalse\ntrue\ntcp ready cancelled\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_net_udp_loopback_preserves_datagram_truncation_metadata() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_udp_truncation_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_udp_truncation",
        r#"
use core.net as net

fn run() {
    server :: net.udp_bind("127.0.0.1:0") ?? panic("server bind")
    client :: net.udp_bind("127.0.0.1:0") ?? panic("client bind")
    address :: net.udp_local_addr(server) ?? panic("server address")
    budget :: Duration.seconds(1) ?? panic("deadline")
    payload :: [U8].{ 0, 255, 1, 2, 3 }
    sent :: client.send_to(payload, address, deadline: budget) ?? panic("send")
    packet :: server.receive(3, deadline: budget) ?? panic("receive")
    print("{sent}:{net.udp_packet_bytes(packet)}:{net.udp_packet_original_len(packet)}:{net.udp_packet_truncated(packet)}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "5:[0, 255, 1]:5:true\n");
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn core_net_udp_same_handle_readiness_cancels_and_close_is_idempotent() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_udp_ready_close_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_udp_ready_close",
        r#"
use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {
    socket :: net.udp_bind("127.0.0.1:0") ?? panic("bind")
    interest :: NetReadyInterest.Read
    (ready_tx, ready_rx) :: tasks.channel<Int>()
    waiter :: task {
        ready_tx.send(1)
        if socket.ready(interest, deadline: Duration.seconds(1) ?? panic("deadline")) == {
            .Ok(_) -> panic("udp unexpectedly ready")
            .Err(error) -> print(net.error_message(error))
        }
    }
    _ready :: ready_rx.receive() ?? panic("ready")
    time.sleep(10)
    waiter.cancel()
    waiter.join() ?? panic("udp readiness task failed")

    closed :: net.udp_bind("127.0.0.1:0") ?? panic("closed bind")
    closed.close() ?? panic("close")
    closed.close() ?? panic("second close")
    if net.udp_receive(closed, 1) == {
        .Ok(_) -> panic("closed receive succeeded")
        .Err(error) -> print(net.error_message(error))
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "udp ready cancelled\nudp receive failed: socket is closed\n");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn core_net_happy_eyeballs_uses_one_deadline_and_live_loopback() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_happy_eyeballs_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_happy_eyeballs",
        r#"
use core.net as net

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    address :: net.listener_local_socket_addr(listener) ?? panic("address")
    if net.tcp_connect_timeout(address, 0) == {
        .Ok(_) -> panic("expired connect succeeded")
        .Err(error) -> print(net.error_operation(error))
    }
    client :: net.tcp_connect_happy("localhost", net.socket_port(address), 1000) ?? panic("happy connect")
    server := listener.accept() ?? panic("accept")
    client.write_text("happy") ?? panic("write")
    print(server.read_text(5) ?? panic("read"))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "tcp connect\nhappy\n");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn core_net_tcp_per_call_deadlines_bound_accept_read_and_write() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_per_call_deadlines_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_per_call_deadlines",
        r#"
use core.net as net

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    expired :: Duration.milliseconds(0) ?? panic("duration")
    if listener.accept(deadline: expired) == {
        .Ok(_) -> panic("expired accept succeeded")
        .Err(error) -> print(net.error_operation(error))
    }
    address :: net.socket_to_string(net.listener_local_socket_addr(listener) ?? panic("address"))
    client := net.tcp_connect(address) ?? panic("connect")
    server := listener.accept() ?? panic("accept")
    if server.read(1, deadline: expired) == {
        .Ok(_) -> panic("expired read succeeded")
        .Err(error) -> print(net.error_operation(error))
    }
    byte :: [U8].{ 1 }
    if client.write(byte, deadline: expired) == {
        .Ok(_) -> panic("expired write succeeded")
        .Err(error) -> print(net.error_operation(error))
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "tcp accept\ntcp read\ntcp write\n");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn core_net_tcp_implements_nominal_io_reader_writer() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_io_contract_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_io_contract",
        r#"
use core.net as net
use core.tasks as tasks

fn receive<T: Reader>(&stream: T, limit: Int) => [U8] ? IOError {
    return stream.read(limit)
}

fn send_four<T: Writer>(&stream: T) => Int ? IOError {
    stream.write_all([1, 2, 3, 4])?
    return .Ok(4)
}

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    typed_address :: net.listener_local_socket_addr(listener) ?? panic("address")
    address :: net.socket_to_string(typed_address)
    server :: task {
        stream := net.tcp_accept(listener) ?? panic("accept")
        if receive(&stream, 0) == {
            .Ok(_) -> panic("zero limit looked like EOF")
            .Err(_) -> print("invalid")
        }
        bytes :: receive(&stream, 4) ?? panic("read")
        print("read:{bytes.len()}")
        eof :: receive(&stream, 4) ?? panic("eof")
        if eof.len() == 0 { print("eof") }
    }
    client := net.tcp_connect(address) ?? panic("connect")
    _count :: send_four(&client) ?? panic("write")
    client.close() ?? panic("close")
    server.join() ?? 0
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "invalid\nread:4\neof\n");
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn core_net_unix_stream_implements_nominal_io_reader_writer() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_unix_io_contract_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let socket = jet_string_path(&dir.join("stream.sock"));
    let source = format!(
        r#"
use core.net as net
use core.tasks as tasks

fn receive<T: Reader>(&stream: T, limit: Int) => [U8] ? IOError {{
    return stream.read(limit)
}}

fn send_four<T: Writer>(&stream: T) => Int ? IOError {{
    first :: stream.write([1, 2])?
    stream.write_all([3, 4])?
    return .Ok(first)
}}

fn run() {{
    listener :: net.unix_listen("{socket}") ?? panic("listen")
    server :: task {{
        stream := net.unix_accept(listener) ?? panic("accept")
        if receive(&stream, 0) == {{
            .Ok(_) -> panic("zero limit looked like EOF")
            .Err(error) -> {{
                if error == {{
                    .InvalidInput(context) -> print(if context.operation == .Read -> "invalid" else -> "wrong-operation")
                    else -> {{ print("wrong-error") }}
                }}
            }}
        }}
        first :: receive(&stream, 2) ?? panic("first read")
        second :: receive(&stream, 2) ?? panic("second read")
        print("read:{{first.len()}}+{{second.len()}}")
        eof :: receive(&stream, 2) ?? panic("eof")
        if eof.len() == 0 {{ print("eof") }}
        net.unix_write_all_bytes(&stream, [9]) ?? panic("reply")
        net.unix_close(&stream) ?? panic("server close")
    }}
    client := net.unix_connect("{socket}") ?? panic("connect")
    first_count :: send_four(&client) ?? panic("write")
    print("wrote:{{first_count}}")
    net.unix_shutdown(&client, .Write) ?? panic("half close")
    reply :: receive(&client, 1) ?? panic("reply")
    print("reply:{{reply.len()}}")
    if net.unix_write_all_bytes(&client, [5]) == {{
        .Ok(_) -> panic("write after half-close succeeded")
        .Err(error) -> print(if net.error_operation(error) == "unix write" -> "half-closed" else -> "wrong-half-close")
    }}
    net.unix_close(&client) ?? panic("close")
    net.unix_close(&client) ?? panic("second close")
    if receive(&client, 1) == {{
        .Ok(_) -> panic("closed read succeeded")
        .Err(error) -> {{
            if error == {{
                .Closed(context) -> print(if context.operation == .Read -> "closed" else -> "wrong-close-operation")
                else -> {{ print("wrong-close-error") }}
            }}
        }}
    }}
    server.join() ?? 0
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "unix_io_contract", &source, &[], None);
    assert_eq!(code, 0, "{stderr}");
    let mut lines: Vec<_> = stdout.lines().collect();
    lines.sort_unstable();
    assert_eq!(lines, ["closed", "eof", "half-closed", "invalid", "read:2+2", "reply:1", "wrote:2"]);
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn core_net_unix_same_handle_deadline_readiness_and_close() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_unix_same_handle_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let socket = jet_string_path(&dir.join("same-handle.sock"));
    let source = format!(
        r#"
use core.net as net

fn run() {{
    listener :: net.unix_listen("{socket}") ?? panic("listen")
    budget :: Duration.seconds(1) ?? panic("budget")
    client := net.unix_connect("{socket}", deadline: budget) ?? panic("connect")
    server := listener.accept(deadline: budget) ?? panic("accept")
    client.set_timeout(budget) ?? panic("persistent timeout")
    both :: NetReadyInterest.ReadWrite
    observed :: client.ready(both, deadline: budget) ?? panic("read-write readiness")
    print(net.ready_readable(observed))
    print(net.ready_writable(observed))
    interest :: NetReadyInterest.Read
    expired :: Duration.milliseconds(0) ?? panic("expired")
    if client.ready(interest, deadline: expired) == {{
        .Ok(_) -> panic("expired readiness succeeded")
        .Err(error) -> print(net.error_operation(error))
    }}
    payload :: [U8].{{ 7 }}
    client.write_all(payload, deadline: budget) ?? panic("write")
    print(server.read(1, deadline: budget) ?? panic("read"))
    client.close() ?? panic("close")
    client.close() ?? panic("second close")
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "unix_same_handle", &source, &[], None);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "false\ntrue\nunix ready\n[7]\n");
    let _ = fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn core_net_udp_and_unix_waits_use_typed_scheduler_interrupts() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_datagram_unix_interrupts_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let socket = jet_string_path(&dir.join("interrupt.sock"));
    let source = format!(
        r#"
use core.net as net
use core.tasks as tasks

fn run() {{
    udp_timeout :: net.udp_bind("127.0.0.1:0") ?? panic("udp timeout bind")
    net.udp_set_timeout(udp_timeout, 20) ?? panic("udp timeout")
    if net.udp_receive(udp_timeout, 8) == {{
        .Ok(_) -> panic("udp timeout returned data")
        .Err(error) -> print(net.error_message(error))
    }}

    udp :: net.udp_bind("127.0.0.1:0") ?? panic("udp bind")
    (udp_ready_tx, udp_ready_rx) :: tasks.channel<Int>()
    udp_wait :: task {{
        udp_ready_tx.send(1)
        if net.udp_receive(udp, 8) == {{
            .Ok(_) -> panic("udp cancel returned data")
            .Err(error) -> print(net.error_message(error))
        }}
    }}
    _udp_ready :: udp_ready_rx.receive() ?? panic("udp ready")
    udp_wait.cancel()
    udp_wait.join() ?? panic("udp wait task failed")

    listener :: net.unix_listen("{socket}") ?? panic("unix listen")
    (unix_ready_tx, unix_ready_rx) :: tasks.channel<Int>()
    unix_wait :: task {{
        unix_ready_tx.send(1)
        if net.unix_accept(listener) == {{
            .Ok(_) -> panic("unix cancel accepted stream")
            .Err(error) -> print(net.error_message(error))
        }}
    }})
    _unix_ready :: unix_ready_rx.receive() ?? panic("unix ready")
    unix_wait.cancel()
    unix_wait.join() ?? panic("udp wait task failed")
}}
"#
    );
    let (code, stdout, stderr) =
        build_and_run(&dir, "net_datagram_unix_interrupts", &source, &[], None);
    assert_eq!(code, 0, "{stderr}");
    let mut lines: Vec<_> = stdout.lines().collect();
    lines.sort_unstable();
    assert_eq!(
        lines,
        ["deadline exceeded while waiting in udp receive", "udp receive cancelled", "unix accept cancelled"]
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_ioerror_preserves_kind_operation_and_resource() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_ioerror_tree_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.files as fs
use core.net as net
use core.process as process

fn receive<T: Reader>(&stream: T, limit: Int) => [U8] ? IOError {
    return stream.read(limit)
}

fn operation_name(operation: IOOperation) => String {
    if operation == {
        .Read -> return "read"
        .Write -> return "write"
        .Flush -> return "flush"
        .Connect -> return "connect"
        .Accept -> return "accept"
        .Close -> return "close"
        .Resolve -> return "resolve"
        .Codec -> return "codec"
    }
}

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    address :: net.socket_to_string(net.listener_local_socket_addr(listener) ?? panic("address"))
    client := net.tcp_connect(address) ?? panic("connect")
    if receive(&client, 0) == {
        .Ok(_) -> panic("zero read succeeded")
        .Err(error) -> {
            if error == {
                .InvalidInput(context) -> print(if operation_name(context.operation) == "read" -> "invalid-read" else -> "invalid-other")
                else -> { print("other") }
            }
        }
    }
    if fs.read("definitely-missing/ioerror-tree") == {
        .Ok(_) -> panic("missing file read succeeded")
        .Err(error) -> {
            if error == {
                .NotFound(context) -> print(context.resource ?? "missing-resource")
                else -> { print("other") }
            }
        }
    }
    if fs.write(".", "cannot replace directory") == {
        .Ok(_) -> panic("directory write succeeded")
        .Err(error) -> {
            if error == {
                .Other(context) -> print(if operation_name(context.operation) == "write" -> "write" else -> "wrong-write-operation")
                else -> { print("wrong-write-kind") }
            }
        }
    }
    if process.cmd([]).run() == {
        .Ok(_) -> panic("empty command succeeded")
        .Err(error) -> {
            if error == {
                .InvalidInput(context) -> print(if operation_name(context.operation) == "resolve" -> "empty-command" else -> "wrong-command-operation")
                else -> { print("wrong-command-kind") }
            }
        }
    }
    if process.pipeline([]) == {
        .Ok(_) -> panic("empty pipeline succeeded")
        .Err(error) -> {
            if error == {
                .InvalidInput(context) -> print(if operation_name(context.operation) == "resolve" -> "empty-pipeline" else -> "wrong-pipeline-operation")
                else -> { print("wrong-pipeline-kind") }
            }
        }
    }
    if process.cmd(["unused"]).env("BAD=NAME", "value").run() == {
        .Ok(_) -> panic("invalid environment succeeded")
        .Err(error) -> {
            if error == {
                .InvalidInput(context) -> print(if operation_name(context.operation) == "resolve" -> context.resource ?? "missing-env-resource" else -> "wrong-env-operation")
                else -> { print("wrong-env-kind") }
            }
        }
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "ioerror_tree",
        source,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    let expected = "invalid-read\ndefinitely-missing/ioerror-tree\nwrite\nempty-command\nempty-pipeline\nBAD=NAME\n";
    assert_eq!(stdout, expected);
    let file = dir.join("ioerror_tree.jet");
    fs::write(&file, source).unwrap();
    match jet::Interpreter::dev_iteration(file.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!((exit_code, stdout.as_str(), stderr.as_str()), (0, expected, ""));
        }
        other => panic!("IOError tree did not run in default dev: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_ioerror_debug_renders_in_aot_and_dev() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_ioerror_debug_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    // Direct core values alone do not emit `jet_std`; this unused helper keeps the AOT prelude present.
    let source = r#"
fn activate_core() => String {
    return input() ?? ""
}

fn fail() => Int ? IOError {
    return .Err(IOError.InvalidInput(IOContext.{
        operation: .Read,
        resource: None,
        os_code: None,
        cause: Val("debug"),
    }))
}

fn fail_other() => Int ? IOError {
    return .Err(IOError.Other(IOContext.{
        cause: Val("denied"),
        os_code: Val(13),
        resource: Val("out.txt"),
        operation: .Write,
    }))
}

fn run() {
    print("{42:Debug}")
    debug :: "debug"
    print("{debug:Debug}")
    print("{[1, 2, 3]:Debug}")
    if fail() == {
        .Ok(_) -> panic("failure succeeded")
        .Err(error) -> {
            print("{error:Debug}")
            print("{error}")
        }
    }
    if fail_other() == {
        .Ok(_) -> panic("other failure succeeded")
        .Err(error) -> {
            print("{error:Debug}")
            print("{error}")
        }
    }
}
"#;
    let expected = concat!(
        "42\n",
        "\"debug\"\n",
        "[1, 2, 3]\n",
        "InvalidInput(IOContext { operation: Read, resource: None, os_code: None, cause: Val(\"debug\") })\n",
        "invalid input during read: debug\n",
        "Other(IOContext { operation: Write, resource: Val(\"out.txt\"), os_code: Val(13), cause: Val(\"denied\") })\n",
        "I/O error during write `out.txt`: denied\n",
    );
    let (code, aot_stdout, stderr) = build_and_run(
        &dir,
        "ioerror_debug",
        source,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(aot_stdout, expected);
    assert!(!aot_stdout.contains("<enum IOOperation>"));
    let file = dir.join("ioerror_debug.jet");
    fs::write(&file, source).unwrap();
    jet_jit::reset_jit_trace_for_test();
    let default_jit_stdout = match jet::Interpreter::dev_iteration(file.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!((exit_code, stdout.as_str(), stderr.as_str()), (0, expected, ""));
            stdout
        }
        other => panic!("IOError Show/Debug did not run in default JIT: {other:?}"),
    };
    assert!(
        jet_jit::jit_executed_for_test(),
        "IOError Show/Debug must execute in resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test(),
        "IOError Show/Debug must not deopt"
    );
    assert!(
        !jet_jit::fallback_invoked_for_test(),
        "IOError Show/Debug must not fall back"
    );

    let forced_interpreter_stdout = match jet::Interpreter::dev_iteration(file.to_str().unwrap(), false, true) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!((exit_code, stdout.as_str(), stderr.as_str()), (0, expected, ""));
            stdout
        }
        other => panic!("IOError Show/Debug did not run in forced interpreter: {other:?}"),
    };
    assert_eq!(default_jit_stdout, aot_stdout);
    assert_eq!(forced_interpreter_stdout, aot_stdout);
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(target_os = "linux")]
#[test]
fn core_ioerror_native_flush_preserves_operation() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_ioerror_flush_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "ioerror_flush",
        r#"
use core.files as files

fn run() {
    output := files.create("/dev/full") ?? panic("open")
    output.write_line("buffered") ?? panic("buffer")
    if output.flush() == {
        .Ok(_) -> panic("flush succeeded")
        .Err(error) -> {
            if error == {
                .Other(context) -> print(if context.operation == .Flush -> "flush" else -> "wrong-flush-operation")
                else -> { print("wrong-flush-kind") }
            }
        }
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "flush\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_net_tcp_read_persistent_timeout_uses_scheduler_budget() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_task_timeout_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_task_timeout",
        r#"
use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    typed_address :: net.listener_local_socket_addr(listener) ?? panic("address")
    address :: net.socket_to_string(typed_address)
    client :: task {
        stream := net.tcp_connect(address) ?? panic("connect")
        time.sleep(100)
        stream.close() ?? panic("close")
    }
    stream := net.tcp_accept(listener) ?? panic("accept")
    net.set_read_timeout(&stream, 20) ?? panic("timeout")
    if stream.read(1) == {
        .Ok(_) -> print("unexpected read")
        .Err(error) -> print(net.error_message(error))
    }
    client.join() ?? 0
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("deadline exceeded while waiting in tcp read"), "{stdout}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_net_tcp_expired_deadlines_return_typed_timeouts() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_expired_deadlines_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_expired_deadlines",
        r#"
use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    typed_address :: net.listener_local_socket_addr(listener) ?? panic("address")
    address :: net.socket_to_string(typed_address)
    server :: task {
        first := net.tcp_accept(listener) ?? return
        time.sleep(100)
        first.close() ?? return
        second := net.tcp_accept(listener) ?? return
        time.sleep(100)
        second.close() ?? return
    }

    first := net.tcp_connect(address) ?? panic("first connect")
    net.set_read_timeout(&first, 0) ?? panic("zero timeout")
    if first.read(1) == {
        .Ok(_) -> print("unexpected first read")
        .Err(error) -> print(net.error_message(error))
    }
    first.close() ?? panic("first close")

    second := net.tcp_connect(address) ?? panic("second connect")
    #Context(deadline: time.now() - 1) {
        if second.read(1) == {
            .Ok(_) -> print("unexpected second read")
            .Err(error) -> print(net.error_message(error))
        }
    }
    second.close() ?? panic("second close")
    server.join() ?? 0
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        "deadline exceeded while waiting in tcp read\ndeadline exceeded while waiting in tcp read\n"
    );
    assert!(!stderr.contains("E3003"), "typed timeout escaped as runtime deadline: {stderr}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_net_tcp_write_all_uses_one_absolute_deadline() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_write_deadline_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_write_deadline",
        r#"
use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    typed_address :: net.listener_local_socket_addr(listener) ?? panic("address")
    address :: net.socket_to_string(typed_address)
    server :: task {
        stream := net.tcp_accept(listener) ?? return
        loop {
            chunk := stream.read(65536) ?? return
            if chunk.len() == 0 {
                return
            }
            time.sleep(15)
        }
    }
    client := net.tcp_connect(address) ?? panic("connect")
    net.set_write_timeout(&client, 80) ?? panic("timeout")
    started := time.now()
    if client.write_text("x".repeat(16000000)) == {
        .Ok(_) -> print("unexpected write")
        .Err(error) -> print(net.error_message(error))
    }
    elapsed := time.now() - started
    print(elapsed < 300)
    client.close() ?? panic("close")
    server.join() ?? 0
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "deadline exceeded while waiting in tcp write\ntrue\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_tls_byte_stream_runs_real_local_handshake_and_close_notify() {
    let dir = std::env::temp_dir().join(format!("jet_core_tls_surface_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ca_cert = root.join("tests/fixtures/tls/localhost.cert.pem");
    let ca_key = root.join("tests/fixtures/tls/localhost.key.pem");
    let cert = dir.join("leaf.cert.pem");
    let key = dir.join("leaf.key.pem");
    let csr = dir.join("leaf.csr.pem");
    let extensions = dir.join("leaf.ext");
    fs::write(&extensions, "basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\n").unwrap();
    let req = Command::new("openssl").args(["req", "-new", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=localhost", "-keyout"])
        .arg(&key).arg("-out").arg(&csr).output().unwrap();
    assert!(req.status.success(), "{}", String::from_utf8_lossy(&req.stderr));
    let sign = Command::new("openssl").args(["x509", "-req", "-days", "1", "-set_serial", "2", "-CA"])
        .arg(&ca_cert).arg("-CAkey").arg(&ca_key).arg("-extfile").arg(&extensions)
        .arg("-in").arg(&csr).arg("-out").arg(&cert).output().unwrap();
    assert!(sign.status.success(), "{}", String::from_utf8_lossy(&sign.stderr));
    let mut server = Command::new("openssl")
        .args(["s_server", "-quiet", "-www", "-alpn", "http/1.0", "-accept", &port.to_string(), "-cert"])
        .arg(&cert)
        .arg("-key")
        .arg(&key)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(250));
    let src = r#"
use core.net as net
use core.tls as tls

fn receive<T: Reader>(&stream: T, limit: Int) => [U8] ? IOError {
    return stream.read(limit)
}


fn send<T: Writer>(&stream: T, bytes: [U8]) => Int ? IOError {
    empty_count :: stream.write([])?
    stream.write_all(bytes)?
    return .Ok(empty_count)
}

fn zero_rejected<T: Reader>(&stream: T) => Bool {
    if stream.read(0) == {
        .Ok(_) -> return false
        .Err(error) -> {
            if error == {
                .InvalidInput(context) -> return context.operation == .Read
                else -> { return false }
            }
        }
    }
    return false
}

fn run() {
    tcp :: net.tcp_connect("127.0.0.1:$PORT") ?? panic("tcp")
    budget :: Duration.seconds(1) ?? panic("deadline")
    cfg :: tls.ClientConfig.default().with_alpn(["http/1.0"]) ?? panic("ALPN")
    secure := tls.client(^tcp, server_name: "localhost", config: cfg, deadline: budget) ?? panic("tls handshake")
    request :: [U8].{ 71, 69, 84, 32, 47, 32, 72, 84, 84, 80, 47, 49, 46, 48, 13, 10, 13, 10 }
    interest :: NetReadyInterest.Write
    readiness :: secure.ready(interest, deadline: budget) ?? panic("ready")
    print(net.ready_readable(readiness))
    print(net.ready_writable(readiness))
    print(zero_rejected(&secure))
    empty :: [U8].{}
    empty_count :: send(&secure, empty) ?? panic("empty write")
    secure.write_all(request, deadline: budget) ?? panic("write bytes")
    print(empty_count)
    read_interest :: NetReadyInterest.Read
    response_ready :: secure.ready(read_interest, deadline: budget) ?? panic("response ready")
    print(net.ready_readable(response_ready))
    response :: secure.read(4096, deadline: budget) ?? panic("read bytes")
    print(response.len() > 0)
    secure.close() ?? panic("close notify")
    secure.close() ?? panic("idempotent close")
    if receive(&secure, 1) == {
        .Ok(_) -> panic("closed read succeeded")
        .Err(error) -> {
            if error == {
                .Closed(context) -> print(if context.operation == .Read -> "closed" else -> "wrong-operation")
                else -> { print("wrong-error") }
            }
        }
    }
}
"#.replace("$PORT", &port.to_string());
    let cert_text = ca_cert.to_string_lossy().into_owned();
    let (code, stdout, stderr) = build_and_run(&dir, "tls_byte_surface", &src, &[("SSL_CERT_FILE", &cert_text)], None);
    let _ = server.kill();
    let _ = server.wait();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "false\ntrue\ntrue\n0\ntrue\ntrue\nclosed\n");
}
#[test]
fn core_tls_expert_config_peer_identity_and_directional_close_are_real() {
    let dir = std::env::temp_dir().join(format!("jet_core_tls_expert_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ca_cert = root.join("tests/fixtures/tls/localhost.cert.pem");
    let ca_key = root.join("tests/fixtures/tls/localhost.key.pem");
    let serial = dir.join("ca.srl");
    let make_cert = |name: &str, usage: &str| {
        let cert = dir.join(format!("{name}.cert.pem"));
        let key = dir.join(format!("{name}.key.pem"));
        let csr = dir.join(format!("{name}.csr.pem"));
        let ext = dir.join(format!("{name}.ext"));
        fs::write(
            &ext,
            format!("basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost\nextendedKeyUsage={usage}\n"),
        ).unwrap();
        let mut request = Command::new("openssl");
        request.args(["req", "-new", "-newkey", "rsa:2048", "-nodes"]);
        if name == "localhost" {
            let config = dir.join("legacy-dn.cnf");
            fs::write(
                &config,
                "[req]\nprompt=no\ndistinguished_name=dn\nstring_mask=default\n[dn]\nCN=Télét\n",
            ).unwrap();
            request.arg("-config").arg(config);
        } else {
            request.arg("-subj").arg(format!("/CN={name}"));
        }
        let req = request.arg("-keyout").arg(&key).arg("-out").arg(&csr).output().unwrap();
        assert!(req.status.success(), "{}", String::from_utf8_lossy(&req.stderr));
        let sign = Command::new("openssl")
            .args(["x509", "-req", "-days", "1", "-CAcreateserial", "-CAserial"])
            .arg(&serial).arg("-CA")
            .arg(&ca_cert).arg("-CAkey").arg(&ca_key).arg("-extfile").arg(&ext)
            .arg("-in").arg(&csr).arg("-out").arg(&cert).output().unwrap();
        assert!(sign.status.success(), "{}", String::from_utf8_lossy(&sign.stderr));
        (cert, key)
    };
    let (server_cert, server_key) = make_cert("localhost", "serverAuth");
    let (client_cert, client_key) = make_cert("jet-client", "clientAuth");
    let parsed = Command::new("openssl").args(["asn1parse", "-in"])
        .arg(&server_cert).output().unwrap();
    assert!(parsed.status.success(), "{}", String::from_utf8_lossy(&parsed.stderr));
    assert!(String::from_utf8_lossy(&parsed.stdout).contains("T61STRING"));
    let mut server = Command::new("openssl")
        .args(["s_server", "-quiet", "-www", "-alpn", "http/1.0", "-Verify", "1", "-verify_return_error", "-accept", &port.to_string(), "-CAfile"])
        .arg(&ca_cert).arg("-cert").arg(&server_cert).arg("-key").arg(&server_key)
        .arg("-cert_chain").arg(&ca_cert)
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(250));
    let jet_bytes = |path: &std::path::Path| {
        fs::read(path)
            .unwrap()
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let source = format!(r#"
use core.net as net
use core.tls as tls

fn invalid_alpn() => [String] {{
    return [""]
}}

fn run() {{
    ca :: [U8].{{ {} }}
    client_cert :: [U8].{{ {} }}
    client_key :: [U8].{{ {} }}
    wrong_key :: [U8].{{ {} }}
    roots :: tls.RootCertificates.from_pem(ca) ?? panic("root validation")
    identity :: tls.ClientIdentity.from_pem(cert_chain: client_cert, private_key: client_key) ?? panic("identity validation")
    if tls.ClientIdentity.from_pem(cert_chain: client_cert, private_key: wrong_key) == {{
        .Ok(_) -> panic("mismatched identity accepted")
        .Err(_) -> print("mismatch-rejected")
    }}
    if tls.ClientConfig.default().with_version_bounds(min: .Tls13, max: .Tls12) == {{
        .Ok(_) -> panic("reversed TLS versions accepted")
        .Err(_) -> print("bounds-rejected")
    }}
    _plus :: tls.ClientConfig.default().with_trust(.SystemPlus(roots)) ?? panic("system plus")
    cfg0 :: tls.ClientConfig.default().with_trust(.CustomOnly(roots)) ?? panic("custom trust")
    cfg1 :: cfg0.with_client_identity(identity) ?? panic("client identity")
    cfg2 :: cfg1.with_version_bounds(min: .Tls12, max: .Tls13) ?? panic("version bounds")
    tcp :: net.tcp_connect("127.0.0.1:{}") ?? panic("tcp")
    if cfg2.with_alpn(invalid_alpn()) == {{
        .Ok(_) -> panic("empty dynamic ALPN accepted")
        .Err(error) -> if error == {{
            .InvalidInput(context) -> print(if context.operation == .Connect -> "alpn-rejected" else -> "wrong-alpn-operation")
            else -> {{ panic("wrong ALPN error") }}
        }}
    }}
    cfg :: cfg2.with_alpn(["http/1.0"]) ?? panic("ALPN")
    budget :: Duration.seconds(2) ?? panic("budget")
    secure := tls.client(^tcp, server_name: "localhost", config: cfg, deadline: budget) ?? panic("mTLS")
    peer :: secure.peer_identity()
    print(peer.verified_server_name)
    print(peer.certificate_chain.len() == 2)
    print(peer.leaf.der == peer.certificate_chain[0].der)
    print(peer.leaf.der.len() > 0)
    print(peer.leaf.sha256.len())
    print(peer.leaf.spki_sha256.len())
    print(peer.leaf.dns_names.contains("localhost"))
    print(peer.leaf.valid_from_unix_ms < peer.leaf.valid_until_unix_ms)
    print(peer.leaf.subject.contains("CN=T") && peer.leaf.subject.contains("\\xc3"))
    print(peer.leaf.issuer.len() > 0)
    request :: [U8].{{ 71, 69, 84, 32, 47, 32, 72, 84, 84, 80, 47, 49, 46, 48, 13, 10, 13, 10 }}
    secure.write_all(request, deadline: budget) ?? panic("request")
    secure.close_write(deadline: budget) ?? panic("close write")
    secure.close_write(deadline: budget) ?? panic("repeat close write")
    one :: [U8].{{ 1 }}
    if secure.write_all(one, deadline: budget) == {{
        .Ok(_) -> panic("write after close_write succeeded")
        .Err(error) -> if error == {{
            .Closed(context) -> print(if context.operation == .Write -> "write-closed" else -> "wrong-write-operation")
            else -> {{ panic("wrong post-close error") }}
        }}
    }}
    total := 0
    loop {{
        chunk :: secure.read(4096, deadline: budget) ?? panic("response read")
        if chunk.is_empty() {{ break }}
        total += chunk.len()
    }}
    print(total > 0)
    secure.close() ?? panic("close")
    print(secure.peer_identity().verified_server_name)
}}
"#,
        jet_bytes(&ca_cert),
        jet_bytes(&client_cert),
        jet_bytes(&client_key),
        jet_bytes(&server_key),
        port,
    );
    let (code, stdout, stderr) = build_and_run(&dir, "tls_expert_surface", &source, &[], None);
    let _ = server.kill();
    let _ = server.wait();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        "mismatch-rejected\nbounds-rejected\nalpn-rejected\nlocalhost\ntrue\ntrue\ntrue\n32\n32\ntrue\ntrue\ntrue\ntrue\nwrite-closed\ntrue\nlocalhost\n",
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn core_tls_identity_drop_and_protocol_mapping_use_shared_runtime_laws() {
    let dir = std::env::temp_dir().join(format!("jet_core_tls_runtime_laws_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let compiled = compile_temp(
        "tls_runtime_laws.jet",
        "use core.tls as tls\nfn run() { _config :: tls.ClientConfig.default() }\n",
    );
    let mut rust = standalone_tls_probe_source(compiled.rust);
    rust.push_str(r#"
fn main() {
    let zeroized = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<u8>>::new()));
    let observed = std::rc::Rc::clone(&zeroized);
    jet_crypto_entropy_set_zeroize_test_observer(move |bytes| {
        observed.borrow_mut().push(bytes.to_vec());
    });
    {
        let identity = JetTLSClientIdentity {
            cert_chain: vec![1, 2, 3],
            private_key: JetCryptoSecretBytes::new(vec![0xa5; 7]),
        };
        let config = jet_tls_client_config_with_client_identity(
            jet_tls_client_config_default(),
            &identity,
        ).unwrap();
        assert!(jet_tls_client_config_with_version_bounds(
            config,
            JetTLSVersion::Tls13,
            JetTLSVersion::Tls12,
        ).is_err());
    }
    jet_crypto_entropy_clear_zeroize_test_observer();
    assert_eq!(&*zeroized.borrow(), &vec![vec![0; 7], vec![0; 7]]);

    let cause = "TLS protocol truncation: peer closed without close-notify".to_string();
    match jet_net_tls_io_result::<()>(Err(cause.clone()), jet_std::IOOperation::Read).unwrap_err() {
        jet_std::IOError::Protocol(context) => {
            assert_eq!(context.operation, jet_std::IOOperation::Read);
            assert_eq!(context.cause, Ok(cause));
        }
        other => panic!("expected Protocol(Read), got {other:?}"),
    }
}
"#);
    let rs = dir.join("runtime_laws.rs");
    let bin = dir.join("runtime_laws");
    let mut rustc = Command::new("rustc");
    common::add_generated_rust(
        &mut rustc,
        &rs,
        &rust,
        false,
        &["--cfg", "test"],
    );
    rustc.arg("-o").arg(&bin);
    let built = rustc.output().unwrap();
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let ran = Command::new(bin).output().unwrap();
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn core_tls_stalled_handshake_observes_timeout_and_cancellation() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_tls_stalled_handshake_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut peers = Vec::new();
        for _ in 0..2 {
            let (peer, _) = listener.accept().unwrap();
            peers.push(peer);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    });
    let source = format!(
        r#"
use core.net as net
use core.tasks as tasks
use core.tls as tls

fn run() {{
    timed := net.tcp_connect("{address}") ?? panic("timeout tcp")
    net.set_timeout(&timed, 30) ?? panic("timeout budget")
    if net.tls_connect(^timed, "localhost") == {{
        .Ok(_) -> panic("stalled handshake succeeded")
        .Err(error) -> print("{{net.error_operation(error)}}:{{net.error_message(error)}}")
    }}

    (ready_tx, ready_rx) :: tasks.channel<Int>()
    blocked :: task {{
        tcp := net.tcp_connect("{address}") ?? panic("cancel tcp")
        ready_tx.send(1)
        if tls.client(^tcp, "localhost") == {{
            .Ok(_) -> panic("cancelled handshake succeeded")
            .Err(error) -> print("{{net.error_operation(error)}}:{{net.error_message(error)}}")
        }}
    }})
    _ready :: ready_rx.receive() ?? panic("ready")
    blocked.cancel()
    blocked.join() ?? 0
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "tls_stalled_handshake", &source, &[], None);
    server.join().unwrap();
    assert_eq!(code, 0, "{stderr}");
    let mut lines: Vec<_> = stdout.lines().collect();
    lines.sort_unstable();
    assert_eq!(
        lines,
        [
            "tls handshake:deadline exceeded while waiting in tls handshake",
            "tls handshake:tls handshake cancelled",
        ]
    );
    let _ = fs::remove_dir_all(dir);
}
