//! Focused parity checks for the remaining CPU compute slices.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{assert_tiers_agree, build_and_run, jit_run_traced, run_default_multi};

fn assert_aot_and_default_parity(name: &str, source: &str, required: &[&str]) {
    let (aot_code, aot_stdout) = build_and_run(name, source);
    assert_eq!(aot_code, 0, "AOT failed for {name}: {aot_stdout}");
    for needle in required {
        assert!(aot_stdout.contains(needle), "{name} missing `{needle}`: {aot_stdout}");
    }
    let (jit_code, jit_stdout, stderr) = run_default_multi(
        &format!("{name}_jit"),
        "main.jet",
        &[("main.jet", source)],
    );
    assert_eq!(jit_code, 0, "default jet run failed for {name}: {stderr}");
    assert_eq!(jit_stdout, aot_stdout, "AOT/JIT drift for {name}");
}

#[test]
fn linalg_and_fft_use_the_cpu_oracle() {
    assert_aot_and_default_parity(
        "compute_linalg_targeted",
        include_str!("../examples/features/tooling/compute_linalg.jet"),
        &["det:10", "solve:", "fft_len:8"],
    );
}

#[test]
fn autodiff_sparse_simd_and_streams_use_real_paths() {
    let source = include_str!("../examples/features/tooling/compute_autodiff.jet");
    let expected = include_str!("../examples/features/expected/tooling/compute_autodiff.out");
    let mut bundle = jet::Loader::load_entry("examples/features/tooling/compute_autodiff.jet")
        .expect("load compute autodiff example");
    jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert_tiers_agree("compute_autodiff_targeted", source, expected);
    let (code, stdout, stderr) = jit_run_traced("compute_autodiff_targeted_trace", source);
    assert_eq!(code, 0, "traced default jet run failed: {stderr}");
    assert_eq!(stdout, expected, "traced default output drifted: {stderr}");
    assert!(stderr.contains("tier1 native"), "autodiff did not stay resident:\n{stderr}");
    assert!(!stderr.contains("tier0 interp"), "autodiff deopted:\n{stderr}");
}

#[test]
fn curried_autodiff_shapes_share_the_prelude_handle() {
    assert_tiers_agree(
        "compute_curried_autodiff_shapes",
        r#"
use core.compute as compute

fn loss(w: Tensor, x: Tensor) Tensor -> compute.mul(w, x) ?? panic("loss")

fn run() {
    w :: compute.from_list([2.0]) ?? panic("w")
    x :: compute.from_list([4.0]) ?? panic("x")
    tangent_w :: compute.ones([1]) ?? panic("tangent_w")
    tangent_x :: compute.ones([1]) ?? panic("tangent_x")

    value_and_gradient :: compute.value_and_gradient(loss)
    (value, gradients) :: value_and_gradient(w, x)
    print("value:{compute.to_list(value)}")
    print("value_gradient_w:{compute.to_list(gradients.w)}")

    jvp :: compute.jvp(loss)
    (jvp_value, jvp_tangent) :: jvp(w, x, tangent_w, tangent_x)
    print("jvp_value:{compute.to_list(jvp_value)}")
    print("jvp_tangent:{compute.to_list(jvp_tangent)}")

    vjp :: compute.vjp(loss)
    vjp_run :: vjp(w, x)
    pull_fn :: vjp_run.pull
    pull :: pull_fn(~tangent_w)
    print("vjp_pull_w:{compute.to_list(pull.w)}")
    print("vjp_grads_x:{compute.to_list(vjp_run.grads.x)}")
}
"#,
        "value:[8.0]\nvalue_gradient_w:[4.0]\njvp_value:[8.0]\njvp_tangent:[6.0]\nvjp_pull_w:[4.0]\nvjp_grads_x:[2.0]\n",
    );
}

