//! D-INTDIV1=A / D-TYPE2-DEFAULT1=A — `/` answers an exact quotient: whole
//! numbers produce a `Fraction`; finite non-whole results print as decimals,
//! and repeating results print as fractions. `/%` is the whole-number path.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{assert_tiers_agree, build_and_run, build_and_run_full, have_rustc, jit_run};

const SEED: &str = "fn score(n: Int) -> Int {\n    return n\n}\n";

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
    assert_eq!(out, "3.5\n3\n-3.5\n0.25\n", "{out}");
}

/// D-TYPE2-DEFAULT1=A: exact division prints its exact result, while a Float
/// parameter requires the explicit destination-owned conversion.
#[test]
fn int_division_lands_exact() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn want_float(x: Float) {{
    print(x)
}}

fn run() {{
    exact :: score(7) / score(2)
    print(exact)
    print(7 / 3)
    want_float(Float.from_fraction(exact))
}}
"
    );
    assert_tiers_agree("intdiv_exact", &src, "3.5\n7/3\n3.5\n");
}

/// D-INTDIV1=A: storing that exact quotient back into a whole number is a
/// type error, and the fix names `/%`. `n /= 2` reaches sema as `n = n / 2`,
/// because compound assignment is desugared before checking, so both spellings
/// get the same advice. Struct fields use the same check.
#[test]
fn storing_a_quotient_in_a_whole_number_points_at_floor_division() {
    let dir = std::env::temp_dir().join(format!("jet_intdiv_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for src in [
        "fn run() {\n    n := 7\n    n /= 2\n    print(n)\n}\n",
        "fn run() {\n    n := 7\n    n = n / 2\n    print(n)\n}\n",
        "struct Boxed { value: Int }\nfn run() {\n    item :: Boxed{ value: 7 / 2 }\n}\n",
    ] {
        let path = dir.join("intdiv_compound.jet");
        std::fs::write(&path, src).unwrap();
        let shown = path.to_string_lossy().into_owned();
        let diags = jet::compile_with_path(src, &shown)
            .err()
            .unwrap_or_default();
        let rendered = jet::render_diagnostics(&shown, src, &diags);
        assert!(
            diags.iter().any(|d| d.code == "E0108"),
            "expected a type error for a quotient stored in a whole number:\n{rendered}"
        );
        assert!(
            rendered.contains("Use `/%` to divide and round down"),
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
    assert_tiers_agree("intdiv_tiers", &src, "3.5\n3\n-3.5\n0.25\n8.5\n8\n");
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
    a :: U8{10}
    zero :: U8{0}
    print(a / zero)
}
"#,
        ),
        (
            "i8_div_zero",
            r#"
fn run() {
    a :: I8{10}
    zero :: I8{0}
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
    div(U8{10}, U8{0})
}
"#,
        ),
        (
            "u8_call_div_zero",
            r#"
fn score(n: U8) -> U8 {
    return n
}
fn run() {
    print(score(U8{10}) / score(U8{0}))
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
            err.contains("E3010") && err.contains("divided by zero"),
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
            jit_err.contains("Stop [E3010]: `divided by zero`"),
            "{name}: jet run expected the E3010 Prelude stop, got: {jit_err}"
        );
        assert!(
            !jit_err.contains("E0953") && !jit_err.contains("attempt to divide by zero"),
            "{name}: jet run must not use E0953 or raw Rust panic: {jit_err}"
        );
    }
}

/// Card #2872: the modulo/division-heavy ring recurrence keeps exact values
/// across the AOT, resident, and interpreter tiers.
#[test]
fn integer_ring_modulo_and_division_preserve_tier_output() {
    let source = include_str!("int_ring_witness.jet");
    assert_tiers_agree("int_ring_mod_div", source, "613081333\n");
}

/// #2872 / #1436: every exact-Int operation answers small inline operands on
/// the machine-word route and everything else on the exact rail. The edges
/// below cross the inline boundary (±2^62), the i64 boundary, and each sign
/// combination of floored `/%` and `%` against truncated `%%`, so a fast route
/// that wraps, truncates the wrong way, or keeps an out-of-range quotient
/// inline disagrees with the exact answer on some tier.
#[test]
fn exact_int_overflow_edges_agree_on_every_tier() {
    let src = format!(
        "{SEED}
fn run() {{
    max :: score(9223372036854775807)
    top :: score(4611686018427387903)
    bottom :: score(-4611686018427387904)
    print(max + score(1))
    print(top + score(1))
    print(bottom - score(1))
    print((top + score(1)) - score(1))
    print(-bottom)
    print(top * score(2))
    print(bottom * score(-1))
    print(score(3037000499) * score(3037000499))
    print(score(3037000500) * score(3037000500))
    print(bottom /% score(-1))
    print(bottom %% score(-1))
    print(score(7) /% score(2))
    print(score(-7) /% score(2))
    print(score(7) /% score(-2))
    print(score(-7) /% score(-2))
    print(score(7) % score(2))
    print(score(-7) % score(2))
    print(score(7) % score(-2))
    print(score(-7) % score(-2))
    print(score(7) %% score(2))
    print(score(-7) %% score(2))
    print(score(7) %% score(-2))
    print(score(-7) %% score(-2))
    print(score(-6) % score(3))
    print(score(-6) /% score(4))
    print(bottom /% score(3))
    print(bottom % score(3))
    print((max + score(1)) /% score(-3))
    print((max + score(1)) % score(-3))
    print(score(1) << score(61))
    print(score(1) << score(62))
    print(score(-1) << score(62))
    print(score(3) << score(100))
    print(score(-7) >> score(1))
    print(score(-1) >> score(200))
    print(score(7) >> score(200))
    print(score(-12) & score(13))
    print(score(-12) | score(10))
    print(score(-12) ~| score(13))
    print(bottom ~| score(-1))
    print(score(-7) < score(2))
    print(bottom < top)
    print((max + score(1)) > top)
}}
"
    );
    assert_tiers_agree(
        "exact_int_edges",
        &src,
        "9223372036854775808
4611686018427387904
-4611686018427387905
4611686018427387903
4611686018427387904
9223372036854775806
4611686018427387904
9223372030926249001
9223372037000250000
4611686018427387904
0
3
-4
-4
3
1
1
-1
-1
1
-1
1
-1
0
-2
-1537228672809129302
2
-3074457345618258603
-1
2305843009213693952
4611686018427387904
-4611686018427387904
3802951800684688204490109616128
-4
-1
0
4
-2
-7
4611686018427387903
true
true
true
",
    );
}

/// #1436 criterion 4: an exact-Int loop whose values stay inside the inline
/// range performs a fixed number of heap allocations however many iterations
/// run. The emitted AOT program is linked with a counting global allocator
/// and run at two iteration counts; before the small-value route existed,
/// every add, multiply, floor division and modulo allocated limb vectors.
#[test]
fn exact_int_inline_loop_allocations_do_not_scale() {
    if !have_rustc() {
        return;
    }
    let src = "#CLI struct Args { rounds: Int }

fn churn(rounds: Int) -> Int {
    total := 0
    loop i in 0..<rounds {
        total = (total * 31 + i * 7 - 3) % 1000003
        total += (i /% 3) %% 5
    }
    total
}

fn run(args: Args) {
    print(churn(args.rounds) >= 0)
}
";
    let scratch = common::Scratch::new("exact_int_allocations");
    let dir = &scratch.path;
    tir_support::write_test_package(dir, tir_support::TIR_TEST_PACKAGE);
    let jet_path = dir.join("churn.jet");
    std::fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let rust = jet::compile_with_path(src, &shown)
        .unwrap_or_else(|diags| {
            panic!("front end rejected:\n{}", jet::render_diagnostics(&shown, src, &diags))
        })
        .rust;
    assert!(
        !rust.contains("#[global_allocator]"),
        "the default AOT program must keep the hidden system heap"
    );
    assert_eq!(
        rust.matches("\nfn main() {\n").count(),
        1,
        "emitted AOT program must have one entry to wrap"
    );
    let counted = rust.replacen("\nfn main() {\n", "\nfn __jet_emitted_main() {\n", 1)
        + r#"
struct JetAllocationCounter;
static JET_ALLOCATIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
unsafe impl std::alloc::GlobalAlloc for JetAllocationCounter {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        JET_ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        unsafe { std::alloc::GlobalAlloc::alloc(&std::alloc::System, layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::GlobalAlloc::dealloc(&std::alloc::System, ptr, layout) }
    }
}
#[global_allocator]
static JET_ALLOCATION_COUNTER: JetAllocationCounter = JetAllocationCounter;
fn main() {
    __jet_emitted_main();
    eprintln!("jet-allocations={}", JET_ALLOCATIONS.load(std::sync::atomic::Ordering::Relaxed));
}
"#;
    let rs = dir.join("churn.rs");
    let bin = dir.join("churn");
    std::fs::write(&rs, counted).unwrap();
    let rustc = std::process::Command::new("rustc")
        .args(["--edition", "2021", "--crate-name", "churn"])
        .args([rs.to_str().unwrap(), "-o", bin.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected the counted AOT program:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let allocations = |rounds: &str| -> usize {
        let run = std::process::Command::new(&bin).arg(rounds).output().unwrap();
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(run.status.success(), "{rounds} rounds failed: {stderr}");
        assert_eq!(String::from_utf8_lossy(&run.stdout), "true\n", "{rounds} rounds");
        stderr
            .lines()
            .find_map(|line| line.strip_prefix("jet-allocations="))
            .unwrap_or_else(|| panic!("no allocation count in: {stderr}"))
            .parse()
            .unwrap()
    };
    let small = allocations("1000");
    let large = allocations("100000");
    // 99,000 extra iterations of eight exact operations each: any per-op
    // allocation shows up as hundreds of thousands of extra allocations.
    assert!(
        large <= small + 64,
        "exact-Int loop allocations scale with iterations: {small} at 1e3 vs {large} at 1e5"
    );
}
