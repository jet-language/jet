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
    print(nums.map((n: Int) -> n * n).to_list().len())
    total := 0
    nums.each((n: Int) -> { total += n })
    print(total)
    print(apply((x: Int) -> x + 1, 5))
    print(nums.filter((n: Int) -> n > 1).to_list().len())
    print(nums.any((n: Int) -> n == 2))
    print(nums.reduce(0, (acc: Int, n: Int) -> acc + n))
}
"#;
    let out = jet::compile(src).expect("closures should compile");
    assert!(
        !common::strip_vetted_prelude_modules(&out.rust).contains("unsafe"),
        "invariant I1"
    );
    assert!(
        out.rust.contains("jet_list_map"),
        "map should lower to prelude helper"
    );
    assert!(out.rust.contains("move |"), "lambdas should emit closures");
}

#[test]
fn single_line_lambda_bodies_need_no_braces() {
    // S46: braces only for multi-statement bodies. One assignment or one void
    // call after `->` is the brace-free form of the same block.
    let src = r#"
struct Box {
    n: Int
}

fn bump(box: Shared<Box>) {
    slot :: box.guard_edit().map(b -> b.n)
    slot.value += 1
}

fn run() {
    box :: shared Box{n: 0}
    t :: task bump(box)
    t.join() ?? panic("task failed")
    print(box.n)
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
    seen := [Int]{}
    ignored :: [1, 2, 3].para_map((n: Int) -> { &seen.push(n) })
}
"#;
    let diags = jet::compile(mutable_capture).expect_err("mutable parallel capture must fail");
    assert_eq!(
        diags.iter().filter(|diag| diag.code == "E1101").count(),
        1,
        "{diags:#?}"
    );

    let borrowed_capture = r#"
fn run() {
    values := [1, 2, 3]
    window :: values[0..1]
    ignored :: [1, 2, 3].para_filter((n: Int) -> window.contains(n))
}
"#;
    let diags = jet::compile(borrowed_capture).expect_err("borrowed parallel capture must fail");
    assert!(diags.iter().any(|diag| diag.code == "E1102"), "{diags:#?}");

    let hidden_capture = r#"
fn run() {
    offset :: 1
    callback :: (n: Int) -> n + offset
    ignored :: [1, 2, 3].para_map(callback)
}
"#;
    let diags = jet::compile(hidden_capture).expect_err("stored parallel callback must fail");
    assert!(diags.iter().any(|diag| diag.code == "E1102"), "{diags:#?}");

    for (role, source) in [
        (
            "item",
            r#"fn bump(n: Int) -> Int { return n + 1 }
fn run() {
    callbacks :: [fn(Int) -> Int]{ bump }
    ignored :: callbacks.para_filter((callback: fn(Int) -> Int) -> true)
}
"#,
        ),
        (
            "result",
            r#"fn run() {
    ignored :: [1].para_map((n: Int) -> (x: Int) -> x + n)
}
"#,
        ),
        (
            "accumulator",
            r#"fn run() {
    ignored :: [1].para_fold(
        () -> (x: Int) -> x,
        (callback: fn(Int) -> Int, n: Int) -> callback,
        (left: fn(Int) -> Int, right: fn(Int) -> Int) -> left
    )
}
"#,
        ),
        (
            "enum payload",
            r#"alias Boxed<T> :: T
enum CallbackPayload { Callback(Boxed<fn(Int) -> Int>) }
fn bump(n: Int) -> Int { return n + 1 }
fn run() {
    payloads :: [CallbackPayload]{ CallbackPayload.Callback(bump) }
    ignored :: payloads.para_map((payload: CallbackPayload) -> 1)
}
"#,
        ),
        (
            "stored-function capture",
            r#"fn run() {
    stored :: (n: Int) -> n + 1
    ignored :: [1].para_map((n: Int) -> stored(n))
}
"#,
        ),
    ] {
        let diags =
            jet::compile(source).expect_err("function-typed worker values must fail in sema");
        assert!(
            diags.iter().any(|diag| diag.code == "E1102"),
            "function {role} must stop before rustc: {diags:#?}"
        );
    }
}

#[test]
fn legacy_parallel_adapter_spellings_are_removed() {
    for method in ["par_map", "par_filter", "par_partition", "par_fold"] {
        let source = format!("fn run() {{ ignored :: [1, 2, 3].{method}((n: Int) -> n) }}\n");
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
    loop x in xs {
        f(x)
    }
}

fn run() {
    run_each([1, 2, 3], (x) -> {
        print(x)
    })
}
"#;
    let out = jet::compile(src).expect("bare lambda to fn-typed param should compile");
    assert!(
        !common::strip_vetted_prelude_modules(&out.rust).contains("unsafe"),
        "invariant I1"
    );
    assert!(
        out.rust.contains("__jet_x: i64"),
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
    bump :: (x: Int) -> x + 1
    print(twice(bump, 10))
}
"#;
    let out = jet::compile(src).expect("stored fn value should compile");
    assert!(out.rust.contains("Box::new"), "stored lambdas should box");
}

