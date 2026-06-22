//! c109 Phase 1: the typed-IR (TIR) path. These programs are squarely inside
//! the Phase-1 subset (scalar/String params, arithmetic, helper calls, an
//! if-expression, bindings, returns, print), so codegen routes them through
//! `Codegen/TIR.rs`. The asserts prove they compile (rustc accepts the output —
//! I2) and run with the right output. Golden parity (`tests/golden.rs`) covers
//! byte-identical equivalence to the AST path for the example suite.

use std::fs;
use std::process::Command;

fn have_rustc() -> bool {
    Command::new("rustc").arg("--version").output().is_ok()
}

fn build_and_run(name: &str, src: &str) -> (i32, String) {
    let dir = std::env::temp_dir().join(format!("jet_tir_test_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    // `compile_with_path` loads the entry from disk, so write the .jet first.
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let rustc = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            rs.to_str().unwrap(),
            "-o",
            bin.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code (I2 violation):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
    )
}

/// Arithmetic + a helper call + interpolation. The helper `double` and `main`
/// are both fully covered, so both route through the TIR.
#[test]
fn arithmetic_and_helper_call() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn double(n: Int) -> Int {
    return (n * 2)
}
fn main() {
    sum @= (7 + (3 * 4))
    print(\"sum {sum}\")
    print(double(sum))
}
";
    let (code, stdout) = build_and_run("tir_arith", src);
    assert_eq!(code, 0, "should exit cleanly");
    assert_eq!(stdout, "sum 19\n38\n");
}

/// An if-expression (S68) bound to a local, plus a String param helper.
#[test]
fn if_expression_and_string_param() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn shout(s: String) -> String {
    return \"{s}!\"
}
fn main() {
    n @= 7
    parity @= if ((n % 2) == 0) { \"even\" } else { \"odd\" }
    print(shout(parity))
}
";
    let (code, stdout) = build_and_run("tir_ifexpr", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "odd!\n");
}

/// Statement-form if / else-if / else with a returning helper — mirrors the
/// shape of examples/features/05_fizzbuzz.jet's `label`.
#[test]
fn if_else_chain_and_return() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn label(n: Int) -> String {
    if ((n % 15) == 0) {
        return \"FizzBuzz\"
    } else if ((n % 3) == 0) {
        return \"Fizz\"
    } else if ((n % 5) == 0) {
        return \"Buzz\"
    }
    return \"{n}\"
}
fn main() {
    print(label(3))
    print(label(5))
    print(label(15))
    print(label(7))
}
";
    let (code, stdout) = build_and_run("tir_ifchain", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "Fizz\nBuzz\nFizzBuzz\n7\n");
}

/// Coexistence: a covered free function (TIR path) and an uncovered type with a
/// method (AST path) in the same program both compile and run. This is the gate
/// working — `tir_covers` is false for the method, true for `add`.
#[test]
fn tir_and_ast_paths_coexist() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Counter {
    n: Int
}
impl Counter {
    fn bumped(self) -> Int {
        return (self.n + 1)
    }
}
fn add(a: Int, b: Int) -> Int {
    return (a + b)
}
fn main() {
    c @= Counter { n: 41 }
    print(add(c.bumped(), 0))
}
";
    let (code, stdout) = build_and_run("tir_coexist", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

/// Checked-by-default integer overflow must still trap when the function is on
/// the TIR path (the `overflow` flag is computed at lowering, not in codegen).
#[test]
fn overflow_still_traps_on_tir_path() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn main() {
    a: U8 @= 200
    b: U8 @= 100
    print(a + b)
}
";
    let (code, _stdout) = build_and_run("tir_overflow", src);
    assert_eq!(code, 70, "U8 overflow should trap (exit 70)");
}

// --- c109 Phase 2: control-flow loops ---------------------------------------

/// Infinite `loop { … }` with a `break`, plus the `loop cond` while form. Both
/// loop kinds, plus a compound assign and an if inside, route through the TIR.
#[test]
fn infinite_and_while_loops() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn main() {
    x := 0
    loop {
        x = (x + 1)
        if (x == 3) {
            break
        }
    }
    print(x)
    fuel := 3
    loop fuel > 0 {
        print(\"t-minus {fuel}\")
        fuel-= 1
    }
    print(\"liftoff\")
}
";
    let (code, stdout) = build_and_run("tir_loops", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\nt-minus 3\nt-minus 2\nt-minus 1\nliftoff\n");
}

/// Numeric range loops: inclusive `1..5` and a strided `0..10 step 2`. The
/// inclusive semantics (`..=`) and the `.step_by` lowering are read off the TIR.
#[test]
fn range_loops_inclusive_and_step() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn main() {
    total := 0
    loop n in 1..5 {
        total = (total + n)
    }
    print(total)
    loop k in 0..10 step 2 {
        print(k)
    }
}
";
    let (code, stdout) = build_and_run("tir_ranges", src);
    assert_eq!(code, 0);
    // 1+2+3+4+5 = 15, then 0,2,4,6,8,10 (inclusive end).
    assert_eq!(stdout, "15\n0\n2\n4\n6\n8\n10\n");
}

/// Labeled loops: a `continue @outer` and a `break @outer` driving a nested
/// range loop. The `'jet_<name>:` labels are resolved at lowering.
#[test]
fn labeled_break_and_continue() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn main() {
    @outer loop i in 1..3 {
        loop j in 1..3 {
            if (j == 2) {
                continue @outer
            }
            print(\"{i}-{j}\")
            if (i == 2) {
                break @outer
            }
        }
    }
    print(\"done\")
}
";
    let (code, stdout) = build_and_run("tir_labeled", src);
    assert_eq!(code, 0);
    // i=1: j=1 prints 1-1, i!=2 so j=2 -> continue @outer.
    // i=2: j=1 prints 2-1, i==2 -> break @outer.
    assert_eq!(stdout, "1-1\n2-1\ndone\n");
}
