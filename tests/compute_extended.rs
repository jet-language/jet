//! Focused parity checks for the remaining CPU compute slices.

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, run_default_multi};

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
        &["grad:", "jvp:", "vjp:", "nnz:2", "mv:", "profile:F32Strict+Reproducible+Tile8", "ComputeStream"],
    );
}

#[test]
fn ml_serialization_and_placement_failures_stay_in_the_same_tier() {
    assert_aot_and_default_parity(
        "compute_ml_targeted",
        include_str!("../examples/features/tooling/compute_ml.jet"),
        &["loss:", "param:", "wire:shape=3;data=", "round:"],
    );
    assert_aot_and_default_parity(
        "compute_device_targeted",
        include_str!("../examples/features/tooling/compute_device.jet"),
        &["device:CPU", "placement:Placement", "transfer:Transfer("],
    );
}

#[test]
fn raw_kernel_boundary_and_f32_profile_are_explicit() {
    assert_aot_and_default_parity(
        "compute_kernel_targeted",
        include_str!("../examples/features/tooling/compute_kernel.jet"),
        &["kernel:42", "bounds:true", "raw:RawKernelContract"],
    );
    assert_aot_and_default_parity(
        "compute_simd_targeted",
        include_str!("../examples/features/tooling/compute_simd.jet"),
        &["profile:F32Strict+Reproducible+Tile8", "tile:[19.0, 22.0, 43.0, 50.0]"],
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
fn raw_kernel_contract_cannot_escape_the_unsafe_gate() {
    let diagnostics = jet::compile(
        "use core.compute as compute\nfn run() { compute.raw_kernel_contract(\"no gate\", 1) }\n",
    )
    .expect_err("raw kernel contracts must be rejected outside #Unsafe");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E3101"),
        "missing E3101: {diagnostics:?}"
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