#[test]
fn multiline_callable_tail_returns_the_declared_result() {
    let src = r#"
fn double(value: Int) -> Int {
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
    item :: NoClone{ label: 7 }
    f :: (n: Int) -> n + item.label
    values :: [1, 2, 3]
    g :: () -> values.len()
    print(values.len())
    print(f(1))
    print(g())
}
"#;
    jet::compile(src).expect("captures should copy cloneable values and move non-cloneable values");
}

#[test]
fn fn_field_callback() {
    let src = r#"
struct Worker { step: fn(Int) -> Int }
fn run() {
    w :: Worker{ step: (n: Int) -> n + 1 }
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
    &nums.sort_by((n: Int) -> n)
    print(nums[0])
}
"#;
    jet::compile(src).expect("sort_by lambda should compile");
}

// D-ITER1 / D-CORE-EAGER2: adapter receiver split.
#[test]
fn iter_adapters_compile() {
    let src = r#"
fn run() {
    nums := [1, 2, 3, 4, 5]
    print(&nums.take(3).to_list())
    print(nums.skip(2).to_list())
    print(nums.step_by(2).to_list())
    print(nums.dedup().to_list())
    print(nums.take_while((n: Int) -> (n < 4)).to_list())
    print(nums.skip_while((n: Int) -> (n < 4)).to_list())
    sum := nums.fold(0, (acc: Int, n: Int) -> (acc + n))
    print(sum)
    pos := nums.position((n: Int) -> (n == 3))
    print(pos)
    words := ["b", "a", "c"]
    print(words.min_by((w: String) -> w.len()))
    print(words.max_by((w: String) -> w.len()))
    nested := [[1, 2], [3, 4]]
    print(nested.flatten())
    print(nested.lazy().flatten().to_list())
    print(nested.flat_map((xs: [Int]) -> xs))
    print(nested.lazy().flat_map((xs: [Int]) -> xs).to_list())
}
"#;
    let out = jet::compile(src).expect("D-ITER1 adapters should compile");
    assert!(
        !common::strip_vetted_prelude_modules(&out.rust).contains("unsafe"),
        "invariant I1"
    );
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
        out.rust.contains("jet_view_fold"),
        "List.fold should lower to the borrowed view helper"
    );
    assert!(
        out.rust.contains("jet_iter_take_while"),
        "take_while should lower lazily"
    );
    assert!(
        out.rust.contains("jet_list_flat_map"),
        "concrete List.flat_map should lower eagerly"
    );
    assert!(
        out.rust.contains("jet_list_flatten"),
        "concrete List.flatten should lower eagerly"
    );
    assert!(
        out.rust.contains("jet_iter_flatten"),
        "`.lazy().flatten` should lower lazily"
    );
    assert!(
        out.rust.contains("jet_iter_flat_map"),
        "`.lazy().flat_map` should lower lazily"
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
    assert!(
        out.rust.contains("jet_iter_chunks"),
        "chunks should lower lazily"
    );
    assert!(
        out.rust.contains("jet_iter_windows"),
        "windows should lower lazily"
    );
}

/// Check `src` as a one-file program and return its sema errors.
fn sema_errors(tag: &str, src: &str) -> Vec<jet::Diagnostics::Diagnostic> {
    let dir = common::unique_tmp(tag);
    std::fs::create_dir_all(&dir).expect("create sema fixture directory");
    let entry = dir.join("main.jet");
    std::fs::write(&entry, src).expect("write sema fixture");
    let mut bundle =
        jet::Loader::load_entry(entry.to_str().unwrap()).expect("sema fixture should load");
    jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
        .collect()
}

/// D-FAILURE-FOUNDATION1: a callback with a plain expected result infers its
/// own failure carrier, so its `ret` row stays open for that carrier. An
/// explicit `return` still owes the expected success row — before and after
/// a fallible call widens the callback — exactly as the value tail does
/// (Examples/features/memory/local_cache.jet).
#[test]
fn block_callback_return_meets_expected_success_row() {
    let src = r#"
struct Tree {
    source: String
    line_starts: Cell<[Int]?>
}

fn line_starts(tree: Tree) -> [Int] {
    return tree.line_starts.get_or_set(() -> {
        if tree.source.len() == 0 {
            return [0]
        }
        print("index built")
        return [0, tree.source.len()]
    })
}

fn source_flag(value: Int) -> Bool {
    if value == 0 {
        return Err("source failed")
    }
    return value > 1
}

fn retain_flag(value: Bool) -> Bool { value }

fn run() {
    tree :: Tree{ source: "first\nsecond", line_starts: Cell.new(None) }
    print(line_starts(tree))
    kept :: [1, 2, 3].filter((value: Int) -> {
        if value == 3 {
            return true
        }
        if value == 2 {
            return retain_flag(source_flag(value))
        }
        retain_flag(source_flag(value))
    }) ?? []
    print(kept.len())
}
"#;
    let errors = sema_errors("callback_return_row", src);
    assert!(errors.is_empty(), "valid callback returns were rejected: {errors:#?}");
}

