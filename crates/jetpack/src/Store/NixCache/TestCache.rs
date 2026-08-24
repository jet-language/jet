use super::*;
use crate::Store::NixNarInfoSignature;
use ed25519_dalek::{Signer, SigningKey};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub(super) fn unique_dir(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("jet-nix-cache-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

pub(super) fn remove_dir(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

pub(super) fn signed_narinfo(
    store_path: &str,
    nar_name: &str,
    nar: &[u8],
    references: &[&str],
    key_id: &str,
    signing_key: &SigningKey,
) -> Vec<u8> {
    let hash = format!("sha256:{}", crate::SHA256::sha256_hex(nar));
    let references = references
        .iter()
        .map(|reference| reference.rsplit('/').next().unwrap())
        .collect::<Vec<_>>()
        .join(" ");
    let mut info = NixNarInfo {
        store_path: store_path.to_string(),
        url: format!("nar/{nar_name}"),
        compression: NixCompression::None,
        file_hash: Some(hash.clone()),
        file_size: Some(nar.len() as u64),
        nar_hash: hash,
        nar_size: nar.len() as u64,
        references: references.split_whitespace().map(str::to_string).collect(),
        deriver: None,
        ca: None,
        signatures: Vec::new(),
    };
    let fingerprint = info.fingerprint("/nix/store").unwrap();
    info.signatures.push(NixNarInfoSignature {
        key_id: key_id.to_string(),
        signature: signing_key.sign(&fingerprint).to_bytes(),
    });
    format!(
        "StorePath: {}\nURL: {}\nCompression: none\nFileHash: {}\nFileSize: {}\nNarHash: {}\nNarSize: {}\nReferences: {}\nSig: {}:{}\n",
        info.store_path,
        info.url,
        info.file_hash.as_deref().unwrap(),
        info.file_size.unwrap(),
        info.nar_hash,
        info.nar_size,
        references,
        key_id,
        base64_encode(&info.signatures[0].signature),
    )
    .into_bytes()
}

pub(super) fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        output.push(ALPHABET[(first >> 2) as usize] as char);
        output.push(
            ALPHABET[((first & 0x03) << 4 | chunk.get(1).copied().unwrap_or(0) >> 4) as usize]
                as char,
        );
        if let Some(second) = chunk.get(1) {
            output.push(
                ALPHABET[((second & 0x0f) << 2 | chunk.get(2).copied().unwrap_or(0) >> 6) as usize]
                    as char,
            );
        } else {
            output.push('=');
        }
        if let Some(third) = chunk.get(2) {
            output.push(ALPHABET[(third & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

pub(super) struct TestCacheServer {
    pub(super) endpoint: String,
    routes: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl TestCacheServer {
    pub(super) fn start(routes: BTreeMap<String, Vec<u8>>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let routes = Arc::new(Mutex::new(routes));
        let thread_routes = Arc::clone(&routes);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let join = thread::spawn(move || {
            while !thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => serve_cache_request(stream, &thread_routes),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            endpoint: format!("http://127.0.0.1:{}", address.port()),
            routes,
            stop,
            join: Some(join),
        }
    }

    pub(super) fn replace_routes(&self, routes: BTreeMap<String, Vec<u8>>) {
        *self.routes.lock().unwrap() = routes;
    }
}

impl Drop for TestCacheServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = TcpStream::connect(self.endpoint.strip_prefix("http://").unwrap());
        if let Some(join) = self.join.take() {
            join.join().unwrap();
        }
    }
}

fn serve_cache_request(mut stream: TcpStream, routes: &Arc<Mutex<BTreeMap<String, Vec<u8>>>>) {
    let mut request = [0u8; 4096];
    let count = stream.read(&mut request).unwrap_or(0);
    let path = std::str::from_utf8(&request[..count])
        .ok()
        .and_then(|request| request.lines().next())
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/");
    let body = routes.lock().unwrap().get(path).cloned();
    match body {
        Some(body) => {
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
        }
        None => {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    }
}
