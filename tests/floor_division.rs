//! D-FLOORDIV1=A — `/%` divides and rounds the answer down, with `/%=`
//! assigning in place. Every operand here reaches the operator as a runtime
//! value, through `seed`, so the arithmetic is genuinely executed rather than
//! folded away at compile time.

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, build_and_run_full, have_rustc};

/// A function the checker cannot see through, so its result is a runtime value
/// and the operator below is really evaluated by the built program.
const SEED: &str = "fn seed(n: Int) => Int {\n    return n\n}\n";

/// D-FLOORDIV1=A: `/%` rounds toward negative infinity. Above zero that agrees
/// with rounding toward zero; below zero it does not, and that is the whole
/// point of the operator.
#[test]
fn floor_division_rounds_down_through_zero() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn run() {{
    two :: seed(2)
    print(seed(7) /% two)
    print(seed(-7) /% two)
    print(seed(6) /% two)
    print(seed(-6) /% two)
    print(seed(7) /% seed(-2))
    print(seed(-7) /% seed(-2))
}}
"
    );
    let (code, out) = build_and_run("floordiv_signs", &src);
    assert_eq!(code, 0, "{out}");
    // 7/2 = 3.5 → 3; -7/2 = -3.5 → -4; exact answers keep their value;
    // 7/-2 = -3.5 → -4; -7/-2 = 3.5 → 3.
    assert_eq!(out, "3\n-4\n3\n-3\n-4\n3\n", "{out}");
}

/// D-FLOORDIV1=A: floats round down the same way, and `/%=` assigns in place.
#[test]
fn floor_division_on_floats_and_in_place() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn run() {{
    print(7.5 /% 2.0)
    print(-7.5 /% 2.0)
    running := seed(9)
    running /%= seed(2)
    print(running)
    running /%= seed(-2)
    print(running)
}}
"
    );
    let (code, out) = build_and_run("floordiv_float_compound", &src);
    assert_eq!(code, 0, "{out}");
    // 7.5/2 = 3.75 → 3.0; -7.5/2 = -3.75 → -4.0; 9 → 4; 4/-2 = -2 exactly.
    assert_eq!(out, "3.0\n-4.0\n4\n-2\n", "{out}");
}

/// D-FLOORDIV1=A: `/%` sits with the other division-family operators, so a
/// chain groups left to right and `*` does not bind tighter.
#[test]
fn floor_division_groups_with_the_division_family() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn run() {{
    print(seed(20) /% seed(3) /% seed(2))
    print(seed(20) /% (seed(3) /% seed(2)))
    print(seed(2) * seed(7) /% seed(4))
    print(seed(7) /% seed(2) + seed(1))
}}
"
    );
    let (code, out) = build_and_run("floordiv_grouping", &src);
    assert_eq!(code, 0, "{out}");
    // (20/%3)/%2 = 6/%2 = 3; 20/%(3/%2) = 20/%1 = 20;
    // (2*7)/%4 = 14/%4 = 3; (7/%2)+1 = 3+1 = 4.
    assert_eq!(out, "3\n20\n3\n4\n", "{out}");
}

/// D-FLOORDIV1=A: dividing by zero traps, exactly as `/` does, and says so.
#[test]
fn floor_division_by_zero_traps() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn run() {{
    print(seed(7) /% seed(0))
}}
"
    );
    let (code, out, err) = build_and_run_full("jet_floordiv", "floordiv_zero", &src);
    assert_ne!(code, 0, "dividing by zero must stop the program: {out}");
    assert!(
        err.contains("divided by zero"),
        "expected the divided-by-zero wording, got: {err}"
    );
}

/// D-FLOORDIV1=A pairs with D-MODSEM1=A: for every pair of whole numbers,
/// `a == b * (a /% b) + a % b`. Proving it across the four sign combinations
/// is what makes the two operators one coherent pair rather than two rules.
#[test]
fn floor_division_and_modulo_satisfy_the_division_identity() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn check(a: Int, b: Int) {{
    print(b * (a /% b) + a % b == a)
}}

