//! D-LIB-EXPORT1=C / D-LIB-NAME1=A end-to-end proof.
//!
//! The fixture is deliberately copied into a scratch Package. The test then
//! builds the loadable/native projection, calls the generated C surface, and
//! runs the same host through the default JIT and release AOT lenses.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod common;
use common::{have_rustc, Scratch};

fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = target.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            fs::copy(&from, &to).unwrap();
        }
    }
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn expected_output() -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples/features/expected/packages/library_loadable.out"),
    )
    .unwrap()
}

fn run_jet(root: &Path, args: &[&str]) -> Output {
    Command::new(jet_bin())
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("jet {:?} could not start: {error}", args))
}

fn compiler_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn cc() -> Option<&'static str> {
    ["cc", "gcc", "clang"]
        .into_iter()
        .find(|candidate| Command::new(candidate).arg("--version").output().is_ok())
}

#[test]
fn native_and_component_exports_share_one_typed_surface() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/packages/library_loadable");
    let scratch = Scratch::new("embedding-parity");
    copy_tree(&fixture, &scratch.path);
    let source = scratch.path.join("library.jet");
    let source = source.to_string_lossy();

    let library = jet::compile_library(&source, None)
        .expect("native Library codegen should accept the fixture")
        .library
        .expect("native Library artifacts");
    let plugin = jet::compile_plugin(&source)
        .expect("sandbox Component codegen should accept the fixture")
        .plugin
        .expect("sandbox Component artifacts");

    let library_rows: Vec<_> = library
        .exports
        .iter()
        .map(|export| {
            (
                export.name.clone(),
                export.scalar,
                export.conventions.clone(),
            )
        })
        .collect();
    let plugin_rows: Vec<_> = plugin
        .exports
        .iter()
        .map(|export| (export.name.clone(), export.scalar, export.params.clone()))
        .collect();
    assert_eq!(library_rows, plugin_rows);
    assert_eq!(
        plugin.exported_fns,
        library
            .exports
            .iter()
            .map(|export| export.name.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn library_build_load_and_foreign_call_are_one_surface() {
    if !have_rustc() {
        eprintln!("note: skipping Library end-to-end proof (need rustc)");
        return;
    }
    let Some(cc) = cc() else {
        eprintln!("note: skipping Library end-to-end proof (need a C compiler)");
        return;
    };

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/packages/library_loadable");
    let scratch = Scratch::new("library-loadable");
    copy_tree(&fixture, &scratch.path);
    let expected = expected_output();

    let build = run_jet(&scratch.path, &["build", "--lib", "library.jet"]);
    assert!(
        build.status.success(),
        "Library build failed:\n{}",
        compiler_text(&build)
    );

    let target = scratch.path.join("target");
    let shared = if cfg!(target_os = "macos") {
        target.join("libloadable.dylib")
    } else if cfg!(target_os = "windows") {
        target.join("libloadable.dll")
    } else {
        target.join("libloadable.so")
    };
    for path in [
        target.join("libloadable.a"),
        target.join("loadable.h"),
        target.join("loadable.jetlib"),
        target.join("bindings/loadable.h"),
        target.join("bindings/loadable.py"),
        target.join("bindings/loadable.swift"),
        shared,
    ] {
        assert!(path.is_file(), "Library build missed {}", path.display());
    }
    let header_text = fs::read_to_string(target.join("loadable.h")).unwrap();
    assert!(header_text.contains("int64_t on_tick(int64_t p0);"));
    assert!(header_text.contains("bool is_enabled(bool p0);"));
    assert!(header_text.contains("JetText greet(JetText p0);"));
    assert!(header_text.contains("void jet_text_free(JetText value);"));

    let c_source = scratch.path.join("foreign.c");
    assert!(c_source.is_file(), "missing checked-in C host example");
    let c_binary = scratch.path.join("foreign");
    let mut c_build = Command::new(cc);
    c_build
        .arg("-std=c11")
        .arg("-I")
        .arg(&target)
        .arg(&c_source)
        .arg(target.join("libloadable.a"))
        .arg("-o")
        .arg(&c_binary);
    if cfg!(target_os = "linux") {
        c_build.args(["-ldl", "-lpthread", "-lm"]);
    }
    let c_result = c_build.output().unwrap();
    assert!(
        c_result.status.success(),
        "foreign C caller failed:\n{}",
        compiler_text(&c_result)
    );
    let foreign = Command::new(&c_binary).output().unwrap();
    assert!(
        foreign.status.success(),
        "foreign C caller failed at runtime"
    );
    assert_eq!(String::from_utf8_lossy(&foreign.stdout), expected.as_str());

    let jit = run_jet(&scratch.path, &["run", "host.jet"]);
    assert!(
        jit.status.success(),
        "default jet run failed:\n{}",
        compiler_text(&jit)
    );
    assert_eq!(String::from_utf8_lossy(&jit.stdout), expected.as_str());

    let aot = run_jet(&scratch.path, &["run", "--release", "host.jet"]);
    assert!(
        aot.status.success(),
        "AOT jet run failed:\n{}",
        compiler_text(&aot)
    );
    assert_eq!(String::from_utf8_lossy(&aot.stdout), expected.as_str());

    let jetlib = target.join("loadable.jetlib");
    let original = fs::read(&jetlib).unwrap();
    let mut artifact = jet::JetLibArtifact::decode(&original).unwrap();
    artifact.stamp.compiler_version = "0.0.1-foreign".to_string();
    artifact.stamp.declared_effects.insert("Net".to_string());
    artifact.payload = b"not a shared object".to_vec();
    fs::write(&jetlib, artifact.encode()).unwrap();
    let mismatched = run_jet(&scratch.path, &["run", "host.jet"]);
    assert!(
        !mismatched.status.success(),
        "mismatched .jetlib unexpectedly loaded"
    );
    let mismatched_text = compiler_text(&mismatched);
    assert!(
        mismatched_text.contains("E1338"),
        "missing identity refusal:\n{mismatched_text}"
    );
    assert!(
        !mismatched_text.contains("cannot map library payload"),
        "identity refusal happened after mapping:\n{mismatched_text}"
    );

    artifact.stamp.compiler_version = jet::Manifest::COMPILER_VERSION.to_string();
    fs::write(&jetlib, artifact.encode()).unwrap();
    let over_granted = run_jet(&scratch.path, &["run", "host.jet"]);
    assert!(
        !over_granted.status.success(),
        "ungranted .jetlib unexpectedly loaded"
    );
    let over_granted_text = compiler_text(&over_granted);
    assert!(
        over_granted_text.contains("E1339"),
        "missing effect refusal:\n{over_granted_text}"
    );
    assert!(
        !over_granted_text.contains("cannot map library payload"),
        "effect refusal happened after mapping:\n{over_granted_text}"
    );
}

#[test]
fn component_build_load_and_foreign_call_are_one_surface() {
    if !have_rustc() {
        eprintln!("note: skipping Component end-to-end proof (need rustc)");
        return;
    }
    if Command::new("wasm-tools")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("note: skipping Component end-to-end proof (need wasm-tools)");
        return;
    }

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/packages/library_loadable");
    let scratch = Scratch::new("component-loadable");
    copy_tree(&fixture, &scratch.path);

    let build = run_jet(&scratch.path, &["build", "--target=sandbox", "library.jet"]);
    assert!(
        build.status.success(),
        "Component build failed:\n{}",
        compiler_text(&build)
    );
    let component = scratch.path.join("build/library.wasm");
    assert!(
        component.is_file(),
        "Component build missed {}",
        component.display()
    );

    let host = run_jet(&scratch.path, &["run", "component_host.jet"]);
    assert!(
        host.status.success(),
        "sandboxed Component host failed:\n{}",
        compiler_text(&host)
    );
    assert_eq!(
        String::from_utf8_lossy(&host.stdout),
        "42|true|hello, Ada!\n"
    );
}
