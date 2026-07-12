mod runtime {
    include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs");
}

use runtime::{
    jet_crypto_entropy_bytes, jet_crypto_entropy_clear_test_provider,
    jet_crypto_entropy_fill, jet_crypto_entropy_fill_with, jet_crypto_entropy_set_test_provider,
    jet_crypto_entropy_clear_zeroize_test_observer,
    jet_crypto_entropy_set_zeroize_test_observer, JetCryptoEntropyError,
    jet_crypto_entropy_unsupported_for_test, jet_crypto_entropy_wasi_with_for_test,
    JetCryptoEntropyStep,
};
use std::sync::{Arc, Barrier};

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

[dependencies]
aes-gcm = "0.10"
argon2 = "0.5"
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
        assert_eq!(err, JetCryptoEntropyError::Unavailable);
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
        Err(JetCryptoEntropyError::Unavailable)
    );
    jet_crypto_entropy_clear_test_provider();
}

#[test]
fn wasi_interrupt_retries_zeroize_each_exact_count_buffer() {
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
    assert_eq!(&*zeroized.borrow(), &vec![vec![0; 32], vec![0; 32]]);
}

#[test]
fn wasi_stops_after_seventeen_interrupts() {
    let mut calls = 0usize;
    let result = jet_crypto_entropy_wasi_with_for_test(8, |out| {
        calls += 1;
        out.fill(0xa5);
        27
    });
    assert_eq!(result, Err(JetCryptoEntropyError::Unavailable));
    assert_eq!(calls, 17);
}

#[test]
fn unsupported_provider_zeroizes_and_fails_closed() {
    let mut out = [0xa5; 16];
    assert_eq!(
        jet_crypto_entropy_unsupported_for_test(&mut out),
        Err(JetCryptoEntropyError::Unavailable)
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
        Err(JetCryptoEntropyError::NegativeLength)
    );
    assert_eq!(
        jet_crypto_entropy_bytes(1_048_577),
        Err(JetCryptoEntropyError::TooLarge)
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
fn crypto_runtime_sources_contain_no_predictable_fallback() {
    let process = include_str!("../crates/jet-codegen/src/Prelude/CoreLib/Top/Process.rs");
    let crypto_random = process
        .split("fn jet_std_crypto_random_bytes")
        .nth(1)
        .expect("crypto random shim exists")
        .split("\n}")
        .next()
        .expect("crypto random shim closes");
    let sources = [
        include_str!("../crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs"),
        include_str!("../crates/jetpack/src/Prelude/Crypto.rs"),
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
fn keygen_entropy_failure_emits_no_unratified_helper_copy() {
    let ffi = include_str!("../crates/jetpack/src/FFI.rs");
    let keygen = ffi
        .split("\"keygen\" => {{")
        .nth(1)
        .expect("crypto helper keygen branch exists")
        .split("\"sign\" => {{")
        .next()
        .unwrap();
    assert!(keygen.contains("Err(_) => exit(1)"));
    assert!(!keygen.contains("fail(&e"));
    assert!(!keygen.contains("eprintln!"));
}
