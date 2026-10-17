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
        let (diagnostics, _facts, nominals) = jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diagnostics
            .into_iter().filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)).collect();
        assert!(errors.is_empty(), "{errors:#?}");
        let request = MirArtifactRequest::new(MirArtifactTarget::RustAot,
            MirArtifactKind::NativeExecutable, MirArtifactBuildMode::Dev);
        let tir = jet::Codegen::TIR::lower_checked_tir_program_for(&bundle, &nominals, request).unwrap();
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
        assert!(mir_operation_can_stop(&operation, None, None, |_| Some(Effect::Panic)));
        assert!(!mir_operation_can_stop(&operation, None, None, |_| None));
        assert!(!mir_operation_can_stop(&operation, None, None, |_| Some(Effect::Mem)));
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
        assert!(mir_operation_can_stop(&operation, None, None, |_| Some(Effect::Panic)));
        assert!(!mir_operation_can_stop(&operation, None, None, |_| None));
    }
    for op in [MirBinaryOp::BitAnd, MirBinaryOp::BitOr, MirBinaryOp::BitXor] {
        let operation = MirOperation::Binary { op, dispatch: MirBinaryDispatch::Primitive,
            left: MirValueId(1), right: MirValueId(2) };
        assert!(!mir_operation_can_stop(&operation, None, None, |_| Some(Effect::Panic)));
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
        let (diagnostics, _facts, nominals) = jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diagnostics
            .into_iter().filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)).collect();
        assert!(errors.is_empty(), "{errors:#?}");
        let request = MirArtifactRequest::new(MirArtifactTarget::RustAot,
            MirArtifactKind::NativeExecutable, MirArtifactBuildMode::Dev);
        let tir = jet::Codegen::TIR::lower_checked_tir_program_for(&bundle, &nominals, request).unwrap();
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

