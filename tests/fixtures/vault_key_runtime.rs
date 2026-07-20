include!("../../crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs");
include!("../../crates/jet-pkg-model/src/Prelude/Crypto.rs");
include!("../../crates/jet-codegen/src/Prelude/CoreLib/Top/UnicodeTables.rs");
include!("../../crates/jet-pkg-model/src/Prelude/VaultNfc.rs");
include!("../../crates/jet-pkg-model/src/Prelude/SecretsCrypto.rs");

#[cfg(test)]
mod vault_key_tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "jet-vault-key-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(path.join(".jet")).unwrap();
        path
    }

    fn provision(root: &std::path::Path) {
        let keys = root.join("keys");
        std::fs::create_dir(&keys).unwrap();
        let (identity, recipient) = jet_vault_keygen_impl();
        std::fs::write(keys.join("secrets.identity"), identity).unwrap();
        std::fs::write(root.join(".jet/secrets-recipients"), format!("{recipient}\n")).unwrap();
    }

    fn decoded_store(root: &std::path::Path) -> JetVaultStore {
        let identity = std::fs::read_to_string(root.join("keys/secrets.identity")).unwrap();
        let ciphertext = std::fs::read(root.join(".jet/secrets.age")).unwrap();
        let plaintext = jet_vault_decrypt_impl(&identity, &ciphertext).unwrap();
        jet_vault_decode_v2(&plaintext).unwrap()
    }

    #[test]
    fn absent_store_reads_do_not_require_mutation_recipients() {
        let root = scratch("absent-read");
        assert_eq!(jet_vault_current_at::<JetSigningKey>(&root, "release").unwrap(), None);
        assert!(jet_vault_versions_at::<JetSigningKey>(&root, "release").unwrap().is_empty());
        let missing = JetVaultKeyRef::<JetSigningKey> {
            provider: JetVaultProvider::Repo,
            repo_uuid: [1; 16],
            name: "release".into(),
            generation: 1,
            opaque_id: [2; 16],
            record_hash: [3; 32],
            marker: std::marker::PhantomData,
        };
        assert!(matches!(jet_vault_load_at(&root, &missing), Err(JetVaultError::NotFound)));
        assert_eq!(jet_vault_status_at(&root, &missing).unwrap_err(), JetVaultError::NotFound);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn typed_generate_rotate_retire_revoke_and_conflict() {
        let root = scratch("lifecycle");
        provision(&root);
        jet_vault_set_test_authorizer(|_| true);

        let plan = jet_vault_prepare_generate_at::<JetSigningKey>(&root, "release").unwrap();
        let stale = jet_vault_prepare_generate_at::<JetX25519SecretKey>(&root, "transport").unwrap();
        let write = jet_vault_authorize_write_impl(&plan, "initial release key").unwrap();
        let first = jet_vault_commit_generate_at(&root, plan, write).unwrap();
        assert_eq!(first.generation(), 1);
        assert_eq!(jet_vault_status_at(&root, &first).unwrap(), JetVaultKeyStatus::Active);
        assert!(jet_vault_load_at(&root, &first).is_ok());

        let stale_write = jet_vault_authorize_write_impl(&stale, "stale write").unwrap();
        assert_eq!(
            jet_vault_commit_generate_at(&root, stale, stale_write).unwrap_err(),
            JetVaultError::Conflict
        );

        let rotate = jet_vault_prepare_rotate_at::<JetSigningKey>(&root, "release").unwrap();
        let write = jet_vault_authorize_write_impl(&rotate, "quarterly rotation").unwrap();
        let rotated = jet_vault_commit_rotate_at(&root, rotate, write).unwrap();
        assert_eq!(rotated.previous, first);
        assert_eq!(rotated.current.generation(), 2);
        assert_eq!(jet_vault_status_at(&root, &first).unwrap(), JetVaultKeyStatus::Retired);
        assert!(jet_vault_load_at(&root, &first).is_ok(), "retired exact refs remain loadable");

        let revoke = jet_vault_prepare_revoke_at(&root, &first, "suspected exposure").unwrap();
        let write = jet_vault_authorize_write_impl(&revoke, "incident response").unwrap();
        jet_vault_commit_revoke_at(&root, revoke, write).unwrap();
        assert_eq!(jet_vault_status_at(&root, &first).unwrap(), JetVaultKeyStatus::Revoked);
        assert!(matches!(jet_vault_load_at(&root, &first), Err(JetVaultError::Revoked)));

        let current = jet_vault_current_at::<JetSigningKey>(&root, "release").unwrap().unwrap();
        assert_eq!(current, rotated.current);
        assert_eq!(jet_vault_versions_at::<JetSigningKey>(&root, "release").unwrap().len(), 2);
        jet_vault_clear_test_authorizer();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn authority_is_exact_and_denial_generates_no_key() {
        let root = scratch("authority");
        provision(&root);
        jet_vault_set_test_authorizer(|preview| {
            assert!(preview.contains("generate SigningKey release"));
            false
        });
        let plan = jet_vault_prepare_generate_at::<JetSigningKey>(&root, "release").unwrap();
        assert_eq!(
            jet_vault_authorize_write_impl(&plan, "unapproved").unwrap_err(),
            JetVaultError::AuthorityDenied
        );
        assert!(!root.join(".jet/secrets.age").exists());
        jet_vault_clear_test_authorizer();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn jvlt2_codec_is_canonical_bounded_and_preserves_legacy_strings() {
        let mut store = JetVaultStore::new([7; 16]);
        store.revision = 1;
        store.strings.push(("db_password".into(), "hunter2".into()));
        let bytes = jet_vault_encode_v2(&store).unwrap();
        assert_eq!(bytes.len(), 100);
        assert_eq!(&bytes[..4], b"JVLT");
        assert_eq!(bytes[4], 2);
        assert_eq!(&bytes[68..], &vault_hash(&[b"JVLT2 payload", &bytes[..68]]));
        assert_eq!(jet_vault_decode_v2(&bytes).unwrap(), store);

        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(jet_vault_decode_v2(&trailing).unwrap_err(), JetVaultError::InvalidEncoding);
        assert_eq!(
            jet_vault_validate_name(" ../secret ").unwrap_err(),
            JetVaultError::InvalidName
        );
        assert_eq!(jet_vault_validate_name("e\u{301}").unwrap_err(), JetVaultError::InvalidName);
        assert!(jet_vault_validate_name("é").is_ok());
    }

    #[test]
    fn exact_operation_preview_provider_and_session_are_bound() {
        let root = scratch("binding");
        provision(&root);
        let key = jet_crypto_signing_generate_impl().unwrap();
        let plan = jet_vault_prepare_store_at(&root, "release", key).unwrap();
        let mut operation = Vec::new();
        operation.extend_from_slice(b"JVLT2 operation");
        operation.extend_from_slice(&plan.repo_uuid);
        operation.extend_from_slice(&plan.start_revision.to_le_bytes());
        operation.extend_from_slice(&plan.start_hash);
        operation.extend_from_slice(&plan.provider_hash);
        operation.extend_from_slice(&[2, 1]);
        operation.extend_from_slice(&7u16.to_le_bytes());
        operation.extend_from_slice(b"release");
        operation.push(0);
        operation.extend_from_slice(&[0; 16]);
        operation.extend_from_slice(&0u64.to_le_bytes());
        operation.extend_from_slice(&[0; 16]);
        operation.extend_from_slice(&[0; 32]);
        operation.extend_from_slice(&0u16.to_le_bytes());
        operation.extend_from_slice(&plan.public_key_hash);
        operation.extend_from_slice(&plan.key_digest);
        operation.extend_from_slice(&[0; 32]);
        assert_eq!(plan.operation_hash, vault_hash(&[&operation]));

        jet_vault_set_test_authorizer(|_| true);
        let mut write = jet_vault_authorize_write_impl(&plan, "approved release signer").unwrap();
        let mut preview = Vec::new();
        preview.extend_from_slice(b"JVLT2 authority preview");
        preview.extend_from_slice(&plan.operation_hash);
        preview.extend_from_slice(&[2, 1]);
        preview.extend_from_slice(&plan.repo_uuid);
        preview.extend_from_slice(&0u64.to_le_bytes());
        preview.extend_from_slice(&7u16.to_le_bytes());
        preview.extend_from_slice(b"release");
        preview.extend_from_slice(&0u64.to_le_bytes());
        preview.extend_from_slice(&1u64.to_le_bytes());
        preview.extend_from_slice(&0u16.to_le_bytes());
        preview.extend_from_slice(&23u16.to_le_bytes());
        preview.extend_from_slice(b"approved release signer");
        preview.extend_from_slice(&plan.expires_unix_ms.to_le_bytes());
        assert_eq!(write.preview_hash, vault_hash(&[&preview]));
        write.session = write.session.wrapping_add(1);
        assert_eq!(jet_vault_commit_store_at(&root, plan, write).unwrap_err(), JetVaultError::Conflict);

        let plan = jet_vault_prepare_generate_at::<JetSigningKey>(&root, "release").unwrap();
        let uuid = vault_uuid_hex(&plan.repo_uuid);
        let trust = root.join("trust");
        std::fs::write(&trust, format!("grant:user:vault.write:{uuid}\n")).unwrap();
        jet_vault_set_test_trust_path(Some(trust));
        assert!(vault_headless_granted(&plan.repo_uuid));
        jet_vault_set_test_trust_path(None);
        jet_vault_clear_test_authorizer();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn historical_pairs_migrate_with_the_previewed_uuid_and_exact_start_hash() {
        let root = scratch("migration");
        provision(&root);
        let identity = std::fs::read_to_string(root.join("keys/secrets.identity")).unwrap();
        let recipient = std::fs::read_to_string(root.join(".jet/secrets-recipients")).unwrap();
        let pairs = vec![("token".to_string(), "secret".to_string())];
        let legacy = jet_vault_encode_pairs(&pairs);
        let encrypted = jet_vault_encrypt_impl(&vec![recipient.trim().to_string()], &legacy).unwrap();
        std::fs::write(root.join(".jet/secrets.age"), encrypted).unwrap();
        let plan = jet_vault_prepare_generate_at::<JetSigningKey>(&root, "release").unwrap();
        let (_, canonical) = vault_canonical_pairs(pairs.clone()).unwrap();
        assert_eq!(plan.start_revision, 0);
        assert_eq!(plan.start_hash, vault_hash(&[b"JVLT1 starting store", &canonical]));
        let previewed_uuid = plan.repo_uuid;
        jet_vault_set_test_authorizer(|_| true);
        let write = jet_vault_authorize_write_impl(&plan, "migrate and provision").unwrap();
        let reference = jet_vault_commit_generate_at(&root, plan, write).unwrap();
        let store = decoded_store(&root);
        assert_eq!(store.revision, 1);
        assert_eq!(store.repo_uuid, previewed_uuid);
        assert_eq!(reference.repo_uuid, previewed_uuid);
        assert_eq!(store.strings, pairs);
        jet_vault_clear_test_authorizer();
        std::fs::remove_dir_all(root).unwrap();
        let _ = identity;
    }

    #[test]
    fn linux_install_cancellation_and_durability_boundaries_are_literal() {
        let root = scratch("faults");
        provision(&root);
        jet_vault_set_test_authorizer(|_| true);
        let plan = jet_vault_prepare_generate_at::<JetSigningKey>(&root, "release").unwrap();
        let write = jet_vault_authorize_write_impl(&plan, "initial signer").unwrap();
        jet_vault_set_test_fault(Some("cancel-before-install"));
        assert_eq!(jet_vault_commit_generate_at(&root, plan, write).unwrap_err(), JetVaultError::Conflict);
        assert!(!root.join(".jet/secrets.age").exists());
        assert!(!std::fs::read_dir(root.join(".jet")).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().contains(".new.")));

        jet_vault_set_test_fault(Some("cancel-after-install"));
        let plan = jet_vault_prepare_generate_at::<JetSigningKey>(&root, "release").unwrap();
        let write = jet_vault_authorize_write_impl(&plan, "initial signer").unwrap();
        let first = jet_vault_commit_generate_at(&root, plan, write).unwrap();
        assert_eq!(first.generation(), 1);

        let plan = jet_vault_prepare_rotate_at::<JetSigningKey>(&root, "release").unwrap();
        let write = jet_vault_authorize_write_impl(&plan, "rotate with durability fault").unwrap();
        jet_vault_set_test_fault(Some("durability-after-install"));
        assert_eq!(jet_vault_commit_rotate_at(&root, plan, write).unwrap_err(), JetVaultError::DurabilityUnknown);
        assert_eq!(decoded_store(&root).revision, 2, "installed revision is visible");
        assert!(std::fs::read_dir(root.join(".jet")).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().contains(".new.")), "old store remains as recovery backup");
        jet_vault_set_test_fault(None);
        jet_vault_clear_test_authorizer();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn linux_openat2_rejects_a_symlinked_vault_directory() {
        use std::os::unix::fs::symlink;
        let root = scratch("symlink");
        let actual = root.join("actual");
        std::fs::create_dir(&actual).unwrap();
        std::fs::remove_dir(root.join(".jet")).unwrap();
        symlink(&actual, root.join(".jet")).unwrap();
        let error = match jet_vault_prepare_generate_at::<JetSigningKey>(&root, "release") {
            Ok(_) => panic!("symlinked .jet directory was accepted"),
            Err(error) => error,
        };
        assert_eq!(error, JetVaultError::UnsupportedProvider);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn provider_change_invalidates_an_authorized_plan() {
        let root = scratch("provider-change");
        provision(&root);
        jet_vault_set_test_authorizer(|_| true);
        let plan = jet_vault_prepare_generate_at::<JetSigningKey>(&root, "release").unwrap();
        let write = jet_vault_authorize_write_impl(&plan, "initial signer").unwrap();
        let (_, other_recipient) = jet_vault_keygen_impl();
        std::fs::write(root.join(".jet/secrets-recipients"), format!("{other_recipient}\n")).unwrap();
        assert_eq!(jet_vault_commit_generate_at(&root, plan, write).unwrap_err(), JetVaultError::Conflict);
        assert!(!root.join(".jet/secrets.age").exists());
        jet_vault_clear_test_authorizer();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn string_updates_preserve_typed_rows_in_the_shared_atomic_store() {
        let root = scratch("hybrid-store");
        provision(&root);
        jet_vault_set_test_authorizer(|_| true);
        let plan = jet_vault_prepare_generate_at::<JetSigningKey>(&root, "release").unwrap();
        let write = jet_vault_authorize_write_impl(&plan, "initial signer").unwrap();
        let reference = jet_vault_commit_generate_at(&root, plan, write).unwrap();
        jet_vault_replace_strings_at(&root, vec![("token".into(), "secret".into())]).unwrap();
        let store = decoded_store(&root);
        assert_eq!(store.revision, 2);
        assert_eq!(store.strings, vec![("token".into(), "secret".into())]);
        assert_eq!(store.keys.len(), 1);
        assert!(jet_vault_load_at(&root, &reference).is_ok());
        jet_vault_clear_test_authorizer();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_lock_requires_age_owner_dead_pid_nonce_and_unchanged_store() {
        let root = scratch("stale-lock");
        provision(&root);
        jet_vault_set_test_authorizer(|_| true);
        let plan = jet_vault_prepare_generate_at::<JetSigningKey>(&root, "release").unwrap();
        let lock = root.join(".jet/.secrets.age.lock");
        std::fs::write(
            &lock,
            format!(
                "repo={} revision=0 pid=999999 process-start=1 nonce=11111111111111111111111111111111 hash={}\n",
                vault_uuid_hex(&plan.repo_uuid),
                hex_bytes(&plan.start_hash)
            ),
        )
        .unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(601);
        std::fs::File::open(&lock)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(old))
            .unwrap();
        let write = jet_vault_authorize_write_impl(&plan, "recover stale lock").unwrap();
        assert_eq!(jet_vault_commit_generate_at(&root, plan, write).unwrap().generation(), 1);
        assert!(!lock.exists());
        assert!(!std::fs::read_dir(root.join(".jet")).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().contains("lock.stale")));
        jet_vault_clear_test_authorizer();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imported_origin_round_trips_and_invalid_combinations_reject() {
        let key = jet_crypto_signing_generate_impl().unwrap();
        let key_bytes = key.into_bytes();
        let public_key_hash = vault_public_hash(1, &key_bytes).unwrap();
        let origin = JetVaultOrigin {
            repo_uuid: [3; 16],
            name: "source".into(),
            generation: 4,
            opaque_id: [5; 16],
            record_hash: [6; 32],
        };
        let mut record = JetVaultRecord {
            name: "release".into(),
            key_type: 1,
            generation: 1,
            status: JetVaultKeyStatus::Active,
            provenance: JetVaultProvenance::Imported,
            opaque_id: [2; 16],
            created_unix_ms: 10,
            status_unix_ms: 10,
            record_hash: [0; 32],
            public_key_hash,
            reason_hash: [0; 32],
            origin: Some(origin.clone()),
            key: key_bytes,
        };
        record.record_hash = vault_record_hash(&record);
        let mut store = JetVaultStore::new([1; 16]);
        store.revision = 1;
        let record_hash = record.record_hash;
        store.keys.push(record);
        let decoded = jet_vault_decode_v2(&jet_vault_encode_v2(&store).unwrap()).unwrap();
        assert_eq!(decoded.keys[0].origin, Some(origin));
        assert_eq!(decoded.keys[0].record_hash, record_hash);
        store.keys[0].provenance = JetVaultProvenance::Generated;
        store.keys[0].record_hash = vault_record_hash(&store.keys[0]);
        assert_eq!(jet_vault_encode_v2(&store).unwrap_err(), JetVaultError::InvalidEncoding);
    }

    #[test]
    fn errors_and_handles_are_redacted() {
        let error = JetVaultError::Io { operation: "read", redacted_path: "<vault-store>" };
        let shown = format!("{error:?} {error}");
        assert!(!shown.contains("secrets.age"));
        assert!(!shown.contains("AGE-SECRET"));
        assert!(!shown.contains("/home/"));
    }

    #[test]
    fn stored_key_bytes_are_redacted_and_zeroized_on_drop() {
        let key_bytes = vec![0x5a; 32];
        let public_key_hash = vault_public_hash(2, &key_bytes).unwrap();
        let mut record = JetVaultRecord {
            name: "transport".into(),
            key_type: 2,
            generation: 1,
            status: JetVaultKeyStatus::Active,
            provenance: JetVaultProvenance::Imported,
            opaque_id: [2; 16],
            created_unix_ms: 10,
            status_unix_ms: 10,
            record_hash: [0; 32],
            public_key_hash,
            reason_hash: [0; 32],
            origin: None,
            key: key_bytes,
        };
        record.record_hash = vault_record_hash(&record);
        let shown = format!("{record:?}");
        assert!(shown.contains("<redacted>"));
        assert!(!shown.contains("90, 90"));
        let observed = std::rc::Rc::new(std::cell::Cell::new(false));
        let witness = observed.clone();
        jet_crypto_set_zeroize_test_observer(move |bytes| {
            if bytes.len() == 32 && bytes.iter().all(|byte| *byte == 0) {
                witness.set(true);
            }
        });
        drop(record);
        jet_crypto_clear_zeroize_test_observer();
        assert!(observed.get());
    }
}