#[test]
fn autodiff_purity_keeps_compute_failure_fallback_but_rejects_effectful_loss() {
    let pure = r#"
use core.compute as compute

fn loss(value: Tensor) Tensor {
    return compute.mul(value, value) ?? panic("loss")
}

fn run() {
    value :: compute.ones([1]) ?? panic("value")
    gradient :: compute.gradient(loss, ~value)
}
"#;
    jet::compile(pure).expect("a checked compute failure fallback must remain differentiable");

    let impure = r#"
use core.compute as compute

fn loss(value: Tensor) Tensor {
    print("side effect")
    return compute.mul(value, value) ?? panic("loss")
}

fn run() {
    value :: compute.ones([1]) ?? panic("value")
    gradient :: compute.gradient(loss, ~value)
    print(compute.to_list(gradient))
}
"#;
    let diagnostics = jet::compile(impure).expect_err("an effectful loss must be rejected");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.what.contains("needs a pure Tensor function")),
        "missing autodiff purity diagnostic: {diagnostics:?}"
    );
}

#[test]
fn f32_simd_matmul_keeps_vjp_and_jvp_on_the_cpu_oracle() {
    assert_tiers_agree(
        "compute_f32_simd_autodiff",
        r#"
use core.compute as compute

fn tiled_loss(left: Tensor, right: Tensor) Tensor {
    return compute.matmul_f32_tile(left, right) ?? panic("tiled_loss")
}

fn run() {
    left :: compute.full([1, 1], 2.0) ?? panic("left")
    right :: compute.full([1, 1], 3.0) ?? panic("right")
    (grad_left, grad_right) :: compute.gradient(tiled_loss, ~left, ~right)
    print("grad_left:{compute.to_list(grad_left)}")
    print("grad_right:{compute.to_list(grad_right)}")

    tangent_left :: compute.full([1, 1], 1.0) ?? panic("tangent_left")
    tangent_right :: compute.full([1, 1], 1.0) ?? panic("tangent_right")
    tiled_jvp :: compute.jvp(tiled_loss)
    (value, tangent) :: tiled_jvp(left, right, tangent_left, tangent_right)
    print("value:{compute.to_list(value)}")
    print("tangent:{compute.to_list(tangent)}")
}
"#,
        "grad_left:[3.0]\ngrad_right:[2.0]\nvalue:[6.0]\ntangent:[5.0]\n",
    );
}

#[test]
fn ml_serialization_and_placement_failures_stay_in_the_same_tier() {
    let ml_source = include_str!("../examples/features/tooling/compute_ml.jet");
    let ml_output = include_str!("../examples/features/expected/tooling/compute_ml.out");
    assert_tiers_agree(
        "compute_ml_targeted",
        ml_source,
        ml_output,
    );
    let (resident_code, resident_stdout, resident_stderr) =
        run_default_multi("compute_ml_resident", "main.jet", &[("main.jet", ml_source)]);
    assert_eq!(resident_code, 0, "resident ML `jet run` failed: {resident_stderr}");
    assert_eq!(resident_stdout, ml_output, "resident ML output drifted: {resident_stderr}");
    assert!(
        resident_stderr.contains("tier1 native"),
        "ML training/inference did not execute in resident JIT:\n{resident_stderr}"
    );
    assert!(
        !resident_stderr.contains("tier0 interp"),
        "ML training/inference fell back to the interpreter:\n{resident_stderr}"
    );
    assert_tiers_agree(
        "compute_ml_f32_wire",
        r#"
use core.compute as compute

fn run() {
    left :: compute.matrix(1, 1, 2.0) ?? panic("left")
    right :: compute.matrix(1, 1, 3.0) ?? panic("right")
    model :: compute.matmul_f32_tile(left, right) ?? panic("model")
    wire :: compute.serialize(model) ?? panic("wire")
    print("wire:{wire}")
    round :: compute.deserialize(wire) ?? panic("round")
    print("round:{compute.to_list(round)}")
}
"#,
        "wire:shape=1,1;data=6.0;profile=F32Strict+Reproducible;checksum=4593129c0b16d781\nround:[6.0]\n",
    );
    assert_aot_and_default_parity(
        "compute_device_targeted",
        include_str!("../examples/features/tooling/compute_device.jet"),
        &["device:CPU", "placement:Placement", "transfer:Transfer("],
    );
}

#[test]
fn safe_kernel_boundary_and_f32_profile_are_explicit() {
    assert_aot_and_default_parity(
        "compute_kernel_targeted",
        include_str!("../examples/features/tooling/compute_kernel.jet"),
        &["kernel:42", "bounds:true"],
    );
    assert_aot_and_default_parity(
        "compute_simd_targeted",
        include_str!("../examples/features/tooling/compute_simd.jet"),
        &["profile=F32Strict+Reproducible", "tile:[19.0, 22.0, 43.0, 50.0]"],
    );
}

