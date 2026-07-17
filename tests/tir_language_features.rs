//! TIR language features integration tests.

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;

use tir_support::{build_and_run, build_and_run_full, build_and_run_multi, have_rustc};

/// D-SHAPE3a=A: inferred fresh construction rewrites to the ordinary static
/// method call before TIR lowering. Expected types flow through bindings,
/// returns, fields, and call arguments; explicit `Type.new` remains valid.
#[test]
fn inferred_new_expected_type_routes_through_tir() {
    if !have_rustc() {
        return;
    }
    let src = r#"
struct Counter {
    value: Int
}

struct Holder {
    counter: Counter
}

impl Counter {
    fn new(value: Int) -> Counter {
        return Counter.{ value: value }
    }
}

fn fresh(value: Int) -> Counter {
    return .new(value)
}

fn read(counter: Counter) -> Int {
    return counter.value
}

fn run() {
    bound: Counter :: .new(1)
    holder: Holder :: .{ counter: .new(2) }
    pool: Pool<Holder> :: .new()
    nested_pool: Pool<Pool<Holder>> :: .new()
    explicit :: Counter.new(4)
    print(read(.new(5)))
    print("{bound.value}{holder.counter.value}{explicit.value}")
    print(fresh(6).value)
}
"#;
    let (code, stdout) = build_and_run("tir_inferred_new", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\n124\n6\n");
}

// ===========================================================================
// c109 Phase 23: @Pure / @Todo / default params / named args / distinct / tuples
// ===========================================================================

/// c109 Phase 23: a `@Pure fn` (S60) routes through the TIR — purity is a sema-only
/// check (E3401), erased at codegen, so the fn lowers byte-identically to a plain fn.
#[test]
fn pure_fn() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn double(n: Int) --[]-> Int {
    return (n * 2)
}
fn greeting(name: String) --[]-> String {
    return \"hi, {name}\"
}
fn run() {
    print(double(21))
    print(greeting(\"jet\"))
}
";
    let (code, stdout) = build_and_run("tir_pure", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\nhi, jet\n");
}

/// c109 Phase 23: a `@Todo` typed hole (`Expr::Todo`) emits a diverging
/// `todo!("@Todo at … — expected <ty>")`. The fn compiles + routes; the hole is never
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
    return @Todo
}
fn run() {
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
fn run() {
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
fn run() {
    print(area(width: 4, height: 3))
    print(area(4, height: 3))
}
";
    let (code, stdout) = build_and_run("tir_named_args", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "12\n12\n");
}

/// c109 Phase 23: distinct types (D-DIST1/D-DIST3). Construction `Name(x)` → newtype
/// `user_Name(x)`; `.raw()` → `(recv).0`; `@Numeric` distinct `+`/`==` use the native
/// operator. A distinct value type passes/returns/binds byte-identically.
#[test]
fn distinct_types() {
    if !have_rustc() {
        return;
    }
    let src = "\
UserId :: distinct Int;
@Numeric Meters :: distinct Float;

fn greet(id: UserId) -> String {
    return \"user {(id.raw())}\"
}
fn run() {
    uid :: UserId(42)
    print(greet(uid))
    a :: Meters(3.0)
    b :: Meters(1.5)
    c :: a + b
    print(\"{(c.raw())} m\")
    x :: UserId(7)
    y :: UserId(7)
    print(\"{(x == y)}\")
}
";
    let (code, stdout) = build_and_run("tir_distinct", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "user 42\n4.5 m\ntrue\n");
}

/// D-RANGETYPE1: range-constrained distinct constructors are fallible for
/// runtime values (`Severity(raw)?`), and arithmetic widens to the base `Int`
/// so codegen must not leave an `Int`-typed expression as raw newtype math.
#[test]
fn range_type_runtime_try_and_arithmetic_widens() {
    if !have_rustc() {
        return;
    }
    let src = "\
@Numeric Severity :: distinct Int(0..10);

fn checked(raw: Int) -> Severity ? String {
    return Ok(Severity(raw)?)
}

fn run() {
    a :: checked(4) ?? Severity(0)
    b :: Severity(6)
    widened: Int :: a + b
    print(\"{widened}\")
}
";
    let (code, stdout) = build_and_run("tir_range_type_checked", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "10\n");
}

/// c109 Phase 23: named tuples (S73/D-SG7). A tuple literal `(x: 1, y: 2)` → a generated
/// `JetTup_<hash>` struct lit (canonical field order); field access `p.x` → `(p).user_x`;
/// destructure `(a, b) :: ~p` → the borrow-temp + per-field `.clone()` form;
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
fn run() {
    p :: (x: 1, y: 2)
    q :: (y: 3, x: 4)
    same_shape :: (p == q)
    (a, b) :: ~p
    print(\"{p.x} {p.y} {a} {b} {same_shape}\")
    pair :: bounds()
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
/// index over `[String: JSON]`, and `json.to_string`. rustc accepting proves byte-parity.
#[test]
fn json_value_construct_match_render() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.encoding.json as json
fn run() {
    raw :: \"{{\\\"name\\\":\\\"jet\\\",\\\"ok\\\":true}}\"
    data :: json.parse(raw) ?? panic(\"bad json\")
    if data == Object(entries) {
        print(entries.len())
    }
    obj: [String: Json] := []
    obj[\"name\"] = Json.Text(\"jet\")
    obj[\"ok\"] = Json.Bool(true)
    obj[\"none\"] = Json.Null
    print(json.to_string(Json.Object(obj)))
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
fn run() {
    raw :: \"{{\\\"port\\\":\\\"8080\\\",\\\"name\\\":\\\"api\\\"}}\"
    data :: json.decode(raw) ?? panic(\"bad json\")
    if data == Object(entries) {
        port :: entries[\"port\"]
        name :: entries[\"name\"]
        if port == Int(n) {
            print(n + 1)
        }
        if name == Text(s) {
            print(s)
        }
    }
}
";
    let (code, stdout) = build_and_run("tir_json_coerce", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "8081\napi\n");
}

/// D-REGEXENGINE1=A: regex `Match` value type + `.group(n)` accessor. Regex is
/// std-only now, so this pins `Match.group` to the opaque match-value method.
#[test]
fn regex_match_group() {
    let src = "\
use core.regex as re
fn run() {
    text :: \"order 42 shipped\"
    m :: re.match(\"(\\\\d+) shipped\", text) ?? panic(\"bad pattern\")
    if m == Val(mat) {
        whole :: mat.group(0) ?? \"none\"
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
    // The `if let Some(user_mat)` if-let binds the `Match`; `.group(0)` reads it.
    assert!(
        out.rust.contains("if let Some(user_mat) ="),
        "Match value not bound via if-let:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("(user_mat).group(0i64)"),
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
fn run() {
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
    // lowering bug, omitted of `import_ns`, outside this fixture). `kind_str` matches
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
    return Note.{name: name, note_type: t, parent: None}
}
pub fn kind_str(n: Note) -> String {
    k :: n.note_type
    if k == {
        User -> { return \"user\" }
        Feedback -> { return \"feedback\" }
        Project -> { return \"project\" }
        Reference -> { return \"reference\" }
    }
}
fn run() { print(\"note\") }
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
fn run() {
    n :: note.make_note(\"x\", NoteType.User)
    q :: classify(\"user\")
    print(describe(n, q))
    q2 :: classify(\"design\")
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

/// D-SHAPE3b: Result spellings stay contextual identifiers, and dotted
/// Optional-looking names remain ordinary variants on user enums.
#[test]
fn result_names_do_not_reserve_user_functions_or_variants() {
    if !have_rustc() {
        return;
    }
    let src = r#"
enum Wrapped {
    Val(Int)
    Ok(Int)
    Err(Int)
}

fn Ok(value: Int) -> Int {
    return (value + 10)
}

fn Err(value: Int) -> Int {
    return (value + 20)
}

fn run() {
    print(Ok(1))
    print(Err(2))
    wrapped :: Wrapped.Val(7)
    if wrapped == {
        .Val(value) -> { print(value) }
        .Ok(value) -> { print(value) }
        .Err(value) -> { print(value) }
    }
}
"#;
    let (code, stdout) = build_and_run("tir_contextual_result_names", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "11\n22\n7\n");
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
        return Rect.{width: width, height: height}
    }
    fn area(self) -> Int {
        return (self.width * self.height)
    }
}
fn run() {
    r :: Rect.new(4, 3)
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
    name :: input() ?? \"world\"
    return \"hello, {name}\"
}
fn run() {
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
use core.http as http
fn handle_root(req: HttpRequest) -> HttpResponse {
    return HttpResponse.{status: \"200 OK\", body: \"welcome\", headers: []}
}
fn handle_user(req: HttpRequest) -> HttpResponse {
    id :: req.param(\"id\") ?? \"unknown\"
    return HttpResponse.{status: \"200 OK\", body: \"user={id}\", headers: []}
}
fn run() {
    router :: http.router()
    router.get(\"/\", handle_root)
    router.get(\"/users/:id\", handle_user)
    req :: http.parse(\"GET / HTTP/1.1\\nHost: localhost\")
    resp :: http.dispatch(router, req)
    print(resp.body())
}
";
    let (code, stdout) = build_and_run("tir_http_router", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "welcome\n");
}

/// c109 Phase 25 / E2804: duplicate routes are user runtime errors. They must
/// print Jet-owned panic text with the Jet source line, not Rust's panic banner.
#[test]
fn http_router_duplicate_route_is_jet_runtime_error() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.http as http
fn handle(req: HttpRequest) -> HttpResponse {
    return HttpResponse.{status: \"200 OK\", body: \"ok\", headers: []}
}
fn run() {
    router :: http.router()
    router.get(\"/users/:id\", handle)
    router.get(\"/users/:name\", handle)
}
";
    let (code, _stdout, stderr) =
        build_and_run_full("jet_tir_test", "tir_http_duplicate_route", src);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("panic: E2804: duplicate route `GET /users/:name`"),
        "missing Jet-owned E2804 runtime text:\n{stderr}"
    );
    assert!(
        stderr.contains("tir_http_duplicate_route.jet:8"),
        "missing Jet source location:\n{stderr}"
    );
    assert!(
        !stderr.contains("thread 'main' panicked"),
        "raw Rust panic banner leaked:\n{stderr}"
    );
}

/// c109 Phase 26: the `require(cond[, msg])` / `require_eq` rich-report builtins (S36,
/// 14_panic). A satisfied `require` is a no-op; the program continues. (The failing
/// branch's rich panic is exercised by the golden suite; here we prove the TIR
/// renders + runs the guard byte-for-byte.)
#[test]
fn require_builtins() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
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

/// c109 Phase 26: a `@Caps(Io) { … }` effect-restriction region (D-EFF1, effect_caps)
/// erases to a plain block in codegen; the body runs unchanged.
#[test]
fn caps_block() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn announce(label: String, n: Int) --[Io]-> {
    print(\"{label}: {n}\")
}
fn run() {
    @Caps(Io) {
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
fn bump(n: &Int) {
    n += 1
}
fn archive(name: ^String) -> String {
    return name
}
fn run() {
    score: Int := 41
    bump(&score)
    print(score)
greeting: String :: \"hello\"
    show(greeting)
saved: String :: archive(^\"vault\")
    print(saved)
}
";
    let (code, stdout) = build_and_run("tir_arg_conv", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\nhello\nvault\n");
}

/// c109 Phase 26: a fan-out result-list DESTRUCTURE `[a, b, c] :: <init>` (S74, 41_fan_out).
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
fn run() {
    doubled :: double.[1, 2, 3]
    [a, b, c] :: doubled
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
/// (24_callbacks). `double_fn :: double` binds a bare fn-name as a value; `apply_twice`
/// takes it (and a lambda) as a Fn arg; `Worker.{ step: … }` constructs a struct with a
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
struct TextWorker {
    step: fn(String) -> Int
}
fn text_len(text: String) -> Int {
    return text.len()
}
fn run() {
    double_fn :: double
    print(apply_twice(double_fn, 3))
    print(apply_twice((x: Int) => (x + 1), 5))
    w :: Worker.{step: (n: Int) => (n * n)}
    print(w.step(4))
    text_worker :: TextWorker.{step: text_len}
    text :: \"read\"
    print(text_worker.step(text))
    print(text)
}
";
    let (code, stdout) = build_and_run("tir_fn_value_struct_field", src);
    assert_eq!(code, 0);
    // double(double(3)) = 12; apply_twice(x+1, 5) = ((5+1)+1) = 7; w.step(4) = 4*4 = 16.
    assert_eq!(stdout, "12\n7\n16\n4\nread\n");
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
fn run() {
red: U8 :: 255
channel: I32 :: 100000
depth: I8 :: -120
    print(red)
    print(channel)
    print(depth)
total: I64 :: 9000000000
    print(total + 1)
half: U8 :: 100
    print(half + half)
bytes: [U8] :: [104, 105, 33]
    print(bytes)
wide: I64 :: red.to_i64()
    print(wide)
clamped: U8 :: channel.to_u8() ?? 255
    print(clamped)
    print(U8.MAX)
    print(I32.MIN)
flags: U8 :: 13
    print(flags.count_ones())
    print(Float.INFINITY.is_infinite())
hi: U8 :: 200
lo: U8 :: 100
    print(wrapping(hi + lo))
    print(saturating(hi + lo))
fallback: U8 :: 0
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
