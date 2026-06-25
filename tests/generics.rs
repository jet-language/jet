//! M9 generic instantiation soundness — examples must compile under rustc.

use std::process::Command;

#[test]
fn generic_examples_build_and_run() {
    for name in [
        "examples/features/25_traits.jet",
        "examples/features/26_generic_types.jet",
        "examples/features/27_printable.jet",
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
    return Pair<T> {{ first: x, second: x }}
}}

struct Pair<T> {{
    first: T
    second: T
}}

fn main() {{
    p @= twice({lit})
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
    return Pair<T> { first: x, second: x }
}

struct Pair<T> {
    first: T
    second: T
}

fn main() {
    a @= twice(1)
    b @= twice(2.5)
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
    return Wrap<Kind> { val: x }
}

fn main() {
    b @= wrap(42)
    print(b.val)
}
"#;
    let diags = jet::compile(src);
    assert!(diags.is_ok(), "multi-char type param (struct) failed: {:?}", diags.err());
}

/// c148: multi-char type param in a free function (no struct) must infer correctly.
#[test]
fn multi_char_type_param_fn_only() {
    let src = r#"
fn identity<Elem>(x: Elem) -> Elem {
    return x
}

fn main() {
    n @= identity(7)
    s @= identity("hello")
    print(n)
    print(s)
}
"#;
    let diags = jet::compile(src);
    assert!(diags.is_ok(), "multi-char type param (fn) failed: {:?}", diags.err());
}

/// c148: multi-char must work as a drop-in for single-char — same program, renamed param.
#[test]
fn multi_char_matches_single_char() {
    // identical to generic_scalar_matrix but using `Elem` instead of `T`
    let src = r#"
fn twice<Elem>(x: Elem) -> Pair<Elem> {
    return Pair<Elem> { first: x, second: x }
}

struct Pair<Elem> {
    first: Elem
    second: Elem
}

fn main() {
    p @= twice(1)
    print(p.first)
}
"#;
    let diags = jet::compile(src);
    assert!(diags.is_ok(), "multi-char matches single-char failed: {:?}", diags.err());
}
