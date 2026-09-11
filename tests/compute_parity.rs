//! Focused CPU-oracle laws for the production `core.compute` Prelude path.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{
    assert_tiers_agree, build_and_run, have_rustc, jit_run_traced, run_default_multi,
};

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

    compute.deserialize("shape=2;data=1") ? value -> { print("corrupt:accepted") } ! error -> { print("corrupt:rejected") }

    compute.deserialize("shape=02;data=1.0,1.0") ? value -> { print("axis:accepted") } ! error -> { print("axis:rejected") }

    compute.deserialize("shape=1;data=1.0;data=1.0") ? value -> { print("field:accepted") } ! error -> { print("field:rejected") }

    compute.deserialize("shape=1;data=1.0;profile=F64Strict+Reproducible;checksum=0000000000000000") ? value -> { print("checksum:accepted") } ! error -> { print("checksum:rejected") }

    mse_left :: compute.full([2], 1.0) ?? panic("mse_left")
    mse_right :: compute.full([3], 1.0) ?? panic("mse_right")
    compute.mse_loss(mse_left, mse_right) ? value -> { print("mse_shape:accepted") } ! error -> { print("mse_shape:rejected") }

    f32_seed :: compute.matrix(1, 1, 1.0) ?? panic("f32_seed")
    f32_tensor :: compute.matmul_f32_tile(f32_seed, f32_seed) ?? panic("f32_tensor")
    f64_target :: compute.full([1], 1.0) ?? panic("f64_target")
    compute.mse_loss(f32_tensor, f64_target) ? value -> { print("mse_profile:accepted") } ! error -> { print("mse_profile:rejected") }

    compute.sgd_step(mse_left, mse_left, -1.0) ? value -> { print("negative_lr:accepted") } ! error -> { print("negative_lr:rejected") }

    compute.kernel_bounds_ok([2, 3], [2, 0]) ? value -> { print("bounds:accepted") } ! error -> { print("bounds:rejected") }

    empty :: compute.full([0, 3], 1.0) ?? panic("empty")
    print("empty:{compute.shape(empty)}:{compute.to_list(empty)}")
    empty_other :: compute.full([1, 3], 2.0) ?? panic("empty_other")
    compute.add(empty, empty_other) ? value -> { print("empty_broadcast:{compute.shape(value)}:{compute.to_list(value)}") } ! error -> { print("empty_broadcast:rejected") }

    left_shape :: compute.full([2, 2], 1.0) ?? panic("left_shape")
    right_shape :: compute.full([3], 1.0) ?? panic("right_shape")
    compute.add(left_shape, right_shape) ? value -> { print("broadcast:accepted") } ! error -> { print("broadcast:rejected") }

    compute.full([9223372036854775807, 2], 1.0) ? value -> { print("overflow:accepted") } ! error -> { print("overflow:rejected") }

    compute.get(tensor, [4]) ? value -> { print("tensor_bounds:accepted") } ! error -> { print("tensor_bounds:rejected") }
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
        "sum:[5.0, 7.0, 9.0]\nproduct:[2.0, 2.0, 2.0, 2.0]\nedited:[1.0, 9.0, 3.0, 4.0]\nround:[2.0, 2.0, 2.0, 2.0]\ncorrupt:rejected\naxis:rejected\nfield:rejected\nchecksum:rejected\nmse_shape:rejected\nmse_profile:rejected\nnegative_lr:rejected\nbounds:rejected\nempty:[0, 3]:[]\nempty_broadcast:[0, 3]:[]\nbroadcast:rejected\noverflow:rejected\ntensor_bounds:rejected\n"
    );
}

