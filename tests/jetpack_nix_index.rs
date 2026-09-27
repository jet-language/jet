//! Card #2158: signed-index Nix provider realization and closure repair.

use std::fs;
use std::path::Path;

use jetpack::Store::{self, ProducerRecord};

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
#[path = "support/nix_index_cache_server.rs"]
mod nix_index_cache_server;

use jetpack_fixtures::{
    assert_jetos_stderr_snapshot_normalized, copy_dir_recursive, jetpack, Scratch,
};
use nix_index_cache_server::{NixIndexCacheServer, LIB_PATH, RUNTIME_PATH};

#[path = "support/static_file_server.rs"]
mod static_file_server;
use static_file_server::StaticFileServer;

const REVISION: &str = nix_index_cache_server::REVISION;

fn project(scratch: &Scratch) {
    fs::create_dir_all(scratch.join(".jet")).expect("project managed directory");
    fs::write(
        scratch.join(".jet/lock"),
        format!(
            "version = 1\n\n[[source_channel]]\nname = \"jetpack\"\nchannel = \"nixpkgs-unstable\"\nexact = \"github:NixOS/nixpkgs#{REVISION}\"\n\n[root]\ndependencies = []\n"
        ),
    )
    .expect("project lock");
    fs::write(
        scratch.join("env.jet"),
        "module env.dev {\n    sources: { jetpack: NixOS/nixpkgs/nixpkgs-unstable@github }\n    packages: [jetpack.ripgrep]\n}\n",
    ).expect("project environment");
}

fn isolated_jetpack(project: &Scratch, root: &Scratch) -> std::process::Command {
    let temporary = root.join("tmp");
    fs::create_dir_all(&temporary).expect("private command temporary directory");
    let mut command = jetpack();
    command
        .env_clear()
        .current_dir(&project.path)
        .env("PATH", "")
        .env("HOME", root.join("home"))
        .env("XDG_DATA_HOME", root.join("home/data"))
        .env("XDG_STATE_HOME", root.join("home/state"))
        .env("XDG_CACHE_HOME", root.join("home/cache"))
        .env("XDG_CONFIG_HOME", root.join("home/config"))
        .env("XDG_RUNTIME_DIR", root.join("run"))
        .env("TMPDIR", &temporary)
        .env("TMP", &temporary)
        .env("TEMP", &temporary)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_SHARED_CAS", root.join("hangar/cas"))
        .env("JETPACK_NIX_FALLBACK_POLICY", "deny");
    command
}

fn prepare(project: &Scratch, root: &Scratch, offline: bool) -> std::process::Output {
    let mut command = isolated_jetpack(project, root);
    command.args(["env", "--env", "dev", "--prep", "--trust", "--yes", "--no-color"]);
    if offline {
        command.arg("--offline");
    }
    command.output().expect("prepare index-backed Nix environment")
}

fn package_entry(root: &Scratch) -> Store::StoreEntry {
    Store::list_checked(&Store::Roots::at(root.path.clone()))
        .expect("list Hangar")
        .into_iter()
        .find(|entry| entry.reference == "ripgrep@jetpack" && entry.version == "15.2.0")
        .expect("realized ripgrep package entry")
}

fn make_directories_writable(path: &Path) {
    let metadata = fs::symlink_metadata(path).expect("inspect Hangar object");
    if !metadata.is_dir() {
        return;
    }
    let mut permissions = metadata.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        permissions.set_mode(permissions.mode() | 0o200);
    }
    #[cfg(not(unix))]
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).expect("make Hangar directory writable");
    for child in fs::read_dir(path).expect("read Hangar object") {
        make_directories_writable(&child.expect("Hangar object child").path());
    }
}

