//! Dependency-free dev-server transport and watch policy.
#![allow(non_snake_case)]
#![deny(warnings)]

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

pub mod Canvas;
pub mod BrowserTrace;
pub mod LiveInspect;
pub mod WebHost;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WatchPolicy { Auto, Restart, Swap, Once }

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

pub struct Request { pub method: String, pub target: String, pub body: Vec<u8> }

impl Request {
    pub fn read(reader: &mut impl BufRead) -> std::io::Result<Option<Self>> {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 { return Ok(None); }
        let mut content_length = 0;
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header)? == 0 || header == "\r\n" || header == "\n" { break; }
            if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
        if content_length > 1024 * 1024 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "devserver request body exceeds 1 MiB"));
        }
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body)?;
        let mut parts = line.split_whitespace();
        Ok(Some(Self {
            method: parts.next().unwrap_or("").to_string(),
            target: parts.next().unwrap_or("/").to_string(),
            body,
        }))
    }
}

pub fn write_response(mut out: impl Write, status: &str, content_type: &str, body: &[u8]) -> std::io::Result<()> {
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
    let bytes = value.as_bytes(); let mut out = Vec::with_capacity(bytes.len()); let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) { out.push(a * 16 + b); i += 3; continue; }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] }); i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> { match b { b'0'..=b'9' => Some(b-b'0'), b'a'..=b'f' => Some(b-b'a'+10), b'A'..=b'F' => Some(b-b'A'+10), _ => None } }

pub fn static_path(root: &Path, path: &str) -> Result<PathBuf, ()> {
    if path.contains("..") { return Err(()); }
    Ok(root.join(if path == "/" { "index.html" } else { path.trim_start_matches('/') }))
}

pub fn content_type_for(path: &Path) -> &'static str { match path.extension().and_then(|e| e.to_str()) { Some("html") => "text/html; charset=utf-8", Some("js") => "application/javascript; charset=utf-8", Some("wasm") => "application/wasm", Some("json") => "application/json; charset=utf-8", Some("css") => "text/css; charset=utf-8", _ => "application/octet-stream" } }

pub struct CanvasAsset { pub status: &'static str, pub content_type: &'static str, pub body: String }

pub fn canvas_asset(method: &str, target: &str, path: &str) -> Option<CanvasAsset> {
    let body = if target == "/?jet_panel=1" {
        jet_canvas::canvas_html_query()
    } else if target == "/?jet_panel_app=1" {
        jet_canvas::canvas_js()
    } else if matches!(path, "/__jet_canvas" | "/__jet_canvas/" | "/canvas" | "/canvas/" | "/panel" | "/panel/") {
        jet_canvas::canvas_html_for(if path.starts_with("/panel") { "/panel" } else { "/canvas" })
    } else if matches!(path, "/__jet_canvas/app.js" | "/canvas/app.js" | "/panel/app.js") {
        jet_canvas::canvas_js()
    } else {
        return None;
    };
    if method != "GET" {
        return Some(CanvasAsset { status: "405 Method Not Allowed", content_type: "text/plain; charset=utf-8", body: "method not allowed".into() });
    }
    let content_type = if path.ends_with("app.js") || target == "/?jet_panel_app=1" { "application/javascript; charset=utf-8" } else { "text/html; charset=utf-8" };
    Some(CanvasAsset { status: "200 OK", content_type, body })
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn request_and_query_policy() { let mut raw=&b"POST /x?q=a%20b HTTP/1.1\r\nContent-Length: 2\r\n\r\nok"[..]; let r=Request::read(&mut raw).unwrap().unwrap(); assert_eq!(r.body,b"ok"); assert_eq!(query_param(&r.target,"q").as_deref(),Some("a b")); }
    #[test] fn traversal_is_rejected() { assert!(static_path(Path::new("build"), "/../x").is_err()); }
    #[test] fn canvas_assets_are_owned_routes() { let page=canvas_asset("GET","/canvas","/canvas").unwrap(); assert_eq!(page.status,"200 OK"); assert!(page.body.contains("<!doctype html>")); let js=canvas_asset("GET","/canvas/app.js","/canvas/app.js").unwrap(); assert_eq!(js.content_type,"application/javascript; charset=utf-8"); assert_eq!(canvas_asset("POST","/canvas","/canvas").unwrap().status,"405 Method Not Allowed"); }
}
