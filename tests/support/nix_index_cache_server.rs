//! Signed loopback index/cache peer for the index-backed Nix provider tests.
//!
//! The peer serves only bytes. It never writes a Hangar, creates a fixture
//! directory, or invokes a Nix process. Request counters make cache-repair
//! scope observable.

#![allow(dead_code)]

use jetpack::test_nix_index::{self, TestIndexRecord, TestSignedIndex};
use jetpack::Store::{self, NixCompression, NixNarInfo, NixNarInfoSignature};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub const CHANNEL: &str = "nixpkgs-unstable";
pub const SYSTEM: &str = "x86_64-linux";
pub const REVISION: &str = "c8f90650c15282fa8656a041bfbbd2403997a9a7";
const INDEX_KEY_ID: &str = "fixture-index-signer-v1";
const CACHE_KEY_ID: &str = "jet-test-cache-v1";
const SIGNING_SEED: [u8; 32] = [7; 32];

pub const ROOT_PATH: &str = "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-ripgrep-15.2.0";
pub const LIB_PATH: &str = "/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-ripgrep-lib-1.0";
pub const RUNTIME_PATH: &str = "/nix/store/cccccccccccccccccccccccccccccccc-ripgrep-runtime-1.0";
pub const DRV_PATH: &str = "/nix/store/dddddddddddddddddddddddddddddddd-ripgrep-15.2.0.drv";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObjectKind {
    Root,
    Library,
    Runtime,
}

pub struct NixIndexCacheServer {
    pub endpoint: String,
    pub index_endpoint: String,
    pub root_store_path: String,
    pub transitive_store_paths: Vec<String>,
    pub signed_index: TestSignedIndex,
    routes: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    counts: Arc<Mutex<BTreeMap<String, usize>>>,
    online: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl NixIndexCacheServer {
    pub fn start_ripgrep(scratch: &Path) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind Nix test server");
        listener
            .set_nonblocking(true)
            .expect("set Nix test server nonblocking");
        let address = listener.local_addr().expect("Nix test server address");
        let endpoint = format!("http://127.0.0.1:{}", address.port());
        let index_endpoint = format!("{endpoint}/index");
        let signed_index = test_nix_index::signed(
            SIGNING_SEED,
            INDEX_KEY_ID,
            &index_endpoint,
            CHANNEL,
            REVISION,
            SYSTEM,
            // Keep the decoded canonical JSON length aligned to a full
            // xxh64 lane.  The producer helper is shared with #2157; this
            // value also remains an otherwise inert publication timestamp.
            100_000_000_000,
            1,
            100_000_000_000,
            400_000_000_000,
            vec![TestIndexRecord {
                attrpath: vec!["ripgrep".to_string()],
                version: "15.2.0".to_string(),
                drv_path: DRV_PATH.to_string(),
                outputs: BTreeMap::from([(String::from("out"), ROOT_PATH.to_string())]),
            }],
            Vec::new(),
        )
        .expect("build signed Nix test index");
        let source = scratch.join("nix-index-cache-nar-source");
        let objects = [
            (ROOT_PATH, ObjectKind::Root, "root.nar"),
            (LIB_PATH, ObjectKind::Library, "lib.nar"),
            (RUNTIME_PATH, ObjectKind::Runtime, "runtime.nar"),
        ];
        let mut nars = BTreeMap::new();
        for (store_path, kind, _nar_name) in objects {
            let nar = make_nar(&source, kind);
            nars.insert(store_path.to_string(), nar);
        }
        let mut normalized_routes = index_routes(&signed_index, &endpoint, CHANNEL);
        normalized_routes.insert(
            "/nix-cache-info".to_string(),
            b"StoreDir: /nix/store\nWantMassQuery: 1\n".to_vec(),
        );
        for (store_path, kind, nar_name) in objects {
            let nar = nars.get(store_path).expect("Nix test NAR was built");
            let basename = store_path
                .strip_prefix("/nix/store/")
                .expect("test store path prefix");
            normalized_routes.insert(
                format!("/{}.narinfo", &basename[..32]),
                signed_narinfo(
                    store_path,
                    nar_name,
                    nar,
                    match kind {
                        ObjectKind::Root => vec![LIB_PATH],
                        ObjectKind::Library => vec![RUNTIME_PATH],
                        ObjectKind::Runtime => Vec::new(),
                    }
                    .as_slice(),
                    CACHE_KEY_ID,
                    SIGNING_SEED,
                ),
            );
            normalized_routes.insert(format!("/nar/{nar_name}"), nar.clone());
        }
        let _ = fs::remove_dir_all(&source);
        Self::finish(
            listener,
            endpoint,
            index_endpoint,
            signed_index,
            normalized_routes,
            ROOT_PATH,
            vec![LIB_PATH.to_string(), RUNTIME_PATH.to_string()],
        )
    }

