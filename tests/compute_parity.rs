//! Focused CPU-oracle laws for the production `core.compute` Prelude path.

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, have_rustc, run_default_multi};

const SOURCE: &str = r#"
use core.compute as compute

fn run() {
    a :: compute.from_list([1.0, 2.0, 3.0]) ?? panic("a")
    b :: compute.from_list([4.0, 5.0, 6.0]) ?? panic("b")
    sum :: compute.add(a, b) ?? panic("sum")
    print("sum:{compute.to_list(sum)}")

    matrix :: compute.full([2, 2], 2.0) ?? panic("matrix")
    identity :: compute.eye(2) ?? panic("eye")
    product :: compute.matmul(matrix, identity) ?? panic("matmul")
    print("product:{compute.to_list(product)}")

    tensor := compute.from_list([1.0, 2.0, 3.0, 4.0]) ?? panic("tensor")
    edit :: &tensor[1..2]
    edit[0] = 9.0
    print("edited:{compute.to_list(tensor)}")

    wire :: compute.serialize(product)
    round :: compute.deserialize(wire) ?? panic("round")
    print("round:{compute.to_list(round)}")

    bad :: compute.deserialize("shape=2;data=1")
    if bad == {
        .Ok(_) -> { print("corrupt:accepted") }
        .Err(_) -> { print("corrupt:rejected") }
    }

    bounds :: compute.kernel_bounds_ok([2, 3], [2, 0])
    if bounds == {
        .Ok(_) -> { print("bounds:accepted") }
        .Err(_) -> { print("bounds:rejected") }
    }
}
"#;

#[test]
fn compute_cpu_oracle_aot_covers_storage_views_algebra_and_corruption() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("compute_cpu_oracle", SOURCE);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "sum:[5.0, 7.0, 9.0]\nproduct:[2.0, 2.0, 2.0, 2.0]\nedited:[1.0, 9.0, 3.0, 4.0]\nround:[2.0, 2.0, 2.0, 2.0]\ncorrupt:rejected\nbounds:rejected\n"
    );
}

#[test]
fn compute_cpu_oracle_default_run_matches_aot_meaning() {
    let (code, stdout, stderr) = run_default_multi("compute_cpu_oracle_jit", "main.jet", &[("main.jet", SOURCE)]);
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(
        stdout,
        "sum:[5.0, 7.0, 9.0]\nproduct:[2.0, 2.0, 2.0, 2.0]\nedited:[1.0, 9.0, 3.0, 4.0]\nround:[2.0, 2.0, 2.0, 2.0]\ncorrupt:rejected\nbounds:rejected\n"
    );
}
