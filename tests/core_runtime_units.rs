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

#[test]
fn grouped_format_is_source_owned_without_a_dispatcher_row() {
    assert!(jet_foundation::CoreModuleExports::core_source_owns_call(
        "core.text.fmt", "grouped", 2,
    ));
    assert!(jet_foundation::Syntax::core_call("core.text.fmt", "grouped").is_none());
    tir_support::assert_tiers_agree(
        "core_text_grouped_source",
        r#"
use core.text.fmt as fmt
use core.math as math

fn run() {
    price :: Float{1234.5678}
    print("{price:Grouped(2)}")
    exact :: 123456789012345678901234567890
    print("{exact:Grouped(2)}")
    count :: 255
    ratio :: Float{1.25}
    word :: "go"
    print("{count:Hex(4)} {count:Bin} {count:Oct}")
    print("{ratio:Sci(2)} {ratio:Percent(1)}")
    print("{word:Pad(4, ".")} {word:PadLeft(4, "0")}")
    print(fmt.sci(math.nan(), 9223372036854775807))
    print(fmt.sci(math.infinity(), -2))
    print(fmt.sci(-math.infinity(), 2))
}
"#,
        "1,234.57\n123,456,789,012,345,678,901,234,567,890.00\n00ff 11111111 377\n1.25e+00 125.0%\ngo.. 00go\nNaN\ninf\n-inf\n",
    );
}
