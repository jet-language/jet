use super::*;

use crate::RuntimePolicy;
use ed25519_dalek::{Signer, SigningKey};
#[cfg(target_os = "linux")]
use jet_env_model::ModuleEval::{PromptPathMode, PromptStripMode};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(target_os = "linux")]
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[cfg(target_os = "linux")]
#[path = "../../../../../../tests/support/no_nix_namespace.rs"]
mod no_nix_namespace;

#[cfg(target_os = "linux")]
const ROOTLESS_PROJECTION_ROOT: &str = "JETPACK_ROOTLESS_PROJECTION_ROOT";

#[cfg(target_os = "linux")]
#[test]
fn nix_projection_runs_in_rootless_namespace_without_host_store() {
    let test_name = std::thread::current()
        .name()
        .expect("rootless projection test name")
        .to_string();
    let is_child = std::env::var_os(no_nix_namespace::CHILD_MARKER).is_some();
    if is_child {
        no_nix_namespace::run_in_no_nix_namespace(
            &test_name,
            no_nix_namespace::NetworkMode::Enabled,
            run_rootless_projection_child,
        );
        return;
    }

    let (roots, _guard) = temp_roots();
    let admitted = admit_signed_closure(&roots);
    let entries = canonicalize_admitted_records(&roots, &admitted);
    assert!(entries.iter().any(|entry| {
        ProducerRecord::decode(&entry.producer_record)
            .ok()
            .and_then(|producer| producer.facts.get("nix.store-path").cloned())
            == Some("/nix/store/00000000000000000000000000000000-root".into())
    }));

    let previous_root = std::env::var_os(ROOTLESS_PROJECTION_ROOT);
    std::env::set_var(ROOTLESS_PROJECTION_ROOT, &roots.root);
    no_nix_namespace::run_in_no_nix_namespace(
        &test_name,
        no_nix_namespace::NetworkMode::Enabled,
        run_rootless_projection_child,
    );
    match previous_root {
        Some(value) => std::env::set_var(ROOTLESS_PROJECTION_ROOT, value),
        None => std::env::remove_var(ROOTLESS_PROJECTION_ROOT),
    }
}

#[cfg(target_os = "linux")]
fn run_rootless_projection_child() {
    let root = PathBuf::from(
        std::env::var_os(ROOTLESS_PROJECTION_ROOT)
            .expect("rootless projection child needs its admitted Hangar root"),
    );
    let roots = Roots {
        root,
        dev_mode: true,
    };
    let entries = list_checked(&roots).unwrap();
    let entry = find_entry(&entries, "/nix/store/00000000000000000000000000000000-root");
    let lease = snapshot_lease(&roots, &entry).unwrap();
    let env = crate::Shell::Env {
        bin_dirs: Vec::new(),
        vars: std::collections::BTreeMap::new(),
        unset_vars: Vec::new(),
        refs: vec![entry.reference.clone()],
        label: "nix-projection-test".into(),
        prompt_path: PromptPathMode::Short,
        prompt_strip: PromptStripMode::Off,
        cache_leases: vec![lease],
    };
    let code = crate::Shell::run_clean_command(
        &env,
        &["/nix/store/00000000000000000000000000000000-root/bin/rg".into()],
    );
    assert_eq!(code, 0);
}

#[test]
fn nix_store_projection_includes_every_hangar_closure_object() {
    let (roots, _guard) = temp_roots();
    let admitted = admit_signed_closure(&roots);
    let entries = canonicalize_admitted_records(&roots, &admitted);
    let root_path = admitted.outputs["out"].store_path.clone();
    let entry = find_entry(&entries, &root_path);
    let lease = snapshot_lease(&roots, &entry).unwrap();

    let projected = lease
        .nix_store_projection()
        .iter()
        .map(|(logical, source)| {
            let resolved = fs::read_link(source).unwrap_or_else(|_| source.clone());
            assert!(
                !resolved.starts_with("/nix/store"),
                "projection source escaped into the host store: {}",
                resolved.display()
            );
            (logical.clone(), resolved)
        })
        .collect::<BTreeMap<_, _>>();
    let expected_paths = admitted
        .objects
        .keys()
        .map(|path| path.strip_prefix("/nix/store/").unwrap().to_string())
        .collect::<BTreeSet<_>>();
    let actual_paths = projected
        .keys()
        .map(|path| path.strip_prefix("/nix/store/").unwrap().to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual_paths, expected_paths);
    assert!(projected.values().all(|path| {
        path == &lease.snapshot_root || path.starts_with(roots.hangar_dir().join(OBJECTS_DIR))
    }));
}

