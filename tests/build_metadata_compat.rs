use std::fs;
use std::path::{Path, PathBuf};

use jet::ReceiptStore::ReceiptStore;
use jet::Lock;
use jet_store::{ActionHandle, BuildRecord, Digest, Store};

const LOCK_V0: &str = include_str!("fixtures/build-metadata-compat/lock-v0.lock");
const BUILD_RECORD_V0: &str =
    include_str!("fixtures/build-metadata-compat/build-record-v0.json");
const STORE_V0: &[u8] = include_bytes!("fixtures/build-metadata-compat/store-v0.magic");
const RECEIPT_V1: &[u8] = include_bytes!("fixtures/build-metadata-compat/receipt-v1.magic");
const RECEIPT_KEY: &str = "e27dfe4d5c5f1b2c8a861de44091b4c7ac7eec90fe2f4cac59a328f73c66337f";

fn scratch_root() -> PathBuf {
    PathBuf::from(
        std::env::var_os("HOME").expect("HOME is required for the compatibility scratch root"),
    )
    .join(".cache/jet-luna/build-metadata-compat-conformance")
}

fn reset_scratch(root: &Path) {
    let _ = fs::remove_dir_all(root);
    fs::create_dir_all(root).expect("create compatibility scratch root");
}

#[test]
fn previous_tool_owned_artifacts_are_read_fail_closed_without_rewriting_input() {
    let root = scratch_root();
    reset_scratch(&root);

    let lock_path = root.join(".jet/lock");
    fs::create_dir_all(lock_path.parent().unwrap()).expect("create lock directory");
    fs::write(&lock_path, LOCK_V0).expect("write lock fixture");
    let lock_before = fs::read(&lock_path).expect("read lock fixture");
    let diagnostic = Lock::load_strict_with_path(&root)
        .expect_err("unsupported lock version must stop the strict reader");
    assert_eq!(diagnostic.code, "E1214");
    assert_eq!(fs::read(&lock_path).unwrap(), lock_before);

    assert!(STORE_V0.starts_with(b"jet.store.v0\0action\0"));
    assert!(STORE_V0.windows(b"jet.store.v0\0artifact\0".len()).any(|window| {
        window == b"jet.store.v0\0artifact\0"
    }));
    let action_key = STORE_V0[18..50]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let action = ActionHandle::new(Digest::from_hex(&action_key).expect("fixture action key"));
    let store = Store::new(root.join("store")).expect("create store");
    let action_path = store.action_path(&action);
    fs::write(&action_path, STORE_V0).expect("write store fixture");
    let store_before = fs::read(&action_path).expect("read store fixture");
    assert!(store
        .get_action(&action)
        .expect("read previous store action")
        .is_none());
    assert_eq!(fs::read(&action_path).unwrap(), store_before);
    assert!(matches!(
        store.lookup_artifact(&action_key).expect("read previous artifact"),
        jet_store::ArtifactLookup::Missing
    ));
    let mut corrupt_store = STORE_V0.to_vec();
    corrupt_store[0] = b'x';
    fs::write(&action_path, corrupt_store).expect("write corrupt store probe");
    assert!(matches!(
        store.lookup_artifact(&action_key).expect("read corrupt store probe"),
        jet_store::ArtifactLookup::Corrupt
    ));
    assert!(
        !action_path.exists(),
        "genuine corruption should still be quarantined"
    );

    let build_record_error = BuildRecord::from_json(BUILD_RECORD_V0)
        .expect_err("unsupported build-record schema must be rejected");
    assert_eq!(
        build_record_error,
        "unsupported build record schema `jet.build-record/v0`"
    );

    let receipts_root = root.join("receipts");
    let receipts_objects = receipts_root.join("objects");
    fs::create_dir_all(&receipts_objects).expect("create receipt object directory");
    let receipt_path = receipts_objects.join(RECEIPT_KEY);
    fs::write(&receipt_path, RECEIPT_V1).expect("write receipt fixture");
    assert!(
        ReceiptStore::new(&receipts_root)
            .list()
            .expect("list previous receipts")
            .is_empty()
    );
    assert_eq!(fs::read(&receipt_path).unwrap(), RECEIPT_V1);

    fs::remove_dir_all(root).expect("remove compatibility scratch root");
}
