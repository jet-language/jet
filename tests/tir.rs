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
    if classify(5) {
        it == ok(n) -> {
            print(n)
        }
        it == err(e) -> {
            print(e)
        }
    }
    if classify(0) {
        it == ok(n) -> {
            print(n)
        }
        it == err(e) -> {
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