#[test]
fn compute_cpu_oracle_default_run_matches_aot_meaning() {
    let (code, stdout, stderr) = run_default_multi(
        "compute_cpu_oracle_jit",
        "main.jet",
        &[("main.jet", SOURCE)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(
        stdout,
        "sum:[5.0, 7.0, 9.0]\nproduct:[2.0, 2.0, 2.0, 2.0]\nedited:[1.0, 9.0, 3.0, 4.0]\nround:[2.0, 2.0, 2.0, 2.0]\ncorrupt:rejected\naxis:rejected\nfield:rejected\nchecksum:rejected\nmse_shape:rejected\nmse_profile:rejected\nnegative_lr:rejected\nbounds:rejected\nempty:[0, 3]:[]\nempty_broadcast:[0, 3]:[]\nbroadcast:rejected\noverflow:rejected\ntensor_bounds:rejected\n"
    );
}

#[test]
fn compute_result_tensor_payload_survives_resident_return_cleanup() {
    let source = r#"
use core.compute as compute

fn make_tensor() Tensor !ComputeError -> {
    tensor :: compute.full([2], 3.0) ?? panic("tensor")
    return tensor
}

fn run() {
    tensor :: make_tensor() ?? panic("result")
    print("shape:{compute.shape(tensor)}")
    print("value:{compute.to_list(tensor)}")
}
"#;
    let expected = "shape:[2]\nvalue:[3.0, 3.0]\n";
    assert_tiers_agree("compute_result_tensor_payload", source, expected);

    let (code, stdout, stderr) = jit_run_traced("compute_result_tensor_payload_trace", source);
    assert_eq!(code, 0, "traced default jet run failed: {stderr}");
    assert_eq!(stdout, expected, "traced default output drifted: {stderr}");
    assert!(
        stderr.contains("tier1 native") && !stderr.contains("tier0 interp"),
        "Result-wrapped Tensor did not stay resident:\n{stderr}"
    );
}

#[test]
fn compute_tensor_in_record_has_parity_across_tiers() {
    let source = r#"
use core.compute as compute

struct TensorRecord {
    value: Tensor
}

fn run() {
    tensor :: compute.from_list([1.0, 2.0]) ?? panic("tensor")
    record :: TensorRecord{ value: tensor }
    print("wrapped:{compute.shape(record.value)}")
}
"#;
    assert_tiers_agree("compute_tensor_record", source, "wrapped:[2]\n");
}

#[test]
fn named_gradient_composes_into_second_derivative() {
    let source = r#"
use core.compute as compute

fn loss(w: Tensor, x: Tensor) Tensor -> compute.mul(w, x) ?? panic("loss")

fn run() {
    w :: compute.from_list([2.0]) ?? panic("w")
    x :: compute.from_list([4.0]) ?? panic("x")
    derivative :: compute.gradient(loss)
    curvature :: compute.gradient(derivative)
    result :: curvature(w, x)
    print("ww:{compute.to_list(result.w.w)}")
    print("wx:{compute.to_list(result.w.x)}")
    print("xw:{compute.to_list(result.x.w)}")
    print("xx:{compute.to_list(result.x.x)}")
}
"#;
    assert_tiers_agree(
        "compute_named_second_derivative",
        source,
        "ww:[0.0]\nwx:[1.0]\nxw:[1.0]\nxx:[0.0]\n",
    );
}

#[test]
fn compute_fixed_vec_and_matrix_aliases_keep_shape_facts_on_the_tensor_substrate() {
    let source = r#"
use core.compute as compute

fn vec_rank(value: Vec<3>) Int -[GPU]> {
    return compute.rank(value)
}

fn matrix_rank(value: Matrix<2, 3>) Int -[GPU]> {
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
fn matrix_shape_measures_match_or_diagnose_inner_sides() {
    let matching = r#"
use core.compute as compute

fn accept_3_by_2(value: Matrix<3, 2>) {}

fn run() {
    left :: compute.matrix(3, 4, 1.0) ?? panic("left")
    right :: compute.matrix(4, 2, 1.0) ?? panic("right")
    product :: left * right ?? panic("product")
    accept_3_by_2(product)
}
"#;
    jet::compile(matching).expect("3x4 multiplied by 4x2 must produce Matrix<3, 2>");

    let mismatched = r#"
use core.compute as compute

fn run() {
    left :: compute.matrix(3, 4, 1.0) ?? panic("left")
    right :: compute.matrix(5, 2, 1.0) ?? panic("right")
    product :: left * right
}
"#;
    let diagnostics =
        jet::compile(mismatched).expect_err("matrix inner measures 4 and 5 must not match");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E2512"
                && diagnostic.what.contains("inner sides")
                && diagnostic.what.contains('4')
                && diagnostic.what.contains('5')
        }),
        "the matrix mismatch must teach the differing inner measures: {diagnostics:?}"
    );
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
        assert_eq!(
            stdout,
            "shape:[2, 3]\nvalues:[4.0, 5.0, 7.0, 4.0, 5.0, 7.0]\n"
        );
    } else {
        eprintln!("SKIP compute_broadcast_ufunc_fuses_indexing_and_arithmetic AOT leg: rustc is unavailable");
    }
    let (code, stdout, stderr) = run_default_multi(
        "compute_fused_broadcast_default",
        "main.jet",
        &[("main.jet", source)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(
        stdout,
        "shape:[2, 3]\nvalues:[4.0, 5.0, 7.0, 4.0, 5.0, 7.0]\n"
    );
}
