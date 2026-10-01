//! Pure-call folding over canonical MIR.
//!
//! Compile time is explicit in the checker (`prep`), so an ordinary immutable
//! binding such as `limit :: square(12)` reaches MIR as a runtime call. This
//! pass recovers that optimization after checking: when an immutable local is
//! initialized from effect-free work over constants, the work runs once in the
//! MIR interpreter and the result becomes a MIR constant. Every execution tier
//! consumes the folded program, so the interpreter, JIT and AOT agree.
//!
//! The pass runs on freshly lowered MIR, before the canonical optimizer, so the
//! optimizer's constant folding and dead-value elimination see the results.
//! Folding is optional: a callee with effects, a failure carrier, memoization,
//! captures or a generator is left alone, as is any evaluation that fails,
//! prints, exhausts its fuel or yields a value too large to inline.

use super::{evaluate_function_with_config_and_state, MirEvalConfig, MirExecutionStatus};
use crate::Diagnostics::Span;
use jet_foundation::AST::CtValue;
use jet_foundation::MIR::{
    MirBasicBlock, MirBlockId, MirCallArg, MirCallee, MirConstant, MirEffectFacts, MirFailureCarrier,
    MirFunction, MirFunctionForm, MirFunctionId, MirFunctionKind, MirInstruction, MirLocalId,
    MirOpId, MirOperation, MirOptimizationFacts, MirPlaceBase, MirPlaceId, MirPreludeAbi,
    MirPreludeCall, MirPreludeCallId, MirProgram, MirSemanticOp, MirTerminator, MirType,
    MirTypeKind, MirValueId,
};
use std::collections::{HashMap, HashSet};

/// Rounds of fold-then-propagate. A later round sees the constants an earlier
/// round produced, so `b :: square(a)` folds after `a :: square(2)`.
const FOLD_ROUNDS: usize = 8;

/// Largest structural weight of a folded value. Folding is an optimization,
/// not a way to force a large value into every generated tier.
const FOLD_OUTPUT_BUDGET: usize = 256 * 1024;

/// One foldable instruction and the synthetic function that evaluates it.
struct Candidate {
    function: usize,
    block: usize,
    instruction: usize,
    ty: MirType,
    synthetic: MirFunction,
}

/// Fold effect-free work that initializes immutable locals from constants.
pub(crate) fn fold_pure_calls(program: &mut MirProgram) {
    let config = MirEvalConfig::default();
    let mut declined: HashSet<(usize, usize, usize)> = HashSet::new();
    for _ in 0..FOLD_ROUNDS {
        let candidates = collect_candidates(program, &declined);
        if candidates.is_empty() {
            break;
        }
        let base_len = program.functions.len();
        let mut sites = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            sites.push((
                candidate.function,
                candidate.block,
                candidate.instruction,
                candidate.synthetic.id,
                candidate.ty,
            ));
            program.functions.push(candidate.synthetic);
        }
        let view: &MirProgram = program;
        let results = sites
            .iter()
            .map(|(_, _, _, synthetic, ty)| evaluate(view, *synthetic, ty, &config))
            .collect::<Vec<_>>();
        program.functions.truncate(base_len);
        let mut changed = false;
        for ((function, block, instruction, _, _), result) in sites.into_iter().zip(results) {
            match result {
                Some(constant) => {
                    program.functions[function].blocks[block].instructions[instruction]
                        .operation = MirOperation::Constant(constant);
                    changed = true;
                }
                None => {
                    declined.insert((function, block, instruction));
                }
            }
        }
        if !changed {
            break;
        }
    }
    resolve_joins(program);
}