#[test]
fn nix_store_projection_rejects_missing_conflicting_or_external_objects() {
    {
        let (roots, _guard) = temp_roots();
        let admitted = admit_signed_closure(&roots);
        let entries = canonicalize_admitted_records(&roots, &admitted);
        let root_path = admitted.outputs["out"].store_path.clone();
        let entry = find_entry(&entries, &root_path);
        let leaf = admitted
            .objects
            .values()
            .find(|object| object.store_path != root_path)
            .unwrap();
        fs::remove_dir_all(&leaf.hangar_path).unwrap();
        let error = snapshot_lease(&roots, &entry)
            .err()
            .expect("missing closure object must reject the lease");
        assert!(error.to_string().contains("Hangar object"), "{error}");
    }

    {
        let (roots, _guard) = temp_roots();
        let admitted = admit_signed_closure(&roots);
        let entries = canonicalize_admitted_records(&roots, &admitted);
        let root_path = admitted.outputs["out"].store_path.clone();
        let root_entry = find_entry(&entries, &root_path);
        let leaf_path = admitted
            .objects
            .keys()
            .find(|path| *path != &root_path)
            .unwrap()
            .clone();
        let mut conflict = entries
            .iter()
            .find(|entry| {
                ProducerRecord::decode(&entry.producer_record)
                    .ok()
                    .and_then(|producer| producer.facts.get("nix.store-path").cloned())
                    == Some(leaf_path.clone())
            })
            .unwrap()
            .clone();
        conflict.name = "nix-projection-conflict".into();
        conflict.reference = "nix-projection-conflict@nixpkgs".into();
        conflict.id = entry_id(
            &conflict.name,
            &conflict.version,
            &conflict.reference,
            &conflict.out,
        );
        let mut producer = ProducerRecord::decode(&conflict.producer_record).unwrap();
        producer.facts.insert(
            "nix.output.out".into(),
            "/nix/store/conflicting-leaf".into(),
        );
        conflict.producer_record = producer.encode();
        conflict.receipt.clear();
        RuntimePolicy::with_lock(&roots.root, "hangar", || {
            Closure::prepare_entry_receipt(&roots, &mut conflict)?;
            Closure::register_entry_unlocked(&roots, &conflict)
        })
        .unwrap();
        let error = snapshot_lease(&roots, &root_entry)
            .err()
            .expect("conflicting closure owner must reject the lease");
        assert!(
            error.to_string().contains("canonical store paths"),
            "{error}"
        );
    }

    #[cfg(unix)]
    {
        let (roots, _guard) = temp_roots();
        let admitted = admit_signed_closure(&roots);
        let entries = canonicalize_admitted_records(&roots, &admitted);
        let root_path = admitted.outputs["out"].store_path.clone();
        let entry = find_entry(&entries, &root_path);
        let leaf = admitted
            .objects
            .values()
            .find(|object| object.store_path != root_path)
            .unwrap();
        let target = roots.root.join("external-object");
        fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &leaf.hangar_path).unwrap();
        let error = snapshot_lease(&roots, &entry)
            .err()
            .expect("external closure object must reject the lease");
        assert!(error.to_string().contains("Hangar object"), "{error}");
    }
}

fn find_entry(entries: &[StoreEntry], store_path: &str) -> StoreEntry {
    entries
        .iter()
        .find(|entry| {
            ProducerRecord::decode(&entry.producer_record)
                .ok()
                .and_then(|producer| producer.facts.get("nix.store-path").cloned())
                .as_deref()
                == Some(store_path)
        })
        .cloned()
        .unwrap_or_else(|| panic!("missing admitted Nix entry for {store_path}"))
}

fn canonicalize_admitted_records(roots: &Roots, admitted: &AdmittedNixClosure) -> Vec<StoreEntry> {
    let entries = list_checked(roots).unwrap();
    let mut canonical = Vec::new();
    for mut entry in entries {
        let mut producer = ProducerRecord::decode(&entry.producer_record).unwrap();
        let Some(store_path) = producer.facts.get("nix.store-path").cloned() else {
            continue;
        };
        if !admitted.objects.contains_key(&store_path) {
            continue;
        }
        for (key, value) in crate::Provider::nix_build_facts_record() {
            producer.facts.insert(key, value);
        }
        producer.facts.insert("nix.output.out".into(), store_path);
        entry.producer_record = producer.encode();
        entry.receipt.clear();
        canonical.push(entry);
    }
    RuntimePolicy::with_lock(&roots.root, "hangar", || {
        for entry in &mut canonical {
            Closure::prepare_entry_receipt(roots, entry)?;
        }
        Closure::register_entries_unlocked(roots, &canonical)
    })
    .unwrap();
    list_checked(roots).unwrap()
}

