//! Focused production-path proof for the CUDA core.compute seam.

// `tir_support` re-exports a helper from `common`, so every binary that
// includes it must declare `common` too.
#[path = "common/mod.rs"]
mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::assert_example_cli_tiers_agree;

#[test]
fn cuda_public_precision_gate_is_identical_on_all_tiers() {
    assert_example_cli_tiers_agree("tooling/compute_cuda", "cuda:f64:rejected\n");
}
