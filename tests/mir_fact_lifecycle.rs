use jet_foundation::MIR::{
    MirArtifactBuildMode, MirArtifactKind, MirArtifactRequest, MirArtifactTarget,
    MirCopyCost, MirOperation, MirOptimizationPolicy, MirPlaceBase, MirProjection,
    MirVectorAccessRoot, MirVectorLayout,
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
    let checked = auto
        .optimization
        .checked_vector_facts
        .iter()
        .find(|proof| {
            auto.optimization
                .vector_facts
                .iter()
                .any(|fact| fact.loop_header == proof.loop_header)
        })
        .expect("eligible vector fact must retain its checked source proof");
    let derived = auto
        .optimization
        .vector_facts
        .iter()
        .find(|fact| fact.loop_header == checked.loop_header)
        .expect("eligible loop must retain its derived vector fact");
    let cursor = checked.cursor.expect("checked proof must identify its cursor");
    assert!(!checked.body_blocks.is_empty());
    assert!(checked.advance_block.is_some());
    assert!(!checked.accesses.is_empty());
    assert!(checked.packed);
    assert!(
        checked.span.start > 0
            && checked.span.end > checked.span.start
            && checked.span.end <= source.len(),
        "checked proof must retain a non-default source origin: {:?}",
        checked.span
    );
    assert!(
        source[checked.span.start..checked.span.end]
            .bytes()
            .any(|byte| !byte.is_ascii_whitespace()),
        "checked proof origin must cover source bytes"
    );
    assert!(
        checked.same_checked_scope(derived),
        "checked proof scope must match the canonical post-optimization row:\nchecked={checked:#?}\nderived={derived:#?}"
    );
    assert_eq!(
        checked.accesses.len(),
        3,
        "the elementwise witness must retain all canonical input/output access identities: {:#?}",
        checked.accesses
    );
    assert!(
        checked.accesses.iter().all(|access| {
            access.layout == MirVectorLayout::Flat
                && access.field.is_none()
                && access.column_index.is_none()
                && matches!(access.root, MirVectorAccessRoot::Place(_))
        }),
        "flat elementwise proof must retain place roots without projections: {:#?}",
        checked.accesses
    );
    let mut access_roots = checked
        .accesses
        .iter()
        .map(|access| {
            let MirVectorAccessRoot::Place(place) = access.root else {
                unreachable!("flat elementwise access must use a place root")
            };
            place
        })
        .collect::<Vec<_>>();
    access_roots.sort();
    access_roots.dedup();
    assert_eq!(
        access_roots.len(),
        checked.accesses.len(),
        "each proven access must retain a distinct MIR place identity"
    );
    let mut access_roles = Vec::new();
    let induction_local = auto
        .locals
        .iter()
        .find(|local| local.name == "i")
        .expect("counted witness must retain its induction local");
    for place_id in access_roots {
        let place = auto
            .places
            .iter()
            .find(|place| place.id == place_id)
            .unwrap_or_else(|| panic!("checked access references unknown MIR place {place_id:?}"));
        let semantic_name = match &place.base {
            MirPlaceBase::Local(local) => auto
                .locals
                .iter()
                .find(|candidate| candidate.id == *local)
                .map(|local| local.name.clone()),
            MirPlaceBase::Parameter(value)
            | MirPlaceBase::Capture(value)
            | MirPlaceBase::Temporary(value) => auto
                .blocks
                .iter()
                .flat_map(|block| block.instructions.iter())
                .find_map(|instruction| {
                    (instruction.result == Some(*value)).then(|| match &instruction.operation {
                        MirOperation::Parameter { name, .. } => Some(name.clone()),
                        _ => None,
                    })?
                }),
            MirPlaceBase::Static(_) => None,
        }
        .unwrap_or_else(|| panic!("checked access has no semantic local name: {place:?}"));
        let has_index = place
            .projections
            .iter()
            .any(|projection| matches!(projection, MirProjection::Index { .. }));
        let indexes_cursor = place.projections.iter().any(|projection| {
            matches!(
                projection,
                MirProjection::Index { index, .. } if *index == cursor
            )
        });
        access_roles.push((
            semantic_name,
            has_index,
            indexes_cursor,
            place.projections.is_empty(),
        ));
    }
    access_roles.sort();
    assert_eq!(
        access_roles,
        vec![
            ("i".to_string(), false, false, true),
            ("output".to_string(), true, false, false),
            ("values".to_string(), true, true, false),
        ],
        "checked access IDs must resolve to the induction, output, and input local roles"
    );
    let output_place = auto
        .places
        .iter()
        .find(|place| {
            matches!(
                &place.base,
                MirPlaceBase::Local(local)
                    if auto
                        .locals
                        .iter()
                        .any(|candidate| candidate.id == *local && candidate.name == "output")
            ) && place
                .projections
                .iter()
                .any(|projection| matches!(projection, MirProjection::Index { .. }))
        })
        .and_then(|place| {
            place
                .projections
                .iter()
                .find_map(|projection| match projection {
                    MirProjection::Index { index, .. } => Some(*index),
                    _ => None,
                })
        })
        .expect("output access must retain its index identity");
    assert!(
        value_reads_place(
            auto,
            output_place,
            induction_local.place,
            &mut Vec::new()
        ),
        "output write index must be tied to the induction local: output index={output_place:?}, induction place={:?}",
        induction_local.place
    );
    assert!(checked.no_aliasing);
    assert!(checked.no_early_exit);
    assert!(checked.effect_free_body);
    assert!(checked.no_cross_iteration_dependencies);

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

#[test]
fn erased_loop_facts_do_not_make_the_function_vectorizable() {
    let source = r#"
fn erased(values: [Float#4]) [Float#4] -> {
    output := [Float#4]{0.0, 0.0, 0.0, 0.0}
    #Off {
        loop i in 0..<4 {
            output[i] = values[i] * 2.0
        }
    }
    return output
}

fn active(values: [Float#4]) [Float#4] -> {
    output := [Float#4]{0.0, 0.0, 0.0, 0.0}
    loop i in 0..<4 {
        output[i] = values[i] * 2.0
    }
    return output
}

fn run() {}
"#;
    let mir = lower_checked(source);
    let optimized =
        jet_foundation::MIR::optimize_mir_program(&mir, &MirOptimizationPolicy::conservative())
            .unwrap();
    let erased = function(&optimized, "erased");
    let active = function(&optimized, "active");
    assert!(
        !erased.optimization.auto_vectorizable,
        "erased loop facts must not make the function vectorizable: {:#?}",
        erased.optimization
    );
    assert!(
        active.optimization.auto_vectorizable,
        "active loop must retain its optimizer premise: {:#?}",
        active.optimization
    );
}

#[test]
fn elementwise_list_writes_keep_copy_cost_outside_the_loop() {
    let source = r#"
fn bare(values: [Float#4]) [Float#4] -> {
    output := [Float#4]{0.0, 0.0, 0.0, 0.0}
    loop i in 0..<4 {
        output[i] = values[i] * 2.0
    }
    return output
}

fn explicit(values: [Float#4]) [Float#4] -> {
    output := ~values
    loop i in 0..<4 {
        output[i] = values[i] * 2.0
    }
    return output
}

fn run() {}
"#;
    let mir = lower_checked(source);
    let policy = MirOptimizationPolicy::conservative();
    let optimized = jet_foundation::MIR::optimize_mir_program(&mir, &policy).unwrap();

    let bare = function(&optimized, "bare");
    let bare_loop = bare
        .optimization
        .loop_facts
        .first()
        .expect("bare elementwise loop fact");
    assert_eq!(bare_loop.copy_cost, MirCopyCost::None);
    assert!(
        bare.optimization
            .vector_facts
            .iter()
            .any(|fact| fact.decision.is_eligible()),
        "bare/read elementwise writes must remain vector-eligible"
    );

    let explicit = function(&optimized, "explicit");
    let explicit_loop = explicit
        .optimization
        .loop_facts
        .first()
        .expect("explicit-copy elementwise loop fact");
    assert_eq!(explicit_loop.copy_cost, MirCopyCost::None);
    let explicit_copies = explicit
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .filter(|instruction| matches!(&instruction.operation, MirOperation::Copy { .. }))
        .count();
    assert_eq!(
        explicit_copies, 1,
        "explicit copy must be materialized once before the loop"
    );
    assert!(
        explicit
            .optimization
            .vector_facts
            .iter()
            .any(|fact| fact.decision.is_eligible()),
        "explicit-copy destination must retain elementwise eligibility"
    );
}

#[test]
fn checked_vector_proof_conflicts_cannot_repromote_a_loop() {
    let source = r#"
fn auto(values: [Float#4]) [Float#4] -> {
    output := [Float#4]{0.0, 0.0, 0.0, 0.0}
    loop i in 0..<4 { output[i] = values[i] * 2.0 }
    return output
}

fn run() {}
"#;
    let mir = lower_checked(source);
    let policy = MirOptimizationPolicy::conservative();

    let mut missing = jet_foundation::MIR::optimize_mir_program(&mir, &policy).unwrap();
    function_mut(&mut missing, "auto")
        .optimization
        .checked_vector_facts
        .clear();
    function_mut(&mut missing, "auto")
        .optimization
        .pass_ids
        .clear();
    let missing = jet_foundation::MIR::optimize_mir_program(&missing, &policy).unwrap();
    assert!(
        function(&missing, "auto")
            .optimization
            .vector_facts
            .iter()
            .all(|fact| !fact.decision.is_eligible()),
        "missing checked proof must keep the loop scalar"
    );
    let mut duplicate = jet_foundation::MIR::optimize_mir_program(&mir, &policy).unwrap();
    let duplicate_proof = function(&duplicate, "auto")
        .optimization
        .checked_vector_facts
        .first()
        .cloned()
        .expect("auto loop checked proof");
    function_mut(&mut duplicate, "auto")
        .optimization
        .checked_vector_facts
        .push(duplicate_proof);
    function_mut(&mut duplicate, "auto")
        .optimization
        .pass_ids
        .clear();
    let duplicate = jet_foundation::MIR::optimize_mir_program(&duplicate, &policy).unwrap();
    assert!(
        function(&duplicate, "auto")
            .optimization
            .vector_facts
            .iter()
            .all(|fact| !fact.decision.is_eligible()),
        "duplicate checked proofs must remain conservatively rejected"
    );


    let mut conflict = jet_foundation::MIR::optimize_mir_program(&mir, &policy).unwrap();
    let proof = function_mut(&mut conflict, "auto")
        .optimization
        .checked_vector_facts
        .first_mut()
        .expect("auto loop checked proof");
    proof.no_cross_iteration_dependencies = false;
    function_mut(&mut conflict, "auto")
        .optimization
        .pass_ids
        .clear();
    let conflict = jet_foundation::MIR::optimize_mir_program(&conflict, &policy).unwrap();
    let auto = function(&conflict, "auto");
    assert!(
        auto.optimization
            .vector_facts
            .iter()
            .all(|fact| !fact.decision.is_eligible()),
        "conflicting checked proof must not be re-promoted"
    );
    assert!(
        auto.optimization
            .vector_facts
            .iter()
            .any(|fact| !fact.no_cross_iteration_dependencies),
        "the rejected cross-iteration fact must remain visible"
    );

    let mut stale_access = jet_foundation::MIR::optimize_mir_program(&mir, &policy).unwrap();
    let proof = function_mut(&mut stale_access, "auto")
        .optimization
        .checked_vector_facts
        .first_mut()
        .expect("auto loop checked proof");
    assert!(!proof.accesses.is_empty());
    proof.accesses.clear();
    function_mut(&mut stale_access, "auto")
        .optimization
        .pass_ids
        .clear();
    let stale_access =
        jet_foundation::MIR::optimize_mir_program(&stale_access, &policy).unwrap();
    assert!(
        function(&stale_access, "auto")
            .optimization
            .vector_facts
            .iter()
            .all(|fact| !fact.decision.is_eligible()),
        "a proof with stale access identity must not re-promote the loop"
    );

    let mut changed_cfg = jet_foundation::MIR::optimize_mir_program(&mir, &policy).unwrap();
    let checked = function(&changed_cfg, "auto")
        .optimization
        .checked_vector_facts
        .first()
        .cloned()
        .expect("auto loop checked proof");
    let exit = function(&changed_cfg, "auto")
        .optimization
        .loop_facts
        .iter()
        .find(|row| row.header == checked.loop_header)
        .and_then(|row| row.exit)
        .expect("auto loop exit");
    let body = checked.body_blocks[0];
    function_mut(&mut changed_cfg, "auto")
        .blocks
        .iter_mut()
        .find(|block| block.id == body)
        .expect("auto loop body block")
        .terminator = jet_foundation::MIR::MirTerminator::Jump { target: exit };
    function_mut(&mut changed_cfg, "auto")
        .optimization
        .pass_ids
        .clear();
    let changed_cfg =
        jet_foundation::MIR::optimize_mir_program(&changed_cfg, &policy).unwrap();
    assert!(
        function(&changed_cfg, "auto")
            .optimization
            .vector_facts
            .iter()
            .all(|fact| !fact.decision.is_eligible()),
        "CFG mutation must invalidate the checked loop scope"
    );

    let mut explicit_scalar = jet_foundation::MIR::optimize_mir_program(&mir, &policy).unwrap();
    function_mut(&mut explicit_scalar, "auto").is_scalar = true;
    function_mut(&mut explicit_scalar, "auto")
        .optimization
        .pass_ids
        .clear();
    let explicit_scalar =
        jet_foundation::MIR::optimize_mir_program(&explicit_scalar, &policy).unwrap();
    assert!(
        function(&explicit_scalar, "auto")
            .optimization
            .vector_facts
            .iter()
            .all(|fact| !fact.decision.is_eligible()),
        "an explicit scalar fact must stay on the scalar path"
    );
}

#[test]
fn columnar_field_arithmetic_keeps_packed_direct_layout() {
    let source = r#"
#Layout(columnar)
struct Particle {
    x: Float
    y: Float
    mass: Float
}

fn energy(ps: [Particle#4]) [Float#4] -> {
    output := [Float#4]{0.0, 0.0, 0.0, 0.0}
    loop i in 0..<4 {
        output[i] = ps[i].x * ps[i].y + ps[i].mass
    }
    return output
}

fn run() {}
"#;
    let mir = lower_checked(source);
    let policy = MirOptimizationPolicy::conservative();
    let optimized = jet_foundation::MIR::optimize_mir_program(&mir, &policy).unwrap();
    let energy = function(&optimized, "energy");
    let fact = energy
        .optimization
        .vector_facts
        .iter()
        .find(|fact| fact.layout == jet_foundation::MIR::MirVectorLayout::ColumnarDirect)
        .unwrap_or_else(|| panic!("missing direct columnar vector fact: {:#?}", energy.optimization.vector_facts));
    assert!(
        fact.decision.is_eligible(),
        "columnar arithmetic should be eligible: {fact:#?}"
    );
    assert!(
        fact.packed,
        "columnar arithmetic should retain packed lowering: {fact:#?}"
    );
    assert!(
        fact.no_cross_iteration_dependencies,
        "independent field reads must not inherit a scalar loop-carried dependency: {fact:#?}"
    );
    assert!(
        fact.accesses
            .iter()
            .any(|access| access.layout == jet_foundation::MIR::MirVectorLayout::ColumnarDirect),
        "vector fact must retain a direct columnar access: {fact:#?}"
    );
    assert!(
        energy
            .optimization
            .acceleration_facts
            .iter()
            .all(|fact| {
                fact.proof.source_proven
                    && fact.proof.no_aliasing
                    && fact.proof.no_cross_iteration_dependencies
                    && fact.proof.no_early_exit
                    && fact.proof.effect_free_body
                    && fact.proof.ownership_safe
                    && fact.proof.failure_order_preserved
            }),
        "columnar acceleration must retain every checked safety guard"
    );
}

fn value_reads_place(
    function: &jet_foundation::MIR::MirFunction,
    value: jet_foundation::MIR::MirValueId,
    place: jet_foundation::MIR::MirPlaceId,
    seen: &mut Vec<jet_foundation::MIR::MirValueId>,
) -> bool {
    if seen.contains(&value) {
        return false;
    }
    seen.push(value);
    let Some(instruction) = function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .find(|instruction| instruction.result == Some(value))
    else {
        return false;
    };
    match &instruction.operation {
        MirOperation::ReadPlace(candidate) | MirOperation::MovePlace { place: candidate } => {
            *candidate == place
        }
        MirOperation::Copy { value }
        | MirOperation::Move { value }
        | MirOperation::AttachTag { value, .. }
        | MirOperation::Convert { value, .. } => value_reads_place(function, *value, place, seen),
        _ => false,
    }
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
