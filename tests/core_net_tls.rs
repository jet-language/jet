mod common;
mod tir_support;

#[test]
fn core_net_dns_error_uses_canonical_acronym_type() {
    let source = r#"
use core.net as net

fn wrap_dns(error: NetDNSError) -> NetError Never! {
    NetError.DNS(error)
}

fn run() {
    _ :: wrap_dns(NetDNSError.NotFound("example.test"))
    _ :: wrap_dns(NetDNSError.Failure("resolver unavailable"))
}
"#;
    tir_support::compile_source("core_net_dns_error_canonical", source)
        .expect("core.net and both DNS variants must check without E0358");
}

const INVALID_DNS_LABEL_SOURCE: &str = r#"
use core.net.tls as tls

fn run() IOError! -[Net, Time.Wait]> {
    _ :: tls.client("bad..example", 443)
}
"#;

#[test]
fn tls_rejects_empty_dns_labels_before_transport_on_all_tiers() {
    let expected_error =
        "invalid input during connect `bad..example`: host is empty or contains invalid endpoint syntax";
    let mut tiers = vec![
        (
            "default run",
            tir_support::default_run("core_net_tls_invalid_dns_label", INVALID_DNS_LABEL_SOURCE),
        ),
        (
            "forced interpreter",
            tir_support::default_run("core_net_tls_invalid_dns_label", INVALID_DNS_LABEL_SOURCE),
        ),
    ];
    if tir_support::have_rustc() {
        tiers.push((
            "AOT",
            tir_support::build_and_run_full(
                "jet_core_net_tls_invalid_dns_label",
                "core_net_tls_invalid_dns_label",
                INVALID_DNS_LABEL_SOURCE,
            ),
        ));
    }

    let baseline = &tiers[0].1;
    for (tier, result) in &tiers {
        assert_eq!(result, baseline, "{tier} disagreed on the TLS error");
        assert_eq!(result.0, 1, "{tier} must report the invalid server name");
        assert!(result.1.is_empty(), "{tier} wrote stdout: {:?}", result.1);
        assert!(
            result.2.contains(expected_error),
            "{tier} lost TLS error classification or context:\n{}",
            result.2
        );
    }
}

#[test]
fn tcp_stream_and_listener_reject_public_fields_and_forged_records() {
    let source = r#"
use core.net as net

fn run() {
    stream :: net.tcp_connect("127.0.0.1:1") ?? panic("connect")
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    print(stream.fd)
    print(stream.host)
    print(stream.port)
    print(listener.fd)
    print(listener.host)
    print(listener.port)
    _ :: net.TCPStream{fd: 0, host: "forged", port: 1}
    _ :: net.TCPListener{fd: 0, host: "forged", port: 1}
}
"#;
    let diagnostics = tir_support::compile_source("private_tcp_handles", source)
        .expect_err("public TCP fields and fabricated handles must be rejected");
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "E0605")
            .count(),
        12,
        "all six private TCP field reads and six forged record fields must be rejected: {diagnostics:?}"
    );
}

#[test]
fn configured_tls_client_keeps_native_builder_identity_and_exact_labels() {
    let source = r#"
use core.net as net
use core.net.tls as tls

fn run() -[Net, Time.Wait]> {
    roots :: tls.RootCertificates.from_pem("invalid".bytes()) ?? return
    config :: tls.ClientConfig.default().with_trust(.CustomOnly(roots)) ?? return
    bounded :: config.with_version_bounds(min: TLSVersion.TLS13, max: TLSVersion.TLS13) ?? return
    protocols :: bounded.with_alpn(["http/1.1"]) ?? return
    tcp :: net.tcp_connect("127.0.0.1:1") ?? return
    _ :: tls.client(^tcp, server_name: "localhost", config: protocols, deadline: 1s)
}
"#;
    tir_support::compile_source("configured_tls_client", source)
        .expect("source endpoint convenience must not shadow the consumed-stream overload");
    let bad_labels = source.replace("server_name:", "host:");
    let diagnostics = tir_support::compile_source("configured_tls_client_bad_labels", &bad_labels)
        .expect_err("configured TLS client must require its ratified exact labels");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E0764"
                && diagnostic.what == "`tls.client` has no parameter labelled `host`"
        }),
        "the canonical call binder must reject the substituted label: {diagnostics:?}"
    );
}

#[test]
fn tls_pem_builders_validate_before_transport_in_aot() {
    if !tir_support::have_rustc() {
        return;
    }
    let source = r#"
use core.net.tls as tls

fn run() -[IO, Net]> {
    print(tls.RootCertificates.from_pem("not PEM".bytes()) == .Err(_))
    print(tls.ClientIdentity.from_pem(cert_chain: "not PEM".bytes(), private_key: "not PEM".bytes()) == .Err(_))
    print(tls.ClientConfig.default().with_alpn([""]) == .Err(_))
    print(tls.ClientConfig.default().with_version_bounds(min: TLSVersion.TLS13, max: TLSVersion.TLS12) == .Err(_))
}
"#;
    let (code, stdout, stderr) =
        tir_support::build_and_run_full("jet_tls_pem_validation", "invalid_pem", source);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "true\ntrue\ntrue\ntrue\n");
}
