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

    #[cfg(target_os = "linux")]
    enum PublishRace {
        SwapParent { at: &'static str, parent: std::path::PathBuf, moved: std::path::PathBuf },
        CreateDestination { at: &'static str, destination: std::path::PathBuf },
        ReplaceCandidate { at: &'static str, parent: std::path::PathBuf, prefix: &'static str },
        AppendFile { at: &'static str, path: std::path::PathBuf },
    }

    #[cfg(target_os = "linux")]
    thread_local! {
        static PUBLISH_RACE: RefCell<Option<PublishRace>> = const { RefCell::new(None) };
        static CANCEL_BOUNDARY: RefCell<Option<&'static str>> = const { RefCell::new(None) };
        static CANCEL_NOW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static OVERSIZE_BOUNDARY_HIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    #[cfg(target_os = "linux")]
    fn publication_race(created: &'static str) {
        PUBLISH_RACE.with(|slot| {
            let mut slot = slot.borrow_mut();
            let ready = match slot.as_ref() {
                Some(PublishRace::SwapParent { at, .. }
                    | PublishRace::CreateDestination { at, .. }
                    | PublishRace::ReplaceCandidate { at, .. }
                    | PublishRace::AppendFile { at, .. }) => *at == created,
                None => false,
            };
            if !ready { return; }
            match slot.take().unwrap() {
                PublishRace::SwapParent { parent, moved, .. } => {
                    std::fs::rename(&parent, &moved).unwrap();
                    std::fs::create_dir(&parent).unwrap();
                }
                PublishRace::CreateDestination { destination, .. } => {
                    std::fs::write(destination, b"racer").unwrap();
                }
                PublishRace::ReplaceCandidate { parent, prefix, .. } => {
                    assert!(std::fs::read_dir(&parent).unwrap().all(|entry| {
                        !entry.unwrap().file_name().to_string_lossy().starts_with(&format!(".{prefix}-"))
                    }));
                    let candidate = parent.join(format!(".{prefix}-attacker"));
                    let moved = parent.join(format!(".{prefix}-attacker-moved"));
                    std::fs::write(&candidate, b"attacker-original").unwrap();
                    std::fs::rename(&candidate, &moved).unwrap();
                    std::fs::write(&candidate, b"attacker-replacement").unwrap();
                }
                PublishRace::AppendFile { path, .. } => {
                    use std::io::Write;
                    std::fs::OpenOptions::new().append(true).open(path).unwrap()
                        .write_all(b"trailing byte").unwrap();
                }
            }
        })
    }

    #[cfg(target_os = "linux")]
    fn arm_publish_race(action: PublishRace) {
        PUBLISH_RACE.with(|slot| *slot.borrow_mut() = Some(action));
        jet_crypto_set_file_boundary_test_observer(publication_race);
    }

    #[cfg(target_os = "linux")]
    fn cancellation_boundary(observed: &'static str) {
        CANCEL_BOUNDARY.with(|slot| {
            if slot.borrow().is_some_and(|boundary| boundary == observed) {
                slot.borrow_mut().take();
                CANCEL_NOW.with(|cancel| cancel.set(true));
            }
        });
    }

    #[cfg(target_os = "linux")]
    fn arm_cancellation(boundary: &'static str) {
        CANCEL_BOUNDARY.with(|slot| *slot.borrow_mut() = Some(boundary));
        CANCEL_NOW.with(|cancel| cancel.set(false));
        jet_crypto_set_file_boundary_test_observer(cancellation_boundary);
    }

    #[cfg(target_os = "linux")]
    fn cancel_at_boundary() -> bool {
        CANCEL_NOW.with(|cancel| cancel.get())
    }

    #[cfg(target_os = "linux")]
    fn assert_cancellation_fired() {
        CANCEL_BOUNDARY.with(|slot| assert!(slot.borrow().is_none()));
        CANCEL_NOW.with(|cancel| assert!(cancel.get()));
    }

    #[cfg(target_os = "linux")]
    fn record_oversize_boundary(observed: &'static str) {
        if matches!(observed, "seal-stage" | "seal-output") {
            OVERSIZE_BOUNDARY_HIT.with(|hit| hit.set(true));
            CANCEL_NOW.with(|cancel| cancel.set(true));
        }
    }

    #[cfg(target_os = "linux")]
    fn arm_oversize_boundary_trap() {
        OVERSIZE_BOUNDARY_HIT.with(|hit| hit.set(false));
        CANCEL_NOW.with(|cancel| cancel.set(false));
        jet_crypto_set_file_boundary_test_observer(record_oversize_boundary);
    }

    #[cfg(target_os = "linux")]
    fn arm_short_io(boundary: &'static str) {
        jet_crypto_set_file_io_test_fault(boundary, JetcIoTestFault::Short(7));
    }

    #[cfg(target_os = "linux")]
    fn arm_zero_progress(boundary: &'static str) {
        jet_crypto_set_file_io_test_fault(boundary, JetcIoTestFault::Short(0));
    }

    #[cfg(target_os = "linux")]
    fn arm_eof_after(boundary: &'static str, bytes: usize) {
        jet_crypto_set_file_io_test_fault(boundary, JetcIoTestFault::EofAfter(bytes));
    }

    #[cfg(target_os = "linux")]
    fn arm_io_error(boundary: &'static str, code: i32) {
        jet_crypto_set_file_io_test_fault(boundary, JetcIoTestFault::Error(code));
    }

    #[cfg(target_os = "linux")]
    fn clear_observed_io_fault() {
        assert!(jet_crypto_file_io_test_hits() > 0);
        jet_crypto_clear_file_io_test_fault();
    }

    #[cfg(target_os = "linux")]
    fn replace_candidate_at(at: &'static str, parent: &std::path::Path, prefix: &'static str) {
        arm_publish_race(PublishRace::ReplaceCandidate {
            at,
            parent: parent.to_path_buf(),
            prefix,
        });
    }

    #[cfg(target_os = "linux")]
    fn assert_attacker_candidates(parent: &std::path::Path, prefix: &str) {
        assert_eq!(std::fs::read(parent.join(format!(".{prefix}-attacker"))).unwrap(), b"attacker-replacement");
        assert_eq!(std::fs::read(parent.join(format!(".{prefix}-attacker-moved"))).unwrap(), b"attacker-original");
    }

    #[cfg(target_os = "linux")]
    fn assert_not_attacker(path: &std::path::Path) {
        let bytes = std::fs::read(path).unwrap();
        assert_ne!(bytes, b"attacker-original");
        assert_ne!(bytes, b"attacker-replacement");
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

    fn file_error_tag(error: &JetFileCryptoError) -> &'static str {
        match error {
            JetFileCryptoError::OpenFailed => "open-failed",
            JetFileCryptoError::SealFailed(_) => "seal-failed",
            JetFileCryptoError::SourceIo => "source-io",
            JetFileCryptoError::DestinationExists => "destination-exists",
            JetFileCryptoError::DestinationIo => "destination-io",
            JetFileCryptoError::Cancelled => "internal-cancelled",
        }
    }

    #[test]
    fn file_crypto_error_projection_is_exhaustive_and_redacted() {
        let public = [
            JetFileCryptoError::OpenFailed,
            JetFileCryptoError::SealFailed(JetCryptoError::EntropyUnavailable),
            JetFileCryptoError::SourceIo,
            JetFileCryptoError::DestinationExists,
            JetFileCryptoError::DestinationIo,
        ];
        assert_eq!(
            public.iter().map(file_error_tag).collect::<Vec<_>>(),
            ["open-failed", "seal-failed", "source-io", "destination-exists", "destination-io"]
        );
        assert_eq!(file_error_tag(&JetFileCryptoError::Cancelled), "internal-cancelled");
        let rendered = public.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(
            rendered,
            [
                "encrypted file could not be opened",
                "encrypted file seal failed: the operating system could not provide cryptographic randomness",
                "encrypted file source I/O failed",
                "encrypted file destination already exists",
                "encrypted file destination I/O failed",
            ]
        );
        let rendered = rendered.join("\n");
        for forbidden in ["hunter2", "plaintext sentinel", "ciphertext sentinel", "/home/nate"] {
            assert!(!rendered.contains(forbidden), "file error leaked `{forbidden}`: {rendered}");
        }
    }

    #[test]
    fn every_bridge_entropy_consumer_fails_without_output() {
        let plaintext = b"secret".to_vec();

        fail_entropy();
        assert!(matches!(
            jet_crypto_seal_typed_impl(vec![JetX25519PublicKey([7; 32])], &plaintext, &vec![]),
            Err(JetCryptoError::EntropyUnavailable)
        ));
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
        let plaintext = b"secret".to_vec();

        fail_entropy();
        assert_eq!(
            jet_crypto_entropy_bytes(32),
            Err(JetCryptoError::EntropyUnavailable)
        );
        jet_crypto_entropy_clear_test_provider();

        fail_entropy();
        assert!(matches!(
            jet_crypto_seal_typed_impl(vec![JetX25519PublicKey([7; 32])], &plaintext, &vec![]),
            Err(JetCryptoError::EntropyUnavailable)
        ));
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
        let plaintext = b"round trip".to_vec();
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        let envelope = jet_crypto_seal_typed_impl(
            vec![jet_crypto_x25519_public_typed_impl(&recipient)],
            &plaintext,
            &vec![],
        ).unwrap();
        assert_eq!(jet_crypto_open_typed_impl(&recipient, envelope, &vec![]).unwrap(), plaintext);

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
        let mut unsupported_version=bytes.clone();unsupported_version[4]=2;
        assert!(matches!(jet_crypto_sealed_from_bytes_impl(unsupported_version),Err(JetCryptoError::UnsupportedVersion{operation:"Sealed.from_bytes",version:2})));
        let mut unsupported_algorithm=bytes.clone();unsupported_algorithm[5]=3;
        match jet_crypto_sealed_from_bytes_impl(unsupported_algorithm){Err(JetCryptoError::UnsupportedAlgorithm{operation,algorithm})=>{assert_eq!(operation,"Sealed.from_bytes");assert_eq!(algorithm,"3")},_=>panic!("JETV suite id must remain public")}
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
        assert_eq!(
            jet_crypto_hkdf_typed_impl(&secret, &vec![], &vec![], -1).err().unwrap().to_string(),
            "hkdf_sha256: output length must be 0..8160; got -1"
        );
        let signing = jet_crypto_signing_generate_impl().unwrap();
        let message = b"release".to_vec();
        let signature = jet_crypto_sign_typed_impl(&signing, &message).unwrap();
        assert!(jet_crypto_verify_typed_impl(jet_crypto_signing_public_impl(&signing), &message, signature).unwrap());
        let stored = jet_crypto_password_hash_typed_impl(&secret).unwrap();
        assert!(jet_crypto_password_verify_typed_impl(&secret, &stored).unwrap());
        let weak_memory = stored.0.replacen("m=65536", "m=4096", 1);
        assert!(matches!(
            jet_crypto_password_parse_impl(weak_memory),
            Err(JetCryptoError::PasswordPolicy { .. })
        ));
        let wrong_algorithm=JetPasswordHash(stored.0.replacen("$argon2id$","$argon2i$",1));
        assert_eq!(jet_crypto_password_verify_typed_impl(&secret,&wrong_algorithm),Err(JetCryptoError::UnsupportedAlgorithm{operation:"password_verify",algorithm:"argon2i".to_string()}));
        match jet_crypto_password_parse_impl(wrong_algorithm.0) { Err(JetCryptoError::UnsupportedAlgorithm{operation,algorithm}) => { assert_eq!(operation,"PasswordHash.parse"); assert_eq!(algorithm,"argon2i"); }, _ => panic!("argon2i must remain a typed unsupported algorithm") }
        let wrong_algorithm=JetPasswordHash(stored.0.replacen("$argon2id$","$argon2d$",1));
        assert_eq!(jet_crypto_password_verify_typed_impl(&secret,&wrong_algorithm),Err(JetCryptoError::UnsupportedAlgorithm{operation:"password_verify",algorithm:"argon2d".to_string()}));
        let unknown_algorithm=JetPasswordHash(stored.0.replacen("$argon2id$","$argon2x$",1));
        assert_eq!(jet_crypto_password_verify_typed_impl(&secret,&unknown_algorithm),Err(JetCryptoError::UnsupportedAlgorithm{operation:"password_verify",algorithm:"argon2x".to_string()}));
        let wrong_version=JetPasswordHash(stored.0.replacen("$v=19$","$v=16$",1));
        assert_eq!(jet_crypto_password_verify_typed_impl(&secret,&wrong_version),Err(JetCryptoError::UnsupportedVersion{operation:"password_verify",version:16}));
        assert!(matches!(jet_crypto_password_parse_impl(wrong_version.0),Err(JetCryptoError::UnsupportedVersion{operation:"PasswordHash.parse",version:16})));
        let wide_version=JetPasswordHash(stored.0.replacen("$v=19$","$v=300$",1));
        assert_eq!(jet_crypto_password_verify_typed_impl(&secret,&wide_version),Err(JetCryptoError::UnsupportedVersion{operation:"password_verify",version:300}));
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
        assert_eq!(
            jet_crypto_expert_hkdf_sha256_impl(&ikm, &salt, &info, -1).err().unwrap().to_string(),
            "expert.hkdf_sha256: output length must be 0..8160; got -1"
        );

        let password = jet_crypto_secret_from_text_impl("password".to_string());
        let derived = jet_crypto_expert_argon2id_impl(&password, &b"12345678".to_vec(), 8192, 1, 1, 32).unwrap();
        assert_eq!(jet_crypto_expert_secret_bytes_impl(&derived).len(), 32);
        assert!(matches!(jet_crypto_expert_argon2id_impl(&password, &b"short".to_vec(), 8192, 1, 1, 32), Err(JetCryptoError::InvalidLength { parameter: "salt", .. })));
        assert_eq!(
            jet_crypto_expert_argon2id_impl(&password, &b"12345678".to_vec(), 8192, 1, 1, -1).err().unwrap().to_string(),
            "expert.argon2id: output length must be 16..64; got -1"
        );
    }

    #[test]
    fn jetc_v1_expert_open_accepts_only_the_pinned_grammar() {
        let source = include_str!("../../crates/jet-pkg-model/src/Prelude/Crypto.rs");
        for retired in [
            "fn seal_with_algo(",
            "fn open_envelope(",
            "fn jet_crypto_seal_impl(",
            "fn jet_crypto_open_impl(",
            "fn jet_crypto_seal_algo_impl(",
        ] {
            assert!(!source.contains(retired), "retired JETC v1 path remains: {retired}");
        }
        assert_eq!(source.matches("pub fn jet_crypto_expert_open_v1_impl(").count(), 1);
        let key = (0u8..32).collect::<Vec<_>>();
        let nonce = (0u8..12).collect::<Vec<_>>();
        let plaintext = b"historical JETC v1".to_vec();

        for (algorithm, ciphertext) in [
            (1u8, ChaCha20Poly1305::new_from_slice(&key).unwrap().encrypt(ChaNonce::from_slice(&nonce), plaintext.as_slice()).map_err(|_| JetCryptoError::OpenFailed)),
            (2u8, jet_crypto_expert_aes256gcm_seal_impl(&key, &nonce, &plaintext, &vec![])),
        ] {
            let ciphertext = ciphertext.expect("pinned v1 cipher");
            let mut envelope = b"JETC".to_vec();
            envelope.extend_from_slice(&[1, algorithm]);
            envelope.extend_from_slice(&nonce);
            envelope.extend_from_slice(&ciphertext);
            assert_eq!(hex_encode(&envelope), match algorithm {
                1 => "4a4554430101000102030405060708090a0be1927b744665cc23d6ef1fb9dd494d43bf41ffb4f45df6e23cf92a743b8c2c35892f",
                _ => "4a4554430102000102030405060708090a0b2f6ba56faa97ab78ec2db7c1f4bd3b4df5e703347885e9b8d1d5780ecb37c61ac49c",
            });
            assert_eq!(jet_crypto_expert_open_v1_impl(&key, &envelope), Ok(plaintext.clone()));

            for hostile in [
                { let mut bytes = envelope.clone(); bytes[0] ^= 1; bytes },
                { let mut bytes = envelope.clone(); bytes[4] = 2; bytes },
                { let mut bytes = envelope.clone(); bytes[5] = 3; bytes },
                { let mut bytes = envelope.clone(); *bytes.last_mut().unwrap() ^= 1; bytes },
                { let mut bytes = envelope.clone(); bytes.push(0); bytes },
            ] {
                assert_eq!(jet_crypto_expert_open_v1_impl(&key, &hostile), Err(JetCryptoError::OpenFailed));
            }
        }

        assert_eq!(jet_crypto_expert_open_v1_impl(&vec![0; 31], &vec![0; 34]), Err(JetCryptoError::OpenFailed));
        assert_eq!(jet_crypto_expert_open_v1_impl(&key, &vec![0; 33]), Err(JetCryptoError::OpenFailed));
    }

    #[test]
    fn jetc_v1_migration_reseals_to_verified_v2_without_touching_source() {
        let root = test_dir("migration");
        let key = (0u8..32).collect::<Vec<_>>();
        let nonce = (0u8..12).collect::<Vec<_>>();
        let plaintext = b"historical migration payload".to_vec();
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        let recipients = vec![jet_crypto_x25519_public_typed_impl(&recipient)];

        for (algorithm, ciphertext) in [
            (1u8, ChaCha20Poly1305::new_from_slice(&key).unwrap().encrypt(ChaNonce::from_slice(&nonce), plaintext.as_slice()).map_err(|_| JetCryptoError::OpenFailed)),
            (2u8, jet_crypto_expert_aes256gcm_seal_impl(&key, &nonce, &plaintext, &vec![])),
        ] {
            let mut v1 = b"JETC".to_vec();
            v1.extend_from_slice(&[1, algorithm]);
            v1.extend_from_slice(&nonce);
            v1.extend_from_slice(&ciphertext.unwrap());
            let source = root.join(format!("source-{algorithm}.jetc"));
            let migrated = root.join(format!("migrated-{algorithm}.jetc"));
            let restored = root.join(format!("restored-{algorithm}.bin"));
            std::fs::write(&source, &v1).unwrap();

            jet_crypto_expert_migrate_v1_impl(
                &key,
                &source.to_string_lossy().into_owned(),
                recipients.clone(),
                &migrated.to_string_lossy().into_owned(),
                never_cancelled,
            ).unwrap();
            assert_eq!(std::fs::read(&source).unwrap(), v1);
            assert_eq!(&std::fs::read(&migrated).unwrap()[..8], b"JETC\x02\x01\x01\x00");
            jet_crypto_file_open_impl(
                &recipient,
                &migrated.to_string_lossy().into_owned(),
                &restored.to_string_lossy().into_owned(),
                never_cancelled,
            ).unwrap();
            assert_eq!(std::fs::read(restored).unwrap(), plaintext);
        }

        let source = root.join("source-1.jetc");
        let original = std::fs::read(&source).unwrap();
        let existing = root.join("existing.jetc");
        std::fs::write(&existing, b"keep").unwrap();
        assert_eq!(
            jet_crypto_expert_migrate_v1_impl(
                &key,
                &source.to_string_lossy().into_owned(),
                recipients.clone(),
                &existing.to_string_lossy().into_owned(),
                never_cancelled,
            ),
            Err(JetFileCryptoError::DestinationExists)
        );
        assert_eq!(std::fs::read(&existing).unwrap(), b"keep");
        assert_eq!(std::fs::read(&source).unwrap(), original);
        assert_eq!(
            jet_crypto_expert_migrate_v1_impl(
                &key,
                &source.to_string_lossy().into_owned(),
                recipients.clone(),
                &source.to_string_lossy().into_owned(),
                never_cancelled,
            ),
            Err(JetFileCryptoError::DestinationExists)
        );
        assert_eq!(std::fs::read(&source).unwrap(), original);

        let wrong_key_output = root.join("wrong-key.jetc");
        assert_eq!(
            jet_crypto_expert_migrate_v1_impl(
                &vec![9; 32],
                &source.to_string_lossy().into_owned(),
                recipients.clone(),
                &wrong_key_output.to_string_lossy().into_owned(),
                never_cancelled,
            ),
            Err(JetFileCryptoError::OpenFailed)
        );
        assert!(!wrong_key_output.exists());

        let hostile = root.join("hostile.jetc");
        let absent = root.join("absent.jetc");
        std::fs::write(&hostile, b"JETC\x01\x01bad").unwrap();
        assert_eq!(
            jet_crypto_expert_migrate_v1_impl(
                &key,
                &hostile.to_string_lossy().into_owned(),
                recipients,
                &absent.to_string_lossy().into_owned(),
                never_cancelled,
            ),
            Err(JetFileCryptoError::OpenFailed)
        );
        assert!(!absent.exists());
        assert!(std::fs::read_dir(&root).unwrap().all(|entry| {
            !entry.unwrap().file_name().to_string_lossy().starts_with(".jetc-")
        }));
        std::fs::remove_dir_all(root).unwrap();
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

    fn never_cancelled() -> bool { false }
    fn always_cancelled() -> bool { true }

    fn test_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "jetc-v2-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn jetc_v2_streams_authenticated_chunks_and_publishes_without_overwrite() {
        let root = test_dir("roundtrip");
        let source = root.join("source.bin");
        let envelope = root.join("sealed.jetc");
        let restored = root.join("restored.bin");
        let wrong_output = root.join("wrong.bin");
        let mut plain = vec![0x5a; JETC_V2_CHUNK + 17];
        plain[0] = 1;
        plain[JETC_V2_CHUNK] = 2;
        std::fs::write(&source, &plain).unwrap();
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        let public = jet_crypto_x25519_public_typed_impl(&recipient);
        jet_crypto_file_seal_impl(
            vec![public],
            &source.to_string_lossy().into_owned(),
            &envelope.to_string_lossy().into_owned(),
            never_cancelled,
        ).unwrap();
        let encoded = std::fs::read(&envelope).unwrap();
        assert_eq!(&encoded[..8], b"JETC\x02\x01\x01\x00");
        let header_len = u32::from_le_bytes(encoded[8..12].try_into().unwrap()) as usize;
        let body_len = u64::from_le_bytes(encoded[12..20].try_into().unwrap()) as usize;
        assert_eq!(header_len, JETC_V2_HEADER_BASE + JETC_V2_STANZA + 16);
        assert_eq!(encoded.len(), 20 + header_len + body_len);
        let body = &encoded[20 + header_len..];
        assert_eq!(u32::from_le_bytes(body[..4].try_into().unwrap()), JETC_V2_CHUNK as u32);
        assert_eq!(body[4], 0);
        let final_at = 5 + JETC_V2_CHUNK + 16;
        assert_eq!(u32::from_le_bytes(body[final_at..final_at+4].try_into().unwrap()), 17);
        assert_eq!(body[final_at+4], 1);
        jet_crypto_file_open_impl(
            &recipient,
            &envelope.to_string_lossy().into_owned(),
            &restored.to_string_lossy().into_owned(),
            never_cancelled,
        ).unwrap();
        assert_eq!(std::fs::read(&restored).unwrap(), plain);

        let wrong = jet_crypto_x25519_generate_impl().unwrap();
        assert_eq!(
            jet_crypto_file_open_impl(
                &wrong,
                &envelope.to_string_lossy().into_owned(),
                &wrong_output.to_string_lossy().into_owned(),
                never_cancelled,
            ),
            Err(JetFileCryptoError::OpenFailed)
        );
        assert!(!wrong_output.exists());

        std::fs::write(&restored, b"do not replace").unwrap();
        assert_eq!(
            jet_crypto_file_open_impl(
                &recipient,
                &envelope.to_string_lossy().into_owned(),
                &restored.to_string_lossy().into_owned(),
                never_cancelled,
            ),
            Err(JetFileCryptoError::DestinationExists)
        );
        assert_eq!(std::fs::read(&restored).unwrap(), b"do not replace");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn jetc_v2_record_boundaries_are_canonical() {
        let root = test_dir("record-boundaries");
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        let public = jet_crypto_x25519_public_typed_impl(&recipient);
        for (name, size, expected) in [
            ("empty", 0, vec![(0, 1)]),
            ("chunk-minus-one", JETC_V2_CHUNK - 1, vec![((JETC_V2_CHUNK - 1) as u32, 1)]),
            ("chunk", JETC_V2_CHUNK, vec![(JETC_V2_CHUNK as u32, 0), (0, 1)]),
            ("chunk-plus-one", JETC_V2_CHUNK + 1, vec![(JETC_V2_CHUNK as u32, 0), (1, 1)]),
        ] {
            let source = root.join(format!("{name}.bin"));
            let envelope = root.join(format!("{name}.jetc"));
            let restored = root.join(format!("{name}.out"));
            let plaintext = (0..size).map(|index| (index % 251) as u8).collect::<Vec<_>>();
            std::fs::write(&source, &plaintext).unwrap();
            jet_crypto_file_seal_impl(
                vec![public.clone()],
                &source.to_string_lossy().into_owned(),
                &envelope.to_string_lossy().into_owned(),
                never_cancelled,
            ).unwrap();
            let encoded = std::fs::read(&envelope).unwrap();
            let header_len = u32::from_le_bytes(encoded[8..12].try_into().unwrap()) as usize;
            let body_len = u64::from_le_bytes(encoded[12..20].try_into().unwrap());
            assert_eq!(jetc_v2_plain_len(body_len), Some(size as u64));
            let mut at = 20 + header_len;
            let mut records = Vec::new();
            while at < encoded.len() {
                let length = u32::from_le_bytes(encoded[at..at + 4].try_into().unwrap());
                let flags = encoded[at + 4];
                assert!(jetc_v2_record_shape_valid(length as usize, flags));
                records.push((length, flags));
                at += 5 + length as usize + 16;
            }
            assert_eq!(at, encoded.len());
            assert_eq!(records, expected);
            jet_crypto_file_open_impl(
                &recipient,
                &envelope.to_string_lossy().into_owned(),
                &restored.to_string_lossy().into_owned(),
                never_cancelled,
            ).unwrap();
            assert_eq!(std::fs::read(&restored).unwrap(), plaintext);
            assert_eq!(std::fs::read(&source).unwrap(), plaintext);
        }
        assert!(std::fs::read_dir(&root).unwrap().all(|entry| {
            !entry.unwrap().file_name().to_string_lossy().starts_with(".jetc-")
        }));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn jetc_v2_rechecks_staged_snapshot_hash_and_eof_before_final() {
        use std::io::{Seek, SeekFrom, Write};

        let root = test_dir("stage-recheck");
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        let expected = b"verified bytes";
        for (name, staged, distinct_identity) in [
            ("identity", expected.as_slice(), true),
            ("changed", b"tampered bytes".as_slice(), false),
            ("trailing", b"verified bytesattacker trailing bytes".as_slice(), false),
        ] {
            let destination = root.join(format!("{name}.jetc"));
            let parent = hold_destination_parent(&destination).unwrap();
            let mut stage = create_unlinked_stage(&parent).unwrap();
            stage.write_all(staged).unwrap();
            stage.sync_all().unwrap();
            let identity_stage = distinct_identity.then(|| {
                let mut file = create_unlinked_stage(&parent).unwrap();
                file.write_all(expected).unwrap();
                file.sync_all().unwrap();
                file
            });
            let metadata = identity_stage.as_ref().unwrap_or(&stage).metadata().unwrap();
            stage.seek(SeekFrom::Start(0)).unwrap();
            assert_eq!(
                seal_jetc_v2_from_snapshot(
                    vec![jet_crypto_x25519_public_typed_impl(&recipient)],
                    JetcSnapshot {
                        file: stage,
                        length: expected.len() as u64,
                        hash: Sha256::digest(expected).into(),
                        metadata,
                    },
                    &parent,
                    &destination.to_string_lossy().into_owned(),
                    never_cancelled,
                    false,
                ),
                Err(JetFileCryptoError::SourceIo),
                "stage {name}",
            );
            assert!(!destination.exists());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn jetc_v2_rejects_oversize_source_before_staging() {
        use std::io::{Read, Seek, SeekFrom, Write};

        let root = test_dir("oversize-source");
        let source = root.join("oversize.bin");
        let destination = root.join("oversize.jetc");
        let mut sparse = std::fs::File::create(&source).unwrap();
        sparse.write_all(b"oversize-source").unwrap();
        sparse.seek(SeekFrom::Start(JETC_V2_MAX_PLAINTEXT)).unwrap();
        sparse.write_all(&[0x7f]).unwrap();
        drop(sparse);
        assert_eq!(std::fs::metadata(&source).unwrap().len(), JETC_V2_MAX_PLAINTEXT + 1);

        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        arm_oversize_boundary_trap();
        assert_eq!(jet_crypto_file_seal_impl(
            vec![jet_crypto_x25519_public_typed_impl(&recipient)],
            &source.to_string_lossy().into_owned(),
            &destination.to_string_lossy().into_owned(),
            cancel_at_boundary,
        ), Err(JetFileCryptoError::SourceIo));
        jet_crypto_clear_file_boundary_test_observer();
        OVERSIZE_BOUNDARY_HIT.with(|hit| assert!(!hit.get()));
        CANCEL_NOW.with(|cancel| assert!(!cancel.get()));
        assert!(!destination.exists());
        assert_eq!(std::fs::metadata(&source).unwrap().len(), JETC_V2_MAX_PLAINTEXT + 1);
        let mut preserved = std::fs::File::open(&source).unwrap();
        let mut prefix = [0u8; 15];
        preserved.read_exact(&mut prefix).unwrap();
        assert_eq!(&prefix, b"oversize-source");
        preserved.seek(SeekFrom::Start(JETC_V2_MAX_PLAINTEXT)).unwrap();
        let mut marker = [0u8; 1];
        preserved.read_exact(&mut marker).unwrap();
        assert_eq!(marker, [0x7f]);
        assert!(std::fs::read_dir(&root).unwrap().all(|entry| {
            !entry.unwrap().file_name().to_string_lossy().starts_with(".jetc-")
        }));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn jetc_v2_rejects_tamper_truncation_append_and_cancellation_without_output() {
        let root = test_dir("hostile");
        let source = root.join("source.bin");
        let envelope = root.join("sealed.jetc");
        std::fs::write(&source, b"authenticated file").unwrap();
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        jet_crypto_file_seal_impl(
            vec![jet_crypto_x25519_public_typed_impl(&recipient)],
            &source.to_string_lossy().into_owned(),
            &envelope.to_string_lossy().into_owned(),
            never_cancelled,
        ).unwrap();
        let canonical = std::fs::read(&envelope).unwrap();
        let header_len = u32::from_le_bytes(canonical[8..12].try_into().unwrap()) as usize;
        let body_at = 20 + header_len;
        assert_eq!(jetc_v2_body_len(0), Some(21));
        assert_eq!(jetc_v2_body_len((JETC_V2_CHUNK - 1) as u64), Some(JETC_V2_CHUNK as u64 + 20));
        assert_eq!(jetc_v2_body_len(JETC_V2_CHUNK as u64), Some(JETC_V2_CHUNK as u64 + 42));
        assert_eq!(jetc_v2_body_len(JETC_V2_CHUNK as u64 + 1), Some(JETC_V2_CHUNK as u64 + 43));
        assert_eq!(jetc_v2_body_len(JETC_V2_MAX_PLAINTEXT), Some(JETC_V2_MAX_BODY_LEN));
        assert_eq!(jetc_v2_body_len(JETC_V2_MAX_PLAINTEXT + 1), None);
        assert_eq!(jetc_v2_plain_len(20), None);
        assert_eq!(jetc_v2_plain_len(JETC_V2_CHUNK as u64 + 21), None);
        assert_eq!(jetc_v2_plain_len(JETC_V2_MAX_BODY_LEN), Some(JETC_V2_MAX_PLAINTEXT));
        assert_eq!(jetc_v2_plain_len(JETC_V2_MAX_BODY_LEN + 1), None);
        assert!(jetc_v2_record_shape_valid(JETC_V2_CHUNK, 0));
        assert!(jetc_v2_record_shape_valid(0, 1));
        assert!(jetc_v2_record_shape_valid(JETC_V2_CHUNK - 1, 1));
        assert!(!jetc_v2_record_shape_valid(JETC_V2_CHUNK - 1, 0));
        assert!(!jetc_v2_record_shape_valid(JETC_V2_CHUNK, 1));
        assert!(!jetc_v2_record_shape_valid(0, 2));
        assert!(jetc_v2_container_len_matches(
            20 + header_len as u64 + JETC_V2_MAX_BODY_LEN,
            header_len,
            JETC_V2_MAX_BODY_LEN,
        ));
        assert!(!jetc_v2_container_len_matches(
            21 + header_len as u64 + JETC_V2_MAX_BODY_LEN,
            header_len,
            JETC_V2_MAX_BODY_LEN + 1,
        ));
        let mut tampered = canonical.clone();
        *tampered.last_mut().unwrap() ^= 1;
        for (name, hostile) in [
            ("tampered", tampered),
            ("truncated", canonical[..canonical.len()-1].to_vec()),
            ("appended", { let mut b = canonical.clone(); b.push(0); b }),
            ("v1", { let mut b = canonical.clone(); b[4] = 1; b }),
            ("kind", { let mut b = canonical.clone(); b[5] = 2; b }),
            ("suite", { let mut b = canonical.clone(); b[6] = 2; b }),
            ("fixed-flags", { let mut b = canonical.clone(); b[7] = 1; b }),
            ("header-cap", { let mut b = canonical.clone(); b[8..12].copy_from_slice(&(32u32 * 1024 * 1024 + 1).to_le_bytes()); b }),
            ("body-cap", { let mut b = canonical.clone(); b[12..20].copy_from_slice(&(JETC_V2_MAX_BODY_LEN + 1).to_le_bytes()); b }),
            ("chunk-size", { let mut b = canonical.clone(); b[84..88].copy_from_slice(&1u32.to_le_bytes()); b }),
            ("recipient-zero", { let mut b = canonical.clone(); b[88..90].copy_from_slice(&0u16.to_le_bytes()); b }),
            ("recipient-cap", { let mut b = canonical.clone(); b[88..90].copy_from_slice(&257u16.to_le_bytes()); b }),
            ("metadata", { let mut b = canonical.clone(); b[90..94].copy_from_slice(&1u32.to_le_bytes()); b }),
            ("recipient-id", { let mut b = canonical.clone(); b[94] ^= 1; b }),
            ("zero-public", { let mut b = canonical.clone(); b[110..142].fill(0); b }),
            ("record-length", { let mut b = canonical.clone(); b[body_at..body_at+4].copy_from_slice(&(JETC_V2_CHUNK as u32 + 1).to_le_bytes()); b }),
            ("record-flags", { let mut b = canonical.clone(); b[body_at+4] = 2; b }),
        ] {
            let input = root.join(format!("{name}.jetc"));
            let output = root.join(format!("{name}.out"));
            std::fs::write(&input, hostile).unwrap();
            assert_eq!(
                jet_crypto_file_open_impl(
                    &recipient,
                    &input.to_string_lossy().into_owned(),
                    &output.to_string_lossy().into_owned(),
                    never_cancelled,
                ),
                Err(JetFileCryptoError::OpenFailed)
            );
            assert!(!output.exists());
        }
        let cancelled = root.join("cancelled.out");
        assert_eq!(
            jet_crypto_file_open_impl(
                &recipient,
                &envelope.to_string_lossy().into_owned(),
                &cancelled.to_string_lossy().into_owned(),
                always_cancelled,
            ),
            Err(JetFileCryptoError::Cancelled)
        );
        assert!(!cancelled.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn jetc_v2_rejects_bytes_appended_after_the_initial_length_check() {
        let root = test_dir("concurrent-append");
        let source = root.join("source.bin");
        let envelope = root.join("sealed.jetc");
        let output = root.join("restored.bin");
        std::fs::write(&source, b"authenticated file").unwrap();
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        jet_crypto_file_seal_impl(
            vec![jet_crypto_x25519_public_typed_impl(&recipient)],
            &source.to_string_lossy().into_owned(),
            &envelope.to_string_lossy().into_owned(),
            never_cancelled,
        ).unwrap();

        arm_publish_race(PublishRace::AppendFile {
            at: "open-record",
            path: envelope.clone(),
        });
        assert_eq!(
            jet_crypto_file_open_impl(
                &recipient,
                &envelope.to_string_lossy().into_owned(),
                &output.to_string_lossy().into_owned(),
                never_cancelled,
            ),
            Err(JetFileCryptoError::OpenFailed),
        );
        PUBLISH_RACE.with(|slot| assert!(slot.borrow().is_none()));
        jet_crypto_clear_file_boundary_test_observer();
        assert!(!output.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn jetc_v2_cancellation_at_stage_record_and_publish_boundaries_publishes_nothing() {
        let root = test_dir("cancellation-boundaries");
        let source = root.join("source.bin");
        let plaintext = vec![0x5a; JETC_V2_CHUNK + 1];
        std::fs::write(&source, &plaintext).unwrap();
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        let public = jet_crypto_x25519_public_typed_impl(&recipient);
        let envelope = root.join("stable.jetc");
        jet_crypto_file_seal_impl(
            vec![public.clone()],
            &source.to_string_lossy().into_owned(),
            &envelope.to_string_lossy().into_owned(),
            never_cancelled,
        ).unwrap();

        for boundary in ["seal-stage", "seal-output", "seal-record", "seal-final", "seal-before-publish"] {
            let output = root.join(format!("{boundary}.jetc"));
            arm_cancellation(boundary);
            assert_eq!(
                jet_crypto_file_seal_impl(
                    vec![public.clone()],
                    &source.to_string_lossy().into_owned(),
                    &output.to_string_lossy().into_owned(),
                    cancel_at_boundary,
                ),
                Err(JetFileCryptoError::Cancelled),
                "seal cancellation boundary {boundary}",
            );
            assert_cancellation_fired();
            assert!(!output.exists());
        }

        for boundary in ["open-output", "open-record", "open-before-publish"] {
            let output = root.join(format!("{boundary}.bin"));
            arm_cancellation(boundary);
            assert_eq!(
                jet_crypto_file_open_impl(
                    &recipient,
                    &envelope.to_string_lossy().into_owned(),
                    &output.to_string_lossy().into_owned(),
                    cancel_at_boundary,
                ),
                Err(JetFileCryptoError::Cancelled),
                "open cancellation boundary {boundary}",
            );
            assert_cancellation_fired();
            assert!(!output.exists());
        }

        let key = (0u8..32).collect::<Vec<_>>();
        let nonce = (0u8..12).collect::<Vec<_>>();
        let ciphertext = ChaCha20Poly1305::new_from_slice(&key).unwrap()
            .encrypt(ChaNonce::from_slice(&nonce), plaintext.as_slice()).unwrap();
        let mut v1 = b"JETC".to_vec();
        v1.extend_from_slice(&[1, 1]);
        v1.extend_from_slice(&nonce);
        v1.extend_from_slice(&ciphertext);
        let v1_source = root.join("historical.jetc");
        std::fs::write(&v1_source, &v1).unwrap();
        for boundary in [
            "migrate-v1-read",
            "migrate-stage",
            "migrate-output",
            "migrate-record",
            "migrate-final",
            "migrate-verify-record",
            "migrate-before-publish",
        ] {
            let output = root.join(format!("{boundary}.jetc"));
            arm_cancellation(boundary);
            assert_eq!(
                jet_crypto_expert_migrate_v1_impl(
                    &key,
                    &v1_source.to_string_lossy().into_owned(),
                    vec![public.clone()],
                    &output.to_string_lossy().into_owned(),
                    cancel_at_boundary,
                ),
                Err(JetFileCryptoError::Cancelled),
                "migration cancellation boundary {boundary}",
            );
            assert_cancellation_fired();
            assert!(!output.exists());
            assert_eq!(std::fs::read(&v1_source).unwrap(), v1);
        }

        jet_crypto_clear_file_boundary_test_observer();
        assert!(std::fs::read_dir(&root).unwrap().all(|entry| {
            !entry.unwrap().file_name().to_string_lossy().starts_with(".jetc-")
        }));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn jetc_v2_short_io_and_faults_preserve_atomic_file_contract() {
        let root = test_dir("io-faults");
        let source = root.join("source.bin");
        let plaintext = vec![0x6d; 4097];
        std::fs::write(&source, &plaintext).unwrap();
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        let public = jet_crypto_x25519_public_typed_impl(&recipient);

        for boundary in ["seal-source-read", "seal-stage-write", "seal-stage-read", "seal-source-recheck-read", "seal-output-write"] {
            let envelope = root.join(format!("short-{boundary}.jetc"));
            arm_short_io(boundary);
            jet_crypto_file_seal_impl(
                vec![public.clone()],
                &source.to_string_lossy().into_owned(),
                &envelope.to_string_lossy().into_owned(),
                never_cancelled,
            ).unwrap();
            clear_observed_io_fault();
            let restored = root.join(format!("short-{boundary}.bin"));
            jet_crypto_file_open_impl(
                &recipient,
                &envelope.to_string_lossy().into_owned(),
                &restored.to_string_lossy().into_owned(),
                never_cancelled,
            ).unwrap();
            assert_eq!(std::fs::read(restored).unwrap(), plaintext);
        }

        let stable = root.join("stable.jetc");
        jet_crypto_file_seal_impl(
            vec![public.clone()],
            &source.to_string_lossy().into_owned(),
            &stable.to_string_lossy().into_owned(),
            never_cancelled,
        ).unwrap();
        for boundary in ["open-input-read", "open-output-write"] {
            let restored = root.join(format!("short-{boundary}.bin"));
            arm_short_io(boundary);
            jet_crypto_file_open_impl(
                &recipient,
                &stable.to_string_lossy().into_owned(),
                &restored.to_string_lossy().into_owned(),
                never_cancelled,
            ).unwrap();
            clear_observed_io_fault();
            assert_eq!(std::fs::read(restored).unwrap(), plaintext);
        }

        let key = (0u8..32).collect::<Vec<_>>();
        let nonce = (0u8..12).collect::<Vec<_>>();
        let ciphertext = ChaCha20Poly1305::new_from_slice(&key).unwrap()
            .encrypt(ChaNonce::from_slice(&nonce), plaintext.as_slice()).unwrap();
        let mut v1 = b"JETC".to_vec();
        v1.extend_from_slice(&[1, 1]);
        v1.extend_from_slice(&nonce);
        v1.extend_from_slice(&ciphertext);
        let v1_source = root.join("historical.jetc");
        std::fs::write(&v1_source, &v1).unwrap();
        for boundary in ["migrate-v1-read-io", "migrate-stage-write", "migrate-stage-read", "migrate-output-write", "migrate-verify-read"] {
            let migrated = root.join(format!("short-{boundary}.jetc"));
            arm_short_io(boundary);
            jet_crypto_expert_migrate_v1_impl(
                &key,
                &v1_source.to_string_lossy().into_owned(),
                vec![public.clone()],
                &migrated.to_string_lossy().into_owned(),
                never_cancelled,
            ).unwrap();
            clear_observed_io_fault();
            assert_eq!(std::fs::read(&v1_source).unwrap(), v1);
            let restored = root.join(format!("short-{boundary}.bin"));
            jet_crypto_file_open_impl(
                &recipient,
                &migrated.to_string_lossy().into_owned(),
                &restored.to_string_lossy().into_owned(),
                never_cancelled,
            ).unwrap();
            assert_eq!(std::fs::read(restored).unwrap(), plaintext);
        }

        for boundary in ["seal-source-read", "seal-stage-read", "seal-source-recheck-read"] {
            let output = root.join(format!("zero-{boundary}.jetc"));
            arm_zero_progress(boundary);
            assert_eq!(jet_crypto_file_seal_impl(
                vec![public.clone()],
                &source.to_string_lossy().into_owned(),
                &output.to_string_lossy().into_owned(),
                never_cancelled,
            ), Err(JetFileCryptoError::SourceIo));
            clear_observed_io_fault();
            assert!(!output.exists());
            assert_eq!(std::fs::read(&source).unwrap(), plaintext);
        }

        for boundary in ["seal-stage-write", "seal-output-write"] {
            let output = root.join(format!("zero-{boundary}.jetc"));
            arm_zero_progress(boundary);
            assert_eq!(jet_crypto_file_seal_impl(
                vec![public.clone()],
                &source.to_string_lossy().into_owned(),
                &output.to_string_lossy().into_owned(),
                never_cancelled,
            ), Err(JetFileCryptoError::DestinationIo));
            clear_observed_io_fault();
            assert!(!output.exists());
            assert_eq!(std::fs::read(&source).unwrap(), plaintext);
        }

        let zero_open = root.join("zero-open-output-write.bin");
        arm_zero_progress("open-output-write");
        assert_eq!(jet_crypto_file_open_impl(
            &recipient,
            &stable.to_string_lossy().into_owned(),
            &zero_open.to_string_lossy().into_owned(),
            never_cancelled,
        ), Err(JetFileCryptoError::DestinationIo));
        clear_observed_io_fault();
        assert!(!zero_open.exists());

        let zero_migrate_read = root.join("zero-migrate-read.jetc");
        arm_zero_progress("migrate-v1-read-io");
        assert_eq!(jet_crypto_expert_migrate_v1_impl(
            &key,
            &v1_source.to_string_lossy().into_owned(),
            vec![public.clone()],
            &zero_migrate_read.to_string_lossy().into_owned(),
            never_cancelled,
        ), Err(JetFileCryptoError::SourceIo));
        clear_observed_io_fault();
        assert!(!zero_migrate_read.exists());
        assert_eq!(std::fs::read(&v1_source).unwrap(), v1);

        for boundary in ["migrate-stage-write", "migrate-output-write"] {
            let output = root.join(format!("zero-{boundary}.jetc"));
            arm_zero_progress(boundary);
            assert_eq!(jet_crypto_expert_migrate_v1_impl(
                &key,
                &v1_source.to_string_lossy().into_owned(),
                vec![public.clone()],
                &output.to_string_lossy().into_owned(),
                never_cancelled,
            ), Err(JetFileCryptoError::DestinationIo));
            clear_observed_io_fault();
            assert!(!output.exists());
            assert_eq!(std::fs::read(&v1_source).unwrap(), v1);
        }

        let mut trailing_v1 = v1.clone();
        trailing_v1.extend_from_slice(b"unread trailing bytes");
        let trailing_source = root.join("historical-trailing.jetc");
        std::fs::write(&trailing_source, &trailing_v1).unwrap();
        let trailing_output = root.join("trailing-prefix.jetc");
        arm_eof_after("migrate-v1-read-io", v1.len());
        assert_eq!(jet_crypto_expert_migrate_v1_impl(
            &key,
            &trailing_source.to_string_lossy().into_owned(),
            vec![public.clone()],
            &trailing_output.to_string_lossy().into_owned(),
            never_cancelled,
        ), Err(JetFileCryptoError::SourceIo));
        clear_observed_io_fault();
        assert!(!trailing_output.exists());
        assert_eq!(std::fs::read(&trailing_source).unwrap(), trailing_v1);

        for boundary in ["seal-source-read", "seal-output-write"] {
            let output = root.join(format!("eintr-{boundary}.jetc"));
            arm_io_error(boundary, 4);
            jet_crypto_file_seal_impl(
                vec![public.clone()],
                &source.to_string_lossy().into_owned(),
                &output.to_string_lossy().into_owned(),
                never_cancelled,
            ).unwrap();
            clear_observed_io_fault();
            let restored = root.join(format!("eintr-{boundary}.bin"));
            jet_crypto_file_open_impl(
                &recipient,
                &output.to_string_lossy().into_owned(),
                &restored.to_string_lossy().into_owned(),
                never_cancelled,
            ).unwrap();
            assert_eq!(std::fs::read(restored).unwrap(), plaintext);
        }

        for (boundary, expected) in [
            ("seal-source-read", JetFileCryptoError::SourceIo),
            ("seal-stage-write", JetFileCryptoError::DestinationIo),
            ("seal-stage-fsync", JetFileCryptoError::DestinationIo),
            ("seal-output-write", JetFileCryptoError::DestinationIo),
            ("seal-output-fsync", JetFileCryptoError::DestinationIo),
        ] {
            let output = root.join(format!("fail-{boundary}.jetc"));
            arm_io_error(boundary, 28);
            assert_eq!(jet_crypto_file_seal_impl(
                vec![public.clone()],
                &source.to_string_lossy().into_owned(),
                &output.to_string_lossy().into_owned(),
                never_cancelled,
            ), Err(expected));
            clear_observed_io_fault();
            assert!(!output.exists());
        }

        for (boundary, expected) in [
            ("open-input-read", JetFileCryptoError::OpenFailed),
            ("open-output-write", JetFileCryptoError::DestinationIo),
            ("open-output-fsync", JetFileCryptoError::DestinationIo),
        ] {
            let output = root.join(format!("fail-{boundary}.bin"));
            arm_io_error(boundary, 28);
            assert_eq!(jet_crypto_file_open_impl(
                &recipient,
                &stable.to_string_lossy().into_owned(),
                &output.to_string_lossy().into_owned(),
                never_cancelled,
            ), Err(expected));
            clear_observed_io_fault();
            assert!(!output.exists());
        }

        for boundary in [
            "migrate-v1-read-io",
            "migrate-stage-write",
            "migrate-stage-fsync",
            "migrate-output-write",
            "migrate-verify-fsync",
            "migrate-reopen-verify",
            "migrate-verify-read",
            "migrate-output-fsync",
        ] {
            let output = root.join(format!("fail-{boundary}.jetc"));
            arm_io_error(boundary, 28);
            let expected = if boundary == "migrate-v1-read-io" { JetFileCryptoError::SourceIo } else { JetFileCryptoError::DestinationIo };
            assert_eq!(jet_crypto_expert_migrate_v1_impl(
                &key,
                &v1_source.to_string_lossy().into_owned(),
                vec![public.clone()],
                &output.to_string_lossy().into_owned(),
                never_cancelled,
            ), Err(expected));
            clear_observed_io_fault();
            assert!(!output.exists());
            assert_eq!(std::fs::read(&v1_source).unwrap(), v1);
        }

        let seal_post = root.join("seal-post-install.jetc");
        arm_io_error("seal-directory-fsync", 5);
        assert_eq!(jet_crypto_file_seal_impl(
            vec![public.clone()],
            &source.to_string_lossy().into_owned(),
            &seal_post.to_string_lossy().into_owned(),
            never_cancelled,
        ), Err(JetFileCryptoError::DestinationIo));
        clear_observed_io_fault();
        assert!(seal_post.exists());
        let seal_post_restored = root.join("seal-post-install.bin");
        jet_crypto_file_open_impl(
            &recipient,
            &seal_post.to_string_lossy().into_owned(),
            &seal_post_restored.to_string_lossy().into_owned(),
            never_cancelled,
        ).unwrap();
        assert_eq!(std::fs::read(seal_post_restored).unwrap(), plaintext);

        let open_post = root.join("open-post-install.bin");
        arm_io_error("open-directory-fsync", 5);
        assert_eq!(jet_crypto_file_open_impl(
            &recipient,
            &stable.to_string_lossy().into_owned(),
            &open_post.to_string_lossy().into_owned(),
            never_cancelled,
        ), Err(JetFileCryptoError::DestinationIo));
        clear_observed_io_fault();
        assert_eq!(std::fs::read(&open_post).unwrap(), plaintext);

        let migrate_post = root.join("migrate-post-install.jetc");
        arm_io_error("migrate-directory-fsync", 5);
        assert_eq!(jet_crypto_expert_migrate_v1_impl(
            &key,
            &v1_source.to_string_lossy().into_owned(),
            vec![public.clone()],
            &migrate_post.to_string_lossy().into_owned(),
            never_cancelled,
        ), Err(JetFileCryptoError::DestinationIo));
        clear_observed_io_fault();
        assert!(migrate_post.exists());
        assert_eq!(std::fs::read(&v1_source).unwrap(), v1);
        let migrate_post_restored = root.join("migrate-post-install.bin");
        jet_crypto_file_open_impl(
            &recipient,
            &migrate_post.to_string_lossy().into_owned(),
            &migrate_post_restored.to_string_lossy().into_owned(),
            never_cancelled,
        ).unwrap();
        assert_eq!(std::fs::read(migrate_post_restored).unwrap(), plaintext);

        let winner = root.join("winner.jetc");
        std::fs::write(&winner, b"winner").unwrap();
        arm_io_error("seal-directory-fsync", 5);
        assert_eq!(jet_crypto_file_seal_impl(
            vec![public],
            &source.to_string_lossy().into_owned(),
            &winner.to_string_lossy().into_owned(),
            never_cancelled,
        ), Err(JetFileCryptoError::DestinationExists));
        assert_eq!(jet_crypto_file_io_test_hits(), 0);
        jet_crypto_clear_file_io_test_fault();
        assert_eq!(std::fs::read(&winner).unwrap(), b"winner");
        assert!(std::fs::read_dir(&root).unwrap().all(|entry| {
            !entry.unwrap().file_name().to_string_lossy().starts_with(".jetc-")
        }));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn jetc_v2_entropy_failure_occurs_after_verified_snapshot_and_publishes_nothing() {
        let root = test_dir("entropy");
        let source = root.join("source.bin");
        let envelope = root.join("sealed.jetc");
        std::fs::write(&source, b"snapshot first").unwrap();
        fail_entropy();
        assert_eq!(
            jet_crypto_file_seal_impl(
                vec![JetX25519PublicKey([9; 32])],
                &source.to_string_lossy().into_owned(),
                &envelope.to_string_lossy().into_owned(),
                never_cancelled,
            ),
            Err(JetFileCryptoError::SealFailed(JetCryptoError::EntropyUnavailable))
        );
        jet_crypto_entropy_clear_test_provider();
        assert!(!envelope.exists());
        assert_eq!(std::fs::read(&source).unwrap(), b"snapshot first");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn jetc_v2_linux_publication_holds_parent_and_never_replaces_a_racer() {
        let root = test_dir("publish-races");
        let source = root.join("source.bin");
        std::fs::write(&source, b"held parent").unwrap();
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        let public = jet_crypto_x25519_public_typed_impl(&recipient);

        let parent = root.join("seal-parent");
        let moved = root.join("seal-parent-moved");
        std::fs::create_dir(&parent).unwrap();
        let destination = parent.join("sealed.jetc");
        arm_publish_race(PublishRace::SwapParent {
            at: "seal-stage",
            parent: parent.clone(),
            moved: moved.clone(),
        });
        assert_eq!(
            jet_crypto_file_seal_impl(
                vec![public.clone()],
                &source.to_string_lossy().into_owned(),
                &destination.to_string_lossy().into_owned(),
                never_cancelled,
            ),
            Err(JetFileCryptoError::DestinationIo)
        );
        assert!(!destination.exists());
        assert!(!moved.join("sealed.jetc").exists());
        assert!(std::fs::read_dir(&moved).unwrap().next().is_none());

        let race_parent = root.join("race-parent");
        std::fs::create_dir(&race_parent).unwrap();
        let raced = race_parent.join("sealed.jetc");
        arm_publish_race(PublishRace::CreateDestination {
            at: "seal-output",
            destination: raced.clone(),
        });
        assert_eq!(
            jet_crypto_file_seal_impl(
                vec![public.clone()],
                &source.to_string_lossy().into_owned(),
                &raced.to_string_lossy().into_owned(),
                never_cancelled,
            ),
            Err(JetFileCryptoError::DestinationExists)
        );
        assert_eq!(std::fs::read(&raced).unwrap(), b"racer");
        assert_eq!(std::fs::read_dir(&race_parent).unwrap().count(), 1);

        let stable_parent = root.join("stable-parent");
        std::fs::create_dir(&stable_parent).unwrap();
        let envelope = stable_parent.join("sealed.jetc");
        jet_crypto_file_seal_impl(
            vec![public],
            &source.to_string_lossy().into_owned(),
            &envelope.to_string_lossy().into_owned(),
            never_cancelled,
        ).unwrap();
        let open_parent = root.join("open-parent");
        let open_moved = root.join("open-parent-moved");
        std::fs::create_dir(&open_parent).unwrap();
        let restored = open_parent.join("restored.bin");
        arm_publish_race(PublishRace::SwapParent {
            at: "open-output",
            parent: open_parent.clone(),
            moved: open_moved.clone(),
        });
        assert_eq!(
            jet_crypto_file_open_impl(
                &recipient,
                &envelope.to_string_lossy().into_owned(),
                &restored.to_string_lossy().into_owned(),
                never_cancelled,
            ),
            Err(JetFileCryptoError::DestinationIo)
        );
        assert!(!restored.exists());
        assert!(!open_moved.join("restored.bin").exists());
        assert!(std::fs::read_dir(&open_moved).unwrap().next().is_none());

        let real_parent = root.join("real-parent");
        let linked_parent = root.join("linked-parent");
        std::fs::create_dir(&real_parent).unwrap();
        std::os::unix::fs::symlink(&real_parent, &linked_parent).unwrap();
        let linked_destination = linked_parent.join("sealed.jetc");
        assert_eq!(
            jet_crypto_file_seal_impl(
                vec![jet_crypto_x25519_public_typed_impl(&recipient)],
                &source.to_string_lossy().into_owned(),
                &linked_destination.to_string_lossy().into_owned(),
                never_cancelled,
            ),
            Err(JetFileCryptoError::DestinationIo)
        );
        assert!(!real_parent.join("sealed.jetc").exists());
        PUBLISH_RACE.with(|slot| assert!(slot.borrow().is_none()));
        jet_crypto_clear_file_boundary_test_observer();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn jetc_v2_linux_anonymous_inodes_ignore_every_former_temp_namespace() {
        let root = test_dir("anonymous-inodes");
        let source = root.join("source.bin");
        let plaintext = b"anonymous inode payload";
        std::fs::write(&source, plaintext).unwrap();
        let recipient = jet_crypto_x25519_generate_impl().unwrap();
        let public = jet_crypto_x25519_public_typed_impl(&recipient);

        let stage_parent = root.join("seal-stage-parent");
        std::fs::create_dir(&stage_parent).unwrap();
        let stage_envelope = stage_parent.join("sealed.jetc");
        replace_candidate_at("seal-stage", &stage_parent, "jetc-stage");
        jet_crypto_file_seal_impl(
            vec![public.clone()],
            &source.to_string_lossy().into_owned(),
            &stage_envelope.to_string_lossy().into_owned(),
            never_cancelled,
        ).unwrap();
        assert_attacker_candidates(&stage_parent, "jetc-stage");
        assert_not_attacker(&stage_envelope);

        let output_parent = root.join("seal-output-parent");
        std::fs::create_dir(&output_parent).unwrap();
        let output_envelope = output_parent.join("sealed.jetc");
        replace_candidate_at("seal-output", &output_parent, "jetc-output");
        jet_crypto_file_seal_impl(
            vec![public.clone()],
            &source.to_string_lossy().into_owned(),
            &output_envelope.to_string_lossy().into_owned(),
            never_cancelled,
        ).unwrap();
        assert_attacker_candidates(&output_parent, "jetc-output");
        assert_not_attacker(&output_envelope);

        let open_parent = root.join("open-output-parent");
        std::fs::create_dir(&open_parent).unwrap();
        let restored = open_parent.join("restored.bin");
        replace_candidate_at("open-output", &open_parent, "jetc-open");
        jet_crypto_file_open_impl(
            &recipient,
            &stage_envelope.to_string_lossy().into_owned(),
            &restored.to_string_lossy().into_owned(),
            never_cancelled,
        ).unwrap();
        assert_eq!(std::fs::read(&restored).unwrap(), plaintext);
        assert_attacker_candidates(&open_parent, "jetc-open");

        let key = (0u8..32).collect::<Vec<_>>();
        let nonce = (0u8..12).collect::<Vec<_>>();
        let historical = b"historical anonymous migration";
        let ciphertext = ChaCha20Poly1305::new_from_slice(&key).unwrap()
            .encrypt(ChaNonce::from_slice(&nonce), historical.as_slice()).unwrap();
        let mut v1 = b"JETC".to_vec();
        v1.extend_from_slice(&[1, 1]);
        v1.extend_from_slice(&nonce);
        v1.extend_from_slice(&ciphertext);
        let v1_source = root.join("historical.jetc");
        std::fs::write(&v1_source, &v1).unwrap();

        let migrate_stage_parent = root.join("migrate-stage-parent");
        std::fs::create_dir(&migrate_stage_parent).unwrap();
        let migrated_stage = migrate_stage_parent.join("migrated.jetc");
        replace_candidate_at("migrate-stage", &migrate_stage_parent, "jetc-stage");
        jet_crypto_expert_migrate_v1_impl(
            &key,
            &v1_source.to_string_lossy().into_owned(),
            vec![public.clone()],
            &migrated_stage.to_string_lossy().into_owned(),
            never_cancelled,
        ).unwrap();
        assert_attacker_candidates(&migrate_stage_parent, "jetc-stage");
        assert_not_attacker(&migrated_stage);
        assert_eq!(std::fs::read(&v1_source).unwrap(), v1);

        let migrate_output_parent = root.join("migrate-output-parent");
        std::fs::create_dir(&migrate_output_parent).unwrap();
        let migrated_output = migrate_output_parent.join("migrated.jetc");
        replace_candidate_at("migrate-output", &migrate_output_parent, "jetc-output");
        jet_crypto_expert_migrate_v1_impl(
            &key,
            &v1_source.to_string_lossy().into_owned(),
            vec![public.clone()],
            &migrated_output.to_string_lossy().into_owned(),
            never_cancelled,
        ).unwrap();
        assert_attacker_candidates(&migrate_output_parent, "jetc-output");
        assert_not_attacker(&migrated_output);
        assert_ne!(std::fs::read(&migrated_output).unwrap(), historical);

        let swapped_parent = root.join("migrate-swapped-parent");
        let moved_parent = root.join("migrate-swapped-parent-moved");
        std::fs::create_dir(&swapped_parent).unwrap();
        let swapped_output = swapped_parent.join("migrated.jetc");
        arm_publish_race(PublishRace::SwapParent {
            at: "migrate-output",
            parent: swapped_parent.clone(),
            moved: moved_parent.clone(),
        });
        assert_eq!(
            jet_crypto_expert_migrate_v1_impl(
                &key,
                &v1_source.to_string_lossy().into_owned(),
                vec![public],
                &swapped_output.to_string_lossy().into_owned(),
                never_cancelled,
            ),
            Err(JetFileCryptoError::DestinationIo)
        );
        assert!(!swapped_output.exists());
        assert!(!moved_parent.join("migrated.jetc").exists());
        assert!(std::fs::read_dir(&moved_parent).unwrap().next().is_none());
        assert_eq!(std::fs::read(&v1_source).unwrap(), v1);
        PUBLISH_RACE.with(|slot| assert!(slot.borrow().is_none()));
        jet_crypto_clear_file_boundary_test_observer();
        std::fs::remove_dir_all(root).unwrap();
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