#[test]
fn f32_tile_matches_cpu_oracle_on_tails_and_rejects_hostile_matrices() {
    let source = r#"
use core.compute as compute

fn run() {
    left :: compute.full([3, 13], 1.0) ?? panic("left")
    right :: compute.full([13, 7], 2.0) ?? panic("right")
    scalar :: compute.matmul(left, right) ?? panic("scalar")
    tiled :: compute.matmul_f32_tile(left, right) ?? panic("tiled")
    print("scalar:{compute.to_list(scalar)}")
    print("tiled:{compute.to_list(tiled)}")

    wrong :: compute.full([14, 7], 2.0) ?? panic("wrong")
    bad_shape :: compute.matmul_f32_tile(left, wrong)
    if bad_shape == {
        .Ok(_) -> { print("bad_shape:accepted") }
        .Err(_) -> { print("bad_shape:rejected") }
    }

    wide :: compute.full([1, 1], 1e40) ?? panic("wide")
    bad_f32 :: compute.matmul_f32_tile(wide, wide)
    if bad_f32 == {
        .Ok(_) -> { print("bad_f32:accepted") }
        .Err(_) -> { print("bad_f32:rejected") }
    }

    overflow :: compute.matrix(9223372036854775807, 2, 0.0)
    if overflow == {
        .Ok(_) -> { print("overflow:accepted") }
        .Err(_) -> { print("overflow:rejected") }
    }
}
"#;
    assert_tiers_agree(
        "compute_f32_tile_hostile",
        source,
        concat!(
            "scalar:[26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0]\n",
            "tiled:[26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0, 26.0]\n",
            "bad_shape:rejected\n",
            "bad_f32:rejected\n",
            "overflow:rejected\n",
        ),
    );
}

#[test]
fn safe_kernel_rejects_effectful_bodies_before_codegen() {
    let diagnostics = jet::compile(
        "#Kernel(.parallel) fn noisy(value: Int) Int -[IO]> { print(value); return value }\n",
    )
    .expect_err("a safe kernel must not lower an effectful body");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E1102"),
        "missing E1102: {diagnostics:?}"
    );
}

#[test]
fn raw_kernel_contract_cannot_be_forged_without_a_provider() {
    let diagnostics = jet::compile(
        "use core.compute as compute\nfn run() { compute.raw_kernel_contract(\"no gate\", 1) }\n",
    )
    .expect_err("the CPU compute module must not expose a raw-contract constructor");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.what.contains("raw_kernel_contract")
                || diagnostic.fix.contains("core.compute")
        }),
        "missing provider-boundary diagnostic: {diagnostics:?}"
    );
}

#[test]
fn safe_kernel_proof_reaches_tir_without_rederivation() {
    let compiled = jet::compile(
        "#Kernel(.parallel) fn add(left: Int, right: Int) Int -> left + right;\nfn run() { print(add(1, 2)) }\n",
    )
    .expect("the checked kernel should compile");
    assert!(
        compiled.rust.contains(
            "jet-kernel-proof: mode=parallel bounds=true alias_free=true captures=true race_free=true barriers_uniform=true control_flow=true"
        ),
        "missing sema kernel proof in TIR output"
    );
    assert!(
        compiled
            .rust
            .contains("const _: () = assert!(true, \"Jet kernel proof must be complete\")"),
        "AOT backend did not consume the complete kernel proof"
    );
}

#[test]
fn data_series_feeds_the_same_compute_tensor_path() {
    assert_aot_and_default_parity(
        "compute_data_integration",
        r#"
use core.data as data
use core.compute as compute

fn run() {
    series :: data.series([Float]{1.0, 2.0, 3.0})
    values :: data.values(series)
    tensor :: compute.from_list(values) ?? panic("tensor")
    doubled :: compute.mul(tensor, compute.full([3], 2.0) ?? panic("factor")) ?? panic("doubled")
    print("data_tensor:{compute.to_list(doubled)}")
}
"#,
        &["data_tensor:[2.0, 4.0, 6.0]"],
    );
}
