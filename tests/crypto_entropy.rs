mod runtime {
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
    assert_eq!(result, Err(JetCryptoEntropyError::Unavailable));
    assert_eq!(calls, 17);
    jet_crypto_entropy_clear_wasi_attempt_test_observer();
    assert_eq!(&*generations.borrow(), &(0..17).collect::<Vec<_>>());
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

#[cfg(target_os = "linux")]
#[test]
fn live_provider_remains_independent_across_fork() {
    use std::os::raw::{c_int, c_void};

    unsafe extern "C" {
        fn close(fd: c_int) -> c_int;
        fn fork() -> c_int;
        fn pipe(fds: *mut c_int) -> c_int;
        fn read(fd: c_int, buffer: *mut c_void, count: usize) -> isize;
        fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
        fn write(fd: c_int, buffer: *const c_void, count: usize) -> isize;
        fn _exit(status: c_int) -> !;
    }

    let parent_bytes = jet_crypto_entropy_bytes(64).unwrap();
    let mut fds = [-1; 2];
    assert_eq!(unsafe { pipe(fds.as_mut_ptr()) }, 0);
    let pid = unsafe { fork() };
    assert!(pid >= 0, "fork failed");
    if pid == 0 {
        unsafe { close(fds[0]) };
        let child_bytes = match jet_crypto_entropy_bytes(64) {
            Ok(bytes) => bytes,
            Err(_) => unsafe { _exit(2) },
        };
        let mut written = 0usize;
        while written < child_bytes.len() {
            let count = unsafe {
                write(
                    fds[1],
                    child_bytes[written..].as_ptr().cast(),
                    child_bytes.len() - written,
                )
            };
            if count <= 0 {
                unsafe { _exit(3) };
            }
            written += count as usize;
        }
        unsafe { close(fds[1]) };
        unsafe { _exit(0) };
    }

    unsafe { close(fds[1]) };
    let mut child_bytes = [0u8; 64];
    let mut received = 0usize;
    while received < child_bytes.len() {
        let count = unsafe {
            read(
                fds[0],
                child_bytes[received..].as_mut_ptr().cast(),
                child_bytes.len() - received,
            )
        };
        assert!(count > 0, "child entropy pipe closed early");
        received += count as usize;
    }
    unsafe { close(fds[0]) };
    let mut status = 0;
    assert_eq!(unsafe { waitpid(pid, &mut status, 0) }, pid);
    assert_eq!(status, 0, "child entropy process failed: wait status {status}");
    assert_ne!(child_bytes, [0; 64]);
    assert_ne!(parent_bytes, child_bytes);
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
