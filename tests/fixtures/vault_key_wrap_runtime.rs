include!("../../crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs");
include!("../../crates/jet-pkg-model/src/Prelude/Crypto.rs");
include!("../../crates/jet-codegen/src/Prelude/CoreLib/Top/UnicodeTables.rs");
include!("../../crates/jet-pkg-model/src/Prelude/VaultNfc.rs");
include!("../../crates/jet-pkg-model/src/Prelude/SecretsCrypto.rs");
include!("../../crates/jet-pkg-model/src/Prelude/VaultKeyWrap.rs");

#[cfg(test)]
mod vault_key_wrap_tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "jet-vault-key-wrap-{tag}-{}-{}",
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

    fn generate<T: JetVaultKey>(root: &std::path::Path, name: &str) -> JetVaultKeyRef<T> {
        jet_vault_set_test_authorizer(|_| true);
        let plan = jet_vault_prepare_generate_at::<T>(root, name).unwrap();
        let write = jet_vault_authorize_write_impl(&plan, "provision test key").unwrap();
        jet_vault_commit_generate_at(root, plan, write).unwrap()
    }

    fn import<T: JetVaultKey>(
        root: &std::path::Path,
        name: &str,
        wrapped: JetWrappedVaultKey,
        unlock: JetVaultKeyUnlock<'_>,
    ) -> Result<JetVaultKeyRef<T>, JetVaultKeyWrapError> {
        let plan = jet_vault_prepare_import_wrapped_at::<T>(root, name, wrapped, unlock)?;
        let write = jet_vault_authorize_wrapped_import_impl(&plan, "restore wrapped key")?;
        jet_vault_commit_import_wrapped_at(root, write, plan)
    }

    #[test]
    fn recipient_and_passphrase_round_trip_both_key_types() {
        let source = scratch("roundtrip-source");
        let destination = scratch("roundtrip-destination");
        provision(&source);
        provision(&destination);
        let signing = generate::<JetSigningKey>(&source, "release");
        let transport = generate::<JetX25519SecretKey>(&source, "transport");
        let recovery = jet_crypto_x25519_generate_impl().unwrap();
        let recovery_public = jet_crypto_x25519_public_typed_impl(&recovery);
        let passphrase = Secret(b"sixteen-byte recovery phrase".to_vec());

        let signing_wrapped = jet_vault_export_to_recipients_at(&source, &signing, &vec![recovery_public.clone()]).unwrap();
        let signing_restored = import::<JetSigningKey>(&destination, "release-restored", signing_wrapped.clone(), JetVaultKeyUnlock::Recipient(&recovery)).unwrap();
        assert_eq!(jet_vault_load_at(&source, &signing).unwrap().public_bytes(), jet_vault_load_at(&destination, &signing_restored).unwrap().public_bytes());
        assert_eq!(signing_wrapped.mode(), JetVaultKeyWrapMode::Recipient);
        assert_eq!(signing_wrapped.key_type(), 1);
        assert_eq!(jet_vault_wrapped_bytes_impl(&signing_wrapped), signing_wrapped.bytes());

        let transport_wrapped = jet_vault_export_to_passphrase_at(&source, &transport, &passphrase).unwrap();
        let transport_restored = import::<JetX25519SecretKey>(&destination, "transport-restored", transport_wrapped.clone(), JetVaultKeyUnlock::Passphrase(&passphrase)).unwrap();
        assert_eq!(jet_vault_load_at(&source, &transport).unwrap().public_bytes(), jet_vault_load_at(&destination, &transport_restored).unwrap().public_bytes());
        assert_eq!(transport_wrapped.mode(), JetVaultKeyWrapMode::Passphrase);
        assert_eq!(transport_wrapped.key_type(), 2);
        assert_eq!(JetWrappedVaultKey::from_bytes(transport_wrapped.bytes()).unwrap(), transport_wrapped);

        jet_vault_clear_test_authorizer();
        std::fs::remove_dir_all(source).unwrap();
        std::fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    fn canonical_vectors_cover_one_and_sixteen_recipients_and_both_types() {
        let signing_bytes = vec![0x11; 32];
        let signing_public: [u8; 32] = JetSigningKey::from_bytes(signing_bytes.clone()).unwrap().public_bytes().try_into().unwrap();
        let x25519_bytes = vec![0x22; 32];
        let x25519_public: [u8; 32] = JetX25519SecretKey::from_bytes(x25519_bytes.clone()).unwrap().public_bytes().try_into().unwrap();
        let signing_origin = JetVaultOrigin { repo_uuid: [1; 16], name: "release".into(), generation: 3, opaque_id: [2; 16], record_hash: [3; 32] };
        let x25519_origin = JetVaultOrigin { repo_uuid: [4; 16], name: "transport".into(), generation: 7, opaque_id: [5; 16], record_hash: [6; 32] };
        let recipients: Vec<_> = (1..=16).map(|byte| {
            let secret = JetX25519SecretKey(vec![byte; 32]);
            jet_crypto_x25519_public_typed_impl(&secret)
        }).collect();
        let passphrase = Secret(b"deterministic recovery phrase".to_vec());
        let counter = std::rc::Rc::new(std::cell::Cell::new(0u8));
        let next = counter.clone();
        jet_crypto_entropy_set_test_provider(move |out| {
            let start = next.get();
            for (offset, byte) in out.iter_mut().enumerate() { *byte = start.wrapping_add(offset as u8).wrapping_add(1); }
            next.set(start.wrapping_add(out.len() as u8));
            JetCryptoEntropyStep::Filled(out.len())
        });
        let one = jvkw_export_to_recipients::<JetSigningKey>(signing_origin.clone(), Zeroizing(signing_bytes.clone()), signing_public, 1_700_000_000_123, &vec![recipients[0].clone()]).unwrap();
        let sixteen = jvkw_export_to_recipients::<JetX25519SecretKey>(x25519_origin.clone(), Zeroizing(x25519_bytes.clone()), x25519_public, 1_700_000_000_123, &recipients).unwrap();
        let pass_signing = jvkw_export_to_passphrase::<JetSigningKey>(signing_origin, Zeroizing(signing_bytes), signing_public, 1_700_000_000_123, &passphrase).unwrap();
        let pass_x25519 = jvkw_export_to_passphrase::<JetX25519SecretKey>(x25519_origin, Zeroizing(x25519_bytes), x25519_public, 1_700_000_000_123, &passphrase).unwrap();
        let hashes: Vec<String> = [&one, &sixteen, &pass_signing, &pass_x25519].into_iter()
            .map(|wrapped| hex_bytes(&vault_hash(&[&wrapped.bytes()])))
            .collect();
        assert_eq!(hashes, vec![
            "4c853abf88a98887d39fa97d4bac2989e1aeffa50f23cd99450993f48a02f055",
            "0ee8b1798d9297317989c7bce4bc629235637602fa3d5d27f4b268555b3c45d9",
            "f230f5f36d00777f50926c44a656a2ffa9ff66a76b7cb3d049d77d11bc51c095",
            "641432fbff634bfa49ad1b2637dcb6bf947a60dec19750e0dd20e07e3656a95d",
        ]);
        assert_eq!(sixteen.bytes().len(), 16 + sixteen.header_len() + 80);
        jet_crypto_entropy_clear_test_provider();
    }

    #[test]
    fn parser_rejects_noncanonical_framing_before_secret_work() {
        let root = scratch("parser");
        provision(&root);
        let reference = generate::<JetSigningKey>(&root, "release");
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        let public = jet_crypto_x25519_public_typed_impl(&recipient);
        let wrapped = jet_vault_export_to_recipients_at(&root, &reference, &vec![public]).unwrap();
        assert_eq!(jet_vault_export_to_recipients_at(&root, &reference, &vec![]).unwrap_err(), JetVaultKeyWrapError::InvalidLength);
        let duplicate = jet_crypto_x25519_public_typed_impl(&recipient);
        assert_eq!(jet_vault_export_to_recipients_at(&root, &reference, &vec![duplicate.clone(), duplicate]).unwrap_err(), JetVaultKeyWrapError::InvalidEncoding);
        let too_many: Vec<_> = (1..=17).map(|byte| JetX25519PublicKey([byte; 32])).collect();
        assert_eq!(jet_vault_export_to_recipients_at(&root, &reference, &too_many).unwrap_err(), JetVaultKeyWrapError::InvalidLength);
        let bytes = wrapped.bytes();
        let mut cases = Vec::new();
        let mut bad = bytes.clone(); bad[0] ^= 1; cases.push((bad, JetVaultKeyWrapError::InvalidEncoding));
        let mut bad = bytes.clone(); bad[4] = 2; cases.push((bad, JetVaultKeyWrapError::UnsupportedVersion));
        let mut bad = bytes.clone(); bad[5] = 3; cases.push((bad, JetVaultKeyWrapError::UnsupportedMode));
        let mut bad = bytes.clone(); bad[6] = 3; cases.push((bad, JetVaultKeyWrapError::UnsupportedKeyType));
        let mut bad = bytes.clone(); bad[7] = 1; cases.push((bad, JetVaultKeyWrapError::InvalidEncoding));
        let mut bad = bytes.clone(); bad[8..12].copy_from_slice(&0u32.to_le_bytes()); cases.push((bad, JetVaultKeyWrapError::InvalidLength));
        let mut bad = bytes.clone(); bad[12..16].copy_from_slice(&63u32.to_le_bytes()); cases.push((bad, JetVaultKeyWrapError::InvalidLength));
        let mut bad = bytes.clone(); bad.push(0); cases.push((bad, JetVaultKeyWrapError::InvalidLength));
        for (bytes, expected) in cases { assert_eq!(JetWrappedVaultKey::from_bytes(bytes).unwrap_err(), expected); }
        assert_eq!(JetWrappedVaultKey::from_bytes(vec![0; 8193]).unwrap_err(), JetVaultKeyWrapError::InvalidLength);
        jet_vault_clear_test_authorizer();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn wrong_unlock_and_authenticated_tamper_collapse_to_open_failed() {
        let root = scratch("open-failed");
        provision(&root);
        let reference = generate::<JetSigningKey>(&root, "release");
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        let wrong = jet_crypto_x25519_generate_impl().unwrap();
        let public = jet_crypto_x25519_public_typed_impl(&recipient);
        let wrapped = jet_vault_export_to_recipients_at(&root, &reference, &vec![public]).unwrap();
        assert_eq!(jet_vault_prepare_import_wrapped_at::<JetSigningKey>(&root, "copy", wrapped.clone(), JetVaultKeyUnlock::Recipient(&wrong)).unwrap_err(), JetVaultKeyWrapError::OpenFailed);
        let phrase = Secret(b"sixteen-byte passphrase".to_vec());
        assert_eq!(jet_vault_prepare_import_wrapped_at::<JetSigningKey>(&root, "copy", wrapped.clone(), JetVaultKeyUnlock::Passphrase(&phrase)).unwrap_err(), JetVaultKeyWrapError::OpenFailed);
        assert_eq!(jet_vault_prepare_import_wrapped_at::<JetX25519SecretKey>(&root, "copy", wrapped.clone(), JetVaultKeyUnlock::Recipient(&recipient)).unwrap_err(), JetVaultKeyWrapError::OpenFailed);
        let mut tampered = wrapped.bytes();
        tampered[20] ^= 1;
        let tampered = JetWrappedVaultKey::from_bytes(tampered).unwrap();
        assert_eq!(jet_vault_prepare_import_wrapped_at::<JetSigningKey>(&root, "copy", tampered, JetVaultKeyUnlock::Recipient(&recipient)).unwrap_err(), JetVaultKeyWrapError::OpenFailed);
        assert_eq!(jet_vault_keywrap_test_recipient_open_count(), 1, "no-match performs exactly one bounded crypto path");
        jet_vault_clear_test_authorizer();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn same_repo_is_idempotent_but_revocation_never_reactivates() {
        let root = scratch("idempotent");
        provision(&root);
        let reference = generate::<JetSigningKey>(&root, "release");
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        let public = jet_crypto_x25519_public_typed_impl(&recipient);
        let wrapped = jet_vault_export_to_recipients_at(&root, &reference, &vec![public]).unwrap();
        let revision = decoded_store(&root).revision;
        let existing = import::<JetSigningKey>(&root, "ignored", wrapped.clone(), JetVaultKeyUnlock::Recipient(&recipient)).unwrap();
        assert_eq!(existing, reference);
        assert_eq!(decoded_store(&root).revision, revision);

        let retire = jet_vault_prepare_retire_at(&root, &reference, "archive").unwrap();
        let write = jet_vault_authorize_write_impl(&retire, "retire archived key").unwrap();
        jet_vault_commit_retire_at(&root, retire, write).unwrap();
        let retired_wrapped = jet_vault_export_to_recipients_at(&root, &reference, &vec![jet_crypto_x25519_public_typed_impl(&recipient)]).unwrap();
        let retired_revision = decoded_store(&root).revision;
        assert_eq!(import::<JetSigningKey>(&root, "ignored", retired_wrapped, JetVaultKeyUnlock::Recipient(&recipient)).unwrap(), reference);
        assert_eq!(decoded_store(&root).revision, retired_revision);

        let revoke = jet_vault_prepare_revoke_at(&root, &reference, "compromised").unwrap();
        let write = jet_vault_authorize_write_impl(&revoke, "revoke compromised key").unwrap();
        jet_vault_commit_revoke_at(&root, revoke, write).unwrap();
        assert_eq!(jet_vault_prepare_import_wrapped_at::<JetSigningKey>(&root, "release-restored", wrapped, JetVaultKeyUnlock::Recipient(&recipient)).unwrap_err(), JetVaultKeyWrapError::Vault(JetVaultError::Revoked));
        jet_vault_clear_test_authorizer();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cross_repo_import_gets_new_identity_and_preserves_origin() {
        let source = scratch("cross-source");
        let destination = scratch("cross-destination");
        provision(&source);
        provision(&destination);
        let reference = generate::<JetX25519SecretKey>(&source, "transport");
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        let public = jet_crypto_x25519_public_typed_impl(&recipient);
        let wrapped = jet_vault_export_to_recipients_at(&source, &reference, &vec![public]).unwrap();
        let revoke = jet_vault_prepare_revoke_at(&source, &reference, "source compromised after export").unwrap();
        let write = jet_vault_authorize_write_impl(&revoke, "revoke source").unwrap();
        jet_vault_commit_revoke_at(&source, revoke, write).unwrap();
        let restored = import::<JetX25519SecretKey>(&destination, "restored", wrapped, JetVaultKeyUnlock::Recipient(&recipient)).unwrap();
        assert_ne!(restored.repo_uuid, reference.repo_uuid);
        assert_ne!(restored.opaque_id, reference.opaque_id);
        let store = decoded_store(&destination);
        let origin = store.keys[0].origin.as_ref().unwrap();
        assert_eq!(origin.repo_uuid, reference.repo_uuid);
        assert_eq!(origin.name, "transport");
        assert_eq!(origin.generation, reference.generation);
        assert_eq!(origin.opaque_id, reference.opaque_id);
        assert_eq!(origin.record_hash, reference.record_hash);
        jet_vault_clear_test_authorizer();
        std::fs::remove_dir_all(source).unwrap();
        std::fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    fn authorized_plan_binds_destination_and_substitution() {
        let source = scratch("binding-source");
        let destination = scratch("binding-destination");
        provision(&source);
        provision(&destination);
        let reference = generate::<JetSigningKey>(&source, "release");
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        let public = jet_crypto_x25519_public_typed_impl(&recipient);
        let wrapped = jet_vault_export_to_recipients_at(&source, &reference, &vec![public]).unwrap();
        let plan = jet_vault_prepare_import_wrapped_at::<JetSigningKey>(&destination, "restored", wrapped.clone(), JetVaultKeyUnlock::Recipient(&recipient)).unwrap();
        let write = jet_vault_authorize_wrapped_import_impl(&plan, "approved restore").unwrap();
        generate::<JetX25519SecretKey>(&destination, "concurrent");
        assert_eq!(jet_vault_commit_import_wrapped_at(&destination, write, plan).unwrap_err(), JetVaultKeyWrapError::Vault(JetVaultError::Conflict));

        let first = jet_vault_prepare_import_wrapped_at::<JetSigningKey>(&destination, "one", wrapped.clone(), JetVaultKeyUnlock::Recipient(&recipient)).unwrap();
        let second = jet_vault_prepare_import_wrapped_at::<JetSigningKey>(&destination, "two", wrapped, JetVaultKeyUnlock::Recipient(&recipient)).unwrap();
        let first_write = jet_vault_authorize_wrapped_import_impl(&first, "first").unwrap();
        assert_eq!(jet_vault_commit_import_wrapped_at(&destination, first_write, second).unwrap_err(), JetVaultKeyWrapError::Vault(JetVaultError::Conflict));
        jet_vault_clear_test_authorizer();
        std::fs::remove_dir_all(source).unwrap();
        std::fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    fn weak_passphrases_entropy_failure_cleanup_and_redaction_are_explicit() {
        let root = scratch("failures");
        provision(&root);
        let reference = generate::<JetSigningKey>(&root, "release");
        let weak = Secret(b"too short".to_vec());
        assert_eq!(jet_vault_export_to_passphrase_at(&root, &reference, &weak).unwrap_err(), JetVaultKeyWrapError::WeakPassphrase);
        jet_crypto_entropy_set_test_provider(|_| JetCryptoEntropyStep::Failed);
        let recipient = JetX25519PublicKey([9; 32]);
        assert_eq!(jet_vault_export_to_recipients_at(&root, &reference, &vec![recipient]).unwrap_err(), JetVaultKeyWrapError::EntropyUnavailable);
        jet_crypto_entropy_clear_test_provider();

        let phrase = Secret(b"sixteen-byte passphrase".to_vec());
        let wrapped = jet_vault_export_to_passphrase_at(&root, &reference, &phrase).unwrap();
        let wrong_phrase = Secret(b"different recovery phrase".to_vec());
        assert_eq!(jet_vault_prepare_import_wrapped_at::<JetSigningKey>(&root, "wrong", wrapped.clone(), JetVaultKeyUnlock::Passphrase(&wrong_phrase)).unwrap_err(), JetVaultKeyWrapError::OpenFailed);
        let shown = format!("{wrapped:?} {wrapped}");
        assert!(shown.contains("mode:passphrase"));
        assert!(shown.contains("type:signing"));
        assert!(!shown.contains("sixteen-byte"));
        assert!(!shown.contains("ciphertext"));

        let observed = std::rc::Rc::new(std::cell::Cell::new(false));
        let witness = observed.clone();
        jet_crypto_set_zeroize_test_observer(move |bytes| {
            if bytes.len() == 32 && bytes.iter().all(|byte| *byte == 0) { witness.set(true); }
        });
        let plan = jet_vault_prepare_import_wrapped_at::<JetSigningKey>(&root, "restored", wrapped, JetVaultKeyUnlock::Passphrase(&phrase)).unwrap();
        drop(plan);
        jet_crypto_clear_zeroize_test_observer();
        assert!(observed.get(), "prepared plaintext key is zeroized when abandoned");
        jet_vault_clear_test_authorizer();
        std::fs::remove_dir_all(root).unwrap();
    }
}