fn run() {{
    check(seed(7), seed(2))
    check(seed(-7), seed(2))
    check(seed(7), seed(-2))
    check(seed(-7), seed(-2))
    check(seed(9), seed(3))
    check(seed(-9), seed(3))
}}
"
    );
    let (code, out) = build_and_run("floordiv_identity", &src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "true\ntrue\ntrue\ntrue\ntrue\ntrue\n", "{out}");
}

/// D-MODSEM1=A: `%` takes the divisor's sign and `%%` takes the dividend's.
/// They agree above zero and part below it.
#[test]
fn the_two_remainders_differ_only_across_zero() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn run() {{
    two :: seed(2)
    print(seed(7) % two)
    print(seed(7) %% two)
    print(seed(-7) % two)
    print(seed(-7) %% two)
    print(seed(7) % seed(-2))
    print(seed(7) %% seed(-2))
    print(seed(-7) % seed(-2))
    print(seed(-7) %% seed(-2))
}}
"
    );
    let (code, out) = build_and_run("modulo_signs", &src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "1\n1\n1\n-1\n-1\n1\n-1\n-1\n", "{out}");
}

/// D-MODSEM1=A: the compounds assign each remainder in place.
#[test]
fn remainder_compounds_assign_in_place() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn run() {{
    slot := seed(-3)
    slot %= seed(5)
    print(slot)
    debt := seed(-7)
    debt %%= seed(5)
    print(debt)
}}
"
    );
    let (code, out) = build_and_run("modulo_compounds", &src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "2\n-2\n", "{out}");
}

/// D-MODSEM1=A: both remainders trap on a zero divisor.
#[test]
fn both_remainders_trap_on_a_zero_divisor() {
    if !have_rustc() {
        return;
    }
    for (name, op) in [("modulo_zero", "%"), ("remainder_zero", "%%")] {
        let src = format!(
            "{SEED}
fn run() {{
    print(seed(7) {op} seed(0))
}}
"
        );
        let (code, out, err) = build_and_run_full("jet_floordiv", name, &src);
        assert_ne!(code, 0, "`{op}` by zero must stop the program: {out}");
        assert!(
            err.contains("divided by zero"),
            "`{op}` by zero: expected the divided-by-zero wording, got: {err}"
        );
    }
}

/// D-MODSEM1=A: `%%` pairs with `/` the way `%` pairs with `/%`, so the other
/// division identity holds too: `a == b * (a / b) + a %% b`.
#[test]
fn truncated_remainder_satisfies_the_truncating_identity() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn check(a: Int, b: Int) {{
    print(b * (a / b) + a %% b == a)
}}

fn run() {{
    check(seed(7), seed(2))
    check(seed(-7), seed(2))
    check(seed(7), seed(-2))
    check(seed(-7), seed(-2))
}}
"
    );
    let (code, out) = build_and_run("remainder_identity", &src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "true\ntrue\ntrue\ntrue\n", "{out}");
}

/// D-EXPSEM1=A / D-FLOORDIV1=A: the Prelude files are the one home for these
/// rules, but the comptime interpreter and the Cranelift host cannot include
/// them, so they carry the trap wordings as constants. This proves the two
/// copies still say the same thing — the wordings cannot drift apart silently.
#[test]
fn prelude_trap_wordings_match_the_shared_constants() {
    use jet::Comptime::MathLayout;
    let power = include_str!("../crates/jet-codegen/src/Prelude/Core/Power.rs");
    let division = include_str!("../crates/jet-codegen/src/Prelude/Core/Division.rs");
    for (source, name, wording) in [
        (power, "Power.rs", MathLayout::INTEGER_POWER_NEGATIVE),
        (power, "Power.rs", MathLayout::INTEGER_POWER_OVERFLOW),
        (division, "Division.rs", MathLayout::INTEGER_DIVIDE_ZERO),
    ] {
        assert!(
            source.contains(wording),
            "Prelude/Core/{name} no longer carries the wording the other tiers report: {wording}"
        );
    }
}
