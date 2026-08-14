//! TIR unsafe and runtime integration tests.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;

use tir_support::{build_and_run, build_and_run_full, have_rustc};

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
fn read_reg(addr: Int) => Int {
    p :: mem.Ptr<Int>.from_addr(addr)
    return mem.volatile_read(p)
}
fn run() {
cell :: 1337
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

/// Regression for a real memory-safety bug: sema's D-VERDICT-1308-1 implicit
/// comptime fold used to bake `mem.address_of`'s TIR-eval synthetic "stable
/// place identity" (an FNV-1a hash, meaningful only inside that evaluator) as
/// a literal AOT `i64` — the compiled program then dereferenced a wild
/// address. The one syntactic guard that used to sit in
/// crates/jet-sema/src/Sema/CheckerCore/bindings.rs was bypassable by any
/// wrapping form (e.g. one extra `( )`, preserved as `Expr::Paren` under
/// D-FMTPARENS1=A) and never covered the `#Known` / method-call / constant
/// paths at all. Fixed at the one place the value is actually minted
/// (crates/jet-codegen/src/Codegen/TIR/eval/exprs.rs's CoreCall handling for
/// `core.mem.address_of`): it now refuses outside `runtime_execution`,
/// so every fold attempt — however it's spelled or reached — declines and
/// falls through to real runtime codegen instead.
#[test]
fn mem_address_of_never_folds_plain_or_parenthesized() {
    if !have_rustc() {
        return;
    }
    for (name, addr_init) in [
        ("tir_addr_of_plain", "mem.address_of(cell)"),
        ("tir_addr_of_paren", "(mem.address_of(cell))"),
    ] {
        let src = format!(
            "\
use core.mem
#Unsafe(\"reads through a raw pointer; addr must be a live, valid Int\")
fn read_reg(addr: Int) => Int {{
    p :: mem.Ptr<Int>.from_addr(addr)
    return mem.volatile_read(p)
}}
fn run() {{
    cell :: 1337
    addr :: {addr_init}
    #Unsafe(\"addr is the address of `cell`, a live Int on this stack frame\") {{
        print(read_reg(addr))
    }}
}}
"
        );
        let (code, stdout) = build_and_run(name, &src);
        assert_eq!(code, 0, "case {name}");
        assert_eq!(stdout, "1337\n", "case {name}");
    }
}

/// The explicit `#Known` path demands a compile-time answer (unlike the
/// silent-decline implicit path above) — `mem.address_of` genuinely has none,
/// so it must now surface a real diagnostic instead of silently baking the
/// same wild-address bug under an even stronger "I promise this is checked"
/// spelling.
#[test]
fn mem_address_of_known_binding_is_a_compile_error_not_a_silent_bake() {
    let src = "\
use core.mem
fn run() {
    cell :: 1337
    #Known addr :: mem.address_of(cell)
    print(addr)
}
";
    let diags = jet::compile(src).expect_err("#Known mem.address_of must not silently fold");
    assert!(
        diags.iter().any(|d| d.code == "E0956"),
        "expected E0956 (can't run at compile time), got: {diags:?}"
    );
}

