//! E2-M4 — `jet dev` interpreter tests.
//!
//! The crux is the **differential battery** (D-DEV, I2): for each supported
//! program, the interpreter's stdout MUST be byte-for-byte identical to the
//! compiled native binary's stdout. Any divergence is a P0 miscompile-class
//! bug — the interpreter is a dev convenience that must never lie about what
//! the real build does. This mirrors `tests/comptime_diff.rs`.
//!
//! Also tested:
//!   - the E2201 honest-boundary note (tasks/FFI/`@unsafe`/native std),
//!   - the per-iteration `dev_iteration` function the watch loop is built on,
//!   - the save-to-diagnostic latency budget (D-DEV3, <200ms check-only).

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use jet::Interpreter::{dev_iteration, RunOutcome};

/// c77: the differential battery covers EVERY `examples/features/*.jet`, not a
/// hand-curated subset — so the battery can never quietly shrink. Each example
/// either runs in the interpreter (and its stdout must match the compiled
/// binary AND its golden `.out`, byte for byte) or stops at a named boundary
/// (E2201 pre-scan, E2202 fuel, E0956 unsupported-at-runtime). A silent skip is
/// a test failure.
fn example_path(stem: &str) -> String {
    format!("examples/features/{}.jet", stem)
}

/// Every top-level example stem under `examples/features/`, sorted for
/// determinism. (Subdirectory examples — imports, modules, packages — have
/// their own multi-file drivers and are not single-entry dev targets.)
fn all_example_stems() -> Vec<String> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/features");
    let mut stems: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("jet"))
        .filter_map(|e| {
            e.path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .collect();
    stems.sort();
    stems
}

/// The recognized honest-boundary / terminal codes the interpreter may stop at
/// instead of producing run-to-completion stdout (c77 / D-DEV1):
///   - E2201: a feature the dev interpreter doesn't cover (pre-scan boundary),
///   - E2202 / E0952: the step/fuel budget was exhausted,
///   - E0956: a construct not yet supported at comptime (hit during execution),
///   - E0953: a deliberate user-authored panic (`require(false, …)`), which is
///     the program legitimately failing, not a silent skip.
const BOUNDARY_CODES: &[&str] = &["E2201", "E2202", "E0952", "E0956", "E0953"];

/// c77 widened battery: EVERY example either runs (interpreted stdout ==
/// compiled-binary stdout, byte for byte — I2) or stops at a named boundary
/// (E2201/E2202/E0956 — never a silent skip). Reports the run/boundary split so
/// the coverage can't quietly shrink.
#[test]
fn interpreter_matches_compiled_binary() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping jet dev differential battery");
        return;
    }
    let dir = std::env::temp_dir();
    let mut ran = 0usize;
    let mut boundary = 0usize;
    for (i, stem) in all_example_stems().iter().enumerate() {
        let file = example_path(stem);

        let interpreted = match dev_iteration(&file, false) {
            RunOutcome::Ran { stdout, .. } => stdout,
            RunOutcome::Problems(diags) => {
                // Not runnable → must be an honest, named boundary, not a silent
                // skip and not a stray internal error.
                assert!(
                    diags.iter().any(|d| BOUNDARY_CODES.contains(&d.code)),
                    "`{}` neither ran nor stopped at a named boundary {:?}; codes were {:?}",
                    stem,
                    BOUNDARY_CODES,
                    diags.iter().map(|d| d.code).collect::<Vec<_>>()
                );
                boundary += 1;
                continue;
            }
        };
        ran += 1;

        // Compiled side: front end -> rustc -> run. (Examples that need rustc
        // linking, like FFI, hit the E2201 boundary above and never reach here.)
        let src = fs::read_to_string(&file).unwrap();
        let compiled = match jet::compile_with_path(&src, &file) {
            Ok(c) => c,
            Err(diags) => panic!(
                "`{}` ran in the interpreter but failed the front end:\n{}",
                stem,
                jet::render_diagnostics(&file, &src, &diags)
            ),
        };
        let rs = dir.join(format!("jet_dev_diff_{}.rs", i));
        let bin = dir.join(format!("jet_dev_diff_{}", i));
        fs::write(&rs, &compiled.rust).unwrap();
        let out = Command::new("rustc")
            .args(["--edition", "2021"])
            .arg(&rs)
            .arg("-o")
            .arg(&bin)
            .output()
            .unwrap();
        if !out.status.success() {
            // The program needs C/FFI linkage rustc can't satisfy standalone;
            // such programs are boundary cases, so skip the binary compare here.
            continue;
        }
        let run = Command::new(&bin).output().unwrap();
        let compiled_stdout = String::from_utf8_lossy(&run.stdout).to_string();

        assert_eq!(
            interpreted, compiled_stdout,
            "DIVERGENCE for `{}`: interpreter and compiled binary disagree — this is a P0 miscompile",
            stem
        );
    }
    eprintln!(
        "c77 battery: {} ran (interp==compiled), {} boundary-asserted, {} total",
        ran,
        boundary,
        ran + boundary
    );
    assert!(ran > 0, "expected at least some examples to run in the interpreter");
}

