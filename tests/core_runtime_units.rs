//! Core runtime units (`Core/<area>/runtime/<unit>.jet`) are compiler-private
//! source parts: only Core sources import them, and a public member whose
//! owned wrapper calls a unit runs that Jet body on every tier.
mod common;
mod tir_support;

#[test]
fn runtime_units_come_from_the_directory() {
    let unit = jet_foundation::CoreSourceParts::core_runtime_part(
        "core.text.runtime.kernel_unicode_string",
    )
    .expect("the text runtime unit is generated from Core/text/runtime");
    assert_eq!(unit.path, "Core/text/runtime/kernel_unicode_string.jet");
    assert_eq!(unit.alias, "__core_text_runtime_kernel_unicode_string");
    assert!(!jet_foundation::Syntax::is_known_core_module(unit.module));
}

#[test]
fn user_program_cannot_import_a_runtime_unit() {
    for source in [
        "use core.text.runtime.kernel_unicode_string as rt\nfn run() { print(rt.lower(\"A\")) }\n",
        "use core.text.runtime.kernel_unicode_string.[lower]\nfn run() { print(lower(\"A\")) }\n",
    ] {
        let diagnostics = jet::compile(source).expect_err("a runtime unit is private to Core");
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "E1001")
            .unwrap_or_else(|| panic!("expected E1001: {diagnostics:?}"));
        assert!(
            diagnostic.what.contains("is private to Core"),
            "E1001 must name the Core-private unit: {diagnostic:?}"
        );
        assert!(diagnostic.fix.contains("`core.text`"), "{diagnostic:?}");
    }
}

#[test]
fn case_mapping_runs_the_runtime_unit_on_every_tier() {
    tir_support::assert_tiers_agree(
        "core_text_case_runtime_unit",
        r#"
use core.text as text

fn run() {
    print(text.lower("ÀB"))
    print(text.upper("straße"))
    print(text.casefold("Straße"))
    decomposed :: String.from_bytes([U8]{101, 204, 129}) ?? ""
    print(text.nfc(decomposed).bytes().len())
    print(text.nfd(text.nfc(decomposed)) == decomposed)
}
"#,
        "àb\nSTRASSE\nstrasse\n2\ntrue\n",
    );
}