/// Replace a scalar join whose live edges all carry one constant with that
/// constant. Folding decided the branch (`m :: if a > b -> a else -> b`, or
/// the test behind `xs.min_by(f) ?? -1`), so every tier reads a literal.
fn resolve_joins(program: &mut MirProgram) {
    for function in &mut program.functions {
        let initializers = immutable_local_initializers(function);
        if initializers.is_empty() {
            continue;
        }
        let root_locals = root_locals(function);
        let constants = function_constants(function, &initializers, &root_locals);
        for block in &mut function.blocks {
            for instruction in &mut block.instructions {
                let (Some(result), Some(ty)) = (instruction.result, instruction.ty.as_ref()) else {
                    continue;
                };
                if !matches!(instruction.operation, MirOperation::Phi { .. })
                    || !matches!(
                        ty.kind,
                        MirTypeKind::Int
                            | MirTypeKind::IntN { .. }
                            | MirTypeKind::Float
                            | MirTypeKind::Float32
                            | MirTypeKind::Bool
                            | MirTypeKind::Char
                    )
                {
                    continue;
                }
                if let Some(constant) = constants.get(&result) {
                    instruction.operation = MirOperation::Constant(constant.clone());
                }
            }
        }
    }
}

fn root_locals(function: &MirFunction) -> HashMap<MirPlaceId, MirLocalId> {
    function
        .places
        .iter()
        .filter(|place| place.projections.is_empty())
        .filter_map(|place| match place.base {
            MirPlaceBase::Local(local) => Some((place.id, local)),
            _ => None,
        })
        .collect()
}

fn collect_candidates(
    program: &MirProgram,
    declined: &HashSet<(usize, usize, usize)>,
) -> Vec<Candidate> {
    let prelude: HashMap<MirPreludeCallId, &MirPreludeCall> = program
        .prelude_calls
        .iter()
        .map(|call| (call.id, call))
        .collect();
    let functions: HashMap<MirFunctionId, &MirFunction> = program
        .functions
        .iter()
        .map(|function| (function.id, function))
        .collect();
    let mut next_id = program
        .functions
        .iter()
        .map(|function| function.id.0)
        .max()
        .unwrap_or(0);
    let mut candidates = Vec::new();
    for (function_index, function) in program.functions.iter().enumerate() {
        let initializers = immutable_local_initializers(function);
        if initializers.is_empty() {
            continue;
        }
        let definitions: HashMap<MirValueId, &MirInstruction> = function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .filter_map(|instruction| instruction.result.map(|result| (result, instruction)))
            .collect();
        let feeding = binding_feeding_values(function, &initializers, &definitions);
        let root_locals = root_locals(function);
        let inputs = SliceInputs {
            constants: function_constants(function, &initializers, &root_locals),
            value_types: function
                .values
                .iter()
                .map(|(value, ty, ..)| (*value, ty))
                .collect(),
            definitions,
            initializers,
            root_locals,
            prelude: &prelude,
            functions: &functions,
        };
        for (block_index, block) in function.blocks.iter().enumerate() {
            for (instruction_index, instruction) in block.instructions.iter().enumerate() {
                if declined.contains(&(function_index, block_index, instruction_index)) {
                    continue;
                }
                let (Some(result), Some(ty)) = (instruction.result, instruction.ty.as_ref())
                else {
                    continue;
                };
                if !feeding.contains(&result)
                    || inputs.constants.contains_key(&result)
                    || !foldable_result_type(ty)
                    || !operation_is_foldable(&instruction.operation, &prelude, &functions)
                {
                    continue;
                }
                let mut slice = Slice::default();
                if !instruction
                    .operation
                    .value_uses()
                    .into_iter()
                    .all(|value| inputs.reproduce(value, instruction, &mut slice))
                {
                    continue;
                }
                slice.instructions.push(instruction.clone());
                next_id += 1;
                candidates.push(Candidate {
                    function: function_index,
                    block: block_index,
                    instruction: instruction_index,
                    ty: ty.clone(),
                    synthetic: synthetic_function(
                        function,
                        MirFunctionId(next_id),
                        slice.instructions,
                        result,
                        ty,
                    ),
                });
            }
        }
    }
    candidates
}

/// Most instructions one fold may re-run to rebuild its inputs.
const MAX_SLICE: usize = 256;

