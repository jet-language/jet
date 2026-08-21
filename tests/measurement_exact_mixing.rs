#[path = "tir_support/mod.rs"]
mod tir_support;
mod common;

#[test]
fn exact_values_enter_measurement_with_zero_uncertainty() {
    let source = r#"
fn run() {
    measured :: measurement(12.0, uncertainty: 0.1)
    exact :: 2
    print(measured + exact)
    print(exact + measured)
}
"#;
    tir_support::assert_tiers_agree(
        "measurement_exact_mixing",
        source,
        "14.0 ± 0.1\n14.0 ± 0.1\n",
    );
}
