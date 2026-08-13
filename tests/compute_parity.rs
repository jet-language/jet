//! Focused CPU-oracle laws for the production `core.compute` Prelude path.

mod common;

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
    compute.set(&tensor, [1], 9.0) ?? panic("set")
    print("edited:{compute.to_list(tensor)}")

    wire :: compute.serialize(product) ?? panic("wire")
    round :: compute.deserialize(wire) ?? panic("round")
    print("round:{compute.to_list(round)}")

    bad :: compute.deserialize("shape=2;data=1")
    if bad == {
        .Ok(_) -> { print("corrupt:accepted") }
        .Err(_) -> { print("corrupt:rejected") }
    }

    bad_axis :: compute.deserialize("shape=02;data=1.0,1.0")
    if bad_axis == {
        .Ok(_) -> { print("axis:accepted") }
        .Err(_) -> { print("axis:rejected") }
    }

    bad_field :: compute.deserialize("shape=1;data=1.0;data=1.0")
    if bad_field == {
        .Ok(_) -> { print("field:accepted") }
        .Err(_) -> { print("field:rejected") }
    }

    bad_checksum :: compute.deserialize("shape=1;data=1.0;profile=F64Strict+Reproducible;checksum=0000000000000000")
    if bad_checksum == {
        .Ok(_) -> { print("checksum:accepted") }
        .Err(_) -> { print("checksum:rejected") }
    }

    mse_left :: compute.full([2], 1.0) ?? panic("mse_left")
    mse_right :: compute.full([3], 1.0) ?? panic("mse_right")
    bad_loss :: compute.mse_loss(mse_left, mse_right)
    if bad_loss == {
        .Ok(_) -> { print("mse_shape:accepted") }
        .Err(_) -> { print("mse_shape:rejected") }
    }

    bad_lr :: compute.sgd_step(mse_left, mse_left, -1.0)
    if bad_lr == {
        .Ok(_) -> { print("negative_lr:accepted") }
        .Err(_) -> { print("negative_lr:rejected") }
    }

    bounds :: compute.kernel_bounds_ok([2, 3], [2, 0])
    if bounds == {
        .Ok(_) -> { print("bounds:accepted") }
        .Err(_) -> { print("bounds:rejected") }
    }

    empty :: compute.full([0, 3], 1.0) ?? panic("empty")
    print("empty:{compute.shape(empty)}:{compute.to_list(empty)}")
    empty_other :: compute.full([1, 3], 2.0) ?? panic("empty_other")
    empty_broadcast :: compute.add(empty, empty_other)
    if empty_broadcast == {
        .Ok(value) -> { print("empty_broadcast:{compute.shape(value)}:{compute.to_list(value)}") }
        .Err(_) -> { print("empty_broadcast:rejected") }
    }

    left_shape :: compute.full([2, 2], 1.0) ?? panic("left_shape")
    right_shape :: compute.full([3], 1.0) ?? panic("right_shape")
    incompatible :: compute.add(left_shape, right_shape)
    if incompatible == {
        .Ok(_) -> { print("broadcast:accepted") }
        .Err(_) -> { print("broadcast:rejected") }
    }

    overflow :: compute.full([9223372036854775807, 2], 1.0)
    if overflow == {
        .Ok(_) -> { print("overflow:accepted") }
        .Err(_) -> { print("overflow:rejected") }
    }

    bad_get :: compute.get(tensor, [4])
    if bad_get == {
        .Ok(_) -> { print("tensor_bounds:accepted") }
        .Err(_) -> { print("tensor_bounds:rejected") }
    }
}
"#;

