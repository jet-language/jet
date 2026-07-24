//! #302 C12: exact nominal-role and cross-tier crypto closeout proof.

mod common;

use common::{build_and_run, have_rustc, FfiBridgeLock};
use jet::Diagnostics::Severity;
use jet::Interpreter::{dev_iteration, RunOutcome};

fn assert_single_type_error(source: &str, expected: &[&str]) {
    let diagnostics = jet::compile(source).expect_err("wrong crypto role must fail in sema");
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "expected one type error: {diagnostics:?}");
    assert_eq!(errors[0].code, "E0112");
    let rendered = jet::render_diagnostics("crypto_wrong_role.jet", source, &diagnostics);
    for text in expected {
        assert!(
            rendered.contains(text),
            "missing {text:?} in wrong-role diagnostic:\n{rendered}"
        );
    }
}

#[test]
fn nominal_crypto_roles_are_rejected_during_compilation() {
    assert_single_type_error(
        r#"use core.crypto as crypto

fn run() {
    recipient :: crypto.X25519SecretKey.new_random() ?? return
    crypto.sign(recipient, [1, 2, 3]) ?? return
}
"#,
        &["SigningKey", "X25519SecretKey"],
    );

    assert_single_type_error(
        r#"use core.crypto as crypto

fn run() {
    signing :: crypto.SigningKey.new_random() ?? return
    recipient :: crypto.X25519SecretKey.new_random() ?? return
    sealed :: crypto.seal([recipient.public_key()], [1, 2, 3], []) ?? return
    crypto.open(signing, sealed, []) ?? return
}
"#,
        &["X25519SecretKey", "SigningKey"],
    );
}

#[test]
fn typed_crypto_matches_aot_in_default_dev_with_honest_jit_boundary() {
    if !have_rustc() {
        return;
    }
    std::fs::create_dir_all(std::env::temp_dir()).unwrap();
    let _ffi_lock = FfiBridgeLock::acquire();
    let path = "examples/features/crypto/typed_crypto.jet";
    let source = std::fs::read_to_string(path).unwrap();
    let expected = std::fs::read_to_string("examples/features/expected/crypto/typed_crypto.out")
        .unwrap();

    let mut bundle = jet::Loader::load_entry(path).expect("typed crypto bundle loads");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "typed crypto must type-check: {errors:?}");
    assert_eq!(
        jet_jit::try_compile_bundle(&bundle),
        Err("run: jit result status unsupported".to_string()),
        "resident JIT boundary changed; either prove native coverage or update the exact gap"
    );

    let (aot_code, aot_stdout, aot_stderr) =
        build_and_run("jet_crypto_c12", "typed_crypto", &source);
    assert_eq!(aot_code, 0);
    assert_eq!(aot_stderr, "");
    assert_eq!(aot_stdout, expected);

    let helper = jetpack::FFI::cached_crypto_helper_path();
    let cache_root = helper
        .ancestors()
        .nth(4)
        .expect("crypto helper lives below its FFI cache root");
    let manifest = std::fs::read_to_string(cache_root.join("Cargo.toml"))
        .expect("actual emitted crypto bridge manifest exists");
    let dependencies = manifest
        .split_once("[dependencies]\n")
        .expect("crypto bridge has dependencies")
        .1;
    assert_eq!(
        dependencies,
        concat!(
            "aes-gcm = { version = \"=0.10.3\", default-features = false, features = [\"aes\", \"alloc\"] }\n",
            "argon2 = { version = \"=0.5.3\", default-features = false, features = [\"alloc\", \"password-hash\"] }\n",
            "blake3 = { version = \"=1.8.2\", default-features = false, features = [\"std\", \"pure\"] }\n",
            "chacha20poly1305 = { version = \"=0.10.1\", default-features = false, features = [\"alloc\"] }\n",
            "ed25519-dalek = { version = \"=2.2.0\", default-features = false, features = [\"alloc\", \"zeroize\"] }\n",
            "hkdf = { version = \"=0.12.4\", default-features = false, features = [] }\n",
            "sha2 = { version = \"=0.10.9\", default-features = false, features = [] }\n",
            "subtle = { version = \"=2.6.1\", default-features = false, features = [] }\n",
            "x25519-dalek = { version = \"=2.0.1\", default-features = false, features = [\"precomputed-tables\", \"zeroize\"] }\n",
        ),
        "generated crypto dependencies must be exact and feature-minimal",
    );

    let dev = match dev_iteration(path, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => (exit_code, stdout, stderr),
        RunOutcome::Problems(diagnostics) => {
            panic!("default dev rejected typed crypto: {diagnostics:?}")
        }
    };
    assert_eq!(dev, (aot_code, aot_stdout, aot_stderr));
}
