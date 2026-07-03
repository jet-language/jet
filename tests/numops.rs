//! D-NUMOPS1: checked-by-default integer overflow. Plain `+`/`-`/`*`/`/` on a
//! fixed-width integer traps at runtime (exit 70) instead of wrapping silently.

mod common;
use common::have_rustc;

fn build_and_run(name: &str, src: &str) -> (i32, String, String) {
    common::build_and_run("jet_numops_test", name, src)
}

#[test]
fn unsigned_addition_overflow_traps() {
    if !have_rustc() {
        return;
    }
    let src = "fn run() {\n    a: U8 :: 200\n    b: U8 :: 100\n    print(a + b)\n}\n";
    let (code, stdout, stderr) = build_and_run("u8_add_overflow", src);
    assert_eq!(
        code, 70,
        "overflow should trap (exit 70), stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(
        stderr.contains("overflow"),
        "panic should mention overflow: {stderr}"
    );
    assert!(
        !stdout.contains("44"),
        "must not silently wrap to 44: {stdout}"
    );
}

#[test]
fn int_multiplication_overflow_traps() {
    if !have_rustc() {
        return;
    }
    // i64::MAX * 2 overflows the default Int.
    let src = "fn run() {\n    big: Int :: 9223372036854775807\n    print(big * 2)\n}\n";
    let (code, _stdout, stderr) = build_and_run("int_mul_overflow", src);
    assert_eq!(
        code, 70,
        "Int multiplication overflow should trap: {stderr}"
    );
}

#[test]
fn arithmetic_within_range_succeeds() {
    if !have_rustc() {
        return;
    }
    let src = "fn run() {\n    a: U8 :: 100\n    b: U8 :: 50\n    print(a + b)\n}\n";
    let (code, stdout, _stderr) = build_and_run("u8_add_ok", src);
    assert_eq!(code, 0, "in-range arithmetic should succeed");
    assert_eq!(stdout.trim(), "150");
}

#[test]
fn overflow_opt_ins_do_not_trap() {
    if !have_rustc() {
        return;
    }
    // 200 + 100 overflows U8 (max 255): wrapping → 44, saturating → 255,
    // checked → null (here fallen back to 0).
    let src = "fn run() {\n    a: U8 :: 200\n    b: U8 :: 100\n    fb: U8 :: 0\n    \
               print(wrapping(a + b))\n    print(saturating(a + b))\n    \
               print(checked(a + b) ?? fb)\n}\n";
    let (code, stdout, stderr) = build_and_run("u8_opt_ins", src);
    assert_eq!(code, 0, "opt-ins must not trap: {stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines,
        ["44", "255", "0"],
        "wrapping/saturating/checked outputs"
    );
}

#[test]
fn bit_operators_and_mixed_width_shift() {
    if !have_rustc() {
        return;
    }
    // `&`/`|`/`^` keep the U8 width; a shift count may be any integer (here the
    // literal `2`/`1` default to Int) and the result keeps the left side's width.
    let src = "fn run() {\n    a: U8 :: 12\n    b: U8 :: 10\n    \
               print(a & b)\n    print(a | b)\n    print(a ^ b)\n    \
               print(a << 2)\n    print(a >> 1)\n}\n";
    let (code, stdout, stderr) = build_and_run("u8_bitops", src);
    assert_eq!(code, 0, "bit ops should succeed: {stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines,
        ["8", "14", "6", "48", "6"],
        "bitwise + shift outputs"
    );
}

#[test]
fn over_width_shift_traps_cleanly() {
    if !have_rustc() {
        return;
    }
    // Shifting a U8 by 200 bits is past its width — it must trap with a Jet
    // panic (exit 70), NOT leak a raw Rust "shift overflow" panic (I2). The
    // shift goes through a function so the count is a runtime value.
    let src = "fn shift_it(a: U8, n: U8) -> U8 {\n    r: U8 :: a << n\n    return r\n}\n\
               fn run() {\n    print(shift_it(4, 200))\n}\n";
    let (code, _stdout, stderr) = build_and_run("u8_overshift", src);
    assert_eq!(code, 70, "over-width shift should trap (exit 70): {stderr}");
    assert!(
        stderr.contains("out of range"),
        "panic should explain the shift: {stderr}"
    );
}
