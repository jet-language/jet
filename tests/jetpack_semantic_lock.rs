//! E4-JP13 — one live semantic lock path: catalogs, overlays, source maps,
//! input graph, selective update, merge revalidation, atomic `.jet/lock` write.

use jetpack::Overlay::{self, OverlayPolicy, OverlaySet, PackageOverride};
use jetpack::SemanticLock::{
    self, apply_overlay_invalidations, atomic_commit, load, merge, merge_revalidate_commit,
    overlay_invalidations, record_catalog_selection, revalidate, selective_update,
    strip_semantic_sections, LockIdentity, LockInput, LockRationale, LockRecordKind,
    SemanticLockFile, SemanticRecord, SourceMapEntry, ValidationIssue,
};
use std::collections::BTreeMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_root(tag: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("jet-jp13-{tag}-{nanos}"));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn pkg(owner: &str, key: &str, exact: &str, hash: &str) -> SemanticRecord {
    SemanticRecord::new(
        LockIdentity {
            kind: LockRecordKind::Package,
            key: key.to_string(),
            exact: exact.to_string(),
            hash: hash.to_string(),
            platform: "x86_64-linux".to_string(),
        },
        LockRationale {
            owner_package: owner.to_string(),
            reason: format!("{owner} declared {key}"),
            source_ref: format!("catalog:{key}"),
            provider: "core".to_string(),
            channel_input: "stable".to_string(),
            exact_output: exact.to_string(),
            policy_fingerprint: "policy-1".to_string(),
            recipe_id: String::new(),
            adapter_id: String::new(),
            signature: "ed25519:sig".to_string(),
            cache_provenance: "hangar".to_string(),
            update_command: format!("jet update {key}"),
        },
    )
}

