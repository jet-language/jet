use jet_foundation::MIR::{
    MirAbi, MirAccess, MirCallee, MirFunction, MirFunctionId, MirOperation, MirPreludeFamily,
    MirPreludeTypeArg, MirProgram, MirSemanticOp, MirType, MirTypeKind, MirTypeDefKind,
};

/// Check the canonical facts required by this adapter. Legality is decided
/// upstream; this function consumes those verdicts, verifies every operation
/// has an emitter arm, and rejects target-specific shapes the resident ABI
/// cannot represent.
pub(crate) fn resident_safe_mir_program(program: &MirProgram) -> Result<(), String> {
    program
        .validate()
        .map_err(|error| format!("invalid MIR: {error}"))?;
    for function in &program.functions {
        resident_safe_mir_function(function)?;
        resident_safe_cell_shapes(program, function)?;
    }
    Ok(())
}

/// Cell's resident ABI stores ordinary maps as opaque handles.  The native
/// path only has a checked representation for string-key maps; other key
/// shapes stay valid MIR and therefore remain available to the interpreter.
fn resident_safe_cell_shapes(
    program: &MirProgram,
    function: &MirFunction,
) -> Result<(), String> {
    for block in &function.blocks {
        for instruction in &block.instructions {
            let MirOperation::Semantic(MirSemanticOp::StaticPreludeCall {
                call,
                owner_type_args,
                ..
            }) = &instruction.operation
            else {
                continue;
            };
            let Some(route) = program
                .prelude_calls
                .iter()
                .find(|candidate| candidate.id == *call)
            else {
                continue;
            };
            if route.family != MirPreludeFamily::StaticPrelude
                || route.module != "::jet_std::JetCell"
                || route.member != "new"
            {
                continue;
            }
            let Some(MirPreludeTypeArg::Type(element_ty)) = owner_type_args.first() else {
                continue;
            };
            let Some(map_key) = cell_map_key(element_ty) else {
                continue;
            };
            if !is_string_key(map_key) {
                return Err(format!(
                    "MIR function `{}` has a Cell map with non-string key `{}`; \
                     resident Cranelift supports only string-key Cell maps",
                    function.key,
                    map_key.display_name()
                ));
            }
        }
    }
    Ok(())
}

fn cell_map_key(ty: &MirType) -> Option<&MirType> {
    match ty.kind() {
        MirTypeKind::Map { key, .. } => Some(key),
        MirTypeKind::Tagged { inner, .. } => cell_map_key(inner),
        _ => None,
    }
}

fn is_string_key(ty: &MirType) -> bool {
    match ty.kind() {
        MirTypeKind::String => true,
        MirTypeKind::Tagged { inner, .. } => is_string_key(inner),
        MirTypeKind::Apply { name, args } => args.is_empty() && name.name == "String",
        _ => false,
    }
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
        | MirOperation::ReplacePlace { .. }
        | MirOperation::InitializeUninit { .. }
        | MirOperation::Copy { .. }
        | MirOperation::TraitBox { .. }
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
        | MirOperation::Semantic(MirSemanticOp::ReflectOf { .. })
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
        | MirOperation::Semantic(MirSemanticOp::CursorTakePattern { .. })
        | MirOperation::Semantic(MirSemanticOp::ReaderTakePattern { .. })
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
