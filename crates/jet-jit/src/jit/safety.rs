use jet_foundation::MIR::{
    MirAbi, MirAccess, MirCallee, MirFunction, MirFunctionId, MirOperation, MirProgram, MirSemanticOp,
    MirTypeDefKind,
};

/// Check the canonical facts required by this adapter.  Legality and target
/// applicability are decided upstream; this function only consumes those
/// verdicts and verifies that every operation has an emitter arm.
pub(crate) fn resident_safe_mir_program(program: &MirProgram) -> Result<(), String> {
    program
        .validate()
        .map_err(|error| format!("invalid MIR: {error}"))?;
    for function in &program.functions {
        resident_safe_mir_function(function)?;
    }
    Ok(())
}

pub(crate) fn resident_safe_mir_function(function: &MirFunction) -> Result<(), String> {
    if !function.target_applicability.cranelift {
        return Err(format!("MIR function `{}` is not applicable to Cranelift", function.key));
    }
    for parameter in &function.params {
        if matches!(parameter.ty.layout.abi, MirAbi::Never) {
            return Err(format!("MIR function `{}` has a never parameter", function.key));
        }
    }
    for block in &function.blocks {
        for instruction in &block.instructions {
            check_operation(function, &instruction.operation)?;
        }
        check_terminator(&block.terminator)?;
    }
    Ok(())
}

fn check_operation(function: &MirFunction, operation: &MirOperation) -> Result<(), String> {
    match operation {
        MirOperation::MovePlace { place } => {
            let Some(place_row) = function.places.iter().find(|candidate| candidate.id == *place) else {
                return Err(format!("MIR move place {:?} is missing", place));
            };
            if place_row.access != MirAccess::Move {
                return Err(format!(
                    "MIR move place {:?} does not have move access",
                    place
                ));
            }
            Ok(())
        }
        MirOperation::Parameter { .. }
        | MirOperation::Capture { .. }
        | MirOperation::Global { .. }
        | MirOperation::Phi { .. }
        | MirOperation::ReadPlace(_)
        | MirOperation::WritePlace { .. }
        | MirOperation::InitializeUninit { .. }
        | MirOperation::Copy { .. }
        | MirOperation::Move { .. }
        | MirOperation::Constant(_)
        | MirOperation::Unary { .. }
        | MirOperation::Binary { .. }
        | MirOperation::BuildString { .. }
        | MirOperation::BuildList { .. }
        | MirOperation::BuildMap { .. }
        | MirOperation::EnumIs { .. }
        | MirOperation::EnumPayload { .. }
        | MirOperation::OptionIsSome { .. }
        | MirOperation::OptionValue { .. }
        | MirOperation::ResultIsOk { .. }
        | MirOperation::ResultValue { .. }
        | MirOperation::PatternCapture { .. }
        | MirOperation::PatternMatched { .. }
        | MirOperation::ProjectMembers { .. }
        | MirOperation::Index { .. }
        | MirOperation::Slice { .. }
        | MirOperation::Range { .. }
        | MirOperation::Field { .. }
        | MirOperation::Struct { .. }
        | MirOperation::Enum { .. }
        | MirOperation::Tuple { .. }
        | MirOperation::Present { .. }
        | MirOperation::Convert { .. }
        | MirOperation::Absent
        | MirOperation::ResultOk { .. }
        | MirOperation::ResultErr { .. }
        | MirOperation::Call { .. }
        | MirOperation::IndirectCall { .. }
        | MirOperation::Closure { .. }
        | MirOperation::PtrFromAddr { .. }
        | MirOperation::Deref { .. }
        | MirOperation::RawAddressOf { .. }
        | MirOperation::AddressOf { .. }
        | MirOperation::CoreCall { .. }
        | MirOperation::AttachTag { .. }
        | MirOperation::Todo { .. }
        | MirOperation::Never { .. }
        | MirOperation::Semantic(MirSemanticOp::DataEntriesToMap { .. })
        | MirOperation::Semantic(MirSemanticOp::MathBuiltin { .. })
        | MirOperation::Semantic(MirSemanticOp::PreciseBuiltin { .. })
        | MirOperation::Semantic(MirSemanticOp::Print { .. })
        | MirOperation::Semantic(MirSemanticOp::AmbientInput { .. })
        | MirOperation::Semantic(MirSemanticOp::RequireStop { .. })
        | MirOperation::Semantic(MirSemanticOp::LayoutCompare { .. })
        | MirOperation::Semantic(MirSemanticOp::LayoutLiteral { .. })
        | MirOperation::Semantic(MirSemanticOp::StructLiteral { .. })
        | MirOperation::Semantic(MirSemanticOp::SharedGuardSplit { .. })
        | MirOperation::Semantic(MirSemanticOp::SharedGuardWait { .. })
        | MirOperation::Semantic(MirSemanticOp::ConditionNotify { .. })
        | MirOperation::Semantic(MirSemanticOp::AllocNew { .. })
        | MirOperation::Semantic(MirSemanticOp::ColumnarRead { .. })
        | MirOperation::Semantic(MirSemanticOp::StaticPreludeCall { .. })
        | MirOperation::Semantic(MirSemanticOp::DecodeUnder { .. })
        | MirOperation::Semantic(MirSemanticOp::BuiltinMethod { .. })
        | MirOperation::Semantic(MirSemanticOp::OptionLift2 { .. })
        | MirOperation::Semantic(MirSemanticOp::ClosureMethod { .. })
        | MirOperation::Semantic(MirSemanticOp::HostBorrowCallback { .. })
        | MirOperation::Semantic(MirSemanticOp::TextPatternMatch { .. })
        | MirOperation::Semantic(MirSemanticOp::BinaryPatternMatch { .. })
        | MirOperation::Semantic(MirSemanticOp::NumericMethod { .. })
        | MirOperation::Semantic(MirSemanticOp::NumericBinaryMethod { .. })
        | MirOperation::Semantic(MirSemanticOp::OverflowOption { .. })
        | MirOperation::Semantic(MirSemanticOp::HandleMethod { .. })
        | MirOperation::Semantic(MirSemanticOp::CoreClosureCall { .. })
        | MirOperation::Semantic(MirSemanticOp::TaskGroup { .. })
        | MirOperation::Semantic(MirSemanticOp::Select { .. })
        | MirOperation::Semantic(MirSemanticOp::PolicyFunction { .. })
        | MirOperation::Semantic(MirSemanticOp::InterruptFunction { .. })
        | MirOperation::Semantic(MirSemanticOp::HostCall { .. })
        | MirOperation::Semantic(MirSemanticOp::CellGuardProject { .. })
        | MirOperation::Semantic(MirSemanticOp::SharedGuardMap { .. })
        | MirOperation::Semantic(MirSemanticOp::HardwareCall { .. })
        | MirOperation::Semantic(MirSemanticOp::PluginInvoke { .. })
        | MirOperation::Semantic(MirSemanticOp::HttpRouterRegister { .. })
        | MirOperation::Semantic(MirSemanticOp::CarrierFact { .. })
        | MirOperation::Semantic(MirSemanticOp::GcEdit { .. })
        | MirOperation::Semantic(MirSemanticOp::TypedTextInterp { .. })
        | MirOperation::Semantic(MirSemanticOp::CCallback { .. })
        | MirOperation::LoopRangeInit { .. }
        | MirOperation::LoopRangeHasNext { .. }
        | MirOperation::LoopRangeValue { .. }
        | MirOperation::LoopRangeAdvance { .. }
        | MirOperation::LoopIterInit { .. }
        | MirOperation::LoopIterHasNext { .. }
        | MirOperation::LoopIterValue { .. }
        | MirOperation::LoopIterAdvance { .. }
        | MirOperation::ScopeEnter { .. }
        | MirOperation::ScopeExit { .. }
        | MirOperation::Drop { .. } => Ok(()),
    }
}