#[test]
fn live_lock_path_atomic_commit_roundtrip_preserves_machine_sections() {
    let root = unique_root("live");
    let lock_path = jetpack::Store::lock_path(&root);
    fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
    fs::write(
        &lock_path,
        r#"version = 1

[[package]]
name = "app"
version = "0.1.0"
source = { root = "." }
fingerprint = "fp-app"
dependencies = []

[root]
dependencies = ["app"]
"#,
    )
    .unwrap();

    let mut semantic = SemanticLockFile::with_records(vec![pkg(
        "app",
        "core.log",
        "1.2.3",
        "sha256-log",
    )]);
    semantic.inputs = vec![
        LockInput {
            name: "nixpkgs".into(),
            url: "github:NixOS/nixpkgs/abc".into(),
            follows: String::new(),
        },
        LockInput {
            name: "home".into(),
            url: "github:nix-community/home-manager".into(),
            follows: "nixpkgs".into(),
        },
    ];
    semantic.source_maps = vec![SourceMapEntry {
        pattern: "core.*".into(),
        sources: vec!["catalog:core.log".into(), "core".into()],
    }];

    atomic_commit(&root, &semantic).expect("atomic commit");
    let raw = fs::read_to_string(&lock_path).unwrap();
    assert!(raw.contains("[[package]]"));
    assert!(raw.contains("name = \"app\""));
    assert!(raw.contains("[[semantic_record]]"));
    assert!(raw.contains("[[lock_input]]"));
    assert!(raw.contains("[[source_map]]"));
    assert!(raw.contains("follows = \"nixpkgs\""));

    // Machine Lock::parse still works on the unified file.
    let machine = jetpack::Lock::parse(&raw).expect("machine lock parse");
    assert_eq!(machine.packages.len(), 1);
    assert_eq!(machine.packages[0].name, "app");

    let loaded = load(&root).expect("semantic load");
    assert_eq!(loaded.records.len(), 1);
    assert_eq!(loaded.inputs.len(), 2);
    assert_eq!(loaded.source_maps.len(), 1);
    assert_eq!(loaded.records[0].identity.exact, "1.2.3");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn catalog_selection_records_owner_and_source_ref() {
    let mut lock = SemanticLockFile::default();
    record_catalog_selection(
        &mut lock,
        "app",
        "log",
        "core.log@1.2.3",
        "sha256-catalog",
        "x86_64-linux",
        "workspace selected shared logging",
    );
    assert_eq!(lock.records.len(), 1);
    assert_eq!(lock.records[0].rationales[0].source_ref, "catalog:log");
    assert_eq!(lock.records[0].rationales[0].adapter_id, "workspace.catalog");
    assert!(revalidate(&lock).is_ok());
}

#[test]
fn selective_update_keeps_unrelated_record_bytes_stable() {
    let mut lock = SemanticLockFile::with_records(vec![
        pkg("app", "core.log", "1.2.3", "sha256-log"),
        pkg("app", "core.http", "2.0.0", "sha256-http"),
    ]);
    let before = SemanticLock::write(&lock);
    let http_block = before
        .split("[[semantic_record]]")
        .find(|b| b.contains("key = \"core.http\""))
        .expect("http record")
        .to_string();

    selective_update(
        &mut lock,
        pkg("app", "core.log", "1.3.0", "sha256-log-new"),
    );
    let after = SemanticLock::write(&lock);
    let http_after = after
        .split("[[semantic_record]]")
        .find(|b| b.contains("key = \"core.http\""))
        .expect("http record after")
        .to_string();
    assert_eq!(http_block, http_after);
    assert!(after.contains("exact = \"1.3.0\""));
}

#[test]
fn input_follows_cycle_and_missing_target_fail_revalidate() {
    let mut lock = SemanticLockFile::with_records(vec![pkg(
        "app",
        "core.log",
        "1.2.3",
        "sha256-log",
    )]);
    lock.inputs = vec![
        LockInput {
            name: "a".into(),
            url: "u-a".into(),
            follows: "b".into(),
        },
        LockInput {
            name: "b".into(),
            url: "u-b".into(),
            follows: "a".into(),
        },
    ];
    let err = revalidate(&lock).expect_err("cycle");
    assert!(err.iter().any(|i| matches!(i, ValidationIssue::InputCycle(_))));

    lock.inputs = vec![LockInput {
        name: "home".into(),
        url: "u".into(),
        follows: "missing".into(),
    }];
    let err = revalidate(&lock).expect_err("missing follows");
    assert!(err
        .iter()
        .any(|i| matches!(i, ValidationIssue::MissingFollows { .. })));
}

#[test]
fn source_map_rejects_wrong_authority() {
    let mut lock = SemanticLockFile::with_records(vec![pkg(
        "app",
        "Company.Lib",
        "1.0.0",
        "sha256-lib",
    )]);
    lock.records[0].rationales[0].source_ref = "nuget:PublicGallery".into();
    lock.source_maps = vec![SourceMapEntry {
        pattern: "Company.*".into(),
        sources: vec!["nuget:CompanyFeed".into()],
    }];
    let err = revalidate(&lock).expect_err("source authority");
    assert!(err
        .iter()
        .any(|i| matches!(i, ValidationIssue::SourceAuthority { .. })));
}

#[test]
fn overlay_change_invalidates_exact_action_keys_and_explains_why() {
    let before = OverlayPolicy {
        overlays: vec![OverlaySet {
            name: "beta".into(),
            provider: None,
            packages: vec![PackageOverride {
                package: "foo".into(),
                source: None,
                version: Some("1.0.0".into()),
                flags: Vec::new(),
                priority: 0,
                env: Vec::new(),
                patches: vec!["patches/a.patch".into()],
                allow_unfree: false,
            }],
        }],
        ..Default::default()
    };
    let after = OverlayPolicy {
        overlays: vec![OverlaySet {
            name: "beta".into(),
            provider: None,
            packages: vec![PackageOverride {
                package: "foo".into(),
                source: None,
                version: Some("1.0.0".into()),
                flags: Vec::new(),
                priority: 0,
                env: Vec::new(),
                patches: vec!["patches/a.patch".into(), "patches/b.patch".into()],
                allow_unfree: false,
            }],
        }],
        ..Default::default()
    };
    let mut actions = BTreeMap::new();
    actions.insert(
        "foo".into(),
        vec!["action:foo:build".into(), "action:foo:check".into()],
    );
    let inv = Overlay::invalidations_against(&before, &after, &actions);
    assert_eq!(inv.len(), 1);
    assert_eq!(
        inv[0].affected_action_keys,
        vec!["action:foo:build".to_string(), "action:foo:check".to_string()]
    );
    assert!(inv[0].reason.contains("changed package `foo`"));
    assert_ne!(
        inv[0].policy_fingerprint_before,
        inv[0].policy_fingerprint_after
    );

    let mut lock = SemanticLockFile::with_records(vec![
        pkg("app", "foo", "1.0.0", "sha256-foo"),
        Overlay::semantic_records(&before, "app", "x86_64-linux")
            .into_iter()
            .next()
            .unwrap(),
    ]);
    apply_overlay_invalidations(&mut lock, &inv);
    assert!(!lock
        .records
        .iter()
        .any(|r| r.identity.kind == LockRecordKind::PackageOverlay));
    let foo = lock
        .records
        .iter()
        .find(|r| r.identity.key == "foo")
        .unwrap();
    assert!(foo.identity.hash.is_empty(), "hash cleared for rebuild");
}

#[test]
fn merge_revalidate_commit_rejects_conflicts_before_write() {
    let root = unique_root("merge-conflict");
    let left = SemanticLockFile::with_records(vec![pkg(
        "app",
        "core.log",
        "1.2.3",
        "sha256-a",
    )]);
    let right = SemanticLockFile::with_records(vec![pkg(
        "app",
        "core.log",
        "1.3.0",
        "sha256-b",
    )]);
    let err = merge_revalidate_commit(&root, &SemanticLockFile::default(), &left, &right)
        .expect_err("conflict");
    assert!(!err.issues.is_empty());
    assert!(!SemanticLock::live_path(&root).exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn merge_revalidate_commit_writes_when_clean() {
    let root = unique_root("merge-ok");
    let left = SemanticLockFile::with_records(vec![pkg(
        "app",
        "core.log",
        "1.2.3",
        "sha256-log",
    )]);
    let right = SemanticLockFile::with_records(vec![pkg(
        "app",
        "core.http",
        "2.0.0",
        "sha256-http",
    )]);
    let merged =
        merge_revalidate_commit(&root, &SemanticLockFile::default(), &left, &right).unwrap();
    assert_eq!(merged.records.len(), 2);
    let loaded = load(&root).unwrap();
    assert_eq!(loaded.records.len(), 2);

    // strip helper leaves only machine bytes
    let raw = fs::read_to_string(SemanticLock::live_path(&root)).unwrap();
    let machine = strip_semantic_sections(&raw);
    assert!(!machine.contains("semantic_record"));
    assert!(machine.contains("version = 1"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn overlay_invalidations_helper_matches_fingerprint_diff() {
    let before = BTreeMap::from([(("beta".into(), "foo".into()), "fp-1".into())]);
    let after = BTreeMap::from([(("beta".into(), "foo".into()), "fp-2".into())]);
    let actions = BTreeMap::from([("foo".into(), vec!["a:foo".into()])]);
    let inv = overlay_invalidations(&before, &after, &actions);
    assert_eq!(inv[0].affected_action_keys, vec!["a:foo".to_string()]);
}

#[test]
fn three_way_merge_preserves_inputs_and_source_maps() {
    let mut base = SemanticLockFile::default();
    base.inputs = vec![LockInput {
        name: "nixpkgs".into(),
        url: "old".into(),
        follows: String::new(),
    }];
    let mut left = base.clone();
    left.source_maps = vec![SourceMapEntry {
        pattern: "Acme.*".into(),
        sources: vec!["nuget:Acme".into()],
    }];
    let mut right = base.clone();
    right.inputs = vec![LockInput {
        name: "nixpkgs".into(),
        url: "new".into(),
        follows: String::new(),
    }];
    let out = merge(&base, &left, &right);
    assert!(out.conflicts.is_empty());
    assert_eq!(out.merged.inputs[0].url, "new");
    assert_eq!(out.merged.source_maps[0].pattern, "Acme.*");
}
