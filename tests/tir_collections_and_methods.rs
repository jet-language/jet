//! TIR collections and methods integration tests.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{assert_tiers_agree, build_and_run, compile, have_rustc};
use jet::Interpreter::{dev_iteration, RunOutcome};

#[test]
fn generated_temporaries_do_not_collide_with_collection_locals() {
    let src = r#"
fn run() {
    value :: 2
    v :: 3
    k :: "apple"
    item :: 0
    i :: 9
    xs := [value, v]
    xs[1] = value
    loop (list_index, list_item), xs {
        print("{list_index}:{list_item}")
    }
    counts := [String: Int].{}
    counts[k] = value
    loop (map_key, map_value), counts {
        print("{map_key}={map_value}")
    }
}
"#;
    let rust = compile("tir_generated_name_collections", src);
    for stem in ["i", "item", "k", "v"] {
        let user = jet::AST::mangle(stem);
        let generated = jet::AST::mangle_generated(stem);
        assert_ne!(user, generated, "allocator lanes must stay distinct for {stem}");
        assert!(rust.contains(&generated), "generated binding {generated} missing");
    }
    assert!(rust.contains(&format!("let {}", jet::AST::mangle("v"))));
    assert!(rust.contains(&format!("let {}", jet::AST::mangle_generated("v"))));
    assert_tiers_agree("tir_generated_name_collections", src, "0:2\n1:2\napple=2\n");
}

