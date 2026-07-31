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

struct GenericHolder<T> {
    value: T
    step: fn(Int) => Int
}

impl GenericHolder {
    fn new(value: ^T) => GenericHolder<T> {
        return GenericHolder<T>.{ value: value, step: (n: Int) => n + 9 }
    }

    fn marker(self) => Int {
        return self.step(0)
    }
}

impl Counter {
    fn new(value: Int) => Counter {
        return Counter.{ value: value }
    }
}

fn fresh(value: Int) => Counter {
    return .new(value)
}

fn read(counter: Counter) => Int {
    return counter.value
}

fn increment(value: Int) => Int {
    return value + 1
}

fn run() {
    bound :: Counter.new(1)
    holder :: Holder.{ counter: .new(2) }
    pool :: Pool<Holder>.new()
    nested_pool :: Pool<Pool<Holder>>.new()
    nested :: GenericHolder<GenericHolder<Int>>.new(.new(7))
    nested_explicit :: GenericHolder<GenericHolder<Int>>.new(GenericHolder<Int>.new(8))
    callback :: increment
    callback_holder :: GenericHolder<fn(Int) => Int>.new(^callback)
    explicit :: Counter.new(4)
    print(read(.new(5)))
    print("{bound.value}{holder.counter.value}{explicit.value}")
    print(fresh(6).value)
    print("{nested.value.value}{nested_explicit.value.value}")
    print(callback_holder.marker())
}
"#;
    let (code, stdout) = build_and_run("tir_inferred_new", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\n124\n6\n78\n9\n");
}

