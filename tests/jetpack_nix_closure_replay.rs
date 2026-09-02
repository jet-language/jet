//! Focused witnesses for locked Nix closure identity and CAS archive policy.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::Scratch;

fn hex_digest(prefix: &str, fill: char) -> String {
    format!("{prefix}{}", fill.to_string().repeat(64))
}

fn cas_digest(fill: char) -> String {
    hex_digest("sha256-", fill)
}

fn valid_nix_closure(output: &str) -> jetpack::Lock::NixClosureRecord {
    jetpack::Lock::NixClosureRecord {
        channel: "nixpkgs-unstable".into(),
        revision: "a".repeat(40),
        system: jetpack::Envelope::host_platform(),
        signed_index_manifest: "b".repeat(64),
        derivation: "c".repeat(64),
        output: output.into(),
        nar_hash: format!("sha256:{}", "d".repeat(64)),
        size: 1,
        compression: "none".into(),
        references: Vec::new(),
        upstream_proof: cas_digest('e'),
        cache_key: "f".repeat(64),
        project_cas_bundle: cas_digest('0'),
    }
}

fn lock_envelope(output: &str, platform: &str) -> jetpack::Lock::LockEnvelope {
    jetpack::Lock::LockEnvelope {
        output_hash: output.into(),
        platform: platform.into(),
        provenance: "integration-test".into(),
        ..Default::default()
    }
}

#[test]
fn malformed_present_lock_is_a_trust_error_not_a_cache_miss() {
    let project = Scratch::new("nix-lock-malformed");
    let lock_path = project.join(".jet/lock");
    fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
    fs::write(&lock_path, "version = [\n").unwrap();

    let error = jetpack::Lock::load_strict(&project.path).unwrap_err();
    assert!(!error.is_empty(), "malformed present lock must be rejected");
    let error =
        jetpack::Lock::nix_realization_strict(&project.path, "pkg@nixpkgs").unwrap_err();
    assert!(!error.is_empty(), "Nix lookup must not downgrade malformed lock state");
}

#[test]
fn nix_lock_rejects_platform_and_envelope_mismatch() {
    let project = Scratch::new("nix-lock-platform");
    let output = cas_digest('1');
    let closure = valid_nix_closure(&output);
    let platform = jetpack::Envelope::host_platform();
    let mismatch = if platform == "aarch64-linux" {
        "x86_64-linux"
    } else {
        "aarch64-linux"
    };

    let error = jetpack::Lock::record_nix_realization(
        &project.path,
        "pkg",
        "1.0.0",
        "pkg@nixpkgs",
        &output,
        closure.clone(),
        lock_envelope(&output, &mismatch),
    )
    .unwrap_err();
    assert!(error.contains("system does not match"), "{error}");
    assert!(
        !project.join(".jet/lock").exists(),
        "rejected identity must not publish a project lock"
    );

    jetpack::Lock::record_nix_realization(
        &project.path,
        "pkg",
        "1.0.0",
        "pkg@nixpkgs",
        &output,
        closure,
        lock_envelope(&output, &platform),
    )
    .unwrap();
    let lock_path = project.join(".jet/lock");
    let mut lock = jetpack::Lock::parse(&fs::read_to_string(&lock_path).unwrap()).unwrap();
    lock.packages[0].envelope.as_mut().unwrap().platform = "aarch64-linux".into();
    fs::write(&lock_path, jetpack::Lock::write(&lock)).unwrap();

    let error =
        jetpack::Lock::nix_realization_strict(&project.path, "pkg@nixpkgs").unwrap_err();
    assert!(error.contains("envelope identity"), "{error}");
}

