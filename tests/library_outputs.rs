//! D-LIB-EXPORT1=C / D-LIB-NAME1=A end-to-end proof.
//!
//! The fixture is deliberately copied into a scratch Package. The test then
//! builds the loadable/native projection, calls the generated C surface, and
//! runs the same host through the default JIT and release AOT lenses.

#![cfg(unix)]

use std::collections::{BTreeMap, BTreeSet};
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

fn expected_cpp_output() -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples/features/expected/packages/library_loadable_cpp.out"),
    )
    .unwrap()
}

fn run_jet_with_pid(root: &Path, args: &[&str]) -> (u32, Output) {
    let mut child = Command::new(jet_bin())
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .spawn()
        .unwrap_or_else(|error| panic!("jet {:?} could not start: {error}", args));
    let pid = child.id();
    let output = child
        .wait_with_output()
        .unwrap_or_else(|error| panic!("jet {:?} could not finish: {error}", args));
    (pid, output)
}

fn run_jet(root: &Path, args: &[&str]) -> Output {
    run_jet_with_pid(root, args).1
}

fn compiler_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn staged_loader_files(pid: u32) -> BTreeSet<String> {
    let prefix = format!("jet-mod-{pid}-");
    fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with(&prefix).then_some(name)
        })
        .collect()
}

fn all_staged_loader_files() -> BTreeSet<String> {
    fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("jet-mod-").then_some(name)
        })
        .collect()
}

fn cc() -> Option<&'static str> {
    ["cc", "gcc", "clang"]
        .into_iter()
        .find(|candidate| Command::new(candidate).arg("--version").output().is_ok())
}

