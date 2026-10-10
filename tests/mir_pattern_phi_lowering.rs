mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

fn lower_mir(name: &str, source: &str) -> jet_foundation::MIR::MirProgram {
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
    let tir = jet::Codegen::TIR::lower_checked_tir_program_for(&bundle, &nominals, request).expect("checked pattern and phi fixtures must lower");
    let program = jet::Codegen::TIR::lower_tir_to_mir(tir).expect("checked TIR must lower to MIR");
    program.validate().expect("lowered ownership and pattern MIR must validate");
    program
}

fn check_mir(name: &str, source: &str) {
    jet_foundation::MIR::optimize_mir_program(lower_mir(name, source), &jet_foundation::MIR::MirOptimizationPolicy::conservative())
        .expect("checked pattern and phi MIR must optimize");
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

#[test]
fn borrowed_trait_coercion_preserves_prepared_enum_payload() {
    let source = include_str!("fixtures/mir_borrowed_trait_coercion.jet");
    check_mir("borrowed_trait_coercion", source);
    let (code, stdout, stderr) = tir_support::build_and_run_full("borrowed_trait_coercion", "run", source);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "17\n17\n10\n");
}

#[test]
fn cfg_simplification_prunes_dead_index_places_and_keeps_live_ones() {
    use jet_foundation::MIR::{MirOptimizationPolicy, MirProjection};
    let source = include_str!("fixtures/mir_cfg_dead_index_places.jet");
    let program = lower_mir("cfg_dead_index_places", source);
    jet_foundation::MIR::verify_mir_legality(&program).expect("pre-pass indexed MIR must be legal");
    let erased = program.functions.iter().find(|function| function.name == "erased").unwrap();
    let erased_blocks = erased.blocks.len();
    let erased_places = erased.places.len();
    assert!(erased.places.iter().any(|place| place.projections.iter().any(|projection| matches!(projection, MirProjection::Index { .. }))));
    let optimized = jet_foundation::MIR::optimize_mir_program(program, &MirOptimizationPolicy::conservative())
        .expect("constant branch removal must not strand an index reference");
    jet_foundation::MIR::verify_mir_legality(&optimized).expect("post-pass indexed MIR must be legal");
    let erased = optimized.functions.iter().find(|function| function.name == "erased").unwrap();
    assert!(erased.blocks.len() < erased_blocks);
    assert!(erased.places.len() < erased_places, "dead index metadata must actually be removed");
    let active = optimized.functions.iter().find(|function| function.name == "active").unwrap();
    assert!(active.places.iter().any(|place| place.projections.iter().any(|projection| matches!(projection, MirProjection::Index { .. }))));
    let (code, stdout, stderr) = tir_support::build_and_run_full("cfg_dead_index_places", "run", source);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "17\n24\n17\n");
}
