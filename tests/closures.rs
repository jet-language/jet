//! M8 closure compile checks (rustc-as-verifier battery subset).

mod common;

#[test]
fn lambdas_compile_to_rust() {
    let src = r#"
fn apply(f: fn(Int) => Int, x: Int) => Int {
    return f(f(x))
}

fn run() {
    nums :: [1, 2, 3]
    print(nums.map((n: Int) => n * n).to_list().len())
    total := 0
    nums.each((n: Int) => { total += n })
    print(total)
    print(apply((x: Int) => x + 1, 5))
    print(nums.filter((n: Int) => n > 1).to_list().len())
    print(nums.any((n: Int) => n == 2))
    print(nums.reduce(0, (acc: Int, n: Int) => acc + n))
}
"#;
    let out = jet::compile(src).expect("closures should compile");
    assert!(!common::strip_vetted_prelude_modules(&out.rust).contains("unsafe"), "invariant I1");
    assert!(
        out.rust.contains("jet_list_map"),
        "map should lower to prelude helper"
    );
    assert!(out.rust.contains("move |"), "lambdas should emit closures");
}

#[test]
fn single_line_lambda_bodies_need_no_braces() {
    // S46: braces only for multi-statement bodies. One assignment or one void
    // call after `=>` is the brace-free form of the same block.
    let src = r#"
use core.tasks as tasks

struct Box {
    n: Int
}

fn bump(box: Shared<Box>) {
    box.edit(b => b.n += 1)
}

fn run() {
    box :: Shared.new(Box.{n: 0})
    t :: tasks.spawn(() => bump(box))
    t.wait()
    print(box.read(b => b.n))
}
"#;
    let out = jet::compile(src).expect("brace-free single-statement lambdas should compile");
    assert!(
        out.rust.contains("move |"),
        "brace-free lambdas should still emit closures"
    );
}

#[test]
fn parallel_adapters_reject_unsafe_boundaries_before_codegen() {
    let mutable_capture = r#"
fn run() {
    seen := [Int].{}
    ignored :: [1, 2, 3].para_map((n: Int) => { seen.push(n) })
}
"#;
    let diags = jet::compile(mutable_capture).expect_err("mutable parallel capture must fail");
    assert_eq!(
        diags.iter().filter(|diag| diag.code == "E1111").count(),
        1,
        "{diags:#?}"
    );

    let borrowed_capture = r#"
fn run() {
    values := [1, 2, 3]
    window :: values[0..1]
    ignored :: [1, 2, 3].para_filter((n: Int) => window.contains(n))
}
"#;
    let diags = jet::compile(borrowed_capture).expect_err("borrowed parallel capture must fail");
    assert!(diags.iter().any(|diag| diag.code == "E1111"), "{diags:#?}");

    let hidden_capture = r#"
fn run() {
    offset :: 1
    callback :: (n: Int) => n + offset
    ignored :: [1, 2, 3].para_map(callback)
}
"#;
    let diags = jet::compile(hidden_capture).expect_err("stored parallel callback must fail");
    assert!(diags.iter().any(|diag| diag.code == "E1111"), "{diags:#?}");

    for (role, source) in [
        (
            "item",
            r#"fn bump(n: Int) => Int { return n + 1 }
fn run() {
    callbacks :: [fn(Int) => Int].{ bump }
    ignored :: callbacks.para_filter((callback: fn(Int) => Int) => true)
}
"#,
        ),
        (
            "result",
            r#"fn run() {
    ignored :: [1].para_map((n: Int) => (x: Int) => x + n)
}
"#,
        ),
        (
            "accumulator",
            r#"fn run() {
    ignored :: [1].para_fold(
        () => (x: Int) => x,
        (callback: fn(Int) => Int, n: Int) => callback,
        (left: fn(Int) => Int, right: fn(Int) => Int) => left
    )
}
"#,
        ),
        (
            "enum payload",
            r#"alias Boxed<T> = T
enum CallbackPayload { Callback(Boxed<fn(Int) => Int>) }
fn bump(n: Int) => Int { return n + 1 }
fn run() {
    payloads :: [CallbackPayload].{ CallbackPayload.Callback(bump) }
    ignored :: payloads.para_map((payload: CallbackPayload) => 1)
}
"#,
        ),
        (
            "stored-function capture",
            r#"fn run() {
    stored :: (n: Int) => n + 1
    ignored :: [1].para_map((n: Int) => stored(n))
}
"#,
        ),
    ] {
        let diags = jet::compile(source)
            .expect_err("function-typed worker values must fail in sema");
        assert!(
            diags.iter().any(|diag| diag.code == "E1111"),
            "function {role} must stop before rustc: {diags:#?}"
        );
    }
}

#[test]
fn legacy_parallel_adapter_spellings_are_removed() {
    for method in ["par_map", "par_filter", "par_partition", "par_fold"] {
        let source = format!("fn run() {{ ignored :: [1, 2, 3].{method}((n: Int) => n) }}\n");
        let diags = jet::compile(&source).expect_err("legacy parallel spelling must fail");
        assert!(
            diags.iter().any(|diag| diag.code == "E0311"),
            "{method} unexpectedly remained available: {diags:#?}"
        );
    }
}