/// c109 Phase 18 / D-UNSAFE2: assert the EMITTED Rust for the unsafe tier is byte-exact
/// (the gate forms + ptr ops), and that EVERY `unsafe` is a gated form (`unsafe fn` /
/// `unsafe {`) — the I1 self-check. The reason string emits no comment/marker.
#[test]
fn unsafe_tier_emit_is_byte_exact() {
    let src = "\
use core.mem
#Unsafe(\"reads through a raw pointer; addr must be valid\")
fn read_reg(addr: Int) => Int {
    p :: mem.Ptr<Int>.from_addr(addr)
    return mem.volatile_read(p)
}
fn run() {
cell :: 1337
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
            .contains("pub unsafe fn __jet_read_reg(__jet_addr: i64) -> i64 {"),
        "unsafe fn signature not byte-exact:\n{}",
        out.rust
    );
    // `mem.Ptr<Int>.from_addr(addr)` and `mem.volatile_read(p)` in the fn body (sema
    // annotates the inferred `p` binding with its resolved `*mut i64` type).
    assert!(
        out.rust
            .contains("let __jet_p: *mut i64 = ((__jet_addr) as usize as *mut i64);"),
        "PtrFromAddr not byte-exact:\n{}",
        out.rust
    );
    assert!(
        out.rust
            .contains("return jet_mem::jet_sentry_volatile_read((__jet_p), \"valid_ptr\");"),
        "volatile_read sentry wrapper missing:\n{}",
        out.rust
    );
    // `mem.address_of(cell)` → the sentry-backed address identity (no `unsafe`).
    assert!(
        out.rust
            .contains("let __jet_addr: i64 = jet_mem::jet_sentry_address_of((&(__jet_cell) as *const _));"),
        "address_of sentry wrapper missing:\n{}",
        out.rust
    );
    // `#Unsafe("…") { … }` → `unsafe {` (the reason string emits nothing).
    assert!(
        out.rust.contains("    unsafe {\n"),
        "unsafe block not emitted:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("jet_mem::jet_sentry_scope(true"),
        "unsafe block must carry its sentry gate:\n{}",
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
#Unsafe(\"UART TX register is mapped by the target machine\")
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
            .contains("let __jet_p: *mut i64 = ((1073742080i64) as usize as *mut i64);"),
        "PtrFromAddr constant not byte-exact:\n{}",
        out.rust
    );
    assert!(
        out.rust
            .contains("jet_mem::jet_sentry_volatile_write((__jet_p), __jet_value, \"valid_ptr\");"),
        "volatile_write sentry wrapper missing:\n{}",
        out.rust
    );
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// c109 Phase 19: generic structs, foreign types, Stopwatch, arena/region.
// ---------------------------------------------------------------------------

/// c109 Phase 19: a GENERIC STRUCT free function — a turbofish struct literal
/// (`__jet_Pair::<i64> { … }`), a `Type::Apply` param/return, a `[T]`-field builtin
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
fn make_pair<T>(a: T, b: T) => Pair<T> {
    return Pair<T>.{first: ~a, second: ~b}
}
struct Stack<T> {
    items: [T]
}
fn empty_stack<T>() => Stack<T> {
    return Stack<T>.{items: []}
}
fn push<T>(s: Stack<T>, item: T) => Stack<T> {
    dup := ~s
    dup.items.push(item)
    return dup
}
fn run() {
p :: Pair<Int>.{ make_pair(1, 2) }
    print(p.first)
    st := Stack<Int>.{ empty_stack() }
    st = push(st, 42)
    print(st.items[0])
}
";
    let (code, stdout) = build_and_run("tir_generic_struct", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n42\n");
}

/// c109 Phase 19: a FOREIGN (imported user) struct constructed via the `import_ns`
/// namespace path (`alias.Note { … }` → `{root}__jet_note::__jet_Note { … }`), passed
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
fn make() => Note {
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
            "__jet_note::__jet_Note { __jet_title: \"hello\".to_string(), __jet_pages: 3i64 }"
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
    loop i, 0..100 {
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
/// `mem.Pool.new(slots:)` / `mem.Fixed.new(size:)` producers and `alloc`/`reset`
/// handle methods, and the `arena_view` binding (`x :: arena.alloc(v)`, read via deref).
#[test]
fn arena_alloc_reset_close() {
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
    close(^arena)
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
    close(^arena)
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
/// parity: guard tests/tir_unsafe_and_runtime.rs::polymorphic_core_specials
#[test]
fn polymorphic_core_specials() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.math as math
use core.random as random
use core.io as io
fn calc() => Int {
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

/// c109 Phase 20: HTTPRequest/HTTPResponse method accessors (`req.method()`/
/// `req.path()`/`req.body()`/`req.header(n)`/`req.param(n)`/`resp.status()`/
/// `resp.body()`/`resp.header(n)`). These carry `recv_type == Some(HTTPRequest|
/// HTTPResponse)`; now that the lambda-param type is written back onto `p.ty`
/// (sema), the slot type is total and the handle-op shape selects correctly. The
/// emit (`(recv).<field>.clone()`, `(recv).headers.get(&a0).cloned()`,
/// `jet_http_request_param(&(recv), &(a0))`) reproduces `emit_builtin_method`
/// byte-for-byte. `handle` is a typed free function (the example form); it routes.
#[test]
fn http_request_response_accessors() {
    if !have_rustc() {
        return;
    }
    // `http.parse` triggers the http prelude (so `JetHTTPRequest`/the accessor
    // helpers are in scope) and yields an HTTPRequest without networking; a
    // single-line request keeps the lexer happy (Jet has no `\r` escape).
    let src = "\
use core.http as http
use core.http.server as server
fn handle(req: HTTPRequest) => HTTPResponse {
    m :: req.method()
    p :: req.path()
    h :: req.header(\"host\")
    q :: req.param(\"id\")
    body :: \"m={m} p={p}\"
    return server.response(200, body)
}
fn describe(resp: HTTPResponse) => String {
    s :: resp.status()
    b :: resp.body().text(1048576) ?? \"invalid body\"
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
    assert_eq!(stdout, "200: m=GET p=/x\n");
}

/// c109 Phase 21: `task` + `Task<T>` value + `Task.join()` — the spawn/join
/// surface (32_tasks). The spawn closure is Phase-11/13 covered; the new coverage is the
/// `Task<Int>` binding value type + the `recv_type == None` `.join()` method.
#[test]
fn task_spawn_join() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn sum_range(first: Int, last: Int) => Int {
    total := 0
    loop n, first..last {
        total = (total + n)
    }
    return total
}
fn run() {
    a :: task sum_range(1, 25)
    b :: task sum_range(26, 50)
    print((a.join() ?? 0) + (b.join() ?? 0))
}
";
    let (code, stdout) = build_and_run("tir_task_spawn_join", src);
    assert_eq!(code, 0);
    // `loop n, first..last` is inclusive (S22/D-SG8): sum(1..=25) + sum(26..=50).
    assert_eq!(stdout, "1275\n");
}

fn assert_task_tier_parity(name: &str, src: &str, expected_stdout: &str) {
    let (code, stdout) = build_and_run(name, src);
    assert_eq!(code, 0);
    assert_eq!(stdout, expected_stdout);

    let dir =
        std::env::temp_dir().join(format!("jet_{name}_parity_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy().into_owned();

    let mut bundle = jet::Loader::load_entry(&shown).expect("task bundle should load");
    let errors = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.severity,
                jet::Diagnostics::Severity::Error
            )
        })
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "task program must type-check: {errors:?}");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "task program must stay resident-JIT safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle).expect("task program must compile in resident JIT");

    for (tier, force_interpreter) in [("resident JIT", false), ("interpreter", true)] {
        jet_jit::reset_jit_trace_for_test();
        match jet::Interpreter::dev_iteration(&shown, false, force_interpreter) {
            jet::Interpreter::RunOutcome::Ran {
                stdout: tier_stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(
                    exit_code, code,
                    "{tier} exit drift: stdout={tier_stdout:?} stderr={stderr:?}"
                );
                assert_eq!(stderr, "", "{tier} stderr drift: stdout={tier_stdout:?}");
                assert_eq!(tier_stdout, stdout, "{tier} ordered output drift");
                if !force_interpreter {
                    assert!(
                        jet_jit::jit_executed_for_test(),
                        "task program must execute native resident JIT code"
                    );
                    assert!(
                        !jet_jit::fallback_invoked_for_test(),
                        "task program resident JIT must not invoke fallback"
                    );
                    assert!(
                        !jet_jit::deopt_invoked_for_test(),
                        "task program resident JIT must not deopt"
                    );
                }
            }
            jet::Interpreter::RunOutcome::Problems(diagnostics) => {
                panic!("{tier} rejected task program: {diagnostics:?}")
            }
        }
    }
}

/// D-CONC-SPAWN1=D: `task.all` waits for each branch in source order and
/// returns the results in that same order.
#[test]
fn task_all() {
    if !have_rustc() {
        return;
    }
    let src = r#"
fn work(value: Int, turns: Int) => Int {
    total := value
    loop _, 1..turns {
        total += 1
        total -= 1
    }
    return total
}
fn run() {
    results :: (task.all {
        work(10, 10000),
        work(20, 1),
        work(30, 100)
    }) ?? panic("task.all failed")
    print(results[0], results[1], results[2])
}
"#;
    assert_task_tier_parity("tir_task_all", src, "10\n20\n30\n");
}

/// D-CONC-FAIL1=A: child failure is one typed rail on AOT, resident JIT, and
/// forced interpreter. `??` is the only consumer-side recovery form.
#[test]
fn task_failure_rail() {
    if !have_rustc() {
        return;
    }
    let src = r#"
use core.time as time

fn boom() => Int {
    panic("boom")
    return 0
}
fn deadline() => Int {
    #Context(deadline: 0) {
        time.sleep(1)
    }
    return 0
}
fn failure_label(error: TaskFailure) => String {
    if error == {
        .Panicked(reason) -> {
            return "panic:{reason}"
        }
        .DeadlineBlown -> { return "deadline" }
        .Cancelled -> { return "cancelled" }
    }
}
fn run() {
    task.group workers {
        failed :: task boom()
        failed_result :: failed.join()
        if failed_result == {
            .Err(error) -> { print(failure_label(error)) }
            .Ok(_) -> { print("wrong panic variant") }
        }
        all_result :: task.all { boom(), 2 }
        if all_result == {
            .Err(error) -> { print(failure_label(error)) }
            .Ok(_) -> { print("wrong all variant") }
        }
        expired :: task deadline()
        expired_result :: expired.join()
        if expired_result == {
            .Err(error) -> { print(failure_label(error)) }
            .Ok(_) -> { print("wrong deadline variant") }
        }
        cancelled :: task {
            time.sleep(1000)
        }
        cancelled.cancel()
        cancelled_result :: cancelled.join()
        if cancelled_result == {
            .Err(error) -> { print(failure_label(error)) }
            .Ok(_) -> { print("wrong cancel variant") }
        }
    }
}
"#;
    assert_task_tier_parity(
        "tir_task_failure_rail",
        src,
        "panic:boom\npanic:boom\ndeadline\ncancelled\n",
    );
}

#[test]
fn task_all_allows_nested_task_in_every_tier() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn nested(value: Int) => Int {
    inner :: task value + 1
    return inner.join() ?? 0
}
fn run() {
    outer :: task nested(40)
    print(outer.join() ?? 0)
}
";
    assert_task_tier_parity("tir_task_all_nested", src, "41\n");
}

#[test]
fn task_join_parent_deadline_is_e3003_in_every_tier() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    #Context(deadline: 0) {
        handle :: task 10
        handle.join() ?? 0
    }
    print(\"unreachable\")
}
";
    let (code, stdout, stderr) =
        build_and_run_full("jet_tir_test", "tir_task_join_deadline", src);
    assert_eq!(code, 70, "{stderr}");
    assert_eq!(stdout, "", "{stderr}");
    assert!(
        stderr.contains("Error [E3003]: deadline exceeded while waiting in task join"),
        "{stderr}"
    );

    let dir = std::env::temp_dir().join(format!(
        "jet_task_join_deadline_parity_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy().into_owned();
    let mut bundle = jet::Loader::load_entry(&shown).expect("deadline bundle should load");
    let errors = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.severity,
                jet::Diagnostics::Severity::Error
            )
        })
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "deadline program must type-check: {errors:?}");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "deadline program must stay resident-JIT safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle)
        .expect("deadline program must compile in resident JIT");

    for (tier, force_interpreter) in [("resident JIT", false), ("interpreter", true)] {
        jet_jit::reset_jit_trace_for_test();
        match jet::Interpreter::dev_iteration(&shown, false, force_interpreter) {
            jet::Interpreter::RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_ne!(exit_code, 0, "{tier} ignored the expired deadline");
                assert_eq!(stdout, "", "{tier} continued after the expired deadline");
                assert!(stderr.contains("E3003"), "{tier}: {stderr}");
            }
            jet::Interpreter::RunOutcome::Problems(diagnostics) => {
                assert!(
                    diagnostics.iter().any(|diagnostic| diagnostic.code == "E3003"),
                    "{tier} reported the wrong deadline error: {diagnostics:?}"
                );
            }
        }
        if !force_interpreter {
            assert!(
                jet_jit::jit_executed_for_test(),
                "deadline program must execute native resident JIT code"
            );
            assert!(
                !jet_jit::fallback_invoked_for_test(),
                "deadline program resident JIT must not invoke fallback"
            );
            assert!(
                !jet_jit::deopt_invoked_for_test(),
                "deadline program resident JIT must not deopt"
            );
        }
    }
}