#[test]
fn compute_cpu_oracle_aot_covers_storage_algebra_and_corruption() {
    if !have_rustc() {
        eprintln!("SKIP compute_cpu_oracle_aot_covers_storage_algebra_and_corruption: rustc is unavailable");
        return;
    }
    let (code, stdout) = build_and_run("compute_cpu_oracle", SOURCE);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "sum:[5.0, 7.0, 9.0]\nproduct:[2.0, 2.0, 2.0, 2.0]\nedited:[1.0, 9.0, 3.0, 4.0]\nround:[2.0, 2.0, 2.0, 2.0]\ncorrupt:rejected\naxis:rejected\nfield:rejected\nchecksum:rejected\nmse_shape:rejected\nnegative_lr:rejected\nbounds:rejected\nempty:[0, 3]:[]\nempty_broadcast:[0, 3]:[]\nbroadcast:rejected\noverflow:rejected\ntensor_bounds:rejected\n"
    );
}

#[test]
fn compute_cpu_oracle_default_run_matches_aot_meaning() {
    let (code, stdout, stderr) = run_default_multi("compute_cpu_oracle_jit", "main.jet", &[("main.jet", SOURCE)]);
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(
        stdout,
        "sum:[5.0, 7.0, 9.0]\nproduct:[2.0, 2.0, 2.0, 2.0]\nedited:[1.0, 9.0, 3.0, 4.0]\nround:[2.0, 2.0, 2.0, 2.0]\ncorrupt:rejected\naxis:rejected\nfield:rejected\nchecksum:rejected\nmse_shape:rejected\nnegative_lr:rejected\nbounds:rejected\nempty:[0, 3]:[]\nempty_broadcast:[0, 3]:[]\nbroadcast:rejected\noverflow:rejected\ntensor_bounds:rejected\n"
    );
}

#[test]
fn compute_fixed_vec_and_matrix_aliases_keep_shape_facts_on_the_tensor_substrate() {
    let source = r#"
use core.compute as compute

fn vec_rank(value: Vec<3>) => Int {
    return compute.rank(value)
}

fn matrix_rank(value: Matrix<2, 3>) => Int {
    return compute.rank(value)
}

fn run() {
    vector :: compute.vec(3, 1.0) ?? panic("vec")
    matrix :: compute.matrix(2, 3, 2.0) ?? panic("matrix")
    print("vec:{vec_rank(vector)}")
    print("matrix:{matrix_rank(matrix)}")
}
"#;
    if have_rustc() {
        let (code, stdout) = build_and_run("compute_fixed_aliases", source);
        assert_eq!(code, 0);
        assert_eq!(stdout, "vec:1\nmatrix:2\n");
    } else {
        eprintln!("SKIP compute_fixed_vec_and_matrix_aliases_keep_shape_facts_on_the_tensor_substrate AOT leg: rustc is unavailable");
    }
    let (code, stdout, stderr) = run_default_multi(
        "compute_fixed_aliases_default",
        "main.jet",
        &[("main.jet", source)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, "vec:1\nmatrix:2\n");
}

#[test]
fn compute_broadcast_ufunc_fuses_indexing_and_arithmetic() {
    let source = r#"
use core.compute as compute

fn run() {
    rows :: compute.full([2, 1], 3.0) ?? panic("rows")
    columns :: compute.from_list([1.0, 2.0, 4.0]) ?? panic("columns")
    fused :: compute.add(rows, columns) ?? panic("fused")
    print("shape:{compute.shape(fused)}")
    print("values:{compute.to_list(fused)}")
}
"#;
    if have_rustc() {
        let (code, stdout) = build_and_run("compute_fused_broadcast", source);
        assert_eq!(code, 0);
        assert_eq!(stdout, "shape:[2, 3]\nvalues:[4.0, 5.0, 7.0, 4.0, 5.0, 7.0]\n");
    } else {
        eprintln!("SKIP compute_broadcast_ufunc_fuses_indexing_and_arithmetic AOT leg: rustc is unavailable");
    }
    let (code, stdout, stderr) = run_default_multi(
        "compute_fused_broadcast_default",
        "main.jet",
        &[("main.jet", source)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, "shape:[2, 3]\nvalues:[4.0, 5.0, 7.0, 4.0, 5.0, 7.0]\n");
}