    pub fn start_unindexed(scratch: &Path) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind Nix test server");
        listener
            .set_nonblocking(true)
            .expect("set Nix test server nonblocking");
        let address = listener.local_addr().expect("Nix test server address");
        let endpoint = format!("http://127.0.0.1:{}", address.port());
        let index_endpoint = format!("{endpoint}/index");
        let signed_index = test_nix_index::signed(
            SIGNING_SEED,
            INDEX_KEY_ID,
            &index_endpoint,
            CHANNEL,
            REVISION,
            SYSTEM,
            1_000_000_000_000,
            1,
            1_000_000_000_000,
            4_000_000_000_000,
            Vec::new(),
            vec![(vec!["postgres".to_string()], "missing-narinfo".to_string())],
        )
        .expect("build signed unindexed Nix test index");
        let _ = scratch;
        Self::finish(
            listener,
            endpoint.clone(),
            index_endpoint,
            signed_index.clone(),
            index_routes(&signed_index, &endpoint, CHANNEL),
            ROOT_PATH,
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn finish(
        listener: TcpListener,
        endpoint: String,
        index_endpoint: String,
        signed_index: TestSignedIndex,
        routes: BTreeMap<String, Vec<u8>>,
        root_store_path: &str,
        transitive_store_paths: Vec<String>,
    ) -> Self {
        let routes = Arc::new(Mutex::new(routes));
        let counts = Arc::new(Mutex::new(BTreeMap::new()));
        let online = Arc::new(AtomicBool::new(true));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_routes = Arc::clone(&routes);
        let thread_counts = Arc::clone(&counts);
        let thread_online = Arc::clone(&online);
        let thread_stop = Arc::clone(&stop);
        let join = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => serve_request(
                        stream,
                        &thread_routes,
                        &thread_counts,
                        thread_online.load(Ordering::Relaxed),
                    ),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            endpoint,
            index_endpoint,
            root_store_path: root_store_path.to_string(),
            transitive_store_paths,
            signed_index,
            routes,
            counts,
            online,
            stop,
            join: Some(join),
        }
    }

    pub fn install(&self, root: &Path) {
        fs::create_dir_all(root.join("config")).expect("Nix index config directory");
        fs::create_dir_all(root.join("trust")).expect("Nix index trust directory");
        fs::write(
            root.join("config/nix-index-v1.endpoint"),
            format!("{}\n", self.index_endpoint),
        )
        .expect("Nix index endpoint");
        fs::write(
            root.join("trust/nix-index-v1.ed25519.pub"),
            format!(
                "{}:{}\n",
                INDEX_KEY_ID,
                base64_encode(&self.signed_index.public_key)
            ),
        )
        .expect("Nix index trust key");
        fs::write(
            root.join("config/nix-cache-v1.endpoint"),
            format!("{}\n", self.endpoint),
        )
        .expect("Nix cache endpoint");
        fs::write(
            root.join("trust/nix-cache-v1.ed25519.pub"),
            format!(
                "{}:{}\n",
                CACHE_KEY_ID,
                base64_encode(&test_nix_index::public_key(SIGNING_SEED))
            ),
        )
        .expect("Nix cache trust key");
    }

    pub fn install_local_catalog(&self, root: &Path) {
        let target_dir = root.join("index-v1").join(REVISION).join(SYSTEM);
        fs::create_dir_all(&target_dir).expect("local Nix catalog target directory");
        let digest = jetpack::SHA256::sha256_hex(&self.signed_index.index_bytes);
        fs::write(
            target_dir.join(format!("{digest}.json.zst")),
            &self.signed_index.index_bytes,
        )
        .expect("local Nix catalog target");
    }

    pub fn corrupt_index_signature(&self) {
        let path = self
            .signed_index
            .target_signature_url
            .strip_prefix(&self.endpoint)
            .expect("index signature belongs to server");
        let mut signature = self.signed_index.index_signature.clone();
        signature.push(b'!');
        self.routes
            .lock()
            .expect("Nix test routes")
            .insert(path.to_string(), signature);
    }

    pub fn stop_network(&self) {
        self.online.store(false, Ordering::Relaxed);
    }

    pub fn start_network(&self) {
        self.online.store(true, Ordering::Relaxed);
    }

    pub fn reset_counts(&self) {
        self.counts.lock().expect("Nix request counts").clear();
    }

    pub fn count(&self, path: &str) -> usize {
        self.counts
            .lock()
            .expect("Nix request counts")
            .get(path)
            .copied()
            .unwrap_or_default()
    }

    pub fn object_request_count(&self, store_path: &str) -> usize {
        let basename = store_path
            .strip_prefix("/nix/store/")
            .expect("test store path prefix");
        let nar_path = match store_path {
            ROOT_PATH => "/nar/root.nar",
            LIB_PATH => "/nar/lib.nar",
            RUNTIME_PATH => "/nar/runtime.nar",
            _ => return self.count(&format!("/{}.narinfo", &basename[..32])),
        };
        self.count(&format!("/{}.narinfo", &basename[..32])) + self.count(nar_path)
    }
}

