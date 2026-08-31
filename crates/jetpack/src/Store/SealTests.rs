use super::*;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestRoot {
    path: PathBuf,
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn temp_roots() -> (Roots, TestRoot) {
    let path = std::env::temp_dir().join(format!(
        "jetpack-seal-{}-{}",
        std::process::id(),
        TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    let guard = TestRoot { path };
    let roots = Roots {
        root: guard.path.clone(),
        dev_mode: true,
    };
    (roots, guard)
}

fn fixture(roots: &Roots, name: &str) -> IngestedObject {
    fixture_with_platform_artifact_kind(roots, name, "seal-test")
}

fn fixture_with_platform_artifact_kind(
    roots: &Roots,
    name: &str,
    platform_artifact_kind: &str,
) -> IngestedObject {
    let source = roots.root.join(format!("seal-source-{name}"));
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("payload"), b"trusted").unwrap();
    ingest_tree(
        roots,
        &IngestRequest {
            name: name.to_string(),
            version: "1".to_string(),
            reference: format!("path:{name}"),
            cache_identity: CacheIdentity {
                source_fingerprint: "seal-source".to_string(),
                recipe_fingerprint: "seal-recipe".to_string(),
                policy_fingerprint: "seal-policy".to_string(),
                platform: crate::Envelope::host_platform(),
            },
            references: Vec::new(),
            outputs: BTreeMap::from([(String::from("out"), source)]),
            signature: String::new(),
            provenance: "seal-test".to_string(),
            platform_artifact_kind: platform_artifact_kind.to_string(),
        },
    )
    .unwrap()
}

#[cfg(unix)]
#[test]
fn native_admission_seals_after_shared_cas_hardlinking() {
    let (roots, _guard) = temp_roots();
    let ingested = fixture_with_platform_artifact_kind(&roots, "native", "");
    let object = PathBuf::from(&ingested.entry.out);
    let digest = &ingested.entry.envelope.output_hash;
    assert!(roots.hangar_dir().join("cas").is_dir());
    assert_eq!(
        check_seal(&object, &roots.hangar_dir()).unwrap(),
        Some(digest.clone())
    );

    reset_verified_digest_hash_count(&object);
    assert_eq!(
        try_entry_output_hash(&roots, &ingested.entry).unwrap(),
        *digest
    );
    assert_eq!(verified_digest_hash_count(&object), 0);
}

#[cfg(unix)]
fn mtime(metadata: &fs::Metadata) -> (i64, i64) {
    use std::os::unix::fs::MetadataExt as _;
    (metadata.mtime(), metadata.mtime_nsec())
}

#[cfg(unix)]
fn set_mtime(path: &Path, seconds: i64, nanos: i64) {
    let stamp = format!("@{seconds}.{nanos:09}");
    assert!(
        Command::new("touch")
            .arg("-m")
            .arg("-d")
            .arg(stamp)
            .arg(path)
            .status()
            .unwrap()
            .success(),
        "touch failed for {}",
        path.display()
    );
}

#[cfg(unix)]
fn make_writable(path: &Path) {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt as _;
    permissions.set_mode(permissions.mode() | 0o200);
    fs::set_permissions(path, permissions).unwrap();
}

#[cfg(unix)]
#[test]
fn seal_written_at_admission_and_warm_verification_skips_content_hash() {
    let (roots, _guard) = temp_roots();
    let ingested = fixture(&roots, "warm");
    let object = PathBuf::from(&ingested.entry.out);
    let digest = &ingested.entry.envelope.output_hash;
    let seal = roots.hangar_dir().join(SEALS_DIR).join(digest);
    assert!(seal.is_file(), "admission must write {seal:?}");

    reset_verified_digest_hash_count(&object);
    assert_eq!(
        try_entry_output_hash(&roots, &ingested.entry).unwrap(),
        *digest
    );
    assert_eq!(verified_digest_hash_count(&object), 0);

    // Owner-ratified verify-once tradeoff: bytes can change without a tuple
    // change and remain trusted until an explicit `hangar verify` audit.
    let payload = object.join("payload");
    let before = fs::metadata(&payload).unwrap();
    make_writable(&payload);
    fs::write(&payload, b"tamper!").unwrap();
    let (seconds, nanos) = mtime(&before);
    set_mtime(&payload, seconds, nanos);
    let after = fs::metadata(&payload).unwrap();
    assert_eq!(before.len(), after.len());
    assert_eq!(mtime(&before), mtime(&after));

    reset_verified_digest_hash_count(&object);
    assert_eq!(list_checked(&roots).unwrap().len(), 1);
    assert_eq!(verified_digest_hash_count(&object), 0);
    assert_eq!(
        check_seal(&object, &roots.hangar_dir()).unwrap(),
        Some(digest.clone())
    );
}

#[cfg(unix)]
#[test]
fn seal_mtime_drift_rehashes_once_and_reseals() {
    let (roots, _guard) = temp_roots();
    let ingested = fixture(&roots, "drift");
    let object = PathBuf::from(&ingested.entry.out);
    let digest = &ingested.entry.envelope.output_hash;
    let seal = roots.hangar_dir().join(SEALS_DIR).join(digest);
    let before = fs::read(&seal).unwrap();
    let payload = object.join("payload");
    let metadata = fs::metadata(&payload).unwrap();
    let (seconds, nanos) = mtime(&metadata);
    set_mtime(&payload, seconds + 1, nanos);

    reset_verified_digest_hash_count(&object);
    assert_eq!(
        try_entry_output_hash(&roots, &ingested.entry).unwrap(),
        *digest
    );
    assert_eq!(verified_digest_hash_count(&object), 1);
    assert_ne!(before, fs::read(&seal).unwrap());

    assert_eq!(
        try_entry_output_hash(&roots, &ingested.entry).unwrap(),
        *digest
    );
    assert_eq!(verified_digest_hash_count(&object), 1);
}

#[cfg(unix)]
#[test]
fn seal_digest_mismatch_quarantines_the_tampered_object() {
    let (roots, _guard) = temp_roots();
    let ingested = fixture(&roots, "quarantine");
    let object = PathBuf::from(&ingested.entry.out);
    let payload = object.join("payload");
    make_writable(&payload);
    fs::write(&payload, b"tamper!").unwrap();

    let expectation = CacheExpectation {
        identity: ingested.entry.cache_identity.clone(),
        owned_output: Some(object.clone()),
        allow_unsigned_local: true,
    };
    quarantine_invalid_entry(&roots, &ingested.entry, &expectation).unwrap();

    assert!(!object.exists());
    let quarantine = roots.hangar_dir().join("quarantine");
    assert!(fs::read_dir(quarantine)
        .unwrap()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().starts_with("output-")));
    assert!(!roots
        .hangar_dir()
        .join(SEALS_DIR)
        .join(&ingested.entry.envelope.output_hash)
        .exists());
}

#[cfg(unix)]
#[test]
fn hangar_verify_rehashes_despite_valid_seal_and_catches_content_corruption() {
    let (roots, _guard) = temp_roots();
    let ingested = fixture(&roots, "audit");
    let object = PathBuf::from(&ingested.entry.out);
    let digest = ingested.entry.envelope.output_hash.clone();
    assert_eq!(
        check_seal(&object, &roots.hangar_dir()).unwrap(),
        Some(digest)
    );

    let payload = object.join("payload");
    let before = fs::metadata(&payload).unwrap();
    make_writable(&payload);
    fs::write(&payload, b"tamper!").unwrap();
    let (seconds, nanos) = mtime(&before);
    set_mtime(&payload, seconds, nanos);
    assert_eq!(mtime(&before), mtime(&fs::metadata(&payload).unwrap()));

    reset_verified_digest_hash_count(&object);
    assert!(verify_hangar_object(&roots, &ingested.entry).is_err());
    assert_eq!(verified_digest_hash_count(&object), 1);
}

#[cfg(unix)]
#[test]
fn armed_command_memo_checks_identity_once_and_stays_thread_local() {
    let (roots, _guard) = temp_roots();
    let ingested = fixture(&roots, "memo");
    let object = PathBuf::from(&ingested.entry.out);
    let digest = ingested.entry.envelope.output_hash.clone();
    let hangar = roots.hangar_dir();

    // Run the armed command window on its own thread so the memo can never
    // leak into sibling tests on this harness thread.
    let armed = {
        let object = object.clone();
        let hangar = hangar.clone();
        let digest = digest.clone();
        std::thread::spawn(move || {
            Seal::arm_command_memo();
            assert_eq!(check_seal(&object, &hangar).unwrap(), Some(digest.clone()));
            // Drift the tuple identity mid-command: one command holds one
            // coherent verified view (D-JPK-VERIFYONCE1=A), so the memoized
            // verdict stands without another identity walk.
            let payload = object.join("payload");
            let metadata = fs::metadata(&payload).unwrap();
            let (seconds, nanos) = mtime(&metadata);
            set_mtime(&payload, seconds + 7, nanos);
            assert_eq!(check_seal(&object, &hangar).unwrap(), Some(digest.clone()));
            // Forgetting the seal forgets the memo too.
            Seal::remove(&object, &hangar).unwrap();
            assert_eq!(check_seal(&object, &hangar).unwrap(), None);
        })
    };
    armed.join().unwrap();

    // A fresh (unarmed) thread keeps strict per-use drift detection: the seal
    // was removed and the tuples drifted, so nothing is trusted.
    assert_eq!(check_seal(&object, &hangar).unwrap(), None);
}
