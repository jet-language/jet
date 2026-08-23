//! Focused production-path proof for the CUDA core.compute seam.

// `tir_support` re-exports a helper from `common`, so every binary that
// includes it must declare `common` too.
#[path = "common/mod.rs"]
mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{
    assert_example_cli_tiers_agree, build_and_run_full, have_rustc, interpreter_run, jit_run,
};

#[test]
fn cuda_public_precision_gate_is_identical_on_all_tiers() {
    assert_example_cli_tiers_agree("tooling/compute_cuda", "cuda:f64:rejected\n");
}

#[test]
fn cuda_f32_backend_runs_or_fails_closed_across_all_tiers() {
    let source = r#"
use core.compute as compute

fn cuda_loss(value: Tensor) Tensor {
    rooted :: compute.sqrt(value) ?? panic("sqrt")
    return compute.sum_axis(rooted, 1) ?? panic("sum")
}

fn run() {
    seed :: compute.matrix(1, 1, 2.0) ?? panic("seed")
    row :: compute.matrix(1, 257, 3.0) ?? panic("row")
    f32_row :: compute.matmul_f32_tile(seed, row) ?? panic("f32_row")
    f32_column :: compute.transpose(f32_row) ?? panic("f32_column")
    request :: compute.on_device(f32_row, compute.device_cuda())
    column_request :: compute.on_device(f32_column, compute.device_cuda())
    if request == {
        .Err(_) -> {
            print("cuda:unavailable")
            stream :: compute.stream_new_on(compute.device_cuda())
            if stream == {
                .Ok(_) -> { print("cuda:stream:unexpected") }
                .Err(_) -> { print("cuda:stream:rejected") }
            }
        }
        .Ok(cuda_row) -> {
            cuda_column :: column_request ?? panic("cuda_column")
            product :: compute.matmul(cuda_column, cuda_row) ?? panic("matmul")
            doubled :: compute.add(product, product) ?? panic("add")
            negated :: compute.negate(doubled) ?? panic("negate")
            print("cuda:available")
            print("cuda:tail:{compute.get(doubled, [256, 256])}")
            print("cuda:negative_tail:{compute.get(negated, [256, 256])}")

            total :: compute.sum_axis(doubled, 1) ?? panic("sum_axis")
            print("cuda:sum_tail:{compute.get(total, [256])}")

            gradient :: compute.gradient(cuda_loss, ~cuda_row)
            print("cuda:sqrt_gradient:{compute.get(gradient, [0, 256])}")

            stream :: compute.stream_new_on(compute.device_cuda()) ?? panic("stream")
            compute.stream_sync(stream) ?? panic("stream_sync")
            print("cuda:stream:ok")

            cpu :: compute.transfer(negated, compute.device_cpu()) ?? panic("transfer")
            print("cuda:transfer:{compute.transfer_show(cpu)}")
            unsupported :: compute.det(product)
            if unsupported == {
                .Ok(_) -> { print("cuda:det:accepted") }
                .Err(_) -> { print("cuda:det:rejected") }
            }
        }
    }
}
"#;

    let (jit_code, jit_stdout, jit_stderr) = jit_run("compute_cuda_backend", source);
    assert_eq!(jit_code, 0, "default jet run failed: {jit_stderr}");
    assert!(
        jit_stdout.contains("cuda:unavailable") || jit_stdout.contains("cuda:available"),
        "CUDA branch did not execute: {jit_stdout}"
    );

    let (interpreter_code, interpreter_stdout, interpreter_stderr) =
        interpreter_run("compute_cuda_backend", source);
    assert_eq!(
        interpreter_code, jit_code,
        "interpreter/JIT exit drift: {interpreter_stderr}"
    );
    assert_eq!(
        interpreter_stdout, jit_stdout,
        "interpreter/JIT output drift"
    );
    assert_eq!(
        interpreter_stderr, jit_stderr,
        "interpreter/JIT diagnostics drift"
    );

    if have_rustc() {
        let (aot_code, aot_stdout, aot_stderr) =
            build_and_run_full("jet_tir_test", "compute_cuda_backend", source);
        assert_eq!(aot_code, jit_code, "AOT/JIT exit drift: {aot_stderr}");
        assert_eq!(aot_stdout, jit_stdout, "AOT/JIT output drift");
        assert_eq!(aot_stderr, jit_stderr, "AOT/JIT diagnostics drift");
    }
}

#[test]
fn cuda_prelude_uses_global_thread_indices_and_real_driver_launches() {
    let prelude = include_str!("../crates/jet-codegen/src/Prelude/CoreLib/Top/Compute.rs");
    assert!(prelude.contains("cuLaunchKernel"));
    assert!(prelude.contains("mod jet_compute_cuda"));
    assert!(
        prelude
            .matches("mad.lo.u32 %r0, %r0, %ntid.x, %tid.x")
            .count()
            >= 10,
        "per-element CUDA kernels must use global block-aware indices"
    );
    assert!(prelude.contains("jet_compute_cuda::binary"));
    assert!(prelude.contains("jet_compute_cuda::matmul"));
}