fn check_terminator(terminator: &jet_foundation::MIR::MirTerminator) -> Result<(), String> {
    match terminator {
        jet_foundation::MIR::MirTerminator::Jump { .. }
        | jet_foundation::MIR::MirTerminator::Branch { .. }
        | jet_foundation::MIR::MirTerminator::Switch { .. }
        | jet_foundation::MIR::MirTerminator::Return { .. }
        | jet_foundation::MIR::MirTerminator::Yield { .. }
        | jet_foundation::MIR::MirTerminator::Break { .. }
        | jet_foundation::MIR::MirTerminator::Continue { .. }
        | jet_foundation::MIR::MirTerminator::Unreachable { .. } => Ok(()),
    }
}

pub(crate) fn function_name(program: &MirProgram, id: MirFunctionId) -> String {
    program
        .functions
        .iter()
        .find(|function| function.id == id)
        .map(|function| function.key.clone())
        .unwrap_or_else(|| format!("<missing:{:?}>", id))
}
pub(crate) fn artifact_entry(program: &MirProgram, artifact: jet_foundation::MIR::MirArtifactId) -> Option<MirFunctionId> {
    program
        .artifacts
        .iter()
        .find(|plan| plan.id == artifact)?
        .entry
        .as_ref()?
        .function
}


pub(crate) fn type_def_is_record_or_enum(kind: &MirTypeDefKind) -> bool {
    match kind {
        MirTypeDefKind::Struct { .. } | MirTypeDefKind::Enum { .. } => true,
        MirTypeDefKind::Distinct { .. }
        | MirTypeDefKind::Alias { .. }
        | MirTypeDefKind::UnitFamily { .. } => false,
    }
}

pub(crate) fn callee_identity(callee: &MirCallee) -> String {
    match callee {
        MirCallee::User(id) => format!("user:{:?}", id),
        MirCallee::Associated { function, .. } => format!("associated:{:?}", function),
        MirCallee::Method { function, .. } => format!("method:{:?}", function),
        MirCallee::TraitMethod {
            method,
            trait_ref,
            ..
        } => format!("trait-method:{:?}:{:?}", trait_ref.id, method),
        MirCallee::Core(id) => format!("core:{:?}", id),
        MirCallee::Prelude(id) => format!("prelude:{:?}", id),
        MirCallee::Foreign(id) => format!("foreign:{:?}", id),
        MirCallee::Indirect(value) => format!("indirect:{:?}", value),
    }
}
