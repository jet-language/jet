//! D-NETDEP1=A: blocking HTTP/file fetch for comptime `core.net.fetch`.
//!
//! Handles `file://` via std::fs and `http(s)://` via ureq.
//! D-TLS1=A: HTTPS is available in the default build through rustls plus
//! system trust roots. Use `--no-default-features` only for size/freestanding
//! builds that knowingly drop HTTPS.

use std::io::Read;
use std::time::Duration;

#[derive(Debug)]
pub enum FetchError {
    IO(String),
    HTTP {
        kind: FetchErrorKind,
        detail: String,
    },
    Scheme(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchErrorKind {
    General,
    TLSHandshake,
    TLSCertificate,
    TLSTrustRoots,
}

impl FetchError {
    pub fn diagnostic_code(&self) -> &'static str {
        match self {
            FetchError::HTTP {
                kind: FetchErrorKind::TLSHandshake,
                ..
            } => "E4201",
            FetchError::HTTP {
                kind: FetchErrorKind::TLSCertificate,
                ..
            } => "E4202",
            FetchError::HTTP {
                kind: FetchErrorKind::TLSTrustRoots,
                ..
            } => "E4203",
            FetchError::IO(_) | FetchError::HTTP { .. } | FetchError::Scheme(_) => "E3414",
        }
    }

    pub fn diagnostic_what(&self, url: &str) -> String {
        let host = host_from_url(url);
        match self {
            FetchError::HTTP {
                kind: FetchErrorKind::TLSHandshake,
                ..
            } => format!("TLS handshake with `{host}` failed"),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSCertificate,
                ..
            } => format!("TLS certificate for `{host}` could not be trusted"),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSTrustRoots,
                ..
            } => "HTTPS could not find system certificate roots".to_string(),
            _ => format!("fetch failed: {self}"),
        }
    }

    pub fn diagnostic_why(&self, url: &str) -> String {
        match self {
            FetchError::HTTP {
                kind: FetchErrorKind::TLSHandshake,
                ..
            } => format!(
                "`{url}` reached the server, but the connection did not complete a secure HTTPS handshake"
            ),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSCertificate,
                ..
            } => format!(
                "`{url}` presented a certificate Jet could not verify for that host"
            ),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSTrustRoots,
                ..
            } => "Jet uses rustls with the system trust store for default HTTPS, but no usable roots were available".to_string(),
            _ => format!("could not retrieve `{url}`"),
        }
    }

    pub fn diagnostic_fix(&self) -> String {
        match self {
            FetchError::HTTP {
                kind: FetchErrorKind::TLSHandshake,
                ..
            } => "verify the URL points at an HTTPS server, not plain HTTP; for local tests, start the TLS fixture server".to_string(),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSCertificate,
                ..
            } => "use a certificate whose subject matches the host and chains to a trusted root; for tests, trust the local fixture CA explicitly".to_string(),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSTrustRoots,
                ..
            } => "install the system certificate bundle (for example `ca-certificates`) or run in an image that includes it".to_string(),
            _ => "check the URL is reachable and the network is available; use `file://` for local paths".to_string(),
        }
    }

    fn http(url: &str, detail: String) -> Self {
        FetchError::HTTP {
            kind: classify_http_error(url, &detail),
            detail,
        }
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::IO(s) | FetchError::Scheme(s) => f.write_str(s),
            FetchError::HTTP {
                kind: FetchErrorKind::General,
                detail,
            } => f.write_str(detail),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSHandshake,
                ..
            } => f.write_str("TLS handshake failed"),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSCertificate,
                ..
            } => f.write_str("TLS certificate could not be trusted"),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSTrustRoots,
                ..
            } => f.write_str("HTTPS could not find system certificate roots"),
        }
    }
}

/// Fetch `url` and return the raw bytes.
///
/// Supports:
/// - `file:///path` → `std::fs::read`
/// - `http://…` / `https://…` → ureq blocking GET
pub fn fetch(url: &str) -> Result<Vec<u8>, FetchError> {
    fetch_with_timeout(url, Duration::from_secs(30))
}

