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

fn cuda_loss(value: Tensor) Tensor -> {
    rooted :: compute.sqrt(value) ?? panic("sqrt")
    return compute.sum_axis(rooted, 1) ?? panic("sum")
}

fn cuda_mse(value: Tensor, target: Tensor) Tensor -> compute.mse_loss(value, target) ?? panic("mse")

fn run() {
    seed :: compute.matrix(1, 1, 2.0) ?? panic("seed")
    row :: compute.matrix(1, 257, 3.0) ?? panic("row")
    f32_row :: compute.matmul_f32_tile(seed, row) ?? panic("f32_row")
    f32_column :: compute.transpose(f32_row) ?? panic("f32_column")
    target_seed :: compute.matrix(1, 257, 1.0) ?? panic("target_seed")
    target :: compute.matmul_f32_tile(seed, target_seed) ?? panic("target")
    zero_seed :: compute.matrix(1, 257, 0.0) ?? panic("zero_seed")
    zero :: compute.matmul_f32_tile(seed, zero_seed) ?? panic("zero")
    one_seed :: compute.matrix(1, 1, 1.0) ?? panic("one_seed")
    one :: compute.matmul_f32_tile(one_seed, one_seed) ?? panic("one")
    cpu_gradient_input :: compute.full([1, 1], 6.0) ?? panic("cpu_gradient_input")
    cpu_gradient_result :: compute.gradient(cuda_loss, ~cpu_gradient_input)
    cpu_gradient :: cpu_gradient_result.value
    cpu_gradient_tail :: compute.get(cpu_gradient, [0, 0]) ?? panic("cpu_gradient_tail")
    request :: compute.on_device(f32_row, compute.device_cuda())
    column_request :: compute.on_device(f32_column, compute.device_cuda())
    target_request :: compute.on_device(target, compute.device_cuda())
    zero_request :: compute.on_device(zero, compute.device_cuda())
    one_request :: compute.on_device(one, compute.device_cuda())
    if request == {
        .Err(_) -> {
            print("cuda:unavailable")
            stream :: compute.stream_new_on(compute.device_cuda())
            if stream == {
                .Ok(_) -> panic("cuda stream unexpectedly accepted")
                .Err(_) -> { print("cuda:stream:rejected") }
            }
        }
        .Ok(cuda_row) -> {
            cuda_column :: column_request ?? panic("cuda_column")
            cuda_target :: target_request ?? panic("cuda_target")
            cuda_zero :: zero_request ?? panic("cuda_zero")
            cuda_one :: one_request ?? panic("cuda_one")
            if !compute.placement(cuda_row).contains("backend=cuda") -> panic("cuda backend")
            if !compute.placement(cuda_row).contains("profile=F32Strict+Reproducible") -> panic("cuda profile")
            product :: compute.matmul(cuda_column, cuda_row) ?? panic("matmul")
            doubled :: compute.add(product, product) ?? panic("add")
            negated :: compute.negate(doubled) ?? panic("negate")
            print("cuda:available")
            tail :: compute.get(doubled, [256, 256]) ?? panic("tail")
            negative_tail :: compute.get(negated, [256, 256]) ?? panic("negative_tail")
            if tail < 35.999 || tail > 36.001 -> panic("cuda add parity")
            if negative_tail < -36.001 || negative_tail > -35.999 -> panic("cuda negate parity")
            print("cuda:tail:{tail}")
            print("cuda:negative_tail:{negative_tail}")

            total :: compute.sum_axis(doubled, 1) ?? panic("sum_axis")
            sum_tail :: compute.get(total, [256]) ?? panic("sum_tail")
            if sum_tail < 9251.999 || sum_tail > 9252.001 -> panic("cuda sum parity")
            print("cuda:sum_tail:{sum_tail}")

            gradient_result :: compute.gradient(cuda_loss, ~cuda_row)
            gradient :: gradient_result.value
            gradient_tail :: compute.get(gradient, [0, 256]) ?? panic("gradient_tail")
            if gradient_tail < cpu_gradient_tail - 0.001 || gradient_tail > cpu_gradient_tail + 0.001 -> panic("cuda gradient parity")
            print("cuda:sqrt_gradient:{gradient_tail}")

            mse :: cuda_mse(cuda_row, cuda_target)
            mse_value :: compute.get(mse, [0]) ?? panic("mse_value")
            if mse_value < 15.999 || mse_value > 16.001 -> panic("cuda mse parity")
            (mse_gradient, mse_target_gradient) :: compute.gradient(cuda_mse, ~cuda_row, ~cuda_target)
            mse_gradient_tail :: compute.get(mse_gradient, [0, 256]) ?? panic("mse_gradient_tail")
            mse_target_gradient_tail :: compute.get(mse_target_gradient, [0, 256]) ?? panic("mse_target_gradient_tail")
            if mse_gradient_tail < 0.030 || mse_gradient_tail > 0.032 -> panic("cuda mse gradient")
            if mse_target_gradient_tail > -0.030 || mse_target_gradient_tail < -0.032 -> panic("cuda mse target gradient")

            mse_jvp :: compute.jvp(cuda_mse)
            (mse_jvp_value, mse_jvp_tangent) :: mse_jvp(cuda_row, cuda_target, cuda_row, cuda_zero)
            mse_jvp_value_tail :: compute.get(mse_jvp_value, [0]) ?? panic("mse_jvp_value")
            mse_jvp_tangent_tail :: compute.get(mse_jvp_tangent, [0]) ?? panic("mse_jvp_tangent")
            if mse_jvp_value_tail < 15.999 || mse_jvp_value_tail > 16.001 -> panic("cuda mse jvp value")
            if mse_jvp_tangent_tail < 47.999 || mse_jvp_tangent_tail > 48.001 -> panic("cuda mse jvp")

            vjp_run :: compute.vjp(cuda_mse, ~cuda_row, ~cuda_target)
            (vjp_gradient, vjp_target_gradient) :: vjp_run.grads
            vjp_gradient_tail :: compute.get(vjp_gradient, [0, 256]) ?? panic("vjp_gradient_tail")
            vjp_target_gradient_tail :: compute.get(vjp_target_gradient, [0, 256]) ?? panic("vjp_target_gradient_tail")
            if vjp_gradient_tail < 0.030 || vjp_gradient_tail > 0.032 -> panic("cuda vjp")
            if vjp_target_gradient_tail > -0.030 || vjp_target_gradient_tail < -0.032 -> panic("cuda vjp target")
            pull_fn :: vjp_run.pull
            (pull_gradient, _) :: pull_fn(~cuda_one)
            pull_gradient_tail :: compute.get(pull_gradient, [0, 256]) ?? panic("pull_gradient_tail")
            if pull_gradient_tail < 0.030 || pull_gradient_tail > 0.032 -> panic("cuda vjp pull")

            updated :: compute.sgd_step(cuda_row, mse_gradient, 0.5) ?? panic("sgd")
            updated_tail :: compute.get(updated, [0, 256]) ?? panic("updated_tail")
            if updated_tail < 5.983 || updated_tail > 5.986 -> panic("cuda sgd")

            stream :: compute.stream_new_on(compute.device_cuda()) ?? panic("stream")
            compute.stream_sync(stream) ?? panic("stream_sync")
            print("cuda:stream:ok")

            cpu :: compute.transfer(negated, compute.device_cpu()) ?? panic("transfer")
            if !compute.transfer_show(cpu).contains("from=CUDA") -> panic("transfer source")
            if !compute.transfer_show(cpu).contains("to=CPU") -> panic("transfer destination")
            print("cuda:transfer:{compute.transfer_show(cpu)}")
            unsupported :: compute.det(product)
            if unsupported == {
                .Ok(_) -> panic("cuda det unexpectedly accepted")
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
    for entry in [
        "jet_copy",
        "jet_binary",
        "jet_unary",
        "jet_matmul",
        "jet_sum",
        "jet_mse",
        "jet_mse_grad",
        "jet_mse_jvp",
        "jet_sgd",
        "jet_scale",
    ] {
        let needle = format!(".visible .entry {entry}");
        assert!(
            prelude.contains(needle.as_str()),
            "missing CUDA kernel {entry}"
        );
    }
    for operation in [
        "jet_compute_cuda::copy",
        "jet_compute_cuda::binary",
        "jet_compute_cuda::unary",
        "jet_compute_cuda::matmul",
        "jet_compute_cuda::sum",
        "jet_compute_cuda::mse",
        "jet_compute_cuda::mse_grad",
        "jet_compute_cuda::mse_jvp",
        "jet_compute_cuda::sgd",
        "jet_compute_cuda::scale",
        "jet_compute_cuda::stream_new",
        "jet_compute_cuda::stream_sync",
    ] {
        assert!(
            prelude.contains(operation),
            "missing CUDA dispatch {operation}"
        );
    }
    assert!(prelude.contains("check(unsafe { (api.init)(0) }, \"driver initialization\")"));
    assert!(prelude.contains("no CPU fallback was selected"));
}
