//! M9 generic instantiation soundness — examples must compile under rustc.

use std::process::Command;

#[test]
fn generic_modules_complete_instantiation() {
    // CAPABILITY_CLAIM: claim.generic-modules / complete-instantiation
    let complete = include_str!("../examples/features/modules/generic_modules.jet");
    let compiled = jet::compile(complete)
        .unwrap_or_else(|diags| panic!("closed generic-module surface failed: {diags:#?}"));
    assert_eq!(
        compiled.rust.matches("// jet:generic-instance").count(),
        5,
        "closed Bool/Int/Char/String/enum values, bounds, layout, and body items must each reach codegen once"
    );

    let nested = r#"
module outer<T, count: Int> {
    module inner<U, extra: Int> {
        pub fn total(first: T, second: U) => Int { return count + extra }
    }
    module closed = inner<T, count>
}
module instance = outer<String, 3>
fn run() {}
"#;
    jet::compile(nested)
        .unwrap_or_else(|diags| panic!("nested generic-module instantiation failed: {diags:#?}"));

    let body_items = r#"
module complete<T, count: Int, label: String> {
    $label :: label
    #Meta(category: $label)
    $value :: count
    $comptime_value :: count + 1
    tag Marked { deny: [Net] }
    trait Reveal { fn reveal(self) => T }
    struct Wrapped { value: T }
    enum Maybe { Empty Value(T) }
    impl Wrapped.Reveal { fn reveal(self) => T { return self.value } }
    enum SourceErr { Bad(T) }
    enum TargetErr { Wrapped(SourceErr) }
    impl SourceErr => TargetErr { return TargetErr.Wrapped(self) }
    #Target(OS.Linux)
    impl Wrapped { fn linux_value(self) => T { return self.value } }
    module plain { pub fn value() => Int { return count } }
    module nested<U> { pub fn keep(value: U) => U { return ~value } }
    module nested_use = nested<T>
    #Meta(category: $label)
    pub fn marked(value: #Marked T) => #Marked T {
        #Meta(category: $label)
        local := T.{ value }
        return ~local
    }
    #Test fn identity(value: T) { expect(count == count) }
    #Bench("complete") { expect($label == $label) }
}
module complete_use = complete<Int, 3, "generic module">
fn run() {}
"#;
    jet::compile(body_items)
        .unwrap_or_else(|diags| panic!("generic-module body substitution failed: {diags:#?}"));

    let distinct_generated_names = r#"
module with_bar_baz<T> { pub struct BarBaz { value: T } }
module with_baz<T> { pub struct Baz { value: T } }
module foo = with_bar_baz<Int>
module foo_bar = with_baz<Int>
fn run() {
    first :: M3FooBarBaz.{ value: 1 }
    second :: M3Foo3BarBaz.{ value: 2 }
    print(first.value + second.value)
}
"#;
    jet::compile(distinct_generated_names)
        .unwrap_or_else(|diags| panic!("generic-module type names collided: {diags:#?}"));

    let root = std::env::temp_dir().join(format!(
        "jet_generic_modules_complete_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create generic-module acceptance directory");
    std::fs::write(
        root.join("left.jet"),
        "pub module boxed<T, n: Int> { pub fn value() => Int { return n } }\n",
    )
    .expect("write left template");
    std::fs::write(
        root.join("right.jet"),
        "pub module boxed<T, n: Int> { pub fn value() => Int { return n } }\n",
    )
    .expect("write right template");
    let main = root.join("main.jet");
    std::fs::write(
        &main,
        r#"
module left
module right
use left.{boxed as left_boxed}
use right.{boxed as right_boxed}
module first = left_boxed<Int, 3>
module equivalent = left_boxed<Int, 3>
module different_type = left_boxed<String, 3>
module different_value = left_boxed<Int, 4>
module different_path = right_boxed<Int, 3>
fn run() {}
"#,
    )
    .expect("write generic-module identity program");

    let index = jet_semindex::open(&main).expect("generic-module identity program checks");
    assert_eq!(
        index.instances().len(),
        4,
        "only complete full keys may reuse an instance"
    );
    let shared = index
        .instances()
        .iter()
        .find(|instance| {
            instance
                .applications
                .iter()
                .any(|application| application.name == "first")
        })
        .expect("canonical first instance");
    assert_eq!(
        shared
            .applications
            .iter()
            .map(|application| application.name.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "equivalent"]
    );
    let full_keys = index
        .instances()
        .iter()
        .map(|instance| instance.full_key_hex.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        full_keys.len(),
        4,
        "type, value, and defining module path remain identity inputs"
    );
    let _ = std::fs::remove_dir_all(root);
    assert_nested_generic_module_execution();
    assert_closed_value_identity();
}

fn assert_nested_generic_module_execution() {
    let source = r#"
module outer<T, count: Int> {
    module plain {
        module inner<U> {
            pub fn total(value: U) => Int { return count }
        }
        module closed = inner<T>
        module forwarded = closed
        pub fn result(value: T) => Int {
            return closed.total(value) + forwarded.total(value)
        }
    }
    pub fn result(value: T) => Int {
        return plain.result(value)
    }
}
module selected = outer<Int, 3>
fn run() {
    print(selected.result(1))
}
"#;
    jet::compile(source).unwrap_or_else(|diags| {
        panic!("nested ordinary-module generic template was dropped: {diags:#?}")
    });
    let root = std::env::temp_dir().join(format!(
        "jet_generic_modules_nested_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create nested generic-module directory");
    let main = root.join("main.jet");
    std::fs::write(&main, source).expect("write nested generic-module program");
    let index = jet_semindex::open(&main).expect("nested generic modules should index");
    assert_eq!(
        index.instances().len(),
        2,
        "outer and nested applications each have one applicative identity"
    );
    let nested = index
        .instances()
        .iter()
        .find(|instance| {
            instance
                .applications
                .iter()
                .any(|application| application.name == "selected_plain_closed")
        })
        .expect("nested generic-module instance fact");
    assert_eq!(
        nested
            .applications
            .iter()
            .map(|application| application.name.as_str())
            .collect::<Vec<_>>(),
        vec!["selected_plain_closed", "selected_plain_forwarded"]
    );
    assert!(nested
        .applications
        .iter()
        .all(|application| application.module_path == main.to_string_lossy()));
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .arg("run")
        .arg(&main)
        .output()
        .expect("run nested generic-module program");
    assert!(
        output.status.success(),
        "nested generic-module AOT run failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "6\n");
    let _ = std::fs::remove_dir_all(root);
}

fn assert_closed_value_identity() {
    let source = r#"
enum Mode { Fast Safe }
module keyed<flag: Bool, count: Int, letter: Char, label: String, mode: Mode> {
    pub fn value() => Int { return count }
}
module same = keyed<true, 3, 'a', "x", Mode.Fast>
module equivalent = keyed<1 < 2, 1 + 2, 'a', "x", Mode.Fast>
module different_bool = keyed<false, 3, 'a', "x", Mode.Fast>
module different_int = keyed<true, 4, 'a', "x", Mode.Fast>
module different_char = keyed<true, 3, 'b', "x", Mode.Fast>
module different_string = keyed<true, 3, 'a', "y", Mode.Fast>
module different_enum = keyed<true, 3, 'a', "x", Mode.Safe>
fn run() {}
"#;
    let root = std::env::temp_dir().join(format!(
        "jet_generic_module_value_identity_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create value-identity directory");
    let main = root.join("main.jet");
    std::fs::write(&main, source).expect("write value-identity program");
    let index = jet_semindex::open(&main).expect("closed values should index");
    assert_eq!(
        index.instances().len(),
        6,
        "normalized equivalents reuse one key; every closed value-kind change stays distinct"
    );
    let shared = index
        .instances()
        .iter()
        .find(|instance| {
            instance
                .applications
                .iter()
                .any(|application| application.name == "same")
        })
        .expect("shared closed-value instance");
    assert_eq!(
        shared
            .applications
            .iter()
            .map(|application| application.name.as_str())
            .collect::<Vec<_>>(),
        vec!["same", "equivalent"]
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn generic_examples_build_and_run() {
    for name in [
        "examples/features/types/traits.jet",
        "examples/features/types/generic_types.jet",
        "examples/features/basics/printable.jet",
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_jet"))
            .arg("run")
            .arg(name)
            .output()
            .unwrap_or_else(|e| panic!("failed to run {name}: {e}"));
        assert!(
            out.status.success(),
            "example {name} failed:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn generic_scalar_matrix() {
    let types = ["1", "2.5", "true", "\"hi\"", "'a'"];
    for lit in types {
        let src = format!(
            r#"
fn twice<T>(x: ^T, y: ^T) => Pair<T> {{
    return Pair<T>.{{ first: x, second: y }}
}}

struct Pair<T> {{
    first: T
    second: T
}}

fn run() {{
    p :: twice({lit}, {lit})
    print(p.first)
}}
"#
        );
        let diags = jet::compile(&src);
        assert!(diags.is_ok(), "scalar {lit} failed: {:?}", diags.err());
    }
}

#[test]
fn generic_fn_with_scalar_types() {
    let src = r#"
fn twice<T>(x: ^T, y: ^T) => Pair<T> {
    return Pair<T>.{ first: x, second: y }
}

struct Pair<T> {
    first: T
    second: T
}

fn run() {
    a :: twice(1, 1)
    b :: twice(2.5, 2.5)
    print(a.first)
    print(b.first)
}
"#;
    let diags = jet::compile(src);
    assert!(diags.is_ok(), "{:?}", diags.err());
}

/// c148: `is_type_var_name` only recognized single-char type params (`T`, `K`).
/// Multi-char names like `Kind`/`Elem` must compile identically to `T`.
#[test]
fn multi_char_type_param_struct() {
    // struct with multi-char type param used in a field
    let src = r#"
struct Wrap<Kind> {
    val: Kind
}

fn wrap<Kind>(x: ^Kind) => Wrap<Kind> {
    return Wrap<Kind>.{ val: x }
}

fn run() {
    b :: wrap(42)
    print(b.val)
}
"#;
    let diags = jet::compile(src);
    assert!(
        diags.is_ok(),
        "multi-char type param (struct) failed: {:?}",
        diags.err()
    );
}

/// c148: multi-char type param in a free function (no struct) must infer correctly.
#[test]
fn multi_char_type_param_fn_only() {
    let src = r#"
fn identity<Elem>(x: ^Elem) => Elem {
    return x
}

fn run() {
    n :: identity(7)
    s :: identity("hello")
    print(n)
    print(s)
}
"#;
    let diags = jet::compile(src);
    assert!(
        diags.is_ok(),
        "multi-char type param (fn) failed: {:?}",
        diags.err()
    );
}

/// c148: multi-char must work as a drop-in for single-char — same program, renamed param.
#[test]
fn multi_char_matches_single_char() {
    // identical to generic_scalar_matrix but using `Elem` instead of `T`
    let src = r#"
fn twice<Elem>(x: ^Elem, y: ^Elem) => Pair<Elem> {
    return Pair<Elem>.{ first: x, second: y }
}

struct Pair<Elem> {
    first: Elem
    second: Elem
}

fn run() {
    p :: twice(1, 1)
    print(p.first)
}
"#;
    let diags = jet::compile(src);
    assert!(
        diags.is_ok(),
        "multi-char matches single-char failed: {:?}",
        diags.err()
    );
}

/// D-GENERIC-CALL1=A: explicit type arguments are available on ordinary free
/// calls, including a result-only generic whose value arguments cannot infer T.
#[test]
fn explicit_generic_calls_cover_value_and_result_only_arguments() {
    let src = r#"
fn identity<T>(value: ^T) => T {
    return value
}

fn empty<T>() => [T] {
    ignored :: input()
    return []
}

fn run() {
    text :: identity<String>(input() ?? "ok")
    values :: empty<Int>()
    print(text)
    print(values.len())
}
"#;
    let compiled = jet::compile(src)
        .unwrap_or_else(|diags| panic!("explicit generic calls failed: {diags:#?}"));
    assert!(
        compiled.rust.contains("user_identity::<String>"),
        "AOT output lost the explicit identity type argument:\n{}",
        compiled.rust
    );
    assert!(
        compiled.rust.contains("user_empty::<i64>"),
        "AOT output lost the result-only type argument:\n{}",
        compiled.rust
    );
}

#[test]
fn generic_call_formatter_keeps_adjacent_angles() {
    let source = "fn run() {\n    value :: identity<Int>(1)\n    nested :: empty<Map<String, [Int]>>()\n    comparison :: 1 < 2\n    json.decode<Order>(text)\n}";
    let formatted = jet::format_source(source).expect("generic call syntax should format");
    assert!(formatted.contains("identity<Int>(1)"), "{formatted}");
    assert!(
        formatted.contains("empty<Map<String, [Int]>>()"),
        "nested generic call arguments must keep both closing angles: {formatted}"
    );
    assert!(
        formatted.contains("comparison :: 1 < 2"),
        "spaced angles must remain a comparison: {formatted}"
    );
    assert!(formatted.contains("json.decode<Order>(text)"), "{formatted}");
    assert_eq!(
        formatted,
        jet::format_source(&formatted).expect("formatted generic calls should reformat")
    );
}

#[test]
fn namespaced_generic_calls_support_explicit_and_inferred_arguments() {
    let source = r#"
module helpers {
    pub fn identity<T>(value: ^T) => T {
        return value
    }
}

fn run() {
    text :: helpers.identity<String>("ok")
    number :: helpers.identity(7)
    print(text)
    print(number)
}
"#;
    let compiled = jet::compile(source)
        .unwrap_or_else(|diags| panic!("namespaced generic call failed: {diags:#?}"));
    assert!(
        compiled.rust.contains("user_helpers__identity::<String>"),
        "AOT output lost the namespaced explicit type argument:\n{}",
        compiled.rust
    );
}

#[test]
fn generic_call_diagnostics_cover_arity_and_bound_failures() {
    let wrong_arity = r#"
fn identity<T>(value: ^T) => T { return value }
fn run() { value :: identity<Int, String>(1) }
"#;
    let arity_diags = jet::compile(wrong_arity).expect_err("wrong generic arity must fail");
    let arity = arity_diags
        .iter()
        .find(|diagnostic| diagnostic.code == "E0119")
        .unwrap_or_else(|| panic!("expected generic arity E0119: {arity_diags:#?}"));
    assert!(!arity.what.is_empty() && !arity.why.is_empty() && !arity.fix.is_empty());

    let wrong_bound = r#"
struct NotComparable { value: Int }
fn choose<T: Comparable>(value: ^T) => T { return value }
fn run() {
    value :: choose<NotComparable>(NotComparable.{ value: 1 })
}
"#;
    let bound_diags = jet::compile(wrong_bound).expect_err("failed generic bound must fail");
    let bound = bound_diags
        .iter()
        .find(|diagnostic| diagnostic.code == "E0905")
        .unwrap_or_else(|| panic!("expected generic bound E0905: {bound_diags:#?}"));
    assert!(!bound.what.is_empty() && !bound.why.is_empty() && !bound.fix.is_empty());

    let non_generic = r#"
fn plain(value: Int) => Int { return value }
fn run() { value :: plain<Int>(1) }
"#;
    let non_generic_diags = jet::compile(non_generic)
        .expect_err("non-generic functions must reject explicit call type arguments");
    assert!(
        non_generic_diags.iter().any(|diagnostic| diagnostic.code == "E0119"),
        "expected non-generic call E0119: {non_generic_diags:#?}"
    );

    let spaced = r#"
fn identity<T>(value: ^T) => T { return value }
fn run() { value :: identity < Int > (1) }
"#;
    assert!(
        jet::compile(spaced).is_err(),
        "spaced angle brackets must not become an explicit generic call"
    );

    let extra_decode_type = r#"
use core.encoding.json as json
fn run() { value :: json.decode<Int, String>("1") }
"#;
    let decode_diags = jet::compile(extra_decode_type)
        .expect_err("typed decode must reject extra call type arguments");
    assert!(
        decode_diags.iter().any(|diagnostic| diagnostic.code == "E0119"),
        "expected typed decode E0119: {decode_diags:#?}"
    );
}

#[test]
fn free_generic_calls_execute_in_the_resident_jit() {
    let source = r#"
fn identity<T>(value: ^T) => T { return value }
fn empty<T>() => [T] { return [] }
fn run() {
    text :: identity<String>("ok")
    values :: empty<Int>()
    print(text)
    print(values.len())
}
"#;
    let root = std::env::temp_dir().join(format!(
        "jet_generic_free_calls_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create generic-free test directory");
    let path = root.join("main.jet");
    std::fs::write(&path, source).expect("write generic-free test source");
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap())
        .expect("generic-free test source should load");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != jet::Diagnostics::Severity::Error),
        "generic-free test source should type-check: {diagnostics:?}"
    );
    jet_jit::try_compile_bundle(&bundle).expect("generic free calls should compile in resident JIT");
    jet_jit::reset_jit_trace_for_test();
    match jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "generic free call JIT run failed: {stderr}");
            assert_eq!(stderr, "");
            assert_eq!(stdout, "ok\n0\n");
        }
        other => panic!("generic free call JIT run did not complete: {other:?}"),
    }
    assert!(
        jet_jit::jit_executed_for_test(),
        "generic free call test did not execute resident JIT"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn generic_method_calls_keep_owner_and_method_arguments_separate() {
    let source = r#"
struct Box<T> {
    value: T
}

impl Box {
    fn new(value: ^T) => Box<T> {
        return Box<T>.{ value: value }
    }

    fn convert<U>(self, value: ^U) => U {
        return value
    }

    fn make<U>(value: ^U) => U {
        return value
    }
}

fn run() {
    box :: Box<Int>.new(3)
    value :: box.convert<String>("ok")
    inferred :: box.convert("again")
    static_value :: Box.make<String>("static")
    static_inferred :: Box.make("inferred")
    print(value)
    print(inferred)
    print(static_value)
    print(static_inferred)
}
"#;
    let compiled = jet::compile(source)
        .unwrap_or_else(|diags| panic!("generic method call failed: {diags:#?}"));
    assert!(compiled.rust.contains("<user_Box<i64>>::user_new"), "{}", compiled.rust);
    assert!(compiled.rust.contains(".user_convert::<String>"), "{}", compiled.rust);
    assert!(compiled.rust.contains("user_Box::user_make::<String>"), "{}", compiled.rust);

    let root = std::env::temp_dir().join(format!(
        "jet_generic_method_calls_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create generic-method test directory");
    let path = root.join("main.jet");
    std::fs::write(&path, source).expect("write generic-method test source");
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap())
        .expect("generic-method test source should load");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != jet::Diagnostics::Severity::Error),
        "generic-method test source should type-check: {diagnostics:?}"
    );
    jet_jit::try_compile_bundle(&bundle).expect("generic methods should compile in resident JIT");
    jet_jit::reset_jit_trace_for_test();
    match jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "generic method JIT run failed: {stderr}");
            assert_eq!(stderr, "");
            assert_eq!(stdout, "ok\nagain\nstatic\ninferred\n");
        }
        other => panic!("generic method JIT run did not complete: {other:?}"),
    }
    assert!(
        jet_jit::jit_executed_for_test(),
        "generic method test did not execute resident JIT"
    );
    let _ = std::fs::remove_dir_all(root);
}