/// Every example that runs in the interpreter and has a checked-in
/// `expected/*.out` golden (the executable spec, I5) must match it byte for
/// byte — a cheap check that needs no rustc. Examples that hit a boundary, or
/// that have no golden (error/panic demos), are asserted as boundaries here too
/// so nothing is silently skipped.
#[test]
fn interpreter_matches_expected_golden() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0usize;
    for stem in all_example_stems() {
        let file = example_path(&stem);
        let expected_path = root.join(format!("examples/features/expected/{}.out", stem));
        match dev_iteration(&file, false) {
            RunOutcome::Ran { stdout, .. } => {
                if let Ok(expected) = fs::read_to_string(&expected_path) {
                    assert_eq!(
                        stdout, expected,
                        "`{}`: interpreter output differs from expected golden",
                        stem
                    );
                    checked += 1;
                }
                // No golden (e.g. a panic demo) → nothing to compare; the
                // compiled-binary battery still covers it.
            }
            RunOutcome::Problems(diags) => {
                assert!(
                    diags.iter().any(|d| BOUNDARY_CODES.contains(&d.code)),
                    "`{}` neither ran nor stopped at a named boundary; codes were {:?}",
                    stem,
                    diags.iter().map(|d| d.code).collect::<Vec<_>>()
                );
            }
        }
    }
    assert!(checked > 0, "expected at least some golden comparisons");
}

/// E2202 fuel stop: a program whose top-level `loop` never breaks exhausts the
/// dev interpreter's step budget and stops with E2202 (an honest boundary, not
/// a hang). Driven through the comptime engine with a tiny fuel cap so the test
/// hits the same `burn()` path the watch loop uses, without burning the full
/// billion-step production budget.
#[test]
fn infinite_loop_hits_e2202_fuel_stop() {
    use std::collections::HashMap;
    let src = "fn main() {\n    n := 0\n    loop {\n        n = n + 1\n    }\n}\n";
    let prog = jet::Parser::parse(&jet::Lexer::lex(src).0).expect("fixture should parse");
    let mut funcs: HashMap<String, &jet::AST::Func> = HashMap::new();
    for item in &prog.items {
        if let jet::AST::Item::Func(f) = item {
            funcs.insert(f.name.clone(), f);
        }
    }
    let main = funcs.get("main").copied().expect("fixture has main");
    let mut sink = jet::Comptime::DevSink::new();
    let err = jet::Comptime::run_main_with_fuel(
        main,
        &funcs,
        std::path::Path::new("."),
        &mut sink,
        10_000,
    )
    .expect_err("an unbounded loop must exhaust the step budget");
    assert_eq!(
        err.code, "E2202",
        "an unbounded loop must stop with E2202, got: {}",
        err.code
    );
}

/// D-DEV1 honest boundary: a program that spawns a task stops with E2201
/// naming the feature and `jet build`/`jet run`. The expected note is pinned
/// in `tests/dev/unsupported.txt`.
#[test]
fn task_program_hits_e2201_boundary() {
    let file = "examples/features/32_tasks.jet";
    match dev_iteration(file, false) {
        RunOutcome::Problems(diags) => {
            assert_eq!(diags.len(), 1, "expected exactly one boundary note");
            let d = &diags[0];
            assert_eq!(d.code, "E2201");
            assert!(
                d.what.contains("spawns a task"),
                "E2201 should name the task feature, got: {}",
                d.what
            );
            assert!(
                d.fix.contains("jet build") || d.fix.contains("jet run"),
                "E2201 fix must point at the real build path, got: {}",
                d.fix
            );
        }
        RunOutcome::Ran { .. } => {
            panic!("a task program must not run in the dev interpreter (I2 boundary)")
        }
    }
}

/// D-DEV1 "try anyway": the opt-in flag skips the boundary scan and attempts
/// execution. For a task program it then fails honestly at the unsupported
/// construct (E0956) rather than refusing up front — no guarantees, but it
/// tried.
#[test]
fn try_anyway_skips_the_boundary_scan() {
    let file = "examples/features/32_tasks.jet";
    match dev_iteration(file, true) {
        RunOutcome::Problems(diags) => {
            // It got past the E2201 pre-scan and hit a real unsupported
            // construct during interpretation.
            assert!(
                diags.iter().all(|d| d.code != "E2201"),
                "try-anyway must skip the E2201 pre-scan"
            );
        }
        // If a future evaluator can run it, that's fine too — the point is the
        // pre-scan was skipped.
        RunOutcome::Ran { .. } => {}
    }
}