fn cxx() -> Option<&'static str> {
    ["c++", "g++", "clang++"]
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
    assert!(have_rustc(), "Library end-to-end proof requires rustc");
    let cc = cc().expect("Library end-to-end proof requires a C compiler");

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
    ] {
        assert!(path.is_file(), "Library build missed {}", path.display());
    }
    assert!(shared.is_file(), "Library build missed {}", shared.display());
    let artifact = jet::JetLibArtifact::decode(&fs::read(target.join("loadable.jetlib")).unwrap())
        .expect("loadable Library metadata should decode");
    assert_eq!(artifact.stamp.library_name, "loadable");
    assert_eq!(artifact.stamp.abi_version, jet::JetLib::ABI_VERSION);
    assert_eq!(
        artifact.stamp.exports,
        vec![
            jet::JetLibExport::new("on_tick", jet::JetLibScalar::Int, 1),
            jet::JetLibExport::new("is_enabled", jet::JetLibScalar::Bool, 1),
            jet::JetLibExport::new("greet", jet::JetLibScalar::Text, 1),
        ]
    );
    jet::JetLib::validate_load_metadata(&artifact.stamp)
        .expect("generated Library metadata should match the loader ABI");
    let completion = fs::read_to_string(target.join(".loadable.jet-library.complete")).unwrap();
    assert!(completion.starts_with("jet-library-set-v1\n"));
    for (name, path) in [
        ("libloadable.a", target.join("libloadable.a")),
        ("loadable.h", target.join("loadable.h")),
        ("loadable.jetlib", target.join("loadable.jetlib")),
    ] {
        let digest = jet::SHA256::sha256_hex(&fs::read(path).unwrap());
        assert!(
            completion.contains(&format!("{name}\tsha256-{digest}\n")),
            "completion marker missed {name}"
        );
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

    let deterministic_paths = [
        target.join("libloadable.a"),
        shared.clone(),
        target.join("loadable.jetlib"),
        target.join("loadable.h"),
        target.join("bindings/loadable.h"),
        target.join("bindings/loadable.py"),
        target.join("bindings/loadable.swift"),
    ];
    let deterministic_before: Vec<_> = deterministic_paths
        .iter()
        .map(|path| fs::read(path).unwrap())
        .collect();
    let rebuild = run_jet(&scratch.path, &["build", "--lib", "library.jet"]);
    assert!(
        rebuild.status.success(),
        "repeat Library build failed:\n{}",
        compiler_text(&rebuild)
    );
    for (path, before) in deterministic_paths.iter().zip(deterministic_before) {
        assert_eq!(fs::read(path).unwrap(), before, "non-deterministic {}", path.display());
    }

    let cxx = cxx().expect("C++ Library host proof requires a C++ compiler");
    let cpp_source = scratch.path.join("foreign.cpp");
    assert!(cpp_source.is_file(), "missing checked-in C++ host example");
    let cpp_binary = scratch.path.join("foreign-cpp");
    let mut cpp_build = Command::new(cxx);
    cpp_build
        .arg("-std=c++17")
        .arg("-I")
        .arg(&target)
        .arg(&cpp_source)
        .arg("-o")
        .arg(&cpp_binary)
        .arg("-pthread");
    if cfg!(target_os = "linux") {
        cpp_build.arg("-ldl");
    }
    let cpp_result = cpp_build.output().unwrap();
    assert!(
        cpp_result.status.success(),
        "foreign C++ host failed to compile:\n{}",
        compiler_text(&cpp_result)
    );
    let cpp = Command::new(&cpp_binary)
        .arg(&shared)
        .output()
        .unwrap();
    assert!(
        cpp.status.success(),
        "foreign C++ host failed at runtime:\n{}",
        compiler_text(&cpp)
    );
    let expected_cpp = expected_cpp_output();
    assert_eq!(String::from_utf8_lossy(&cpp.stdout), expected_cpp.as_str());

    let panic_scratch = Scratch::new("library-panic");
    copy_tree(&fixture, &panic_scratch.path);
    fs::write(
        panic_scratch.path.join("library.jet"),
        "#Export(c) pub fn panic_now(value: Int) Int -> panic(\"foreign panic {value}\")\n",
    )
    .unwrap();
    let panic_build = run_jet(&panic_scratch.path, &["build", "--lib", "library.jet"]);
    assert!(
        panic_build.status.success(),
        "panic Library build failed:\n{}",
        compiler_text(&panic_build)
    );
    let panic_shared = if cfg!(target_os = "macos") {
        panic_scratch.path.join("target/libloadable.dylib")
    } else if cfg!(target_os = "windows") {
        panic_scratch.path.join("target/libloadable.dll")
    } else {
        panic_scratch.path.join("target/libloadable.so")
    };
    assert!(
        panic_shared.is_file(),
        "panic Library build missed {}",
        panic_shared.display()
    );
    let panic = Command::new(&cpp_binary)
        .args(["--panic", panic_shared.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        !panic.status.success(),
        "panic crossed the C ABI as a successful C++ call"
    );
    let panic_text = compiler_text(&panic);
    assert!(
        panic_text.contains("foreign panic"),
        "panic did not report a runtime panic:\n{panic_text}"
    );

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

    let mut malformed = jet::JetLibArtifact::decode(&original).unwrap();
    malformed.payload = b"not a shared object".to_vec();
    fs::write(&jetlib, malformed.encode()).unwrap();
    let before_staging = all_staged_loader_files();
    let (loader_pid, invalid_payload) = run_jet_with_pid(&scratch.path, &["run", "host.jet"]);
    assert!(!invalid_payload.status.success());
    let invalid_payload_text = compiler_text(&invalid_payload);
    assert!(
        invalid_payload_text.contains("cannot map library payload"),
        "missing native mapping diagnostic:\n{invalid_payload_text}"
    );
    assert_eq!(
        staged_loader_files(loader_pid),
        before_staging
            .into_iter()
            .filter(|name| name.starts_with(&format!("jet-mod-{loader_pid}-")))
            .collect(),
        "failed native mapping left a staged payload behind"
    );

    malformed = jet::JetLibArtifact::decode(&original).unwrap();
    malformed.stamp.target = "foreign-target".to_string();
    fs::write(&jetlib, malformed.encode()).unwrap();
    let wrong_target = run_jet(&scratch.path, &["run", "host.jet"]);
    assert!(!wrong_target.status.success());
    let wrong_target_text = compiler_text(&wrong_target);
    assert!(
        wrong_target_text.contains("E1341") && wrong_target_text.contains("targets"),
        "missing target diagnostic:\n{wrong_target_text}"
    );
    assert!(
        !wrong_target_text.contains("cannot map library payload"),
        "target refusal happened after mapping:\n{wrong_target_text}"
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

#[test]
fn library_rejects_colliding_c_symbols_before_codegen() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/packages/library_loadable");
    let scratch = Scratch::new("library-symbol-collision");
    copy_tree(&fixture, &scratch.path);
    fs::write(
        scratch.path.join("library.jet"),
        "#Export(c) pub fn café() Int -> 1\n#Export(c) pub fn cafö() Int -> 2\n#Export(c) pub fn jet_text_free() String -> \"bad\"\n",
    )
    .unwrap();

    let source = scratch.path.join("library.jet");
    let source = source.to_string_lossy();
    let errors = jet::compile_library(&source, None).expect_err("colliding C symbols accepted");
    assert!(
        errors.iter().any(|error| {
            error.code == "E1341" && error.what.contains("same C symbol")
        }),
        "missing C symbol collision diagnostic: {errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|error| error.code == "E1341" && error.what.contains("generated C symbol")),
        "missing generated allocator symbol diagnostic: {errors:?}"
    );
}

#[test]
fn library_output_selection_is_named_and_fail_closed() {
    let scratch = Scratch::new("library-output-selection");
    fs::write(
        scratch.path.join("package.jet"),
        "name: \"library-selection\"\nversion: \"0.1.0\"\noutputs: .{\n    alpha: .Library{ name: \"alpha\", entry: run, native: true }\n    beta: .Library{ name: \"beta\", entry: run, native: true }\n    app: .Executable{ name: \"app\", entry: run }\n}\n",
    )
    .unwrap();
    fs::write(
        scratch.path.join("library.jet"),
        "#Export(c) pub fn on_tick(dt: Int) Int -> dt + 1\nfn run() {}\n",
    )
    .unwrap();
    let source = scratch.path.join("library.jet");
    let source = source.to_string_lossy();

    let ambiguous = jet::compile_library(&source, None).expect_err("ambiguous Library accepted");
    assert!(ambiguous.iter().any(|error| {
        error.code == "E1341"
            && error.what.contains("multiple Library outputs")
            && error.fix.contains("alpha, beta")
    }));

    let selected = jet::compile_library(&source, Some("beta"))
        .expect("named Library output should compile");
    let selected_config = selected.library_config.unwrap();
    assert_eq!(selected_config.name, "beta");
    assert_eq!(selected_config.entry.as_deref(), Some("run"));

    let missing = jet::compile_library(&source, Some("missing"))
        .expect_err("missing Library output accepted");
    assert!(missing.iter().any(|error| {
        error.code == "E1341" && error.what.contains("Library output `missing`")
    }));

    let non_library = jet::compile_library(&source, Some("app"))
        .expect_err("non-Library output accepted");
    assert!(non_library.iter().any(|error| {
        error.code == "E1341" && error.what.contains("output `app` is not a Library")
    }));
}

#[test]
fn locked_library_compile_requires_the_lock_stamp() {
    let scratch = Scratch::new("library-locked");
    fs::write(
        scratch.path.join("package.jet"),
        "name: \"locked-library\"\nversion: \"0.1.0\"\noutputs: .{ core: .Library{ name: \"core\", native: true } }\n",
    )
    .unwrap();
    fs::write(
        scratch.path.join("library.jet"),
        "#Export(c) pub fn on_tick(dt: Int) Int -> dt + 1\n",
    )
    .unwrap();
    let source = scratch.path.join("library.jet");
    let source = source.to_string_lossy();
    let errors = jet::compile_library_with_gates_and_settings(
        &source,
        None,
        jet::Policy::GateSet::default(),
        true,
        &BTreeMap::new(),
    )
    .expect_err("locked Library compile accepted a missing lock stamp");
    assert!(errors.iter().any(|error| error.code == "E3512"));
}

#[test]
fn locked_named_library_build_selects_the_requested_output() {
    assert!(have_rustc(), "locked Library build proof requires rustc");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/foreign_build_hosts/cmake");
    let scratch = Scratch::new("library-locked-build");
    copy_tree(&fixture, &scratch.path);
    let build = run_jet(
        &scratch.path,
        &["build", "--lib", "--locked", "--output", "core", "library.jet"],
    );
    assert!(
        build.status.success(),
        "locked named Library build failed:\n{}",
        compiler_text(&build)
    );
    assert!(scratch.path.join("target/libloadable.a").is_file());
    assert!(scratch.path.join("target/loadable.h").is_file());
    assert!(scratch.path.join("target/loadable.jetlib").is_file());
}

#[test]
fn default_library_rejects_cross_target_before_publication() {
    assert!(have_rustc(), "Library target diagnostic proof requires rustc");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/packages/library_loadable");
    let scratch = Scratch::new("library-target");
    copy_tree(&fixture, &scratch.path);
    let target = format!("--target={}", env!("JET_BUILD_TARGET"));
    let rejected = run_jet(&scratch.path, &["build", target.as_str(), "library.jet"]);
    assert!(
        !rejected.status.success(),
        "cross-target Library build unexpectedly succeeded:\n{}",
        compiler_text(&rejected)
    );
    let text = compiler_text(&rejected);
    assert!(text.contains("E1341"), "missing unsupported-target diagnostic:\n{text}");
    let shared = if cfg!(target_os = "macos") {
        scratch.path.join("target/libloadable.dylib")
    } else if cfg!(target_os = "windows") {
        scratch.path.join("target/libloadable.dll")
    } else {
        scratch.path.join("target/libloadable.so")
    };
    assert!(
        !scratch.path.join("target/libloadable.a").exists()
            && !scratch.path.join("target/loadable.h").exists()
            && !scratch.path.join("target/loadable.jetlib").exists()
            && !shared.exists(),
        "unsupported Library target published artifacts"
    );
}
