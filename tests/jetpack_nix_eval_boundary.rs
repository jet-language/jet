use std::fs;
use std::path::Path;

#[test]
fn evaluator_seam_is_no_std_dependency_free_and_unsafe_forbidden() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("crates/jet-nix-eval/Cargo.toml"))
        .expect("native evaluator seam manifest");
    let dependencies = manifest
        .split_once("[dependencies]")
        .expect("evaluator seam must declare an explicit dependency section")
        .1
        .trim();
    assert!(
        dependencies.is_empty(),
        "native evaluator seam must remain dependency-free, got:\n{dependencies}"
    );
    assert!(manifest.lines().any(|line| line.trim() == "build = false"));
    assert!(!root.join("crates/jet-nix-eval/build.rs").exists());

    let seam = fs::read_to_string(root.join("crates/jet-nix-eval/src/lib.rs"))
        .expect("native evaluator seam root");
    assert!(seam.contains("#![no_std]"));
    assert!(seam.contains("#![forbid(unsafe_code)]"));
    assert!(seam.contains("#![deny(clippy::disallowed_types)]"));

    let clippy = fs::read_to_string(root.join("clippy.toml")).expect("workspace Clippy policy");
    assert!(clippy.contains("std::process::Command"));
    let verify = fs::read_to_string(root.join("scripts/agent/verify-full.sh"))
        .expect("full verification entry point");
    assert!(verify.contains("verify-nix-eval-stopline.sh"));
    let escape = fs::read_to_string(
        root.join("tests/fixtures/nix-compat/authority-escape/lib.rs"),
    )
    .expect("native evaluator authority escape fixture");
    assert!(escape.contains("extern crate std as host;"));
    assert!(escape.contains("host::process::Command::new"));
    assert!(escape.contains("host::net::TcpStream as Wire"));
    assert!(escape.contains("host::net::ToSocketAddrs as Resolve"));
    assert!(escape.contains("host::os::unix::net::UnixStream as Wire"));
    let build_escape = fs::read_to_string(
        root.join("tests/fixtures/nix-compat/build-script-escape/build.rs"),
    )
    .expect("native evaluator build-script escape fixture");
    assert!(build_escape.contains("std::process::Command::new"));

    let jetpack_manifest = fs::read_to_string(root.join("crates/jetpack/Cargo.toml"))
        .expect("jetpack manifest");
    assert!(jetpack_manifest.contains("jet-nix-eval = { path = \"../jet-nix-eval\" }"));
    let jetpack = fs::read_to_string(root.join("crates/jetpack/src/lib.rs"))
        .expect("jetpack library root");
    assert!(jetpack.contains("pub(crate) mod NixEval;"));
    assert!(!jetpack.contains("pub mod NixEval;"));
}

#[test]
fn oracle_pin_is_independent_from_mutable_root_flake_lock() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let oracle = fs::read_to_string(root.join("tests/fixtures/nix-compat/oracle.json"))
        .expect("committed oracle manifest");
    assert!(oracle.contains("\"version\": \"2.34.8\""));
    assert!(oracle.contains("b6769c588f60b3e762f73d3a8cf60294df078ccd"));
    assert!(oracle.contains("f3f1c3c5b8ad91850e0f7c590cf177f7ab022024"));
    assert!(oracle.contains("b5aa0fbd538984f6e3d201be0005b4463d8b09f8"));
    assert!(oracle.contains("\"last_modified\": 1782723713"));
    assert!(oracle.contains("sha256-oPXCU/SSUokcGaJREHibG1CBX3+s/W7orDWQOZDsEeQ="));
    assert_eq!(oracle.matches("\"build_nar_hash\": \"").count(), 4);
    assert_eq!(oracle.matches("\"executable_nar_hash\": \"").count(), 4);
    assert_eq!(oracle.matches("\"status\": \"ready\"").count(), 4);
    assert!(oracle.contains("\"corpus_status\": \"bit_exact\""));

    let verifier = fs::read_to_string(root.join("scripts/agent/verify-nix-eval-fixture.js"))
        .expect("pinned oracle verifier");
    assert!(verifier.contains("packages.${system}.nix"));
    assert!(verifier.contains("complete install and its evaluator executable"));
    assert!(verifier.contains("path-info"));
}
