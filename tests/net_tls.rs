mod common;

use std::fs;
use std::net::TcpListener;
use std::time::{Duration, Instant};

#[test]
fn tls_client_diagnostic_snapshot_covers_e42xx_explain_codes() {
    let rendered = [
        jet::Diagnostics::Diagnostic::error(
            "E4201",
            "TLS handshake with `localhost` failed".to_string(),
            "`https://localhost/data.txt` reached the server, but the connection did not complete a secure HTTPS handshake".to_string(),
            "verify the URL points at an HTTPS server, not plain HTTP; for local tests, start the TLS fixture server".to_string(),
            None,
        ),
        jet::Diagnostics::Diagnostic::error(
            "E4202",
            "TLS certificate for `api.example.test` could not be trusted".to_string(),
            "`https://api.example.test/data` presented a certificate Jet could not verify for that host".to_string(),
            "use a certificate whose subject matches the host and chains to a trusted root; for tests, trust the local fixture CA explicitly".to_string(),
            None,
        ),
        jet::Diagnostics::Diagnostic::error(
            "E4203",
            "HTTPS could not find system certificate roots".to_string(),
            "Jet uses rustls with the system trust store for default HTTPS, but no usable roots were available".to_string(),
            "install the system certificate bundle (for example `ca-certificates`) or run in an image that includes it".to_string(),
            None,
        ),
    ];
    let text = jet::render_diagnostics("tests/ui/comptime_net_fetch_tls_roots.jet", "", &rendered);

    assert_eq!(
        text,
        fs::read_to_string("tests/cli/tls_client_diagnostics.txt").unwrap()
    );
}

#[test]
fn comptime_fetch_rejects_outside_files_and_private_networks() {
    let root = common::unique_tmp("comptime_fetch_root");
    let outside = common::unique_tmp("comptime_fetch_outside");
    fs::create_dir_all(&root).unwrap();
    fs::write(&outside, "private fixture").unwrap();
    let source_path = root.join("main.jet");
    let file_source = format!(
        "use core.net as net\n@data :: net.fetch(\"file://{}\", sha256: \"{}\")\nfn run() {{}}\n",
        outside.display(),
        "0".repeat(64)
    );
    fs::write(&source_path, &file_source).unwrap();
    let file_diags = jet::compile_with_path(&file_source, source_path.to_str().unwrap())
        .expect_err("comptime fetch must not read outside its source directory");
    assert!(file_diags.iter().any(|diag| diag.code == "E3414"));

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match listener.accept() {
                Ok(_) => return true,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return false;
                    }
                    std::thread::yield_now();
                }
                Err(_) => return false,
            }
        }
    });
    let network_source = format!(
        "use core.net as net\n@data :: net.fetch(\"http://127.0.0.1:{port}/secret\", sha256: \"{}\")\nfn run() {{}}\n",
        "0".repeat(64)
    );
    fs::write(&source_path, &network_source).unwrap();
    let network_diags = jet::compile_with_path(&network_source, source_path.to_str().unwrap())
        .expect_err("comptime fetch must reject loopback destinations");
    assert!(network_diags.iter().any(|diag| diag.code == "E3414"));
    assert!(!server.join().unwrap(), "private destination was contacted");

    let reserved_source = format!(
        "use core.net as net\n@data :: net.fetch(\"http://192.0.0.1/secret\", sha256: \"{}\")\nfn run() {{}}\n",
        "0".repeat(64)
    );
    fs::write(&source_path, &reserved_source).unwrap();
    let reserved_diags = jet::compile_with_path(&reserved_source, source_path.to_str().unwrap())
        .expect_err("comptime fetch must reject reserved IPv4 destinations");
    assert!(reserved_diags.iter().any(|diag| diag.code == "E3414"));

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_file(outside);
}

#[cfg(unix)]
#[test]
fn comptime_fetch_rejects_hardlinks_to_outside_files() {
    let root = common::unique_tmp("comptime_fetch_hardlink_root");
    let outside = common::unique_tmp("comptime_fetch_hardlink_outside");
    fs::create_dir_all(&root).unwrap();
    fs::write(&outside, "private fixture").unwrap();
    let hardlink = root.join("linked-secret");
    fs::hard_link(&outside, &hardlink).unwrap();
    let source_path = root.join("main.jet");
    let source = format!(
        "use core.net as net\n@data :: net.fetch(\"file://{}\", sha256: \"{}\")\nfn run() {{}}\n",
        hardlink.display(),
        "0".repeat(64)
    );
    fs::write(&source_path, &source).unwrap();

    let diagnostics = jet::compile_with_path(&source, source_path.to_str().unwrap())
        .expect_err("compile-time fetch must reject hardlinks to outside files");
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E3414"));

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_file(outside);
}