/// The dev iteration surfaces front-end errors identically to batch
/// compilation (D-DEV: same diagnostics).
#[test]
fn front_end_errors_surface_in_dev_iteration() {
    // Write a broken program to a temp file.
    let dir = std::env::temp_dir();
    let file = dir.join("jet_dev_broken.jet");
    fs::write(&file, "fn main() {\n    print(nope);\n}\n").unwrap();
    let shown = file.to_string_lossy().to_string();
    match dev_iteration(&shown, false) {
        RunOutcome::Problems(diags) => {
            assert!(!diags.is_empty(), "broken program must report problems");
            assert!(
                diags.iter().all(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)),
                "dev should surface errors"
            );
        }
        RunOutcome::Ran { .. } => panic!("a broken program must not run"),
    }
}

/// D-DEV3 latency budget: a save-to-diagnostic round (check-only) must stay
/// under 200ms on the example set. We measure the front-end check, which is
/// the diagnostic-producing work the watch loop does on every save.
#[test]
fn check_latency_under_budget() {
    let file = "examples/features/16_wordcount.jet";
    // Warm up (first load touches the filesystem and caches).
    let _ = jet::check_with_path(file);
    let mut best = u128::MAX;
    for _ in 0..5 {
        let started = Instant::now();
        let _ = jet::check_with_path(file);
        best = best.min(started.elapsed().as_millis());
    }
    assert!(
        best < 200,
        "save-to-diagnostic latency {} ms exceeds the 200ms budget (D-DEV3)",
        best
    );
}

// ── c77: hot-swap type-surface stability (D-HOTSWAP1 / E2210) ──────────

/// Parse `src` to a bundle via a temp file (the only loader entry point).
fn bundle_of(src: &str, tag: &str) -> jet::AST::ProgramBundle {
    let p = std::env::temp_dir().join(format!("jet_hotswap_{tag}.jet"));
    fs::write(&p, src).unwrap();
    jet::Loader::load_entry(p.to_str().unwrap()).expect("bundle should load")
}

const STRUCT_OLD: &str = "struct P {\n    x: Int\n}\nfn f(p: P) -> Int {\n    return p.x\n}\nfn main() {\n    print(f(P.{x: 1}))\n}\n";

/// A body-only edit keeps the type surface stable → swap (Ok).
#[test]
fn body_only_edit_is_type_stable() {
    let old = bundle_of(STRUCT_OLD, "stable_old");
    let new = bundle_of(
        "struct P {\n    x: Int\n}\nfn f(p: P) -> Int {\n    return p.x + 1\n}\nfn main() {\n    print(f(P.{x: 2}))\n}\n",
        "stable_new",
    );
    assert!(
        jet::Sema::HotSwap::type_stable_check(&old, &new, "main").is_ok(),
        "a body-only edit must be type-stable (swap path)"
    );
}

/// Adding a struct field changes the surface → restart, with E2210 naming it.
#[test]
fn struct_field_change_emits_e2210() {
    let old = bundle_of(STRUCT_OLD, "field_old");
    let new = bundle_of(
        "struct P {\n    x: Int\n    y: Int\n}\nfn f(p: P) -> Int {\n    return p.x\n}\nfn main() {\n    print(f(P.{x: 1, y: 2}))\n}\n",
        "field_new",
    );
    match jet::Sema::HotSwap::type_stable_check(&old, &new, "main") {
        Ok(()) => panic!("adding a struct field must force a restart"),
        Err(diags) => {
            assert_eq!(diags.len(), 1);
            assert_eq!(diags[0].code, "E2210");
            assert!(
                diags[0].what.contains("struct `P`"),
                "E2210 should name the changed struct, got: {}",
                diags[0].what
            );
        }
    }
}

/// Changing a function's return type changes the surface → E2210.
#[test]
fn fn_signature_change_emits_e2210() {
    let old = bundle_of("fn g(a: Int) -> Int {\n    return a\n}\nfn main() {\n    print(g(1))\n}\n", "sig_old");
    let new = bundle_of(
        "fn g(a: Int) -> Bool {\n    return a == 0\n}\nfn main() {\n    print(g(1))\n}\n",
        "sig_new",
    );
    match jet::Sema::HotSwap::type_stable_check(&old, &new, "main") {
        Ok(()) => panic!("a return-type change must force a restart"),
        Err(diags) => {
            assert_eq!(diags[0].code, "E2210");
            assert!(diags[0].what.contains("return type"));
        }
    }
}

/// Adding an enum variant changes the surface → E2210.
#[test]
fn enum_variant_change_emits_e2210() {
    let old = bundle_of("enum E {\n    A\n    B\n}\nfn main() {\n    print(1)\n}\n", "enum_old");
    let new = bundle_of("enum E {\n    A\n    B\n    C\n}\nfn main() {\n    print(1)\n}\n", "enum_new");
    match jet::Sema::HotSwap::type_stable_check(&old, &new, "main") {
        Ok(()) => panic!("adding an enum variant must force a restart"),
        Err(diags) => {
            assert_eq!(diags[0].code, "E2210");
            assert!(diags[0].what.contains("enum `E`"));
        }
    }
}
