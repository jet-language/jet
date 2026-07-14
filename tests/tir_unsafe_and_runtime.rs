//! TIR unsafe and runtime integration tests.

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;

use tir_support::{build_and_run, have_rustc};

/// c109 Phase 18 / D-UNSAFE2: the expert low-level tier (S58, E2-M13/D-LL1). A
/// `#Unsafe("reason") fn` lowers to a Rust `unsafe fn`; a `#Unsafe("reason") { … }`
/// audited region lowers to `unsafe { … }` (the reason string emits nothing);
/// `mem.Ptr<T>.from_addr(addr)`, `mem.address_of(x)`, and `mem.volatile_read(p)`
/// lower to the raw-pointer ops. I1: every emitted `unsafe` is a gated form tied
/// 1:1 to a source gate.
#[test]
fn unsafe_fn_block_and_ptr_ops() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.mem
#Unsafe(\"reads through a raw pointer; addr must be a live, valid Int\")
fn read_reg(addr: Int) -> Int {
    p :: mem.Ptr<Int>.from_addr(addr)
    return mem.volatile_read(p)
}
fn run() {
cell: Int :: 1337
    addr :: mem.address_of(cell)
    #Unsafe(\"addr is the address of `cell`, a live Int on this stack frame\") {
        p :: mem.Ptr<Int>.from_addr(addr)
        seen :: mem.volatile_read(p)
        print(seen)
        again :: read_reg(addr)
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
    p :: mem.Ptr<Int>.from_addr(addr)
    return mem.volatile_read(p)
}
fn run() {
cell: Int :: 1337
    addr :: mem.address_of(cell)
    #Unsafe(\"safe: cell is live\") {
        seen :: read_reg(addr)
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
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    // `#Unsafe fn` → `pub unsafe fn …`.
    assert!(
        out.rust
            .contains("pub unsafe fn user_read_reg(user_addr: i64) -> i64 {"),
        "unsafe fn signature not byte-exact:\n{}",
        out.rust
    );
    // `mem.Ptr<Int>.from_addr(addr)` and `mem.volatile_read(p)` in the fn body (sema
    // annotates the inferred `p` binding with its resolved `*mut i64` type).
    assert!(
        out.rust
            .contains("let user_p: *mut i64 = ((user_addr) as usize as *mut i64);"),
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
        out.rust
            .contains("let user_addr: i64 = (&(user_cell) as *const _ as usize as i64);"),
        "address_of not byte-exact:\n{}",
        out.rust
    );
    // `#Unsafe("…") { … }` → `unsafe {` (the reason string emits nothing).
    assert!(
        out.rust.contains("    unsafe {\n"),
        "unsafe block not emitted:\n{}",
        out.rust
    );
    // Reason string emits nothing — "safe: cell is live" must not appear in generated Rust.
    assert!(
        !out.rust.contains("safe: cell is live"),
        "reason string must emit nothing:\n{}",
        out.rust
    );
    // I1 self-check: drop every vetted prelude region (jet_mem and the rest of
    // the canonical list), then every remaining `unsafe` must be a gated form
    // (`unsafe {` or `unsafe fn`).
    let user = tir_support::strip_vetted_prelude_modules(&out.rust);
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

#[test]
fn volatile_write_emit_is_byte_exact() {
    let src = "\
use core.mem
#Unsafe(\"UART TX register is mapped by the target profile\")
fn write_reg(value: Int) {
    p :: mem.Ptr<Int>.from_addr(0x40000100)
    mem.volatile_write(p, value)
}
fn run() {
}
";
    let dir = std::env::temp_dir().join(format!("jet_tir_volatile_write_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join("volatile_write.jet");
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    assert!(
        out.rust
            .contains("let user_p: *mut i64 = ((1073742080i64) as usize as *mut i64);"),
        "PtrFromAddr constant not byte-exact:\n{}",
        out.rust
    );
    assert!(
        out.rust
            .contains("std::ptr::write_volatile(user_p, user_value);"),
        "volatile_write not byte-exact:\n{}",
        out.rust
    );
    let _ = fs::remove_dir_all(&dir);
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
    return Pair<T>.{first: a, second: b}
}
struct Stack<T> {
    items: [T]
}
fn empty_stack<T>() -> Stack<T> {
    return Stack<T>.{items: []}
}
fn push<T>(s: Stack<T>, item: T) -> Stack<T> {
    dup := s
    dup.items.push(item)
    return dup
}
fn run() {
p: Pair<Int> :: make_pair(1, 2)
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
    return note.Note.{ title: \"hello\", pages: 3 }
}
fn run() {
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
        out.rust.contains(
            "user_note::user_Note { user_title: \"hello\".to_string(), user_pages: 3i64 }"
        ),
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
fn run() {
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
/// handle methods, and the `arena_view` binding (`x :: arena.alloc(v)`, read via deref).
#[test]
fn arena_alloc_reset_free() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.mem
fn run() {
    arena :: mem.Arena.new()
    x :: arena.alloc(42)
    print(x)
    arena.reset()
    y :: arena.alloc(99)
    print(y)
    sized :: mem.Arena.new(capacity: 4096)
    s :: sized.alloc(7)
    print(s)
    pool :: mem.Pool.new(slots: 8)
    p :: pool.alloc(3)
    print(p)
    arena.free()
}
";
    let (code, stdout) = build_and_run("tir_arena", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n99\n7\n3\n");
}

/// D-BLOCKPLANE1: explicit `#Region(r) { … }` — plain Rust block
/// scope; views made inside live only until the block ends.
#[test]
fn arena_region_block() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.mem
fn run() {
    #Region(scratch) {
        a :: mem.Arena.new()
        b :: mem.Bump.new()
        x :: a.alloc(1)
        y :: b.alloc(2)
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
/// plain lexical block with an `_ctx_guard_<i>` RAII guard.
#[test]
fn smart_context_block() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.mem
fn run() {
    arena :: mem.Arena.new()
    #Context(allocator: arena) {
        x :: arena.alloc(10)
        print(x)
    }
    y :: arena.alloc(20)
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
fn calc() -> Int {
    a :: math.abs((-5))
    b :: math.min(3, 7)
    c :: math.max(3, 7)
    d :: math.clamp(15, 0, 10)
    io.eprint(\"trace: {a} {b} {c} {d}\")
    xs := [1, 2, 3]
    random.shuffle(&xs)
    p :: random.pick(xs)
    return ((((a + b) + c) + d) + (p ?? 0))
}
fn run() {
    print(calc())
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
use core.http as http
fn handle(req: HttpRequest) -> HttpResponse {
    m :: req.method()
    p :: req.path()
    h :: req.header(\"host\")
    q :: req.param(\"id\")
    body :: \"m={m} p={p}\"
    return HttpResponse.{status: \"200 OK\", body: body, headers: []}
}
fn describe(resp: HttpResponse) -> String {
    s :: resp.status()
    b :: resp.body()
    return \"{s}: {b}\"
}
fn run() {
    req :: http.parse(\"GET /x HTTP/1.1\\nHost: localhost\")
    resp :: handle(req)
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
fn run() {
    a :: tasks.spawn(() => sum_range(1, 25))
    b :: tasks.spawn(() => sum_range(26, 50))
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
fn run() {
    tasks.spawn(() => 42).detach()
    print(\"launched\")
}
";
    let (code, stdout) = build_and_run("tir_task_detach", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "launched\n");
}

/// c109 Phase 21 / D-TUPLE-DESTRUCT1: the full channel surface —
/// `tasks.channel<T>()` producer returning `(Sender<T>, Receiver<T>)`,
/// `sender.clone()` (a second sender), `Sender.send(v)` (inside a `take(..)`
/// spawn closure), `Task.join()`, and `Receiver.receive() ?? panic(..)`
/// (`Result<T, Closed>` unwrap).
#[test]
fn channel_send_receive() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.tasks as tasks
fn run() {
(s1, ch) :: tasks.channel<Int>()
    s2 :: copy s1
    t1 :: tasks.spawn(take(s1) () => {
        s1.send(30)
    })
    t2 :: tasks.spawn(take(s2) () => {
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

#[test]
fn taskgroup_select_receives_from_real_channel() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.tasks as tasks
fn run() {
    taskgroup g {
        (sender, receiver) :: tasks.channel<Int>()
        sender.send(42)
        value :: g.select().recv(receiver).wait()
        print(value)
    }
}
";
    let (code, stdout) = build_and_run("tir_taskgroup_select", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
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
fn run() {
    print(count_chars(\"hello\"))
    print(join_words(\"a,b,c\"))
}
";
    let (code, stdout) = build_and_run("tir_method_iter", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\n[a][b][c]\n");
}

/// c109 Phase 22: the optional-binding `if` condition — `if x == Val(b) { … b … }`
/// lowers to `if let Some(b) = x`, and `x == None` lowers to `.is_none()`. Reproduces
/// `emit_if`'s if-let / is_none condition shapes byte-for-byte.
#[test]
fn optional_binding_if_condition() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn describe(x: Int?) -> String {
    if x == Val(n) {
        return \"got {n}\"
    }
    if x == None {
        return \"nothing\"
    }
    return \"?\"
}
fn first_even(xs: [Int]) -> Int {
    out: [Int] := []
    i := 0
    loop i < xs.len() {
        if xs.get(i) == Val(v) {
            out.push(v)
        }
        i+= 1
    }
    return out.len()
}
fn run() {
    nothing: Int? :: None
    print(describe(Val(7)))
    print(describe(nothing))
    print(first_even([1, 2, 3]))
}
";
    let (code, stdout) = build_and_run("tir_opt_if", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "got 7\nnothing\n3\n");
}
