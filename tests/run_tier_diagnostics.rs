//! #1629 — runtime-tier E0956 uses the shared diagnostic voice.
//!
//! Default `jet run` shares the TIR evaluator with comptime. When that
//! evaluator hits an unsupported construct, every tier renders the same
//! E0956 what/why/fix text.

use std::fs;

mod common;

use jet::Interpreter::run_jit_once;
use jet_foundation::JitBackend::RunOutcome;

fn skip_if_cranelift_host_unsupported() -> bool {
    if jet_jit::cranelift_host_supported() {
        false
    } else if std::env::var("JET_REQUIRE_CRANELIFT_HOST").as_deref() == Ok("1") {
        panic!(
            "cranelift-jit host path unsupported on this architecture \
             (JET_REQUIRE_CRANELIFT_HOST=1)"
        );
    } else {
        eprintln!("note: cranelift-jit host path unsupported; skipping run-tier diag assertion");
        true
    }
}

#[test]
fn e1112_row_text_matches_aot_run_and_interpreter() {
    std::thread::Builder::new()
        .name("e1112-tier-parity".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(e1112_row_text_matches_aot_run_and_interpreter_inner)
        .unwrap()
        .join()
        .unwrap();
}

fn e1112_row_text_matches_aot_run_and_interpreter_inner() {
    let file = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui/empty_task_combinator.jet");
    let path = file.to_string_lossy().into_owned();
    let src = fs::read_to_string(&file).unwrap();
    let snapshot = fs::read_to_string(file.with_extension("stderr")).unwrap();

    let aot = jet::compile_with_path(&src, &path)
        .expect_err("AOT front end must reject an empty task combinator");
    let run = match run_jit_once(&path) {
        RunOutcome::Problems(diags) => diags,
        other => panic!("default jet run must reject E1112: {other:?}"),
    };
    let interpreter = match jet::Interpreter::dev_iteration(&path, false, true) {
        RunOutcome::Problems(diags) => diags,
        other => panic!("interpreter gate must reject E1112: {other:?}"),
    };

    let shape = |diags: &[jet::Diagnostics::Diagnostic]| {
        diags
            .iter()
            .map(|diag| {
                (
                    diag.code.clone(),
                    diag.what.clone(),
                    diag.why.clone(),
                    diag.fix.clone(),
                    diag.span,
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = shape(&aot);
    let expected_report = jet::render_all_json(
        &jet::Diagnostics::ReportPath::from_path(file.as_path()),
        &src,
        &aot,
    );
    assert_eq!(expected.len(), 3);
    assert!(expected.iter().all(|(code, ..)| code == "E1112"));
    for (tier, diags) in [("default jet run", run), ("interpreter", interpreter)] {
        assert_eq!(
            shape(&diags),
            expected,
            "{tier} diagnostic drifted from AOT"
        );
        assert_eq!(
            jet::render_all_json(
                &jet::Diagnostics::ReportPath::from_path(file.as_path()),
                &src,
                &diags,
            ),
            expected_report,
            "{tier} structured report drifted from AOT"
        );
        assert_eq!(
            jet::render_diagnostics("tests/ui/empty_task_combinator.jet", &src, &diags),
            snapshot,
            "{tier} text drifted from the UI snapshot"
        );
    }
    assert_eq!(
        jet::render_diagnostics("tests/ui/empty_task_combinator.jet", &src, &aot),
        snapshot,
        "AOT text drifted from the UI snapshot"
    );
}

#[test]
fn e0956_row_text_matches_aot_run_and_interpreter() {
    std::thread::Builder::new()
        .name("e0956-tier-parity".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(e0956_row_text_matches_aot_run_and_interpreter_inner)
        .unwrap()
        .join()
        .unwrap();
}

fn e0956_row_text_matches_aot_run_and_interpreter_inner() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let file =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui/comptime_panic.jet");
    let path = file.to_string_lossy().into_owned();
    let src = fs::read_to_string(&file).unwrap();
    let snapshot = fs::read_to_string(file.with_extension("stderr")).unwrap();

    let aot = jet::compile_with_path(&src, &path)
        .expect_err("AOT front end must reject the unsupported comptime construct");
    let run = match run_jit_once(&path) {
        RunOutcome::Problems(diags) => diags,
        other => panic!("default jet run must reject E0956: {other:?}"),
    };
    let interpreter = match jet::Interpreter::dev_iteration(&path, false, true) {
        RunOutcome::Problems(diags) => diags,
        other => panic!("interpreter gate must reject E0956: {other:?}"),
    };

    let shape = |diags: &[jet::Diagnostics::Diagnostic]| {
        diags
            .iter()
            .map(|diag| {
                (
                    diag.code.clone(),
                    diag.what.clone(),
                    diag.why.clone(),
                    diag.fix.clone(),
                    diag.span,
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = shape(&aot);
    let expected_report = jet::render_all_json(
        &jet::Diagnostics::ReportPath::from_path(file.as_path()),
        &src,
        &aot,
    );
    assert!(expected.iter().any(|(code, ..)| code == "E0956"));
    for (tier, diags) in [("default jet run", run), ("interpreter", interpreter)] {
        assert_eq!(
            shape(&diags),
            expected,
            "{tier} diagnostic drifted from AOT"
        );
        assert_eq!(
            jet::render_all_json(
                &jet::Diagnostics::ReportPath::from_path(file.as_path()),
                &src,
                &diags,
            ),
            expected_report,
            "{tier} structured report drifted from AOT"
        );
        assert_eq!(
            jet::render_diagnostics("tests/ui/comptime_panic.jet", &src, &diags),
            snapshot,
            "{tier} text drifted from the UI snapshot"
        );
    }
    assert_eq!(
        jet::render_diagnostics("tests/ui/comptime_panic.jet", &src, &aot),
        snapshot,
        "AOT text drifted from the UI snapshot"
    );
}

#[test]
fn e0999_row_fix_matches_aot_run_and_interpreter() {
    std::thread::Builder::new()
        .name("e0999-tier-parity".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(e0999_row_fix_matches_aot_run_and_interpreter_inner)
        .unwrap()
        .join()
        .unwrap();
}

fn e0999_row_fix_matches_aot_run_and_interpreter_inner() {
    let file = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui/single_bracket_marker.jet");
    let path = file.to_string_lossy().into_owned();
    let src = fs::read_to_string(&file).unwrap();
    let snapshot = fs::read_to_string(file.with_extension("stderr")).unwrap();

    let aot = jet::compile_with_path(&src, &path)
        .expect_err("AOT front end must reject a bare marker with brackets");
    let run = match run_jit_once(&path) {
        RunOutcome::Problems(diags) => diags,
        other => panic!("default jet run must reject E0999: {other:?}"),
    };
    let interpreter = match jet::Interpreter::dev_iteration(&path, false, true) {
        RunOutcome::Problems(diags) => diags,
        other => panic!("interpreter gate must reject E0999: {other:?}"),
    };

    let shape = |diags: &[jet::Diagnostics::Diagnostic]| {
        diags
            .iter()
            .map(|diag| {
                (
                    diag.code.clone(),
                    diag.what.clone(),
                    diag.why.clone(),
                    diag.fix.clone(),
                    diag.span,
                    diag.edit.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = shape(&aot);
    let expected_report = jet::render_all_json(
        &jet::Diagnostics::ReportPath::from_path(file.as_path()),
        &src,
        &aot,
    );
    assert_eq!(expected.len(), 1);
    assert_eq!(expected[0].0, "E0999");
    assert_eq!(
        expected[0].5.as_ref().map(|edit| edit.new_text.as_str()),
        Some("#Codable")
    );

    for (tier, diags) in [("default jet run", run), ("interpreter", interpreter)] {
        assert_eq!(
            shape(&diags),
            expected,
            "{tier} diagnostic or recovery data drifted from AOT"
        );
        assert_eq!(
            jet::render_all_json(
                &jet::Diagnostics::ReportPath::from_path(file.as_path()),
                &src,
                &diags,
            ),
            expected_report,
            "{tier} structured report drifted from AOT"
        );
        assert_eq!(
            jet::render_diagnostics("tests/ui/single_bracket_marker.jet", &src, &diags),
            snapshot,
            "{tier} text drifted from the UI snapshot"
        );
    }
    assert_eq!(
        jet::render_diagnostics("tests/ui/single_bracket_marker.jet", &src, &aot),
        snapshot,
        "AOT text drifted from the UI snapshot"
    );
}

#[test]
fn e0311_suggested_fix_matches_aot_run_and_interpreter() {
    std::thread::Builder::new()
        .name("e0311-tier-parity".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(e0311_suggested_fix_matches_aot_run_and_interpreter_inner)
        .unwrap()
        .join()
        .unwrap();
}

fn e0311_suggested_fix_matches_aot_run_and_interpreter_inner() {
    let file =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui/map_method_typo.jet");
    let path = file.to_string_lossy().into_owned();
    let src = fs::read_to_string(&file).unwrap();
    let snapshot = fs::read_to_string(file.with_extension("stderr")).unwrap();

    let aot =
        jet::compile_with_path(&src, &path).expect_err("AOT front end must reject missing methods");
    let run = match run_jit_once(&path) {
        RunOutcome::Problems(diags) => diags,
        other => panic!("default jet run must reject E0311: {other:?}"),
    };
    let interpreter = match jet::Interpreter::dev_iteration(&path, false, true) {
        RunOutcome::Problems(diags) => diags,
        other => panic!("interpreter gate must reject E0311: {other:?}"),
    };

    let shape = |diags: &[jet::Diagnostics::Diagnostic]| {
        diags
            .iter()
            .map(|diag| {
                (
                    diag.code.clone(),
                    diag.what.clone(),
                    diag.why.clone(),
                    diag.fix.clone(),
                    diag.span,
                    diag.edit.clone(),
                    diag.applicability,
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = shape(&aot);
    let expected_report = jet::render_all_json(
        &jet::Diagnostics::ReportPath::from_path(file.as_path()),
        &src,
        &aot,
    );
    assert_eq!(expected.len(), 4);
    assert!(expected.iter().all(|(code, ..)| code == "E0311"));
    assert!(expected.iter().all(|(_, _, _, _, _, edit, applicability)| {
        edit.is_some() && *applicability == Some(jet::Diagnostics::FixApplicability::Suggested)
    }));

    for (tier, diags) in [("default jet run", run), ("interpreter", interpreter)] {
        assert_eq!(
            shape(&diags),
            expected,
            "{tier} diagnostic or recovery data drifted from AOT"
        );
        assert_eq!(
            jet::render_all_json(
                &jet::Diagnostics::ReportPath::from_path(file.as_path()),
                &src,
                &diags,
            ),
            expected_report,
            "{tier} structured report drifted from AOT"
        );
        assert_eq!(
            jet::render_diagnostics("tests/ui/map_method_typo.jet", &src, &diags),
            snapshot,
            "{tier} text drifted from the UI snapshot"
        );
    }
    assert_eq!(
        jet::render_diagnostics("tests/ui/map_method_typo.jet", &src, &aot),
        snapshot,
        "AOT text drifted from the UI snapshot"
    );
}

#[test]
fn programmable_build_check_matches_aot_interpreter_dev_and_web() {
    let scratch = common::Scratch::new("programmable-build-tier-parity");
    let file = scratch.join("main.jet");
    let source = r#"
struct Entity { id: Int }

fn build(b: BuildContext) -> BuildPlan {
    types :: b.program.types()
    b.error(types[0].span, "ORG01", "entity must define archive", "company policy requires archival", "add an archive method")
    return b.plan()
}

fn run() {}
"#;
    fs::write(&file, source).unwrap();
    let path = file.to_string_lossy().into_owned();
    let shape = |diags: &[jet::Diagnostics::Diagnostic]| {
        diags
            .iter()
            .map(|diag| {
                (
                    diag.code.clone(),
                    diag.what.clone(),
                    diag.why.clone(),
                    diag.fix.clone(),
                    diag.span,
                )
            })
            .collect::<Vec<_>>()
    };

    let aot = jet::compile_programmable_build(&path, &[])
        .expect_err("programmable build check must reject this source");
    let expected = shape(&aot);
    assert_eq!(expected.len(), 1);
    assert_eq!(expected[0].0, "ORG01");

    let interpreter = match jet::Interpreter::run_interpreter_once_with_args(&path, &[]) {
        RunOutcome::Problems(diags) => diags,
        other => panic!("interpreter must reject the programmable build check: {other:?}"),
    };
    let jit = match jet::Interpreter::run_jit_once_with_args(&path, &[]) {
        RunOutcome::Problems(diags) => diags,
        other => panic!("Cranelift run must reject the programmable build check: {other:?}"),
    };
    let dev = match jet::Interpreter::dev_iteration(&path, false, false) {
        RunOutcome::Problems(diags) => diags,
        other => panic!("dev must reject the programmable build check: {other:?}"),
    };
    let web = jet::compile_web_with_gates(&path, jet::Policy::GateSet::default())
        .expect_err("web checking must reject the programmable build check");

    for (tier, diags) in [
        ("Cranelift run", jit),
        ("interpreter", interpreter),
        ("dev", dev),
        ("web", web),
    ] {
        assert_eq!(
            shape(&diags),
            expected,
            "{tier} diagnostic drifted from AOT"
        );
    }
}

#[test]
fn pattern_hole_reuse_reports_and_repairs_match_run_tiers() {
    std::thread::Builder::new()
        .name("pattern-hole-reuse-parity".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            for position in ["if", "value", "route", "bytes", "or", "and", "optional", "constant", "typed"] {
                let shown = format!("tests/ui/pattern_hole_reuse_{position}.jet");
                let file = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(&shown);
                let path = file.to_string_lossy().into_owned();
                let source = fs::read_to_string(&file).unwrap();
                let snapshot = fs::read_to_string(file.with_extension("stderr")).unwrap();
                let aot = jet::compile_with_path(&source, &path)
                    .expect_err("a pattern hole cannot reuse an existing value");
                let refusals = aot.iter().filter(|diag| diag.code == "E0118").collect::<Vec<_>>();
                assert_eq!(refusals.len(), 1, "{position}: {aot:?}");
                let refusal = refusals[0];
                if matches!(position, "typed" | "bytes" | "route") {
                    assert!(refusal.edit.is_none(), "{position} has no parenthesis-only repair");
                } else {
                    let edit = refusal.edit.as_ref().expect("plain text comparison has a structured repair");
                    assert!(edit.new_text.starts_with('(') && edit.new_text.ends_with(')'));
                    let mut fixed = source.clone();
                    fixed.replace_range(edit.span.start..edit.span.end, &edit.new_text);
                    jet::compile_with_path(&fixed, &path)
                        .unwrap_or_else(|diags| panic!("{position} comparison repair failed: {diags:?}"));
                }
                let report_path = jet::Diagnostics::ReportPath::from_path(&file);
                let expected_json = jet::render_all_json(&report_path, &source, &aot);
                assert_eq!(jet::render_diagnostics(&shown, &source, &aot), snapshot);
                let run = match run_jit_once(&path) {
                    RunOutcome::Problems(diags) => diags,
                    other => panic!("{position}: run must refuse the hole: {other:?}"),
                };
                let interpreter = match jet::Interpreter::dev_iteration(&path, false, true) {
                    RunOutcome::Problems(diags) => diags,
                    other => panic!("{position}: interpreter must refuse the hole: {other:?}"),
                };
                for diags in [run, interpreter] {
                    assert_eq!(jet::render_all_json(&report_path, &source, &diags), expected_json);
                    assert_eq!(jet::render_diagnostics(&shown, &source, &diags), snapshot);
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
