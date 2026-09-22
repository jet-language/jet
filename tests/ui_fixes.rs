//! M2 exit criterion: every ownership ui fixture's Fix compiles.
//!
//! Each failing tests/ui/NAME.jet may have a sibling NAME.fixed.jet that
//! applies the diagnostic's Fix line. Those companions must pass the front
//! end; when rustc is available, generated Rust must build too (I2).

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

mod common;
use common::{panic_message, test_worker_count, Scratch};

#[test]
fn ownership_ui_fixes_compile() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui");
    let ext = jet::Syntax::FILE_EXT;
    let have_rustc = common::have_rustc();
    let have_cargo = Command::new("cargo").arg("--version").output().is_ok();

    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(&format!(".fixed.{}", ext)))
        })
        .collect();
    entries.sort();

    assert!(
        entries.len() >= 10,
        "expected M2 ownership .fixed.jet companions, found {}",
        entries.len()
    );

    let entries = Arc::new(entries);
    let next = Arc::new(AtomicUsize::new(0));
    let failures = Arc::new(Mutex::new(Vec::new()));
    let workers = test_worker_count(16).min(entries.len().max(1));
    let mut handles = Vec::new();
    for _ in 0..workers {
        let entries = Arc::clone(&entries);
        let next = Arc::clone(&next);
        let failures = Arc::clone(&failures);
        handles.push(std::thread::spawn(move || loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= entries.len() {
                break;
            }
            let path = entries[i].clone();
            if let Err(payload) =
                std::panic::catch_unwind(|| check_fixed_companion(i, &path, have_rustc, have_cargo))
            {
                failures.lock().unwrap().push(panic_message(payload));
            }
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
    let failures = failures.lock().unwrap();
    if !failures.is_empty() {
        panic!("{}", failures.join("\n\n"));
    }
}

fn check_fixed_companion(i: usize, path: &PathBuf, have_rustc: bool, have_cargo: bool) {
    let name = path.file_name().unwrap().to_string_lossy();
    let src = fs::read_to_string(path).unwrap();
    let shown = format!("tests/ui/{}", name);

    let stem_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if stem_name.starts_with("ffi_") && !have_cargo {
        return;
    }

    let out = jet::compile_with_path(&src, &shown).unwrap_or_else(|diags| {
        panic!(
            "fixed companion {} should compile:\n{}",
            name,
            jet::render_diagnostics(&shown, &src, &diags)
        );
    });

    // Until M3 struct literals, `take_required.fixed` proves the sema
    // fix only (Int passed to `take NoClone` is not valid Rust yet).
    let rustc_skip = stem_name == "take_required.fixed";

    if have_rustc && !rustc_skip {
        let stem = stem_name.replace('.', "_");
        let tmp = std::env::temp_dir();
        let rs = tmp.join(format!(
            "jet_ui_fix_{}_{}_{}.rs",
            std::process::id(),
            i,
            stem
        ));
        let bin = tmp.join(format!("jet_ui_fix_{}_{}_{}", std::process::id(), i, stem));
        fs::write(&rs, &out.rust).unwrap();
        let mut cmd = Command::new("rustc");
        cmd.args(["--edition", "2021", "-o"]).arg(&bin).arg(&rs);
        if let Some(link) = &out.ffi {
            cmd.arg("--extern")
                .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
            for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
                cmd.arg("-L")
                    .arg(format!("dependency={}", deps_dir.display()));
            }
        }
        let status = cmd.status().unwrap();
        assert!(
            status.success(),
            "rustc rejected fixed companion {} (I2)",
            name
        );
    }
}

#[test]
fn liveness_fix_renames_to_existing_underscore_form() {
    let scratch = Scratch::new("liveness_fix");
    let path = scratch.join("liveness.jet");
    fs::write(
        &path,
        "use core.files as files\nuse core.math.random as random\n\n\
         pub(package) fn package_export() {}\n\n\
         fn unused_private(value: Int) { print(value) }\n\n\
         fn run() {\n    unused_binding :: random.normal(0.0, 1.0)\n}\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["fix", path.to_str().unwrap()])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "jet fix failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "use core.files as _files\nuse core.math.random as random\n\n\
         pub(package) fn _package_export() {}\n\n\
         fn _unused_private(value: Int) { print(value) }\n\n\
         fn run() {\n    _unused_binding :: random.normal(0.0, 1.0)\n}\n"
    );
}

#[test]
fn liveness_fix_removes_only_literal_locals_and_empty_private_functions() {
    let scratch = Scratch::new("liveness_fix_removal");
    let path = scratch.join("liveness.jet");
    let original = "use core.math.random as random\n\n\
                    pub fn public_api() {}\n\n\
                    fn empty_private() {}\n\n\
                    fn run() {\n    pure_local :: 42\n    effectful :: random.normal(0.0, 1.0)\n    print(\"done\")\n}\n";
    fs::write(&path, original).unwrap();

    let dry_run = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["fix", path.to_str().unwrap(), "--dry-run"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        dry_run.status.code(),
        Some(0),
        "dry-run failed: {}",
        String::from_utf8_lossy(&dry_run.stderr)
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    let fixed = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["fix", path.to_str().unwrap()])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        fixed.status.code(),
        Some(0),
        "jet fix failed: {}",
        String::from_utf8_lossy(&fixed.stderr)
    );
    let source = fs::read_to_string(&path).unwrap();
    assert!(source.contains("pub fn public_api() {}"));
    assert!(!source.contains("empty_private"));
    assert!(!source.contains("pure_local"));
    assert!(source.contains("_effectful :: random.normal(0.0, 1.0)"));

    let checked = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["check", path.to_str().unwrap()])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "fixed source does not check: {}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(!String::from_utf8_lossy(&checked.stderr).contains("L0101"));
}