#[test]
fn index_backed_nixpkgs_records_complete_closure_and_both_proofs() {
    let project_root = Scratch::new("nix-index-project");
    let hangar_root = Scratch::new("nix-index-hangar");
    let server = NixIndexCacheServer::start_ripgrep(&project_root.path);
    server.install(&hangar_root.path);
    project(&project_root);

    let output = prepare(&project_root, &hangar_root, false);
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let entry = package_entry(&hangar_root);
    assert!(!entry.references.is_empty(), "Realized.references is empty");
    let roots = Store::Roots::at(hangar_root.path.clone());
    let graph = Store::closure_graph(&roots).expect("read Hangar closure graph");
    let closure = graph.closure(&entry.envelope.output_hash);
    assert_eq!(closure.len(), 3, "closure graph: {graph:?}");
    assert!(entry
        .references
        .iter()
        .all(|digest| graph.objects.contains_key(digest)));
    assert!(
        graph
            .transitive_references(&entry.envelope.output_hash)
            .len()
            >= 2
    );

    let producer = ProducerRecord::decode(&entry.producer_record).expect("decode Nix producer");
    for fact in [
        "nix.index.proof.v1",
        "nix.index.record.sha256",
        "nix.index.target.sha256",
        "nix.index.manifest.sha256",
        "nix.cache.output.out.proof.sha256",
        "nix.cache.closure.receipt.sha256",
    ] {
        assert!(
            producer
                .facts
                .get(fact)
                .is_some_and(|value| !value.is_empty()),
            "missing provenance fact {fact}: {:?}",
            producer.facts
        );
    }
    assert_eq!(
        producer.facts.get("nix.native.format").map(String::as_str),
        Some("jet-nixpkgs-index-v1")
    );
    assert!(producer
        .facts
        .get("nix.index.proof.v1")
        .is_some_and(|proof| proof.contains("fixture-index-signer-v1")));
}

// Card #2200: the client must resolve the same signed layout from a
// disk-backed static publication root.
#[test]
fn static_publication_resolves_signed_index_and_nix_objects() {
    let project_root = Scratch::new("static-nix-index-project");
    let hangar_root = Scratch::new("static-nix-index-hangar");
    let publication = Scratch::new("static-nix-index-publication");
    let server = NixIndexCacheServer::start_static_ripgrep(&project_root.path, &publication.path);
    server.install(&hangar_root.path);
    project(&project_root);

    let output = prepare(&project_root, &hangar_root, false);
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(publication.join("v1/nixpkgs-unstable/manifest.json").is_file());
    assert!(publication
        .join("v1/nixpkgs-unstable/manifest.json.sig.json")
        .is_file());
    assert!(publication.join("index-v1").is_dir());
    assert!(publication.join("nar").is_dir());
}

