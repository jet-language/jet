use cranelift_codegen::settings::{self, Configurable};
use cranelift_module::Module;
use cranelift_object::{ObjectBuilder, ObjectModule};
use jet_foundation::{
    JitBackend::RunOutcome,
    MIR::{
        MirArtifactId, MirCoreClosureKind, MirOperation, MirProgram, MirSelectKind,
        MirSemanticOp, MirTerminator,
    },
};
use jet_pkg_model::Package::ReleaseDevtoolsPolicy;

use super::gap::JitGap;
use super::resident::{
    ensure_resident_module, fresh_runtime, publish_runtime_decisions, resident_hot_swap,
    resident_run_fresh, resident_teardown,
};
use super::runtime_host::catch_jit_panic;
use super::safety::{
    artifact_entry, resident_safe_mir_function, resident_safe_mir_program,
};
use super::tiers::{plan_mir_tiers, record_trace};
use super::trace::note_jit_execution;
use super::RESIDENT_RUNTIME;

fn entry_gap(program: &MirProgram, artifact: MirArtifactId, reason: impl Into<String>) -> Option<JitGap> {
    let id = artifact_entry(program, artifact)?;
    let name = program
        .functions
        .iter()
        .find(|function| function.id == id)
        .map(|function| function.key.clone())
        .unwrap_or_else(|| "<no entry>".to_string());
    Some(JitGap::new(id, name, reason))
}

pub fn cranelift_host_supported() -> bool {
    // cranelift-jit 0.112's PLT path panics on non-x86_64 hosts. Keep the
    // default dev path safe by delegating to the tier-0 backend there.
    cfg!(target_arch = "x86_64")
}

/// Object emitted by the debug AOT Cranelift route. The caller owns the final
/// native link because it also owns output paths, C-link flags, and the
/// existing rustc fallback.
#[derive(Debug)]
pub struct DebugAotObject {
    pub bytes: Vec<u8>,
    pub entry: String,
}