#[test]
fn normal_cleanup_stop_consumes_first_and_runs_remaining_once() {
    use jet_foundation::MIR::{MirAccess, MirOpId, MirScopeKind, MirValueId};
    use std::collections::{HashMap, HashSet};
    jet::run_compiler_work(|| {
        let scratch = common::Scratch::new("mir_cleanup_stop");
        std::fs::write(scratch.path.join("package.jet"),
            "name: \"mir_cleanup_stop\"\nversion: \"0.1.0\"\n").unwrap();
        let entry = scratch.path.join("main.jet");
        std::fs::write(&entry, include_str!("fixtures/mir_cleanup_stop.jet")).unwrap();
        let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
        let (diagnostics, _facts, nominals) = jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diagnostics
            .into_iter().filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)).collect();
        assert!(errors.is_empty(), "{errors:#?}");
        let request = MirArtifactRequest::new(MirArtifactTarget::RustAot,
            MirArtifactKind::NativeExecutable, MirArtifactBuildMode::Dev);
        let tir = jet::Codegen::TIR::lower_checked_tir_program_for(&bundle, &nominals, request).unwrap();
        let mut mir = jet::Codegen::TIR::lower_native_tir_to_mir(tir).unwrap();
        mir.validate().unwrap();
        let function = mir.functions.iter_mut().find(|function|
            function.locals.iter().any(|local| local.name == "first")).unwrap();
        let first = function.locals.iter().find(|local| local.name == "first").unwrap().place;
        let remaining = function.locals.iter().find(|local| local.name == "remaining").unwrap().place;
        let moved: HashMap<_, _> = function.blocks.iter().flat_map(|block| &block.instructions)
            .filter_map(|instruction| match (&instruction.operation, instruction.result) {
                (MirOperation::MovePlace { place }, Some(value)) => Some((value, *place)),
                _ => None,
            }).collect();
        let cleanup_place = |operation: &MirOperation| match operation {
            MirOperation::Call { args, .. } => args.iter()
                .find(|arg| arg.access == MirAccess::Move).and_then(|arg| moved.get(&arg.value)).copied(),
            _ => None,
        };
        let (stop, mut duplicate) = function.blocks.iter().find_map(|block| {
            let instruction = block.instructions.last()?;
            if cleanup_place(&instruction.operation) != Some(first) { return None; }
            match block.terminator {
                MirTerminator::Poll { kind: MirPollKind::Propagate, stop, .. } =>
                    Some((stop, instruction.clone())),
                _ => None,
            }
        }).expect("normal first cleanup has no propagation edge");
        let mut pending = vec![stop];
        let mut seen = HashSet::new();
        let mut remaining_calls = 0;
        let cleanup_scopes: HashSet<_> = function.scopes.iter()
            .filter(|scope| scope.kind == MirScopeKind::StopCleanup)
            .map(|scope| scope.id).collect();
        let mut cleanup_entries = 0;
        let mut cleanup_leaves = 0;
        while let Some(id) = pending.pop() {
            if !seen.insert(id) { continue; }
            let block = function.blocks.iter().find(|block| block.id == id).unwrap();
            for instruction in &block.instructions {
                match instruction.operation {
                    MirOperation::ScopeEnter { scope, .. } if cleanup_scopes.contains(&scope) => cleanup_entries += 1,
                    MirOperation::ScopeExit { scope } if cleanup_scopes.contains(&scope) => cleanup_leaves += 1,
                    _ => {}
                }
                match cleanup_place(&instruction.operation) {
                    Some(place) if place == first => panic!("first cleanup was replayed"),
                    Some(place) if place == remaining => remaining_calls += 1,
                    _ => {}
                }
            }
            assert!(!matches!(block.terminator, MirTerminator::Poll { .. }),
                "propagation cleanup recursively polled");
            pending.extend(block.terminator.targets());
        }
        assert_eq!(remaining_calls, 1, "stop edge must run remaining cleanup exactly once");
        assert!(cleanup_entries > 0, "stop cleanup lacks its typed runtime owner");
        assert_eq!(cleanup_entries, cleanup_leaves, "stop cleanup must restore every token");
        let cleanup_exit = function.blocks.iter().find_map(|block|
            block.instructions.iter().position(|instruction| matches!(instruction.operation,
                MirOperation::ScopeExit { scope } if cleanup_scopes.contains(&scope)))
                .map(|index| (block.id, index))).unwrap();
        let function_id = function.id;
        let mut missing_leave_function = function.clone();
        missing_leave_function.blocks.iter_mut().find(|row| row.id == cleanup_exit.0).unwrap()
            .instructions.remove(cleanup_exit.1);
        duplicate.id = MirOpId(function.blocks.iter().flat_map(|block| &block.instructions)
            .map(|instruction| instruction.id.0).max().unwrap() + 1);
        if let Some(old) = duplicate.result {
            let new = MirValueId(function.values.iter().map(|row| row.0.0).max().unwrap() + 1);
            let mut metadata = function.values.iter().find(|row| row.0 == old).unwrap().clone();
            metadata.0 = new;
            function.values.push(metadata);
            duplicate.result = Some(new);
        }
        function.blocks.iter_mut().find(|block| block.id == stop).unwrap()
            .instructions.insert(0, duplicate);
        let error = mir.validate().expect_err("verifier accepted replay of first cleanup");
        assert!(format!("{error:?}").contains("already-consumed cleanup input"), "{error:?}");
        let mut missing_leave = mir.clone();
        *missing_leave.functions.iter_mut().find(|row| row.id == function_id).unwrap() = missing_leave_function;
        let error = missing_leave.validate().expect_err("verifier accepted an open StopCleanup token");
        assert!(format!("{error:?}").contains("StopCleanup"), "{error:?}");
    });
}

