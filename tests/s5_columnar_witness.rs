mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run_full, have_rustc, interpreter_run, jit_run};

const EXPECTED: &str = "599.9999999999977\n";
const AOS: &str = include_str!("s5_columnar_witness/aos.jet");
const COLUMNAR: &str = include_str!("s5_columnar_witness/columnar.jet");

fn assert_tiers(name: &str, source: &str) {
    let (jit_code, jit_stdout, jit_stderr) = jit_run(name, source);
    assert_eq!(jit_code, 0, "{name} default run failed: {jit_stderr}");
    assert_eq!(jit_stdout, EXPECTED, "{name} default output drifted");
    let (interpreter_code, interpreter_stdout, interpreter_stderr) = interpreter_run(name, source);
    assert_eq!(interpreter_code, jit_code, "{name} interpreter failed: {interpreter_stderr}");
    assert_eq!(interpreter_stdout, EXPECTED, "{name} interpreter output drifted");
    if have_rustc() {
        let (aot_code, aot_stdout, aot_stderr) = build_and_run_full("jet_s5_columnar", name, source);
        assert_eq!(aot_code, jit_code, "{name} AOT failed: {aot_stderr}");
        assert_eq!(aot_stdout, EXPECTED, "{name} AOT output drifted");
    }
}

#[test]
fn s5_aos_and_columnar_variants_are_byte_identical_across_tiers() {
    assert_tiers("s5_aos", AOS);
    assert_tiers("s5_columnar", COLUMNAR);
}
