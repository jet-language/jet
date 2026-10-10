mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

fn check_mir(name: &str, source: &str) {
    use jet_foundation::MIR::{MirArtifactBuildMode, MirArtifactKind, MirArtifactRequest, MirArtifactTarget};
    let scratch = common::Scratch::new(name);
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    let entry = scratch.path.join("run.jet");
    std::fs::write(&entry, source).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let (diagnostics, _, nominals) = jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diagnostics.into_iter().filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)).collect();
    assert!(errors.is_empty(), "{errors:#?}");
    let request = MirArtifactRequest::new(MirArtifactTarget::Interpreter, MirArtifactKind::NativeExecutable, MirArtifactBuildMode::Dev);
    let (program, _) = jet::Codegen::TIR::lower_checked_mir_program_for(&bundle, &nominals, request).expect("checked pattern and phi fixtures must lower");
    program.validate().expect("lowered ownership and pattern MIR must validate");
}

#[test]
fn phi_optional_fields_and_absent_branches_compile_and_run() {
    let source = include_str!("fixtures/mir_phi_optional_fields.jet");
    check_mir("phi_optional_fields", source);
    let (code, stdout, stderr) = tir_support::build_and_run_full("phi_optional_fields", "run", source);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "17\nready\n0\nnone\n23\nmissing\n");
}

#[test]
fn or_patterns_bind_by_name_and_preserve_optional_subjects() {
    let source = include_str!("fixtures/mir_or_pattern_binding_order.jet");
    check_mir("or_pattern_binding_order", source);
    let (code, stdout, stderr) = tir_support::build_and_run_full("or_pattern_binding_order", "run", source);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "policy\n2\ncall\n1\ntrue\ntrue\nfalse\nfalse\n");
}

#[test]
fn or_patterns_short_circuit_and_preserve_failed_nested_carriers() {
    let source = include_str!("fixtures/mir_or_pattern_ordered_carriers.jet");
    check_mir("or_pattern_ordered_carriers", source);
    let (code, stdout, stderr) = tir_support::build_and_run_full("or_pattern_ordered_carriers", "run", source);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "1\n8\n3\n7\n0\n11\n13\n0\n0\n13\n17\n");
}
