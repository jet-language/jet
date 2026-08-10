//! D-EXPOP1=A, D-EXPSEM1=A, D-XORSPELL1=A — the power operator `^`, its
//! grouping and result types, and the `~|` spelling exclusive-or moved to.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{assert_tiers_agree, build_and_run, have_rustc};

/// Compile a snippet from a real path on disk — the front end resolves
/// imports against the file it is given, so an invented name fails E0603.
fn compile_snippet(name: &str, src: &str) -> Result<(), Vec<String>> {
    let dir = std::env::temp_dir().join(format!("jet_power_{}_{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.jet"));
    std::fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy().into_owned();
    match jet::compile_with_path(src, &shown) {
        Ok(_) => Ok(()),
        Err(diags) => Err(diags.iter().map(|d| d.code.to_string()).collect()),
    }
}

fn accepts(name: &str, src: &str) -> bool {
    match compile_snippet(name, src) {
        Ok(()) => true,
        Err(codes) => {
            eprintln!("rejected {name}: {codes:?}");
            false
        }
    }
}

fn diagnostic_codes(name: &str, src: &str) -> Vec<String> {
    compile_snippet(name, src).err().unwrap_or_default()
}

/// A power's inferred type is proved by the parameter it fits: `want_int`
/// takes only a whole number, `want_float` only a Float.
const WANT: &str = "fn want_int(n: Int) { print(n) }\nfn want_float(n: Float) { print(n) }\n";

/// D-EXPSEM1=A: `^` groups to the right and binds tighter than a leading
/// minus. Evaluating the two shapes proves the grouping the parser built.
#[test]
fn power_groups_right_and_beats_unary_minus() {
    if !have_rustc() {
        return;
    }
    let src = r#"
fn run() {
    print(2 ^ 3 ^ 2)
    print((2 ^ 3) ^ 2)
    print(-3 ^ 2)
    print((-3) ^ 2)
    print(2 * 3 ^ 2)
    print(2 ^ 3 * 2)
}
"#;
    let (code, out) = build_and_run("power_grouping", src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "512\n64\n-9\n9\n18\n16\n", "{out}");
}

/// D-EXPSEM1=A: a whole number raised to a whole power stays exact; a written
/// negative exponent gives a Float; any Float operand makes the result Float.
#[test]
fn power_result_types_follow_the_operands() {
    // A whole base and a non-negative whole exponent stay exact.
    assert!(accepts(
        "int_pow",
        &format!("{WANT}fn run() {{\n    want_int(2 ^ 10)\n}}\n")
    ));
    // A written negative exponent gives a fraction, so it is a Float.
    assert!(accepts(
        "neg_pow_float",
        &format!("{WANT}fn run() {{\n    want_float(2 ^ -1)\n}}\n")
    ));
    assert!(
        !diagnostic_codes(
            "neg_pow_int",
            &format!("{WANT}fn run() {{\n    want_int(2 ^ -1)\n}}\n")
        )
        .is_empty(),
        "a negative exponent must not infer a whole number"
    );
    // Any Float operand makes the whole power a Float.
    assert!(accepts(
        "float_operand",
        &format!("{WANT}fn run() {{\n    want_float(2.0 ^ 10)\n}}\n")
    ));
    assert!(
        !diagnostic_codes(
            "float_operand_int",
            &format!("{WANT}fn run() {{\n    want_int(2.0 ^ 10)\n}}\n")
        )
        .is_empty(),
        "a Float operand must not infer a whole number"
    );
}

/// D-EXPSEM1=A: `^=` raises a binding in place, on whole numbers and floats.
#[test]
fn power_assign_raises_in_place() {
    if !have_rustc() {
        return;
    }
    let src = r#"
fn run() {
    growth := 2
    growth ^= 10
    print(growth)
    price := 1.5
    price ^= 2.0
    print(price)
}
"#;
    let (code, out) = build_and_run("power_assign", src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "1024\n2.25\n", "{out}");
}

/// D-XORSPELL1=A: `~|` is exclusive-or and `~|=` is its compound.
#[test]
fn exclusive_or_uses_the_tilde_pipe_spelling() {
    if !have_rustc() {
        return;
    }
    let src = r#"
fn run() {
    print(12 ~| 10)
    acc := 0
    loop byte, [17, 42, 99, 8] { acc ~|= byte }
    print(acc)
}
"#;
    let (code, out) = build_and_run("exclusive_or", src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "6\n80\n", "{out}");
}

/// D-SG9: exclusive-or wants the same integer width on both sides.
#[test]
fn exclusive_or_needs_matching_integer_widths() {
    assert!(accepts(
        "xor_same_width",
        "fn run() {\n    a :: U8.{12}\n    b :: U8.{10}\n    print(a ~| b)\n}\n"
    ));
    // A pair with no common integer type reports the D-SG9 error, exactly as
    // it did under the old `^` spelling.
    let mixed = diagnostic_codes(
        "xor_mixed_width",
        "fn run() {\n    a :: U8.{12}\n    b :: I8.{10}\n    print(a ~| b)\n}\n",
    );
    assert!(
        mixed.iter().any(|code| code == "E0109"),
        "mixed widths must report E0109, got {mixed:?}"
    );
    let floats = diagnostic_codes("xor_floats", "fn run() {\n    print(1.5 ~| 2.5)\n}\n");
    assert!(
        floats.iter().any(|code| code == "E0109"),
        "exclusive-or on floats must report E0109, got {floats:?}"
    );
}

/// D-EXPOP1=A / D-SHAPE-COPY1=A: prefix `^` is still take and prefix `~` is
/// still copy. Position tells each apart from its infix neighbour.
#[test]
fn prefix_take_and_copy_survive_the_rebind() {
    if !have_rustc() {
        return;
    }
    let src = r#"
struct Sword { power: Int }

fn melt(item: ^Sword) => Int { return item.power }

fn run() {
    blade :: Sword.{ power: 3 }
    print(melt(^blade))
    flags :: 12
    source :: 5
    print(flags ~| ~source)
    print(flags ^ 2)
}
"#;
    let (code, out) = build_and_run("prefix_sigils", src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "3\n9\n144\n", "{out}");
}

/// Greenfield: infix `^` never means exclusive-or again. `12 ^ 2` is a power,
/// so it must not produce the old exclusive-or answer of 14.
#[test]
fn infix_caret_never_means_exclusive_or() {
    if !have_rustc() {
        return;
    }
    let (code, out) = build_and_run("caret_is_power", "fn run() {\n    print(12 ^ 2)\n}\n");
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "144\n", "{out}");
}

/// I9 / #1485: powers above 2^53 stay exact on every native tier. The JS tier
/// is proved separately in `web_build` (needs node + wasm); here AOT and
/// `jet run` must agree on the answers that used to round when the JS helper
/// returned `Number(value)`.
#[test]
fn powers_above_two_to_the_fifty_three_stay_exact() {
    let src = r#"
fn run() {
    print(2 ^ 60)
    print(3 ^ 39)
}
"#;
    assert_tiers_agree(
        "power_above_53",
        src,
        "1152921504606846976\n4052555153018976267\n",
    );
}