/// What one function offers a fold: known constants, the instruction that
/// defines each value, and the one initializer of each immutable local.
struct SliceInputs<'a> {
    constants: HashMap<MirValueId, MirConstant>,
    value_types: HashMap<MirValueId, &'a MirType>,
    definitions: HashMap<MirValueId, &'a MirInstruction>,
    initializers: HashMap<MirLocalId, MirValueId>,
    root_locals: HashMap<MirPlaceId, MirLocalId>,
    prelude: &'a HashMap<MirPreludeCallId, &'a MirPreludeCall>,
    functions: &'a HashMap<MirFunctionId, &'a MirFunction>,
}

/// The instructions a synthetic function runs before the folded one.
#[derive(Default)]
struct Slice {
    instructions: Vec<MirInstruction>,
    defined: HashSet<MirValueId>,
    visiting: HashSet<MirValueId>,
}

impl SliceInputs<'_> {
    /// Append instructions that rebuild `value` without run-time input:
    /// a known constant, a read of an immutable local rebuilt from its one
    /// initializer, or effect-free work over values that are themselves
    /// rebuilt. `Path("/tmp/a").to_string()` and `xs.reduce((a, b) -> a + b)`
    /// fold this way even though the path and the closure are no literals.
    fn reproduce(&self, value: MirValueId, site: &MirInstruction, slice: &mut Slice) -> bool {
        if slice.defined.contains(&value) {
            return true;
        }
        if slice.instructions.len() >= MAX_SLICE || !slice.visiting.insert(value) {
            return false;
        }
        let operation = if let Some(constant) = self.constants.get(&value) {
            Some(MirOperation::Constant(constant.clone()))
        } else {
            self.definitions
                .get(&value)
                .and_then(|definition| self.rebuild(&definition.operation, site, slice))
        };
        slice.visiting.remove(&value);
        let Some(operation) = operation else {
            return false;
        };
        slice.defined.insert(value);
        slice.instructions.push(MirInstruction {
            id: MirOpId(u64::MAX - slice.instructions.len() as u64),
            span: site.span,
            source_line: site.source_line,
            result: Some(value),
            ty: self.value_types.get(&value).map(|ty| (*ty).clone()),
            operation,
        });
        true
    }

    fn rebuild(
        &self,
        operation: &MirOperation,
        site: &MirInstruction,
        slice: &mut Slice,
    ) -> Option<MirOperation> {
        match operation {
            MirOperation::ReadPlace(place) | MirOperation::MovePlace { place } => {
                let initial = *self
                    .root_locals
                    .get(place)
                    .and_then(|local| self.initializers.get(local))?;
                self.reproduce(initial, site, slice)
                    .then_some(MirOperation::Copy {
                        value: initial,
                        materialize_view: false,
                    })
            }
            MirOperation::Closure {
                function, captures, ..
            } => (captures.is_empty()
                && self
                    .functions
                    .get(function)
                    .is_some_and(|function| callee_is_foldable(function)))
            .then(|| operation.clone()),
            MirOperation::Copy { .. }
            | MirOperation::Move { .. }
            | MirOperation::BuildList { .. }
            | MirOperation::BuildMap { .. }
            | MirOperation::Tuple { .. }
            | MirOperation::Struct { .. }
            | MirOperation::Enum { .. }
            | MirOperation::Present { .. }
            | MirOperation::Absent
            | MirOperation::Field { .. }
            | MirOperation::Range { .. } => self.rebuild_operands(operation, site, slice),
            _ if operation_is_foldable(operation, self.prelude, self.functions) => {
                self.rebuild_operands(operation, site, slice)
            }
            _ => None,
        }
    }

    fn rebuild_operands(
        &self,
        operation: &MirOperation,
        site: &MirInstruction,
        slice: &mut Slice,
    ) -> Option<MirOperation> {
        operation
            .value_uses()
            .into_iter()
            .all(|value| self.reproduce(value, site, slice))
            .then(|| operation.clone())
    }
}

