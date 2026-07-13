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
            Err(JetCryptoError::Unavailable)
        );
        jet_crypto_entropy_clear_test_provider();

        fail_entropy();
        assert_eq!(
            seal_with_algo(&key, &plaintext, ALGO_CHACHA20),
            Err(JetCryptoError::Unavailable)
        );
        jet_crypto_entropy_clear_test_provider();

        fail_entropy();
        assert_eq!(
            crypto_keygen(),
            Err(JetCryptoError::Unavailable)
        );
        jet_crypto_entropy_clear_test_provider();

        fail_entropy();
        assert_eq!(
            crypto_password_hash(&"password".to_string()),
            Err(JetCryptoError::Unavailable)
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
}
