//! Checked dense solve and LU behavior for the source-owned `core.compute.solve` API.

mod common;
mod tir_support;

#[test]
fn compute_solve_uses_scale_relative_pivots_and_checked_errors() {
    tir_support::assert_tiers_agree(
        "compute_solve_relative_pivot_and_errors",
        r#"
use core.compute.[Tensor, ComputeError]
use core.compute.solve as solve

fn run() {
    scaled :: Tensor{shape: [Int]{1, 1}, data: [Float]{1e-20}, device: "CPU", numeric_profile: ""}
    rhs :: Tensor{shape: [Int]{1}, data: [Float]{2e-20}, device: "CPU", numeric_profile: ""}
    solution :: solve.dense(scaled, rhs) ?? panic("scaled dense solve")
    print("dense:{solution.data}")
    factors :: solve.lu(scaled) ?? panic("scaled LU")
    print("lu:{factors.pivots}")

    singular :: Tensor{shape: [Int]{2, 2}, data: [Float]{1.0, 2.0, 2.0, 4.0}, device: "CPU", numeric_profile: ""}
    if solve.lu(singular) == {
        .Ok(_) -> print("singular:accepted")
        .Err(error) -> {
            if error == ComputeError.Singular {
                print("singular:Singular")
            } else {
                print("singular:wrong-error")
            }
        }
    }

    wrong_rhs :: Tensor{shape: [Int]{2}, data: [Float]{1.0, 2.0}, device: "CPU", numeric_profile: ""}
    if solve.dense(scaled, wrong_rhs) == {
        .Ok(_) -> print("shape:accepted")
        .Err(error) -> {
            if error == ComputeError.Shape {
                print("shape:Shape")
            } else {
                print("shape:wrong-error")
            }
        }
    }

    cuda :: Tensor{shape: [Int]{1, 1}, data: [Float]{1.0}, device: "CUDA", numeric_profile: ""}
    if solve.lu(cuda) == {
        .Ok(_) -> print("device:accepted")
        .Err(error) -> {
            if error == ComputeError.Unsupported {
                print("device:Unsupported")
            } else {
                print("device:wrong-error")
            }
        }
    }
}
"#,
        "dense:[2.0]\nlu:[0]\nsingular:Singular\nshape:Shape\ndevice:Unsupported\n",
    );
}