#[test]
fn nix_lock_receipt_follows_complete_closure_identity() {
    let project = Scratch::new("nix-lock-identity");
    let output = cas_digest('2');
    let mut closure = valid_nix_closure(&output);
    let envelope = lock_envelope(&output, "x86_64-linux");
    let reference = "pkg@nixpkgs";

    jetpack::Lock::record_nix_realization(
        &project.path,
        "pkg",
        "1.0.0",
        reference,
        &output,
        closure.clone(),
        envelope.clone(),
    )
    .unwrap();
    let lock_path = project.join(".jet/lock");
    let mut lock = jetpack::Lock::parse(&fs::read_to_string(&lock_path).unwrap()).unwrap();
    let receipt = cas_digest('9');
    lock.packages[0].receipt = Some(receipt.clone());
    fs::write(&lock_path, jetpack::Lock::write(&lock)).unwrap();

    jetpack::Lock::record_nix_realization(
        &project.path,
        "pkg",
        "1.0.0",
        reference,
        &output,
        closure.clone(),
        envelope.clone(),
    )
    .unwrap();
    let lock = jetpack::Lock::parse(&fs::read_to_string(&lock_path).unwrap()).unwrap();
    assert_eq!(lock.packages[0].receipt.as_deref(), Some(receipt.as_str()));

    closure.revision = "b".repeat(40);
    jetpack::Lock::record_nix_realization(
        &project.path,
        "pkg",
        "1.0.0",
        reference,
        &output,
        closure,
        envelope,
    )
    .unwrap();
    let lock = jetpack::Lock::parse(&fs::read_to_string(&lock_path).unwrap()).unwrap();
    assert_eq!(lock.packages[0].receipt, None);
}

#[cfg(unix)]
fn copy_tree_nofollow(source: &Path, destination: &Path) {
    use std::os::unix::fs::PermissionsExt as _;

    let metadata = fs::symlink_metadata(source).unwrap();
    if metadata.file_type().is_symlink() {
        std::os::unix::fs::symlink(fs::read_link(source).unwrap(), destination).unwrap();
    } else if metadata.is_dir() {
        fs::create_dir(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            copy_tree_nofollow(&entry.path(), &destination.join(entry.file_name()));
        }
        fs::set_permissions(
            destination,
            fs::Permissions::from_mode(metadata.permissions().mode()),
        )
        .unwrap();
    } else {
        fs::copy(source, destination).unwrap();
        fs::set_permissions(
            destination,
            fs::Permissions::from_mode(metadata.permissions().mode()),
        )
        .unwrap();
    }
}

#[cfg(unix)]
fn relative_link_entry(
    root: &Scratch,
    source: &Scratch,
    target: &str,
) -> jetpack::Store::StoreEntry {
    fs::write(source.path.join(target), b"link target payload\n").unwrap();
    std::os::unix::fs::symlink(target, source.join("link")).unwrap();

    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let request = jetpack::Store::IngestRequest {
        name: "link-fixture".into(),
        version: "1.0.0".into(),
        reference: "link-fixture@fixture".into(),
        cache_identity: jetpack::Store::CacheIdentity {
            source_fingerprint: "source-fingerprint".into(),
            recipe_fingerprint: "recipe-fingerprint".into(),
            policy_fingerprint: "policy-fingerprint".into(),
            platform: "x86_64-linux".into(),
        },
        references: Vec::new(),
        outputs: BTreeMap::from([(String::from("out"), source.path.clone())]),
        signature: String::new(),
        provenance: "integration-test".into(),
        platform_artifact_kind: String::new(),
    };
    jetpack::Store::ingest_tree(&roots, &request).unwrap().entry
}

#[cfg(unix)]
fn read_u32(bytes: &[u8], at: &mut usize) -> u32 {
    let end = *at + 4;
    let value = u32::from_le_bytes(bytes[*at..end].try_into().unwrap());
    *at = end;
    value
}

#[cfg(unix)]
fn read_u64(bytes: &[u8], at: &mut usize) -> u64 {
    let end = *at + 8;
    let value = u64::from_le_bytes(bytes[*at..end].try_into().unwrap());
    *at = end;
    value
}

#[cfg(unix)]
fn skip_string(bytes: &[u8], at: &mut usize) {
    let length = read_u32(bytes, at) as usize;
    *at += length;
}

#[cfg(unix)]
fn read_bytes(bytes: &[u8], at: &mut usize) -> Vec<u8> {
    let length = read_u32(bytes, at) as usize;
    let end = *at + length;
    let value = bytes[*at..end].to_vec();
    *at = end;
    value
}

#[cfg(unix)]
#[derive(Clone)]
struct RawArchiveNode {
    kind: u8,
    path: Vec<u8>,
    mode: u32,
    bytes: Vec<u8>,
}

#[cfg(unix)]
#[derive(Clone)]
struct RawArchiveOutput {
    root_mode: u32,
    nodes: Vec<RawArchiveNode>,
}

