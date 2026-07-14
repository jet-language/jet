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

    fn decode_hex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digit = |byte: u8| match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    _ => panic!("invalid test hex"),
                };
                digit(pair[0]) << 4 | digit(pair[1])
            })
            .collect()
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
        assert!(jet_crypto_password_verify_typed_impl(&secret, &stored).unwrap());
        let wrong = jet_crypto_secret_from_text_impl("wrong".to_string());
        let stored = jet_crypto_password_hash_typed_impl(&secret).unwrap();
        assert!(!jet_crypto_password_verify_typed_impl(&wrong, &stored).unwrap());
    }

    #[test]
    fn expert_surface_matches_reference_vectors_and_bounds() {
        let key = vec![0x80; 32];
        let xnonce = vec![0x24; 24];
        let anonce = vec![0x12; 12];
        let message = b"expert crypto".to_vec();
        let aad = b"tenant".to_vec();
        let xsealed = jet_crypto_expert_xchacha20poly1305_seal_impl(&key, &xnonce, &message, &aad).unwrap();
        assert_eq!(jet_crypto_expert_xchacha20poly1305_open_impl(&key, &xnonce, &xsealed, &aad).unwrap(), message);
        let asealed = jet_crypto_expert_aes256gcm_seal_impl(&key, &anonce, &message, &aad).unwrap();
        assert_eq!(jet_crypto_expert_aes256gcm_open_impl(&key, &anonce, &asealed, &aad).unwrap(), message);
        assert_eq!(jet_crypto_expert_xchacha20poly1305_open_impl(&key, &xnonce, &xsealed, &b"wrong".to_vec()), Err(JetCryptoError::OpenFailed));
        assert!(matches!(jet_crypto_expert_aes256gcm_seal_impl(&key, &xnonce, &message, &aad), Err(JetCryptoError::InvalidLength { parameter: "nonce", .. })));

        // RFC 8032 test vector 1.
        let seed = decode_hex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
        let public = decode_hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        let expected = decode_hex("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b");
        let signature = jet_crypto_expert_ed25519_sign_impl(&seed, &vec![]).unwrap();
        assert_eq!(jet_crypto_signature_bytes_impl(&signature), expected);
        assert!(jet_crypto_expert_ed25519_verify_strict_impl(&public, &vec![], &expected).unwrap());

        // RFC 7748 X25519 Alice/Bob vector.
        let alice_secret = decode_hex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let bob_public = decode_hex("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f");
        let expected_shared = decode_hex("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742");
        let shared = jet_crypto_expert_x25519_impl(&alice_secret, &bob_public, true).unwrap();
        assert!(jet_crypto_constant_time_secret_impl(&shared, &jet_crypto_secret_from_bytes_impl(expected_shared)));
        assert!(matches!(jet_crypto_expert_x25519_impl(&alice_secret, &vec![0; 32], true), Err(JetCryptoError::NonContributoryKey)));

        // RFC 5869 test case 1.
        let ikm = vec![0x0b; 22];
        let salt = decode_hex("000102030405060708090a0b0c");
        let info = decode_hex("f0f1f2f3f4f5f6f7f8f9");
        let expected_okm = decode_hex("3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865");
        let okm = jet_crypto_expert_hkdf_sha256_impl(&ikm, &salt, &info, 42).unwrap();
        assert!(jet_crypto_constant_time_secret_impl(&okm, &jet_crypto_secret_from_bytes_impl(expected_okm)));
        assert!(matches!(jet_crypto_expert_hkdf_sha256_impl(&ikm, &salt, &info, 8161), Err(JetCryptoError::OutputLength { .. })));

        let password = jet_crypto_secret_from_text_impl("password".to_string());
        let derived = jet_crypto_expert_argon2id_impl(&password, &b"12345678".to_vec(), 8192, 1, 1, 32).unwrap();
        assert_eq!(jet_crypto_expert_secret_bytes_impl(&derived).len(), 32);
        assert!(matches!(jet_crypto_expert_argon2id_impl(&password, &b"short".to_vec(), 8192, 1, 1, 32), Err(JetCryptoError::InvalidLength { parameter: "salt", .. })));
    }

    #[test]
    fn bech32m_recipient_parser_rejects_hostile_noncanonical_text() {
        let key = JetX25519PublicKey([0; 32]);
        let canonical = jet_crypto_x25519_public_text_impl(&key);
        assert_eq!(canonical.len(), 68);
        assert_eq!(jet_crypto_x25519_public_from_text_impl(canonical.clone()).unwrap().0, [0; 32]);
        let mut checksum = canonical.clone().into_bytes();
        let replacement = if checksum.last() == Some(&b'q') { b'p' } else { b'q' };
        *checksum.last_mut().unwrap() = replacement;
        for hostile in [
            canonical.to_uppercase(),
            canonical[..67].to_string(),
            format!("{canonical}q"),
            canonical.replacen("jetx25519", "jetx25518", 1),
            String::from_utf8(checksum).unwrap(),
            format!("jetx255191{}", "q".repeat(58)),
        ] {
            assert!(jet_crypto_x25519_public_from_text_impl(hostile).is_err());
        }
    }

    #[test]
    fn every_secret_bearing_nominal_zeroizes_on_drop() {
        let observed = Rc::new(RefCell::new(Vec::<Vec<u8>>::new()));
        let snapshots = Rc::clone(&observed);
        jet_crypto_set_zeroize_test_observer(move |bytes| {
            snapshots.borrow_mut().push(bytes.to_vec());
        });
        drop(Secret(vec![1; 17]));
        drop(JetSigningKey(vec![2; 18]));
        drop(JetX25519SecretKey(vec![3; 19]));
        drop(JetSharedSecret(vec![4; 20]));
        jet_crypto_clear_zeroize_test_observer();
        assert_eq!(
            *observed.borrow(),
            vec![vec![0; 17], vec![0; 18], vec![0; 19], vec![0; 20]]
        );
    }
}