fn admit_signed_closure(roots: &Roots) -> AdmittedNixClosure {
    let source = roots.root.join("nix-projection-source");
    fs::create_dir_all(source.join("root/bin")).unwrap();
    fs::create_dir_all(source.join("leaf")).unwrap();
    fs::write(
        source.join("root/bin/rg"),
        b"#!/bin/sh\ntest -f /nix/store/11111111111111111111111111111111-leaf/leaf/payload\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(
            source.join("root/bin/rg"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    fs::write(source.join("leaf/payload"), b"leaf").unwrap();
    let (root_nar, _) = write_nar(&source.join("root")).unwrap();
    let (leaf_nar, _) = write_nar(&source.join("leaf")).unwrap();
    let root_path = "/nix/store/00000000000000000000000000000000-root";
    let leaf_path = "/nix/store/11111111111111111111111111111111-leaf";
    let signing_key = SigningKey::from_bytes(&[9; 32]);
    let key_id = "jetpack-projection-test-1";
    let routes = BTreeMap::from([
        (
            "/nix-cache-info".into(),
            b"StoreDir: /nix/store\nWantMassQuery: 1\n".to_vec(),
        ),
        (
            "/00000000000000000000000000000000.narinfo".into(),
            signed_narinfo(
                root_path,
                "root.nar",
                &root_nar,
                &[leaf_path],
                key_id,
                &signing_key,
            ),
        ),
        (
            "/11111111111111111111111111111111.narinfo".into(),
            signed_narinfo(leaf_path, "leaf.nar", &leaf_nar, &[], key_id, &signing_key),
        ),
        ("/nar/root.nar".into(), root_nar),
        ("/nar/leaf.nar".into(), leaf_nar),
    ]);
    let server = ProjectionCacheServer::start(routes);
    fs::create_dir_all(roots.root.join("config")).unwrap();
    fs::create_dir_all(roots.root.join("trust")).unwrap();
    fs::write(
        roots.root.join("config/nix-cache-v1.endpoint"),
        &server.endpoint,
    )
    .unwrap();
    fs::write(
        roots.root.join("trust/nix-cache-v1.ed25519.pub"),
        format!(
            "{key_id}:{}\n",
            base64_encode(&signing_key.verifying_key().to_bytes())
        ),
    )
    .unwrap();
    let admitted = admit_nix_closure(
        roots,
        &[NixOutputRequest {
            name: "out".into(),
            store_path: root_path.into(),
        }],
        false,
    )
    .unwrap();
    drop(server);
    admitted
}

fn signed_narinfo(
    store_path: &str,
    nar_name: &str,
    nar: &[u8],
    references: &[&str],
    key_id: &str,
    signing_key: &SigningKey,
) -> Vec<u8> {
    let hash = format!("sha256:{}", SHA256::sha256_hex(nar));
    let references = references
        .iter()
        .map(|reference| reference.rsplit('/').next().unwrap())
        .collect::<Vec<_>>()
        .join(" ");
    let mut info = NixNarInfo {
        store_path: store_path.into(),
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
        key_id: key_id.into(),
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

fn base64_encode(bytes: &[u8]) -> String {
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

struct ProjectionCacheServer {
    endpoint: String,
    stop: Arc<std::sync::atomic::AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl ProjectionCacheServer {
    fn start(routes: BTreeMap<String, Vec<u8>>) -> Self {
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
                    Ok((stream, _)) => serve_request(stream, &thread_routes),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            endpoint: format!("http://127.0.0.1:{}", address.port()),
            stop,
            join: Some(join),
        }
    }
}

impl Drop for ProjectionCacheServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = TcpStream::connect(self.endpoint.strip_prefix("http://").unwrap());
        if let Some(join) = self.join.take() {
            join.join().unwrap();
        }
    }
}

fn serve_request(mut stream: TcpStream, routes: &Arc<Mutex<BTreeMap<String, Vec<u8>>>>) {
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
