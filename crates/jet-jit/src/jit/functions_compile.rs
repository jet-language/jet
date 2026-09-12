use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    self, types, AbiParam, InstBuilder, MemFlags, Signature, StackSlotData, StackSlotKind, Value,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Linkage, Module};
use jet_foundation::MIR::{
    MirAbi, MirAccess, MirArtifactId, MirArtifactPlan, MirArtifactTarget, MirBasicBlock,
    MirBinaryOp, MirCallArg, MirCallee, MirCaptureOperand, MirConstKey, MirConstReport,
    MirConstant, MirCoreClosureKind, MirDropKind, MirFailureCarrier, MirFieldId, MirFieldRow,
    MirFunction, MirFunctionForm, MirFunctionId, MirGcEditKind, MirInstruction, MirOperation,
    MirInternalTag, MirOwnershipMode, MirPanicContext, MirPanicLoc, MirPlace, MirPlaceBase,
    MirPlaceId, MirPreludeTypeArg, MirProgram, MirRequireKind, MirScalarKind, MirSemanticOp,
    MirStringPart, MirTagMarker, MirTerminator, MirType, MirTypeDefKind, MirTypeId, MirTypeKind,
    MirUnaryOp, MirUnionCoercion, MirValueId,
};
use jet_rt::{
    RECORD_FIELD_ADDRESS_BOOL, RECORD_FIELD_ADDRESS_CHAR, RECORD_FIELD_ADDRESS_F64,
    RECORD_FIELD_ADDRESS_I64,
};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use super::runtime_host::{runtime_type_id, HostFns, JitRuntime};
use super::types_meta::{clif_ty_from_mir, mir_fn_name, prelude_enum_variant_index};
use crate::Cell::CellProjection;

/// Define a lowered function, keeping the backend's own explanation when it
/// rejects the IR.  `ModuleError`'s `Display` collapses a verifier failure to
/// the bare summary `Compilation error: Verifier errors`; the verifier's
/// per-instruction messages and the annotated function text are the whole
/// diagnosis of an invalid-IR bug, so they must reach the ICE.

fn cfg_block_order(function: &MirFunction) -> Vec<&MirBasicBlock> {
    let mut by_id = HashMap::with_capacity(function.blocks.len());
    for block in &function.blocks {
        by_id.insert(block.id, block);
    }
    let mut order = Vec::with_capacity(function.blocks.len());
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([function.entry]);
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        let Some(block) = by_id.get(&id) else {
            continue;
        };
        order.push(*block);
        for target in block.terminator.targets() {
            queue.push_back(target);
        }
    }
    for block in &function.blocks {
        if seen.insert(block.id) {
            order.push(block);
        }
    }
    order
}

fn define_function_checked<M: Module + ?Sized>(
    module: &mut M,
    id: FuncId,
    context: &mut cranelift_codegen::Context,
) -> Result<(), String> {
    module
        .define_function(id, context)
        .map_err(|error| match error {
            cranelift_module::ModuleError::Compilation(
                cranelift_codegen::CodegenError::Verifier(errors),
            ) => {
                let mut detail = format!("Cranelift verifier rejected `{}`:\n", context.func.name);
                for error in &errors.0 {
                    detail.push_str("  - ");
                    detail.push_str(&error.to_string());
                    detail.push('\n');
                }
                detail.push_str(&cranelift_codegen::print_errors::pretty_verifier_error(
                    &context.func,
                    None,
                    errors,
                ));
                detail
            }
            other => other.to_string(),
        })
}

/// Which resident map host family a key type selects. Mirrors the AOT
/// `JetMap` key specialisation: string keys hash by text, `Int`/`Bool`/`Char`
/// keys by their scalar bits. Compiler-owned `Ordering` uses the same scalar
/// carrier; every other key (tuples, structs, enums, unions) uses a canonical
/// composite record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MapKeyKind {
    String,
    Int,
    Composite,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComparisonElementKind {
    Integer,
    Float,
    String,
    Date,
}
fn is_ordering_name(name: &str) -> bool {
    name == jet_foundation::Syntax::TYPE_ORDERING
}
fn is_exact_int_type(ty: &MirType) -> bool {
    match ty.kind() {
        MirTypeKind::Int => true,
        MirTypeKind::InlineRange { base, .. }
        | MirTypeKind::Tagged { inner: base, .. }
        | MirTypeKind::Quantity { base, .. } => is_exact_int_type(base),
        _ => false,
    }
}

fn is_allocator_view_type(ty: &MirType) -> bool {
    matches!(
        ty.kind(),
        MirTypeKind::Tagged {
            marker: MirTagMarker::Internal(MirInternalTag::AllocatorView),
            ..
        }
    )
}

fn allocator_requested_bytes(ty: &MirType) -> i64 {
    match ty.layout.size {
        jet_foundation::MIR::MirSize::Static(size) => {
            i64::try_from(size).unwrap_or(i64::MAX).max(1)
        }
        jet_foundation::MIR::MirSize::Dynamic => 8,
    }
}

fn is_core_data_type_name(name: &str) -> bool {
    let name = match name.rsplit_once("::").or_else(|| name.rsplit_once('.')) {
        Some((prefix, leaf))
            if prefix == "core" || prefix.starts_with("core.") || prefix.starts_with("core::") =>
        {
            leaf
        }
        Some(_) => return false,
        None => name,
    };
    jet_foundation::Syntax::is_data_type_name(name)
}

fn is_ordering_type(ty: &MirType) -> bool {
    matches!(
        ty.kind(),
        MirTypeKind::Apply { name, args }
            if args.is_empty() && is_ordering_name(&name.name)
    )
}

fn is_comparison_scalar(ty: &MirType) -> bool {
    ty.is_scalar() || is_ordering_type(ty)
}
fn is_named_type(ty: &MirType, expected: &str) -> bool {
    match ty.kind() {
        MirTypeKind::Apply { name, args }
            if args.is_empty() && name.name == expected =>
        {
            true
        }
        MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Tagged { inner, .. }
        | MirTypeKind::Quantity { base: inner, .. } => is_named_type(inner, expected),
        _ => false,
    }
}

fn is_duration_type(ty: &MirType) -> bool {
    is_named_type(ty, "Duration")
}



fn comparison_element_kind(ty: &MirType) -> Option<ComparisonElementKind> {
    if is_ordering_type(ty) {
        return Some(ComparisonElementKind::Integer);
    }
    match ty.kind() {
        MirTypeKind::Int
        | MirTypeKind::IntN { .. }
        | MirTypeKind::Bool
        | MirTypeKind::Char
        | MirTypeKind::Measure(_) => Some(ComparisonElementKind::Integer),
        MirTypeKind::Float | MirTypeKind::Float32 => Some(ComparisonElementKind::Float),
        MirTypeKind::String => Some(ComparisonElementKind::String),
        MirTypeKind::InlineRange { base, .. }
        | MirTypeKind::Tagged { inner: base, .. }
        | MirTypeKind::Quantity { base, .. } => comparison_element_kind(base),
        MirTypeKind::Apply { name, args } if args.is_empty() => match name.name.as_str() {
            "Int" | "I8" | "I16" | "I32" | "I64" | "U8" | "U16" | "U32" | "U64" => {
                Some(ComparisonElementKind::Integer)
            }
            "Float" | "F32" | "F64" => Some(ComparisonElementKind::Float),
            "String" => Some(ComparisonElementKind::String),
            "Date" | "LocalDate" => Some(ComparisonElementKind::Date),
            _ => None,
        },
        _ => None,
    }
}

fn comparison_sequence_element_type(ty: &MirType) -> Option<&MirType> {
    match ty.kind() {
        MirTypeKind::List(inner) => Some(inner),
        MirTypeKind::FixedList { elem, .. } => Some(elem),
        MirTypeKind::Apply { name, args }
            if matches!(name.name.as_str(), "List" | "Iter" | "View" | "ViewIter")
                && args.len() == 1 =>
        {
            args.first()
        }
        MirTypeKind::Tagged { inner, .. } => comparison_sequence_element_type(inner),
        _ => None,
    }
}
fn copy_is_clock_type(ty: &MirType) -> bool {
    match ty.kind() {
        MirTypeKind::Apply { name, .. } => name.name == "Clock",
        MirTypeKind::Tagged { inner, .. }
        | MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Quantity { base: inner, .. } => copy_is_clock_type(inner),
        _ => false,
    }
}

/// Whether a copied MIR carrier can own a nested stateful value.  Shared
/// handles remain identity-preserving; all other aggregate/nominal carriers
/// use the checked runtime descriptor to recurse into their fields.
fn copy_needs_typed_clone(ty: &MirType) -> bool {
    match ty.kind() {
        MirTypeKind::Apply { .. } => true,
        MirTypeKind::Shared(_) => false,
        MirTypeKind::List(inner)
        | MirTypeKind::FixedList { elem: inner, .. }
        | MirTypeKind::Tagged { inner, .. }
        | MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Quantity { base: inner, .. } => copy_needs_typed_clone(inner),
        MirTypeKind::Map { key, value } => {
            copy_needs_typed_clone(key) || copy_needs_typed_clone(value)
        }
        MirTypeKind::Option(inner) => copy_needs_typed_clone(inner),
        MirTypeKind::Result { ok, err } => {
            copy_needs_typed_clone(ok) || copy_needs_typed_clone(err)
        }
        MirTypeKind::Tuple(fields) => fields
            .iter()
            .any(|(_, field)| copy_needs_typed_clone(field)),
        MirTypeKind::Union(variants) => variants.iter().any(copy_needs_typed_clone),
        _ => false,
    }
}

fn comparison_map_parts(ty: &MirType) -> Option<(&MirType, &MirType)> {
    match ty.kind() {
        MirTypeKind::Map { key, value } => Some((key, value)),
        MirTypeKind::Tagged { inner, .. } => comparison_map_parts(inner),
        _ => None,
    }
}

fn callable_signature(ty: &MirType) -> Option<(&[MirType], Option<&MirType>)> {
    ty.function_signature()
        .map(|signature| (signature.params.as_slice(), signature.ret.as_deref()))
        .or_else(|| ty.send_fn_signature())
}

fn atomic_scalar_kind_tag(ty: &MirType) -> Option<i64> {
    match ty.kind() {
        MirTypeKind::Bool => Some(0),
        MirTypeKind::Int => Some(5),
        MirTypeKind::IntN {
            signed: true,
            bits: 32,
        } => Some(1),
        MirTypeKind::IntN {
            signed: false,
            bits: 32,
        } => Some(2),
        MirTypeKind::IntN {
            signed: true,
            bits: 64,
        } => Some(3),
        MirTypeKind::IntN {
            signed: false,
            bits: 64,
        } => Some(4),
        _ => None,
    }
}
fn list_format_kind(inner: &MirType) -> Result<i64, String> {
    let kind = match inner.kind() {
        MirTypeKind::String => 1,
        MirTypeKind::Int => 2,
        MirTypeKind::IntN {
            signed: true,
            bits: 8,
        } => 8,
        MirTypeKind::IntN {
            signed: true,
            bits: 16,
        } => 9,
        MirTypeKind::IntN {
            signed: true,
            bits: 32,
        } => 10,
        MirTypeKind::IntN {
            signed: true,
            bits: 64,
        } => 11,
        MirTypeKind::IntN {
            signed: false,
            bits: 8,
        } => 12,
        MirTypeKind::IntN {
            signed: false,
            bits: 16,
        } => 13,
        MirTypeKind::IntN {
            signed: false,
            bits: 32,
        } => 14,
        MirTypeKind::IntN {
            signed: false,
            bits: 64,
        } => 3,
        MirTypeKind::Float => 4,
        MirTypeKind::Float32 => 7,
        MirTypeKind::Bool => 5,
        MirTypeKind::Char => 6,
        MirTypeKind::InlineRange { .. } | MirTypeKind::Measure(_) => 2,
        MirTypeKind::Apply { name, args } if args.is_empty() && name.name == "Fraction" => 15,
        MirTypeKind::List(inner) => 16 + list_format_kind(inner)?,
        _ => {
            return Err(format!(
                "MIR list element type `{}` has no checked formatting carrier",
                inner.display_name()
            ))
        }
    };
    Ok(kind)
}

fn is_atomic_builtin_member(member: &str) -> bool {
    matches!(
        member,
        "load" | "store" | "add" | "try_add" | "compare_exchange" | "publish" | "observe"
    )
}
type CsvDecodeThunkKey = (MirFunctionId, MirFunctionId);

#[derive(Clone, Copy)]
enum CsvDecodeTarget {
    Function(MirFunctionId),
    Scalar(i64),
}

#[derive(Clone)]
struct CsvDecodeSpec {
    row_type: MirType,
    target: CsvDecodeTarget,
    element_kind: i64,
}

fn csv_decode_function<'a>(
    program: &'a MirProgram,
    row_type: &MirType,
) -> Result<Option<&'a MirFunction>, String> {
    let matches = program
        .functions
        .iter()
        .filter(|function| function.is_decode_for(row_type))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Ok(None),
        [function] => {
            if !function.target_applicability.cranelift {
                return Err(format!(
                    "CSV Decode function `{}` is unavailable to Cranelift",
                    function.name
                ));
            }
            if !function.capture_params.is_empty() || function.generator.is_some() {
                return Err(format!(
                    "CSV Decode function `{}` has an unsupported callable shape",
                    function.name
                ));
            }
            if function.params.len() != 1
                || clif_ty_from_mir(&function.params[0].ty) != Some(types::I64)
                || clif_ty_from_mir(&function.return_type) != Some(types::I64)
            {
                return Err(format!(
                    "CSV Decode function `{}` does not use the canonical DataTree/result ABI",
                    function.name
                ));
            }
            Ok(Some(*function))
        }
        _ => Err(format!(
            "CSV row type `{}` resolves to multiple Decode functions",
            row_type.display_name()
        )),
    }
}

fn csv_scalar_decode_kind(ty: &MirType) -> Option<i64> {
    match &ty.kind {
        MirTypeKind::Tagged { inner, .. } => csv_scalar_decode_kind(inner),
        MirTypeKind::Int => Some(crate::Encoding::CSV_DECODE_INT),
        MirTypeKind::Float | MirTypeKind::Float32 => Some(crate::Encoding::CSV_DECODE_FLOAT),
        MirTypeKind::Bool => Some(crate::Encoding::CSV_DECODE_BOOL),
        MirTypeKind::String => Some(crate::Encoding::CSV_DECODE_STRING),
        MirTypeKind::Char => Some(crate::Encoding::CSV_DECODE_CHAR),
        MirTypeKind::Apply { name, args } if args.is_empty() => match name.name.as_str() {
            "DataTree" => Some(crate::Encoding::CSV_DECODE_DATATREE),
            "Date" => Some(crate::Encoding::CSV_DECODE_DATE),
            "LocalDate" => Some(crate::Encoding::CSV_DECODE_LOCAL_DATE),
            "LocalTime" => Some(crate::Encoding::CSV_DECODE_LOCAL_TIME),
            "DateTime" => Some(crate::Encoding::CSV_DECODE_DATETIME),
            "Duration" => Some(crate::Encoding::CSV_DECODE_DURATION),
            "Decimal" => Some(crate::Encoding::CSV_DECODE_DECIMAL),
            _ => None,
        },
        _ => None,
    }
}

fn csv_decode_spec(
    program: &MirProgram,
    call: jet_foundation::MIR::MirCoreCallId,
    type_args: &[MirType],
) -> Result<Option<CsvDecodeSpec>, String> {
    let row = program
        .core_calls
        .iter()
        .find(|candidate| candidate.id == call)
        .ok_or_else(|| format!("MIR Core call {:?} is missing", call))?;
    if !matches!(
        (row.module.as_str(), row.member.as_str()),
        ("core.data", "csv") | ("core.encoding.csv", "decode" | "query")
    ) {
        return Ok(None);
    }
    if type_args.len() != 1 {
        return Err(format!(
            "typed CSV CoreCall needs one row type argument, got {}",
            type_args.len()
        ));
    }
    let row_type = type_args[0].clone();
    let target = if let Some(function) = csv_decode_function(program, &row_type)? {
        CsvDecodeTarget::Function(function.id)
    } else {
        CsvDecodeTarget::Scalar(csv_scalar_decode_kind(&row_type).ok_or_else(|| {
            format!(
                "CSV row type `{}` has no resident Decode adapter",
                row_type.display_name()
            )
        })?)
    };
    let element_kind = match row_type.layout.abi {
        MirAbi::Scalar(MirScalarKind::Float | MirScalarKind::Float32) => {
            crate::Encoding::CSV_DECODE_FLOAT
        }
        _ => crate::Encoding::CSV_DECODE_INT,
    };
    Ok(Some(CsvDecodeSpec {
        row_type,
        target,
        element_kind,
    }))
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledIterableHook {
    pub(crate) source_wire: String,
    pub(crate) coll_type: String,
    pub(crate) iter_type: String,
    pub(crate) iter: FuncId,
    pub(crate) next: FuncId,
}

#[derive(Debug)]
pub(crate) struct CompiledMirProgram {
    pub(crate) entry_id: FuncId,
    pub(crate) function_ids: HashMap<MirFunctionId, FuncId>,
    pub(crate) iterable_hooks: Vec<CompiledIterableHook>,
}

type ViewThunkKey = (MirFunctionId, MirValueId, usize);
type AppThunkKey = (MirFunctionId, MirValueId);
type IterableThunkKey = String;

#[derive(Clone, Copy)]
enum AppThunkMode {
    Page,
    Encoded,
}

#[derive(Clone, Copy)]
struct AppThunk {
    id: FuncId,
    mode: AppThunkMode,
    output_type: i64,
    arity: usize,
}

#[derive(Clone, Copy)]
struct IterableThunkIds {
    iter: FuncId,
    next: FuncId,
}
/// Storage identity of a MIR place.  Canonical MIR keeps one place row per
/// access mode over the same base (a `let`'s initializing write lands on a
/// Write row, a later use on a Read row), so slots must be shared by base:
/// keyed by place id, the write and the read would get two slots and the
/// read would return uninitialized stack.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum SlotKey {
    Local(jet_foundation::MIR::MirLocalId),
    Value(MirValueId),
}

struct FunctionLower<'a, 'm> {
    module: &'m mut dyn Module,
    host: &'a HostFns,
    program: &'a MirProgram,
    function: &'a MirFunction,
    runtime: &'a mut JitRuntime,
    blocks: HashMap<jet_foundation::MIR::MirBlockId, ir::Block>,
    block_phis: HashMap<jet_foundation::MIR::MirBlockId, Vec<(MirValueId, types::Type)>>,
    values: HashMap<MirValueId, Value>,
    places: HashMap<SlotKey, ir::StackSlot>,
    write_parameters: Vec<(MirValueId, Value, types::Type)>,
    capture_env: Option<Value>,
    function_ids: &'a HashMap<MirFunctionId, FuncId>,
    view_thunks: &'a HashMap<ViewThunkKey, FuncId>,
    csv_thunks: &'a HashMap<CsvDecodeThunkKey, FuncId>,
    app_thunks: &'a HashMap<AppThunkKey, AppThunk>,
    yield_sender: Option<Value>,
    stop_block: Option<ir::Block>,
}

#[derive(Clone, Copy)]
enum PatternCaptureKind {
    String,
    Char,
    Int,
    Float,
    Bool,
    Bytes,
}

fn cranelift_artifact<'a>(
    program: &'a MirProgram,
    artifact_id: MirArtifactId,
) -> Result<&'a MirArtifactPlan, String> {
    let artifact = program
        .artifacts
        .iter()
        .find(|artifact| artifact.id == artifact_id)
        .ok_or_else(|| format!("MIR artifact {:?} is missing", artifact_id))?;
    match artifact.target {
        MirArtifactTarget::Cranelift => {}
        MirArtifactTarget::RustAot | MirArtifactTarget::Interpreter | MirArtifactTarget::Web => {
            return Err(format!(
                "MIR artifact {:?} targets {:?}, not Cranelift",
                artifact.id, artifact.target
            ));
        }
    }
    for module_id in &artifact.modules {
        if !program.modules.iter().any(|module| module.id == *module_id) {
            return Err(format!(
                "MIR artifact {:?} references missing module {:?}",
                artifact.id, module_id
            ));
        }
    }
    for link_id in &artifact.links {
        let link = program
            .links
            .iter()
            .find(|link| link.id == *link_id)
            .ok_or_else(|| {
                format!(
                    "MIR artifact {:?} references missing link {:?}",
                    artifact.id, link_id
                )
            })?;
        if !link.target_applicability.cranelift {
            return Err(format!(
                "MIR artifact {:?} references link {:?} unavailable to Cranelift",
                artifact.id, link_id
            ));
        }
    }
    for job_id in &artifact.jobs {
        if !program.jobs.iter().any(|job| job.id == *job_id) {
            return Err(format!(
                "MIR artifact {:?} references missing job {:?}",
                artifact.id, job_id
            ));
        }
    }
    if let Some(harness_id) = artifact.harness {
        if !program
            .harnesses
            .iter()
            .any(|harness| harness.id == harness_id)
        {
            return Err(format!(
                "MIR artifact {:?} references missing harness {:?}",
                artifact.id, harness_id
            ));
        }
    }
    Ok(artifact)
}

fn direct_function_call(operation: &MirOperation) -> Option<MirFunctionId> {
    match operation {
        MirOperation::Call {
            callee:
                MirCallee::User(function)
                | MirCallee::Associated { function, .. }
                | MirCallee::Method { function, .. },
            ..
        }
        | MirOperation::Closure { function, .. } => Some(*function),
        MirOperation::Call {
            callee:
                MirCallee::Core(_)
                | MirCallee::Prelude(_)
                | MirCallee::Foreign(_)
                | MirCallee::Indirect(_)
                | MirCallee::TraitMethod { .. },
            ..
        }
        | MirOperation::Parameter { .. }
        | MirOperation::Capture { .. }
        | MirOperation::Global { .. }
        | MirOperation::Phi { .. }
        | MirOperation::ReadPlace(_)
        | MirOperation::MovePlace { .. }
        | MirOperation::InitializeUninit { .. }
        | MirOperation::WritePlace { .. }
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
        | MirOperation::IndirectCall { .. }
        | MirOperation::PtrFromAddr { .. }
        | MirOperation::Deref { .. }
        | MirOperation::RawAddressOf { .. }
        | MirOperation::AddressOf { .. }
        | MirOperation::CoreCall { .. }
        | MirOperation::AttachTag { .. }
        | MirOperation::Todo { .. }
        | MirOperation::Never { .. }
        | MirOperation::Semantic(_)
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
        | MirOperation::Drop { .. } => None,
    }
}

fn artifact_function_ids(
    program: &MirProgram,
    artifact: &MirArtifactPlan,
) -> Result<BTreeSet<MirFunctionId>, String> {
    let mut ids = BTreeSet::new();
    for module_id in &artifact.modules {
        for function in program
            .functions
            .iter()
            .filter(|function| function.module_id == *module_id)
        {
            ids.insert(function.id);
        }
    }
    let entry = artifact
        .entry
        .as_ref()
        .and_then(|entry| entry.function)
        .ok_or_else(|| format!("MIR artifact {:?} has no entry function", artifact.id))?;
    ids.insert(entry);
    for export in &artifact.exports {
        ids.insert(export.function);
    }
    for job_id in &artifact.jobs {
        let job = program
            .jobs
            .iter()
            .find(|job| job.id == *job_id)
            .ok_or_else(|| format!("MIR job {:?} is missing", job_id))?;
        ids.insert(job.function);
    }
    if let Some(harness_id) = artifact.harness {
        let harness = program
            .harnesses
            .iter()
            .find(|harness| harness.id == harness_id)
            .ok_or_else(|| format!("MIR harness {:?} is missing", harness_id))?;
        let selected_tests = harness
            .selected_test
            .into_iter()
            .chain(harness.tests.iter().copied());
        for test_id in selected_tests {
            let test = program
                .tests
                .iter()
                .find(|test| test.id == test_id)
                .ok_or_else(|| format!("MIR test {:?} is missing", test_id))?;
            ids.insert(test.function);
            if let Some(eligibility) = test.eligibility {
                ids.insert(eligibility);
            }
        }
        for check in &harness.output_checks {
            ids.insert(check.function);
        }
        for point in &harness.coverage_points {
            ids.insert(point.function);
        }
    }
    hardware_handler_function_ids(program, &mut ids)?;
    let mut changed = true;
    while changed {
        changed = false;
        let current = ids.iter().copied().collect::<Vec<_>>();
        for function_id in current {
            let function = program
                .functions
                .iter()
                .find(|function| function.id == function_id)
                .ok_or_else(|| format!("MIR function {:?} is missing", function_id))?;
            for block in &function.blocks {
                for instruction in &block.instructions {
                    if let Some(callee) = direct_function_call(&instruction.operation) {
                        changed |= ids.insert(callee);
                    }
                    if let MirOperation::CoreCall {
                        call, type_args, ..
                    } = &instruction.operation
                    {
                        if let Some(spec) = csv_decode_spec(program, *call, type_args)? {
                            if let CsvDecodeTarget::Function(callee) = spec.target {
                                changed |= ids.insert(callee);
                            }
                        }
                    }
                }
            }
        }
    }
    for function_id in &ids {
        if !program
            .functions
            .iter()
            .any(|function| function.id == *function_id)
        {
            return Err(format!(
                "MIR artifact references missing function {:?}",
                function_id
            ));
        }
    }
    Ok(ids)
}
fn iterable_hook_function_ids(
    program: &MirProgram,
    selected_ids: &mut BTreeSet<MirFunctionId>,
) -> Result<(), String> {
    let mut changed = true;
    while changed {
        changed = false;
        let current = selected_ids.iter().copied().collect::<Vec<_>>();
        for selected in current {
            let function = program
                .functions
                .iter()
                .find(|function| function.id == selected)
                .ok_or_else(|| format!("MIR function {:?} is missing", selected))?;
            for block in &function.blocks {
                for instruction in &block.instructions {
                    let MirOperation::LoopIterInit {
                        source_kind:
                            jet_foundation::MIR::MirLoopSourceKind::Iterable {
                                iter_symbol,
                                next_symbol,
                                ..
                            },
                        ..
                    } = &instruction.operation
                    else {
                        continue;
                    };
                    for (role, symbol) in [("iter", iter_symbol), ("next", next_symbol)] {
                        let mut matches = program
                            .functions
                            .iter()
                            .filter(|candidate| candidate.key == *symbol);
                        let target = matches.next().ok_or_else(|| {
                            format!("MIR iterable {role} symbol `{symbol}` has no function row")
                        })?;
                        if matches.next().is_some() {
                            return Err(format!(
                                "MIR iterable {role} symbol `{symbol}` resolves to multiple function rows"
                            ));
                        }
                        if !target.target_applicability.cranelift {
                            return Err(format!(
                                "MIR iterable {role} function `{symbol}` is unavailable to Cranelift"
                            ));
                        }
                        changed |= selected_ids.insert(target.id);
                    }
                }
            }
        }
    }
    Ok(())
}
fn hardware_handler_function<'a>(
    program: &'a MirProgram,
    handler_symbol: &str,
) -> Result<&'a MirFunction, String> {
    let mut matches = program
        .functions
        .iter()
        .filter(|function| function.key == handler_symbol);
    let function = matches
        .next()
        .ok_or_else(|| format!("MIR interrupt handler `{handler_symbol}` has no function row"))?;
    if matches.next().is_some() {
        return Err(format!(
            "MIR interrupt handler `{handler_symbol}` resolves to multiple function rows"
        ));
    }
    Ok(function)
}

fn hardware_handler_function_ids(
    program: &MirProgram,
    selected_ids: &mut BTreeSet<MirFunctionId>,
) -> Result<(), String> {
    for setup in &program.facts.hardware_setups {
        let jet_foundation::MIR::MirHardwareSetup::InterruptBind { handler_symbol, .. } = setup
        else {
            continue;
        };
        let function = hardware_handler_function(program, handler_symbol)?;
        if !function.target_applicability.cranelift {
            return Err(format!(
                "MIR interrupt handler `{handler_symbol}` is unavailable to Cranelift"
            ));
        }
        selected_ids.insert(function.id);
    }
    Ok(())
}

/// Compile the checked MIR functions and linkage selected by one Cranelift
/// artifact row. The artifact id is mandatory so no target or entry is inferred.
pub(crate) fn compile_program(
    module: &mut JITModule,
    host: &HostFns,
    program: &MirProgram,
    artifact: MirArtifactId,
    runtime: &mut JitRuntime,
) -> Result<CompiledMirProgram, String> {
    let compiled = compile_program_inner(module, host, program, artifact, runtime)?;
    module
        .finalize_definitions()
        .map_err(|error| error.to_string())?;
    Ok(compiled)
}

pub(crate) fn compile_program_object(
    module: &mut dyn Module,
    host: &HostFns,
    program: &MirProgram,
    artifact: MirArtifactId,
    runtime: &mut JitRuntime,
) -> Result<FuncId, String> {
    let compiled = compile_program_inner(module, host, program, artifact, runtime)?;
    Ok(compiled.entry_id)
}

/// Compile one explicitly requested MIR function without selecting an artifact.
/// This low-level compiler has no execution artifact to attach to deopt frames;
/// whole-artifact execution uses `compile_program_inner` below.
pub(crate) fn compile_mir_function(
    module: &mut dyn Module,
    host: &HostFns,
    program: &MirProgram,
    function_id: MirFunctionId,
    runtime: &mut JitRuntime,
) -> Result<FuncId, String> {
    program
        .validate()
        .map_err(|error| format!("invalid MIR: {error}"))?;
    jet_foundation::MIROptimization::require_canonical_mir_optimization(program)
        .map_err(|error| format!("MIR is not canonically optimized: {error}"))?;
    let root = program
        .functions
        .iter()
        .find(|function| function.id == function_id)
        .ok_or_else(|| format!("MIR function {:?} is missing", function_id))?;
    if !root.target_applicability.cranelift {
        return Err(format!(
            "MIR function {:?} is not applicable to Cranelift",
            function_id
        ));
    }
    runtime.clear_iterable_hooks();
    runtime.model_outputs = program.facts.model_outputs.clone();
    runtime.model_sessions.clear();
    let mut selected_ids = BTreeSet::from([function_id]);
    hardware_handler_function_ids(program, &mut selected_ids)?;
    let mut changed = true;
    while changed {
        changed = false;
        let current = selected_ids.iter().copied().collect::<Vec<_>>();
        for selected in current {
            let function = program
                .functions
                .iter()
                .find(|function| function.id == selected)
                .ok_or_else(|| format!("MIR function {:?} is missing", selected))?;
            for block in &function.blocks {
                for instruction in &block.instructions {
                    if let Some(callee) = direct_function_call(&instruction.operation) {
                        let callee_function = program
                            .functions
                            .iter()
                            .find(|function| function.id == callee)
                            .ok_or_else(|| format!("MIR function {:?} is missing", callee))?;
                        if !callee_function.target_applicability.cranelift {
                            return Err(format!(
                                "MIR function {:?} calls function {:?} unavailable to Cranelift",
                                selected, callee
                            ));
                        }
                        changed |= selected_ids.insert(callee);
                    }
                    if let MirOperation::CoreCall {
                        call, type_args, ..
                    } = &instruction.operation
                    {
                        if let Some(spec) = csv_decode_spec(program, *call, type_args)? {
                            if let CsvDecodeTarget::Function(callee) = spec.target {
                                let callee_function = program
                                    .functions
                                    .iter()
                                    .find(|function| function.id == callee)
                                    .ok_or_else(|| {
                                        format!("MIR function {:?} is missing", callee)
                                    })?;
                                if !callee_function.target_applicability.cranelift {
                                    return Err(format!(
                                        "MIR function {:?} calls function {:?} unavailable to Cranelift",
                                        selected, callee
                                    ));
                                }
                                changed |= selected_ids.insert(callee);
                            }
                        }
                    }
                }
            }
        }
    }
    iterable_hook_function_ids(program, &mut selected_ids)?;
    let selected_functions = program
        .functions
        .iter()
        .filter(|function| selected_ids.contains(&function.id))
        .collect::<Vec<_>>();
    super::runtime_host::install_program_type_descriptors(runtime, program, &selected_functions);
    let mut function_ids = HashMap::new();
    let mut generator_body_ids = HashMap::new();
    for function in &selected_functions {
        let signature = function_signature(module, function)?;
        let id = module
            .declare_function(&mir_fn_name(function.id), Linkage::Local, &signature)
            .map_err(|error| error.to_string())?;
        function_ids.insert(function.id, id);
        if function.generator.is_some() {
            let body_signature = generator_body_signature(module, function)?;
            let body_id = module
                .declare_function(
                    &format!("{}__generator", mir_fn_name(function.id)),
                    Linkage::Local,
                    &body_signature,
                )
                .map_err(|error| error.to_string())?;
            generator_body_ids.insert(function.id, body_id);
        }
    }
    let view_thunks = install_view_callback_thunks(module, host, program, &selected_functions)?;
    let app_thunks =
        install_app_callback_thunks(module, host, program, &selected_functions, runtime)?;
    let csv_thunks =
        install_csv_decode_thunks(module, program, &selected_functions, &function_ids)?;
    let _ = install_iterable_hook_thunks(module, program, &selected_functions, &function_ids)?;
    for function in &selected_functions {
        let wrapper_id = *function_ids
            .get(&function.id)
            .ok_or_else(|| format!("MIR function {:?} was not declared", function.id))?;
        if let Some(body_id) = generator_body_ids.get(&function.id).copied() {
            lower_function(
                module,
                host,
                program,
                function,
                runtime,
                &function_ids,
                &view_thunks,
                &app_thunks,
                &csv_thunks,
                body_id,
                true,
            )?;
            lower_generator_wrapper(module, host, function, wrapper_id, body_id)?;
        } else {
            lower_function(
                module,
                host,
                program,
                function,
                runtime,
                &function_ids,
                &view_thunks,
                &app_thunks,
                &csv_thunks,
                wrapper_id,
                false,
            )?;
        }
    }
    function_ids
        .get(&function_id)
        .copied()
        .ok_or_else(|| format!("MIR function {:?} was not declared", function_id))
}

fn compile_program_inner(
    module: &mut dyn Module,
    host: &HostFns,
    program: &MirProgram,
    artifact_id: MirArtifactId,
    runtime: &mut JitRuntime,
) -> Result<CompiledMirProgram, String> {
    program
        .validate()
        .map_err(|error| format!("invalid MIR: {error}"))?;
    jet_foundation::MIROptimization::require_canonical_mir_optimization(program)
        .map_err(|error| format!("MIR is not canonically optimized: {error}"))?;
    runtime.model_outputs = program.facts.model_outputs.clone();
    runtime.model_sessions.clear();
    let artifact = cranelift_artifact(program, artifact_id)?;
    let mut selected_ids = artifact_function_ids(program, artifact)?;
    iterable_hook_function_ids(program, &mut selected_ids)?;
    let selected_functions = program
        .functions
        .iter()
        .filter(|function| selected_ids.contains(&function.id))
        .collect::<Vec<_>>();
    super::runtime_host::install_program_type_descriptors(runtime, program, &selected_functions);
    let mut function_ids = HashMap::new();
    let mut generator_body_ids = HashMap::new();
    for function in &selected_functions {
        let signature = function_signature(module, function)?;
        let id = module
            .declare_function(&mir_fn_name(function.id), Linkage::Local, &signature)
            .map_err(|error| error.to_string())?;
        function_ids.insert(function.id, id);
        if function.generator.is_some() {
            let body_signature = generator_body_signature(module, function)?;
            let body_id = module
                .declare_function(
                    &format!("{}__generator", mir_fn_name(function.id)),
                    Linkage::Local,
                    &body_signature,
                )
                .map_err(|error| error.to_string())?;
            generator_body_ids.insert(function.id, body_id);
        }
    }
    let view_thunks = install_view_callback_thunks(module, host, program, &selected_functions)?;
    let app_thunks =
        install_app_callback_thunks(module, host, program, &selected_functions, runtime)?;
    let csv_thunks =
        install_csv_decode_thunks(module, program, &selected_functions, &function_ids)?;
    let (_, iterable_hooks) =
        install_iterable_hook_thunks(module, program, &selected_functions, &function_ids)?;
    super::deopt::install_frame_schemas(program, Some(artifact_id))?;
    for function in &selected_functions {
        let wrapper_id = *function_ids
            .get(&function.id)
            .ok_or_else(|| format!("MIR function {:?} was not declared", function.id))?;
        if let Some(body_id) = generator_body_ids.get(&function.id).copied() {
            lower_function(
                module,
                host,
                program,
                function,
                runtime,
                &function_ids,
                &view_thunks,
                &app_thunks,
                &csv_thunks,
                body_id,
                true,
            )?;
            lower_generator_wrapper(module, host, function, wrapper_id, body_id)?;
        } else {
            lower_function(
                module,
                host,
                program,
                function,
                runtime,
                &function_ids,
                &view_thunks,
                &app_thunks,
                &csv_thunks,
                wrapper_id,
                false,
            )?;
        }
    }
    let entry = artifact
        .entry
        .as_ref()
        .and_then(|entry| entry.function)
        .ok_or_else(|| format!("MIR artifact {:?} has no entry function", artifact.id))?;
    let entry_id = *function_ids
        .get(&entry)
        .ok_or_else(|| format!("MIR entry {:?} was not declared", entry))?;
    Ok(CompiledMirProgram {
        entry_id,
        function_ids,
        iterable_hooks,
    })
}

fn view_callback_shape(row: &jet_foundation::MIR::MirPreludeCall) -> Option<(bool, usize, usize)> {
    if row.family != jet_foundation::MIR::MirPreludeFamily::ClosureMethod
        || row.module != "core.view"
    {
        return None;
    }
    match row.member.as_str() {
        "map" | "try_map" => Some((false, 0, 1)),
        "try_filter" => Some((false, 0, 1)),
        "fold" => Some((true, 1, 2)),
        _ => None,
    }
}

fn sequence_element_type(ty: &MirType) -> Option<&MirType> {
    match &ty.kind {
        MirTypeKind::List(inner) => Some(inner),
        MirTypeKind::FixedList { elem, .. } => Some(elem),
        MirTypeKind::Apply { name, args }
            if matches!(name.name.as_str(), "List" | "Iter" | "View" | "ViewIter")
                && args.len() == 1 =>
        {
            args.first()
        }
        MirTypeKind::Shared(inner)
        | MirTypeKind::Tagged { inner, .. }
        | MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Quantity { base: inner, .. } => sequence_element_type(inner),
        _ => None,
    }
}
fn indexed_element_type(ty: &MirType, kind: jet_foundation::MIR::MirIndexKind) -> Option<&MirType> {
    match kind {
        jet_foundation::MIR::MirIndexKind::List
        | jet_foundation::MIR::MirIndexKind::FixedListProof => sequence_element_type(ty),
        jet_foundation::MIR::MirIndexKind::Map => match &ty.kind {
            MirTypeKind::Map { value, .. } => Some(value),
            MirTypeKind::Shared(inner)
            | MirTypeKind::Tagged { inner, .. }
            | MirTypeKind::InlineRange { base: inner, .. }
            | MirTypeKind::Quantity { base: inner, .. } => indexed_element_type(inner, kind),
            _ => None,
        },
        jet_foundation::MIR::MirIndexKind::Pool => match &ty.kind {
            MirTypeKind::Apply { args, .. } if args.len() == 1 => args.first(),
            MirTypeKind::Shared(inner)
            | MirTypeKind::Tagged { inner, .. }
            | MirTypeKind::InlineRange { base: inner, .. }
            | MirTypeKind::Quantity { base: inner, .. } => indexed_element_type(inner, kind),
            _ => None,
        },
        jet_foundation::MIR::MirIndexKind::Lane => None,
    }
}
fn checked_index_element_type(
    ty: &MirType,
    kind: jet_foundation::MIR::MirIndexKind,
) -> Result<&MirType, String> {
    indexed_element_type(ty, kind).ok_or_else(|| match kind {
        jet_foundation::MIR::MirIndexKind::List
        | jet_foundation::MIR::MirIndexKind::FixedListProof => {
            "MIR checked list index has a non-list base".to_string()
        }
        jet_foundation::MIR::MirIndexKind::Map => {
            "MIR checked map index has a non-map base".to_string()
        }
        jet_foundation::MIR::MirIndexKind::Pool => {
            "MIR checked pool index has no element type".to_string()
        }
        jet_foundation::MIR::MirIndexKind::Lane => {
            "MIR checked lane index has no assignable place route".to_string()
        }
    })
}

fn callback_return_type(ty: &MirType) -> Option<&MirType> {
    callable_signature(ty).and_then(|(_, ret)| ret)
}

fn result_ok_type(ty: &MirType) -> Option<&MirType> {
    match &ty.kind {
        MirTypeKind::Result { ok, .. } => Some(ok),
        MirTypeKind::Tagged { inner, .. } => result_ok_type(inner),
        _ => None,
    }
}

fn plot_callback_shape(row: &jet_foundation::MIR::MirPreludeCall) -> Option<(bool, usize, usize)> {
    if row.family != jet_foundation::MIR::MirPreludeFamily::ClosureMethod
        || row.module != "core.data.plot"
        || row.member != "column"
    {
        return None;
    }
    Some((false, 0, 1))
}

/// One callback site: `(parameter count, argument index)`.
type CallbackShape = (usize, usize);

/// Every Core call argument whose checked MIR type is a function type is a
/// universal callback. Enrolment is decided by the type row, never by a
/// `(module, member)` table: a host that takes a Jet callback receives a slot
/// with the raw universal thunk attached, whichever Core row it implements.
/// Callback payloads wider than two words use the checked many-word thunk ABI.
fn core_callback_shapes(
    function: &MirFunction,
    args: &[MirCallArg],
) -> Result<Vec<CallbackShape>, String> {
    let mut shapes = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        if arg.access == MirAccess::Write {
            continue;
        }
        let Some((_, ty, _, _)) = function
            .values
            .iter()
            .find(|(value, _, _, _)| *value == arg.value)
        else {
            continue;
        };
        let Some((params, _)) = callable_signature(ty) else {
            continue;
        };
        // Wider callbacks are represented by the many-word thunk ABI below;
        // rejecting them here would make the host callback contract depend on
        // an arbitrary Cranelift register-count limit.
        if let Some(conventions) = ty
            .function_signature()
            .and_then(|signature| signature.call_metadata.as_ref())
            .map(|metadata| metadata.conventions.as_slice())
        {
            if conventions.len() != params.len() {
                return Err(format!(
                    "MIR Core callback {:?} argument conventions do not match its signature",
                    arg.value
                ));
            }
        }
        shapes.push((params.len(), index));
    }
    Ok(shapes)
}

fn add_callback_thunk_spec(
    specs: &mut Vec<(ViewThunkKey, MirType, usize)>,
    owner: MirFunctionId,
    callback: MirValueId,
    callback_ty: MirType,
    expected_params: usize,
) -> Result<(), String> {
    let (params, ret) = callable_signature(&callback_ty)
        .ok_or_else(|| format!("MIR callback {:?} has no function signature", callback))?;
    if params.len() != expected_params {
        return Err(format!(
            "MIR callback {:?} expects {} parameters, got {}",
            callback,
            expected_params,
            params.len()
        ));
    }
    if params
        .iter()
        .any(|parameter| clif_ty_from_mir(parameter).is_none())
    {
        return Err(format!(
            "MIR callback {:?} has a parameter without a Cranelift ABI",
            callback
        ));
    }
    if let Some(result) = ret {
        if clif_ty_from_mir(result).is_none() {
            return Err(format!(
                "MIR callback {:?} has a return without a Cranelift ABI",
                callback
            ));
        }
    }
    let key = (owner, callback, expected_params);
    if !specs.iter().any(|(existing, _, _)| *existing == key) {
        specs.push((key, callback_ty, expected_params));
    }
    Ok(())
}

fn view_callback_thunk_specs(
    program: &MirProgram,
    selected_functions: &[&MirFunction],
) -> Result<Vec<(ViewThunkKey, MirType, usize)>, String> {
    let mut specs = Vec::new();
    for function in selected_functions {
        for block in &function.blocks {
            for instruction in &block.instructions {
                // Keep a universal thunk on every closure value. History
                // strategies store callback closures inside a record, so
                // there is no Core call site at which to attach one later.
                if let MirOperation::Closure { .. } = &instruction.operation {
                    let Some(callback) = instruction.result else {
                        continue;
                    };
                    let Some(callback_ty) = instruction.ty.clone() else {
                        continue;
                    };
                    let Some((params, _)) = callable_signature(&callback_ty) else {
                        continue;
                    };
                    add_callback_thunk_spec(
                        &mut specs,
                        function.id,
                        callback,
                        callback_ty.clone(),
                        params.len(),
                    )?;
                    continue;
                }
                let (args, shapes): (&[MirCallArg], Vec<CallbackShape>) = match &instruction
                    .operation
                {
                    MirOperation::Semantic(MirSemanticOp::ClosureMethod { call, args, .. }) => {
                        let row = program
                            .prelude_calls
                            .iter()
                            .find(|row| row.id == *call)
                            .ok_or_else(|| format!("MIR collection route {call:?} is missing"))?;
                        if let Some((pair, index, count)) =
                            view_callback_shape(row).or_else(|| plot_callback_shape(row))
                        {
                            if args.len() != count {
                                return Err(format!(
                                    "MIR {}.{} route expects {} explicit arguments, got {}",
                                    row.module,
                                    row.member,
                                    count,
                                    args.len()
                                ));
                            }
                            (args.as_slice(), vec![(if pair { 2 } else { 1 }, index)])
                        } else {
                            (args.as_slice(), core_callback_shapes(function, args)?)
                        }
                    }
                    MirOperation::CoreCall { args, .. }
                    | MirOperation::Call {
                        callee: MirCallee::Core(_),
                        args,
                        ..
                    } => (args.as_slice(), core_callback_shapes(function, args)?),
                    _ => continue,
                };
                for (expected_params, callback_index) in shapes {
                    let callback = args
                        .get(callback_index)
                        .ok_or_else(|| {
                            format!("MIR callback route has no argument at index {callback_index}")
                        })?
                        .value;
                    let callback_ty = function
                        .values
                        .iter()
                        .find(|(value, _, _, _)| *value == callback)
                        .map(|(_, ty, _, _)| ty.clone())
                        .ok_or_else(|| format!("MIR callback {:?} has no type row", callback))?;
                    add_callback_thunk_spec(
                        &mut specs,
                        function.id,
                        callback,
                        callback_ty,
                        expected_params,
                    )?;
                }
            }
        }
    }
    specs.sort_by_key(|(key, _, _)| *key);
    Ok(specs)
}

fn install_view_callback_thunks(
    module: &mut dyn Module,
    host: &HostFns,
    program: &MirProgram,
    selected_functions: &[&MirFunction],
) -> Result<HashMap<ViewThunkKey, FuncId>, String> {
    let specs = view_callback_thunk_specs(program, selected_functions)?;
    let mut thunks = HashMap::new();
    for ((function, callback, arity), callback_ty, _) in specs {
        let mut signature = Signature::new(module.target_config().default_call_conv);
        signature.params.push(AbiParam::new(types::I64));
        if arity <= 2 {
            signature
                .params
                .extend((0..arity).map(|_| AbiParam::new(types::I64)));
        } else {
            // The many-word ABI receives a pointer to a caller-owned array of
            // i64 payload words. The thunk loads each word with the checked
            // callback signature, so no typed carrier reaches the host.
            signature.params.push(AbiParam::new(types::I64));
        }
        signature.returns.push(AbiParam::new(types::I64));
        let name = format!(
            "__jet_view_callback_{}_{}_arity{}",
            function.0, callback.0, arity
        );
        let thunk_id = module
            .declare_function(&name, Linkage::Local, &signature)
            .map_err(|error| error.to_string())?;
        lower_view_callback_thunk(module, host, &callback_ty, arity, thunk_id)?;
        thunks.insert((function, callback, arity), thunk_id);
    }
    Ok(thunks)
}
fn app_callback_shape(row: &jet_foundation::MIR::MirPreludeCall) -> Option<(usize, AppThunkMode)> {
    if row.family != jet_foundation::MIR::MirPreludeFamily::HandleMethod
        || row.module != "core.web.app"
    {
        return None;
    }
    match row.member.as_str() {
        "route" | "page" | "layout" => Some((1, AppThunkMode::Page)),
        "loader" => Some((1, AppThunkMode::Encoded)),
        "action" | "form" | "data" => Some((1, AppThunkMode::Encoded)),
        "pending" | "not_found" | "error" => Some((0, AppThunkMode::Page)),
        _ => None,
    }
}

fn app_callback_thunk_specs(
    program: &MirProgram,
    selected_functions: &[&MirFunction],
) -> Result<Vec<(AppThunkKey, MirType, AppThunkMode)>, String> {
    let mut specs = Vec::new();
    for function in selected_functions {
        for block in &function.blocks {
            for instruction in &block.instructions {
                let MirOperation::Semantic(MirSemanticOp::HandleMethod { call, args, .. }) =
                    &instruction.operation
                else {
                    continue;
                };
                let Some((callback_index, mode)) = program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .and_then(app_callback_shape)
                else {
                    continue;
                };
                let callback = args.get(callback_index).ok_or_else(|| {
                    format!("MIR App `{}` route has no callback argument", call.0)
                })?;
                let callback_ty = function
                    .values
                    .iter()
                    .find(|(value, _, _, _)| *value == *callback)
                    .map(|(_, ty, _, _)| ty.clone())
                    .ok_or_else(|| format!("MIR App callback {:?} has no type row", callback))?;
                let (params, ret) = callable_signature(&callback_ty).ok_or_else(|| {
                    format!("MIR App callback {:?} has no function signature", callback)
                })?;
                if params
                    .iter()
                    .any(|parameter| clif_ty_from_mir(parameter).is_none())
                {
                    return Err(format!(
                        "MIR App callback {:?} has a parameter without a Cranelift ABI",
                        callback
                    ));
                }
                if let Some(result) = ret {
                    if clif_ty_from_mir(result).is_none() {
                        return Err(format!(
                            "MIR App callback {:?} has a return without a Cranelift ABI",
                            callback
                        ));
                    }
                }
                let key = (function.id, *callback);
                if let Some((_, _, existing_mode)) =
                    specs.iter().find(|(existing, _, _)| *existing == key)
                {
                    if std::mem::discriminant(existing_mode) != std::mem::discriminant(&mode) {
                        return Err(format!(
                            "MIR App callback {:?} is used by incompatible route modes",
                            callback
                        ));
                    }
                } else {
                    specs.push((key, callback_ty, mode));
                }
            }
        }
    }
    specs.sort_by_key(|(key, _, _)| *key);
    Ok(specs)
}

fn app_result_constructor(host: &HostFns, ty: Option<types::Type>) -> FuncId {
    match ty {
        Some(types::F64) => host.result_new_f64,
        Some(types::F32) => host.result_new_f64,
        Some(types::I8) => host.result_new_i8,
        Some(types::I32) => host.result_new_i32,
        _ => host.result_new_i64,
    }
}

fn lower_app_result(
    module: &mut dyn Module,
    builder: &mut FunctionBuilder<'_>,
    host: &HostFns,
    mode: AppThunkMode,
    return_kind: Option<&MirTypeKind>,
    return_type: Option<types::Type>,
    result_handles: (i64, i64),
    raw: Value,
) -> Result<Value, String> {
    let result = if return_kind.is_none() {
        let ok = builder.ins().iconst(types::I8, 1);
        let zero = builder.ins().iconst(types::I64, 0);
        thunk_call_host(module, builder, host.result_new_i64, &[ok, zero])?
            .first()
            .copied()
            .ok_or_else(|| "MIR App callback result constructor returned no value".to_string())?
    } else if matches!(return_kind, Some(MirTypeKind::Result { .. })) {
        raw
    } else {
        let ok = builder.ins().iconst(types::I8, 1);
        let value = match return_type {
            Some(types::F64) => raw,
            Some(types::F32) => builder.ins().fpromote(types::F64, raw),
            Some(types::I8) => raw,
            Some(types::I32) => raw,
            _ => thunk_encode_raw(builder, raw, return_type.unwrap_or(types::I64))?,
        };
        thunk_call_host(
            module,
            builder,
            app_result_constructor(host, return_type),
            &[ok, value],
        )?
        .first()
        .copied()
        .ok_or_else(|| "MIR App callback result constructor returned no value".to_string())?
    };
    match mode {
        AppThunkMode::Encoded => {
            let ok_type = builder.ins().iconst(types::I64, result_handles.0);
            let err_type = builder.ins().iconst(types::I64, result_handles.1);
            thunk_call_host(
                module,
                builder,
                host.web.route_encode_result,
                &[result, ok_type, err_type],
            )?
            .first()
            .copied()
            .ok_or_else(|| "MIR App encoded result adapter returned no value".to_string())
        }
        AppThunkMode::Page if matches!(return_kind, Some(MirTypeKind::Result { .. })) => {
            let err_type = builder.ins().iconst(types::I64, result_handles.1);
            thunk_call_host(
                module,
                builder,
                host.web.route_stringify_error,
                &[result, err_type],
            )?
            .first()
            .copied()
            .ok_or_else(|| "MIR App page error adapter returned no value".to_string())
        }
        AppThunkMode::Page => Ok(result),
    }
}

fn lower_app_callback_thunk(
    module: &mut dyn Module,
    host: &HostFns,
    callback_ty: &MirType,
    mode: AppThunkMode,
    type_handles: &[i64],
    result_handles: (i64, i64),
    thunk_id: FuncId,
) -> Result<(), String> {
    let (params, ret) = callable_signature(callback_ty)
        .ok_or_else(|| "MIR App callback thunk has no function signature".to_string())?;
    if params.len() != type_handles.len() {
        return Err("MIR App callback type metadata has the wrong arity".to_string());
    }
    let parameter_types = params
        .iter()
        .map(|parameter| {
            clif_ty_from_mir(parameter)
                .ok_or_else(|| "MIR App callback parameter has no Cranelift ABI".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let return_type = ret
        .map(|result| {
            clif_ty_from_mir(result)
                .ok_or_else(|| "MIR App callback return has no Cranelift ABI".to_string())
        })
        .transpose()?;
    let mut context = module.make_context();
    let mut thunk_signature = Signature::new(module.target_config().default_call_conv);
    thunk_signature.params.push(AbiParam::new(types::I64));
    thunk_signature.params.push(AbiParam::new(types::I64));
    thunk_signature.returns.push(AbiParam::new(types::I64));
    context.func.signature = thunk_signature;
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let params_in = builder.block_params(entry).to_vec();
    let context_handle = params_in
        .first()
        .copied()
        .ok_or_else(|| "MIR App callback thunk has no context parameter".to_string())?;
    let packed = params_in
        .get(1)
        .copied()
        .ok_or_else(|| "MIR App callback thunk has no packed input".to_string())?;

    let mut current = entry;
    let mut decoded = Vec::new();
    for (index, type_handle) in type_handles.iter().copied().enumerate() {
        builder.switch_to_block(current);
        let index_value = builder.ins().iconst(types::I64, index as i64);
        let tree = thunk_call_host(
            module,
            &mut builder,
            host.coll.list_get,
            &[packed, index_value],
        )?
        .first()
        .copied()
        .ok_or_else(|| "MIR App callback input list getter returned no value".to_string())?;
        let type_key = builder.ins().iconst(types::I64, type_handle);
        let decoded_result = thunk_call_host(
            module,
            &mut builder,
            host.web.route_decode_tree,
            &[tree, type_key],
        )?
        .first()
        .copied()
        .ok_or_else(|| "MIR App callback typed decoder returned no value".to_string())?;
        let ok = thunk_call_host(module, &mut builder, host.result_is_ok, &[decoded_result])?
            .first()
            .copied()
            .ok_or_else(|| "MIR App callback decoder result query returned no value".to_string())?;
        let ok = builder.ins().icmp_imm(IntCC::NotEqual, ok, 0);
        let success = builder.create_block();
        let failure = builder.create_block();
        for _ in 0..(decoded.len() + 1) {
            builder.append_block_param(success, types::I64);
        }
        builder.append_block_param(failure, types::I64);
        let mut success_args = decoded.clone();
        success_args.push(
            thunk_call_host(module, &mut builder, host.result_get_i64, &[decoded_result])?
                .first()
                .copied()
                .ok_or_else(|| {
                    "MIR App callback decoder payload query returned no value".to_string()
                })?,
        );
        builder
            .ins()
            .brif(ok, success, &success_args, failure, &[decoded_result]);
        builder.switch_to_block(failure);
        builder.seal_block(failure);
        let failure_result = builder
            .block_params(failure)
            .first()
            .copied()
            .ok_or_else(|| "MIR App callback decoder failure block has no result".to_string())?;
        builder.ins().return_(&[failure_result]);
        builder.switch_to_block(success);
        builder.seal_block(success);
        decoded = builder.block_params(success).to_vec();
        current = success;
    }

    builder.switch_to_block(current);
    let normalized = thunk_call_host(
        module,
        &mut builder,
        host.callable_normalize,
        &[context_handle],
    )?
    .first()
    .copied()
    .ok_or_else(|| "MIR App callable normalizer returned no value".to_string())?;
    let fn_ptr = thunk_call_host(module, &mut builder, host.callable_fn, &[normalized])?
        .first()
        .copied()
        .ok_or_else(|| "MIR App callable function getter returned no value".to_string())?;
    let has_env = thunk_call_host(module, &mut builder, host.callable_has_env, &[normalized])?
        .first()
        .copied()
        .ok_or_else(|| "MIR App callable environment flag returned no value".to_string())?;
    let has_env = builder.ins().icmp_imm(IntCC::NotEqual, has_env, 0);
    let env_block = builder.create_block();
    let plain_block = builder.create_block();
    let merge_block = builder.create_block();
    builder.append_block_param(merge_block, types::I64);
    builder
        .ins()
        .brif(has_env, env_block, &[], plain_block, &[]);

    let mut typed_signature = Signature::new(module.target_config().default_call_conv);
    typed_signature
        .params
        .extend(parameter_types.iter().copied().map(AbiParam::new));
    if let Some(return_type) = return_type {
        typed_signature.returns.push(AbiParam::new(return_type));
    }
    builder.switch_to_block(env_block);
    let env = thunk_call_host(module, &mut builder, host.callable_env, &[normalized])?
        .first()
        .copied()
        .ok_or_else(|| "MIR App callable environment getter returned no value".to_string())?;
    let mut env_signature = typed_signature.clone();
    env_signature.params.insert(0, AbiParam::new(types::I64));
    let env_signature_ref = builder.import_signature(env_signature);
    let mut env_values = Vec::with_capacity(decoded.len() + 1);
    env_values.push(env);
    for (raw, target) in decoded.iter().copied().zip(parameter_types.iter().copied()) {
        env_values.push(thunk_decode_raw(&mut builder, raw, target)?);
    }
    let env_call = builder
        .ins()
        .call_indirect(env_signature_ref, fn_ptr, &env_values);
    let env_result = builder
        .inst_results(env_call)
        .first()
        .copied()
        .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
    let env_result = lower_app_result(
        module,
        &mut builder,
        host,
        mode,
        ret.map(|result| &result.kind),
        return_type,
        result_handles,
        env_result,
    )?;
    builder.ins().jump(merge_block, &[env_result]);

    builder.switch_to_block(plain_block);
    let signature_ref = builder.import_signature(typed_signature);
    let mut plain_values = Vec::with_capacity(decoded.len());
    for (raw, target) in decoded.iter().copied().zip(parameter_types.iter().copied()) {
        plain_values.push(thunk_decode_raw(&mut builder, raw, target)?);
    }
    let plain_call = builder
        .ins()
        .call_indirect(signature_ref, fn_ptr, &plain_values);
    let plain_result = builder
        .inst_results(plain_call)
        .first()
        .copied()
        .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
    let plain_result = lower_app_result(
        module,
        &mut builder,
        host,
        mode,
        ret.map(|result| &result.kind),
        return_type,
        result_handles,
        plain_result,
    )?;
    builder.ins().jump(merge_block, &[plain_result]);
    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    let result = builder
        .block_params(merge_block)
        .first()
        .copied()
        .ok_or_else(|| "MIR App callback result merge has no value".to_string())?;
    builder.ins().return_(&[result]);
    builder.seal_all_blocks();
    builder.finalize();
    define_function_checked(module, thunk_id, &mut context)?;
    module.clear_context(&mut context);
    Ok(())
}

fn install_app_callback_thunks(
    module: &mut dyn Module,
    host: &HostFns,
    program: &MirProgram,
    selected_functions: &[&MirFunction],
    runtime: &mut JitRuntime,
) -> Result<HashMap<AppThunkKey, AppThunk>, String> {
    let specs = app_callback_thunk_specs(program, selected_functions)?;
    let mut thunks = HashMap::new();
    for ((function, callback), callback_ty, mode) in specs {
        let (params, ret) = callable_signature(&callback_ty)
            .ok_or_else(|| "MIR App callback thunk has no callable signature".to_string())?;
        let type_handles = params
            .iter()
            .map(|parameter| runtime.heap.alloc_string(parameter.identity_key()))
            .collect::<Vec<_>>();
        let result_handles = match ret.map(|result| &result.kind) {
            Some(MirTypeKind::Result { ok, err }) => (
                runtime.heap.alloc_string(ok.identity_key()),
                runtime.heap.alloc_string(err.identity_key()),
            ),
            Some(_) => (
                runtime
                    .heap
                    .alloc_string(ret.expect("callback return").identity_key()),
                0,
            ),
            None => (0, 0),
        };
        let output_type = match ret {
            Some(_) => result_handles.0,
            None => runtime.heap.alloc_string("Unit".to_string()),
        };
        let mut signature = Signature::new(module.target_config().default_call_conv);
        signature.params.push(AbiParam::new(types::I64));
        signature.params.push(AbiParam::new(types::I64));
        signature.returns.push(AbiParam::new(types::I64));
        let name = format!("__jet_app_callback_{}_{}", function.0, callback.0);
        let thunk_id = module
            .declare_function(&name, Linkage::Local, &signature)
            .map_err(|error| error.to_string())?;
        lower_app_callback_thunk(
            module,
            host,
            &callback_ty,
            mode,
            &type_handles,
            result_handles,
            thunk_id,
        )?;
        thunks.insert(
            (function, callback),
            AppThunk {
                id: thunk_id,
                mode,
                output_type,
                arity: params.len(),
            },
        );
    }
    Ok(thunks)
}
fn lower_csv_decode_thunk(
    module: &mut dyn Module,
    target_id: FuncId,
    thunk_id: FuncId,
) -> Result<(), String> {
    let mut context = module.make_context();
    let mut signature = Signature::new(module.target_config().default_call_conv);
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    context.func.signature = signature;
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let payload = builder
        .block_params(entry)
        .get(1)
        .copied()
        .ok_or_else(|| "CSV Decode thunk has no DataTree payload".to_string())?;
    let target = module.declare_func_in_func(target_id, builder.func);
    let call = builder.ins().call(target, &[payload]);
    let result = builder
        .inst_results(call)
        .first()
        .copied()
        .ok_or_else(|| "CSV Decode function returned no Result carrier".to_string())?;
    builder.ins().return_(&[result]);
    builder.finalize();
    define_function_checked(module, thunk_id, &mut context)?;
    module.clear_context(&mut context);
    Ok(())
}

fn install_csv_decode_thunks(
    module: &mut dyn Module,
    program: &MirProgram,
    selected_functions: &[&MirFunction],
    function_ids: &HashMap<MirFunctionId, FuncId>,
) -> Result<HashMap<CsvDecodeThunkKey, FuncId>, String> {
    let mut specs = Vec::new();
    for function in selected_functions {
        for block in &function.blocks {
            for instruction in &block.instructions {
                let MirOperation::CoreCall {
                    call, type_args, ..
                } = &instruction.operation
                else {
                    continue;
                };
                let Some(spec) = csv_decode_spec(program, *call, type_args)? else {
                    continue;
                };
                let CsvDecodeTarget::Function(target) = spec.target else {
                    continue;
                };
                let key = (function.id, target);
                if !specs.iter().any(|(existing, _)| *existing == key) {
                    specs.push((key, target));
                }
            }
        }
    }
    specs.sort_by_key(|(key, _)| *key);
    let mut thunks = HashMap::new();
    for ((function, target), target_function) in specs {
        let target_id = *function_ids.get(&target_function).ok_or_else(|| {
            format!(
                "MIR CSV Decode function {:?} was not declared",
                target_function
            )
        })?;
        let signature = {
            let mut signature = Signature::new(module.target_config().default_call_conv);
            signature.params.push(AbiParam::new(types::I64));
            signature.params.push(AbiParam::new(types::I64));
            signature.returns.push(AbiParam::new(types::I64));
            signature
        };
        let thunk_id = module
            .declare_function(
                &format!("__jet_csv_decode_{}_{}", function.0, target.0),
                Linkage::Local,
                &signature,
            )
            .map_err(|error| error.to_string())?;
        lower_csv_decode_thunk(module, target_id, thunk_id)?;
        thunks.insert((function, target), thunk_id);
    }
    Ok(thunks)
}

fn iterable_function_by_symbol<'a>(
    program: &'a MirProgram,
    symbol: &str,
    role: &str,
) -> Result<&'a MirFunction, String> {
    let matches = program
        .functions
        .iter()
        .filter(|function| function.key == symbol)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [function] => Ok(function),
        [] => Err(format!(
            "MIR iterable {role} symbol `{symbol}` has no function row"
        )),
        _ => Err(format!(
            "MIR iterable {role} symbol `{symbol}` resolves to multiple function rows"
        )),
    }
}

fn validate_iterable_hook_function(
    function: &MirFunction,
    role: &str,
) -> Result<types::Type, String> {
    if !function.capture_params.is_empty() {
        return Err(format!(
            "MIR iterable {role} function `{}` has capture parameters",
            function.key
        ));
    }
    if function.generator.is_some() {
        return Err(format!(
            "MIR iterable {role} function `{}` is a generator",
            function.key
        ));
    }
    let parameter = function.params.first().ok_or_else(|| {
        format!(
            "MIR iterable {role} function `{}` has no collection parameter",
            function.key
        )
    })?;
    if function.params.len() != 1 {
        return Err(format!(
            "MIR iterable {role} function `{}` expects {} parameters, not one",
            function.key,
            function.params.len()
        ));
    }
    if clif_ty_from_mir(&parameter.ty).is_none() {
        return Err(format!(
            "MIR iterable {role} function `{}` parameter has no Cranelift ABI",
            function.key
        ));
    }
    clif_ty_from_mir(&function.return_type).ok_or_else(|| {
        format!(
            "MIR iterable {role} function `{}` return has no Cranelift ABI",
            function.key
        )
    })
}

fn iterable_hook_specs(
    program: &MirProgram,
    selected_functions: &[&MirFunction],
) -> Result<
    Vec<(
        IterableThunkKey,
        String,
        String,
        MirFunctionId,
        MirFunctionId,
    )>,
    String,
> {
    let mut specs: Vec<(
        IterableThunkKey,
        String,
        String,
        MirFunctionId,
        MirFunctionId,
    )> = Vec::new();
    for function in selected_functions {
        for block in &function.blocks {
            for instruction in &block.instructions {
                let MirOperation::LoopIterInit {
                    source_kind:
                        jet_foundation::MIR::MirLoopSourceKind::Iterable {
                            coll_type,
                            iter_type,
                            iter_symbol,
                            next_symbol,
                        },
                    ..
                } = &instruction.operation
                else {
                    continue;
                };
                let iter = iterable_function_by_symbol(program, iter_symbol, "iter")?;
                let next = iterable_function_by_symbol(program, next_symbol, "next")?;
                validate_iterable_hook_function(iter, "iter")?;
                validate_iterable_hook_function(next, "next")?;
                let spec = (
                    jet_foundation::MIR::MirLoopSourceKind::Iterable {
                        coll_type: coll_type.clone(),
                        iter_type: iter_type.clone(),
                        iter_symbol: iter_symbol.clone(),
                        next_symbol: next_symbol.clone(),
                    }
                    .wire(),
                    coll_type.clone(),
                    iter_type.clone(),
                    iter.id,
                    next.id,
                );
                if let Some(existing) = specs.iter().find(|existing| existing.0 == spec.0) {
                    if existing != &spec {
                        return Err(format!(
                            "MIR iterable source wire `{}` has conflicting hook functions",
                            spec.0
                        ));
                    }
                } else {
                    specs.push(spec);
                }
            }
        }
    }
    specs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(specs)
}

fn lower_iterable_hook_thunk(
    module: &mut dyn Module,
    function: &MirFunction,
    target_id: FuncId,
    thunk_id: FuncId,
) -> Result<(), String> {
    let parameter = function
        .params
        .first()
        .ok_or_else(|| format!("MIR iterable hook `{}` has no parameter", function.key))?;
    let parameter_type = clif_ty_from_mir(&parameter.ty)
        .ok_or_else(|| format!("MIR iterable hook `{}` parameter has no ABI", function.key))?;
    let return_type = clif_ty_from_mir(&function.return_type)
        .ok_or_else(|| format!("MIR iterable hook `{}` return has no ABI", function.key))?;
    let mut context = module.make_context();
    let mut signature = Signature::new(module.target_config().default_call_conv);
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    context.func.signature = signature;
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let params = builder.block_params(entry).to_vec();
    let raw = params
        .get(1)
        .copied()
        .ok_or_else(|| format!("MIR iterable hook `{}` has no payload", function.key))?;
    let argument = thunk_decode_raw(&mut builder, raw, parameter_type)?;
    let target = module.declare_func_in_func(target_id, builder.func);
    let call = builder.ins().call(target, &[argument]);
    let result = builder
        .inst_results(call)
        .first()
        .copied()
        .ok_or_else(|| format!("MIR iterable hook `{}` returned no value", function.key))?;
    let result = thunk_encode_raw(&mut builder, result, return_type)?;
    builder.ins().return_(&[result]);
    builder.finalize();
    define_function_checked(module, thunk_id, &mut context)?;
    module.clear_context(&mut context);
    Ok(())
}

fn install_iterable_hook_thunks(
    module: &mut dyn Module,
    program: &MirProgram,
    selected_functions: &[&MirFunction],
    function_ids: &HashMap<MirFunctionId, FuncId>,
) -> Result<
    (
        HashMap<IterableThunkKey, IterableThunkIds>,
        Vec<CompiledIterableHook>,
    ),
    String,
> {
    let specs = iterable_hook_specs(program, selected_functions)?;
    let mut thunks = HashMap::new();
    let mut target_thunks = HashMap::new();
    let mut compiled_hooks = Vec::new();
    for (wire, coll_type, iter_type, iter_function, next_function) in specs {
        let ids = if let Some(ids) = target_thunks.get(&(iter_function, next_function)).copied() {
            ids
        } else {
            let iter_id = *function_ids.get(&iter_function).ok_or_else(|| {
                format!(
                    "MIR iterable iter function {:?} was not declared",
                    iter_function
                )
            })?;
            let next_id = *function_ids.get(&next_function).ok_or_else(|| {
                format!(
                    "MIR iterable next function {:?} was not declared",
                    next_function
                )
            })?;
            let signature = {
                let mut signature = Signature::new(module.target_config().default_call_conv);
                signature.params.push(AbiParam::new(types::I64));
                signature.params.push(AbiParam::new(types::I64));
                signature.returns.push(AbiParam::new(types::I64));
                signature
            };
            let iter_thunk = module
                .declare_function(
                    &format!(
                        "__jet_iterable_{}_{}_iter",
                        iter_function.0, next_function.0
                    ),
                    Linkage::Local,
                    &signature,
                )
                .map_err(|error| error.to_string())?;
            let next_thunk = module
                .declare_function(
                    &format!(
                        "__jet_iterable_{}_{}_next",
                        iter_function.0, next_function.0
                    ),
                    Linkage::Local,
                    &signature,
                )
                .map_err(|error| error.to_string())?;
            let iter = program
                .functions
                .iter()
                .find(|function| function.id == iter_function)
                .ok_or_else(|| format!("MIR iterable function {:?} is missing", iter_function))?;
            let next = program
                .functions
                .iter()
                .find(|function| function.id == next_function)
                .ok_or_else(|| format!("MIR iterable function {:?} is missing", next_function))?;
            lower_iterable_hook_thunk(module, iter, iter_id, iter_thunk)?;
            lower_iterable_hook_thunk(module, next, next_id, next_thunk)?;
            let ids = IterableThunkIds {
                iter: iter_thunk,
                next: next_thunk,
            };
            target_thunks.insert((iter_function, next_function), ids);
            ids
        };
        compiled_hooks.push(CompiledIterableHook {
            source_wire: wire.clone(),
            coll_type,
            iter_type,
            iter: ids.iter,
            next: ids.next,
        });
        thunks.insert(wire, ids);
    }
    Ok((thunks, compiled_hooks))
}

pub(crate) fn install_finalized_iterable_hooks(
    module: &JITModule,
    runtime: &mut JitRuntime,
    compiled: &CompiledMirProgram,
) -> Result<(), String> {
    runtime.clear_iterable_hooks();
    for hook in &compiled.iterable_hooks {
        let iter_slot = module.get_finalized_function(hook.iter) as usize as i64;
        let next_slot = module.get_finalized_function(hook.next) as usize as i64;
        runtime.register_iterable_hook(
            &hook.source_wire,
            iter_slot,
            next_slot,
            &hook.coll_type,
            &hook.iter_type,
        )?;
    }
    Ok(())
}
pub(crate) fn install_finalized_hardware_handlers(
    module: &JITModule,
    runtime: &mut JitRuntime,
    program: &MirProgram,
    compiled: &CompiledMirProgram,
) -> Result<(), String> {
    runtime.clear_hardware_interrupt_handlers();
    for setup in &program.facts.hardware_setups {
        let jet_foundation::MIR::MirHardwareSetup::InterruptBind {
            vector,
            handler_symbol,
            ..
        } = setup
        else {
            continue;
        };
        let function = hardware_handler_function(program, handler_symbol)?;
        let function_id = compiled
            .function_ids
            .get(&function.id)
            .copied()
            .ok_or_else(|| {
                format!("MIR interrupt handler `{handler_symbol}` was not compiled by Cranelift")
            })?;
        let handler = module.get_finalized_function(function_id) as usize as i64;
        if handler == 0 {
            return Err(format!(
                "MIR interrupt handler `{handler_symbol}` has no finalized Cranelift address"
            ));
        }
        runtime.register_hardware_interrupt_handler(*vector, handler)?;
    }
    Ok(())
}

fn thunk_cast(
    builder: &mut FunctionBuilder<'_>,
    value: Value,
    target: types::Type,
) -> Result<Value, String> {
    let source = builder.func.dfg.value_type(value);
    if source == target {
        return Ok(value);
    }
    if target == types::I64 {
        if source == types::F64 {
            return Ok(builder.ins().bitcast(types::I64, MemFlags::new(), value));
        }
        if source == types::F32 {
            let bits = builder.ins().bitcast(types::I32, MemFlags::new(), value);
            return Ok(builder.ins().uextend(types::I64, bits));
        }
        return Ok(builder.ins().uextend(types::I64, value));
    }
    if target == types::F64 && source == types::I64 {
        return Ok(builder.ins().bitcast(types::F64, MemFlags::new(), value));
    }
    if target == types::F32 && source == types::I64 {
        let bits = builder.ins().ireduce(types::I32, value);
        return Ok(builder.ins().bitcast(types::F32, MemFlags::new(), bits));
    }
    if target == types::I8 || target == types::I32 {
        if source.bits() > target.bits() {
            return Ok(builder.ins().ireduce(target, value));
        }
        return Ok(builder.ins().uextend(target, value));
    }
    Err(format!(
        "cannot cast Cranelift thunk value {source} -> {target}"
    ))
}

fn thunk_call_host(
    module: &mut dyn Module,
    builder: &mut FunctionBuilder<'_>,
    id: FuncId,
    values: &[Value],
) -> Result<Vec<Value>, String> {
    let signature = module
        .declarations()
        .get_function_decl(id)
        .signature
        .clone();
    if signature.params.len() != values.len() {
        return Err(format!(
            "universal callback host ABI expects {} arguments, got {}",
            signature.params.len(),
            values.len()
        ));
    }
    let values = values
        .iter()
        .copied()
        .zip(signature.params.iter())
        .map(|(value, parameter)| thunk_cast(builder, value, parameter.value_type))
        .collect::<Result<Vec<_>, _>>()?;
    let local = module.declare_func_in_func(id, builder.func);
    let call = builder.ins().call(local, &values);
    Ok(builder.inst_results(call).to_vec())
}

fn thunk_decode_raw(
    builder: &mut FunctionBuilder<'_>,
    value: Value,
    target: types::Type,
) -> Result<Value, String> {
    thunk_cast(builder, value, target)
}

fn thunk_encode_raw(
    builder: &mut FunctionBuilder<'_>,
    value: Value,
    source: types::Type,
) -> Result<Value, String> {
    if source == types::I64 {
        return Ok(value);
    }
    if source == types::F64 {
        return Ok(builder.ins().bitcast(types::I64, MemFlags::new(), value));
    }
    if source == types::F32 {
        let bits = builder.ins().bitcast(types::I32, MemFlags::new(), value);
        return Ok(builder.ins().uextend(types::I64, bits));
    }
    if source == types::I8 || source == types::I32 {
        return Ok(thunk_cast(builder, value, types::I64)?);
    }
    Err(format!(
        "universal callback return has unsupported Cranelift carrier {source}"
    ))
}

fn lower_view_callback_thunk(
    module: &mut dyn Module,
    host: &HostFns,
    callback_ty: &MirType,
    arity: usize,
    thunk_id: FuncId,
) -> Result<(), String> {
    let (params, ret) = callable_signature(callback_ty)
        .ok_or_else(|| "MIR View callback thunk has no function signature".to_string())?;
    let parameter_types = params
        .iter()
        .map(|parameter| {
            clif_ty_from_mir(parameter)
                .ok_or_else(|| "MIR View callback parameter has no Cranelift ABI".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if parameter_types.len() != arity {
        return Err("MIR View callback thunk parameter arity is not canonical".to_string());
    }
    let conventions = callback_ty
        .function_signature()
        .and_then(|signature| signature.call_metadata.as_ref())
        .map(|metadata| metadata.conventions.as_slice());
    let return_type = ret
        .map(|result| {
            clif_ty_from_mir(result)
                .ok_or_else(|| "MIR View callback return has no Cranelift ABI".to_string())
        })
        .transpose()?;
    let mut context = module.make_context();
    let mut thunk_signature = Signature::new(module.target_config().default_call_conv);
    thunk_signature.params.push(AbiParam::new(types::I64));
    if arity <= 2 {
        thunk_signature
            .params
            .extend((0..arity).map(|_| AbiParam::new(types::I64)));
    } else {
        thunk_signature.params.push(AbiParam::new(types::I64));
    }
    thunk_signature.returns.push(AbiParam::new(types::I64));
    context.func.signature = thunk_signature;
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let params = builder.block_params(entry).to_vec();
    let context_handle = params
        .first()
        .copied()
        .ok_or_else(|| "universal callback thunk has no context parameter".to_string())?;
    let raw_values = if arity <= 2 {
        params
            .get(1..1 + parameter_types.len())
            .ok_or_else(|| "universal callback thunk has no payload parameters".to_string())?
            .to_vec()
    } else {
        let payload = params
            .get(1)
            .copied()
            .ok_or_else(|| "universal callback thunk has no many-word payload".to_string())?;
        (0..arity)
            .map(|index| {
                let offset = builder
                    .ins()
                    .iconst(types::I64, (index.saturating_mul(8)) as i64);
                let address = builder.ins().iadd(payload, offset);
                if conventions.and_then(|access| access.get(index)) == Some(&MirAccess::Write) {
                    address
                } else {
                    builder.ins().load(types::I64, MemFlags::new(), address, 0)
                }
            })
            .collect()
    };
    let normalized = thunk_call_host(
        module,
        &mut builder,
        host.callable_normalize,
        &[context_handle],
    )?
    .first()
    .copied()
    .ok_or_else(|| "MIR callable normalizer returned no value".to_string())?;
    let fn_ptr = thunk_call_host(module, &mut builder, host.callable_fn, &[normalized])?
        .first()
        .copied()
        .ok_or_else(|| "MIR callable function getter returned no value".to_string())?;
    let has_env = thunk_call_host(module, &mut builder, host.callable_has_env, &[normalized])?
        .first()
        .copied()
        .ok_or_else(|| "MIR callable environment flag returned no value".to_string())?;
    let has_env = builder.ins().icmp_imm(IntCC::NotEqual, has_env, 0);
    let env_block = builder.create_block();
    let plain_block = builder.create_block();
    let merge_block = builder.create_block();
    if return_type.is_some() {
        builder.append_block_param(merge_block, types::I64);
    }
    let mut typed_signature = Signature::new(module.target_config().default_call_conv);
    let mut typed_values = Vec::with_capacity(raw_values.len());
    let mut writebacks = Vec::new();
    for (index, (raw, target)) in raw_values
        .iter()
        .copied()
        .zip(parameter_types.iter().copied())
        .enumerate()
    {
        if conventions.and_then(|access| access.get(index)) == Some(&MirAccess::Write) {
            // The host lends a raw word. Give the callback a correctly sized
            // typed place, then encode its final value back into that word.
            let word = builder.ins().load(types::I64, MemFlags::new(), raw, 0);
            let value = thunk_decode_raw(&mut builder, word, target)?;
            let slot = builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                8,
                0,
            ));
            builder.ins().stack_store(value, slot, 0);
            typed_values.push(builder.ins().stack_addr(types::I64, slot, 0));
            typed_signature.params.push(AbiParam::new(types::I64));
            writebacks.push((raw, slot, target));
        } else {
            typed_values.push(thunk_decode_raw(&mut builder, raw, target)?);
            typed_signature.params.push(AbiParam::new(target));
        }
    }
    if let Some(return_type) = return_type {
        typed_signature.returns.push(AbiParam::new(return_type));
    }
    builder
        .ins()
        .brif(has_env, env_block, &[], plain_block, &[]);

    builder.switch_to_block(env_block);
    let env = thunk_call_host(module, &mut builder, host.callable_env, &[normalized])?
        .first()
        .copied()
        .ok_or_else(|| "MIR callable environment getter returned no value".to_string())?;
    let mut env_signature = typed_signature.clone();
    env_signature.params.insert(0, AbiParam::new(types::I64));
    let env_signature_ref = builder.import_signature(env_signature);
    let mut env_values = Vec::with_capacity(raw_values.len() + 1);
    env_values.push(env);
    env_values.extend_from_slice(&typed_values);
    let env_call = builder
        .ins()
        .call_indirect(env_signature_ref, fn_ptr, &env_values);
    if let Some(return_type) = return_type {
        let result = builder
            .inst_results(env_call)
            .first()
            .copied()
            .ok_or_else(|| "MIR captured View callback returned no value".to_string())?;
        let result = thunk_encode_raw(&mut builder, result, return_type)?;
        builder.ins().jump(merge_block, &[result]);
    } else {
        builder.ins().jump(merge_block, &[]);
    }

    builder.switch_to_block(plain_block);
    let signature_ref = builder.import_signature(typed_signature);
    let plain_call = builder
        .ins()
        .call_indirect(signature_ref, fn_ptr, &typed_values);
    if let Some(return_type) = return_type {
        let result = builder
            .inst_results(plain_call)
            .first()
            .copied()
            .ok_or_else(|| "MIR plain View callback returned no value".to_string())?;
        let result = thunk_encode_raw(&mut builder, result, return_type)?;
        builder.ins().jump(merge_block, &[result]);
    } else {
        builder.ins().jump(merge_block, &[]);
    }

    builder.switch_to_block(merge_block);
    for (address, slot, target) in writebacks {
        let value = builder.ins().stack_load(target, slot, 0);
        let raw = thunk_encode_raw(&mut builder, value, target)?;
        builder.ins().store(MemFlags::new(), raw, address, 0);
    }
    if return_type.is_some() {
        let result = builder
            .block_params(merge_block)
            .first()
            .copied()
            .ok_or_else(|| "universal callback thunk merge has no result".to_string())?;
        builder.ins().return_(&[result]);
    } else {
        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().return_(&[zero]);
    }
    builder.seal_all_blocks();
    builder.finalize();
    define_function_checked(module, thunk_id, &mut context)?;
    module.clear_context(&mut context);
    Ok(())
}

fn function_signature<M: Module + ?Sized>(
    module: &M,
    function: &MirFunction,
) -> Result<Signature, String> {
    let mut signature = Signature::new(module.target_config().default_call_conv);
    if !function.capture_params.is_empty() {
        signature.params.push(AbiParam::new(types::I64));
    }
    for parameter in &function.params {
        let ty = if parameter.access == MirAccess::Write {
            types::I64
        } else {
            clif_ty_from_mir(&parameter.ty)
                .ok_or_else(|| format!("MIR function `{}` has a never parameter", function.key))?
        };
        signature.params.push(AbiParam::new(ty));
    }
    if function.generator.is_some() {
        signature.returns.push(AbiParam::new(types::I64));
    } else if let Some(ty) = clif_ty_from_mir(&function.return_type) {
        signature.returns.push(AbiParam::new(ty));
    }
    Ok(signature)
}

fn generator_body_signature<M: Module + ?Sized>(
    module: &M,
    function: &MirFunction,
) -> Result<Signature, String> {
    let mut signature = Signature::new(module.target_config().default_call_conv);
    if !function.capture_params.is_empty() {
        signature.params.push(AbiParam::new(types::I64));
    }
    for parameter in &function.params {
        let ty = clif_ty_from_mir(&parameter.ty)
            .ok_or_else(|| format!("MIR generator `{}` has a never parameter", function.key))?;
        if ty != types::I64 {
            return Err(format!(
                "MIR generator `{}` parameter {} has non-word ABI {ty}",
                function.key, parameter.name
            ));
        }
        signature.params.push(AbiParam::new(ty));
    }
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    Ok(signature)
}

fn lower_generator_wrapper(
    module: &mut dyn Module,
    host: &HostFns,
    function: &MirFunction,
    function_id: FuncId,
    body_id: FuncId,
) -> Result<(), String> {
    let mut context = module.make_context();
    context.func.signature = function_signature(module, function)?;
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let params = builder.block_params(entry).to_vec();
    if params
        .iter()
        .any(|value| builder.func.dfg.value_type(*value) != types::I64)
    {
        return Err(format!(
            "MIR generator `{}` wrapper has non-word parameter ABI",
            function.key
        ));
    }
    let body_args = params
        .len()
        .checked_add(1)
        .ok_or_else(|| format!("MIR generator `{}` parameter count overflow", function.key))?;
    let spawn_id = match body_args {
        1 => host.conc.spawn1,
        2 => host.conc.spawn2,
        3 => host.conc.spawn3,
        4 => host.conc.spawn4,
        _ => return Err(format!(
            "MIR generator `{}` has {} hidden/source arguments; spawn ABI supports at most four",
            function.key, body_args
        )),
    };
    let channel_new = module.declare_func_in_func(host.conc.generator_channel_new, builder.func);
    let channel = builder.ins().call(channel_new, &[]);
    let channel = builder.inst_results(channel)[0];
    let channel_sender = module.declare_func_in_func(host.conc.channel_sender, builder.func);
    let sender = builder.ins().call(channel_sender, &[channel]);
    let sender = builder.inst_results(sender)[0];
    let body = module.declare_func_in_func(body_id, builder.func);
    let body_ptr = builder.ins().func_addr(types::I64, body);
    let spawn_site = builder.ins().iconst(types::I64, 0);
    let mut spawn_args = vec![spawn_site, body_ptr];
    spawn_args.extend(params);
    spawn_args.push(sender);
    let spawn = module.declare_func_in_func(spawn_id, builder.func);
    let task = builder.ins().call(spawn, &spawn_args);
    let task = builder.inst_results(task)[0];
    let stream_attach =
        module.declare_func_in_func(host.conc.generator_stream_attach, builder.func);
    builder.ins().call(stream_attach, &[channel, task]);
    builder.ins().return_(&[channel]);
    builder.finalize();
    define_function_checked(module, function_id, &mut context)?;
    module.clear_context(&mut context);
    Ok(())
}

fn lower_function(
    module: &mut dyn Module,
    host: &HostFns,
    program: &MirProgram,
    function: &MirFunction,
    runtime: &mut JitRuntime,
    function_ids: &HashMap<MirFunctionId, FuncId>,
    view_thunks: &HashMap<ViewThunkKey, FuncId>,
    app_thunks: &HashMap<AppThunkKey, AppThunk>,
    csv_thunks: &HashMap<CsvDecodeThunkKey, FuncId>,
    function_id: FuncId,
    generator_body: bool,
) -> Result<(), String> {
    let mut context = module.make_context();
    context.func.signature = if generator_body {
        generator_body_signature(module, function)?
    } else {
        function_signature(module, function)?
    };
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
    let mut lower = FunctionLower {
        module,
        host,
        program,
        function,
        runtime,
        blocks: HashMap::new(),
        block_phis: HashMap::new(),
        values: HashMap::new(),
        places: HashMap::new(),
        write_parameters: Vec::new(),
        function_ids,
        view_thunks,
        app_thunks,
        csv_thunks,
        capture_env: None,
        yield_sender: None,
        stop_block: None,
    };
    for block in &function.blocks {
        lower.blocks.insert(block.id, builder.create_block());
        let mut phis = Vec::new();
        for instruction in &block.instructions {
            if let MirOperation::Phi { .. } = instruction.operation {
                let result = instruction
                    .result
                    .ok_or_else(|| format!("MIR phi in `{}` has no result", function.key))?;
                let ty = instruction
                    .ty
                    .as_ref()
                    .or_else(|| {
                        function
                            .values
                            .iter()
                            .find(|(id, _, _, _)| *id == result)
                            .map(|(_, ty, _, _)| ty)
                    })
                    .and_then(clif_ty_from_mir)
                    .ok_or_else(|| format!("MIR phi {:?} has no ABI", result))?;
                phis.push((result, ty));
            }
        }
        lower.block_phis.insert(block.id, phis);
    }
    let entry_block = *lower
        .blocks
        .get(&function.entry)
        .ok_or_else(|| format!("MIR `{}` entry block is missing", function.key))?;
    builder.append_block_params_for_function_params(entry_block);
    for block in &function.blocks {
        let clif_block = lower.blocks[&block.id];
        if let Some(phis) = lower.block_phis.get(&block.id) {
            for (_, ty) in phis {
                builder.append_block_param(clif_block, *ty);
            }
        }
    }
    let parameter_values = builder.block_params(entry_block).to_vec();
    let capture_env_offset = usize::from(!function.capture_params.is_empty());
    if capture_env_offset != 0 {
        let env = *parameter_values.first().ok_or_else(|| {
            format!(
                "MIR captured function `{}` has no environment parameter",
                function.key
            )
        })?;
        lower.capture_env = Some(env);
    }
    if generator_body {
        let sender_index = capture_env_offset + function.params.len();
        lower.yield_sender =
            Some(*parameter_values.get(sender_index).ok_or_else(|| {
                format!("MIR generator `{}` has no sender parameter", function.key)
            })?);
    }
    for (index, parameter) in function.params.iter().enumerate() {
        let value = *parameter_values
            .get(index + capture_env_offset)
            .ok_or_else(|| {
                format!(
                    "MIR parameter {} missing in `{}`",
                    parameter.index, function.key
                )
            })?;
        for instruction in function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .filter(|instruction| {
                matches!(
                    &instruction.operation,
                    MirOperation::Parameter { index: i, name, .. }
                        if *i == parameter.index && name == &parameter.name
                )
            })
        {
            if let Some(result) = instruction.result {
                lower.values.insert(result, value);
                if parameter.access == MirAccess::Write {
                    let ty = clif_ty_from_mir(&parameter.ty).ok_or_else(|| {
                        format!("MIR write parameter {} has no ABI", parameter.name)
                    })?;
                    lower.write_parameters.push((result, value, ty));
                }
            }
        }
    }
    // Cranelift's first switched block is the function entry. Canonical MIR
    // sorts blocks by id, so a successor can sort before its predecessor.
    // Lower in CFG order from `function.entry` so a value defined in one
    // block exists before a later block reads it (`??` OptionValue, etc.).
    for block in cfg_block_order(function) {
        let clif_block = lower.blocks[&block.id];
        builder.switch_to_block(clif_block);
        let _ = lower.call_host(&mut builder, lower.host.hardware_interrupt_poll, &[])?;
        if block.id == function.entry {
            for &(result, address, ty) in &lower.write_parameters {
                let value = builder.ins().load(ty, MemFlags::new(), address, 0);
                lower.values.insert(result, value);
            }
        }
        let block_params = builder.block_params(clif_block).to_vec();
        let phi_offset = if block.id == function.entry {
            function.params.len() + capture_env_offset + usize::from(generator_body)
        } else {
            0
        };
        if let Some(phis) = lower.block_phis.get(&block.id) {
            for (index, (result, _)) in phis.iter().enumerate() {
                let value = *block_params
                    .get(phi_offset + index)
                    .ok_or_else(|| format!("MIR phi parameter missing in `{}`", function.key))?;
                lower.values.insert(*result, value);
            }
        }
        for instruction in &block.instructions {
            if matches!(
                &instruction.operation,
                MirOperation::Phi { .. } | MirOperation::Parameter { .. }
            ) {
                continue;
            }
            let result = lower.lower_instruction(&mut builder, instruction)?;
            if let (Some(id), Some(value)) = (instruction.result, result) {
                lower.values.insert(id, value);
            } else if instruction.result.is_some() {
                return Err(format!(
                    "MIR operation {:?} produced no value",
                    instruction.operation
                ));
            }
        }
        lower.lower_terminator(&mut builder, block)?;
    }
    if let Some(stop_block) = lower.stop_block {
        builder.switch_to_block(stop_block);
        let mut values = Vec::with_capacity(builder.func.signature.returns.len());
        for index in 0..builder.func.signature.returns.len() {
            let ty = builder.func.signature.returns[index].value_type;
            values.push(match ty {
                types::F32 => builder.ins().f32const(0.0),
                types::F64 => builder.ins().f64const(0.0),
                _ => builder.ins().iconst(ty, 0),
            });
        }
        builder.ins().return_(&values);
    }
    builder.seal_all_blocks();
    builder.finalize();
    define_function_checked(module, function_id, &mut context)?;
    module.clear_context(&mut context);
    Ok(())
}

impl<'a, 'm> FunctionLower<'a, 'm> {
    fn lower_instruction(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        instruction: &MirInstruction,
    ) -> Result<Option<Value>, String> {
        let expected = instruction.ty.as_ref().and_then(clif_ty_from_mir);
        let result = match &instruction.operation {
            MirOperation::Parameter { .. } | MirOperation::Phi { .. } => None,
            MirOperation::Capture { slot } => Some(self.capture(builder, *slot, expected)?),
            MirOperation::ReadPlace(place) => Some(self.read_place(builder, *place)?),
            MirOperation::MovePlace { place } => {
                let place_row = self
                    .function
                    .places
                    .iter()
                    .find(|candidate| candidate.id == *place)
                    .ok_or_else(|| format!("MIR place {:?} is missing", place))?;
                if place_row.access != MirAccess::Move {
                    return Err(format!(
                        "MIR MovePlace {:?} does not have move access",
                        place
                    ));
                }
                let value = self.read_place(builder, *place)?;
                Some(value)
            }
            MirOperation::InitializeUninit { place } => {
                let place_row = self
                    .function
                    .places
                    .iter()
                    .find(|candidate| candidate.id == *place)
                    .cloned()
                    .ok_or_else(|| format!("MIR place {:?} is missing", place))?;
                let MirPlaceBase::Local(local) = &place_row.base else {
                    return Err(format!("MIR uninitialized place {:?} is not local", place));
                };
                let fixed_len = self
                    .function
                    .locals
                    .iter()
                    .find(|candidate| candidate.id == *local)
                    .ok_or_else(|| format!("MIR local {:?} is missing", local))?
                    .ty
                    .kind()
                    .clone();
                let slot = self.slot(builder, &place_row)?;
                if let MirTypeKind::FixedList { len, .. } = fixed_len {
                    let len = len
                        .literal_value()
                        .and_then(|len| i64::try_from(len).ok())
                        .ok_or_else(|| {
                            format!(
                                "MIR uninitialized fixed-list local {:?} has no static length",
                                local
                            )
                        })?;
                    let length = builder.ins().iconst(types::I64, len);
                    let carrier = self
                        .call_host(builder, self.host.coll.list_uninit, &[length])?
                        .first()
                        .copied()
                        .ok_or_else(|| "MIR fixed-list initializer returned no carrier".to_string())?;
                    builder.ins().stack_store(carrier, slot, 0);
                }
                None
            }
            MirOperation::WritePlace { place, value } => {
                let value_id = *value;
                let value = self.value(value_id)?;
                self.write_place(builder, *place, value_id, value)?;
                self.observe_live_place(builder, *place)?;
                None
            }
            MirOperation::Copy { value } => Some(self.copy_value(builder, *value)?),
            MirOperation::Move { value } => Some(self.value(*value)?),
            MirOperation::Constant(constant) => Some(self.constant(builder, constant, expected)?),
            MirOperation::Unary { op, value } => {
                let default_int = Self::default_int_type(&self.mir_value_type(*value)?);
                let value = self.value(*value)?;
                Some(self.unary(builder, *op, default_int, value)?)
            }
            MirOperation::Binary {
                op,
                dispatch,
                left,
                right,
            } => match dispatch {
                jet_foundation::MIR::MirBinaryDispatch::Primitive => {
                    let left_ty = self.mir_value_type(*left)?;
                    let right_ty = self.mir_value_type(*right)?;
                    let left = self.value(*left)?;
                    let right = self.value(*right)?;
                    let location = self.instruction_location(instruction)?;
                    Some(self.binary(builder, *op, &left_ty, &right_ty, left, right, location)?)
                }
                jet_foundation::MIR::MirBinaryDispatch::Prelude { call, location } => {
                    // The lowering attaches a source location to every
                    // Prelude binary dispatch; only rows whose canonical
                    // signature carries `(file, line)` consume it
                    // (`routes.rs` `exact_int_binary_route`: arity 4 for the
                    // division family, 2 for `add`/`sub`/`mul`/bit ops). The
                    // interpreter's `binary_prelude_args` and the AOT emitter
                    // gate on the same arity.
                    let arity = self
                        .program
                        .prelude_calls
                        .iter()
                        .find(|row| row.id == *call)
                        .map(|row| row.signature.arity)
                        .ok_or_else(|| format!("MIR Prelude call {:?} is missing", call))?;
                    let mut values = vec![self.value(*left)?, self.value(*right)?];
                    match (arity, location) {
                        (2, _) => {}
                        (4, Some(location)) => {
                            values.extend(self.panic_location(builder, location)?)
                        }
                        (4, None) => {
                            return Err(
                                "MIR binary Prelude route requires source location".to_string()
                            )
                        }
                        (arity, _) => {
                            return Err(format!(
                                "MIR binary Prelude route has unsupported arity {arity}"
                            ))
                        }
                    }
                    Some(self.call_prelude(builder, *call, values, expected)?)
                }
            },
            MirOperation::BuildString { parts } => Some(self.build_string(builder, parts)?),
            MirOperation::BuildList { values } => Some(self.build_list(builder, values)?),
            MirOperation::BuildMap { entries } => Some(self.build_map(builder, entries)?),
            MirOperation::ProjectMembers { base, members } => {
                Some(self.project_members(builder, *base, members)?)
            }
            MirOperation::EnumIs {
                subject,
                owner,
                variant,
            } => Some(self.enum_is(builder, *subject, *owner, variant)?),
            MirOperation::EnumPayload {
                subject,
                owner,
                variant,
                index,
            } => Some(self.enum_payload(builder, *subject, *owner, variant, *index, expected)?),
            MirOperation::OptionIsSome { subject } => {
                if self.packed_optional_subject(*subject) {
                    let raw = self.cast(builder, self.value(*subject)?, types::I64)?;
                    let zero = builder.ins().iconst(types::I64, 0);
                    Some(builder.ins().icmp(IntCC::NotEqual, raw, zero))
                } else {
                    Some(self.result_is_ok(builder, *subject)?)
                }
            }
            MirOperation::OptionValue { subject } => {
                if self.packed_optional_subject(*subject) {
                    let raw = self.cast(builder, self.value(*subject)?, types::I64)?;
                    Some(builder.ins().iadd_imm(raw, -1))
                } else {
                    Some(self.result_value_get(builder, *subject, true, expected)?)
                }
            }
            MirOperation::ResultIsOk { subject } => Some(self.result_is_ok(builder, *subject)?),
            MirOperation::ResultValue { subject, ok } => {
                Some(self.result_value_get(builder, *subject, *ok, expected)?)
            }
            MirOperation::PatternCapture { matched, index } => {
                Some(self.pattern_capture(builder, *matched, *index, expected)?)
            }
            MirOperation::Index {
                call,
                base,
                index,
                kind,
                access,
                location,
                context,
            } => {
                if *access == jet_foundation::MIR::MirAccess::Move {
                    return Err("MIR index move has no checked Cranelift carrier".to_string());
                }
                let base_type = self.mir_value_type(*base)?;
                Some(self.index_read(
                    builder,
                    self.value(*base)?,
                    Some(&base_type),
                    *index,
                    *kind,
                    *call,
                    location,
                    Some(context),
                    None,
                    &instruction.span,
                    expected,
                )?)
            }
            MirOperation::Slice {
                call,
                base,
                start,
                end,
                range,
                location,
            } => {
                let mut values = if let Some(range) = range {
                    vec![self.value(*base)?, self.value(*range)?]
                } else {
                    vec![self.value(*base)?, self.value(*start)?, self.value(*end)?]
                };
                values.extend(self.panic_location(builder, location)?);
                Some(self.call_prelude(builder, *call, values, expected)?)
            }
            MirOperation::Range {
                start,
                end,
                exclusive,
            } => Some(self.range(builder, *start, *end, *exclusive, expected)?),
            MirOperation::Struct { type_id, fields } | MirOperation::Tuple { type_id, fields } => {
                Some(self.aggregate(builder, *type_id, fields)?)
            }
            MirOperation::Enum {
                type_id,
                variant,
                args,
            } => Some(self.enum_value(builder, *type_id, variant, args)?),
            MirOperation::PatternMatched { matched } => Some(self.result_is_ok(builder, *matched)?),
            MirOperation::Present { value } | MirOperation::ResultOk { value } => {
                Some(self.result_value(builder, true, self.value(*value)?, expected)?)
            }
            MirOperation::Convert {
                value,
                parameters,
                target,
                conversion,
            } => match conversion {
                jet_foundation::MIR::MirConversion::NumericCast => {
                    if !parameters.is_empty() {
                        return Err("numeric MIR conversion has unexpected parameters".to_string());
                    }
                    Some(self.numeric_cast(builder, *value, target)?)
                }
                jet_foundation::MIR::MirConversion::Transparent
                | jet_foundation::MIR::MirConversion::SendFn => {
                    if !parameters.is_empty() {
                        return Err("MIR callable conversion has unexpected parameters".to_string());
                    }
                    Some(self.value(*value)?)
                }
                jet_foundation::MIR::MirConversion::Prelude { call, location, .. } => {
                    let arity = self
                        .program
                        .prelude_calls
                        .iter()
                        .find(|row| row.id == *call)
                        .ok_or_else(|| {
                            format!("MIR conversion references missing Prelude call {call:?}")
                        })?
                        .signature
                        .arity;
                    let mut operand = self.value(*value)?;
                    if let Some((signed, _)) = self.mir_value_type(*value)?.fixed_int() {
                        let ty = builder.func.dfg.value_type(operand);
                        if ty.is_int() && ty.bits() < 64 {
                            operand = if signed {
                                builder.ins().sextend(types::I64, operand)
                            } else {
                                builder.ins().uextend(types::I64, operand)
                            };
                        }
                    }
                    let mut values = vec![operand];
                    values.extend(
                        parameters
                            .iter()
                            .map(|id| self.value(*id))
                            .collect::<Result<Vec<_>, _>>()?,
                    );
                    match arity.checked_sub(values.len()) {
                        Some(0) => {}
                        Some(2) => values.extend(self.panic_location(builder, location)?),
                        _ => {
                            return Err(
                                "MIR conversion operands do not match its checked Prelude ABI"
                                    .to_string(),
                            )
                        }
                    }
                    Some(self.call_prelude(builder, *call, values, expected)?)
                }
            },
            MirOperation::Absent => Some(self.result_absent(builder)?),
            MirOperation::ResultErr { value } => {
                Some(self.result_value(builder, false, self.value(*value)?, expected)?)
            }
            MirOperation::Call { callee, args, .. } => {
                Some(self.call_callee(builder, callee, args, expected)?)
            }
            MirOperation::CoreCall {
                call,
                route,
                args,
                type_args,
                data_plan: _data_plan,
                ..
            } => Some(self.call_core(
                builder,
                *call,
                Some(*route),
                type_args,
                args,
                instruction.ty.as_ref(),
                expected,
            )?),
            MirOperation::IndirectCall { callee, args, .. } => {
                Some(self.indirect_call(builder, *callee, args, expected)?)
            }
            MirOperation::Closure {
                function,
                captures,
                facts,
            } => {
                let callback_value = instruction.result;
                let callback_ty = instruction.ty.as_ref();
                let owned_callback_captures = instruction.result.is_some_and(|result| {
                    self.function
                        .blocks
                        .iter()
                        .flat_map(|block| block.instructions.iter())
                        .any(|candidate| {
                            let MirOperation::Semantic(MirSemanticOp::CCallback {
                                callback,
                                lambda,
                                ..
                            }) = &candidate.operation
                            else {
                                return false;
                            };
                            *lambda == result
                                && self
                                    .program
                                    .callbacks
                                    .iter()
                                    .any(|row| row.id == *callback && row.managed)
                        })
                });
                Some(self.closure(
                    builder,
                    *function,
                    captures,
                    facts,
                    owned_callback_captures,
                    callback_value,
                    callback_ty,
                    expected,
                )?)
            }
            MirOperation::PtrFromAddr { addr, .. } => Some(self.value(*addr)?),
            MirOperation::Deref { value: addr } => {
                let addr_ty = self.mir_value_type(*addr)?;
                let addr = self.cast(builder, self.value(*addr)?, types::I64)?;
                if is_allocator_view_type(&addr_ty) {
                    Some(
                        self.call_host(builder, self.host.memory.allocator_view_read, &[addr])?
                            .first()
                            .copied()
                            .ok_or_else(|| {
                                "MIR allocator view reader returned no value".to_string()
                            })?,
                    )
                } else {
                    let ty =
                        expected.ok_or_else(|| "MIR dereference has no pointee ABI".to_string())?;
                    Some(builder.ins().load(ty, MemFlags::new(), addr, 0))
                }
            }
            MirOperation::RawAddressOf { place } => Some(self.address_of(builder, *place)?),
            MirOperation::AddressOf {
                place,
                access: MirAccess::Read,
            } => Some(self.read_place(builder, *place)?),
            MirOperation::AddressOf { place, .. } => Some(self.address_of(builder, *place)?),
            MirOperation::AttachTag { value, .. } => Some(self.value(*value)?),
            MirOperation::Field { base, field } => {
                Some(self.field_load(builder, *base, *field, expected)?)
            }
            MirOperation::Global { name } => {
                // No tier carries run-provided globals: the interpreter refuses
                // the op at runtime ("not provided by the run") and this backend
                // reports unsupported MIR at its execution boundary.
                return Err(format!(
                    "MIR global `{name}` is not provided by the resident run"
                ));
            }
            MirOperation::Todo {
                call,
                location,
                expected_type,
            } => {
                let expected_type = expected_type
                    .as_ref()
                    .map(|ty| ty.display_name())
                    .unwrap_or_else(|| "(unknown)".to_string());
                let expected_type = builder
                    .ins()
                    .iconst(types::I64, self.runtime.heap.alloc_string(expected_type));
                let line = builder.ins().iconst(types::I64, i64::from(location.line));
                Some(self.call_prelude(builder, *call, vec![line, expected_type], expected)?)
            }
            MirOperation::Never { .. } => Some(self.trap(builder)?),
            MirOperation::Semantic(operation) => {
                self.semantic(builder, operation, instruction.source_line, expected)?
            }
            MirOperation::LoopRangeInit {
                call,
                start,
                end,
                step,
                exclusive,
            } => {
                let start =
                    self.index_operand(builder, *start, jet_foundation::MIR::MirIndexKind::List)?;
                let end =
                    self.index_operand(builder, *end, jet_foundation::MIR::MirIndexKind::List)?;
                let (step, has_step) = match step {
                    Some(step) => (
                        self.index_operand(
                            builder,
                            *step,
                            jet_foundation::MIR::MirIndexKind::List,
                        )?,
                        builder.ins().iconst(types::I8, 1),
                    ),
                    None => (
                        builder.ins().iconst(types::I64, 0),
                        builder.ins().iconst(types::I8, 0),
                    ),
                };
                let exclusive = builder.ins().iconst(types::I8, i64::from(*exclusive));
                Some(self.call_prelude(
                    builder,
                    *call,
                    vec![start, end, step, has_step, exclusive],
                    expected,
                )?)
            }
            MirOperation::LoopRangeValue { call, cursor } => {
                let value =
                    self.call_prelude(builder, *call, vec![self.value(*cursor)?], expected)?;
                Some(self.native_int_result(builder, value, instruction.ty.as_ref())?)
            }
            MirOperation::LoopRangeHasNext { call, cursor }
            | MirOperation::LoopRangeAdvance { call, cursor }
            | MirOperation::LoopIterHasNext { call, cursor }
            | MirOperation::LoopIterValue { call, cursor }
            | MirOperation::LoopIterAdvance { call, cursor } => {
                Some(self.call_prelude(builder, *call, vec![self.value(*cursor)?], expected)?)
            }
            MirOperation::LoopIterInit {
                call,
                collection,
                step,
                by_value,
                source_kind,
            } => {
                let collection_type = self.mir_value_type(*collection)?;
                let map_types = match (source_kind, comparison_map_parts(&collection_type)) {
                    (jet_foundation::MIR::MirLoopSourceKind::Plain, Some((key, value))) => Some((
                        runtime_type_id(key).ok_or_else(|| {
                            "MIR map loop key lacks a runtime type identity".to_string()
                        })?,
                        runtime_type_id(value).ok_or_else(|| {
                            "MIR map loop value lacks a runtime type identity".to_string()
                        })?,
                    )),
                    _ => None,
                };
                let collection = self.value(*collection)?;
                let (step, has_step) = match step {
                    Some(step) => (
                        self.index_operand(
                            builder,
                            *step,
                            jet_foundation::MIR::MirIndexKind::List,
                        )?,
                        builder.ins().iconst(types::I8, 1),
                    ),
                    None => (
                        builder.ins().iconst(types::I64, 0),
                        builder.ins().iconst(types::I8, 0),
                    ),
                };
                if let Some((key_type_id, value_type_id)) = map_types {
                    let has_step = builder.ins().uextend(types::I64, has_step);
                    let key_type_id = builder.ins().iconst(types::I64, key_type_id as i64);
                    let value_type_id = builder.ins().iconst(types::I64, value_type_id as i64);
                    let result = self.call_host(
                        builder,
                        self.host.coll.loop_map_init,
                        &[collection, step, has_step, key_type_id, value_type_id],
                    )?;
                    let cursor = result.first().copied().ok_or_else(|| {
                        "typed map loop initializer returned no cursor".to_string()
                    })?;
                    Some(self.cast(builder, cursor, expected.unwrap_or(types::I64))?)
                } else {
                    let by_value = builder.ins().iconst(types::I8, i64::from(*by_value));
                    let source_kind = builder.ins().iconst(
                        types::I64,
                        self.runtime.heap.alloc_string(source_kind.wire()),
                    );
                    Some(self.call_prelude(
                        builder,
                        *call,
                        vec![collection, step, has_step, by_value, source_kind],
                        expected,
                    )?)
                }
            }
            MirOperation::ScopeEnter { .. } | MirOperation::ScopeExit { .. } => None,
            MirOperation::Drop { value, kind } => {
                let value_ty = self.mir_value_type(*value)?;
                if matches!(kind, MirDropKind::ForeignHandle) {
                    if !self.program.handles.iter().any(|handle| {
                        handle.ty.same_checked_type(&value_ty)
                            || (handle.ty.nominal_name().is_some()
                                && handle.ty.nominal_name() == value_ty.nominal_name())
                    }) {
                        return Err("MIR foreign-handle drop has no lifecycle row".to_string());
                    }
                    let token = self.cast(builder, self.value(*value)?, types::I64)?;
                    let _ = self.call_host(builder, self.host.ffi.drop_handle, &[token])?;
                } else if value_ty.nominal_name() == Some("DbLease") {
                    let handle = self.cast(builder, self.value(*value)?, types::I64)?;
                    let _ = self.call_host(builder, self.host.db.pool_lease_close, &[handle])?;
                } else if matches!(
                    value_ty.nominal_name(),
                    Some("Arena" | "Bump" | "Pool" | "Fixed")
                ) {
                    let handle = self.cast(builder, self.value(*value)?, types::I64)?;
                    let _ = self.call_host(
                        builder,
                        self.host.memory.allocator_close,
                        &[handle],
                    )?;
                }
                instruction
                    .result
                    .map(|_| builder.ins().iconst(types::I64, 0))
            }
        };
        result
            .map(|value| expected.map_or(Ok(value), |ty| self.cast(builder, value, ty)))
            .transpose()
    }

    fn lower_terminator(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        block: &MirBasicBlock,
    ) -> Result<(), String> {
        match &block.terminator {
            MirTerminator::Jump { target } | MirTerminator::Continue { target } => {
                let target_block = self.block(*target)?;
                let args = self.edge_args(*target, block.id)?;
                builder.ins().jump(target_block, &args);
            }
            MirTerminator::Branch {
                condition,
                then_target,
                else_target,
            } => {
                let condition = self.bool_value(builder, self.value(*condition)?)?;
                let then_block = self.block(*then_target)?;
                let else_block = self.block(*else_target)?;
                let then_args = self.edge_args(*then_target, block.id)?;
                let else_args = self.edge_args(*else_target, block.id)?;
                builder
                    .ins()
                    .brif(condition, then_block, &then_args, else_block, &else_args);
            }
            MirTerminator::Switch {
                subject,
                arms,
                otherwise,
            } => {
                let _ = self.value(*subject)?;
                if arms.is_empty() {
                    let target = self.block(*otherwise)?;
                    let args = self.edge_args(*otherwise, block.id)?;
                    builder.ins().jump(target, &args);
                } else {
                    for (index, arm) in arms.iter().enumerate() {
                        let arm_condition = self.bool_value(builder, self.value(arm.condition)?)?;
                        let target = self.block(arm.target)?;
                        let target_args = self.edge_args(arm.target, block.id)?;
                        let next = if index + 1 == arms.len() {
                            self.block(*otherwise)?
                        } else {
                            builder.create_block()
                        };
                        let next_args = if index + 1 == arms.len() {
                            self.edge_args(*otherwise, block.id)?
                        } else {
                            Vec::new()
                        };
                        builder
                            .ins()
                            .brif(arm_condition, target, &target_args, next, &next_args);
                        if index + 1 != arms.len() {
                            builder.switch_to_block(next);
                        }
                    }
                }
            }
            MirTerminator::Return { value } => {
                if self.yield_sender.is_some() {
                    let value = value
                        .map(|id| self.value(id))
                        .transpose()?
                        .map(|value| self.cast(builder, value, types::I64))
                        .transpose()?
                        .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                    builder.ins().return_(&[value]);
                } else if matches!(
                    &self.function.failure,
                    MirFailureCarrier::Result { success, .. } if success.is_unit()
                ) {
                    // A block-bodied fallible Unit function has an implicit
                    // successful return.  Encode the same Ok(()) that AOT
                    // emits, rather than returning a bare Unit carrier.
                    let value = value
                        .map(|id| self.value(id))
                        .transpose()?
                        .map(|value| self.normalize_function_return(builder, value))
                        .transpose()?;
                    let value = match value {
                        Some(value) => value,
                        None => {
                            let unit = builder.ins().iconst(types::I64, 0);
                            self.result_value(builder, true, unit, Some(types::I64))?
                        }
                    };
                    builder.ins().return_(&[value]);
                } else if self.function.return_type.is_unit() {
                    // Infallible MIR Unit still uses the uniform i64 carrier;
                    // AOT spells the same successful value as Rust's `()`.
                    let value = value
                        .map(|id| self.value(id))
                        .transpose()?
                        .map(|value| self.normalize_function_return(builder, value))
                        .transpose()?
                        .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                    builder.ins().return_(&[value]);
                } else {
                    let values = value
                        .map(|id| self.value(id))
                        .transpose()?
                        .map(|value| self.normalize_function_return(builder, value))
                        .transpose()?
                        .into_iter()
                        .collect::<Vec<_>>();
                    builder.ins().return_(&values);
                }
            }
            MirTerminator::Yield { value, resume } => {
                let sender = self
                    .yield_sender
                    .ok_or_else(|| "MIR yield occurs outside a generator body".to_string())?;
                let value = self.pack_generator_value(builder, self.value(*value)?)?;
                let status = self
                    .call_host(builder, self.host.conc.sender_send, &[sender, value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR generator sender returned no wait status".to_string())?;
                let zero = builder.ins().iconst(types::I64, 0);
                let interrupted = builder.ins().icmp(IntCC::NotEqual, status, zero);
                let cancelled = builder.create_block();
                let ready = builder.create_block();
                builder.ins().brif(interrupted, cancelled, &[], ready, &[]);
                builder.switch_to_block(cancelled);
                builder.ins().return_(&[zero]);
                builder.switch_to_block(ready);
                let target_block = self.block(*resume)?;
                let args = self.edge_args(*resume, block.id)?;
                builder.ins().jump(target_block, &args);
            }
            MirTerminator::Break { target, .. } => {
                let target_block = self.block(*target)?;
                let args = self.edge_args(*target, block.id)?;
                builder.ins().jump(target_block, &args);
            }
            MirTerminator::Unreachable { .. } => {
                builder.ins().trap(ir::TrapCode::UnreachableCodeReached);
            }
        }
        Ok(())
    }

    fn value(&self, id: MirValueId) -> Result<Value, String> {
        self.values
            .get(&id)
            .copied()
            .ok_or_else(|| format!("MIR value {:?} has no lowered definition", id))
    }

    fn value_type(&self, builder: &FunctionBuilder<'_>, value: Value) -> types::Type {
        builder.func.dfg.value_type(value)
    }
    fn default_int_type(ty: &MirType) -> bool {
        match ty.kind() {
            MirTypeKind::Int => true,
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. }
            | MirTypeKind::Quantity { base, .. } => Self::default_int_type(base),
            _ => false,
        }
    }

    fn mir_value_type(&self, id: MirValueId) -> Result<MirType, String> {
        self.function
            .values
            .iter()
            .find(|(value, _, _, _)| *value == id)
            .map(|(_, ty, _, _)| ty.clone())
            .ok_or_else(|| format!("MIR value {:?} has no type row", id))
    }

    fn enum_variant(
        &self,
        owner: jet_foundation::MIR::MirTypeId,
        name: &str,
    ) -> Result<(i64, jet_foundation::MIR::MirVariantPayload), String> {
        let definition = self
            .program
            .types
            .iter()
            .find(|definition| definition.id == owner)
            .ok_or_else(|| format!("MIR enum type {:?} is missing", owner))?;
        let jet_foundation::MIR::MirTypeDefKind::Enum { variants, .. } = &definition.kind else {
            return Err(format!("MIR type {:?} is not an enum", owner));
        };
        let (index, variant) = variants
            .iter()
            .enumerate()
            .find(|(_, variant)| variant.name == name)
            .ok_or_else(|| format!("MIR enum variant `{name}` is missing"))?;
        let discriminant = if is_ordering_name(&definition.key) {
            prelude_enum_variant_index(jet_foundation::Syntax::TYPE_ORDERING, name)
                .ok_or_else(|| format!("Prelude Ordering variant `{name}` is missing metadata"))?
        } else {
            variant.discriminant.unwrap_or(index as i64)
        };
        Ok((discriminant, variant.payload.clone()))
    }
    fn is_ordering_enum(&self, owner: jet_foundation::MIR::MirTypeId) -> bool {
        self.program
            .types
            .iter()
            .find(|definition| definition.id == owner)
            .is_some_and(|definition| is_ordering_name(&definition.key))
    }
    fn enum_discriminant_value(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        subject: Value,
        ordering: bool,
    ) -> Result<Value, String> {
        let subject = self.cast(builder, subject, types::I64)?;
        if ordering {
            return Ok(subject);
        }
        let field = builder.ins().iconst(types::I64, 0);
        self.call_host(builder, self.host.struct_get_i64, &[subject, field])?
            .first()
            .copied()
            .ok_or_else(|| "MIR enum discriminator getter returned no value".to_string())
    }

    fn enum_is(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        subject: MirValueId,
        owner: jet_foundation::MIR::MirTypeId,
        variant: &str,
    ) -> Result<Value, String> {
        let (discriminant, _) = self.enum_variant(owner, variant)?;
        let subject = self.value(subject)?;
        let ordering = self.is_ordering_enum(owner);
        let actual = self.enum_discriminant_value(builder, subject, ordering)?;
        let expected = builder.ins().iconst(types::I64, discriminant);
        let equal = builder.ins().icmp(IntCC::Equal, actual, expected);
        Ok(equal)
    }

    fn enum_payload(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        subject: MirValueId,
        owner: jet_foundation::MIR::MirTypeId,
        variant: &str,
        index: usize,
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let (_, payload) = self.enum_variant(owner, variant)?;
        let fields = match payload {
            jet_foundation::MIR::MirVariantPayload::Unit => Vec::new(),
            jet_foundation::MIR::MirVariantPayload::Single(ty) => vec![ty],
            jet_foundation::MIR::MirVariantPayload::Named(fields) => {
                fields.into_iter().map(|field| field.ty).collect()
            }
        };
        let ty = fields
            .get(index)
            .ok_or_else(|| format!("MIR enum variant `{variant}` has no payload {index}"))?
            .clone();
        let subject = self.cast(builder, self.value(subject)?, types::I64)?;
        let field_index = builder.ins().iconst(types::I64, (index + 1) as i64);
        let getter = self.field_getter(&ty)?;
        let value = self
            .call_host(builder, getter, &[subject, field_index])?
            .first()
            .copied()
            .ok_or_else(|| {
                format!("MIR enum payload `{variant}[{index}]` getter returned no value")
            })?;
        expected.map_or(Ok(value), |target| self.cast(builder, value, target))
    }

    fn packed_optional_subject(&self, subject: MirValueId) -> bool {
        self.function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| {
                if instruction.result != Some(subject) {
                    return false;
                }
                match &instruction.operation {
                    MirOperation::CoreCall {
                        fallibility:
                            jet_foundation::MIR::MirCallFallibility::Failure(
                                jet_foundation::MIR::MirFailureCarrier::Optional { .. },
                            ),
                        ..
                    } => true,
                    MirOperation::Semantic(MirSemanticOp::HandleMethod { call, .. }) => self
                        .program
                        .prelude_calls
                        .iter()
                        .find(|row| row.id == *call)
                        .is_some_and(|row| {
                            (row.module != "core.time"
                                || !matches!(
                                    row.member.as_str(),
                                    "Zone.next_transition"
                                        | "Zone.previous_transition"
                                        | "ZonedDateTime.next_transition"
                                        | "ZonedDateTime.previous_transition"
                                ))
                                && matches!(
                                    &row.fallibility,
                                    jet_foundation::MIR::MirCallFallibility::Failure(
                                        jet_foundation::MIR::MirFailureCarrier::Optional { .. }
                                    )
                                )
                        }),
                    MirOperation::Semantic(MirSemanticOp::HostCall { call, .. }) => self
                        .program
                        .prelude_calls
                        .iter()
                        .find(|row| row.id == *call)
                        .is_some_and(|row| {
                            row.module == "core.host"
                                && matches!(
                                    &row.fallibility,
                                    jet_foundation::MIR::MirCallFallibility::Failure(
                                        jet_foundation::MIR::MirFailureCarrier::Optional { .. }
                                    )
                                )
                        }),
                    MirOperation::Semantic(MirSemanticOp::BuiltinMethod { call, .. }) => self
                        .program
                        .prelude_calls
                        .iter()
                        .find(|row| row.id == *call)
                        .is_some_and(|row| {
                            row.family == jet_foundation::MIR::MirPreludeFamily::BuiltinMethod
                                && row.module == "core.builtin"
                                && matches!(
                                    row.member.as_str(),
                                    "list_pop" | "list_remove_value" | "list_remove_slot"
                                )
                                && matches!(
                                    &row.fallibility,
                                    jet_foundation::MIR::MirCallFallibility::Failure(
                                        jet_foundation::MIR::MirFailureCarrier::Optional { .. }
                                    )
                                )
                        }),
                    MirOperation::Copy { value } => self.packed_optional_subject(*value),
                    _ => false,
                }
            })
    }

    fn result_is_ok(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        subject: MirValueId,
    ) -> Result<Value, String> {
        let subject = self.cast(builder, self.value(subject)?, types::I64)?;
        self.call_host(builder, self.host.result_is_ok, &[subject])?
            .first()
            .copied()
            .ok_or_else(|| "MIR result discriminator getter returned no value".to_string())
    }

    fn result_value_get(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        subject: MirValueId,
        _ok: bool,
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let subject = self.cast(builder, self.value(subject)?, types::I64)?;
        self.result_value_get_raw(builder, subject, expected)
    }

    fn result_value_get_raw(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        subject: Value,
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let getter = match expected {
            Some(ty) if ty == types::F64 || ty == types::F32 => self.host.result_get_f64,
            Some(ty) if ty == types::I8 => self.host.result_get_i8,
            Some(ty) if ty == types::I32 => self.host.result_get_i32,
            Some(ty) if ty == types::I64 => self.host.result_get_i64,
            Some(ty) => return Err(format!("MIR result payload has unsupported carrier {ty}")),
            None => return Err("MIR result payload has no expected carrier".to_string()),
        };
        let value = self
            .call_host(builder, getter, &[subject])?
            .first()
            .copied()
            .ok_or_else(|| "MIR result payload getter returned no value".to_string())?;
        expected.map_or(Ok(value), |ty| self.cast(builder, value, ty))
    }

    fn pattern_capture_type(&self, matched: MirValueId, index: usize) -> Result<MirType, String> {
        let producer = self
            .function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find(|instruction| instruction.result == Some(matched))
            .ok_or_else(|| format!("MIR pattern match value {:?} has no producer", matched))?;
        let type_id =
            if let MirOperation::Semantic(MirSemanticOp::TextPatternMatch { parts, .. }) =
                &producer.operation
            {
                parts
                    .iter()
                    .filter_map(|part| match part {
                        jet_foundation::MIR::MirTextPatternPart::Literal(_) => None,
                        jet_foundation::MIR::MirTextPatternPart::Hole { ty, .. } => Some(*ty),
                    })
                    .nth(index)
            } else if let MirOperation::Semantic(MirSemanticOp::BinaryPatternMatch {
                parts, ..
            }) = &producer.operation
            {
                parts
                    .iter()
                    .filter_map(|part| match part {
                        jet_foundation::MIR::MirBinaryPatternPart::Literal(_) => None,
                        jet_foundation::MIR::MirBinaryPatternPart::Bits { ty, .. }
                        | jet_foundation::MIR::MirBinaryPatternPart::Rest { ty, .. } => Some(*ty),
                    })
                    .nth(index)
            } else {
                None
            }
            .ok_or_else(|| format!("MIR pattern capture index {index} has no static type"))?;
        self.program
            .type_instances
            .iter()
            .find(|ty| ty.identity == Some(type_id))
            .cloned()
            .ok_or_else(|| format!("MIR pattern capture type {:?} has no instance row", type_id))
    }

    fn pattern_capture_kind(ty: &MirType) -> Result<PatternCaptureKind, String> {
        match ty.kind() {
            MirTypeKind::String => Ok(PatternCaptureKind::String),
            MirTypeKind::Char => Ok(PatternCaptureKind::Char),
            MirTypeKind::Int
            | MirTypeKind::IntN { .. }
            | MirTypeKind::InlineRange { .. }
            | MirTypeKind::Measure(_) => Ok(PatternCaptureKind::Int),
            MirTypeKind::Float | MirTypeKind::Float32 => Ok(PatternCaptureKind::Float),
            MirTypeKind::Bool => Ok(PatternCaptureKind::Bool),
            MirTypeKind::List(inner) if inner.fixed_int() == Some((false, 8)) => {
                Ok(PatternCaptureKind::Bytes)
            }
            MirTypeKind::List(_)
            | MirTypeKind::Map { .. }
            | MirTypeKind::Shared(_)
            | MirTypeKind::Option(_)
            | MirTypeKind::Result { .. }
            | MirTypeKind::Fn(_)
            | MirTypeKind::SendFn { .. }
            | MirTypeKind::Apply { .. }
            | MirTypeKind::TraitObject(_)
            | MirTypeKind::Tuple(_)
            | MirTypeKind::FixedList { .. }
            | MirTypeKind::Union(_) => Err(format!(
                "MIR pattern capture type `{}` is not sema-admitted",
                ty.display_name()
            )),
            MirTypeKind::Tagged { inner, .. } | MirTypeKind::Quantity { base: inner, .. } => {
                Self::pattern_capture_kind(inner)
            }
        }
    }

    fn pattern_capture(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        matched: MirValueId,
        index: usize,
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let capture_ty = self.pattern_capture_type(matched, index)?;
        let matched = self.cast(builder, self.value(matched)?, types::I64)?;
        let captures = self
            .call_host(builder, self.host.result_get_i64, &[matched])?
            .first()
            .copied()
            .ok_or_else(|| "MIR pattern result payload getter returned no value".to_string())?;
        let index = builder.ins().iconst(types::I64, index as i64);
        let line = builder.ins().iconst(types::I32, 0);
        let record = self
            .call_host(builder, self.host.coll.list_get, &[captures, index, line])?
            .first()
            .copied()
            .ok_or_else(|| "MIR pattern capture list getter returned no value".to_string())?;
        let field = builder.ins().iconst(types::I64, 1);
        let capture_kind = Self::pattern_capture_kind(&capture_ty)?;
        let value = match capture_kind {
            PatternCaptureKind::String => self
                .call_host(builder, self.host.struct_get_str, &[record, field])?
                .first()
                .copied()
                .ok_or_else(|| "MIR text capture String getter returned no value".to_string())?,
            PatternCaptureKind::Char => self
                .call_host(builder, self.host.pattern_capture_char, &[record, field])?
                .first()
                .copied()
                .ok_or_else(|| "MIR text capture Char getter returned no value".to_string())?,
            PatternCaptureKind::Int | PatternCaptureKind::Bytes => self
                .call_host(builder, self.host.struct_get_i64, &[record, field])?
                .first()
                .copied()
                .ok_or_else(|| "MIR integer capture getter returned no value".to_string())?,
            PatternCaptureKind::Float => self
                .call_host(builder, self.host.struct_get_f64, &[record, field])?
                .first()
                .copied()
                .ok_or_else(|| "MIR float capture getter returned no value".to_string())?,
            PatternCaptureKind::Bool => self
                .call_host(builder, self.host.struct_get_bool, &[record, field])?
                .first()
                .copied()
                .ok_or_else(|| "MIR bool capture getter returned no value".to_string())?,
        };
        expected.map_or(Ok(value), |target| self.cast(builder, value, target))
    }

    fn instruction_location(&self, instruction: &MirInstruction) -> Result<MirPanicLoc, String> {
        let module = self
            .program
            .modules
            .iter()
            .find(|module| module.id == self.function.module_id)
            .ok_or_else(|| {
                format!(
                    "MIR function module {:?} is missing",
                    self.function.module_id
                )
            })?;
        let source = self
            .program
            .source_files
            .iter()
            .find(|source| source.id == module.source_file)
            .ok_or_else(|| format!("MIR module source file {:?} is missing", module.source_file))?;
        let line = instruction
            .source_line
            .map(|line| line as usize)
            .unwrap_or_else(|| {
                jet_foundation::Diagnostics::span_line_col(&source.source, instruction.span.start).0
            });
        let line = u32::try_from(line)
            .map_err(|_| format!("MIR instruction source line {line} exceeds u32"))?;
        Ok(MirPanicLoc {
            file: module.source_file,
            line,
            column: 0,
        })
    }

    fn panic_location(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        location: &jet_foundation::MIR::MirPanicLoc,
    ) -> Result<Vec<Value>, String> {
        let path = self
            .program
            .source_files
            .iter()
            .find(|source| source.id == location.file)
            .map(|source| source.path.clone())
            .ok_or_else(|| {
                format!(
                    "MIR panic location references missing source file {:?}",
                    location.file
                )
            })?;
        let file = builder
            .ins()
            .iconst(types::I64, self.runtime.heap.alloc_string(path));
        let line = builder.ins().iconst(types::I32, i64::from(location.line));
        Ok(vec![file, line])
    }
    fn panic_context_locals(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        context: &MirPanicContext,
    ) -> Result<Value, String> {
        let mut rendered = builder
            .ins()
            .iconst(types::I64, self.runtime.heap.alloc_string(String::new()));
        for (index, (name, local_id)) in context.locals.iter().enumerate() {
            let local = self
                .function
                .locals
                .iter()
                .find(|candidate| candidate.id == *local_id)
                .ok_or_else(|| format!("MIR panic context local {:?} is missing", local_id))?;
            let place = local.place;
            let ty = local.ty.clone();
            let value = self.read_place(builder, place)?;
            let (debug_host, carrier) = match ty.layout.abi {
                MirAbi::Scalar(MirScalarKind::Int | MirScalarKind::Pointer) => {
                    (self.host.debug_i64, types::I64)
                }
                MirAbi::Scalar(MirScalarKind::Float) => (self.host.debug_f64, types::F64),
                MirAbi::Scalar(MirScalarKind::Float32) => (self.host.debug_f32, types::F32),
                MirAbi::Scalar(MirScalarKind::Bool) => (self.host.debug_bool, types::I8),
                MirAbi::Scalar(MirScalarKind::Char) => (self.host.debug_char, types::I32),
                abi => {
                    return Err(format!(
                        "MIR panic context local `{name}` has unsupported debug ABI {abi:?}"
                    ))
                }
            };
            let value = self.cast(builder, value, carrier)?;
            let debug = self
                .call_host(builder, debug_host, &[value])?
                .first()
                .copied()
                .ok_or_else(|| "MIR panic local debug host returned no value".to_string())?;
            let name = builder
                .ins()
                .iconst(types::I64, self.runtime.heap.alloc_string(name.clone()));
            let first = builder.ins().iconst(types::I8, i64::from(index == 0));
            rendered = self
                .call_host(
                    builder,
                    self.host.debug_local_append,
                    &[rendered, name, debug, first],
                )?
                .first()
                .copied()
                .ok_or_else(|| "MIR panic local formatter returned no value".to_string())?;
        }
        Ok(rendered)
    }

    fn panic_context_values(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        location: &MirPanicLoc,
        context: &MirPanicContext,
    ) -> Result<[Value; 7], String> {
        let file = self
            .program
            .source_files
            .iter()
            .find(|source| source.id == location.file)
            .ok_or_else(|| {
                format!(
                    "MIR panic location references missing source file {:?}",
                    location.file
                )
            })?
            .path
            .clone();
        let file = builder
            .ins()
            .iconst(types::I64, self.runtime.heap.alloc_string(file));
        let line = builder.ins().iconst(types::I64, i64::from(location.line));
        let function = builder.ins().iconst(
            types::I64,
            self.runtime.heap.alloc_string(context.function.clone()),
        );
        let source_line = builder.ins().iconst(
            types::I64,
            self.runtime.heap.alloc_string(context.source_line.clone()),
        );
        let column = builder.ins().iconst(types::I64, i64::from(location.column));
        let caret = builder.ins().iconst(types::I64, i64::from(context.caret));
        let locals = self.panic_context_locals(builder, context)?;
        Ok([file, line, function, source_line, column, caret, locals])
    }
    fn nominal_render_type_id(&self, ty: &MirType, debug: bool) -> Option<MirTypeId> {
        let identity = ty.nominal_id()?;
        let definition = self
            .program
            .types
            .iter()
            .find(|definition| definition.id == identity)?;
        if !matches!(
            &definition.kind,
            MirTypeDefKind::Enum { .. } | MirTypeDefKind::Struct { .. }
        ) {
            return None;
        }
        if debug {
            let has_debug = definition.derives.iter().any(|trait_id| {
                self.program.traits.iter().any(|trait_def| {
                    trait_def.id == *trait_id && trait_def.name == jet_foundation::Generics::DEBUG
                })
            });
            if !has_debug {
                return None;
            }
        }
        Some(identity)
    }

    fn render_nominal_list(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        list: Value,
        inner: &MirType,
        debug: bool,
    ) -> Result<Value, String> {
        let type_id = self.nominal_render_type_id(inner, debug).ok_or_else(|| {
            format!(
                "MIR {} list element type `{}` has no checked carrier",
                if debug { "debug" } else { "display" },
                inner.display_name()
            )
        })?;
        let list = self.cast(builder, list, types::I64)?;
        let buffer = self
            .call_host(builder, self.host.str_begin, &[])?
            .first()
            .copied()
            .ok_or_else(|| "MIR string host returned no buffer".to_string())?;
        self.display_push_literal(builder, buffer, "[")?;
        let header = builder.create_block();
        let body = builder.create_block();
        let done = builder.create_block();
        builder.append_block_param(header, types::I64);
        builder.append_block_param(header, types::I64);
        builder.append_block_param(body, types::I64);
        builder.append_block_param(body, types::I64);
        builder.append_block_param(done, types::I64);
        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().jump(header, &[buffer, zero]);
        builder.switch_to_block(header);
        let header_params = builder.block_params(header).to_vec();
        let current_buffer = header_params[0];
        let index = header_params[1];
        let len = self
            .call_host(builder, self.host.coll.list_len, &[list])?
            .first()
            .copied()
            .ok_or_else(|| "MIR list length host returned no value".to_string())?;
        let has_next = builder.ins().icmp(IntCC::UnsignedLessThan, index, len);
        let body_args = [current_buffer, index];
        let done_args = [current_buffer];
        builder
            .ins()
            .brif(has_next, body, &body_args, done, &done_args);
        builder.switch_to_block(body);
        let body_params = builder.block_params(body).to_vec();
        let current_buffer = body_params[0];
        let index = body_params[1];
        let line = builder.ins().iconst(types::I32, 0);
        let element = self
            .call_host(builder, self.host.coll.list_get, &[list, index, line])?
            .first()
            .copied()
            .ok_or_else(|| "MIR list element host returned no value".to_string())?;
        let type_id_value = builder.ins().iconst(types::I64, type_id.0 as i64);
        let rendered = if debug {
            self.call_host(builder, self.host.debug_nominal, &[element, type_id_value])?
                .first()
                .copied()
                .ok_or_else(|| "MIR nominal debug host returned no value".to_string())?
        } else {
            self.call_host(builder, self.host.display_nominal, &[element, type_id_value])?
                .first()
                .copied()
                .ok_or_else(|| "MIR nominal display host returned no value".to_string())?
        };
        let first = builder.ins().icmp_imm(IntCC::Equal, index, 0);
        let comma = builder.create_block();
        let append = builder.create_block();
        builder.ins().brif(first, append, &[], comma, &[]);
        builder.switch_to_block(comma);
        self.display_push_literal(builder, current_buffer, ", ")?;
        builder.ins().jump(append, &[]);
        builder.switch_to_block(append);
        let append_buffer = current_buffer;
        let _ = self
            .call_host(builder, self.host.str_push_str, &[append_buffer, rendered])?;
        let next = builder.ins().iadd_imm(index, 1);
        builder.ins().jump(header, &[append_buffer, next]);
        builder.switch_to_block(done);
        let buffer = builder
            .block_params(done)
            .first()
            .copied()
            .ok_or_else(|| "MIR list render merge has no buffer".to_string())?;
        self.display_push_literal(builder, buffer, "]")?;
        Ok(buffer)
    }
    fn render_display_list(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        list: Value,
        inner: &MirType,
    ) -> Result<Value, String> {
        let element_type = clif_ty_from_mir(inner).ok_or_else(|| {
            format!(
                "MIR display list element type `{}` has no checked carrier",
                inner.display_name()
            )
        })?;
        let list = self.cast(builder, list, types::I64)?;
        let buffer = self
            .call_host(builder, self.host.str_begin, &[])?
            .first()
            .copied()
            .ok_or_else(|| "MIR string host returned no buffer".to_string())?;
        self.display_push_literal(builder, buffer, "[")?;
        let header = builder.create_block();
        let body = builder.create_block();
        let done = builder.create_block();
        builder.append_block_param(header, types::I64);
        builder.append_block_param(header, types::I64);
        builder.append_block_param(body, types::I64);
        builder.append_block_param(body, types::I64);
        builder.append_block_param(done, types::I64);
        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().jump(header, &[buffer, zero]);
        builder.switch_to_block(header);
        let header_params = builder.block_params(header).to_vec();
        let current_buffer = header_params[0];
        let index = header_params[1];
        let len = self
            .call_host(builder, self.host.coll.list_len, &[list])?
            .first()
            .copied()
            .ok_or_else(|| "MIR list length host returned no value".to_string())?;
        let has_next = builder.ins().icmp(IntCC::UnsignedLessThan, index, len);
        builder.ins().brif(
            has_next,
            body,
            &[current_buffer, index],
            done,
            &[current_buffer],
        );
        builder.switch_to_block(body);
        let body_params = builder.block_params(body).to_vec();
        let current_buffer = body_params[0];
        let index = body_params[1];
        let line = builder.ins().iconst(types::I32, 0);
        let element = self
            .call_host(builder, self.host.coll.list_get, &[list, index, line])?
            .first()
            .copied()
            .ok_or_else(|| "MIR list element host returned no value".to_string())?;
        let element = self.cast(builder, element, element_type)?;
        let rendered = self.display_value_of_type(builder, inner, element, false)?;
        let first = builder.ins().icmp_imm(IntCC::Equal, index, 0);
        let comma = builder.create_block();
        let append = builder.create_block();
        builder.ins().brif(first, append, &[], comma, &[]);
        builder.switch_to_block(comma);
        self.display_push_literal(builder, current_buffer, ", ")?;
        builder.ins().jump(append, &[]);
        builder.switch_to_block(append);
        let append_buffer = current_buffer;
        let _ = self
            .call_host(builder, self.host.str_push_str, &[append_buffer, rendered])?;
        let next = builder.ins().iadd_imm(index, 1);
        builder.ins().jump(header, &[append_buffer, next]);
        builder.switch_to_block(done);
        let buffer = builder
            .block_params(done)
            .first()
            .copied()
            .ok_or_else(|| "MIR list render merge has no buffer".to_string())?;
        self.display_push_literal(builder, buffer, "]")?;
        Ok(buffer)
    }

    fn debug_value(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        id: MirValueId,
    ) -> Result<Value, String> {
        let ty = self.mir_value_type(id)?;
        let value = self.value(id)?;
        self.debug_value_of_type(builder, &ty, value)
    }

    fn debug_value_of_type(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        ty: &MirType,
        value: Value,
    ) -> Result<Value, String> {
        if matches!(
            ty.kind(),
            MirTypeKind::Apply { name, args }
                if args.is_empty() && name.name == "EnvError"
        ) {
            // `core.sys` host failures use `Marshal::result_err_msg`, so the
            // resident EnvError carrier is already the Prelude's canonical
            // display text as a String handle.
            return Ok(self.cast(builder, value, types::I64)?);
        }
        if matches!(
            ty.kind(),
            MirTypeKind::Apply { name, args }
                if args.is_empty() && matches!(name.name.as_str(), "TextError" | "RangeError")
        ) {
            // These Prelude errors already contain their complete rendered text.
            // Unpack the sole String field; do not format the resident record.
            let handle = self.cast(builder, value, types::I64)?;
            let index = builder.ins().iconst(types::I64, 0);
            return self
                .call_host(builder, self.host.struct_get_i64, &[handle, index])?
                .first()
                .copied()
                .ok_or_else(|| "MIR error text getter returned no value".to_string());
        }
        if let MirTypeKind::Apply { name, args } = ty.kind() {
            if args.is_empty() {
                let host = match name.name.as_str() {
                    "Duration" => Some(self.host.time.duration_display),
                    "Date"
                    | "LocalDate"
                    | "LocalTime"
                    | "DateTime"
                    | "Period"
                    | "Instant"
                    | "Zone"
                    | "ZonedDateTime" => Some(self.host.time.display),
                    "Path" => Some(self.host.core.path_to_string),
                    "Complex" => Some(self.host.num.complex_to_string),
                    "ServiceRuntime" => Some(self.host.service_show),
                    _ => None,
                };
                if let Some(host) = host {
                    let value = self.cast(builder, value, types::I64)?;
                    return self
                        .call_host(builder, host, &[value])?
                        .first()
                        .copied()
                        .ok_or_else(|| "MIR Apply debug host returned no value".to_string());
                }
            }
        }
        if let Some(identity) = ty.nominal_id() {
            if let Some(definition) = self
                .program
                .types
                .iter()
                .find(|definition| definition.id == identity)
            {
                match &definition.kind {
                    MirTypeDefKind::Distinct { base, .. }
                    | MirTypeDefKind::Alias { target: base } => {
                        return self.debug_value_of_type(builder, base, value);
                    }
                    MirTypeDefKind::Enum { .. } | MirTypeDefKind::Struct { .. } => {
                        let checked_debug = definition.derives.iter().any(|trait_id| {
                            self.program.traits.iter().any(|trait_def| {
                                trait_def.id == *trait_id
                                    && trait_def.name == jet_foundation::Generics::DEBUG
                            })
                        });
                        if !checked_debug {
                            return Err(format!(
                                "MIR debug type `{}` has no checked debug carrier",
                                ty.display_name()
                            ));
                        }
                        let handle = self.cast(builder, value, types::I64)?;
                        let type_id = builder.ins().iconst(types::I64, identity.0 as i64);
                        return self
                            .call_host(builder, self.host.debug_nominal, &[handle, type_id])?
                            .first()
                            .copied()
                            .ok_or_else(|| {
                                "MIR nominal debug host returned no value".to_string()
                            });
                    }
                    _ => {}
                }
            }
        }
        match ty.kind() {
            MirTypeKind::Int
            | MirTypeKind::IntN { .. }
            | MirTypeKind::InlineRange { .. }
            | MirTypeKind::Measure(_) => {
                let value = self.cast(builder, value, types::I64)?;
                self.call_host(builder, self.host.debug_i64, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR integer debug host returned no value".to_string())
            }
            MirTypeKind::Float => {
                let value = self.cast(builder, value, types::F64)?;
                self.call_host(builder, self.host.debug_f64, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR float debug host returned no value".to_string())
            }
            MirTypeKind::Float32 => {
                let value = self.cast(builder, value, types::F32)?;
                self.call_host(builder, self.host.debug_f32, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR F32 debug host returned no value".to_string())
            }
            MirTypeKind::Bool => {
                let value = self.cast(builder, value, types::I8)?;
                self.call_host(builder, self.host.debug_bool, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR Bool debug host returned no value".to_string())
            }
            MirTypeKind::Char => {
                let value = self.cast(builder, value, types::I32)?;
                self.call_host(builder, self.host.debug_char, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR Char debug host returned no value".to_string())
            }
            MirTypeKind::String => {
                let value = self.cast(builder, value, types::I64)?;
                self.call_host(builder, self.host.debug_string, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR String debug host returned no value".to_string())
            }
            MirTypeKind::List(inner)
                if self.nominal_render_type_id(inner, true).is_some() =>
            {
                let value = self.cast(builder, value, types::I64)?;
                return self.render_nominal_list(builder, value, inner, true);
            }
            MirTypeKind::List(inner) => {
                let kind = list_format_kind(inner).map_err(|_| {
                    format!(
                        "MIR debug list element type `{}` has no checked debug carrier",
                        inner.display_name()
                    )
                })?;
                let value = self.cast(builder, value, types::I64)?;
                let kind = builder.ins().iconst(types::I64, kind);
                self.call_host(builder, self.host.coll.list_debug, &[value, kind])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR list debug host returned no value".to_string())
            }
            MirTypeKind::Tagged { inner, .. } | MirTypeKind::Quantity { base: inner, .. } => {
                self.debug_value_of_type(builder, inner, value)
            }
            MirTypeKind::Map { .. }
            | MirTypeKind::Shared(_)
            | MirTypeKind::Option(_)
            | MirTypeKind::Result { .. }
            | MirTypeKind::Fn(_)
            | MirTypeKind::SendFn { .. }
            | MirTypeKind::Apply { .. }
            | MirTypeKind::TraitObject(_)
            | MirTypeKind::Tuple(_)
            | MirTypeKind::FixedList { .. }
            | MirTypeKind::Union(_) => Err(format!(
                "MIR debug type `{}` has no checked debug carrier",
                ty.display_name()
            )),
        }
    }

    /// The canonical Prelude treats only `JetAbsent` as a clean report.  It
    /// is the optional carrier's absence, so `?T` renders its payload bare and
    /// its absence as `null`; told results retain their `Ok(...)`/`Err(...)`
    /// verdict.
    fn display_report_is_clean(ty: &MirType) -> bool {
        ty.nominal_name().is_some_and(|name| {
            name == "JetAbsent" || name.ends_with("::JetAbsent") || name.ends_with(".JetAbsent")
        })
    }

    fn display_push_literal(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        buffer: Value,
        text: &str,
    ) -> Result<(), String> {
        let literal = builder
            .ins()
            .iconst(types::I64, self.runtime.heap.alloc_string(text.to_string()));
        let _ = self.call_host(builder, self.host.str_push_lit, &[buffer, literal])?;
        Ok(())
    }

    /// `core.text.fmt.display`: the one Display route every tier lowers for
    /// `"{value}"` and `print(value)`.  AOT's `jet_fmt_display<T: JetDisplay>`
    /// selects the render from the Rust type; the resident engine has no type
    /// at the host boundary, so it selects it from the MIR type here and
    /// renders through the same string hosts interpolation uses.  A carrier
    /// alone cannot decide: `Int`, `String`, and every handle share `i64`.
    fn display_value_of_type(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        ty: &MirType,
        value: Value,
        packed_optional: bool,
    ) -> Result<Value, String> {
        if is_allocator_view_type(ty) {
            let view = self.cast(builder, value, types::I64)?;
            let value = self
                .call_host(builder, self.host.memory.allocator_view_read, &[view])?
                .first()
                .copied()
                .ok_or_else(|| "MIR allocator view reader returned no value".to_string())?;
            let inner = match ty.kind() {
                MirTypeKind::Tagged { inner, .. } => inner.as_ref(),
                _ => unreachable!("allocator view predicate must match a tagged MIR type"),
            };
            return self.display_value_of_type(builder, inner, value, packed_optional);
        }
        if matches!(
            ty.kind(),
            MirTypeKind::Apply { name, args }
                if args.is_empty() && name.name == "EnvError"
        ) {
            // `core.sys` host failures use `Marshal::result_err_msg`, so the
            // resident EnvError carrier is already the Prelude's canonical
            // display text as a String handle.
            return Ok(self.cast(builder, value, types::I64)?);
        }
        if matches!(
            ty.kind(),
            MirTypeKind::Apply { name, args }
                if args.is_empty() && matches!(name.name.as_str(), "TextError" | "RangeError")
        ) {
            return self.debug_value_of_type(builder, ty, value);
        }
        if let Some(identity) = ty.nominal_id() {
            if let Some(definition) = self
                .program
                .types
                .iter()
                .find(|definition| definition.id == identity)
            {
                match &definition.kind {
                    MirTypeDefKind::Distinct { base, .. }
                    | MirTypeDefKind::Alias { target: base } => {
                        return self.display_value_of_type(builder, base, value, packed_optional);
                    }
                    MirTypeDefKind::Enum { .. } | MirTypeDefKind::Struct { .. } => {
                        let handle = self.cast(builder, value, types::I64)?;
                        let type_id = builder.ins().iconst(types::I64, identity.0 as i64);
                        return self
                            .call_host(builder, self.host.display_nominal, &[handle, type_id])?
                            .first()
                            .copied()
                            .ok_or_else(|| {
                                "MIR nominal display host returned no value".to_string()
                            });
                    }
                    _ => {}
                }
            }
        }
        if let MirTypeKind::Apply { name, args } = ty.kind() {
            if args.is_empty() && name.name == "IOError" {
                let value = self.cast(builder, value, types::I64)?;
                return self
                    .call_host(builder, self.host.coll.io_error_show, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR IOError display host returned no value".to_string());
            }
            if args.is_empty() && name.name == "Duration" {
                let value = self.cast(builder, value, types::I64)?;
                return self
                    .call_host(builder, self.host.time.duration_display, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR Duration display host returned no value".to_string());
            }
            if name.name == "Measurement"
                && args.len() == 1
                && matches!(args[0].kind(), MirTypeKind::Float)
            {
                let value = self.cast(builder, value, types::I64)?;
                return self
                    .call_host(builder, self.host.measurement_show, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR Measurement display host returned no value".to_string());
            }
            if args.is_empty()
                && matches!(
                    name.name.as_str(),
                    "Date"
                        | "LocalDate"
                        | "LocalTime"
                        | "DateTime"
                        | "Period"
                        | "Instant"
                        | "Zone"
                        | "ZonedDateTime"
                )
            {
                let value = self.cast(builder, value, types::I64)?;
                return self
                    .call_host(builder, self.host.time.display, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR time display host returned no value".to_string());
            }
            if args.is_empty() && name.name == "Path" {
                let value = self.cast(builder, value, types::I64)?;
                return self
                    .call_host(builder, self.host.core.path_to_string, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR Path display host returned no value".to_string());
            }
            if args.is_empty() && name.name == "Complex" {
                let value = self.cast(builder, value, types::I64)?;
                return self
                    .call_host(builder, self.host.num.complex_to_string, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR Complex display host returned no value".to_string());
            }
            if args.is_empty() && name.name == "ServiceRuntime" {
                let value = self.cast(builder, value, types::I64)?;
                return self
                    .call_host(builder, self.host.service_show, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| {
                        "MIR ServiceRuntime display host returned no value".to_string()
                    });
            }
        }
        let (host, value) = match ty.kind() {
            MirTypeKind::Option(inner) => {
                let raw = self.cast(builder, value, types::I64)?;
                let present = if packed_optional {
                    builder.ins().icmp_imm(IntCC::NotEqual, raw, 0)
                } else {
                    let ok = self
                        .call_host(builder, self.host.result_is_ok, &[raw])?
                        .first()
                        .copied()
                        .ok_or_else(|| {
                            "MIR result discriminator getter returned no value".to_string()
                        })?;
                    self.bool_value(builder, ok)?
                };
                let payload_type = clif_ty_from_mir(inner).ok_or_else(|| {
                    format!(
                        "MIR option payload `{}` has no checked carrier",
                        inner.display_name()
                    )
                })?;
                let buffer = self
                    .call_host(builder, self.host.str_begin, &[])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR string host returned no buffer".to_string())?;
                let present_block = builder.create_block();
                let absent_block = builder.create_block();
                let merge_block = builder.create_block();
                builder
                    .ins()
                    .brif(present, present_block, &[], absent_block, &[]);

                builder.switch_to_block(present_block);
                let payload = if packed_optional {
                    let one = builder.ins().iconst(types::I64, 1);
                    let payload = builder.ins().isub(raw, one);
                    self.cast(builder, payload, payload_type)?
                } else {
                    self.result_value_get_raw(builder, raw, Some(payload_type))?
                };
                let rendered = self.display_value_of_type(builder, inner, payload, false)?;
                let _ = self.call_host(builder, self.host.str_push_str, &[buffer, rendered])?;
                builder.ins().jump(merge_block, &[]);

                builder.switch_to_block(absent_block);
                self.display_push_literal(builder, buffer, "null")?;
                builder.ins().jump(merge_block, &[]);

                builder.switch_to_block(merge_block);
                return Ok(buffer);
            }
            MirTypeKind::Result { ok, err } => {
                let raw = self.cast(builder, value, types::I64)?;
                let ok_tag = self
                    .call_host(builder, self.host.result_is_ok, &[raw])?
                    .first()
                    .copied()
                    .ok_or_else(|| {
                        "MIR result discriminator getter returned no value".to_string()
                    })?;
                let ok_tag = self.bool_value(builder, ok_tag)?;
                let clean = Self::display_report_is_clean(err);
                let ok_type = clif_ty_from_mir(ok).ok_or_else(|| {
                    format!(
                        "MIR result payload `{}` has no checked carrier",
                        ok.display_name()
                    )
                })?;
                let buffer = self
                    .call_host(builder, self.host.str_begin, &[])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR string host returned no buffer".to_string())?;
                let success_block = builder.create_block();
                let failure_block = builder.create_block();
                let merge_block = builder.create_block();
                builder
                    .ins()
                    .brif(ok_tag, success_block, &[], failure_block, &[]);

                builder.switch_to_block(success_block);
                let payload = self.result_value_get_raw(builder, raw, Some(ok_type))?;
                let rendered = self.display_value_of_type(builder, ok, payload, false)?;
                if !clean {
                    self.display_push_literal(builder, buffer, "Ok(")?;
                }
                let _ = self.call_host(builder, self.host.str_push_str, &[buffer, rendered])?;
                if !clean {
                    self.display_push_literal(builder, buffer, ")")?;
                }
                builder.ins().jump(merge_block, &[]);

                builder.switch_to_block(failure_block);
                if clean {
                    self.display_push_literal(builder, buffer, "null")?;
                } else if err.is_never() {
                    let _ = self.trap(builder)?;
                } else {
                    let err_type = clif_ty_from_mir(err).ok_or_else(|| {
                        format!(
                            "MIR result error `{}` has no checked carrier",
                            err.display_name()
                        )
                    })?;
                    let payload = self.result_value_get_raw(builder, raw, Some(err_type))?;
                    let rendered = self.display_value_of_type(builder, err, payload, false)?;
                    self.display_push_literal(builder, buffer, "Err(")?;
                    let _ = self.call_host(builder, self.host.str_push_str, &[buffer, rendered])?;
                    self.display_push_literal(builder, buffer, ")")?;
                }
                builder.ins().jump(merge_block, &[]);

                builder.switch_to_block(merge_block);
                return Ok(buffer);
            }
            MirTypeKind::Int | MirTypeKind::InlineRange { .. } | MirTypeKind::Measure(_) => (
                self.host.str_push_i64,
                self.cast(builder, value, types::I64)?,
            ),
            MirTypeKind::IntN { signed, .. } => {
                let value = self.cast(builder, value, types::I64)?;
                let signed = builder.ins().iconst(types::I64, i64::from(*signed));
                return self
                    .call_host(builder, self.host.intn_to_string, &[value, signed])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR sized integer display host returned no value".to_string());
            }
            MirTypeKind::Float => (
                self.host.str_push_f64,
                self.cast(builder, value, types::F64)?,
            ),
            MirTypeKind::Float32 => {
                let value = self.cast(builder, value, types::F32)?;
                (
                    self.host.str_push_f64,
                    builder.ins().fpromote(types::F64, value),
                )
            }
            MirTypeKind::Bool => (
                self.host.str_push_bool,
                self.cast(builder, value, types::I8)?,
            ),
            MirTypeKind::Char => (
                self.host.str_push_char,
                self.cast(builder, value, types::I32)?,
            ),
            MirTypeKind::String => (
                self.host.str_push_str,
                self.cast(builder, value, types::I64)?,
            ),
            MirTypeKind::Tagged { inner, .. } | MirTypeKind::Quantity { base: inner, .. } => {
                return self.display_value_of_type(builder, inner, value, packed_optional);
            }
            // Precise numerics are opaque handles into the runtime's side
            // tables; their render is the same `to_string` the Prelude and
            // the interpreter use (`jet_decimal_to_string` / `jet_fraction_to_string`).
            MirTypeKind::Apply { name, args } if args.is_empty() && name.name == "Decimal" => {
                let value = self.cast(builder, value, types::I64)?;
                return self
                    .call_host(builder, self.host.num.decimal_to_string, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR Decimal display host returned no value".to_string());
            }
            MirTypeKind::Apply { name, args } if args.is_empty() && name.name == "Fraction" => {
                let value = self.cast(builder, value, types::I64)?;
                return self
                    .call_host(builder, self.host.num.fraction_to_string, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR Fraction display host returned no value".to_string());
            }
            MirTypeKind::List(inner)
                if matches!(inner.kind(), MirTypeKind::Apply { name, args }
                    if name.name == "FieldError" && args.is_empty()) =>
            {
                let value = self.cast(builder, value, types::I64)?;
                return self
                    .call_host(builder, self.host.encoding.decode_error_show, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| {
                        "MIR FieldError list display host returned no value".to_string()
                    });
            }
            MirTypeKind::List(inner)
                if self.nominal_render_type_id(inner, false).is_some() =>
            {
                let value = self.cast(builder, value, types::I64)?;
                return self.render_nominal_list(builder, value, inner, false);
            }
            MirTypeKind::List(inner)
                if matches!(inner.kind(), MirTypeKind::Apply { name, args }
                    if args.is_empty() && matches!(name.name.as_str(), "DateTime" | "Duration")) =>
            {
                let value = self.cast(builder, value, types::I64)?;
                return self.render_display_list(builder, value, inner);
            }
            MirTypeKind::List(inner) => {
                let kind = list_format_kind(inner).map_err(|_| {
                    format!(
                        "MIR display list element type `{}` has no checked display carrier",
                        inner.display_name()
                    )
                })?;
                let value = self.cast(builder, value, types::I64)?;
                let kind = builder.ins().iconst(types::I64, kind);
                return self
                    .call_host(builder, self.host.coll.list_display, &[value, kind])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR list display host returned no value".to_string());
            }
            MirTypeKind::FixedList { elem, .. } => {
                let kind = list_format_kind(elem).map_err(|_| {
                    format!(
                        "MIR display fixed-list element type `{}` has no checked display carrier",
                        elem.display_name()
                    )
                })?;
                let value = self.cast(builder, value, types::I64)?;
                let kind = builder.ins().iconst(types::I64, kind);
                return self
                    .call_host(builder, self.host.coll.list_display, &[value, kind])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR fixed-list display host returned no value".to_string());
            }
            MirTypeKind::Map { .. }
            | MirTypeKind::Shared(_)
            | MirTypeKind::Fn(_)
            | MirTypeKind::SendFn { .. }
            | MirTypeKind::Apply { .. }
            | MirTypeKind::TraitObject(_)
            | MirTypeKind::Tuple(_)
            | MirTypeKind::Union(_) => {
                return Err(format!(
                    "MIR display type `{}` has no resident render",
                    ty.display_name()
                ))
            }
        };
        let buffer = self
            .call_host(builder, self.host.str_begin, &[])?
            .first()
            .copied()
            .ok_or_else(|| "MIR string host returned no buffer".to_string())?;
        let _ = self.call_host(builder, host, &[buffer, value])?;
        Ok(buffer)
    }

    fn observe_live_place(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        id: jet_foundation::MIR::MirPlaceId,
    ) -> Result<(), String> {
        let place = self
            .function
            .places
            .iter()
            .find(|place| place.id == id)
            .cloned()
            .ok_or_else(|| format!("MIR place {:?} is missing", id))?;
        let Some(key) = place.persist_key else {
            return Ok(());
        };
        let value = self.read_place(builder, id)?;
        let rendered = self
            .debug_value_of_type(builder, &place.ty, value)
            .unwrap_or_else(|_| builder.ins().iconst(types::I64, 0));
        let key = builder
            .ins()
            .iconst(types::I64, self.runtime.heap.alloc_string(key));
        let type_identity = builder.ins().iconst(
            types::I64,
            self.runtime.heap.alloc_string(place.ty.canonical_key()),
        );
        let _ = self.call_host(
            builder,
            self.host.observe_live_value_update,
            &[key, type_identity, rendered],
        )?;
        Ok(())
    }

    fn index_location(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        call: jet_foundation::MIR::MirPreludeCallId,
        location: &jet_foundation::MIR::MirPanicLoc,
        context: Option<&jet_foundation::MIR::MirPanicContext>,
        source_line: Option<&str>,
        span: &jet_foundation::Diagnostics::Span,
    ) -> Result<Vec<Value>, String> {
        let arity = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .ok_or_else(|| format!("MIR references missing Prelude call {:?}", call))?
            .signature
            .arity;
        if arity == 3 {
            return Ok(Vec::new());
        }
        let source = self
            .program
            .source_files
            .iter()
            .find(|source| source.id == location.file)
            .ok_or_else(|| {
                format!(
                    "MIR index location references missing source file {:?}",
                    location.file
                )
            })?;
        let file = builder.ins().iconst(
            types::I64,
            self.runtime.heap.alloc_string(source.path.clone()),
        );
        let line = builder.ins().iconst(types::I64, i64::from(location.line));
        let mut values = vec![file, line];
        match arity {
            4 | 5 => {}
            6 | 7 | 8 | 9 => {
                let (function_name, source_line) = if let Some(context) = context {
                    (context.function.clone(), context.source_line.clone())
                } else if arity == 6 || arity == 7 {
                    return Err(format!(
                        "MIR index Prelude route `{}` requires checked panic context",
                        source.path
                    ));
                } else {
                    (
                        self.function.name.clone(),
                        source_line
                            .ok_or_else(|| {
                                format!(
                                    "MIR index Prelude route `{}` requires a checked source line",
                                    source.path
                                )
                            })?
                            .to_owned(),
                    )
                };
                let function = builder
                    .ins()
                    .iconst(types::I64, self.runtime.heap.alloc_string(function_name));
                let source_line = builder
                    .ins()
                    .iconst(types::I64, self.runtime.heap.alloc_string(source_line));
                values.extend([function, source_line]);
                if arity == 8 || arity == 9 {
                    let column = builder.ins().iconst(types::I64, i64::from(location.column));
                    let caret_len = span
                        .end
                        .checked_sub(span.start)
                        .ok_or_else(|| "MIR index span has inverted bounds".to_string())?;
                    let caret_len = i64::try_from(caret_len)
                        .map_err(|_| "MIR index span is too large".to_string())?;
                    let caret_len = builder.ins().iconst(types::I64, caret_len);
                    values.extend([column, caret_len]);
                }
            }
            other => {
                return Err(format!(
                    "MIR index Prelude route has unsupported ABI arity {}",
                    other
                ));
            }
        }
        Ok(values)
    }

    fn overflow_location(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        call: jet_foundation::MIR::MirPreludeCallId,
        location: Option<&jet_foundation::MIR::MirPanicLoc>,
    ) -> Result<Vec<Value>, String> {
        let arity = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .ok_or_else(|| format!("MIR references missing Prelude call {:?}", call))?
            .signature
            .arity;
        match (arity, location) {
            (2, None) => Ok(Vec::new()),
            (2, Some(_)) => {
                Err("MIR overflow option route has a location for its two-argument ABI".to_string())
            }
            (4, Some(location)) => self.panic_location(builder, location),
            (4, None) => Err(
                "MIR overflow option route omits location for its four-argument ABI".to_string(),
            ),
            (other, _) => Err(format!(
                "MIR overflow option route has unsupported ABI arity {other}"
            )),
        }
    }

    fn map_key_kind_for_type(&self, ty: &MirType) -> Result<MapKeyKind, String> {
        match ty.kind() {
            MirTypeKind::String => Ok(MapKeyKind::String),
            MirTypeKind::Int | MirTypeKind::IntN { .. } => Ok(MapKeyKind::Int),
            MirTypeKind::Apply { name, args }
                if args.is_empty() && is_ordering_name(&name.name) =>
            {
                Ok(MapKeyKind::Int)
            }
            MirTypeKind::Tuple(_) | MirTypeKind::Apply { .. } | MirTypeKind::Union(_) => {
                Ok(MapKeyKind::Composite)
            }
            MirTypeKind::Tagged { inner, .. }
            | MirTypeKind::InlineRange { base: inner, .. }
            | MirTypeKind::Quantity { base: inner, .. } => self.map_key_kind_for_type(inner),
            MirTypeKind::Float
            | MirTypeKind::Bool
            | MirTypeKind::Char
            | MirTypeKind::List(_)
            | MirTypeKind::Map { .. }
            | MirTypeKind::Shared(_)
            | MirTypeKind::Option(_)
            | MirTypeKind::Result { .. }
            | MirTypeKind::Fn(_)
            | MirTypeKind::SendFn { .. }
            | MirTypeKind::TraitObject(_)
            | MirTypeKind::FixedList { .. }
            | MirTypeKind::Float32
            | MirTypeKind::Measure(_) => Err(format!(
                "MIR map key type `{}` has no registered JIT map carrier",
                ty.display_name()
            )),
        }
    }

    fn map_key_kind(&self, id: MirValueId) -> Result<MapKeyKind, String> {
        let ty = self.mir_value_type(id)?;
        self.map_key_kind_for_type(&ty)
    }
    /// Lower a collection index to the native carrier expected by JIT hosts.
    /// Checked `Int` values may be boxed, while list/lane/pool kernels consume
    /// a native i64 position. Maps intentionally keep their raw carrier: map
    /// hosts own exact-Int key semantics rather than positional indexing.
    fn index_operand(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        index: MirValueId,
        kind: jet_foundation::MIR::MirIndexKind,
    ) -> Result<Value, String> {
        let value = self.cast(builder, self.value(index)?, types::I64)?;
        if matches!(kind, jet_foundation::MIR::MirIndexKind::Map)
            || !is_exact_int_type(&self.mir_value_type(index)?)
        {
            return Ok(value);
        }
        self.call_host(builder, self.host.coll.index_to_i64, &[value])?
            .first()
            .copied()
            .ok_or_else(|| "MIR collection index conversion returned no value".to_string())
    }

    fn index_read(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        base: Value,
        base_type: Option<&MirType>,
        index: MirValueId,
        kind: jet_foundation::MIR::MirIndexKind,
        call: jet_foundation::MIR::MirPreludeCallId,
        location: &jet_foundation::MIR::MirPanicLoc,
        context: Option<&jet_foundation::MIR::MirPanicContext>,
        source_line: Option<&str>,
        span: &jet_foundation::Diagnostics::Span,
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let metadata = self.index_location(builder, call, location, context, source_line, span)?;
        let base = self.cast(builder, base, types::I64)?;
        let index_value = self.index_operand(builder, index, kind)?;
        let source_line = *metadata
            .get(1)
            .ok_or_else(|| "MIR index route omitted its source-line argument".to_string())?;
        let line = self.cast(builder, source_line, types::I32)?;
        let value = match kind {
            jet_foundation::MIR::MirIndexKind::List
            | jet_foundation::MIR::MirIndexKind::FixedListProof => {
                let float = base_type.and_then(sequence_element_type).is_some_and(|ty| {
                    matches!(ty.kind(), MirTypeKind::Float | MirTypeKind::Float32)
                });
                let host = match kind {
                    jet_foundation::MIR::MirIndexKind::List => {
                        if float {
                            self.host.coll.list_get_f64
                        } else {
                            self.host.coll.list_get
                        }
                    }
                    jet_foundation::MIR::MirIndexKind::FixedListProof => {
                        if float {
                            self.host.coll.fixed_list_get_f64
                        } else {
                            self.host.coll.fixed_list_get
                        }
                    }
                    jet_foundation::MIR::MirIndexKind::Map
                    | jet_foundation::MIR::MirIndexKind::Lane
                    | jet_foundation::MIR::MirIndexKind::Pool => {
                        return Err(format!("MIR index kind {kind:?} has no list getter"))
                    }
                };
                self.call_host(builder, host, &[base, index_value, line])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR list index getter returned no value".to_string())?
            }
            jet_foundation::MIR::MirIndexKind::Map => {
                let host = match self.map_key_kind(index)? {
                    MapKeyKind::String => self.host.coll.map_get,
                    MapKeyKind::Int => self.host.coll.map_get_int,
                    MapKeyKind::Composite => self.host.coll.map_get_composite,
                };
                self.call_host(builder, host, &[base, index_value, line])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR map index getter returned no value".to_string())?
            }
            jet_foundation::MIR::MirIndexKind::Pool => self
                .call_host(
                    builder,
                    self.host.memory.index_pool_get,
                    &[
                        base,
                        index_value,
                        metadata[0],
                        metadata[1],
                        metadata[2],
                        metadata[3],
                    ],
                )?
                .first()
                .copied()
                .ok_or_else(|| "MIR pool index getter returned no value".to_string())?,
            jet_foundation::MIR::MirIndexKind::Lane => {
                let route = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == call)
                    .cloned()
                    .ok_or_else(|| format!("MIR references missing Prelude call {:?}", call))?;
                let host = self.lookup_prelude_host(&route)?;
                self.call_host(
                    builder,
                    host,
                    &[base, index_value, metadata[0], source_line],
                )?
                .first()
                .copied()
                .ok_or_else(|| "MIR lane index getter returned no value".to_string())?
            }
        };
        expected.map_or(Ok(value), |target| self.cast(builder, value, target))
    }
    fn index_write(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        base: Value,
        index: MirValueId,
        kind: jet_foundation::MIR::MirIndexKind,
        write_call: Option<jet_foundation::MIR::MirPreludeCallId>,
        location: &jet_foundation::MIR::MirPanicLoc,
        context: Option<&jet_foundation::MIR::MirPanicContext>,
        source_line: Option<&str>,
        span: &jet_foundation::Diagnostics::Span,
        value_ty: &MirType,
        value: Value,
    ) -> Result<(), String> {
        let write_call = write_call.ok_or_else(|| {
            "MIR writable index place has no exact setter Prelude route".to_string()
        })?;
        let metadata =
            self.index_location(builder, write_call, location, context, source_line, span)?;
        let base = self.cast(builder, base, types::I64)?;
        let index_value = self.index_operand(builder, index, kind)?;
        match kind {
            jet_foundation::MIR::MirIndexKind::List
            | jet_foundation::MIR::MirIndexKind::FixedListProof => {
                let target = clif_ty_from_mir(value_ty)
                    .ok_or_else(|| "MIR list index value has no ABI".to_string())?;
                let (host, value) = if target == types::F64 || target == types::F32 {
                    (
                        self.host.coll.list_set_f64,
                        self.cast(builder, value, types::F64)?,
                    )
                } else {
                    (
                        self.host.coll.list_set,
                        self.cast(builder, value, types::I64)?,
                    )
                };
                let line = self.cast(
                    builder,
                    *metadata.last().ok_or_else(|| {
                        "MIR list setter has no source line metadata".to_string()
                    })?,
                    types::I32,
                )?;
                let _ = self.call_host(builder, host, &[base, index_value, value, line])?;
                Ok(())
            }
            jet_foundation::MIR::MirIndexKind::Map => {
                let value = self.cast(builder, value, types::I64)?;
                let host = match self.map_key_kind(index)? {
                    MapKeyKind::String => self.host.coll.index_map_set,
                    MapKeyKind::Int => self.host.coll.map_insert_int,
                    MapKeyKind::Composite => self.host.coll.map_insert_composite,
                };
                let mut args = vec![base, index_value, value];
                args.extend(metadata);
                let _ = self.call_host(builder, host, &args)?;
                Ok(())
            }
            jet_foundation::MIR::MirIndexKind::Pool => {
                let value = self.cast(builder, value, types::I64)?;
                let mut args = vec![base, index_value, value];
                args.extend(metadata);
                let _ = self.call_host(builder, self.host.memory.index_pool_set, &args)?;
                Ok(())
            }
            jet_foundation::MIR::MirIndexKind::Lane => {
                Err("MIR lane index has no checked Cranelift setter".to_string())
            }
        }
    }

    fn range(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        start: MirValueId,
        end: MirValueId,
        exclusive: bool,
        _expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let arity = builder.ins().iconst(types::I64, 3);
        let record = self
            .call_host(builder, self.host.struct_new, &[arity])?
            .first()
            .copied()
            .ok_or_else(|| "MIR range constructor returned no record".to_string())?;
        let start = self.cast(builder, self.value(start)?, types::I64)?;
        let end = self.cast(builder, self.value(end)?, types::I64)?;
        let start_index = builder.ins().iconst(types::I64, 0);
        let end_index = builder.ins().iconst(types::I64, 1);
        let exclusive_index = builder.ins().iconst(types::I64, 2);
        let exclusive = builder.ins().iconst(types::I8, i64::from(exclusive));
        let _ = self.call_host(
            builder,
            self.host.struct_set_i64,
            &[record, start_index, start],
        )?;
        let _ = self.call_host(builder, self.host.struct_set_i64, &[record, end_index, end])?;
        let _ = self.call_host(
            builder,
            self.host.struct_set_bool,
            &[record, exclusive_index, exclusive],
        )?;
        Ok(record)
    }

    fn block(&self, id: jet_foundation::MIR::MirBlockId) -> Result<ir::Block, String> {
        self.blocks
            .get(&id)
            .copied()
            .ok_or_else(|| format!("MIR block {:?} is missing", id))
    }
    fn edge_args(
        &self,
        target: jet_foundation::MIR::MirBlockId,
        source: jet_foundation::MIR::MirBlockId,
    ) -> Result<Vec<Value>, String> {
        let Some(block) = self.function.blocks.iter().find(|block| block.id == target) else {
            return Err(format!("MIR block {:?} is missing", target));
        };
        let mut args = Vec::new();
        for instruction in &block.instructions {
            let MirOperation::Phi { incoming } = &instruction.operation else {
                continue;
            };
            let value = incoming
                .iter()
                .find(|(predecessor, _)| *predecessor == source)
                .map(|(_, value)| *value)
                .ok_or_else(|| {
                    format!(
                        "MIR phi {:?} has no incoming edge from {:?}",
                        instruction.result, source
                    )
                })?;
            args.push(self.value(value)?);
        }
        Ok(args)
    }

    fn read_place(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        id: jet_foundation::MIR::MirPlaceId,
    ) -> Result<Value, String> {
        let place = self
            .function
            .places
            .iter()
            .find(|place| place.id == id)
            .cloned()
            .ok_or_else(|| format!("MIR place {:?} is missing", id))?;
        let mut current = self.place_base_value(builder, &place)?;
        let mut current_type = self.place_base_type(&place)?;
        let result_ty =
            clif_ty_from_mir(&place.ty).ok_or_else(|| format!("MIR place {:?} has no ABI", id))?;
        for (index, projection) in place.projections.iter().enumerate() {
            let final_projection = index + 1 == place.projections.len();
            current = match projection {
                jet_foundation::MIR::MirProjection::Field { field, .. } => {
                    let field_ty = self.field_info(*field)?.1;
                    let value = self.field_load_value(
                        builder,
                        current,
                        *field,
                        final_projection.then_some(result_ty),
                    )?;
                    current_type = field_ty;
                    value
                }
                jet_foundation::MIR::MirProjection::Index {
                    kind,
                    index,
                    call,
                    location,
                    context,
                    span,
                    ..
                } => {
                    let base_type = current_type.clone();
                    let element_type = checked_index_element_type(&base_type, *kind)?.clone();
                    let value = self.index_read(
                        builder,
                        current,
                        Some(&base_type),
                        *index,
                        *kind,
                        *call,
                        location,
                        context.as_ref(),
                        None,
                        span,
                        final_projection.then_some(result_ty),
                    )?;
                    current_type = element_type;
                    value
                }
                jet_foundation::MIR::MirProjection::Deref { .. } => {
                    let address = self.cast(builder, current, types::I64)?;
                    if is_allocator_view_type(&current_type) {
                        self.call_host(
                            builder,
                            self.host.memory.allocator_view_read,
                            &[address],
                        )?
                        .first()
                        .copied()
                        .ok_or_else(|| "MIR allocator view reader returned no value".to_string())?
                    } else {
                        let target = if final_projection {
                            result_ty
                        } else {
                            types::I64
                        };
                        builder.ins().load(target, MemFlags::new(), address, 0)
                    }
                }
            };
        }
        self.cast(builder, current, result_ty)
    }

    fn write_place(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        id: jet_foundation::MIR::MirPlaceId,
        value_id: MirValueId,
        value: Value,
    ) -> Result<(), String> {
        let place = self
            .function
            .places
            .iter()
            .find(|place| place.id == id)
            .cloned()
            .ok_or_else(|| format!("MIR place {:?} is missing", id))?;
        if place.projections.is_empty() {
            let base_type = self.place_base_type(&place)?;
            let value_type = self.mir_value_type(value_id)?;
            if is_allocator_view_type(&base_type) && !is_allocator_view_type(&value_type) {
                let current = self.place_base_value(builder, &place)?;
                let view = self.cast(builder, current, types::I64)?;
                let value = self.cast(builder, value, types::I64)?;
                let _ = self.call_host(
                    builder,
                    self.host.memory.allocator_view_write,
                    &[view, value],
                )?;
                return Ok(());
            }
            return self.write_place_base(builder, &place, value);
        }
        let persistent_root = matches!(&place.base, MirPlaceBase::Static(_))
            .then(|| self.persistent_root_place(&place))
            .transpose()?;
        let result_ty =
            clif_ty_from_mir(&place.ty).ok_or_else(|| format!("MIR place {:?} has no ABI", id))?;
        let mut current_type = self.place_base_type(&place)?;
        let mut current = self.place_base_value(builder, &place)?;
        let persistent_root_value = current;
        let last = place.projections.len() - 1;
        for (index, projection) in place.projections.iter().enumerate() {
            if index == last {
                match projection {
                    jet_foundation::MIR::MirProjection::Field { field, .. } => {
                        let (field_index, field_ty) = self.field_info(*field)?;
                        self.set_field(builder, current, field_index, &field_ty, value)?;
                    }
                    jet_foundation::MIR::MirProjection::Index {
                        kind,
                        index,
                        write_call,
                        location,
                        context,
                        span,
                        ..
                    } => {
                        self.index_write(
                            builder,
                            current,
                            *index,
                            *kind,
                            *write_call,
                            location,
                            context.as_ref(),
                            None,
                            span,
                            &place.ty,
                            value,
                        )?;
                    }
                    jet_foundation::MIR::MirProjection::Deref { .. } => {
                        let address = self.cast(builder, current, types::I64)?;
                        let value = self.cast(builder, value, types::I64)?;
                        if is_allocator_view_type(&current_type) {
                            let _ = self.call_host(
                                builder,
                                self.host.memory.allocator_view_write,
                                &[address, value],
                            )?;
                        } else {
                            let value = self.cast(builder, value, result_ty)?;
                            builder.ins().store(MemFlags::new(), value, address, 0);
                        }
                    }
                }
                if let Some(root) = persistent_root.as_ref() {
                    self.write_static_place(builder, root, persistent_root_value)?;
                }
                return Ok(());
            }
            current = match projection {
                jet_foundation::MIR::MirProjection::Field { field, .. } => {
                    let field_ty = self.field_info(*field)?.1;
                    let value = self.field_load_value(builder, current, *field, None)?;
                    current_type = field_ty;
                    value
                }
                jet_foundation::MIR::MirProjection::Index {
                    kind,
                    index,
                    call,
                    location,
                    context,
                    span,
                    ..
                } => {
                    let base_type = current_type.clone();
                    let element_type = checked_index_element_type(&base_type, *kind)?.clone();
                    let value = self.index_read(
                        builder,
                        current,
                        Some(&base_type),
                        *index,
                        *kind,
                        *call,
                        location,
                        context.as_ref(),
                        None,
                        span,
                        None,
                    )?;
                    current_type = element_type;
                    value
                }
                jet_foundation::MIR::MirProjection::Deref { .. } => {
                    let address = self.cast(builder, current, types::I64)?;
                    builder.ins().load(types::I64, MemFlags::new(), address, 0)
                }
            };
        }
        Err(format!("MIR place {:?} has no terminal projection", id))
    }

    fn persistent_key_handle(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        place: &MirPlace,
    ) -> Result<Value, String> {
        let key = place
            .persist_key
            .as_deref()
            .ok_or_else(|| format!("MIR static place {:?} has no persist key", place.id))?;
        Ok(builder
            .ins()
            .iconst(types::I64, self.runtime.heap.alloc_string(key)))
    }
    fn persistent_root_place(&self, place: &MirPlace) -> Result<MirPlace, String> {
        if place.projections.is_empty() {
            return Ok(place.clone());
        }
        let MirPlaceBase::Static(name) = &place.base else {
            return Err(format!(
                "MIR projected persistent place {:?} has a non-static base",
                place.id
            ));
        };
        self.function
            .places
            .iter()
            .find(|candidate| {
                candidate.projections.is_empty()
                    && matches!(
                        &candidate.base,
                        MirPlaceBase::Static(candidate_name) if candidate_name == name
                    )
                    && candidate.persist_key.as_ref() == place.persist_key.as_ref()
            })
            .cloned()
            .ok_or_else(|| {
                format!(
                    "MIR projected persistent place {:?} has no root place",
                    place.id
                )
            })
    }

    fn persistent_type_handle(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        place: &MirPlace,
    ) -> Result<Value, String> {
        Ok(builder.ins().iconst(
            types::I64,
            self.runtime.heap.alloc_string(place.ty.identity_key()),
        ))
    }

    fn read_static_place(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        place: &MirPlace,
    ) -> Result<Value, String> {
        if matches!(place.ty.layout.abi, MirAbi::Never) {
            return Err(format!(
                "MIR static place {:?} has no runtime value",
                place.id
            ));
        }
        let key = self.persistent_key_handle(builder, place)?;
        let type_handle = self.persistent_type_handle(builder, place)?;
        let raw = self
            .call_host(builder, self.host.persist_runtime_get, &[key, type_handle])?
            .first()
            .copied()
            .ok_or_else(|| "persistent slot read returned no value".to_string())?;
        let target = clif_ty_from_mir(&place.ty)
            .ok_or_else(|| format!("MIR static place {:?} has no ABI", place.id))?;
        if target == types::F64 {
            Ok(builder.ins().bitcast(types::F64, MemFlags::new(), raw))
        } else if target == types::F32 {
            let bits = builder.ins().ireduce(types::I32, raw);
            Ok(builder.ins().bitcast(types::F32, MemFlags::new(), bits))
        } else {
            self.cast(builder, raw, target)
        }
    }

    fn write_static_place(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        place: &MirPlace,
        value: Value,
    ) -> Result<(), String> {
        if matches!(place.ty.layout.abi, MirAbi::Never) {
            return Err(format!(
                "MIR static place {:?} has no runtime value",
                place.id
            ));
        }
        let key = self.persistent_key_handle(builder, place)?;
        let type_handle = self.persistent_type_handle(builder, place)?;
        let target = clif_ty_from_mir(&place.ty)
            .ok_or_else(|| format!("MIR static place {:?} has no ABI", place.id))?;
        let raw = if target == types::F64 {
            let value = self.cast(builder, value, types::F64)?;
            builder.ins().bitcast(types::I64, MemFlags::new(), value)
        } else if target == types::F32 {
            let value = self.cast(builder, value, types::F32)?;
            let bits = builder.ins().bitcast(types::I32, MemFlags::new(), value);
            builder.ins().uextend(types::I64, bits)
        } else {
            self.cast(builder, value, types::I64)?
        };
        let _ = self.call_host(
            builder,
            self.host.persist_runtime_set,
            &[key, type_handle, raw],
        )?;
        Ok(())
    }

    fn write_parameter_address(&self, place: &MirPlace) -> Option<Value> {
        let MirPlaceBase::Parameter(value) = place.base else {
            return None;
        };
        self.write_parameters
            .iter()
            .find(|(parameter, _, _)| *parameter == value)
            .map(|(_, address, _)| *address)
    }

    fn write_place_base(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        place: &MirPlace,
        value: Value,
    ) -> Result<(), String> {
        if let Some(address) = self.write_parameter_address(place) {
            let target = clif_ty_from_mir(&self.place_base_type(place)?)
                .ok_or_else(|| format!("MIR write parameter {:?} has no ABI", place.id))?;
            let value = self.cast(builder, value, target)?;
            builder.ins().store(MemFlags::new(), value, address, 0);
            return Ok(());
        }
        if matches!(&place.base, MirPlaceBase::Static(_)) {
            return self.write_static_place(builder, place, value);
        }
        if let MirPlaceBase::Capture(base_value) = place.base {
            let slot = self.capture_slot_for_value(base_value)?;
            let (access, ty, owned) = self.capture_parameter(slot)?;
            if access != jet_foundation::MIR::MirAccess::Write {
                return Err(format!(
                    "MIR captured place {:?} is not writable ({access:?})",
                    place.id
                ));
            }
            let env = self.capture_env.ok_or_else(|| {
                format!(
                    "invalid MIR: captured place {:?} has no environment",
                    place.id
                )
            })?;
            let index = builder.ins().iconst(types::I64, slot as i64);
            if owned {
                return self.set_field(builder, env, slot, &ty, value);
            }
            let address = self
                .call_host(builder, self.host.struct_get_i64, &[env, index])?
                .first()
                .copied()
                .ok_or_else(|| {
                    format!(
                        "MIR captured place {:?} address getter returned no value",
                        place.id
                    )
                })?;
            let target = clif_ty_from_mir(&ty)
                .ok_or_else(|| format!("MIR captured place {:?} has no ABI", place.id))?;
            let value = self.cast(builder, value, target)?;
            builder.ins().store(MemFlags::new(), value, address, 0);
            return Ok(());
        }
        let slot = self.slot(builder, place)?;
        let ty = self.place_base_type(place)?;
        let target =
            clif_ty_from_mir(&ty).ok_or_else(|| format!("MIR place {:?} has no ABI", place.id))?;
        let value = self.cast(builder, value, target)?;
        builder.ins().stack_store(value, slot, 0);
        Ok(())
    }

    fn slot(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        place: &MirPlace,
    ) -> Result<ir::StackSlot, String> {
        let key = match &place.base {
            MirPlaceBase::Local(local) => SlotKey::Local(*local),
            MirPlaceBase::Parameter(value) | MirPlaceBase::Temporary(value) => {
                SlotKey::Value(*value)
            }
            MirPlaceBase::Capture(_) => {
                return Err(format!(
                    "invalid MIR: captured place {:?} requires an env-backed slot",
                    place.id
                ))
            }
            MirPlaceBase::Static(name) => {
                return Err(format!(
                    "invalid MIR: static place `{name}` has no resolved storage"
                ))
            }
        };
        if let Some(slot) = self.places.get(&key).copied() {
            return Ok(slot);
        }
        let slot =
            builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 0));
        self.places.insert(key, slot);
        match &place.base {
            MirPlaceBase::Parameter(value) | MirPlaceBase::Temporary(value) => {
                let initial = self.value(*value)?;
                let ty = self.place_base_type(place)?;
                let ty = clif_ty_from_mir(&ty)
                    .ok_or_else(|| format!("MIR place {:?} has no ABI", place.id))?;
                let initial = self.cast(builder, initial, ty)?;
                builder.ins().stack_store(initial, slot, 0);
            }
            MirPlaceBase::Local(_) | MirPlaceBase::Capture(_) | MirPlaceBase::Static(_) => {}
        }
        Ok(slot)
    }

    fn place_base_type(&self, place: &MirPlace) -> Result<MirType, String> {
        match &place.base {
            MirPlaceBase::Local(local) => self
                .function
                .locals
                .iter()
                .find(|candidate| candidate.id == *local)
                .map(|candidate| candidate.ty.clone())
                .ok_or_else(|| format!("MIR local {:?} is missing", local)),
            MirPlaceBase::Parameter(value)
            | MirPlaceBase::Capture(value)
            | MirPlaceBase::Temporary(value) => self.mir_value_type(*value),
            MirPlaceBase::Static(name) => self
                .function
                .places
                .iter()
                .find(|candidate| {
                    candidate.projections.is_empty()
                        && matches!(
                            &candidate.base,
                            MirPlaceBase::Static(candidate_name) if candidate_name == name
                        )
                        && candidate.persist_key.as_ref() == place.persist_key.as_ref()
                })
                .map(|candidate| candidate.ty.clone())
                .ok_or_else(|| format!("MIR static place `{name}` has no resolved root type")),
        }
    }

    fn place_base_value(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        place: &MirPlace,
    ) -> Result<Value, String> {
        if let Some(address) = self.write_parameter_address(place) {
            let ty = clif_ty_from_mir(&self.place_base_type(place)?)
                .ok_or_else(|| format!("MIR write parameter {:?} has no ABI", place.id))?;
            return Ok(builder.ins().load(ty, MemFlags::new(), address, 0));
        }
        match &place.base {
            MirPlaceBase::Local(_) => {
                let slot = self.slot(builder, place)?;
                let ty = clif_ty_from_mir(&self.place_base_type(place)?)
                    .ok_or_else(|| format!("MIR place {:?} has no ABI", place.id))?;
                Ok(builder.ins().stack_load(ty, slot, 0))
            }
            MirPlaceBase::Parameter(value) | MirPlaceBase::Temporary(value) => self.value(*value),
            MirPlaceBase::Capture(value) => {
                let slot = self.capture_slot_for_value(*value)?;
                self.capture_slot_value(builder, slot)
            }
            MirPlaceBase::Static(_) => {
                let root = self.persistent_root_place(place)?;
                self.read_static_place(builder, &root)
            }
        }
    }

    fn capture_parameter(
        &self,
        slot: usize,
    ) -> Result<(jet_foundation::MIR::MirAccess, MirType, bool), String> {
        let parameter = self
            .function
            .capture_params
            .iter()
            .find(|parameter| parameter.slot == slot)
            .ok_or_else(|| format!("invalid MIR: capture slot {slot} is not declared"))?;
        Ok((
            parameter.access,
            parameter.ty.clone(),
            parameter.ownership.mode == MirOwnershipMode::Owned,
        ))
    }

    fn capture_slot_value(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        slot: usize,
    ) -> Result<Value, String> {
        let (access, ty, owned) = self.capture_parameter(slot)?;
        let env = self
            .capture_env
            .ok_or_else(|| format!("invalid MIR: capture slot {slot} has no environment"))?;
        let index = builder.ins().iconst(types::I64, slot as i64);
        if access == jet_foundation::MIR::MirAccess::Move || owned {
            let getter = self.field_getter(&ty)?;
            return self
                .call_host(builder, getter, &[env, index])?
                .first()
                .copied()
                .ok_or_else(|| format!("MIR capture slot {slot} getter returned no value"));
        }
        let address = self
            .call_host(builder, self.host.struct_get_i64, &[env, index])?
            .first()
            .copied()
            .ok_or_else(|| format!("MIR capture slot {slot} address getter returned no value"))?;
        let ty =
            clif_ty_from_mir(&ty).ok_or_else(|| format!("MIR capture slot {slot} has no ABI"))?;
        Ok(builder.ins().load(ty, MemFlags::new(), address, 0))
    }

    fn capture(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        slot: usize,
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let value = self.capture_slot_value(builder, slot)?;
        expected.map_or(Ok(value), |target| self.cast(builder, value, target))
    }

    fn capture_slot_for_value(&self, value: MirValueId) -> Result<usize, String> {
        let mut slot = None;
        for instruction in self
            .function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
        {
            if instruction.result != Some(value) {
                continue;
            }
            if let MirOperation::Capture { slot: candidate } = &instruction.operation {
                if slot.replace(*candidate).is_some() {
                    return Err(format!(
                        "invalid MIR: capture value {:?} has multiple slots",
                        value
                    ));
                }
            }
        }
        slot.ok_or_else(|| {
            format!(
                "invalid MIR: place capture value {:?} has no Capture operation",
                value
            )
        })
    }

    fn address_of(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        id: jet_foundation::MIR::MirPlaceId,
    ) -> Result<Value, String> {
        let place = self
            .function
            .places
            .iter()
            .find(|place| place.id == id)
            .cloned()
            .ok_or_else(|| format!("MIR place {:?} is missing", id))?;
        if place.projections.is_empty() {
            if let Some(address) = self.write_parameter_address(&place) {
                return Ok(address);
            }
            if let MirPlaceBase::Capture(value) = place.base {
                let slot = self.capture_slot_for_value(value)?;
                let (access, _, owned) = self.capture_parameter(slot)?;
                if access == jet_foundation::MIR::MirAccess::Move {
                    return Err(format!("MIR captured place {:?} is move-only", id));
                }
                if owned {
                    return Err(format!(
                        "MIR captured place {:?} is owned and has no stable address",
                        id
                    ));
                }
                let env = self.capture_env.ok_or_else(|| {
                    format!(
                        "invalid MIR: captured place {:?} has no environment",
                        place.id
                    )
                })?;
                let index = builder.ins().iconst(types::I64, slot as i64);
                return self
                    .call_host(builder, self.host.struct_get_i64, &[env, index])?
                    .first()
                    .copied()
                    .ok_or_else(|| {
                        format!(
                            "MIR captured place {:?} address getter returned no value",
                            id
                        )
                    });
            }
            let slot = self.slot(builder, &place)?;
            return Ok(builder.ins().stack_addr(types::I64, slot, 0));
        }
        let mut current = self.place_base_value(builder, &place)?;
        let mut current_type = self.place_base_type(&place)?;
        let last = place.projections.len() - 1;
        for (index, projection) in place.projections.iter().enumerate() {
            let final_projection = index == last;
            current = match projection {
                jet_foundation::MIR::MirProjection::Field { field, .. } => {
                    let (field_index, field_ty) = self.field_info(*field)?;
                    if final_projection {
                        return self.field_address(builder, current, field_index, &field_ty);
                    }
                    let value = self.field_load_value(builder, current, *field, None)?;
                    current_type = field_ty;
                    value
                }
                jet_foundation::MIR::MirProjection::Index {
                    kind,
                    index,
                    call,
                    location,
                    context,
                    span,
                    ..
                } => {
                    if final_projection {
                        return Err(format!(
                            "MIR place {:?} projection has no stable address carrier",
                            id
                        ));
                    }
                    let base_type = current_type.clone();
                    let element_type = checked_index_element_type(&base_type, *kind)?.clone();
                    let value = self.index_read(
                        builder,
                        current,
                        Some(&base_type),
                        *index,
                        *kind,
                        *call,
                        location,
                        context.as_ref(),
                        None,
                        span,
                        None,
                    )?;
                    current_type = element_type;
                    value
                }
                jet_foundation::MIR::MirProjection::Deref { .. } => {
                    let address = self.cast(builder, current, types::I64)?;
                    if final_projection {
                        return Ok(address);
                    }
                    builder.ins().load(types::I64, MemFlags::new(), address, 0)
                }
            };
        }
        self.cast(builder, current, types::I64)
    }

    fn pack_generator_value(
        &self,
        builder: &mut FunctionBuilder<'_>,
        value: Value,
    ) -> Result<Value, String> {
        let item = self
            .function
            .generator
            .as_ref()
            .ok_or_else(|| "MIR generator yield has no generator facts".to_string())?
            .item
            .clone();
        let item_ty = clif_ty_from_mir(&item)
            .ok_or_else(|| "MIR generator item has no scalar ABI".to_string())?;
        let value = self.cast(builder, value, item_ty)?;
        self.cast(builder, value, types::I64)
    }

    fn constant(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        constant: &MirConstant,
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let value = match constant {
            MirConstant::Int { value, width, .. } => {
                let value = match width {
                    Some((false, _)) => self.runtime.heap.int_from_u64(*value as u64),
                    _ => self.runtime.heap.int_from_i64(*value),
                };
                builder.ins().iconst(expected.unwrap_or(types::I64), value)
            }
            MirConstant::Float { value, f32, .. } => {
                if *f32 || expected == Some(types::F32) {
                    builder.ins().f32const(*value as f32)
                } else {
                    builder.ins().f64const(*value)
                }
            }
            MirConstant::Bool(value) => builder.ins().iconst(types::I8, i64::from(*value)),
            MirConstant::Char(value) => builder.ins().iconst(types::I32, i64::from(*value as u32)),
            MirConstant::String(value) => builder
                .ins()
                .iconst(types::I64, self.runtime.heap.alloc_string(value.clone())),
            MirConstant::Bytes(values) => {
                let list = self
                    .runtime
                    .heap
                    .alloc_int_list(values.iter().map(|value| i64::from(*value)).collect());
                builder.ins().iconst(types::I64, list)
            }
            MirConstant::Unit => builder.ins().iconst(expected.unwrap_or(types::I64), 0),
            MirConstant::BigInt(value) => {
                let value = self
                    .runtime
                    .heap
                    .int_from_str(value)
                    .map_err(|error| format!("MIR BigInt constant is invalid: {error}"))?;
                builder.ins().iconst(types::I64, value)
            }
            MirConstant::List(values) => self.constant_list(builder, values)?,
            MirConstant::Map(values) => self.constant_map(builder, values)?,
            MirConstant::Struct { type_name, fields } => {
                self.constant_struct(builder, type_name, fields)?
            }
            MirConstant::Enum {
                type_name,
                variant,
                args,
            } => self.constant_enum(builder, type_name, variant, args)?,
            MirConstant::Present(value) => {
                let value = self.constant(builder, value, None)?;
                self.result_value(builder, true, value, expected)?
            }
            MirConstant::Failed(report) => match report {
                MirConstReport::Clean(_) => self.result_absent(builder)?,
                MirConstReport::Told(value) => {
                    let value = self.constant(builder, value, None)?;
                    self.result_value(builder, false, value, expected)?
                }
            },
        };
        expected.map_or(Ok(value), |target| self.cast(builder, value, target))
    }

    fn constant_list(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        values: &[MirConstant],
    ) -> Result<Value, String> {
        let list = self
            .call_host(builder, self.host.coll.list_new, &[])?
            .first()
            .copied()
            .ok_or_else(|| "MIR constant list host returned no list".to_string())?;
        for value in values {
            let value = self.constant(builder, value, None)?;
            let value_type = self.value_type(builder, value);
            let (host, value) = if value_type == types::F64 {
                (self.host.coll.list_push_f64, value)
            } else if value_type == types::F32 {
                (
                    self.host.coll.list_push_f64,
                    builder.ins().fpromote(types::F64, value),
                )
            } else {
                (
                    self.host.coll.list_push,
                    self.cast(builder, value, types::I64)?,
                )
            };
            let _ = self.call_host(builder, host, &[list, value])?;
        }
        Ok(list)
    }

    fn constant_map(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        values: &std::collections::BTreeMap<MirConstKey, MirConstant>,
    ) -> Result<Value, String> {
        let map = self
            .call_host(builder, self.host.coll.map_new, &[])?
            .first()
            .copied()
            .ok_or_else(|| "MIR constant map host returned no map".to_string())?;
        for (key, value) in values {
            let (kind, key) = self.constant_key(builder, key)?;
            let value = self.constant(builder, value, None)?;
            let value = self.cast(builder, value, types::I64)?;
            let host = match kind {
                MapKeyKind::String => self.host.coll.map_insert,
                MapKeyKind::Int => self.host.coll.map_insert_int,
                MapKeyKind::Composite => self.host.coll.map_insert_composite,
            };
            let _ = self.call_host(builder, host, &[map, key, value])?;
        }
        Ok(map)
    }

    fn constant_key(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        key: &MirConstKey,
    ) -> Result<(MapKeyKind, Value), String> {
        match key {
            MirConstKey::Int(value) => Ok((
                MapKeyKind::Int,
                builder
                    .ins()
                    .iconst(types::I64, self.runtime.heap.int_from_i64(*value)),
            )),
            MirConstKey::String(value) => Ok((
                MapKeyKind::String,
                builder
                    .ins()
                    .iconst(types::I64, self.runtime.heap.alloc_string(value.clone())),
            )),
            MirConstKey::Bool(value) => Ok((
                MapKeyKind::Int,
                builder.ins().iconst(types::I64, i64::from(*value)),
            )),
            MirConstKey::Char(value) => Ok((
                MapKeyKind::Int,
                builder.ins().iconst(types::I64, i64::from(*value as u32)),
            )),
            MirConstKey::Tuple(values) => {
                let record =
                    self.constant_key_record(builder, values.iter().map(|(_, key)| key))?;
                Ok((MapKeyKind::Composite, record))
            }
            MirConstKey::Struct { fields, .. } => {
                let record =
                    self.constant_key_record(builder, fields.iter().map(|(_, key)| key))?;
                Ok((MapKeyKind::Composite, record))
            }
            MirConstKey::Enum { type_name, variant } => {
                if is_ordering_name(type_name) {
                    let discriminant = self.enum_discriminant(type_name, variant)?;
                    return Ok((
                        MapKeyKind::Int,
                        builder.ins().iconst(types::I64, discriminant),
                    ));
                }
                let one = builder.ins().iconst(types::I64, 1);
                let record = self
                    .call_host(builder, self.host.struct_new, &[one])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR enum map key host returned no record".to_string())?;
                let discriminant = self.enum_discriminant(type_name, variant)?;
                let index = builder.ins().iconst(types::I64, 0);
                let value = builder.ins().iconst(types::I64, discriminant);
                let _ =
                    self.call_host(builder, self.host.struct_set_i64, &[record, index, value])?;
                Ok((MapKeyKind::Composite, record))
            }
        }
    }

    fn constant_key_record<'k, I>(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        keys: I,
    ) -> Result<Value, String>
    where
        I: IntoIterator<Item = &'k MirConstKey>,
    {
        let keys = keys.into_iter().collect::<Vec<&MirConstKey>>();
        let count = builder.ins().iconst(types::I64, keys.len() as i64);
        let record = self
            .call_host(builder, self.host.struct_new, &[count])?
            .first()
            .copied()
            .ok_or_else(|| "MIR composite map key host returned no record".to_string())?;
        for (index, key) in keys.into_iter().enumerate() {
            let (kind, value) = self.constant_key(builder, key)?;
            let index = builder.ins().iconst(types::I64, index as i64);
            let _ = match kind {
                MapKeyKind::String => {
                    self.call_host(builder, self.host.struct_set_str, &[record, index, value])?
                }
                MapKeyKind::Int => {
                    self.call_host(builder, self.host.struct_set_i64, &[record, index, value])?
                }
                MapKeyKind::Composite => self.call_host(
                    builder,
                    self.host.struct_set_record,
                    &[record, index, value],
                )?,
            };
        }
        Ok(record)
    }

    fn constant_struct(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        _type_name: &str,
        fields: &[(String, MirConstant)],
    ) -> Result<Value, String> {
        let count = builder.ins().iconst(types::I64, fields.len() as i64);
        let record = self
            .call_host(builder, self.host.struct_new, &[count])?
            .first()
            .copied()
            .ok_or_else(|| "MIR struct constant host returned no record".to_string())?;
        for (index, (_, constant)) in fields.iter().enumerate() {
            let value = self.constant(builder, constant, None)?;
            self.set_constant_field(builder, record, index, constant, value)?;
        }
        Ok(record)
    }

    fn constant_enum(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        type_name: &str,
        variant: &str,
        args: &[(Option<String>, MirConstant)],
    ) -> Result<Value, String> {
        if is_ordering_name(type_name) {
            if !args.is_empty() {
                return Err(format!(
                    "MIR enum constant `{type_name}::{variant}` expects no arguments, got {}",
                    args.len()
                ));
            }
            let discriminant = self.enum_discriminant(type_name, variant)?;
            return Ok(builder.ins().iconst(types::I64, discriminant));
        }
        let has_named = args.iter().any(|(field, _)| field.is_some());
        let has_positional = args.iter().any(|(field, _)| field.is_none());
        if has_named && has_positional {
            return Err("MIR enum constant mixes named and positional payloads".to_string());
        }
        let count = builder.ins().iconst(types::I64, args.len() as i64 + 1);
        let record = self
            .call_host(builder, self.host.struct_new, &[count])?
            .first()
            .copied()
            .ok_or_else(|| "MIR enum constant host returned no record".to_string())?;
        let zero = builder.ins().iconst(types::I64, 0);
        let discriminant = builder
            .ins()
            .iconst(types::I64, self.enum_discriminant(type_name, variant)?);
        let _ = self.call_host(
            builder,
            self.host.struct_set_i64,
            &[record, zero, discriminant],
        )?;
        for (index, (_, constant)) in args.iter().enumerate() {
            let value = self.constant(builder, constant, None)?;
            self.set_constant_field(builder, record, index + 1, constant, value)?;
        }
        Ok(record)
    }

    fn set_constant_field(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        record: Value,
        index: usize,
        constant: &MirConstant,
        value: Value,
    ) -> Result<(), String> {
        let index = builder.ins().iconst(types::I64, index as i64);
        let host = match constant {
            MirConstant::Float { .. } => {
                let value = self.cast(builder, value, types::F64)?;
                return self
                    .call_host(builder, self.host.struct_set_f64, &[record, index, value])
                    .map(|_| ());
            }
            MirConstant::Bool(_) => self.host.struct_set_bool,
            MirConstant::Char(_) => self.host.struct_set_char,
            MirConstant::String(_) => self.host.struct_set_str,
            MirConstant::Enum { type_name, .. } if is_ordering_name(type_name) => {
                self.host.struct_set_i64
            }
            MirConstant::Struct { .. } | MirConstant::Enum { .. } => self.host.struct_set_record,
            MirConstant::Int { .. }
            | MirConstant::Bytes(_)
            | MirConstant::Unit
            | MirConstant::BigInt(_)
            | MirConstant::List(_)
            | MirConstant::Map(_)
            | MirConstant::Present(_)
            | MirConstant::Failed(_) => self.host.struct_set_i64,
        };
        let target = if matches!(constant, MirConstant::Bool(_)) {
            types::I8
        } else if matches!(constant, MirConstant::Char(_)) {
            types::I32
        } else {
            types::I64
        };
        let value = self.cast(builder, value, target)?;
        self.call_host(builder, host, &[record, index, value])
            .map(|_| ())
    }

    fn enum_discriminant(&self, type_name: &str, variant: &str) -> Result<i64, String> {
        if is_ordering_name(type_name) {
            return prelude_enum_variant_index(jet_foundation::Syntax::TYPE_ORDERING, variant)
                .ok_or_else(|| {
                    format!("Prelude Ordering variant `{type_name}::{variant}` is missing metadata")
                });
        }
        let definition = self
            .program
            .types
            .iter()
            .find(|definition| definition.name == type_name || definition.key == type_name)
            .ok_or_else(|| format!("MIR enum type `{type_name}` is missing"))?;
        let MirTypeDefKind::Enum { variants, .. } = &definition.kind else {
            return Err(format!("MIR type `{type_name}` is not an enum"));
        };
        variants
            .iter()
            .enumerate()
            .find(|(_, candidate)| candidate.name == variant)
            .map(|(index, candidate)| candidate.discriminant.unwrap_or(index as i64))
            .ok_or_else(|| format!("MIR enum variant `{type_name}::{variant}` is missing"))
    }

    fn unary(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        op: MirUnaryOp,
        default_int: bool,
        value: Value,
    ) -> Result<Value, String> {
        let ty = self.value_type(builder, value);
        match op {
            MirUnaryOp::Neg => {
                if ty == types::F64 || ty == types::F32 {
                    Ok(builder.ins().fneg(value))
                } else if default_int {
                    self.call_host(builder, self.host.num.int_neg, &[value])?
                        .first()
                        .copied()
                        .ok_or_else(|| {
                            "MIR default Int negation host returned no value".to_string()
                        })
                } else {
                    Ok(builder.ins().ineg(value))
                }
            }
            MirUnaryOp::Not => {
                let value = self.bool_value(builder, value)?;
                let one = builder.ins().iconst(types::I8, 1);
                Ok(builder.ins().bxor(value, one))
            }
        }
    }

    fn binary(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        op: MirBinaryOp,
        left_ty: &MirType,
        right_ty: &MirType,
        left: Value,
        right: Value,
        location: MirPanicLoc,
    ) -> Result<Value, String> {
        let ty = self.value_type(builder, left);
        if op.is_comparison() || matches!(op, MirBinaryOp::Compare) {
            if !left_ty.same_checked_type(right_ty) {
                return Err(format!(
                    "MIR primitive comparison has mismatched checked operands `{}` and `{}`",
                    left_ty.display_name(),
                    right_ty.display_name()
                ));
            }
            if is_exact_int_type(left_ty) {
                return self.exact_int_comparison(builder, op, left, right);
            }
            if !is_comparison_scalar(left_ty) || !is_comparison_scalar(right_ty) {
                return self.typed_comparison(builder, op, left_ty, left, right, location);

            }
            if op.is_comparison() {
                return Ok(self.compare(builder, op, left, right));
            }
        }
        if ty == types::F64 || ty == types::F32 {
            return match op {
                MirBinaryOp::Add => Ok(builder.ins().fadd(left, right)),
                MirBinaryOp::Sub => Ok(builder.ins().fsub(left, right)),
                MirBinaryOp::Mul => Ok(builder.ins().fmul(left, right)),
                MirBinaryOp::Div => Ok(builder.ins().fdiv(left, right)),
                MirBinaryOp::Pow | MirBinaryOp::FloorDiv => {
                    let left = self.cast(builder, left, types::F64)?;
                    let right = self.cast(builder, right, types::F64)?;
                    let id = if matches!(op, MirBinaryOp::Pow) {
                        self.host.pow_f64
                    } else {
                        self.host.floordiv_f64
                    };
                    self.call_host(builder, id, &[left, right])?
                        .first()
                        .copied()
                        .ok_or_else(|| "float arithmetic host returned no value".to_string())
                }
                MirBinaryOp::Rem | MirBinaryOp::Mod => {
                    Err("MIR float modulo has no resolved Prelude route".to_string())
                }
                MirBinaryOp::BitAnd
                | MirBinaryOp::BitOr
                | MirBinaryOp::BitXor
                | MirBinaryOp::Shl
                | MirBinaryOp::Shr
                | MirBinaryOp::Compare
                | MirBinaryOp::And
                | MirBinaryOp::Or => {
                    Err(format!("MIR operator {} is invalid for float", op.spell()))
                }
                MirBinaryOp::Eq
                | MirBinaryOp::Ne
                | MirBinaryOp::Lt
                | MirBinaryOp::Gt
                | MirBinaryOp::Le
                | MirBinaryOp::Ge => unreachable!(),
            };
        }
        if is_exact_int_type(left_ty) {
            return self.exact_int_binary(builder, op, left, right, location);
        }
        let line = builder.ins().iconst(types::I32, i64::from(location.line));
        match op {
            MirBinaryOp::Add => self.call_i64_binary(builder, self.host.add_i64, left, right, line),
            MirBinaryOp::Sub => self.call_i64_binary(builder, self.host.sub_i64, left, right, line),
            MirBinaryOp::Mul => self.call_i64_binary(builder, self.host.mul_i64, left, right, line),
            MirBinaryOp::Div => self.call_i64_binary(builder, self.host.div_i64, left, right, line),
            MirBinaryOp::Rem => self.call_i64_binary(builder, self.host.rem_i64, left, right, line),
            MirBinaryOp::Pow => self.call_i64_binary(builder, self.host.pow_i64, left, right, line),
            MirBinaryOp::FloorDiv => {
                self.call_i64_binary(builder, self.host.floordiv_i64, left, right, line)
            }
            MirBinaryOp::Mod => self.call_i64_binary(builder, self.host.mod_i64, left, right, line),
            MirBinaryOp::BitAnd | MirBinaryOp::And => Ok(builder.ins().band(left, right)),
            MirBinaryOp::BitOr | MirBinaryOp::Or => Ok(builder.ins().bor(left, right)),
            MirBinaryOp::BitXor => Ok(builder.ins().bxor(left, right)),
            MirBinaryOp::Shl => Ok(builder.ins().ishl(left, right)),
            MirBinaryOp::Shr => Ok(builder.ins().sshr(left, right)),
            MirBinaryOp::Compare => {
                let less = self.compare(builder, MirBinaryOp::Lt, left, right);
                let greater = self.compare(builder, MirBinaryOp::Gt, left, right);
                let less = builder.ins().uextend(types::I64, less);
                let greater = builder.ins().uextend(types::I64, greater);
                let order = builder.ins().isub(greater, less);
                Ok(self.ordering_value(builder, order))
            }
            MirBinaryOp::Eq
            | MirBinaryOp::Ne
            | MirBinaryOp::Lt
            | MirBinaryOp::Gt
            | MirBinaryOp::Le
            | MirBinaryOp::Ge => unreachable!(),
        }
    }
    fn exact_int_binary(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        op: MirBinaryOp,
        left: Value,
        right: Value,
        location: MirPanicLoc,
    ) -> Result<Value, String> {
        let left = self.cast(builder, left, types::I64)?;
        let right = self.cast(builder, right, types::I64)?;
        let (id, located) = match op {
            MirBinaryOp::Add => (self.host.num.row_int_add, false),
            MirBinaryOp::Sub => (self.host.num.row_int_sub, false),
            MirBinaryOp::Mul => (self.host.num.row_int_mul, false),
            MirBinaryOp::BitAnd => (self.host.num.row_int_bit_and, false),
            MirBinaryOp::BitOr => (self.host.num.row_int_bit_or, false),
            MirBinaryOp::BitXor => (self.host.num.row_int_bit_xor, false),
            MirBinaryOp::Div => (self.host.num.row_int_div, true),
            MirBinaryOp::Rem => (self.host.num.row_int_rem, true),
            MirBinaryOp::FloorDiv => (self.host.num.row_int_floor_div, true),
            MirBinaryOp::Mod => (self.host.num.row_int_mod, true),
            MirBinaryOp::Pow => (self.host.num.row_int_pow, true),
            MirBinaryOp::Shl => (self.host.num.row_int_shl, true),
            MirBinaryOp::Shr => (self.host.num.row_int_shr, true),
            _ => {
                return Err(format!(
                    "MIR exact integer arithmetic cannot implement `{}`",
                    op.spell()
                ))
            }
        };
        let mut args = vec![left, right];
        if located {
            args.extend(self.panic_location(builder, &location)?);
        }
        self.call_host(builder, id, &args)?
            .first()
            .copied()
            .ok_or_else(|| "exact integer arithmetic host returned no value".to_string())
    }

    fn exact_int_comparison(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        op: MirBinaryOp,
        left: Value,
        right: Value,
    ) -> Result<Value, String> {
        let left = self.cast(builder, left, types::I64)?;
        let right = self.cast(builder, right, types::I64)?;
        let order = self
            .call_host(builder, self.host.num.int_compare, &[left, right])?
            .first()
            .copied()
            .ok_or_else(|| "exact integer comparison host returned no value".to_string())?;
        if matches!(op, MirBinaryOp::Compare) {
            return Ok(self.ordering_value(builder, order));
        }
        let zero = builder.ins().iconst(types::I64, 0);
        let cc = match op {
            MirBinaryOp::Eq => IntCC::Equal,
            MirBinaryOp::Ne => IntCC::NotEqual,
            MirBinaryOp::Lt => IntCC::SignedLessThan,
            MirBinaryOp::Gt => IntCC::SignedGreaterThan,
            MirBinaryOp::Le => IntCC::SignedLessThanOrEqual,
            MirBinaryOp::Ge => IntCC::SignedGreaterThanOrEqual,
            _ => {
                return Err(format!(
                    "MIR exact integer comparison cannot implement `{}`",
                    op.spell()
                ))
            }
        };
        Ok(builder.ins().icmp_imm(cc, order, 0))
    }

    fn typed_equal(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        left_ty: &MirType,
        left: Value,
        right: Value,
    ) -> Result<Value, String> {
        let type_id = runtime_type_id(left_ty).ok_or_else(|| {
            format!(
                "MIR structural comparison type `{}` has no runtime identity",
                left_ty.display_name()
            )
        })?;
        let left = self.cast(builder, left, types::I64)?;
        let right = self.cast(builder, right, types::I64)?;
        let type_id = builder.ins().iconst(types::I64, type_id as i64);
        self.call_host(builder, self.host.typed_eq, &[left, right, type_id])?
            .first()
            .copied()
            .ok_or_else(|| "MIR structural comparison host returned no value".to_string())
    }

    fn option_comparison(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        op: MirBinaryOp,
        left_ty: &MirType,
        left: Value,
        right: Value,
        location: MirPanicLoc,
    ) -> Result<Value, String> {
        let MirTypeKind::Option(inner) = left_ty.kind() else {
            return Err("MIR option comparison has a non-option checked type".to_string());
        };
        let left = self.cast(builder, left, types::I64)?;
        let right = self.cast(builder, right, types::I64)?;
        let left_present = self
            .call_host(builder, self.host.result_is_ok, &[left])?
            .first()
            .copied()
            .ok_or_else(|| "MIR option comparison left discriminator returned no value".to_string())?;
        let right_present = self
            .call_host(builder, self.host.result_is_ok, &[right])?
            .first()
            .copied()
            .ok_or_else(|| "MIR option comparison right discriminator returned no value".to_string())?;
        let left_present = self.bool_value(builder, left_present)?;
        let right_present = self.bool_value(builder, right_present)?;

        let result_type = if matches!(op, MirBinaryOp::Compare) {
            types::I64
        } else {
            types::I8
        };
        let merge = builder.create_block();
        builder.append_block_param(merge, result_type);
        let both_present = builder.ins().band(left_present, right_present);
        let both_present_block = builder.create_block();
        let not_both_present_block = builder.create_block();
        let left_present_only_block = builder.create_block();
        let left_absent_only_block = builder.create_block();
        let left_absent_right_present_block = builder.create_block();
        let both_absent_block = builder.create_block();
        builder.ins().brif(
            both_present,
            both_present_block,
            &[],
            not_both_present_block,
            &[],
        );

        builder.switch_to_block(both_present_block);
        let payload_type = clif_ty_from_mir(inner).ok_or_else(|| {
            format!(
                "MIR option comparison payload `{}` has no checked carrier",
                inner.display_name()
            )
        })?;
        let left_payload = self.result_value_get_raw(builder, left, Some(payload_type))?;
        let right_payload = self.result_value_get_raw(builder, right, Some(payload_type))?;
        let payload = self.binary(
            builder,
            op,
            inner,
            inner,
            left_payload,
            right_payload,
            location,
        )?;
        builder.ins().jump(merge, &[payload]);

        builder.switch_to_block(not_both_present_block);
        builder.ins().brif(
            left_present,
            left_present_only_block,
            &[],
            left_absent_only_block,
            &[],
        );

        builder.switch_to_block(left_present_only_block);
        let value = self.option_ordering_result(builder, op, 1)?;
        builder.ins().jump(merge, &[value]);

        builder.switch_to_block(left_absent_only_block);
        builder.ins().brif(
            right_present,
            left_absent_right_present_block,
            &[],
            both_absent_block,
            &[],
        );

        builder.switch_to_block(left_absent_right_present_block);
        let value = self.option_ordering_result(builder, op, -1)?;
        builder.ins().jump(merge, &[value]);

        builder.switch_to_block(both_absent_block);
        let value = self.option_ordering_result(builder, op, 0)?;
        builder.ins().jump(merge, &[value]);

        builder.switch_to_block(merge);
        builder
            .block_params(merge)
            .first()
            .copied()
            .ok_or_else(|| "MIR option comparison merge has no result".to_string())
    }

    fn option_ordering_result(
        &self,
        builder: &mut FunctionBuilder<'_>,
        op: MirBinaryOp,
        ordering: i64,
    ) -> Result<Value, String> {
        if matches!(op, MirBinaryOp::Compare) {
            let ordering = builder.ins().iconst(types::I64, ordering);
            return Ok(self.ordering_value(builder, ordering));
        }
        let value = match op {
            MirBinaryOp::Eq => ordering == 0,
            MirBinaryOp::Ne => ordering != 0,
            MirBinaryOp::Lt => ordering < 0,
            MirBinaryOp::Gt => ordering > 0,
            MirBinaryOp::Le => ordering <= 0,
            MirBinaryOp::Ge => ordering >= 0,
            _ => {
                return Err(format!(
                    "MIR option comparison cannot implement `{}`",
                    op.spell()
                ))
            }
        };
        Ok(builder.ins().iconst(types::I8, if value { 1 } else { 0 }))
    }

    fn comparison_from_order(
        &self,
        builder: &mut FunctionBuilder<'_>,
        op: MirBinaryOp,
        order: Value,
    ) -> Result<Value, String> {
        if matches!(op, MirBinaryOp::Compare) {
            return Ok(self.ordering_value(builder, order));
        }
        let cc = match op {
            MirBinaryOp::Eq => IntCC::Equal,
            MirBinaryOp::Ne => IntCC::NotEqual,
            MirBinaryOp::Lt => IntCC::SignedLessThan,
            MirBinaryOp::Gt => IntCC::SignedGreaterThan,
            MirBinaryOp::Le => IntCC::SignedLessThanOrEqual,
            MirBinaryOp::Ge => IntCC::SignedGreaterThanOrEqual,
            _ => {
                return Err(format!(
                    "MIR temporal comparison cannot implement `{}`",
                    op.spell()
                ))
            }
        };
        Ok(builder.ins().icmp_imm(cc, order, 0))
    }

    fn zoned_comparison(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        op: MirBinaryOp,
        left: Value,
        right: Value,
    ) -> Result<Value, String> {
        let method_name = if matches!(op, MirBinaryOp::Eq | MirBinaryOp::Ne) {
            "equal"
        } else {
            "compare"
        };
        let method = builder
            .ins()
            .iconst(types::I64, self.runtime.heap.alloc_string(method_name));
        let zero = builder.ins().iconst(types::I64, 0);
        let result = self
            .call_host(
                builder,
                self.host.time.civil_method,
                &[
                    left, method, right, zero, zero, zero, zero, zero, zero,
                ],
            )?
            .first()
            .copied()
            .ok_or_else(|| "MIR ZonedDateTime comparison host returned no value".to_string())?;
        if matches!(op, MirBinaryOp::Eq | MirBinaryOp::Ne) {
            let equal = self.bool_value(builder, result)?;
            if matches!(op, MirBinaryOp::Ne) {
                let one = builder.ins().iconst(types::I8, 1);
                return Ok(builder.ins().bxor(equal, one));
            }
            return Ok(equal);
        }

        let less_tag = prelude_enum_variant_index(
            jet_foundation::Syntax::TYPE_ORDERING,
            "Less",
        )
        .ok_or_else(|| "MIR Ordering enum has no Less variant".to_string())?;
        let greater_tag = prelude_enum_variant_index(
            jet_foundation::Syntax::TYPE_ORDERING,
            "Greater",
        )
        .ok_or_else(|| "MIR Ordering enum has no Greater variant".to_string())?;
        let less = builder.ins().icmp_imm(IntCC::Equal, result, less_tag);
        let greater = builder
            .ins()
            .icmp_imm(IntCC::Equal, result, greater_tag);
        let one = builder.ins().iconst(types::I64, 1);
        let zero = builder.ins().iconst(types::I64, 0);
        let nonless = builder.ins().select(greater, one, zero);
        let negative_one = builder.ins().iconst(types::I64, -1);
        let order = builder.ins().select(less, negative_one, nonless);
        self.comparison_from_order(builder, op, order)
    }

    fn typed_comparison(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        op: MirBinaryOp,
        left_ty: &MirType,
        left: Value,
        right: Value,
        location: MirPanicLoc,
    ) -> Result<Value, String> {
        let unsupported = || {
            format!(
                "MIR primitive comparison is not defined for these values (checked type `{}`)",
                left_ty.display_name()
            )
        };
        if is_duration_type(left_ty) {
            return self.exact_int_comparison(builder, op, left, right);
        }
        if is_named_type(left_ty, "Instant") {
            let order = self
                .call_host(builder, self.host.time.instant_compare, &[left, right])?
                .first()
                .copied()
                .ok_or_else(|| "MIR Instant comparison host returned no value".to_string())?;
            return self.comparison_from_order(builder, op, order);
        }
        if is_named_type(left_ty, "ZonedDateTime") {
            return self.zoned_comparison(builder, op, left, right);
        }
        if matches!(left_ty.kind(), MirTypeKind::Option(_)) {
            return self.option_comparison(builder, op, left_ty, left, right, location);
        }


        if comparison_element_kind(left_ty) == Some(ComparisonElementKind::Date) {
            if matches!(op, MirBinaryOp::Eq | MirBinaryOp::Ne) {
                let equal = self
                    .call_host(builder, self.host.time.date_equal, &[left, right])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR Date comparison host returned no value".to_string())?;
                if matches!(op, MirBinaryOp::Ne) {
                    let equal = self.bool_value(builder, equal)?;
                    let one = builder.ins().iconst(types::I8, 1);
                    return Ok(builder.ins().bxor(equal, one));
                }
                return Ok(equal);
            }
            if matches!(
                op,
                MirBinaryOp::Lt
                    | MirBinaryOp::Gt
                    | MirBinaryOp::Le
                    | MirBinaryOp::Ge
                    | MirBinaryOp::Compare
            ) {
                let order = self
                    .call_host(builder, self.host.time.date_compare, &[left, right])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR Date ordering host returned no value".to_string())?;
                if matches!(op, MirBinaryOp::Compare) {
                    return Ok(self.ordering_value(builder, order));
                }
                let zero = builder.ins().iconst(types::I64, 0);
                return Ok(self.compare(builder, op, order, zero));
            }
            return Err(unsupported());
        }

        if comparison_element_kind(left_ty) == Some(ComparisonElementKind::String) {
            if matches!(
                op,
                MirBinaryOp::Lt | MirBinaryOp::Gt | MirBinaryOp::Le | MirBinaryOp::Ge
            ) {
                let order = self
                    .call_host(builder, self.host.str_order, &[left, right])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR String ordering host returned no value".to_string())?;
                let zero = builder.ins().iconst(types::I64, 0);
                return Ok(self.compare(builder, op, order, zero));
            }
            if matches!(op, MirBinaryOp::Compare) {
                let order = self
                    .call_host(builder, self.host.str_order, &[left, right])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR String ordering host returned no value".to_string())?;
                return Ok(self.ordering_value(builder, order));
            }
            if !matches!(op, MirBinaryOp::Eq | MirBinaryOp::Ne) {
                return Err(unsupported());
            }
            let equal = self
                .call_host(builder, self.host.str_eq, &[left, right])?
                .first()
                .copied()
                .ok_or_else(|| "MIR String comparison host returned no value".to_string())?;
            if matches!(op, MirBinaryOp::Ne) {
                let equal = self.bool_value(builder, equal)?;
                let one = builder.ins().iconst(types::I8, 1);
                return Ok(builder.ins().bxor(equal, one));
            }
            return Ok(equal);
        }

        if let Some(element) = comparison_sequence_element_type(left_ty) {
            if matches!(op, MirBinaryOp::Eq | MirBinaryOp::Ne)
                && comparison_element_kind(element) != Some(ComparisonElementKind::Date)
            {
                let comparison = self.typed_equal(builder, left_ty, left, right)?;
                if matches!(op, MirBinaryOp::Ne) {
                    let equal = self.bool_value(builder, comparison)?;
                    let one = builder.ins().iconst(types::I8, 1);
                    return Ok(builder.ins().bxor(equal, one));
                }
                return Ok(comparison);
            }
            let (host, ordered) = match op {
                MirBinaryOp::Eq | MirBinaryOp::Ne => (self.host.coll.list_eq_date, false),
                MirBinaryOp::Lt
                | MirBinaryOp::Gt
                | MirBinaryOp::Le
                | MirBinaryOp::Ge
                | MirBinaryOp::Compare => {
                    let host = match comparison_element_kind(element) {
                        Some(ComparisonElementKind::Integer) => self.host.coll.list_order,
                        Some(ComparisonElementKind::Float) => self.host.coll.list_order_f64,
                        Some(ComparisonElementKind::String) => self.host.coll.list_order_str,
                        Some(ComparisonElementKind::Date) => self.host.coll.list_order_date,
                        _ => return Err(unsupported()),
                    };
                    (host, true)
                }
                _ => return Err(unsupported()),
            };
            let comparison = self
                .call_host(builder, host, &[left, right])?
                .first()
                .copied()
                .ok_or_else(|| "MIR list comparison host returned no value".to_string())?;
            if matches!(op, MirBinaryOp::Compare) {
                return Ok(comparison);
            }
            if ordered {
                return self.list_order_result(builder, op, comparison);
            }
            if matches!(op, MirBinaryOp::Ne) {
                let equal = self.bool_value(builder, comparison)?;
                let one = builder.ins().iconst(types::I8, 1);
                return Ok(builder.ins().bxor(equal, one));
            }
            return Ok(comparison);
        }

        if let Some((key, value)) = comparison_map_parts(left_ty) {
            if matches!(op, MirBinaryOp::Eq | MirBinaryOp::Ne)
                && comparison_element_kind(key) == Some(ComparisonElementKind::String)
                && comparison_element_kind(value) == Some(ComparisonElementKind::Integer)
            {
                let equal = self
                    .call_host(builder, self.host.coll.map_equal, &[left, right])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR map comparison host returned no value".to_string())?;
                if matches!(op, MirBinaryOp::Ne) {
                    let equal = self.bool_value(builder, equal)?;
                    let one = builder.ins().iconst(types::I8, 1);
                    return Ok(builder.ins().bxor(equal, one));
                }
                return Ok(equal);
            }
        }

        if matches!(op, MirBinaryOp::Eq | MirBinaryOp::Ne)
            && comparison_map_parts(left_ty).is_none()
            && matches!(
                left_ty.layout.abi,
                MirAbi::Aggregate | MirAbi::Sequence | MirAbi::Nominal | MirAbi::Dynamic
            )
        {
            let equal = self.typed_equal(builder, left_ty, left, right)?;
            if matches!(op, MirBinaryOp::Ne) {
                let equal = self.bool_value(builder, equal)?;
                let one = builder.ins().iconst(types::I8, 1);
                return Ok(builder.ins().bxor(equal, one));
            }
            return Ok(equal);
        }
        Err(unsupported())
    }

    fn list_order_result(
        &self,
        builder: &mut FunctionBuilder<'_>,
        op: MirBinaryOp,
        order: Value,
    ) -> Result<Value, String> {
        let valid = builder.ins().icmp_imm(IntCC::SignedLessThan, order, 3);
        let relation = match op {
            MirBinaryOp::Lt => builder.ins().icmp_imm(IntCC::Equal, order, 0),
            MirBinaryOp::Gt => builder.ins().icmp_imm(IntCC::Equal, order, 2),
            MirBinaryOp::Le => builder
                .ins()
                .icmp_imm(IntCC::SignedLessThanOrEqual, order, 1),
            MirBinaryOp::Ge => builder
                .ins()
                .icmp_imm(IntCC::SignedGreaterThanOrEqual, order, 1),
            _ => {
                return Err(format!(
                    "MIR list ordering host cannot implement `{}`",
                    op.spell()
                ))
            }
        };
        Ok(builder.ins().band(valid, relation))
    }

    fn ordering_value(&self, builder: &mut FunctionBuilder<'_>, order: Value) -> Value {
        let less = builder.ins().icmp_imm(IntCC::SignedLessThan, order, 0);
        let greater = builder.ins().icmp_imm(IntCC::SignedGreaterThan, order, 0);
        let equal = builder.ins().iconst(types::I64, 1);
        let greater_value = builder.ins().iconst(types::I64, 2);
        let greater = builder.ins().select(greater, greater_value, equal);
        let less_value = builder.ins().iconst(types::I64, 0);
        builder.ins().select(less, less_value, greater)
    }

    fn compare(
        &self,
        builder: &mut FunctionBuilder<'_>,
        op: MirBinaryOp,
        left: Value,
        right: Value,
    ) -> Value {
        let ty = self.value_type(builder, left);
        if ty == types::F64 || ty == types::F32 {
            let cc = match op {
                MirBinaryOp::Eq => FloatCC::Equal,
                MirBinaryOp::Ne => FloatCC::NotEqual,
                MirBinaryOp::Lt => FloatCC::LessThan,
                MirBinaryOp::Gt => FloatCC::GreaterThan,
                MirBinaryOp::Le => FloatCC::LessThanOrEqual,
                MirBinaryOp::Ge => FloatCC::GreaterThanOrEqual,
                _ => FloatCC::Equal,
            };
            builder.ins().fcmp(cc, left, right)
        } else {
            let cc = match op {
                MirBinaryOp::Eq => IntCC::Equal,
                MirBinaryOp::Ne => IntCC::NotEqual,
                MirBinaryOp::Lt => IntCC::SignedLessThan,
                MirBinaryOp::Gt => IntCC::SignedGreaterThan,
                MirBinaryOp::Le => IntCC::SignedLessThanOrEqual,
                MirBinaryOp::Ge => IntCC::SignedGreaterThanOrEqual,
                _ => IntCC::Equal,
            };
            builder.ins().icmp(cc, left, right)
        }
    }

    fn build_list(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        values: &[MirValueId],
    ) -> Result<Value, String> {
        let list = self
            .call_host(builder, self.host.coll.list_new, &[])?
            .first()
            .copied()
            .ok_or_else(|| "MIR list host returned no value".to_string())?;
        for id in values {
            let value = self.value(*id)?;
            let ty = self.value_type(builder, value);
            let (host, value) = if ty == types::F64 {
                (self.host.coll.list_push_f64, value)
            } else if ty == types::F32 {
                (
                    self.host.coll.list_push_f64,
                    builder.ins().fpromote(types::F64, value),
                )
            } else {
                (
                    self.host.coll.list_push,
                    self.cast(builder, value, types::I64)?,
                )
            };
            let _ = self.call_host(builder, host, &[list, value])?;
        }
        Ok(list)
    }
    fn list_copy_host(&self, element: &MirType) -> FuncId {
        match element.kind() {
            MirTypeKind::Float | MirTypeKind::Float32 => self.host.coll.list_copy_f64,
            MirTypeKind::String => self.host.coll.list_copy_str,
            MirTypeKind::Tagged { inner, .. } | MirTypeKind::Quantity { base: inner, .. } => {
                self.list_copy_host(inner)
            }
            _ => self.host.coll.list_copy,
        }
    }

    fn collection_copy_host(&self, ty: &MirType) -> Option<FuncId> {
        match ty.kind() {
            MirTypeKind::Map { .. } => Some(self.host.coll.map_clone),
            MirTypeKind::List(element) | MirTypeKind::FixedList { elem: element, .. } => {
                Some(self.list_copy_host(element))
            }
            MirTypeKind::Tagged { inner, .. } | MirTypeKind::Quantity { base: inner, .. } => {
                self.collection_copy_host(inner)
            }
            _ => None,
        }
    }

    fn copy_value(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        value_id: MirValueId,
    ) -> Result<Value, String> {
        let ty = self.mir_value_type(value_id)?;
        if copy_is_clock_type(&ty) {
            let value = self.cast(builder, self.value(value_id)?, types::I64)?;
            return self
                .call_host(builder, self.host.clock_clone, &[value])?
                .first()
                .copied()
                .ok_or_else(|| {
                    format!(
                        "MIR `{}` clock copy host returned no value",
                        ty.display_name()
                    )
                });
        }
        if copy_needs_typed_clone(&ty) {
            let value = self.cast(builder, self.value(value_id)?, types::I64)?;
            let type_id = runtime_type_id(&ty).ok_or_else(|| {
                format!(
                    "MIR `{}` copy has no runtime type identity",
                    ty.display_name()
                )
            })?;
            let type_id = builder.ins().iconst(types::I64, type_id as i64);
            return self
                .call_host(builder, self.host.typed_clone, &[value, type_id])?
                .first()
                .copied()
                .ok_or_else(|| {
                    format!(
                        "MIR `{}` typed copy host returned no value",
                        ty.display_name()
                    )
                });
        }
        let Some(host) = self.collection_copy_host(&ty) else {
            return self.value(value_id);
        };
        let value = self.cast(builder, self.value(value_id)?, types::I64)?;
        self.call_host(builder, host, &[value])?
            .first()
            .copied()
            .ok_or_else(|| format!("MIR `{}` copy host returned no value", ty.display_name()))
    }
    fn build_string(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        parts: &[MirStringPart],
    ) -> Result<Value, String> {
        let buffer = self
            .call_host(builder, self.host.str_begin, &[])?
            .first()
            .copied()
            .ok_or_else(|| "MIR string host returned no buffer".to_string())?;
        for part in parts {
            match part {
                MirStringPart::Literal(text) => {
                    let literal = builder
                        .ins()
                        .iconst(types::I64, self.runtime.heap.alloc_string(text.clone()));
                    let _ = self.call_host(builder, self.host.str_push_lit, &[buffer, literal])?;
                }
                MirStringPart::Value(id) => {
                    let value = self.value(*id)?;
                    let ty = self.value_type(builder, value);
                    let (host, value) = if ty == types::F64 {
                        (self.host.str_push_f64, value)
                    } else if ty == types::F32 {
                        (
                            self.host.str_push_f64,
                            builder.ins().fpromote(types::F64, value),
                        )
                    } else if ty == types::I8 {
                        (self.host.str_push_bool, value)
                    } else if ty == types::I32 {
                        (self.host.str_push_char, value)
                    } else if ty == types::I64 {
                        (self.host.str_push_str, value)
                    } else {
                        return Err(format!(
                            "MIR string interpolation has unsupported carrier {ty}"
                        ));
                    };
                    let _ = self.call_host(builder, host, &[buffer, value])?;
                }
            }
        }
        Ok(buffer)
    }
    fn project_members(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        base: MirValueId,
        members: &[MirFieldId],
    ) -> Result<Value, String> {
        let count = builder.ins().iconst(types::I64, members.len() as i64);
        let record = self
            .call_host(builder, self.host.struct_new, &[count])?
            .first()
            .copied()
            .ok_or_else(|| "MIR member projection host returned no record".to_string())?;
        for (index, field) in members.iter().enumerate() {
            let (_, ty) = self.field_info(*field)?;
            let value = self.field_load(builder, base, *field, None)?;
            self.set_field(builder, record, index, &ty, value)?;
        }
        Ok(record)
    }

    fn field_row(&self, id: MirFieldId) -> Result<&MirFieldRow, String> {
        self.program
            .fields
            .iter()
            .find(|row| row.id == id)
            .ok_or_else(|| format!("MIR field {:?} is missing", id))
    }

    fn field_info(&self, id: MirFieldId) -> Result<(usize, MirType), String> {
        let row = self.field_row(id)?;
        if let Some(definition) = self
            .program
            .types
            .iter()
            .find(|definition| definition.id == row.owner)
        {
            return match &definition.kind {
                MirTypeDefKind::Struct { fields, .. } => fields
                    .iter()
                    .position(|field| field.id == id)
                    .map(|index| (index, row.field.ty.clone()))
                    .ok_or_else(|| format!("MIR field {:?} is not in type {:?}", id, row.owner)),
                MirTypeDefKind::Enum { variants, .. } => variants
                    .iter()
                    .find_map(|variant| match &variant.payload {
                        jet_foundation::MIR::MirVariantPayload::Named(fields) => fields
                            .iter()
                            .position(|field| field.id == id)
                            .map(|index| (index + 1, row.field.ty.clone())),
                        jet_foundation::MIR::MirVariantPayload::Unit
                        | jet_foundation::MIR::MirVariantPayload::Single(_) => None,
                    })
                    .ok_or_else(|| format!("MIR field {:?} is not in type {:?}", id, row.owner)),
                MirTypeDefKind::Distinct { .. }
                | MirTypeDefKind::Alias { .. }
                | MirTypeDefKind::UnitFamily { .. } => Err(format!(
                    "MIR field {:?} owner {:?} is not aggregate",
                    id, row.owner
                )),
            };
        }
        // Tuple fields have no nominal `MirTypeDef`; the checked tuple instance
        // owns their canonical order and names.
        if let Some(fields) = self
            .program
            .type_instances
            .iter()
            .filter(|ty| ty.identity == Some(row.owner))
            .find_map(|ty| ty.tuple_fields())
        {
            return fields
                .iter()
                .position(|(name, _)| name == &row.field.name)
                .map(|index| (index, row.field.ty.clone()))
                .ok_or_else(|| format!("MIR field {:?} is not in type {:?}", id, row.owner));
        }
        Err(format!(
            "MIR field {:?} owner {:?} is missing",
            id, row.owner
        ))
    }

    fn field_load(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        base: MirValueId,
        field: MirFieldId,
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let base_ty = self.mir_value_type(base)?;
        let event_value = matches!(
            base_ty.kind(),
            MirTypeKind::Apply { name, args }
                if name.name == "FfiCallbackEvent" && args.len() == 1
        ) && self
            .program
            .fields
            .iter()
            .find(|row| row.id == field)
            .is_some_and(|row| row.field.name == "value");
        if event_value {
            let value = self.value(base)?;
            return expected.map_or(Ok(value), |ty| self.cast(builder, value, ty));
        }
        let base = self.value(base)?;
        let base = self.cast(builder, base, types::I64)?;
        self.field_load_value(builder, base, field, expected)
    }

    fn field_load_value(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        base: Value,
        field: MirFieldId,
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let (index, ty) = self.field_info(field)?;
        let base = self.cast(builder, base, types::I64)?;
        let index = builder.ins().iconst(types::I64, index as i64);
        let host = self.field_getter(&ty)?;
        let value = self
            .call_host(builder, host, &[base, index])?
            .first()
            .copied()
            .ok_or_else(|| format!("MIR field {:?} getter returned no value", field))?;
        expected.map_or(Ok(value), |target| self.cast(builder, value, target))
    }

    fn field_address_kind(ty: &MirType) -> Result<i64, String> {
        if matches!(
            comparison_element_kind(ty),
            Some(ComparisonElementKind::String)
        ) {
            return Err("MIR String field has no native address carrier".to_string());
        }
        match ty.layout.abi {
            MirAbi::Scalar(MirScalarKind::Float) => Ok(RECORD_FIELD_ADDRESS_F64),
            MirAbi::Scalar(MirScalarKind::Bool) => Ok(RECORD_FIELD_ADDRESS_BOOL),
            MirAbi::Scalar(MirScalarKind::Char) => Ok(RECORD_FIELD_ADDRESS_CHAR),
            MirAbi::Scalar(MirScalarKind::Float32) => {
                Err("MIR Float32 field has no native address carrier".to_string())
            }
            MirAbi::Scalar(MirScalarKind::Int | MirScalarKind::Pointer)
            | MirAbi::Aggregate
            | MirAbi::Sequence
            | MirAbi::Function
            | MirAbi::Nominal
            | MirAbi::Dynamic => Ok(RECORD_FIELD_ADDRESS_I64),
            MirAbi::Never => Err("MIR never field has no native address carrier".to_string()),
        }
    }

    fn field_address(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        record: Value,
        index: usize,
        ty: &MirType,
    ) -> Result<Value, String> {
        let kind = builder
            .ins()
            .iconst(types::I64, Self::field_address_kind(ty)?);
        let record = self.cast(builder, record, types::I64)?;
        let index = builder.ins().iconst(types::I64, index as i64);
        self.call_host(
            builder,
            self.host.struct_field_address,
            &[record, index, kind],
        )?
        .first()
        .copied()
        .ok_or_else(|| "MIR record field address host returned no address".to_string())
    }

    fn field_getter(&self, ty: &MirType) -> Result<FuncId, String> {
        if matches!(
            comparison_element_kind(ty),
            Some(ComparisonElementKind::String)
        ) {
            return Ok(self.host.struct_get_str);
        }
        match ty.layout.abi {
            MirAbi::Scalar(MirScalarKind::Float | MirScalarKind::Float32) => {
                Ok(self.host.struct_get_f64)
            }
            MirAbi::Scalar(MirScalarKind::Bool) => Ok(self.host.struct_get_bool),
            MirAbi::Scalar(MirScalarKind::Char) => Ok(self.host.struct_get_char),
            MirAbi::Scalar(MirScalarKind::Int | MirScalarKind::Pointer)
            | MirAbi::Aggregate
            | MirAbi::Sequence
            | MirAbi::Function
            | MirAbi::Nominal
            | MirAbi::Dynamic => Ok(self.host.struct_get_i64),
            MirAbi::Never => Err("MIR never field has no carrier".to_string()),
        }
    }

    fn field_setter(&self, ty: &MirType) -> Result<FuncId, String> {
        if matches!(
            comparison_element_kind(ty),
            Some(ComparisonElementKind::String)
        ) {
            return Ok(self.host.struct_set_str);
        }
        match ty.layout.abi {
            MirAbi::Scalar(MirScalarKind::Float | MirScalarKind::Float32) => {
                Ok(self.host.struct_set_f64)
            }
            MirAbi::Scalar(MirScalarKind::Bool) => Ok(self.host.struct_set_bool),
            MirAbi::Scalar(MirScalarKind::Char) => Ok(self.host.struct_set_char),
            MirAbi::Scalar(MirScalarKind::Int | MirScalarKind::Pointer)
            | MirAbi::Aggregate
            | MirAbi::Sequence
            | MirAbi::Function
            | MirAbi::Nominal
            | MirAbi::Dynamic => Ok(self.host.struct_set_i64),
            MirAbi::Never => Err("MIR never field has no carrier".to_string()),
        }
    }

    fn set_field(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        record: Value,
        index: usize,
        ty: &MirType,
        value: Value,
    ) -> Result<(), String> {
        let index = builder.ins().iconst(types::I64, index as i64);
        let host = self.field_setter(ty)?;
        let target = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .params
            .get(2)
            .map(|parameter| parameter.value_type)
            .ok_or_else(|| "MIR field setter has no value parameter".to_string())?;
        let value = self.cast(builder, value, target)?;
        let _ = self.call_host(builder, host, &[record, index, value])?;
        Ok(())
    }

    fn aggregate(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        type_id: jet_foundation::MIR::MirTypeId,
        fields: &[(MirFieldId, MirValueId)],
    ) -> Result<Value, String> {
        let arity = self.aggregate_arity(type_id, fields.len())?;
        let arity_value = builder.ins().iconst(types::I64, arity as i64);
        let record = self
            .call_host(builder, self.host.struct_new, &[arity_value])?
            .first()
            .copied()
            .ok_or_else(|| "MIR aggregate host returned no record".to_string())?;
        for (field, value) in fields {
            let (index, ty) = self.field_info(*field)?;
            let value = self.value(*value)?;
            self.set_field(builder, record, index, &ty, value)?;
        }
        let type_id_value = builder.ins().iconst(types::I64, type_id.0 as i64);
        let _ = self.call_host(
            builder,
            self.host.trait_object_tag,
            &[record, type_id_value],
        )?;
        Ok(record)
    }

    fn aggregate_arity(
        &self,
        type_id: jet_foundation::MIR::MirTypeId,
        supplied: usize,
    ) -> Result<usize, String> {
        if let Some(fields) = self
            .program
            .type_instances
            .iter()
            .find(|instance| instance.identity == Some(type_id))
            .and_then(|instance| instance.tuple_fields())
        {
            return Ok(fields.len());
        }

        let definition = self
            .program
            .types
            .iter()
            .find(|definition| definition.id == type_id)
            .ok_or_else(|| format!("MIR aggregate type {:?} is missing", type_id))?;
        Ok(match &definition.kind {
            MirTypeDefKind::Struct { fields, .. } => fields.len(),
            MirTypeDefKind::Enum { variants, .. } => variants
                .iter()
                .map(|variant| match &variant.payload {
                    jet_foundation::MIR::MirVariantPayload::Unit => 1,
                    jet_foundation::MIR::MirVariantPayload::Single(_) => 2,
                    jet_foundation::MIR::MirVariantPayload::Named(fields) => fields.len() + 1,
                })
                .max()
                .unwrap_or(1),
            MirTypeDefKind::Distinct { .. }
            | MirTypeDefKind::Alias { .. }
            | MirTypeDefKind::UnitFamily { .. } => supplied,
        })
    }

    fn enum_value(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        type_id: jet_foundation::MIR::MirTypeId,
        variant_name: &str,
        args: &[jet_foundation::MIR::MirEnumArg],
    ) -> Result<Value, String> {
        let definition = self
            .program
            .types
            .iter()
            .find(|definition| definition.id == type_id)
            .ok_or_else(|| format!("MIR enum type {:?} is missing", type_id))?;
        let is_ordering = is_ordering_name(&definition.key);
        let jet_foundation::MIR::MirTypeDefKind::Enum { variants, .. } = &definition.kind else {
            return Err(format!("MIR type {:?} is not an enum", type_id));
        };
        let (discriminant, payload) = variants
            .iter()
            .enumerate()
            .find(|(_, variant)| variant.name == variant_name)
            .map(|(index, variant)| {
                let discriminant = variant.discriminant.unwrap_or(index as i64);
                (discriminant, variant.payload.clone())
            })
            .ok_or_else(|| format!("MIR enum variant `{variant_name}` is missing"))?;
        let payload_fields = match payload {
            jet_foundation::MIR::MirVariantPayload::Unit => Vec::new(),
            jet_foundation::MIR::MirVariantPayload::Single(ty) => vec![(None, ty)],
            jet_foundation::MIR::MirVariantPayload::Named(fields) => fields
                .into_iter()
                .map(|field| (Some(field.id), field.ty))
                .collect(),
        };
        if payload_fields.len() != args.len() {
            return Err(format!(
                "MIR enum `{variant_name}` expects {} arguments, got {}",
                payload_fields.len(),
                args.len()
            ));
        }
        if is_ordering {
            let discriminant =
                prelude_enum_variant_index(jet_foundation::Syntax::TYPE_ORDERING, variant_name)
                    .ok_or_else(|| {
                        format!("Prelude Ordering variant `{variant_name}` is missing metadata")
                    })?;
            return Ok(builder.ins().iconst(types::I64, discriminant));
        }
        let arity = args.len() + 1;
        let arity_value = builder.ins().iconst(types::I64, arity as i64);
        let record = self
            .call_host(builder, self.host.struct_new, &[arity_value])?
            .first()
            .copied()
            .ok_or_else(|| "MIR enum host returned no record".to_string())?;
        let discriminant_value = builder.ins().iconst(types::I64, discriminant);
        let discriminant_index = builder.ins().iconst(types::I64, 0);
        let _ = self.call_host(
            builder,
            self.host.struct_set_i64,
            &[record, discriminant_index, discriminant_value],
        )?;
        for (index, (arg, (field, ty))) in args.iter().zip(payload_fields.iter()).enumerate() {
            if arg.field != *field {
                return Err(format!(
                    "MIR enum `{variant_name}` payload argument {index} is out of canonical order"
                ));
            }
            if arg.boxed && matches!(ty.layout.abi, MirAbi::Scalar(_) | MirAbi::Never) {
                return Err(format!(
                    "MIR enum `{variant_name}` marks scalar payload argument {index} as boxed"
                ));
            }
            // Heap carriers already provide the recursive indirection that
            // Rust spells as Box<T>; do not allocate a second wrapper record.
            let mut value = self.value(arg.value)?;
            if index == 0
                && variant_name == "Object"
                && is_core_data_type_name(&definition.key)
                && matches!(ty.kind(), MirTypeKind::Map { .. })
            {
                value = self
                    .call_host(builder, self.host.encoding.object_from_map, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| {
                        "MIR DataTree object map adapter returned no value".to_string()
                    })?;
            }
            self.set_field(builder, record, index + 1, ty, value)?;
        }
        let type_id_value = builder.ins().iconst(types::I64, type_id.0 as i64);
        let _ = self.call_host(
            builder,
            self.host.trait_object_tag,
            &[record, type_id_value],
        )?;
        Ok(record)
    }
    fn build_map(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        entries: &[(MirValueId, MirValueId)],
    ) -> Result<Value, String> {
        let map = self
            .call_host(builder, self.host.coll.map_new, &[])?
            .first()
            .copied()
            .ok_or_else(|| "MIR map host returned no value".to_string())?;
        for (key, value) in entries {
            let key = self.cast(builder, self.value(*key)?, types::I64)?;
            let value = self.cast(builder, self.value(*value)?, types::I64)?;
            let _ = self.call_host(builder, self.host.coll.map_insert, &[map, key, value])?;
        }
        Ok(map)
    }

    fn result_value(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        ok: bool,
        value: Value,
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let tag = builder.ins().iconst(types::I8, i64::from(ok));
        let value_type = self.value_type(builder, value);
        let result = if value_type == types::F64 {
            self.call_host(builder, self.host.result_new_f64, &[tag, value])?
        } else if value_type == types::F32 {
            let value = builder.ins().fpromote(types::F64, value);
            self.call_host(builder, self.host.result_new_f64, &[tag, value])?
        } else if value_type == types::I8 {
            self.call_host(builder, self.host.result_new_i8, &[tag, value])?
        } else if value_type == types::I32 {
            self.call_host(builder, self.host.result_new_i32, &[tag, value])?
        } else if value_type == types::I64 {
            self.call_host(builder, self.host.result_new_i64, &[tag, value])?
        } else {
            return Err(format!(
                "MIR result payload has unsupported carrier {value_type}"
            ));
        };
        let result = result
            .first()
            .copied()
            .ok_or_else(|| "MIR result host returned no value".to_string())?;
        expected.map_or(Ok(result), |ty| self.cast(builder, result, ty))
    }
    fn result_absent(&mut self, builder: &mut FunctionBuilder<'_>) -> Result<Value, String> {
        let absent = builder.ins().iconst(types::I8, 0);
        let zero = builder.ins().iconst(types::I64, 0);
        self.call_host(builder, self.host.result_new_i64, &[absent, zero])?
            .first()
            .copied()
            .ok_or_else(|| "MIR absent result constructor returned no value".to_string())
    }
    fn indirect_call(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        callee: MirValueId,
        args: &[MirCallArg],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let callee_type = self.mir_value_type(callee)?;
        let (params, ret) = callable_signature(&callee_type)
            .ok_or_else(|| format!("MIR indirect callee {:?} has no function signature", callee))?;
        if params.len() != args.len() {
            return Err(format!(
                "MIR indirect call expects {} arguments, got {}",
                params.len(),
                args.len()
            ));
        }
        // Function-value parameter and result carriers are fixed by checked
        // MIR. The function type is only projected into Cranelift ABI types;
        // no source-level callable semantics are rediscovered here.
        let parameter_types = params
            .iter()
            .zip(args)
            .map(|(param, arg)| {
                if arg.access == MirAccess::Write {
                    Ok(types::I64)
                } else {
                    clif_ty_from_mir(param)
                        .ok_or_else(|| "MIR indirect parameter has no ABI".to_string())
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let return_type = ret
            .map(|ret| {
                clif_ty_from_mir(ret).ok_or_else(|| "MIR indirect return has no ABI".to_string())
            })
            .transpose()?;
        let mut signature = Signature::new(self.module.target_config().default_call_conv);
        signature
            .params
            .extend(parameter_types.iter().copied().map(AbiParam::new));
        if let Some(ty) = return_type {
            signature.returns.push(AbiParam::new(ty));
        }
        let values = self.lower_call_args(builder, args, &signature)?;
        let handle = self.cast(builder, self.value(callee)?, types::I64)?;
        let result = self.call_callable_values(builder, handle, signature, values, return_type)?;
        expected.map_or(Ok(result), |target| self.cast(builder, result, target))
    }

    /// Invoke a checked callable handle with already-lowered argument values:
    /// normalize the handle, then dispatch on its environment flag exactly as
    /// `MirOperation::IndirectCall` does. `signature` is the plain (env-less)
    /// Cranelift signature the callee type projects to.
    fn call_callable_values(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        handle: Value,
        signature: Signature,
        values: Vec<Value>,
        return_type: Option<types::Type>,
    ) -> Result<Value, String> {
        let normalized = self
            .call_host(builder, self.host.callable_normalize, &[handle])?
            .first()
            .copied()
            .ok_or_else(|| "MIR callable normalizer returned no value".to_string())?;
        let fn_ptr = self
            .call_host(builder, self.host.callable_fn, &[normalized])?
            .first()
            .copied()
            .ok_or_else(|| "MIR callable function getter returned no value".to_string())?;
        let has_env = self
            .call_host(builder, self.host.callable_has_env, &[normalized])?
            .first()
            .copied()
            .ok_or_else(|| "MIR callable environment flag returned no value".to_string())?;
        let has_env = self.bool_value(builder, has_env)?;
        let env_block = builder.create_block();
        let plain_block = builder.create_block();
        let merge_block = builder.create_block();
        if let Some(return_type) = return_type {
            builder.append_block_param(merge_block, return_type);
        }
        builder
            .ins()
            .brif(has_env, env_block, &[], plain_block, &[]);

        builder.switch_to_block(env_block);
        let env = self
            .call_host(builder, self.host.callable_env, &[normalized])?
            .first()
            .copied()
            .ok_or_else(|| "MIR callable environment getter returned no value".to_string())?;
        let mut env_signature = signature.clone();
        env_signature.params.insert(0, AbiParam::new(types::I64));
        let env_signature_ref = builder.import_signature(env_signature);
        let mut env_values = Vec::with_capacity(values.len() + 1);
        env_values.push(env);
        env_values.extend(values.iter().copied());
        let env_call = builder
            .ins()
            .call_indirect(env_signature_ref, fn_ptr, &env_values);
        if return_type.is_some() {
            let env_result = builder
                .inst_results(env_call)
                .first()
                .copied()
                .ok_or_else(|| "MIR captured callable returned no value".to_string())?;
            builder.ins().jump(merge_block, &[env_result]);
        } else {
            builder.ins().jump(merge_block, &[]);
        }

        builder.switch_to_block(plain_block);
        let signature_ref = builder.import_signature(signature);
        let plain_call = builder.ins().call_indirect(signature_ref, fn_ptr, &values);
        if return_type.is_some() {
            let plain_result = builder
                .inst_results(plain_call)
                .first()
                .copied()
                .ok_or_else(|| "MIR callable returned no value".to_string())?;
            builder.ins().jump(merge_block, &[plain_result]);
        } else {
            builder.ins().jump(merge_block, &[]);
        }

        builder.switch_to_block(merge_block);
        self.emit_pending_exit_check(builder);
        if return_type.is_some() {
            builder
                .block_params(merge_block)
                .first()
                .copied()
                .ok_or_else(|| "MIR indirect call merge block has no result".to_string())
        } else {
            Ok(builder.ins().iconst(types::I64, 0))
        }
    }

    fn app_plain_method_call(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        call: jet_foundation::MIR::MirPreludeCallId,
        receiver: MirValueId,
        args: &[MirValueId],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let row = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .ok_or_else(|| format!("MIR App call {:?} is missing", call))?;
        let zero = builder.ins().iconst(types::I64, 0);
        let app = self.value(receiver)?;
        let method = builder.ins().iconst(
            types::I64,
            self.runtime.heap.alloc_string(row.member.clone()),
        );
        let a0 = args
            .first()
            .map(|value| self.value(*value))
            .unwrap_or_else(|| Ok(zero))?;
        let a1 = args
            .get(1)
            .map(|value| self.value(*value))
            .unwrap_or_else(|| Ok(zero))?;
        let a2 = args
            .get(2)
            .map(|value| self.value(*value))
            .unwrap_or_else(|| Ok(zero))?;
        let host = self
            .host
            .lookup("jet_jit_web_app_method")
            .ok_or_else(|| "JIT App method host is not registered".to_string())?;
        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        let result = self
            .call_declared_values(
                builder,
                host,
                &signature,
                vec![app, method, a0, a1, a2, zero, zero],
            )?
            .first()
            .copied()
            .ok_or_else(|| "JIT App method host returned no value".to_string())?;
        expected.map_or(Ok(result), |ty| self.cast(builder, result, ty))
    }

    fn app_callback_call(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        call: jet_foundation::MIR::MirPreludeCallId,
        receiver: MirValueId,
        args: &[MirValueId],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let row = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .ok_or_else(|| format!("MIR App call {:?} is missing", call))?;
        let Some((callback_index, _)) = app_callback_shape(row) else {
            return Err(format!(
                "MIR App call `{}` has no callback shape",
                row.member
            ));
        };
        let callback = args
            .get(callback_index)
            .ok_or_else(|| format!("MIR App `{}` route has no callback argument", row.member))?;
        let thunk = self
            .app_thunks
            .get(&(self.function.id, *callback))
            .ok_or_else(|| {
                format!(
                    "MIR App callback {:?} has no generated universal thunk",
                    callback
                )
            })?;
        let callback = self.cast(builder, self.value(*callback)?, types::I64)?;
        let thunk_ref = self.module.declare_func_in_func(thunk.id, builder.func);
        let thunk_ptr = builder.ins().func_addr(types::I64, thunk_ref);
        let has_env = builder.ins().iconst(types::I8, 1);
        let bound = self
            .call_host(
                builder,
                self.host.callable_bind,
                &[thunk_ptr, callback, has_env],
            )?
            .first()
            .copied()
            .ok_or_else(|| "MIR App callback binder returned no value".to_string())?;
        let zero = builder.ins().iconst(types::I64, 0);
        let _ = self.call_host(
            builder,
            self.host.callable_bind_raw,
            &[bound, thunk_ptr, zero],
        )?;
        let app = self.value(receiver)?;
        let method = builder.ins().iconst(
            types::I64,
            self.runtime.heap.alloc_string(row.member.clone()),
        );
        let a0 = args
            .first()
            .map(|value| self.value(*value))
            .unwrap_or_else(|| Ok(zero))?;
        let a1 = if callback_index == 0 { zero } else { bound };
        let a2 = if matches!(row.member.as_str(), "action" | "form" | "data") {
            builder.ins().iconst(types::I64, thunk.arity as i64)
        } else if row.member == "loader" && args.len() >= 4 {
            args.get(2)
                .map(|value| self.value(*value))
                .unwrap_or_else(|| Ok(zero))?
        } else {
            zero
        };
        let binding = if matches!(row.member.as_str(), "route" | "page" | "layout" | "loader") {
            args.last()
                .map(|value| self.value(*value))
                .unwrap_or_else(|| Ok(zero))?
        } else {
            zero
        };
        let host = self
            .host
            .lookup("jet_jit_web_app_method")
            .ok_or_else(|| "JIT App method host is not registered".to_string())?;
        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        let output_type = builder.ins().iconst(types::I64, thunk.output_type);
        let result = self
            .call_declared_values(
                builder,
                host,
                &signature,
                vec![app, method, a0, a1, a2, output_type, binding],
            )?
            .first()
            .copied()
            .ok_or_else(|| "JIT App method host returned no value".to_string())?;
        expected.map_or(Ok(result), |ty| self.cast(builder, result, ty))
    }

    fn checked_sort_by_callback_call(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        receiver: MirValueId,
        args: &[MirCallArg],
        expected: Option<types::Type>,
        descending: bool,
        fallible: bool,
    ) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "MIR sort_by callback route expects one explicit argument, got {}",
                args.len()
            ));
        }
        let callback_arg = &args[0];
        let callback_ty = self.mir_value_type(callback_arg.value)?;
        let callback_result = callback_return_type(&callback_ty)
            .ok_or_else(|| "MIR sort_by callback has no checked result type".to_string())?;
        let callback_ret = if fallible {
            result_ok_type(callback_result).ok_or_else(|| {
                "MIR try_sort_by callback result is not a checked Result".to_string()
            })?
        } else {
            callback_result
        };
        let key_kind = self.map_key_kind_for_type(callback_ret)?;
        let sort_host_symbol = match (descending, key_kind) {
            (false, MapKeyKind::Int) => "jet_jit_list_sort_by_i64_keys",
            (true, MapKeyKind::Int) => "jet_jit_list_sort_by_i64_keys_desc",
            (false, MapKeyKind::String) => "jet_jit_list_sort_by_str_keys",
            (true, MapKeyKind::String) => "jet_jit_list_sort_by_str_keys_desc",
            (_, MapKeyKind::Composite) => {
                return Err(
                    "MIR sort_by callback result has no registered JIT key carrier".to_string(),
                )
            }
        };
        let receiver_ty = self.mir_value_type(receiver)?;
        let element_ty = sequence_element_type(&receiver_ty).ok_or_else(|| {
            "MIR sort_by callback receiver has no checked element type".to_string()
        })?;
        let element_id = runtime_type_id(element_ty).ok_or_else(|| {
            format!(
                "MIR sort_by element type `{}` has no runtime identity",
                element_ty.display_name()
            )
        })?;
        let result_id = runtime_type_id(callback_ret).ok_or_else(|| {
            format!(
                "MIR sort_by key type `{}` has no runtime identity",
                callback_ret.display_name()
            )
        })?;
        let bound = self.bind_universal_callback(builder, callback_arg.value, 1)?;
        let map_host_symbol = if fallible {
            "jet_jit_checked_try_map"
        } else {
            "jet_jit_checked_collection_map"
        };
        let map_host = self
            .host
            .lookup(map_host_symbol)
            .ok_or_else(|| format!("JIT collection host `{map_host_symbol}` is not registered"))?;
        let map_signature = self
            .module
            .declarations()
            .get_function_decl(map_host)
            .signature
            .clone();
        let receiver_value = self.value(receiver)?;
        let element_id = builder.ins().iconst(types::I64, element_id as i64);
        let result_id = builder.ins().iconst(types::I64, result_id as i64);
        let mapped = self
            .call_declared_values(
                builder,
                map_host,
                &map_signature,
                vec![receiver_value, bound, element_id, result_id],
            )?
            .first()
            .copied()
            .ok_or_else(|| "JIT sort_by key map returned no value".to_string())?;
        let sort_host = self
            .host
            .lookup(sort_host_symbol)
            .ok_or_else(|| format!("JIT collection host `{sort_host_symbol}` is not registered"))?;
        let sort_signature = self
            .module
            .declarations()
            .get_function_decl(sort_host)
            .signature
            .clone();
        if !fallible {
            let _ = self.call_declared_values(
                builder,
                sort_host,
                &sort_signature,
                vec![receiver_value, mapped],
            )?;
            let unit = builder.ins().iconst(types::I64, 0);
            return expected.map_or(Ok(unit), |ty| self.cast(builder, unit, ty));
        }

        let ok = self
            .call_host(builder, self.host.result_is_ok, &[mapped])?
            .first()
            .copied()
            .ok_or_else(|| "JIT try_sort_by result discriminator returned no value".to_string())?;
        let ok = self.bool_value(builder, ok)?;
        let success_block = builder.create_block();
        let failure_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, types::I64);
        builder
            .ins()
            .brif(ok, success_block, &[], failure_block, &[]);

        builder.switch_to_block(success_block);
        let keys = self
            .call_host(builder, self.host.result_get_i64, &[mapped])?
            .first()
            .copied()
            .ok_or_else(|| "JIT try_sort_by result payload getter returned no value".to_string())?;
        let _ = self.call_declared_values(
            builder,
            sort_host,
            &sort_signature,
            vec![receiver_value, keys],
        )?;
        let ok_tag = builder.ins().iconst(types::I8, 1);
        let unit = builder.ins().iconst(types::I64, 0);
        let success = self
            .call_host(builder, self.host.result_new_i64, &[ok_tag, unit])?
            .first()
            .copied()
            .ok_or_else(|| {
                "JIT try_sort_by success result constructor returned no value".to_string()
            })?;
        builder.ins().jump(merge_block, &[success]);

        builder.switch_to_block(failure_block);
        builder.ins().jump(merge_block, &[mapped]);

        builder.switch_to_block(merge_block);
        let result = builder
            .block_params(merge_block)
            .first()
            .copied()
            .ok_or_else(|| "JIT try_sort_by result merge has no value".to_string())?;
        expected.map_or(Ok(result), |ty| self.cast(builder, result, ty))
    }

    fn checked_partition_callback_call(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        receiver: MirValueId,
        args: &[MirCallArg],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "MIR partition callback route expects one explicit argument, got {}",
                args.len()
            ));
        }
        let callback_arg = &args[0];
        let callback_ty = self.mir_value_type(callback_arg.value)?;
        let callback_ret = callback_return_type(&callback_ty)
            .ok_or_else(|| "MIR partition callback has no checked result type".to_string())?;
        if !callback_ret.is_bool() {
            return Err("MIR partition callback must return checked Bool".to_string());
        }
        let receiver_ty = self.mir_value_type(receiver)?;
        let element_ty = sequence_element_type(&receiver_ty).ok_or_else(|| {
            "MIR partition callback receiver has no checked element type".to_string()
        })?;
        let element_id = runtime_type_id(element_ty).ok_or_else(|| {
            format!(
                "MIR partition element type `{}` has no runtime identity",
                element_ty.display_name()
            )
        })?;
        let bound = self.bind_universal_callback(builder, callback_arg.value, 1)?;
        let host = self
            .host
            .lookup("jet_jit_checked_collection_partition")
            .ok_or_else(|| {
                "JIT collection host `jet_jit_checked_collection_partition` is not registered"
                    .to_string()
            })?;
        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        let receiver_value = self.value(receiver)?;
        let element_id = builder.ins().iconst(types::I64, element_id as i64);
        let result = self
            .call_declared_values(
                builder,
                host,
                &signature,
                vec![receiver_value, bound, element_id],
            )?
            .first()
            .copied()
            .ok_or_else(|| "JIT collection partition host returned no value".to_string())?;
        expected.map_or(Ok(result), |ty| self.cast(builder, result, ty))
    }

    fn checked_collection_callback_call(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        call: jet_foundation::MIR::MirPreludeCallId,
        receiver: MirValueId,
        args: &[MirCallArg],
        expected: Option<types::Type>,
    ) -> Result<Option<Value>, String> {
        let row = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .ok_or_else(|| format!("MIR collection callback {:?} is missing", call))?;
        if row.family == jet_foundation::MIR::MirPreludeFamily::ClosureMethod
            && row.module == "core.list"
            && matches!(
                row.member.as_str(),
                "sort_by" | "sort_by_desc" | "try_sort_by" | "try_sort_by_desc"
            )
        {
            let descending = row.member.ends_with("_desc");
            let fallible = row.member.starts_with("try_");
            return self
                .checked_sort_by_callback_call(
                    builder, receiver, args, expected, descending, fallible,
                )
                .map(Some);
        }
        if row.family == jet_foundation::MIR::MirPreludeFamily::ClosureMethod
            && matches!(row.module.as_str(), "core.list" | "core.iter")
            && row.member == "partition"
        {
            return self
                .checked_partition_callback_call(builder, receiver, args, expected)
                .map(Some);
        }
        if row.family == jet_foundation::MIR::MirPreludeFamily::ClosureMethod
            && row.module == "core.iter"
            && matches!(row.member.as_str(), "zip" | "zip_strict" | "zip_pad")
        {
            let (callback_index, callback_arity, expected_args, host_symbol) =
                match row.member.as_str() {
                    "zip" => (1, 2, 2, "jet_jit_checked_iter_zip"),
                    "zip_strict" => (1, 2, 2, "jet_jit_checked_iter_zip_strict"),
                    "zip_pad" => (3, 2, 4, "jet_jit_checked_iter_zip_pad"),
                    _ => unreachable!("zip route member checked above"),
                };
            if args.len() != expected_args {
                return Err(format!(
                    "MIR {} callback route expects {} explicit arguments, got {}",
                    row.member,
                    expected_args,
                    args.len()
                ));
            }
            if args.iter().any(|arg| arg.access == MirAccess::Write) {
                return Err(format!(
                    "MIR {} callback route requires read or move operands",
                    row.member
                ));
            }
            let callback_arg = args
                .get(callback_index)
                .ok_or_else(|| format!("MIR {} callback argument is missing", row.member))?;
            let callback_ty = self.mir_value_type(callback_arg.value)?;
            callback_return_type(&callback_ty)
                .ok_or_else(|| format!("MIR {} callback has no checked result type", row.member))?;
            let receiver_ty = self.mir_value_type(receiver)?;
            sequence_element_type(&receiver_ty).ok_or_else(|| {
                format!("MIR {} receiver has no checked element type", row.member)
            })?;
            let right_ty = self.mir_value_type(
                args.first()
                    .ok_or_else(|| {
                        format!("MIR {} right sequence argument is missing", row.member)
                    })?
                    .value,
            )?;
            sequence_element_type(&right_ty).ok_or_else(|| {
                format!(
                    "MIR {} right argument has no checked element type",
                    row.member
                )
            })?;
            let bound =
                self.bind_universal_callback(builder, callback_arg.value, callback_arity)?;
            let mut values = Vec::with_capacity(args.len() + 1);
            values.push(self.value(receiver)?);
            for (index, arg) in args.iter().enumerate() {
                values.push(if index == callback_index {
                    bound
                } else {
                    self.value(arg.value)?
                });
            }
            let host = self
                .host
                .lookup(host_symbol)
                .ok_or_else(|| format!("JIT collection host `{host_symbol}` is not registered"))?;
            let signature = self
                .module
                .declarations()
                .get_function_decl(host)
                .signature
                .clone();
            let result = self
                .call_declared_values(builder, host, &signature, values)?
                .first()
                .copied()
                .ok_or_else(|| format!("JIT collection host `{host_symbol}` returned no value"))?;
            return expected
                .map_or(Ok(result), |ty| self.cast(builder, result, ty))
                .map(Some);
        }
        if row.family != jet_foundation::MIR::MirPreludeFamily::ClosureMethod
            || !matches!(row.module.as_str(), "core.list" | "core.iter")
            || !matches!(
                row.member.as_str(),
                "map"
                    | "filter"
                    | "filter_map"
                    | "try_map"
                    | "try_filter"
                    | "flat_map"
                    | "take_while"
                    | "skip_while"
                    | "scan"
                    | "each"
            )
        {
            return Ok(None);
        }

        let member = row.member.as_str();
        let (callback_index, callback_arity, expected_args) = if member == "scan" {
            (1, 2, 2)
        } else {
            (0, 1, 1)
        };
        if args.len() != expected_args {
            return Err(format!(
                "MIR {} callback route expects {} explicit arguments, got {}",
                member,
                expected_args,
                args.len()
            ));
        }
        let callback_arg = args
            .get(callback_index)
            .ok_or_else(|| format!("MIR {member} callback argument is missing"))?;
        let callback_ty = self.mir_value_type(callback_arg.value)?;
        let callback_ret = callback_return_type(&callback_ty);
        let receiver_ty = self.mir_value_type(receiver)?;
        let element_ty = sequence_element_type(&receiver_ty)
            .ok_or_else(|| format!("MIR {member} callback receiver has no checked element type"))?;
        let is_iter = row.module == "core.iter";
        let list_result_iter = row.module == "core.list" && row.symbol.name().ends_with("_iter");
        let lazy = is_iter || list_result_iter;

        let (host_symbol, descriptor_types): (&str, Vec<MirType>) = match member {
            "each" if row.symbol.name() == "jet_list_each_ref" => {
                let error = callback_ret
                    .and_then(|ty| match ty.kind() {
                        MirTypeKind::Result { ok, err } if ok.is_unit() => Some(err.as_ref()),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        "MIR each_ref callback result is not a checked Result<Unit, E>".to_string()
                    })?;
                ("jet_list_each_ref", vec![element_ty.clone(), error.clone()])
            }
            "map" if lazy => ("jet_jit_checked_iter_map", Vec::new()),
            "map" => {
                let result = callback_ret
                    .ok_or_else(|| "MIR List map callback has no checked result type")?;
                (
                    "jet_jit_checked_collection_map",
                    vec![element_ty.clone(), result.clone()],
                )
            }
            "filter" if lazy => ("jet_jit_checked_iter_filter", Vec::new()),
            "filter" => (
                "jet_jit_checked_collection_filter",
                vec![element_ty.clone()],
            ),
            "filter_map" if lazy => ("jet_jit_checked_iter_filter_map", Vec::new()),
            "try_map" => {
                let result = callback_ret.and_then(result_ok_type).ok_or_else(|| {
                    "MIR checked try_map callback result is not a checked Result".to_string()
                })?;
                (
                    "jet_jit_checked_try_map",
                    vec![element_ty.clone(), result.clone()],
                )
            }
            "try_filter" => {
                let result = callback_ret.and_then(result_ok_type).ok_or_else(|| {
                    "MIR checked try_filter callback result is not a checked Result".to_string()
                })?;
                (
                    "jet_jit_checked_try_filter",
                    vec![element_ty.clone(), result.clone()],
                )
            }
            "flat_map" if lazy => ("jet_jit_checked_iter_flat_map", Vec::new()),
            "flat_map" => {
                let result = callback_ret
                    .and_then(sequence_element_type)
                    .ok_or_else(|| {
                        "MIR List flat_map callback has no checked sequence result type".to_string()
                    })?;
                (
                    "jet_jit_checked_list_flat_map",
                    vec![element_ty.clone(), result.clone()],
                )
            }
            "take_while" if lazy => ("jet_jit_checked_iter_take_while", Vec::new()),
            "take_while" => ("jet_jit_checked_list_take_while", vec![element_ty.clone()]),
            "skip_while" if lazy => ("jet_jit_checked_iter_skip_while", Vec::new()),
            "skip_while" => ("jet_jit_checked_list_skip_while", vec![element_ty.clone()]),
            "scan" => {
                let accumulator = self.mir_value_type(
                    args.first()
                        .ok_or_else(|| "MIR scan accumulator argument is missing".to_string())?
                        .value,
                )?;
                (
                    "jet_jit_checked_iter_scan",
                    vec![element_ty.clone(), accumulator],
                )
            }
            _ => return Ok(None),
        };

        let bound = self.bind_universal_callback(builder, callback_arg.value, callback_arity)?;
        let mut values = Vec::with_capacity(args.len() + 1 + descriptor_types.len());
        values.push(self.value(receiver)?);
        for (index, arg) in args.iter().enumerate() {
            if index == callback_index {
                values.push(bound);
            } else if arg.access == MirAccess::Write {
                let place = arg.place.ok_or_else(|| {
                    "MIR collection write argument has no checked place".to_string()
                })?;
                values.push(self.address_of(builder, place)?);
            } else {
                values.push(self.value(arg.value)?);
            }
        }
        for ty in &descriptor_types {
            let id = runtime_type_id(ty).ok_or_else(|| {
                format!(
                    "MIR collection type `{}` has no runtime identity",
                    ty.display_name()
                )
            })?;
            values.push(builder.ins().iconst(types::I64, id as i64));
        }

        let host = self
            .host
            .lookup(host_symbol)
            .ok_or_else(|| format!("JIT collection host `{host_symbol}` is not registered"))?;
        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        let result = self
            .call_declared_values(builder, host, &signature, values)?
            .first()
            .copied()
            .ok_or_else(|| format!("JIT collection host `{host_symbol}` returned no value"))?;
        expected
            .map_or(Ok(result), |ty| self.cast(builder, result, ty))
            .map(Some)
    }

    fn iterator_builtin_call(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        call: jet_foundation::MIR::MirPreludeCallId,
        receiver: MirValueId,
        args: &[MirValueId],
        expected: Option<types::Type>,
    ) -> Result<Option<Value>, String> {
        let row = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .ok_or_else(|| format!("MIR iterator builtin {:?} is missing", call))?;
        if row.family != jet_foundation::MIR::MirPreludeFamily::BuiltinMethod
            || row.module != "core.builtin"
            || !matches!(
                row.member.as_str(),
                "iter_to_list" | "iter_collect" | "list_lazy" | "iter_dedup"
            )
        {
            return Ok(None);
        }
        if !args.is_empty() {
            return Err(format!(
                "MIR iterator builtin `{}` expects no explicit arguments, got {}",
                row.member,
                args.len()
            ));
        }
        let host_symbol = row.symbol.name().to_owned();
        let host = self.host.lookup(&host_symbol).ok_or_else(|| {
            format!("JIT iterator builtin host `{host_symbol}` is not registered")
        })?;
        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        let receiver_id = receiver;
        let receiver_value = self.value(receiver_id)?;
        let mut values = vec![receiver_value];
        if row.member != "list_lazy" {
            let receiver_ty = self.mir_value_type(receiver_id)?;
            let element_ty = sequence_element_type(&receiver_ty).ok_or_else(|| {
                format!(
                    "MIR iterator builtin `{}` receiver has no checked element type",
                    row.member
                )
            })?;
            let element_id = runtime_type_id(element_ty).ok_or_else(|| {
                format!(
                    "MIR iterator builtin `{}` element has no runtime identity",
                    row.member
                )
            })?;
            values.push(builder.ins().iconst(types::I64, element_id as i64));
        }
        let result = self
            .call_declared_values(builder, host, &signature, values)?
            .first()
            .copied()
            .ok_or_else(|| format!("JIT iterator builtin `{host_symbol}` returned no value"))?;
        expected
            .map_or(Ok(result), |ty| self.cast(builder, result, ty))
            .map(Some)
    }

    fn view_callback_call(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        call: jet_foundation::MIR::MirPreludeCallId,
        receiver: MirValueId,
        args: &[MirCallArg],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let row = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .ok_or_else(|| format!("MIR View call {:?} is missing", call))?;
        let (pair, callback_index, expected_args) = view_callback_shape(row)
            .ok_or_else(|| format!("MIR call {:?} is not a View callback route", call))?;
        if args.len() != expected_args {
            return Err(format!(
                "MIR View callback route expects {} explicit arguments, got {}",
                expected_args,
                args.len()
            ));
        }
        let callback_arg = args
            .get(callback_index)
            .ok_or_else(|| "MIR View callback argument is missing".to_string())?;
        let bound =
            self.bind_universal_callback(builder, callback_arg.value, if pair { 2 } else { 1 })?;
        let mut values = Vec::with_capacity(args.len() + 3);
        values.push(self.value(receiver)?);
        for (index, arg) in args.iter().enumerate() {
            if index == callback_index {
                values.push(bound);
            } else if arg.access == MirAccess::Write {
                let place = arg
                    .place
                    .ok_or_else(|| "MIR View write argument has no checked place".to_string())?;
                values.push(self.address_of(builder, place)?);
            } else {
                values.push(self.value(arg.value)?);
            }
        }
        let receiver_ty = self.mir_value_type(receiver)?;
        let element_ty = sequence_element_type(&receiver_ty)
            .ok_or_else(|| "MIR View callback receiver has no checked element type".to_string())?;
        let callback_ty = self.mir_value_type(callback_arg.value)?;
        let callback_ret = callback_return_type(&callback_ty);
        let result_ty = match row.member.as_str() {
            "map" => callback_ret
                .ok_or_else(|| "MIR View map callback has no checked result type".to_string())?
                .clone(),
            "try_map" => {
                let ret = callback_ret.ok_or_else(|| {
                    "MIR View try_map callback has no checked result type".to_string()
                })?;
                result_ok_type(ret)
                    .ok_or_else(|| {
                        "MIR View try_map callback result is not a checked Result".to_string()
                    })?
                    .clone()
            }
            "try_filter" => {
                let ret = callback_ret.ok_or_else(|| {
                    "MIR View try_filter callback has no checked result type".to_string()
                })?;
                if result_ok_type(ret).is_none() {
                    return Err(
                        "MIR View try_filter callback result is not a checked Result".to_string(),
                    );
                }
                element_ty.clone()
            }
            "fold" => self.mir_value_type(
                args.first()
                    .ok_or_else(|| "MIR View fold accumulator argument is missing".to_string())?
                    .value,
            )?,
            _ => {
                return Err(format!(
                    "MIR View callback member `{}` has no descriptor ABI",
                    row.member
                ));
            }
        };
        let element_id = runtime_type_id(element_ty)
            .ok_or_else(|| "MIR View element has no checked runtime type identity".to_string())?;
        let result_id = runtime_type_id(&result_ty)
            .ok_or_else(|| "MIR View result has no checked runtime type identity".to_string())?;
        values.push(builder.ins().iconst(types::I64, element_id as i64));
        values.push(builder.ins().iconst(types::I64, result_id as i64));
        self.call_prelude(builder, call, values, expected)
    }

    fn plot_column_call(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        call: jet_foundation::MIR::MirPreludeCallId,
        receiver: MirValueId,
        args: &[MirCallArg],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let row = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .ok_or_else(|| format!("MIR plot column call {:?} is missing", call))?;
        let (_, callback_index, expected_args) = plot_callback_shape(row)
            .ok_or_else(|| format!("MIR call {:?} is not a plot column route", call))?;
        if args.len() != expected_args {
            return Err(format!(
                "MIR plot column route expects {} explicit arguments, got {}",
                expected_args,
                args.len()
            ));
        }
        let callback_arg = args
            .get(callback_index)
            .ok_or_else(|| "MIR plot column callback argument is missing".to_string())?;
        let bound = self.bind_universal_callback(builder, callback_arg.value, 1)?;
        let host = self
            .host
            .lookup("jet_jit_data_plot_column")
            .ok_or_else(|| "JIT plot column constructor is not registered".to_string())?;
        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        if signature.params.len() != 2 {
            return Err(format!(
                "JIT plot column constructor expects two arguments, got {}",
                signature.params.len()
            ));
        }
        let field = self.value(receiver)?;
        let result = self
            .call_declared_values(builder, host, &signature, vec![field, bound])?
            .first()
            .copied()
            .ok_or_else(|| "JIT plot column constructor returned no value".to_string())?;
        expected.map_or(Ok(result), |ty| self.cast(builder, result, ty))
    }
    fn closure(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        function: MirFunctionId,
        captures: &[MirCaptureOperand],
        _facts: &jet_foundation::MIR::MirCaptureFacts,
        owned_callback_captures: bool,
        callback_value: Option<MirValueId>,
        callback_ty: Option<&MirType>,
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let target = *self
            .function_ids
            .get(&function)
            .ok_or_else(|| format!("MIR closure function {:?} is missing", function))?;
        let capture_params = self
            .program
            .functions
            .iter()
            .find(|candidate| candidate.id == function)
            .ok_or_else(|| format!("MIR closure function {:?} is missing", function))?
            .capture_params
            .clone();
        if capture_params.len() != captures.len() {
            return Err(format!(
                "MIR closure {:?} has {} captures but its target declares {} capture slots",
                function,
                captures.len(),
                capture_params.len()
            ));
        }
        for (slot, capture) in capture_params.iter().enumerate() {
            if capture.slot != slot {
                return Err(format!(
                    "MIR closure {:?} capture slot {} is not in canonical order",
                    function, capture.slot
                ));
            }
        }
        let local = self.module.declare_func_in_func(target, builder.func);
        let pointer = builder.ins().func_addr(types::I64, local);
        let (env, has_env) = if capture_params.is_empty() {
            (
                builder.ins().iconst(types::I64, 0),
                builder.ins().iconst(types::I8, 0),
            )
        } else {
            let arity = builder
                .ins()
                .iconst(types::I64, capture_params.len() as i64);
            let env = self
                .call_host(builder, self.host.struct_new, &[arity])?
                .first()
                .copied()
                .ok_or_else(|| {
                    "MIR closure environment constructor returned no record".to_string()
                })?;
            for (index, (operand, parameter)) in
                captures.iter().zip(capture_params.iter()).enumerate()
            {
                if owned_callback_captures
                    && parameter.access != jet_foundation::MIR::MirAccess::Read
                {
                    return Err(format!(
                        "MIR managed callback capture slot {} is not read-only",
                        parameter.slot
                    ));
                }
                let value = match operand {
                    MirCaptureOperand::Value(value) => {
                        if parameter.access != jet_foundation::MIR::MirAccess::Move
                            && parameter.ownership.mode != MirOwnershipMode::Owned
                        {
                            return Err(format!(
                                "MIR closure {:?} value capture in slot {} is not owned",
                                function, parameter.slot
                            ));
                        }
                        self.value(*value)?
                    }
                    MirCaptureOperand::Place(place) => {
                        if parameter.access == jet_foundation::MIR::MirAccess::Move {
                            return Err(format!(
                                "MIR closure {:?} place capture in slot {} is a move capture",
                                function, parameter.slot
                            ));
                        }
                        if owned_callback_captures {
                            self.read_place(builder, *place)?
                        } else {
                            self.address_of(builder, *place)?
                        }
                    }
                };
                if matches!(operand, MirCaptureOperand::Place(_)) && !owned_callback_captures {
                    let index = builder.ins().iconst(types::I64, index as i64);
                    let _ =
                        self.call_host(builder, self.host.struct_set_i64, &[env, index, value])?;
                } else {
                    self.set_field(builder, env, index, &parameter.ty, value)?;
                }
            }
            (env, builder.ins().iconst(types::I8, 1))
        };
        let value = self
            .call_host(builder, self.host.callable_bind, &[pointer, env, has_env])?
            .first()
            .copied()
            .ok_or_else(|| "MIR closure binder returned no value".to_string())?;
        let owned = builder
            .ins()
            .iconst(types::I8, i64::from(owned_callback_captures));
        let _ = self.call_host(
            builder,
            self.host.callable_bind_history_capture_mode,
            &[value, owned],
        )?;
        if let (Some(callback), Some(callback_ty)) = (callback_value, callback_ty) {
            if let Some((params, _)) = callable_signature(callback_ty) {
                let arity = params.len();
                if let Some(thunk) = self
                    .view_thunks
                    .get(&(self.function.id, callback, arity))
                    .copied()
                {
                    let thunk_ref = self.module.declare_func_in_func(thunk, builder.func);
                    let thunk = builder.ins().func_addr(types::I64, thunk_ref);
                    if arity == 1 {
                        let zero = builder.ins().iconst(types::I64, 0);
                        let _ = self.call_host(
                            builder,
                            self.host.callable_bind_raw,
                            &[value, thunk, zero],
                        )?;
                    } else if arity == 2 {
                        let zero = builder.ins().iconst(types::I64, 0);
                        let _ = self.call_host(
                            builder,
                            self.host.callable_bind_raw,
                            &[value, zero, thunk],
                        )?;
                    } else {
                        let _ = self.call_host(
                            builder,
                            self.host.callable_bind_raw_many,
                            &[value, thunk],
                        )?;
                    }
                }
            }
        }
        expected.map_or(Ok(value), |target| self.cast(builder, value, target))
    }

    fn validate_associated_owner(&self, owner: &MirType) -> Result<(), String> {
        if clif_ty_from_mir(owner).is_none() {
            return Err("MIR associated owner has no Cranelift carrier".to_string());
        }
        if let Some(identity) = owner.identity {
            if !self
                .program
                .type_instances
                .iter()
                .any(|instance| instance.identity == Some(identity))
            {
                return Err(format!(
                    "MIR associated owner type {:?} has no instance row",
                    identity
                ));
            }
        }
        Ok(())
    }

    fn call_user(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        id: MirFunctionId,
        args: &[MirCallArg],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let target = *self
            .function_ids
            .get(&id)
            .ok_or_else(|| format!("MIR user function {:?} is missing", id))?;
        let signature = self
            .module
            .declarations()
            .get_function_decl(target)
            .signature
            .clone();
        let values = self.lower_call_args(builder, args, &signature)?;
        self.call_declared_values(builder, target, &signature, values)
            .and_then(|results| {
                let value = results
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR user call returned no value".to_string())?;
                expected.map_or(Ok(value), |ty| self.cast(builder, value, ty))
            })
    }

    fn call_foreign(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        id: jet_foundation::MIR::MirForeignId,
        args: &[MirCallArg],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let row = self
            .program
            .foreign
            .iter()
            .find(|row| row.id == id)
            .ok_or_else(|| format!("MIR foreign function {:?} is missing", id))?;
        if row.path.is_empty() || row.symbol.is_empty() {
            return Err(format!(
                "MIR foreign function {:?} has no checked bridge identity",
                id
            ));
        }
        if !matches!(
            row.foreign_language,
            jet_foundation::MIR::MirForeignLanguage::C
                | jet_foundation::MIR::MirForeignLanguage::Cpp
        ) || !matches!(
            row.foreign_abi,
            jet_foundation::MIR::MirForeignAbi::C | jet_foundation::MIR::MirForeignAbi::CUnwind
        ) {
            return Err(format!(
                "MIR foreign function {:?} has no supported resident C bridge ABI",
                id
            ));
        }
        if args.len() != row.params.len() {
            return Err(format!(
                "MIR foreign function {:?} expects {} arguments, got {}",
                id,
                row.params.len(),
                args.len()
            ));
        }
        match row.callback_transport.as_deref() {
            Some("managed-close") => {
                if args.len() != 1 || args[0].access != jet_foundation::MIR::MirAccess::Move {
                    return Err(
                        "managed callback unsubscribe must consume one registration".to_string()
                    );
                }
                let registration = self.value(args[0].value)?;
                let value = self
                    .call_host(builder, self.host.ffi.callback_unsubscribe, &[registration])?
                    .first()
                    .copied()
                    .ok_or_else(|| {
                        "managed callback unsubscribe host returned no task".to_string()
                    })?;
                return expected.map_or(Ok(value), |ty| self.cast(builder, value, ty));
            }
            Some("emit-task") => {
                if args.len() != 1 {
                    return Err("managed callback emission must receive one payload".to_string());
                }
                let native = self
                    .program
                    .foreign
                    .iter()
                    .find(|candidate| {
                        candidate.module_id == row.module_id
                            && candidate.name == "__jet_native_emit_async"
                    })
                    .ok_or_else(|| {
                        "managed callback emission has no native emit row".to_string()
                    })?;
                let wrapper = builder.ins().iconst(
                    types::I64,
                    self.runtime
                        .heap
                        .alloc_string(crate::Ffi::bridge_wrapper_name(native)),
                );
                let value = self.value(args[0].value)?;
                let value = self
                    .call_host(builder, self.host.ffi.emit_task, &[wrapper, value])?
                    .first()
                    .copied()
                    .ok_or_else(|| "managed callback emission host returned no task".to_string())?;
                return expected.map_or(Ok(value), |ty| self.cast(builder, value, ty));
            }
            Some("managed") => {
                if args.len() != 1 {
                    return Err(
                        "managed callback registration must receive one callback".to_string()
                    );
                }
                let callback = self.value(args[0].value)?;
                let native_start = self
                    .program
                    .foreign
                    .iter()
                    .find(|candidate| {
                        candidate.module_id == row.module_id
                            && candidate.name == format!("__jet_native_{}", row.name)
                            && candidate.callback_transport.as_deref() == Some("native-start")
                    })
                    .ok_or_else(|| {
                        "managed callback registration has no native start row".to_string()
                    })?;
                let native_stop = self
                    .program
                    .foreign
                    .iter()
                    .find(|candidate| {
                        candidate.module_id == row.module_id
                            && candidate.name == "__jet_native_unsubscribe"
                    })
                    .ok_or_else(|| {
                        "managed callback registration has no native stop row".to_string()
                    })?;
                let identity = row
                    .callback_identity
                    .as_deref()
                    .filter(|identity| !identity.is_empty())
                    .ok_or_else(|| "managed callback registration has no identity".to_string())?;
                let digest = row
                    .callback_plan_digest
                    .as_deref()
                    .filter(|digest| !digest.is_empty())
                    .ok_or_else(|| {
                        "managed callback registration has no plan digest".to_string()
                    })?;
                let native_start = builder.ins().iconst(
                    types::I64,
                    self.runtime
                        .heap
                        .alloc_string(crate::Ffi::bridge_wrapper_name(native_start)),
                );
                let native_stop = builder.ins().iconst(
                    types::I64,
                    self.runtime
                        .heap
                        .alloc_string(crate::Ffi::bridge_wrapper_name(native_stop)),
                );
                let identity = builder.ins().iconst(
                    types::I64,
                    self.runtime.heap.alloc_string(identity.to_string()),
                );
                let digest = builder.ins().iconst(
                    types::I64,
                    self.runtime.heap.alloc_string(digest.to_string()),
                );
                let value = self
                    .call_host(
                        builder,
                        self.host.ffi.callback_register,
                        &[callback, native_start, native_stop, identity, digest],
                    )?
                    .first()
                    .copied()
                    .ok_or_else(|| {
                        "managed callback registration host returned no handle".to_string()
                    })?;
                return expected.map_or(Ok(value), |ty| self.cast(builder, value, ty));
            }
            Some("native-start") => {
                return Err(
                    "managed callback native start is only callable through registration"
                        .to_string(),
                );
            }
            _ => {}
        }
        for parameter in &row.params {
            if clif_ty_from_mir(&parameter.ty).is_none() {
                return Err(format!(
                    "MIR foreign parameter `{}` has no resident carrier",
                    parameter.name
                ));
            }
        }
        let wrapper = builder.ins().iconst(
            types::I64,
            self.runtime
                .heap
                .alloc_string(crate::Ffi::bridge_wrapper_name(row)),
        );
        let values = args
            .iter()
            .map(|arg| self.value(arg.value))
            .collect::<Result<Vec<_>, _>>()?;
        let values = self.build_value_list(builder, &values)?;
        let value = self
            .call_host(builder, self.host.ffi.call, &[wrapper, values])?
            .first()
            .copied()
            .ok_or_else(|| "MIR foreign bridge returned no value".to_string())?;
        expected.map_or(Ok(value), |ty| self.cast(builder, value, ty))
    }
    fn is_model_embed_method(&self, function: MirFunctionId, owner: &MirType) -> bool {
        let Some(method) = self
            .program
            .functions
            .iter()
            .find(|candidate| candidate.id == function)
        else {
            return false;
        };
        if method.name != "embed" {
            return false;
        }
        let owner_name = owner.display_name();
        let owner = owner_name
            .rsplit("::")
            .next()
            .unwrap_or(owner_name.as_str())
            .rsplit('.')
            .next()
            .unwrap_or(owner_name.as_str());
        self.program
            .facts
            .model_outputs
            .iter()
            .any(|fact| fact.signature_name.as_deref() == Some(owner))
    }

    fn call_model_embed(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        args: &[MirCallArg],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "model embed expects receiver and document list, got {}",
                args.len()
            ));
        }
        let host = self.host.model_embed;
        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        let values = self.lower_call_args(builder, args, &signature)?;
        self.call_declared_values(builder, host, &signature, values)
            .and_then(|results| {
                let value = results
                    .first()
                    .copied()
                    .ok_or_else(|| "model embed host returned no value".to_string())?;
                expected.map_or(Ok(value), |ty| self.cast(builder, value, ty))
            })
    }
    fn call_trait_method(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        method_id: jet_foundation::MIR::MirTraitMethodId,
        trait_ref: &jet_foundation::MIR::MirTraitRef,
        args: &[MirCallArg],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let trait_def = self
            .program
            .traits
            .iter()
            .find(|definition| definition.id == trait_ref.id)
            .ok_or_else(|| format!("MIR trait {:?} is missing", trait_ref.id))?;
        let method = trait_def
            .methods
            .iter()
            .find(|method| method.id == method_id)
            .ok_or_else(|| format!("MIR trait method {:?} is missing", method_id))?;
        let mut targets = Vec::new();
        for impl_row in &self.program.impls {
            let Some(impl_trait) = impl_row.trait_ref.as_ref() else {
                continue;
            };
            if impl_trait.id != trait_ref.id {
                continue;
            }
            let type_id = impl_row
                .self_type
                .identity
                .ok_or_else(|| format!("MIR impl {:?} has no self type identity", impl_row.id))?;
            for function_id in &impl_row.methods {
                let function = self
                    .program
                    .functions
                    .iter()
                    .find(|function| function.id == *function_id)
                    .ok_or_else(|| {
                        format!("MIR impl {:?} references missing function", impl_row.id)
                    })?;
                if function.name == method.name
                    && matches!(
                        &function.form,
                        MirFunctionForm::TraitMethod {
                            trait_ref: function_trait,
                            ..
                        } if function_trait.id == trait_ref.id
                    )
                {
                    if targets.iter().any(|(existing, _)| *existing == type_id) {
                        return Err(format!(
                            "MIR trait method {:?} has ambiguous impls for type {:?}",
                            method_id, type_id
                        ));
                    }
                    targets.push((type_id, *function_id));
                }
            }
        }
        let default = method.default;
        if targets.is_empty() {
            let function = default.ok_or_else(|| {
                format!(
                    "MIR trait method `{}` has no selected implementation",
                    method.name
                )
            })?;
            return self.call_user(builder, function, args, expected);
        }
        let first = args
            .first()
            .ok_or_else(|| "MIR trait method call has no receiver".to_string())?;
        let receiver = if first.access == MirAccess::Write {
            self.address_of(
                builder,
                first
                    .place
                    .ok_or_else(|| "MIR trait method write receiver has no place".to_string())?,
            )?
        } else {
            self.value(first.value)?
        };
        let receiver = self.cast(builder, receiver, types::I64)?;
        let runtime_type = self
            .call_host(builder, self.host.trait_object_type, &[receiver])?
            .first()
            .copied()
            .ok_or_else(|| "MIR trait object type lookup returned no value".to_string())?;
        let result_type = expected.unwrap_or(types::I64);
        let merge = builder.create_block();
        builder.append_block_param(merge, result_type);
        let mut test = builder.create_block();
        builder.ins().jump(test, &[]);
        for (index, (type_id, function)) in targets.iter().enumerate() {
            builder.switch_to_block(test);
            let condition = builder
                .ins()
                .icmp_imm(IntCC::Equal, runtime_type, type_id.0 as i64);
            let matched = builder.create_block();
            let fallback = if index + 1 == targets.len() {
                builder.create_block()
            } else {
                let next = builder.create_block();
                test = next;
                next
            };
            builder.ins().brif(condition, matched, &[], fallback, &[]);
            builder.switch_to_block(matched);
            let result = self.call_user(builder, *function, args, expected)?;
            let result = self.cast(builder, result, result_type)?;
            builder.ins().jump(merge, &[result]);
            if index + 1 == targets.len() {
                builder.switch_to_block(fallback);
                if let Some(default) = default {
                    let result = self.call_user(builder, default, args, expected)?;
                    let result = self.cast(builder, result, result_type)?;
                    builder.ins().jump(merge, &[result]);
                } else {
                    let result = self.trap(builder)?;
                    let result = self.cast(builder, result, result_type)?;
                    builder.ins().jump(merge, &[result]);
                }
            }
        }
        builder.switch_to_block(merge);
        builder.seal_block(merge);
        builder
            .block_params(merge)
            .first()
            .copied()
            .ok_or_else(|| "MIR trait method dispatch merge has no result".to_string())
    }

    fn call_callee(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        callee: &MirCallee,
        args: &[MirCallArg],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        match callee {
            MirCallee::User(id) => self.call_user(builder, *id, args, expected),
            MirCallee::Associated { function, owner } | MirCallee::Method { function, owner } => {
                self.validate_associated_owner(owner)?;
                if self.is_model_embed_method(*function, owner) {
                    self.call_model_embed(builder, args, expected)
                } else {
                    self.call_user(builder, *function, args, expected)
                }
            }
            MirCallee::TraitMethod {
                method, trait_ref, ..
            } => self.call_trait_method(builder, *method, trait_ref, args, expected),
            MirCallee::Prelude(id) => self.call_prelude_args(builder, *id, args, expected),
            MirCallee::Foreign(id) => self.call_foreign(builder, *id, args, expected),
            MirCallee::Indirect(value) => self.indirect_call(builder, *value, args, expected),
            MirCallee::Core(id) => self.call_core(builder, *id, None, &[], args, None, expected),
        }
    }

    fn csv_decode_callback(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        spec: &CsvDecodeSpec,
    ) -> Result<Value, String> {
        let (raw, env) = match spec.target {
            CsvDecodeTarget::Function(target) => {
                let thunk = *self
                    .csv_thunks
                    .get(&(self.function.id, target))
                    .ok_or_else(|| {
                        format!(
                            "CSV Decode function {:?} has no generated universal thunk",
                            target
                        )
                    })?;
                let thunk_ref = self.module.declare_func_in_func(thunk, builder.func);
                (
                    builder.ins().func_addr(types::I64, thunk_ref),
                    builder.ins().iconst(types::I64, 0),
                )
            }
            CsvDecodeTarget::Scalar(kind) => {
                let scalar = self
                    .module
                    .declare_func_in_func(self.host.encoding.csv_decode_scalar, builder.func);
                (
                    builder.ins().func_addr(types::I64, scalar),
                    builder.ins().iconst(types::I64, kind),
                )
            }
        };
        let has_env = builder.ins().iconst(types::I8, 1);
        let bound = self
            .call_host(builder, self.host.callable_bind, &[raw, env, has_env])?
            .first()
            .copied()
            .ok_or_else(|| "CSV Decode callback binder returned no value".to_string())?;
        let zero = builder.ins().iconst(types::I64, 0);
        let _ = self.call_host(builder, self.host.callable_bind_raw, &[bound, raw, zero])?;
        Ok(bound)
    }

    fn bind_universal_callback(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        callback: MirValueId,
        arity: usize,
    ) -> Result<Value, String> {
        let value = self.cast(builder, self.value(callback)?, types::I64)?;
        let thunk = *self
            .view_thunks
            .get(&(self.function.id, callback, arity))
            .ok_or_else(|| format!("MIR callback {callback:?} has no generated universal thunk"))?;
        let thunk_ref = self.module.declare_func_in_func(thunk, builder.func);
        let thunk = builder.ins().func_addr(types::I64, thunk_ref);
        let has_env = builder.ins().iconst(types::I8, 1);
        let bound = self
            .call_host(builder, self.host.callable_bind, &[thunk, value, has_env])?
            .first()
            .copied()
            .ok_or_else(|| "MIR callback binder returned no value".to_string())?;
        if arity == 1 {
            let zero = builder.ins().iconst(types::I64, 0);
            let _ = self.call_host(builder, self.host.callable_bind_raw, &[bound, thunk, zero])?;
        } else if arity == 2 {
            let zero = builder.ins().iconst(types::I64, 0);
            let _ = self.call_host(builder, self.host.callable_bind_raw, &[bound, zero, thunk])?;
        } else {
            let _ = self.call_host(builder, self.host.callable_bind_raw_many, &[bound, thunk])?;
        }
        Ok(bound)
    }

    /// Replace every function-typed Core call argument with its bound
    /// universal-thunk slot. `values` is indexed like `args`; trailing host
    /// metadata words (CSV decode callback, type key) sit past `args.len()`
    /// and are never callback positions.
    fn bind_core_callbacks(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        args: &[MirCallArg],
        callbacks: &[CallbackShape],
        values: &mut [Value],
    ) -> Result<(), String> {
        for &(parameters, index) in callbacks {
            let slot = values.get_mut(index).ok_or_else(|| {
                format!("MIR Core callback argument {index} has no lowered value")
            })?;
            *slot = self.bind_universal_callback(builder, args[index].value, parameters)?;
        }
        Ok(())
    }

    /// Core rows whose host materialises a generic callback result (`K`/`V`
    /// fixed only at the call site) receive the checked return type of their
    /// first callback as one trailing string-handle word, the same trailer
    /// the typed `decode` rows carry.  Returns the host signature without
    /// that trailer and the key word to append, or `None` for every other row.
    fn callback_return_key(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        module: &str,
        member: &str,
        args: &[MirCallArg],
        callbacks: &[CallbackShape],
        signature: &Signature,
    ) -> Result<Option<(Signature, Value)>, String> {
        if !matches!(
            (module, member),
            ("core.data", "group_by" | "sum") | ("", "group_by" | "sum")
        ) {
            return Ok(None);
        }
        let &(_, index) = callbacks
            .first()
            .ok_or_else(|| format!("MIR Core call `{module}`.{member} has no callback argument"))?;
        if signature.params.len() != args.len() + 1 {
            return Err(format!(
                "MIR Core call `{module}`.{member} host expects {} source arguments plus a callback return key, got {}",
                signature.params.len().saturating_sub(1),
                args.len()
            ));
        }
        let callback_ty = self.mir_value_type(args[index].value)?;
        let (_, ret) = callable_signature(&callback_ty).ok_or_else(|| {
            format!(
                "MIR Core callback {:?} has no function signature",
                args[index].value
            )
        })?;
        let key = ret.map_or_else(|| "Unit".to_string(), MirType::identity_key);
        let key = self.runtime.heap.alloc_string(key);
        let mut source_signature = signature.clone();
        source_signature.params.pop();
        Ok(Some((
            source_signature,
            builder.ins().iconst(types::I64, key),
        )))
    }

    fn call_core(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        id: jet_foundation::MIR::MirCoreCallId,
        route: Option<jet_foundation::MIR::MirPreludeCallId>,
        type_args: &[MirType],
        args: &[jet_foundation::MIR::MirCallArg],
        result_type: Option<&MirType>,
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let row = self
            .program
            .core_calls
            .iter()
            .find(|row| row.id == id)
            .ok_or_else(|| format!("MIR Core call {:?} is missing", id))?;
        let model_open = row.module == "core.models" && row.member == "open";
        let csv_typed = matches!(
            (row.module.as_str(), row.member.as_str()),
            ("core.data", "csv") | ("core.encoding.csv", "decode" | "query")
        );
        let callbacks = core_callback_shapes(self.function, args)?;
        if row.module == "core.compute"
            && matches!(
                row.member.as_str(),
                "gradient" | "value_and_gradient" | "vjp" | "jvp"
            )
        {
            return self.call_compute(
                builder,
                &row.module,
                &row.member,
                type_args,
                args,
                result_type,
                expected,
            );
        }
        if Self::service_module_id(&row.module).is_some() {
            return self.call_service_args(builder, &row.module, &row.member, args, expected);
        }
        if row.module == "core.time" {
            if let Some(value) = self.call_time_static_args(builder, &row.member, args, expected)? {
                return Ok(value);
            }
        }
        let measurement_sqrt = row.module == "core.math"
            && row.member == "sqrt"
            && result_type.is_some_and(|ty| {
                matches!(
                    ty.kind(),
                    MirTypeKind::Apply { name, args }
                        if name.name == "Measurement"
                            && args.len() == 1
                            && matches!(args[0].kind(), MirTypeKind::Float)
                )
            });
        let typed_symbol = if measurement_sqrt {
            Some("jet_std::JetMeasurement::sqrt")
        } else if model_open {
            Some("jet_jit_model_open")
        } else if !type_args.is_empty() {
            match (row.module.as_str(), row.member.as_str()) {
                ("core.args", "decode") => Some("jet_jit_args_decode"),
                ("core.args", "merge") => Some("jet_jit_args_merge"),
                ("core.sys", "decode") => Some("jet_jit_env_decode"),
                ("core.encoding.json", "decode") => Some("jet_jit_json_decode_typed"),
                ("core.data", "json") => Some("jet_jit_data_json_decode_typed"),
                ("core.data", "csv") | ("core.encoding.csv", "decode") => {
                    Some("jet_jit_enc_csv_decode")
                }
                ("core.encoding.csv", "to_string") => Some("jet_jit_enc_csv_to_string"),
                ("core.db", "decode") => Some("jet_jit_db_decode"),
                ("core.testing", "histories") => Some("jet_jit_testing_histories"),
                // The public list receiver spelling is `query`; its
                // internal SQL symbol carries the element descriptor trailer.
                ("", "query") => Some("jet_jit_data_query_sql"),
                ("core.data", "load") => Some("jet_data_loader_load"),
                ("core.data", "load_default") => Some("jet_data_loader_load_default"),
                ("core.data", "file") => Some("jet_data_loader_file"),
                ("core.data", "file_member") => Some("jet_data_loader_file_member"),
                ("core.data", "url") => Some("jet_data_loader_url"),
                ("core.data", "database") => Some("jet_data_loader_database"),
                ("core.data", "value") => Some("jet_data_loader_value"),
                ("core.data", "snapshot") => Some("jet_data_loader_snapshot"),
                ("core.data.loader", "stream") => Some("jet_data_loader_stream"),
                ("core.data.stream", "next") => Some("jet_data_stream_next"),
                ("core.data.stream", "collect") => Some("jet_data_stream_collect"),
                _ => None,
            }
        } else {
            None
        };
        let jit_symbol = typed_symbol
            .map(str::to_owned)
            .or_else(|| row.jit_symbol.clone());
        if let Some(symbol) = jit_symbol {
            let host = self
                .host
                .lookup(&symbol)
                .ok_or_else(|| format!("resolved MIR Core symbol `{symbol}` is not registered"))?;
            let signature = self
                .module
                .declarations()
                .get_function_decl(host)
                .signature
                .clone();
            let mut values = if model_open {
                if type_args.len() != 1 || signature.params.len() != args.len() + 1 {
                    return Err(
                        "model.open JIT ABI requires one trait key and one output argument"
                            .to_string(),
                    );
                }
                let mut source_signature = signature.clone();
                source_signature.params.pop();
                let mut values = self.lower_call_args(builder, args, &source_signature)?;
                let type_key = self.runtime.heap.alloc_string(type_args[0].identity_key());
                values.push(builder.ins().iconst(types::I64, type_key));
                values
            } else if csv_typed {
                let spec = csv_decode_spec(self.program, id, type_args)?
                    .ok_or_else(|| "typed CSV CoreCall has no row type".to_string())?;
                if signature.params.len() != args.len() + 2 {
                    return Err(format!(
                        "typed CSV host ABI expects {} source arguments plus callback metadata, got {}",
                        args.len(),
                        signature.params.len()
                    ));
                }
                let mut source_signature = signature.clone();
                source_signature.params.truncate(args.len());
                let mut values = self.lower_call_args(builder, args, &source_signature)?;
                values.push(self.csv_decode_callback(builder, &spec)?);
                values.push(builder.ins().iconst(types::I64, spec.element_kind));
                values
            } else if !type_args.is_empty()
                && matches!(
                    (row.module.as_str(), row.member.as_str()),
                    ("core.args", "decode")
                        | ("core.args", "merge")
                        | ("core.sys", "decode")
                        | ("core.encoding.json", "decode")
                        | ("core.data", "json")
                        | ("core.encoding.csv", "to_string")
                        | ("core.db", "decode")
                        | ("", "query")
                        | ("core.data", "load")
                        | ("core.data", "load_default")
                        | ("core.data", "file")
                        | ("core.data", "file_member")
                        | ("core.data", "url")
                        | ("core.data", "database")
                        | ("core.data", "value")
                        | ("core.data", "snapshot")
                        | ("core.testing", "histories")
                        | ("core.data.loader", "stream")
                        | ("core.data.stream", "next")
                        | ("core.data.stream", "collect")
                )
            {
                if type_args.len() < 1 || signature.params.len() != args.len() + 1 {
                    return Err(format!(
                        "typed MIR Core call `{}`.{} expects {} source arguments plus type metadata, got {}",
                        row.module,
                        row.member,
                        signature.params.len().saturating_sub(1),
                        args.len()
                    ));
                }
                let mut source_signature = signature.clone();
                source_signature.params.pop();
                let mut values = self.lower_call_args(builder, args, &source_signature)?;
                let type_key = self.runtime.heap.alloc_string(type_args[0].identity_key());
                values.push(builder.ins().iconst(types::I64, type_key));
                values
            } else if let Some((source_signature, key)) = self.callback_return_key(
                builder,
                &row.module,
                &row.member,
                args,
                &callbacks,
                &signature,
            )? {
                let mut values = self.lower_call_args(builder, args, &source_signature)?;
                values.push(key);
                values
            } else {
                self.lower_core_call_args(builder, &row.module, &row.member, args, &signature)?
            };
            self.bind_core_callbacks(builder, args, &callbacks, &mut values)?;
            return self
                .call_declared_values(builder, host, &signature, values)
                .and_then(|results| {
                    let value = results
                        .first()
                        .copied()
                        .ok_or_else(|| "MIR Core call returned no value".to_string())?;
                    expected.map_or(Ok(value), |ty| self.cast(builder, value, ty))
                });
        }
        if csv_typed {
            return Err(format!(
                "MIR typed CSV call {:?} has no resolved JIT symbol",
                id
            ));
        }
        if let Some(route) = route {
            return self.call_prelude_args_bound(builder, route, args, &callbacks, expected);
        }
        Err(format!("MIR Core call {:?} has no resolved JIT symbol", id))
    }
    fn hardware_call(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        call: jet_foundation::MIR::MirPreludeCallId,
        op: &jet_foundation::MIR::MirHardwareOp,
        receiver: Option<MirValueId>,
        args: &[MirCallArg],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let row = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .ok_or_else(|| format!("MIR hardware Prelude call {:?} is missing", call))?;
        let symbol = row.symbol.name();
        let host_symbol = match op {
            jet_foundation::MIR::MirHardwareOp::DmaStart { .. } => "jet_jit_dma_start_marshaled",
            jet_foundation::MIR::MirHardwareOp::DmaWait { .. } => "jet_jit_dma_wait_marshaled",
            _ => symbol,
        };
        let host = self.host.lookup(host_symbol).ok_or_else(|| {
            format!("resolved MIR hardware symbol `{host_symbol}` is not registered")
        })?;
        let handle = |runtime: &mut JitRuntime, builder: &mut FunctionBuilder<'_>, value: &str| {
            builder
                .ins()
                .iconst(types::I64, runtime.heap.alloc_string(value))
        };
        let width_tag = |width: jet_foundation::TargetMachine::RegisterWidth| -> i64 {
            match width {
                jet_foundation::TargetMachine::RegisterWidth::U8 => 1,
                jet_foundation::TargetMachine::RegisterWidth::U16 => 2,
                jet_foundation::TargetMachine::RegisterWidth::U32 => 4,
                jet_foundation::TargetMachine::RegisterWidth::U64 => 8,
            }
        };
        let mut values = Vec::new();
        match op {
            jet_foundation::MIR::MirHardwareOp::RegisterRead {
                profile_id,
                block,
                register,
                width,
            } => {
                if receiver.is_some() || !args.is_empty() {
                    return Err("MIR hardware register read has unexpected operands".to_string());
                }
                values.push(handle(self.runtime, builder, profile_id));
                values.push(handle(self.runtime, builder, block));
                values.push(handle(self.runtime, builder, register));
                values.push(builder.ins().iconst(types::I64, width_tag(*width)));
            }
            jet_foundation::MIR::MirHardwareOp::RegisterWrite {
                profile_id,
                block,
                register,
                width,
            } => {
                if receiver.is_some() || args.len() != 1 {
                    return Err("MIR hardware register write has invalid operands".to_string());
                }
                let value = &args[0];
                if value.access != MirAccess::Read || value.place.is_some() {
                    return Err(
                        "MIR hardware register write value is not a checked read".to_string()
                    );
                }
                values.push(handle(self.runtime, builder, profile_id));
                values.push(handle(self.runtime, builder, block));
                values.push(handle(self.runtime, builder, register));
                values.push(builder.ins().iconst(types::I64, width_tag(*width)));
                values.push(self.value(value.value)?);
            }
            jet_foundation::MIR::MirHardwareOp::DmaStart {
                profile_id,
                channel,
                buffer_ty,
            } => {
                if receiver.is_some() || args.len() != 1 {
                    return Err("MIR hardware DMA start has invalid operands".to_string());
                }
                let value = &args[0];
                if value.access != MirAccess::Move || value.place.is_some() {
                    return Err("MIR hardware DMA start buffer is not a checked move".to_string());
                }
                self.runtime.register_dma_type(buffer_ty);
                let type_key = self.runtime.heap.alloc_string(buffer_ty.identity_key());
                values.push(handle(self.runtime, builder, profile_id));
                values.push(handle(self.runtime, builder, channel));
                values.push(self.value(value.value)?);
                values.push(builder.ins().iconst(types::I64, type_key));
            }
            jet_foundation::MIR::MirHardwareOp::DmaWait {
                profile_id,
                channel,
                buffer_ty,
            } => {
                let transfer = receiver
                    .ok_or_else(|| "MIR hardware DMA wait has no checked transfer".to_string())?;
                if !args.is_empty() {
                    return Err("MIR hardware DMA wait has unexpected operands".to_string());
                }
                self.runtime.register_dma_type(buffer_ty);
                values.push(handle(self.runtime, builder, profile_id));
                values.push(handle(self.runtime, builder, channel));
                values.push(self.value(transfer)?);
            }
        }
        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        let result = self
            .call_declared_values(builder, host, &signature, values)?
            .first()
            .copied()
            .ok_or_else(|| format!("MIR hardware symbol `{host_symbol}` returned no value"))?;
        expected.map_or(Ok(result), |ty| self.cast(builder, result, ty))
    }

    /// Keep the JIT's value-producing convention aligned with AOT for Unit.
    ///
    /// A Rust Prelude function returning `()` has no Cranelift result, while
    /// MIR still represents Unit as the canonical zero `i64` carrier.  Only
    /// signatures with no declared return use that carrier; a declared return
    /// with no result remains an ICE below.
    fn prelude_results(
        builder: &mut FunctionBuilder<'_>,
        signature: &Signature,
        results: Vec<Value>,
    ) -> Vec<Value> {
        if signature.returns.is_empty() {
            vec![builder.ins().iconst(types::I64, 0)]
        } else {
            results
        }
    }
    fn time_static_host(&self, member: &str) -> Option<FuncId> {
        Some(match member {
            "new" => self.host.time.date_new,
            "from_timestamp" => self.host.time.datetime_from_timestamp,
            "parse_time" => self.host.time.parse_time,
            "parse" => self.host.time.date_parse,
            "parse_iso_week_date" => self.host.time.parse_iso_week_date,
            "time" | "local_time" => self.host.time.local_time,
            _ => return None,
        })
    }

    fn call_time_static_values(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        member: &str,
        values: Vec<Value>,
    ) -> Result<Option<Vec<Value>>, String> {
        let Some(host) = self.time_static_host(member) else {
            return Ok(None);
        };
        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        let results = self.call_declared_values(builder, host, &signature, values)?;
        Ok(Some(Self::prelude_results(builder, &signature, results)))
    }

    fn call_time_static_args(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        member: &str,
        args: &[MirCallArg],
        expected: Option<types::Type>,
    ) -> Result<Option<Value>, String> {
        let Some(host) = self.time_static_host(member) else {
            return Ok(None);
        };
        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        let values = self.lower_call_args(builder, args, &signature)?;
        let results = self.call_declared_values(builder, host, &signature, values)?;
        let result = Self::prelude_results(builder, &signature, results)
            .first()
            .copied()
            .ok_or_else(|| format!("MIR time.{member} host returned no value"))?;
        Ok(Some(
            expected.map_or(Ok(result), |ty| self.cast(builder, result, ty))?,
        ))
    }

    fn service_module_id(module: &str) -> Option<i64> {
        match module {
            "core.service" => Some(0),
            "core.sync" => Some(1),
            _ => None,
        }
    }

    fn call_service_values(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        module: &str,
        member: &str,
        values: Vec<Value>,
        bool_result: bool,
    ) -> Result<Vec<Value>, String> {
        let module_id = Self::service_module_id(module)
            .ok_or_else(|| format!("MIR service module `{module}` has no resident adapter"))?;
        if values.len() > 7 {
            return Err(format!(
                "MIR service call `{module}`.{member} has {} arguments; resident adapter accepts at most seven",
                values.len()
            ));
        }
        let method = builder
            .ins()
            .iconst(types::I64, self.runtime.heap.alloc_string(member));
        let argc = builder.ins().iconst(types::I64, values.len() as i64);
        let zero = builder.ins().iconst(types::I64, 0);
        let mut call_values = Vec::with_capacity(10);
        call_values.push(builder.ins().iconst(types::I64, module_id));
        call_values.push(method);
        call_values.push(argc);
        call_values.extend(values);
        call_values.resize(10, zero);
        let host = if bool_result {
            self.host.service_call_bool
        } else {
            self.host.service_call
        };
        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        if signature.params.len() != 10 {
            return Err(format!(
                "MIR service adapter for `{module}`.{member} has {} parameters, expected ten",
                signature.params.len()
            ));
        }
        self.call_host(builder, host, &call_values)
    }

    fn call_service_args(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        module: &str,
        member: &str,
        args: &[MirCallArg],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let values = args
            .iter()
            .map(|arg| {
                if arg.access == MirAccess::Write {
                    return Err(format!(
                        "MIR service call `{module}`.{member} cannot pass a write argument"
                    ));
                }
                self.value(arg.value)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let results =
            self.call_service_values(builder, module, member, values, expected == Some(types::I8))?;
        let result = results
            .first()
            .copied()
            .ok_or_else(|| format!("MIR service call `{module}`.{member} returned no value"))?;
        expected.map_or(Ok(result), |ty| self.cast(builder, result, ty))
    }

    fn call_compute(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        module: &str,
        member: &str,
        type_args: &[MirType],
        args: &[MirCallArg],
        result_type: Option<&MirType>,
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        if type_args.len() != 1 {
            return Err(format!(
                "MIR {module}.{member} requires one checked callable type argument"
            ));
        }
        if args.len() < 2 {
            return Err(format!(
                "MIR {module}.{member} requires a callable and target list"
            ));
        }
        if args[0].access == MirAccess::Write
            || args
                .last()
                .is_some_and(|arg| arg.access == MirAccess::Write)
        {
            return Err(format!(
                "MIR {module}.{member} has an invalid write operand"
            ));
        }
        let callable_type = &type_args[0];
        let (parameters, return_type) = callable_signature(callable_type).ok_or_else(|| {
            format!("MIR {module}.{member} callable has no checked function signature")
        })?;
        let output = return_type
            .ok_or_else(|| format!("MIR {module}.{member} callable has no checked return type"))?;
        let return_type_id = crate::runtime_host::runtime_type_id(output)
            .ok_or_else(|| format!("MIR {module}.{member} callback return has no descriptor"))?;
        let output = match output.kind() {
            MirTypeKind::Result { ok, .. } => ok.as_ref(),
            _ => output,
        };
        let result_fields = match output.kind() {
            MirTypeKind::Tuple(fields) => fields.len(),
            _ => 0,
        };
        let base_arity = i64::try_from(parameters.len())
            .map_err(|_| format!("MIR {module}.{member} callable arity is too large"))?;
        let result_fields = i64::try_from(result_fields)
            .map_err(|_| format!("MIR {module}.{member} result shape is too large"))?;
        let method = match member {
            "gradient" => 0,
            "value_and_gradient" => 1,
            "vjp" => 2,
            "jvp" => 3,
            _ => return Err(format!("MIR {module}.{member} is not a compute transform")),
        };
        let base = self.value(args[0].value)?;
        let targets = self.value(args.last().expect("checked target argument").value)?;
        let primal_count = args.len() - 2;
        let returns_function = result_type
            .is_some_and(|ty| matches!(ty.kind(), MirTypeKind::Fn(_) | MirTypeKind::SendFn { .. }));
        let return_type_id = builder.ins().iconst(types::I64, return_type_id as i64);
        let result = if returns_function {
            if primal_count != 0 {
                return Err(format!(
                    "MIR {module}.{member} curried form has {} primal arguments",
                    primal_count
                ));
            }
            let method = builder.ins().iconst(types::I64, method);
            let base_arity = builder.ins().iconst(types::I64, base_arity);
            let result_fields = builder.ins().iconst(types::I64, result_fields);
            self.call_host(
                builder,
                self.host.compute.curried_new,
                &[
                    base,
                    targets,
                    method,
                    base_arity,
                    result_fields,
                    return_type_id,
                ],
            )?
        } else {
            let expected_inputs = if method == 3 {
                parameters
                    .len()
                    .checked_mul(2)
                    .ok_or_else(|| format!("MIR {module}.{member} input arity is too large"))?
            } else {
                parameters.len()
            };
            if primal_count != expected_inputs {
                return Err(format!(
                    "MIR {module}.{member} expects {} primal arguments, got {}",
                    expected_inputs, primal_count
                ));
            }
            let input_values = args[1..args.len() - 1]
                .iter()
                .map(|arg| {
                    if arg.access == MirAccess::Write {
                        return Err(format!("MIR {module}.{member} has a write primal argument"));
                    }
                    self.value(arg.value)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let inputs = self.build_value_list(builder, &input_values)?;
            let method = builder.ins().iconst(types::I64, method);
            let base_arity = builder.ins().iconst(types::I64, base_arity);
            let result_fields = builder.ins().iconst(types::I64, result_fields);
            self.call_host(
                builder,
                self.host.compute.transform,
                &[
                    base,
                    inputs,
                    targets,
                    method,
                    base_arity,
                    result_fields,
                    return_type_id,
                ],
            )?
        };
        let result = result
            .first()
            .copied()
            .ok_or_else(|| format!("MIR {module}.{member} host returned no value"))?;
        expected.map_or(Ok(result), |ty| self.cast(builder, result, ty))
    }

    fn call_civil_time_values(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        row: &jet_foundation::MIR::MirPreludeCall,
        mut values: Vec<Value>,
    ) -> Result<Vec<Value>, String> {
        if values.len() != row.signature.arity {
            return Err(format!(
                "MIR civil-time call `{}` expects {} arguments, got {}",
                row.member,
                row.signature.arity,
                values.len()
            ));
        }
        let (_, method) = row
            .member
            .split_once('.')
            .ok_or_else(|| format!("MIR civil-time member `{}` is malformed", row.member))?;
        if row.member == "Period.total_in" {
            let host = self.host.time.period_total_in;
            let signature = self
                .module
                .declarations()
                .get_function_decl(host)
                .signature
                .clone();
            let results = self.call_declared_values(builder, host, &signature, values)?;
            return Ok(Self::prelude_results(builder, &signature, results));
        }
        if values.len() > 8 {
            return Err(format!(
                "MIR civil-time method `{}` has too many arguments for its resident host ABI",
                row.member
            ));
        }
        let host = self.host.time.civil_method;
        let method = builder
            .ins()
            .iconst(types::I64, self.runtime.heap.alloc_string(method));
        values.insert(1, method);
        while values.len() < 9 {
            values.push(builder.ins().iconst(types::I64, 0));
        }
        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        let results = self.call_declared_values(builder, host, &signature, values)?;
        Ok(Self::prelude_results(builder, &signature, results))
    }
    fn duration_unit_discriminant(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        unit: Value,
    ) -> Result<Value, String> {
        self.enum_discriminant_value(builder, unit, false)
    }

    fn ui_backend_host(&self, member: &str) -> Option<FuncId> {
        Some(match member {
            "NullBackend.measure" | "TuiBackend.measure" | "GtkBackend.measure" => {
                self.host.ui.measure
            }
            "NullBackend.layout" | "TuiBackend.layout" | "GtkBackend.layout" => self.host.ui.layout,
            "NullBackend.paint" | "TuiBackend.paint" | "GtkBackend.paint" => self.host.ui.paint,
            "NullBackend.on_event" | "TuiBackend.on_event" | "GtkBackend.on_event" => {
                self.host.ui.on_event
            }
            "NullBackend.mount_default"
            | "TuiBackend.mount_default"
            | "GtkBackend.mount_default" => self.host.ui.mount_default,
            "NullBackend.mount" | "TuiBackend.mount" | "GtkBackend.mount" => self.host.ui.mount,
            "NullBackend.commands" => self.host.ui.commands,
            "TuiBackend.frame_lines" => self.host.ui.frame_lines,
            "TuiBackend.render_count" => self.host.ui.render_count,
            _ => return None,
        })
    }

    fn lookup_prelude_host(
        &self,
        row: &jet_foundation::MIR::MirPreludeCall,
    ) -> Result<FuncId, String> {
        let symbol = row.symbol.name();
        if row.family == jet_foundation::MIR::MirPreludeFamily::HandleMethod
            && row.module == "core.ui"
        {
            if let Some(host) = self.ui_backend_host(&row.member) {
                return Ok(host);
            }
        }
        if let Some(host) = self.host.lookup(symbol) {
            return Ok(host);
        }
        if let Some(suffix) = symbol.strip_prefix("jet_") {
            let candidate = format!("jet_jit_{suffix}");
            if let Some(host) = self.host.lookup(&candidate) {
                return Ok(host);
            }
        }
        let mut candidates = jet_foundation::Syntax::core_call(&row.module, &row.member)
            .map(|record| record.jit_symbol_candidates())
            .unwrap_or_default();
        if candidates.is_empty() {
            candidates = jet_foundation::Syntax::CORE_CALLS
                .iter()
                .filter(|record| record.module == row.module && record.member == row.member)
                .flat_map(|record| record.jit_symbol_candidates())
                .collect();
        }
        for candidate in candidates {
            if candidate == symbol {
                continue;
            }
            if let Some(host) = self.host.lookup(&candidate) {
                return Ok(host);
            }
        }
        Err(format!(
            "resolved MIR Prelude symbol `{symbol}` is not registered"
        ))
    }

    fn collection_receiver_value(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        receiver: MirValueId,
        receiver_place: Option<jet_foundation::MIR::MirPlaceId>,
    ) -> Result<Value, String> {
        match receiver_place {
            Some(place) => self
                .read_place(builder, place)
                .and_then(|value| self.cast(builder, value, types::I64)),
            None => self.cast(builder, self.value(receiver)?, types::I64),
        }
    }

    fn collection_builtin_call(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        call: jet_foundation::MIR::MirPreludeCallId,
        receiver: MirValueId,
        receiver_place: Option<jet_foundation::MIR::MirPlaceId>,
        args: &[MirValueId],
        expected: Option<types::Type>,
    ) -> Result<Option<Value>, String> {
        let Some(row) = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .cloned()
        else {
            return Err(format!("MIR Prelude call {:?} is missing", call));
        };
        if row.family != jet_foundation::MIR::MirPreludeFamily::BuiltinMethod
            || row.module != "core.builtin"
        {
            return Ok(None);
        }

        let member = row.member.as_str();
        if member == "contains"
            && sequence_element_type(&self.mir_value_type(receiver)?)
                .and_then(comparison_element_kind)
                == Some(ComparisonElementKind::String)
        {
            let [needle] = args else {
                return Err("MIR List.contains expects one value argument".to_string());
            };
            let receiver = self.collection_receiver_value(builder, receiver, receiver_place)?;
            let needle = self.cast(builder, self.value(*needle)?, types::I64)?;
            let value = self
                .call_host(
                    builder,
                    self.host.coll.list_contains_str,
                    &[receiver, needle],
                )?
                .first()
                .copied()
                .ok_or_else(|| "MIR List.contains host returned no value".to_string())?;
            return self.bool_value(builder, value).map(Some);
        }
        let supported = matches!(
            member,
            "list_try_new"
                | "list_try_with_capacity"
                | "list_push"
                | "list_try_push"
                | "list_try_reserve"
                | "list_insert"
                | "list_remove_value"
                | "list_remove_slot"
                | "list_pop"
                | "list_extend"
                | "list_reverse"
                | "list_concat"
                | "list_sort"
                | "list_sort_desc"
                | "list_clear"
                | "list_count"
                | "list_counts"
                | "map_top_n"
                | "map_min"
                | "map_max"
                | "map_insert"
                | "map_add_new"
                | "map_try_insert"
                | "map_remove"
                | "map_pop_first"
                | "map_merge"
                | "join"
                | "view_new"
        );
        if !supported {
            return Ok(None);
        }
        let receiver_ty = self.mir_value_type(receiver)?;
        macro_rules! receiver_value {
            () => {
                self.collection_receiver_value(builder, receiver, receiver_place)
            };
        }
        macro_rules! integer_value {
            ($id:expr) => {
                self.cast(builder, self.value($id)?, types::I64)
            };
        }
        let (host, values) = match member {
            "list_try_new" => {
                if !args.is_empty() {
                    return Err("MIR List.try_new expects no value arguments".to_string());
                }
                (self.host.coll.list_try_new, Vec::new())
            }
            "list_try_with_capacity" => {
                if args.len() != 1 {
                    return Err("MIR List.try_with_capacity expects one value argument".to_string());
                }
                (
                    self.host.coll.list_try_with_capacity,
                    vec![integer_value!(args[0])?],
                )
            }
            "list_push" | "list_try_push" => {
                if args.len() != 1 {
                    return Err(format!("MIR {member} expects one value argument"));
                }
                let receiver = receiver_value!()?;
                let value = self.value(args[0])?;
                let value_type = self.value_type(builder, value);
                let (host, value) = if value_type == types::F64 {
                    (self.host.coll.list_push_f64, value)
                } else if value_type == types::F32 {
                    (
                        self.host.coll.list_push_f64,
                        builder.ins().fpromote(types::F64, value),
                    )
                } else {
                    (
                        self.host.coll.list_push,
                        self.cast(builder, value, types::I64)?,
                    )
                };
                let host = if member == "list_try_push" {
                    if value_type == types::F64 || value_type == types::F32 {
                        self.host.coll.list_try_push_f64
                    } else {
                        self.host.coll.list_try_push
                    }
                } else {
                    host
                };
                (host, vec![receiver, value])
            }
            "list_try_reserve" => {
                if args.len() != 1 {
                    return Err("MIR List.try_reserve expects one value argument".to_string());
                }
                let receiver = receiver_value!()?;
                let amount = integer_value!(args[0])?;
                let element = sequence_element_type(&receiver_ty)
                    .and_then(comparison_element_kind)
                    .ok_or_else(|| {
                        format!(
                            "MIR List.try_reserve receiver `{}` has no supported element carrier",
                            receiver_ty.display_name()
                        )
                    })?;
                let host = match element {
                    ComparisonElementKind::Float => self.host.coll.list_try_reserve_f64,
                    ComparisonElementKind::Integer
                    | ComparisonElementKind::String
                    | ComparisonElementKind::Date => self.host.coll.list_try_reserve,
                };
                (host, vec![receiver, amount])
            }
            "list_insert" => {
                if args.len() != 2 {
                    return Err("MIR List.insert expects index and value arguments".to_string());
                }
                let receiver = receiver_value!()?;
                let index = integer_value!(args[0])?;
                let value = self.value(args[1])?;
                let value_type = self.value_type(builder, value);
                let (host, value) = if value_type == types::F64 {
                    (self.host.coll.list_insert_f64, value)
                } else if value_type == types::F32 {
                    (
                        self.host.coll.list_insert_f64,
                        builder.ins().fpromote(types::F64, value),
                    )
                } else {
                    (
                        self.host.coll.list_insert,
                        self.cast(builder, value, types::I64)?,
                    )
                };
                (host, vec![receiver, index, value])
            }
            "list_remove_value" => {
                if args.len() != 1 {
                    return Err("MIR List.remove(value) expects one value argument".to_string());
                }
                let receiver = receiver_value!()?;
                let value = self.value(args[0])?;
                let value_type = self.value_type(builder, value);
                let (host, value) = if value_type == types::F64 {
                    (self.host.coll.list_remove_value_f64, value)
                } else if value_type == types::F32 {
                    (
                        self.host.coll.list_remove_value_f64,
                        builder.ins().fpromote(types::F64, value),
                    )
                } else {
                    let element = sequence_element_type(&receiver_ty)
                        .and_then(comparison_element_kind)
                        .ok_or_else(|| {
                            format!(
                                "MIR List.remove(value) receiver `{}` has no supported element carrier",
                                receiver_ty.display_name()
                            )
                        })?;
                    match element {
                        ComparisonElementKind::String => (
                            self.host.coll.list_remove_value_str,
                            self.cast(builder, value, types::I64)?,
                        ),
                        ComparisonElementKind::Integer | ComparisonElementKind::Float => (
                            self.host.coll.list_remove_value,
                            self.cast(builder, value, types::I64)?,
                        ),
                        ComparisonElementKind::Date => {
                            return Err(
                                "MIR List.remove(value) has no resident Date equality host"
                                    .to_string(),
                            );
                        }
                    }
                };
                (host, vec![receiver, value])
            }
            "list_remove_slot" => {
                if args.len() != 1 {
                    return Err("MIR List.remove(slot) expects one index argument".to_string());
                }
                let receiver = receiver_value!()?;
                let index = integer_value!(args[0])?;
                (
                    self.host.coll.list_remove_slot_generic,
                    vec![receiver, index],
                )
            }
            "list_pop" => {
                if !args.is_empty() {
                    return Err("MIR List.pop expects no value arguments".to_string());
                }
                (self.host.coll.list_pop, vec![receiver_value!()?])
            }
            "list_extend" => {
                if args.len() != 1 {
                    return Err("MIR List.extend expects one list argument".to_string());
                }
                (
                    self.host.coll.list_extend,
                    vec![receiver_value!()?, integer_value!(args[0])?],
                )
            }
            "list_reverse" => {
                if !args.is_empty() {
                    return Err("MIR List.reverse expects no value arguments".to_string());
                }
                (self.host.coll.list_reverse, vec![receiver_value!()?])
            }
            "list_concat" => {
                if args.len() != 1 {
                    return Err("MIR List.concat expects one list argument".to_string());
                }
                (
                    self.host.coll.list_concat,
                    vec![receiver_value!()?, integer_value!(args[0])?],
                )
            }
            "list_sort" | "list_sort_desc" => {
                if !args.is_empty() {
                    return Err(format!("MIR {member} expects no value arguments"));
                }
                let element_ty = sequence_element_type(&receiver_ty);
                let fraction = element_ty.is_some_and(|ty| {
                    matches!(
                        ty.kind(),
                        jet_foundation::MIR::MirTypeKind::Apply { name, args }
                            if args.is_empty() && name.name == "Fraction"
                    )
                });
                let datetime =
                    element_ty.is_some_and(|ty| is_named_type(ty, "DateTime"));
                let date = element_ty.is_some_and(|ty| {
                    is_named_type(ty, "Date") || is_named_type(ty, "LocalDate")
                });
                let host = if fraction && member == "list_sort" {
                    self.host.coll.list_sort_fraction
                } else if datetime && member == "list_sort" {
                    self.host.coll.list_sort_datetime
                } else if date && member == "list_sort" {
                    self.host.coll.list_sort_date
                } else {
                    let element = element_ty
                        .and_then(comparison_element_kind)
                        .ok_or_else(|| {
                            format!(
                                "MIR {member} receiver `{}` has no supported element carrier",
                                receiver_ty.display_name()
                            )
                        })?;
                    match (member, element) {
                        ("list_sort", ComparisonElementKind::Integer) => self.host.coll.list_sort,
                        ("list_sort", ComparisonElementKind::Float) => self.host.coll.list_sort_f64,
                        ("list_sort", ComparisonElementKind::String) => self.host.coll.list_sort_str,
                        ("list_sort_desc", ComparisonElementKind::Integer) => {
                            self.host.coll.list_sort_desc
                        }
                        ("list_sort_desc", ComparisonElementKind::Float) => {
                            self.host.coll.list_sort_f64_desc
                        }
                        ("list_sort_desc", ComparisonElementKind::String) => {
                            self.host.coll.list_sort_str_desc
                        }
                        _ => {
                            return Err(format!(
                                "MIR {member} has no resident sort host for `{}` elements",
                                receiver_ty.display_name()
                            ));
                        }
                    }
                };
                (host, vec![receiver_value!()?])
            }
            "list_clear" => {
                if !args.is_empty() {
                    return Err("MIR collection.clear expects no value arguments".to_string());
                }
                let host = if comparison_map_parts(&receiver_ty).is_some() {
                    self.host.coll.map_clear
                } else {
                    self.host.coll.list_clear
                };
                (host, vec![receiver_value!()?])
            }
            "list_count" => {
                if args.len() != 1 {
                    return Err("MIR List.count expects one value argument".to_string());
                }
                let receiver = receiver_value!()?;
                let value = self.value(args[0])?;
                let value_type = self.value_type(builder, value);
                let (host, value) = if value_type == types::F64 {
                    (self.host.coll.list_count_f64, value)
                } else if value_type == types::F32 {
                    (
                        self.host.coll.list_count_f64,
                        builder.ins().fpromote(types::F64, value),
                    )
                } else {
                    let element = sequence_element_type(&receiver_ty)
                        .and_then(comparison_element_kind)
                        .ok_or_else(|| {
                            format!(
                                "MIR List.count receiver `{}` has no supported element carrier",
                                receiver_ty.display_name()
                            )
                        })?;
                    let (host, value) = match element {
                        ComparisonElementKind::String => (
                            self.host.coll.list_count_str,
                            self.cast(builder, value, types::I64)?,
                        ),
                        ComparisonElementKind::Integer | ComparisonElementKind::Float => (
                            self.host.coll.list_count,
                            self.cast(builder, value, types::I64)?,
                        ),
                        ComparisonElementKind::Date => {
                            return Err(
                                "MIR List.count has no resident value-count host for Date elements"
                                    .to_string(),
                            );
                        }
                    };
                    (host, value)
                };
                (host, vec![receiver, value])
            }
            "list_counts" => {
                if !args.is_empty() {
                    return Err("MIR List.counts expects no value arguments".to_string());
                }
                if sequence_element_type(&receiver_ty).and_then(comparison_element_kind)
                    != Some(ComparisonElementKind::String)
                {
                    return Err(format!(
                        "MIR List.counts receiver `{}` is not a string list",
                        receiver_ty.display_name()
                    ));
                }
                (self.host.coll.list_counts, vec![receiver_value!()?])
            }
            "map_top_n" => {
                if args.len() != 1 {
                    return Err("MIR Map.top_n expects one limit argument".to_string());
                }
                let (_, value_ty) = comparison_map_parts(&receiver_ty).ok_or_else(|| {
                    format!(
                        "MIR Map.top_n receiver `{}` has no checked map value type",
                        receiver_ty.display_name()
                    )
                })?;
                let host = if is_exact_int_type(value_ty) {
                    self.host.coll.map_top_n_int
                } else {
                    self.host.coll.map_top_n
                };
                (host, vec![receiver_value!()?, integer_value!(args[0])?])
            }
            "map_min" | "map_max" => {
                if !args.is_empty() {
                    return Err(format!("MIR {member} expects no value arguments"));
                }
                let (_, value_ty) = comparison_map_parts(&receiver_ty).ok_or_else(|| {
                    format!(
                        "MIR {member} receiver `{}` has no checked map value type",
                        receiver_ty.display_name()
                    )
                })?;
                let host = if is_exact_int_type(value_ty) {
                    if member == "map_min" {
                        self.host.coll.map_min_int
                    } else {
                        self.host.coll.map_max_int
                    }
                } else if member == "map_min" {
                    self.host.coll.map_min
                } else {
                    self.host.coll.map_max
                };
                (host, vec![receiver_value!()?])
            }
            "map_insert" | "map_add_new" | "map_try_insert" => {
                if args.len() != 2 {
                    return Err(format!("MIR {member} expects key and value arguments"));
                }
                let receiver = receiver_value!()?;
                let key = integer_value!(args[0])?;
                let value = integer_value!(args[1])?;
                let host = match (member, self.map_key_kind(args[0])?) {
                    ("map_insert", MapKeyKind::String) => self.host.coll.map_insert,
                    ("map_insert", MapKeyKind::Int) => self.host.coll.map_insert_int,
                    ("map_insert", MapKeyKind::Composite) => self.host.coll.map_insert_composite,
                    ("map_add_new", MapKeyKind::String) => self.host.coll.map_add_new,
                    ("map_add_new", MapKeyKind::Int) => self.host.coll.map_add_new_int,
                    ("map_add_new", MapKeyKind::Composite) => self.host.coll.map_add_new_composite,
                    ("map_try_insert", MapKeyKind::String) => self.host.coll.map_try_insert,
                    ("map_try_insert", MapKeyKind::Int) => self.host.coll.map_try_insert_int,
                    ("map_try_insert", MapKeyKind::Composite) => {
                        return Err(
                            "MIR Map.try_insert has no composite-key JIT host route".to_string()
                        );
                    }
                    _ => unreachable!("collection map member and key kind are exhaustive"),
                };
                (host, vec![receiver, key, value])
            }
            "map_remove" => {
                if args.len() != 1 {
                    return Err("MIR Map.remove expects one key argument".to_string());
                }
                let receiver = receiver_value!()?;
                let key = integer_value!(args[0])?;
                let host = match self.map_key_kind(args[0])? {
                    MapKeyKind::String => self.host.coll.map_remove,
                    MapKeyKind::Int => self.host.coll.map_remove_int,
                    MapKeyKind::Composite => self.host.coll.map_remove_composite,
                };
                (host, vec![receiver, key])
            }
            "map_pop_first" => {
                if !args.is_empty() {
                    return Err("MIR Map.pop_first expects no value arguments".to_string());
                }
                (self.host.coll.map_pop_first, vec![receiver_value!()?])
            }
            "map_merge" => {
                if args.len() != 1 {
                    return Err("MIR Map.merge expects one map argument".to_string());
                }
                let (key_ty, _) = comparison_map_parts(&receiver_ty).ok_or_else(|| {
                    format!(
                        "MIR Map.merge receiver `{}` has no map key type",
                        receiver_ty.display_name()
                    )
                })?;
                let host = match self.map_key_kind_for_type(key_ty)? {
                    MapKeyKind::String => self.host.coll.map_merge,
                    MapKeyKind::Int => self.host.coll.map_merge_int,
                    MapKeyKind::Composite => {
                        return Err(
                            "MIR Map.merge has no composite-key JIT host route".to_string()
                        );
                    }
                };
                (host, vec![receiver_value!()?, integer_value!(args[0])?])
            }
            "join" => {
                if args.len() != 1 {
                    return Err("MIR List.join expects one separator argument".to_string());
                }
                let inner = sequence_element_type(&receiver_ty).ok_or_else(|| {
                    format!(
                        "MIR List.join receiver `{}` has no element type",
                        receiver_ty.display_name()
                    )
                })?;
                let kind = list_format_kind(inner)?;
                let kind_value = builder.ins().iconst(types::I64, kind);
                (
                    self.host.checked_list_join,
                    vec![receiver_value!()?, integer_value!(args[0])?, kind_value],
                )
            }
            "view_new" => {
                let receiver = receiver_value!()?;
                if args.len() == 1 {
                    (
                        self.host.coll.view_new_range,
                        vec![receiver, integer_value!(args[0])?],
                    )
                } else if args.len() == 2 {
                    (
                        self.host.coll.view_new,
                        vec![receiver, integer_value!(args[0])?, integer_value!(args[1])?],
                    )
                } else {
                    return Err(format!(
                        "MIR View.new expects a range or start/end, got {} arguments",
                        args.len()
                    ));
                }
            }
            _ => unreachable!("unsupported collection member passed route guard"),
        };

        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        let results = self.call_declared_values(builder, host, &signature, values)?;
        let value = Self::prelude_results(builder, &signature, results)
            .first()
            .copied()
            .ok_or_else(|| format!("MIR collection host `{member}` returned no value"))?;
        expected
            .map_or(Ok(value), |ty| self.cast(builder, value, ty))
            .map(Some)
    }
    /// Materialize checked addresses that can reach a borrowed HandleMethod
    /// operand through MIR.  JIT handle hosts consume erased value handles,
    /// while AOT renders the same operand as `&`/`&mut` around the place.
    fn borrowed_handle_place(
        &self,
        value_id: MirValueId,
        seen: &mut HashSet<MirValueId>,
    ) -> Option<MirPlaceId> {
        if !seen.insert(value_id) {
            return None;
        }
        let operation = self
            .function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find(|instruction| instruction.result == Some(value_id))
            .map(|instruction| &instruction.operation)?;
        match operation {
            MirOperation::AddressOf { place, .. } => Some(*place),
            MirOperation::Copy { value }
            | MirOperation::Move { value }
            | MirOperation::AttachTag { value, .. } => self.borrowed_handle_place(*value, seen),
            MirOperation::Phi { incoming } => {
                let mut origin = None;
                for (_, value) in incoming {
                    let place = self.borrowed_handle_place(*value, seen)?;
                    if origin.is_some_and(|candidate| candidate != place) {
                        return None;
                    }
                    origin = Some(place);
                }
                origin
            }
            MirOperation::ReadPlace(place) | MirOperation::MovePlace { place } => {
                let mut origin = None;
                let mut saw_write = false;
                for instruction in self
                    .function
                    .blocks
                    .iter()
                    .flat_map(|block| block.instructions.iter())
                {
                    let MirOperation::WritePlace {
                        place: write_place,
                        value,
                    } = &instruction.operation
                    else {
                        continue;
                    };
                    if write_place != place {
                        continue;
                    }
                    saw_write = true;
                    let candidate = self.borrowed_handle_place(*value, seen)?;
                    if origin.is_some_and(|existing| existing != candidate) {
                        return None;
                    }
                    origin = Some(candidate);
                }
                if saw_write {
                    origin
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn handle_method_value(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        value_id: MirValueId,
        borrowed: bool,
    ) -> Result<Value, String> {
        let value = self.value(value_id)?;
        if borrowed {
            let mut seen = HashSet::new();
            if let Some(place) = self.borrowed_handle_place(value_id, &mut seen) {
                return self.read_place(builder, place);
            }
        }
        Ok(value)
    }

    fn call_prelude_values(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        id: jet_foundation::MIR::MirPreludeCallId,
        mut values: Vec<Value>,
    ) -> Result<Vec<Value>, String> {
        let row = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == id)
            .cloned()
            .ok_or_else(|| format!("MIR Prelude call {:?} is missing", id))?;
        if row.family == jet_foundation::MIR::MirPreludeFamily::StaticPrelude
            && row.module == "core.time"
            && self.time_static_host(&row.member).is_some()
        {
            return self
                .call_time_static_values(builder, &row.member, values)?
                .ok_or_else(|| format!("MIR time.{} host route disappeared", row.member));
        }
        if Self::service_module_id(&row.module).is_some() {
            return self.call_service_values(builder, &row.module, &row.member, values, false);
        }
        if row.family == jet_foundation::MIR::MirPreludeFamily::HandleMethod
            && row.module == "core.time"
            && row.member.split_once('.').is_some_and(|(kind, method)| {
                jet_codegen::Codegen::TIR::is_civil_time_method_name(Some(kind), method)
            })
        {
            return self.call_civil_time_values(builder, &row, values);
        }
        // Resident exact Duration hosts take the DurationUnit discriminant;
        // MIR's ordinary enum carrier is the tagged record that stores it.
        if row.family == jet_foundation::MIR::MirPreludeFamily::HandleMethod
            && matches!(
                (row.module.as_str(), row.member.as_str()),
                ("core.time", "duration_new") | ("core.handle", "duration.in")
            )
        {
            if values.len() != 2 {
                return Err(format!(
                    "MIR Duration call `{}` expects a value and DurationUnit",
                    row.member
                ));
            }
            let unit = values
                .pop()
                .expect("checked Duration call has a unit operand");
            values.push(self.duration_unit_discriminant(builder, unit)?);
        }

        let host = self.lookup_prelude_host(&row)?;
        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        let results = self.call_declared_values(builder, host, &signature, values)?;
        Ok(Self::prelude_results(builder, &signature, results))
    }
    fn plugin_load_args(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        args: &[MirCallArg],
        expected: Option<types::Type>,
        host: FuncId,
        signature: &Signature,
    ) -> Result<Value, String> {
        if args.len() != 2 || signature.params.len() != 3 {
            return Err(format!(
                "MIR plugin load expects two checked arguments and three resident ABI arguments, got {} and {}",
                args.len(),
                signature.params.len()
            ));
        }
        let mut user_signature = signature.clone();
        user_signature.params.truncate(2);
        let mut values = self.lower_call_args(builder, args, &user_signature)?;
        let needs = self.program.facts.authority_needs.join("\n");
        values.push(
            builder
                .ins()
                .iconst(types::I64, self.runtime.heap.alloc_string(needs)),
        );
        let result = self
            .call_declared_values(builder, host, signature, values)?
            .first()
            .copied()
            .ok_or_else(|| "MIR plugin load returned no value".to_string())?;
        expected.map_or(Ok(result), |ty| self.cast(builder, result, ty))
    }

    fn plugin_call_method(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        call: jet_foundation::MIR::MirPreludeCallId,
        receiver: MirValueId,
        args: &[MirValueId],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "MIR plugin call expects an export name and one checked value list, got {}",
                args.len()
            ));
        }
        let list_ty = self.mir_value_type(args[1])?;
        let mut element_ty = &list_ty;
        loop {
            match &element_ty.kind {
                MirTypeKind::List(inner) => {
                    element_ty = inner;
                    break;
                }
                MirTypeKind::FixedList { elem, .. } => {
                    element_ty = elem;
                    break;
                }
                MirTypeKind::InlineRange { base, .. } | MirTypeKind::Tagged { inner: base, .. } => {
                    element_ty = base.as_ref()
                }
                _ => {
                    return Err(format!(
                        "MIR plugin call parameter `{}` is not a checked list",
                        list_ty.display_name()
                    ));
                }
            }
        }
        let component = jet_codegen::Codegen::Embedding::ComponentSignature {
            params: vec![element_ty.clone()],
            result: element_ty.clone(),
        };
        let descriptor =
            jet_codegen::Codegen::Embedding::component_descriptor(self.program, &component)
                .ok_or_else(|| {
                    format!(
                        "MIR plugin call parameter `{}` has no checked component descriptor",
                        element_ty.display_name()
                    )
                })?;
        let descriptor = builder.ins().iconst(
            types::I64,
            self.runtime.heap.alloc_string(descriptor.params[0].wire()),
        );
        let symbol = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .map(|row| row.symbol.name().to_owned())
            .ok_or_else(|| format!("MIR plugin call {:?} is missing", call))?;
        let host = self
            .host
            .lookup(&symbol)
            .ok_or_else(|| format!("resolved MIR plugin symbol `{symbol}` is not registered"))?;
        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        if signature.params.len() != 4 {
            return Err(format!(
                "MIR plugin symbol `{symbol}` has {} resident parameters, expected four",
                signature.params.len()
            ));
        }
        let values = vec![
            self.value(receiver)?,
            self.value(args[0])?,
            self.value(args[1])?,
            descriptor,
        ];
        let result = self
            .call_declared_values(builder, host, &signature, values)?
            .first()
            .copied()
            .ok_or_else(|| "MIR plugin call returned no value".to_string())?;
        expected.map_or(Ok(result), |ty| self.cast(builder, result, ty))
    }

    fn call_prelude_args(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        id: jet_foundation::MIR::MirPreludeCallId,
        args: &[MirCallArg],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        self.call_prelude_args_bound(builder, id, args, &[], expected)
    }

    /// Prelude host call whose function-typed arguments at `callbacks` are
    /// handed over as universal-thunk slots.  Core rows without a resident
    /// `jit_symbol` reach their host through this route, so it binds the same
    /// callbacks as the direct-symbol path in `call_core`.
    fn call_prelude_args_bound(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        id: jet_foundation::MIR::MirPreludeCallId,
        args: &[MirCallArg],
        callbacks: &[CallbackShape],
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let row = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == id)
            .ok_or_else(|| format!("MIR Prelude call {:?} is missing", id))?;
        if Self::service_module_id(&row.module).is_some() {
            return self.call_service_args(builder, &row.module, &row.member, args, expected);
        }
        if row.family == jet_foundation::MIR::MirPreludeFamily::StaticPrelude
            && row.module == "core.time"
        {
            if let Some(value) = self.call_time_static_args(builder, &row.member, args, expected)? {
                return Ok(value);
            }
        }
        if row.module == "core.text.fmt" && matches!(row.member.as_str(), "display" | "debug") {
            let debug = row.member == "debug";
            let [arg] = args else {
                return Err(format!(
                    "MIR formatting route expects one argument, got {}",
                    args.len()
                ));
            };
            let ty = self.mir_value_type(arg.value)?;
            let value = self.value(arg.value)?;
            let text = if debug {
                self.debug_value_of_type(builder, &ty, value)?
            } else {
                let packed_optional = self.packed_optional_subject(arg.value);
                self.display_value_of_type(builder, &ty, value, packed_optional)?
            };
            return expected.map_or(Ok(text), |ty| self.cast(builder, text, ty));
        }
        let symbol = row.symbol.name().to_owned();
        let host = self.lookup_prelude_host(row)?;
        let signature = self
            .module
            .declarations()
            .get_function_decl(host)
            .signature
            .clone();
        if symbol == "jet_plugin_load" {
            return self.plugin_load_args(builder, args, expected, host, &signature);
        }
        let mut values = if let Some((source_signature, key)) = self.callback_return_key(
            builder,
            &row.module,
            &row.member,
            args,
            callbacks,
            &signature,
        )? {
            let mut values = self.lower_call_args(builder, args, &source_signature)?;
            values.push(key);
            values
        } else {
            self.lower_core_call_args(builder, &row.module, &row.member, args, &signature)?
        };
        self.bind_core_callbacks(builder, args, callbacks, &mut values)?;
        let results = self.call_declared_values(builder, host, &signature, values)?;
        let result = Self::prelude_results(builder, &signature, results)
            .first()
            .copied()
            .ok_or_else(|| "MIR Prelude call returned no value".to_string())?;
        expected.map_or(Ok(result), |ty| self.cast(builder, result, ty))
    }

    fn lower_call_args(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        args: &[MirCallArg],
        signature: &Signature,
    ) -> Result<Vec<Value>, String> {
        self.lower_call_args_with_mode(builder, args, signature, false)
    }

    /// Direct JIT Core hosts receive resident handles, not addresses of the
    /// compiler's checked write places.  Keep this escape hatch local to the
    /// direct Core projection; ordinary Prelude/user write calls retain their
    /// address ABI through `lower_call_args`.
    fn lower_core_direct_args(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        args: &[MirCallArg],
        signature: &Signature,
    ) -> Result<Vec<Value>, String> {
        self.lower_call_args_with_mode(builder, args, signature, true)
    }
    fn lower_core_call_args(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        module: &str,
        member: &str,
        args: &[MirCallArg],
        signature: &Signature,
    ) -> Result<Vec<Value>, String> {
        if module == "core.host" || (module == "core.compute" && member == "set") {
            self.lower_core_direct_args(builder, args, signature)
        } else {
            self.lower_call_args(builder, args, signature)
        }
    }

    fn lower_call_args_with_mode(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        args: &[MirCallArg],
        signature: &Signature,
        write_as_value: bool,
    ) -> Result<Vec<Value>, String> {
        if signature.params.len() != args.len() {
            return Err(format!(
                "MIR call ABI expects {} arguments, got {}",
                signature.params.len(),
                args.len()
            ));
        }
        args.iter()
            .zip(signature.params.iter())
            .map(|(arg, parameter)| {
                if arg.access == MirAccess::Write && arg.place.is_none() {
                    return Err("MIR write call argument has no checked place".to_string());
                }
                if let Some(coercion) = &arg.fn_coercion {
                    let carrier = clif_ty_from_mir(&coercion.ty)
                        .ok_or_else(|| "MIR function coercion has no ABI".to_string())?;
                    if carrier != types::I64 {
                        return Err(format!(
                            "MIR function coercion carrier {carrier} is not a function handle"
                        ));
                    }
                }
                if let Some(coercion) = &arg.widen_to_union {
                    self.validate_union_coercion(coercion)?;
                }
                if let Some(type_id) = arg.box_as_trait {
                    let target = self
                        .program
                        .type_instances
                        .iter()
                        .find(|instance| instance.identity == Some(type_id))
                        .ok_or_else(|| {
                            format!(
                                "MIR trait coercion target {:?} has no instance row",
                                type_id
                            )
                        })?;
                    if !matches!(target.kind(), MirTypeKind::TraitObject(_)) {
                        return Err(format!(
                            "MIR trait coercion target {:?} is not a trait object",
                            type_id
                        ));
                    }
                }
                let value = if arg.access == MirAccess::Write {
                    let place = arg.place.expect("checked MIR write argument place");
                    if write_as_value {
                        self.read_place(builder, place)?
                    } else {
                        self.address_of(builder, place)?
                    }
                } else {
                    self.value(arg.value)?
                };
                let mut value = if let Some(coercion) = &arg.widen_to_union {
                    self.wrap_union(builder, coercion, arg.value, value)?
                } else {
                    value
                };
                if arg.box_as_trait.is_some() {
                    let source_type = self.mir_value_type(arg.value)?;
                    let source_id = source_type.identity.ok_or_else(|| {
                        "MIR trait coercion source has no type identity".to_string()
                    })?;
                    let source_id_value = builder.ins().iconst(types::I64, source_id.0 as i64);
                    let record = self.cast(builder, value, types::I64)?;
                    value = self
                        .call_host(
                            builder,
                            self.host.trait_object_tag,
                            &[record, source_id_value],
                        )?
                        .first()
                        .copied()
                        .ok_or_else(|| "MIR trait coercion host returned no record".to_string())?;
                }
                self.cast(builder, value, parameter.value_type)
            })
            .collect()
    }
    fn validate_union_coercion(&self, coercion: &MirUnionCoercion) -> Result<(), String> {
        let definition = self
            .program
            .types
            .iter()
            .find(|definition| definition.id == coercion.union)
            .ok_or_else(|| format!("MIR union type {:?} is missing", coercion.union))?;
        let MirTypeDefKind::Enum { variants, .. } = &definition.kind else {
            return Err(format!(
                "MIR union type {:?} is not an enum carrier",
                coercion.union
            ));
        };
        if !variants
            .iter()
            .any(|variant| variant.name == coercion.variant)
        {
            return Err(format!(
                "MIR union {:?} has no variant `{}`",
                coercion.union, coercion.variant
            ));
        }
        Ok(())
    }

    fn wrap_union(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        coercion: &MirUnionCoercion,
        _source_id: MirValueId,
        value: Value,
    ) -> Result<Value, String> {
        self.validate_union_coercion(coercion)?;
        let definition = self
            .program
            .types
            .iter()
            .find(|definition| definition.id == coercion.union)
            .ok_or_else(|| format!("MIR union type {:?} is missing", coercion.union))?;
        let MirTypeDefKind::Enum { variants, .. } = &definition.kind else {
            return Err(format!(
                "MIR union type {:?} is not an enum carrier",
                coercion.union
            ));
        };
        let (discriminant, payload) = variants
            .iter()
            .enumerate()
            .find(|(_, variant)| variant.name == coercion.variant)
            .map(|(index, variant)| {
                (
                    variant.discriminant.unwrap_or(index as i64),
                    variant.payload.clone(),
                )
            })
            .ok_or_else(|| {
                format!(
                    "MIR union {:?} has no variant `{}`",
                    coercion.union, coercion.variant
                )
            })?;
        let payload_types = match payload {
            jet_foundation::MIR::MirVariantPayload::Unit => Vec::new(),
            jet_foundation::MIR::MirVariantPayload::Single(ty) => vec![ty],
            jet_foundation::MIR::MirVariantPayload::Named(fields) => {
                fields.into_iter().map(|field| field.ty).collect()
            }
        };
        let arity = payload_types.len() + 1;
        let arity_value = builder.ins().iconst(types::I64, arity as i64);
        let record = self
            .call_host(builder, self.host.struct_new, &[arity_value])?
            .first()
            .copied()
            .ok_or_else(|| "MIR union coercion host returned no record".to_string())?;
        let zero = builder.ins().iconst(types::I64, 0);
        let discriminant_value = builder.ins().iconst(types::I64, discriminant);
        let _ = self.call_host(
            builder,
            self.host.struct_set_i64,
            &[record, zero, discriminant_value],
        )?;
        if payload_types.len() == 1 {
            self.set_field(builder, record, 1, &payload_types[0], value)?;
        } else if payload_types.len() > 1 {
            let source = self.cast(builder, value, types::I64)?;
            for (index, ty) in payload_types.iter().enumerate() {
                let source_index = builder.ins().iconst(types::I64, index as i64);
                let getter = self.field_getter(ty)?;
                let field = self
                    .call_host(builder, getter, &[source, source_index])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR union payload getter returned no value".to_string())?;
                self.set_field(builder, record, index + 1, ty, field)?;
            }
        }
        Ok(record)
    }
    fn call_prelude(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        id: jet_foundation::MIR::MirPreludeCallId,
        values: Vec<Value>,
        expected: Option<types::Type>,
    ) -> Result<Value, String> {
        let value = self
            .call_prelude_values(builder, id, values)?
            .first()
            .copied()
            .ok_or_else(|| "MIR Prelude call returned no value".to_string())?;
        expected.map_or(Ok(value), |ty| self.cast(builder, value, ty))
    }

    fn call_declared_values(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        id: FuncId,
        signature: &Signature,
        values: Vec<Value>,
    ) -> Result<Vec<Value>, String> {
        if signature.params.len() != values.len() {
            let name = self
                .module
                .declarations()
                .get_function_decl(id)
                .name
                .clone()
                .unwrap_or_else(|| format!("{id:?}"));
            return Err(format!(
                "MIR call ABI for `{name}` expects {} arguments, got {}",
                signature.params.len(),
                values.len()
            ));
        }
        let args = values
            .into_iter()
            .zip(signature.params.iter())
            .map(|(value, parameter)| self.cast(builder, value, parameter.value_type))
            .collect::<Result<Vec<_>, _>>()?;
        let local = self.module.declare_func_in_func(id, builder.func);
        let call = builder.ins().call(local, &args);
        self.emit_pending_exit_check(builder);
        Ok(builder.inst_results(call).to_vec())
    }

    fn call_host(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        id: FuncId,
        values: &[Value],
    ) -> Result<Vec<Value>, String> {
        let signature = self
            .module
            .declarations()
            .get_function_decl(id)
            .signature
            .clone();
        if signature.params.len() != values.len() {
            return Err(format!(
                "host ABI expects {} arguments, got {}",
                signature.params.len(),
                values.len()
            ));
        }
        let args = values
            .iter()
            .copied()
            .zip(signature.params.iter())
            .map(|(value, parameter)| self.cast(builder, value, parameter.value_type))
            .collect::<Result<Vec<_>, _>>()?;
        let local = self.module.declare_func_in_func(id, builder.func);
        let call = builder.ins().call(local, &args);
        self.emit_pending_exit_check(builder);
        Ok(builder.inst_results(call).to_vec())
    }

    fn emit_pending_exit_check(&mut self, builder: &mut FunctionBuilder<'_>) {
        let trapped = self
            .module
            .declare_func_in_func(self.host.is_trapped, builder.func);
        let trapped_call = builder.ins().call(trapped, &[]);
        let trapped = builder.inst_results(trapped_call)[0];
        let stop_block = *self
            .stop_block
            .get_or_insert_with(|| builder.create_block());
        let resume = builder.create_block();
        builder.ins().brif(trapped, stop_block, &[], resume, &[]);
        builder.switch_to_block(resume);
        builder.seal_block(resume);
    }

    fn native_int_result(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        value: Value,
        ty: Option<&MirType>,
    ) -> Result<Value, String> {
        if ty.is_some_and(is_exact_int_type) {
            return self
                .call_host(builder, self.host.num.int_from_int, &[value])?
                .first()
                .copied()
                .ok_or_else(|| "MIR native Int result boxer returned no value".to_string());
        }
        Ok(value)
    }

    fn call_i64_binary(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        id: FuncId,
        left: Value,
        right: Value,
        line: Value,
    ) -> Result<Value, String> {
        let left = self.cast(builder, left, types::I64)?;
        let right = self.cast(builder, right, types::I64)?;
        self.call_host(builder, id, &[left, right, line])?
            .first()
            .copied()
            .ok_or_else(|| "integer arithmetic host returned no value".to_string())
    }

    fn dynamic_call_args(&self, values: &[MirValueId]) -> Vec<MirCallArg> {
        values
            .iter()
            .map(|value| MirCallArg {
                value: *value,
                place: None,
                access: jet_foundation::MIR::MirAccess::Move,
                span: self.function.span.clone(),
                label: None,
                source_index: None,
                binder_slot: None,
                spread: false,
                implicit_clone: false,
                shared_auto_clone: false,
                owned_last_use: true,
                authority_boundary: false,
                fn_coercion: None,
                widen_fixed_to_list: false,
                widen_to_union: None,
                box_as_trait: None,
            })
            .collect()
    }

    fn shared_guard_split(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        call: jet_foundation::MIR::MirPreludeCallId,
        map_call: jet_foundation::MIR::MirPreludeCallId,
        guard: MirValueId,
        first: &[MirFieldId],
        second: &[MirFieldId],
        editable: bool,
        expected: Option<types::Type>,
    ) -> Result<Option<Value>, String> {
        if first.is_empty() || second.is_empty() {
            return Err("MIR shared guard split path is empty".to_string());
        }
        let common = first
            .iter()
            .zip(second.iter())
            .take_while(|(left, right)| left == right)
            .count();
        if common == first.len()
            || common == second.len()
            || (first == second)
            || first.starts_with(second)
            || second.starts_with(first)
        {
            return Err(
                "MIR shared guard split paths must diverge after a common prefix".to_string(),
            );
        }
        let editable = builder.ins().iconst(types::I64, i64::from(editable));
        let mut mapped = self.value(guard)?;
        for field in &first[..common] {
            let field = i64::try_from(field.0)
                .map_err(|_| format!("MIR shared guard field {:?} exceeds JIT ABI", field))?;
            let field = builder.ins().iconst(types::I64, field);
            mapped = self.call_prelude(
                builder,
                map_call,
                vec![mapped, field, editable],
                Some(types::I64),
            )?;
        }
        let first_field = i64::try_from(first[common].0)
            .map_err(|_| format!("MIR shared guard field {:?} exceeds JIT ABI", first[common]))?;
        let second_field = i64::try_from(second[common].0).map_err(|_| {
            format!(
                "MIR shared guard field {:?} exceeds JIT ABI",
                second[common]
            )
        })?;
        let first_field = builder.ins().iconst(types::I64, first_field);
        let second_field = builder.ins().iconst(types::I64, second_field);
        let split = self.call_prelude(
            builder,
            call,
            vec![mapped, first_field, second_field, editable],
            Some(types::I64),
        )?;
        let zero = builder.ins().iconst(types::I64, 0);
        let one = builder.ins().iconst(types::I64, 1);
        let first_guard = self
            .call_host(builder, self.host.struct_get_i64, &[split, zero])?
            .first()
            .copied()
            .ok_or_else(|| "MIR shared guard split returned no first guard".to_string())?;
        let second_guard = self
            .call_host(builder, self.host.struct_get_i64, &[split, one])?
            .first()
            .copied()
            .ok_or_else(|| "MIR shared guard split returned no second guard".to_string())?;
        let mut first_guard = first_guard;
        for field in &first[common + 1..] {
            let field = i64::try_from(field.0)
                .map_err(|_| format!("MIR shared guard field {:?} exceeds JIT ABI", field))?;
            let field = builder.ins().iconst(types::I64, field);
            first_guard = self.call_prelude(
                builder,
                map_call,
                vec![first_guard, field, editable],
                Some(types::I64),
            )?;
        }
        let mut second_guard = second_guard;
        for field in &second[common + 1..] {
            let field = i64::try_from(field.0)
                .map_err(|_| format!("MIR shared guard field {:?} exceeds JIT ABI", field))?;
            let field = builder.ins().iconst(types::I64, field);
            second_guard = self.call_prelude(
                builder,
                map_call,
                vec![second_guard, field, editable],
                Some(types::I64),
            )?;
        }
        let arity = builder.ins().iconst(types::I64, 2);
        let pair = self
            .call_host(builder, self.host.struct_new, &[arity])?
            .first()
            .copied()
            .ok_or_else(|| "MIR shared guard split pair allocation failed".to_string())?;
        let zero = builder.ins().iconst(types::I64, 0);
        let one = builder.ins().iconst(types::I64, 1);
        let _ = self.call_host(
            builder,
            self.host.struct_set_i64,
            &[pair, zero, first_guard],
        )?;
        let _ = self.call_host(
            builder,
            self.host.struct_set_i64,
            &[pair, one, second_guard],
        )?;
        expected
            .map_or(Ok(pair), |ty| self.cast(builder, pair, ty))
            .map(Some)
    }

    fn cell_projection_handle(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        paths: &[Vec<MirFieldId>],
        editable: bool,
    ) -> Result<Value, String> {
        if paths.is_empty() || paths.len() > 2 || paths.iter().any(Vec::is_empty) {
            return Err("MIR Cell guard projection has an invalid path shape".to_string());
        }
        let paths = paths
            .iter()
            .map(|path| {
                path.iter()
                    .map(|field| {
                        let name = self.field_row(*field)?.field.name.clone();
                        Ok(name
                            .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                            .unwrap_or(&name)
                            .to_string())
                    })
                    .collect::<Result<Vec<_>, String>>()
            })
            .collect::<Result<Vec<_>, String>>()?;
        let handle = self
            .runtime
            .cells
            .register_projection(CellProjection { paths, editable });
        Ok(builder.ins().iconst(types::I64, handle))
    }

    fn cell_guard_project(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        map_call: jet_foundation::MIR::MirPreludeCallId,
        split_call: Option<jet_foundation::MIR::MirPreludeCallId>,
        guard: MirValueId,
        paths: &[Vec<MirFieldId>],
        editable: bool,
        edit_paths_disjoint: bool,
        expected: Option<types::Type>,
    ) -> Result<Option<Value>, String> {
        if paths.is_empty() || paths.iter().any(Vec::is_empty) || paths.len() > 2 {
            return Err("MIR Cell guard projection has an invalid path shape".to_string());
        }
        let mut projected = self.value(guard)?;
        if let Some(split_call) = split_call {
            if paths.len() != 2 || !edit_paths_disjoint {
                return Err(
                    "MIR Cell guard split route requires two disjoint edit paths".to_string(),
                );
            }
            let common = paths[0]
                .iter()
                .zip(paths[1].iter())
                .take_while(|(left, right)| left == right)
                .count();
            if common == paths[0].len() || common == paths[1].len() || paths[0] == paths[1] {
                return Err(
                    "MIR Cell guard split paths must diverge after a common prefix".to_string(),
                );
            }
            for field in &paths[0][..common] {
                let descriptor = self.cell_projection_handle(builder, &[vec![*field]], editable)?;
                projected = self.call_prelude(
                    builder,
                    map_call,
                    vec![projected, descriptor],
                    Some(types::I64),
                )?;
            }
            let first =
                self.cell_projection_handle(builder, &[paths[0][common..].to_vec()], editable)?;
            let second =
                self.cell_projection_handle(builder, &[paths[1][common..].to_vec()], editable)?;
            projected = self.call_prelude(
                builder,
                split_call,
                vec![projected, first, second],
                Some(types::I64),
            )?;
        } else {
            let descriptor = self.cell_projection_handle(builder, paths, editable)?;
            projected = self.call_prelude(
                builder,
                map_call,
                vec![projected, descriptor],
                Some(types::I64),
            )?;
        }
        expected
            .map_or(Ok(projected), |ty| self.cast(builder, projected, ty))
            .map(Some)
    }

    fn core_closure(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        call: jet_foundation::MIR::MirPreludeCallId,
        kind: MirCoreClosureKind,
        values: &[MirValueId],
        closure: Option<MirValueId>,
        expected: Option<types::Type>,
    ) -> Result<Option<Value>, String> {
        let preview_source = match &kind {
            MirCoreClosureKind::UiPreview {
                source_file,
                source_start_line,
                source_start_column,
                source_end_line,
                source_end_column,
                build_id,
                revision,
                ..
            } => Some((
                source_file.clone(),
                *source_start_line,
                *source_start_column,
                *source_end_line,
                *source_end_column,
                build_id.clone(),
                revision.clone(),
            )),
            _ => None,
        };
        let arity = self
            .program
            .prelude_calls
            .iter()
            .find(|row| row.id == call)
            .ok_or_else(|| {
                format!(
                    "MIR references missing Core closure Prelude call {:?}",
                    call
                )
            })?
            .signature
            .arity;
        let closure = match &kind {
            MirCoreClosureKind::OnInterrupt => {
                if closure.is_some() {
                    return Err("MIR interrupt closure must use its callback value".to_string());
                }
                None
            }
            MirCoreClosureKind::Spawn
            | MirCoreClosureKind::Realtime
            | MirCoreClosureKind::Serve
            | MirCoreClosureKind::Guard
            | MirCoreClosureKind::OnCommit
            | MirCoreClosureKind::OnRollback
            | MirCoreClosureKind::ReactiveDerived
            | MirCoreClosureKind::ReactiveEffect
            | MirCoreClosureKind::UiMount
            | MirCoreClosureKind::UiAction
            | MirCoreClosureKind::UiTextInputOnDrop
            | MirCoreClosureKind::UiPreview { .. } => Some(
                closure
                    .ok_or_else(|| format!("MIR {:?} closure call has no closure value", kind))?,
            ),
        };
        let mut argument_ids = Vec::with_capacity(values.len() + usize::from(closure.is_some()));
        argument_ids.extend_from_slice(values);
        if let Some(closure) = closure {
            argument_ids.push(closure);
        }
        if argument_ids.len() != arity {
            return Err(format!(
                "MIR {:?} closure route expects {arity} arguments, got {}",
                kind,
                argument_ids.len()
            ));
        }
        let values = argument_ids
            .into_iter()
            .map(|id| self.value(id))
            .collect::<Result<Vec<_>, _>>()?;
        let result = self.call_prelude(builder, call, values, expected)?;
        let Some((
            source_file,
            source_start_line,
            source_start_column,
            source_end_line,
            source_end_column,
            build_id,
            revision,
        )) = preview_source
        else {
            return Ok(Some(result));
        };
        let source_id = builder.ins().iconst(
            types::I64,
            self.runtime.heap.alloc_string(source_file.clone()),
        );
        let source_file = builder
            .ins()
            .iconst(types::I64, self.runtime.heap.alloc_string(source_file));
        let build_id = builder
            .ins()
            .iconst(types::I64, self.runtime.heap.alloc_string(build_id));
        let revision = builder
            .ins()
            .iconst(types::I64, self.runtime.heap.alloc_string(revision));
        let args = [
            result,
            source_id,
            source_file,
            build_id,
            revision,
            builder
                .ins()
                .iconst(types::I64, i64::from(source_start_line)),
            builder
                .ins()
                .iconst(types::I64, i64::from(source_start_column)),
            builder.ins().iconst(types::I64, i64::from(source_end_line)),
            builder
                .ins()
                .iconst(types::I64, i64::from(source_end_column)),
        ];
        let attached = self
            .call_host(builder, self.host.ui.core_preview_source_attach, &args)?
            .first()
            .copied()
            .ok_or_else(|| "JIT UI preview source attachment returned no value".to_string())?;
        Ok(Some(attached))
    }
    fn build_value_list(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        values: &[Value],
    ) -> Result<Value, String> {
        let list = self
            .call_host(builder, self.host.coll.list_new, &[])?
            .first()
            .copied()
            .ok_or_else(|| "MIR list host returned no value".to_string())?;
        for value in values {
            let ty = self.value_type(builder, *value);
            let (host, value) = if ty == types::F64 {
                (self.host.coll.list_push_f64, *value)
            } else if ty == types::F32 {
                (
                    self.host.coll.list_push_f64,
                    builder.ins().fpromote(types::F64, *value),
                )
            } else {
                (
                    self.host.coll.list_push,
                    self.cast(builder, *value, types::I64)?,
                )
            };
            let _ = self.call_host(builder, host, &[list, value])?;
        }
        Ok(list)
    }

    fn typed_text_interp(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        call: jet_foundation::MIR::MirPreludeCallId,
        kind: jet_foundation::Syntax::TypedHeadKind,
        literals: &[String],
        holes: &[MirValueId],
        expected: Option<types::Type>,
    ) -> Result<Option<Value>, String> {
        let literal_values = literals
            .iter()
            .map(|literal| {
                builder
                    .ins()
                    .iconst(types::I64, self.runtime.heap.alloc_string(literal.clone()))
            })
            .collect::<Vec<_>>();
        let literal_list = self.build_value_list(builder, &literal_values)?;
        let hole_values = match kind {
            jet_foundation::Syntax::TypedHeadKind::SQL => holes
                .iter()
                .map(|id| self.value(*id))
                .collect::<Result<Vec<_>, _>>()?,
            jet_foundation::Syntax::TypedHeadKind::HTML
            | jet_foundation::Syntax::TypedHeadKind::Sh
            | jet_foundation::Syntax::TypedHeadKind::URL
            | jet_foundation::Syntax::TypedHeadKind::Path
            | jet_foundation::Syntax::TypedHeadKind::DateTime => holes
                .iter()
                .map(|id| {
                    let ty = self.mir_value_type(*id)?;
                    if matches!(ty.kind(), MirTypeKind::String)
                        || ty.nominal_name() == Some(jet_foundation::Syntax::TYPE_HTML)
                    {
                        self.cast(builder, self.value(*id)?, types::I64)
                    } else {
                        self.debug_value(builder, *id)
                    }
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        let hole_list = self.build_value_list(builder, &hole_values)?;
        let mut values = vec![literal_list, hole_list];
        if matches!(kind, jet_foundation::Syntax::TypedHeadKind::HTML) {
            let trusted = holes
                .iter()
                .map(|id| {
                    let ty = self.mir_value_type(*id)?;
                    Ok(builder.ins().iconst(
                        types::I8,
                        i64::from(ty.nominal_name() == Some(jet_foundation::Syntax::TYPE_HTML)),
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            values.push(self.build_value_list(builder, &trusted)?);
        }
        self.call_prelude(builder, call, values, expected).map(Some)
    }

    fn c_callback(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        call: jet_foundation::MIR::MirPreludeCallId,
        callback: jet_foundation::MIR::MirCallbackId,
        lambda: MirValueId,
        expected: Option<types::Type>,
    ) -> Result<Option<Value>, String> {
        let row = self
            .program
            .callbacks
            .iter()
            .find(|row| row.id == callback)
            .ok_or_else(|| format!("MIR callback ID {:?} has no adapter row", callback))?;
        let target = self
            .program
            .functions
            .iter()
            .find(|function| function.id == row.function)
            .ok_or_else(|| format!("MIR callback target {:?} is missing", row.function))?;
        let (lambda_target, captures) = self
            .function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find_map(|instruction| {
                (instruction.result == Some(lambda)).then(|| match &instruction.operation {
                    MirOperation::Closure {
                        function, captures, ..
                    } => {
                        if row.managed || captures.is_empty() {
                            Ok((*function, captures.clone()))
                        } else {
                            Err("MIR C callback lambda captures values".to_string())
                        }
                    }
                    _ => Err("MIR C callback lambda is not a Closure operation".to_string()),
                })
            })
            .ok_or_else(|| format!("MIR C callback lambda {:?} has no producer", lambda))??;
        if lambda_target != row.function {
            return Err(format!(
                "MIR C callback {:?} lambda target {:?} disagrees with adapter target {:?}",
                callback, lambda_target, row.function
            ));
        }
        if !matches!(&target.form, MirFunctionForm::TopLevel) {
            return Err("MIR C callback target is not a top-level callable".to_string());
        }
        if row.managed {
            if captures.len() != target.capture_params.len() {
                return Err(format!(
                    "managed MIR callback {:?} captures {}, expected {}",
                    callback,
                    captures.len(),
                    target.capture_params.len()
                ));
            }
            for (index, (operand, capture)) in
                captures.iter().zip(&target.capture_params).enumerate()
            {
                if capture.slot != index || capture.access != MirAccess::Read {
                    return Err(
                        "managed MIR callback captures must be ordered read captures".to_string(),
                    );
                }
                if !matches!(operand, MirCaptureOperand::Place(_)) {
                    return Err("managed MIR callback read capture is not a place".to_string());
                }
            }
        } else if !captures.is_empty() {
            return Err("MIR C callback lambda captures values".to_string());
        }
        if !row.managed && !target.capture_params.is_empty() {
            return Err(
                "MIR C callback target has captures not representable by its adapter".to_string(),
            );
        }
        match (&target.declared_return, &row.return_type) {
            (Some(target), Some(callback)) if !target.same_checked_type(callback) => {
                return Err(
                    "MIR C callback adapter return type disagrees with target function".to_string(),
                );
            }
            (Some(_), None) if !target.return_type.is_unit() => {
                return Err("MIR C callback adapter omits a non-unit return type".to_string());
            }
            (None, Some(_)) => {
                return Err(
                    "MIR C callback adapter adds a return type to a unit target".to_string()
                );
            }
            _ => {}
        }
        if target.params.len() != row.params.len()
            || target
                .params
                .iter()
                .zip(&row.params)
                .any(|(left, right)| !left.ty.same_checked_type(&right.ty))
        {
            return Err(
                "MIR C callback adapter parameters disagree with target function".to_string(),
            );
        }
        match (&target.return_type, &row.return_type) {
            (target, Some(callback)) if !target.same_checked_type(callback) => {
                return Err(
                    "MIR C callback adapter return type disagrees with target function".to_string(),
                )
            }
            (target, None) if !target.is_unit() => {
                return Err("MIR C callback adapter omits a non-unit return type".to_string())
            }
            _ => {}
        }
        let lambda = self.value(lambda)?;
        if row.managed {
            if row.plan_digest.as_deref().is_none_or(str::is_empty)
                || row.callback_identity.as_deref().is_none_or(str::is_empty)
            {
                return Err("managed MIR C callback is missing plan identity".to_string());
            }
            return expected
                .map_or(Ok(lambda), |ty| self.cast(builder, lambda, ty))
                .map(Some);
        }
        self.call_prelude(builder, call, vec![lambda], expected)
            .map(Some)
    }

    fn semantic(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        operation: &MirSemanticOp,
        source_line: Option<u32>,
        expected: Option<types::Type>,
    ) -> Result<Option<Value>, String> {
        match operation {
            MirSemanticOp::DataEntriesToMap { call, local } => {
                let local = self
                    .function
                    .locals
                    .iter()
                    .find(|candidate| candidate.id == *local)
                    .ok_or_else(|| format!("MIR local {:?} is missing", local))?;
                let value = self.read_place(builder, local.place)?;
                self.call_prelude(builder, *call, vec![value], expected)
                    .map(Some)
            }
            MirSemanticOp::MathBuiltin { call, args, .. }
            | MirSemanticOp::PreciseBuiltin { call, args, .. } => self
                .call_prelude(
                    builder,
                    *call,
                    args.iter()
                        .map(|id| self.value(*id))
                        .collect::<Result<Vec<_>, _>>()?,
                    expected,
                )
                .map(Some),
            MirSemanticOp::Print { call, value } => {
                // `jet_term_write_stdout_line(text, flush)`; AOT passes
                // `flush = true`, so the resident JIT does too (I9).
                let value_id = *value;
                let value = self.value(value_id)?;
                let ty = self.mir_value_type(value_id)?;
                let text = if matches!(ty.kind(), MirTypeKind::String) {
                    value
                } else {
                    let packed_optional = self.packed_optional_subject(value_id);
                    self.display_value_of_type(builder, &ty, value, packed_optional)?
                };
                let flush = builder.ins().iconst(types::I64, 1);
                self.call_prelude(builder, *call, vec![text, flush], expected)
                    .map(Some)
            }
            MirSemanticOp::AmbientInput { call, prompt } => {
                let prompt = prompt
                    .map(|id| self.value(id))
                    .transpose()?
                    .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                self.call_prelude(builder, *call, vec![prompt], expected)
                    .map(Some)
            }
            MirSemanticOp::TextPatternMatch {
                call,
                subject,
                parts,
            } => {
                let descriptor = self.runtime.register_text_pattern(parts);
                let descriptor = builder.ins().iconst(types::I64, descriptor);
                self.call_prelude(
                    builder,
                    *call,
                    vec![self.value(*subject)?, descriptor],
                    expected,
                )
                .map(Some)
            }
            MirSemanticOp::BinaryPatternMatch {
                call,
                subject,
                parts,
            } => {
                let descriptor = self.runtime.register_binary_pattern(parts);
                let descriptor = builder.ins().iconst(types::I64, descriptor);
                self.call_prelude(
                    builder,
                    *call,
                    vec![self.value(*subject)?, descriptor],
                    expected,
                )
                .map(Some)
            }
            MirSemanticOp::RequireStop {
                call,
                kind,
                condition,
                location,
                context,
                values,
                always_stops: _,
            } => {
                let panic_context = self.panic_context_values(builder, location, context)?;
                let mut args = match kind {
                    MirRequireKind::Require => {
                        let Some(condition) = *condition else {
                            return Err("MIR require stop has no checked condition".to_string());
                        };
                        match values.as_slice() {
                            [] => vec![
                                self.value(condition)?,
                                builder
                                    .ins()
                                    .iconst(types::I64, self.runtime.heap.alloc_string("condition failed")),
                            ],
                            [message] => vec![self.value(condition)?, self.value(*message)?],
                            _ => {
                                return Err(
                                    "MIR require stop expects one checked condition and an optional message"
                                        .to_string(),
                                )
                            }
                        }
                    }
                    MirRequireKind::RequireEq => {
                        let Some(condition) = *condition else {
                            return Err("MIR require_eq stop has no checked condition".to_string());
                        };
                        let [left, right] = values.as_slice() else {
                            return Err(
                                "MIR require_eq stop expects exactly two values".to_string()
                            );
                        };
                        vec![
                            self.value(condition)?,
                            self.debug_value(builder, *left)?,
                            self.debug_value(builder, *right)?,
                        ]
                    }
                    MirRequireKind::Panic => {
                        if condition.is_some() {
                            return Err(
                                "MIR panic stop carries an unexpected condition".to_string()
                            );
                        }
                        match values.as_slice() {
                            [message] => {
                                let [file, line, fn_name, src_line, col, caret, locals] =
                                    panic_context;
                                vec![
                                    file,
                                    line,
                                    fn_name,
                                    src_line,
                                    col,
                                    caret,
                                    self.value(*message)?,
                                    locals,
                                ]
                            }
                            _ => {
                                return Err(
                                    "MIR panic stop expects exactly one message value".to_string()
                                )
                            }
                        }
                    }
                };
                if !matches!(kind, MirRequireKind::Panic) {
                    args.extend(panic_context);
                }
                Ok(Some(self.call_prelude(builder, *call, args, expected)?))
            }
            MirSemanticOp::LayoutCompare {
                call, left, right, ..
            } => self
                .call_prelude(
                    builder,
                    *call,
                    vec![self.value(*left)?, self.value(*right)?],
                    expected,
                )
                .map(Some),
            MirSemanticOp::LayoutLiteral { inner } => Ok(Some(self.value(*inner)?)),
            MirSemanticOp::StructLiteral {
                type_id, fields, ..
            } => Ok(Some(self.aggregate(builder, *type_id, fields)?)),
            MirSemanticOp::CellGuardProject {
                map_call,
                split_call,
                guard,
                paths,
                editable,
                edit_paths_disjoint,
            } => self.cell_guard_project(
                builder,
                *map_call,
                *split_call,
                *guard,
                paths,
                *editable,
                *edit_paths_disjoint,
                expected,
            ),
            MirSemanticOp::SharedGuardMap {
                call,
                guard,
                path,
                editable,
            } => {
                let editable = builder.ins().iconst(types::I64, i64::from(*editable));
                let mut mapped = self.value(*guard)?;
                for field in path {
                    let field_id = i64::try_from(field.0).map_err(|_| {
                        format!("MIR shared guard field {:?} exceeds JIT ABI", field)
                    })?;
                    let field = builder.ins().iconst(types::I64, field_id);
                    mapped =
                        self.call_prelude(builder, *call, vec![mapped, field, editable], None)?;
                }
                expected
                    .map_or(Ok(mapped), |ty| self.cast(builder, mapped, ty))
                    .map(Some)
            }
            MirSemanticOp::SharedGuardSplit {
                call,
                map_call,
                guard,
                first,
                second,
                editable,
            } => self.shared_guard_split(
                builder, *call, *map_call, *guard, first, second, *editable, expected,
            ),
            MirSemanticOp::ClosureMethod {
                call,
                receiver,
                args,
            } => {
                if let Some(value) = self
                    .checked_collection_callback_call(builder, *call, *receiver, args, expected)?
                {
                    Ok(Some(value))
                } else {
                    let prelude = self
                        .program
                        .prelude_calls
                        .iter()
                        .find(|row| row.id == *call);
                    let is_plot = prelude.and_then(plot_callback_shape).is_some();
                    let is_view = prelude.and_then(view_callback_shape).is_some();
                    if is_plot {
                        self.plot_column_call(builder, *call, *receiver, args, expected)
                            .map(Some)
                    } else if is_view {
                        self.view_callback_call(builder, *call, *receiver, args, expected)
                            .map(Some)
                    } else {
                        let mut values = Vec::with_capacity(args.len() + 1);
                        values.push(MirCallArg {
                            value: *receiver,
                            place: None,
                            access: jet_foundation::MIR::MirAccess::Read,
                            span: args
                                .first()
                                .map(|arg| arg.span.clone())
                                .unwrap_or_else(|| self.function.span.clone()),
                            label: None,
                            source_index: None,
                            binder_slot: None,
                            spread: false,
                            implicit_clone: false,
                            shared_auto_clone: false,
                            owned_last_use: false,
                            authority_boundary: false,
                            fn_coercion: None,
                            widen_fixed_to_list: false,
                            widen_to_union: None,
                            box_as_trait: None,
                        });
                        values.extend(args.iter().cloned());
                        let callbacks = core_callback_shapes(self.function, args)?
                            .into_iter()
                            .map(|(arity, index)| (arity, index + 1))
                            .collect::<Vec<_>>();
                        self.call_prelude_args_bound(builder, *call, &values, &callbacks, expected)
                            .map(Some)
                    }
                }
            }
            MirSemanticOp::StaticPreludeCall {
                call,
                args,
                owner_type_args,
                type_args,
            } => {
                let is_typed_codec_encode = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .is_some_and(|row| {
                        row.family == jet_foundation::MIR::MirPreludeFamily::StaticPrelude
                            && row.module == "core.encoding.codec"
                            && row.member == "encode"
                    });
                if is_typed_codec_encode {
                    if args.len() != 2 || type_args.len() != 1 {
                        return Err("MIR typed codec encode requires one checked value and type"
                            .to_string());
                    }
                    let value = self.value(args[1].value)?;
                    let value =
                        thunk_encode_raw(builder, value, builder.func.dfg.value_type(value))?;
                    let type_key = self.runtime.heap.alloc_string(type_args[0].identity_key());
                    let type_key = builder.ins().iconst(types::I64, type_key);
                    let result = self
                        .call_host(
                            builder,
                            self.host.encoding.codec_encode_typed,
                            &[value, type_key],
                        )?
                        .first()
                        .copied()
                        .ok_or_else(|| {
                            "MIR typed codec encode host returned no value".to_string()
                        })?;
                    return expected
                        .map_or(Ok(result), |ty| self.cast(builder, result, ty))
                        .map(Some);
                }
                let is_typed_codec_decode = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .is_some_and(|row| {
                        row.family == jet_foundation::MIR::MirPreludeFamily::StaticPrelude
                            && row.module == "core.encoding.codec"
                            && row.member == "decode_typed"
                    });
                if is_typed_codec_decode {
                    let row = self
                        .program
                        .prelude_calls
                        .iter()
                        .find(|row| row.id == *call)
                        .ok_or_else(|| format!("MIR Prelude call {:?} is missing", call))?;
                    if type_args.len() != 1 {
                        return Err(
                            "MIR typed codec decode requires one checked target type argument"
                                .to_string(),
                        );
                    }
                    let host = self.lookup_prelude_host(row)?;
                    let signature = self
                        .module
                        .declarations()
                        .get_function_decl(host)
                        .signature
                        .clone();
                    if signature.params.len() != args.len() + 1 {
                        return Err(format!(
                            "MIR typed codec decode host expects {} source arguments plus type metadata, got {}",
                            signature.params.len().saturating_sub(1),
                            args.len()
                        ));
                    }
                    let mut source_signature = signature.clone();
                    source_signature.params.pop();
                    let mut values = self.lower_call_args(builder, args, &source_signature)?;
                    let type_key = self.runtime.heap.alloc_string(type_args[0].identity_key());
                    values.push(builder.ins().iconst(types::I64, type_key));
                    let result = self
                        .call_declared_values(builder, host, &signature, values)?
                        .first()
                        .copied()
                        .ok_or_else(|| {
                            "MIR typed codec decode host returned no value".to_string()
                        })?;
                    return expected
                        .map_or(Ok(result), |ty| self.cast(builder, result, ty))
                        .map(Some);
                }

                let is_atomic_constructor = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .is_some_and(|row| {
                        row.family == jet_foundation::MIR::MirPreludeFamily::StaticPrelude
                            && row.module == "::JetAtomic"
                            && matches!(row.member.as_str(), "new" | "try_new")
                    });
                if !is_atomic_constructor {
                    return self
                        .call_prelude_args(builder, *call, args, expected)
                        .map(Some);
                }
                let constructor_member = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .map(|row| row.member.clone())
                    .ok_or_else(|| format!("MIR Prelude call {:?} is missing", call))?;
                let constructor_symbol = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .map(|row| row.symbol.name().to_owned())
                    .ok_or_else(|| format!("MIR Prelude call {:?} is missing", call))?;
                let expected_symbol = format!("JetAtomic::{constructor_member}");
                if constructor_symbol != expected_symbol {
                    return Err(format!(
                        "MIR Atomic constructor has symbol `{constructor_symbol}`, expected `{expected_symbol}`"
                    ));
                }
                if args.len() != 1 || owner_type_args.len() != 1 || !type_args.is_empty() {
                    return Err(
                        "MIR Atomic constructor expects one scalar argument and one owner type argument"
                            .to_string(),
                    );
                }
                let owner_ty = match &owner_type_args[0] {
                    MirPreludeTypeArg::Type(ty) => ty,
                    MirPreludeTypeArg::HostUsize => {
                        return Err(
                            "MIR Atomic constructor owner argument must be a scalar type"
                                .to_string(),
                        )
                    }
                };
                let kind = atomic_scalar_kind_tag(owner_ty).ok_or_else(|| {
                    format!(
                        "MIR Atomic constructor inner type `{}` is not a supported scalar",
                        owner_ty.display_name()
                    )
                })?;
                let value = self.cast(builder, self.value(args[0].value)?, types::I64)?;
                let kind = builder.ins().iconst(types::I64, kind);
                self.call_prelude(builder, *call, vec![value, kind], expected)
                    .map(Some)
            }
            MirSemanticOp::HostCall { call, args } => self
                .call_prelude_args(builder, *call, args, expected)
                .map(Some),
            MirSemanticOp::HardwareCall {
                call,
                op,
                receiver,
                args,
            } => self
                .hardware_call(builder, *call, op, *receiver, args, expected)
                .map(Some),
            MirSemanticOp::DecodeUnder {
                call,
                segment,
                inner,
            } => self
                .call_prelude(
                    builder,
                    *call,
                    vec![self.value(*segment)?, self.value(*inner)?],
                    expected,
                )
                .map(Some),
            MirSemanticOp::BuiltinMethod {
                call,
                receiver,
                receiver_place,
                args,
                ..
            } => {
                let list_min_max = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .is_some_and(|row| {
                        row.family == jet_foundation::MIR::MirPreludeFamily::BuiltinMethod
                            && row.module == "core.list"
                            && row.member == "min_max"
                    });
                if list_min_max {
                    if !args.is_empty() {
                        return Err("MIR List.min_max expects no value arguments".to_string());
                    }
                    let receiver_ty = self.mir_value_type(*receiver)?;
                    let element = sequence_element_type(&receiver_ty)
                        .and_then(comparison_element_kind)
                        .ok_or_else(|| {
                            format!(
                                "MIR List.min_max receiver `{}` has no supported element carrier",
                                receiver_ty.display_name()
                            )
                        })?;
                    let host = match element {
                        ComparisonElementKind::Integer => self.host.coll.list_min_max,
                        ComparisonElementKind::Float => self.host.coll.list_min_max_f64,
                        ComparisonElementKind::String => self.host.coll.list_min_max_str,
                        ComparisonElementKind::Date => {
                            return Err(
                                "MIR List.min_max has no resident Date ordering host".to_string()
                            );
                        }
                    };
                    let receiver =
                        self.collection_receiver_value(builder, *receiver, *receiver_place)?;
                    let value = self
                        .call_host(builder, host, &[receiver])?
                        .first()
                        .copied()
                        .ok_or_else(|| "MIR List.min_max host returned no value".to_string())?;
                    return expected
                        .map_or(Ok(value), |ty| self.cast(builder, value, ty))
                        .map(Some);
                }
                if let Some(value) =
                    self.iterator_builtin_call(builder, *call, *receiver, args, expected)?
                {
                    return Ok(Some(value));
                }
                if let Some(value) = self.collection_builtin_call(
                    builder,
                    *call,
                    *receiver,
                    *receiver_place,
                    args,
                    expected,
                )? {
                    return Ok(Some(value));
                }
                let atomic_builtin = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .is_some_and(|row| {
                        row.family == jet_foundation::MIR::MirPreludeFamily::BuiltinMethod
                            && row.module == "core.mem"
                            && is_atomic_builtin_member(&row.member)
                    });
                if atomic_builtin {
                    if receiver_place.is_some() {
                        return Err(
                            "MIR Atomic methods require a value receiver, not a receiver place"
                                .to_string(),
                        );
                    }
                    let receiver_ty = self.mir_value_type(*receiver)?;
                    let MirTypeKind::Apply {
                        name,
                        args: type_args,
                    } = receiver_ty.kind()
                    else {
                        return Err(
                            "MIR Atomic receiver is not an Atomic<T> application".to_string()
                        );
                    };
                    if name.name != "Atomic"
                        || type_args.len() != 1
                        || atomic_scalar_kind_tag(&type_args[0]).is_none()
                    {
                        return Err(format!(
                            "MIR Atomic receiver inner type `{}` is not a supported scalar",
                            receiver_ty.display_name()
                        ));
                    }
                    let route = self
                        .program
                        .prelude_calls
                        .iter()
                        .find(|row| row.id == *call)
                        .ok_or_else(|| format!("MIR Prelude call {:?} is missing", call))?;
                    let expected_symbol = format!("jet_atomic_{}", route.member);
                    if route.symbol.name() != expected_symbol {
                        return Err(format!(
                            "MIR Atomic route `{}` has symbol `{}`, expected `{}`",
                            route.member,
                            route.symbol.name(),
                            expected_symbol
                        ));
                    }
                }
                let core_builtin = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .is_some_and(|row| {
                        row.family == jet_foundation::MIR::MirPreludeFamily::BuiltinMethod
                            && row.module == "core.builtin"
                    });
                let receiver = if core_builtin {
                    self.collection_receiver_value(builder, *receiver, *receiver_place)?
                } else {
                    match receiver_place {
                        Some(place) => self.address_of(builder, *place)?,
                        None => self.value(*receiver)?,
                    }
                };
                let values = std::iter::once(Ok(receiver))
                    .chain(args.iter().map(|id| self.value(*id)))
                    .collect::<Result<Vec<_>, String>>()?;
                self.call_prelude(builder, *call, values, expected)
                    .map(Some)
            }
            MirSemanticOp::HandleMethod {
                call,
                receiver,
                args,
                frame_schedule,
                frame_schedule_derivation,
            } => {
                let allocator_method = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .and_then(|row| {
                        if row.module != "core.handle" {
                            return None;
                        }
                        let (owner, method) = row.member.split_once('.')?;
                        matches!(owner, "Arena" | "Bump" | "Pool" | "Fixed")
                            .then_some(method.to_owned())
                    });
                if let Some(method) = allocator_method {
                    let receiver = self.handle_method_value(builder, *receiver, true)?;
                    match method.as_str() {
                        "alloc" | "try_alloc" => {
                            let [value_id] = args.as_slice() else {
                                return Err(format!(
                                    "MIR allocator `{method}` expects one checked value"
                                ));
                            };
                            let value = self.value(*value_id)?;
                            let requested = builder.ins().iconst(
                                types::I64,
                                allocator_requested_bytes(&self.mir_value_type(*value_id)?),
                            );
                            let host = if method == "alloc" {
                                self.host.memory.allocator_alloc
                            } else {
                                self.host.memory.allocator_try_alloc
                            };
                            let result = self
                                .call_host(builder, host, &[receiver, value, requested])?
                                .first()
                                .copied()
                                .ok_or_else(|| {
                                    format!("MIR allocator `{method}` host returned no value")
                                })?;
                            return expected
                                .map_or(Ok(result), |ty| self.cast(builder, result, ty))
                                .map(Some);
                        }
                        "reset" => {
                            if !args.is_empty() {
                                return Err(
                                    "MIR allocator reset received unexpected arguments".to_string()
                                );
                            }
                            let _ = self.call_host(
                                builder,
                                self.host.memory.allocator_reset,
                                &[receiver],
                            )?;
                            let unit = builder.ins().iconst(types::I64, 0);
                            return expected
                                .map_or(Ok(unit), |ty| self.cast(builder, unit, ty))
                                .map(Some);
                        }
                        _ => {}
                    }
                }
                let callback_event_stop = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .is_some_and(|row| {
                        row.module == "core.ffi" && row.member == "callback_event_stop"
                    });
                if callback_event_stop {
                    if !args.is_empty() {
                        return Err("managed callback event.stop takes no arguments".to_string());
                    }
                    let value = self
                        .call_host(builder, self.host.ffi.callback_event_stop, &[])?
                        .first()
                        .copied()
                        .ok_or_else(|| {
                            "managed callback event.stop host returned no value".to_string()
                        })?;
                    return expected
                        .map_or(Ok(value), |ty| self.cast(builder, value, ty))
                        .map(Some);
                }
                let plugin_call = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .is_some_and(|row| row.symbol.name() == "jet_plugin_call");
                if plugin_call {
                    return self
                        .plugin_call_method(builder, *call, *receiver, args, expected)
                        .map(Some);
                }
                let app_method = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .is_some_and(|row| row.module == "core.web.app");
                if app_method {
                    let has_callback = self
                        .program
                        .prelude_calls
                        .iter()
                        .find(|row| row.id == *call)
                        .is_some_and(|row| app_callback_shape(row).is_some());
                    return if has_callback {
                        self.app_callback_call(builder, *call, *receiver, args, expected)
                    } else {
                        self.app_plain_method_call(builder, *call, *receiver, args, expected)
                    }
                    .map(Some);
                }
                let row = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .cloned()
                    .ok_or_else(|| format!("MIR Prelude call {:?} is missing", call))?;
                let operand_count = args.len() + 1;
                if row.signature.borrow_mask.len() < operand_count {
                    return Err(format!(
                        "MIR HandleMethod route `{}` has {} operands but only {} borrow flags",
                        row.member,
                        operand_count,
                        row.signature.borrow_mask.len()
                    ));
                }
                let mut values = Vec::with_capacity(operand_count);
                for (index, value_id) in std::iter::once(*receiver)
                    .chain(args.iter().copied())
                    .enumerate()
                {
                    values.push(self.handle_method_value(
                        builder,
                        value_id,
                        row.signature.borrow_mask[index],
                    )?);
                }
                if let Some(metadata) = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .and_then(|row| row.db_metadata.as_ref())
                {
                    values.push(builder.ins().iconst(
                        types::I64,
                        self.runtime.heap.alloc_string(metadata.to_wire()),
                    ));
                }
                if self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .is_some_and(|row| row.member == "game.scene_on_frame")
                {
                    let schedule = frame_schedule
                        .as_ref()
                        .map(|schedule| self.runtime.heap.alloc_string(schedule.canonical_json()))
                        .unwrap_or(0);
                    let derivation = frame_schedule_derivation
                        .as_ref()
                        .map(|reference| self.runtime.heap.alloc_string(reference.id.clone()))
                        .unwrap_or(0);
                    values.push(builder.ins().iconst(types::I64, schedule));
                    values.push(builder.ins().iconst(types::I64, derivation));
                }
                self.call_prelude(builder, *call, values, expected)
                    .map(Some)
            }
            MirSemanticOp::OptionLift2 {
                call,
                function,
                left,
                right,
            } => self
                .call_prelude(
                    builder,
                    *call,
                    vec![
                        self.value(*function)?,
                        self.value(*left)?,
                        self.value(*right)?,
                    ],
                    expected,
                )
                .map(Some),
            MirSemanticOp::HostBorrowCallback { callable, params } => {
                let callable_ty = self.mir_value_type(*callable)?;
                if clif_ty_from_mir(&callable_ty) != Some(types::I64)
                    || params.iter().any(|param| clif_ty_from_mir(param).is_none())
                {
                    return Err(
                        "MIR host-borrow callback has no checked function carrier".to_string()
                    );
                }
                let value = self.value(*callable)?;
                expected
                    .map_or(Ok(value), |ty| self.cast(builder, value, ty))
                    .map(Some)
            }
            MirSemanticOp::PolicyFunction { policy, values }
            | MirSemanticOp::InterruptFunction {
                interrupt: policy,
                values,
            } => self
                .indirect_call(builder, *policy, &self.dynamic_call_args(values), expected)
                .map(Some),
            MirSemanticOp::NumericMethod { call, receiver } => self
                .call_prelude(builder, *call, vec![self.value(*receiver)?], expected)
                .map(Some),
            MirSemanticOp::NumericBinaryMethod {
                call,
                receiver,
                argument,
            } => self
                .call_prelude(
                    builder,
                    *call,
                    vec![self.value(*receiver)?, self.value(*argument)?],
                    expected,
                )
                .map(Some),
            MirSemanticOp::OverflowOption {
                call,
                left,
                right,
                location,
            } => {
                let mut values = vec![self.value(*left)?, self.value(*right)?];
                values.extend(self.overflow_location(builder, *call, location.as_ref())?);
                self.call_prelude(builder, *call, values, expected)
                    .map(Some)
            }
            MirSemanticOp::GcEdit {
                call,
                root,
                edges,
                edit,
                index,
                kind,
                site,
            } => {
                let mut values = vec![self.value(*root)?];
                match kind {
                    MirGcEditKind::Clear | MirGcEditKind::Pop | MirGcEditKind::Plain => {
                        if index.is_some() || !edges.is_empty() {
                            return Err(
                                "MIR GC edit carries unused index or edge operands".to_string()
                            );
                        }
                    }
                    MirGcEditKind::RemoveIndex => {
                        if !edges.is_empty() {
                            return Err("MIR GC remove edit carries edge operands".to_string());
                        }
                        values.push(self.value(
                            index.ok_or_else(|| "MIR GC remove edit has no index".to_string())?,
                        )?);
                    }
                    MirGcEditKind::InsertIndex => {
                        values.push(self.value(
                            index.ok_or_else(|| "MIR GC insert edit has no index".to_string())?,
                        )?);
                        values.push(self.build_list(builder, edges)?);
                    }
                    MirGcEditKind::Prepend | MirGcEditKind::Additive => {
                        if index.is_some() {
                            return Err("MIR GC edge edit carries an unused index".to_string());
                        }
                        values.push(self.build_list(builder, edges)?);
                    }
                    MirGcEditKind::EdgeSlot => {
                        if index.is_some() {
                            return Err("MIR GC edge-slot edit carries an unused index".to_string());
                        }
                        values.push(self.build_list(builder, edges)?);
                    }
                }
                values.push(self.value(*edit)?);
                if matches!(kind, MirGcEditKind::EdgeSlot) {
                    let site = i64::try_from(site.0)
                        .map_err(|_| "MIR GC edit site exceeds Cranelift ABI".to_string())?;
                    values.push(builder.ins().iconst(types::I64, site));
                }
                self.call_prelude(builder, *call, values, expected)
                    .map(Some)
            }
            MirSemanticOp::TypedTextInterp {
                call,
                kind,
                literals,
                holes,
            } => self.typed_text_interp(builder, *call, *kind, literals, holes, expected),
            MirSemanticOp::CCallback {
                call,
                callback,
                lambda,
            } => self.c_callback(builder, *call, *callback, *lambda, expected),
            MirSemanticOp::HttpRouterRegister {
                call,
                receiver,
                path,
                handler,
                method,
                contract_json,
                handler_param_names: _,
                location,
            } => {
                let row = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .ok_or_else(|| format!("MIR Prelude call {:?} is missing", call))?;
                let mut values = vec![
                    self.value(*receiver)?,
                    builder
                        .ins()
                        .iconst(types::I64, self.runtime.heap.alloc_string(method.as_str())),
                    self.value(*path)?,
                    self.value(*handler)?,
                ];
                if !matches!(row.member.as_str(), "mux_add" | "mux_add_zero") {
                    values.extend(self.panic_location(builder, location)?);
                    values.push(builder.ins().iconst(
                        types::I64,
                        self.runtime.heap.alloc_string(contract_json.clone()),
                    ));
                }
                self.call_prelude(builder, *call, values, expected)
                    .map(Some)
            }
            MirSemanticOp::CoreClosureCall {
                call,
                kind,
                values,
                closure,
                ..
            } => self.core_closure(builder, *call, kind.clone(), values, *closure, expected),
            MirSemanticOp::TaskGroup { call, tasks, .. } => self
                .call_prelude(
                    builder,
                    *call,
                    tasks
                        .iter()
                        .map(|id| self.value(*id))
                        .collect::<Result<Vec<_>, _>>()?,
                    expected,
                )
                .map(Some),
            MirSemanticOp::Select { call, values, .. } => self
                .call_prelude(
                    builder,
                    *call,
                    values
                        .iter()
                        .map(|id| self.value(*id))
                        .collect::<Result<Vec<_>, _>>()?,
                    expected,
                )
                .map(Some),
            MirSemanticOp::CarrierFact {
                call,
                receiver,
                field,
                notes,
            } => {
                // `jet_partial` / `jet_notes` own the carrier meaning (AOT:
                // `jet_partial(&outcome, |report| report.<field>.clone())`).
                // The resident host takes the checked report field index and
                // answers the packed Option carrier (`0` absent, `payload + 1`
                // present) or the notes list, as the interpreter's
                // `eval_carrier_fact` does over the same Outcome readers.
                let row = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .ok_or_else(|| format!("MIR Prelude call {:?} is missing", call))?;
                if row.abi != jet_foundation::MIR::MirPreludeAbi::Value {
                    return Err("MIR carrier fact route has unsupported Prelude ABI".to_string());
                }
                let (index, _) = self.field_info(*field)?;
                let result = self.value(*receiver)?;
                let field = builder.ins().iconst(types::I64, index as i64);
                let notes = builder.ins().iconst(types::I8, i64::from(*notes));
                let value = self
                    .call_host(
                        builder,
                        self.host.core.carrier_fact,
                        &[result, field, notes],
                    )?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR carrier fact host returned no value".to_string())?;
                expected.map_or(Ok(Some(value)), |ty| {
                    self.cast(builder, value, ty).map(Some)
                })
            }
            MirSemanticOp::PluginInvoke {
                call,
                handle,
                export_name,
                signature,
                args,
            } => {
                // The resident `Plugin` carrier is the raw sandbox handle the
                // load row returned; AOT reads `.handle` off `JetPlugin` and the
                // interpreter reads the `handle` field of the same record. The
                // `jet_plugin_call` host decodes the checked value list against
                // the Component descriptor wire, encodes the params, and
                // answers the same `Result<T, String>` carrier as both tiers.
                if args.len() != signature.params.len() {
                    return Err(format!(
                        "MIR plugin export `{export_name}` has {} checked parameters but {} values",
                        signature.params.len(),
                        args.len()
                    ));
                }
                let receiver = self.value(*handle)?;
                let name = builder.ins().iconst(
                    types::I64,
                    self.runtime.heap.alloc_string(export_name.clone()),
                );
                let params = self.build_list(builder, args)?;
                let descriptor = builder
                    .ins()
                    .iconst(types::I64, self.runtime.heap.alloc_string(signature.wire()));
                self.call_prelude(
                    builder,
                    *call,
                    vec![receiver, name, params, descriptor],
                    expected,
                )
                .map(Some)
            }
            MirSemanticOp::SharedGuardWait {
                call,
                guard,
                condition,
                predicate,
            } => {
                // AOT `JetSharedGuard::wait`: `loop { if ready(&*guard) { return
                // Ok(()) } jet_shared_guard_wait_once(state, condition)? }`.
                // The predicate is the checked callable value; the wait kernel
                // owns release, park, reacquire, and cancellation, and its
                // failure carrier is returned as-is.
                let row = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .ok_or_else(|| format!("MIR Prelude call {:?} is missing", call))?;
                if row.abi != jet_foundation::MIR::MirPreludeAbi::Control {
                    return Err(
                        "MIR shared guard wait route has unsupported Prelude ABI".to_string()
                    );
                }
                let predicate_ty = self.mir_value_type(*predicate)?;
                let (params, ret) = callable_signature(&predicate_ty).ok_or_else(|| {
                    "MIR shared guard wait predicate has no function signature".to_string()
                })?;
                let [param] = params else {
                    return Err(
                        "MIR shared guard wait predicate takes exactly one value".to_string()
                    );
                };
                let param_ty = clif_ty_from_mir(param).ok_or_else(|| {
                    "MIR shared guard wait predicate parameter has no ABI".to_string()
                })?;
                let ret_ty = ret.and_then(clif_ty_from_mir).ok_or_else(|| {
                    "MIR shared guard wait predicate has no Bool result".to_string()
                })?;
                let mut signature = Signature::new(self.module.target_config().default_call_conv);
                signature.params.push(AbiParam::new(param_ty));
                signature.returns.push(AbiParam::new(ret_ty));
                let reader = match param.layout.abi {
                    MirAbi::Scalar(MirScalarKind::Float | MirScalarKind::Float32) => {
                        self.host.memory.shared_guard_value_f64
                    }
                    MirAbi::Scalar(MirScalarKind::Bool) => self.host.memory.shared_guard_value_bool,
                    MirAbi::Scalar(MirScalarKind::Char) => self.host.memory.shared_guard_value_char,
                    MirAbi::Scalar(MirScalarKind::Int | MirScalarKind::Pointer)
                    | MirAbi::Aggregate
                    | MirAbi::Sequence
                    | MirAbi::Function
                    | MirAbi::Nominal
                    | MirAbi::Dynamic => self.host.memory.shared_guard_value,
                    MirAbi::Never => {
                        return Err("MIR shared guard wait predicate parameter is never".to_string())
                    }
                };
                let guard = self.cast(builder, self.value(*guard)?, types::I64)?;
                let condition = self.cast(builder, self.value(*condition)?, types::I64)?;
                let predicate = self.cast(builder, self.value(*predicate)?, types::I64)?;
                let header = builder.create_block();
                let body = builder.create_block();
                let done = builder.create_block();
                builder.append_block_param(done, types::I64);
                builder.ins().jump(header, &[]);
                builder.switch_to_block(header);
                let payload = self
                    .call_host(builder, reader, &[guard])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR shared guard value reader returned no value".to_string())?;
                let payload = self.cast(builder, payload, param_ty)?;
                let ready = self.call_callable_values(
                    builder,
                    predicate,
                    signature,
                    vec![payload],
                    Some(ret_ty),
                )?;
                let ready = self.bool_value(builder, ready)?;
                let ok_tag = builder.ins().iconst(types::I8, 1);
                let zero = builder.ins().iconst(types::I64, 0);
                let ok = self
                    .call_host(builder, self.host.result_new_i64, &[ok_tag, zero])?
                    .first()
                    .copied()
                    .ok_or_else(|| {
                        "MIR shared guard wait Ok carrier returned no value".to_string()
                    })?;
                builder.ins().brif(ready, done, &[ok], body, &[]);
                builder.switch_to_block(body);
                let waited = self
                    .call_host(
                        builder,
                        self.host.memory.shared_guard_wait_once,
                        &[guard, condition],
                    )?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR shared guard wait kernel returned no value".to_string())?;
                let wait_ok = self
                    .call_host(builder, self.host.result_is_ok, &[waited])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR shared guard wait probe returned no value".to_string())?;
                let wait_ok = self.bool_value(builder, wait_ok)?;
                builder.ins().brif(wait_ok, header, &[], done, &[waited]);
                builder.switch_to_block(done);
                let result =
                    builder.block_params(done).first().copied().ok_or_else(|| {
                        "MIR shared guard wait merge block has no result".to_string()
                    })?;
                expected
                    .map_or(Ok(result), |ty| self.cast(builder, result, ty))
                    .map(Some)
            }
            MirSemanticOp::ConditionNotify {
                call,
                condition,
                all,
            } => {
                // AOT `JetCondition::notify_all` / `notify_one`; the resident
                // rows wake the same condition protocol and answer Unit.
                let row = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .ok_or_else(|| format!("MIR Prelude call {:?} is missing", call))?;
                if row.abi != jet_foundation::MIR::MirPreludeAbi::Value {
                    return Err(
                        "MIR condition notify route has unsupported Prelude ABI".to_string()
                    );
                }
                let condition = self.cast(builder, self.value(*condition)?, types::I64)?;
                let host = if *all {
                    self.host.memory.condition_notify_all
                } else {
                    self.host.memory.condition_notify_one
                };
                let _ = self.call_host(builder, host, &[condition])?;
                let unit = builder.ins().iconst(types::I64, 0);
                expected
                    .map_or(Ok(unit), |ty| self.cast(builder, unit, ty))
                    .map(Some)
            }
            MirSemanticOp::AllocNew {
                call,
                kind,
                args,
                ..
            } => {
                let row = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *call)
                    .ok_or_else(|| format!("MIR Prelude call {:?} is missing", call))?;
                if row.abi != jet_foundation::MIR::MirPreludeAbi::Value {
                    return Err("MIR AllocNew route has unsupported Prelude ABI".to_string());
                }
                let expected_member = match kind {
                    jet_foundation::MIR::MirAllocatorKind::Arena => "arena.new",
                    jet_foundation::MIR::MirAllocatorKind::Bump => "bump.new",
                    jet_foundation::MIR::MirAllocatorKind::Pool => "pool.new",
                    jet_foundation::MIR::MirAllocatorKind::Fixed
                        if row.member == "fixed.over" =>
                    {
                        "fixed.over"
                    }
                    jet_foundation::MIR::MirAllocatorKind::Fixed => "fixed.new",
                };
                if row.member != expected_member {
                    return Err("MIR AllocNew route does not match its checked kind".to_string());
                }
                let kind_code = match kind {
                    jet_foundation::MIR::MirAllocatorKind::Arena => 0,
                    jet_foundation::MIR::MirAllocatorKind::Bump => 1,
                    jet_foundation::MIR::MirAllocatorKind::Pool => 2,
                    jet_foundation::MIR::MirAllocatorKind::Fixed => 1,
                };
                let allocator = if matches!(
                    kind,
                    jet_foundation::MIR::MirAllocatorKind::Arena
                        | jet_foundation::MIR::MirAllocatorKind::Bump
                        | jet_foundation::MIR::MirAllocatorKind::Pool
                ) && args.is_empty()
                {
                    let code = builder.ins().iconst(types::I64, kind_code);
                    self.call_host(builder, self.host.memory.allocator_new_named, &[code])?
                } else {
                    let bytes = args
                        .first()
                        .map(|arg| self.cast(builder, self.value(arg.value)?, types::I64))
                        .transpose()?
                        .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                    let capacity_kind = match kind {
                        jet_foundation::MIR::MirAllocatorKind::Arena => 0,
                        jet_foundation::MIR::MirAllocatorKind::Bump => 2,
                        jet_foundation::MIR::MirAllocatorKind::Pool => 3,
                        jet_foundation::MIR::MirAllocatorKind::Fixed => 1,
                    };
                    let fixed = builder.ins().iconst(types::I64, capacity_kind);
                    self.call_host(
                        builder,
                        self.host.memory.allocator_new_capacity,
                        &[bytes, fixed],
                    )?
                }
                .first()
                .copied()
                .ok_or_else(|| "MIR allocator constructor returned no value".to_string())?;
                expected
                    .map_or(Ok(allocator), |ty| self.cast(builder, allocator, ty))
                    .map(Some)
            }
            MirSemanticOp::ColumnarRead {
                base,
                index,
                column,
                column_index: _,
                accessor,
            } => {
                // AOT: `jet_columns_gather_cell(&store, column, index)` then the
                // arithmetic stop with the instruction's source line on a bounds
                // failure. The resident tier gathers the whole row through the
                // Prelude's own column store (one E3010 stop, same wording) and
                // reads the checked field slot, whose declared order is the
                // column order the store was built with.
                let row = self
                    .program
                    .prelude_calls
                    .iter()
                    .find(|row| row.id == *accessor)
                    .ok_or_else(|| format!("MIR Prelude call {:?} is missing", accessor))?;
                if row.abi != jet_foundation::MIR::MirPreludeAbi::Value {
                    return Err("MIR columnar accessor has unsupported Prelude ABI".to_string());
                }
                let line = source_line.ok_or_else(|| {
                    "MIR columnar read is missing its instruction source location".to_string()
                })?;
                let list = self.cast(builder, self.value(*base)?, types::I64)?;
                let at =
                    self.index_operand(builder, *index, jet_foundation::MIR::MirIndexKind::List)?;
                let line = builder.ins().iconst(types::I32, i64::from(line));
                let record = self
                    .call_host(builder, self.host.coll.columnar_gather, &[list, at, line])?
                    .first()
                    .copied()
                    .ok_or_else(|| "MIR columnar gather returned no value".to_string())?;
                self.field_load_value(builder, record, *column, expected)
                    .map(Some)
            }
        }
    }

    fn trap(&mut self, builder: &mut FunctionBuilder<'_>) -> Result<Value, String> {
        let zero = builder.ins().iconst(types::I64, 0);
        let _ = self.call_host(builder, self.host.trap_panic, &[zero])?;
        Ok(zero)
    }

    fn fixed_integer_carrier(
        &self,
        builder: &mut FunctionBuilder<'_>,
        value: Value,
        source: &MirType,
        target: types::Type,
    ) -> Result<Value, String> {
        let actual = self.value_type(builder, value);
        if !actual.is_int() || !target.is_int() {
            return Err(format!(
                "cannot cast non-integer carrier {actual} -> {target}"
            ));
        }
        if actual == target {
            return Ok(value);
        }
        let (signed, _) = source
            .fixed_int()
            .ok_or_else(|| "fixed integer carrier cast has no fixed source type".to_string())?;
        if actual.bits() < target.bits() {
            Ok(if signed {
                builder.ins().sextend(target, value)
            } else {
                builder.ins().uextend(target, value)
            })
        } else if actual.bits() > target.bits() {
            Ok(builder.ins().ireduce(target, value))
        } else {
            Ok(value)
        }
    }

    fn numeric_cast(
        &mut self,
        builder: &mut FunctionBuilder<'_>,
        value_id: MirValueId,
        target: &MirType,
    ) -> Result<Value, String> {
        let source = self.mir_value_type(value_id)?;
        let value = self.value(value_id)?;
        let target_carrier = clif_ty_from_mir(target).ok_or_else(|| {
            format!(
                "MIR numeric cast target `{}` has no Cranelift ABI",
                target.display_name()
            )
        })?;

        if matches!(source.kind(), MirTypeKind::Int) {
            if matches!(target.kind(), MirTypeKind::Int) {
                return Ok(value);
            }
            if target.is_float() {
                let converted = self
                    .call_host(builder, self.host.num.int_to_f64, &[value])?
                    .first()
                    .copied()
                    .ok_or_else(|| {
                        "MIR exact Int float conversion returned no value".to_string()
                    })?;
                return self.cast(builder, converted, target_carrier);
            }
        }

        if matches!(target.kind(), MirTypeKind::Int) {
            let (signed, _) = source.fixed_int().ok_or_else(|| {
                "MIR numeric cast to exact Int requires a fixed integer".to_string()
            })?;
            let value = self.fixed_integer_carrier(builder, value, &source, types::I64)?;
            let host = if signed {
                self.host.num.int_from_int
            } else {
                self.host.num.int_from_u64
            };
            return self
                .call_host(builder, host, &[value])?
                .first()
                .copied()
                .ok_or_else(|| "MIR exact Int constructor returned no value".to_string());
        }

        if let Some((signed, _)) = source.fixed_int() {
            if target_carrier == types::F64 || target_carrier == types::F32 {
                let value = self.fixed_integer_carrier(builder, value, &source, types::I64)?;
                return Ok(if signed {
                    builder.ins().fcvt_from_sint(target_carrier, value)
                } else {
                    builder.ins().fcvt_from_uint(target_carrier, value)
                });
            }
            if target.fixed_int().is_some() && target_carrier.is_int() {
                return self.fixed_integer_carrier(builder, value, &source, target_carrier);
            }
        }

        if source.is_float() && target.is_float() {
            let source_carrier = self.value_type(builder, value);
            return match (source_carrier, target_carrier) {
                (source, target) if source == target => Ok(value),
                (types::F32, types::F64) => Ok(builder.ins().fpromote(types::F64, value)),
                (types::F64, types::F32) => Ok(builder.ins().fdemote(types::F32, value)),
                _ => Err(format!(
                    "unsupported MIR floating-point cast {source_carrier} -> {target_carrier}"
                )),
            };
        }

        Err(format!(
            "unsupported MIR numeric cast {} -> {}",
            source.display_name(),
            target.display_name()
        ))
    }

    fn cast(
        &self,
        builder: &mut FunctionBuilder<'_>,
        value: Value,
        target: types::Type,
    ) -> Result<Value, String> {
        let source = self.value_type(builder, value);
        if source == target {
            return Ok(value);
        }
        if target == types::I64 {
            return if source == types::F64 {
                Ok(builder.ins().bitcast(types::I64, MemFlags::new(), value))
            } else if source == types::F32 {
                let bits = builder.ins().bitcast(types::I32, MemFlags::new(), value);
                Ok(builder.ins().uextend(types::I64, bits))
            } else {
                Ok(builder.ins().uextend(types::I64, value))
            };
        }
        if target == types::F64 {
            return if source == types::F32 {
                Ok(builder.ins().fpromote(types::F64, value))
            } else if source == types::I64 {
                Ok(builder.ins().bitcast(types::F64, MemFlags::new(), value))
            } else {
                Err(format!("cannot cast Cranelift {source} to f64"))
            };
        }
        if target == types::F32 {
            return if source == types::F64 {
                Ok(builder.ins().fdemote(types::F32, value))
            } else {
                Err(format!("cannot cast Cranelift {source} to f32"))
            };
        }
        if target == types::I8 || target == types::I32 {
            return if source.bits() > target.bits() {
                Ok(builder.ins().ireduce(target, value))
            } else {
                Ok(builder.ins().uextend(target, value))
            };
        }
        Err(format!("unsupported MIR carrier cast {source} -> {target}"))
    }
    /// Normalize a lowered return to the function's declared Cranelift
    /// carrier. MIR tail expressions can retain a wider intermediate carrier
    /// (for example an `i64` cast of a comparison), but a callback function's
    /// ABI is fixed by its checked return type.
    fn normalize_function_return(
        &self,
        builder: &mut FunctionBuilder<'_>,
        value: Value,
    ) -> Result<Value, String> {
        let target = clif_ty_from_mir(&self.function.return_type).ok_or_else(|| {
            format!(
                "MIR function `{}` return has no Cranelift ABI",
                self.function.key
            )
        })?;
        self.cast(builder, value, target)
    }

    /// Normalise a value to the `Bool` carrier (`I8`, exactly 0 or 1 — see
    /// `clif_ty_from_mir`). An `I8` value is already a checked `Bool`; any
    /// wider carrier is truth-tested against zero.
    fn bool_value(&self, builder: &mut FunctionBuilder<'_>, value: Value) -> Result<Value, String> {
        if self.value_type(builder, value) == types::I8 {
            Ok(value)
        } else {
            Ok(builder.ins().icmp_imm(IntCC::NotEqual, value, 0))
        }
    }
}