/// Values whose constant is known in `function`: literal constants, copies and
/// moves of them, lists built from them, reads of an immutable local whose one
/// initialization stores a known constant, and joins whose live edges all
/// carry one constant.
fn function_constants(
    function: &MirFunction,
    initializers: &HashMap<MirLocalId, MirValueId>,
    root_locals: &HashMap<MirPlaceId, MirLocalId>,
) -> HashMap<MirValueId, MirConstant> {
    let mut constants: HashMap<MirValueId, MirConstant> = HashMap::new();
    let mut local_values: HashMap<MirLocalId, MirConstant> = HashMap::new();
    loop {
        let before = constants.len() + local_values.len();
        for (local, value) in initializers {
            if !local_values.contains_key(local) {
                if let Some(constant) = constants.get(value) {
                    local_values.insert(*local, constant.clone());
                }
            }
        }
        // A branch on a known condition takes one edge; the other is dead.
        let taken: HashMap<MirBlockId, MirBlockId> = function
            .blocks
            .iter()
            .filter_map(|block| match &block.terminator {
                MirTerminator::Branch {
                    condition,
                    then_target,
                    else_target,
                } => match constants.get(condition) {
                    Some(MirConstant::Bool(true)) => Some((block.id, *then_target)),
                    Some(MirConstant::Bool(false)) => Some((block.id, *else_target)),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        for block in &function.blocks {
            for instruction in &block.instructions {
                let Some(result) = instruction.result else {
                    continue;
                };
                if constants.contains_key(&result) {
                    continue;
                }
                let constant = match &instruction.operation {
                    MirOperation::Constant(constant) => Some(constant.clone()),
                    MirOperation::Copy { value, .. } | MirOperation::Move { value } => {
                        constants.get(value).cloned()
                    }
                    MirOperation::ReadPlace(place) | MirOperation::MovePlace { place } => {
                        root_locals
                            .get(place)
                            .and_then(|local| local_values.get(local))
                            .cloned()
                    }
                    // `[1, 2, 3].len()` folds like the checker's old fold did.
                    MirOperation::BuildList {
                        values,
                        trait_coercion: None,
                    } => values
                        .iter()
                        .map(|value| constants.get(value).cloned())
                        .collect::<Option<Vec<_>>>()
                        .map(MirConstant::List),
                    MirOperation::Phi { incoming } => {
                        join_constant(block.id, incoming, &taken, &constants)
                    }
                    _ => None,
                };
                if let Some(constant) = constant {
                    constants.insert(result, constant);
                }
            }
        }
        if constants.len() + local_values.len() == before {
            return constants;
        }
    }
}

/// The one constant every live edge into a join carries. An edge from a
/// branch that provably goes elsewhere is dead.
fn join_constant(
    join: MirBlockId,
    incoming: &[(MirBlockId, MirValueId)],
    taken: &HashMap<MirBlockId, MirBlockId>,
    constants: &HashMap<MirValueId, MirConstant>,
) -> Option<MirConstant> {
    let mut joined: Option<&MirConstant> = None;
    for (predecessor, value) in incoming {
        if taken.get(predecessor).is_some_and(|target| *target != join) {
            continue;
        }
        let constant = constants.get(value)?;
        if joined.is_some_and(|joined| joined != constant) {
            return None;
        }
        joined = Some(constant);
    }
    joined.cloned()
}

/// Immutable locals written exactly once, at their root, mapped to the value
/// that initializes them.
fn immutable_local_initializers(function: &MirFunction) -> HashMap<MirLocalId, MirValueId> {
    let immutable: HashSet<MirLocalId> = function
        .locals
        .iter()
        .filter(|local| !local.mutable && !local.uninit)
        .map(|local| local.id)
        .collect();
    let places: HashMap<MirPlaceId, (MirLocalId, bool)> = function
        .places
        .iter()
        .filter_map(|place| match place.base {
            MirPlaceBase::Local(local) if immutable.contains(&local) => {
                Some((place.id, (local, place.projections.is_empty())))
            }
            _ => None,
        })
        .collect();
    let mut writes: HashMap<MirLocalId, Vec<MirValueId>> = HashMap::new();
    let mut disqualified: HashSet<MirLocalId> = HashSet::new();
    for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
        match &instruction.operation {
            MirOperation::WritePlace { place, value } => match places.get(place) {
                Some((local, true)) => writes.entry(*local).or_default().push(*value),
                Some((local, false)) => {
                    disqualified.insert(*local);
                }
                None => {}
            },
            MirOperation::ReadPlace(_) | MirOperation::MovePlace { .. } => {}
            operation => {
                for place in operation_places(operation) {
                    if let Some((local, _)) = places.get(&place) {
                        disqualified.insert(*local);
                    }
                }
            }
        }
    }
    writes
        .into_iter()
        .filter(|(local, values)| values.len() == 1 && !disqualified.contains(local))
        .map(|(local, values)| (local, values[0]))
        .collect()
}

/// Every place an operation other than a plain read or root write touches.
fn operation_places(operation: &MirOperation) -> Vec<MirPlaceId> {
    match operation {
        MirOperation::ReadPlace(place)
        | MirOperation::MovePlace { place }
        | MirOperation::InitializeUninit { place }
        | MirOperation::RawAddressOf { place }
        | MirOperation::AddressOf { place, .. }
        | MirOperation::WritePlace { place, .. }
        | MirOperation::ReplacePlace { place, .. } => vec![*place],
        MirOperation::Closure { captures, .. } => captures
            .iter()
            .filter_map(|capture| match capture {
                jet_foundation::MIR::MirCaptureOperand::Place(place) => Some(*place),
                jet_foundation::MIR::MirCaptureOperand::Value(_) => None,
            })
            .collect(),
        MirOperation::Call { args, .. }
        | MirOperation::CoreCall { args, .. }
        | MirOperation::IndirectCall { args, .. } => arg_places(args),
        MirOperation::Semantic(operation) => semantic_places(operation),
        _ => Vec::new(),
    }
}

fn arg_places(args: &[MirCallArg]) -> Vec<MirPlaceId> {
    args.iter().filter_map(|arg| arg.place).collect()
}

fn semantic_places(operation: &MirSemanticOp) -> Vec<MirPlaceId> {
    match operation {
        MirSemanticOp::BuiltinMethod {
            receiver_place: Some(place),
            ..
        } => vec![*place],
        MirSemanticOp::StaticPreludeCall { args, .. }
        | MirSemanticOp::ClosureMethod { args, .. }
        | MirSemanticOp::HostCall { args, .. } => arg_places(args),
        _ => Vec::new(),
    }
}

/// Values that flow into the initializer of an immutable local, directly or
/// as an operand of other work that does, plus branch conditions, which
/// decide the joins such initializers read. Only these are folded: the pass
/// replaces the old checker fold of immutable bindings, not every call.
fn binding_feeding_values(
    function: &MirFunction,
    initializers: &HashMap<MirLocalId, MirValueId>,
    definitions: &HashMap<MirValueId, &MirInstruction>,
) -> HashSet<MirValueId> {
    let mut pending: Vec<MirValueId> = initializers.values().copied().collect();
    pending.extend(function.blocks.iter().filter_map(|block| match &block.terminator {
        MirTerminator::Branch { condition, .. } => Some(*condition),
        _ => None,
    }));
    let mut feeding = HashSet::new();
    while let Some(value) = pending.pop() {
        if !feeding.insert(value) {
            continue;
        }
        if let Some(instruction) = definitions.get(&value) {
            pending.extend(instruction.operation.value_uses());
        }
    }
    feeding
}

fn operation_is_foldable(
    operation: &MirOperation,
    prelude: &HashMap<MirPreludeCallId, &MirPreludeCall>,
    functions: &HashMap<MirFunctionId, &MirFunction>,
) -> bool {
    match operation {
        MirOperation::Call {
            callee,
            args,
            type_args,
        } => {
            type_args.is_empty()
                && args.iter().all(value_argument)
                && match callee {
                    MirCallee::User(function)
                    | MirCallee::Associated { function, .. }
                    | MirCallee::Method { function, .. } => functions
                        .get(function)
                        .is_some_and(|function| callee_is_foldable(function)),
                    MirCallee::Prelude(call) => prelude_is_pure(prelude, *call),
                    MirCallee::Core(_)
                    | MirCallee::TraitMethod { .. }
                    | MirCallee::Foreign(_)
                    | MirCallee::Indirect(_) => false,
                }
        }
        MirOperation::CoreCall { route, args, .. } => {
            args.iter().all(value_argument) && prelude_is_pure(prelude, *route)
        }
        MirOperation::Binary { dispatch, .. } => match dispatch {
            jet_foundation::MIR::MirBinaryDispatch::Primitive => true,
            jet_foundation::MIR::MirBinaryDispatch::Prelude { call, .. } => {
                prelude_is_pure(prelude, *call)
            }
        },
        MirOperation::Unary { .. } | MirOperation::BuildString { .. } => true,
        // Closure-taking work (`xs.reduce(f)`) folds when its closure is a
        // capture-free pure function, which the operand rebuild checks.
        MirOperation::Semantic(operation) => {
            !matches!(operation, MirSemanticOp::HandleMethod { .. })
                && semantic_places(operation).is_empty()
                && {
                    let calls = operation.prelude_calls();
                    !calls.is_empty() && calls.into_iter().all(|call| prelude_is_pure(prelude, call))
                }
        }
        // Structural reads of a rebuilt value: the test and payload behind
        // `opt ?? fallback`, `result.ok`, a field or an enum case.
        MirOperation::OptionIsSome { .. }
        | MirOperation::OptionValue { .. }
        | MirOperation::ResultIsOk { .. }
        | MirOperation::ResultValue { .. }
        | MirOperation::EnumIs { .. }
        | MirOperation::EnumPayload { .. }
        | MirOperation::Field { .. } => true,
        _ => false,
    }
}

/// An argument passed by value. The folded instruction is copied whole, so
/// its representation adaptations run in the synthetic call too; only a
/// write place would reach outside it.
fn value_argument(arg: &MirCallArg) -> bool {
    arg.place.is_none()
}

fn callee_is_foldable(function: &MirFunction) -> bool {
    function.kind == MirFunctionKind::Jet
        && function.effects.direct.is_empty()
        && function.effects.solved.is_empty()
        && !function.effects.maximal
        && function.failure == MirFailureCarrier::Infallible
        && function.memo_bound.is_none()
        && function.generator.is_none()
        && function.capture_params.is_empty()
        && function.generic_params.is_empty()
        && !function.is_reactive
        && function.foreign_language.is_none()
        && function.target_applicability.interpreter
}

/// The same purity rule the optimizer's dead-value pass applies to a
/// Prelude route: no effect row, not an Effect-ABI row, and infallible.
fn prelude_is_pure(prelude: &HashMap<MirPreludeCallId, &MirPreludeCall>, call: MirPreludeCallId) -> bool {
    prelude.get(&call).is_some_and(|record| {
        record.effect.is_none()
            && record.authority.is_none()
            && record.abi != MirPreludeAbi::Effect
            && matches!(
                record.fallibility,
                jet_foundation::MIR::MirCallFallibility::Infallible
            )
    })
}

/// Only values every tier already writes as a literal become constants.
fn foldable_result_type(ty: &MirType) -> bool {
    match &ty.kind {
        MirTypeKind::Int
        | MirTypeKind::IntN { .. }
        | MirTypeKind::Float
        | MirTypeKind::Float32
        | MirTypeKind::Bool
        | MirTypeKind::Char
        | MirTypeKind::String => true,
        MirTypeKind::List(element) => foldable_result_type(element),
        _ => false,
    }
}

/// A function that runs `instructions` (the reproduced inputs, operands
/// first, then the folded instruction) and returns `result`.
fn synthetic_function(
    host: &MirFunction,
    id: MirFunctionId,
    instructions: Vec<MirInstruction>,
    result: MirValueId,
    ty: &MirType,
) -> MirFunction {
    let span = instructions.last().map_or(host.span, |instruction| instruction.span);
    let needed: HashSet<MirValueId> = instructions
        .iter()
        .filter_map(|instruction| instruction.result)
        .collect();
    MirFunction {
        id,
        module_id: host.module_id,
        source_file: host.source_file,
        key: format!("{}::__jet_pure_fold_{}", host.key, id.0),
        module: host.module.clone(),
        name: host.name.clone(),
        span,
        kind: MirFunctionKind::Jet,
        form: MirFunctionForm::TopLevel,
        visibility: host.visibility,
        target_applicability: host.target_applicability,
        web_bucket: None,
        web_marker: None,
        generic_params: Vec::new(),
        capture_params: Vec::new(),
        params: Vec::new(),
        declared_return: Some(ty.clone()),
        return_type: ty.clone(),
        failure: MirFailureCarrier::Infallible,
        effects: MirEffectFacts::default(),
        captures: None,
        generator: None,
        optimization: MirOptimizationFacts::default(),
        is_unsafe: host.is_unsafe,
        unsafe_gate: host.unsafe_gate.clone(),
        is_pure: true,
        memo_bound: None,
        is_reactive: false,
        reactive_upgrades: Vec::new(),
        is_inline: false,
        is_inline_always: false,
        is_scalar: false,
        kernel_proof: None,
        gc_return: false,
        return_view_provenance: None,
        web_param_reconstructions: Vec::new(),
        blocks: vec![MirBasicBlock {
            id: host.entry,
            span,
            instructions,
            terminator: MirTerminator::Return {
                value: Some(result),
            },
        }],
        entry: host.entry,
        locals: Vec::new(),
        values: host
            .values
            .iter()
            .filter(|(value, ..)| needed.contains(value))
            .cloned()
            .collect(),
        places: Vec::new(),
        scopes: Vec::new(),
        drops: Vec::new(),
        foreign_language: None,
    }
}

fn evaluate(
    program: &MirProgram,
    function: MirFunctionId,
    ty: &MirType,
    config: &MirEvalConfig,
) -> Option<MirConstant> {
    let mut data_pipeline = crate::Comptime::DataPipelineState::default();
    let result =
        evaluate_function_with_config_and_state(program, function, &[], config, &mut data_pipeline)
            .ok()?;
    if result.status != MirExecutionStatus::Completed
        || result.exit_code != 0
        || !result.stdout.is_empty()
        || !result.stderr.is_empty()
    {
        return None;
    }
    let value = crate::Comptime::MirBridge::mir_to_ct_value(result.value, Span::new(0, 0)).ok()?;
    let constant = folded_constant(&value, ty)?;
    let mut cost = 0usize;
    (constant_cost(&constant, &mut cost) <= FOLD_OUTPUT_BUDGET).then_some(constant)
}

fn folded_constant(value: &CtValue, ty: &MirType) -> Option<MirConstant> {
    Some(match (&ty.kind, value) {
        (MirTypeKind::Int, CtValue::Int(value)) => MirConstant::Int {
            value: *value,
            width: None,
            spelling: None,
        },
        (MirTypeKind::IntN { signed, bits }, CtValue::Int(value)) => MirConstant::Int {
            value: *value,
            width: Some((*signed, *bits)),
            spelling: None,
        },
        (MirTypeKind::Float | MirTypeKind::Float32, CtValue::Float(value)) => MirConstant::Float {
            value: value.as_f64(),
            f32: matches!(ty.kind, MirTypeKind::Float32),
            spelling: None,
        },
        (MirTypeKind::Bool, CtValue::Bool(value)) => MirConstant::Bool(*value),
        (MirTypeKind::Char, CtValue::Char(value)) => MirConstant::Char(*value),
        (MirTypeKind::String, CtValue::Str(value)) => MirConstant::String(value.clone()),
        (MirTypeKind::List(element), CtValue::List(values)) => MirConstant::List(
            values
                .iter()
                .map(|value| folded_constant(value, element))
                .collect::<Option<Vec<_>>>()?,
        ),
        _ => return None,
    })
}

fn constant_cost(constant: &MirConstant, cost: &mut usize) -> usize {
    if *cost > FOLD_OUTPUT_BUDGET {
        return *cost;
    }
    match constant {
        MirConstant::String(text) => *cost = cost.saturating_add(16 + text.len()),
        MirConstant::List(values) => {
            *cost = cost.saturating_add(16 + values.len().saturating_mul(8));
            for value in values {
                constant_cost(value, cost);
            }
        }
        _ => *cost = cost.saturating_add(16),
    }
    *cost
}