impl Drop for NixIndexCacheServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(
            self.endpoint
                .strip_prefix("http://")
                .expect("loopback Nix test endpoint"),
        );
        if let Some(join) = self.join.take() {
            join.join().expect("Nix test server thread");
        }
    }
}

fn index_routes(
    signed: &TestSignedIndex,
    endpoint: &str,
    channel: &str,
) -> BTreeMap<String, Vec<u8>> {
    let manifest = format!("/index/v1/{channel}/manifest.json");
    let target_path = signed
        .target_url
        .strip_prefix(endpoint)
        .expect("index target belongs to server")
        .to_string();
    let target_signature_path = signed
        .target_signature_url
        .strip_prefix(endpoint)
        .expect("index target signature belongs to server")
        .to_string();
    BTreeMap::from([
        (manifest.clone(), signed.manifest_bytes.clone()),
        (
            format!("{manifest}.sig.json"),
            signed.manifest_signature.clone(),
        ),
        (target_path, signed.index_bytes.clone()),
        (target_signature_path, signed.index_signature.clone()),
    ])
}

fn make_nar(root: &Path, kind: ObjectKind) -> Vec<u8> {
    let _ = fs::remove_dir_all(root);
    fs::create_dir_all(root).expect("Nix NAR source root");
    match kind {
        ObjectKind::Root => {
            fs::create_dir_all(root.join("bin")).expect("Nix executable directory");
            let executable = root.join("bin/rg");
            fs::write(&executable, b"#!/bin/sh\necho ripgrep 15.2.0\n").expect("Nix executable");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
                    .expect("Nix executable mode");
            }
        }
        ObjectKind::Library => {
            fs::create_dir_all(root.join("lib")).expect("Nix library directory");
            fs::write(root.join("lib/marker"), b"library").expect("Nix library marker");
        }
        ObjectKind::Runtime => {
            fs::create_dir_all(root.join("share")).expect("Nix runtime directory");
            fs::write(root.join("share/marker"), b"runtime").expect("Nix runtime marker");
        }
    }
    let (nar, _) = Store::write_nar(root).expect("encode Nix test NAR");
    nar
}

fn signed_narinfo(
    store_path: &str,
    nar_name: &str,
    nar: &[u8],
    references: &[&str],
    key_id: &str,
    seed: [u8; 32],
) -> Vec<u8> {
    let hash = format!("sha256:{}", jetpack::SHA256::sha256_hex(nar));
    let references = references
        .iter()
        .map(|reference| {
            reference
                .rsplit('/')
                .next()
                .expect("Nix reference basename")
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    let info = NixNarInfo {
        store_path: store_path.to_string(),
        url: format!("nar/{nar_name}"),
        compression: NixCompression::None,
        file_hash: Some(hash.clone()),
        file_size: Some(nar.len() as u64),
        nar_hash: hash,
        nar_size: nar.len() as u64,
        references: references.clone(),
        deriver: None,
        ca: None,
        signatures: Vec::new(),
    };
    let signature = test_nix_index::sign(
        seed,
        &info
            .fingerprint("/nix/store")
            .expect("Nix narinfo fingerprint"),
    );
    let signature = signature
        .try_into()
        .expect("ed25519 signature has 64 bytes");
    let signature = NixNarInfoSignature {
        key_id: key_id.to_string(),
        signature,
    };
    format!(
        "StorePath: {}\nURL: {}\nCompression: none\nFileHash: {}\nFileSize: {}\nNarHash: {}\nNarSize: {}\nReferences: {}\nSig: {}:{}\n",
        info.store_path,
        info.url,
        info.file_hash.as_deref().expect("Nix FileHash"),
        info.file_size.expect("Nix FileSize"),
        info.nar_hash,
        info.nar_size,
        references.join(" "),
        key_id,
        base64_encode(&signature.signature),
    )
    .into_bytes()
}

fn serve_request(
    mut stream: TcpStream,
    routes: &Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    counts: &Arc<Mutex<BTreeMap<String, usize>>>,
    online: bool,
) {
    let mut request = [0u8; 16 * 1024];
    let count = stream.read(&mut request).unwrap_or_default();
    let path = std::str::from_utf8(&request[..count])
        .ok()
        .and_then(|request| request.lines().next())
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/")
        .to_string();
    *counts
        .lock()
        .expect("Nix request counts")
        .entry(path.clone())
        .or_default() += 1;
    let body = if online {
        routes.lock().expect("Nix test routes").get(&path).cloned()
    } else {
        None
    };
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
                b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        output.push(ALPHABET[(first >> 2) as usize] as char);
        output.push(
            ALPHABET
                [((first & 0x03) << 4 | chunk.get(1).copied().unwrap_or_default() >> 4) as usize]
                as char,
        );
        if let Some(second) = chunk.get(1) {
            output.push(
                ALPHABET[((second & 0x0f) << 2 | chunk.get(2).copied().unwrap_or_default() >> 6)
                    as usize] as char,
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
