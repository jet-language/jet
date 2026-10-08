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
fn rng_owned_int_bounds_and_machine_counts_reach_native_aot() {
    if !tir_support::have_rustc() {
        eprintln!("note: skipping RNG native ABI regression (need rustc)");
        return;
    }
    let (code, stdout, stderr) = tir_support::build_and_run_full(
        "core_rng_owned_abi",
        "rng_owned_int_bounds",
        r#"
use core.math.random as random

fn run() {
    rng := random.rng(7)
    print(rng.int(-9223372036854775808, -9223372036854775808))
    print(rng.int(9223372036854775807, 9223372036854775807))
    print(rng.bytes(0).len())
    print(rng.sample([1, 2, 3], 0).len())
    deck := [1, 2, 3]
    rng.shuffle(&deck)
    print(deck.len())
    child := rng.split()
    print(child.int(1, 1))
    print(rng.float() >= Float{0.0})
    print(rng.bool_p(Float{1.0}))
}
"#,
    );
    assert_eq!(code, 0, "RNG native ABI failed: {stderr}");
    assert_eq!(stdout, "-9223372036854775808\n9223372036854775807\n0\n0\n3\n1\ntrue\ntrue\n");
}

#[test]
fn rng_reversed_bounds_keep_runtime_stop_exit_in_native_aot() {
    if !tir_support::have_rustc() {
        eprintln!("note: skipping reversed RNG native ABI regression (need rustc)");
        return;
    }
    let (code, stdout, stderr) = tir_support::build_and_run_full(
        "core_rng_owned_abi",
        "rng_reversed_bounds",
        r#"
use core.math.random as random

fn run() {
    rng := random.rng(9)
    print(rng.int(9, 3))
}
"#,
    );
    assert_eq!(code, 70, "reversed RNG range exit drifted: {stderr}");
    assert!(stdout.is_empty(), "the failed draw must not print a value: {stdout}");
    assert!(stderr.starts_with("Stop [E3010]:"), "{stderr}");
}

#[test]
fn rng_out_of_host_range_keeps_runtime_stop_exit_in_native_aot() {
    if !tir_support::have_rustc() {
        eprintln!("note: skipping out-of-range RNG native ABI regression (need rustc)");
        return;
    }
    let (code, stdout, stderr) = tir_support::build_and_run_full(
        "core_rng_owned_abi",
        "rng_out_of_host_range",
        r#"
use core.math.random as random

fn run() {
    rng := random.rng(9)
    print(rng.int(9223372036854775808, 9223372036854775808))
}
"#,
    );
    // D-FAIL-EXIT1: runtime E1003 is a Stop, not an entry error return.
    // This also exercises the shared Core boundary's range-stop classification.
    assert_eq!(code, 70, "out-of-range RNG exit drifted: {stderr}");
    assert!(stdout.is_empty(), "the failed draw must not print a value: {stdout}");
    assert!(stderr.contains("E1003"), "{stderr}");
}
