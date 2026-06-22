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
