//! TIR pattern, field, and collection-receiver integration tests.

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;
use std::process::Command;

use tir_support::{build_and_run, have_rustc};

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
struct Crate {
    items: [Int]

    fn get(self) -> Int {
        return 42
    }
    fn len(self) -> Int {
        return 7
    }
}
fn run() {
    b :: Crate.{ items: [1, 2, 3] }
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
fn run() {
    e :: [1, 2, 3].is_empty()
    print(e)
    m :: [1: 2]
    print(m.is_empty())
    s :: \"hi\"
    print(s.is_empty())
empty :: [Int].{}
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
fn run() {
    f([10, 20])
empty :: [Int].{}
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
kid ::  t.child 
    if kid == {
        Val(c) -> {
            total = total + sum(c)
        }
        None -> {}
    }
    return total
}
fn run() {
    root :: Tree.{
        value: 3,
        child: Val(Tree.{
            value: 2,
            child: Val(Tree.{ value: 1, child: None })
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
/// bare borrowed-in-env non-Copy ident (`Person.{ name: n }` where `n: String` is a
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
    return Person.{ name: n }
}
fn run() {
    p :: make(\"Ada\")
    print(p.name)
}
";
    let (code, stdout) = build_and_run("tir_borrowed_struct_lit", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "Ada\n");
}

/// c109 (B3): a struct-destructuring binding `Type.{ x, y } :: p` routes through
/// the TIR and prints the field sum, matching the old `BindPattern::Struct` baseline.
#[test]
fn struct_destructure_binding() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Point { x: Int, y: Int }
fn run() {
    p :: Point.{ x: 1, y: 2 }
    Point.{ x, y } :: p
    print(x + y)
}
";
    let (code, stdout) = build_and_run("tir_struct_destructure", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

/// D-DESTRUCT1: struct-shaped dispatch arm heads bind fields and test literal
/// fields in the same arm. This is the source-level dispatch spelling; the
/// internal Rust lowering may still call the helper path a switch.
#[test]
fn struct_pattern_dispatch_arm_head_runs() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Incident {
    kind: String
    title: String
    retries: Int
}
fn route(i: Incident) -> String {
    if i == {
        .{ kind: \"page\", title, .. } -> { return title }
        .{ kind: \"ticket\", title, .. } -> { return title }
        else -> { return \"other\" }
    }
}
fn run() {
    page :: Incident.{ kind: \"page\", title: \"database\", retries: 2 }
    ticket :: Incident.{ kind: \"ticket\", title: \"docs\", retries: 1 }
    other :: Incident.{ kind: \"note\", title: \"memo\", retries: 0 }
    print(route(page))
    print(route(ticket))
    print(route(other))
}
";
    let (code, stdout) = build_and_run("tir_struct_pattern_dispatch", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "database\ndocs\nother\n");
}

/// c109 (B4): a user-enum variant if-let condition `if m == Ping(n) { } else { }`
/// routes through the TIR and binds the payload, matching the old if-let baseline.
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
fn run() {
    print(f(Msg.Ping(7)))
    print(f(Msg.Pong))
}
";
    let (code, stdout) = build_and_run("tir_user_enum_if_let", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "7\n-1\n");
}

/// c109 (B2): a fixed-size-list type `[E#N]` as a param (fed a fan-out result) and
/// as a struct field routes through the TIR (rendered `Vec<E>`).
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
fn run() {
    print(firstof(double.[1, 2, 3]))
    g :: Grid.{ row: double.[1, 2, 3] }
    print(g.row[1])
}
";
    let (code, stdout) = build_and_run("tir_fixed_list", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\n4\n");
}

/// D-FIXARR1 / D-CRYPTO-DIAG1: Core calls use the same fixed-list widening as
/// ordinary calls after sema consumes the compile-known length fact.
#[test]
fn fixed_size_list_widens_at_core_call() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.crypto.expert as expert

fn run() {
    seed :: [U8#32].{ 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31 }
    #Unsafe(\"fixed signature vector\") {
        signature :: expert.ed25519_sign(seed, [])
    }
    print(\"ok\")
}
";
    let (code, stdout) = build_and_run("tir_fixed_list_core_call", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "ok\n");
}

/// c109 (B1): a mixed-switch over a NON-IDENT subject (a field access) with a
/// payload-binding arm head. The deleted emitter once produced
/// `matches!(…, Some(c))` then used the unbound `c` (E0425); TIR emits the Rust
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
        Val(c) -> { return c }
        else -> { return 0 }
    }
}
fn run() {
    hold :: Holder.{ val: Val(5) }
    print(f(hold))
    empty :: Holder.{ val: None }
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
fn run() {
    print(classify())
}
";
    let (code, stdout) = build_and_run("tir_mixed_nonident_variant", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

/// c109 (S57/M9.5): a comptime LOCAL `comptime name = expr` in a function body. Sema
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
fn build() -> [Int] {
    xs := [Int].{}
    loop i; 1..3 {
        xs.push(i * 10)
    }
    return xs
}
fn run() {
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
/// auto-clones the handle — `emit_call_args` emits `(…).clone()` (D-MEM1 S6: `Shared<T>`
/// lowers to `jet_std::JetShared<T>`, a newtype with its own cheap-handle `Clone` impl,
/// not a bare `Arc<T>` — was `Arc::clone(&…)` before this stage) and the receiving
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
fn run() {
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
    // Byte-exact auto-clone emit: the free-call arg auto-clones the handle, then
    // the `Read` non-scalar `Shared<Int>` param borrows it. (D-MEM1 S6: `Shared<T>`
    // now lowers to `jet_std::JetShared<T>`, not a bare `std::sync::Arc<T>` — its
    // own `Clone` impl is a cheap handle clone, so plain `.clone()` replaces the
    // old `Arc::clone(&…)` text.)
    assert!(
        out.rust.contains("user_noop(&(((*user_h)).clone()));"),
        "shared auto-clone free-call arg not byte-exact:\n{}",
        out.rust
    );
    // The receiving param signature is the shared `rust_param_type` form.
    assert!(
        out.rust
            .contains("pub fn user_noop(user_h: &jet_std::JetShared<i64>)"),
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

/// c109: an owning field read of a NON-SCALAR field (`s :: p.name`, `name:
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

fn run() {
    p :: P.{ name: \"x\" }
    s :: p.name
    t :: p.name
    print(s)
    print(t)
}
";
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust
            .contains("let user_s: String = ((user_p).user_name).clone();"),
        "owning non-scalar field-read clone not byte-exact:\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_owning_field_clone", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "x\nx\n");
}

/// c109: an indexed map-assign whose index BASE is a struct field read
/// (`s.scores["a"] = 1`, `scores: [String: Int]`). The `LValue::Index` gate
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
    scores: [String: Int]
}

fn run() {
    s := S.{ scores: [] }
    s.scores[\"a\"] = 1
    print(s.scores[\"a\"])
}
";
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust
            .contains("jet_map_insert(&mut ((user_s).user_scores),"),
        "map-assign through field not byte-exact:\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_map_assign_field", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

/// c109: a map builtin (`.len()`) on a struct-FIELD-read receiver
/// (`s.scores.len()`), where the field came from an empty-map struct-literal
/// field (`scores: []` takes its type from the struct field). The builtin gate
/// admits a field-read receiver; `main` routes through the TIR and emits
/// `((user_s).user_scores).len() as i64` byte-for-byte. Runs (empty map → 0).
#[test]
fn map_builtin_on_struct_field_receiver() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct S {
    scores: [String: Int]
}

fn run() {
    s := S.{ scores: [] }
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

/// c109: a field read off a comptime-const STRUCT value (`comptime pair_value = Pair{…}`;
/// `pair_value.left`) and an `==` against a comptime-const ENUM value (`comptime light_value =
/// Light.Green`; `light_value == Light.Green`). Each const inlines to its pre-rendered
/// Rust value; the field read / comparison matches the old emitter baseline.
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

comptime pair_value = Pair.{left: 7, right: \"seven\"}
comptime light_value = Light.Green

fn run() {
    p :: Pair.{left: 7, right: \"seven\"}
    l :: Light.Green
    print(\"{pair_value.left}\")
    print(\"{p.left}\")
    print(\"{pair_value.right}\")
    print(\"{p.right}\")
    print(\"{light_value == Light.Green}\")
    print(\"{l == Light.Green}\")
}
";
    let out = jet::compile(src).expect("should compile");
    // Byte-exact: `pair_value.left` reads a field off the inlined struct literal.
    assert!(
        out.rust.contains(
            "(user_Pair { user_left: 7i64, user_right: \"seven\".to_string() }).user_left"
        ),
        "comptime struct field read not byte-exact:\n{}",
        out.rust
    );
    // Byte-exact: `light_value == Light.Green` compares the inlined enum value.
    assert!(
        out.rust
            .contains("(user_Light::user_Green) == (user_Light::user_Green)"),
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
    Empty
}
fn run() {
    w :: Wrapper.Some(42)
    if w == Some(_) {
        print(\"has value\")
    }
}
";
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust
            .contains("if let user_Wrapper::user_Some(_) = user_w"),
        "wildcard enum-payload if-let not byte-exact:\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_wildcard_payload_iflet", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "has value\n");
}

/// c97/D-STRPARSE1: `String.lines()` (→ `[String]`) and `Int.parse(text)` (→
/// `Int ? ParseError`). Both are compiler built-ins, so `main` routes
/// through the TIR — proven by the emitted `jet_string_lines` helper call and
/// the static parse form. `Int.parse` composes with `??`: a good parse yields the
/// value, a bad one (`"abc"`) takes the fallback.
#[test]
fn string_lines_and_int_parse() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    n :: Int.parse(\"42\") ?? -1
    print((n + 1))
    bad :: Int.parse(\"abc\") ?? -1
    print(bad)
    lines :: \"a\\nb\\nc\".lines()
    print(lines.len())
    loop line; lines {
        print(line)
    }
    total := 0
    loop row; \"10\\n20\\n30\".lines() {
        total += (Int.parse(row) ?? 0)
    }
    print(total)
}
";
    let out = jet::compile(src).expect("should compile");
    // TIR routing: `lines()` lowers to the `jet_string_lines` helper, `Int.parse`
    // to the trim+parse form. (The AST emit path is gone — these prove the TIR.)
    assert!(
        out.rust.contains("jet_string_lines(&("),
        "lines() did not lower through the TIR (no jet_string_lines):\n{}",
        out.rust
    );
    assert!(
        out.rust
            .contains(".trim().parse::<i64>().map_err(|_| format!"),
        "Int.parse did not lower through the TIR (no parse form):\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_string_parse", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "43\n-1\n3\na\nb\nc\n60\n");
}

#[test]
fn array_of_structs_field_mutation() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Point {
    x: Int
}
fn run() {
    points := [Point.{x: 1}, Point.{x: 2}]
    points[0].x = 11
    points[0].x += 1
    print(points[0].x)
}
";
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust.contains(
            "{ let __jet_v = 11i64; (user_points)[0i64 as usize].user_x = __jet_v; }"
        ),
        "plain indexed field assignment did not mutate the list element:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains(").user_x).jet_add((1i64)")
            && out
                .rust
                .contains("(user_points)[0i64 as usize].user_x = __jet_v;"),
        "compound indexed field assignment did not use the checked add spine:\n{}",
        out.rust
    );
    assert!(
        !out.rust.contains(".user_x +="),
        "indexed field compound assignment leaked to Rust +=:\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_struct_list_mutation", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "12\n");
}

#[test]
fn indexed_struct_field_compound_rejects_user_operator_before_codegen() {
    let src = r#"
struct Vec2 {
    x: Int
    y: Int
}

struct Holder {
    value: Vec2
}

impl Vec2.Add {
    fn add(self, rhs: Vec2) -> Vec2 {
        return Vec2.{ x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

fn run() {
    hs := [Holder.{ value: Vec2.{ x: 1, y: 2 } }]
    hs[0].value += Vec2.{ x: 3, y: 4 }
    print("{hs[0].value.x},{hs[0].value.y}")
}
"#;
    let diags = jet::compile(src).expect_err("indexed user operator needs a stable place");
    assert!(
        diags.iter().any(|diag| diag.code == "E0362"),
        "indexed field compound assignment reached codegen instead of E0362: {diags:#?}"
    );
}
