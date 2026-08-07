//! D-RINGLAYER1=A M1: runtime profile inference and package ceilings.

use std::fs;
use std::path::PathBuf;

fn tmp_project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jet_ring_layer_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn manifest_parses_layer_ceiling() {
    let raw = r#"
name: "embed"
version: "0.1.0"
runtime: alloc
"#;
    let pm = jetpack::Package::PackageFacts::parse(raw, "test").unwrap();
    assert_eq!(pm.layer, Some(jet::Syntax::RuntimeLayer::Alloc));
    let mf = jetpack::Package::to_manifest(&pm, raw).unwrap();
    assert_eq!(mf.package.layer, Some(jet::Syntax::RuntimeLayer::Alloc));
}

#[test]
fn manifest_rejects_unknown_layer() {
    let raw = "name: \"x\"\nversion: \"1\"\nruntime: heap\n";
    let err = jetpack::Package::PackageFacts::parse(raw, "test").unwrap_err();
    assert!(matches!(
        err,
        jetpack::Package::PackageParseError::InvalidValue { .. }
    ));
}

#[test]
fn manifest_rejects_retired_std_runtime_name() {
    let raw = "name: \"x\"\nversion: \"1\"\nruntime: std\n";
    let err = jetpack::Package::PackageFacts::parse(raw, "test").unwrap_err();
    assert!(matches!(
        err,
        jetpack::Package::PackageParseError::InvalidValue { .. }
    ));
}

#[test]
fn math_import_infers_core_layer() {
    let dir = tmp_project("math");
    fs::write(
        dir.join("pkg.jet"),
        "name: \"m\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    let main = r#"
use core.math as math

fn run() {
    print(math.sqrt(9.0))
}
"#;
    fs::write(dir.join("main.jet"), main).unwrap();
    let main_path = dir.join("main.jet");
    let path = main_path.to_str().unwrap();
    let mut bundle = jet::Loader::load_entry(path).unwrap();
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == jet::Diagnostics::Severity::Error),
        "{}",
        jet::render_diagnostics(path, main, &diags)
    );
    assert_eq!(bundle.inferred_layer, jet::Syntax::RuntimeLayer::Core);
}

#[test]
fn solve_import_infers_alloc_layer() {
    let dir = tmp_project("solve");
    fs::write(
        dir.join("pkg.jet"),
        "name: \"m\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    let main = r#"
use core.solve as solve

fn run() {
    solver := solve.Solver.new(1)
    solver.require(true)
}
"#;
    fs::write(dir.join("main.jet"), main).unwrap();
    let main_path = dir.join("main.jet");
    let path = main_path.to_str().unwrap();
    let mut bundle = jet::Loader::load_entry(path).unwrap();
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == jet::Diagnostics::Severity::Error),
        "{}",
        jet::render_diagnostics(path, main, &diags)
    );
    assert_eq!(bundle.inferred_layer, jet::Syntax::RuntimeLayer::Alloc);
}

#[test]
fn fs_import_infers_std_layer() {
    let dir = tmp_project("fs");
    fs::write(
        dir.join("pkg.jet"),
        "name: \"m\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    let main = r#"
use core.files as fs

fn run() {
    print("ok")
}
"#;
    fs::write(dir.join("main.jet"), main).unwrap();
    let main_path = dir.join("main.jet");
    let path = main_path.to_str().unwrap();
    let mut bundle = jet::Loader::load_entry(path).unwrap();
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == jet::Diagnostics::Severity::Error),
        "{}",
        jet::render_diagnostics(path, main, &diags)
    );
    assert_eq!(bundle.inferred_layer, jet::Syntax::RuntimeLayer::Std);
}

#[test]
fn ceiling_blocks_std_import() {
    let dir = tmp_project("ceiling");
    fs::write(
        dir.join("pkg.jet"),
        "name: \"m\"\nversion: \"0.1.0\"\nruntime: core\n",
    )
    .unwrap();
    let main = r#"
use core.files as fs

fn run() {
    print("ok")
}
"#;
    fs::write(dir.join("main.jet"), main).unwrap();
    let path = dir.join("main.jet");
    let shown = path.to_string_lossy();
    let diags = jet::check_with_path(&shown);
    assert!(
        diags.iter().any(|d| d.code == "E1006"),
        "expected E1006, got: {}",
        jet::render_diagnostics(&shown, main, &diags)
    );
}