// Card #2200: native cache objects remain content-addressed and untrusted
// bytes never cross the normal verification boundary.
#[test]
fn static_jetpack_cache_admits_content_addressed_objects_only_after_verification() {
    let root = Scratch::new("static-cache-root");
    let source = Scratch::new("static-cache-source");
    let local_cache = Scratch::new("static-cache-local");
    let publication = Scratch::new("static-cache-publication");
    let restored = Scratch::new("static-cache-restored");
    let roots = Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    fs::write(source.join("payload"), "static cache bytes\n").expect("cache source");
    let entry = Store::ingest_tree(
        &roots,
        &jetpack::Store::IngestRequest {
            name: "static-cache-demo".into(),
            version: "1".into(),
            reference: "./static-cache-demo".into(),
            cache_identity: jetpack::Store::CacheIdentity {
                source_fingerprint: "sha256:static-cache-source".into(),
                recipe_fingerprint: "sha256:static-cache-recipe".into(),
                policy_fingerprint: "sha256:static-cache-policy".into(),
                platform: jetpack::Envelope::host_platform(),
            },
            references: Vec::new(),
            outputs: std::collections::BTreeMap::from([("out".into(), source.path.clone())]),
            signature: String::new(),
            provenance: "static cache test".into(),
            platform_artifact_kind: String::new(),
        },
    )
    .expect("ingest static cache entry")
    .entry;
    Store::bind_cache(
        &roots,
        "public",
        vec![local_cache.path.display().to_string()],
        None,
        None,
        true,
    )
    .expect("bind local cache publisher");
    let published = Store::publish_cache_entry(&roots, &entry.id, "public")
        .expect("publish signed local cache objects");
    copy_dir_recursive(&local_cache.path, &publication.path);
    assert!(publication
        .join("nar")
        .join(format!("{}.nar", entry.envelope.output_hash))
        .is_file());
    assert!(publication
        .join(&format!("{}-{}.narinfo", entry.envelope.output_hash, entry.id))
        .is_file());
    assert!(publication
        .join("trust")
        .join(format!("{}-{}.receipt", entry.envelope.output_hash, entry.id))
        .is_file());
    assert!(!publication.join("cache-public.key").exists());

    let server = StaticFileServer::start(&publication.path);
    let trust_key = root.join("trust/cache-public.key");
    Store::bind_cache(
        &roots,
        "public",
        vec![server.endpoint.clone()],
        Some(&trust_key),
        None,
        false,
    )
    .expect("bind read-only static cache");
    let verified = Store::verify_cache_transfer(&roots, &entry.id, "public")
        .expect("verify static cache objects");
    assert_eq!(verified.mirror, server.endpoint);
    assert_eq!(verified.output_hash, published.output_hash);
    assert_eq!(verified.witness, published.witness);
    Store::substitute_cache_entry(&roots, &entry.id, "public", &restored.join("out"))
        .expect("substitute from verified static cache");
    assert_eq!(fs::read_to_string(restored.join("out/payload")).unwrap(), "static cache bytes\n");

    let info_path = publication.join(&format!(
        "{}-{}.narinfo",
        entry.envelope.output_hash, entry.id
    ));
    let signed_info = fs::read(&info_path).expect("signed static narinfo");
    let unsigned_info = String::from_utf8(signed_info.clone())
        .expect("narinfo UTF-8")
        .lines()
        .filter(|line| !line.starts_with("Sig: "))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&info_path, unsigned_info).expect("remove static narinfo signature");
    assert!(Store::verify_cache_transfer(&roots, &entry.id, "public").is_err());
    fs::write(info_path, signed_info).expect("restore signed static narinfo");
}

#[test]
fn index_backed_nixpkgs_reuses_closure_offline_after_network_removal() {
    let project_root = Scratch::new("nix-index-offline-project");
    let hangar_root = Scratch::new("nix-index-offline-hangar");
    let server = NixIndexCacheServer::start_ripgrep(&project_root.path);
    server.install(&hangar_root.path);
    project(&project_root);
    let initial = prepare(&project_root, &hangar_root, false);
    assert!(
        initial.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&initial.stdout),
        String::from_utf8_lossy(&initial.stderr),
    );

    server.reset_counts();
    server.stop_network();
    let output = prepare(&project_root, &hangar_root, true);
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(server.count("/nix-cache-info"), 0);
    assert_eq!(server.object_request_count(&server.root_store_path), 0);
    assert_eq!(server.object_request_count(LIB_PATH), 0);
    assert_eq!(server.object_request_count(RUNTIME_PATH), 0);
}

