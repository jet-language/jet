//! Rust FFI conformance against the unified descriptor and bridge contract.

use std::fs;
use std::process::Command;

mod common;
use common::have_rustc;

fn add_ffi_bridge_args(rustc: &mut Command, link: &jet::FFI::FfiLink) {
    rustc
        .arg("--extern")
        .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
    for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
        rustc
            .arg("-L")
            .arg(format!("dependency={}", deps_dir.display()));
    }
}

#[test]
fn rust_extern_std_round_trip_uses_the_canonical_descriptor() {
    if !have_rustc() || Command::new("cargo").arg("--version").output().is_err() {
        eprintln!("note: skipping Rust FFI conformance (need rustc and cargo)");
        return;
    }

    let root = common::unique_tmp("jet_ffi_rust_conformance");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("main.jet");
    let source = r#"
extern rust "std" {
    fn rust_max(a: Int, b: Int) Int = "std::cmp::max";
}
fn run() {
    print(rust_max(2, 40));
}
"#;
    fs::write(&path, source).unwrap();

    let output = jet::compile_with_path(source, path.to_str().unwrap()).unwrap_or_else(|diags| {
        panic!(
            "Rust FFI source failed the production path:\n{}",
            jet::render_diagnostics(path.to_str().unwrap(), source, &diags)
        )
    });
    let link = output
        .ffi
        .as_ref()
        .expect("extern rust must publish a bridge link");
    let descriptor = jet::AST::binder_descriptor(jet::AST::ForeignLanguage::Rust)
        .expect("Rust must have a canonical binder descriptor")
        .stamp();
    let provenance = fs::read_to_string(&link.provenance_path).unwrap();
    assert!(
        provenance.contains(&format!("descriptor={descriptor}")),
        "Rust bridge provenance must carry the canonical descriptor: {provenance}"
    );
    assert!(
        output.rust.contains("jet_ffi_rust_max"),
        "the production bridge wrapper must reach generated Rust"
    );

    let rust_source = root.join("main.rs");
    let binary = root.join("main_bin");
    fs::write(&rust_source, &output.rust).unwrap();
    let mut rustc = Command::new("rustc");
    rustc
        .args(["--edition", "2021"])
        .arg(&rust_source)
        .arg("-o")
        .arg(&binary);
    add_ffi_bridge_args(&mut rustc, link);
    let built = rustc.output().unwrap();
    assert!(
        built.status.success(),
        "rustc rejected Rust FFI output:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );

    let run = Command::new(&binary).output().unwrap();
    assert!(
        run.status.success(),
        "Rust FFI program failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "40\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rust_extern_signature_mismatch_is_typed_before_a_foreign_call() {
    if !have_rustc() || Command::new("cargo").arg("--version").output().is_err() {
        eprintln!("note: skipping Rust FFI mismatch conformance (need rustc and cargo)");
        return;
    }

    let root = common::unique_tmp("jet_ffi_rust_mismatch");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("main.jet");
    let source = r#"
extern rust "std" {
    fn wrong_result(value: Int) String = "std::convert::identity";
}
fn run() {}
"#;
    fs::write(&path, source).unwrap();

    let diagnostics = jet::compile_with_path(source, path.to_str().unwrap())
        .expect_err("a Rust ABI mismatch must fail before a foreign call");
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E0705")
        .expect("Rust ABI mismatch must use E0705");
    assert!(!diagnostic.what.is_empty());
    assert!(!diagnostic.why.is_empty());
    assert!(!diagnostic.fix.is_empty());
    let rendered = jet::render_diagnostics(path.to_str().unwrap(), source, &diagnostics);
    assert!(rendered.contains("Error [E0705]"), "{rendered}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn c_rust_python_and_javascript_share_the_typed_scalar_contract() {
    use jet::AST::{binder_descriptor, ForeignLanguage, ForeignSafety, ForeignScalar};

    for language in [
        ForeignLanguage::C,
        ForeignLanguage::Rust,
        ForeignLanguage::Py,
        ForeignLanguage::JS,
    ] {
        let descriptor = binder_descriptor(language).expect("conformance language descriptor");
        let contract = descriptor.contract;
        assert_eq!(contract.version, jet::AST::FOREIGN_ABI_CONTRACT_VERSION);
        assert_eq!(contract.integer, ForeignScalar::Int);
        assert_eq!(contract.floating, ForeignScalar::Float);
        assert_eq!(contract.boolean, ForeignScalar::Bool);
        assert_eq!(contract.character, ForeignScalar::Char);
        assert_eq!(contract.string, ForeignScalar::String);
        assert_eq!(contract.safety, ForeignSafety::GeneratedWrapper);
        assert!(descriptor
            .stamp()
            .starts_with(jet::AST::FOREIGN_DESCRIPTOR_SCHEMA));
    }
}
