//! D-NETDEP1=A: blocking HTTP/file fetch for comptime `core.net.fetch`.
//!
//! Handles `file://` via std::fs and `http(s)://` via ureq.
//! D-TLS1=A: HTTPS is available in the default build through rustls plus
//! system trust roots. Use `--no-default-features` only for size/freestanding
//! builds that knowingly drop HTTPS.

use std::io::Read;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

static HTTP_AGENT: LazyLock<ureq::Agent> =
    LazyLock::new(|| ureq::AgentBuilder::new().redirects(0).build());
static COMPTIME_HTTP_AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::AgentBuilder::new()
        .redirects(0)
        .resolver(restricted_resolver)
        .build()
});

const MAX_FETCH_BYTES: usize = 64 * 1024 * 1024;

pub struct StreamResponse {
    status: u16,
    content_length: Option<u64>,
    location: Option<String>,
    reader: Box<dyn Read + Send>,
}

impl StreamResponse {
    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    /// The redirect target, when the response carries a `Location` header.
    pub fn location(&self) -> Option<&str> {
        self.location.as_deref()
    }
}

impl Read for StreamResponse {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buffer)
    }
}

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
            _ => "check the URL is reachable and the network is available; use `file://` for a local path inside the compile-time source directory".to_string(),
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
/// - `file:///path` → `std::fs::read`, scoped to the current directory
/// - `http://…` / `https://…` → ureq blocking GET
pub fn fetch(url: &str) -> Result<Vec<u8>, FetchError> {
    fetch_in_root(url, Path::new("."))
}

/// Fetch a comptime resource while keeping local files below `base_dir` and
/// network connections on publicly routable destinations.
pub fn fetch_in_root(url: &str, base_dir: &Path) -> Result<Vec<u8>, FetchError> {
    fetch_with_root_timeout(url, Duration::from_secs(30), base_dir)
}

fn fetch_with_root_timeout(
    url: &str,
    timeout: Duration,
    base_dir: &Path,
) -> Result<Vec<u8>, FetchError> {
    if let Some(path) = url.strip_prefix("file://") {
        let path = scoped_file_path(path, base_dir)?;
        let file = std::fs::File::open(path).map_err(|e| FetchError::IO(e.to_string()))?;
        read_limited(file, MAX_FETCH_BYTES).map_err(|e| FetchError::IO(e.to_string()))
    } else if url.starts_with("http://") || url.starts_with("https://") {
        read_limited(comptime_http_stream(url, timeout)?, MAX_FETCH_BYTES)
            .map_err(|e| FetchError::http(url, e.to_string()))
    } else {
        let scheme = url.find("://").map(|i| &url[..i]).unwrap_or(url);
        Err(FetchError::Scheme(format!(
            "unsupported URL scheme `{scheme}`; expected `file://`, `http://`, or `https://`"
        )))
    }
}

fn scoped_file_path(raw_path: &str, base_dir: &Path) -> Result<std::path::PathBuf, FetchError> {
    #[cfg(unix)]
    if Path::new(raw_path) == Path::new("/dev/null") {
        return Ok(Path::new(raw_path).to_path_buf());
    }
    let root = std::fs::canonicalize(base_dir)
        .map_err(|error| FetchError::IO(error.to_string()))?;
    let requested = Path::new(raw_path);
    let requested = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        root.join(requested)
    };
    let canonical = std::fs::canonicalize(&requested)
        .map_err(|error| FetchError::IO(error.to_string()))?;
    if !canonical.starts_with(&root) {
        return Err(FetchError::IO(
            "file URL resolves outside the compile-time source directory".to_string(),
        ));
    }
    Ok(canonical)
}

fn comptime_http_stream(
    url: &str,
    timeout: Duration,
) -> Result<StreamResponse, FetchError> {
    let response = match COMPTIME_HTTP_AGENT
        .get(url)
        .timeout(timeout)
        .set("Accept-Encoding", "identity")
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(error) => return Err(FetchError::http(url, error.to_string())),
    };
    let status = response.status();
    let content_length = response
        .header("Content-Length")
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| FetchError::http(url, "invalid Content-Length".to_string()))
        })
        .transpose()?;
    Ok(StreamResponse {
        status,
        content_length,
        location: response.header("Location").map(str::to_string),
        reader: Box::new(response.into_reader()),
    })
}

fn restricted_resolver(netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
    let addresses = netloc.to_socket_addrs()?.collect::<Vec<_>>();
    if !addresses_are_public(&addresses) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "compile-time network destination is not public",
        ));
    }
    Ok(addresses)
}

/// Resolve a host and reject any DNS answer that is not globally routable.
/// Callers that invoke an external client should pin the returned addresses
/// so the client cannot perform a second, different DNS lookup.
pub fn resolve_public_addresses(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    let endpoint = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let addresses = endpoint
        .to_socket_addrs()
        .map_err(|error| format!("could not resolve provider destination: {error}"))?
        .collect::<Vec<_>>();
    if !addresses_are_public(&addresses) {
        return Err("provider destination resolves to a non-public address".to_string());
    }
    Ok(addresses)
}

fn addresses_are_public(addresses: &[SocketAddr]) -> bool {
    !addresses.is_empty() && addresses.iter().all(|address| is_public_ip(address.ip()))
}