#[test]
fn block_callback_return_mismatch_names_expected_success_row() {
    let cell = r#"
struct Tree {
    source: String
    line_starts: Cell<[Int]?>
}

fn line_starts(tree: Tree) -> [Int] {
    return tree.line_starts.get_or_set(() -> {
        print("index built")
        return "built"
    })
}

fn run() {
    tree :: Tree{ source: "first", line_starts: Cell.new(None) }
    print(line_starts(tree))
}
"#;
    let errors = sema_errors("callback_return_mismatch", cell);
    assert!(
        errors
            .iter()
            .any(|diagnostic| diagnostic.code == "E0113" && diagnostic.what.contains("String")),
        "a wrong callback return must still be E0113: {errors:#?}"
    );

    let inferred = r#"
fn source_flag(value: Int) -> Bool {
    if value == 0 {
        return Err("source failed")
    }
    return value > 1
}

fn retain_flag(value: Bool) -> Bool { value }

fn run() {
    kept :: [1, 2, 3].filter((value: Int) -> {
        if value == 2 {
            return retain_flag(source_flag(value))
        }
        if value == 3 {
            return "kept"
        }
        retain_flag(source_flag(value))
    }) ?? []
    print(kept.len())
}
"#;
    let errors = sema_errors("callback_return_carrier_mismatch", inferred);
    assert!(
        errors
            .iter()
            .any(|diagnostic| diagnostic.code == "E0113" && diagnostic.what.contains("String")),
        "a wrong return after carrier inference must still be E0113: {errors:#?}"
    );
}

#[test]
fn member_namespaces_select_methods_and_explicit_callable_fields() {
    let source = include_str!("../Examples/features/callable/member_namespaces.jet");
    jet::compile(source).expect("same-named fields and methods must lower through their checked routes");
}

#[test]
fn member_namespaces_keep_within_kind_duplicates() {
    for (tag, source, text) in [
        ("namespace_duplicate_field", "struct Worker { step: Int step: Int }\nfn run() {}", "field `step`"),
        ("namespace_duplicate_method", "struct Worker { step: Int fn step(self) {} fn step(self) {} }\nfn run() {}", "method `step`"),
    ] {
        let errors = sema_errors(tag, source);
        assert!(errors.iter().any(|diagnostic| diagnostic.code == "E0105" && diagnostic.what.contains(text)), "{errors:#?}");
    }
}

#[test]
fn member_namespaces_wrong_method_arguments_never_call_the_field() {
    let source = r#"
struct Worker {
    step: fn() -> Int
    fn step(self, amount: Int) -> Int Never! { amount }
}
fn run() {
    worker :: Worker{step: () -> 1}
    print(worker.step())
}
"#;
    let errors = sema_errors("namespace_wrong_method_args", source);
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "E0766" && diagnostic.what.contains("amount")), "{errors:#?}");
    assert!(!errors.iter().any(|diagnostic| diagnostic.code == "E0105"), "{errors:#?}");
}

#[test]
fn member_namespaces_keep_generic_trait_default_and_operator_routes() {
    let source = include_str!("../Compiler/JetSema/Tests/MemberNamespaceFixtures.jet");
    jet::compile(source).expect("namespace selection must preserve established generic, trait and operator lowering");
}

#[test]
fn member_namespaces_private_method_never_calls_public_field() {
    let dir = common::unique_tmp("namespace_private_method");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("worker.jet"), "struct Worker { step: fn(Int) -> Int fn step(self, amount: Int) -> Int Never! { amount } }\n").unwrap();
    let entry = dir.join("run.jet");
    std::fs::write(&entry, "use worker.[Worker]\nfn run() { worker :: Worker{step: n -> n}\nprint(worker.step(1)) }\n").unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let errors = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "E0605" && diagnostic.what.contains("step")), "{errors:#?}");
    assert!(!errors.iter().any(|diagnostic| diagnostic.code == "E0105"), "{errors:#?}");
}

#[test]
fn member_namespace_callable_field_normalization_is_idempotent() {
    let dir = common::unique_tmp("namespace_recheck");
    std::fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("run.jet");
    std::fs::write(&entry, include_str!("../Examples/features/callable/member_namespaces.jet")).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    for _ in 0..2 {
        let errors = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
            .into_iter()
            .filter(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
            .collect::<Vec<_>>();
        assert!(errors.is_empty(), "{errors:#?}");
    }
}

#[test]
fn member_namespaces_preserve_method_receiver_and_effect_checks() {
    let receiver = r#"
struct Worker {
    step: fn(Int) -> Int
    fn step(&self, amount: Int) -> Int Never! { amount }
}
fn run() {
    worker := Worker{step: n -> n}
    print(worker.step(1))
}
"#;
    let errors = sema_errors("namespace_method_receiver", receiver);
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "E0224"), "{errors:#?}");

    let effects = r#"
struct Worker {
    step: fn(Int) -> Int
    fn step(self, amount: Int) -> Int {
        print(amount)
        amount
    }
}
fn pure(worker: Worker) -[]> Int { worker.step(1) }
fn run() {}
"#;
    let errors = sema_errors("namespace_method_effect", effects);
    assert!(errors.iter().any(|diagnostic| diagnostic.code == "E0740"), "{errors:#?}");
}
