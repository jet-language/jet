use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use jet::ReceiptStore::{participating_verb, ReceiptStore, PARTICIPATING_VERBS};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn participating_verbs_are_explicit_and_bounded() {
    assert_eq!(PARTICIPATING_VERBS, &["test", "prove", "budget check"]);
    for verb in PARTICIPATING_VERBS {
        let argv = verb
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(participating_verb(&argv), Some(*verb));
    }
    assert_eq!(
        participating_verb(&["budget".into(), "update".into()]),
        None
    );
    assert_eq!(participating_verb(&["run".into()]), None);
    // D-BUILD-NOCHANGE1=A: `check` and `build` answer from their typed
    // Receipt node.
    assert_eq!(participating_verb(&["check".into()]), None);
    assert_eq!(participating_verb(&["build".into()]), None);
}

/// `(subject, why_ran)` for every Check node in the newest store log entry.
fn check_nodes(workspace: &Path, store: &Path, program: &str) -> Vec<(String, String)> {
    jet_store::Store::new(store)
        .unwrap()
        .last_build_record(workspace, program)
        .unwrap()
        .map(|record| {
            record
                .nodes
                .into_iter()
                .filter(|node| node.kind == "check")
                .map(|node| (node.subject, node.why_ran))
                .collect()
        })
        .unwrap_or_default()
}

fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "jet-receipts-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn content_keys_reuse_without_timestamps_and_invalidate_only_dependents() {
    let root = temp_root("identity");
    let source = root.join("source.jet");
    let unrelated = root.join("unrelated.jet");
    std::fs::write(&source, "fn run() {}\n").unwrap();
    std::fs::write(&unrelated, "fn run() {}\n").unwrap();

    let store = ReceiptStore::new(root.join("store"));
    let args = vec!["check".to_string(), source.display().to_string()];
    let source_claim = store
        .claim("check", &args, std::slice::from_ref(&source))
        .unwrap();
    let unrelated_claim = store
        .claim("check", &args, std::slice::from_ref(&unrelated))
        .unwrap();
    store
        .write(&source_claim, &args, 0, b"source", b"")
        .unwrap();
    store
        .write(&unrelated_claim, &args, 0, b"unrelated", b"")
        .unwrap();

    // Rewriting identical bytes changes ordinary filesystem metadata, but not
    // the receipt identity.
    std::fs::write(&source, "fn run() {}\n").unwrap();
    assert!(store.lookup(&source_claim).unwrap().is_some());
    assert!(store.lookup(&unrelated_claim).unwrap().is_some());

    std::fs::write(&source, "fn run() { print(1) }\n").unwrap();
    let changed_source = store
        .claim("check", &args, std::slice::from_ref(&source))
        .unwrap();
    assert_ne!(changed_source.key, source_claim.key);
    assert!(store.lookup(&source_claim).unwrap().is_none());
    assert!(store.lookup(&unrelated_claim).unwrap().is_some());

    let _ = std::fs::remove_dir_all(root);
}

