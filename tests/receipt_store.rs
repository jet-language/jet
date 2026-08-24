use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use jet::ReceiptStore::{participating_verb, ReceiptStore, PARTICIPATING_VERBS, RECEIPT_KINDS};

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
    store.write(&source_claim, 0, b"source", b"").unwrap();
    store.write(&unrelated_claim, 0, b"unrelated", b"").unwrap();

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
fn test_golden_budget_and_api_share_one_current_receipt_store() {
    let root = temp_root("kinds");
    let source = root.join("main.jet");
    std::fs::write(&source, "fn run() {}\n").unwrap();
    let store = ReceiptStore::new(root.join("store"));
    let args = vec!["act".into()];

    for kind in RECEIPT_KINDS {
        store
            .record(
                kind,
                &args,
                std::slice::from_ref(&source),
                0,
                kind.as_bytes(),
                b"",
            )
            .unwrap();
    }

    assert_eq!(store.list().unwrap().len(), RECEIPT_KINDS.len());
    assert_eq!(store.list_current().unwrap().len(), RECEIPT_KINDS.len());
    std::fs::write(&source, "fn run() { print(1) }\n").unwrap();
    assert!(store.list_current().unwrap().is_empty());
    assert_eq!(store.list().unwrap().len(), RECEIPT_KINDS.len());

    let _ = std::fs::remove_dir_all(root);
}
