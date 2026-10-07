mod common;

mod runtime {
    // This module includes shared Prelude source that several hosts compile,
    // each using a different subset, so dead-code reports here describe the
    // other hosts' usage rather than this target's.
    #![allow(dead_code)]
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs");
    pub use jet_crypto_entropy::{jet_crypto_entropy_fill, JetCryptoEntropyError};
}

use jet::Interpreter::{dev_iteration, RunOutcome};
use jet_foundation::JitBackend::JitBackend;
use jet_jit::CraneliftBackend;
pub(crate) use runtime::*;
use std::fs;
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
        for forbidden in [
            "hunter2",
            "plaintext sentinel",
            "ciphertext sentinel",
            "/home/nate",
        ] {
            assert!(
                !rendered.contains(forbidden),
                "error leaked `{forbidden}`: {rendered}"
            );
        }
    }
}

#[test]
fn actual_bridge_fixture_compiles_and_runs() {
    let root = std::env::current_dir().unwrap();
    let dir =
        std::env::temp_dir().join(format!("jet-crypto-bridge-entropy-{}", std::process::id()));
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!(
            "crypto bridge fixture pre-cleanup failed for {}: {error}",
            dir.display()
        ),
    }
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
    // The bridge crate's exact runtime projection, then the fixture's crypto
    // runtime and its tests.
    std::fs::write(
        dir.join("src/lib.rs"),
        format!(
            "#![allow(non_snake_case, dead_code)]\n{}\n{}\ninclude!({:?});\n",
            jet_pkg_model::FFI::crypto_bridge_root_modules(),
            jet_pkg_model::FFI::crypto_bridge_entropy_runtime(),
            fixture
        ),
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
    for terminal in [
        JetCryptoEntropyStep::Filled(0),
        JetCryptoEntropyStep::Failed,
    ] {
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
        Err(JetCryptoEntropyError::InvalidLength {
            operation: "core.crypto.random.bytes",
            parameter: "count",
            expected: "non-negative",
            actual: 1
        })
    );
    assert_eq!(
        jet_crypto_entropy_bytes(1_048_577),
        Err(JetCryptoEntropyError::OutputLength {
            operation: "core.crypto.random.bytes",
            minimum: 0,
            maximum: 1_048_576,
            actual: 1_048_577
        })
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
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    // Single-threaded libtest prints the child's line after its own
    // `test … ... ` status prefix, so the marker need not start the line.
    let encoded = stdout
        .lines()
        .find_map(|line| line.split_once("JET_CRYPTO_ENTROPY=").map(|(_, hex)| hex))
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
    let delivered_base =
        include_str!("../crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs");
    let sources = [
        delivered_base,
        include_str!("../crates/jet-foundation/src/Syntax/core_calls.rs"),
        include_str!("../crates/jet-codegen/src/Prelude/CoreLib/Top/EncodingCodecs.rs"),
        include_str!("../crates/jet-pkg-model/src/Prelude/Crypto.rs"),
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
    let host_sources = [
        include_str!("../crates/jet-jit/src/Crypto.rs"),
        include_str!("../crates/jet-jit/src/Encoding.rs"),
        include_str!("../crates/jet-jit/src/net_http_rt.rs"),
    ]
    .join("\n");
    for forbidden in [
        "SplitMix",
        "xorshift",
        "/dev/urandom",
        "/dev/random",
        "Math.random",
        "uuid_fill_random",
    ] {
        assert!(
            !host_sources.contains(forbidden),
            "JIT/runtime adapter contains forbidden fallback marker {forbidden}"
        );
    }
}

#[allow(dead_code)]
mod auth_session_regression {
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

    static FAIL_ENTROPY: AtomicBool = AtomicBool::new(false);
    static NEXT_BYTE: AtomicU8 = AtomicU8::new(1);

    fn jet_crypto_entropy_bytes(count: i64) -> Result<Vec<u8>, ()> {
        if FAIL_ENTROPY.load(Ordering::Relaxed) {
            return Err(());
        }
        let count = usize::try_from(count).map_err(|_| ())?;
        let byte = NEXT_BYTE.fetch_add(1, Ordering::Relaxed);
        Ok(vec![byte; count])
    }
    include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/SHA256Raw.rs");
    include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs");

    #[test]
    fn auth_tokens_are_opaque_and_fail_closed_without_entropy() {
        FAIL_ENTROPY.store(false, Ordering::Relaxed);
        let user = format!("auth-regression-{}@example.com", std::process::id());
        jet_auth_register_user(user.clone(), "password-hash".into()).unwrap();

        let first = jet_auth_password_login(user.clone(), "password-hash".into(), 0, 1_000)
            .unwrap();
        let second = jet_auth_password_login(user.clone(), "password-hash".into(), 0, 1_000)
            .unwrap();
        assert!(first.id.starts_with("sess-"));
        assert!(second.id.starts_with("sess-"));
        assert_ne!(first.id, second.id);

        let magic = jet_auth_magic_link_issue(user.clone(), 0, 1_000).unwrap();
        assert!(magic.starts_with("magic-"));
        assert_ne!(magic, format!("magic-{}", "00".repeat(32)));
        assert!(jet_auth_magic_link_consume(magic, 0, 1_000).is_ok());

        FAIL_ENTROPY.store(true, Ordering::Relaxed);
        assert!(jet_auth_password_login(user.clone(), "password-hash".into(), 0, 1_000).is_err());
        assert!(jet_auth_magic_link_issue(user, 0, 1_000).is_err());
    }
}

#[test]
fn uuid_entropy_shared_seam_is_live_and_fail_closed() {
    let first = runtime::jet_crypto_uuid_v4_result().expect("live entropy should produce UUID v4");
    let second = runtime::jet_crypto_uuid_v4_result().expect("live entropy should produce UUID v4");
    assert_ne!(first, second, "UUID v4 must use live entropy for each call");
    let ordered = runtime::jet_crypto_uuid_v7_result(1_700_000_000_000)
        .expect("live entropy should produce UUID v7");
    assert_eq!(&ordered[14..15], "7");

    jet_crypto_entropy_set_test_provider(|out| {
        out.fill(0xa5);
        JetCryptoEntropyStep::Failed
    });
    let v4 = runtime::jet_crypto_uuid_v4_result();
    let v7 = runtime::jet_crypto_uuid_v7_result(1_700_000_000_000);
    jet_crypto_entropy_clear_test_provider();

    assert!(matches!(v4, Err(JetCryptoEntropyError::EntropyUnavailable)));
    assert!(matches!(v7, Err(JetCryptoEntropyError::EntropyUnavailable)));
}

#[test]
fn live_crypto_entropy_and_uuid_match_interpreter_resident_jit_and_aot() {
    assert!(common::have_rustc(), "AOT parity requires rustc");
    assert!(
        jet_jit::cranelift_host_supported(),
        "resident-JIT parity requires a supported Cranelift host"
    );

    const SOURCE: &str = r#"
use core.crypto.random as crypto
use core.time as time
use core.crypto.uuid as uuid

fn run() {
    first :: crypto.bytes(32) ?? return
    second :: crypto.bytes(32) ?? return
    print(first.len() == 32)
    print(first != second)
    v4a :: uuid.v4() ?? return
    v4b :: uuid.v4() ?? return
    print(v4a.len() == 36)
    print(v4a != v4b)
    clk := Clock.new(1_700_000_000_000)
    v7 :: uuid.v7(&clk) ?? return
    print(v7.len() == 36)
    print(v7.slice(14, 15) == "7")
}
"#;
    let expected = "true\ntrue\ntrue\ntrue\ntrue\ntrue\n";
    let dir = common::unique_tmp("jet_crypto_entropy_parity");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("parity.jet");
        fs::write(&path, SOURCE).unwrap();
        let shown = path.to_string_lossy().into_owned();

        let mut bundle = jet::Loader::load_entry(&shown).expect("parity bundle should load");
        let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
        assert!(
            diagnostics.iter().all(|diagnostic| !matches!(
                diagnostic.severity,
                jet::Diagnostics::Severity::Error
            )),
            "parity source must type-check: {diagnostics:?}"
        );
        assert!(
            common::cranelift_resident_safe(&bundle),
            "parity source must be resident-JIT safe"
        );
        let policy = common::development_policy();
        assert!(
            common::compile_cranelift_bundle(&bundle, &policy).is_ok(),
            "parity source must compile in the resident JIT"
        );

        let interpreted = match dev_iteration(&shown, false, true) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => (stdout, stderr, exit_code),
            RunOutcome::Problems(diagnostics) => {
                panic!("interpreter entropy/UUID parity failed: {diagnostics:?}")
            }
        };
        assert_eq!(interpreted, (expected.to_string(), String::new(), 0));

        jet_jit::reset_jit_trace_for_test();
        let mut backend = CraneliftBackend::new();
        let jit =
            jet_jit::with_program_args(&[shown.clone()], || {
                match common::run_cranelift_bundle(&mut backend, &bundle, false, &policy) {
                    RunOutcome::Ran {
                        stdout,
                        stderr,
                        exit_code,
                    } => (stdout, stderr, exit_code),
                    RunOutcome::Problems(diagnostics) => {
                        panic!("resident JIT entropy/UUID parity failed: {diagnostics:?}")
                    }
                }
            });
        assert!(jet_jit::jit_executed_for_test());
        assert!(!jet_jit::deopt_invoked_for_test());
        assert!(!jet_jit::fallback_invoked_for_test());
        assert_eq!(jit, interpreted, "resident JIT drifted from interpreter");

        let aot = common::build_and_run("jet_crypto_entropy_parity", "parity", SOURCE);
        assert_eq!(aot, (0, expected.to_string(), String::new()));
        assert_eq!(jit, (expected.to_string(), String::new(), 0));
    }));
    match (result, fs::remove_dir_all(&dir)) {
        (Ok(()), Ok(())) => {}
        (Ok(()), Err(error)) => {
            panic!(
                "crypto entropy parity artifact cleanup failed for {}: {error}",
                dir.display()
            );
        }
        (Err(payload), Ok(())) => {
            std::panic::resume_unwind(payload);
        }
        (Err(payload), Err(error)) => {
            eprintln!(
                "crypto entropy parity artifact cleanup failed for {}: {error}",
                dir.display()
            );
            std::panic::resume_unwind(payload);
        }
    }
}

