#[path = "tir_support/mod.rs"]
mod tir_support;
mod common;

use std::fs;
use std::process::Command;
use tir_support::{
    build_and_run, have_rustc, run_default_multi, strip_vetted_prelude_modules,
};

const SOURCE: &str = r#"
use core.mem

fn run() {
    bytes := [U8#4].{ uninit }
    bytes[0] = 65
    bytes[1] = 66
    bytes[2] = 67
    bytes[3] = 68
    print("{bytes[0]} {bytes[1]} {bytes[2]} {bytes[3]}")
}
"#;

const WHOLE_VALUE_SOURCE: &str = r#"
use core.mem

fn make() => [U8#2] {
    bytes := [U8#2].{ uninit }
    bytes[0] = 7
    bytes[1] = 9
    return bytes
}

fn first(bytes: [U8#2]) => U8 {
    index :: 0
    return bytes[index]
}

fn run() {
    bytes :: make()
    print(first(bytes))
}
"#;

const MUTATING_BORROW_SOURCE: &str = r#"
use core.mem

fn set_first(bytes: &[U8#2]) {
    bytes[0] = 8
}

fn first(bytes: [U8#2]) => U8 {
    index :: 0
    return bytes[index]
}

fn run() {
    bytes := [U8#2].{ uninit }
    bytes[0] = 1
    bytes[1] = 2
    set_first(&bytes)
    print(bytes[0])
    print(first(bytes))
}
"#;

#[test]
fn fixed_uninit_index_fill_runs_through_aot_without_user_unsafe() {
    let generated = jet::compile(SOURCE).expect("fixed uninit fill should compile");
    let user = strip_vetted_prelude_modules(&generated.rust);
    assert!(user.contains("JetUninitFixed"), "{user}");
    assert!(user.contains(".write("), "{user}");
    assert!(
        !user.contains("unsafe"),
        "fixed uninit unsafe must stay in the vetted core.mem runtime:\n{user}"
    );

    if have_rustc() {
        let (code, stdout) = build_and_run("uninit_fixed_aot", SOURCE);
        assert_eq!(code, 0);
        assert_eq!(stdout, "65 66 67 68\n");
    }
}

#[test]
fn fixed_uninit_index_fill_runs_through_default_tier() {
    let (code, stdout, stderr) =
        run_default_multi("uninit_fixed_default", "main.jet", &[("main.jet", SOURCE)]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "65 66 67 68\n");
}

#[test]
fn fixed_uninit_index_fill_is_resident_jit_safe() {
    let root = common::unique_tmp("jet_uninit_fixed_jit");
    fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.jet");
    fs::write(&entry, SOURCE).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:#?}");

    let detail = jet_jit::resident_jit_safe_bundle_detail(&bundle);
    let compiled = jet_jit::try_compile_bundle(&bundle);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle) && compiled.is_ok(),
        "fixed uninit fill must stay on the resident JIT tier: safety={detail:?}, compile={compiled:?}"
    );
}

#[test]
fn initialized_fixed_storage_is_an_ordinary_fixed_list_value() {
    if have_rustc() {
        let (code, stdout) = build_and_run("uninit_fixed_whole_value", WHOLE_VALUE_SOURCE);
        assert_eq!(code, 0);
        assert_eq!(stdout, "7\n");
    }

    let (code, stdout, stderr) = run_default_multi(
        "uninit_fixed_whole_value",
        "main.jet",
        &[("main.jet", WHOLE_VALUE_SOURCE)],
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "7\n");
}

#[test]
fn initialized_fixed_storage_mutating_borrow_writes_back() {
    let generated = jet::compile(MUTATING_BORROW_SOURCE).unwrap();
    let user = strip_vetted_prelude_modules(&generated.rust);
    assert!(
        user.contains("__jet_set_first((__jet_bytes).as_array_mut())"),
        "{user}"
    );
    if have_rustc() {
        let (code, stdout) =
            build_and_run("uninit_fixed_mutating_borrow", MUTATING_BORROW_SOURCE);
        assert_eq!(code, 0);
        assert_eq!(stdout, "8\n8\n");
    }

    let (code, stdout, stderr) = run_default_multi(
        "uninit_fixed_mutating_borrow",
        "main.jet",
        &[("main.jet", MUTATING_BORROW_SOURCE)],
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "8\n8\n");
}

#[test]
fn scalar_uninit_storage_never_emits_user_unsafe() {
    let source =
        "use core.mem\nfn run() {\n    flag := Bool.{ uninit }\n    flag = true\n    print(flag)\n}\n";
    let generated = jet::compile(source).expect("scalar uninit should compile");
    let user = strip_vetted_prelude_modules(&generated.rust);
    assert!(user.contains("JetUninit::<bool>"), "{user}");
    assert!(
        !user.contains("unsafe"),
        "scalar uninit unsafe must stay in the vetted core.mem runtime:\n{user}"
    );
    if have_rustc() {
        let (code, stdout) = build_and_run("uninit_scalar_safe", source);
        assert_eq!(code, 0);
        assert_eq!(stdout, "true\n");
    }
}

#[test]
fn fixed_uninit_reuses_vetted_storage_on_web() {
    let source = "use core.mem\nfn run() {\n    bytes := [U8#2].{ uninit }\n    bytes[0] = 1\n    bytes[1] = 2\n}\n";
    let web = jet::compile_web_with_path(source, "tests/fixtures/web_uninit_fixed.jet")
        .expect("fixed uninit should compile for the web target")
        .web
        .expect("web output");
    assert!(
        web.wasm_rust
            .contains("jet_mem::JetUninitFixed::<u64, 2>::new()"),
        "{}",
        web.wasm_rust
    );
    assert!(web.wasm_rust.contains(".write("), "{}", web.wasm_rust);
    assert!(
        !web.wasm_rust.contains("vec![0"),
        "web must not replace uninitialized storage with zero-filled values:\n{}",
        web.wasm_rust
    );
    if have_rustc() {
        let root = common::unique_tmp("jet_uninit_fixed_web");
        fs::create_dir_all(&root).unwrap();
        let source = root.join("app_wasm.rs");
        fs::write(&source, &web.wasm_rust).unwrap();
        let output = Command::new("rustc")
            .args([
                "--edition",
                "2021",
                "--crate-type",
                "lib",
                source.to_str().unwrap(),
                "-o",
                root.join("app_wasm.rlib").to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "rustc rejected generated web Rust:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn fixed_uninit_requires_every_slot_before_read() {
    let diagnostics = jet::compile(
        "use core.mem\nfn run() {\n    bytes := [U8#2].{ uninit }\n    bytes[0] = 1\n    print(bytes[0])\n}\n",
    )
    .expect_err("a partially initialized fixed list must not be readable");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0420"),
        "{diagnostics:#?}"
    );
}

#[test]
fn write_argument_does_not_claim_an_uninit_buffer_was_filled() {
    let diagnostics = jet::compile(
        "use core.mem\nfn noop(bytes: &[U8#2]) {}\nfn run() {\n    bytes := [U8#2].{ uninit }\n    noop(&bytes)\n    print(bytes[0])\n}\n",
    )
    .expect_err("a no-op write callee cannot prove definite initialization");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0420"),
        "{diagnostics:#?}"
    );
}

/// D-FACT-FLOW1 (card #1621): the branch merge intersects what each path
/// initialised. Both arms fill every slot, so the value is written after the
/// branch and reading it is fine.
#[test]
fn every_path_initialising_a_slot_makes_it_written() {
    let out = jet::compile(
        "use core.mem\nfn decide(flag: Bool) {\n    bytes := [U8#2].{ uninit }\n    if {\n        flag -> {\n            bytes[0] = 1\n            bytes[1] = 2\n        }\n        else -> {\n            bytes[0] = 3\n            bytes[1] = 4\n        }\n    }\n    print(bytes[0])\n}\nfn run() { decide(true) }\n",
    );
    assert!(
        out.is_ok(),
        "both paths write every slot: {:#?}",
        out.err()
    );
}

/// The other half of the same rule: a slot written on one path only is not
/// written after the branch. Before the one join rule the walker kept whatever
/// arm it saw last and let this read through.
#[test]
fn a_slot_written_on_one_path_only_stays_unwritten() {
    let diagnostics = jet::compile(
        "use core.mem\nfn decide(flag: Bool) {\n    bytes := [U8#2].{ uninit }\n    if {\n        flag -> {\n            bytes[0] = 1\n            bytes[1] = 2\n        }\n        else -> {\n            bytes[0] = 3\n        }\n    }\n    print(bytes[1])\n}\nfn run() { decide(true) }\n",
    )
    .expect_err("one path leaves a slot unwritten");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0420"),
        "{diagnostics:#?}"
    );
}