#[cfg(unix)]
fn read_generic_output(bytes: &[u8], expected: &str) -> RawArchiveOutput {
    let magic = b"jet-hangar-archive-v1\0";
    let mut at = magic.len();
    skip_string(bytes, &mut at);
    let object_count = read_u32(bytes, &mut at) as usize;
    let mut output = None;
    for _ in 0..object_count {
        let _id = read_bytes(bytes, &mut at);
        let digest = read_bytes(bytes, &mut at);
        let meta = read_bytes(bytes, &mut at);
        let root_mode = read_u32(bytes, &mut at);
        let node_count = read_u32(bytes, &mut at) as usize;
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            let kind = bytes[at];
            at += 1;
            let path = read_bytes(bytes, &mut at);
            let mode = read_u32(bytes, &mut at);
            let size = read_u64(bytes, &mut at) as usize;
            let end = at + size;
            let payload = bytes[at..end].to_vec();
            at = end;
            nodes.push(RawArchiveNode {
                kind,
                path,
                mode,
                bytes: payload,
            });
        }
        if meta.is_empty() && digest == expected.as_bytes() {
            output = Some(RawArchiveOutput { root_mode, nodes });
        }
    }
    output.expect("generic archive must contain the requested output")
}

#[cfg(unix)]
fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

#[cfg(unix)]
fn put_raw_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(unix)]
fn encode_nix_bundle(
    output: &str,
    source: &RawArchiveOutput,
    platform: &str,
    revision: &str,
    cache_key: &str,
) -> Vec<u8> {
    let mut canonical = Vec::new();
    canonical.extend_from_slice(&source.root_mode.to_le_bytes());
    for node in &source.nodes {
        canonical.push(node.kind);
        put_bytes(&mut canonical, &node.path);
        canonical.extend_from_slice(&node.mode.to_le_bytes());
        put_raw_u64(&mut canonical, node.bytes.len() as u64);
        canonical.extend_from_slice(&node.bytes);
    }
    let member_hash = format!("sha256-{}", jetpack::SHA256::sha256_hex(&canonical));

    let mut out = Vec::new();
    out.extend_from_slice(b"jet-hangar-archive-v1\0");
    put_bytes(&mut out, output.as_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    put_bytes(&mut out, output.as_bytes());
    put_bytes(&mut out, output.as_bytes());
    put_bytes(&mut out, &[]);
    out.extend_from_slice(&source.root_mode.to_le_bytes());
    out.extend_from_slice(&(source.nodes.len() as u32).to_le_bytes());
    for node in &source.nodes {
        out.push(node.kind);
        put_bytes(&mut out, &node.path);
        out.extend_from_slice(&node.mode.to_le_bytes());
        put_raw_u64(&mut out, node.bytes.len() as u64);
        out.extend_from_slice(&node.bytes);
    }
    out.push(2);
    put_bytes(&mut out, output.as_bytes());
    put_bytes(&mut out, platform.as_bytes());
    put_bytes(&mut out, revision.as_bytes());
    put_bytes(&mut out, cache_key.as_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    put_bytes(&mut out, output.as_bytes());
    put_bytes(&mut out, member_hash.as_bytes());
    put_raw_u64(&mut out, canonical.len() as u64);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(0);
    out
}

#[cfg(unix)]
fn replace_link_target(bytes: &[u8], old: &[u8], replacement: &[u8]) -> Vec<u8> {
    assert!(!old.is_empty());
    assert_eq!(old.len(), replacement.len());
    let magic = b"jet-hangar-archive-v1\0";
    assert!(bytes.starts_with(magic));
    let mut at = magic.len();
    skip_string(bytes, &mut at);
    let object_count = read_u32(bytes, &mut at) as usize;
    assert_eq!(object_count, 1, "fixture must contain one output object");
    let mut object_digest = None;
    let mut object = None;
    for _ in 0..object_count {
        let _id = read_bytes(bytes, &mut at);
        let digest = read_bytes(bytes, &mut at);
        let meta = read_bytes(bytes, &mut at);
        let root_mode = read_u32(bytes, &mut at);
        let node_count = read_u32(bytes, &mut at) as usize;
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            let kind = bytes[at];
            at += 1;
            let path = read_bytes(bytes, &mut at);
            let mode = read_u32(bytes, &mut at);
            let size = read_u64(bytes, &mut at) as usize;
            let end = at + size;
            nodes.push(RawArchiveNode {
                kind,
                path,
                mode,
                bytes: bytes[at..end].to_vec(),
            });
            at = end;
        }
        if meta.is_empty() {
            object_digest = Some(digest);
            object = Some(RawArchiveOutput { root_mode, nodes });
        }
    }

    assert_eq!(bytes[at], 2, "fixture must carry a Nix manifest");
    at += 1;
    let manifest_output = read_bytes(bytes, &mut at);
    let platform = read_bytes(bytes, &mut at);
    let revision = read_bytes(bytes, &mut at);
    let cache_key = read_bytes(bytes, &mut at);
    let member_count = read_u32(bytes, &mut at) as usize;
    for _ in 0..member_count {
        skip_string(bytes, &mut at);
        skip_string(bytes, &mut at);
        read_u64(bytes, &mut at);
        let reference_count = read_u32(bytes, &mut at) as usize;
        for _ in 0..reference_count {
            skip_string(bytes, &mut at);
        }
    }
    let object_digest = object_digest.expect("fixture must carry an output object");
    assert_eq!(object_digest, manifest_output);
    let mut object = object.expect("fixture must carry an output object");
    let mut matches = 0;
    for node in &mut object.nodes {
        if node.kind == 2 && node.bytes == old {
            node.bytes = replacement.to_vec();
            matches += 1;
        }
    }
    assert_eq!(matches, 1, "archive must contain one symlink target");
    encode_nix_bundle(
        &String::from_utf8(manifest_output).unwrap(),
        &object,
        &String::from_utf8(platform).unwrap(),
        &String::from_utf8(revision).unwrap(),
        &String::from_utf8(cache_key).unwrap(),
    )
}

#[cfg(unix)]
fn replace_all(bytes: &[u8], old: &[u8], replacement: &[u8]) -> (Vec<u8>, usize) {
    assert!(!old.is_empty());
    assert_eq!(old.len(), replacement.len());
    let mut result = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    let mut count = 0;
    while let Some(relative) = bytes[cursor..]
        .windows(old.len())
        .position(|window| window == old)
    {
        let offset = cursor + relative;
        result.extend_from_slice(&bytes[cursor..offset]);
        result.extend_from_slice(replacement);
        cursor = offset + old.len();
        count += 1;
    }
    result.extend_from_slice(&bytes[cursor..]);
    (result, count)
}

#[cfg(unix)]
fn cas_digest_from_bytes(bytes: &[u8]) -> String {
    format!("sha256-{}", jetpack::SHA256::sha256_hex(bytes))
}

#[cfg(unix)]
#[test]
fn nix_cas_build_and_import_preserve_absolute_links_but_generic_paths_reject_them() {
    let source_root = Scratch::new("nix-cas-source-root");
    let source = Scratch::new("nix-cas-source");
    let absolute_target = "/nix/store/jet-closure-target";
    let relative_target = "x".repeat(absolute_target.len());
    let entry = relative_link_entry(&source_root, &source, &relative_target);
    let roots = jetpack::Store::Roots {
        root: source_root.path.clone(),
        dev_mode: false,
    };
    let generic_archive =
        jetpack::Store::export_unsigned_archive(&roots, &entry.reference, true).unwrap();
    let generic_output = read_generic_output(&generic_archive, &entry.envelope.output_hash);
    let platform = jetpack::Envelope::host_platform();
    let revision = "a".repeat(40);
    let cache_key = "f".repeat(64);
    let bundle = encode_nix_bundle(
        &entry.envelope.output_hash,
        &generic_output,
        &platform,
        &revision,
        &cache_key,
    );
    let absolute_source = Scratch::new("nix-cas-absolute-digest");
    let absolute_tree = absolute_source.join("tree");
    copy_tree_nofollow(&source.path, &absolute_tree);
    fs::remove_file(absolute_tree.join("link")).unwrap();
    std::os::unix::fs::symlink(absolute_target, absolute_tree.join("link")).unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(
        &absolute_tree,
        fs::Permissions::from_mode(generic_output.root_mode),
    )
    .unwrap();
    for node in &generic_output.nodes {
        if node.kind != 2 {
            let path = absolute_tree.join(String::from_utf8(node.path.clone()).unwrap());
            fs::set_permissions(path, fs::Permissions::from_mode(node.mode)).unwrap();
        }
    }
    let new_output = jetpack::Envelope::try_output_hash_of_in_hangar(
        &absolute_tree.to_string_lossy(),
        &roots.hangar_dir(),
        false,
    )
    .unwrap();
    assert_ne!(new_output, entry.envelope.output_hash);

    let (mut absolute_bundle, replaced_digests) = replace_all(
        &bundle,
        entry.envelope.output_hash.as_bytes(),
        new_output.as_bytes(),
    );
    assert!(replaced_digests >= 2, "root and output digest must be rewritten");
    absolute_bundle = replace_link_target(
        &absolute_bundle,
        relative_target.as_bytes(),
        absolute_target.as_bytes(),
    );

    let project = Scratch::new("nix-cas-project");
    let bundle_dir = project.join(".jet/nix-cas");
    fs::create_dir_all(&bundle_dir).unwrap();
    let bundle_digest = cas_digest_from_bytes(&absolute_bundle);
    fs::write(bundle_dir.join(&bundle_digest), &absolute_bundle).unwrap();

    let destination = Scratch::new("nix-cas-destination");
    let destination_roots = jetpack::Store::Roots {
        root: destination.path.clone(),
        dev_mode: false,
    };
    let (report, digests) = jetpack::Store::import_nix_cas_bundle(
        &project.path,
        &destination_roots,
        &bundle_digest,
    )
    .unwrap();
    assert_eq!(report.objects, 1);
    assert_eq!(digests, vec![new_output.clone()]);
    let imported_link = destination_roots
        .hangar_dir()
        .join("objects")
        .join(&new_output)
        .join("link");
    assert_eq!(
        fs::read_link(imported_link).unwrap(),
        PathBuf::from(absolute_target)
    );

    let mut closure = valid_nix_closure(&new_output);
    closure.project_cas_bundle = bundle_digest.clone();
    let replay_envelope = lock_envelope(&new_output, &platform);
    jetpack::Lock::record_nix_realization(
        &project.path,
        "link-fixture",
        "1.0.0",
        "link-fixture@jetpack",
        &new_output,
        closure,
        replay_envelope,
    )
    .unwrap();
    let spec = jetpack::RefSpec::classify("link-fixture@jetpack").unwrap();
    let table = jetpack::RefSpec::SourceTable::empty();
    let replayed = jetpack::Store::realize_verified(
        &destination_roots,
        &jetpack::Provider::Ctx {
            fixtures: None,
            store_dir: &destination.path,
            offline: true,
            project_dir: Some(&project.path),
            nix_index: None,
            nix_roots: Some(&destination_roots),
        },
        jetpack::Store::RealizeRequest::Package {
            spec: &spec,
            table: &table,
        },
    )
    .unwrap();
    let nix_entry = replayed.metadata().clone();

    let (exported, _) =
        jetpack::Store::export_nix_cas_bundle(&destination_roots, &nix_entry.reference).unwrap();
    assert!(
        exported
            .windows(absolute_target.len())
            .any(|window| window == absolute_target.as_bytes()),
        "Nix CAS export must retain the absolute target as link data"
    );
    let generic_export =
        jetpack::Store::export_unsigned_archive(&destination_roots, &nix_entry.reference, false)
            .unwrap_err();
    assert!(
        generic_export
            .to_string()
            .contains("absolute symlinks are not portable Hangar archive nodes"),
        "{generic_export}"
    );

    let generic_absolute_root = Scratch::new("nix-cas-generic-absolute");
    let generic_absolute_roots = jetpack::Store::Roots {
        root: generic_absolute_root.path.clone(),
        dev_mode: false,
    };
    let error = jetpack::Store::import_archive(
        &generic_absolute_roots,
        &absolute_bundle,
        None,
        true,
    )
    .unwrap_err();
    assert!(error.to_string().contains("archive link target is absolute"), "{error}");

    let escape_target = format!("../{}", "x".repeat(relative_target.len() - 3));
    let control_target = {
        let mut target = vec![b'x'; relative_target.len()];
        target[relative_target.len() / 2] = 1;
        target
    };
    let backslash_target = {
        let mut target = vec![b'x'; relative_target.len()];
        target[relative_target.len() / 2] = b'\\';
        target
    };
    for (tag, target, message) in [
        (
            "escape",
            escape_target.into_bytes(),
            "archive link target escapes its output root",
        ),
        (
            "control",
            control_target,
            "archive link target is not portable",
        ),
        (
            "backslash",
            backslash_target,
            "archive link target is not portable",
        ),
    ] {
        let hostile_bundle = replace_link_target(&bundle, relative_target.as_bytes(), &target);
        let hostile_root = Scratch::new(&format!("nix-cas-generic-{tag}"));
        let hostile_roots = jetpack::Store::Roots {
            root: hostile_root.path.clone(),
            dev_mode: false,
        };
        let error = jetpack::Store::import_archive(&hostile_roots, &hostile_bundle, None, true)
            .unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
}
