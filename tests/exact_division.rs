mod common;
mod tir_support;

#[test]
fn exact_integer_division_is_fraction_and_multiplies_back() {
    tir_support::assert_tiers_agree(
        "exact_integer_division",
        r#"
fn run() {
    third :: 1 / 3
    print(third)
    print(third * 3 == 1)
}
"#,
        "1/3\ntrue\n",
    );
}
