mod common;
mod tir_support;

const SOURCE: &str = r#"
fn run() {
    small_left :: Decimal("1.20")
    small_right :: Decimal("2.30")
    print(small_left + small_right)
    print(small_right - small_left)
    print(Decimal("-1.20") + Decimal("0.20"))
    print(Decimal("9.5") * Decimal("2.0"))

    equal_left :: Decimal("1.0")
    equal_right :: Decimal("1.00")
    print(equal_left == equal_right)
    zero_left :: Decimal("0.00")
    zero_right :: Decimal("-0.0")
    print(zero_left == zero_right)

    print(Decimal("12.00").div(Decimal("4.00")))
    print(Decimal("1.00").div(Decimal("8.00")))

    big :: Decimal("170141183460469231731687303715884105727")
    print(big + Decimal("1"))
    print(big * Decimal("2"))
    print(Decimal("12345678901234567890123456789012345678901234567890.00"))
    big_scale_left :: Decimal("12345678901234567890123456789012345678901234567890.00")
    big_scale_right :: Decimal("12345678901234567890123456789012345678901234567890.0")
    print(big_scale_left == big_scale_right)
}
"#;

#[test]
fn decimal_small_and_big_paths_keep_exact_tier_behavior() {
    tir_support::assert_tiers_agree(
        "decimal_fast_path",
        SOURCE,
        concat!(
            "3.50\n",
            "1.10\n",
            "-1.00\n",
            "19.0\n",
            "true\n",
            "true\n",
            "3\n",
            "0.125\n",
            "170141183460469231731687303715884105728\n",
            "340282366920938463463374607431768211454\n",
            "12345678901234567890123456789012345678901234567890.00\n",
            "true\n",
        ),
    );
}