/// D-SHAPE-OPAQUE-INFER1=A: a generic constructor receiver may omit its
/// arguments when ordinary input/expected-type inference determines one answer.
#[test]
fn generic_constructor_arguments_infer_through_tir() {
    if !have_rustc() {
        return;
    }
    let src = r#"
struct Box<T> {
    value: T
}

impl Box {
    fn new(value: ^T) => Box<T> {
        return Box<T>.{ value: value }
    }
}

struct Pair<A, B> {
    first: A
    second: B
}

impl Pair {
    fn new(first: ^A, second: ^B) => Pair<A, B> {
        return Pair<A, B>.{ first: first, second: second }
    }
}

fn returned() => Box<Int> {
    return Box.new(4)
}

fn run() {
    direct :: Box.new(1)
    expected :: Box.new([])
    nested :: Box.new(Box.new(2))
    pair :: Pair.new("three", 3)
    explicit :: Box<Int>.new(5)
    print("{direct.value}{nested.value.value}")
    print("{pair.first}{pair.second}{returned().value}{explicit.value}")
}
"#;
    let (code, stdout) = build_and_run("tir_generic_constructor_infer", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "12\nthree345\n");
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
fn double(n: Int) =[]=> Int {
    return (n * 2)
}
fn greeting(name: String) =[]=> String {
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

/// c109 Phase 23: a `#Todo` typed hole (`Expr::Todo`) emits a diverging
/// `todo!("#Todo at … — expected <ty>")`. The fn compiles + routes; the hole is never
/// reached at runtime here (only the implemented fn is called).
#[test]
fn todo_hole() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn double(n: Int) => Int {
    return (n * 2)
}
fn not_yet(n: Int) => Int {
    return #Todo
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
fn box_dims(w: Int, h: Int = w, d: Int = h) => String {
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
fn area(width: Int, height: Int) => Int {
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

/// c109 Phase 23: distinct types (D-DIST1/D-DIST3). Destination conversion
/// `Name.from_kind(x)` → newtype
/// `user_Name(x)`; `.raw()` → `(recv).0`; `#Numeric` distinct `+`/`==` use the native
/// operator. A distinct value type passes/returns/binds byte-identically.
#[test]
fn distinct_types() {
    if !have_rustc() {
        return;
    }
    let src = "\
UserId :: distinct Int;
#Numeric Meters :: distinct Float;
#UnitFamily(Currency) { usd }

fn greet(id: UserId) => String {
    return \"user {(id.raw())}\"
}
fn run() {
    uid :: UserId.from_int(42)
    print(greet(uid))
    a :: Meters.from_float(3.0)
    b :: Meters.from_float(1.5)
    c :: a + b
    print(\"{(c.raw())} m\")
    x :: UserId.from_int(7)
    y :: UserId.from_int(7)
    print(\"{(x == y)}\")
    from_byte :: UserId.from_u8(8)
    from_float :: UserId.from_float(9.9) ?? UserId.from_int(0)
    print(\"{(from_byte.raw())} {(from_float.raw())}\")
    meters :: Meters.from_int(3)
    dollars :: Usd.from_int(5)
    print(\"{(meters.raw())} {(dollars.raw())}\")
}
";
    let (code, stdout) = build_and_run("tir_distinct", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "user 42\n4.5 m\ntrue\n8 9\n3.0 5.0\n");
}

#[test]
fn distinct_and_unit_numeric_source_matrix() {
    if !have_rustc() {
        return;
    }
    let src = "\
UserId :: distinct Int;
Label :: distinct String;
#UnitFamily(Currency) { usd }

fn checked_user(value: U64) => UserId ? String { return UserId.from_u64(value) }
fn pass_user(value: UserId ? String) => UserId ? String { return ~value }

fn run() {
    fallback :: UserId.from_int(0)
    print(UserId.from_i8(-8).raw())
    print(UserId.from_i16(-16).raw())
    print(UserId.from_i32(-32).raw())
    print(UserId.from_int(-64).raw())
    print(UserId.from_u8(8).raw())
    print(UserId.from_u16(16).raw())
    print(UserId.from_u32(32).raw())
    print((pass_user(checked_user(64)) ?? fallback).raw())
    print((UserId.from_f32(3.75) ?? fallback).raw())
    print((UserId.from_float(4.75) ?? fallback).raw())
    print(Usd.from_i8(-8).raw())
    print(Usd.from_i16(-16).raw())
    print(Usd.from_i32(-32).raw())
    print(Usd.from_int(-64).raw())
    print(Usd.from_u8(8).raw())
    print(Usd.from_u16(16).raw())
    print(Usd.from_u32(32).raw())
    print(Usd.from_u64(64).raw())
    print(Usd.from_f32(3.75).raw())
    print(Usd.from_float(4.75).raw())
    label :: Label.from_string(\"converted\")
    print(label.raw())
}
";
    let (code, stdout) = build_and_run("tir_distinct_source_matrix", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "-8\n-16\n-32\n-64\n8\n16\n32\n64\n3\n4\n\
-8.0\n-16.0\n-32.0\n-64.0\n8.0\n16.0\n32.0\n64.0\n3.75\n4.75\nconverted\n"
    );
}

/// D-RANGETYPE1/D-SHAPE-CONVERT1: range-constrained distinct conversions are
/// fallible for runtime values (`Severity.from_int(raw)?`), and arithmetic widens to the base `Int`
/// so codegen must not leave an `Int`-typed expression as raw newtype math.
#[test]
fn range_type_runtime_try_and_arithmetic_widens() {
    if !have_rustc() {
        return;
    }
    let src = "\
#Numeric Severity :: distinct Int(0..10);

fn checked(raw: Int) => Severity ? String {
    return Ok(Severity.from_int(raw)?)
}

fn pass_checked(value: Severity ? String) => Severity ? String { return ~value }
fn direct() => Severity { return Severity.from_u8(8) }

fn run() {
    a :: pass_checked(checked(4)) ?? panic(\"range\")
    b :: checked(6) ?? panic(\"range\")
    widened :: a + b
    print(\"{widened}\")
    print(direct().raw())
}
";
    let (code, stdout) = build_and_run("tir_range_type_checked", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "10\n8\n");
}

#[test]
fn range_type_literal_proofs_cover_every_numeric_source_family() {
    let src = r#"
#Numeric Severity :: distinct Int(0..10)

fn run() {
    signed :: Severity.from_i8(11)
    unsigned :: Severity.from_u8(12)
    narrow_float :: Severity.from_f32(13.0)
    float :: Severity.from_float(14.0)
}
"#;
    let diagnostics = jet::compile(src).expect_err("all four literals exceed Severity's range");
    let range_errors = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "E0135")
        .count();
    assert_eq!(range_errors, 4, "diagnostics: {diagnostics:#?}");
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
fn bounds() => (max: Int, min: Int) {
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
/// `JSON.Boolean`/`JSON.Object` construction (non-mangled `jet_std::JSON::…`), a Map
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
    obj := [String: JSON].{}
    obj[\"name\"] = JSON.Text(\"jet\")
    obj[\"ok\"] = JSON.Bool(true)
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
    m :: re.match(.{\"(\\\\d+) shipped\"}, text)
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
#Known version :: \"1.0\"
#Known banner :: \"logbook {version}\"
fn wrap(s: String) => String {
    return \"{banner}: {s}\"
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

pub fn make_note(name: ^String, t: ^NoteType) => Note {
    return Note.{name: name, note_type: t, parent: None}
}
pub fn kind_str(n: Note) => String {
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
fn classify(raw: String) => Query {
    if raw == \"user\" {
        return Query.Kind(NoteType.User)
    }
    return Query.Tag(raw)
}
fn describe(n: Note, q: Query) => String {
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

fn Ok(value: Int) => Int {
    return (value + 10)
}

fn Err(value: Int) => Int {
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
    fn new(width: Int, height: Int) => Rect {
        return Rect.{width: width, height: height}
    }
    fn area(self) => Int {
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
fn greet() => String {
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

/// c109 Phase 25: the HTTPRouter handle surface (D-ROUTE1=A, 76_http_routes). `http.router()`
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
use core.http.server as server
fn handle_root(req: HTTPRequest) => HTTPResponse ? HTTPError {
    return Ok(server.response(200, \"welcome\"))
}

fn handle_user(req: HTTPRequest) => HTTPResponse ? HTTPError {
    id :: req.param(\"id\") ?? \"unknown\"
    return Ok(server.response(200, \"user={id}\"))
}
fn run() {
    router :: http.router()
    router.get(\"/\", handle_root)
    router.get(\"/users/:id\", handle_user)
    req :: http.parse(\"GET / HTTP/1.1\\nHost: localhost\")
    resp :: http.dispatch(router, req) ?? panic(\"dispatch\")
    print(resp.body().text(1024) ?? panic(\"body\"))
}
";
    let (code, stdout) = build_and_run("tir_http_router", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "welcome\n");
}

#[test]
fn http_mux_unannotated_handler_lambda_is_owned_in_aot() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.http.server as server
fn run() {
    mux :: server.mux()
    mux.get(\"/\", (req) => Ok(server.response(200, req.path())))
    print(\"registered\")
}
";
    let (code, stdout) = build_and_run("tir_http_unannotated_handler", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "registered\n");
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
use core.http.server as server
fn handle(req: HTTPRequest) => HTTPResponse ? HTTPError {
    return Ok(server.response(200, \"ok\"))
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
        stderr.contains("tir_http_duplicate_route.jet:9"),
        "missing Jet source location:\n{stderr}"
    );
    assert!(
        !stderr.contains("thread 'main' panicked"),
        "raw Rust panic banner leaked:\n{stderr}"
    );
}

#[test]
fn http_router_named_catchall_and_encoded_marker_literals() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.http as http
use core.http.server as server
fn asset(req: HTTPRequest) => HTTPResponse ? HTTPError {
    return Ok(server.response(200, req.param(\"path\") ?? \"missing\"))
}
fn literal(req: HTTPRequest) => HTTPResponse ? HTTPError {
    return Ok(server.response(200, \"literal\"))
}
fn catch(req: HTTPRequest) => HTTPResponse ? HTTPError {
    return Ok(server.response(200, \"catch\"))
}
fn param_catch(req: HTTPRequest) => HTTPResponse ? HTTPError {
    return Ok(server.response(200, \"param-catch\"))
}
fn param_first(req: HTTPRequest) => HTTPResponse ? HTTPError {
    return Ok(server.response(200, \"param-first\"))
}
fn static_first(req: HTTPRequest) => HTTPResponse ? HTTPError {
    return Ok(server.response(200, \"static-first\"))
}
fn run() {
    router :: http.router()
    router.get(\"/assets/*path\", asset)
    router.get(\"/literal/%3Aadmin/%2Astar\", literal)
    router.get(\"/a/*rest\", catch)
    router.get(\"/a/:id/*rest\", param_catch)
    router.get(\"/tie/:first/static\", param_first)
    router.get(\"/tie/static/:last\", static_first)
    first :: http.dispatch(router, http.parse(\"GET /assets/css/site.css HTTP/1.1\\nHost: localhost\")) ?? panic(\"dispatch\")
    second :: http.dispatch(router, http.parse(\"GET /literal/%3Aadmin/%2Astar HTTP/1.1\\nHost: localhost\")) ?? panic(\"dispatch\")
    third :: http.dispatch(router, http.parse(\"GET /a/x/y HTTP/1.1\\nHost: localhost\")) ?? panic(\"dispatch\")
    fourth :: http.dispatch(router, http.parse(\"GET /tie/static/static HTTP/1.1\\nHost: localhost\")) ?? panic(\"dispatch\")
    print(first.body().text(1024) ?? panic(\"body\"))
    print(second.body().text(1024) ?? panic(\"body\"))
    print(third.body().text(1024) ?? panic(\"body\"))
    print(fourth.body().text(1024) ?? panic(\"body\"))
}
";
    let (code, stdout) = build_and_run("tir_http_route_syntax", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "css/site.css\nliteral\nparam-catch\nstatic-first\n");
}

#[test]
fn http_router_retired_bare_catchall_is_jet_runtime_error() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.http as http
use core.http.server as server
use core.env as env
fn handle(req: HTTPRequest) => HTTPResponse ? HTTPError {
    return Ok(server.response(200, \"ok\"))
}
fn run() {
    router :: http.router()
    dynamic :: env.get(\"JET_HTTP_TEST_MARKER\") ?? \"*\"
    pattern :: \"/assets/{dynamic}\"
    router.get(pattern, handle)
}
";
    let (code, _stdout, stderr) =
        build_and_run_full("jet_tir_test", "tir_http_retired_catchall", src);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("panic: E2805: invalid HTTP route `/assets/*`: write a named catch-all such as `*wildcard`"),
        "missing Jet-owned E2805 runtime text:\n{stderr}"
    );
    assert!(
        stderr.contains("tir_http_retired_catchall.jet:11"),
        "missing source location:\n{stderr}"
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

/// c109 Phase 26: a `#Caps(IO) { … }` effect-restriction region (D-EFF1, effect_caps)
/// erases to a plain block in codegen; the body runs unchanged.
#[test]
fn caps_block() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn announce(label: String, n: Int) =[IO]=> {
    print(\"{label}: {n}\")
}
fn run() {
    #Caps(IO) {
        announce(\"answer\", 42)
    }
}
";
    let (code, stdout) = build_and_run("tir_caps", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "answer: 42\n");
}
