use std::process::Command;

mod common;
use common::{build_and_run, have_rustc, FfiBridgeLock};

fn have_cargo() -> bool {
    Command::new("cargo").arg("--version").output().is_ok()
}

fn jet_string(s: &str) -> String {
    format!("{s:?}")
}

#[test]
fn server_tls_option_validates_fixture_before_binding() {
    if !have_rustc() || !have_cargo() {
        eprintln!("note: skipping server TLS bridge test (need cargo + rustc)");
        return;
    }
    let _lock = FfiBridgeLock::acquire();
    let cert = jet_string(include_str!("../tests/fixtures/tls/localhost.cert.pem"));
    let key = jet_string(include_str!("../tests/fixtures/tls/localhost.key.pem"));
    let src = format!(
        r#"
use core.http.server as Server

fn run() {{
    mux :: Server.mux()
    result :: Server.serve("127.0.0.1:999999", mux, tls: Server.tls({cert}, {key}))
    if result == {{
        ok(_) -> {{ print("unexpected") }}
        err(e) -> {{ print(e) }}
    }}
}}
"#
    );

    let (code, stdout, stderr) = build_and_run("jet_http_server_tls", "valid_fixture", &src);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains("bind on `127.0.0.1:999999` failed"),
        "fixture should validate before controlled bind failure:\nstdout={stdout}\nstderr={stderr}"
    );
    assert!(!stdout.contains("TLS certificate"), "{stdout}");
}

#[test]
fn server_tls_bad_cert_is_jet_voiced() {
    if !have_rustc() || !have_cargo() {
        eprintln!("note: skipping server TLS bridge test (need cargo + rustc)");
        return;
    }
    let _lock = FfiBridgeLock::acquire();
    let src = r#"
use core.http.server as Server

fn run() {
    mux :: Server.mux()
    result :: Server.serve("127.0.0.1:999999", mux, tls: Server.tls("not a cert", "not a key"))
    if result == {
        ok(_) -> { print("unexpected") }
        err(e) -> { print(e) }
    }
}
"#;

    let (code, stdout, stderr) = build_and_run("jet_http_server_tls", "bad_cert", src);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout.trim(),
        "TLS certificate PEM did not contain a certificate"
    );
}

#[test]
fn server_tls_requires_named_tls_option() {
    let src = r#"
use core.http.server as Server

fn run() {
    mux :: Server.mux()
    _ :: Server.serve("127.0.0.1:0", mux, Server.tls("cert", "key"))
}
"#;

    let err = jet::compile(src)
        .expect_err("unlabeled third server argument should be rejected");
    let rendered = jet::render_diagnostics("tests/http_server_tls/bad_label.jet", src, &err);
    assert!(rendered.contains("Error [E0125]"), "{rendered}");
    assert!(
        rendered.contains("`serve` needs `tls:` before the third argument"),
        "{rendered}"
    );
}
