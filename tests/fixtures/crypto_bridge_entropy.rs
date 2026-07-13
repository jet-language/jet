include!("../../crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs");
include!("../../crates/jet-pkg-model/src/Prelude/Crypto.rs");

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn fail_entropy() {
        jet_crypto_entropy_set_test_provider(|_| JetCryptoEntropyStep::Failed);
    }

    #[test]
    fn every_bridge_entropy_consumer_fails_without_output() {
        let key = vec![7u8; 32];
        let plaintext = b"secret".to_vec();

        fail_entropy();
        assert_eq!(
            jet_crypto_seal_impl(&key, &plaintext),
            Err("the operating system could not provide cryptographic randomness".to_string())
        );
        jet_crypto_entropy_clear_test_provider();

        fail_entropy();
        assert_eq!(
            jet_crypto_file_seal_impl(&key, &plaintext),
            Err("the operating system could not provide cryptographic randomness".to_string())
        );
        jet_crypto_entropy_clear_test_provider();

        fail_entropy();
        assert_eq!(
            jet_crypto_keygen_impl(),
            Err("the operating system could not provide cryptographic randomness".to_string())
        );
        jet_crypto_entropy_clear_test_provider();

        fail_entropy();
        assert_eq!(
            jet_crypto_password_hash_impl(&"password".to_string()),
            Err("the operating system could not provide cryptographic randomness".to_string())
        );
        jet_crypto_entropy_clear_test_provider();
    }

    #[test]
    fn entropy_failure_stays_typed_until_each_public_bridge_boundary() {
        let key = vec![7u8; 32];
        let plaintext = b"secret".to_vec();

        fail_entropy();
        assert_eq!(
            jet_crypto_entropy_bytes(32),
            Err(JetCryptoError::EntropyUnavailable)
        );
        jet_crypto_entropy_clear_test_provider();

        fail_entropy();
        assert_eq!(
            seal_with_algo(&key, &plaintext, ALGO_CHACHA20),
            Err(JetCryptoError::EntropyUnavailable)
        );
        jet_crypto_entropy_clear_test_provider();

        fail_entropy();
        assert_eq!(
            crypto_keygen(),
            Err(JetCryptoError::EntropyUnavailable)
        );
        jet_crypto_entropy_clear_test_provider();

        fail_entropy();
        assert_eq!(
            crypto_password_hash(&"password".to_string()),
            Err(JetCryptoError::EntropyUnavailable)
        );
        jet_crypto_entropy_clear_test_provider();
    }

    #[test]
    fn prior_crypto_suite_still_uses_live_entropy() {
        let key = vec![9u8; 32];
        let plaintext = b"round trip".to_vec();
        let envelope = jet_crypto_seal_impl(&key, &plaintext).unwrap();
        assert_eq!(jet_crypto_open_impl(&key, &envelope).unwrap(), plaintext);

        let (seed, public) = jet_crypto_keygen_impl().unwrap();
        assert_eq!(seed.len(), 32);
        assert_eq!(public.len(), 32);

        let stored = jet_crypto_password_hash_impl(&"password".to_string()).unwrap();
        assert!(jet_crypto_password_verify_impl(
            &"password".to_string(),
            &stored
        ));
    }

    #[test]
    fn keygen_zeroizes_provider_copy_and_stack_seed() {
        let zeroized = Rc::new(RefCell::new(Vec::<Vec<u8>>::new()));
        let observed = Rc::clone(&zeroized);
        jet_crypto_entropy_set_zeroize_test_observer(move |bytes| {
            if bytes.len() == 32 {
                observed.borrow_mut().push(bytes.to_vec());
            }
        });
        let (seed, public) = jet_crypto_keygen_impl().unwrap();
        jet_crypto_entropy_clear_zeroize_test_observer();
        assert_eq!(seed.len(), 32);
        assert_eq!(public.len(), 32);
        assert!(seed.iter().any(|byte| *byte != 0));
        assert!(zeroized.borrow().len() >= 2);
        assert!(zeroized
            .borrow()
            .iter()
            .all(|snapshot| snapshot == &vec![0; 32]));
    }

    #[test]
    fn typed_recipient_envelopes_are_canonical_and_fail_closed() {
        let alice = jet_crypto_x25519_generate_impl().unwrap();
        let bob = jet_crypto_x25519_generate_impl().unwrap();
        let wrong = jet_crypto_x25519_generate_impl().unwrap();
        let plain = b"purpose-bound payload".to_vec();
        let aad = b"tenant-7".to_vec();
        let sealed = jet_crypto_seal_typed_impl(
            vec![jet_crypto_x25519_public_typed_impl(&bob), jet_crypto_x25519_public_typed_impl(&alice)],
            &plain,
            &aad,
        ).unwrap();
        let bytes = jet_crypto_sealed_bytes_impl(&sealed);
        assert_eq!(&bytes[..4], b"JETV");
        assert_eq!(jet_crypto_open_typed_impl(&alice, jet_crypto_sealed_from_bytes_impl(bytes.clone()).unwrap(), &aad).unwrap(), plain);
        assert_eq!(jet_crypto_open_typed_impl(&wrong, jet_crypto_sealed_from_bytes_impl(bytes.clone()).unwrap(), &aad), Err(JetCryptoError::OpenFailed));
        assert_eq!(jet_crypto_open_typed_impl(&alice, jet_crypto_sealed_from_bytes_impl(bytes.clone()).unwrap(), &b"wrong".to_vec()), Err(JetCryptoError::OpenFailed));
        let mut tampered = bytes; *tampered.last_mut().unwrap() ^= 1;
        assert_eq!(jet_crypto_open_typed_impl(&alice, jet_crypto_sealed_from_bytes_impl(tampered).unwrap(), &aad), Err(JetCryptoError::OpenFailed));
    }

    #[test]
    fn typed_secret_wrap_sign_kdf_and_password_paths_preserve_roles() {
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        let secret = jet_crypto_secret_from_text_impl("do not print".to_string());
        let wrapped = jet_crypto_wrap_typed_impl(&secret, jet_crypto_x25519_public_typed_impl(&recipient)).unwrap();
        let bytes = jet_crypto_wrapped_bytes_impl(&wrapped);
        assert_eq!(&bytes[..4], b"JETW");
        let unwrapped = jet_crypto_unwrap_typed_impl(&recipient, jet_crypto_wrapped_from_bytes_impl(bytes).unwrap()).unwrap();
        assert!(jet_crypto_constant_time_secret_impl(&secret, &unwrapped));
        let derived = jet_crypto_hkdf_typed_impl(&secret, &vec![], &b"domain".to_vec(), 32).unwrap();
        assert!(!jet_crypto_constant_time_secret_impl(&secret, &derived));
        let signing = jet_crypto_signing_generate_impl().unwrap();
        let message = b"release".to_vec();
        let signature = jet_crypto_sign_typed_impl(&signing, &message).unwrap();
        assert!(jet_crypto_verify_typed_impl(jet_crypto_signing_public_impl(&signing), &message, signature).unwrap());
        let stored = jet_crypto_password_hash_typed_impl(&secret).unwrap();
        assert!(jet_crypto_password_verify_typed_impl(&secret, stored).unwrap());
        let wrong = jet_crypto_secret_from_text_impl("wrong".to_string());
        let stored = jet_crypto_password_hash_typed_impl(&secret).unwrap();
        assert!(!jet_crypto_password_verify_typed_impl(&wrong, stored).unwrap());
    }
}
