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
    print("interpolated {third}")
    print(third * 3 == 1)
}
"#,
        "1/3\ninterpolated 1/3\ntrue\n",
    );
}

#[test]
fn comptime_division_keeps_the_same_exact_value_as_runtime() {
    tir_support::assert_tiers_agree(
        "comptime_exact_division",
        r#"
@ten :: 10
@third :: @ten / 3
fn run() {
    runtime :: 10 / 3
    print(@third)
    print(@third == runtime)
    print(@third * 3 == 10)
}
"#,
        "10/3\ntrue\ntrue\n",
    );
}