#[test]
fn index_backed_nixpkgs_repairs_only_missing_transitive_object() {
    let project_root = Scratch::new("nix-index-repair-project");
    let hangar_root = Scratch::new("nix-index-repair-hangar");
    let server = NixIndexCacheServer::start_ripgrep(&project_root.path);
    server.install(&hangar_root.path);
    project(&project_root);
    let initial = prepare(&project_root, &hangar_root, false);
    assert!(
        initial.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&initial.stdout),
        String::from_utf8_lossy(&initial.stderr),
    );

    let roots = Store::Roots::at(hangar_root.path.clone());
    let original = package_entry(&hangar_root);
    let original_lock = fs::read(project_root.join(".jet/lock")).unwrap();
    let expectation = Store::CacheExpectation {
        identity: original.cache_identity.clone(),
        owned_output: None,
        allow_unsigned_local: true,
    };
    let deleted_path = Store::list_checked(&roots)
        .expect("list before deletion")
        .into_iter()
        .find_map(|entry| {
            let producer = ProducerRecord::decode(&entry.producer_record).ok()?;
            (producer.facts.get("nix.store-path").map(String::as_str) == Some(RUNTIME_PATH))
                .then_some(entry.out)
        })
        .expect("runtime Hangar object");
    make_directories_writable(Path::new(&deleted_path));
    fs::remove_dir_all(&deleted_path).expect("delete one transitive Hangar object");
    let mut forged_entry = original.clone();
    let mut forged_producer = ProducerRecord::decode(&original.producer_record).unwrap();
    forged_producer.facts.insert(
        "nix.cache.closure.receipt.sha256".into(), format!("sha256-{}", "d".repeat(64)),
    );
    forged_entry.producer_record = forged_producer.encode();
    assert!(!Store::verify_cache_entry(
        &roots, &forged_entry, "ripgrep@jetpack", &expectation,
    ).policy, "a re-checksummed producer must not replace the original WAL-bound repair authority");

    server.reset_counts();
    server.stop_network();
    let offline = prepare(&project_root, &hangar_root, true);
    assert_eq!(
        offline.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&offline.stdout),
        String::from_utf8_lossy(&offline.stderr)
    );
    let offline_stderr = String::from_utf8_lossy(&offline.stderr);
    assert!(offline_stderr.contains("E1350"), "stderr: {offline_stderr}");
    assert!(
        offline_stderr.contains("ripgrep@jetpack"),
        "offline error must name the affected locked package: {offline_stderr}"
    );
    assert_eq!(server.count("/nix-cache-info"), 0);
    assert_eq!(server.object_request_count(RUNTIME_PATH), 0);

    server.reset_counts();
    server.start_network();
    let mut tampered = jetpack::Lock::parse(std::str::from_utf8(&original_lock).unwrap()).unwrap();
    tampered.packages.iter_mut().find_map(|package| package.nix_closure.as_mut())
        .unwrap().index_authority = "d".repeat(64);
    fs::write(project_root.join(".jet/lock"), jetpack::Lock::write(&tampered)).unwrap();
    let rejected = prepare(&project_root, &hangar_root, false);
    assert_eq!(rejected.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("E1350"));
    assert_eq!(server.count("/nix-cache-info"), 0);
    assert_eq!(server.object_request_count(&server.root_store_path), 0);
    assert_eq!(server.object_request_count(LIB_PATH), 0);
    assert_eq!(server.object_request_count(RUNTIME_PATH), 0);
    assert!(!Path::new(&deleted_path).exists());
    assert_eq!(package_entry(&hangar_root).producer_record, original.producer_record);
    fs::write(project_root.join(".jet/lock"), &original_lock).unwrap();
    let repaired = prepare(&project_root, &hangar_root, false);
    assert!(
        repaired.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&repaired.stdout),
        String::from_utf8_lossy(&repaired.stderr)
    );
    assert_eq!(server.object_request_count(&server.root_store_path), 0);
    assert_eq!(server.object_request_count(LIB_PATH), 0);
    assert_eq!(server.object_request_count(RUNTIME_PATH), 2);
    assert_eq!(server.count("/nix-cache-info"), 1);
    let after = package_entry(&hangar_root);
    assert_eq!(after.producer_record, original.producer_record);
    assert_eq!(after.envelope, original.envelope);
    assert_eq!(after.named_outputs, original.named_outputs);
    assert_eq!(fs::read(project_root.join(".jet/lock")).unwrap(), original_lock);
    assert!(Store::verify_cache_entry(&roots, &after, "ripgrep@jetpack", &expectation).trusted());
}

