//! A stopping Prelude producer must be followed by Propagate before its result
//! is bound or consumed. Nonstopping Prelude calls must not acquire a poll.
mod common;

use jet_foundation::MIR::{MirArtifactBuildMode, MirArtifactKind, MirArtifactRequest,
    MirArtifactTarget, MirOperation, MirPollKind, MirSemanticOp, MirTerminator};

#[test]
fn authority_with_propagates_before_binding_its_result() {
    jet::run_compiler_work(|| {
        let scratch = common::Scratch::new("mir_prelude_stop_points");
        std::fs::write(scratch.path.join("package.jet"),
            "name: \"mir_prelude_stop_points\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [IO] } }\n").unwrap();
        let entry = scratch.path.join("main.jet");
        // The parameter keeps the request dynamic, so denial cannot be folded.
        std::fs::write(&entry, r#"
fn restrict(request: String) {
    #FX(authority: IO) {
        denied :: authority.with(request)
        print("must not run after denial")
        _ :: denied.without("IO")
    }
}
fn run() { restrict("Net") }
"#).unwrap();
        let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
        let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
            .into_iter().filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)).collect();
        assert!(errors.is_empty(), "{errors:#?}");
        let request = MirArtifactRequest::new(MirArtifactTarget::RustAot,
            MirArtifactKind::NativeExecutable, MirArtifactBuildMode::Dev);
        let tir = jet::Codegen::TIR::lower_checked_tir_program_for(&bundle, request).unwrap();
        let mir = jet::Codegen::TIR::lower_native_tir_to_mir(tir).unwrap();
        let mut stopping = 0;
        let mut nonstopping = 0;
        for function in &mir.functions {
            for block in &function.blocks {
                for (index, instruction) in block.instructions.iter().enumerate() {
                    let MirOperation::Semantic(MirSemanticOp::StaticPreludeCall { call, .. }) = &instruction.operation else { continue };
                    let route = mir.prelude_calls.iter().find(|row| row.id == *call).unwrap();
                    if route.module != "::JetAuthority" { continue; }
                    if route.member == "with" {
                        stopping += 1;
                        assert_eq!(index + 1, block.instructions.len(), "binding/consumer before Propagate");
                        assert!(matches!(block.terminator, MirTerminator::Poll { kind: MirPollKind::Propagate, .. }),
                            "Authority.with has no Propagate: {:?}", block.terminator);
                    } else if route.member == "without" {
                        nonstopping += 1;
                        assert!(!(index + 1 == block.instructions.len() && matches!(block.terminator,
                            MirTerminator::Poll { kind: MirPollKind::Propagate, .. })), "nonstopping Prelude polled");
                    }
                }
            }
        }
        assert!(stopping > 0 && nonstopping > 0);
        mir.validate().unwrap();
    });
}

#[test]
fn prelude_stop_classification_uses_checked_effect_not_call_spelling() {
    use jet_foundation::MIR::{mir_operation_can_stop, MirPreludeCallId, MirValueId};
    use jet_foundation::Effects::{prelude_route_effect, Effect};
    let call = MirPreludeCallId(1);
    let map_call = MirPreludeCallId(2);
    let value = MirValueId(1);
    let operations = [
        MirSemanticOp::StaticPreludeCall { call, args: vec![], owner_type_args: vec![], type_args: vec![] },
        MirSemanticOp::SharedGuardMap { call, guard: value, path: vec![], editable: false },
        MirSemanticOp::SharedGuardSplit { call, map_call, guard: value, first: vec![], second: vec![], editable: false },
        MirSemanticOp::SharedGuardWait { call, guard: value, condition: value, predicate: value },
    ];
    for semantic in operations {
        let operation = MirOperation::Semantic(semantic);
        assert!(mir_operation_can_stop(&operation, |_| Some(Effect::Panic)));
        assert!(!mir_operation_can_stop(&operation, |_| None));
        assert!(!mir_operation_can_stop(&operation, |_| Some(Effect::Mem)));
    }
    assert_eq!(prelude_route_effect("::JetAuthority", "with"), Some(Effect::Panic));
    assert_eq!(prelude_route_effect("::JetAuthority", "without"), None);
    assert_eq!(prelude_route_effect("::JetAtomic", "try_new"), Some(Effect::Mem));
    for member in ["map_read", "map_edit", "split_read", "split_edit", "wait"] {
        assert_eq!(prelude_route_effect("core.shared_guard", member), Some(Effect::Panic));
    }
}