#[test]
fn alloc_ceiling_allows_mem_not_fs() {
    let dir = tmp_project("alloc_ok");
    fs::write(
        dir.join("pkg.jet"),
        "name: \"m\"\nversion: \"0.1.0\"\nruntime: alloc\n",
    )
    .unwrap();
    let main = r#"
use core.mem as mem

fn run() {
    print("ok")
}
"#;
    fs::write(dir.join("main.jet"), main).unwrap();
    let main_path = dir.join("main.jet");
    let path = main_path.to_str().unwrap();
    let mut bundle = jet::Loader::load_entry(path).unwrap();
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        !diags.iter().any(|d| d.code == "E1006"),
        "mem import should be allowed under alloc ceiling"
    );
    assert_eq!(bundle.inferred_layer, jet::Syntax::RuntimeLayer::Alloc);
}

#[test]
fn ambient_input_infers_std_layer() {
    let dir = tmp_project("input");
    fs::write(
        dir.join("pkg.jet"),
        "name: \"m\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    let main = r#"
fn run() {
    _ :: input("name? ")
    print("ok")
}
"#;
    fs::write(dir.join("main.jet"), main).unwrap();
    let main_path = dir.join("main.jet");
    let path = main_path.to_str().unwrap();
    let mut bundle = jet::Loader::load_entry(path).unwrap();
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == jet::Diagnostics::Severity::Error),
        "{}",
        jet::render_diagnostics(path, main, &diags)
    );
    assert_eq!(bundle.inferred_layer, jet::Syntax::RuntimeLayer::Std);
}

#[test]
fn ceiling_blocks_ambient_input_helper() {
    let dir = tmp_project("input_ceiling");
    fs::write(
        dir.join("pkg.jet"),
        "name: \"m\"\nversion: \"0.1.0\"\nruntime: core\n",
    )
    .unwrap();
    let main = r#"
fn run() {
    _ :: input("name? ")
    print("ok")
}
"#;
    fs::write(dir.join("main.jet"), main).unwrap();
    let path = dir.join("main.jet");
    let shown = path.to_string_lossy();
    let diags = jet::check_with_path(&shown);
    assert!(
        diags.iter().any(|d| d.code == "E1006"),
        "expected E1006 for ambient input under core ceiling"
    );
    let e1006 = diags.iter().find(|d| d.code == "E1006").unwrap();
    assert!(
        e1006.why.contains("import chain:"),
        "E1006 should include import chain: {}",
        e1006.why
    );
    assert!(
        e1006.why.contains("input()"),
        "chain should name ambient input: {}",
        e1006.why
    );
}

#[test]
fn lock_roundtrip_layer_metadata() {
    use jet::Lock::{LockFile, LockSource, LockedPackage};
    let pkg = LockedPackage {
        name: "embed".into(),
        version: "0.1.0".into(),
        source: LockSource::Root,
        locked: None,
        fingerprint: String::new(),
        content_hash: None,
        dependencies: vec![],
        layer: Some(jet::Syntax::RuntimeLayer::Alloc),
        inferred_layer: Some(jet::Syntax::RuntimeLayer::Std),
        effects: vec![],
        effect_grants: vec![],
        envelope: None,
    };
    let lock = LockFile {
        version: 1,
        packages: vec![pkg],
        root_dependencies: vec![],
        workspace_members: vec![],
        workspace_source_digest: None,
        workspace_overlay_policy: Default::default(),
        comptime_inputs: vec![],
        toolchains: Vec::new(),
        browsers: Vec::new(),
        source_channels: Vec::new(),
    };
    let raw = jet::Lock::write(&lock);
    let parsed = jet::Lock::parse(&raw).unwrap();
    assert_eq!(
        parsed.packages[0].layer,
        Some(jet::Syntax::RuntimeLayer::Alloc)
    );
    assert_eq!(
        parsed.packages[0].inferred_layer,
        Some(jet::Syntax::RuntimeLayer::Std)
    );
}
