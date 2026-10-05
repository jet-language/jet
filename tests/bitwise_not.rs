//! D-SIGIL-ALGEBRA1=B — `bit_not()` flips integer bits; `!` negates Bool.
//! Operands arrive through `bits` so the work happens in the built program.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{assert_tiers_agree, build_and_run, have_rustc};

const SEED: &str = "fn bits(n: Int) -> Int {\n    return n\n}\n";

/// On width-free `Int`, complement is `-x - 1`, not a lookup table.
#[test]
fn bit_not_on_int_is_minus_x_minus_one() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn check(x: Int) {{
    print(x.bit_not() == -x - 1)
}}

fn run() {{
    check(bits(0))
    check(bits(5))
    check(bits(-6))
    check(bits(1000))
    print(bits(0).bit_not())
    print(bits(5).bit_not())
}}
"
    );
    let (code, out) = build_and_run("bitnot_identity", &src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "true\ntrue\ntrue\ntrue\n-1\n-6\n", "{out}");
}

/// A sized type flips exactly its own width and comes back as the
/// same width. Each result below is that width's all-ones minus the value,
/// which only holds if the flip stayed inside the declared width.
#[test]
fn bit_not_keeps_each_integer_width() {
    if !have_rustc() {
        return;
    }
    let src = "
fn run() {
    a :: U8{5}
    print(a.bit_not())
    b :: U16{5}
    print(b.bit_not())
    c :: U32{5}
    print(c.bit_not())
    d :: I8{5}
    print(d.bit_not())
    e :: I16{5}
    print(e.bit_not())
    f :: I32{5}
    print(f.bit_not())
}
";
    let (code, out) = build_and_run("bitnot_widths", src);
    assert_eq!(code, 0, "{out}");
    // Unsigned: all-ones minus 5. Signed: -6 at every width.
    assert_eq!(out, "250\n65530\n4294967290\n-6\n-6\n-6\n", "{out}");
}

/// `flags.bit_and(mask.bit_not())` clears every bit named by the mask.
#[test]
fn bit_not_clears_the_bits_a_mask_names() {
    if !have_rustc() {
        return;
    }
    let src = format!(
        "{SEED}
fn run() {{
    flags :: bits(15)
    print(flags.bit_and(bits(4).bit_not()))
    print(flags.bit_and(bits(1).bit_not()))
    print(flags.bit_and(bits(15).bit_not()))
    print(flags.bit_and(bits(0).bit_not()))
}}
"
    );
    let (code, out) = build_and_run("bitnot_mask", &src);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "11\n14\n0\n15\n", "{out}");
}

/// D-SIGIL-ALGEBRA1=B: the Bool meaning is untouched.
#[test]
fn bit_not_on_bool_is_unchanged() {
    if !have_rustc() {
        return;
    }
    let src = "
fn yes() -> Bool {
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
    print(bits(0).bit_not())
    print(bits(5).bit_not())
    print(bits(-6).bit_not())
    print(bits(4294967296).bit_not())
    print(bits(15).bit_and(bits(4).bit_not()))
    print(!true)
    a :: U8{{5}}
    print(a.bit_not())
    b :: I16{{5}}
    print(b.bit_not())
    c :: U32{{5}}
    print(c.bit_not())
}}
"
    );
    assert_tiers_agree(
        "bitnot_tiers",
        &src,
        "-1\n-6\n5\n-4294967297\n11\nfalse\n250\n-6\n4294967290\n",
    );
}
