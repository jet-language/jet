//! D-INTDIV1=A — `/` answers the true quotient, so two whole numbers give a
//! Float. `/%` is the whole-number path. Operands arrive through `score` so
//! nothing folds away before the built program runs.

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{assert_tiers_agree, build_and_run, build_and_run_full, have_rustc, jit_run};

const SEED: &str = "fn score(n: Int) => Int {\n    return n\n}\n";

/// D-INTDIV1=A: `7 / 2` is 3.5, and the fraction survives an average.
#[test]
fn int_division_answers_the_true_quotient() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn run() {{
    print(score(7) / score(2))
    print(score(6) / score(2))
    print(score(-7) / score(2))
    print(score(1) / score(4))
}}
"
    );
    let (code, out) = build_and_run("intdiv_quotient", &src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "3.5\n3.0\n-3.5\n0.25\n", "{out}");
}

/// D-INTDIV1=A: the result really is a Float, proved by the slot it fits.
/// `want_float` takes only a Float, so this compiling at all is the evidence.
#[test]
fn int_division_result_is_a_float() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn want_float(x: Float) {{
    print(x)
}}

fn run() {{
    want_float(score(7) / score(2))
    want_float(score(9) / score(3))
}}
"
    );
    let (code, out) = build_and_run("intdiv_is_float", &src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "3.5\n3.0\n", "{out}");
}

/// D-INTDIV1=A: storing that Float back into a whole number is a type error,
/// and the fix names `/%`. `n /= 2` reaches sema as `n = n / 2`, because
/// compound assignment is desugared before checking, so both spellings get the
/// same advice.
#[test]
fn storing_a_quotient_in_a_whole_number_points_at_floor_division() {
    let dir = std::env::temp_dir().join(format!("jet_intdiv_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for src in [
        "fn run() {\n    n := 7\n    n /= 2\n    print(n)\n}\n",
        "fn run() {\n    n := 7\n    n = n / 2\n    print(n)\n}\n",
    ] {
        let path = dir.join("intdiv_compound.jet");
        std::fs::write(&path, src).unwrap();
        let shown = path.to_string_lossy().into_owned();
        let diags = jet::compile_with_path(src, &shown).err().unwrap_or_default();
        let rendered = jet::render_diagnostics(&shown, src, &diags);
        assert!(
            diags.iter().any(|d| d.code == "E0108"),
            "expected a type error for a quotient stored in a whole number:\n{rendered}"
        );
        assert!(
            rendered.contains("use `/%` to divide and round down"),
            "the fix must name floor division:\n{rendered}"
        );
    }
}

/// D-INTDIV1=A: `/%` is the whole-number path, and it still answers an Int.
#[test]
fn floor_division_remains_the_whole_number_path() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn want_int(n: Int) {{
    print(n)
}}

fn run() {{
    want_int(score(17) /% score(2))
    running := score(9)
    running /%= score(2)
    want_int(running)
}}
"
    );
    let (code, out) = build_and_run("intdiv_floor_still_int", &src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "8\n4\n", "{out}");
}

/// I9: the cases above are AOT only. This runs the same divisions under
/// `jet run` and asserts both tiers agree, so a quotient rule re-encoded in an
/// engine cannot pass.
#[test]
fn int_division_agrees_on_every_tier() {
    let src = format!(
        "{SEED}
fn run() {{
    print(score(7) / score(2))
    print(score(6) / score(2))
    print(score(-7) / score(2))
    print(score(1) / score(4))
    total :: score(8) + score(9)
    print(total / score(2))
    print(score(17) /% score(2))
}}
"
    );
    assert_tiers_agree(
        "intdiv_tiers",
        &src,
        "3.5\n3.0\n-3.5\n0.25\n8.5\n8\n",
    );
}

/// #1484: fixed-width `/` by zero must use the Prelude wording and exit 70 —
/// never a raw Rust "attempt to divide by zero" at exit 101 (I2). Includes a
/// call-result operand so the TIR overflow flag cannot hide behind Ident-only
/// AST replay, plus bare Ident and parameter shapes.
#[test]
fn fixed_width_divide_by_zero_traps_with_prelude_wording() {
    if !have_rustc() {
        return;
    }
    for (name, src) in [
        (
            "u8_div_zero",
            r#"
fn run() {
    a :: U8.{10}
    zero :: U8.{0}
    print(a / zero)
}
"#,
        ),
        (
            "i8_div_zero",
            r#"
fn run() {
    a :: I8.{10}
    zero :: I8.{0}
    print(a / zero)
}
"#,
        ),
        (
            "u8_param_div_zero",
            r#"
fn div(a: U8, zero: U8) {
    print(a / zero)
}
fn run() {
    div(U8.{10}, U8.{0})
}
"#,
        ),
        (
            "u8_call_div_zero",
            r#"
fn score(n: U8) => U8 {
    return n
}
fn run() {
    print(score(U8.{10}) / score(U8.{0}))
}
"#,
        ),
    ] {
        let (code, out, err) = build_and_run_full("jet_intdiv", name, src);
        assert_eq!(
            code, 70,
            "{name}: fixed-width / by zero must exit 70, got {code}; out={out} err={err}"
        );
        assert!(
            err.contains("this division can't be done")
                || err.contains("dividing by zero")
                || err.contains("divided by zero"),
            "{name}: expected Prelude division wording, got: {err}"
        );
        assert!(
            !err.contains("attempt to divide by zero"),
            "{name}: raw Rust panic leaked (I2): {err}"
        );

        let (jit_code, jit_out, jit_err) = jit_run(&format!("{name}_jit"), src);
        assert_eq!(
            jit_code, 70,
            "{name}: jet run must exit 70 (I9), got {jit_code}: {jit_out}{jit_err}"
        );
        assert!(
            jit_err.contains("panic:")
                && (jit_err.contains("this division can't be done")
                    || jit_err.contains("dividing by zero")
                    || jit_err.contains("divided by zero")),
            "{name}: jet run expected Prelude panic wording, got: {jit_err}"
        );
        assert!(
            !jit_err.contains("E0953") && !jit_err.contains("attempt to divide by zero"),
            "{name}: jet run must not use E0953 or raw Rust panic: {jit_err}"
        );
    }
}
