//! D-FLOORDIV1=A — `/%` divides and rounds the answer down, with `/%=`
//! assigning in place. Every operand here reaches the operator as a runtime
//! value, through `seed`, so the arithmetic is genuinely executed rather than
//! folded away at compile time.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{
    assert_tiers_agree, build_and_run, build_and_run_full, have_rustc, jit_run_traced,
    jit_run_with_env, jit_run_with_env_args,
};

/// A function the checker cannot see through, so its result is a runtime value
/// and the operator below is really evaluated by the built program.
const SEED: &str = "fn seed(n: Int) Int -[]> {\n    return n\n}\n";

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
    a :: I8{-7}
    b :: I8{2}
    print(a /% b)
    print(a % b)
    print(a %% b)
    c :: U8{200}
    d :: U8{7}
    print(c /% d)
    print(c % d)
}
";
    assert_tiers_agree("division_family_widths", src, "-4\n1\n-1\n28\n4\n");
}

/// D-FLOORDIV1=A: dividing by zero must stop the program on `jet run` too, and
/// say the same sentence the AOT binary says — as a runtime trap (exit 70 +
/// `panic:`), never E0953 "comptime stopped the build" (#1483).
///
/// The divisor comes from the process environment so the Cranelift / deopt
/// host really executes the trap instead of folding a literal away first.
#[test]
fn dividing_by_zero_traps_on_the_jit_tier() {
    for (name, op) in [
        ("jit_floordiv_zero", "/%"),
        ("jit_modulo_zero", "%"),
        ("jit_remainder_zero", "%%"),
    ] {
        let src = format!(
            "use core.sys as env
{SEED}
fn run() {{
    zero :: Int.parse(env.get(\"JET_TRAP_DIVISOR\") ?? \"0\") ?? 0
    print(seed(7) {op} zero)
}}
"
        );
        let (code, out, err) = jit_run_with_env(name, &src, &[("JET_TRAP_DIVISOR", "0")]);
        assert_eq!(
            code, 70,
            "`{op}` by zero must exit 70 under `jet run`: out={out} err={err}"
        );
        assert!(
            err.contains("Stop [E3010]") && err.contains("divided by zero"),
            "`{op}` by zero on `jet run`: expected the E3010 runtime stop, got: {err}"
        );
        assert!(
            !err.contains("E0953") && !err.contains("comptime"),
            "`{op}` by zero must not speak in comptime voice, got: {err}"
        );
    }
}

/// Returning from a native callee must restore the caller's complete source
/// frame before a later shared-Prelude stop is rendered.
#[test]
fn jit_restores_caller_source_after_nested_return() {
    let src = r#"fn leaf(value: Int) Int -[]> {
    return value
}
fn caller() {
    values := [String:Int]{ "ok": 7 }
    _ :: leaf(7)
    print(values["missing"])
}
fn run() {
    caller()
}
"#;
    let (code, out, err) = jit_run_traced("jit_nested_source_restore", src);
    assert_eq!(code, 70, "nested stop must exit 70: out={out} err={err}");
    assert!(
        err.lines()
            .any(|line| line.starts_with("leaf") && line.contains("tier1 native")),
        "leaf did not run natively: {err}"
    );
    assert!(
        err.lines()
            .any(|line| line.starts_with("caller") && line.contains("tier1 native")),
        "caller did not run natively: {err}"
    );
    assert!(
        err.contains("Stop [E3001]") && err.contains("missing"),
        "{err}"
    );
    assert!(
        err.contains("jit_nested_source_restore.jet:7 in caller()")
            && err.contains(r#"7 |     print(values["missing"])"#),
        "stop kept incomplete callee context instead of the caller source location: {err}"
    );
    assert!(
        !err.contains("jit_nested_source_restore.jet:7 in leaf()"),
        "{err}"
    );
}

/// #1483: `env.get` under `jet run` reads the live process environment — not a
/// value frozen at build time (I9 with AOT).
#[test]
fn env_divisor_uses_live_process_environment() {
    let src = format!(
        "use core.sys as env
{SEED}
fn run() {{
    d :: Int.parse(env.get(\"JET_TRAP_DIVISOR\") ?? \"0\") ?? 0
    print(seed(10) /% d)
}}
"
    );
    let (code, out, err) =
        jit_run_with_env("jit_env_live_divisor", &src, &[("JET_TRAP_DIVISOR", "2")]);
    assert_eq!(code, 0, "live env divisor must run: out={out} err={err}");
    assert_eq!(out, "5\n", "10 /% 2 from live env, got: {out}");
}

/// #1483: `process.argv` under `jet run` reads the live invocation argv — a non-zero
/// divisor from args must compute, not trap or fold at build time.
#[test]
fn args_divisor_uses_live_invocation_argv() {
    let src = format!(
        "use core.term as io
{SEED}
fn run() {{
    raw :: process.argv().get(1) ?? \"0\"
    print(seed(10) /% (Int.parse(raw) ?? 0))
}}
"
    );
    let (code, out, err) = jit_run_with_env_args("jit_args_live_divisor", &src, &[], &["5"]);
    assert_eq!(code, 0, "live args divisor must run: out={out} err={err}");
    assert_eq!(out, "2\n", "10 /% 5 from argv, got: {out}");
    assert!(
        !err.contains("E0953") && !err.contains("comptime"),
        "args divisor must not use comptime voice: {err}"
    );
}

/// D-EXPSEM1=A / D-FLOORDIV1=A / D-MODSEM1=A: Rust execution tiers read
/// arithmetic stop wording from the shared Prelude contract kernel. The Web
/// target carries the same text in its generated JavaScript preamble.
#[test]
fn every_tier_reports_one_trap_wording() {
    use jet::Comptime::MathLayout;
    let contracts = include_str!("../crates/jet-codegen/src/Prelude/Core/Contracts.rs");
    let power = include_str!("../crates/jet-codegen/src/Prelude/Core/Power.rs");
    let division = include_str!("../crates/jet-codegen/src/Prelude/Core/Division.rs");
    let evaluator = include_str!("../crates/jet-codegen/src/Codegen/TIR/eval/mod.rs");
    let web = include_str!("../crates/jet-codegen/src/Codegen/Web.rs");
    let jit_host = include_str!("../crates/jet-jit/src/jit/runtime_host.rs");

    // 1. The shared Prelude kernel is the one Rust wording owner. Its AOT
    // adapters name those constants instead of repeating their literals.
    for (wording, symbol) in [
        (
            MathLayout::INTEGER_POWER_NEGATIVE,
            "JET_ARITHMETIC_POWER_NEGATIVE",
        ),
        (
            MathLayout::INTEGER_POWER_OVERFLOW,
            "JET_ARITHMETIC_POWER_OVERFLOW",
        ),
        (
            MathLayout::INTEGER_DIVIDE_ZERO,
            "JET_ARITHMETIC_DIVIDE_ZERO",
        ),
        (
            MathLayout::INTEGER_DIVIDE_OVERFLOW,
            "JET_ARITHMETIC_DIVIDE_OVERFLOW",
        ),
    ] {
        assert!(
            contracts.contains(wording),
            "contract kernel lost: {wording}"
        );
        assert!(
            power.contains(symbol) || division.contains(symbol),
            "AOT adapter no longer reads {symbol}"
        );
        assert!(
            !power.contains(&format!("{wording:?}")),
            "Power.rs duplicated {wording}"
        );
        assert!(
            !division.contains(&format!("{wording:?}")),
            "Division.rs duplicated {wording}"
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

    // 3. The evaluator and Cranelift host compile the same Prelude kernel;
    // neither adapter owns a second wording table.
    assert!(
        evaluator.contains("include!(\"../../../Prelude/Core/Contracts.rs\")"),
        "the evaluator stopped importing the shared arithmetic contract"
    );
    assert!(
        jit_host.contains("include!(\"../../../jet-codegen/src/Prelude/Core/Contracts.rs\")"),
        "the Cranelift host stopped importing the shared arithmetic contract"
    );
    assert!(jit_host.contains("contract_kernel::jet_arithmetic_message"));
    assert!(jit_host.contains("contract_kernel::jet_runtime_stop_report"));
    assert!(evaluator.contains("contract_semantics::jet_runtime_stop_report"));
    for wording in [
        MathLayout::INTEGER_POWER_NEGATIVE,
        MathLayout::INTEGER_POWER_OVERFLOW,
        MathLayout::INTEGER_DIVIDE_ZERO,
        MathLayout::INTEGER_DIVIDE_OVERFLOW,
    ] {
        assert!(
            !jit_host.contains(&format!("{wording:?}")),
            "the Cranelift host duplicated the shared wording: {wording}"
        );
    }
}