/// FrontFeed.jet consumes this exact op table too. Its focused runner compares
/// the two rendered result files, including all input facts, not just totals.
#[test]
fn shared_stop_predicate_table() {
    use jet_foundation::Diagnostics::Span;
    use jet_foundation::Effects::Effect;
    use jet_foundation::MIR::*;
    let table = include_str!("fixtures/mir_stop_predicate.tsv");
    let mut rendered = String::new();
    for row in table.lines() {
        let columns: Vec<_> = row.split('\t').collect();
        assert_eq!(columns.len(), 8, "{row}");
        let value = MirValueId(1);
        let place_id = MirPlaceId(1);
        let getter = MirPreludeCallId(1);
        let setter = MirPreludeCallId(2);
        let route = MirPreludeCallId(3);
        let fixed = || MirType::from_kind(MirTypeKind::IntN { signed: true, bits: 8 });
        let ty = match columns[2] {
            "i8" => fixed(),
            "u64" => MirType::from_kind(MirTypeKind::IntN { signed: false, bits: 64 }),
            "int" => MirType::from_kind(MirTypeKind::Int),
            "float" => MirType::from_kind(MirTypeKind::Float),
            "float32" => MirType::from_kind(MirTypeKind::Float32),
            "tagged-i8" => MirType::from_kind(MirTypeKind::Tagged {
                marker: MirTagMarker::User("checked".into()), inner: Box::new(fixed()) }),
            "range-i8" => MirType::from_kind(MirTypeKind::InlineRange {
                base: Box::new(fixed()), lo: -8, hi: 8 }),
            "quantity-i8" => MirType::from_kind(MirTypeKind::Quantity {
                base: Box::new(fixed()), dimension: MirDimension::scalar() }),
            "list" => MirType::from_kind(MirTypeKind::List(Box::new(fixed()))),
            "map" => MirType::from_kind(MirTypeKind::Map {
                key: Box::new(fixed()), value: Box::new(fixed()) }),
            other => panic!("unknown predicate type {other}"),
        };
        let location = MirPanicLoc { file: MirSourceFileId(1), line: 1, column: 1 };
        let context = MirPanicContext { function: "predicate".into(), source_line: "".into(), caret: 0, locals: vec![] };
        let operation = match columns[1] {
            "neg" => MirOperation::Unary { op: MirUnaryOp::Neg, value },
            "not" => MirOperation::Unary { op: MirUnaryOp::Not, value },
            "cast" => MirOperation::Convert { value, parameters: vec![], target: ty.clone(), conversion: MirConversion::NumericCast },
            "binary-trap" | "binary-prelude" => MirOperation::Binary {
                op: MirBinaryOp::Add, dispatch: MirBinaryDispatch::Prelude { call: route, location: None },
                left: value, right: value },
            "binary-total" => MirOperation::Binary {
                op: MirBinaryOp::Add, dispatch: MirBinaryDispatch::Primitive, left: value, right: value },
            "read" => MirOperation::ReadPlace(place_id),
            "move" => MirOperation::MovePlace { place: place_id },
            "write" => MirOperation::WritePlace { place: place_id, value },
            "replace" => MirOperation::ReplacePlace { place: place_id, value },
            "scope-exit" => MirOperation::ScopeExit { scope: MirScopeId(1) },
            "require-stop" => MirOperation::Semantic(MirSemanticOp::RequireStop {
                call: route, kind: MirRequireKind::Panic, condition: None, location: location.clone(),
                context, values: vec![], always_stops: true }),
            "call" => MirOperation::Call { callee: MirCallee::User(MirFunctionId(1)), args: vec![], type_args: vec![] },
            "indirect-call" => MirOperation::IndirectCall { callee: value, args: vec![], type_args: vec![] },
            "core-call" => MirOperation::CoreCall { call: MirCoreCallId(1), route, args: vec![],
                type_args: vec![], fallibility: MirCallFallibility::Infallible, data_plan: None },
            other => panic!("unknown predicate operation {other}"),
        };
        let place = MirPlace { id: place_id, span: Span { start: 0, end: 0 }, ty: ty.clone(),
            base: MirPlaceBase::Temporary(value), access: MirAccess::Read, persist_key: None,
            projections: vec![MirProjection::Index {
                kind: if columns[2] == "map" { MirIndexKind::Map } else { MirIndexKind::List },
                index: value, call: getter, write_call: Some(setter), location, context: None,
                span: Span { start: 0, end: 0 } }] };
        let effect = |call| {
            let fact = if call == getter { columns[3] } else if call == setter { columns[4] }
                else if call == route { columns[5] } else { panic!("unlisted predicate route {call:?}") };
            match fact { "Panic" => Some(Effect::Panic), "None" => None, other => panic!("unknown effect {other}") }
        };
        let stop = mir_operation_can_stop(&operation, Some(&ty), Some(&place), effect);
        let panic = mir_operation_can_panic(&operation, Some(&ty), Some(&place), effect);
        assert_eq!(stop.to_string(), columns[6], "{} stop", columns[0]);
        assert_eq!(panic.to_string(), columns[7], "{} panic", columns[0]);
        rendered.push_str(&format!("{}\t{stop}\t{panic}\n", columns[..6].join("\t")));
    }
    if let Some(path) = std::env::var_os("JET_STOP_PREDICATE_RESULTS") {
        std::fs::write(path, rendered).expect("write exercised Rust predicate results");
    }
}