#[test]
fn shared_busy_wait_fix_rewrites_empty_plain_field_loop() {
    let scratch = Scratch::new("shared_busy_wait_fix");
    let path = scratch.join("busy_spin.jet");
    fs::write(&path, include_str!("ui_lint/busy_spin.jet")).unwrap();

    let fixed = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["fix", path.to_str().unwrap()])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        fixed.status.code(),
        Some(0),
        "jet fix failed: {}",
        String::from_utf8_lossy(&fixed.stderr)
    );
    let source = fs::read_to_string(&path).unwrap();
    assert!(
        source.contains(
            "flag.guard_edit().wait(_changed, value -> value.ready) ?? panic(\"wait failed\")"
        ),
        "safe busy-loop fix missing:\n{source}"
    );
    assert_eq!(
        source.matches("loop flag.ready == false {}").count(),
        1,
        "safe busy-loop fix changed the deliberate allow:\n{source}"
    );
    assert!(
        source.contains("#allow(shared_busy_wait) loop flag.ready == false {}"),
        "safe busy-loop fix removed the deliberate allow:\n{source}"
    );

    let checked = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["check", path.to_str().unwrap()])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "fixed busy-loop source does not check:\n{}",
        String::from_utf8_lossy(&checked.stderr)
    );
}

#[test]
fn repeated_list_head_fix_preserves_behavior() {
    let scratch = Scratch::new("repeated_list_head_fix");
    let path = scratch.join("main.jet");
    let original = r#"struct LineItem {
    label: String
    cents: Int
}

fn run() {
    items :: [
        LineItem{
            label: "coffee", // field comment stays with the first item
            cents: 450
        },
        // The second item keeps its position and evaluation order.
        LineItem{label: "tea", cents: 325}
    ]
    print("{items[0].label}:{items[0].cents}")
    print("{items[1].label}:{items[1].cents}")
}
"#;
    fs::write(&path, original).unwrap();
    fs::write(
        scratch.join("package.jet"),
        "name: \"repeated_list_head_fix\"\n\
         version: \"0.1.0\"\n\
         edition: \"2026\"\n\
         authority: { holds: { allow: [IO, Mem.Alloc] } }\n",
    )
    .unwrap();
    let shown = path.to_string_lossy().into_owned();

    let lint_output = jet::compile_with_path(original, &shown).unwrap();
    let lint = lint_output
        .lints
        .iter()
        .find(|diagnostic| diagnostic.code == "L0523")
        .expect("LineItem list must produce L0523");
    assert_eq!(lint.applicability.map(|value| value.as_str()), Some("safe"));
    assert!(
        lint.safety.is_some_and(|value| value.auto_apply()),
        "L0523 must carry an auto-applicable safety grade"
    );
    let edit = lint.edit.clone().expect("L0523 must carry its source edit");
    let expected_fixed = jet::FixEngine::apply_edits(original, std::slice::from_ref(&edit))
        .expect("L0523 edit must apply without overlap");

    let dry_run = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["fix", path.to_str().unwrap(), "--dry-run"])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        dry_run.status.code(),
        Some(0),
        "L0523 dry-run failed: {}",
        String::from_utf8_lossy(&dry_run.stderr)
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        original,
        "L0523 dry-run changed the source"
    );

    let before_outputs = ["default", "interpret", "release"]
        .into_iter()
        .map(|mode| {
            let args: Vec<&str> = match mode {
                "default" => vec!["run", "main.jet"],
                "interpret" => vec!["run", "--interpret", "main.jet"],
                "release" => vec!["run", "--release", "main.jet"],
                _ => unreachable!(),
            };
            let output = Command::new(env!("CARGO_BIN_EXE_jet"))
                .args(&args)
                .current_dir(&scratch.path)
                .env("NO_COLOR", "1")
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(0),
                "L0523 source failed in {mode}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            (mode, output.stdout)
        })
        .collect::<Vec<_>>();

    let applied = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["fix", path.to_str().unwrap()])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        applied.status.code(),
        Some(0),
        "L0523 fix failed: {}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), expected_fixed);
    assert!(expected_fixed.contains("[LineItem]{"));
    assert!(expected_fixed.contains("// field comment stays with the first item"));
    assert!(expected_fixed.contains("// The second item keeps its position"));

    let fixed = fs::read_to_string(&path).unwrap();
    let checked = jet::compile_with_path(&fixed, &shown).unwrap();
    assert!(
        !checked.lints.iter().any(|diagnostic| diagnostic.code == "L0523"),
        "L0523 remained after its explicit fix"
    );
    let second_fix = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["fix", path.to_str().unwrap()])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        second_fix.status.code(),
        Some(0),
        "second L0523 fix failed: {}",
        String::from_utf8_lossy(&second_fix.stderr)
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        fixed,
        "L0523 fix is not idempotent"
    );

    let after_outputs = ["default", "interpret", "release"]
        .into_iter()
        .map(|mode| {
            let args: Vec<&str> = match mode {
                "default" => vec!["run", "main.jet"],
                "interpret" => vec!["run", "--interpret", "main.jet"],
                "release" => vec!["run", "--release", "main.jet"],
                _ => unreachable!(),
            };
            let output = Command::new(env!("CARGO_BIN_EXE_jet"))
                .args(&args)
                .current_dir(&scratch.path)
                .env("NO_COLOR", "1")
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(0),
                "fixed L0523 source failed in {mode}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            (mode, output.stdout)
        })
        .collect::<Vec<_>>();
    assert_eq!(before_outputs, after_outputs);

    let fmt_path = scratch.join("fmt.jet");
    fs::write(&fmt_path, original).unwrap();
    let fmt = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["fmt", fmt_path.to_str().unwrap()])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        fmt.status.code(),
        Some(0),
        "fmt failed on repeated heads: {}",
        String::from_utf8_lossy(&fmt.stderr)
    );
    let formatted = fs::read_to_string(&fmt_path).unwrap();
    assert!(
        !formatted.contains("[LineItem]{"),
        "ordinary fmt must not apply the L0523 source fix: {formatted}"
    );
    assert!(formatted.contains("// field comment stays with the first item"));
    assert!(formatted.contains("// The second item keeps its position"));

    let generic_path = scratch.join("generic.jet");
    let generic = r#"struct Box<T> {
    value: T
}