#[test]
fn checked_slice_routes_publish_panic_stop_facts() {
    use jet_foundation::Effects::{prelude_route_effect, Effect};
    for (module, member) in [
        ("core.collections", "slice_string"),
        ("core.collections", "slice_string_range"),
        ("core.builtin", "string_slice"),
        ("core.collections", "slice_list"),
        ("core.collections", "slice_list_range"),
    ] {
        assert_eq!(prelude_route_effect(module, member), Some(Effect::Panic));
    }
    assert_eq!(prelude_route_effect("core.collections", "loop_range_has_next"), None);
}

#[test]
fn checked_numeric_policy_classifies_only_trapping_operations() {
    use jet_foundation::MIR::{mir_binary_can_trap, mir_operation_can_stop,
        MirBinaryOp, MirBinaryDispatch, MirPreludeCallId, MirValueId};
    use jet_foundation::Effects::{prelude_route_effect, Effect};
    let dispatch = MirBinaryDispatch::Prelude { call: MirPreludeCallId(1), location: None };
    assert!(mir_binary_can_trap(&dispatch, |_| Some(Effect::Panic)));
    assert!(!mir_binary_can_trap(&dispatch, |_| None));
    assert!(!mir_binary_can_trap(&MirBinaryDispatch::Primitive, |_| Some(Effect::Panic)));
    for op in [MirBinaryOp::Add, MirBinaryOp::Sub, MirBinaryOp::Mul,
        MirBinaryOp::Div, MirBinaryOp::FloorDiv, MirBinaryOp::Mod, MirBinaryOp::Rem,
        MirBinaryOp::Pow, MirBinaryOp::Shl, MirBinaryOp::Shr] {
        let operation = MirOperation::Binary { op, dispatch: dispatch.clone(),
            left: MirValueId(1), right: MirValueId(2) };
        assert!(mir_operation_can_stop(&operation, |_| Some(Effect::Panic)));
        assert!(!mir_operation_can_stop(&operation, |_| None));
    }
    for op in [MirBinaryOp::BitAnd, MirBinaryOp::BitOr, MirBinaryOp::BitXor] {
        let operation = MirOperation::Binary { op, dispatch: MirBinaryDispatch::Primitive,
            left: MirValueId(1), right: MirValueId(2) };
        assert!(!mir_operation_can_stop(&operation, |_| Some(Effect::Panic)));
    }
    for member in ["div_euclid", "rem_euclid"] {
        assert_eq!(prelude_route_effect("core.numeric", member), Some(Effect::Panic));
    }
}

#[test]
fn panic_propagation_crosses_shield_but_cancellation_checks_do_not() {
    jet::run_compiler_work(|| {
        let scratch = common::Scratch::new("mir_shield_panic");
        std::fs::write(scratch.path.join("package.jet"),
            "name: \"mir_shield_panic\"\nversion: \"0.1.0\"\n").unwrap();
        let entry = scratch.path.join("main.jet");
        std::fs::write(&entry, r#"
fn trap(n: I64) {
    shield {
        result :: n + I64{1}
        print(result)
    }
}
fn run() { trap(I64{9223372036854775807}) }
"#).unwrap();
        let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
        let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
            .into_iter().filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)).collect();
        assert!(errors.is_empty(), "{errors:#?}");
        let request = MirArtifactRequest::new(MirArtifactTarget::RustAot,
            MirArtifactKind::NativeExecutable, MirArtifactBuildMode::Dev);
        let tir = jet::Codegen::TIR::lower_checked_tir_program_for(&bundle, request).unwrap();
        let mut mir = jet::Codegen::TIR::lower_native_tir_to_mir(tir).unwrap();
        mir.validate().unwrap();
        let mut witnessed = false;
        for function in &mut mir.functions {
            for block in &mut function.blocks {
                if block.instructions.last().is_some_and(|instruction|
                    matches!(instruction.operation, MirOperation::Binary {
                        dispatch: jet_foundation::MIR::MirBinaryDispatch::Prelude { .. }, .. })) {
                    let MirTerminator::Poll { kind, .. } = &mut block.terminator else {
                        panic!("shielded trapping operation has no propagation edge");
                    };
                    assert_eq!(*kind, MirPollKind::Propagate);
                    // A cancellation-only check must remain masked at this site.
                    *kind = MirPollKind::Check;
                    witnessed = true;
                    break;
                }
            }
            if witnessed { break; }
        }
        assert!(witnessed);
        assert!(mir.validate().is_err(), "verifier accepted a cancellation check under Shield");
    });
}