#[test]
fn task_join_all_consumes_handles_once() {
    let valid = "\
use core.tasks as tasks
fn run() {
    task.group g {
        first :: task { return 10 }
        second :: task { return 20 }
        handles :: [first, second]
        results :: tasks.join_all(^handles)
        print(results.len())
    }
}
";
    let compiled = jet::compile(valid).expect("join_all should consume the handle list");
    assert!(
        compiled.lints.iter().all(|lint| lint.code != "L1101"),
        "joined handles must not trigger L1101: {:?}",
        compiled.lints
    );

    let duplicate = "\
use core.tasks as tasks
fn run() {
    task.group g {
        handle :: task { return 10 }
        tasks.join_all([handle, handle])
    }
}
";
    let diagnostics = jet::compile(duplicate).expect_err("one handle cannot be joined twice");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0121"),
        "expected E0121 for duplicate handle consumption, got {diagnostics:?}"
    );

    let reused = "\
use core.tasks as tasks
fn run() {
    task.group g {
        handle :: task { return 10 }
        tasks.join_all([handle])
        handle.join()
    }
}
";
    let diagnostics = jet::compile(reused).expect_err("joined handle must stay consumed");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0121"),
        "expected E0121 after join_all consumption, got {diagnostics:?}"
    );

    let borrowed_list = "\
use core.tasks as tasks
fn run() {
    task.group g {
        handle :: task { return 10 }
        handles :: [handle]
        tasks.join_all(handles)
    }
}
";
    let diagnostics =
        jet::compile(borrowed_list).expect_err("named handle lists need an ownership transfer");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0201"),
        "expected E0201 for a borrowed handle list, got {diagnostics:?}"
    );
}

