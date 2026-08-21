mod tir_support;

#[test]
fn rational_math_crosses_to_float_once() {
    tir_support::assert_tiers_agree(
        "exact_rational_math",
        r#"
use core.math as math

fn run() {
    print(math.sqrt(1 / 3))
}
"#,
        "0.5773502691896257\n",
    );
}
