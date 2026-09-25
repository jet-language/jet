//! String's primitive identity stays compiler-owned while its receiver method
//! body is carried by a compiler-private Core source part.
mod common;
mod tir_support;

#[test]
fn string_source_part_is_not_a_public_core_module() {
    let source = include_str!("../Core/text/string.jet");
    assert!(source.contains("fn len(receiver: String) -> Int"));
    assert!(!source.contains("pub fn"));

    let public_core = include_str!("../crates/jet-codegen/src/Prelude/Core.jet");
    assert!(!public_core.contains("core.text.string"));

    let public_exports = include_str!("../crates/jet-foundation/src/CoreModuleExports.rs");
    assert!(!public_exports.contains("core.text.string"));
}

#[test]
fn private_string_source_path_is_not_importable() {
    let diagnostics = jet::compile(
        "use core.text.string as string\nfn run() { print(string.len(\"x\")) }\n",
    )
    .expect_err("the compiler-private String source part must not be a public import");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E1001"),
        "private source path should be rejected as an unknown Core module: {diagnostics:?}"
    );
}

#[test]
fn string_method_calls_target_the_private_jet_source_function() {
    let source = r#"
use core.text as text

fn run() {
    value :: "hello"
    print(value.len())
}
"#;
    let output = jet::compile(source).expect("String source method must compile");
    assert!(
        !output.rust.contains("impl String"),
        "String methods must not be emitted as an inherent Rust impl"
    );
    assert!(
        !output.rust.contains("impl &'static str"),
        "String methods must not be attached to the primitive Rust view"
    );
    assert!(
        !output.rust.contains("core.text.string"),
        "the private source part must not leak a public Core import path"
    );
}

#[test]
fn string_len_source_agrees_across_execution_tiers() {
    tir_support::assert_tiers_agree(
        "string_len_source",
        r#"
use core.text as text

fn run() {
    print("hello".len())
}
"#,
        "5\n",
    );
}