/// The golden I1 scan removes the provider exactly as codegen emits it for a
/// crypto program, and nothing the user wrote.
#[test]
fn golden_i1_scan_strips_only_the_vetted_entropy_module() {
    let source = "use core.crypto.random as random\n\nfn run() {\n    drawn :: random.bytes(16) ?? return\n    print(\"i1-user-marker {drawn.len()}\")\n}\n";
    let dir = common::unique_tmp("jet_crypto_entropy_i1");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("i1.jet");
    fs::write(&path, source).unwrap();
    let shown = path.to_string_lossy().into_owned();
    let compiled = jet::compile_with_path(source, &shown).unwrap_or_else(|diagnostics| {
        panic!("{}", jet::render_diagnostics(&shown, source, &diagnostics))
    });
    let _ = fs::remove_dir_all(&dir);
    assert!(
        compiled.rust.contains("fn getrandom("),
        "a crypto program must embed the OS entropy provider"
    );
    let user = common::strip_vetted_prelude_modules(&compiled.rust);
    assert!(!user.contains("fn getrandom("), "the audited provider survived the I1 scan");
    assert!(!user.contains("mod jet_crypto_entropy"), "the provider module survived the I1 scan");
    assert!(user.contains("i1-user-marker"), "the I1 scan removed user code");
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

/// Start `command` on a platform whose OS entropy source is absent: the Linux
/// `getrandom` syscall answers ENOSYS under a seccomp filter the child inherits.
/// Every execution tier then reaches the production provider's real failure
/// path; no test hook or fallback provider is involved.
#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
fn without_os_entropy(command: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::unix::process::CommandExt;

    #[repr(C)]
    struct SockFilter {
        code: u16,
        jt: u8,
        jf: u8,
        k: u32,
    }
    #[repr(C)]
    struct SockFprog {
        len: u16,
        filter: *const SockFilter,
    }
    #[cfg(target_arch = "x86_64")]
    const SYS_GETRANDOM: u32 = 318;
    #[cfg(target_arch = "aarch64")]
    const SYS_GETRANDOM: u32 = 278;
    const ENOSYS: u32 = 38;
    const PR_SET_NO_NEW_PRIVS: std::ffi::c_int = 38;
    const PR_SET_SECCOMP: std::ffi::c_int = 22;
    const SECCOMP_MODE_FILTER: std::ffi::c_ulong = 2;
    // ld [seccomp_data.nr]; jeq getrandom; ret ERRNO(ENOSYS); ret ALLOW
    static FILTER: [SockFilter; 4] = [
        SockFilter { code: 0x20, jt: 0, jf: 0, k: 0 },
        SockFilter { code: 0x15, jt: 0, jf: 1, k: SYS_GETRANDOM },
        SockFilter { code: 0x06, jt: 0, jf: 0, k: 0x0005_0000 | ENOSYS },
        SockFilter { code: 0x06, jt: 0, jf: 0, k: 0x7fff_0000 },
    ];
    unsafe extern "C" {
        fn prctl(option: std::ffi::c_int, ...) -> std::ffi::c_int;
    }
    // SAFETY: the closure runs between fork and exec and only issues two
    // async-signal-safe prctl calls over a static, fully initialized filter.
    unsafe {
        command.pre_exec(|| {
            let program = SockFprog {
                len: FILTER.len() as u16,
                filter: FILTER.as_ptr(),
            };
            let zero: std::ffi::c_ulong = 0;
            if prctl(PR_SET_NO_NEW_PRIVS, 1 as std::ffi::c_ulong, zero, zero, zero) != 0
                || prctl(
                    PR_SET_SECCOMP,
                    SECCOMP_MODE_FILTER,
                    &program as *const SockFprog,
                    zero,
                    zero,
                ) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        })
    }
}

/// #1000: with no OS entropy, every core.crypto consumer (raw bytes, UUID v4,
/// X25519 key generation) stops with one typed failure that is byte-identical
/// on the AOT binary, the default `jet run` and the interpreter, and no tier
/// prints a value that should have come from entropy.
#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
#[test]
fn entropy_unavailable_propagates_through_every_consumer_on_all_tiers() {
    assert!(common::have_rustc(), "AOT parity requires rustc");
    let consumers = [
        (
            "bytes",
            "use core.crypto.random as random\n\nfn run() {\n    print(\"before\")\n    secret :: random.bytes(32) ?? {\n        print(\"typed failure\")\n        return\n    }\n    print(\"drew {secret.len()} bytes\")\n}\n",
        ),
        (
            "uuid",
            "use core.crypto.uuid as uuid\n\nfn run() {\n    print(\"before\")\n    id :: uuid.v4() ?? {\n        print(\"typed failure\")\n        return\n    }\n    print(\"drew {id}\")\n}\n",
        ),
        (
            "keygen",
            "use core.crypto as crypto\n\nfn run() {\n    print(\"before\")\n    key :: crypto.generatekey() ?? {\n        print(\"typed failure\")\n        return\n    }\n    print(\"drew a key: {key.public_key().bytes.len()} public bytes\")\n}\n",
        ),
    ];
    let scratch = common::Scratch::new("crypto-entropy-unavailable");
    fs::write(
        scratch.join("package.jet"),
        "name: \"entropy-unavailable\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [IO, Mem.Alloc, Rand] } }\n",
    )
    .unwrap();
    let jet = env!("CARGO_BIN_EXE_jet");
    let normalize = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes).replace(&scratch.path.display().to_string(), "$CASE")
    };
    for (name, source) in consumers {
        let file = format!("{name}.jet");
        fs::write(scratch.join(&file), source).unwrap();
        let cache = scratch.join(&format!("cache-{name}"));
        let build = std::process::Command::new(jet)
            .args(["build", &file])
            .current_dir(&scratch.path)
            .env("JET_STORE_DIR", cache.join("aot"))
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{name}: AOT build failed:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let mut tiers = Vec::new();
        for (tier, mut command) in [
            ("aot", std::process::Command::new(scratch.join(".jet").join("build").join(name))),
            ("default run", {
                let mut command = std::process::Command::new(jet);
                command.args(["run", &file]);
                command
            }),
            ("interpreter", {
                let mut command = std::process::Command::new(jet);
                command.args(["run", "--interpret", &file]);
                command
            }),
        ] {
            let output = without_os_entropy(&mut command)
                .current_dir(&scratch.path)
                .env("JET_RUN_CACHE_DIR", cache.join("run"))
                .env("JET_STORE_DIR", cache.join("build"))
                .env("NO_COLOR", "1")
                .output()
                .unwrap();
            let observed = (
                output.status.code(),
                normalize(&output.stdout),
                normalize(&output.stderr),
            );
            assert!(
                observed.1.starts_with("before\n") && !observed.1.contains("drew"),
                "{name} on {tier} produced a value without entropy: {observed:?}"
            );
            assert!(
                observed.1.contains("typed failure")
                    || (observed.0 != Some(0)
                        && observed.2.contains("could not provide cryptographic randomness")),
                "{name} on {tier} did not report the entropy failure: {observed:?}"
            );
            tiers.push((tier, observed));
        }
        let (first_tier, first) = &tiers[0];
        for (tier, observed) in &tiers[1..] {
            assert_eq!(
                observed, first,
                "{name}: {tier} differs from {first_tier} without entropy"
            );
        }
    }
}
