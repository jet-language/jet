mod runtime {
    include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs");
}

use runtime::{
    jet_crypto_entropy_bytes, jet_crypto_entropy_clear_test_provider,
    jet_crypto_entropy_fill_with, jet_crypto_entropy_set_test_provider, JetCryptoEntropyError,
    JetCryptoEntropyStep,
};
use std::sync::{Arc, Barrier};

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
fn live_provider_obeys_bounds_zero_and_concurrency() {
    assert_eq!(jet_crypto_entropy_bytes(0).unwrap(), Vec::<u8>::new());
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
