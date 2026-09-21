
mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run_full, have_rustc, interpreter_run, jit_run};

const EXPECTED: &str = "-0.169075164\n";
const STANDARD: &str = include_str!("nbody_witness/standard.jet");
const SCALAR: &str = include_str!("nbody_witness/scalar.jet");

fn assert_fixture_tiers(name: &str, source: &str) {
    let (jit_code, jit_stdout, jit_stderr) = jit_run(name, source);
    assert_eq!(jit_code, 0, "{name} default run failed: {jit_stderr}");
    assert_eq!(jit_stdout, EXPECTED, "{name} default output drifted");

    let (interpreter_code, interpreter_stdout, interpreter_stderr) = interpreter_run(name, source);
    assert_eq!(interpreter_code, jit_code, "{name} interpreter exit drifted: {interpreter_stderr}");
    assert_eq!(interpreter_stdout, EXPECTED, "{name} interpreter output drifted");

    if have_rustc() {
        let (aot_code, aot_stdout, aot_stderr) = build_and_run_full("jet_nbody_witness", name, source);
        assert_eq!(aot_code, jit_code, "{name} AOT exit drifted: {aot_stderr}");
        assert_eq!(aot_stdout, EXPECTED, "{name} AOT output drifted");
    }
}

#[test]
fn nbody_default_and_scalar_witnesses_have_identical_observable_output() {
    assert_fixture_tiers("nbody_standard_witness", STANDARD);
    assert_fixture_tiers("nbody_scalar_witness", SCALAR);
}