/// D-BUILD-NOCHANGE1=A: an unchanged `jet check` answers from its Receipt,
/// prints exactly the fresh output, and the store log shows no Check ran.
#[test]
fn check_reuses_receipt_at_the_cli_boundary() {
    let root = temp_root("cli");
    let source = root.join("main.jet");
    let store = root.join("store");
    std::fs::write(&source, "fn run() {}\n").unwrap();
    let jet = env!("CARGO_BIN_EXE_jet");
    let check = || {
        Command::new(jet)
            .current_dir(&root)
            .args(["check", source.to_str().unwrap()])
            .env("JET_STORE_DIR", &store)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };
    let program = "main.jet";

    let first = check();
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    let fresh = check_nodes(&root, &store, &program);
    assert!(!fresh.is_empty(), "a fresh check must log its Check nodes");
    assert!(fresh.iter().all(|(_, why)| why != "reused"), "{fresh:?}");

    let second = check();
    assert!(second.status.success(), "{}", String::from_utf8_lossy(&second.stderr));
    assert_eq!(second.stdout, first.stdout);
    assert_eq!(second.stderr, first.stderr);
    let replayed = check_nodes(&root, &store, &program);
    assert_eq!(replayed.len(), fresh.len());
    assert!(replayed.iter().all(|(_, why)| why == "reused"), "{replayed:?}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn project_check_does_not_replay_after_higher_priority_entry_appears() {
    let root = temp_root("stale-entry");
    let source_dir = root.join("src");
    let store = root.join("store");
    std::fs::create_dir_all(&source_dir).unwrap();
    std::fs::write(
        root.join("package.jet"),
        "name: \"stale-entry\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(source_dir.join("run.jet"), "fn run() {}\n").unwrap();
    let jet = env!("CARGO_BIN_EXE_jet");
    let check = || {
        Command::new(jet)
            .args(["check", "--verbose"])
            .current_dir(&root)
            .env("JET_STORE_DIR", &store)
            .output()
            .unwrap()
    };

    let first = check();
    assert!(
        first.status.success(),
        "initial project check failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );

    std::fs::write(root.join("run.jet"), "fn run() {}\n").unwrap();
    let second = check();
    assert!(
        second.status.success(),
        "project check after entry creation failed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let nodes = check_nodes(&root, &store, "run.jet");
    assert!(
        !nodes.is_empty() && nodes.iter().all(|(_, why)| why != "reused"),
        "new higher-priority entry must check fresh, not replay the old receipt: {nodes:?}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn explicit_check_receipt_tracks_package_workspace_and_generated_authorities() {
    let root = temp_root("project-authorities");
    let package = root.join("packages/app");
    let source = package.join("src/main.jet");
    let package_manifest = package.join("package.jet");
    let workspace = root.join("workspace.jet");
    let workspace_lock = root.join(".jet/lock");
    let package_lock = package.join(".jet/lock");
    let generated = package.join(".jet/generated/inputs.jet");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::create_dir_all(workspace_lock.parent().unwrap()).unwrap();
    std::fs::create_dir_all(package_lock.parent().unwrap()).unwrap();
    std::fs::create_dir_all(generated.parent().unwrap()).unwrap();
    std::fs::write(
        &workspace,
        "module workspace { members: [\"packages/app\"] }\n",
    )
    .unwrap();
    std::fs::write(
        &package_manifest,
        "name: \"app\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(&source, "fn run() {}\n").unwrap();
    std::fs::write(&workspace_lock, "workspace-lock-v1\n").unwrap();
    std::fs::write(&package_lock, "package-lock-v1\n").unwrap();
    std::fs::write(&generated, "generated-v1\n").unwrap();

    let argv = vec!["check".to_string(), "packages/app/src/main.jet".to_string()];
    let inputs = jet::ReceiptStore::input_paths_for("check", &argv, &root);
    for expected in [
        &workspace,
        &package_manifest,
        &workspace_lock,
        &package_lock,
        &generated,
    ] {
        assert!(
            inputs.iter().any(|path| path == expected),
            "explicit check omitted authority input {}: {inputs:?}",
            expected.display()
        );
    }

    let store = ReceiptStore::new(root.join("receipts"));
    let claim = store.claim("check", &argv, &inputs).unwrap();
    store.write(&claim, &argv, 0, b"ok", b"").unwrap();
    assert!(store.lookup(&claim).unwrap().is_some());

    let cases = [
        (
            &workspace,
            "module workspace { members: [] }\n",
            "module workspace { members: [\"packages/app\"] }\n",
        ),
        (
            &package_manifest,
            "name: \"changed\"\n",
            "name: \"app\"\nversion: \"0.1.0\"\n",
        ),
        (&workspace_lock, "workspace-lock-v2\n", "workspace-lock-v1\n"),
        (&package_lock, "package-lock-v2\n", "package-lock-v1\n"),
        (&generated, "generated-v2\n", "generated-v1\n"),
    ];
    for (path, replacement, original) in cases {
        std::fs::write(path, replacement).unwrap();
        assert!(
            store.lookup(&claim).unwrap().is_none(),
            "changed authority input {} replayed an old receipt",
            path.display()
        );
        std::fs::write(path, original).unwrap();
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn result_payloads_share_one_receipt_codec_and_store() {
    let root = temp_root("kinds");
    let source = root.join("main.jet");
    std::fs::write(&source, "fn run() {}\n").unwrap();
    let store = ReceiptStore::new(root.join("store"));
    let args = vec!["act".into()];
    let payloads = [
        ("test", b"test".as_slice()),
        ("golden", b"golden".as_slice()),
        ("budget", b"budget".as_slice()),
        ("api", b"api".as_slice()),
    ];

    for (kind, payload) in payloads {
        let receipt = store
            .record(kind, &args, std::slice::from_ref(&source), 0, payload, b"")
            .unwrap();
        let bytes = std::fs::read(store.object_path(&receipt.claim.key)).unwrap();
        assert!(bytes.starts_with(b"jet-receipt-v2\0"));
    }

    assert_eq!(store.list().unwrap().len(), payloads.len());
    assert_eq!(store.list_current().unwrap().len(), payloads.len());
    std::fs::write(&source, "fn run() { print(1) }\n").unwrap();
    assert!(store.list_current().unwrap().is_empty());
    assert_eq!(store.list().unwrap().len(), payloads.len());

    let _ = std::fs::remove_dir_all(root);
}
#[cfg(unix)]
#[test]
fn canonical_history_keeps_legacy_namespaces_and_separates_checkout_identities() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let write_project = |root: &std::path::Path| {
        std::fs::write(
            root.join("package.jet"),
            "name: \"receipt-boundary\"\nversion: \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(root.join("main.jet"), "fn run() {}\n").unwrap();
    };
    let run_check = |root: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(["check", "main.jet"])
            .current_dir(root)
            .output()
            .unwrap()
    };
    let root = temp_root("canonical");
    write_project(&root);
    std::fs::create_dir_all(root.join("build")).unwrap();
    std::fs::write(root.join("build/legacy-output"), b"legacy-build").unwrap();
    std::fs::create_dir_all(root.join(".jet-build")).unwrap();
    std::fs::write(root.join(".jet-build/legacy-output"), b"legacy-jet-build").unwrap();
    std::fs::create_dir_all(root.join("src/.jet")).unwrap();
    std::fs::write(root.join("src/.jet/legacy-output"), b"legacy-input").unwrap();
    let mut source_permissions = std::fs::metadata(root.join("main.jet"))
        .unwrap()
        .permissions();
    source_permissions.set_mode(0o444);
    std::fs::set_permissions(root.join("main.jet"), source_permissions).unwrap();

    let first = run_check(&root);
    assert!(
        first.status.success(),
        "canonical check failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let receipts = root.join(".jet/receipts");
    assert!(receipts.is_dir(), "history must use the selected `.jet` root");
    assert_eq!(
        std::fs::read(root.join("build/legacy-output")).unwrap(),
        b"legacy-build"
    );
    assert_eq!(
        std::fs::read(root.join(".jet-build/legacy-output")).unwrap(),
        b"legacy-jet-build"
    );
    assert_eq!(
        std::fs::read(root.join("src/.jet/legacy-output")).unwrap(),
        b"legacy-input"
    );
    assert!(
        !root.join("build/objects").exists()
            && !root.join(".jet-build/objects").exists()
            && !root.join("src/.jet/objects").exists(),
        "legacy namespaces must not receive duplicate history"
    );

    let moved = temp_root("moved");
    write_project(&moved);
    let second = run_check(&moved);
    assert!(
        second.status.success(),
        "moved checkout check failed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(moved.join(".jet/receipts").is_dir());
    assert_ne!(
        std::fs::canonicalize(&receipts).unwrap(),
        std::fs::canonicalize(moved.join(".jet/receipts")).unwrap(),
        "moved checkout history must not escape into the old project"
    );

    let store = ReceiptStore::new(&receipts);
    let source = root.join("main.jet");
    let debug = vec![
        "check".to_string(),
        source.display().to_string(),
        "--profile=debug".to_string(),
    ];
    let release = vec![
        "check".to_string(),
        source.display().to_string(),
        "--profile=release".to_string(),
    ];
    let debug_claim = store
        .claim("check", &debug, std::slice::from_ref(&source))
        .unwrap();
    let release_claim = store
        .claim("check", &release, std::slice::from_ref(&source))
        .unwrap();
    assert_ne!(
        debug_claim.key, release_claim.key,
        "profile selection must remain part of receipt identity"
    );
    let moved_source = moved.join("main.jet");
    let moved_claim = ReceiptStore::new(moved.join(".jet/receipts"))
        .claim(
            "check",
            &debug,
            std::slice::from_ref(&moved_source),
        )
        .unwrap();
    assert_ne!(
        debug_claim.key, moved_claim.key,
        "project roots must remain separate after a checkout move"
    );

    let alias = temp_root("symlink-alias");
    std::fs::remove_dir_all(&alias).unwrap();
    symlink(&root, &alias).unwrap();
    let alias_run = run_check(&alias);
    assert!(
        alias_run.status.success(),
        "symlink checkout check failed:\n{}",
        String::from_utf8_lossy(&alias_run.stderr)
    );
    assert_eq!(
        std::fs::canonicalize(alias.join(".jet/receipts")).unwrap(),
        std::fs::canonicalize(&receipts).unwrap(),
        "a symlinked checkout must resolve to its canonical project root"
    );
    std::fs::remove_file(&alias).unwrap();

    let outside = temp_root("escape-outside");
    let escaped = temp_root("escape-project");
    write_project(&escaped);
    std::fs::create_dir_all(escaped.join(".jet")).unwrap();
    symlink(&outside, escaped.join(".jet/receipts")).unwrap();
    let escaped_run = run_check(&escaped);
    assert!(
        escaped_run.status.success(),
        "an unsafe history destination must not fail the required check:\n{}",
        String::from_utf8_lossy(&escaped_run.stderr)
    );
    assert!(
        !outside.join("objects").exists()
            && !outside.join("contexts").exists()
            && !outside.join("records").exists(),
        "a symlinked `.jet/receipts` must never receive history"
    );

    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(moved);
    let _ = std::fs::remove_dir_all(outside);
    let _ = std::fs::remove_dir_all(escaped);
}
