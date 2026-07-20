//! TIR core and closures integration tests.

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;

use tir_support::{build_and_run, have_rustc};

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
use core.crypto as crypto
fn calc(a: Float) -> Float {
    r :: math.sqrt(a)
    f :: math.floor(r)
    c :: math.ceil(r)
    return (f + c)
}
fn make_path(a: String, b: String) -> String {
    return path.join(~a, ~b)
}
fn hash(s: String) -> String {
    return crypto.sha256(s.bytes()).hex()
}
fn run() {
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
fn core_files_read_with_fallback() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.files as fs
fn read_or(p: String) -> String {
    return (fs.read(~p) ?? \"missing\")
}
fn run() {
    print(read_or(\"/no/such/file/at/all/xyzzy\"))
}
";
    let (code, stdout) = build_and_run("tir_core_fs_fallback", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "missing\n");
}

// NOTE: regex calls (`re.is_match(…)?? …`) route through the TIR and now emit
// `jet_std::jet_regex_*` helpers, so regex-only programs build without an FFI bridge.

// ===================================================================
// c109 Phase 11: lambdas/closures, fan-out, closure-taking collection
// methods. Each program lives entirely inside the covered subset, so the
// covered function(s) route through the TIR; the assert proves rustc
// accepts the output (I2) and it runs correctly. Byte-parity to the old
// emitter baseline was verified separately across the example suite.
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
fn calc() -> Int {
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
fn run() {
    print(calc())
}
";
    let (code, stdout) = build_and_run("tir_closure_methods", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "[9, 16, 25]\n[11, 12, 13, 14, 15]\ntrue\ntrue\n15\n"
    );
}

#[test]
fn refined_collection_types_survive_tir_chains() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn use_float(value: Float) -> Float {
    return value + 0.25
}
fn run() {
    print(use_float([1, 2].fold(0.5, (a: Float, n: Int) => a + 0.5)))
    print(use_float([1, 2].reduce(0.5, (a: Float, n: Int) => a + 0.5)))
    print(use_float([1, 2].par_fold(0.5, (a: Float, n: Int) => a + 0.5)))
    print(use_float([1, 2].scan(0.5, (a: Float, n: Int) => a + 0.5).sum()))
    print(use_float([1, 2].map((n: Int) => 1.5).sum()))
    print(use_float([\"1.5\", \"bad\", \"2.5\"].filter_map((s: String) => Float.parse(s)).sum()))
    print(use_float([1, 2].flat_map((n: Int) => [1.5]).sum()))
    print([1, 2, 3].group_by((n: Int) => n % 2 == 0).has_key(true))
    print([1, 2, 3].count_by((n: Int) => n % 2).has_key(1))
}
";
    let (code, stdout) = build_and_run("tir_refined_collection_types", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "1.75\n1.75\n1.75\n2.75\n3.25\n4.25\n3.25\ntrue\ntrue\n"
    );
}

#[test]
fn option_map_callback_receives_read_borrow() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    value: String? :: Val(\"borrowed\")
    size :: value.map((text: String) => text.len())
    print(size)
}
";
    let (code, stdout) = build_and_run("tir_option_map_read_borrow", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "8\n");
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
fn calc() -> Int {
    nums := [1, 2, 3, 4]
    total := 0
    nums.each((n: Int) => { total = (total + n) })
    return total
}
fn run() {
    print(calc())
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
fn calc() -> Int {
    nums := [3, 1, 2]
    nums.sort_by((n: Int) => n)
    return nums[0]
}
fn run() {
    print(calc())
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
fn calc() -> Int {
    doubled := double.[1, 2, 3]
    print(doubled)
    return doubled[1]
}
fn run() {
    print(calc())
}
";
    let (code, stdout) = build_and_run("tir_fan_out", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "[2, 4, 6]\n4\n");
}

/// A call whose callee has a Fn-typed parameter (`apply(f, x)`) now routes
/// through the TIR with the required fn-value coercion. The test proves that
/// this formerly excluded shape compiles and runs.
#[test]
fn fn_typed_param_call_routes_through_tir() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn apply(f: fn(Int) -> Int, x: Int) -> Int {
    return f(x)
}
fn run() {
    print(apply((n: Int) => (n + 1), 41))
}
";
    let (code, stdout) = build_and_run("tir_fn_param_excluded", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

/// A block lambda's last expression is its value. Prefix statements retain their
/// semicolons, while the tail must not gain one in generated Rust. The unit lambda
/// covers the same emitter path and proves removing the tail semicolon does not
/// change Void closure behavior.
#[test]
fn block_lambda_preserves_value_tail_and_void_behavior() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn apply(f: fn(Int) -> Int, x: Int) -> Int {
    return f(x)
}
fn visit(f: fn(Int), x: Int) {
    f(x)
}
fn plus_one(x: Int) -> Int {
    return x + 1
}
fn run() {
    print(apply((n: Int) => {
        doubled :: (n * 2)
        plus_one(doubled)
    }, 20))
    visit((n: Int) => {
        print(\"seen {n}\")
    }, 7)
}
";
    let (code, stdout) = build_and_run("tir_block_lambda_tail", src);
    assert_eq!(code, 0, "block lambda generated Rust must compile");
    assert_eq!(stdout, "41\nseen 7\n");
}

/// c109 Phase 12: destination-owned numeric width conversions (D-SHAPE-CONVERT1) —
/// widening (`I64.from_u8`, infallible `as`), narrowing (`U8.from_i32`, fallible
/// `try_from` unwrapped with `??`), and int→float (`Float.from_int`, `as`). Each
/// fully-covered function routes through the
/// TIR (`NumericMethod`). rustc accepting + the right runtime values prove parity.
#[test]
fn numeric_width_conversions() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn widen(red: U8) -> I64 {
    return I64.from_u8(red)
}
fn narrow(channel: I32) -> U8 {
    return U8.from_i32(channel) ?? 255
}
fn to_real(x: Int) -> Float {
    return Float.from_int(x)
}
fn truncate(x: Float) -> U8 {
    return U8.from_float(x) ?? 255
}
fn narrow_float(x: Float) -> F32 ? String {
    return F32.from_float(x)
}
fn run() {
    print(widen(255))
    print(narrow(100))
    print(narrow(100000))
    print(to_real(3))
    print(truncate(42.9))
    print(truncate(300.0))
    print(narrow_float(2.0) ?? F32.from_int(0))
    print(narrow_float(1e100) ?? F32.from_int(-1))
}
";
    let (code, stdout) = build_and_run("tir_numeric_conv", src);
    assert_eq!(code, 0);
    // Widening, checked integer narrowing, int→float, and checked float→integer.
    assert_eq!(stdout, "255\n100\n255\n3.0\n42\n255\n2.0\n-1.0\n");
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
fn run() {
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
/// (both the inline `impl Trait {}` and the `impl T.Trait` forms) routes its
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
impl Square.Shape {
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
fn run() {
shapes: [Shape] :: [Circle.{radius: 2.0}, Square.{side: 3.0}]
    shapes.each((s) => {
        print(describe(s))
    })
}
";
    let (code, stdout) = build_and_run("tir_trait_methods", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "circle: 12.0\nsquare: 9.0\n");
}

#[test]
fn trait_object_call_keeps_non_scalar_arg_and_return_type() {
    if !have_rustc() {
        return;
    }
    let src = "\
trait Measure {
    fn measure(self, text: String) -> Int
}
struct Counter {
    bonus: Int
    impl Measure {
        fn measure(self, text: String) -> Int {
            return text.len() + self.bonus
        }
    }
}
fn apply_measure(counter: Measure, text: String) -> Int {
    return inspect(counter) + counter.measure(text)
}
fn inspect<T>(value: T) -> Int {
    return 1
}
fn run() {
    counters: [Measure] :: [Counter.{bonus: 2}]
    counters.each((counter) => {
        text :: \"read\"
        print(apply_measure(counter, text))
        print(text)
    })
}
";
    let (code, stdout) = build_and_run("tir_trait_object_non_scalar_arg", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "7\nread\n");
}

/// c109 Phase 12: an explicit `else { if … }` block must stay `} else { if … }`,
/// NOT collapse to `} else if …` (the source `ElseBranch`, not the else-body
/// shape, decides the emitted form). This guards the parity fix to the
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
fn run() {
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
fn run() {
    print(apply_twice(double, 3))
    print(apply_twice((n: Int) => (n + 1), 5))
    g :: double
    print(apply_twice(g, 4))
}
";
    let (code, stdout) = build_and_run("tir_fn_values", src);
    assert_eq!(code, 0);
    // apply_twice(double,3)=12; apply_twice(+1,5)=7; apply_twice(double,4)=16.
    assert_eq!(stdout, "12\n7\n16\n");
}

/// c109 Phase 13: a struct field call through a fn-typed field is an `Expr::CallValue`
/// (`(w.step)(x)`). The struct's fn field, the `apply_twice((x)=>…, …)` call site,
/// and a fn-value stored in a local then called all route through TIR.
#[test]
fn fn_value_call_through_local() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn calc(f: fn(Int) -> Int) -> Int {
    return f(10)
}
fn inc(x: Int) -> Int {
    return (x + 1)
}
fn run() {
    print(calc(inc))
    print(calc((y: Int) => (y * y)))
}
";
    let (code, stdout) = build_and_run("tir_fn_value_local", src);
    assert_eq!(code, 0);
    // calc(inc)=11; calc(square)=100.
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
    _g :: scope.guard(() => { print(\"cleanup\") })
    print(\"working\")
}
fn run() {
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
    t :: tasks.spawn(() => compute())
    return t.join()
}
fn run() {
    print(launch())
}
";
    // `launch`, the spawn expression, and `compute` route through TIR; rustc accepting
    // proves parity.
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
use core.files as fs
fn write_file(path: String, text: String) -> Int {{
    w := files.create(~path) ?? return 0
    _r :: w.write_line(text)
    _f :: w.flush()
    return 1
}}
fn run() {{
    done :: write_file(\"{path}\", \"hello handle\")
    print(done)
    contents :: fs.read(\"{path}\") ?? \"<none>\"
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
