//! Card #2158: signed-index Nix provider realization and closure repair.

use std::fs;
use std::path::Path;

use jetpack::Store::{self, ProducerRecord};

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
#[path = "support/nix_index_cache_server.rs"]
mod nix_index_cache_server;

use jetpack_fixtures::{jetpack, Scratch};
use nix_index_cache_server::{NixIndexCacheServer, LIB_PATH, RUNTIME_PATH};

const REVISION: &str = nix_index_cache_server::REVISION;

fn project(scratch: &Scratch) {
    fs::create_dir_all(scratch.join(".jet")).expect("project managed directory");
    fs::write(
        scratch.join(".jet/lock"),
        format!(
            "version = 1\n\n[[source_channel]]\nname = \"nixpkgs\"\nchannel = \"nixpkgs-unstable\"\nexact = \"github:NixOS/nixpkgs#{REVISION}\"\n\n[root]\ndependencies = []\n"
        ),
    )
    .expect("project lock");
}

fn build(project: &Scratch, root: &Scratch, offline: bool) -> std::process::Output {
    let mut command = jetpack();
    command
        .args(["build", "ripgrep@nixpkgs", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "");
    if offline {
        command.arg("--offline");
    }
    command.output().expect("run index-backed Nix build")
}

fn package_entry(root: &Scratch) -> Store::StoreEntry {
    Store::list_checked(&Store::Roots::at(root.path.clone()))
        .expect("list Hangar")
        .into_iter()
        .find(|entry| entry.reference == "ripgrep@nixpkgs" && entry.version == "15.2.0")
        .expect("realized ripgrep package entry")
}

fn make_writable(path: &Path) {
    let metadata = fs::symlink_metadata(path).expect("inspect Hangar object");
    if metadata.is_dir() {
        for child in fs::read_dir(path).expect("read Hangar object") {
            make_writable(&child.expect("Hangar object child").path());
        }
    }
    let mut permissions = metadata.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        permissions.set_mode(permissions.mode() | 0o200);
    }
    #[cfg(not(unix))]
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).expect("make Hangar object writable");
}

#[test]
fn index_backed_nixpkgs_records_complete_closure_and_both_proofs() {
    let project_root = Scratch::new("nix-index-project");
    let hangar_root = Scratch::new("nix-index-hangar");
    let server = NixIndexCacheServer::start_ripgrep(&project_root.path);
    server.install(&hangar_root.path);
    project(&project_root);

    let output = build(&project_root, &hangar_root, false);
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
        .is_some_and(|proof| proof.contains("jet-test-index-v1")));
}

#[test]
fn index_backed_nixpkgs_reuses_closure_offline_after_network_removal() {
    let project_root = Scratch::new("nix-index-offline-project");
    let hangar_root = Scratch::new("nix-index-offline-hangar");
    let server = NixIndexCacheServer::start_ripgrep(&project_root.path);
    server.install(&hangar_root.path);
    project(&project_root);
    assert!(build(&project_root, &hangar_root, false).status.success());

    server.reset_counts();
    server.stop_network();
    let output = build(&project_root, &hangar_root, true);
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
    assert!(build(&project_root, &hangar_root, false).status.success());

    let roots = Store::Roots::at(hangar_root.path.clone());
    let deleted_path = Store::list_checked(&roots)
        .expect("list before deletion")
        .into_iter()
        .find_map(|entry| {
            let producer = ProducerRecord::decode(&entry.producer_record).ok()?;
            (producer.facts.get("nix.store-path").map(String::as_str) == Some(RUNTIME_PATH))
                .then_some(entry.out)
        })
        .expect("runtime Hangar object");
    make_writable(Path::new(&deleted_path));
    fs::remove_dir_all(&deleted_path).expect("delete one transitive Hangar object");

    server.reset_counts();
    server.stop_network();
    let offline = build(&project_root, &hangar_root, true);
    assert_eq!(offline.status.code(), Some(2));
    let offline_stderr = String::from_utf8_lossy(&offline.stderr);
    assert!(offline_stderr.contains("E1350"), "stderr: {offline_stderr}");
    assert!(
        offline_stderr.contains(RUNTIME_PATH),
        "offline error must name the missing reference: {offline_stderr}"
    );
    assert_eq!(server.count("/nix-cache-info"), 0);
    assert_eq!(server.object_request_count(RUNTIME_PATH), 0);

    server.reset_counts();
    server.start_network();
    let repaired = build(&project_root, &hangar_root, false);
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
}
