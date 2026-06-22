//! D-NUMOPS1: checked-by-default integer overflow. Plain `+`/`-`/`*`/`/` on a
//! fixed-width integer traps at runtime (exit 70) instead of wrapping silently.

use std::fs;
use std::process::Command;

fn build_and_run(name: &str, src: &str) -> (i32, String, String) {
    let dir = std::env::temp_dir().join(format!("jet_numops_test_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!("front end rejected:\n{}", jet::render_diagnostics(&shown, src, &diags))
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let rustc = Command::new("rustc")
        .args(["--edition", "2021", rs.to_str().unwrap(), "-o", bin.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code (I2):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

fn have_rustc() -> bool {
    Command::new("rustc").arg("--version").output().is_ok()
}

#[test]
fn unsigned_addition_overflow_traps() {
    if !have_rustc() {
        return;
    }
    let src = "fn main() {\n    a: U8 @= 200\n    b: U8 @= 100\n    print(a + b)\n}\n";
    let (code, stdout, stderr) = build_and_run("u8_add_overflow", src);
    assert_eq!(code, 70, "overflow should trap (exit 70), stdout={stdout:?} stderr={stderr:?}");
    assert!(stderr.contains("overflow"), "panic should mention overflow: {stderr}");
    assert!(!stdout.contains("44"), "must not silently wrap to 44: {stdout}");
}

#[test]
fn int_multiplication_overflow_traps() {
    if !have_rustc() {
        return;
    }
    // i64::MAX * 2 overflows the default Int.
    let src = "fn main() {\n    big: Int @= 9223372036854775807\n    print(big * 2)\n}\n";
    let (code, _stdout, stderr) = build_and_run("int_mul_overflow", src);
    assert_eq!(code, 70, "Int multiplication overflow should trap: {stderr}");
}

#[test]
fn arithmetic_within_range_succeeds() {
    if !have_rustc() {
        return;
    }
    let src = "fn main() {\n    a: U8 @= 100\n    b: U8 @= 50\n    print(a + b)\n}\n";
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
    let src = "fn main() {\n    a: U8 @= 200\n    b: U8 @= 100\n    fb: U8 @= 0\n    \
               print(wrapping(a + b))\n    print(saturating(a + b))\n    \
               print(checked(a + b) ?? fb)\n}\n";
    let (code, stdout, stderr) = build_and_run("u8_opt_ins", src);
    assert_eq!(code, 0, "opt-ins must not trap: {stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, ["44", "255", "0"], "wrapping/saturating/checked outputs");
}
