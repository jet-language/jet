//! D-NETDEP1=A: blocking HTTP/file fetch for comptime `core.net.fetch`.
//!
//! Handles `file://` via std::fs and `http(s)://` via ureq.
//! HTTPS requires a TLS feature; the default build is HTTP-only
//! (sufficient for the `file://` test path and plain-HTTP fetches).

use std::io::Read;

pub enum FetchError {
    Io(String),
    Http(String),
    Scheme(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Io(s) | FetchError::Http(s) | FetchError::Scheme(s) => f.write_str(s),
        }
    }
}

/// Fetch `url` and return the raw bytes.
///
/// Supports:
/// - `file:///path` → `std::fs::read`
/// - `http://…` / `https://…` → ureq blocking GET
pub fn fetch(url: &str) -> Result<Vec<u8>, FetchError> {
    if let Some(path) = url.strip_prefix("file://") {
        std::fs::read(path).map_err(|e| FetchError::Io(e.to_string()))
    } else if url.starts_with("http://") || url.starts_with("https://") {
        let mut bytes = Vec::new();
        ureq::get(url)
            .call()
            .map_err(|e| FetchError::Http(e.to_string()))?
            .into_reader()
            .read_to_end(&mut bytes)
            .map_err(|e| FetchError::Http(e.to_string()))?;
        Ok(bytes)
    } else {
        let scheme = url.find("://").map(|i| &url[..i]).unwrap_or(url);
        Err(FetchError::Scheme(format!(
            "unsupported URL scheme `{scheme}`; expected `file://`, `http://`, or `https://`"
        )))
    }
}