/// Return whether an address is safe for a comptime outbound request.
/// Only globally routable unicast space is allowed. This check is applied to
/// every address returned by DNS, so a mixed public/private answer is denied.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 100 && (b & 0b1100_0000) == 0b0100_0000
                || a == 127
                || a == 169 && b == 254
                || a == 172 && (16..=31).contains(&b)
                || a == 192 && b == 0 && c == 0
                || a == 192 && b == 0 && c == 2
                || a == 192 && b == 168
                || a == 198 && (18..=19).contains(&b)
                || a == 198 && b == 51 && c == 100
                || a == 203 && b == 0 && c == 113
                || a >= 224)
        }
        IpAddr::V6(ip) => {
            if let Some(ipv4) = ip.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(ipv4));
            }
            let [first, second, ..] = ip.segments();
            (first & 0xe000) == 0x2000
                && !ip.is_unspecified()
                && !ip.is_loopback()
                && !ip.is_multicast()
                && (first & 0xfe00) != 0xfc00
                && (first & 0xffc0) != 0xfe80
                && !(first == 0x2001 && second == 0x0db8)
        }
    }
}

fn read_limited(mut reader: impl Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("fetch response exceeds {limit} bytes"),
        ));
    }
    Ok(bytes)
}

pub fn get_stream(url: &str, timeout: Duration) -> Result<StreamResponse, FetchError> {
    get_stream_with_timeout(url, timeout)
}

fn get_stream_with_timeout(url: &str, timeout: Duration) -> Result<StreamResponse, FetchError> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        let scheme = url.find("://").map(|i| &url[..i]).unwrap_or(url);
        return Err(FetchError::Scheme(format!(
            "unsupported URL scheme `{scheme}`; expected `http://` or `https://`"
        )));
    }
    let response = match HTTP_AGENT
        .get(url)
        .timeout(timeout)
        .set("Accept-Encoding", "identity")
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(error) => return Err(FetchError::http(url, error.to_string())),
    };
    let status = response.status();
    let content_length = response
        .header("Content-Length")
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| FetchError::http(url, "invalid Content-Length".to_string()))
        })
        .transpose()?;
    Ok(StreamResponse {
        status,
        content_length,
        location: response.header("Location").map(str::to_string),
        reader: Box::new(response.into_reader()),
    })
}

/// Fetch a bounded stream while following a small, explicit redirect chain.
/// The plain `get_stream` API remains redirect-free for callers that need to
/// inspect the original HTTP response.
pub fn get_stream_follow_redirects(
    url: &str,
    timeout: Duration,
    max_redirects: usize,
) -> Result<StreamResponse, FetchError> {
    let mut current = url.to_string();
    for _ in 0..=max_redirects {
        let response = get_stream_with_timeout(&current, timeout)?;
        if !(300..400).contains(&response.status()) {
            return Ok(response);
        }
        let location = response.location().ok_or_else(|| {
            FetchError::http(
                &current,
                "redirect response has no Location header".to_string(),
            )
        })?;
        current = resolve_redirect(&current, location)?;
    }
    Err(FetchError::http(
        url,
        format!("too many redirects (limit {max_redirects})"),
    ))
}

fn resolve_redirect(current: &str, location: &str) -> Result<String, FetchError> {
    if location.starts_with("https://") || location.starts_with("http://") {
        return Ok(location.to_string());
    }
    if location.starts_with('/') {
        let authority = current
            .split_once("://")
            .and_then(|(_, rest)| rest.split('/').next())
            .ok_or_else(|| FetchError::http(current, "redirect URL has no authority".into()))?;
        return Ok(format!(
            "{}://{}{}",
            current.split_once("://").unwrap().0,
            authority,
            location
        ));
    }
    Err(FetchError::http(
        current,
        "redirect Location must be an absolute or root-relative URL".into(),
    ))
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

        let bytes = fetch_in_root(&url, path.parent().unwrap()).expect("file fetch");

        assert_eq!(bytes, b"fixture");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn comptime_file_fetch_rejects_paths_outside_source_root() {
        let root = std::env::temp_dir().join(format!("jet-net-fetch-root-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("jet-net-fetch-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
        std::fs::create_dir_all(&root).expect("create source root");
        std::fs::write(&outside, b"private fixture").expect("write outside fixture");

        let error = fetch_in_root(&format!("file://{}", outside.display()), &root)
            .expect_err("absolute path outside source root must fail");
        assert!(error.to_string().contains("outside the compile-time source directory"));

        #[cfg(unix)]
        {
            let link = root.join("outside-link");
            std::os::unix::fs::symlink(&outside, &link).expect("create escape symlink");
            let error = fetch_in_root("file://outside-link", &root)
                .expect_err("symlink outside source root must fail");
            assert!(error
                .to_string()
                .contains("outside the compile-time source directory"));
            let _ = std::fs::remove_file(link);
        }

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(outside);
    }

    #[test]
    fn comptime_http_fetch_rejects_loopback_before_connecting() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback fixture");
        let port = listener.local_addr().expect("loopback address").port();
        let error = fetch(&format!("http://127.0.0.1:{port}/secret"))
            .expect_err("loopback destination must fail");
        assert!(format!("{error:?}").contains("not public"), "unexpected error: {error:?}");

        listener
            .set_nonblocking(true)
            .expect("make listener nonblocking");
        assert!(matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock));
    }

    #[test]
    fn unsupported_scheme_names_allowed_schemes() {
        let err = fetch("ftp://example.invalid/data").expect_err("ftp is rejected");

        assert!(err
            .to_string()
            .contains("expected `file://`, `http://`, or `https://`"));
    }

    #[test]
    fn fetch_reader_rejects_an_endless_response_at_the_boundary() {
        let error = read_limited(std::io::repeat(0), 8).expect_err("reader must be bounded");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
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

        let result = get_stream_with_timeout(
            &format!("https://localhost:{}", addr.port()),
            Duration::from_millis(500),
        );
        assert!(result.is_err(), "fixture is not a TLS server");
        server.join().expect("fixture server joins");

        let err = result.err().unwrap();
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
