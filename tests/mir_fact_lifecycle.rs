use jet_foundation::MIR::{
    MirArtifactBuildMode, MirArtifactKind, MirArtifactRequest, MirArtifactTarget,
    MirOptimizationPolicy,
};
mod common;

fn lower_checked(source: &str) -> jet_foundation::MIR::MirProgram {
    let root = common::unique_tmp("jet_mir_fact_lifecycle");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("package.jet"),
        "name: \"mir_fact_lifecycle\"\nversion: \"1.0.0\"\n",
    )
    .unwrap();
    let entry = root.join("main.jet");
    std::fs::write(&entry, source).unwrap();

    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");

    let request = MirArtifactRequest::new(
        MirArtifactTarget::Cranelift,
        MirArtifactKind::NativeExecutable,
        MirArtifactBuildMode::Dev,
    );
    jet::Codegen::TIR::lower_checked_mir_program_for(&bundle, request)
        .expect("checked bundle lowers through the public MIR optimizer path")
        .0
}

fn function<'a>(
    program: &'a jet_foundation::MIR::MirProgram,
    name: &str,
) -> &'a jet_foundation::MIR::MirFunction {
    program
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("missing MIR function {name}"))
}

#[test]
fn changed_mir_recomputes_facts_and_rejects_stale_passes() {
    let source = r#"
fn auto(values: [Float#4]) [Float#4] -> {
    output := [Float#4]{0.0, 0.0, 0.0, 0.0}
    loop i in 0..<4 {
        output[i] = values[i] * 2.0
    }
    return output
}

fn scalar_sum(values: [Float#4]) Float -> {
    total := Float{0.0}
    loop i in 0..<4 { total += values[i] }
    return total
}

fn run() {}
"#;
    let mir = lower_checked(source);
    let policy = MirOptimizationPolicy::conservative();
    let optimized = jet_foundation::MIR::optimize_mir_program(&mir, &policy).unwrap();
    let reduction = function(&optimized, "scalar_sum");
    assert!(!reduction.optimization.auto_vectorizable);
    assert!(reduction
        .optimization
        .vector_facts
        .iter()
        .all(|fact| !fact.decision.is_eligible()));
    let auto = function(&optimized, "auto");
    assert!(
        auto.optimization.auto_vectorizable,
        "loop facts: {:#?}\nvector facts: {:#?}",
        auto.optimization.loop_facts,
        auto.optimization.vector_facts,
    );
    assert!(
        auto.optimization
            .vector_facts
            .iter()
            .any(|fact| fact.decision.is_eligible())
    );

    let mut overrun = optimized.clone();
    let overrun_function = function_mut(&mut overrun, "auto");
    let upper_bound = overrun_function
        .blocks
        .iter_mut()
        .flat_map(|block| &mut block.instructions)
        .find_map(|instruction| match &mut instruction.operation {
            jet_foundation::MIR::MirOperation::Constant(
                jet_foundation::MIR::MirConstant::Int { value, .. },
            ) if *value == 4 => Some(value),
            _ => None,
        })
        .expect("the checked loop has a literal upper bound");
    *upper_bound = 5;
    overrun_function.optimization.pass_ids.clear();
    let overrun = jet_foundation::MIR::optimize_mir_program(&overrun, &policy).unwrap();
    let overrun_function = function(&overrun, "auto");
    assert!(overrun_function
        .optimization
        .loop_facts
        .iter()
        .any(|fact| fact.trip_count == Some(5)));
    assert!(!overrun_function.optimization.auto_vectorizable);

    let mut changed = optimized.clone();
    function_mut(&mut changed, "auto")
        .effects
        .direct
        .insert("IO".to_string());
    assert!(
        jet_foundation::MIR::require_canonical_mir_optimization(&changed).is_err(),
        "changed MIR must not inherit the old pass receipt"
    );
    function_mut(&mut changed, "auto")
        .optimization
        .pass_ids
        .clear();

    let rejected = jet_foundation::MIR::optimize_mir_program(&changed, &policy).unwrap();
    let auto = function(&rejected, "auto");
    assert!(!auto.optimization.auto_vectorizable);
    assert!(
        auto.optimization
            .vector_facts
            .iter()
            .all(|fact| !fact.decision.is_eligible())
    );
    jet_foundation::MIR::require_canonical_mir_optimization(&rejected).unwrap();

    let no_op = jet_foundation::MIR::optimize_mir_program(&rejected, &policy).unwrap();
    assert_eq!(
        rejected.deterministic_digest(),
        no_op.deterministic_digest(),
        "reoptimizing unchanged MIR must preserve identity"
    );
}

fn function_mut<'a>(
    program: &'a mut jet_foundation::MIR::MirProgram,
    name: &str,
) -> &'a mut jet_foundation::MIR::MirFunction {
    program
        .functions
        .iter_mut()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("missing MIR function {name}"))
}
