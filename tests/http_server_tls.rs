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
use core.http.server as server

fn run() {{
    mux :: server.mux()
    result :: server.serve("127.0.0.1:999999", mux, tls: server.tls({cert}, {key}))
    if result == {{
        Ok(_) -> {{ print("unexpected") }}
        Err(e) -> {{ print(e) }}
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
use core.http.server as server

fn run() {
    mux :: server.mux()
    result :: server.serve("127.0.0.1:999999", mux, tls: server.tls("not a cert", "not a key"))
    if result == {
        Ok(_) -> { print("unexpected") }
        Err(e) -> { print(e) }
    }
}
"#;

    let (code, stdout, stderr) = build_and_run("jet_http_server_tls", "bad_cert", src);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains("TLS certificate PEM did not contain a certificate"),
        "bad cert should stay Jet-voiced:\nstdout={stdout}\nstderr={stderr}"
    );
}

#[test]
fn server_tls_requires_named_tls_option() {
    let src = r#"
use core.http.server as server

fn run() {
    mux :: server.mux()
    _ :: server.serve("127.0.0.1:0", mux, server.tls("cert", "key"))
}
"#;

    let err = jet::compile(src).expect_err("unlabeled third server argument should be rejected");
    let rendered = jet::render_diagnostics("tests/http_server_tls/bad_label.jet", src, &err);
    assert!(rendered.contains("Error [E0125]"), "{rendered}");
    assert!(
        rendered.contains("`serve` needs `tls:` before the third argument"),
        "{rendered}"
    );
}

#[test]
fn server_bind_tls_requires_named_tls_option() {
    let src = r#"
use core.http.server as server

fn run() {
    mux :: server.mux()
    _ :: server.bind("127.0.0.1:0", mux, server.tls("cert", "key"))
}
"#;

    let err = jet::compile(src).expect_err("unlabeled bind TLS argument should be rejected");
    let rendered = jet::render_diagnostics("tests/http_server_tls/bad_bind_label.jet", src, &err);
    assert!(rendered.contains("Error [E0125]"), "{rendered}");
    assert!(
        rendered.contains("`bind` needs `tls:` before the third argument"),
        "{rendered}"
    );
}

#[test]
fn server_bind_tls_labeled_compiles() {
    let src = r#"
use core.http.server as server

fn run() {
    mux :: server.mux()
    _ :: server.bind("127.0.0.1:0", mux, tls: server.tls("cert", "key"))
}
"#;

    // Sema + TIR must cover labeled bind+tls (serve already did; bind was missing).
    // Full compile may still fail later on PEM validation at runtime — front end must not ICE/E0104.
    match jet::compile(src) {
        Ok(_) => {}
        Err(err) => {
            let rendered =
                jet::render_diagnostics("tests/http_server_tls/labeled_bind.jet", src, &err);
            assert!(
                !rendered.contains("E0104"),
                "labeled bind+tls must not be rejected as arity-2:\n{rendered}"
            );
            assert!(
                !rendered.contains("E0125"),
                "labeled bind+tls must not be rejected for missing tls: label:\n{rendered}"
            );
            assert!(
                !rendered.contains("typed IR does not cover"),
                "labeled bind+tls must be in the TIR subset:\n{rendered}"
            );
        }
    }
}

#[test]
fn server_bind_tls_validates_fixture_before_binding() {
    if !have_rustc() || !have_cargo() {
        eprintln!("note: skipping server bind TLS bridge test (need cargo + rustc)");
        return;
    }
    let _lock = FfiBridgeLock::acquire();
    let cert = jet_string(include_str!("../tests/fixtures/tls/localhost.cert.pem"));
    let key = jet_string(include_str!("../tests/fixtures/tls/localhost.key.pem"));
    let src = format!(
        r#"
use core.http.server as server

fn run() {{
    mux :: server.mux()
    result :: server.bind("127.0.0.1:999999", mux, tls: server.tls({cert}, {key}))
    if result == {{
        Ok(_) -> {{ print("unexpected") }}
        Err(e) -> {{ print(e) }}
    }}
}}
"#
    );

    let (code, stdout, stderr) = build_and_run("jet_http_server_tls", "bind_valid_fixture", &src);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains("bind on `127.0.0.1:999999` failed"),
        "labeled bind+tls must validate PEM then hit controlled bind failure:\nstdout={stdout}\nstderr={stderr}"
    );
    assert!(!stdout.contains("TLS certificate"), "{stdout}");
}
