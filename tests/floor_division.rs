//! D-FLOORDIV1=A — `/%` divides and rounds the answer down, with `/%=`
//! assigning in place. Every operand here reaches the operator as a runtime
//! value, through `seed`, so the arithmetic is genuinely executed rather than
//! folded away at compile time.

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{assert_tiers_agree, build_and_run, build_and_run_full, have_rustc, jit_run};

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

/// D-MODSEM1=A: the two remainders are exactly one divisor apart whenever they
/// disagree, and equal when they agree. That is the whole relationship, and it
/// is what makes `%` the one that stays on the divisor's side of zero.
///
/// The older identity `a == b * (a / b) + a %% b` no longer holds, because
/// D-INTDIV1=A made `/` answer the exact quotient rather than truncating.
#[test]
fn the_two_remainders_are_one_divisor_apart() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn check(a: Int, b: Int) {{
    floored :: a % b
    truncated :: a %% b
    print(floored == truncated || floored == truncated + b)
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
    let (code, out) = build_and_run("remainder_relationship", &src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "true\ntrue\ntrue\ntrue\ntrue\ntrue\n", "{out}");
}

/// I9: every case above is proved through `build_and_run`, which is AOT only.
/// This runs the same arithmetic under `jet run` — the Cranelift host, with the
/// interpreter taking whatever it deopts on — and asserts both tiers print the
/// same thing. A rounding rule re-encoded in an engine passes every AOT test
/// and fails here.
#[test]
fn the_division_family_agrees_on_every_tier() {
    let src = format!(
        "{SEED}
fn run() {{
    two :: seed(2)
    print(seed(7) /% two)
    print(seed(-7) /% two)
    print(seed(7) /% seed(-2))
    print(seed(-7) /% seed(-2))
    print(seed(7) % two)
    print(seed(-7) % two)
    print(seed(7) %% two)
    print(seed(-7) %% two)
    print(7.5 /% 2.0)
    print(-7.5 /% 2.0)
    running := seed(9)
    running /%= seed(2)
    print(running)
    slot := seed(-3)
    slot %= seed(5)
    print(slot)
    debt := seed(-7)
    debt %%= seed(5)
    print(debt)
}}
"
    );
    assert_tiers_agree(
        "division_family_tiers",
        &src,
        "3\n-4\n-4\n3\n1\n1\n1\n-1\n3.0\n-4.0\n4\n2\n-2\n",
    );
}

/// D-MODSEM1=A: the smallest signed value with a divisor of -1. The quotient
/// leaves the width, but both remainders are 0, which every width holds — so
/// neither `%` nor `%%` may trap there, and they must agree. This is the pair
/// the two implementations most easily disagree on, because one uses a checked
/// remainder and the other a wrapping one.
#[test]
fn the_two_remainders_agree_at_the_smallest_value() {
    let src = format!(
        "{SEED}
fn run() {{
    smallest :: seed(-9223372036854775807) - seed(1)
    minus_one :: seed(-1)
    print(smallest % minus_one)
    print(smallest %% minus_one)
}}
"
    );
    assert_tiers_agree("remainder_smallest", &src, "0\n0\n");
}

/// D-FLOORDIV1=A / D-MODSEM1=A: fixed widths run the same rules, on both tiers.
/// Nothing else covers `/%` or `%` on a sized type, and the fixed-width path is
/// a different host table from the default `Int` one.
#[test]
fn the_division_family_holds_at_fixed_widths() {
    let src = "
fn run() {
    a :: I8.{-7}
    b :: I8.{2}
    print(a /% b)
    print(a % b)
    print(a %% b)
    c :: U8.{200}
    d :: U8.{7}
    print(c /% d)
    print(c % d)
}
";
    assert_tiers_agree("division_family_widths", src, "-4\n1\n-1\n28\n4\n");
}

/// D-FLOORDIV1=A: the zero-divisor trap must stop the program on `jet run` too,
/// with the same wording, not only in the AOT binary.
#[test]
fn dividing_by_zero_traps_on_the_jit_tier() {
    for (name, op) in [
        ("jit_floordiv_zero", "/%"),
        ("jit_modulo_zero", "%"),
        ("jit_remainder_zero", "%%"),
    ] {
        let src = format!(
            "{SEED}
fn run() {{
    print(seed(7) {op} seed(0))
}}
"
        );
        let (code, out, err) = jit_run(name, &src);
        assert_ne!(code, 0, "`{op}` by zero must stop `jet run`: {out}{err}");
        assert!(
            err.contains("divided by zero"),
            "`{op}` by zero on `jet run`: expected the divided-by-zero wording, got: {err}"
        );
    }
}

/// D-EXPSEM1=A / D-FLOORDIV1=A / D-MODSEM1=A: the Prelude files are the one
/// home for these rules, but three tiers cannot include them — the comptime
/// interpreter, the Cranelift host, and the JS preamble. Each carries the trap
/// wording separately, so each is a place the wording can drift.
///
/// This pins all four copies to the same strings at once. It fails if any
/// Prelude file drops a wording, if the JS preamble drifts, or if a tier stops
/// reading the shared constant and inlines its own literal — the JIT host is
/// checked by grep because its strings are compiled into a different crate.
#[test]
fn every_tier_reports_one_trap_wording() {
    use jet::Comptime::MathLayout;
    let power = include_str!("../crates/jet-codegen/src/Prelude/Core/Power.rs");
    let division = include_str!("../crates/jet-codegen/src/Prelude/Core/Division.rs");
    let web = include_str!("../crates/jet-codegen/src/Codegen/Web.rs");
    let jit_host = include_str!("../crates/jet-jit/src/jit/runtime_host.rs");

    // 1. Each Prelude file still spells every wording its operators can report.
    for (source, name, wording) in [
        (power, "Power.rs", MathLayout::INTEGER_POWER_NEGATIVE),
        (power, "Power.rs", MathLayout::INTEGER_POWER_OVERFLOW),
        (division, "Division.rs", MathLayout::INTEGER_DIVIDE_ZERO),
        (division, "Division.rs", MathLayout::INTEGER_DIVIDE_OVERFLOW),
    ] {
        assert!(
            source.contains(wording),
            "Prelude/Core/{name} no longer carries a wording other tiers report: {wording}"
        );
    }

    // 2. The JS preamble carries the same four, since it cannot include either
    //    Prelude file and re-states them as JavaScript string literals.
    for wording in [
        MathLayout::INTEGER_POWER_NEGATIVE,
        MathLayout::INTEGER_POWER_OVERFLOW,
        MathLayout::INTEGER_DIVIDE_ZERO,
        MathLayout::INTEGER_DIVIDE_OVERFLOW,
    ] {
        // The JS source is emitted in pieces, so compare on the longest run
        // that survives concatenation rather than the whole sentence.
        let anchor = wording.split(" (").next().expect("non-empty wording");
        assert!(
            web.contains(anchor),
            "the JS preamble no longer reports this trap the way every other tier does: {wording}"
        );
    }

    // 3. The Cranelift host reads the shared constants rather than inlining a
    //    sentence of its own. A literal quote of any wording here would mean a
    //    fourth copy that nothing keeps in step.
    for wording in [
        MathLayout::INTEGER_POWER_OVERFLOW,
        MathLayout::INTEGER_DIVIDE_ZERO,
        MathLayout::INTEGER_DIVIDE_OVERFLOW,
    ] {
        assert!(
            !jit_host.contains(&format!("{wording:?}")),
            "the Cranelift host inlined a trap wording instead of reading the shared \
             constant, so the two can now drift: {wording}"
        );
    }
    for symbol in [
        "INTEGER_POWER_NEGATIVE",
        "INTEGER_POWER_OVERFLOW",
        "INTEGER_DIVIDE_ZERO",
        "INTEGER_DIVIDE_OVERFLOW",
    ] {
        assert!(
            jit_host.contains(symbol),
            "the Cranelift host no longer reports {symbol}, so that trap can drift"
        );
    }
}