/// Compile a checked MIR executable through Cranelift's object backend.
/// `Err` is a deliberate capability refusal: the caller owns the fallback
/// artifact path and reports the same checked program.
pub fn try_compile_debug_aot(
    program: &MirProgram,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<DebugAotObject, String> {
    crate::on_compiler_stack(|| {
        try_compile_debug_aot_on_stack(program, artifact, release_devtools_policy)
    })
}

fn try_compile_debug_aot_on_stack(
    program: &MirProgram,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<DebugAotObject, String> {
    // Object emission is a separate artifact path. Never let its compiled
    // bytes enter the resident JIT's process-local warm-code capture.
    super::tier_cache::abort_capture();
    if !cfg!(target_arch = "x86_64") {
        return Err(
            "Cranelift debug-AOT object emission is unavailable on this architecture".into(),
        );
    }
    let plan = plan_mir_tiers(program, artifact);
    if plan.whole_program_deopt || !plan.deopt.is_empty() {
        return Err("the checked MIR program is not fully applicable to Cranelift".into());
    }
    // Keep debug AOT at the same unoptimized Cranelift level as the resident
    // JIT; #2919 owns any opt-level change after the cross-tier corpus gate.
    let mut flags = settings::builder();
    flags
        .set("opt_level", "none")
        .map_err(|error| format!("Cranelift debug flags: {error}"))?;
    flags
        .set("use_colocated_libcalls", "false")
        .map_err(|error| format!("Cranelift debug flags: {error}"))?;
    flags
        .set("is_pic", "true")
        .map_err(|error| format!("Cranelift debug flags: {error}"))?;
    let isa_builder = cranelift_native::builder().map_err(str::to_string)?;
    let isa = isa_builder
        .finish(settings::Flags::new(flags))
        .map_err(|error| format!("Cranelift native ISA: {error}"))?;
    let mut object_builder = ObjectBuilder::new(
        isa,
        "jet-debug-aot",
        cranelift_module::default_libcall_names(),
    )
    .map_err(|error| format!("Cranelift object builder: {error}"))?;
    object_builder.per_function_section(true);
    let mut module = ObjectModule::new(object_builder);
    let host = super::runtime_host::declare_host_fns_for_module(&mut module)?;
    let mut runtime = super::resident::fresh_runtime(release_devtools_policy.clone());
    let entry_id = super::functions_compile::compile_program_object(
        &mut module,
        &host,
        program,
        artifact,
        &mut runtime,
    )?;
    let entry = module
        .declarations()
        .get_function_decl(entry_id)
        .linkage_name(entry_id)
        .into_owned();
    let bytes = module
        .finish()
        .emit()
        .map_err(|error| format!("Cranelift object emission: {error}"))?;
    Ok(DebugAotObject { bytes, entry })
}

pub(crate) fn try_resident(
    program: &MirProgram,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<RunOutcome, super::tiers::MirTierPlan> {
    if !cranelift_host_supported() {
        return Err(plan_mir_tiers(program, artifact));
    }
    super::types_meta::install_struct_redact(program);
    let plan = plan_mir_tiers(program, artifact);
    if plan.whole_program_deopt || !plan.deopt.is_empty() {
        return Err(plan);
    }
    note_jit_execution();
    match catch_jit_panic("resident run", || {
        resident_run_fresh(program, None, artifact, release_devtools_policy)
    }) {
        Ok(outcome) => {
            record_trace(plan.rows.clone());
            publish_runtime_decisions(program, artifact, &plan.rows);
            Ok(outcome)
        }
        Err(reason) => {
            let mut plan = plan;
            plan.gap = entry_gap(program, artifact, reason);
            Err(plan)
        }
    }
}
pub(crate) fn try_resident_hot_swap(
    program: &MirProgram,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<RunOutcome, super::tiers::MirTierPlan> {
    if !cranelift_host_supported() {
        return Err(plan_mir_tiers(program, artifact));
    }
    super::types_meta::install_struct_redact(program);
    let plan = plan_mir_tiers(program, artifact);
    if plan.whole_program_deopt || !plan.deopt.is_empty() {
        return Err(plan);
    }
    note_jit_execution();
    match catch_jit_panic("resident hot swap", || {
        resident_hot_swap(program, None, artifact, release_devtools_policy)
    }) {
        Ok(outcome) => {
            record_trace(plan.rows.clone());
            publish_runtime_decisions(program, artifact, &plan.rows);
            Ok(outcome)
        }
        Err(reason) => {
            let mut plan = plan;
            plan.gap = entry_gap(program, artifact, reason);
            Err(plan)
        }
    }
}

pub(crate) fn try_resident_restart(
    program: &MirProgram,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<RunOutcome, super::tiers::MirTierPlan> {
    if !cranelift_host_supported() {
        return Err(plan_mir_tiers(program, artifact));
    }
    jet_foundation::Persist::shared_clear();
    super::types_meta::install_struct_redact(program);
    let plan = plan_mir_tiers(program, artifact);
    if plan.whole_program_deopt || !plan.deopt.is_empty() {
        return Err(plan);
    }
    note_jit_execution();
    match catch_jit_panic("resident restart", || {
        resident_run_fresh(program, None, artifact, release_devtools_policy)
    }) {
        Ok(outcome) => {
            record_trace(plan.rows.clone());
            publish_runtime_decisions(program, artifact, &plan.rows);
            Ok(outcome)
        }
        Err(reason) => {
            let mut plan = plan;
            plan.gap = entry_gap(program, artifact, reason);
            Err(plan)
        }
    }
}

/// Test hook: inspect the lowered MIR switch conditions.
#[doc(hidden)]
pub fn jit_dump_mixed_switch_conds(
    program: &MirProgram,
    artifact: MirArtifactId,
) -> Vec<String> {
    artifact_entry(program, artifact)
        .and_then(|entry| program.functions.iter().find(|function| function.id == entry))
        .into_iter()
        .flat_map(|function| function.blocks.iter())
        .filter_map(|block| match &block.terminator {
            MirTerminator::Switch { arms, .. } => Some(
                arms.iter()
                    .map(|arm| format!("{:?}", arm.condition))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect()
}

/// Test hook: compile a checked MIR program through the resident JIT.
pub fn try_compile_program(
    program: &MirProgram,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<(), String> {
    crate::on_compiler_stack(|| {
        if !cranelift_host_supported() {
            return Err("cranelift-jit host path unsupported on this architecture".to_string());
        }
        let plan = plan_mir_tiers(program, artifact);
        if plan.whole_program_deopt || !plan.deopt.is_empty() {
            return Err(plan
                .gap
                .map(|gap| format!("{}: {}", gap.function_name, gap.reason))
                .unwrap_or_else(|| "checked MIR is not fully applicable to Cranelift".into()));
        }
        catch_jit_panic("compile", || {
            resident_teardown();
            RESIDENT_RUNTIME.with(|slot| {
                *slot.borrow_mut() = Some(fresh_runtime(release_devtools_policy.clone()))
            });
            ensure_resident_module(program, artifact, release_devtools_policy)
        })
    })
}

/// Test hook: run only the resident Cranelift path.
#[doc(hidden)]
pub fn run_resident_strict_for_test(
    program: &MirProgram,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<RunOutcome, String> {
    crate::on_compiler_stack(|| {
        if !cranelift_host_supported() {
            return Err("cranelift-jit host path unsupported on this architecture".to_string());
        }
        let plan = plan_mir_tiers(program, artifact);
        if plan.whole_program_deopt || !plan.deopt.is_empty() {
            return Err(plan
                .gap
                .map(|gap| format!("{}: {}", gap.function_name, gap.reason))
                .unwrap_or_else(|| "checked MIR is not fully applicable to Cranelift".into()));
        }
        try_resident(program, artifact, release_devtools_policy).map_err(|plan| {
            plan.gap
                .map(|gap| format!("{}: {}", gap.function_name, gap.reason))
                .unwrap_or_else(|| "resident Cranelift execution failed".into())
        })
    })
}

/// Test hook: names of functions in a checked MIR program.
#[doc(hidden)]
pub fn jit_program_func_names(program: &MirProgram) -> Vec<String> {
    program
        .functions
        .iter()
        .map(|function| function.name.clone())
        .collect()
}

/// Test hook: per-function resident safety. `Covered` is the only green answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResidentJitSafety {
    Covered,
    Gap(String),
    Unavailable(String),
}

/// Test hook: per-function resident safety detail.
#[doc(hidden)]
pub fn resident_jit_func_safety_detail(
    program: &MirProgram,
    name: &str,
) -> ResidentJitSafety {
    let Some(function) = program
        .functions
        .iter()
        .find(|function| function.name == name || function.key == name)
    else {
        return ResidentJitSafety::Unavailable(format!(
            "function `{name}` missing from canonical MIR"
        ));
    };
    match resident_safe_mir_function(function) {
        Ok(()) => ResidentJitSafety::Covered,
        Err(detail) => ResidentJitSafety::Gap(detail),
    }
}

/// Test hook: operation tags in the checked MIR entry function.
#[doc(hidden)]
pub fn jit_dump_main_stmts(program: &MirProgram, artifact: MirArtifactId) -> Vec<String> {
    let Some(entry) = artifact_entry(program, artifact) else {
        return vec!["<no entry>".into()];
    };
    let Some(function) = program.functions.iter().find(|function| function.id == entry) else {
        return vec!["<no entry>".into()];
    };
    function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .enumerate()
        .map(|(index, instruction)| format!("{index}:{}", jit_stmt_tag(&instruction.operation)))
        .collect()
}

/// Test hook: operation and terminator tags in the checked MIR entry function.
#[doc(hidden)]
pub fn jit_dump_main_ops(program: &MirProgram, artifact: MirArtifactId) -> Vec<String> {
    let Some(entry) = artifact_entry(program, artifact) else {
        return vec!["Mir::<no entry>".into()];
    };
    let Some(function) = program.functions.iter().find(|function| function.id == entry) else {
        return vec!["Mir::<no entry>".into()];
    };
    let mut out = Vec::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            out.push(format!(
                "MirOperation::{}",
                jit_expr_tag(&instruction.operation)
            ));
        }
        out.push(format!("MirTerminator::{}", terminator_tag(&block.terminator)));
    }
    out.sort();
    out
}

fn semantic_tag(operation: &MirSemanticOp) -> &'static str {
    match operation {
        MirSemanticOp::DataEntriesToMap { .. } => "DataEntriesToMap",
        MirSemanticOp::MathBuiltin { .. } => "MathBuiltin",
        MirSemanticOp::PreciseBuiltin { .. } => "PreciseBuiltin",
        MirSemanticOp::Print { .. } => "Print",
        MirSemanticOp::AmbientInput { .. } => "AmbientInput",
        MirSemanticOp::RequireStop { .. } => "RequireStop",
        MirSemanticOp::LayoutCompare { .. } => "LayoutCompare",
        MirSemanticOp::LayoutLiteral { .. } => "LayoutLiteral",
        MirSemanticOp::StructLiteral { .. } => "StructLiteral",
        MirSemanticOp::SharedGuardSplit { .. } => "SharedGuardSplit",
        MirSemanticOp::SharedGuardWait { .. } => "SharedGuardWait",
        MirSemanticOp::ConditionNotify { .. } => "ConditionNotify",
        MirSemanticOp::AllocNew { .. } => "AllocNew",
        MirSemanticOp::ColumnarRead { .. } => "ColumnarRead",
        MirSemanticOp::StaticPreludeCall { .. } => "StaticPreludeCall",
        MirSemanticOp::DecodeUnder { .. } => "DecodeUnder",
        MirSemanticOp::BuiltinMethod { .. } => "BuiltinMethod",
        MirSemanticOp::OptionLift2 { .. } => "OptionLift2",
        MirSemanticOp::ClosureMethod { .. } => "ClosureMethod",
        MirSemanticOp::HostBorrowCallback { .. } => "HostBorrowCallback",
        MirSemanticOp::TextPatternMatch { .. } => "TextPatternMatch",
        MirSemanticOp::BinaryPatternMatch { .. } => "BinaryPatternMatch",
        MirSemanticOp::NumericMethod { .. } => "NumericMethod",
        MirSemanticOp::NumericBinaryMethod { .. } => "NumericBinaryMethod",
        MirSemanticOp::OverflowOption { .. } => "OverflowOption",
        MirSemanticOp::HandleMethod { .. } => "HandleMethod",
        MirSemanticOp::CoreClosureCall { .. } => "CoreClosureCall",
        MirSemanticOp::TaskGroup { .. } => "TaskGroup",
        MirSemanticOp::Select { kind, .. } => match kind {
            MirSelectKind::Start => "SelectStart",
            MirSelectKind::Receive => "SelectReceive",
            MirSelectKind::After => "SelectAfter",
            MirSelectKind::Wait => "SelectWait",
        },
        MirSemanticOp::PolicyFunction { .. } => "PolicyFunction",
        MirSemanticOp::InterruptFunction { .. } => "InterruptFunction",
        MirSemanticOp::HostCall { .. } => "HostCall",
        MirSemanticOp::CellGuardProject { .. } => "CellGuardProject",
        MirSemanticOp::SharedGuardMap { .. } => "SharedGuardMap",
        MirSemanticOp::HardwareCall { .. } => "HardwareCall",
        MirSemanticOp::PluginInvoke { .. } => "PluginInvoke",
        MirSemanticOp::HttpRouterRegister { .. } => "HttpRouterRegister",
        MirSemanticOp::CarrierFact { .. } => "CarrierFact",
        MirSemanticOp::GcEdit { .. } => "GcEdit",
        MirSemanticOp::TypedTextInterp { .. } => "TypedTextInterp",
        MirSemanticOp::CCallback { .. } => "CCallback",
    }
}

/// Test hook: label one checked MIR operation.
#[doc(hidden)]
pub fn jit_expr_tag(operation: &MirOperation) -> &'static str {
    match operation {
        MirOperation::Parameter { .. } => "Parameter",
        MirOperation::Capture { .. } => "Capture",
        MirOperation::Global { .. } => "Global",
        MirOperation::Phi { .. } => "Phi",
        MirOperation::ReadPlace(_) => "ReadPlace",
        MirOperation::MovePlace { .. } => "MovePlace",
        MirOperation::WritePlace { .. } => "WritePlace",
        MirOperation::InitializeUninit { .. } => "InitializeUninit",
        MirOperation::Copy { .. } => "Copy",
        MirOperation::Move { .. } => "Move",
        MirOperation::Constant(_) => "Constant",
        MirOperation::Unary { .. } => "Unary",
        MirOperation::Binary { .. } => "Binary",
        MirOperation::BuildString { .. } => "BuildString",
        MirOperation::BuildList { .. } => "BuildList",
        MirOperation::BuildMap { .. } => "BuildMap",
        MirOperation::EnumIs { .. } => "EnumIs",
        MirOperation::EnumPayload { .. } => "EnumPayload",
        MirOperation::OptionIsSome { .. } => "OptionIsSome",
        MirOperation::OptionValue { .. } => "OptionValue",
        MirOperation::ResultIsOk { .. } => "ResultIsOk",
        MirOperation::ResultValue { .. } => "ResultValue",
        MirOperation::PatternCapture { .. } => "PatternCapture",
        MirOperation::PatternMatched { .. } => "PatternMatched",
        MirOperation::ProjectMembers { .. } => "ProjectMembers",
        MirOperation::Index { .. } => "Index",
        MirOperation::Slice { .. } => "Slice",
        MirOperation::Range { .. } => "Range",
        MirOperation::Field { .. } => "Field",
        MirOperation::Struct { .. } => "Struct",
        MirOperation::Enum { .. } => "Enum",
        MirOperation::Tuple { .. } => "Tuple",
        MirOperation::Present { .. } => "Present",
        MirOperation::Convert { .. } => "Convert",
        MirOperation::Absent => "Absent",
        MirOperation::ResultOk { .. } => "ResultOk",
        MirOperation::ResultErr { .. } => "ResultErr",
        MirOperation::Call { .. } => "Call",
        MirOperation::IndirectCall { .. } => "IndirectCall",
        MirOperation::Closure { .. } => "Closure",
        MirOperation::PtrFromAddr { .. } => "PtrFromAddr",
        MirOperation::Deref { .. } => "Deref",
        MirOperation::RawAddressOf { .. } => "RawAddressOf",
        MirOperation::AddressOf { .. } => "AddressOf",
        MirOperation::CoreCall { .. } => "CoreCall",
        MirOperation::AttachTag { .. } => "AttachTag",
        MirOperation::Todo { .. } => "Todo",
        MirOperation::Never { .. } => "Never",
        MirOperation::Semantic(operation) => semantic_tag(operation),
        MirOperation::LoopRangeInit { .. } => "LoopRangeInit",
        MirOperation::LoopRangeHasNext { .. } => "LoopRangeHasNext",
        MirOperation::LoopRangeValue { .. } => "LoopRangeValue",
        MirOperation::LoopRangeAdvance { .. } => "LoopRangeAdvance",
        MirOperation::LoopIterInit { .. } => "LoopIterInit",
        MirOperation::LoopIterHasNext { .. } => "LoopIterHasNext",
        MirOperation::LoopIterValue { .. } => "LoopIterValue",
        MirOperation::LoopIterAdvance { .. } => "LoopIterAdvance",
        MirOperation::ScopeEnter { .. } => "ScopeEnter",
        MirOperation::ScopeExit { .. } => "ScopeExit",
        MirOperation::Drop { .. } => "Drop",
    }
}

/// Test hook: label one checked MIR operation in statement position.
#[doc(hidden)]
pub fn jit_stmt_tag(operation: &MirOperation) -> &'static str {
    jit_expr_tag(operation)
}

fn terminator_tag(terminator: &MirTerminator) -> &'static str {
    match terminator {
        MirTerminator::Jump { .. } => "Jump",
        MirTerminator::Branch { .. } => "Branch",
        MirTerminator::Switch { .. } => "Switch",
        MirTerminator::Return { .. } => "Return",
        MirTerminator::Yield { .. } => "Yield",
        MirTerminator::Break { .. } => "Break",
        MirTerminator::Continue { .. } => "Continue",
        MirTerminator::Unreachable { .. } => "Unreachable",
    }
}

/// Test hook: count semantic select arms in the checked MIR entry function.
#[doc(hidden)]
pub fn jit_select_arm_counts(
    program: &MirProgram,
    artifact: MirArtifactId,
) -> Option<(usize, usize)> {
    let entry = artifact_entry(program, artifact)?;
    let function = program.functions.iter().find(|function| function.id == entry)?;
    let mut recv = 0;
    let mut after = 0;
    for operation in function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter().map(|instruction| &instruction.operation))
    {
        if let MirOperation::Semantic(MirSemanticOp::Select { kind, .. }) = operation {
            match kind {
                MirSelectKind::Receive => recv += 1,
                MirSelectKind::After => after += 1,
                MirSelectKind::Start | MirSelectKind::Wait => {}
            }
        }
    }
    (recv > 0 || after > 0).then_some((recv, after))
}

#[doc(hidden)]
pub fn jit_main_uncovered_detail(
    program: &MirProgram,
    artifact: MirArtifactId,
) -> Option<String> {
    let entry = artifact_entry(program, artifact)?;
    let function = program.functions.iter().find(|function| function.id == entry)?;
    resident_safe_mir_function(function)
        .err()
        .map(|detail| format!("entry not resident-safe: {detail}"))
}

/// Test hook: count spawn semantic sites and closure values in checked MIR.
#[doc(hidden)]
pub fn jit_spawn_stats(program: &MirProgram) -> (usize, usize) {
    let mut spawn_sites = 0;
    let mut closures = 0;
    for operation in program
        .functions
        .iter()
        .flat_map(|function| function.blocks.iter())
        .flat_map(|block| block.instructions.iter().map(|instruction| &instruction.operation))
    {
        match operation {
            MirOperation::Closure { .. } => closures += 1,
            MirOperation::Semantic(MirSemanticOp::CoreClosureCall {
                kind: MirCoreClosureKind::Spawn,
                ..
            }) => spawn_sites += 1,
            _ => {}
        }
    }
    (spawn_sites, closures)
}

/// Test hook: whether the checked MIR program is resident-JIT safe.
#[doc(hidden)]
pub fn resident_jit_safe_program(program: &MirProgram) -> bool {
    resident_jit_safe_program_detail(program).is_empty()
}

/// Test hook: explain why the checked MIR program is not resident-JIT safe.
#[doc(hidden)]
pub fn resident_jit_safe_program_detail(program: &MirProgram) -> String {
    if !cranelift_host_supported() {
        return "cranelift-jit host path unsupported on this architecture".into();
    }
    match resident_safe_mir_program(program) {
        Ok(()) => String::new(),
        Err(detail) => detail,
    }
}

/// Test hook: how many times resident `main` ran without a clean restart.
#[doc(hidden)]
pub fn resident_invocations_for_test() -> u64 {
    RESIDENT_RUNTIME.with(|slot| slot.borrow().as_ref().map(|r| r.invocations).unwrap_or(0))
}
