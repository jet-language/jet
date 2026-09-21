mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;
use std::fs;
use std::process::Command;


use jet_foundation::MIR::{
    MirArtifactBuildMode, MirArtifactKind, MirArtifactRequest, MirArtifactTarget,
    MirOptimizationDecision,
};
use tir_support::{build_and_run_full, have_rustc, interpreter_run, jit_run};

const SOURCE: &str = r#"
fn auto(values: [Float#4]) -> [Float#4] {
    output := [Float#4]{0.0, 0.0, 0.0, 0.0}
    loop i in 0..<4 {
        output[i] = values[i] * 2.0 + 1.0
    }
    return output
}

#Scalar
fn scalar(values: [Float#4]) -> [Float#4] {
    output := [Float#4]{0.0, 0.0, 0.0, 0.0}
    loop i in 0..<4 {
        output[i] = values[i] * 2.0 + 1.0
    }
    return output
}

fn run() {
    values := [Float#4]{1.0, 2.0, 3.0, 4.0}
    loop value in auto(values) { print(value) }
    loop value in scalar(values) { print(value) }
}
"#;

fn lower_checked(source: &str) -> jet_foundation::MIR::MirProgram {
    let root = common::unique_tmp("jet_simd_witness");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("package.jet"),
        "name: \"simd_witness\"\nversion: \"1.0.0\"\n",
    )
    .unwrap();
    let entry = root.join("main.jet");
    std::fs::write(&entry, source).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    let request = MirArtifactRequest::new(
        MirArtifactTarget::Cranelift,
        MirArtifactKind::NativeExecutable,
        MirArtifactBuildMode::Dev,
    );
    jet::Codegen::TIR::lower_checked_mir_program_for(&bundle, request)
        .expect("witness lowers through canonical MIR")
        .0
}

fn mir_function<'a>(
    program: &'a jet_foundation::MIR::MirProgram,
    name: &str,
) -> &'a jet_foundation::MIR::MirFunction {
    program
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("missing MIR function {name}"))
}

fn assert_native_simd_artifact(expected: &str) {
    let scratch = common::Scratch::new("jet_simd_native_artifact");
    let source_path = scratch.join("main.jet");
    fs::write(&source_path, SOURCE).unwrap();
    let shown = source_path.to_string_lossy().into_owned();
    let output = jet::compile_with_path(SOURCE, &shown)
        .expect("SIMD witness must compile for native artifact inspection");
    let rust_path = scratch.join("main.rs");
    let binary_path = scratch.join("main");
    fs::write(&rust_path, &output.rust).unwrap();
    let rustc = Command::new("rustc")
        .args(["--edition", "2021", "-C", "opt-level=3", "-C", "debuginfo=1"])
        .arg(&rust_path)
        .arg("-o")
        .arg(&binary_path)
        .output()
        .expect("spawn rustc for SIMD native artifact");
    assert!(
        rustc.status.success(),
        "rustc rejected SIMD witness generated Rust:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&binary_path)
        .output()
        .expect("run SIMD native artifact");
    assert_eq!(
        run.status.code(),
        Some(0),
        "SIMD native artifact failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);

    if !cfg!(any(target_arch = "x86", target_arch = "x86_64")) {
        return;
    }
    let has_objdump = Command::new("objdump")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !has_objdump {
        eprintln!("note: skipping SIMD instruction inspection (objdump unavailable)");
        return;
    }
    let disassembly = Command::new("objdump")
        .arg("-d")
        .arg(&binary_path)
        .output()
        .expect("inspect SIMD native artifact");
    assert!(
        disassembly.status.success(),
        "objdump rejected SIMD native artifact:\n{}",
        String::from_utf8_lossy(&disassembly.stderr)
    );
    let disassembly = String::from_utf8_lossy(&disassembly.stdout).to_ascii_lowercase();
    assert!(
        disassembly.contains("mulpd")
            || disassembly.contains("vmulpd")
            || disassembly.contains("addpd")
            || disassembly.contains("vaddpd"),
        "eligible SIMD witness must leave packed arithmetic in its binary"
    );
}

fn assert_web_simd_artifacts_run(expected: &str) {
    let output = jet::compile_web_with_path(SOURCE, "simd_witness_web.jet")
        .expect("SIMD witness must compile for Web");
    let web = output
        .web
        .expect("SIMD witness must produce Web artifacts");
    assert!(
        web.wasm_rust.contains("jet_entry_"),
        "Web SIMD witness must retain an entry point"
    );

    let have_tool = |name: &str| {
        Command::new(name)
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    };
    let have_wasm_target = Command::new("rustc")
        .args([
            "--print",
            "target-libdir",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !have_tool("rustc") || !have_tool("node") || !have_wasm_target {
        eprintln!("note: skipping SIMD Web execution (need rustc, wasm32 target, and node)");
        return;
    }

    let scratch = common::Scratch::new("jet_simd_web_artifact");
    fs::write(scratch.join("app.js"), &web.js_app).unwrap();
    fs::write(scratch.join("jet_dom_runtime.js"), &web.dom_runtime).unwrap();
    fs::write(scratch.join("app_wasm.rs"), &web.wasm_rust).unwrap();
    fs::write(scratch.join("package.json"), r#"{"type":"module"}"#).unwrap();
    let wasm = Command::new("rustc")
        .current_dir(&scratch.path)
        .args([
            "--edition",
            "2021",
            "--target",
            "wasm32-unknown-unknown",
            "--crate-type",
            "cdylib",
            "-O",
            "app_wasm.rs",
            "-o",
            "app.wasm",
        ])
        .output()
        .expect("spawn Web rustc for SIMD witness");
    assert!(
        wasm.status.success(),
        "rustc rejected SIMD Web output:\n{}",
        String::from_utf8_lossy(&wasm.stderr)
    );
    let node = Command::new("node")
        .current_dir(&scratch.path)
        .arg("app.js")
        .output()
        .expect("spawn Web SIMD witness");
    assert!(
        node.status.success(),
        "Node rejected SIMD Web output: stdout={} stderr={}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&node.stdout), expected);
}

#[test]
fn default_float_loop_is_packed_and_scalar_marker_is_a_hard_opt_out() {
    let optimized = lower_checked(SOURCE);
    let auto = mir_function(&optimized, "auto");
    let scalar = mir_function(&optimized, "scalar");
    let fact = auto
        .optimization
        .vector_facts
        .iter()
        .find(|fact| fact.decision.is_eligible())
        .unwrap_or_else(|| {
            panic!(
                "default Float loop must retain an eligible vector fact: {:#?}",
                auto.optimization.vector_facts
            )
        });
    assert!(matches!(fact.lane_width, Some(2 | 4)));
    assert!(scalar
        .optimization
        .vector_facts
        .iter()
        .all(|fact| !fact.decision.is_eligible()));
    assert!(scalar
        .optimization
        .vector_facts
        .iter()
        .any(|fact| matches!(fact.decision, MirOptimizationDecision::Rejected(_))));
}

#[test]
fn default_and_scalar_float_witnesses_print_identical_values_on_every_tier() {
    let expected = "3.0\n5.0\n7.0\n9.0\n3.0\n5.0\n7.0\n9.0\n";
    let (jit_code, jit_stdout, jit_stderr) = jit_run("simd_default_scalar", SOURCE);
    assert_eq!(jit_code, 0, "default run failed: {jit_stderr}");
    assert_eq!(jit_stdout, expected);
    let (interpreter_code, interpreter_stdout, interpreter_stderr) =
        interpreter_run("simd_default_scalar", SOURCE);
    assert_eq!(interpreter_code, jit_code, "interpreter failed: {interpreter_stderr}");
    assert_eq!(interpreter_stdout, expected);
    if have_rustc() {
        let (aot_code, aot_stdout, aot_stderr) =
            build_and_run_full("jet_simd_witness", "simd_default_scalar", SOURCE);
        assert_eq!(aot_code, jit_code, "AOT failed: {aot_stderr}");
        assert_eq!(aot_stdout, expected);
    }
    if have_rustc() {
        assert_native_simd_artifact(expected);
    }
    assert_web_simd_artifacts_run(expected);
}
