//! D-LAYOUT1 / D-LAYOUT-GATES1 (ratified 2026-06-28/29): `layout NAME { … }`,
//! the Cassowary-style constraint solver (`crates/jet-codegen/src/Prelude/
//! Layout.rs`, `jet_layout`). Covers: solver convergence on common patterns
//! (equal split, min/max bounds, proportional split via addition — layout
//! values don't support `*`/`/`, only `+`/`-`/comparisons, matching every
//! ratified example), axis-mismatch/not-a-constraint compile errors,
//! redundant-constraint lint, and infeasibility (a runtime query + panic,
//! not a static diagnostic — see docs/spec/diagnostics.md).

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);
fn unique_tmp() -> std::path::PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("jet_layout_{}_{}", std::process::id(), n))
}

fn error_codes(src: &str) -> Vec<String> {
    let dir = unique_tmp();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.jet");
    std::fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    match jet::compile_with_path(src, &shown) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.into_iter().map(|d| d.code.to_string()).collect(),
    }
}

/// Compile to Rust and (if rustc is available) build + run, returning stdout.
/// The I2 backstop: `jet_layout`'s runtime signatures must accept what Jet's
/// front end accepted (this is exactly the check that caught the borrow bug
/// in `Handle::value`'s infeasible-panic path before this card shipped).
fn build_and_run(name: &str, src: &str) -> Option<String> {
    let dir0 = unique_tmp();
    std::fs::create_dir_all(&dir0).unwrap();
    let fpath = dir0.join("fixture.jet");
    std::fs::write(&fpath, src).unwrap();
    let out = jet::compile_with_path(src, &fpath.to_string_lossy()).unwrap_or_else(|d| {
        panic!(
            "front end rejected a should-compile layout fixture: {:?}",
            d.iter().map(|x| x.code).collect::<Vec<_>>()
        )
    });
    assert!(
        !out.rust.contains("unsafe"),
        "`unsafe` leaked from the layout solver prelude (I1) — jet_layout must stay plain safe Rust"
    );
    if Command::new("rustc").arg("--version").output().is_err() {
        eprintln!("note: rustc not found; compiled front end only");
        return None;
    }
    let dir = std::env::temp_dir();
    let rs = dir.join(format!("jet_layout_{}.rs", name));
    let bin = dir.join(format!("jet_layout_{}", name));
    std::fs::write(&rs, &out.rust).unwrap();
    let c = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    assert!(
        c.status.success(),
        "I2 violated: rustc rejected generated layout code:\n{}",
        String::from_utf8_lossy(&c.stderr)
    );
    let r = Command::new(&bin).output().unwrap();
    assert!(
        r.status.success(),
        "generated layout program panicked/exited non-zero:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&r.stdout),
        String::from_utf8_lossy(&r.stderr)
    );
    Some(String::from_utf8_lossy(&r.stdout).to_string())
}

#[test]
fn equal_split_compiles_and_runs() {
    // Two boxes forced to equal widths that together fill a fixed total —
    // both constraints REQUIRED, so the solution is a single exact point:
    // a.width == b.width, a.width + b.width == 200 → both 100.
    let src = r#"
fn run() {
    layout form {
        a.width == b.width
        a.width + b.width == 200.0
    }
    aw :: form.value(form.h("a", "width"))
    bw :: form.value(form.h("b", "width"))
    print("a={aw} b={bw}")
}
"#;
    assert!(
        error_codes(src).is_empty(),
        "equal-split layout should compile clean: {:?}",
        error_codes(src)
    );
    if let Some(out) = build_and_run("equal_split", src) {
        assert_eq!(out, "a=100.0 b=100.0\n");
    }
}

#[test]
fn min_max_bounds_clamp() {
    // A width bounded to [50, 80] with a soft suggestion WAY above the
    // range must clamp to the upper bound — proves the required bounds win
    // over the soft preference, not the other way around.
    let src = r#"
fn run() {
    layout form {
        x.width >= 50.0
        x.width <= 80.0
    }
    xv :: form.h("x", "width")
    form.suggest(xv, 1000.0)
    print("x={(form.value(xv))}")
}
"#;
    assert!(
        error_codes(src).is_empty(),
        "min/max bounds layout should compile clean: {:?}",
        error_codes(src)
    );
    if let Some(out) = build_and_run("min_max", src) {
        assert_eq!(out, "x=80.0\n");
    }
}

