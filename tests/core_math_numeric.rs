//! Numeric CoreLib boundary coverage for the source-owned `core.math` API.

mod common;
mod tir_support;

#[test]
fn core_math_constants_and_rounding_match_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_math_numeric_boundaries",
        r#"
use core.math as math

fn run() {
    print(math.pi())
    print(math.round(1.4))
    print(math.round(1.5))
    print(math.round(-1.4))
    print(math.round(-1.5))
    print(math.round(0.5))
    print(math.round(-0.5))
}
"#,
        "3.141592653589793\n1\n2\n-1\n-2\n1\n-1\n",
    );
}

#[test]
fn core_math_exact_int_crosses_native_width_on_every_tier() {
    tir_support::assert_tiers_agree(
        "core_math_exact_int_boundaries",
        r#"
use core.math as math

fn run() {
    print(math.checked_abs(-9223372036854775808) ?? -1)
    print(math.checked_neg(-9223372036854775808) ?? -1)
    print(math.checked_add(9223372036854775807, 1) ?? -1)
    print(math.checked_sub(-9223372036854775808, 1) ?? 0)
    print(math.checked_mul(9223372036854775808, 2) ?? -1)
    print(math.checked_div(-9223372036854775808, -1) ?? -1)
    print(math.checked_rem(-9223372036854775808, -1) ?? -1)
    print(math.checked_pow(2, 100) ?? -1)
    print(math.saturating_add(9223372036854775807, 1))
    print(math.saturating_mul(9223372036854775808, 2))
    print(math.int_pow(2, 100))
    print(math.sum_int([Int]{9223372036854775807, 1}))
    print(math.prod_int([Int]{9223372036854775808, 2}))
    print(math.abs_diff(-9223372036854775808, 9223372036854775808))
    print(math.checked_div(1, 0) == None)
    print(math.checked_pow(2, -1) == None)
    print(math.checked_div(-7, 3) ?? 0)
    print(math.checked_rem(-7, 3) ?? 0)
    floored :: math.div_mod(-7, 3)
    print(floored.quot)
    print(floored.rem)
    euclidean :: math.div_mod(-7, -3)
    print(euclidean.quot)
    print(euclidean.rem)
    truncated :: math.div_rem(-7, 3)
    print(truncated.quot)
    print(truncated.rem)
    wide :: math.div_mod(18446744073709551616, 3)
    print(wide.quot)
}
"#,
        "9223372036854775808\n9223372036854775808\n9223372036854775808\n-9223372036854775809\n18446744073709551616\n9223372036854775808\n0\n1267650600228229401496703205376\n9223372036854775808\n18446744073709551616\n1267650600228229401496703205376\n9223372036854775808\n18446744073709551616\n18446744073709551616\ntrue\ntrue\n-2\n-1\n-3\n2\n3\n2\n-2\n-1\n6148914691236517205\n",
    );
}

#[test]
fn core_math_scalar_predicates_and_comparison_match_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_math_scalar_predicates",
        r#"
use core.math as math

fn run() {
    print(math.cmp(math.nan(), 1.0))
    print(math.cmp(1.0, math.nan()))
    print(math.cmp(math.nan(), math.nan()))
    print(math.cmp(-0.0, 0.0))
    print(math.cmp(-2.0, -1.0))
    print(math.cmp(math.infinity(), 1.0))
    print(math.is_finite(math.nan()))
    print(math.is_finite(math.infinity()))
    print(math.is_finite(math.from_bits(1)))
    print(math.is_finite(-0.0))
    print(math.to_bits(math.fabs(-0.0)))
    print(math.fabs(-2.0))
}
"#,
        "1\n-1\n0\n0\n-1\n1\nfalse\nfalse\ntrue\ntrue\n0\n2\n",
    );
}

#[test]
fn core_math_ambient_transition_and_bit_projection_match_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_math_ambient_transition",
        r#"
use core.math as math
use core.math.random as random

fn run() {
    random.seed(1)
    print(random.__word())
    random.seed(1)
    print(random.getrandbits(63))
    random.seed(1)
    print(math.random())
    random.seed(0)
    print(random.__word())
    print(random.getrandbits(0))
}
"#,
        "13399429869638026901\n6699714934819013450\n0.7263845487364354\n0\n0\n",
    );
}