// c109 Phase 5: collections — list/map literals, indexing/slicing, index-assign,
// and `loop x, coll` / `loop (k, v), map` iteration. The `IndexKind` (List/Map)
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
fn total(xs: [Int]) => Int {
    sum := 0
    loop x, xs {
        sum = (sum + x)
    }
    return sum
}
fn run() {
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

/// D-RANGE-EXCL1=C: two-binding yields index then item; `indexes` is always in bounds.
#[test]
fn list_two_binding_and_indexes() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    xs := [10, 20, 30]
    loop (i, v), xs {
        print(\"{i}:{v}\")
    }
    loop i, xs.indexes() {
        print(i)
    }
    empty := [Int].{}
    count := 0
    loop i, empty.indexes() {
        count = (count + 1)
    }
    print(count)
    loop pair, xs.indexed() {
        print(\"{pair.idx}:{pair.item}\")
    }
}
";
    let (code, stdout) = build_and_run("tir_list_index_idioms", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "0:10\n1:20\n2:30\n0\n1\n2\n0\n0:10\n1:20\n2:30\n"
    );
}

/// Indexed assignment into a list (`xs[i] = v`) — the `LValue::Index` vec form.
#[test]
fn list_index_assignment() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
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

/// A map literal (`[]`), map indexing, map insert (`m[k] = v`), and two-binding
/// `loop (k, v), map` iteration — the map-specific helpers and the `.iter()` clone
/// form. BTreeMap iterates in sorted key order, so output is deterministic.
#[test]
fn map_literal_index_insert_and_iteration() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    counts := [String: Int].{}
    counts[\"banana\"] = 3
    counts[\"apple\"] = 5
    print(counts[\"apple\"])
    loop (k, v), counts {
        print(\"{k}={v}\")
    }
    loop entry, counts {
        print(\"{entry.key}:{entry.value}\")
    }
}
";
    let (code, stdout) = build_and_run("tir_map", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\napple=5\nbanana=3\napple:5\nbanana:3\n");
}

/// E0163/E0164 teach this total update. Empty-map input proves a missing key
/// uses the default instead of panicking during the read-modify-write.
#[test]
fn map_get_update_is_total_for_missing_key() {
    let src = r#"
fn run() {
    counts := [String: Int].{}
    counts["missing"] = (counts.get("missing") ?? 0) + 1
    print(counts["missing"])
}
"#;
    assert_tiers_agree("tir_map_get_update", src, "1\n");
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
fn name_of(p: Person) => String {
    return ~p.name
}
fn run() {
    p :: Person.{ name: \"Grace\", age: 40 }
    print(name_of(p))
}
";
    let (code, stdout) = build_and_run("tir_clone_getter", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "Grace\n");
}

/// A user-defined instance method with scalar args on a covered struct. The
/// caller `run` routes through the TIR; `(c).__jet_add(10i64, 20i64)` is emitted
/// from the resolved `method_sigs` conventions; the method body with `self`
/// also routes through executable TIR.
#[test]
fn user_method_with_scalar_args() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Calc {
    base: Int

    fn add(self, x: Int, y: Int) => Int {
        return ((self.base + x) + y)
    }
}
fn calc(c: Calc) => Int {
    return c.add(10, 20)
}
fn run() {
    c :: Calc.{ base: 1 }
    print(calc(c))
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
    label: String

    fn combine(self, other: String) => String {
        return \"{self.label}-{other}\"
    }
}
fn calc(b: Crate) => String {
    name :: \"x\"
    return b.combine(name)
}
fn run() {
    b :: Crate.{ label: \"t\" }
    print(calc(b))
}
";
    let (code, stdout) = build_and_run("tir_method_string_arg", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "t-x\n");
}

/// A trait-impl method call. `(d).label()` is emitted with the BARE method name
/// (the trait impl owns it — no generated mangle), decided at lowering from
/// `cx.trait_methods`. The caller `describe` routes through the TIR.
#[test]
fn trait_impl_method_call_no_mangle() {
    if !have_rustc() {
        return;
    }
    let src = "\
trait Named {
    fn label(self) => String
}
struct Dog {
    sound: String
}
impl Dog.Named {
    fn label(self) => String {
        return \"dog\"
    }
}
fn describe(d: Dog) => String {
    return d.label()
}
fn run() {
    d :: Dog.{ sound: \"woof\" }
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

    fn code(self) => Int {
        if self == {
            .Red -> { return 1 }
            .Green -> { return 2 }
        }
    }
}
fn calc(l: Light) => Int {
    return l.code()
}
fn run() {
    print(calc(Light.Green))
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
fn scores() => [String: Int] {
    return [\"a\": 1, \"b\": 2]
}
fn run() {
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
// that static dispatch (`Type.make(x)` → `__jet_T::__jet_make(x)`) is correct.
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

    fn make(v: Int) => Counter {
        return Counter.{ n: v }
    }
    fn value(self) => Int {
        return self.n
    }
}
fn run() {
    c :: Counter.make(5)
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

    fn doubled(&self) => Int {
        return (self.total + self.total)
    }
}
fn run() {
    a := Acc.{ total: 7 }
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

    fn make_pos() => Sign {
        return Sign.Pos
    }
    fn to_num(self) => Int {
        if self == {
            .Pos -> { return 1 }
            .Neg -> { return 0 }
            .Zero -> { return 0 }
        }
    }
}
fn run() {
    s :: Sign.make_pos()
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

    fn make(x: Int, y: Int) => Vec2 {
        return Vec2.{ x: x, y: y }
    }
    fn sum(self) => Int {
        return (self.x + self.y)
    }
    fn shifted(self, dx: Int) => Vec2 {
        return Vec2.{ x: (self.x + dx), y: self.y }
    }
}
fn run() {
    p :: Vec2.make(3, 4)
    q :: p.shifted(10)
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
fn parse_age(raw: Int) => Int ? ParseError {
    if raw == 0 {
        return Err(ParseError.Empty)
    }
    if raw == 1 {
        return Err(ParseError.BadDigit(raw))
    }
    return Ok((raw * 2))
}
fn load(raw: Int) => Int ? ParseError {
    n :: parse_age(raw)?
    return Ok((n + 1))
}
fn run() {
    a :: load(21) ?? 0
    print(a)
    b :: load(0) ?? 99
    print(b)
}
";
    let (code, stdout) = build_and_run("tir_fallible", src);
    assert_eq!(code, 0);
    // load(21): parse_age→Ok(42), n=42, Ok(43); ?? → 43.
    // load(0):  parse_age→Err(Empty), ? propagates Err; ?? → 99.
    assert_eq!(stdout, "43\n99\n");
}

/// The `??` fallback in its early-`return` form (a `T ? E` value), plus `ok`/`err`.
#[test]
fn or_fallback_return_form() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn checked(x: Int) => Int ? Err {
    if x == 0 {
        return Err(\"zero\")
    }
    return Ok((100 /% x))
}
fn safe(x: Int) => Int {
    return checked(x) ?? return -1
}
fn run() {
    print(safe(4))
    print(safe(0))
}
";
    let (code, stdout) = build_and_run("tir_or_return", src);
    assert_eq!(code, 0);
    // safe(4): checked→Ok(25), ?? → 25. safe(0): checked→err, ?? return -1.
    assert_eq!(stdout, "25\n-1\n");
}

/// An optional `T?` with `Val`/`None` constructors and a `??` fallback.
#[test]
fn optional_val_none_and_fallback() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn first_even(limit: Int) => (Int?) {
    loop i, 1..limit {
        if (i % 2) == 0 {
            return Val(i)
        }
    }
    return None
}
fn run() {
    print(first_even(9) ?? 0)
    print(first_even(1) ?? 0)
}
";
    let (code, stdout) = build_and_run("tir_optional", src);
    assert_eq!(code, 0);
    // first_even(9)→Val(2); first_even(1)→None → 0.
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
fn handle_of(a: (Account?)) => (String?) {
    return a?.profile?.handle
}
fn run() {
    p :: Profile.{ handle: Val(\"jay\") }
    acct :: Account.{ profile: p }
    print(handle_of(Val(acct)) ?? \"none\")
    print(handle_of(None) ?? \"none\")
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
fn build() => [Int] {
    xs := [3, 1, 2]
    xs.push(5)
    xs.insert(0, 0)
    xs.sort()
    xs.reverse()
    return xs
}
fn run() {
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
fn run() {
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

/// E3 breadth: String owns the common search, trim, padding, classification,
/// title, and single-split operations. The test runs through the TIR lowering
/// path and proves the tuple-shaped `split_once` result is usable by name.
#[test]
fn string_surface_breadth() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    s := \"  hello jet world  \"
    print(s.trim_start())
    print(s.trim_end())
    print(s.pad_start(22, \".\"))
    print(s.pad_end(22, \".\"))
    print(s.index_of(\"jet\"))
    print(s.count(\"l\"))
    print(\"Hello\".is_alphabetic())
    print(\"123\".is_numeric())
    print(\" \\t\".is_whitespace())
    print(\"Jet lang\".is_ascii())
    print(\"hELLO jet\".to_title())
    pair :: \"left:right\".split_once(\":\") ?? panic(\"split\")
    print(pair.before)
    print(pair.after)
}
";
    let (code, stdout) = build_and_run("tir_string_surface_breadth", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "hello jet world  \n  hello jet world\n...  hello jet world  \n  hello jet world  ...\n8\n3\ntrue\ntrue\ntrue\ntrue\nHello Jet\nleft\nright\n"
    );
}

/// E3 breadth: Set and Rank expose the complete algebra surface with one
/// semantic operation family on every execution tier.
#[test]
fn set_algebra_methods() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn sorted(xs: [Int]) => [Int] {
    ys := ~xs
    ys.sort()
    return ys
}
fn run() {
    a := Set.from([1, 2, 3])
    b := Set.from([3, 4])
    i := a.intersection(b).to_list()
    d := a.difference(b).to_list()
    x := a.symmetric_difference(b).to_list()
    print(sorted(i))
    print(sorted(d))
    print(sorted(x))
    print(a.is_subset(Set.from([1, 2, 3, 4])))
    print(a.is_superset(Set.from([1, 2])))
    print(a.is_disjoint(Set.from([8])))
    s := Rank.from([1, 2, 3])
    t := Rank.from([3, 4])
    print(s.intersection(t).to_list())
    print(s.difference(t).to_list())
    print(s.symmetric_difference(t).to_list())
    print(s.is_subset(Rank.from([1, 2, 3, 4])))
    print(s.is_superset(Rank.from([1, 2])))
    print(s.is_disjoint(Rank.from([8])))
}
";
    let (code, stdout) = build_and_run("tir_set_algebra", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "[3]\n[1, 2]\n[1, 2, 4]\ntrue\ntrue\ntrue\n[3]\n[1, 2]\n[1, 2, 4]\ntrue\ntrue\ntrue\n"
    );
}

/// D-STR-AFTER1: `.after(sep)`/`.before(sep)` — first-occurrence substring
/// split. `sep` absent -> the whole original string on both sides (mirrors
/// `.replace`'s no-match-is-identity convention; no `Option` to unwrap).
#[test]
fn string_after_before() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    email := \"nate@jet.dev\"
    print(email.after(\"@\"))
    print(email.before(\"@\"))
    plain := \"no-separator\"
    print(plain.after(\"@\"))
    print(plain.before(\"@\"))
}
";
    let (code, stdout) = build_and_run("tir_string_after_before", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "jet.dev\nnate\nno-separator\nno-separator\n");
}

/// Map methods: add, add_new, get, has_key, lazy keys/values, len, clear.
/// JetMap iterates in sorted key order, so output is deterministic.
#[test]
fn map_builtin_methods() {
    let src = "\
fn run() {
    probe := [String: Int].{ "k": 41 }
    print(probe.add(\"k\", 5) ?? 0)
    print(probe.add(\"new\", 9) ?? -99)
    m := [String: Int].{}
    print(m.add(\"banana\", 3) ?? 0)
    print(m.add(\"apple\", 5) ?? 0)
    print(m.add(\"apple\", 7) ?? 0)
    print(m.add_new(\"apple\", 9))
    print(m.add_new(\"cherry\", 11))
    print(m.len())
    print(m.has_key(\"apple\"))
    v := m.get(\"apple\")
    print(v ?? 0)
    ks := m.keys()
    print(ks.len())
    vs := m.values()
    print(vs.len())
}
";
    assert_tiers_agree(
        "tir_map_builtins",
        src,
        "41\n-99\n0\n0\n5\nfalse\ntrue\n3\ntrue\n7\n3\n3\n",
    );
}

/// D-ONCE-VERB1=A: every collection's removal operation returns the removed
/// value, and List.replace names only an indexed swap.
#[test]
fn collection_pop_table() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    xs := [10, 20, 30]
    print(xs.pop() ?? -1)
    print(xs.replace(0, 99))
    counts := [String: Int].{ \"words\": 4 }
    print(counts.pop(\"words\") ?? -1)
    seen := Set.from([7, 8])
    print(seen.pop(8) ?? -1)
    queue := Queue.init([1, 2, 3])
    print(queue.pop_front() ?? -1)
    priorities := PriorityQueue.from([2, 9, 4])
    print(priorities.pop() ?? -1)
}
";
    let (code, stdout) = build_and_run("tir_collection_pop_table", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "30\n[99, 20]\n4\n8\n1\n9\n");
}

/// D-CORE-EAGER1=A / D-LOOPMAP1=B: concrete containers evaluate map/filter
/// once and return plain lists; `.lazy()` selects the deferred Iter plane.
#[test]
fn eager_container_adapters_and_lazy_opt_in() {
    let src = "\
fn count_and_keep(n: Int, visits: Cell<Int>) => Bool {
    visits.edit(count => count += 1)
    return n % 2 == 0
}
fn run() {
    nums := [1, 2, 3, 4]
    mapped := nums.map((n: Int) => n + 1)
    print(mapped)
    print(mapped.len())
    visits := Cell.new(0)
    visits_copy :: ~visits
    even := nums.map((n: Int) => n + 1).filter((n: Int) => count_and_keep(n, visits_copy))
    print(even)
    print(visits.get())
    lazy_values := nums.lazy().filter((n: Int) => n > 2).map((n: Int) => n * 10).to_list()
    print(lazy_values)
    parts := \"a,b,c\".split(\",\")
    print(parts.to_list())
}
";
    let rust = compile("tir_eager_container_adapters", src);
    assert!(
        rust.contains("jet_list_map_filter(("),
        "adjacent eager adapters did not use the fused Prelude kernel"
    );
    assert!(rust.contains("jet_iter_filter("));
    assert!(rust.contains("jet_iter_map("));
    assert!(rust.contains("jet_iter_string_split("));
    assert_tiers_agree(
        "tir_eager_container_adapters",
        src,
        "[2, 3, 4, 5]\n4\n[2, 4]\n4\n[30, 40]\n[a, b, c]\n",
    );
}

/// `remove` on both a list (value default and explicit slot mode) and a map
/// (the `.remove(&(k).clone())` form) — the Map-vs-List branch resolved at lowering.
#[test]
fn list_and_map_remove() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn drop_first(xs: [Int]) => Int {
    ys := ~xs
    r := ys.remove(0, .Slot)
    return ys.len()
}
fn drop_key(m: [String: Int]) => Int {
    m2 := ~m
    r := m2.remove(\"a\")
    return m2.len()
}
fn run() {
    print(drop_first([10, 20, 30]))
    counts := [String: Int].{}
    counts[\"a\"] = 1
    counts[\"b\"] = 2
    print(drop_key(counts))
}
";
    let (code, stdout) = build_and_run("tir_remove", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\n1\n");
}

/// D-LISTREMOVE1/F plus the missing list verbs: value removal is the default,
/// `.Slot` preserves positional removal, `count` counts equal items, `extend`
/// mutates in order, and `concat` returns a new list.
#[test]
fn list_remove_modes_and_surface_gaps() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    xs := [1, 2, 1]
    print(xs.remove(1) ?? 0)
    print(xs.count(1))
    print(xs.remove(0, .Slot) ?? 0)
    xs.extend([3, 4])
    print(xs.concat([5]).len())
    print(xs.len())
    by :: RemoveBy.Val
    print(xs.remove(1, by) ?? 0)
    print(xs.len())
    ss := [\"a\", \"b\", \"a\"]
    print(ss.remove(\"a\", .Val) ?? \"\")
    print(ss.len())
}
";
    let (code, stdout) = build_and_run("tir_list_remove_modes", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n1\n2\n4\n3\n1\n2\na\n2\n");
}

/// D-LISTREMOVE1/F: the same surface must run through the forced interpreter
/// path, including the map-view calls that use its list-shaped iterator value.
#[test]
fn list_surface_forced_interpreter() {
    let path = format!(
        "{}/examples/features/collections/list_surface.jet",
        env!("CARGO_MANIFEST_DIR")
    );
    match dev_iteration(&path, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0);
            assert_eq!(stderr, "");
            // Matches examples/features/expected/collections/list_surface.out
            // (the full #1477 List surface — this assertion was stale, left
            // over from a smaller version of the fixture).
            assert_eq!(
                stdout,
                "true\ntrue\n[2, 3, 4]\ntrue\n2\n[1, 2, 3, 4, 5, 6]\n[2, 4]\n[1, 3, 5]\n5\n1\n5\n1\n1\n[1, 9, 1]\n"
            );
        }
        RunOutcome::Problems(diagnostics) => {
            panic!("forced interpreter rejected list surface: {diagnostics:?}")
        }
    }
}

/// `join(sep)` on a list of strings — the `.iter().map(jet_show)…join` form.
#[test]
fn list_join_with_separator() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    words := [\"a\", \"b\", \"c\"]
    print(words.join(\"-\"))
}
";
    let (code, stdout) = build_and_run("tir_join", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "a-b-c\n");
}

/// `join(sep)` also works on a borrowed list window.
#[test]
fn view_join_with_separator() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    words := [\"a\", \"b\", \"c\"]
    middle :: words[0..1]
    print(middle.join(\"-\"))
}
";
    let (code, stdout) = build_and_run("tir_view_join", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "a-b\n");
}

/// `join(sep)` on an exclusive write window (`ViewMut`) — must not clone `&mut [T]`.
#[test]
fn view_mut_join_with_separator() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    words := [\"a\", \"b\", \"c\"]
    middle := &words[0..1]
    print(middle.join(\"-\"))
}
";
    let (code, stdout) = build_and_run("tir_view_mut_join", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "a-b\n");
}

/// A `when` over a fallible value with `ok`/`err` patterns (Shape C). The subject
/// is a user fallible fn call; the bound payload prints.
#[test]
fn fallible_when_match() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum ClassifyError {
    Bad(String)
}
fn classify(x: Int) => Int ? ClassifyError {
    if x == 0 {
        return Err(ClassifyError.Bad(\"bad\"))
    }
    return Ok((x + 10))
}
fn run() {
    if classify(5) == {
        .Ok(n) -> {
            print(n)
        }
        .Err(e) -> {
            print(e)
        }
    }
    if classify(0) == {
        .Ok(n) -> {
            print(n)
        }
        .Err(e) -> {
            print(e)
        }
    }
}
";
    let (code, stdout) = build_and_run("tir_fallible_when", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "15\nBad(\"bad\")\n");
}
