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
module Outer<T, count: Int> {
    module Inner<U, extra: Int> {
        pub fn total(first: T, second: U) -> Int { return count + extra }
    }
    module Closed = Inner<T, count>
}
module Use = Outer<String, 3>
fn run() {}
"#;
    jet::compile(nested)
        .unwrap_or_else(|diags| panic!("nested generic-module instantiation failed: {diags:#?}"));

    let body_items = r#"
module Complete<T, count: Int, label: String> {
    @Meta(category: label)
    const VALUE = count
    comptime COMPTIME_VALUE = count + 1
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
    module Plain { pub fn value() -> Int { return count } }
    module Nested<U> { pub fn keep(value: U) -> U { return ~value } }
    module NestedUse = Nested<T>
    @Meta(category: label)
    pub fn marked(value: @Marked T) -> @Marked T {
        @Meta(category: label)
        local: T := value
        return ~local
    }
    @Test fn identity(value: T) { expect(count == count) }
    @Bench("complete") { expect(label == label) }
}
module CompleteUse = Complete<Int, 3, "generic module">
fn run() {}
"#;
    jet::compile(body_items)
        .unwrap_or_else(|diags| panic!("generic-module body substitution failed: {diags:#?}"));

    let root = std::env::temp_dir().join(format!(
        "jet_generic_modules_complete_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create generic-module acceptance directory");
    std::fs::write(
        root.join("left.jet"),
        "pub module Boxed<T, n: Int> { pub fn value() -> Int { return n } }\n",
    )
    .expect("write left template");
    std::fs::write(
        root.join("right.jet"),
        "pub module Boxed<T, n: Int> { pub fn value() -> Int { return n } }\n",
    )
    .expect("write right template");
    let main = root.join("main.jet");
    std::fs::write(
        &main,
        r#"
module left
module right
use left.{Boxed as LeftBoxed}
use right.{Boxed as RightBoxed}
module First = LeftBoxed<Int, 3>
module Equivalent = LeftBoxed<Int, 3>
module DifferentType = LeftBoxed<String, 3>
module DifferentValue = LeftBoxed<Int, 4>
module DifferentPath = RightBoxed<Int, 3>
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
                .any(|application| application.name == "First")
        })
        .expect("canonical First instance");
    assert_eq!(
        shared
            .applications
            .iter()
            .map(|application| application.name.as_str())
            .collect::<Vec<_>>(),
        vec!["First", "Equivalent"]
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
