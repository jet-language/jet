mod common;

mod runtime {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs");
}

use runtime::{
    jet_crypto_entropy_bytes, jet_crypto_entropy_clear_test_provider,
    jet_crypto_entropy_fill, jet_crypto_entropy_fill_with, jet_crypto_entropy_set_test_provider,
    jet_crypto_entropy_clear_zeroize_test_observer,
    jet_crypto_entropy_clear_wasi_attempt_test_observer,
    jet_crypto_entropy_set_zeroize_test_observer, JetCryptoEntropyError,
    jet_crypto_entropy_set_wasi_attempt_test_observer,
    jet_crypto_entropy_unsupported_for_test, jet_crypto_entropy_wasi_with_for_test,
    JetCryptoEntropyStep, JetCryptoWasiAttemptEvent,
};
use std::sync::{Arc, Barrier};

#[test]
fn crypto_error_display_contract_is_exact_and_redacted() {
    let cases = [
        (
            JetCryptoEntropyError::InvalidLength {
                operation: "seal",
                parameter: "key",
                expected: "exactly 32",
                actual: 31,
            },
            "seal: key must be exactly 32; got 31",
        ),
        (
            JetCryptoEntropyError::InvalidEncoding {
                operation: "PasswordHash.parse",
                value_kind: "PHC string",
            },
            "PasswordHash.parse: PHC string is not canonical",
        ),
        (
            JetCryptoEntropyError::UnsupportedVersion {
                operation: "PasswordHash.parse",
                version: 16,
            },
            "PasswordHash.parse: version 16 is not supported",
        ),
        (
            JetCryptoEntropyError::UnsupportedAlgorithm {
                operation: "PasswordHash.parse",
                algorithm: "argon2i".to_string(),
            },
            "PasswordHash.parse: algorithm argon2i is not supported",
        ),
        (
            JetCryptoEntropyError::OpenFailed,
            "encrypted data could not be opened",
        ),
        (
            JetCryptoEntropyError::NonContributoryKey,
            "X25519 peer key does not contribute to a shared secret",
        ),
        (
            JetCryptoEntropyError::OutputLength {
                operation: "hkdf_sha256",
                minimum: 0,
                maximum: 8160,
                actual: 8161,
            },
            "hkdf_sha256: output length must be 0..8160; got 8161",
        ),
        (
            JetCryptoEntropyError::PasswordPolicy {
                reason: "public-policy-id",
            },
            "password hash is outside Jet's accepted policy",
        ),
        (
            JetCryptoEntropyError::EntropyUnavailable,
            "the operating system could not provide cryptographic randomness",
        ),
        (
            JetCryptoEntropyError::ResourceUnavailable {
                resource: "Argon2 worker pool",
            },
            "Argon2 worker pool is unavailable for this cryptographic operation",
        ),
        (
            JetCryptoEntropyError::Internal {
                incident_id: "crypto-17",
            },
            "Jet could not preserve a cryptographic invariant; incident crypto-17",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let rendered = format!("{error:?} {error}");
        for forbidden in ["hunter2", "plaintext sentinel", "ciphertext sentinel", "/home/nate"] {
            assert!(!rendered.contains(forbidden), "error leaked `{forbidden}`: {rendered}");
        }
    }
}

#[test]
fn actual_bridge_fixture_compiles_and_runs() {
    let root = std::env::current_dir().unwrap();
    let dir = std::env::temp_dir().join(format!(
        "jet-crypto-bridge-entropy-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        r#"[package]
name = "jet-crypto-entropy-proof"
version = "0.0.0"
edition = "2021"

[workspace]

[dependencies]
aes-gcm = "0.10"
argon2 = { version = "=0.5.3", default-features = false, features = ["alloc", "password-hash"] }
blake3 = "1"
chacha20poly1305 = "0.10"
ed25519-dalek = "2"
hkdf = "0.12"
sha2 = "0.10"
subtle = "2"
x25519-dalek = "2"
"#,
    )
    .unwrap();
    let fixture = root.join("tests/fixtures/crypto_bridge_entropy.rs");
    std::fs::write(
        dir.join("src/lib.rs"),
        format!("include!({:?});\n", fixture),
    )
    .unwrap();
    let output = std::process::Command::new("cargo")
        .args(["test", "--offline", "--quiet", "--manifest-path"])
        .arg(dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", dir.join("target"))
        .output()
        .unwrap();
    let cleanup = std::fs::remove_dir_all(&dir);
    assert!(cleanup.is_ok(), "bridge proof left temporary artifacts");
    assert!(
        output.status.success(),
        "bridge proof failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn scripted_provider_fills_suffix_and_retries_interruption() {
    let mut out = [0u8; 7];
    let mut calls = 0usize;
    jet_crypto_entropy_fill_with(&mut out, |suffix| {
        calls += 1;
        match calls {
            1 => {
                suffix[..2].copy_from_slice(&[1, 2]);
                JetCryptoEntropyStep::Filled(2)
            }
            2 => JetCryptoEntropyStep::Interrupted,
            3 => {
                suffix.copy_from_slice(&[3, 4, 5, 6, 7]);
                JetCryptoEntropyStep::Filled(5)
            }
            _ => unreachable!(),
        }
    })
    .unwrap();
    assert_eq!(out, [1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(calls, 3);
}

#[test]
fn failure_and_zero_fill_clear_the_entire_attempt() {
    for terminal in [JetCryptoEntropyStep::Filled(0), JetCryptoEntropyStep::Failed] {
        let mut out = [0xa5; 32];
        let err = jet_crypto_entropy_fill_with(&mut out, |suffix| {
            suffix[..8].fill(0x5a);
            terminal
        })
        .unwrap_err();
        assert_eq!(err, JetCryptoEntropyError::EntropyUnavailable);
        assert_eq!(out, [0; 32], "tainted attempt must be zeroized");
    }
}

#[test]
fn injected_partial_failure_returns_no_bytes() {
    let mut first = true;
    jet_crypto_entropy_set_test_provider(move |suffix| {
        if first {
            first = false;
            suffix[..4].copy_from_slice(&[1, 2, 3, 4]);
            JetCryptoEntropyStep::Filled(4)
        } else {
            JetCryptoEntropyStep::Failed
        }
    });
    assert_eq!(
        jet_crypto_entropy_bytes(16),
        Err(JetCryptoEntropyError::EntropyUnavailable)
    );
    jet_crypto_entropy_clear_test_provider();
}

#[test]
fn wasi_interrupt_retries_zeroize_each_exact_count_buffer() {
    let lifecycle = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let observed_lifecycle = std::rc::Rc::clone(&lifecycle);
    jet_crypto_entropy_set_wasi_attempt_test_observer(move |event, generation, bytes| {
        observed_lifecycle
            .borrow_mut()
            .push((event, generation, bytes.to_vec()));
    });
    let zeroized = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<u8>>::new()));
    let observed = std::rc::Rc::clone(&zeroized);
    jet_crypto_entropy_set_zeroize_test_observer(move |bytes| {
        observed.borrow_mut().push(bytes.to_vec());
    });
    let mut calls = 0usize;
    let bytes = jet_crypto_entropy_wasi_with_for_test(32, |out| {
        assert_eq!(out.len(), 32);
        calls += 1;
        if calls < 3 {
            out[..4].fill(0xa5);
            27
        } else {
            out.fill(7);
            0
        }
    })
    .unwrap();
    assert_eq!(bytes, vec![7; 32]);
    assert_eq!(calls, 3);
    jet_crypto_entropy_clear_zeroize_test_observer();
    jet_crypto_entropy_clear_wasi_attempt_test_observer();
    assert_eq!(&*zeroized.borrow(), &vec![vec![0; 32], vec![0; 32]]);

    let lifecycle = lifecycle.borrow();
    let mut live_attempt = None;
    let mut generations = Vec::new();
    for (event, generation, snapshot) in lifecycle.iter() {
        match event {
            JetCryptoWasiAttemptEvent::Created => {
                assert!(live_attempt.replace(*generation).is_none());
                assert_eq!(snapshot, &vec![0; 32]);
                generations.push(*generation);
            }
            JetCryptoWasiAttemptEvent::ProviderReturned(27) => {
                assert_eq!(live_attempt, Some(*generation));
                assert_eq!(&snapshot[..4], &[0xa5; 4]);
            }
            JetCryptoWasiAttemptEvent::ProviderReturned(0) => {
                assert_eq!(live_attempt, Some(*generation));
                assert_eq!(snapshot, &vec![7; 32]);
            }
            JetCryptoWasiAttemptEvent::Zeroized => {
                assert_eq!(live_attempt, Some(*generation));
                assert_eq!(snapshot, &vec![0; 32]);
            }
            JetCryptoWasiAttemptEvent::Released | JetCryptoWasiAttemptEvent::Returned => {
                assert_eq!(live_attempt.take(), Some(*generation));
            }
            JetCryptoWasiAttemptEvent::ProviderReturned(errno) => {
                panic!("unexpected WASI errno {errno}")
            }
        }
    }
    assert_eq!(generations, vec![0, 1, 2]);
    assert!(live_attempt.is_none());
    assert!(!bytes.contains(&0xa5));
}

#[test]
fn wasi_stops_after_seventeen_interrupts() {
    let generations = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let observed = std::rc::Rc::clone(&generations);
    jet_crypto_entropy_set_wasi_attempt_test_observer(move |event, generation, bytes| {
        if event == JetCryptoWasiAttemptEvent::Released {
            assert!(bytes.is_empty());
            observed.borrow_mut().push(generation);
        }
    });
    let mut calls = 0usize;
    let result = jet_crypto_entropy_wasi_with_for_test(8, |out| {
        calls += 1;
        out.fill(0xa5);
        27
    });
    assert_eq!(result, Err(JetCryptoEntropyError::EntropyUnavailable));
    assert_eq!(calls, 17);
    jet_crypto_entropy_clear_wasi_attempt_test_observer();
    assert_eq!(&*generations.borrow(), &(0..17).collect::<Vec<_>>());
}

#[test]
fn unsupported_provider_zeroizes_and_fails_closed() {
    let mut out = [0xa5; 16];
    assert_eq!(
        jet_crypto_entropy_unsupported_for_test(&mut out),
        Err(JetCryptoEntropyError::EntropyUnavailable)
    );
    assert_eq!(out, [0; 16]);
}

#[test]
fn live_provider_obeys_bounds_zero_and_concurrency() {
    assert_eq!(jet_crypto_entropy_bytes(0).unwrap(), Vec::<u8>::new());
    let mut filled = [0u8; 32];
    jet_crypto_entropy_fill(&mut filled).unwrap();
    assert_ne!(filled, [0; 32]);
    assert_eq!(
        jet_crypto_entropy_bytes(-1),
        Err(JetCryptoEntropyError::InvalidLength { operation: "core.crypto.random.bytes", parameter: "count", expected: "non-negative", actual: 1 })
    );
    assert_eq!(
        jet_crypto_entropy_bytes(1_048_577),
        Err(JetCryptoEntropyError::OutputLength { operation: "core.crypto.random.bytes", minimum: 0, maximum: 1_048_576, actual: 1_048_577 })
    );

    let workers = 8;
    let barrier = Arc::new(Barrier::new(workers));
    let joins: Vec<_> = (0..workers)
        .map(|_| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                jet_crypto_entropy_bytes(64).unwrap()
            })
        })
        .collect();
    let outputs: Vec<Vec<u8>> = joins.into_iter().map(|j| j.join().unwrap()).collect();
    assert!(outputs.iter().all(|bytes| bytes.len() == 64));
    for i in 0..outputs.len() {
        for j in i + 1..outputs.len() {
            assert_ne!(outputs[i], outputs[j]);
        }
    }
}

#[test]
fn live_provider_subprocess_child() {
    if std::env::var_os("JET_CRYPTO_ENTROPY_SUBPROCESS").is_none() {
        return;
    }
    let bytes = jet_crypto_entropy_bytes(64).unwrap();
    let encoded: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    println!("JET_CRYPTO_ENTROPY={encoded}");
}

#[cfg(target_os = "linux")]
#[test]
fn live_provider_remains_independent_across_process_exec() {
    use std::io::Read;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let parent_bytes = jet_crypto_entropy_bytes(64).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "live_provider_subprocess_child", "--nocapture"])
        .env("JET_CRYPTO_ENTROPY_SUBPROCESS", "1")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("entropy subprocess exceeded five-second deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "entropy subprocess failed: {status}");
    let mut stdout = String::new();
    child.stdout.take().unwrap().read_to_string(&mut stdout).unwrap();
    let encoded = stdout
        .lines()
        .find_map(|line| line.strip_prefix("JET_CRYPTO_ENTROPY="))
        .expect("child emitted entropy marker");
    assert_eq!(encoded.len(), 128);
    assert!(encoded.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let parent_encoded: String = parent_bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_ne!(encoded, parent_encoded);
}

#[test]
fn crypto_runtime_sources_contain_no_predictable_fallback() {
    let crypto_entropy = include_str!(
        "../crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs"
    );
    let crypto_random = crypto_entropy
        .split("fn jet_std_crypto_random_bytes")
        .nth(1)
        .expect("crypto random shim exists")
        .split("\n}")
        .next()
        .expect("crypto random shim closes");
    let sources = [
        crypto_entropy,
        include_str!("../crates/jet-pkg-model/src/Prelude/Crypto.rs"),
        crypto_random,
    ]
    .join("\n");
    for forbidden in [
        "SplitMix",
        "xorshift",
        "SystemTime",
        "UNIX_EPOCH",
        "/dev/urandom",
        "/dev/random",
        "Math.random",
        "jet_uuid_fill_random",
    ] {
        assert!(
            !sources.contains(forbidden),
            "cryptographic runtime contains forbidden fallback marker {forbidden}"
        );
    }
}

#[test]
fn golden_i1_scan_strips_only_the_vetted_entropy_module() {
    let provider = include_str!(
        "../crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs"
    );
    assert!(provider.contains("mod jet_crypto_entropy {"));
    let golden = include_str!("golden.rs");
    assert!(golden.contains("strip_mod(&s, \"jet_crypto_entropy\")"));
    assert!(!provider
        .split("mod jet_crypto_entropy {")
        .next()
        .unwrap()
        .contains("unsafe"));
    assert!(!provider
        .rsplit_once("}\n\npub use jet_crypto_entropy")
        .expect("vetted module has one explicit end")
        .1
        .contains("unsafe"));
}

#[test]
fn keygen_entropy_failure_uses_closed_silent_helper_status() {
    let ffi = include_str!("../crates/jet-pkg-model/src/FFI.rs");
    let keygen = ffi
        .split("\"keygen\" => {{")
        .nth(1)
        .expect("crypto helper keygen branch exists")
        .split("\"sign\" => {{")
        .next()
        .unwrap();
    assert!(keygen.contains("Err(_) => exit(ENTROPY_UNAVAILABLE)"));
    assert!(ffi.contains("const ENTROPY_UNAVAILABLE: i32 = 75;"));
    assert!(!keygen.contains("fail(&e"));
    assert!(!keygen.contains("eprintln!"));
}
