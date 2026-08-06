//! D-BITNOT1=A — prefix `!` turns over every bit it is given: the one bit of a
//! `Bool`, or every bit of a whole number. Operands arrive through `bits` so
//! the work really happens in the built program rather than folding away.

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{assert_tiers_agree, build_and_run, have_rustc};

const SEED: &str = "fn bits(n: Int) => Int {\n    return n\n}\n";

/// D-BITNOT1=A: on the width-free default `Int`, turning over every bit is the
/// same as `-x - 1`. Proving the identity rather than a table of constants is
/// what shows the operator is the complement and not a lookup.
#[test]
fn bit_not_on_int_is_minus_x_minus_one() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn check(x: Int) {{
    print(!x == -x - 1)
}}

fn run() {{
    check(bits(0))
    check(bits(5))
    check(bits(-6))
    check(bits(1000))
    print(!bits(0))
    print(!bits(5))
}}
"
    );
    let (code, out) = build_and_run("bitnot_identity", &src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "true\ntrue\ntrue\ntrue\n-1\n-6\n", "{out}");
}

/// D-BITNOT1=A: a sized type flips exactly its own width and comes back as the
/// same width. Each result below is that width's all-ones minus the value,
/// which only holds if the flip stayed inside the declared width.
#[test]
fn bit_not_keeps_each_integer_width() {
    if !have_rustc() {
        return;
    }
    let src = "
fn run() {
    a :: U8.{5}
    print(!a)
    b :: U16.{5}
    print(!b)
    c :: U32.{5}
    print(!c)
    d :: I8.{5}
    print(!d)
    e :: I16.{5}
    print(!e)
    f :: I32.{5}
    print(!f)
}
";
    let (code, out) = build_and_run("bitnot_widths", src);
    assert_eq!(code, 0, "{out}");
    // Unsigned: all-ones minus 5. Signed: -6 at every width.
    assert_eq!(out, "250\n65530\n4294967290\n-6\n-6\n-6\n", "{out}");
}

/// D-BITNOT1=A: the everyday use — `flags & !mask` keeps every bit except the
/// ones the mask names.
#[test]
fn bit_not_clears_the_bits_a_mask_names() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn run() {{
    flags :: bits(15)
    print(flags & !bits(4))
    print(flags & !bits(1))
    print(flags & !bits(15))
    print(flags & !bits(0))
}}
"
    );
    let (code, out) = build_and_run("bitnot_mask", &src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "11\n14\n0\n15\n", "{out}");
}

/// D-BITNOT1=A: the Bool meaning is untouched.
#[test]
fn bit_not_on_bool_is_unchanged() {
    if !have_rustc() {
        return;
    }
    let src = "
fn yes() => Bool {
    return true
}

fn run() {
    print(!true)
    print(!false)
    print(!yes())
}
";
    let (code, out) = build_and_run("bitnot_bool", src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "false\ntrue\nfalse\n", "{out}");
}

/// I9: every case above is AOT only. This runs the same flips under `jet run`
/// — the Cranelift host, with the interpreter taking whatever it deopts on —
/// and asserts both tiers agree. The width-clamping is the part most likely to
/// differ, so each sized type is checked, and the value above 2^32 is the one
/// that catches a tier doing 32-bit work.
#[test]
fn bit_not_agrees_on_every_tier() {
    let src = format!(
        "{SEED}
fn run() {{
    print(!bits(0))
    print(!bits(5))
    print(!bits(-6))
    print(!bits(4294967296))
    print(bits(15) & !bits(4))
    print(!true)
    a :: U8.{{5}}
    print(!a)
    b :: I16.{{5}}
    print(!b)
    c :: U32.{{5}}
    print(!c)
}}
"
    );
    assert_tiers_agree(
        "bitnot_tiers",
        &src,
        "-1\n-6\n5\n-4294967297\n11\nfalse\n250\n-6\n4294967290\n",
    );
}