#[test]
fn explicit_process_stops_close_defers_before_guards() {
    use jet_foundation::MIR::{MirBlockId, MirConstant, MirPlaceId, MirValueId};
    use std::collections::{HashMap, HashSet};
    jet::run_compiler_work(|| {
        let scratch = common::Scratch::new("mir_explicit_cleanup_order");
        let entry = scratch.path.join("main.jet");
        std::fs::write(&entry, include_str!("fixtures/mir_explicit_cleanup_order.jet")).unwrap();
        let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
        let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
            .into_iter().filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)).collect();
        assert!(errors.is_empty(), "{errors:#?}");
        let request = MirArtifactRequest::new(MirArtifactTarget::RustAot,
            MirArtifactKind::NativeExecutable, MirArtifactBuildMode::Dev);
        let tir = jet::Codegen::TIR::lower_checked_tir_program_for(&bundle, request).unwrap();
        let mir = jet::Codegen::TIR::lower_native_tir_to_mir(tir).unwrap();
        mir.validate().unwrap();
        let mut witnessed = Vec::new();
        for name in ["exit_case", "stop_case"] {
            let function = mir.functions.iter().find(|function| function.name == name).unwrap();
            let owners: HashMap<MirPlaceId, &str> = function.locals.iter()
                .filter(|local| matches!(local.name.as_str(), "first" | "second" | "_guard_1" | "_guard_2"))
                .map(|local| (local.place, local.name.as_str())).collect();
            assert_eq!(owners.len(), 4, "{name} lost its checked cleanup owners");
            let mut booleans: HashMap<MirValueId, bool> = HashMap::new();
            let mut flags: HashMap<MirPlaceId, bool> = HashMap::new();
            let mut seen: HashSet<MirBlockId> = HashSet::new();
            let mut current = function.entry;
            let mut order = Vec::new();
            'path: loop {
                assert!(seen.insert(current), "{name} cleanup path cycles");
                let block = function.blocks.iter().find(|block| block.id == current).unwrap();
                for instruction in &block.instructions {
                    if let MirOperation::CoreCall { call, .. } = instruction.operation {
                        let row = mir.core_calls.iter().find(|row| row.id == call).unwrap();
                        if (row.module == "core.process" && row.member == "exit")
                            || (row.module == "core.sys" && row.member == "stop")
                        {
                            assert_eq!(order, ["second", "first", "_guard_2", "_guard_1"], "{name}");
                            witnessed.push(format!("{name}\t{}\n", order.join("\t")));
                            break 'path;
                        }
                    }
                    match (&instruction.operation, instruction.result) {
                        (MirOperation::Constant(MirConstant::Bool(value)), Some(id)) => {
                            booleans.insert(id, *value);
                        }
                        (MirOperation::ReadPlace(place), Some(id)) => {
                            if let Some(value) = flags.get(place) { booleans.insert(id, *value); }
                        }
                        (MirOperation::WritePlace { place, value }, _) => {
                            if let Some(value) = booleans.get(value) { flags.insert(*place, *value); }
                        }
                        (MirOperation::MovePlace { place }, _) => {
                            if let Some(owner) = owners.get(place) { order.push(*owner); }
                        }
                        _ => {}
                    }
                }
                current = match block.terminator {
                    MirTerminator::Jump { target } => target,
                    MirTerminator::Poll { resume, .. } => resume,
                    MirTerminator::Branch { condition, then_target, else_target } => {
                        if *booleans.get(&condition).expect("fixture live flag has no checked state") {
                            then_target
                        } else {
                            else_target
                        }
                    }
                    _ => panic!("{name} left its ordinary path before the explicit stop"),
                };
            }
        }
        if let Some(path) = std::env::var_os("JET_EXPLICIT_CLEANUP_ORDER_RESULTS") {
            let mut fixture_path = path.clone();
            fixture_path.push(".fixture.jet");
            std::fs::write(fixture_path, include_bytes!("fixtures/mir_explicit_cleanup_order.jet"))
                .expect("write the fixture actually checked by the Rust cleanup producer");
            std::fs::write(path, witnessed.concat()).expect("write exercised Rust cleanup order");
        }
    });
}
