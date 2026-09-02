use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use jet::ReceiptStore::{participating_verb, ReceiptStore, PARTICIPATING_VERBS};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn participating_verbs_are_explicit_and_bounded() {
    assert_eq!(
        PARTICIPATING_VERBS,
        &["check", "build", "test", "prove", "budget check"]
    );
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

#[test]
fn check_reuses_receipt_at_the_cli_boundary() {
    let root = temp_root("cli");
    let source = root.join("main.jet");
    let receipt_dir = root.join("receipts");
    std::fs::write(&source, "fn run() {}\n").unwrap();
    let jet = env!("CARGO_BIN_EXE_jet");

    let first = Command::new(&jet)
        .args(["check", source.to_str().unwrap()])
        .env("JET_RECEIPT_DIR", &receipt_dir)
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );

    let second = Command::new(&jet)
        .args(["check", source.to_str().unwrap()])
        .env("JET_RECEIPT_DIR", &receipt_dir)
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        String::from_utf8_lossy(&second.stderr).contains("ok: check current"),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn project_check_does_not_replay_after_higher_priority_entry_appears() {
    let root = temp_root("stale-entry");
    let source_dir = root.join("src");
    let receipt_dir = root.join("receipts");
    std::fs::create_dir_all(&source_dir).unwrap();
    std::fs::write(
        root.join("package.jet"),
        "name: \"stale-entry\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(source_dir.join("run.jet"), "fn run() {}\n").unwrap();
    let jet = env!("CARGO_BIN_EXE_jet");

    let first = Command::new(jet)
        .arg("check")
        .current_dir(&root)
        .env("JET_RECEIPT_DIR", &receipt_dir)
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "initial project check failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );

    std::fs::write(root.join("run.jet"), "fn run() {}\n").unwrap();
    let second = Command::new(jet)
        .arg("check")
        .current_dir(&root)
        .env("JET_RECEIPT_DIR", &receipt_dir)
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "project check after entry creation failed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&second.stderr).contains("ok: check current"),
        "new higher-priority entry incorrectly replayed the old receipt:\n{}",
        String::from_utf8_lossy(&second.stderr)
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
