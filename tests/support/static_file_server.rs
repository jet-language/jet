//! Disk-backed HTTP fixture for static publication tests.
//!
//! The server has no route map and no generated responses. Every successful
//! response comes from one regular file below the supplied publication root.

#![allow(dead_code)]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub struct StaticFileServer {
    pub endpoint: String,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl StaticFileServer {
    pub fn start(root: &Path) -> Self {
        fs::create_dir_all(root).expect("static publication root");
        let metadata = fs::symlink_metadata(root).expect("static publication metadata");
        assert!(metadata.is_dir(), "static publication root is not a directory");
        assert!(
            !metadata.file_type().is_symlink(),
            "static publication root must not be a symlink"
        );
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind static file server");
        listener
            .set_nonblocking(true)
            .expect("set static file server nonblocking");
        let address = listener.local_addr().expect("static file server address");
        let endpoint = format!("http://127.0.0.1:{}", address.port());
        let root = root.to_path_buf();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_root = root.clone();
        let join = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => serve_request(stream, &thread_root),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            endpoint,
            stop,
            join: Some(join),
        }
    }
}

impl Drop for StaticFileServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self
            .endpoint
            .strip_prefix("http://")
            .and_then(|address| TcpStream::connect(address).ok());
        if let Some(join) = self.join.take() {
            join.join().expect("static file server thread");
        }
    }
}

fn serve_request(mut stream: TcpStream, root: &Path) {
    let mut request = [0u8; 16 * 1024];
    let count = stream.read(&mut request).unwrap_or_default();
    let Some((method, target)) = std::str::from_utf8(&request[..count])
        .ok()
        .and_then(|request| request.lines().next())
        .and_then(|line| {
            let mut fields = line.split_whitespace();
            Some((fields.next()?, fields.next()?))
        })
    else {
        respond(&mut stream, "400 Bad Request", "text/plain", b"");
        return;
    };
    if method != "GET" {
        respond(&mut stream, "405 Method Not Allowed", "text/plain", b"");
        return;
    }
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    let path = path.split_once('#').map_or(path, |(path, _)| path);
    let Some(file) = safe_file_path(root, path) else {
        respond(&mut stream, "400 Bad Request", "text/plain", b"");
        return;
    };
    let body = match fs::symlink_metadata(&file) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => None,
        Ok(_) => fs::read(&file).ok(),
        Err(_) => None,
    };
    match body {
        Some(body) => respond(&mut stream, "200 OK", content_type(&file), &body),
        None => respond(&mut stream, "404 Not Found", "text/plain", b""),
    }
}

fn safe_file_path(root: &Path, target: &str) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    for raw in target.strip_prefix('/')?.split('/') {
        if raw.is_empty() || raw.contains('\\') {
            return None;
        }
        let component = Path::new(raw).components().next()?;
        if !matches!(component, Component::Normal(_)) {
            return None;
        }
        path.push(raw);
        let metadata = fs::symlink_metadata(&path).ok()?;
        if metadata.file_type().is_symlink() {
            return None;
        }
    }
    (path != root).then_some(path)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|value| value.to_str()) {
        Some("json") => "application/json",
        Some("zst") => "application/zstd",
        Some("nar") => "application/octet-stream",
        _ => "text/plain",
    }
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}
