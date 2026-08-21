mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

#[test]
fn exact_zero_sqrt_keeps_zero_uncertainty() {
    tir_support::assert_tiers_agree(
        "measurement_sqrt_zero",
        r#"
use core.math as math

fn run() {
    zero :: measurement(0.0, uncertainty: 0.0)
    print(math.sqrt(zero))
}
"#,
        "0.0 ± 0.0\n",
    );
}
