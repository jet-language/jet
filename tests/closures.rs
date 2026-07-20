//! M8 closure compile checks (rustc-as-verifier battery subset).

mod common;

#[test]
fn lambdas_compile_to_rust() {
    let src = r#"
fn apply(f: fn(Int) -> Int, x: Int) -> Int {
    return f(f(x))
}

fn run() {
    nums :: [1, 2, 3]
    print(nums.map((n: Int) => n * n).len())
    total := 0
    nums.each((n: Int) => { total += n })
    print(total)
    print(apply((x: Int) => x + 1, 5))
    print(nums.filter((n: Int) => n > 1).len())
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
fn parallel_adapters_reject_unsafe_boundaries_before_codegen() {
    let mutable_capture = r#"
fn run() {
    seen: [Int] := []
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
    // used to ICE — codegen emitted `move |user_x| …` with no Rust type, so
    // rustc couldn't infer it. Sema now elaborates the param type from the
    // expected fn-type back onto the AST so codegen emits it.
    let src = r#"
fn run_each(xs: [Int], f: fn(Int)) {
    loop x; xs {
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
        out.rust.contains("user_x: i64"),
        "bare lambda param must get its type from the fn-typed slot, got:\n{}",
        out.rust
    );
}

#[test]
fn stored_callback_boxes() {
    let src = r#"
fn twice(f: fn(Int) -> Int, x: Int) -> Int {
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
fn take_prefix_moves_non_clone_capture() {
    let src = r#"
struct NoClone { label: Int }
fn run() {
    item :: NoClone.{ label: 7 }
    f :: take(item) (n: Int) => n + item.label
    print(f(1))
}
"#;
    jet::compile(src).expect("take-prefixed lambda should compile");
}

#[test]
fn fn_field_callback() {
    let src = r#"
struct Worker { step: fn(Int) -> Int }
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
    print(nums.take(3))
    print(nums.skip(2))
    print(nums.step_by(2))
    print(nums.dedup())
    print(nums.take_while((n: Int) => (n < 4)))
    print(nums.skip_while((n: Int) => (n < 4)))
    sum := nums.fold(0, (acc: Int, n: Int) => (acc + n))
    print(sum)
    pos := nums.position((n: Int) => (n == 3))
    print(pos)
    words := ["b", "a", "c"]
    print(words.min_by((w: String) => w.len()))
    print(words.max_by((w: String) => w.len()))
    nested := [[1, 2], [3, 4]]
    print(nested.flat_map((xs: [Int]) => xs))
}
"#;
    let out = jet::compile(src).expect("D-ITER1 adapters should compile");
    assert!(!common::strip_vetted_prelude_modules(&out.rust).contains("unsafe"), "invariant I1");
    assert!(
        out.rust.contains("jet_list_take"),
        "take should lower to helper"
    );
    assert!(
        out.rust.contains("jet_list_skip("),
        "skip should lower to helper"
    );
    assert!(
        out.rust.contains("jet_list_fold"),
        "fold should lower to helper"
    );
    assert!(
        out.rust.contains("jet_list_take_while"),
        "take_while should lower"
    );
    assert!(
        out.rust.contains("jet_list_flat_map"),
        "flat_map should lower"
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
    assert!(out.rust.contains("jet_list_chunks"), "chunks should lower");
    assert!(
        out.rust.contains("jet_list_windows"),
        "windows should lower"
    );
}