#[test]
fn bare_lambda_to_fn_typed_param_emits_param_type() {
    // c142: a bare lambda (no param annotation) passed to a user fn-typed param
    // used to ICE — codegen emitted `move |__jet_x| …` with no Rust type, so
    // rustc couldn't infer it. Sema now elaborates the param type from the
    // expected fn-type back onto the AST so codegen emits it.
    let src = r#"
fn run_each(xs: [Int], f: fn(Int)) {
    loop x, xs {
        f(x)
    }
}

fn run() {
    run_each([1, 2, 3], (x) => {
        print(x)
    })
}
"#;
    let out = jet::compile(src).expect("bare lambda to fn-typed param should compile");
    assert!(!common::strip_vetted_prelude_modules(&out.rust).contains("unsafe"), "invariant I1");
    assert!(
        out.rust.contains("__jet_x: i64"),
        "bare lambda param must get its type from the fn-typed slot, got:\n{}",
        out.rust
    );
}

#[test]
fn stored_callback_boxes() {
    let src = r#"
fn twice(f: fn(Int) => Int, x: Int) => Int {
    return f(f(x))
}

fn run() {
    bump :: (x: Int) => x + 1
    print(twice(bump, 10))
}
"#;
    let out = jet::compile(src).expect("stored fn value should compile");
    assert!(out.rust.contains("Box::new"), "stored lambdas should box");
}

#[test]
fn multiline_callable_tail_returns_the_declared_result() {
    let src = r#"
fn double(value: Int) => Int {
    adjusted :: value + 1
    adjusted * 2
}

fn run() {
    print(double(4))
}
"#;
    let out = jet::compile(src).expect("multiline callable tail should return");
    assert!(
        out.rust.contains("return (__jet_adjusted).jet_mul"),
        "the final expression must lower as the function result:\n{}",
        out.rust
    );
}

#[test]
fn implicit_capture_copies_cloneable_and_moves_non_cloneable_values() {
    let src = r#"
struct NoClone { label: Int }
fn run() {
    item :: NoClone.{ label: 7 }
    f :: (n: Int) => n + item.label
    values :: [1, 2, 3]
    g :: () => values.len()
    print(values.len())
    print(f(1))
    print(g())
}
"#;
    jet::compile(src)
        .expect("captures should copy cloneable values and move non-cloneable values");
}

#[test]
fn fn_field_callback() {
    let src = r#"
struct Worker { step: fn(Int) => Int }
fn run() {
    w :: Worker.{ step: (n: Int) => n + 1 }
    print(w.step(4))
}
"#;
    let out = jet::compile(src).expect("fn field callback should compile");
    assert!(out.rust.contains("Box::new"), "fn fields should box");
}

#[test]
fn sort_by_with_lambda() {
    let src = r#"
fn run() {
    nums := [3, 1, 2]
    nums.sort_by((n: Int) => n)
    print(nums[0])
}
"#;
    jet::compile(src).expect("sort_by lambda should compile");
}

// D-ITER1: lazy iterator adapter set.
#[test]
fn iter_adapters_compile() {
    let src = r#"
fn run() {
    nums := [1, 2, 3, 4, 5]
    print(nums.take(3).to_list())
    print(nums.skip(2).to_list())
    print(nums.step_by(2).to_list())
    print(nums.dedup().to_list())
    print(nums.take_while((n: Int) => (n < 4)).to_list())
    print(nums.skip_while((n: Int) => (n < 4)).to_list())
    sum := nums.fold(0, (acc: Int, n: Int) => (acc + n))
    print(sum)
    pos := nums.position((n: Int) => (n == 3))
    print(pos)
    words := ["b", "a", "c"]
    print(words.min_by((w: String) => w.len()))
    print(words.max_by((w: String) => w.len()))
    nested := [[1, 2], [3, 4]]
    print(nested.flat_map((xs: [Int]) => xs).to_list())
}
"#;
    let out = jet::compile(src).expect("D-ITER1 adapters should compile");
    assert!(!common::strip_vetted_prelude_modules(&out.rust).contains("unsafe"), "invariant I1");
    assert!(
        out.rust.contains("jet_iter_take"),
        "take should lower to lazy jet_iter_take"
    );
    assert!(
        out.rust.contains("jet_iter_from_vec"),
        "list→Iter adapters should start from jet_iter_from_vec"
    );
    assert!(
        !out.rust.contains("jet_iter_from_vec(jet_list_take"),
        "adapters must not eagerly collect then re-wrap"
    );
    assert!(
        out.rust.contains("jet_iter_skip("),
        "skip should lower to lazy helper"
    );
    assert!(
        out.rust.contains("jet_list_fold"),
        "fold should lower to helper"
    );
    assert!(
        out.rust.contains("jet_iter_take_while"),
        "take_while should lower lazily"
    );
    assert!(
        out.rust.contains("jet_iter_flat_map"),
        "flat_map should lower lazily"
    );
}

#[test]
fn iter_chunks_windows() {
    let src = r#"
fn run() {
    nums := [1, 2, 3, 4, 5, 6]
    print(nums.chunks(2).len())
    print(nums.windows(3).len())
}
"#;
    let out = jet::compile(src).expect("chunks/windows should compile");
    assert!(out.rust.contains("jet_iter_chunks"), "chunks should lower lazily");
    assert!(
        out.rust.contains("jet_iter_windows"),
        "windows should lower lazily"
    );
}
