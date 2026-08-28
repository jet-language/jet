//! Dependency-free dev-server transport and watch policy.
#![allow(non_snake_case)]
#![deny(warnings)]

use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::path::{Component, Path, PathBuf};

pub mod BrowserTrace;
pub mod Canvas;
pub mod LiveInspect;
pub mod Session;
pub mod WatchService;
pub mod WebHost;

pub use Session::ResidentDevSession;

pub use WatchService::{
    any_stamp_changed, within_budget, ChangeKind, HotReplaceTxn, InvalidationReceipt, PersistEntry,
    PersistOutcome, PersistStore, RootKind, SessionSnapshot, WatchGraph, WatchSession,
    EDIT_TO_VISIBLE_BUDGET_MS,
};

pub const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;
pub const MAX_REQUEST_HEADER_BYTES: usize = 32 * 1024;
pub const MAX_REQUEST_HEADER_COUNT: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WatchPolicy {
    Auto,
    Restart,
    Swap,
    Once,
}

pub fn watch_policy_from(raw: &[String], default: WatchPolicy) -> WatchPolicy {
    raw.iter().fold(default, |policy, arg| match arg.as_str() {
        "--restart" => WatchPolicy::Restart,
        "--swap" => WatchPolicy::Swap,
        "--watch=off" => WatchPolicy::Once,
        _ => policy,
    })
}

pub fn file_mtime(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

pub struct Request {
    pub method: String,
    pub target: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn read(reader: &mut impl BufRead) -> std::io::Result<Option<Self>> {
        let mut line = String::new();
        if read_bounded_line(reader, &mut line, MAX_REQUEST_LINE_BYTES)? == 0 {
            return Ok(None);
        }
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or("");
        let target = parts.next().unwrap_or("");
        let version = parts.next().unwrap_or("");
        if method.is_empty()
            || target.is_empty()
            || version != "HTTP/1.1"
            || parts.next().is_some()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "malformed devserver request line",
            ));
        }
        let mut content_length = None;
        let mut headers = HashMap::new();
        let mut header_bytes = 0usize;
        let mut header_count = 0usize;
        loop {
            let mut header = String::new();
            let read = read_bounded_line(reader, &mut header, MAX_REQUEST_LINE_BYTES)?;
            if read == 0 || header == "\r\n" || header == "\n" {
                break;
            }
            header_count = header_count.checked_add(1).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "too many devserver headers")
            })?;
            header_bytes = header_bytes.checked_add(read).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "devserver headers too large")
            })?;
            if header_count > MAX_REQUEST_HEADER_COUNT || header_bytes > MAX_REQUEST_HEADER_BYTES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "devserver headers exceed the request budget",
                ));
            }
            let Some((name, value)) = header.trim_end_matches(['\r', '\n']).split_once(':') else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "malformed devserver header",
                ));
            };
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "malformed devserver header name",
                ));
            }
            if matches!(
                name.as_str(),
                "authorization" | "content-length" | "host" | "origin" | "transfer-encoding"
            ) && headers.contains_key(&name)
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "duplicate devserver security header",
                ));
            }
            if name == "content-length" {
                let length = value.parse::<usize>().map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid devserver content length",
                    )
                })?;
                content_length = Some(length);
            }
            headers.insert(name, value);
        }
        let content_length = content_length.unwrap_or(0);
        if content_length > MAX_REQUEST_BODY_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "devserver request body exceeds 1 MiB",
            ));
        }
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body)?;
        Ok(Some(Self {
            method: method.to_string(),
            target: target.to_string(),
            headers,
            body,
        }))
    }
}

fn read_bounded_line(
    reader: &mut impl BufRead,
    line: &mut String,
    limit: usize,
) -> std::io::Result<usize> {
    let read = reader
        .take(limit.saturating_add(1) as u64)
        .read_line(line)?;
    if read > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "devserver request line exceeds 8 KiB",
        ));
    }
    Ok(read)
}

pub fn write_response(
    mut out: impl Write,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    write!(out, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len())?;
    out.write_all(body)?;
    out.flush()
}