fn fetch_with_timeout(url: &str, timeout: Duration) -> Result<Vec<u8>, FetchError> {
    if let Some(path) = url.strip_prefix("file://") {
        std::fs::read(path).map_err(|e| FetchError::IO(e.to_string()))
    } else if url.starts_with("http://") || url.starts_with("https://") {
        let mut bytes = Vec::new();
        ureq::AgentBuilder::new()
            .timeout_connect(timeout)
            .timeout_read(timeout)
            .build()
            .get(url)
            .call()
            .map_err(|e| FetchError::http(url, e.to_string()))?
            .into_reader()
            .read_to_end(&mut bytes)
            .map_err(|e| FetchError::http(url, e.to_string()))?;
        Ok(bytes)
    } else {
        let scheme = url.find("://").map(|i| &url[..i]).unwrap_or(url);
        Err(FetchError::Scheme(format!(
            "unsupported URL scheme `{scheme}`; expected `file://`, `http://`, or `https://`"
        )))
    }
}

fn classify_http_error(url: &str, detail: &str) -> FetchErrorKind {
    if !url.starts_with("https://") {
        return FetchErrorKind::General;
    }
    let lower = detail.to_ascii_lowercase();
    if lower.contains("no valid certificates loaded")
        || lower.contains("no root cert")
        || lower.contains("no roots")
        || lower.contains("empty root")
        || lower.contains("trust store")
    {
        FetchErrorKind::TLSTrustRoots
    } else if lower.contains("certificate")
        || lower.contains("unknownissuer")
        || lower.contains("notvalid")
        || lower.contains("expired")
        || lower.contains("hostname")
        || lower.contains("cert")
    {
        FetchErrorKind::TLSCertificate
    } else if lower.contains("tls")
        || lower.contains("rustls")
        || lower.contains("handshake")
        || lower.contains("alert")
    {
        FetchErrorKind::TLSHandshake
    } else {
        FetchErrorKind::General
    }
}

fn host_from_url(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    rest.split('/').next().unwrap_or(rest).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn file_fetch_still_works_without_network() {
        let path = std::env::temp_dir().join(format!("jet-net-file-fetch-{}", std::process::id()));
        std::fs::write(&path, b"fixture").expect("write temp fixture");
        let url = format!("file://{}", path.display());

        let bytes = fetch(&url).expect("file fetch");

        assert_eq!(bytes, b"fixture");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn unsupported_scheme_names_allowed_schemes() {
        let err = fetch("ftp://example.invalid/data").expect_err("ftp is rejected");

        assert!(err
            .to_string()
            .contains("expected `file://`, `http://`, or `https://`"));
    }

    #[test]
    #[cfg(feature = "tls")]
    fn https_default_build_has_tls_backend() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind local fixture");
        let addr = listener.local_addr().expect("local addr");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept one client");
            let mut buf = [0_u8; 8];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"not tls");
        });

        let err = fetch_with_timeout(
            &format!("https://localhost:{}", addr.port()),
            Duration::from_millis(500),
        )
        .expect_err("fixture is not a TLS server");
        server.join().expect("fixture server joins");

        assert_eq!(err.diagnostic_code(), "E4201", "{err:?}");
        assert_eq!(err.to_string(), "TLS handshake failed");
    }

    #[test]
    fn tls_error_classifier_names_certificate_failures() {
        let err = FetchError::http(
            "https://api.example.test/data",
            "Connection Failed: invalid peer certificate: UnknownIssuer".to_string(),
        );

        assert_eq!(err.diagnostic_code(), "E4202");
        assert!(err
            .diagnostic_what("https://api.example.test/data")
            .contains("api.example.test"));
    }

    #[test]
    fn tls_error_classifier_names_missing_trust_roots() {
        let err = FetchError::http(
            "https://api.example.test/data",
            "no valid certificates loaded by rustls-native-certs".to_string(),
        );

        assert_eq!(err.diagnostic_code(), "E4203");
        assert!(err.diagnostic_fix().contains("ca-certificates"));
    }

    #[test]
    fn tls_fixture_pems_are_checked_in_for_future_handshake_tests() {
        let cert = include_str!("../../../tests/fixtures/tls/localhost.cert.pem");
        let key = include_str!("../../../tests/fixtures/tls/localhost.key.pem");

        assert!(cert.contains("BEGIN CERTIFICATE"));
        assert!(cert.contains("DNS:localhost") || cert.contains("MIID"));
        assert!(key.contains("BEGIN PRIVATE KEY"));
    }
}
