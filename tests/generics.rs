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
        pub fn total(first: T, second: U) -> Int { return count + extra }
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
    @Meta(category: label)
    const value = count
    comptime comptime_value = count + 1
    tag Marked;
    trait Reveal { fn reveal(self) -> T }
    struct Wrapped { value: T }
    enum Maybe { Empty Value(T) }
    impl Wrapped.Reveal { fn reveal(self) -> T { return self.value } }
    enum SourceErr { Bad(T) }
    enum TargetErr { Wrapped(SourceErr) }
    impl SourceErr -> TargetErr { return TargetErr.Wrapped(self) }
    @Target(Os.Linux)
    impl Wrapped { fn linux_value(self) -> T { return self.value } }
    module plain { pub fn value() -> Int { return count } }
    module nested<U> { pub fn keep(value: U) -> U { return ~value } }
    module nested_use = nested<T>
    @Meta(category: label)
    pub fn marked(value: @Marked T) -> @Marked T {
        @Meta(category: label)
        local: T := value
        return ~local
    }
    @Test fn identity(value: T) { expect(count == count) }
    @Bench("complete") { expect(label == label) }
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
        "pub module boxed<T, n: Int> { pub fn value() -> Int { return n } }\n",
    )
    .expect("write left template");
    std::fs::write(
        root.join("right.jet"),
        "pub module boxed<T, n: Int> { pub fn value() -> Int { return n } }\n",
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
        pub fn captured() -> Int { return count }
    }
    module inner<U, extra: Int> {
        pub fn total(value: U) -> Int { return count + extra }
    }
    module closed = inner<T, count>
    module forwarded = closed
    pub fn result(value: T) -> Int {
        return plain.captured() + closed.total(value) + forwarded.total(value)
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
                .any(|application| application.name == "selected_closed")
        })
        .expect("nested generic-module instance fact");
    assert_eq!(
        nested
            .applications
            .iter()
            .map(|application| application.name.as_str())
            .collect::<Vec<_>>(),
        vec!["selected_closed", "selected_forwarded"]
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
    assert_eq!(String::from_utf8_lossy(&output.stdout), "15\n");
    let _ = std::fs::remove_dir_all(root);
}

fn assert_closed_value_identity() {
    let source = r#"
enum Mode { Fast Safe }
module keyed<flag: Bool, count: Int, letter: Char, label: String, mode: Mode> {
    pub fn value() -> Int { return count }
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
fn twice<T>(x: T) -> Pair<T> {{
    return Pair<T>.{{ first: x, second: x }}
}}

struct Pair<T> {{
    first: T
    second: T
}}

fn run() {{
    p :: twice({lit})
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
fn twice<T>(x: T) -> Pair<T> {
    return Pair<T>.{ first: x, second: x }
}

struct Pair<T> {
    first: T
    second: T
}

fn run() {
    a :: twice(1)
    b :: twice(2.5)
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

fn wrap<Kind>(x: Kind) -> Wrap<Kind> {
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
fn identity<Elem>(x: Elem) -> Elem {
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
fn twice<Elem>(x: Elem) -> Pair<Elem> {
    return Pair<Elem>.{ first: x, second: x }
}

struct Pair<Elem> {
    first: Elem
    second: Elem
}

fn run() {
    p :: twice(1)
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
