mod common;
mod tir_support;

const INVALID_DNS_LABEL_SOURCE: &str = r#"
use core.net.tls as tls

fn run() !IOError -[Net, Time.Wait]> {
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
            tir_support::jit_run("core_net_tls_invalid_dns_label", INVALID_DNS_LABEL_SOURCE),
        ),
        (
            "forced interpreter",
            tir_support::interpreter_run("core_net_tls_invalid_dns_label", INVALID_DNS_LABEL_SOURCE),
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