fn run() {
    boxes := [Box<Int>{value: 1}, Box<Int>{value: 2}]
    print(boxes[0].value + boxes[1].value)
}
"#;
    fs::write(&generic_path, generic).unwrap();
    let generic_output =
        jet::compile_with_path(generic, &generic_path.to_string_lossy()).unwrap();
    let generic_lint = generic_output
        .lints
        .iter()
        .find(|diagnostic| diagnostic.code == "L0523")
        .expect("generic repeated heads must produce L0523");
    let generic_edit = generic_lint
        .edit
        .clone()
        .expect("generic L0523 edit must be safe");
    let generic_fixed =
        jet::FixEngine::apply_edits(generic, std::slice::from_ref(&generic_edit)).unwrap();
    assert!(generic_fixed.contains("[Box<Int>]{"));
    fs::write(&generic_path, &generic_fixed).unwrap();
    let generic_checked =
        jet::compile_with_path(&generic_fixed, &generic_path.to_string_lossy()).unwrap();
    assert!(
        !generic_checked
            .lints
            .iter()
            .any(|diagnostic| diagnostic.code == "L0523")
    );

    let fixed_shape_path = scratch.join("fixed_shape.jet");
    let fixed_shape = r#"struct LineItem {
    label: String
    cents: Int
}

fn fixed() -> [LineItem#2] {
    [LineItem{label: "coffee", cents: 450}, LineItem{label: "tea", cents: 325}]
}

fn run() {
    print(fixed()[0].cents)
}
"#;
    fs::write(&fixed_shape_path, fixed_shape).unwrap();
    let fixed_shape_output =
        jet::compile_with_path(fixed_shape, &fixed_shape_path.to_string_lossy()).unwrap();
    assert!(
        fixed_shape_output
            .lints
            .iter()
            .filter(|diagnostic| diagnostic.code == "L0523")
            .all(|diagnostic| diagnostic.edit.is_none()),
        "L0523 must not offer a growable-list edit for a fixed-length expected type"
    );

    let commented = r#"struct LineItem {
    label: String
    cents: Int
}

fn run() {
    items :: [
        LineItem /* keep this constructor spelling */ {label: "coffee", cents: 450},
        LineItem{label: "tea", cents: 325}
    ]
}
"#;
    fs::write(scratch.join("commented.jet"), commented).unwrap();
    let commented_output =
        jet::compile_with_path(commented, &scratch.join("commented.jet").to_string_lossy())
            .unwrap();
    assert!(
        commented_output
            .lints
            .iter()
            .filter(|diagnostic| diagnostic.code == "L0523")
            .all(|diagnostic| diagnostic.edit.is_none()),
        "L0523 must not offer an edit when constructor comments make the head ambiguous"
    );
}
