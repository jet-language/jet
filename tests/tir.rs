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

// --- c109 Phase 3: structs --------------------------------------------------

/// Struct literal, a struct-typed param with scalar field reads (borrow
/// position — no clone), a struct return value, and a struct-typed local. All
/// of `sum_pt`, `origin`, and `main` are inside the subset, so all route
/// through the TIR. The scalar field-read arithmetic (`p.x + p.y`) must NOT
/// overflow-trap: in the AST path a field operand is unresolved, so the plain
/// `+` is used — the TIR reproduces that exactly (parity).
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
    return Point { x: 0, y: 0 }
}
fn main() {
    p @= Point { x: 3, y: 4 }
    print(sum_pt(p))
    print(p.x)
    o @= origin()
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
fn main() {
    label @= \"Ada\"
    p @= Person { name: label, age: 36 }
    describe(p)
    print(p.age)
}
";
    let (code, stdout) = build_and_run("tir_struct_person", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "Ada is 36\n36\n");
}

/// Nested struct: a struct field whose type is itself a covered struct. Both the
/// nested literal (`Outer { inner: Inner { … }, … }`) and the chained field read
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
    tag: Int
}
fn deep(o: Outer) -> Int {
    return (o.inner.v + o.tag)
}
fn main() {
    o @= Outer { inner: Inner { v: 10 }, tag: 5 }
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
/// route through the TIR. Mirrors examples/features/11_enums.jet.
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
    if light {
        Red -> { return Light.Yellow }
        Yellow -> { return Light.Green }
        Green -> { return Light.Red }
    }
}
fn label(light: Light) -> String {
    if light {
        Red -> { return \"stop\" }
        Yellow -> { return \"caution\" }
        Green -> { return \"go\" }
    }
}
fn main() {
    start @= Light.Red
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
    if c {
        c == Active(id) | Reconnecting(id) -> { return \"live:{id}\" }
        c == Idle(_) -> { return \"idle\" }
        c == Closed -> { return \"closed\" }
    }
    return \"unknown\"
}
fn main() {
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
    if r {
        r == Good(200..299) -> { return \"success\" }
        r == Good(400..499) -> { return \"client error\" }
        r == Good(_) -> { return \"other\" }
        r == Fail(_) -> { return \"network error\" }
    }
    return \"unknown\"
}
fn main() {
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

/// An arm-head range switch over a scalar subject with an `else` (the mixed-switch
/// `if/else if … else` lowering, with the parity `_jet_switch_subject` binding).
/// Mirrors examples/features/71_pattern_matching.jet's `score_grade`.
#[test]
fn arm_head_range_switch() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn grade(score: Int) -> String {
    if score {
        0..59 -> { return \"F\" }
        60..69 -> { return \"D\" }
        70..89 -> { return \"C\" }
        90..100 -> { return \"A\" }
        else -> { return \"?\" }
    }
}
fn main() {
    print(grade(95))
    print(grade(72))
    print(grade(45))
    print(grade(120))
}
";
    let (code, stdout) = build_and_run("tir_range_switch", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "A\nC\nF\n?\n");
}

// c109 Phase 5: collections — list/map literals, indexing/slicing, index-assign,
// and `loop x in coll` / `loop k, v in map` iteration. The `IndexKind` (List/Map)
// is carried as a total fact from sema and dispatched at lowering (never
// re-inferred). All asserts prove rustc accepts the output (I2) and runs correctly.

/// A list literal, indexing, a slice, and single-binding iteration over a
/// list-typed param — all in one covered function pair.
#[test]
fn list_literal_index_slice_and_iteration() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn total(xs: [Int]) -> Int {
    sum := 0
    loop x in xs {
        sum = (sum + x)
    }
    return sum
}
fn main() {
    nums := [10, 20, 30, 40]
    print(nums[0])
    print(nums[1..2])
    print(total(nums))
}
";
    let (code, stdout) = build_and_run("tir_list", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "10\n[20, 30]\n100\n");
}

/// Indexed assignment into a list (`xs[i] = v`) — the `LValue::Index` vec form.
#[test]
fn list_index_assignment() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn main() {
    nums := [1, 2, 3]
    nums[1] = 99
    print(nums[0])
    print(nums[1])
    print(nums[2])
}
";
    let (code, stdout) = build_and_run("tir_list_assign", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n99\n3\n");
}

/// A map literal (`[:]`), map indexing, map insert (`m[k] = v`), and two-binding
/// `loop k, v in map` iteration — the map-specific helpers and the `.iter()` clone
/// form. BTreeMap iterates in sorted key order, so output is deterministic.
#[test]
fn map_literal_index_insert_and_iteration() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn main() {
    counts: [String, Int] := [:]
    counts[\"banana\"] = 3
    counts[\"apple\"] = 5
    print(counts[\"apple\"])
    loop k, v in counts {
        print(\"{k}={v}\")
    }
}
";
    let (code, stdout) = build_and_run("tir_map", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\napple=5\nbanana=3\n");
}

// --- c109 Phase 6: methods + clones -----------------------------------------

/// The sema-inserted `.clone()` inside a COVERED function (no `self`): `p.name`
/// is an owning non-`Copy` String field read, which sema rewrites to a
/// `(p.name).clone()` MethodCall. Phases 3–5 excluded this (the getter that moves
/// a field out); Phase 6 covers it, so `name_of` now routes through the TIR.
#[test]
fn covered_fn_returns_cloned_string_field() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Person {
    name: String
    age: Int
}
fn name_of(p: Person) -> String {
    return p.name
}
fn main() {
    p @= Person { name: \"Grace\", age: 40 }
    print(name_of(p))
}
";
    let (code, stdout) = build_and_run("tir_clone_getter", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "Grace\n");
}

/// A user-defined instance method with scalar args on a covered struct. The
/// caller `run` routes through the TIR; `(c).user_add(10i64, 20i64)` is emitted
/// from the resolved `method_sigs` conventions (the method body, which has `self`,
/// stays on the AST path — the gate excludes `self` functions).
#[test]
fn user_method_with_scalar_args() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Calc {
    base: Int

    fn add(self, x: Int, y: Int) -> Int {
        return ((self.base + x) + y)
    }
}
fn run(c: Calc) -> Int {
    return c.add(10, 20)
}
fn main() {
    c @= Calc { base: 1 }
    print(run(c))
}
";
    let (code, stdout) = build_and_run("tir_method_args", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "31\n");
}

/// A user method taking a String argument by value — the arg carries an implicit
/// clone (`(name).clone()`), reproduced from the total `CallArg.flags` exactly as
/// `emit_call_args` does. The caller `run` routes through the TIR.
#[test]
fn user_method_with_string_arg_implicit_clone() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Crate {
    tag: String

    fn combine(self, other: String) -> String {
        return \"{self.tag}-{other}\"
    }
}
fn run(b: Crate) -> String {
    name @= \"x\"
    return b.combine(name)
}
fn main() {
    b @= Crate { tag: \"t\" }
    print(run(b))
}
";
    let (code, stdout) = build_and_run("tir_method_string_arg", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "t-x\n");
}

/// A trait-impl method call. `(d).label()` is emitted with the BARE method name
/// (the trait impl owns it — no `user_` mangle), decided at lowering from
/// `cx.trait_methods`. The caller `describe` routes through the TIR.
#[test]
fn trait_impl_method_call_no_mangle() {
    if !have_rustc() {
        return;
    }
    let src = "\
trait Named {
    fn label(self) -> String
}
struct Dog {
    sound: String
}
impl Dog: Named {
    fn label(self) -> String {
        return \"dog\"
    }
}
fn describe(d: Dog) -> String {
    return d.label()
}
fn main() {
    d @= Dog { sound: \"woof\" }
    print(describe(d))
}
";
    let (code, stdout) = build_and_run("tir_trait_method", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "dog\n");
}

/// An instance method on a covered ENUM, called from a covered function. The
/// enum-method dispatch and the enum-literal argument both route through the TIR.
#[test]
fn user_method_on_covered_enum() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Light {
    Red
    Green

    fn code(self) -> Int {
        if self {
            Red -> { return 1 }
            Green -> { return 2 }
        }
    }
}
fn run(l: Light) -> Int {
    return l.code()
}
fn main() {
    print(run(Light.Green))
}
";
    let (code, stdout) = build_and_run("tir_enum_method", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\n");
}

/// A non-empty map literal `[k: v, …]` returned from a covered function, then
/// indexed in `main` — the map-builder lowering plus map indexing.
#[test]
fn map_literal_with_entries() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn scores() -> [String, Int] {
    return [\"a\": 1, \"b\": 2]
}
fn main() {
    s := scores()
    print(s[\"a\"])
    print(s[\"b\"])
}
";
    let (code, stdout) = build_and_run("tir_map_entries", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n2\n");
}

// ---------------------------------------------------------------------------
// c109 Phase 7: method bodies + static methods. The method body (with a `self`
// param) and static (associated) methods + their call sites now route through
// the TIR. These prove the lowered method definitions compile (I2) and run, and
// that static dispatch (`Type.make(x)` → `user_T::user_make(x)`) is correct.
// ---------------------------------------------------------------------------

/// A static constructor returning the owning type, plus a `self` getter that is
/// now covered end-to-end (definition + call). Static dispatch + instance call.
#[test]
fn static_constructor_and_self_getter() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Counter {
    n: Int

    fn make(v: Int) -> Counter {
        return Counter { n: v }
    }
    fn value(self) -> Int {
        return self.n
    }
}
fn main() {
    c @= Counter.make(5)
    print(c.value())
}
";
    let (code, stdout) = build_and_run("tir_static_ctor", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\n");
}

/// A `mut self` method (receiver `&mut self`) whose body reads `self.field`. The
/// receiver form differs from a `self` getter, exercising the `&mut self` path.
#[test]
fn mut_self_method_body() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Acc {
    total: Int

    fn doubled(mut self) -> Int {
        return (self.total + self.total)
    }
}
fn main() {
    a := Acc { total: 7 }
    print(a.doubled())
}
";
    let (code, stdout) = build_and_run("tir_mut_self", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "14\n");
}

/// An enum method (a `when self` match in the body), plus a static call site,
/// covered end-to-end.
#[test]
fn enum_method_body_and_static_call() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Sign {
    Pos
    Neg
    Zero

    fn make_pos() -> Sign {
        return Sign.Pos
    }
    fn to_num(self) -> Int {
        if self {
            Pos -> { return 1 }
            Neg -> { return 0 }
            Zero -> { return 0 }
        }
    }
}
fn main() {
    s @= Sign.make_pos()
    print(s.to_num())
}
";
    let (code, stdout) = build_and_run("tir_enum_method_static", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

/// An instance method that calls another method on `self` and returns a new value
/// of the owning struct type — the method-to-method dispatch through the TIR, plus
/// a static constructor and a method returning a fresh struct literal.
#[test]
fn method_calls_method_and_returns_struct() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Vec2 {
    x: Int
    y: Int

    fn make(x: Int, y: Int) -> Vec2 {
        return Vec2 { x: x, y: y }
    }
    fn sum(self) -> Int {
        return (self.x + self.y)
    }
    fn shifted(self, dx: Int) -> Vec2 {
        return Vec2 { x: (self.x + dx), y: self.y }
    }
}
fn main() {
    p @= Vec2.make(3, 4)
    q @= p.shifted(10)
    print(q.sum())
}
";
    let (code, stdout) = build_and_run("tir_method_chain", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "17\n");
}
