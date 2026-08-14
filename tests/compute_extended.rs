//! Focused parity checks for the remaining CPU compute slices.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{assert_tiers_agree, build_and_run, run_default_multi};

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
    assert_aot_and_default_parity(
        "compute_autodiff_targeted",
        include_str!("../examples/features/tooling/compute_autodiff.jet"),
        &["grad:", "jvp:", "vjp:", "nnz:2", "mv:", "profile=F32Strict+Reproducible", "ComputeStream"],
    );
}

#[test]
fn ml_serialization_and_placement_failures_stay_in_the_same_tier() {
    assert_tiers_agree(
        "compute_ml_targeted",
        include_str!("../examples/features/tooling/compute_ml.jet"),
        "before:[0.0, 0.0, 0.0, 0.0]\nloss:[1.0]\nafter:[0.25, 0.25, 0.25, 0.25]\ntrained_loss:[0.5625]\nwire:shape=2,2;data=0.25,0.25,0.25,0.25;profile=F64Strict+Reproducible;checksum=8551306b599382c8\nround:[0.25, 0.25, 0.25, 0.25]\nf32_before:[2.0]\nf32_loss:[4.0]\nf32_after:[4.0]\nf32_trained_loss:[0.0]\nf32_round:[2.0]\nf32_placement:Placement(requested=CPU, selected=CPU, backend=cpu-oracle, version=builtin, profile=F32Strict+Reproducible, cache=none, capabilities=[\"ranked-storage\", \"strided-view\", \"checked-bounds\", \"f32-arithmetic\", \"cpu-simd-dispatch\", \"simd-tail\", \"blocked-matmul\", \"differential-oracle\"], reason=deserialized canonical Tensor)\n",
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
        "#Kernel(.parallel) fn noisy(value: Int) => Int { print(value); return value }\n",
    )
    .expect_err("a safe kernel must not lower an effectful body");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E1130"),
        "missing E1130: {diagnostics:?}"
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
        "#Kernel(.parallel) fn add(left: Int, right: Int) => Int :: left + right;\nfn run() { print(add(1, 2)) }\n",
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
    series :: data.series([1.0, 2.0, 3.0])
    values :: data.values(series)
    tensor :: compute.from_list(values) ?? panic("tensor")
    doubled :: compute.mul(tensor, compute.full([3], 2.0) ?? panic("factor")) ?? panic("doubled")
    print("data_tensor:{compute.to_list(doubled)}")
}
"#,
        &["data_tensor:[2.0, 4.0, 6.0]"],
    );
}
