//! TIR control and data integration tests.

use super::*;

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
fn run() {
    sum :: (7 + (3 * 4))
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
fn run() {
    n :: 7
    parity :: if ((n % 2) == 0) { \"even\" } else { \"odd\" }
    print(shout(parity))
}
";
    let (code, stdout) = build_and_run("tir_ifexpr", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "odd!\n");
}

/// Statement-form if / else-if / else with a returning helper — mirrors the
/// shape of examples/features/basics/fizzbuzz.jet's `label`.
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
fn run() {
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

/// Coexistence: a free function and a method in the same program both route
/// through the executable TIR. A construct outside TIR coverage is an ICE gate,
/// not a legacy AST fallback.
#[test]
fn tir_function_and_method_coexist() {
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
fn run() {
    c :: Counter.{ n: 41 }
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
fn run() {
a: U8 :: 200
b: U8 :: 100
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
fn run() {
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
fn run() {
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

/// Labeled loops: a `continue outer@` and a `break outer@` driving a nested
/// range loop. The `'jet_<name>:` labels are resolved at lowering.
#[test]
fn labeled_break_and_continue() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    outer@ loop i in 1..3 {
        loop j in 1..3 {
            if (j == 2) {
                continue outer@
            }
            print(\"{i}-{j}\")
            if (i == 2) {
                break outer@
            }
        }
    }
    print(\"done\")
}
";
    let (code, stdout) = build_and_run("tir_labeled", src);
    assert_eq!(code, 0);
    // i=1: j=1 prints 1-1, i!=2 so j=2 -> continue outer@.
    // i=2: j=1 prints 2-1, i==2 -> break outer@.
    assert_eq!(stdout, "1-1\n2-1\ndone\n");
}

// --- c109 Phase 3: structs --------------------------------------------------

/// Struct literal, a struct-typed param with scalar field reads (borrow
/// position — no clone), a struct return value, and a struct-typed local. All
/// of `sum_pt`, `origin`, and `main` are inside the subset, so all route
/// through the TIR. The scalar field-read arithmetic (`p.x + p.y`) must NOT
/// overflow-trap: the old emitter baseline left this field operand unresolved,
/// so the plain `+` was used — the TIR reproduces that parity exactly.
#[test]
fn struct_literal_field_read_and_return() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Point {
    x: Int
    y: Int
}
fn sum_pt(p: Point) -> Int {
    return (p.x + p.y)
}
fn origin() -> Point {
    return Point.{ x: 0, y: 0 }
}
fn run() {
    p :: Point.{ x: 3, y: 4 }
    print(sum_pt(p))
    print(p.x)
    o :: origin()
    print(sum_pt(o))
}
";
    let (code, stdout) = build_and_run("tir_struct_pt", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "7\n3\n0\n");
}

/// A String struct field read in interpolation (a borrow-position read, so no
/// clone is inserted) plus a struct literal whose String field is moved from an
/// owned local. `describe` and `main` both route through the TIR.
#[test]
fn struct_string_field_in_interpolation() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Person {
    name: String
    age: Int
}
fn describe(p: Person) {
    print(\"{p.name} is {p.age}\")
}
fn run() {
    label :: \"Ada\"
    p :: Person.{ name: label, age: 36 }
    describe(p)
    print(p.age)
}
";
    let (code, stdout) = build_and_run("tir_struct_person", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "Ada is 36\n36\n");
}

/// Nested struct: a struct field whose type is itself a covered struct. Both the
/// nested literal (`Outer.{ inner: Inner { … }, … }`) and the chained field read
/// (`o.inner.v`) are covered, so `deep` and `main` route through the TIR.
#[test]
fn nested_struct_literal_and_chained_field() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Inner {
    v: Int
}
struct Outer {
    inner: Inner
    label: Int
}
fn deep(o: Outer) -> Int {
    return (o.inner.v + o.label)
}
fn run() {
    o :: Outer.{ inner: Inner.{ v: 10 }, label: 5 }
    print(deep(o))
    print(o.inner.v)
}
";
    let (code, stdout) = build_and_run("tir_struct_nested", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "15\n10\n");
}

// --- c109 Phase 4: enums + when/match + patterns ----------------------------

/// A unit-variant enum, enum literals (`Light.Red` etc.), and two exhaustive
/// variant matches (the `_ => unreachable!` fallthrough is dead but mandatory).
/// `next`, `label`, and `main` (an enum-typed local + covered helper calls) all
/// route through the TIR. Mirrors examples/features/types/enums.jet.
#[test]
fn enum_unit_variants_and_exhaustive_match() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Light {
    Red
    Yellow
    Green
}
fn next(light: Light) -> Light {
    if light == {
        Red -> { return Light.Yellow }
        Yellow -> { return Light.Green }
        Green -> { return Light.Red }
    }
}
fn label(light: Light) -> String {
    if light == {
        Red -> { return \"stop\" }
        Yellow -> { return \"caution\" }
        Green -> { return \"go\" }
    }
}
fn run() {
    start :: Light.Red
    print(label(start))
    print(label(next(start)))
}
";
    let (code, stdout) = build_and_run("tir_enum_unit", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "stop\ncaution\n");
}

/// Scalar-payload variants, an enum literal with a payload (`Conn.Active(42)`), a
/// payload binding read in the arm body, an or-pattern sharing a binding
/// (`Active(id) | Reconnecting(id)`), and a wildcard slot (`Idle(_)`).
#[test]
fn enum_payload_or_pattern_and_binding() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Conn {
    Active(Int)
    Reconnecting(Int)
    Idle(Int)
    Closed
}
fn describe(c: Conn) -> String {
    if c == {
        Active(id) | Reconnecting(id) -> { return \"live:{id}\" }
        Idle(_) -> { return \"idle\" }
        Closed -> { return \"closed\" }
    }
    return \"unknown\"
}
fn run() {
    print(describe(Conn.Active(42)))
    print(describe(Conn.Reconnecting(7)))
    print(describe(Conn.Idle(99)))
    print(describe(Conn.Closed))
}
";
    let (code, stdout) = build_and_run("tir_enum_payload", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "live:42\nlive:7\nidle\nclosed\n");
}

/// A range pattern in a *payload* slot (`Good(200..299)`, lowered to a match-arm
/// guard) alongside wildcard slots, all over an exhaustive enum match.
#[test]
fn enum_payload_range_pattern_guard() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Http {
    Good(Int)
    Fail(Int)
}
fn classify(r: Http) -> String {
    if r == {
        Good(200..299) -> { return \"success\" }
        Good(400..499) -> { return \"client error\" }
        Good(_) -> { return \"other\" }
        Fail(_) -> { return \"network error\" }
    }
    return \"unknown\"
}
fn run() {
    print(classify(Http.Good(201)))
    print(classify(Http.Good(404)))
    print(classify(Http.Good(302)))
    print(classify(Http.Fail(0)))
}
";
    let (code, stdout) = build_and_run("tir_enum_range", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "success\nclient error\nother\nnetwork error\n");
}

/// An arm-head range dispatch over a scalar subject with an `else` (the mixed-dispatch
/// `if/else if … else` lowering, with the parity `_jet_switch_subject` binding).
/// Mirrors examples/features/basics/pattern_matching.jet's `score_grade`.
#[test]
fn arm_head_range_dispatch() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn grade(score: Int) -> String {
    if score == {
        0..59 -> { return \"F\" }
        60..69 -> { return \"D\" }
        70..89 -> { return \"C\" }
        90..100 -> { return \"A\" }
        else -> { return \"?\" }
    }
}
fn run() {
    print(grade(95))
    print(grade(72))
    print(grade(45))
    print(grade(120))
}
";
    let (code, stdout) = build_and_run("tir_range_dispatch", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "A\nC\nF\n?\n");
}