#[test]
fn proportional_split_via_addition() {
    // Layout values only support `+`/`-`/comparisons (matching every
    // ratified D-LAYOUT1 example — no `*`/`/`), so "a is twice as wide as
    // b" is expressed as repeated addition: a == b + b.
    let src = r#"
fn run() {
    layout form {
        a.width == b.width + b.width
        a.width + b.width == 300.0
    }
    aw :: form.value(form.h("a", "width"))
    bw :: form.value(form.h("b", "width"))
    print("a={aw} b={bw}")
}
"#;
    assert!(
        error_codes(src).is_empty(),
        "proportional-split layout should compile clean: {:?}",
        error_codes(src)
    );
    if let Some(out) = build_and_run("proportional", src) {
        assert_eq!(out, "a=200.0 b=100.0\n");
    }
}

#[test]
fn cross_axis_width_vs_height_is_e2932() {
    let src = r#"
fn run() {
    layout form {
        a.width >= a.height
    }
}
"#;
    assert_eq!(error_codes(src), vec!["E2932".to_string()]);
}

#[test]
fn non_comparison_line_is_e2933() {
    let src = r#"
fn run() {
    x :: 1
    layout form {
        x == 1
    }
}
"#;
    assert_eq!(error_codes(src), vec!["E2933".to_string()]);
}

#[test]
fn duplicate_constraint_is_lint_not_error() {
    // E2934 is a WARNING (`out.lints`), not a hard error — the fixture must
    // still compile clean.
    let src = r#"
fn run() {
    layout form {
        a.width >= 80.0
        a.width >= 80.0
    }
    print("ok")
}
"#;
    assert!(
        error_codes(src).is_empty(),
        "a duplicate constraint is a lint, not a compile error: {:?}",
        error_codes(src)
    );
    let dir = unique_tmp();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.jet");
    std::fs::write(&path, src).unwrap();
    let out = jet::compile_with_path(src, &path.to_string_lossy()).unwrap();
    assert!(
        out.lints.iter().any(|d| d.code == "E2934"),
        "expected an E2934 lint, got: {:?}",
        out.lints.iter().map(|d| d.code).collect::<Vec<_>>()
    );
}

#[test]
fn infeasible_required_constraints_report_conflict() {
    // Two required constraints that can't both hold — `is_feasible()` must
    // be false and `conflict()` must name at least one of them (a real
    // simplex-derived trace, not a canned message).
    let src = r#"
fn run() {
    layout form {
        a.width >= 100.0
        a.width <= 50.0
    }
    print("feasible={(form.is_feasible())}")
    loop c in form.conflict() {
        print("conflict: {c}")
    }
}
"#;
    assert!(
        error_codes(src).is_empty(),
        "infeasibility is a runtime fact, not a compile error: {:?}",
        error_codes(src)
    );
    if let Some(out) = build_and_run("infeasible", src) {
        assert!(out.starts_with("feasible=false\n"), "got: {out}");
        assert!(
            out.contains("conflict:"),
            "expected a named conflicting constraint, got: {out}"
        );
    }
}

#[test]
fn infeasible_value_read_panics_loudly() {
    // Reading a value from an infeasible layout without checking
    // `is_feasible()` first panics (I1: a loud failure beats a silent
    // wrong number) rather than returning a made-up 0.
    let src = r#"
fn run() {
    layout form {
        a.width >= 100.0
        a.width <= 50.0
    }
    print("{(form.value(form.h("a", "width")))}")
}
"#;
    assert!(error_codes(src).is_empty());
    let dir0 = unique_tmp();
    std::fs::create_dir_all(&dir0).unwrap();
    let fpath = dir0.join("fixture.jet");
    std::fs::write(&fpath, src).unwrap();
    let out = jet::compile_with_path(src, &fpath.to_string_lossy()).unwrap();
    if Command::new("rustc").arg("--version").output().is_err() {
        eprintln!("note: rustc not found; compiled front end only");
        return;
    }
    let dir = std::env::temp_dir();
    let rs = dir.join("jet_layout_panic.rs");
    let bin = dir.join("jet_layout_panic");
    std::fs::write(&rs, &out.rust).unwrap();
    let c = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
    let r = Command::new(&bin).output().unwrap();
    assert!(!r.status.success(), "expected a panic (non-zero exit)");
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(
        stderr.contains("no feasible solution"),
        "expected the conflict message in the panic, got: {stderr}"
    );
}
