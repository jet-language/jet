//! TIR modules and enums integration tests.

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;

use tir_support::{build_and_run, build_and_run_multi, have_rustc};

/// Imported public adapters may carry core.data containers across a module
/// boundary. Their signatures are ordinary TIR types; codegen must not fall out
/// of the typed-IR seam merely because the row is dynamic or generic.
#[test]
fn imported_public_table_adapters_stay_in_tir() {
    if !have_rustc() {
        return;
    }
    let main_src = "\
module adapter
fn run() {
    print(\"ok\")
}
";
    let adapter_src = "\
pub fn concrete(table: ^Table<DataTree>) -> Table<DataTree> {
    return table
}
pub fn generic<T>(table: ^Table<T>) -> Table<T> {
    return table
}
";
    let (code, stdout) = build_and_run_multi(
        "tir_imported_table_adapter",
        "main.jet",
        &[("main.jet", main_src), ("adapter.jet", adapter_src)],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "ok\n");
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
fn run() {
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
fn run() {
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
fn run() {
    print(math.clamp(15, 0, 10))
    print(math.label(\"x\", 5))
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
pub fn label(prefix: String, n: Int) -> String {
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
fn run() {
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
fn run() {
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
fn run() {
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
fn run() {
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

/// c109 Phase 15: a DELEGATION trait method (`impl T.Trait using field`). The
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
impl Voice.Speaker {
    fn say(self, msg: String) -> String {
        p :: self.prefix
        return \"{p}: {msg}\"
    }
}
struct Megaphone {
    inner: Voice
}
impl Megaphone.Speaker using inner
fn run() {
    v := Voice.{ prefix: \"HEY\" }
    m := Megaphone.{ inner: v }
    print(m.say(\"go\"))
}
";
    let (code, stdout) = build_and_run("tir_delegation", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "HEY: go\n");
}

/// c109 Phase 15: the `a ?? panic(…)` fallback form. The panic message + the sorted
/// scalar-locals snapshot (`safe_locals_expr`) is reproduced from the lexical lowering
/// environment. On the success path the fallback is never taken; the program returns the
/// unwrapped value.
#[test]
fn or_fallback_panic_form() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn maybe(n: Int) -> (Int?) {
    if n > 0 {
        return Val(n)
    }
    return None
}
fn risky(count: Int, ratio: Float) -> Int {
    base := count + 1
    got :: maybe(count) ?? panic(\"no value at {count}\")
    return got + base
}
fn run() {
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
fn run() {
    m :: wrap(\"hi\")
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
fn run() {
    a :: Tree.Leaf(7)
    b :: wrap(a)
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
fn run() {
    pt :: Point.{ x: 3, y: 4 }
    sh :: mk(pt)
    print(\"{first(sh)}\")
}
";
    let (code, stdout) = build_and_run("tir_struct_payload_enum", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

/// c109 Phase 16: a struct with a covered collection field, and an enum variant
/// carrying a covered collection payload. Both emit the field/payload value plainly
/// (`items: vec![…]`, `Holder.Nums(xs)`) against the old emitter baseline.
#[test]
fn collection_field_and_payload() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Crate {
    items: [Int]
    label: String
}
enum Holder {
    Nums([Int])
    One(Int)
}
fn mk(xs: [Int]) -> Holder {
    return Holder.Nums(xs)
}
fn run() {
    b :: Crate.{ items: [1, 2, 3], label: \"x\" }
    d :: mk([4, 5])
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
fn run() {
    print(\"{id(5)}\")
    print(\"{pick(1, 2, true)}\")
    print(\"{firstof([10, 20, 30])}\")
    ys :: wrap(7)
    print(\"{ys[0]}\")
}
";
    let (code, stdout) = build_and_run("tir_generic_free_fns", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\n1\n10\n7\n");
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
    return HttpResponse.{status: \"200 OK\", body: body, headers: []}
}
fn build_req() -> HttpRequest {
    return HttpRequest.{method: \"GET\", path: \"/\", body: \"\", headers: []}
}
fn run() {
    r :: build_resp(\"hi\")
    q :: build_req()
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
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
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
