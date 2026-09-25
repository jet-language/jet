//! Source-owned `core.units` conversion and hostile-scale behavior across tiers.

mod common;
mod tir_support;

#[test]
fn core_units_conversion_rejects_nonfinite_and_zero_targets() {
    tir_support::assert_tiers_agree(
        "core_units_conversion_hostile_targets",
        r#"
use core.units as units

fn run() {
    measured :: measurement(12.0, uncertainty: 0.5)
    converted :: units.convert(measured, 1000.0)
    print(units.show(converted))

    // Invalid target scales keep the existing non-fallible measurement carrier
    // unchanged instead of manufacturing NaN or infinity.
    print(units.to_si(units.convert(measured, Float.NAN)) == 12.0)
    print(units.to_si(units.convert(measured, Float.INFINITY)) == 12.0)
    print(units.to_si(units.convert(measured, Float{0.0})) == 12.0)
    huge :: units.convert(measurement(Float.MAX, uncertainty: 1.0), 0.5)
    print(units.to_si(huge) == Float.MAX)
}
"#,
        "0.012 ± 0.0005\ntrue\ntrue\ntrue\ntrue\n",
    );
}