pub fn query_param(target: &str, key: &str) -> Option<String> {
    let (_, query) = target.split_once('?')?;
    query.split('&').find_map(|part| {
        let (name, value) = part.split_once('=').unwrap_or((part, ""));
        (name == key).then(|| percent_decode(value))
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(a * 16 + b);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn static_path(root: &Path, path: &str) -> Result<PathBuf, ()> {
    let relative = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    if path.contains("..")
        || relative.starts_with('/')
        || relative.starts_with('\\')
        || has_windows_drive_prefix(relative)
        || Path::new(relative)
            .components()
            .any(|component| matches!(component, Component::Prefix(_) | Component::RootDir))
    {
        return Err(());
    }
    let root_metadata = std::fs::symlink_metadata(root).map_err(|_| ())?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(());
    }
    let canonical_root = std::fs::canonicalize(root).map_err(|_| ())?;
    let candidate = root.join(relative);
    let mut current = root.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(component) = component else {
            return Err(());
        };
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(()),
            Ok(_) => {
                let canonical = std::fs::canonicalize(&current).map_err(|_| ())?;
                if !canonical.starts_with(&canonical_root) {
                    return Err(());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(()),
        }
    }
    if candidate.exists() {
        let canonical = std::fs::canonicalize(&candidate).map_err(|_| ())?;
        if !canonical.starts_with(&canonical_root) {
            return Err(());
        }
        return Ok(canonical);
    }
    Ok(candidate)
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

pub fn content_type_for(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match extension.as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "application/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

pub struct CanvasAsset {
    pub status: &'static str,
    pub content_type: &'static str,
    pub body: String,
}

pub fn canvas_asset(method: &str, target: &str, path: &str) -> Option<CanvasAsset> {
    let body = if target == "/?jet_panel=1" {
        jet_canvas::canvas_html_query()
    } else if target == "/?jet_panel_app=1" {
        jet_canvas::canvas_js()
    } else if matches!(
        path,
        "/__jet_canvas" | "/__jet_canvas/" | "/canvas" | "/canvas/" | "/panel" | "/panel/"
    ) {
        jet_canvas::canvas_html_for(if path.starts_with("/panel") {
            "/panel"
        } else {
            "/canvas"
        })
    } else if matches!(
        path,
        "/__jet_canvas/app.js" | "/canvas/app.js" | "/panel/app.js"
    ) {
        jet_canvas::canvas_js()
    } else {
        return None;
    };
    if method != "GET" {
        return Some(CanvasAsset {
            status: "405 Method Not Allowed",
            content_type: "text/plain; charset=utf-8",
            body: "method not allowed".into(),
        });
    }
    let content_type = if path.ends_with("app.js") || target == "/?jet_panel_app=1" {
        "application/javascript; charset=utf-8"
    } else {
        "text/html; charset=utf-8"
    };
    Some(CanvasAsset {
        status: "200 OK",
        content_type,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_and_query_policy() {
        let mut raw = &b"POST /x?q=a%20b HTTP/1.1\r\nContent-Length: 2\r\n\r\nok"[..];
        let r = Request::read(&mut raw).unwrap().unwrap();
        assert_eq!(r.body, b"ok");
        assert_eq!(r.headers.get("content-length").map(String::as_str), Some("2"));
        assert_eq!(query_param(&r.target, "q").as_deref(), Some("a b"));
    }
    #[test]
    fn malformed_request_metadata_fails_closed() {
        let mut invalid_length = &b"POST /x HTTP/1.1\r\nContent-Length: nope\r\n\r\n"[..];
        assert!(Request::read(&mut invalid_length).is_err());

        let mut malformed_line = &b"GET /x\r\nHost: localhost\r\n\r\n"[..];
        assert!(Request::read(&mut malformed_line).is_err());

        let raw = format!(
            "POST /x HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_REQUEST_BODY_BYTES + 1
        );
        let mut oversized = raw.as_bytes();
        assert!(Request::read(&mut oversized).is_err());
    }
    #[test]
    fn request_lines_and_headers_are_bounded_before_allocation() {
        let raw = format!(
            "GET /{} HTTP/1.1\r\n\r\n",
            "x".repeat(MAX_REQUEST_LINE_BYTES)
        );
        assert!(Request::read(&mut raw.as_bytes()).is_err());

        let raw = format!(
            "GET / HTTP/1.1\r\nX-Hostile: {}\r\n\r\n",
            "x".repeat(MAX_REQUEST_LINE_BYTES)
        );
        assert!(Request::read(&mut raw.as_bytes()).is_err());
    }
    #[test]
    fn traversal_is_rejected() {
        assert!(static_path(Path::new("build"), "/../x").is_err());
    }
    #[test]
    fn windows_absolute_static_paths_are_rejected() {
        for path in [
            "/C:/Windows/win.ini",
            "/C:\\Windows\\win.ini",
            "/\\\\server\\share\\secret",
            "/\\Windows\\win.ini",
        ] {
            assert!(
                static_path(Path::new("build"), path).is_err(),
                "absolute Windows path escaped static root: {path}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn static_paths_reject_symlinked_files() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "jet-devserver-static-symlink-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let outside = root.with_file_name(format!(
            "jet-devserver-static-outside-{}",
            std::process::id()
        ));
        std::fs::write(&outside, "must not be served").unwrap();
        symlink(&outside, root.join("escape.js")).unwrap();

        assert!(
            static_path(&root, "/escape.js").is_err(),
            "static serving must not follow a symlink out of its root"
        );
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "must not be served");

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
    }

    #[test]
    fn canvas_assets_are_owned_routes() {
        let page = canvas_asset("GET", "/canvas", "/canvas").unwrap();
        assert_eq!(page.status, "200 OK");
        assert!(page.body.contains("<!doctype html>"));
        let js = canvas_asset("GET", "/canvas/app.js", "/canvas/app.js").unwrap();
        assert_eq!(js.content_type, "application/javascript; charset=utf-8");
        assert_eq!(
            canvas_asset("POST", "/canvas", "/canvas").unwrap().status,
            "405 Method Not Allowed"
        );
    }
}