#[test]
fn hangar_doctor_reports_and_repairs_cache_drift_and_staging() {
    let project_root = Scratch::new("nix-index-doctor-project");
    let hangar_root = Scratch::new("nix-index-doctor-hangar");
    let server = NixIndexCacheServer::start_ripgrep(&project_root.path);
    server.install(&hangar_root.path);
    project(&project_root);
    let initial = prepare(&project_root, &hangar_root, false);
    assert!(
        initial.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&initial.stdout),
        String::from_utf8_lossy(&initial.stderr),
    );

    let roots = Store::Roots::at(hangar_root.path.clone());
    let (runtime_output, runtime_digest) = Store::list_checked(&roots)
        .expect("list Hangar before doctor")
        .into_iter()
        .find_map(|entry| {
            let producer = ProducerRecord::decode(&entry.producer_record).ok()?;
            (producer.facts.get("nix.store-path").map(String::as_str) == Some(RUNTIME_PATH))
                .then_some((entry.out, entry.envelope.output_hash))
        })
        .expect("runtime Hangar object");
    let runtime_output = Path::new(&runtime_output);
    make_directories_writable(runtime_output);
    let corruption = runtime_output.join("share/doctor-corruption");
    fs::write(&corruption, b"drift").expect("corrupt runtime object");

    let stale_stage = roots
        .hangar_dir()
        .join("stage/nix-cache-999999999-1/payload");
    fs::create_dir_all(&stale_stage).expect("create stale admission stage");
    fs::write(stale_stage.join("bytes"), b"stale").expect("write stale admission stage");
    let orphan_cas = roots.hangar_dir().join("cas/.unreferenced");
    fs::create_dir_all(orphan_cas.parent().expect("CAS parent")).expect("create CAS pool");
    fs::write(&orphan_cas, b"orphan").expect("write orphan CAS entry");

    let cas_locks = roots.hangar_dir().join("cas/.locks");
    assert!(cas_locks.is_dir(), "admission did not create the CAS lock namespace");
    #[cfg(unix)]
    let cas_lock_identity = {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = fs::metadata(&cas_locks).unwrap();
        (metadata.dev(), metadata.ino())
    };
    let live_payload = fs::read_dir(roots.hangar_dir().join("cas"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.is_file() && fs::read(path).unwrap() == b"runtime")
        .expect("live runtime payload in the private CAS");
    #[cfg(unix)]
    let live_payload_identity = {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = fs::metadata(&live_payload).unwrap();
        (metadata.dev(), metadata.ino())
    };
    let mut doctor = isolated_jetpack(&project_root, &hangar_root);
    let read_only = doctor
        .args(["hangar", "doctor", "--no-color"])
        .output()
        .expect("run read-only Hangar doctor");
    assert_eq!(read_only.status.code(), Some(1));
    assert!(corruption.exists(), "read-only doctor changed object bytes");
    assert!(
        stale_stage.parent().expect("stage parent").exists(),
        "read-only doctor removed staging"
    );
    assert!(orphan_cas.exists(), "read-only doctor removed CAS entry");
    assert_eq!(fs::read(&live_payload).unwrap(), b"runtime");

    server.reset_counts();
    let mut repair = isolated_jetpack(&project_root, &hangar_root);
    let repaired = repair
        .args(["hangar", "doctor", "--repair", "--no-color"])
        .output()
        .expect("run repairing Hangar doctor");
    assert!(
        repaired.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&repaired.stdout),
        String::from_utf8_lossy(&repaired.stderr)
    );
    assert!(server.object_request_count(RUNTIME_PATH) > 0);
    assert!(!stale_stage.parent().expect("stage parent").exists());
    assert!(!orphan_cas.exists());
    assert!(!corruption.exists());
    assert_eq!(fs::read(&live_payload).unwrap(), b"runtime");
    assert!(cas_locks.is_dir(), "repair removed the live CAS lock namespace");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = fs::metadata(&cas_locks).unwrap();
        assert_eq!((metadata.dev(), metadata.ino()), cas_lock_identity);
        let metadata = fs::metadata(&live_payload).unwrap();
        assert_eq!((metadata.dev(), metadata.ino()), live_payload_identity);
    }
    assert_eq!(
        jetpack::Envelope::try_output_hash_of_in_hangar(
            &runtime_output.to_string_lossy(),
            &roots.hangar_dir(),
            false,
        )
        .expect("rehash repaired runtime"),
        runtime_digest
    );
    assert_jetos_stderr_snapshot_normalized(
        "hangar_doctor_findings",
        &String::from_utf8_lossy(&read_only.stderr),
        &[(runtime_digest.as_str(), "<runtime-digest>")],
    );
    assert_jetos_stderr_snapshot_normalized(
        "hangar_doctor_repair",
        &String::from_utf8_lossy(&repaired.stderr),
        &[(runtime_digest.as_str(), "<runtime-digest>")],
    );
}