#[test]
fn task_combinator_parent_deadline_is_e3003_in_every_tier() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.time as time

fn slow(value: Int) => Int {
    time.sleep(1)
    return value
}

fn run() {
    #Context(deadline: 0) {
        task.group workers {
            if (task.all { slow(1), slow(2) }) == {
                .Ok(results) -> {
                    print(results[0], results[1])
                }
                .Err(error) -> {
                    panic(\"unexpected child task failure\")
                }
            }
        }
    }
    print(\"unreachable\")
}
";
    let (code, stdout, stderr) =
        build_and_run_full("jet_tir_test", "tir_task_all_deadline", src);
    assert_eq!(code, 70, "{stderr}");
    assert_eq!(stdout, "", "{stderr}");
    assert!(
        stderr.contains("Error [E3003]: deadline exceeded while waiting in task selection"),
        "{stderr}"
    );

    let dir = std::env::temp_dir().join(format!(
        "jet_task_all_deadline_parity_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy().into_owned();
    let mut bundle = jet::Loader::load_entry(&shown).expect("combinator bundle should load");
    let errors = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.severity,
                jet::Diagnostics::Severity::Error
            )
        })
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "combinator program must type-check: {errors:?}");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "combinator program must stay resident-JIT safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle)
        .expect("combinator program must compile in resident JIT");

    for (tier, force_interpreter) in [("resident JIT", false), ("interpreter", true)] {
        jet_jit::reset_jit_trace_for_test();
        match jet::Interpreter::dev_iteration(&shown, false, force_interpreter) {
            jet::Interpreter::RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_ne!(exit_code, 0, "{tier} ignored the expired deadline");
                assert_eq!(stdout, "", "{tier} continued after the expired deadline");
                assert!(stderr.contains("E3003"), "{tier}: {stderr}");
            }
            jet::Interpreter::RunOutcome::Problems(diagnostics) => {
                assert!(
                    diagnostics.iter().any(|diagnostic| diagnostic.code == "E3003"),
                    "{tier} reported the wrong deadline error: {diagnostics:?}"
                );
            }
        }
        if !force_interpreter {
            assert!(
                jet_jit::jit_executed_for_test(),
                "combinator program must execute native resident JIT code"
            );
            assert!(
                !jet_jit::fallback_invoked_for_test(),
                "combinator program resident JIT must not invoke fallback"
            );
            assert!(
                !jet_jit::deopt_invoked_for_test(),
                "combinator program resident JIT must not deopt"
            );
        }
    }
}

