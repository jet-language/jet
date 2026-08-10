mod common;

use std::fs;

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
