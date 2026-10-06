//! Core runtime units (`Core/<area>/runtime/<unit>.jet`) are compiler-private
//! source parts: only Core sources import them, and a public member whose
//! owned wrapper calls a unit runs that Jet body on every tier.
mod common;
mod tir_support;

#[test]
fn runtime_units_come_from_the_directory() {
    let unit = jet_foundation::CoreSourceParts::core_runtime_part(
        "core.math.runtime.top_math_lib_pure",
    )
    .expect("the math runtime unit is generated from Core/math/runtime");
    assert_eq!(unit.path, "Core/math/runtime/top_math_lib_pure.jet");
    assert_eq!(unit.alias, "__core_math_runtime_top_math_lib_pure");
    assert!(!jet_foundation::Syntax::is_known_core_module(unit.module));
}

#[test]
fn user_program_cannot_import_a_runtime_unit() {
    for source in [
        "use core.math.runtime.top_math_lib_pure as rt\nfn run() { print(rt.jet_std_math_digits(5)) }\n",
        "use core.math.runtime.top_math_lib_pure.[jet_std_math_digits]\nfn run() { print(jet_std_math_digits(5)) }\n",
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
        assert!(diagnostic.fix.contains("`core.math`"), "{diagnostic:?}");
    }
}

#[test]
fn digits_runs_the_runtime_unit_on_every_tier() {
    tir_support::assert_tiers_agree(
        "core_math_digits_runtime_unit",
        r#"
use core.math as math

fn run() {
    print(math.digits(0))
    print(math.digits(-7))
    print(math.digits(12345))
    print(math.digits(9223372036854775807))
    print(math.digits(-9223372036854775807 - 1))
}
"#,
        "1\n1\n5\n19\n19\n",
    );
}

#[test]
fn digits_folds_at_compile_time_through_the_same_unit() {
    tir_support::assert_tiers_agree(
        "core_math_digits_comptime",
        r#"
use core.math as math

WIDTH :: prep { math.digits(1000000) }

fn run() {
    print(WIDTH)
}
"#,
        "7\n",
    );
}
