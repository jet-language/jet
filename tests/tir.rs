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
    if c == {
        Active(id) | Reconnecting(id) -> { return \"live:{id}\" }
        Idle(_) -> { return \"idle\" }
        Closed -> { return \"closed\" }
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
    if r == {
        Good(200..299) -> { return \"success\" }
        Good(400..499) -> { return \"client error\" }
        Good(_) -> { return \"other\" }
        Fail(_) -> { return \"network error\" }
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
    if score == {
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
        if self == {
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

    fn doubled(~self) -> Int {
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
        if self == {
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

// c109 Phase 8: fallible + optional.

/// A fallible `T ? E` function with `ok`/`err` constructors and `?` propagation
/// across a covered scalar-payload error enum, consumed with `??` value fallback.
/// `parse_age`, `load`, and `main` all route through the TIR.
#[test]
fn fallible_try_and_or_fallback() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum ParseError {
    Empty
    BadDigit(Int)
}
fn parse_age(raw: Int) -> Int ? ParseError {
    if raw == 0 {
        return err(ParseError.Empty)
    }
    if raw == 1 {
        return err(ParseError.BadDigit(raw))
    }
    return ok((raw * 2))
}
fn load(raw: Int) -> Int ? ParseError {
    n @= parse_age(raw)?
    return ok((n + 1))
}
fn main() {
    a @= load(21) ?? 0
    print(a)
    b @= load(0) ?? 99
    print(b)
}
";
    let (code, stdout) = build_and_run("tir_fallible", src);
    assert_eq!(code, 0);
    // load(21): parse_age→ok(42), n=42, ok(43); ?? → 43.
    // load(0):  parse_age→err(Empty), ? propagates Err; ?? → 99.
    assert_eq!(stdout, "43\n99\n");
}

/// The `??` fallback in its early-`return` form (a `T ? E` value), plus `ok`/`err`.
#[test]
fn or_fallback_return_form() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn checked(x: Int) -> Int ? Error {
    if x == 0 {
        return err(\"zero\")
    }
    return ok((100 / x))
}
fn safe(x: Int) -> Int {
    return checked(x) ?? return -1
}
fn main() {
    print(safe(4))
    print(safe(0))
}
";
    let (code, stdout) = build_and_run("tir_or_return", src);
    assert_eq!(code, 0);
    // safe(4): checked→ok(25), ?? → 25. safe(0): checked→err, ?? return -1.
    assert_eq!(stdout, "25\n-1\n");
}

/// An optional `T?` with `value`/`null` constructors and a `??` fallback.
#[test]
fn optional_value_null_and_fallback() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn first_even(limit: Int) -> (Int?) {
    loop i in 1..limit {
        if (i % 2) == 0 {
            return value(i)
        }
    }
    return null
}
fn main() {
    print(first_even(9) ?? 0)
    print(first_even(1) ?? 0)
}
";
    let (code, stdout) = build_and_run("tir_optional", src);
    assert_eq!(code, 0);
    // first_even(9)→value(2); first_even(1)→null → 0.
    assert_eq!(stdout, "2\n0\n");
}

/// Optional field chaining `?.` (both `.map` and flattening `.and_then`), with a
/// nested optional field. `nick` routes through the TIR.
#[test]
fn optional_chaining() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Profile {
    handle: (String?)
}
struct Account {
    profile: Profile
}
fn handle_of(a: (Account?)) -> (String?) {
    return a?.profile?.handle
}
fn main() {
    p @= Profile { handle: value(\"jay\") }
    acct @= Account { profile: p }
    print(handle_of(value(acct)) ?? \"none\")
    missing: (Account?) @= null
    print(handle_of(missing) ?? \"none\")
}
";
    let (code, stdout) = build_and_run("tir_optchain", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "jay\nnone\n");
}

// c109 Phase 9: built-in collection/string methods. These route through the TIR
// (`recv_type == None` + a covered builtin name), with the Map-vs-List-vs-String
// emit branch resolved at lowering. Each proves rustc accepts the output (I2) and
// runs correctly. Closure-taking methods (`map`/`filter`/…) are deferred (Phase 11).

/// List methods: push, insert, get, first, last, len, contains, index_of, sort,
/// reverse, pop — a covered function exercising the non-closure list surface.
#[test]
fn list_builtin_methods() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn build() -> [Int] {
    xs := [3, 1, 2]
    xs.push(5)
    xs.insert(0, 0)
    xs.sort()
    xs.reverse()
    return xs
}
fn main() {
    xs := build()
    print(xs.len())
    print(xs.contains(5))
    print(xs.index_of(2))
    g := xs.get(0)
    print(g ?? 0)
    f := xs.first()
    print(f ?? 0)
}
";
    let (code, stdout) = build_and_run("tir_list_builtins", src);
    assert_eq!(code, 0);
    // sorted [0,1,2,3,5] reversed → [5,3,2,1,0]. len 5, contains 5 true,
    // index_of 2 → 2, get(0) → 5, first → 5.
    assert_eq!(stdout, "5\ntrue\n2\n5\n5\n");
}

/// String methods: len (char count), to_upper, to_lower, trim, split, starts_with,
/// ends_with, replace, repeat, slice, chars, contains, to_string.
#[test]
fn string_builtin_methods() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn main() {
    s := \"  Hello, World  \"
    t := s.trim()
    print(t.to_upper())
    print(t.to_lower())
    print(t.len())
    print(t.starts_with(\"Hello\"))
    print(t.ends_with(\"World\"))
    print(t.replace(\"World\", \"Jet\"))
    print(\"ab\".repeat(3))
    print(t.contains(\"World\"))
    parts := \"a,b,c\".split(\",\")
    print(parts.len())
}
";
    let (code, stdout) = build_and_run("tir_string_builtins", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "HELLO, WORLD\nhello, world\n12\ntrue\ntrue\nHello, Jet\nababab\ntrue\n3\n"
    );
}

/// Map methods: insert, get, contains_key, keys, values, len, clear. BTreeMap
/// iterates/collects in sorted key order, so output is deterministic.
#[test]
fn map_builtin_methods() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn main() {
    m: [String, Int] := [:]
    m.insert(\"banana\", 3)
    m.insert(\"apple\", 5)
    print(m.len())
    print(m.contains_key(\"apple\"))
    v := m.get(\"apple\")
    print(v ?? 0)
    ks := m.keys()
    print(ks.len())
    vs := m.values()
    print(vs.len())
}
";
    let (code, stdout) = build_and_run("tir_map_builtins", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\ntrue\n5\n2\n2\n");
}

/// `remove` on both a list (the `jet_list_remove` panic-framed helper) and a map
/// (the `.remove(&(k).clone())` form) — the Map-vs-List branch resolved at lowering.
#[test]
fn list_and_map_remove() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn drop_first(xs: [Int]) -> Int {
    ys := xs
    r := ys.remove(0)
    return ys.len()
}
fn drop_key(m: [String, Int]) -> Int {
    m2 := m
    r := m2.remove(\"a\")
    return m2.len()
}
fn main() {
    print(drop_first([10, 20, 30]))
    counts: [String, Int] := [:]
    counts[\"a\"] = 1
    counts[\"b\"] = 2
    print(drop_key(counts))
}
";
    let (code, stdout) = build_and_run("tir_remove", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\n1\n");
}

/// `join(sep)` on a list of strings — the `.iter().map(jet_show)…join` form.
#[test]
fn list_join_with_separator() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn main() {
    words := [\"a\", \"b\", \"c\"]
    print(words.join(\"-\"))
}
";
    let (code, stdout) = build_and_run("tir_join", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "a-b-c\n");
}

/// A `when` over a fallible value with `ok`/`err` patterns (Shape C). The subject
/// is a user fallible fn call; the bound payload prints.
#[test]
fn fallible_when_match() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn classify(x: Int) -> Int ? Error {
    if x == 0 {
        return err(\"bad\")
    }
    return ok((x + 10))
}
fn main() {
    if classify(5) == {
        ok(n) -> {
            print(n)
        }
        err(e) -> {
            print(e)
        }
    }
    if classify(0) == {
        ok(n) -> {
            print(n)
        }
        err(e) -> {
            print(e)
        }
    }
}
";
    let (code, stdout) = build_and_run("tir_fallible_when", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "15\nbad\n");
}

/// c109 Phase 10: core/stdlib module calls route through the TIR. `math.*`,
/// `path.join`, and `crypto.sha256` are type-monomorphic (in `core_fixed_sig`),
/// so `calc`/`make_path`/`hash`/`main` are all covered. The call forms
/// (`jet_std_math_*`, `jet_std_path_join`, `jet_ring_crypto_sha256`) reproduce
/// `emit_core_call` byte-for-byte; here we prove they compile (I2) and run.
#[test]
fn core_math_path_crypto_calls() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.math as math
use core.path as path
use jet.crypto as crypto
fn calc(a: Float) -> Float {
    r @= math.sqrt(a)
    f @= math.floor(r)
    c @= math.ceil(r)
    return (f + c)
}
fn make_path(a: String, b: String) -> String {
    return path.join(a, b)
}
fn hash(s: String) -> String {
    return crypto.sha256(s)
}
fn main() {
    print(calc(16.0))
    print(make_path(\"/usr\", \"bin\"))
    print(hash(\"hello\"))
}
";
    let (code, stdout) = build_and_run("tir_core_math_path_crypto", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "8.0\n/usr/bin\n\
         2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824\n"
    );
}

/// c109 Phase 10: a fallible core call composed with `??` (Phase 8). `fs.read`
/// returns `Result<String, IOError>`; the `??` value fallback unwraps it, so
/// `read_or` is covered and the `jet_std_fs_read(&(…))` form composes with the
/// `match { Ok(v) => v, Err(_) => fb }` fallback. The missing file takes the
/// fallback branch — proving the composition runs.
#[test]
fn core_fs_read_with_fallback() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.fs as fs
fn read_or(p: String) -> String {
    return (fs.read(p) ?? \"missing\")
}
fn main() {
    print(read_or(\"/no/such/file/at/all/xyzzy\"))
}
";
    let (code, stdout) = build_and_run("tir_core_fs_fallback", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "missing\n");
}

// NOTE: a regex call (`re.is_match(…)?? …`) also routes through the TIR — the
// emitted `<ffi_crate>::jet_regex_is_match(…)` is byte-identical to the AST path
// (verified via the forced-AST-path diff). It is NOT given a `build_and_run` test
// here because the call references the external FFI bridge crate (`cx.ffi_crate`),
// which the bare-`rustc` harness can't resolve standalone — the same reason the
// example suite drives the regex example through the project build, not this file.

// ===================================================================
// c109 Phase 11: lambdas/closures, fan-out, closure-taking collection
// methods. Each program lives entirely inside the covered subset, so the
// covered function(s) route through the TIR; the assert proves rustc
// accepts the output (I2) and it runs correctly. Byte-parity to the AST
// path is verified separately (forced-AST diff across the example suite).
// ===================================================================

/// A list `map`/`filter`/`reduce`/`find`/`any`/`all` with expression-body
/// lambdas, plus a captured (Copy) outer local. The closure methods compose a
/// lambda with the builtin method — the whole `run` routes through the TIR.
#[test]
fn closure_collection_methods() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() -> Int {
    base := 10
    nums := [1, 2, 3, 4, 5]
    squares := nums.map((n: Int) => (n * n))
    big := squares.filter((n: Int) => (n > 5))
    shifted := nums.map((n: Int) => (n + base))
    total := nums.reduce(0, (acc: Int, n: Int) => (acc + n))
    has := nums.any((n: Int) => (n > 4))
    every := nums.all((n: Int) => (n > 0))
    print(big)
    print(shifted)
    print(has)
    print(every)
    return total
}
fn main() {
    print(run())
}
";
    let (code, stdout) = build_and_run("tir_closure_methods", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "[9, 16, 25]\n[11, 12, 13, 14, 15]\ntrue\ntrue\n15\n"
    );
}

/// A FnMut closure (mutates a captured mutable local) routes through the
/// FnMut branch (`jet_list_each_mut`, no `move` keyword) — the Fn-vs-FnMut
/// decision read off the lambda's `needs_fn_mut` meta.
#[test]
fn fnmut_each_closure() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() -> Int {
    nums := [1, 2, 3, 4]
    total := 0
    nums.each((n: Int) => { total = (total + n) })
    return total
}
fn main() {
    print(run())
}
";
    let (code, stdout) = build_and_run("tir_fnmut_each", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "10\n");
}

/// `sort_by` with a key lambda (a list mutated in place). Routes through the
/// `SortBy` op (`{ jet_list_sort_by(&mut recv, f); }`).
#[test]
fn sort_by_closure() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() -> Int {
    nums := [3, 1, 2]
    nums.sort_by((n: Int) => n)
    return nums[0]
}
fn main() {
    print(run())
}
";
    let (code, stdout) = build_and_run("tir_sort_by", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

/// The fan-out operator `f.[a, b, c]` ≡ `[f(a), f(b), f(c)]` (S75/S76) over a
/// plain top-level function. Routes through `TExprKind::FanOut` (each item a
/// synthetic single-arg call, wrapped in `vec![…]`).
#[test]
fn fan_out_operator() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn double(n: Int) -> Int {
    return (n * 2)
}
fn run() -> Int {
    doubled := double.[1, 2, 3]
    print(doubled)
    return doubled[1]
}
fn main() {
    print(run())
}
";
    let (code, stdout) = build_and_run("tir_fan_out", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "[2, 4, 6]\n4\n");
}

/// A call whose callee has a Fn-typed parameter (`apply(f, x)`) is EXCLUDED
/// from the TIR (the fn-value arg needs the `Box::new(…) as …` coercion the
/// plain-call lowering does not emit). It stays on the AST path and still
/// compiles + runs — proving the gate's exclusion is conservative, not lossy.
#[test]
fn fn_typed_param_call_stays_on_ast_path() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn apply(f: fn(Int) -> Int, x: Int) -> Int {
    return f(x)
}
fn main() {
    print(apply((n: Int) => (n + 1), 41))
}
";
    let (code, stdout) = build_and_run("tir_fn_param_excluded", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

/// c109 Phase 12: numeric width conversions (D-NUMOPS1) — widening (`to_i64`,
/// infallible `as`), narrowing (`to_u8`, fallible `try_from` unwrapped with `??`),
/// and int→float (`to_float`, `as`). Each fully-covered function routes through the
/// TIR (`NumericMethod`). rustc accepting + the right runtime values prove parity.
#[test]
fn numeric_width_conversions() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn widen(red: U8) -> I64 {
    return red.to_i64()
}
fn narrow(channel: I32) -> U8 {
    return channel.to_u8() ?? 255
}
fn to_real(x: Int) -> Float {
    return x.to_float()
}
fn main() {
    print(widen(255))
    print(narrow(100))
    print(narrow(100000))
    print(to_real(3))
}
";
    let (code, stdout) = build_and_run("tir_numeric_conv", src);
    assert_eq!(code, 0);
    // 255 (widen), 100 (fits), 255 (overflow → fallback), 3.0 (int→float).
    assert_eq!(stdout, "255\n100\n255\n3.0\n");
}

/// c109 Phase 12: numeric predicates (`is_nan`/`is_finite`), bit-population queries
/// (`count_ones`), and a numeric `to_string`. Each routes through the TIR's
/// `NumericMethod` op; the source widths come from sema's `recv_type` (total).
#[test]
fn numeric_predicates_and_bits() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn bits(flags: U8) -> Int {
    return flags.count_ones()
}
fn finite(f: Float) -> Bool {
    return f.is_finite()
}
fn show(n: I32) -> String {
    return n.to_string()
}
fn main() {
    print(bits(13))
    print(finite(1.5))
    print(show(42))
}
";
    let (code, stdout) = build_and_run("tir_numeric_pred", src);
    assert_eq!(code, 0);
    // 13 = 0b1101 → 3 set bits; 1.5 is finite; 42 as String.
    assert_eq!(stdout, "3\ntrue\n42\n");
}

/// c109 Phase 12: TRAIT-IMPL method bodies. A covered struct implementing a trait
/// (both the inline `impl Trait {}` and the `impl T: Trait` forms) routes its
/// trait-method bodies through the TIR via the `emit_trait_method` hook — bare name,
/// no `pub`, `&self`. rustc accepting + the right output prove byte parity.
#[test]
fn trait_impl_method_bodies() {
    if !have_rustc() {
        return;
    }
    let src = "\
trait Shape {
    fn area(self) -> Float
    fn name(self) -> String
}
struct Circle {
    radius: Float
    impl Shape {
        fn area(self) -> Float {
            return ((3.0 * self.radius) * self.radius)
        }
        fn name(self) -> String {
            return \"circle\"
        }
    }
}
struct Square {
    side: Float
}
impl Square: Shape {
    fn area(self) -> Float {
        return (self.side * self.side)
    }
    fn name(self) -> String {
        return \"square\"
    }
}
fn describe(s: Shape) -> String {
    return \"{s.name()}: {s.area()}\"
}
fn main() {
    shapes: [Shape] @= [Circle {radius: 2.0}, Square {side: 3.0}]
    shapes.each((s) => {
        print(describe(s))
    })
}
";
    let (code, stdout) = build_and_run("tir_trait_methods", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "circle: 12.0\nsquare: 9.0\n");
}

/// c109 Phase 12: an explicit `else { if … }` block must stay `} else { if … }`,
/// NOT collapse to `} else if …` (the AST path keys solely on the source
/// `ElseBranch`, never on the else-body shape). This guards the parity fix to the
/// TIR `If` emit. The function routes through the TIR; rustc accepting proves it
/// compiles, and the value proves the branch is taken correctly.
#[test]
fn explicit_else_block_with_inner_if_not_flattened() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn pick(a: Int, b: Int) -> Int {
    if a > b {
        return a
    } else {
        if b > 0 {
            return b
        }
    }
    return 0
}
fn main() {
    print(pick(5, 3))
    print(pick(2, 7))
    print(pick(0, 0))
}
";
    let (code, stdout) = build_and_run("tir_else_block_if", src);
    assert_eq!(code, 0);
    // pick(5,3)=5 (then); pick(2,7)=7 (else→inner-if true); pick(0,0)=0
    // (else→inner-if false → falls through to the trailing `return 0`).
    assert_eq!(stdout, "5\n7\n0\n");
}

/// c109 Phase 13: fn-typed values. A fn with a `fn(Int)->Int` parameter routes
/// through the TIR (the Box-coercion arg form); a bare fn-name value, a lambda arg,
/// and a call through the fn-value (`f(x)` where `f` is the local param) all lower in
/// subset. Proves the `Box::new(…) as <fn-type>` coercion + the `(f)(args)` call.
#[test]
fn fn_typed_values() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn apply_twice(f: fn(Int) -> Int, x: Int) -> Int {
    return f(f(x))
}
fn double(x: Int) -> Int {
    return (x * 2)
}
fn main() {
    print(apply_twice(double, 3))
    print(apply_twice((n: Int) => (n + 1), 5))
    g @= double
    print(apply_twice(g, 4))
}
";
    let (code, stdout) = build_and_run("tir_fn_values", src);
    assert_eq!(code, 0);
    // apply_twice(double,3)=12; apply_twice(+1,5)=7; apply_twice(double,4)=16.
    assert_eq!(stdout, "12\n7\n16\n");
}

/// c109 Phase 13: a struct field call through a fn-typed field is an `Expr::CallValue`
/// (`(w.step)(x)`). The struct has a `fn` field so it stays on the AST path, but the
/// *call site* `apply_twice((x)=>…, …)` routes — and a fn-value stored in a local then
/// called routes too. This proves the `Expr::Call`-to-a-local fn-value form.
#[test]
fn fn_value_call_through_local() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run(f: fn(Int) -> Int) -> Int {
    return f(10)
}
fn inc(x: Int) -> Int {
    return (x + 1)
}
fn main() {
    print(run(inc))
    print(run((y: Int) => (y * y)))
}
";
    let (code, stdout) = build_and_run("tir_fn_value_local", src);
    assert_eq!(code, 0);
    // run(inc)=11; run(square)=100.
    assert_eq!(stdout, "11\n100\n");
}

/// c109 Phase 13: `scope.guard(() => { … })` — a closure-taking core call (NOT in
/// `core_fixed_sig`). The guard fires on scope exit (LIFO). Routes through the TIR with
/// the bespoke `jet_scope_guard(<closure>)` emit shape.
#[test]
fn scope_guard_closure_core_call() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.scope as scope
fn work() {
    _g @= scope.guard(() => { print(\"cleanup\") })
    print(\"working\")
}
fn main() {
    work()
}
";
    let (code, stdout) = build_and_run("tir_scope_guard", src);
    assert_eq!(code, 0);
    // The guard's closure runs at scope exit, AFTER \"working\".
    assert_eq!(stdout, "working\ncleanup\n");
}

/// c109 Phase 13: `tasks.spawn(() => …)` — the distinct `emit_spawn_lambda` form
/// (`move |…|`, never `Box::new`). The spawned task computes a value joined back.
/// Routes through the TIR with `JetTask::spawn(move || …)`.
#[test]
fn tasks_spawn_closure_core_call() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.tasks as tasks
fn compute() -> Int {
    return 21
}
fn launch() -> Int {
    t @= tasks.spawn(() => compute())
    return t.join()
}
fn main() {
    print(launch())
}
";
    // `launch` itself stays on the AST path (`t.join()` is a Task method, not covered),
    // but the spawn EXPRESSION and `compute` route; rustc accepting proves parity.
    let (code, stdout) = build_and_run("tir_tasks_spawn", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "21\n");
}

/// c109 Phase 13: handle methods. A FileWriter (from `files.create`) routes through
/// the TIR for `write_line`/`flush` (the `&mut` handle arms of `emit_builtin_method`).
/// A handle binding also forces `let mut` even when bound immutably — the parity fix
/// to the TIR `Let`. Proves the handle-method emit + the forced-mut binding.
#[test]
fn handle_methods_file_writer() {
    if !have_rustc() {
        return;
    }
    // Write/read through an absolute temp path so the test leaves no repo artifact.
    let tmp = std::env::temp_dir().join(format!("jet_tir_handle_{}.txt", std::process::id()));
    let tmp_str = tmp.to_string_lossy().replace('\\', "\\\\");
    let src = format!(
        "\
use core.files as files
use core.fs as fs
fn write_file(path: String, text: String) -> Int {{
    w := files.create(path) ?? return 0
    _r @= w.write_line(text)
    _f @= w.flush()
    return 1
}}
fn main() {{
    done @= write_file(\"{path}\", \"hello handle\")
    print(done)
    contents @= fs.read(\"{path}\") ?? \"<none>\"
    print(contents)
}}
",
        path = tmp_str
    );
    let (code, stdout) = build_and_run("tir_handle_writer", &src);
    let _ = fs::remove_file(&tmp);
    assert_eq!(code, 0);
    // write_file returns 1 (success); the file contains the written line + newline.
    assert_eq!(stdout, "1\nhello handle\n\n");
}

/// Build + run a multi-file program: write each `(relative path, source)` pair into a
/// fresh temp dir, compile the entry, then rustc + run. Used by the Phase-14
/// cross-module tests, which need sibling module files on disk.
fn build_and_run_multi(name: &str, entry: &str, files: &[(&str, &str)]) -> (i32, String) {
    let dir = std::env::temp_dir().join(format!("jet_tir_multi_{}_{}", name, std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for (rel, src) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, src).unwrap();
    }
    let entry_path = dir.join(entry);
    let shown = entry_path.to_string_lossy().into_owned();
    let entry_src = fs::read_to_string(&entry_path).unwrap();
    let out = jet::compile_with_path(&entry_src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, &entry_src, &diags)
        )
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
        "rustc rejected generated code (I2 violation):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    (run.status.code().unwrap_or(0), String::from_utf8_lossy(&run.stdout).into_owned())
}

/// c109 Phase 14: a qualified inline code-module call `math.double(5)` (D-MOD2).
/// `main` routes through the TIR (`ModuleCall::InlineMangled` → `user_math__double`),
/// as do the module's own functions. rustc accepting proves byte-parity.
#[test]
fn inline_code_module_qualified_call() {
    if !have_rustc() {
        return;
    }
    let src = "\
module math {
    pub fn double(n: Int) -> Int {
        return (n * 2)
    }
    pub fn add(a: Int, b: Int) -> Int {
        return (a + b)
    }
}
fn main() {
    print(math.double(5))
    print(math.add(3, 4))
}
";
    let (code, stdout) = build_and_run("tir_inline_mod", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "10\n7\n");
}

/// c109 Phase 14: an unqualified inline-module import `use math.double` (D-MOD3).
/// The bare `double(7)` lowers via `emit_call`'s `unqualified_inline` arm
/// (`ModuleCall::InlineMangled`). `main` routes through the TIR.
#[test]
fn unqualified_inline_module_call() {
    if !have_rustc() {
        return;
    }
    let src = "\
use math.double
module math {
    pub fn double(n: Int) -> Int {
        return (n * 2)
    }
}
fn main() {
    print(double(7))
}
";
    let (code, stdout) = build_and_run("tir_unqual_inline", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "14\n");
}

/// c109 Phase 14: a qualified file-module call `math.clamp(...)` (D-MOD1). `main`
/// routes through the TIR (`ModuleCall::Qualified` → `user_math::user_clamp`); the
/// imported module's `clamp` routes too. A String-arg module call also exercises the
/// `&(...)` Read-borrow arg form.
#[test]
fn file_module_qualified_call() {
    if !have_rustc() {
        return;
    }
    let main_src = "\
module math
fn main() {
    print(math.clamp(15, 0, 10))
    print(math.tag(\"x\", 5))
}
";
    let math_src = "\
pub fn clamp(x: Int, lo: Int, hi: Int) -> Int {
    if (x < lo) {
        return lo
    }
    if (x > hi) {
        return hi
    }
    return x
}
pub fn tag(prefix: String, n: Int) -> String {
    return \"{prefix}:{n}\"
}
";
    let (code, stdout) = build_and_run_multi(
        "tir_file_mod",
        "main.jet",
        &[("main.jet", main_src), ("math.jet", math_src)],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "10\nx:5\n");
}

/// c109 Phase 14: an unqualified file-module import `use mathlib.{clamp, lo, hi}`
/// (D-MOD3). The bare calls lower via `emit_call`'s `unqualified_file` arm
/// (`ModuleCall::Qualified` → `user_mathlib::user_*`). `main` routes through the TIR.
#[test]
fn unqualified_file_module_call() {
    if !have_rustc() {
        return;
    }
    let main_src = "\
use mathlib.clamp
use mathlib.{lo, hi}
module mathlib
fn main() {
    print(clamp(200, lo(), hi()))
}
";
    let mathlib_src = "\
pub fn clamp(n: Int, lo: Int, hi: Int) -> Int {
    if (n < lo) {
        return lo
    }
    if (n > hi) {
        return hi
    }
    return n
}
pub fn lo() -> Int {
    return 0
}
pub fn hi() -> Int {
    return 100
}
";
    let (code, stdout) = build_and_run_multi(
        "tir_unqual_file",
        "main.jet",
        &[("main.jet", main_src), ("mathlib.jet", mathlib_src)],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "100\n");
}

/// c109 Phase 14: a `pub use` re-export (D-MOD4). `text.wrap(...)` resolves through
/// the directory module's `pub use wrap.wrap` and lowers via `reexport_calls`
/// (`ModuleCall::Qualified` → `user_wrap::user_wrap`). The String arg exercises the
/// Read-borrow form (`&(...)`). `main` + the submodule fns route through the TIR.
#[test]
fn reexport_module_call() {
    if !have_rustc() {
        return;
    }
    let main_src = "\
module text
fn main() {
    print(text.wrap(\"hi\"))
}
";
    let module_src = "\
pub use wrap.wrap
module wrap
";
    let wrap_src = "\
pub fn wrap(s: String) -> String {
    return \"[{decorate(s)}]\"
}
fn decorate(s: String) -> String {
    return \"{s}\"
}
";
    let (code, stdout) = build_and_run_multi(
        "tir_reexport",
        "main.jet",
        &[
            ("main.jet", main_src),
            ("text/module.jet", module_src),
            ("text/wrap.jet", wrap_src),
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "[hi]\n");
}

/// c109 Phase 15: a resolved comptime-if (`comptime if … { } else { }`). Sema selects
/// the branch; codegen emits ONLY that branch's statements inline (no `if`). Here
/// `DEBUG` is `false`, so the `else` branch is emitted; the `then` branch is dropped.
#[test]
fn comptime_if_selected_branch() {
    if !have_rustc() {
        return;
    }
    let src = "\
comptime DEBUG = false
fn pick(x: Int) -> Int {
    comptime if DEBUG {
        return x + 100
    } else {
        return x + 1
    }
    return 0
}
fn main() {
    print(\"{pick(5)}\")
}
";
    let (code, stdout) = build_and_run("tir_comptime_if", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "6\n");
}

/// c109 Phase 15 / D-IF3: a MIXED value+range dispatch — the general `emit_mixed_switch`
/// `if/else if … else` chain (shape D). A bare-value arm (`100 ->` ≡ `score == 100`)
/// sits next to range arms (`90..99 ->`), which lower to `score >= lo && score <= hi`;
/// the body picks the first matching band. (Q4 retired free-predicate arm heads; a
/// value+range mix is the surviving way to reach the mixed if/else chain.)
#[test]
fn mixed_comparison_switch() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn grade(score: Int) -> String {
    if score == {
        100 -> { return \"A+\" }
        90..99 -> { return \"A\" }
        80..89 -> { return \"B\" }
        else -> { return \"F\" }
    }
    return \"?\"
}
fn main() {
    print(grade(85))
    print(grade(95))
    print(grade(100))
    print(grade(50))
}
";
    let (code, stdout) = build_and_run("tir_mixed_switch", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "B\nA\nA+\nF\n");
}

/// c109 Phase 15: a DELEGATION trait method (`impl T: Trait using field`). The
/// forwarding method routes through the TIR's `Delegation` kind — `(self).<field>.<m>(…)`
/// with the bare trait method name.
#[test]
fn delegation_trait_method() {
    if !have_rustc() {
        return;
    }
    let src = "\
trait Speaker {
    fn say(self, msg: String) -> String
}
struct Voice {
    prefix: String
}
impl Voice: Speaker {
    fn say(self, msg: String) -> String {
        p @= self.prefix
        return \"{p}: {msg}\"
    }
}
struct Megaphone {
    inner: Voice
}
impl Megaphone: Speaker using inner
fn main() {
    v := Voice { prefix: \"HEY\" }
    m := Megaphone { inner: v }
    print(m.say(\"go\"))
}
";
    let (code, stdout) = build_and_run("tir_delegation", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "HEY: go\n");
}

/// c109 Phase 15: the `a ?? panic(…)` fallback form. The panic message + the sorted
/// scalar-locals snapshot (`safe_locals_expr`) are reproduced from the `panic_locals`
/// replica. On the success path the fallback is never taken; the program returns the
/// unwrapped value.
#[test]
fn or_fallback_panic_form() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn maybe(n: Int) -> (Int?) {
    if n > 0 {
        return value(n)
    }
    return null
}
fn risky(count: Int, ratio: Float) -> Int {
    base := count + 1
    got @= maybe(count) ?? panic(\"no value at {count}\")
    return got + base
}
fn main() {
    print(\"{risky(5, 1.5)}\")
}
";
    let (code, stdout) = build_and_run("tir_panic_fallback", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "11\n");
}

/// c109 Phase 16: a String-payload enum. The construction `Msg.Text(s)` (a borrowed
/// String param) inserts `((*s)).clone()` at the literal site (`emit_boxed_enum_arg`);
/// the match binds the payload value and returns it (owning).
#[test]
fn string_payload_enum() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Msg {
    Text(String)
    Code(Int)
}
fn wrap(s: String) -> Msg {
    return Msg.Text(s)
}
fn render(m: Msg) -> String {
    if m == {
        Text(s) -> { return s }
        Code(n) -> { return \"code\" }
    }
    return \"?\"
}
fn main() {
    m @= wrap(\"hi\")
    print(render(m))
}
";
    let (code, stdout) = build_and_run("tir_string_payload_enum", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "hi\n");
}

/// c109 Phase 16: a recursive (boxed) enum. Constructing `Tree.Node(inner)` from a
/// borrowed `inner: Tree` emits `Box::new(((*inner)).clone())` — the non-scalar
/// payload borrowed-clone, then the recursive boxed edge. The match traverses it
/// (Rust auto-derefs the `Box`).
#[test]
fn recursive_boxed_enum() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Tree {
    Leaf(Int)
    Node(Tree)
}
fn wrap(inner: Tree) -> Tree {
    return Tree.Node(inner)
}
fn leaf_val(t: Tree) -> Int {
    if t == {
        Leaf(n) -> { return n }
        Node(inner) -> { return 0 }
    }
    return 0
}
fn main() {
    a @= Tree.Leaf(7)
    b @= wrap(a)
    print(\"{leaf_val(b)}\")
}
";
    let (code, stdout) = build_and_run("tir_recursive_boxed_enum", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "0\n");
}

/// c109 Phase 16: an enum variant carrying a covered struct payload. The struct flows
/// through the variant construction and the pattern binding; reading `p.x` from the
/// bound struct is an owning field read (sema rewrites it to `.clone()`).
#[test]
fn struct_payload_enum() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Point {
    x: Int
    y: Int
}
enum Shape {
    Dot(Point)
    Line(Int)
}
fn mk(p: Point) -> Shape {
    return Shape.Dot(p)
}
fn first(s: Shape) -> Int {
    if s == {
        Dot(p) -> { return p.x }
        Line(n) -> { return n }
    }
    return 0
}
fn main() {
    pt @= Point { x: 3, y: 4 }
    sh @= mk(pt)
    print(\"{first(sh)}\")
}
";
    let (code, stdout) = build_and_run("tir_struct_payload_enum", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

/// c109 Phase 16: a struct with a covered collection field, and an enum variant
/// carrying a covered collection payload. Both emit the field/payload value plainly
/// (`items: vec![…]`, `Data.Nums(xs)`), byte-identical to the AST path.
#[test]
fn collection_field_and_payload() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Bag {
    items: [Int]
    label: String
}
enum Data {
    Nums([Int])
    One(Int)
}
fn mk(xs: [Int]) -> Data {
    return Data.Nums(xs)
}
fn main() {
    b @= Bag { items: [1, 2, 3], label: \"x\" }
    d @= mk([4, 5])
    print(b.label)
}
";
    let (code, stdout) = build_and_run("tir_collection_field_payload", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "x\n");
}

/// c109 Phase 17: a GENERIC free function. The `<T: Clone>` clause renders at lowering
/// (every type param carries an extra `Clone` bound, exactly `emit_func`), a type-var
/// param/return is by-value (`user_x: T`), and the body returns the type-var value. A
/// generic `[T]` list param/return is covered too (`Vec<T>`).
#[test]
fn generic_free_fns() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn id<T>(x: ^T) -> T {
    return x
}
fn pick<T>(a: ^T, b: ^T, first: Bool) -> T {
    if first {
        return a
    }
    return b
}
fn firstof<T>(xs: ^[T]) -> T {
    return xs[0]
}
fn wrap<T>(x: ^T) -> [T] {
    return [x]
}
fn main() {
    print(\"{id(5)}\")
    print(\"{pick(1, 2, true)}\")
    print(\"{firstof([10, 20, 30])}\")
    ys @= wrap(7)
    print(\"{ys[0]}\")
}
";
    let (code, stdout) = build_and_run("tir_generic_free_fns", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\n1\n10\n7\n");
}

/// c109 Phase 17: a `-> &T` function returns a borrow. The signature renders `&T`
/// and the body's `return field.name` lowers via `emit_view_return` to `&((*field)).name`
/// (a field read of an owned param, address taken). A `view` ident param returns the bare
/// borrow.
#[test]
fn view_return_fn() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Rec {
    name: String
    value: String
}
fn name_of(rec: Rec) -> &String {
    return rec.name
}
fn main() {
    r @= Rec { name: \"alpha\", value: \"beta\" }
    print(\"{name_of(r)}\")
}
";
    let (code, stdout) = build_and_run("tir_view_return_fn", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "alpha\n");
}

/// c109 Phase 17: a PRELUDE struct (HttpResponse/HttpRequest) constructed via a struct
/// literal. The `is_prelude_struct` emit branch renders a `Jet…` Rust head with PLAIN
/// (unmangled) fields, and HttpRequest injects a `params: BTreeMap::new()` field. The
/// prelude types live in `jet_std`, which a standalone `rustc` here can't link, so this
/// asserts the EMITTED Rust contains the byte-exact construction (the example suite +
/// the JET_NO_TIR full-suite diff prove it compiles & runs). The type is a covered value
/// type as a param/return.
#[test]
fn prelude_struct_construction() {
    let src = "\
fn build_resp(body: String) -> HttpResponse {
    return HttpResponse {status: \"200 OK\", body: body, headers: [:]}
}
fn build_req() -> HttpRequest {
    return HttpRequest {method: \"GET\", path: \"/\", body: \"\", headers: [:]}
}
fn main() {
    r @= build_resp(\"hi\")
    q @= build_req()
    print(\"built\")
}
";
    // `compile_with_path` loads the entry from disk, so write the .jet first.
    let dir = std::env::temp_dir().join(format!("jet_tir_prelude_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join("prelude.jet");
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!("front end rejected:\n{}", jet::render_diagnostics(&shown, src, &diags))
    });
    // HttpResponse: prelude head (`…JetHttpResponse`), PLAIN field names, no injected
    // `params`. The `…` root prefix varies by emit layout — assert the prefix-independent
    // construction body.
    assert!(
        out.rust.contains("JetHttpResponse { status: \"200 OK\".to_string(), body: (*user_body), headers: std::collections::BTreeMap::new() }"),
        "HttpResponse construction not byte-exact:\n{}",
        out.rust
    );
    // HttpRequest: prelude head, plain fields, injected `params` field appended verbatim.
    assert!(
        out.rust.contains("JetHttpRequest { method: \"GET\".to_string(), path: \"/\".to_string(), body: \"\".to_string(), headers: std::collections::BTreeMap::new(), params: std::collections::BTreeMap::new() }"),
        "HttpRequest construction not byte-exact:\n{}",
        out.rust
    );
}

/// c109 Phase 18 / D-UNSAFE2: the expert low-level tier (S58, E2-M13/D-LL1). A
/// `#Unsafe("reason") fn` lowers to a Rust `unsafe fn`; a `#Unsafe("reason") { … }`
/// audited region lowers to `unsafe { … }` (the reason string emits nothing);
/// `mem.Ptr<T>.from_addr(addr)`, `mem.address_of(x)`, and `mem.volatile_read(p)` lower
/// to the raw-pointer ops. I1: every emitted `unsafe` is a gated form tied 1:1 to a source gate.
#[test]
fn unsafe_fn_block_and_ptr_ops() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.mem
#Unsafe(\"reads through a raw pointer; addr must be a live, valid Int\")
fn read_reg(addr: Int) -> Int {
    p @= mem.Ptr<Int>.from_addr(addr)
    return mem.volatile_read(p)
}
fn main() {
    cell: Int @= 1337
    addr @= mem.address_of(cell)
    #Unsafe(\"addr is the address of `cell`, a live Int on this stack frame\") {
        p @= mem.Ptr<Int>.from_addr(addr)
        seen @= mem.volatile_read(p)
        print(seen)
        again @= read_reg(addr)
        print(again)
    }
}
";
    let (code, stdout) = build_and_run("tir_unsafe_lowlevel", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1337\n1337\n");
}

/// c109 Phase 18 / D-UNSAFE2: assert the EMITTED Rust for the unsafe tier is byte-exact
/// (the gate forms + ptr ops), and that EVERY `unsafe` is a gated form (`unsafe fn` /
/// `unsafe {`) — the I1 self-check. The reason string emits no comment/marker.
#[test]
fn unsafe_tier_emit_is_byte_exact() {
    let src = "\
use core.mem
#Unsafe(\"reads through a raw pointer; addr must be valid\")
fn read_reg(addr: Int) -> Int {
    p @= mem.Ptr<Int>.from_addr(addr)
    return mem.volatile_read(p)
}
fn main() {
    cell: Int @= 1337
    addr @= mem.address_of(cell)
    #Unsafe(\"safe: cell is live\") {
        seen @= read_reg(addr)
        print(\"{seen}\")
    }
}
";
    let dir = std::env::temp_dir().join(format!("jet_tir_unsafe_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join("unsafe.jet");
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!("front end rejected:\n{}", jet::render_diagnostics(&shown, src, &diags))
    });
    // `#Unsafe fn` → `pub unsafe fn …`.
    assert!(
        out.rust.contains("pub unsafe fn user_read_reg(user_addr: i64) -> i64 {"),
        "unsafe fn signature not byte-exact:\n{}",
        out.rust
    );
    // `mem.Ptr<Int>.from_addr(addr)` and `mem.volatile_read(p)` in the fn body (sema
    // annotates the inferred `p` binding with its resolved `*mut i64` type).
    assert!(
        out.rust.contains("let user_p: *mut i64 = ((user_addr) as usize as *mut i64);"),
        "PtrFromAddr not byte-exact:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("return std::ptr::read_volatile(user_p);"),
        "volatile_read not byte-exact:\n{}",
        out.rust
    );
    // `mem.address_of(cell)` → the inert address cast (no `unsafe`).
    assert!(
        out.rust.contains("let user_addr: i64 = (&(user_cell) as *const _ as usize as i64);"),
        "address_of not byte-exact:\n{}",
        out.rust
    );
    // `#Unsafe("…") { … }` → `unsafe {` (the reason string emits nothing).
    assert!(out.rust.contains("    unsafe {\n"), "unsafe block not emitted:\n{}", out.rust);
    // Reason string emits nothing — "safe: cell is live" must not appear in generated Rust.
    assert!(!out.rust.contains("safe: cell is live"), "reason string must emit nothing:\n{}", out.rust);
    // I1 self-check: drop the vetted `jet_mem` prelude, then every remaining `unsafe`
    // must be a gated form (`unsafe {` or `unsafe fn`).
    let user = if let Some(s) = out.rust.find("mod jet_mem") {
        let b = out.rust.as_bytes();
        let (mut d, mut i, mut end, mut seen) = (0usize, s, out.rust.len(), false);
        while i < b.len() {
            match b[i] {
                b'{' => { d += 1; seen = true; }
                b'}' => { d -= 1; if seen && d == 0 { end = i + 1; break; } }
                _ => {}
            }
            i += 1;
        }
        format!("{}{}", &out.rust[..s], &out.rust[end..])
    } else {
        out.rust.clone()
    };
    for line in user.lines() {
        // Skip comment lines (the source-map path comment can contain the word).
        if line.trim_start().starts_with("//") {
            continue;
        }
        if let Some(col) = line.find("unsafe") {
            let after = line[col..].trim_start_matches("unsafe").trim_start();
            assert!(
                after.starts_with('{') || after.starts_with("fn "),
                "I1: ungated `unsafe` in generated code: {}",
                line.trim()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// c109 Phase 19: generic structs, foreign types, Stopwatch, arena/region.
// ---------------------------------------------------------------------------

/// c109 Phase 19: a GENERIC STRUCT free function — a turbofish struct literal
/// (`user_Pair::<i64> { … }`), a `Type::Apply` param/return, a `[T]`-field builtin
/// (`copy.items.push(item)`), and the generic-struct value clone (`copy := s`).
#[test]
fn generic_struct_fns() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Pair<T> {
    first: T
    second: T
}
fn make_pair<T>(a: T, b: T) -> Pair<T> {
    return Pair<T> {first: a, second: b}
}
struct Stack<T> {
    items: [T]
}
fn empty_stack<T>() -> Stack<T> {
    return Stack<T> {items: []}
}
fn push<T>(s: Stack<T>, item: T) -> Stack<T> {
    copy := s
    copy.items.push(item)
    return copy
}
fn main() {
    p: Pair<Int> @= make_pair(1, 2)
    print(p.first)
    st: Stack<Int> := empty_stack()
    st = push(st, 42)
    print(st.items[0])
}
";
    let (code, stdout) = build_and_run("tir_generic_struct", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n42\n");
}

/// c109 Phase 19: a FOREIGN (imported user) struct constructed via the `import_ns`
/// namespace path (`alias.Note { … }` → `{root}user_note::user_Note { … }`), passed
/// across the module boundary, with a field read on the returned value.
#[test]
fn foreign_struct_construction() {
    if !have_rustc() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_tir_foreign_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("note.jet"),
        "pub struct Note {\n    pub title: String\n    pub pages: Int\n}\n",
    )
    .unwrap();
    let main_src = "\
use \"note\"
fn make() -> Note {
    return note.Note { title: \"hello\", pages: 3 }
}
fn main() {
    n := make()
    print(n.title)
    print(n.pages)
}
";
    let main_path = dir.join("main.jet");
    fs::write(&main_path, main_src).unwrap();
    let shown = main_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(main_src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, main_src, &diags)
        )
    });
    // The foreign struct head + mangled fields, byte-exact.
    assert!(
        out.rust
            .contains("user_note::user_Note { user_title: \"hello\".to_string(), user_pages: 3i64 }"),
        "foreign struct construction not byte-exact:\n{}",
        out.rust
    );
}

/// c109 Phase 19: `Stopwatch.elapsed_millis()` (a `recv_type == None` builtin-name
/// handle method) — the `time.start` producer (covered) + the elapsed read.
#[test]
fn stopwatch_elapsed_millis() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.time
fn main() {
    sw := time.start()
    n := 0
    loop i in 0..100 {
        n = n + i
    }
    ms := sw.elapsed_millis()
    print(ms >= 0)
    print(n)
}
";
    let (code, stdout) = build_and_run("tir_stopwatch", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n5050\n");
}

/// c109 Phase 19: arena allocators — the `mem.Arena.new()` / `mem.Bump.new()` /
/// `mem.Pool.new(slots:)` / `mem.Fixed.new(size:)` producers, the `alloc`/`reset`/`free`
/// handle methods, and the `arena_view` binding (`x @= arena.alloc(v)`, read via deref).
#[test]
fn arena_alloc_reset_free() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.mem
fn main() {
    arena @= mem.Arena.new()
    x @= arena.alloc(42)
    print(x)
    arena.reset()
    y @= arena.alloc(99)
    print(y)
    sized @= mem.Arena.new(capacity: 4096)
    s @= sized.alloc(7)
    print(s)
    pool @= mem.Pool.new(slots: 8)
    p @= pool.alloc(3)
    print(p)
    arena.free()
}
";
    let (code, stdout) = build_and_run("tir_arena", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n99\n7\n3\n");
}

/// c109 Phase 19: an explicit `region r { … }` block (D-REGION1) — a plain Rust block
/// scope; views made inside live only until the block ends.
#[test]
fn arena_region_block() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.mem
fn main() {
    region scratch {
        a @= mem.Arena.new()
        b @= mem.Bump.new()
        x @= a.alloc(1)
        y @= b.alloc(2)
        print(x)
        print(y)
    }
    print(99)
}
";
    let (code, stdout) = build_and_run("tir_region", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n2\n99\n");
}

/// c109 Phase 19: a `#Context(allocator: …) { … }` smart-context block (D-CTX1) — a
/// plain block with an `_ctx_guard_<i>` RAII guard, body leaking like a region.
#[test]
fn smart_context_block() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.mem
fn main() {
    arena @= mem.Arena.new()
    #Context(allocator: arena) {
        x @= arena.alloc(10)
        print(x)
    }
    y @= arena.alloc(20)
    print(y)
    arena.free()
}
";
    let (code, stdout) = build_and_run("tir_context", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "10\n20\n");
}

/// c109 Phase 20: the polymorphic core specials (`math.abs/min/max/clamp`,
/// `random.pick/shuffle`, `io.eprint`). Their return type is arg-type dependent
/// (resolved by sema's bespoke `infer_core_call`) and written onto the
/// `Expr::MethodCall.resolved_ret` field, read at lowering so the TIR is total
/// (I3). The emit forms (`(x).abs()`, `(a).min(b)`, `jet_std_random_pick(&(xs))`,
/// `eprintln!`) reproduce `emit_core_call` byte-for-byte. `random.pick` returns
/// `Int?` (the element type wrapped in Option), proving the resolved_ret writeback.
#[test]
fn polymorphic_core_specials() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.math as math
use core.random as random
use core.io as io
fn run() -> Int {
    a @= math.abs((-5))
    b @= math.min(3, 7)
    c @= math.max(3, 7)
    d @= math.clamp(15, 0, 10)
    io.eprint(\"trace: {a} {b} {c} {d}\")
    xs := [1, 2, 3]
    random.shuffle(~xs)
    p @= random.pick(xs)
    return ((((a + b) + c) + d) + (p ?? 0))
}
fn main() {
    print(run())
}
";
    let (code, _stdout) = build_and_run("tir_poly_specials", src);
    assert_eq!(code, 0);
    // a=5, b=3, c=7, d=10, p ∈ {1,2,3}; sum = 25 + p ∈ {26,27,28}.
}

/// c109 Phase 20: HttpRequest/HttpResponse method accessors (`req.method()`/
/// `req.path()`/`req.body()`/`req.header(n)`/`req.param(n)`/`resp.status()`/
/// `resp.body()`/`resp.header(n)`). These carry `recv_type == Some(HttpRequest|
/// HttpResponse)`; now that the lambda-param type is written back onto `p.ty`
/// (sema), the slot type is total and the handle-op shape selects correctly. The
/// emit (`(recv).<field>.clone()`, `(recv).headers.get(&a0).cloned()`,
/// `jet_http_request_param(&(recv), &(a0))`) reproduces `emit_builtin_method`
/// byte-for-byte. `handle` is a typed free function (the example form); it routes.
#[test]
fn http_request_response_accessors() {
    if !have_rustc() {
        return;
    }
    // `http.parse` triggers the http prelude (so `JetHttpRequest`/the accessor
    // helpers are in scope) and yields an HttpRequest without networking; a
    // single-line request keeps the lexer happy (Jet has no `\r` escape).
    let src = "\
use jet.http as http
fn handle(req: HttpRequest) -> HttpResponse {
    m @= req.method()
    p @= req.path()
    h @= req.header(\"host\")
    q @= req.param(\"id\")
    body @= \"m={m} p={p}\"
    return HttpResponse {status: \"200 OK\", body: body, headers: [:]}
}
fn describe(resp: HttpResponse) -> String {
    s @= resp.status()
    b @= resp.body()
    return \"{s}: {b}\"
}
fn main() {
    req @= http.parse(\"GET /x HTTP/1.1\\nHost: localhost\")
    resp @= handle(req)
    print(describe(resp))
}
";
    let (code, stdout) = build_and_run("tir_http_accessors", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "200 OK: m=GET p=/x\n");
}

/// c109 Phase 21: `tasks.spawn` + `Task<T>` value + `Task.join()` — the spawn/join
/// surface (32_tasks). The spawn closure is Phase-11/13 covered; the new coverage is the
/// `Task<Int>` binding value type + the `recv_type == None` `.join()` method.
#[test]
fn task_spawn_join() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.tasks as tasks
fn sum_range(first: Int, last: Int) -> Int {
    total := 0
    loop n in first..last {
        total = (total + n)
    }
    return total
}
fn main() {
    a @= tasks.spawn(() => sum_range(1, 25))
    b @= tasks.spawn(() => sum_range(26, 50))
    print((a.join() + b.join()))
}
";
    let (code, stdout) = build_and_run("tir_task_spawn_join", src);
    assert_eq!(code, 0);
    // `loop n in first..last` is inclusive (S22/D-SG8): sum(1..=25) + sum(26..=50).
    assert_eq!(stdout, "1275\n");
}

/// c109 Phase 21: `Task.detach()` (D-DETACH1) — fire-and-forget; drops the handle.
#[test]
fn task_detach() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.tasks
fn main() {
    tasks.spawn(() => 42).detach()
    print(\"launched\")
}
";
    let (code, stdout) = build_and_run("tir_task_detach", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "launched\n");
}

/// c109 Phase 21: the full channel surface — `tasks.channel()` producer, `Channel<T>`
/// value, `Channel.sender()`, `Sender.send(v)` (inside a `take(..)` spawn closure),
/// `Task.join()`, and `Channel.receive() ?? panic(..)` (`Result<T, Closed>` unwrap).
#[test]
fn channel_send_receive() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.tasks as tasks
fn main() {
    ch: Channel<Int> @= tasks.channel()
    s1 @= ch.sender()
    t1 @= tasks.spawn(take(s1) () => {
        s1.send(30)
    })
    s2 @= ch.sender()
    t2 @= tasks.spawn(take(s2) () => {
        s2.send(12)
    })
    t1.join()
    t2.join()
    results: [Int] := []
    results.push(ch.receive() ?? panic(\"channel closed\"))
    results.push(ch.receive() ?? panic(\"channel closed\"))
    results.sort()
    loop x in results {
        print(x)
    }
}
";
    let (code, stdout) = build_and_run("tir_channel", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "12\n30\n");
}

/// c109 Phase 22: method-call-collection iteration — `loop c in s.chars()` (char
/// iteration) and `loop w in s.split(sep)` (the `.iter().cloned()` default), both
/// reproduced from `emit_for_in`'s `Expr::MethodCall` branches.
#[test]
fn method_call_collection_iteration() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn count_chars(s: String) -> Int {
    n := 0
    loop c in s.chars() {
        n+= 1
    }
    return n
}
fn join_words(s: String) -> String {
    out := \"\"
    loop w in s.split(\",\") {
        out = \"{out}[{w}]\"
    }
    return out
}
fn main() {
    print(count_chars(\"hello\"))
    print(join_words(\"a,b,c\"))
}
";
    let (code, stdout) = build_and_run("tir_method_iter", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\n[a][b][c]\n");
}

/// c109 Phase 22: the optional-binding `if` condition — `if x == value(b) { … b … }`
/// lowers to `if let Some(b) = x`, and `x == null` lowers to `.is_none()`. Reproduces
/// `emit_if`'s if-let / is_none condition shapes byte-for-byte.
#[test]
fn optional_binding_if_condition() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn describe(x: Int?) -> String {
    if x == value(n) {
        return \"got {n}\"
    }
    if x == null {
        return \"nothing\"
    }
    return \"?\"
}
fn first_even(xs: [Int]) -> Int {
    out: [Int] := []
    i := 0
    loop i < xs.len() {
        if xs.get(i) == value(v) {
            out.push(v)
        }
        i+= 1
    }
    return out.len()
}
fn main() {
    nothing: Int? := null
    print(describe(value(7)))
    print(describe(nothing))
    print(first_even([1, 2, 3]))
}
";
    let (code, stdout) = build_and_run("tir_opt_if", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "got 7\nnothing\n3\n");
}

// ===========================================================================
// c109 Phase 23: #Pure / #Todo / default params / named args / distinct / tuples
// ===========================================================================

/// c109 Phase 23: a `#Pure fn` (S60) routes through the TIR — purity is a sema-only
/// check (E3401), erased at codegen, so the fn lowers byte-identically to a plain fn.
#[test]
fn pure_fn() {
    if !have_rustc() {
        return;
    }
    let src = "\
#Pure fn double(n: Int) -> Int {
    return (n * 2)
}
#Pure fn greeting(name: String) -> String {
    return \"hi, {name}\"
}
fn main() {
    print(double(21))
    print(greeting(\"jet\"))
}
";
    let (code, stdout) = build_and_run("tir_pure", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\nhi, jet\n");
}

/// c109 Phase 23: a `#Todo` typed hole (`Expr::Todo`) emits a diverging
/// `todo!("#Todo at … — expected <ty>")`. The fn compiles + routes; the hole is never
/// reached at runtime here (only the implemented fn is called).
#[test]
fn todo_hole() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn double(n: Int) -> Int {
    return (n * 2)
}
fn not_yet(n: Int) -> Int {
    return #Todo
}
fn main() {
    print(double(21))
}
";
    let (code, stdout) = build_and_run("tir_todo", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

/// c109 Phase 23: default parameter values (S61/D-NARG-D2). Sema fills omitted trailing
/// args at the call site (substituting earlier-param refs), so the defaulted fn lowers
/// byte-identically and the call routes through the TIR.
#[test]
fn default_param_values() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn box_dims(w: Int, h: Int = w, d: Int = h) -> String {
    return \"{w}x{h}x{d}\"
}
fn main() {
    print(box_dims(4))
    print(box_dims(4, 2))
    print(box_dims(4, 2, 1))
}
";
    let (code, stdout) = build_and_run("tir_defaults", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "4x4x4\n4x2x2\n4x2x1\n");
}

/// c109 Phase 23: call-site labels (D-NARG1) on a free function. Labels are checked
/// documentation that never reorder (D-NARG-D4); codegen ignores them, so a labeled
/// call routes through the TIR identically to an unlabeled one.
#[test]
fn named_args() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn area(width: Int, height: Int) -> Int {
    return (width * height)
}
fn main() {
    print(area(width: 4, height: 3))
    print(area(4, height: 3))
}
";
    let (code, stdout) = build_and_run("tir_named_args", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "12\n12\n");
}

/// c109 Phase 23: distinct types (D-DIST1/D-DIST3). Construction `Name(x)` → newtype
/// `user_Name(x)`; `.raw()` → `(recv).0`; `#Numeric` distinct `+`/`==` use the native
/// operator. A distinct value type passes/returns/binds byte-identically.
#[test]
fn distinct_types() {
    if !have_rustc() {
        return;
    }
    let src = "\
UserId @= distinct Int;
#Numeric Meters @= distinct Float;

fn greet(id: UserId) -> String {
    return \"user {(id.raw())}\"
}
fn main() {
    uid @= UserId(42)
    print(greet(uid))
    a @= Meters(3.0)
    b @= Meters(1.5)
    c @= a + b
    print(\"{(c.raw())} m\")
    x @= UserId(7)
    y @= UserId(7)
    print(\"{(x == y)}\")
}
";
    let (code, stdout) = build_and_run("tir_distinct", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "user 42\n4.5 m\ntrue\n");
}

/// c109 Phase 23: named tuples (S73/D-SG7). A tuple literal `(x: 1, y: 2)` → a generated
/// `JetTup_<hash>` struct lit (canonical field order); field access `p.x` → `(p).user_x`;
/// destructure `(a, b) @= p.clone()` → the borrow-temp + per-field `.clone()` form;
/// equality is native. The tuple type passes/returns byte-identically.
#[test]
fn named_tuples() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn bounds() -> (max: Int, min: Int) {
    return (min: 0, max: 10)
}
fn main() {
    p @= (x: 1, y: 2)
    q @= (y: 3, x: 4)
    same_shape @= (p == q)
    (a, b) @= p.clone()
    print(\"{p.x} {p.y} {a} {b} {same_shape}\")
    pair @= bounds()
    print(\"{pair.min} {pair.max}\")
}
";
    let (code, stdout) = build_and_run("tir_tuples", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1 2 1 2 false\n0 10\n");
}

/// c109 Phase 24: JSON value type + construction + if-let matching + render/parse
/// round-trip (the coupled prelude-`JSON` slice). `main` routes through the TIR:
/// `json.parse(raw) ?? panic`, `if data == Object(entries)` (JSON if-let), `JSON.Text`/
/// `JSON.Boolean`/`JSON.Object` construction (non-mangled `jet_std::Json::…`), a Map
/// index over `[String, JSON]`, and `json.to_string`. rustc accepting proves byte-parity.
#[test]
fn json_value_construct_match_render() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.encoding.json as json
fn main() {
    raw @= \"{{\\\"name\\\":\\\"jet\\\",\\\"ok\\\":true}}\"
    data @= json.parse(raw) ?? panic(\"bad json\")
    if data == Object(entries) {
        print(entries.len())
    }
    obj: [String, JSON] := [:]
    obj[\"name\"] = JSON.Text(\"jet\")
    obj[\"ok\"] = JSON.Boolean(true)
    obj[\"none\"] = JSON.Null
    print(json.to_string(JSON.Object(obj)))
}
";
    let (code, stdout) = build_and_run("tir_json", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\n{\"name\":\"jet\",\"none\":null,\"ok\":true}\n");
}

/// c109 Phase 24: nested JSON if-let matching coercing typed payloads (`73_json_coerce`).
/// `if data == Object(entries)` then `if port == Number(n)` / `Text(s)` / `Boolean(b)`
/// — each binds a typed payload (Float/String/Bool) off `core_json_pattern_types`.
#[test]
fn json_nested_variant_matching() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.encoding.json as json
fn main() {
    raw @= \"{{\\\"port\\\":\\\"8080\\\",\\\"name\\\":\\\"api\\\"}}\"
    data @= json.decode(raw) ?? panic(\"bad json\")
    if data == Object(entries) {
        port @= entries[\"port\"]
        name @= entries[\"name\"]
        if port == Number(n) {
            print(n + 1.0)
        }
        if name == Text(s) {
            print(s)
        }
    }
}
";
    let (code, stdout) = build_and_run("tir_json_coerce", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "8081.0\napi\n");
}

/// c109 Phase 24: regex `Match` value type + `.group(n)` accessor (`74_regex`). The
/// `Match?` value (`if m == value(mat)` binds `mat: Match`) and `mat.group(0)` route.
/// Regex needs the FFI bridge crate (not linkable from a standalone `rustc`), so this
/// is a COMPILE-only check that the `Match.group` lowering emits the AST's
/// `.get((n) as usize).cloned().flatten()` form (byte-parity is proven by the
/// whole-suite diff on `74_regex` + the `74_regex` golden run).
#[test]
fn regex_match_group() {
    let src = "\
use jet.regex as re
fn main() {
    text @= \"order 42 shipped\"
    m @= re.match(\"(\\\\d+) shipped\", text) ?? panic(\"bad pattern\")
    if m == value(mat) {
        whole @= mat.group(0) ?? \"none\"
        print(whole)
    }
}
";
    let dir = std::env::temp_dir().join(format!("jet_tir_regex_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("regex.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown).expect("front end rejected regex fixture");
    // The `if let Some(user_mat)` if-let binds the `Match`; `.group(0)` indexes it.
    assert!(
        out.rust.contains("if let Some(user_mat) ="),
        "Match value not bound via if-let:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("(user_mat).get((0i64) as usize).cloned().flatten()"),
        "Match.group lowering not byte-exact:\n{}",
        out.rust
    );
}

/// c109 Phase 24: a comptime const inlined at the use site (`{HEADER}` in interpolation).
/// `wrap` routes — the const inlines its pre-rendered value (`cx.consts`).
#[test]
fn comptime_const_inline() {
    if !have_rustc() {
        return;
    }
    let src = "\
comptime VERSION = \"1.0\"
comptime BANNER = \"logbook {VERSION}\"
fn wrap(s: String) -> String {
    return \"{BANNER}: {s}\"
}
fn main() {
    print(wrap(\"hi\"))
}
";
    let (code, stdout) = build_and_run("tir_const", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "logbook 1.0: hi\n");
}

/// c109 Phase 24: foreign-enum matching + a local enum with a foreign-enum payload, plus
/// a foreign struct with enum/optional fields — the logbook `note`/`search` shapes. The
/// `note` module defines `NoteType`/`Note`; `search` (entry) matches over the foreign
/// `NoteType` directly AND over a local `Query` whose `Kind` payload is the foreign enum,
/// constructs `NoteType.User` cross-module, and reads a foreign struct's enum field.
#[test]
fn foreign_enum_matching_and_payload() {
    if !have_rustc() {
        return;
    }
    // The foreign struct `Note` is CONSTRUCTED in its own module (`make_note`, matching the
    // real logbook — an unqualified cross-module `Note {…}` literal is a separate pre-existing
    // AST-path bug, omitted of `import_ns`, that the gate already excludes). `kind_str` matches
    // the foreign-LOCAL `NoteType`; the entry matches the foreign `NoteType` via a local
    // `Query`'s `Kind(NoteType)` payload + constructs `NoteType.User` cross-module.
    let note = "\
pub enum NoteType { User Feedback Project Reference }
pub struct Note {
    pub name: String
    pub note_type: NoteType
    pub parent: String?
}
pub fn make_note(name: ^String, t: ^NoteType) -> Note {
    return Note {name: name, note_type: t, parent: null}
}
pub fn kind_str(n: Note) -> String {
    k @= n.note_type
    if k == {
        User -> { return \"user\" }
        Feedback -> { return \"feedback\" }
        Project -> { return \"project\" }
        Reference -> { return \"reference\" }
    }
}
fn main() { print(\"note\") }
";
    let entry = "\
use \"note\"
enum Query {
    Tag(String)
    Kind(NoteType)
}
fn classify(raw: String) -> Query {
    if raw == \"user\" {
        return Query.Kind(NoteType.User)
    }
    return Query.Tag(raw)
}
fn describe(n: Note, q: Query) -> String {
    if q == {
        Tag(t) -> { return \"tag:{t}\" }
        Kind(k) -> { return \"kind:{note.kind_str(n)}\" }
    }
}
fn main() {
    n @= note.make_note(\"x\", NoteType.User)
    q @= classify(\"user\")
    print(describe(n, q))
    q2 @= classify(\"design\")
    print(describe(n, q2))
}
";
    let (code, stdout) = build_and_run_multi(
        "tir_foreign_enum",
        "main.jet",
        &[("main.jet", entry), ("note.jet", note)],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "kind:user\ntag:design\n");
}

/// c109 Phase 25: a STATIC constructor `Type.new(args)` (D-NARG1, 63_named_args). `new`
/// is in `is_intercepted_method_name` (the instance-method intercept stays), but the
/// STATIC shape (`recv_type == None`, type-name receiver, `(Type, "new") ∈ method_sigs`)
/// is the Phase-7 `user_<Type>::user_new(args)` form — not a builtin intercept — so it
/// now routes. The instance method named `area` still routes too.
#[test]
fn static_new_constructor() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Rect {
    width: Int
    height: Int
}
impl Rect {
    fn new(width: Int, height: Int) -> Rect {
        return Rect{width: width, height: height}
    }
    fn area(self) -> Int {
        return (self.width * self.height)
    }
}
fn main() {
    r @= Rect.new(4, 3)
    print(r.area())
}
";
    let (code, stdout) = build_and_run("tir_static_new", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "12\n");
}

/// c109 Phase 25: the ambient prelude `input(...)` (D-PRELUDE1 = B, 65_io_prelude). A bare
/// `input()` with NO user `input` fn lowers to `jet_std_io_input(None)` → `Result<String,
/// IOError>`, composing with the `??` fallback. No stdin is provided, so `input()` errs and
/// the fallback value is used (deterministic).
#[test]
fn ambient_input() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn greet() -> String {
    name @= input() ?? \"world\"
    return \"hello, {name}\"
}
fn main() {
    print(greet())
}
";
    // No stdin is piped, so `input()` reads EOF and yields Ok("") — the `??` fallback is
    // NOT taken (it fires only on Err), so `name` is the empty string. (The point of the
    // test is that the ambient `input()` lowers + runs through the TIR, not the fallback.)
    let (code, stdout) = build_and_run("tir_ambient_input", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "hello, \n");
}

/// c109 Phase 25: the HttpRouter handle surface (D-ROUTE1=A, 76_http_routes). `http.router()`
/// (producer), `router.get(path, handler)` with a named-fn handler (the boxed-closure
/// `emit_router_handler` reproduction), and `http.dispatch(router, req)` — all without
/// networking (dispatch a directly-parsed request). The handler routes too.
#[test]
fn http_router_dispatch() {
    if !have_rustc() {
        return;
    }
    let src = "\
use jet.http as http
fn handle_root(req: HttpRequest) -> HttpResponse {
    return HttpResponse {status: \"200 OK\", body: \"welcome\", headers: [:]}
}
fn handle_user(req: HttpRequest) -> HttpResponse {
    id @= req.param(\"id\") ?? \"unknown\"
    return HttpResponse {status: \"200 OK\", body: \"user={id}\", headers: [:]}
}
fn main() {
    router @= http.router()
    router.get(\"/\", handle_root)
    router.get(\"/users/:id\", handle_user)
    req @= http.parse(\"GET / HTTP/1.1\\nHost: localhost\")
    resp @= http.dispatch(router, req)
    print(resp.body())
}
";
    let (code, stdout) = build_and_run("tir_http_router", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "welcome\n");
}

/// c109 Phase 26: the `require(cond[, msg])` / `require_eq` rich-report builtins (S36,
/// 14_panic). A satisfied `require` is a no-op; the program continues. (The failing
/// branch's rich panic is exercised by the AST-path golden suite; here we prove the TIR
/// renders + runs the guard byte-for-byte.)
#[test]
fn require_builtins() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn main() {
    require(((1 + 1) == 2))
    require(true, \"unreachable\")
    require_eq(6, (2 * 3))
    print(\"ok\")
}
";
    let (code, stdout) = build_and_run("tir_require", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "ok\n");
}

/// c109 Phase 26: a `#Caps(Io) { … }` effect-restriction region (D-EFF1, effect_caps)
/// erases to a plain block in codegen; the body runs unchanged.
#[test]
fn caps_block() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn announce(label: String, n: Int) #(Io) {
    print(\"{label}: {n}\")
}
fn main() {
    #Caps(Io) {
        announce(\"answer\", 42)
    }
}
";
    let (code, stdout) = build_and_run("tir_caps", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "answer: 42\n");
}

/// c109 Phase 26: the three free-call argument conventions (08_ownership) — `mut place`
/// (`&mut (…)`), `take value` (move), and a plain shared `Read` borrow.
#[test]
fn free_call_arg_conventions() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn show(msg: String) {
    print(msg)
}
fn bump(n: ~Int) {
    n += 1
}
fn archive(name: ^String) -> String {
    return name
}
fn main() {
    score: Int := 41
    bump(~score)
    print(score)
    greeting: String @= \"hello\"
    show(greeting)
    saved: String @= archive(^\"vault\")
    print(saved)
}
";
    let (code, stdout) = build_and_run("tir_arg_conv", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\nhello\nvault\n");
}

/// c109 Phase 26: a fan-out result-list DESTRUCTURE `[a, b, c] @= <init>` (S74, 41_fan_out).
/// Binds each element via the runtime bounds-checked `jet_unpack_vec`.
#[test]
fn list_destructure() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn double(n: Int) -> Int {
    return (n * 2)
}
fn main() {
    doubled @= double.[1, 2, 3]
    [a, b, c] @= doubled
    print(a)
    print(b)
    print(c)
}
";
    let (code, stdout) = build_and_run("tir_list_destructure", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\n4\n6\n");
}

/// c109 Phase 27: a fn-typed VALUE stored in a local + a struct fn-FIELD method
/// (24_callbacks). `double_fn @= double` binds a bare fn-name as a value; `apply_twice`
/// takes it (and a lambda) as a Fn arg; `Worker { step: … }` constructs a struct with a
/// fn-typed field; `w.step(4)` calls THROUGH that field. All route through the TIR.
#[test]
fn fn_value_and_struct_fn_field() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn apply_twice(f: fn(Int) -> Int, x: Int) -> Int {
    return f(f(x))
}
fn double(x: Int) -> Int {
    return (x * 2)
}
struct Worker {
    step: fn(Int) -> Int
}
fn main() {
    double_fn @= double
    print(apply_twice(double_fn, 3))
    print(apply_twice((x: Int) => (x + 1), 5))
    w @= Worker {step: (n: Int) => (n * n)}
    print(w.step(4))
}
";
    let (code, stdout) = build_and_run("tir_fn_value_struct_field", src);
    assert_eq!(code, 0);
    // double(double(3)) = 12; apply_twice(x+1, 5) = ((5+1)+1) = 7; w.step(4) = 4*4 = 16.
    assert_eq!(stdout, "12\n7\n16\n");
}

/// c109 Phase 28: the full sized-integer surface (82_sized_integers, D-SG9/S42/D-NUMOPS1).
/// Literal width-elaboration (`U8`/`I32`/`I8`/`I64`), per-element list widening (`[U8]`),
/// width-preserving overflow-trapping arithmetic, width conversions (`to_i64`/`to_u8() ??`),
/// per-type bounds constants (`U8.MAX`/`I32.MIN`/`Float.INFINITY`), bit/float queries
/// (`count_ones`/`is_infinite`), and the overflow opt-outs (`wrapping`/`saturating`/
/// `checked`). The whole `main` routes through the TIR.
#[test]
fn sized_integers() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn main() {
    red: U8 @= 255
    channel: I32 @= 100000
    depth: I8 @= -120
    print(red)
    print(channel)
    print(depth)
    total: I64 @= 9000000000
    print(total + 1)
    half: U8 @= 100
    print(half + half)
    bytes: [U8] @= [104, 105, 33]
    print(bytes)
    wide: I64 @= red.to_i64()
    print(wide)
    clamped: U8 @= channel.to_u8() ?? 255
    print(clamped)
    print(U8.MAX)
    print(I32.MIN)
    flags: U8 @= 13
    print(flags.count_ones())
    print(Float.INFINITY.is_infinite())
    hi: U8 @= 200
    lo: U8 @= 100
    print(wrapping(hi + lo))
    print(saturating(hi + lo))
    fallback: U8 @= 0
    print(checked(hi + lo) ?? fallback)
}
";
    let (code, stdout) = build_and_run("tir_sized_integers", src);
    assert_eq!(code, 0);
    // 255; 100000; -120; total+1=9000000001; half+half=200; [104,105,33]; red.to_i64()=255;
    // channel.to_u8()=None ?? 255 = 255; U8.MAX=255; I32.MIN=-2147483648; 13.count_ones()=3;
    // INFINITY.is_infinite()=true; wrapping 200+100=44; saturating=255; checked=None ?? 0 = 0.
    assert_eq!(
        stdout,
        "255\n100000\n-120\n9000000001\n200\n[104, 105, 33]\n255\n255\n255\n-2147483648\n3\ntrue\n44\n255\n0\n"
    );
}

/// Build `src` to a binary, then run it with `stdin` piped in. Like `build_and_run`
/// but feeds a deterministic stdin so an `io.input(...)` reads known lines (and EOF).
fn build_and_run_stdin(name: &str, src: &str, stdin: &str) -> (i32, String) {
    use std::io::Write;
    use std::process::Stdio;
    let dir = std::env::temp_dir().join(format!("jet_tir_test_{}", std::process::id()));
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
        "rustc rejected generated code (I2 violation):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let mut child = Command::new(&bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let run = child.wait_with_output().unwrap();
    (run.status.code().unwrap_or(0), String::from_utf8_lossy(&run.stdout).into_owned())
}

/// c109 Phase 29: qualified `io.input(prompt)` (surface (H), 34_parallel_scan
/// `paths_from_prompt`). DISTINCT from the ambient bare `input()` (Phase 25 `AmbientInput`):
/// this is a `MethodCall` on a `core.io` alias, lowered to a `CoreCall`
/// (`jet_std_io_input(Some(&(prompt)))` → `Result<String, IOError>`). It composes with a
/// `?? return <value>` fallback (the early-return form, already covered since Phase 8).
/// The loop accumulates piped lines; a blank line breaks; EOF yields `Ok("")` (read_line on
/// EOF) so the loop also breaks — the `?? return` fires only on a genuine Err. Both stdin
/// shapes run deterministically.
#[test]
fn qualified_io_input_or_return() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.io as io
fn collect() -> [String] {
    out: [String] := []
    loop true {
        line @= io.input(\"> \") ?? return out.clone()
        if line == \"\" {
            break
        }
        out.push(line)
    }
    return out
}
fn main() {
    got @= collect()
    print(\"count={got.len()}\")
    loop g in got {
        print(g)
    }
    print(\"done\")
}
";
    // Two lines then a blank line: the loop accumulates `alpha`/`beta`, the blank breaks.
    let (code, stdout) = build_and_run_stdin("tir_io_input_lines", src, "alpha\nbeta\n\n");
    assert_eq!(code, 0);
    assert_eq!(stdout, "> > > count=2\nalpha\nbeta\ndone\n");
    // Immediate EOF: read_line yields Ok("") → the loop breaks on the empty line, no input.
    let (code, stdout) = build_and_run_stdin("tir_io_input_eof", src, "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "> count=0\ndone\n");
}

/// c109 Phase 30: GENERIC functions + TRAIT-OBJECT dispatch (surface (G), 25_traits).
/// Three covered fns: a generic `largest<T: Comparable>(xs: [T]) -> (T?)` (a `>` on a
/// `Comparable`-bound type var, `[T]` indexing, a `T?` return with `value`/`null`); a
/// trait-OBJECT param `print_area(s: Shape)` (dynamic dispatch `s.name()`/`s.area()`
/// through a `Box<dyn user_Shape>`); and `main` — a `[Shape]` trait-object list built from
/// `Box::new(<lit>) as Box<dyn user_Shape>` element coercions, iterated via `.each`
/// (`jet_list_each_ref`), plus a generic call `largest(nums)` and a derived-Comparable
/// `scores.sort_by(...)`. All route `ROUTE TIR` (the Circle/Square trait methods already
/// route since Phase 12), and the whole suite is byte-identical (golden parity).
#[test]
fn generic_fns_and_trait_object_dispatch() {
    if !have_rustc() {
        return;
    }
    let src = "\
trait Shape {
    fn area(self) -> Float
    fn name(self) -> String
}
struct Circle {
    radius: Float

    impl Shape {
        fn area(self) -> Float {
            return ((3.14159 * self.radius) * self.radius)
        }
        fn name(self) -> String {
            return \"circle\"
        }
    }
}
struct Square {
    side: Float
}
impl Square: Shape {
    fn area(self) -> Float {
        return (self.side * self.side)
    }
    fn name(self) -> String {
        return \"square\"
    }
}
fn largest<T: Comparable>(xs: [T]) -> (T?) {
    if xs.len() == 0 {
        return null
    }
    best := xs[0]
    i := 1
    loop i < xs.len() {
        if xs[i] > best {
            best = xs[i]
        }
        i+= 1
    }
    return value(best)
}
fn print_area(s: Shape) {
    print(\"{s.name()}: {s.area()}\")
}
struct Score {
    points: Int
    derive Comparable
}
fn main() {
    shapes: [Shape] @= [Circle {radius: 1.0}, Square {side: 2.0}]
    shapes.each((s) => {
        print_area(s)
    })
    nums @= [3, 1, 4, 1, 5]
    print(largest(nums))
    scores := [Score {points: 10}, Score {points: 20}]
    scores.sort_by((s: Score) => s.points)
    print(scores[0].points)
}
";
    let (code, stdout) = build_and_run("tir_generic_trait_object", src);
    assert_eq!(code, 0);
    // circle/square areas via dynamic dispatch; largest([3,1,4,1,5]) = 5; scores[0].points = 10.
    assert_eq!(stdout, "circle: 3.14159\nsquare: 4.0\n5\n10\n");
}

/// c109 (view-trait fix): a `view`-returning TRAIT method `fn label(self) -> &String`
/// implemented in an `impl Dog: Named` block. This was a latent I2 hole on BOTH paths:
/// `emit_trait_def` rendered the trait DECLARATION's return as `-> String` (ignoring
/// `is_view_return`) while the impl emitted `-> &String`, so rustc rejected the generated
/// Rust with E0053 ("incompatible type for trait"). The fix threads `is_view_return` into
/// the declared return type so the trait says `-> &String` to match the impl. The method
/// now compiles AND routes through the TIR (the gate's view-trait exclusion is dropped; the
/// borrow shape is the existing total `TStmt::ViewReturn { wrap }` from Phase 17).
#[test]
fn view_returning_trait_method() {
    if !have_rustc() {
        return;
    }
    let src = "\
trait Named {
    fn label(self) -> &String
}
struct Dog {
    name: String
}
impl Dog: Named {
    fn label(self) -> &String {
        return self.name
    }
}
fn main() {
    d @= Dog { name: \"Rex\" }
    print(d.label())
}
";
    let (code, stdout) = build_and_run("tir_view_trait_method", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "Rex\n");
}

/// D-MUTSELF1: a `mut self` method that assigns a field in place — `self.field = v`
/// and the compound `self.field += v` (S17) — lowers to `((*self)).field = …` on the
/// `&mut Self` receiver. rustc accepts it (I2); the receiver mutates as written.
#[test]
fn mut_self_field_assign() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Counter {
    n: Int
    fn bump(~self) {
        self.n = self.n + 1
    }
    fn add(~self, k: Int) {
        self.n += k
    }
}
fn main() {
    c: Counter := Counter { n: 0 }
    c.bump()
    c.add(10)
    print(c.n)
}
";
    let (code, stdout) = build_and_run("tir_mut_self_field", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "11\n");
}

/// D-MUTSELF1: whole-`self` reassignment — `self = New{…}` — lowers to `(*self) = …`
/// (the prior AST-path I2 hole, where the `mut self` slot wasn't dereferenced on the
/// LHS, is now closed). rustc accepts the dereferenced assignment.
#[test]
fn mut_self_whole_reassignment() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Counter {
    n: Int
    fn reset(~self) {
        self = Counter { n: 0 }
    }
}
fn main() {
    c: Counter := Counter { n: 9 }
    c.reset()
    print(c.n)
}
";
    let (code, stdout) = build_and_run("tir_mut_self_whole", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "0\n");
}

/// D-MUTSELF1: self-mutation through a TRAIT-impl `mut self` method. The trait
/// declaration and impl both render `&mut self` (was hardcoded `&self`), so the
/// in-place field write compiles. Exercises the trait emit + self-slot deref.
#[test]
fn mut_self_trait_method_field_assign() {
    if !have_rustc() {
        return;
    }
    let src = "\
trait Bumpable {
    fn bump(~self)
}
struct Counter {
    n: Int
}
impl Counter: Bumpable {
    fn bump(~self) {
        self.n = self.n + 1
    }
}
fn main() {
    c: Counter := Counter { n: 0 }
    c.bump()
    c.bump()
    print(c.n)
}
";
    let (code, stdout) = build_and_run("tir_mut_self_trait", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\n");
}

/// c109 (recursive struct): a self-referential struct field has Rust type
/// `Box<…>` (`cx.boxed_edges`), so its construction value must be wrapped
/// `Box::new(…)` (E0308 otherwise — the AST `emit_struct_lit` was not wrapping).
/// A nested inline `Tree { value, child: value(Tree { … }) }` exercises the boxed
/// wrap at multiple levels; the boxed field READ stays on the AST path (deref), so
/// `main` reads only the non-boxed scalar `value`. Both construction levels and
/// `main` route through the TIR.
#[test]
fn recursive_struct_construction() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Tree {
    value: Int
    child: Tree?
}
fn main() {
    root @= Tree {
        value: 3,
        child: value(Tree {
            value: 2,
            child: value(Tree { value: 1, child: null })
        })
    }
    print(root.value)
}
";
    let (code, stdout) = build_and_run("tir_recursive_struct", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

/// c109 (foreign struct literal): an UNqualified cross-module foreign struct literal
/// (`Note { text: "hi" }` written in an importing module, no `note.` namespace) must
/// prefix the foreign module (`user_notes::user_Note`) or rustc can't find the type
/// (E0422). The AST `emit_struct_lit` plain branch only prefixed via `user_type_apply_rust`
/// once `cx.foreign_types` is consulted (the fix); the TIR reproduces the prefixed head.
/// `main` constructs + reads the foreign struct and routes through the TIR.
#[test]
fn unqualified_foreign_struct_literal() {
    if !have_rustc() {
        return;
    }
    let main_src = "\
use \"notes\"
fn main() {
    n @= Note { text: \"hi\" }
    print(n.text)
}
";
    let notes_src = "\
pub struct Note {
    pub text: String
}
";
    let (code, stdout) = build_and_run_multi(
        "tir_foreign_struct_lit",
        "main.jet",
        &[("main.jet", main_src), ("notes.jet", notes_src)],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "hi\n");
}

/// c109 (builtin-name collision): a user method whose name collides with a builtin
/// (`get`/`len`) was mis-dispatched by `emit_builtin_method` (name-keyed, not
/// receiver-typed) → `b.get()` emitted garbage, `b.len()` → E0599. The fix dispatches
/// to the USER method (`user_<method>`) when `recv_type == Some(T)` and `(T, method) ∈
/// cx.method_sigs`. `main` and both methods route through the TIR.
#[test]
fn user_method_shadowing_builtin_name() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Bag {
    items: [Int]

    fn get(self) -> Int {
        return 42
    }
    fn len(self) -> Int {
        return 7
    }
}
fn main() {
    b @= Bag { items: [1, 2, 3] }
    print(b.get())
    print(b.len())
}
";
    let (code, stdout) = build_and_run("tir_builtin_collision", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n7\n");
}

/// c109 (`is_empty` Bool fix): `Collections::*_method_return` typed `is_empty` as
/// `Int`, so `e := xs.is_empty()` emitted `let e: i64 = (…).is_empty()` (bool ≠ i64
/// → rustc E0308) and `if xs.is_empty()` was E0110 at sema. The fix returns `Bool`;
/// `is_empty` is now covered (`TBuiltinOp::IsEmpty`) on list/map/string receivers.
#[test]
fn is_empty_returns_bool() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn check(xs: [Int]) {
    if xs.is_empty() {
        print(\"empty\")
    } else {
        print(\"not empty\")
    }
}
fn main() {
    e @= [1, 2, 3].is_empty()
    print(e)
    m @= [1: 2]
    print(m.is_empty())
    s @= \"hi\"
    print(s.is_empty())
    empty: [Int] @= []
    check(empty)
    check([9])
}
";
    let (code, stdout) = build_and_run("tir_is_empty", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "false\nfalse\nfalse\nempty\nnot empty\n");
}

/// c109 (bare `?? return` fix): `infer_or_fallback`'s logic was inverted — a bare
/// `?? return` (no value) was sema-rejected in a UNIT fn (E0405) and accepted in a
/// NON-unit fn (where rustc then rejected the emitted `return;` → E0069). The fix
/// accepts a bare `?? return` ONLY in a unit fn (`return;` is valid) and rejects it
/// in a value-returning fn. The unit-fn form routes through the TIR
/// (`orfallback_rhs_in_subset → Return(None)`, emitting `None => return`).
#[test]
fn bare_or_return_in_unit_fn() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn f(xs: [Int]) {
    x := xs.first() ?? return
    print(x)
}
fn main() {
    f([10, 20])
    empty: [Int] @= []
    f(empty)
    print(99)
}
";
    let (code, stdout) = build_and_run("tir_bare_or_return", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "10\n99\n");
}

/// c109 (boxed recursive field read): reading a self-referential struct field
/// (`t.child`, Rust type `Box<…>` via `cx.boxed_edges`) miscompiled — the read
/// yielded a `Box<…>` where the unboxed type was wanted (rustc E0308). The fix
/// derefs the `Box` (`(*(…))`) on a boxed-field read. With the read fixed, a
/// recursive struct is now a covered VALUE type, so a fn that builds AND traverses
/// a `Tree` (binds a boxed child, matches, recurses) routes through the TIR.
#[test]
fn boxed_recursive_struct_field_read() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Tree {
    value: Int
    child: Tree?
}
fn sum(t: Tree) -> Int {
    total := t.value
    kid: Tree? @= t.child
    if kid == {
        value(c) -> {
            total = total + sum(c)
        }
        null -> {}
    }
    return total
}
fn main() {
    root @= Tree {
        value: 3,
        child: value(Tree {
            value: 2,
            child: value(Tree { value: 1, child: null })
        })
    }
    print(sum(root))
}
";
    let (code, stdout) = build_and_run("tir_boxed_field_read", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "6\n");
}

/// c109 (borrowed struct-lit value clone): a struct literal whose field value is a
/// bare borrowed-in-env non-Copy ident (`Person { name: n }` where `n: String` is a
/// `read` param → `&String`) emitted `user_name: (*user_n)` → rustc E0507 ("cannot
/// move out of `*user_n`"). `field_read_to_clone` clones owning field READS but not a
/// bare borrowed ident used as a struct-lit value; the fix clones it in sema's
/// elaboration. `make` (struct lit + the sema-inserted clone) routes through the TIR.
#[test]
fn borrowed_struct_lit_field_value_cloned() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Person {
    name: String
}
fn make(n: String) -> Person {
    return Person { name: n }
}
fn main() {
    p @= make(\"Ada\")
    print(p.name)
}
";
    let (code, stdout) = build_and_run("tir_borrowed_struct_lit", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "Ada\n");
}

/// c109 (B3): a struct-destructuring binding `Type { x, y } @= p` routes through
/// the TIR and prints the field sum (byte-for-byte the AST `BindPattern::Struct`).
#[test]
fn struct_destructure_binding() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Point { x: Int, y: Int }
fn main() {
    p @= Point { x: 1, y: 2 }
    Point { x, y } @= p
    print(x + y)
}
";
    let (code, stdout) = build_and_run("tir_struct_destructure", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

/// c109 (B4): a user-enum variant if-let condition `if m == Ping(n) { } else { }`
/// routes through the TIR and binds the payload (byte-for-byte the AST if-let).
#[test]
fn user_enum_variant_if_let_condition() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Msg { Ping(Int) Pong }
fn f(m: Msg) -> Int {
    if m == Ping(n) {
        return n
    } else {
        return -1
    }
}
fn main() {
    print(f(Msg.Ping(7)))
    print(f(Msg.Pong))
}
";
    let (code, stdout) = build_and_run("tir_user_enum_if_let", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "7\n-1\n");
}

/// c109 (B2): a fixed-size-list type `[E#N]` as a param (fed a fan-out result) and
/// as a struct field routes through the TIR (rendered `Vec<E>`, byte-for-byte the AST).
#[test]
fn fixed_size_list_param_and_field() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn double(n: Int) -> Int {
    return (n * 2)
}
struct Grid { row: [Int#3] }
fn firstof(xs: [Int#3]) -> Int {
    return xs[0]
}
fn main() {
    print(firstof(double.[1, 2, 3]))
    g @= Grid { row: double.[1, 2, 3] }
    print(g.row[1])
}
";
    let (code, stdout) = build_and_run("tir_fixed_list", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\n4\n");
}

/// c109 (B1): a mixed-switch over a NON-IDENT subject (a field access) with a
/// payload-binding arm head. Previously the AST path emitted `matches!(…, Some(c))`
/// then used the unbound `c` (E0425); now it routes (both paths) through the Rust
/// `match` that binds the payload. The subject is evaluated once.
#[test]
fn mixed_switch_non_ident_subject_binds_payload() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Holder { val: Int? }
fn f(h: Holder) -> Int {
    if h.val == {
        value(c) -> { return c }
        else -> { return 0 }
    }
}
fn main() {
    hold @= Holder { val: value(5) }
    print(f(hold))
    empty @= Holder { val: null }
    print(f(empty))
}
";
    let (code, stdout) = build_and_run("tir_mixed_nonident_payload", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\n0\n");
}

/// c109 (B1): a mixed-switch over a NON-IDENT subject (a call) with unit-variant arm
/// heads. Previously the AST emitted a bare unqualified `(subj == (user_Red))` and
/// re-evaluated the call per arm (E0425); now it routes through the Rust `match` over
/// the qualified variants, subject evaluated once.
#[test]
fn mixed_switch_non_ident_subject_qualifies_variants() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Light { Red Green Yellow }
fn pick() -> Light {
    return Light.Red
}
fn classify() -> Int {
    if pick() == {
        Red -> { return 1 }
        Green -> { return 2 }
        else -> { return 0 }
    }
}
fn main() {
    print(classify())
}
";
    let (code, stdout) = build_and_run("tir_mixed_nonident_variant", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

/// c109 (S57/M9.5): a comptime LOCAL `comptime NAME = expr` in a function body. Sema
/// evaluates `build()` at compile time and codegen emits the result as literal data
/// (`let user_xs: Vec<i64> = vec![10i64, 20i64, 30i64];`). The TIR reproduces that
/// serialized literal verbatim; the runtime `init` expr is never emitted. Mirrors
/// `tests/comptime_diff.rs::local_comptime_is_literal_data`.
#[test]
fn comptime_local_is_literal_data() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn build() -> List<Int> {
    xs: List<Int> := []
    loop i in 1..3 {
        xs.push(i * 10)
    }
    return xs
}
fn main() {
    comptime xs = build()
    print(\"{xs}\")
    print(\"{xs[1]}\")
}
";
    let (code, stdout) = build_and_run("tir_comptime_local", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "[10, 20, 30]\n20\n");
}

/// c109 Phase 6b: a `Shared<T>` value passed to a FREE (non-method) call inside a loop
/// auto-clones the Arc — `emit_call_args` emits `Arc::clone(&…)` and the receiving
/// `Shared<T>` `Read` param borrows it (`&(…)`). The gate previously excluded
/// `shared_auto_clone` on plain `Call` args, routing `loop_user`/`noop` through the AST
/// path; both now route through the TIR with a byte-identical emit. A `Shared<T>` value
/// has no surface constructor (it only ever arrives as a param), so this is a compile +
/// byte-exact-Rust assertion (the same surface `tests/ownership.rs` and `tests/ui_lint`
/// exercise) rather than a build+run. rustc accepting the output proves I2.
#[test]
fn shared_auto_clone_in_free_call_arg() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn noop(h: Shared<Int>) {
    print(0)
}
fn loop_user(h: Shared<Int>) {
    loop {
        noop(h)
    }
}
fn main() {
    print(0)
}
";
    let dir = std::env::temp_dir().join(format!("jet_tir_shared_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join("shared.jet");
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    // Byte-exact auto-clone emit: the free-call arg auto-clones the Arc, then the
    // `Read` non-scalar `Shared<Int>` param borrows it.
    assert!(
        out.rust.contains("user_noop(&(std::sync::Arc::clone(&(*user_h))));"),
        "shared auto-clone free-call arg not byte-exact:\n{}",
        out.rust
    );
    // The receiving param signature is the shared `rust_param_type` form.
    assert!(
        out.rust.contains("pub fn user_noop(user_h: &std::sync::Arc<i64>)"),
        "Shared<Int> param signature not byte-exact:\n{}",
        out.rust
    );
    // I2: rustc accepts the generated Rust.
    let rs = dir.join("shared.rs");
    let bin = dir.join("shared");
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
}

/// c109: an owning field read of a NON-SCALAR field (`s @= p.name`, `name:
/// String`). Sema rewrites the read in owning position to `(p.name).clone()`;
/// the TIR emits `((user_p).user_name).clone()`. The single-uppercase-letter
/// struct name `P` is a concrete declared type (not a type var), so `main`
/// routes through the TIR. Runs (the two clones print independently) and is
/// byte-exact on the owning-clone emit.
#[test]
fn owning_nonscalar_field_read_clones() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct P {
    name: String
}

fn main() {
    p @= P { name: \"x\" }
    s @= p.name
    t @= p.name
    print(s)
    print(t)
}
";
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust.contains("let user_s: String = ((user_p).user_name).clone();"),
        "owning non-scalar field-read clone not byte-exact:\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_owning_field_clone", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "x\nx\n");
}

/// c109: an indexed map-assign whose index BASE is a struct field read
/// (`s.scores["a"] = 1`, `scores: Map<String, Int>`). The `LValue::Index` gate
/// admits a field-read base + the sema-resolved `IndexKind::Map`; `main` routes
/// through the TIR and the assign emits the `jet_map_insert` helper form
/// byte-for-byte. Runs (insert then index-read prints the value).
#[test]
fn indexed_map_assign_through_field() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct S {
    scores: Map<String, Int>
}

fn main() {
    s := S { scores: [:] }
    s.scores[\"a\"] = 1
    print(s.scores[\"a\"])
}
";
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust.contains("jet_map_insert(&mut ((user_s).user_scores),"),
        "map-assign through field not byte-exact:\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_map_assign_field", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

/// c109: a map builtin (`.len()`) on a struct-FIELD-read receiver
/// (`s.scores.len()`), where the field came from an empty-map struct-literal
/// field (`scores: [:]` takes its type from the struct field). The builtin gate
/// admits a field-read receiver; `main` routes through the TIR and emits
/// `((user_s).user_scores).len() as i64` byte-for-byte. Runs (empty map → 0).
#[test]
fn map_builtin_on_struct_field_receiver() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct S {
    scores: Map<String, Int>
}

fn main() {
    s := S { scores: [:] }
    print(s.scores.len())
}
";
    let out = jet::compile(src).expect("empty map literal in field position should typecheck");
    assert!(
        out.rust.contains("((user_s).user_scores).len() as i64"),
        "map builtin on field receiver not byte-exact:\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_map_builtin_field", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "0\n");
}

/// c109: a field read off a comptime-const STRUCT value (`comptime P = Pair{…}`;
/// `P.left`) and an `==` against a comptime-const ENUM value (`comptime L =
/// Light.Green`; `L == Light.Green`). Each const inlines to its pre-rendered
/// Rust value; the field read / comparison is byte-identical to the AST path.
/// `main` routes through the TIR; runs to the round-trip output.
#[test]
fn field_read_and_eq_on_inlined_comptime_values() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Pair {
    left: Int
    right: String
}

enum Light {
    Red
    Green
}

comptime P = Pair {left: 7, right: \"seven\"}
comptime L = Light.Green

fn main() {
    p @= Pair {left: 7, right: \"seven\"}
    l @= Light.Green
    print(\"{P.left}\")
    print(\"{p.left}\")
    print(\"{P.right}\")
    print(\"{p.right}\")
    print(\"{L == Light.Green}\")
    print(\"{l == Light.Green}\")
}
";
    let out = jet::compile(src).expect("should compile");
    // Byte-exact: `P.left` reads a field off the inlined struct literal.
    assert!(
        out.rust.contains("(user_Pair { user_left: 7i64, user_right: \"seven\".to_string() }).user_left"),
        "comptime struct field read not byte-exact:\n{}",
        out.rust
    );
    // Byte-exact: `L == Light.Green` compares the inlined enum value.
    assert!(
        out.rust.contains("(user_Light::user_Green) == (user_Light::user_Green)"),
        "comptime enum `==` not byte-exact:\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_comptime_struct_enum_values", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "7\n7\nseven\nseven\ntrue\ntrue\n");
}

/// c109 (D-PATW): a user-enum variant if-let condition with a WILDCARD payload
/// slot (`if w == Some(_)`). The `_` binds nothing; the if-let head renders
/// `if let user_Wrapper::user_Some(_) = user_w` byte-for-byte. `main` routes
/// through the TIR; runs (the `Some(42)` value matches the wildcard).
#[test]
fn wildcard_enum_payload_if_let() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Wrapper {
    Some(Int)
    None
}
fn main() {
    w @= Wrapper.Some(42)
    if w == Some(_) {
        print(\"has value\")
    }
}
";
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust.contains("if let user_Wrapper::user_Some(_) = user_w"),
        "wildcard enum-payload if-let not byte-exact:\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_wildcard_payload_iflet", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "has value\n");
}

/// c97/D-STRPARSE1: `String.lines()` (→ `[String]`) and `String.to_int()` (→
/// `Int ? ParseError`). Both are built-in String methods, so `main` routes
/// through the TIR — proven by the emitted `jet_string_lines` helper call and
/// the `to_int` parse form. `to_int` composes with `??`: a good parse yields the
/// value, a bad one (`"abc"`) takes the fallback.
#[test]
fn string_lines_and_to_int() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn main() {
    n @= \"42\".to_int() ?? -1
    print((n + 1))
    bad @= \"abc\".to_int() ?? -1
    print(bad)
    lines @= \"a\\nb\\nc\".lines()
    print(lines.len())
    loop line in lines {
        print(line)
    }
    total := 0
    loop row in \"10\\n20\\n30\".lines() {
        total += (row.to_int() ?? 0)
    }
    print(total)
}
";
    let out = jet::compile(src).expect("should compile");
    // TIR routing: `lines()` lowers to the `jet_string_lines` helper, `to_int()`
    // to the trim+parse form. (The AST emit path is gone — these prove the TIR.)
    assert!(
        out.rust.contains("jet_string_lines(&("),
        "lines() did not lower through the TIR (no jet_string_lines):\n{}",
        out.rust
    );
    assert!(
        out.rust
            .contains(".trim().parse::<i64>().map_err(|e| e.to_string())"),
        "to_int() did not lower through the TIR (no parse form):\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_string_parse", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "43\n-1\n3\na\nb\nc\n60\n");
}

// --- D-SOA1 / D-SOA2A-D: `#layout(columnar)` struct-of-arrays --------------

/// Compile a program to Rust (front end only) for source-level assertions.
fn compile_rust(name: &str, src: &str) -> String {
    let dir = std::env::temp_dir().join(format!("jet_tir_test_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    jet::compile_with_path(src, &shown)
        .unwrap_or_else(|diags| {
            panic!(
                "front end rejected:\n{}",
                jet::render_diagnostics(&shown, src, &diags)
            )
        })
        .rust
}

const COLUMNAR_PROG: &str = "\
#layout(columnar)
struct P {
    x: Float
    mass: Float
}
fn total(ps: [P]) -> Float {
    s := 0.0
    loop p in ps {
        s = s + p.mass
    }
    return s
}
fn main() {
    ps: [P] := [P { x: 0.0, mass: 1.0 }, P { x: 1.0, mass: 2.0 }]
    ps.push(P { x: 2.0, mass: 3.0 })
    print(ps.len())
    print(ps[2].x)
    print(ps[1].mass)
    print(total(ps))
}
";

/// The whole columnar surface (construct, push, len, index-read, field-read,
/// iterate) compiles and runs with AoS-identical behavior.
#[test]
fn columnar_list_core_surface_runs() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("tir_columnar_core", COLUMNAR_PROG);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n2.0\n2.0\n6.0\n");
}

/// Codegen emits the struct-of-arrays storage type and routes the list ops
/// through it — and emits ZERO `unsafe` (I1 golden grep).
#[test]
fn columnar_lowers_to_struct_of_arrays_no_unsafe() {
    let rust = compile_rust("tir_columnar_gen", COLUMNAR_PROG);
    assert!(
        rust.contains("struct user_P_columns"),
        "expected a generated struct-of-arrays type"
    );
    assert!(
        rust.contains("from_aos") && rust.contains("gather_at") && rust.contains("iter_aos"),
        "expected the columnar inherent API in the output"
    );
    // I1: no `unsafe` anywhere in generated columnar code.
    assert!(
        !rust.contains("unsafe"),
        "columnar codegen must emit no `unsafe`"
    );
}

/// D-SOA2D: serialization is transparent — a columnar `[S]` encodes identically
/// to the array-of-structs form.
#[test]
fn columnar_serialization_is_transparent() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.encoding.json as json
#[Codable]
#layout(columnar)
struct Pt { a: Int, b: Int }
#[Codable]
struct PlainPt { a: Int, b: Int }
fn main() {
    cs: [Pt] := [Pt { a: 1, b: 2 }, Pt { a: 3, b: 4 }]
    ps: [PlainPt] := [PlainPt { a: 1, b: 2 }, PlainPt { a: 3, b: 4 }]
    print(json.to_string(cs) == json.to_string(ps))
    print(json.to_string(cs))
}
";
    let (code, stdout) = build_and_run("tir_columnar_serde", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n[{\"a\":1,\"b\":2},{\"a\":3,\"b\":4}]\n");
}