/// c109 Phase 21: `Task.detach()` (D-DETACH1) — fire-and-forget; drops the handle.
#[test]
fn task_detach() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    handle :: task 42
    handle.detach()
    print(\"launched\")
}
";
    let (code, stdout) = build_and_run("tir_task_detach", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "launched\n");
}

/// c109 Phase 21 / D-TUPLE-DESTRUCT1: the full channel surface —
/// `tasks.channel<T>()` producer returning `(Sender<T>, Receiver<T>)`,
/// `sender.clone()` (a second sender), `Sender.send(v)` (inside a `task` body),
/// `Task.join()`, and `Receiver.receive() ?? panic(..)`
/// (`Result<T, Closed>` unwrap).
#[test]
fn channel_send_receive() {
    if !have_rustc() {
        return;
    }
    let src = r#"
use core.tasks as tasks
fn run() {
(s1, ch) :: tasks.channel<Int>()
    s2 :: ~s1
    t1 :: task {
        s1.send(30)
    }
    t2 :: task {
        s2.send(12)
    }
    t1.join() ?? panic("task failed")
    t2.join() ?? panic("task failed")
    results := [Int].{}
    results.push(ch.receive() ?? panic("channel closed"))
    results.push(ch.receive() ?? panic("channel closed"))
    results.sort()
    loop x, results {
        print(x)
    }
}
"#;
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
    task.group g {
        (sender, receiver) :: tasks.channel<Int>()
        sender.send(42)
        value :: if {
            received, receiver -> received
        }
        print(value)
    }
}
";
    let (code, stdout) = build_and_run("tir_taskgroup_select", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

/// c109 Phase 22: method-call-collection iteration — `loop c, s.chars()` (char
/// iteration) and `loop w, s.split(sep)` (the `.iter().cloned()` default), both
/// reproduced from `emit_for_in`'s `Expr::MethodCall` branches.
#[test]
fn method_call_collection_iteration() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn count_chars(s: String) => Int {
    n := 0
    loop c, s.chars() {
        n+= 1
    }
    return n
}
fn join_words(s: String) => String {
    out := \"\"
    loop w, s.split(\",\") {
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
fn describe(x: Int?) => String {
    if x == Val(n) {
        return \"got {n}\"
    }
    if x == None {
        return \"nothing\"
    }
    return \"?\"
}
fn first_even(xs: [Int]) => Int {
    out := [Int].{}
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
    print(describe(Val(7)))
    print(describe(None))
    print(first_even([1, 2, 3]))
}
";
    let (code, stdout) = build_and_run("tir_opt_if", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "got 7\nnothing\n3\n");
}

/// D-FLOWTYPE1=A: stable immutable Optional narrows after `!= None` / else of `== None`.
#[test]
fn optional_flow_narrowing_after_none_check() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn from_ne(x: Int?) => Int {
    if x != None {
        return x + 1
    }
    return 0
}
fn from_else(x: Int?) => Int {
    if x == None {
        return 0
    } else {
        return x + 2
    }
}
fn and_tail(x: Int?) => Int {
    if x != None && x > 0 {
        return x
    }
    return -1
}
fn still_binds(x: Int?) => Int {
    if x == Val(n) {
        return n * 10
    }
    return -2
}
fn text_ne(x: String?) => String {
    if x != None {
        return x
    }
    return \"?\"
}
fn run() {
    print(from_ne(Val(3)))
    print(from_ne(None))
    print(from_else(Val(5)))
    print(from_else(None))
    print(and_tail(Val(7)))
    print(and_tail(Val(0)))
    print(and_tail(None))
    print(still_binds(Val(4)))
    print(text_ne(Val(\"hi\")))
    print(text_ne(None))
}
";
    let (code, stdout) = build_and_run("tir_opt_flow", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "4\n0\n7\n0\n7\n-1\n-1\n40\nhi\n?\n");
}

/// D-FLOWTYPE1=A negatives: mutable / field / call subjects do not narrow;
/// facts end at the branch boundary and do not travel through `||`.
#[test]
fn optional_flow_narrowing_rejects_unstable_subjects() {
    let mutable = jet::compile(
        "\
fn run() {
    x := Val(1)
    if x != None {
        print(x + 1)
    }
}
",
    );
    assert!(
        mutable.is_err(),
        "mutable Optional must not narrow after != None"
    );

    let field = jet::compile(
        "\
struct Box {
    n: Int?
}
fn run() {
    b :: Box.{ n: Val(1) }
    if b.n != None {
        print(b.n + 1)
    }
}
",
    );
    assert!(
        field.is_err(),
        "field Optional must not narrow after != None"
    );

    let call = jet::compile(
        "\
fn get() => Int? { return Val(1) }
fn run() {
    if get() != None {
        print(get() + 1)
    }
}
",
    );
    assert!(call.is_err(), "call Optional must not narrow after != None");

    let after_branch = jet::compile(
        "\
fn run() {
    x :: Val(1)
    if x != None {
        print(x)
    }
    print(x + 1)
}
",
    );
    assert!(
        after_branch.is_err(),
        "Optional narrowing must end at the branch boundary"
    );

    let through_or = jet::compile(
        "\
fn run() {
    x :: Val(1)
    if x != None || true {
        print(x + 1)
    }
}
",
    );
    assert!(
        through_or.is_err(),
        "Optional narrowing must not travel through ||"
    );
}
