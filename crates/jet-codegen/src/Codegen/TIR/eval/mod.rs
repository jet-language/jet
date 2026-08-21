//! Canonical TIR evaluator — reference semantics (D-ONECORE1=A / #777).

mod builtins;
mod browser;
mod cli;
mod closure_ops;
mod compute_calls;
mod data_calls;
mod event_ops;
mod exprs;
mod handles;
mod local_cell;
mod regex_ops;
mod services_calls;
mod stmts;
mod stream;
mod webapp;

mod jet_mem {
    pub(super) use jet_foundation::MemSentry::{
        jet_memory_ledger_record, MemoryLedgerWitness,
    };
}

#[allow(dead_code)]
mod gc_runtime {
    include!("../../../../../jet-rt/src/__gc.rs");
}

// Included Prelude fragment: the evaluator marshals only the arms it reaches,
// so the unreached rows in the shared source are not dead product code.
#[allow(dead_code)]
mod contract_semantics {
    use jet_foundation::Outcome::{jet_render_runtime_stop, JetRuntimeDiagnostic};
    include!("../../../Prelude/Core/Contracts.rs");
}

mod range_semantics {
    use jet_foundation::StructuralDebug::jet_debug_range;
    include!("../../../Prelude/Core/RangeBounds.rs");
    include!("../../../Prelude/Core/InlineRange.rs");
}
#[allow(dead_code)]
mod measurement_semantics {
    include!("../../../Prelude/Core/Measurement.rs");
}


mod disjoint_semantics {
    include!("../../../Prelude/Core/Disjoint.rs");

    pub(super) fn split(
        len: usize,
        mid: i64,
    ) -> Result<((usize, usize), (usize, usize)), String> {
        jet_disjoint_split_bounds(len, mid)
    }

    pub(super) fn indexes(
        len: usize,
        indices: &[i64],
    ) -> Result<Vec<(usize, usize, usize)>, String> {
        jet_disjoint_index_bounds(len, indices)
    }
}

#[allow(dead_code)]
mod division_semantics {
    use super::contract_semantics::{
        JET_ARITHMETIC_DIVIDE_OVERFLOW, JET_ARITHMETIC_DIVIDE_ZERO,
        JET_ARITHMETIC_DIVISION_ERROR,
    };

    // Division.rs also carries AOT-only stop adapters. The evaluator uses its
    // fallible Prelude policy below, so this path must never be called here.
    fn jet_arithmetic_stop(_: &str, _: u32, _: &str) -> ! {
        unreachable!("TIR evaluator division adapter must return a diagnostic")
    }

    include!("../../../Prelude/Core/Division.rs");
}

/// #2027 / I8+I9: the interpreter ambient reaches the one signal mechanism
/// through `crate::interrupt_runtime` — the single in-binary instance of
/// `Prelude/CoreLib/Top/Interrupt.rs` that the resident Cranelift host also
/// uses. A private `include!` here would compile a second pending count whose
/// `signal(SIGINT, …)` install disarmed whichever tier armed first.
use crate::interrupt_runtime;

#[allow(dead_code)]
mod uninit_semantics {
    include!("../../../Prelude/Uninit.rs");
}

#[allow(dead_code)]
mod shared_protocol {
    include!("../../../Prelude/SharedProtocol.rs");
}

#[allow(dead_code)]
pub(super) mod term_semantics {
    include!("../../../Prelude/Term.rs");
}

/// The generated-CLI argv boundary: the interpreter reads the same Prelude
/// part AOT embeds and the Cranelift host includes, so program-name
/// normalization and banner termination are decided once.
#[allow(dead_code, unexpected_cfgs)]
mod cli_boundary {
    include!("../../../Prelude/Job.rs");
}

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::AST::{BinMatchPart, Expr, Func, Item, ProgramBundle, Stmt, Type, UnitFamilyDef};
use crate::Codegen::mangle;
use super::Cx;
use crate::Codegen::TIR::{
    self, JitProgram, LowerEnv, TExpr, TExprKind, TFunc, TJitSpawnBody, TJitSpawnLambda, TLocal,
    TStmt,
};
use super::build_cx_items;
use crate::Comptime::{self, CtReport, CtValue, DevSink};
use crate::Diagnostics::{Diagnostic, Span};
use jet_foundation::MatchScan::BinBind;
use jet_foundation::Reflection::ReflectionField;

/// Cross-tier hook: Cranelift-native functions callable from the TIR evaluator (#778).
pub type NativeCallHook = fn(&str, &[CtValue]) -> Option<Result<CtValue, Diagnostic>>;

/// D-MEMO1=A: the evaluator/deopt carrier for one run's Prelude memo stores.
/// The cache implementation remains in `Prelude/Memo.rs`; this type only keeps
/// one store alive while a tiered run crosses the interpreter boundary.
pub type MemoState = Arc<Mutex<HashMap<String, crate::memo::JetMemo<Vec<CtValue>, CtValue>>>>;

pub fn new_memo_state() -> MemoState {
    Arc::new(Mutex::new(HashMap::new()))
}

thread_local! {
    static NATIVE_CALL_HOOK: Cell<Option<NativeCallHook>> = const { Cell::new(None) };
}

pub fn set_native_call_hook(hook: Option<NativeCallHook>) {
    NATIVE_CALL_HOOK.with(|slot| slot.set(hook));
}

pub(super) fn native_call_hook() -> Option<NativeCallHook> {
    NATIVE_CALL_HOOK.with(Cell::get)
}

/// Binary-pattern marshalling shared by arm probes and direct pattern binds.
/// The scan and endian policy live in Foundation's Prelude kernel.
pub(super) fn bin_match_scan_value(
    value: &CtValue,
    parts: &[BinMatchPart],
    consume_prefix: bool,
) -> Option<(usize, Vec<(String, Type, BinBind)>)> {
    let bytes = match value {
        CtValue::Bytes(bytes) => bytes.clone(),
        CtValue::List(items) => items
            .iter()
            .map(|item| match item {
                CtValue::Int(value) => Some(*value as u8),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?,
        _ => return None,
    };
    jet_foundation::MatchScan::bin_match_scan(&bytes, parts, consume_prefix)
}

pub(super) fn bin_match_bind_value(bind: BinBind) -> CtValue {
    match bind {
        BinBind::Int(value) => CtValue::Int(value),
        BinBind::Rest(bytes) => CtValue::Bytes(bytes),
    }
}

fn wall_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

pub(super) fn raw_place_local(expr: &TExpr) -> Option<&TLocal> {
    match &expr.kind {
        TIR::TExprKind::Local(local) => Some(local),
        TIR::TExprKind::Borrow { place, .. } => raw_place_local(place),
        TIR::TExprKind::DistinctCtor { arg, .. } => raw_place_local(arg),
        _ => None,
    }
}

pub use exprs::{stable_memo_field_slot, stable_place_address, tir_place_address_key};

/// The evaluator's one refusal for a construct it cannot run (E0956).
///
/// `what` is a NOUN PHRASE naming the construct — "raw pointer address",
/// "task receiver", "view-mut path" — and nothing else. The registered row
/// renders it as `` `{what}` isn't supported by the current evaluator yet ``,
/// and `jet dev` re-uses the same phrase inside its own boundary sentence
/// ("it uses `raw pointer address`", E2201 via
/// `jet_driver::InterpreterBoundary::dev_boundary_for_construct`). A `what`
/// that carries its own clause therefore renders twice as a spliced,
/// ungrammatical sentence: never write "X isn't supported", "X expects a
/// String", or any trailing "yet" here. The construct also travels
/// structurally on the diagnostic, so no consumer parses this prose back.
pub(super) fn unsupported(what: &str, span: Span) -> Diagnostic {
    jet_foundation::Prelude::jet_e0956_unsupported(what, span)
}

/// D-CONC-FAIL1=A: the evaluator turns a child diagnostic into the same
/// normal TaskFailure enum that the shared scheduler reports. A diagnostic
/// raised by the joining parent remains a control diagnostic and is not
/// converted here.
fn task_failure_value(error: &Diagnostic) -> CtValue {
    let failure = crate::task_group::jet_task_failure_from_code(error.code.as_str(), error.what.clone());
    let (variant, args) = match failure {
        crate::task_group::JetTaskFailure::Cancelled => ("Cancelled", Vec::new()),
        crate::task_group::JetTaskFailure::DeadlineBlown => ("DeadlineBlown", Vec::new()),
        crate::task_group::JetTaskFailure::Panicked(reason) => (
            "Panicked",
            vec![(None, CtValue::Str(reason))],
        ),
    };
    CtValue::Enum {
        type_name: crate::Syntax::TYPE_TASK_FAILURE.to_string(),
        variant: variant.to_string(),
        args,
    }
}

/// D-CONC-STREAM1=A: the consumer's view of a cancelled producer child. `break`
/// cancels the child, the child unwinds at its next wait point through
/// `task_wait_check`, and the consumer turns that outcome into ordinary stream
/// completion — the evaluator's spelling of `jet_stream_task`'s cancel
/// classification plus the spawn frame's `JetSchedulerResult::Cancelled`. Both
/// ends read the one shared code table in `Prelude/TaskGroup.rs`.
fn stream_producer_cancel_completed(
    control: &Arc<crate::scheduler::JetTaskControl>,
    error: &Diagnostic,
) -> bool {
    control.cancelled.load(Ordering::Acquire)
        && crate::task_group::jet_task_failure_from_code(error.code.as_str(), error.what.clone())
            == crate::task_group::JetTaskFailure::Cancelled
}

fn task_child_panic(message: String, span: Span) -> Diagnostic {
    crate::Sema::Diagnostics::render_registered(
        "E0953",
        message,
        "a child task panicked".to_string(),
        String::new(),
        Some(span),
    )
}

pub(super) fn reborrow_repl_authorizer<'short, 'long: 'short>(
    authorizer: &'short mut Option<&'long mut dyn Comptime::ReplAuthorizer>,
) -> Option<&'short mut (dyn Comptime::ReplAuthorizer + 'short)> {
    match authorizer {
        Some(authorizer) => Some(&mut **authorizer),
        None => None,
    }
}

pub(super) fn progress_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

pub(super) fn progress_elapsed(started_at: f64) -> f64 {
    (progress_now() - started_at).max(0.0)
}

pub(super) fn progress_no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some()
}

pub(super) fn progress_emit(
    sink: Option<&Arc<Mutex<DevSink>>>,
    text: &str,
) {
    // Framing stays a terminal question — a pipe gets no carriage returns — but
    // where the frame GOES is the stream-ownership question.
    let tty = term_semantics::jet_term_stdout_is_terminal();
    let frame = term_semantics::jet_term_progress_frame(tty, text);
    if term_semantics::jet_term_stdout_is_program_stream() {
        let _ = term_semantics::jet_term_write_stdout(&frame, true);
        return;
    }
    if let Some(sink) = sink {
        let mut sink = sink.lock().expect("evaluator sink poisoned");
        sink.stdout.push_str(&frame);
    }
}

pub(super) fn progress_source_has_exact_total(expr: &TExpr) -> bool {
    matches!(
        &expr.kind,
        TIR::TExprKind::BuiltinMethod {
            op: TIR::TBuiltinOp::ListLazy,
            ..
        }
    )
}

pub(super) fn progress_iter_value(items: Vec<CtValue>, known_total: bool) -> CtValue {
    CtValue::Struct {
        type_name: "__JetIter".to_string(),
        fields: vec![
            ("items".to_string(), CtValue::List(items)),
            ("known_total".to_string(), CtValue::Bool(known_total)),
        ],
    }
}

pub(super) fn progress_iter_parts(value: &CtValue) -> Option<(Vec<CtValue>, bool)> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "__JetIter" {
        return None;
    }
    let items = fields.iter().find_map(|(name, value)| {
        (name == "items").then(|| match value {
            CtValue::List(items) => Some(items.clone()),
            _ => None,
        })
    })??;
    let known_total = fields
        .iter()
        .find_map(|(name, value)| {
            (name == "known_total").then(|| match value {
                CtValue::Bool(value) => Some(*value),
                _ => None,
            })
        })
        .flatten()
        .unwrap_or(true);
    Some((items, known_total))
}

pub(super) fn range_window(
    value: &CtValue,
    len: usize,
    span: Span,
) -> Result<(i64, i64), Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("Range window", span));
    };
    if type_name != crate::Syntax::TYPE_RANGE {
        return Err(unsupported("Range window type", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
    };
    let start = match field("start") {
        Some(CtValue::Int(value)) => *value,
        _ => return Err(unsupported("Range.start", span)),
    };
    let end = match field("end") {
        Some(CtValue::Int(value)) => *value,
        _ => return Err(unsupported("Range.end", span)),
    };
    let exclusive = match field("exclusive") {
        Some(CtValue::Bool(value)) => *value,
        _ => return Err(unsupported("Range.exclusive", span)),
    };
    checked_view_window(start, end, exclusive, len, span)
}

pub(super) fn checked_view_window(
    start: i64,
    end: i64,
    exclusive: bool,
    len: usize,
    span: Span,
) -> Result<(i64, i64), Diagnostic> {
    range_semantics::jet_checked_view_bounds(start, end, exclusive, len as i64)
        .map_err(|message| view_bounds_diagnostic(message, span))
}

pub(super) fn view_bounds_diagnostic(
    message: String,
    span: Span,
) -> Diagnostic {
    // Same E0953 voice as the JIT trap / comptime panic path so every tier
    // reports one code for an out-of-bounds view (I9).
    crate::Sema::Diagnostics::render_registered(
        "E0953",
        "your comptime code stopped the build".to_string(),
        format!("while computing this value at compile time, the program panicked: {message}"),
        "this is the sanctioned way to validate at compile time — fix the input the check rejects"
            .to_string(),
        Some(span),
    )
}

fn enter_source_nesting(depth: &mut usize, span: Span) -> Result<(), Diagnostic> {
    *depth += 1;
    if *depth <= crate::Diagnostics::MAX_SOURCE_NESTING {
        return Ok(());
    }
    let exceeded = *depth;
    *depth -= 1;
    Err(crate::Sema::Diagnostics::source_nesting_exceeded(exceeded, span))
}

pub(super) fn range_contains(
    value: &CtValue,
    needle: &CtValue,
    span: Span,
) -> Result<bool, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("Range.contains receiver", span));
    };
    if type_name != crate::Syntax::TYPE_RANGE {
        return Err(unsupported("Range.contains receiver", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
    };
    let (Some(CtValue::Int(start)), Some(CtValue::Int(end)), CtValue::Int(needle)) =
        (field("start"), field("end"), needle)
    else {
        return Err(unsupported("Range.contains arguments", span));
    };
    let exclusive = matches!(field("exclusive"), Some(CtValue::Bool(true)));
    Ok(range_semantics::jet_range_contains(
        *start, *end, exclusive, *needle,
    ))
}

const UNINIT_FIXED_CARRIER: &str = "__JetUninitFixed";

pub(super) fn uninit_fixed_carrier(len: usize) -> CtValue {
    CtValue::Struct {
        type_name: UNINIT_FIXED_CARRIER.to_string(),
        fields: vec![
            (
                "values".to_string(),
                CtValue::List(vec![CtValue::Unit; len]),
            ),
            (
                "initialized".to_string(),
                CtValue::List(
                    uninit_semantics::jet_uninit_bitmap(len)
                        .into_iter()
                        .map(CtValue::Bool)
                        .collect(),
                ),
            ),
        ],
    }
}

pub(super) fn uninit_fixed_read(value: &CtValue, index: usize) -> Option<CtValue> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != UNINIT_FIXED_CARRIER {
        return None;
    }
    let CtValue::List(values) = &fields.iter().find(|(name, _)| name == "values")?.1 else {
        return None;
    };
    let CtValue::List(initialized) = &fields
        .iter()
        .find(|(name, _)| name == "initialized")?
        .1
    else {
        return None;
    };
    let bitmap = initialized
        .iter()
        .map(|value| matches!(value, CtValue::Bool(true)))
        .collect::<Vec<_>>();
    let index = uninit_semantics::jet_uninit_read(&bitmap, index).ok()?;
    values.get(index).cloned()
}

pub(super) fn uninit_fixed_materialize(value: &CtValue) -> Option<CtValue> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != UNINIT_FIXED_CARRIER {
        return None;
    }
    let CtValue::List(values) = &fields.iter().find(|(name, _)| name == "values")?.1 else {
        return None;
    };
    let CtValue::List(initialized) = &fields
        .iter()
        .find(|(name, _)| name == "initialized")?
        .1
    else {
        return None;
    };
    let bitmap = initialized
        .iter()
        .map(|value| matches!(value, CtValue::Bool(true)))
        .collect::<Vec<_>>();
    uninit_semantics::jet_uninit_all(&bitmap).ok()?;
    Some(CtValue::List(values.clone()))
}

pub(super) fn uninit_fixed_write(
    value: &mut CtValue,
    index: usize,
    replacement: CtValue,
) -> bool {
    let CtValue::Struct { type_name, fields } = value else {
        return false;
    };
    if type_name != UNINIT_FIXED_CARRIER {
        return false;
    }
    let Some(values_index) = fields.iter().position(|(name, _)| name == "values") else {
        return false;
    };
    let Some(initialized_index) = fields
        .iter()
        .position(|(name, _)| name == "initialized")
    else {
        return false;
    };
    let CtValue::List(initialized) = &mut fields[initialized_index].1 else {
        return false;
    };
    let mut bitmap = initialized
        .iter()
        .map(|value| matches!(value, CtValue::Bool(true)))
        .collect::<Vec<_>>();
    let Ok((index, _)) = uninit_semantics::jet_uninit_write(&mut bitmap, index) else {
        return false;
    };
    *initialized = bitmap.into_iter().map(CtValue::Bool).collect();
    let CtValue::List(values) = &mut fields[values_index].1 else {
        return false;
    };
    values[index] = replacement;
    true
}

/// D-MEM1 S9 / D-PIN1=A: marker for a whole-place write window (`p :: &node`,
/// `pinned :: mem.pin(&node)`). AOT and Cranelift both give the local a real
/// exclusive reference to the owner's storage, so the interpreter has to alias
/// too — storing the copied value would silently drop every edit made through
/// the window (I9). The handle carries the owner local plus the field/index
/// path, exactly like `__JetViewMut` does for range windows.
pub(super) const PLACE_MUT_TYPE: &str = "__JetPlaceMut";

pub(super) fn place_mut_handle(base: &str, path: &[ViewMutPathStep]) -> CtValue {
    CtValue::Struct {
        type_name: PLACE_MUT_TYPE.into(),
        fields: vec![
            ("base".into(), CtValue::Str(base.to_string())),
            ("path".into(), encode_view_mut_path(path)),
        ],
    }
}

/// D-TASKBORROW1=A: a loaned window carries a runtime slot instead of an
/// owner local, because the child thread cannot name the owner's scope.
pub(super) fn place_loan_handle(slot: usize) -> CtValue {
    CtValue::Struct {
        type_name: PLACE_MUT_TYPE.into(),
        fields: vec![("loan".into(), CtValue::Int(slot as i64))],
    }
}

/// D-TASKBORROW1=A: the same loan for a `__JetViewMut` place region. The slot
/// holds exactly the loaned window, so the handle's indexes rebase onto it and
/// every existing view call site keeps indexing its owner absolutely.
pub(super) fn view_loan_handle(slot: usize, width: i64) -> CtValue {
    CtValue::Struct {
        type_name: "__JetViewMut".into(),
        fields: vec![
            ("loan".into(), CtValue::Int(slot as i64)),
            ("start".into(), CtValue::Int(0)),
            ("end".into(), CtValue::Int(width - 1)),
        ],
    }
}

/// What a `__JetPlaceMut` handle windows into.
pub(super) enum PlaceMutTarget {
    /// A place inside an owner local held by the reading scope.
    Owner(String, Vec<ViewMutPathStep>),
    /// A shared runtime slot loaned across a task boundary.
    Loan(usize),
}

pub(super) fn place_mut_target(value: &CtValue) -> Option<PlaceMutTarget> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != PLACE_MUT_TYPE {
        return None;
    }
    for (name, value) in fields {
        match (name.as_str(), value) {
            ("loan", CtValue::Int(slot)) => {
                return usize::try_from(*slot).ok().map(PlaceMutTarget::Loan);
            }
            ("base", CtValue::Str(base)) => {
                return Some(PlaceMutTarget::Owner(
                    base.clone(),
                    parse_view_mut_path(fields),
                ));
            }
            _ => {}
        }
    }
    None
}

/// One step from a root local to the list a `__JetViewMut` windows into.
#[derive(Clone, Debug)]
pub(super) enum ViewMutPathStep {
    Field(String),
    Index(i64),
}

/// Recover the complete owner place carried by a view expression. The caller
/// supplies index evaluation so dynamic projected indexes retain their TIR
/// provenance instead of being reduced to a bare root local.
pub(super) fn view_mut_place(
    expr: &TExpr,
    resolve_index: &mut impl FnMut(&TExpr) -> Result<i64, Diagnostic>,
) -> Result<Option<(String, Vec<ViewMutPathStep>)>, Diagnostic> {
    match &expr.kind {
        TExprKind::Local(local) => Ok(Some((local.name.clone(), Vec::new()))),
        TExprKind::Borrow { place, .. } | TExprKind::Deref(place) => {
            view_mut_place(place, resolve_index)
        }
        TExprKind::Field { recv, field, .. } => {
            let Some((base, mut path)) = view_mut_place(recv, resolve_index)? else {
                return Ok(None);
            };
            path.push(ViewMutPathStep::Field(field.clone()));
            Ok(Some((base, path)))
        }
        TExprKind::Index {
            base,
            index,
            is_map: false,
            ..
        } => {
            let Some((root, mut path)) = view_mut_place(base, resolve_index)? else {
                return Ok(None);
            };
            path.push(ViewMutPathStep::Index(resolve_index(index)?));
            Ok(Some((root, path)))
        }
        _ => Ok(None),
    }
}

pub(super) fn parse_view_mut_path(fields: &[(String, CtValue)]) -> Vec<ViewMutPathStep> {
    let Some((_, CtValue::List(steps))) = fields.iter().find(|(name, _)| name == "path") else {
        return Vec::new();
    };
    steps
        .iter()
        .filter_map(|step| {
            let CtValue::Str(s) = step else {
                return None;
            };
            if let Some(field) = s.strip_prefix("f:") {
                Some(ViewMutPathStep::Field(field.to_string()))
            } else if let Some(index) = s.strip_prefix("i:") {
                index.parse().ok().map(ViewMutPathStep::Index)
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn encode_view_mut_path(path: &[ViewMutPathStep]) -> CtValue {
    CtValue::List(
        path.iter()
            .map(|step| {
                CtValue::Str(match step {
                    ViewMutPathStep::Field(field) => format!("f:{field}"),
                    ViewMutPathStep::Index(index) => format!("i:{index}"),
                })
            })
            .collect(),
    )
}

pub(super) fn view_mut_parts(
    fields: &[(String, CtValue)],
) -> Option<(String, Vec<ViewMutPathStep>, i64, i64)> {
    let mut base = None;
    let mut start = None;
    let mut end = None;
    for (name, value) in fields {
        match (name.as_str(), value) {
            ("base", CtValue::Str(s)) => base = Some(s.clone()),
            ("start", CtValue::Int(n)) => start = Some(*n),
            ("end", CtValue::Int(n)) => end = Some(*n),
            _ => {}
        }
    }
    Some((base?, parse_view_mut_path(fields), start?, end?))
}

pub(super) fn project_list_place<'a>(
    root: &'a CtValue,
    path: &[ViewMutPathStep],
    span: Span,
) -> Result<&'a CtValue, Diagnostic> {
    let mut cur = root;
    for step in path {
        cur = match (step, cur) {
            (ViewMutPathStep::Field(field), CtValue::Struct { fields, .. }) => fields
                .iter()
                .find(|(name, _)| {
                    name == field
                        || name.strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX) == Some(field.as_str())
                        || name == &crate::Codegen::mangle(field)
                })
                .map(|(_, value)| value)
                .ok_or_else(|| unsupported("view-mut path field", span))?,
            (ViewMutPathStep::Index(index), CtValue::List(items)) => {
                if *index < 0 || *index as usize >= items.len() {
                    return Err(unsupported("view-mut path index", span));
                }
                &items[*index as usize]
            }
            _ => return Err(unsupported("view-mut path", span)),
        };
    }
    Ok(cur)
}

pub(super) fn replace_list_place(
    root: CtValue,
    path: &[ViewMutPathStep],
    replacement: CtValue,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    if path.is_empty() {
        return Ok(replacement);
    }
    let step = &path[0];
    let rest = &path[1..];
    match (step, root) {
        (ViewMutPathStep::Field(field), CtValue::Struct { type_name, mut fields }) => {
            let mangled = crate::Codegen::mangle(field);
            let slot = fields.iter_mut().find(|(name, _)| {
                name == field
                    || name.strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX) == Some(field.as_str())
                    || name == &mangled
            });
            let Some((_, value)) = slot else {
                return Err(unsupported("view-mut path field", span));
            };
            *value = replace_list_place(value.clone(), rest, replacement, span)?;
            Ok(CtValue::Struct { type_name, fields })
        }
        (ViewMutPathStep::Index(index), CtValue::List(mut items)) => {
            if *index < 0 || *index as usize >= items.len() {
                return Err(unsupported("view-mut path index", span));
            }
            let i = *index as usize;
            items[i] = replace_list_place(items[i].clone(), rest, replacement, span)?;
            Ok(CtValue::List(items))
        }
        _ => Err(unsupported("view-mut path", span)),
    }
}

pub(super) fn view_mut_window_args(fields: &[(String, CtValue)]) -> Option<&[CtValue]> {
    if !crate::Comptime::ComputeLite::tensor_window_is_live(fields) {
        return None;
    }
    fields.iter().find_map(|(name, value)| {
        (name == "window").then(|| match value {
            CtValue::List(args) => Some(args.as_slice()),
            _ => None,
        })
    }).flatten()
}

/// D-TASKBORROW1=A: the runtime slot a loaned window addresses. A loaned handle
/// carries no owner local, because a child thread cannot name its parent's
/// scope.
pub(super) fn view_mut_loan(fields: &[(String, CtValue)]) -> Option<usize> {
    fields
        .iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            ("loan", CtValue::Int(slot)) => usize::try_from(*slot).ok(),
            _ => None,
        })
}

/// The inclusive window bounds a `__JetViewMut` handle carries. Unlike
/// `view_mut_parts` this does not need an owner local, so it also reads a
/// loaned handle.
pub(super) fn view_mut_bounds(fields: &[(String, CtValue)]) -> Option<(i64, i64)> {
    let mut start = None;
    let mut end = None;
    for (name, value) in fields {
        match (name.as_str(), value) {
            ("start", CtValue::Int(n)) => start = Some(*n),
            ("end", CtValue::Int(n)) => end = Some(*n),
            _ => {}
        }
    }
    Some((start?, end?))
}

/// The element list behind a view owner, materializing a compute tensor.
fn view_owner_items(owner: &CtValue, span: Span) -> Result<Vec<CtValue>, Diagnostic> {
    match owner {
        CtValue::List(items) => Ok(items.clone()),
        CtValue::Struct { type_name, .. } if type_name == "Tensor" || type_name == "JetTensor" => {
            match crate::Comptime::ComputeLite::tensor_to_list_value(owner, span)? {
                CtValue::List(items) => Ok(items),
                _ => Err(unsupported("Tensor view owner data", span)),
            }
        }
        _ => Err(unsupported("view-mut owner list", span)),
    }
}

/// The value that replaces a view owner when a window write lands on it.
fn view_owner_replacement(
    owner: &CtValue,
    items: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    match owner {
        CtValue::Struct { type_name, .. } if type_name == "Tensor" || type_name == "JetTensor" => {
            crate::Comptime::ComputeLite::tensor_replace_data(owner, items, span)
        }
        _ => Ok(CtValue::List(items)),
    }
}

impl<'a> EvalCtx<'a> {
    /// The value a `__JetViewMut` handle windows into.
    ///
    /// D-TASKBORROW1=A: a loaned handle resolves to its shared runtime slot,
    /// which holds exactly the loaned window. The slot is therefore the owner
    /// as far as the borrowing child is concerned, and the handle's absolute
    /// indexes are already rebased onto it.
    pub(super) fn view_mut_owner_value(
        &self,
        fields: &[(String, CtValue)],
        scope: &HashMap<String, CtValue>,
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        if !crate::Comptime::ComputeLite::tensor_window_is_live(fields) {
            return Err(unsupported("Tensor view window", span));
        }
        if let Some(slot) = view_mut_loan(fields) {
            return Ok(self
                .place_loan_slot(slot, span)?
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .clone());
        }
        let (base, path, _, _) =
            view_mut_parts(fields).ok_or_else(|| unsupported("view-mut fields", span))?;
        let root = scope
            .get(&base)
            .ok_or_else(|| unsupported("view-mut owner", span))?;
        project_list_place(root, &path, span).cloned()
    }

    pub(super) fn store_view_mut_owner_value(
        &self,
        fields: &[(String, CtValue)],
        scope: &mut HashMap<String, CtValue>,
        replacement: CtValue,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if let Some(slot) = view_mut_loan(fields) {
            let slot = self.place_loan_slot(slot, span)?;
            *slot
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) = replacement;
            return Ok(());
        }
        let (base, path, _, _) =
            view_mut_parts(fields).ok_or_else(|| unsupported("view-mut fields", span))?;
        let root = scope
            .get(&base)
            .cloned()
            .ok_or_else(|| unsupported("view-mut owner", span))?;
        let updated = replace_list_place(root, &path, replacement, span)?;
        scope.insert(base, updated);
        Ok(())
    }

    pub(super) fn load_view_mut_owner_list(
        &self,
        fields: &[(String, CtValue)],
        scope: &HashMap<String, CtValue>,
        span: Span,
    ) -> Result<Vec<CtValue>, Diagnostic> {
        if let Some(slot) = view_mut_loan(fields) {
            let slot = self.place_loan_slot(slot, span)?;
            let owner = slot.lock().unwrap_or_else(|poison| poison.into_inner());
            return view_owner_items(&owner, span);
        }
        let (base, path, _, _) =
            view_mut_parts(fields).ok_or_else(|| unsupported("view-mut fields", span))?;
        let root = scope
            .get(&base)
            .ok_or_else(|| unsupported("view-mut owner", span))?;
        view_owner_items(project_list_place(root, &path, span)?, span)
    }

    pub(super) fn store_view_mut_owner_list(
        &self,
        fields: &[(String, CtValue)],
        scope: &mut HashMap<String, CtValue>,
        items: Vec<CtValue>,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if let Some(slot) = view_mut_loan(fields) {
            let slot = self.place_loan_slot(slot, span)?;
            let mut owner = slot.lock().unwrap_or_else(|poison| poison.into_inner());
            let replacement = view_owner_replacement(&owner, items, span)?;
            *owner = replacement;
            return Ok(());
        }
        let (base, path, _, _) =
            view_mut_parts(fields).ok_or_else(|| unsupported("view-mut fields", span))?;
        let root = scope
            .get(&base)
            .cloned()
            .ok_or_else(|| unsupported("view-mut owner", span))?;
        let replacement =
            view_owner_replacement(project_list_place(&root, &path, span)?, items, span)?;
        let updated = replace_list_place(root, &path, replacement, span)?;
        scope.insert(base, updated);
        Ok(())
    }

    /// Resolve a `__JetViewMut { base | loan, start, end }` handle to the
    /// inclusive window List.
    pub(super) fn materialize_view_mut_window(
        &self,
        fields: &[(String, CtValue)],
        scope: &HashMap<String, CtValue>,
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        let (start, end) =
            view_mut_bounds(fields).ok_or_else(|| unsupported("view-mut fields", span))?;
        let owner = self.view_mut_owner_value(fields, scope, span)?;
        if let Some(window) = view_mut_window_args(fields) {
            if matches!(&owner, CtValue::Struct { type_name, .. } if type_name == "Tensor" || type_name == "JetTensor") {
                return crate::Comptime::ComputeLite::tensor_view_list(&owner, window, span);
            }
        }
        let items = self.load_view_mut_owner_list(fields, scope, span)?;
        let (start, end_exclusive) = if end < start {
            if end.checked_add(1) != Some(start) {
                return Err(view_bounds_diagnostic(
                    range_semantics::jet_view_bounds_error(
                        start,
                        end,
                        false,
                        items.len() as i64,
                    ),
                    span,
                ));
            }
            range_semantics::jet_checked_view_bounds(start, start, true, items.len() as i64)
                .map_err(|_| {
                    view_bounds_diagnostic(
                        range_semantics::jet_view_bounds_error(
                            start,
                            end,
                            false,
                            items.len() as i64,
                        ),
                        span,
                    )
                })?
        } else {
            checked_view_window(start, end, false, items.len(), span)?
        };
        Ok(CtValue::List(
            items[start as usize..end_exclusive as usize].to_vec(),
        ))
    }
}

fn rebase_view_mut_owners(
    value: &mut CtValue,
    owners: &HashMap<String, String>,
) {
    match value {
        CtValue::Struct { type_name, fields } if type_name == "__JetViewMut" => {
            if let Some((_, CtValue::Str(base))) =
                fields.iter_mut().find(|(name, _)| name == "base")
            {
                if let Some(owner) = owners.get(base) {
                    *base = owner.clone();
                }
            }
        }
        CtValue::Struct { fields, .. } => {
            for (_, field) in fields {
                rebase_view_mut_owners(field, owners);
            }
        }
        CtValue::Enum { args, .. } => {
            for (_, arg) in args {
                rebase_view_mut_owners(arg, owners);
            }
        }
        CtValue::List(values) => {
            for value in values {
                rebase_view_mut_owners(value, owners);
            }
        }
        CtValue::Map(values) => {
            for value in values.values_mut() {
                rebase_view_mut_owners(value, owners);
            }
        }
        CtValue::Present(value) | CtValue::Failed(CtReport::Told(value)) => {
            rebase_view_mut_owners(value, owners);
        }
        _ => {}
    }
}

#[derive(Debug)]
pub(super) enum Flow {
    Normal,
    Break,
    Continue,
    BreakLabel(String),
    BreakValue(Option<String>, CtValue),
    ContinueLabel(String),
    Return(CtValue),
}

const DEV_FUEL: u64 = 1_000_000_000;
#[allow(dead_code)]
const CT_FUEL: u64 = 10_000_000;

pub(super) struct EvalCtx<'a> {
    pub(super) funcs: HashMap<String, &'a TFunc>,
    #[allow(dead_code)]
    pub(super) base_dir: PathBuf,
    pub(super) source_file: String,
    pub(super) source_text: String,
    pub(super) fuel: u64,
    pub(super) sink: Option<Arc<Mutex<DevSink>>>,
    #[allow(dead_code)]
    pub(super) core_imports: &'a HashMap<String, String>,
    pub(super) globals: HashMap<String, CtValue>,
    /// Runtime-only address identities minted by `core.mem.address_of`. The
    /// Foundation sentry kernel owns their provenance; this map only lets the
    /// CtValue carrier recover the source place after `Ptr.from_addr`.
    pub(super) sentry_places: HashMap<usize, String>,
    /// Synthetic allocator identities used only by the TIR evaluator. The
    /// Foundation sentry kernel still owns their live/dead state.
    pub(super) next_sentry_allocator: usize,
    #[allow(dead_code)]
    pub(super) gates: jet_foundation::Policy::GateSet,
    #[allow(dead_code)]
    pub(super) impure_depth: usize,
    /// True only for an actual dev/JIT runtime execution. Comptime may permit
    /// explicit I/O, but it must never open a Browser session while compiling.
    pub(super) runtime_execution: bool,
    /// Keep calls inside a codec-sensitive named deopt on canonical TIR.
    pub(super) prefer_tir_calls: bool,
    pub(super) repl_mode: bool,
    /// Lexical REPL capabilities forwarded from the frontend. Authorization
    /// decisions remain in the shared Comptime host seam.
    pub(super) repl_grants: Vec<String>,
    pub(super) repl_authorizer: Option<&'a mut dyn Comptime::ReplAuthorizer>,
    pub(super) pending_return: Option<CtValue>,
    pub(super) preserve_allocator_view: bool,
    /// `defer close(^…)` exprs scheduled in the current eval frame (LIFO).
    pub(super) deferred_closes: Vec<&'a TExpr>,
    /// Control emitted by an inline loop expression that targets an enclosing
    /// loop. The containing statement list consumes and propagates it.
    pub(super) pending_flow: Option<Flow>,
    /// Compiler-private eager List sinks for raw comptime yielding-loop
    /// fragments. Fully checked programs rewrite these sends to `List.push`.
    pub(super) collecting_items: Vec<Vec<CtValue>>,
    pub(super) call_depth: usize,
    pub(super) source_nesting: usize,
    pub(super) current_span: Span,
    pub(super) current_fn: String,
    pub(super) embed_inputs: Option<&'a mut Vec<crate::AST::ComptimeInput>>,
    /// `TypeName -> [(field, redact)]` for JetDebug formatting (D-DISPLAYDBG).
    pub(super) struct_fields: HashMap<String, Vec<(String, bool)>>,
    /// D-FIELDMEMO1=A: sema-owned source-to-memo edges shared with the
    /// interpreter invalidation adapter.
    pub(super) memo_dependencies: HashMap<String, HashMap<String, Vec<String>>>,
    /// Registered reflection rows shared with comptime reflection and runtime
    /// projections. This is the only source for reflected field names.
    pub(super) reflection_fields: HashMap<String, Vec<ReflectionField>>,
    /// Canonical typeable paths for runtime reflection.
    pub(super) reflect_paths: HashMap<String, String>,
    /// Declared generic parameter names per reflected struct, in source order.
    pub(super) struct_type_params: HashMap<String, Vec<String>>,
    /// `TypeName -> [(field, Type)]` for `core.data.csv` / decode on deopt.
    pub(super) struct_field_types: HashMap<String, Vec<(String, crate::AST::Type)>>,
    /// Sema-compiled published-schema plans. These contain wire keys and
    /// lowered helper names, never AST expressions or a field/type tuple codec.
    pub(super) codec_migrations: HashMap<String, TIR::TCodecMigrationPlan>,
    pub(super) distinct_bases: HashMap<String, crate::AST::Type>,
    pub(super) distinct_ranges: HashMap<String, (i64, i64)>,
    /// Current `MixedSwitch` subject for structured field conditions.
    switch_subject: Option<CtValue>,
    /// Handle-backed runtime state is shared by lexical task children. The TIR
    /// and local variable scopes remain borrowed by each evaluator context.
    runtime: Arc<Mutex<EvalRuntime<'a>>>,
    /// Thread-confined Cell values and loans. Never shared with spawned tasks.
    local_cells: local_cell::EvalLocalCells,
    shared_transactions: Vec<EvalSharedTransaction<'a>>,
    /// Spawn bodies are lowered separately because native tiers compile them as
    /// independent functions. The evaluator records each outcome behind a task
    /// handle, then observes it only at join/group boundaries.
    spawn_lambdas: &'a [TJitSpawnLambda],
    task_sender: Option<mpsc::Sender<EvalTaskJob<'a>>>,
    task_cancel: Option<Arc<AtomicBool>>,
    task_paused: Option<Arc<AtomicBool>>,
    pub(super) context_deadline: Option<i64>,
    pub(super) shield_depth: usize,
    /// Direct yield delivery keeps generator evaluation streaming: no eager
    /// collection is materialized between producer and consumer.
    yield_consumer: Option<YieldConsumer<'a>>,
    yield_scope: Option<HashMap<String, CtValue>>,
    /// `scope.guard` cleanups — run LIFO when the enclosing function returns.
    scope_guards: Vec<&'a TIR::TLambda>,
    /// Owned Shared lock leases acquired by this evaluator context.
    shared_guards: Vec<usize>,
    /// Nested `#Transact` frames for auto-snapshot + commit/rollback hooks.
    txn_stack: Vec<EvalTxnFrame<'a>>,
    /// D-TASKBORROW1=A: whole-place write windows currently loaned to task
    /// children, innermost `task.group` last. Opened at the spawn boundary,
    /// written back into the owner when the group joins.
    place_loans: Vec<EvalPlaceLoan>,
    /// One `place_loans` watermark per open `task.group` block.
    loan_scopes: Vec<usize>,
}

/// D-TASKBORROW1=A: one open loan of a write window.
///
/// AOT and Cranelift hand the child a real reference into the owner's storage.
/// The evaluator runs children on their own threads with their own scopes, so
/// the window is backed by one shared runtime slot for the life of the loan
/// and flushed into the owner when the group joins. Without this the child
/// cannot even name the owner local and the whole join fails (I9).
struct EvalPlaceLoan {
    /// Window local rebound to the loan on the spawning side.
    source: String,
    /// The pre-loan window handle, restored into `source` when the loan closes.
    restore: CtValue,
    /// Owner local the window projects out of.
    base: String,
    path: Vec<ViewMutPathStep>,
    /// Inclusive element range a `__JetViewMut` place region covers. `None` is
    /// a whole-place `__JetPlaceMut` window, which loans the place itself.
    window: Option<(i64, i64)>,
    slot: usize,
}

pub(super) struct EvalTxnFrame<'a> {
    pub(super) snapshots: Vec<(String, CtValue)>,
    pub(super) on_commit: Vec<&'a TIR::TLambda>,
    pub(super) on_rollback: Vec<&'a TIR::TLambda>,
}

pub(super) struct EvalSharedDelta<'a> {
    pub(super) shared_index: usize,
    pub(super) lambda: &'a TIR::TLambda,
    pub(super) captured: HashMap<String, CtValue>,
}

pub(super) struct EvalSharedTransaction<'a> {
    pub(super) transaction: shared_protocol::JetSharedTransaction,
    pub(super) deltas: Vec<EvalSharedDelta<'a>>,
}

impl<'a> EvalSharedTransaction<'a> {
    pub(super) fn new() -> Self {
        Self {
            transaction: shared_protocol::jet_shared_transaction_begin(),
            deltas: Vec::new(),
        }
    }
}

enum EvalCallable<'a> {
    Lambda {
        lambda: &'a TIR::TLambda,
        captured: HashMap<String, CtValue>,
    },
    Named(&'a str),
    ComputeTransform {
        base: CtValue,
        method: String,
        targets: Vec<i64>,
        result_ty: Type,
    },
    ComputePull {
        output: CtValue,
        anchor: CtValue,
        targets: Vec<i64>,
        gradient_ty: Type,
    },
    ComputeGrads {
        output: CtValue,
        anchor: CtValue,
        targets: Vec<i64>,
        gradient_ty: Type,
    },
}

enum EvalCallableSnapshot<'a> {
    Lambda {
        lambda: &'a TIR::TLambda,
        captured: HashMap<String, CtValue>,
    },
    Named(&'a str),
    ComputeTransform {
        base: CtValue,
        method: String,
        targets: Vec<i64>,
        result_ty: Type,
    },
    ComputePull {
        output: CtValue,
        anchor: CtValue,
        targets: Vec<i64>,
        gradient_ty: Type,
    },
    ComputeGrads {
        output: CtValue,
        anchor: CtValue,
        targets: Vec<i64>,
        gradient_ty: Type,
    },
}

#[derive(Clone)]
struct EvalApp {
    steps: Vec<EvalAppStep>,
}

#[derive(Clone)]
struct EvalAppStep {
    method: String,
    args: Vec<CtValue>,
}

/// D-CONC-STREAM1=A: a stream producer is a scheduler child, so the handle
/// carries the child's ordinary `JetTaskControl` — not a stream-local
/// cancellation fact. `break` cancels that control and `yield` reads it at the
/// shared task wait point.
struct EvalStream<'a> {
    func: &'a TFunc,
    args: Vec<CtValue>,
    control: Arc<crate::scheduler::JetTaskControl>,
}

const TIR_SELECT_BUILDER: &str = "__JetTirSelectBuilder";
const TIR_SELECT_AFTER: &str = "__JetTirSelectAfter";

struct EvalChannel {
    channel: crate::scheduler::JetSchedulerChannel<CtValue>,
    sender: crate::scheduler::JetSchedulerSender<CtValue>,
}

struct EvalRuntime<'a> {
    callables: Vec<EvalCallable<'a>>,
    interrupt_handlers: Vec<usize>,
    /// Process-edge callbacks. The evaluator invokes these after lexical
    /// cleanup, matching the Prelude runtime boundary used by AOT/JIT.
    atexit_handlers: Vec<usize>,
    streams: Vec<EvalStream<'a>>,
    shared_values: Vec<Arc<EvalSharedState>>,
    shared_guards: Vec<Arc<shared_protocol::JetSharedGuardState>>,
    shared_conditions: Vec<Arc<shared_protocol::JetConditionProtocol>>,
    clocks: Vec<i64>,
    channels: Vec<EvalChannel>,
    task_groups: Vec<Arc<crate::task_group::JetTaskGroupRuntime<usize>>>,
    tasks: Vec<Option<EvalTask>>,
    apps: Vec<EvalApp>,
    /// D-TASKBORROW1=A: storage behind every open whole-place task loan. This
    /// is the evaluator's stand-in for the address AOT passes to the child, so
    /// parent and child address one slot instead of two scope copies.
    place_loan_slots: Vec<Arc<Mutex<CtValue>>>,
    /// D-MEMO1=A: one Prelude memo store per memoized function. The evaluator
    /// keeps only the key/result values; bound, LRU order, and counters live in
    /// the shared Memo substrate.
    memos: MemoState,
    allocators: HashMap<usize, EvalAllocator>,
    gc_roots: Vec<gc_runtime::AutomaticRoot<CtValue>>,
    completion_order: AtomicU64,
}

struct EvalAllocator {
    generation: u32,
    used: usize,
    capacity: usize,
    allocator: String,
    fixed: bool,
    slots: Vec<CtValue>,
    closed: bool,
}

struct EvalSharedState {
    value: Mutex<CtValue>,
    protocol: Arc<shared_protocol::JetSharedProtocol>,
}

impl EvalSharedState {
    fn new(value: CtValue) -> Self {
        Self {
            value: Mutex::new(value),
            protocol: shared_protocol::JetSharedProtocol::new(),
        }
    }

    fn acquire(
        self: &Arc<Self>,
        editable: bool,
        cancel: Option<&Arc<AtomicBool>>,
    ) -> Option<Arc<shared_protocol::JetSharedPermit>> {
        shared_protocol::jet_shared_acquire(&self.protocol, editable, || {
            cancel.is_some_and(|cancel| cancel.load(Ordering::Acquire))
        })
    }

    fn acquire_guard(
        self: &Arc<Self>,
        editable: bool,
        cancel: Option<&Arc<AtomicBool>>,
    ) -> Option<Arc<shared_protocol::JetSharedGuardState>> {
        shared_protocol::jet_shared_guard_acquire(&self.protocol, editable, || {
            cancel.is_some_and(|cancel| cancel.load(Ordering::Acquire))
        })
    }
}

struct EvalConditionWaiter {
    notified: Mutex<bool>,
    wake: Condvar,
    cancel: Option<Arc<AtomicBool>>,
    deadline: Option<i64>,
}

impl EvalConditionWaiter {
    fn new(cancel: Option<Arc<AtomicBool>>, deadline: Option<i64>) -> Self {
        Self {
            notified: Mutex::new(false),
            wake: Condvar::new(),
            cancel,
            deadline,
        }
    }
}

impl shared_protocol::JetConditionWaiter for EvalConditionWaiter {
    fn park(&self) -> Result<(), ()> {
        let mut notified = self
            .notified
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while !*notified {
            if self
                .cancel
                .as_ref()
                .is_some_and(|cancel| cancel.load(Ordering::Acquire))
                || self.deadline.is_some_and(|deadline| wall_now_ms() >= deadline)
            {
                return Err(());
            }
            let (next, _) = self
                .wake
                .wait_timeout(notified, std::time::Duration::from_millis(10))
                .unwrap_or_else(|error| error.into_inner());
            notified = next;
        }
        Ok(())
    }

    fn wake(&self) {
        *self
            .notified
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = true;
        self.wake.notify_one();
    }

    fn interrupted(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(Ordering::Acquire))
            || self.deadline.is_some_and(|deadline| wall_now_ms() >= deadline)
    }
}

struct EvalTask {
    completion: mpsc::Receiver<EvalTaskCompletion>,
    completion_order: Arc<OnceLock<u64>>,
    completion_wait: Arc<crate::scheduler::ParkSlot>,
    control: Arc<crate::scheduler::JetTaskControl>,
}

struct EvalTaskCompletion {
    result: Result<CtValue, Diagnostic>,
}

#[derive(Debug)]
enum EvalTaskSelectError {
    /// The parent reached its own cancellation/deadline wait policy.
    Wait(Diagnostic),
    /// A child failed while completing. This becomes `? TaskFailure` at the
    /// task surface instead of escaping as an interpreter diagnostic.
    Child(Diagnostic),
}

struct EvalTaskJob<'a> {
    lambda: &'a TJitSpawnLambda,
    captured: HashMap<String, CtValue>,
    task_sender: mpsc::Sender<EvalTaskJob<'a>>,
    context_deadline: Option<i64>,
    completion: mpsc::SyncSender<EvalTaskCompletion>,
    completion_order: Arc<OnceLock<u64>>,
    completion_wait: Arc<crate::scheduler::ParkSlot>,
    control: Arc<crate::scheduler::JetTaskControl>,
    permit: Option<crate::task_group::JetTaskGroupPermit>,
}

fn select_eval_tasks(
    tasks: Vec<EvalTask>,
    mode: crate::task_group::JetTaskSelectMode,
    span: Span,
    mut wait_check: impl FnMut() -> Result<(), Diagnostic>,
) -> Result<Vec<CtValue>, EvalTaskSelectError> {
    crate::task_group::jet_task_select(
        tasks,
        mode,
        || wait_check().map_err(EvalTaskSelectError::Wait),
        |task| task.completion_order.get().copied().map(Into::into),
        |task| match task.completion.try_recv() {
            Ok(completion) => Some(completion.result.map_err(EvalTaskSelectError::Child)),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(EvalTaskSelectError::Child(unsupported(
                    "task completion",
                    span,
                ))))
            }
        },
        |task| task.control.cancel(),
        |task| {
            let _ = task.completion.recv();
        },
    )
}

#[derive(Clone)]
struct EvalTaskConfig<'a> {
    funcs: HashMap<String, &'a TFunc>,
    base_dir: PathBuf,
    source_file: String,
    source_text: String,
    sink: Option<Arc<Mutex<DevSink>>>,
    core_imports: &'a HashMap<String, String>,
    globals: HashMap<String, CtValue>,
    gates: jet_foundation::Policy::GateSet,
    impure_depth: usize,
    runtime_execution: bool,
    prefer_tir_calls: bool,
    repl_mode: bool,
    repl_grants: Vec<String>,
    struct_fields: HashMap<String, Vec<(String, bool)>>,
    memo_dependencies: HashMap<String, HashMap<String, Vec<String>>>,
    reflection_fields: HashMap<String, Vec<ReflectionField>>,
    reflect_paths: HashMap<String, String>,
    struct_type_params: HashMap<String, Vec<String>>,
    struct_field_types: HashMap<String, Vec<(String, Type)>>,
    codec_migrations: HashMap<String, TIR::TCodecMigrationPlan>,
    distinct_bases: HashMap<String, Type>,
    distinct_ranges: HashMap<String, (i64, i64)>,
    spawn_lambdas: &'a [TJitSpawnLambda],
    runtime: Arc<Mutex<EvalRuntime<'a>>>,
}

impl EvalRuntime<'_> {
    fn new() -> Self {
        Self::with_memos(new_memo_state())
    }

    fn with_memos(memos: MemoState) -> Self {
        Self {
            callables: Vec::new(),
            interrupt_handlers: Vec::new(),
            atexit_handlers: Vec::new(),
            streams: Vec::new(),
            shared_values: Vec::new(),
            shared_guards: Vec::new(),
            shared_conditions: Vec::new(),
            clocks: Vec::new(),
            channels: Vec::new(),
            task_groups: Vec::new(),
            tasks: Vec::new(),
            apps: Vec::new(),
            place_loan_slots: Vec::new(),
            memos,
            allocators: HashMap::new(),
            gc_roots: Vec::new(),
            completion_order: AtomicU64::new(0),
        }
    }
}

#[derive(Clone)]
struct YieldConsumer<'a> {
    var: String,
    body: &'a [TStmt],
    /// The producer child's control: `break` out of the consumer cancels it,
    /// which is what the producer's next wait point observes.
    producer: Arc<crate::scheduler::JetTaskControl>,
    /// The consuming frame's wait-point facts, restored while a delivered value
    /// runs its body. A delivered value executes under the consuming task's
    /// cancellation and its own shield depth: the producer child is a separate
    /// task, so a `#Shield` in the producer never shields the consumer.
    consumer_cancel: Option<Arc<AtomicBool>>,
    consumer_shield_depth: usize,
}

impl<'a> EvalCtx<'a> {
    /// D-DEADLINE1 / I2: a `#Context(deadline: …)` budget blown at a wait point
    /// the JOINING PARENT owns is a program-side stop, not a compiler boundary.
    /// AOT (`SchedulerHost.rs::jet_deadline_exceeded`) and the resident JIT
    /// (`jet-jit/src/Concurrency.rs::record_deadline_interrupt`) both keep
    /// everything the program already printed, write the one Prelude-rendered
    /// E3003 report to stderr, and stop with 70 — the shape pinned by
    /// `examples/features/expected/concurrency/deadline_context.{out,err.out}`
    /// and by `assert_concurrency_and_game_three_way`. Rendering a registered
    /// diagnostic instead made the interpreter answer `RunOutcome::Problems`,
    /// which drops that stdout and that exit code, so the interpreter tier
    /// disagreed with every other tier about the same program. This is the same
    /// seam `runtime_stop` and the panic/require stops use: with no sink there is
    /// no run to stop, so comptime still sees the registered row at its span.
    ///
    /// A CHILD task's blown deadline is NOT a process stop: `task_failure_value`
    /// reads the E3003 code to build `TaskFailure.DeadlineBlown` (D-CONC-FAIL1=A),
    /// so a child keeps the plain diagnostic exactly as the panic sites do.
    fn deadline_stop(&self, deadline: crate::task_group::JetTaskDeadline) -> Diagnostic {
        let registered = || {
            crate::Sema::Diagnostics::render_registered(
                "E3003",
                deadline.what.clone(),
                deadline.why.clone(),
                deadline.fix.clone(),
                Some(self.span()),
            )
        };
        if self.task_cancel.is_some() {
            return registered();
        }
        let Some(sink) = self.sink.as_ref() else {
            return registered();
        };
        let mut rendered = deadline.render();
        if !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        let mut sink = sink.lock().expect("evaluator sink poisoned");
        sink.stderr.push_str(&rendered);
        sink.exit_code = Some(70);
        crate::Sema::Diagnostics::soft_exit(
            "70".to_string(),
            "runtime stop E3003".to_string(),
            Some(self.span()),
        )
    }

    /// Republish the report a failed stream producer recorded at its stop.
    ///
    /// `Prelude/Stream.rs`'s completion boundary does exactly this: a failed
    /// producer publishes the COMPLETE report from
    /// `jet_stream_take_failure_report()`, not the bare message a task frame
    /// carries. The producer runs inline in the consumer's frame here, so the
    /// record and the take are on one thread and the slot is the whole
    /// handoff.
    ///
    /// The stop is already rendered, so this is the same "print it verbatim"
    /// edge `jet_runtime_caught_stop` owns — hand the boundary a soft exit
    /// afterwards so it does not render a second, locationless report from a
    /// `why` that was never a panic message.
    fn stream_producer_failure(&self, error: Diagnostic) -> Diagnostic {
        // Only the child's own panic claims the slot. `task_child_stop` is the
        // one recorder, and it mints exactly this code, so an escape that never
        // recorded — a cancel, an `unsupported` boundary — cannot walk off with
        // a report some earlier task on this pooled thread left behind.
        if error.code != "E0953" {
            return error;
        }
        let Some(report) = crate::scheduler::jet_stream_take_failure_report() else {
            return error;
        };
        let Some(sink) = self.sink.as_ref() else {
            return error;
        };
        let mut sink = sink.lock().expect("evaluator sink poisoned");
        sink.stderr.push_str(&report);
        if !report.ends_with('\n') {
            sink.stderr.push('\n');
        }
        sink.exit_code = Some(jet_foundation::ExitCodes::RUNTIME_PANIC);
        crate::Sema::Diagnostics::soft_exit(
            "70".to_string(),
            "stream producer failure".to_string(),
            Some(self.span()),
        )
    }

    fn scheduler_wait<T>(
        &self,
        wait_kind: &str,
        wait: impl FnOnce() -> T,
    ) -> Result<T, Diagnostic> {
        match crate::scheduler::jet_scheduler_wait_without_unwind(wait) {
            crate::scheduler::JetSchedulerWait::Ready(value) => Ok(value),
            crate::scheduler::JetSchedulerWait::Cancelled => {
                let cancelled = crate::task_group::jet_task_cancellation();
                Err(crate::Sema::Diagnostics::render_registered(
                    cancelled.code,
                    cancelled.what.to_string(),
                    cancelled.why.to_string(),
                    cancelled.fix.to_string(),
                    Some(self.span()),
                ))
            }
            crate::scheduler::JetSchedulerWait::Deadline(_) => Err(
                self.deadline_stop(crate::task_group::jet_task_deadline(wait_kind)),
            ),
            crate::scheduler::JetSchedulerWait::Panicked(message) => {
                Err(task_child_panic(message, self.span()))
            }
        }
    }

    fn task_wait_check(&self, wait_kind: &str) -> Result<(), Diagnostic> {
        self.task_wait_while_paused()?;
        let deadline = crate::task_group::jet_task_deadline_if_expired(
            self.context_deadline
                .map(|deadline| {
                    deadline.saturating_sub(crate::scheduler::jet_std_time_now())
                }),
            wait_kind,
        );
        let cancelled = self
            .task_cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(Ordering::Acquire));
        match crate::task_group::jet_task_wait_policy(deadline, cancelled, self.shield_depth > 0) {
            Ok(()) => Ok(()),
            Err(crate::task_group::JetTaskWaitInterrupt::Deadline(deadline)) => {
                Err(self.deadline_stop(deadline))
            }
            Err(crate::task_group::JetTaskWaitInterrupt::Cancelled) => {
                Err(crate::Sema::Diagnostics::task_cancelled(Some(self.span())))
            }
        }
    }

    fn task_wait_cancel_check(&self) -> Result<(), Diagnostic> {
        self.task_wait_check("task selection")
    }

    fn task_join_wait_check(&self) -> Result<(), Diagnostic> {
        self.task_wait_check("task join")
    }

    /// The evaluator twin of `JetTaskControl::wait_while_paused`: a paused task
    /// stops at its next cooperative wait point and stays there until it is
    /// resumed or cancelled. The shared scheduler owns the park, wake, and
    /// deadline behavior; this method only maps its boundary status.
    fn task_wait_while_paused(&self) -> Result<(), Diagnostic> {
        if self.task_paused.is_none() {
            return Ok(());
        }
        self.scheduler_wait(
            "task pause",
            crate::scheduler::jet_scheduler_wait_while_paused,
        )
    }

    /// D-VERDICT-1323-1 / D-COROUTINE1=A: set or clear one task's pause flag
    /// without consuming the handle — the evaluator twin of `JetTask::pause`
    /// and `JetTask::resume`.
    pub(super) fn set_task_paused_value(
        &mut self,
        value: &CtValue,
        paused: bool,
    ) -> Result<(), Diagnostic> {
        let index = Self::task_index(value)
            .ok_or_else(|| unsupported("task receiver", self.span()))?;
        if let Some(Some(task)) = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .tasks
            .get(index)
        {
            if paused {
                task.control.pause();
            } else {
                task.control.resume();
            }
        }
        Ok(())
    }

    /// The evaluator twin of `JetTask::detach`: drop the join handle so the
    /// task runs unattached and its result is never observed.
    pub(super) fn detach_task_value(&mut self, value: &CtValue) -> Result<(), Diagnostic> {
        let index = Self::task_index(value)
            .ok_or_else(|| unsupported("task receiver", self.span()))?;
        if let Some(slot) = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .tasks
            .get_mut(index)
        {
            let _ = slot.take();
        }
        Ok(())
    }

    fn task_config(&self) -> EvalTaskConfig<'a> {
        EvalTaskConfig {
            funcs: self.funcs.clone(),
            base_dir: self.base_dir.clone(),
            source_file: self.source_file.clone(),
            source_text: self.source_text.clone(),
            sink: self.sink.clone(),
            core_imports: self.core_imports,
            globals: self.globals.clone(),
            gates: self.gates,
            impure_depth: self.impure_depth,
            runtime_execution: self.runtime_execution,
            prefer_tir_calls: self.prefer_tir_calls,
            repl_mode: self.repl_mode,
            repl_grants: self.repl_grants.clone(),
            struct_fields: self.struct_fields.clone(),
            memo_dependencies: self.memo_dependencies.clone(),
            reflection_fields: self.reflection_fields.clone(),
            reflect_paths: self.reflect_paths.clone(),
            struct_type_params: self.struct_type_params.clone(),
            struct_field_types: self.struct_field_types.clone(),
            codec_migrations: self.codec_migrations.clone(),
            distinct_bases: self.distinct_bases.clone(),
            distinct_ranges: self.distinct_ranges.clone(),
            spawn_lambdas: self.spawn_lambdas,
            runtime: self.runtime.clone(),
        }
    }

    fn with_task_dispatcher<R>(&mut self, run: impl FnOnce(&mut Self) -> R) -> R {
        if self.task_sender.is_some() {
            return run(self);
        }
        let (ambient_core, ambient_handle) = crate::Comptime::ambient_hooks();
        std::thread::scope(|threads| {
            let (sender, receiver) = mpsc::channel();
            self.task_sender = Some(sender);
            let config = Arc::new(self.task_config());
            let dispatcher = std::thread::Builder::new()
                .name("jet-tir-task-dispatch".to_string())
                .stack_size(8 * 1024 * 1024)
                .spawn_scoped(threads, {
                    let config = config.clone();
                    move || {
                        while let Ok(job) = receiver.recv() {
                            let job_config = (*config).clone();
                            std::thread::Builder::new()
                                .name("jet-tir-task".to_string())
                                .stack_size(8 * 1024 * 1024)
                                .spawn_scoped(threads, move || {
                                    crate::Comptime::with_ambient(
                                        ambient_core,
                                        ambient_handle,
                                        || Self::run_eval_job(job_config, job),
                                    )
                                })
                                .expect("evaluator task worker");
                        }
                    }
                })
                .expect("evaluator task dispatcher");
            let result = run(self);
            drop(self.task_sender.take());
            dispatcher
                .join()
                .expect("evaluator task dispatcher panicked");
            result
        })
    }

    fn run_eval_job(config: EvalTaskConfig<'a>, job: EvalTaskJob<'a>) {
        let _permit = job.permit;
        crate::scheduler::jet_scheduler_set_task_control(Some(job.control.clone()));
        let _deadline = job
            .context_deadline
            .map(crate::scheduler::jet_ctx_push_deadline);
        let mut ctx = EvalCtx {
            funcs: config.funcs,
            base_dir: config.base_dir,
            source_file: config.source_file,
            source_text: config.source_text,
            fuel: DEV_FUEL,
            sink: config.sink,
            core_imports: config.core_imports,
            globals: config.globals,
            sentry_places: HashMap::new(),
            next_sentry_allocator: 1,
            gates: config.gates,
            impure_depth: config.impure_depth,
            runtime_execution: config.runtime_execution,
            prefer_tir_calls: config.prefer_tir_calls,
            repl_mode: config.repl_mode,
            repl_grants: config.repl_grants,
            repl_authorizer: None,
            pending_return: None,
            preserve_allocator_view: false,
            deferred_closes: Vec::new(),
            pending_flow: None,
            collecting_items: Vec::new(),
            call_depth: 0,
            source_nesting: 0,
            current_span: Span::new(0, 0),
            current_fn: String::new(),
            embed_inputs: None,
            struct_fields: config.struct_fields,
            memo_dependencies: config.memo_dependencies,
            reflection_fields: config.reflection_fields,
            reflect_paths: config.reflect_paths,
            struct_type_params: config.struct_type_params,
            struct_field_types: config.struct_field_types,
            codec_migrations: config.codec_migrations,
            distinct_bases: config.distinct_bases,
            distinct_ranges: config.distinct_ranges,
            switch_subject: None,
            runtime: config.runtime.clone(),
            local_cells: local_cell::EvalLocalCells::new(),
            shared_transactions: Vec::new(),
            spawn_lambdas: config.spawn_lambdas,
            task_sender: Some(job.task_sender),
            task_cancel: Some(job.control.cancelled.clone()),
            task_paused: Some(job.control.paused.clone()),
            context_deadline: job.context_deadline,
            shield_depth: 0,
            yield_consumer: None,
            yield_scope: None,
            scope_guards: Vec::new(),
            shared_guards: Vec::new(),
            txn_stack: Vec::new(),
            place_loans: Vec::new(),
            loan_scopes: Vec::new(),
        };
        let mut scope = job.captured;
        let result = match &job.lambda.body {
            TJitSpawnBody::Expr(expr) => ctx.eval_expr(expr, &mut scope),
            TJitSpawnBody::Block { prefix, tail } => match ctx.exec_stmts(prefix, &mut scope) {
                Ok(Flow::Return(value)) => Ok(value),
                Ok(Flow::Normal) => match tail {
                    Some(expr) => ctx.eval_expr(expr, &mut scope),
                    None => Ok(CtValue::Unit),
                },
                Ok(other) => Err(unsupported(
                    &format!("control flow {other:?} escaping spawn"),
                    ctx.span(),
                )),
                Err(error) => Err(error),
            },
            TJitSpawnBody::SharedBlock { body, tail } => {
                if *tail {
                    match body[..].split_last() {
                        Some((last, prefix)) => match ctx.exec_stmts(prefix, &mut scope) {
                            Ok(Flow::Return(value)) => Ok(value),
                            Ok(Flow::Normal) => match last {
                                TStmt::ExprStmt(expr) => ctx.eval_expr(expr, &mut scope),
                                TStmt::Return(Some(expr)) => ctx.eval_expr(expr, &mut scope),
                                _ => Ok(CtValue::Unit),
                            },
                            Ok(other) => Err(unsupported(
                                &format!("control flow {other:?} escaping shared spawn"),
                                ctx.span(),
                            )),
                            Err(error) => Err(error),
                        },
                        None => Err(unsupported("empty shared spawn body", ctx.span())),
                    }
                } else {
                    match ctx.exec_stmts(&body[..], &mut scope) {
                        Ok(Flow::Return(value)) => Ok(value),
                        Ok(Flow::Normal) => Ok(CtValue::Unit),
                        Ok(other) => Err(unsupported(
                            &format!("control flow {other:?} escaping shared spawn"),
                            ctx.span(),
                        )),
                        Err(error) => Err(error),
                    }
                }
            },
        };
        crate::scheduler::jet_scheduler_set_task_control(None);
        let order = config
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .completion_order
            .fetch_add(1, Ordering::AcqRel);
        job.completion_order
            .set(order)
            .expect("task completion recorded twice");
        let _ = job.completion.send(EvalTaskCompletion { result });
        job.completion_wait.wake();
    }

    fn eval_spawn(
        &mut self,
        site: usize,
        group: Option<&'a TExpr>,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let group = match group {
            Some(group) => Some(
                Self::taskgroup_index(&self.eval_expr(group, scope)?)
                    .ok_or_else(|| unsupported("task group handle", self.span()))?,
            ),
            None => None,
        };
        let lam = self
            .spawn_lambdas
            .get(site)
            .ok_or_else(|| unsupported("spawn body", self.span()))?;
        let span = self.span();
        let mut child = HashMap::new();
        for capture in &lam.captures {
            let value = scope
                .get(&capture.source)
                .cloned()
                .or_else(|| self.globals.get(&capture.source).cloned())
                .unwrap_or(CtValue::Unit);
            // D-TASKBORROW1=A: a whole-place write window crossing into a child
            // is a loan. AOT gives the child the owner's address; the evaluator
            // promotes the window to one shared slot both sides address.
            let value = self.open_place_loan(&capture.source, value, scope, span)?;
            child.insert(capture.name.clone(), value);
        }
        let sender = self
            .task_sender
            .as_ref()
            .ok_or_else(|| unsupported("spawn outside a task group", self.span()))?;
        let (completion, receiver) = mpsc::sync_channel(1);
        let completion_order = Arc::new(OnceLock::new());
        let completion_wait = crate::scheduler::ParkSlot::new();
        let control = crate::scheduler::JetTaskControl::new();
        let group_runtime = match group {
            Some(group) => Some(
                self.runtime
                    .lock()
                    .expect("evaluator runtime poisoned")
                    .task_groups
                    .get(group)
                    .cloned()
                    .ok_or_else(|| unsupported("task group handle", self.span()))?,
            ),
            None => None,
        };
        let _deadline = group_runtime
            .as_ref()
            .and_then(|_| self.context_deadline.map(crate::scheduler::jet_ctx_push_deadline));
        let permit = match group_runtime.as_ref() {
            Some(group) => {
                let waiter = crate::scheduler::ParkSlot::new();
                group.acquire_with(waiter, |waiter| {
                    self.task_wait_check("task admission")?;
                    self.scheduler_wait("task admission", || {
                        crate::scheduler::jet_scheduler_yield("task admission", waiter, None)
                    })?;
                    Ok(())
                })?
            }
            None => None,
        };
        drop(_deadline);
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let task = runtime.tasks.len();
        runtime.tasks.push(Some(EvalTask {
            completion: receiver,
            completion_order: completion_order.clone(),
            completion_wait: completion_wait.clone(),
            control: control.clone(),
        }));
        if let Some(group) = group_runtime {
            group.register(task);
        }
        drop(runtime);
        sender
            .send(EvalTaskJob {
                lambda: lam,
                captured: child,
                task_sender: sender.clone(),
                context_deadline: self.context_deadline,
                completion,
                completion_order,
                completion_wait,
                control,
                permit,
            })
            .map_err(|_| unsupported("closed task group", self.span()))?;
        Ok(CtValue::Struct {
            type_name: "__JetTirTask".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(task as i64))],
        })
    }

    /// Storage behind one open loan.
    fn place_loan_slot(
        &self,
        slot: usize,
        span: Span,
    ) -> Result<Arc<Mutex<CtValue>>, Diagnostic> {
        self.runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .place_loan_slots
            .get(slot)
            .cloned()
            .ok_or_else(|| unsupported("task loan storage", span))
    }

    /// Read the value a `__JetPlaceMut` handle windows into, or `None` when the
    /// value is not a window at all.
    pub(super) fn read_place_mut(
        &self,
        value: &CtValue,
        scope: &HashMap<String, CtValue>,
        span: Span,
    ) -> Option<Result<CtValue, Diagnostic>> {
        Some(match place_mut_target(value)? {
            PlaceMutTarget::Loan(slot) => self.place_loan_slot(slot, span).map(|slot| {
                slot.lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .clone()
            }),
            PlaceMutTarget::Owner(base, path) => match scope.get(&base) {
                Some(root) => project_list_place(root, &path, span).cloned(),
                None => Err(unsupported("place window owner", span)),
            },
        })
    }

    /// Write through the handle into the storage it windows into.
    pub(super) fn write_place_mut(
        &self,
        handle: &CtValue,
        replacement: CtValue,
        scope: &mut HashMap<String, CtValue>,
        span: Span,
    ) -> Option<Result<(), Diagnostic>> {
        Some(match place_mut_target(handle)? {
            PlaceMutTarget::Loan(slot) => self.place_loan_slot(slot, span).map(|slot| {
                *slot
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner()) = replacement;
            }),
            PlaceMutTarget::Owner(base, path) => match scope.get(&base).cloned() {
                Some(root) => {
                    replace_list_place(root, &path, replacement, span).map(|updated| {
                        scope.insert(base, updated);
                    })
                }
                None => Err(unsupported("place window owner", span)),
            },
        })
    }

    /// D-TASKBORROW1=A: what a capture can loan — an owner-rooted write
    /// window, either a whole-place `__JetPlaceMut` (`p :: &node`) or the
    /// `__JetViewMut` place region the split-view planner binds for a constant
    /// index (`left :: &particles[0]`). Returns the owner local, the path to
    /// the windowed place, and the inclusive element range for a region.
    ///
    /// A window that is already loaned returns `None`: one loan per window,
    /// shared by every child that names it.
    #[allow(clippy::type_complexity)]
    fn loanable_window(
        value: &CtValue,
    ) -> Option<(String, Vec<ViewMutPathStep>, Option<(i64, i64)>)> {
        let CtValue::Struct { type_name, fields } = value else {
            return None;
        };
        if type_name == PLACE_MUT_TYPE {
            return match place_mut_target(value)? {
                PlaceMutTarget::Owner(base, path) => Some((base, path, None)),
                PlaceMutTarget::Loan(_) => None,
            };
        }
        if type_name != "__JetViewMut" || view_mut_loan(fields).is_some() {
            return None;
        }
        let (base, path, start, end) = view_mut_parts(fields)?;
        Some((base, path, Some((start, end))))
    }

    /// D-TASKBORROW1=A: open the loan a capture needs to cross into a child.
    ///
    /// Values that are not owner-rooted write windows pass straight through.
    fn open_place_loan(
        &mut self,
        source: &str,
        value: CtValue,
        scope: &mut HashMap<String, CtValue>,
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        let Some((base, path, region)) = Self::loanable_window(&value) else {
            return Ok(value);
        };
        if self.loan_scopes.is_empty() {
            // No enclosing group means no join, so no boundary at which the
            // owner could see the child's writes. Refuse instead of dropping
            // them: a silent wrong answer is worse than a stop.
            return Err(unsupported("task borrow outside a task group", span));
        }
        let content = {
            let Some(root) = scope.get(&base) else {
                return Err(unsupported("place window owner", span));
            };
            let owner = project_list_place(root, &path, span)?;
            match region {
                None => owner.clone(),
                // A place region loans only its own elements. Two disjoint
                // bands of one owner therefore never share storage, so neither
                // child's read-modify-write can lose the other's write.
                Some((start, end)) => {
                    let CtValue::List(items) = owner else {
                        return Err(unsupported("task borrow window owner", span));
                    };
                    if start < 0 || end < start || end as usize >= items.len() {
                        return Err(unsupported("task borrow window bounds", span));
                    }
                    CtValue::List(items[start as usize..=end as usize].to_vec())
                }
            }
        };
        let slot = {
            let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
            runtime.place_loan_slots.push(Arc::new(Mutex::new(content)));
            runtime.place_loan_slots.len() - 1
        };
        // A region handle rebases onto the slot, which holds exactly the loaned
        // window: every view call site indexes its owner absolutely, so the
        // slot is the owner as far as the child is concerned.
        let handle = match region {
            None => place_loan_handle(slot),
            Some((start, end)) => view_loan_handle(slot, end - start + 1),
        };
        // The owner side addresses the loan too, so a read after the spawn sees
        // the child's writes rather than a stale copy.
        scope.insert(source.to_string(), handle.clone());
        self.place_loans.push(EvalPlaceLoan {
            source: source.to_string(),
            restore: value,
            base,
            path,
            window: region,
            slot,
        });
        Ok(handle)
    }

    /// Mark the loan watermark for a `task.group` block that just opened.
    fn open_loan_scope(&mut self) {
        self.loan_scopes.push(self.place_loans.len());
    }

    /// D-TASKBORROW1=A: every loan opened inside this `task.group` closes when
    /// the group joins, and the owner sees the writes from that point on.
    fn close_place_loans(
        &mut self,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        let span = self.span();
        let mark = self.loan_scopes.pop().unwrap_or(0);
        let closing = self.place_loans.split_off(mark.min(self.place_loans.len()));
        for loan in closing {
            let value = self
                .place_loan_slot(loan.slot, span)?
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .clone();
            let Some(root) = scope.get(&loan.base).cloned() else {
                return Err(unsupported("task loan owner", span));
            };
            let replacement = match loan.window {
                None => value,
                // Only this loan's own band lands back in the owner, so two
                // loans on one owner cannot overwrite each other.
                Some((start, _)) => {
                    let CtValue::List(loaned) = value else {
                        return Err(unsupported("task loan window", span));
                    };
                    let CtValue::List(mut items) =
                        project_list_place(&root, &loan.path, span)?.clone()
                    else {
                        return Err(unsupported("task loan window owner", span));
                    };
                    for (offset, element) in loaned.into_iter().enumerate() {
                        let index = start as usize + offset;
                        if index >= items.len() {
                            return Err(unsupported("task loan window bounds", span));
                        }
                        items[index] = element;
                    }
                    CtValue::List(items)
                }
            };
            let updated = replace_list_place(root, &loan.path, replacement, span)?;
            scope.insert(loan.base.clone(), updated);
            scope.insert(loan.source, loan.restore);
        }
        Ok(())
    }

    fn taskgroup_index(value: &CtValue) -> Option<usize> {
        Self::internal_index(value, "__JetTirTaskGroup")
    }

    fn task_index(value: &CtValue) -> Option<usize> {
        Self::internal_index(value, "__JetTirTask")
    }

    fn internal_index(value: &CtValue, expected: &str) -> Option<usize> {
        let CtValue::Struct { type_name, fields } = value else {
            return None;
        };
        if type_name != expected {
            return None;
        }
        fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
            ("index", CtValue::Int(index)) => usize::try_from(*index).ok(),
            _ => None,
        })
    }

    fn new_taskgroup(
        &mut self,
        limit: Option<&'a TExpr>,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let limit = match limit {
            Some(limit) => match self.eval_expr(limit, scope)? {
                // The shared Prelude owns defaulting and bound clamping for
                // every execution tier.
                CtValue::Int(value) => Some(value),
                _ => return Err(unsupported("task-group limit", self.span())),
            },
            None => None,
        };
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let index = runtime.task_groups.len();
        runtime.task_groups.push(Arc::new(
            crate::task_group::JetTaskGroupRuntime::new_defaulted(limit),
        ));
        Ok(CtValue::Struct {
            type_name: "__JetTirTaskGroup".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(index as i64))],
        })
    }

    fn select_builder_value(
        receivers: Vec<usize>,
        afters: Vec<(i64, CtValue)>,
    ) -> CtValue {
        CtValue::Struct {
            type_name: TIR_SELECT_BUILDER.to_string(),
            fields: vec![
                (
                    "receivers".to_string(),
                    CtValue::List(
                        receivers
                            .into_iter()
                            .map(|index| CtValue::Int(index as i64))
                            .collect(),
                    ),
                ),
                (
                    "afters".to_string(),
                    CtValue::List(
                        afters
                            .into_iter()
                            .map(|(duration_ns, value)| CtValue::Struct {
                                type_name: TIR_SELECT_AFTER.to_string(),
                                fields: vec![
                                    ("duration_ns".to_string(), CtValue::Int(duration_ns)),
                                    ("value".to_string(), value),
                                ],
                            })
                            .collect(),
                    ),
                ),
            ],
        }
    }

    fn select_builder_parts(
        value: &CtValue,
    ) -> Option<(Vec<usize>, Vec<(i64, CtValue)>)> {
        let CtValue::Struct { type_name, fields } = value else {
            return None;
        };
        if type_name != TIR_SELECT_BUILDER {
            return None;
        }
        let receivers = fields.iter().find_map(|(name, value)| {
            (name == "receivers").then(|| match value {
                CtValue::List(values) => values
                    .iter()
                    .map(|value| match value {
                        CtValue::Int(index) => usize::try_from(*index).ok(),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>(),
                _ => None,
            })
        })??;
        let afters = fields.iter().find_map(|(name, value)| {
            (name == "afters").then(|| match value {
                CtValue::List(values) => values
                    .iter()
                    .map(|value| {
                        let CtValue::Struct { type_name, fields } = value else {
                            return None;
                        };
                        if type_name != TIR_SELECT_AFTER {
                            return None;
                        }
                        let duration_ns = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
                            ("duration_ns", CtValue::Int(duration_ns)) => Some(*duration_ns),
                            _ => None,
                        })?;
                        let payload = fields
                            .iter()
                            .find_map(|(name, value)| (name == "value").then(|| value.clone()))?;
                        Some((duration_ns, payload))
                    })
                    .collect::<Option<Vec<_>>>(),
                _ => None,
            })
        })??;
        Some((receivers, afters))
    }

    pub(super) fn new_eval_channel(&mut self, capacity: Option<i64>) -> CtValue {
        let channel = match capacity {
            Some(capacity) => crate::scheduler::JetSchedulerChannel::bounded(capacity),
            None => crate::scheduler::JetSchedulerChannel::new(),
        };
        let sender = channel.sender();
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let index = runtime.channels.len();
        runtime.channels.push(EvalChannel { channel, sender });
        let sender_value = CtValue::Struct {
            type_name: "Sender".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(index as i64))],
        };
        let receiver_value = CtValue::Struct {
            type_name: "Receiver".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(index as i64))],
        };
        CtValue::Struct {
            type_name: "tuple".to_string(),
            fields: vec![
                (crate::Codegen::mangle("sender"), sender_value),
                (crate::Codegen::mangle("receiver"), receiver_value),
            ],
        }
    }

    /// D-TYPE2-TIME1=A: timer channels use the same Duration nanosecond
    /// carrier as every other interpreter time operation. The scheduler
    /// remains an adapter: it receives the shared millisecond boundary only
    /// after the canonical Duration has been evaluated.
    pub(super) fn new_eval_timer_channel(
        &mut self,
        duration_ns: i64,
        value: CtValue,
        repeating: bool,
    ) -> CtValue {
        let channel = crate::scheduler::JetSchedulerChannel::new();
        let sender = channel.sender();
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let index = runtime.channels.len();
        runtime.channels.push(EvalChannel {
            channel,
            sender: sender.clone(),
        });
        drop(runtime);
        let receiver = CtValue::Struct {
            type_name: "Receiver".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(index as i64))],
        };
        let duration_ms = crate::scheduler::jet_std_time_duration_to_millis(duration_ns);
        let delay = if repeating {
            crate::scheduler::jet_task_interval_ms_defaulted(duration_ms)
        } else {
            crate::scheduler::jet_task_delay_ms_defaulted(duration_ms)
        };
        crate::scheduler::jet_scheduler_spawn(move || {
            if repeating {
                let mut tick = 1i64;
                loop {
                    crate::scheduler::jet_scheduler_sleep_ms(delay);
                    if !sender.send(CtValue::Int(tick)) {
                        break;
                    }
                    tick = tick.saturating_add(1);
                }
            } else {
                crate::scheduler::jet_scheduler_sleep_ms(delay);
                let _ = sender.send(value);
            }
        });
        receiver
    }

    pub(super) fn send_eval_channel(
        &self,
        index: usize,
        value: CtValue,
    ) -> Result<(), Diagnostic> {
        self.task_wait_cancel_check()?;
        let sender = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .channels
            .get(index)
            .map(|channel| channel.sender.clone())
            .ok_or_else(|| unsupported("channel sender", self.span()))?;
        let _deadline = self
            .context_deadline
            .map(crate::scheduler::jet_ctx_push_deadline);
        self.scheduler_wait("channel send", || sender.send(value))?;
        drop(_deadline);
        self.task_wait_cancel_check()?;
        Ok(())
    }

    pub(super) fn receive_eval_channel(
        &self,
        index: usize,
    ) -> Result<CtValue, Diagnostic> {
        self.task_wait_cancel_check()?;
        let channel = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .channels
            .get(index)
            .map(|channel| channel.channel.clone())
            .ok_or_else(|| unsupported("channel receiver", self.span()))?;
        let _deadline = self
            .context_deadline
            .map(crate::scheduler::jet_ctx_push_deadline);
        let value = self.scheduler_wait("channel receive", || channel.receive())?;
        drop(_deadline);
        self.task_wait_cancel_check()?;
        Ok(match value {
            Some(value) => CtValue::Present(Box::new(value)),
            None => CtValue::failed(Box::new(CtValue::Enum {
                type_name: "Closed".to_string(),
                variant: "Closed".to_string(),
                args: Vec::new(),
            })),
        })
    }

    pub(super) fn close_eval_channel(&self, index: usize) -> Result<(), Diagnostic> {
        let channel = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .channels
            .get(index)
            .map(|channel| channel.channel.clone())
            .ok_or_else(|| unsupported("channel", self.span()))?;
        channel.close();
        Ok(())
    }

    pub(super) fn new_eval_select(&self) -> CtValue {
        Self::select_builder_value(Vec::new(), Vec::new())
    }

    pub(super) fn eval_select_recv(
        &self,
        builder: CtValue,
        receiver: usize,
    ) -> Result<CtValue, Diagnostic> {
        let (mut receivers, afters) = Self::select_builder_parts(&builder)
            .ok_or_else(|| unsupported("select builder", self.span()))?;
        let valid = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .channels
            .get(receiver)
            .is_some();
        if !valid {
            return Err(unsupported("select receiver", self.span()));
        }
        receivers.push(receiver);
        Ok(Self::select_builder_value(receivers, afters))
    }

    pub(super) fn eval_select_after(
        &self,
        builder: CtValue,
        duration_ns: i64,
        value: CtValue,
    ) -> Result<CtValue, Diagnostic> {
        let (receivers, mut afters) = Self::select_builder_parts(&builder)
            .ok_or_else(|| unsupported("select builder", self.span()))?;
        afters.push((duration_ns, value));
        Ok(Self::select_builder_value(receivers, afters))
    }

    pub(super) fn eval_select_wait(&self, builder: CtValue) -> Result<CtValue, Diagnostic> {
        let (receiver_ids, after_values) = Self::select_builder_parts(&builder)
            .ok_or_else(|| unsupported("select builder", self.span()))?;
        if receiver_ids.is_empty() && after_values.is_empty() {
            return Err(unsupported("empty select", self.span()));
        }
        let channels = {
            let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
            receiver_ids
                .iter()
                .map(|index| {
                    runtime
                        .channels
                        .get(*index)
                        .map(|channel| channel.channel.clone())
                })
                .collect::<Option<Vec<_>>>()
        }
        .ok_or_else(|| unsupported("select receiver", self.span()))?;
        let recvs = channels
            .iter()
            .map(|channel| channel.select_inner())
            .collect();
        let timers = after_values
            .into_iter()
            .map(|(duration_ns, value)| {
                (
                    crate::scheduler::jet_task_delay_ms_defaulted(
                        crate::scheduler::jet_std_time_duration_to_millis(duration_ns),
                    ),
                    Some(value),
                )
            })
            .collect();
        let _deadline = self
            .context_deadline
            .map(crate::scheduler::jet_ctx_push_deadline);
        let value = self.scheduler_wait("select wait", || {
            crate::scheduler::jet_scheduler_select_values(recvs, timers)
        })?;
        drop(_deadline);
        self.task_wait_cancel_check()?;
        Ok(value)
    }

    /// D-CONC-CHAN1: interpreter twin of the tagged Prelude select door.
    pub(super) fn eval_select_wait_tagged(
        &self,
        builder: CtValue,
    ) -> Result<CtValue, Diagnostic> {
        let (receiver_ids, after_values) = Self::select_builder_parts(&builder)
            .ok_or_else(|| unsupported("select builder", self.span()))?;
        if receiver_ids.is_empty() && after_values.is_empty() {
            return Err(unsupported("empty select", self.span()));
        }
        let channels = {
            let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
            receiver_ids
                .iter()
                .map(|index| {
                    runtime
                        .channels
                        .get(*index)
                        .map(|channel| channel.channel.clone())
                })
                .collect::<Option<Vec<_>>>()
        }
        .ok_or_else(|| unsupported("select receiver", self.span()))?;
        let recvs = channels
            .iter()
            .map(|channel| channel.select_inner())
            .collect();
        // The scheduler door takes millisecond delays; Duration stays the
        // canonical signed nanosecond carrier until this adapter boundary.
        // Use the shared Prelude conversion/default instead of rebuilding the
        // time policy in the interpreter.
        let after_ms = after_values
            .iter()
            .map(|(duration_ns, _)| {
                crate::scheduler::jet_task_delay_ms_defaulted(
                    crate::scheduler::jet_std_time_duration_to_millis(*duration_ns),
                )
            })
            .collect();
        let _deadline = self
            .context_deadline
            .map(crate::scheduler::jet_ctx_push_deadline);
        let outcome = self.scheduler_wait("select wait", || {
            crate::scheduler::jet_scheduler_select(recvs, after_ms)
        })?;
        drop(_deadline);
        self.task_wait_cancel_check()?;
        let (arm, value) = match outcome {
            crate::scheduler::JetSelectOutcome::Recv { arm, value } => {
                (arm as i64, CtValue::Present(Box::new(value)))
            }
            crate::scheduler::JetSelectOutcome::After { arm } => {
                (receiver_ids.len() as i64 + arm as i64, CtValue::absent(Type::Named("Unit".to_string())))
            }
            crate::scheduler::JetSelectOutcome::Closed => {
                return Err(unsupported("select closed", self.span()));
            }
        };
        Ok(CtValue::Struct {
            type_name: "tuple".to_string(),
            fields: vec![("arm".to_string(), CtValue::Int(arm)), ("value".to_string(), value)],
        })
    }

    /// D-VERDICT-1323-1: request cancellation for one task without consuming
    /// it, the evaluator twin of `JetTask::cancel`.
    pub(super) fn cancel_task_value(&mut self, value: &CtValue) -> Result<(), Diagnostic> {
        let index = Self::task_index(value)
            .ok_or_else(|| unsupported("task receiver", self.span()))?;
        if let Some(Some(task)) = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .tasks
            .get(index)
        {
            task.control.cancel();
        }
        Ok(())
    }

    fn take_task_entry(&mut self, value: &CtValue) -> Result<(usize, EvalTask), Diagnostic> {
        let index = Self::task_index(value)
            .ok_or_else(|| unsupported("task receiver", self.span()))?;
        let task = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .tasks
            .get_mut(index)
            .and_then(Option::take)
            .ok_or_else(|| unsupported("task already joined", self.span()))?;
        Ok((index, task))
    }

    fn restore_task_entry(&mut self, index: usize, task: EvalTask) {
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        if let Some(slot) = runtime.tasks.get_mut(index) {
            if slot.is_none() {
                *slot = Some(task);
            }
        }
    }

    pub(super) fn take_task(&mut self, value: &CtValue) -> Result<CtValue, Diagnostic> {
        if let CtValue::Struct { type_name, fields } = value {
            if type_name == "__JetTirTask" {
                if let Some(result) = fields
                    .iter()
                    .find_map(|(name, value)| (name == "value").then(|| value.clone()))
                {
                    // The already-completed carrier still answers to the one
                    // Prelude wait policy (bd15-rev): a cancelled scope or an
                    // expired deadline refuses the join here exactly as
                    // jet_task_wait_policy does on the other tiers.
                    self.task_join_wait_check()?;
                    return Ok(CtValue::Present(Box::new(result)));
                }
            }
        }
        self.task_join_wait_check()?;
        let (index, task) = self.take_task_entry(value)?;
        let _deadline = self
            .context_deadline
            .map(crate::scheduler::jet_ctx_push_deadline);
        let result = loop {
            if let Err(error) = self.task_join_wait_check() {
                self.restore_task_entry(index, task);
                return Err(error);
            }
            match task.completion.try_recv() {
                Ok(completion) => break completion.result,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.restore_task_entry(index, task);
                    return Err(unsupported("task completion", self.span()));
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if let Err(error) = self.scheduler_wait("task join", || {
                        crate::scheduler::jet_scheduler_yield(
                            "task join",
                            &task.completion_wait,
                            None,
                        )
                    }) {
                        self.restore_task_entry(index, task);
                        return Err(error);
                    }
                }
            }
        };
        drop(_deadline);
        self.task_join_wait_check()?;
        match result {
            Ok(value) => Ok(CtValue::Present(Box::new(value))),
            Err(error) => Ok(CtValue::failed(Box::new(task_failure_value(&error)))),
        }
    }

    pub(super) fn task_select(
        &mut self,
        values: &[CtValue],
        mode: crate::task_group::JetTaskSelectMode,
    ) -> Result<CtValue, Diagnostic> {
        let tasks = values
            .iter()
            .map(|value| self.take_task_entry(value).map(|(_, task)| task))
            .collect::<Result<Vec<_>, _>>()?;
        if tasks.is_empty() {
            let method_label = match mode {
                crate::task_group::JetTaskSelectMode::All => "`task.all`",
                crate::task_group::JetTaskSelectMode::Race => "`task.race`",
                crate::task_group::JetTaskSelectMode::Any => "`task.any`",
            };
            return Err(crate::Sema::Diagnostics::e1112(method_label, self.span()));
        }
        match select_eval_tasks(tasks, mode, self.span(), || self.task_wait_cancel_check()) {
            Ok(mut values) => {
                let value = if matches!(mode, crate::task_group::JetTaskSelectMode::All) {
                    CtValue::List(values)
                } else {
                    values.pop().expect("race/any result missing")
                };
                Ok(CtValue::Present(Box::new(value)))
            }
            Err(EvalTaskSelectError::Wait(error)) => Err(error),
            Err(EvalTaskSelectError::Child(error)) => {
                Ok(CtValue::failed(Box::new(task_failure_value(&error))))
            }
        }
    }

    /// Join every child, then close the loans the block opened. The loans are
    /// closed on the cancelled path too: the children are already drained, and
    /// dropping their writes would be a silent divergence from AOT (I9).
    fn close_taskgroup(
        &mut self,
        index: usize,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        let joined = self.join_taskgroup(index);
        let closed = self.close_place_loans(scope);
        joined.and(closed)
    }

    fn join_taskgroup(&mut self, index: usize) -> Result<(), Diagnostic> {
        let span = self.span();
        let group = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .task_groups
            .get(index)
            .cloned()
            .ok_or_else(|| unsupported("task group handle", span))?;
        let join_runtime = self.runtime.clone();
        let drain = move |child| {
            let task: Option<EvalTask> = join_runtime
                .lock()
                .expect("evaluator runtime poisoned")
                .tasks
                .get_mut(child)
                .and_then(|slot: &mut Option<EvalTask>| slot.take());
            if let Some(task) = task {
                let _ = task.completion.recv();
            }
        };
        if let Err(interruption) = self.task_wait_cancel_check() {
            let cancel_runtime = self.runtime.clone();
            group.close_with_cancel(
                move |child| {
                    if let Some(task) = cancel_runtime
                        .lock()
                        .expect("evaluator runtime poisoned")
                        .tasks
                        .get(*child)
                        .and_then(Option::as_ref)
                    {
                        task.control.cancel();
                    }
                },
                drain,
            );
            return Err(interruption);
        }
        group.close_with(drain);
        self.task_wait_cancel_check()
    }

    pub(crate) fn span(&self) -> Span {
        self.current_span
    }

    pub(super) fn runtime_stop(
        &mut self,
        code: &'static str,
        line: u32,
        message: &str,
    ) -> Diagnostic {
        let source_line = self
            .source_text
            .lines()
            .nth((line as usize).saturating_sub(1))
            .unwrap_or_default();
        let report = contract_semantics::jet_runtime_stop_report(
            code,
            &self.source_file,
            line,
            &self.current_fn,
            source_line,
            1,
            1,
            message,
            "",
        );
        if let Some(sink) = self.sink.as_ref() {
            let mut sink = sink.lock().expect("evaluator sink poisoned");
            sink.stderr.push_str(&report.rendered);
            sink.exit_code = Some(report.exit_code);
            crate::Sema::Diagnostics::soft_exit(
                "70".to_string(),
                format!("runtime stop {code}"),
                Some(self.span()),
            )
        } else {
            crate::Sema::Diagnostics::render_registered(
                code,
                report.what,
                report.why,
                report.fix,
                Some(self.span()),
            )
        }
    }

    pub(super) fn eval_runtime_binop(
        &mut self,
        op: crate::AST::BinOp,
        left: CtValue,
        right: CtValue,
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        self.route_runtime_arithmetic(
            crate::Comptime::Builtins::eval_binop(op, left, right, span),
            span,
        )
    }

    /// The Jet line a span falls on. A runtime report names Jet source facts,
    /// never generated-Rust ones (I2).
    pub(super) fn span_line(&self, span: Span) -> u32 {
        self.source_text
            .get(..span.start.min(self.source_text.len()))
            .map(|prefix| prefix.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1)
            .unwrap_or(1)
    }

    /// D-FAIL-TIER1 / I9: the shared evaluator mints the comptime `E0953`
    /// panic for a failed check because the same code also runs at compile
    /// time. While it is *running* a program that stop is program-side, so
    /// re-enter the one `jet_runtime_stop_report` with this tier's source
    /// facts — the same report AOT gets from `jet_panic` and the resident JIT
    /// gets from `JitRuntime::set_runtime_stop`. Without this the stop escapes
    /// to `Interpreter::runtime_trap_from_e0953`, which has no file, line, or
    /// function left to name and prints a locationless report.
    pub(super) fn route_runtime_panic<T>(
        &mut self,
        result: Result<T, Diagnostic>,
        code: &'static str,
        line: u32,
    ) -> Result<T, Diagnostic> {
        match result {
            Err(diagnostic) if self.runtime_execution && diagnostic.code == "E0953" => {
                let message = diagnostic
                    .why
                    .strip_prefix(
                        "while computing this value at compile time, the program panicked: ",
                    )
                    .unwrap_or(diagnostic.what.as_str())
                    .to_string();
                Err(self.runtime_stop(code, line, &message))
            }
            result => result,
        }
    }

    pub(super) fn route_runtime_arithmetic(
        &mut self,
        result: Result<CtValue, Diagnostic>,
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        let line = self.span_line(span);
        self.route_runtime_panic(result, "E3010", line)
    }

    pub(super) fn eval_fixed_width_division(
        &mut self,
        left: i64,
        right: i64,
        signed: bool,
        bits: u8,
        right_signed: bool,
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        let left = crate::Comptime::MathLayout::integer_widen(left, signed);
        let right = crate::Comptime::MathLayout::integer_widen(right, right_signed);
        let (minimum, maximum) = crate::AST::int_range(signed, bits);
        match division_semantics::jet_division(left, right, minimum, maximum) {
            Ok(value) => Ok(CtValue::Int(crate::Comptime::MathLayout::integer_narrow(
                value, signed, bits,
            ))),
            Err(message) => {
                let line = self.span_line(span);
                Err(self.runtime_stop("E3010", line, message))
            }
        }
    }

    pub(super) fn runtime_index_stop(
        &mut self,
        code: &'static str,
        line: u32,
        message: &str,
    ) -> Diagnostic {
        if self.runtime_execution {
            self.runtime_stop(code, line, message)
        } else {
            unsupported(message, self.span())
        }
    }

    fn check_contracts(
        &mut self,
        contracts: &'a [TIR::TContract],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        for contract in contracts {
            if contract.disposition != TIR::TContractDisposition::Check {
                continue;
            }
            let previous_span = self.current_span;
            self.current_span = contract.span;
            let condition = match self.eval_expr(&contract.condition, scope) {
                Ok(CtValue::Bool(value)) => value,
                Ok(_) => {
                    self.current_span = previous_span;
                    return Err(unsupported("contract condition", contract.span));
                }
                Err(error) => {
                    self.current_span = previous_span;
                    return Err(error);
                }
            };
            if contract_semantics::jet_contract_check(condition) {
                self.current_span = previous_span;
                continue;
            }
            let message = match self.eval_expr(&contract.message, scope) {
                Ok(value) => value.jet_show(),
                Err(error) => {
                    self.current_span = previous_span;
                    return Err(error);
                }
            };
            self.current_span = previous_span;
            let keyword = match contract.kind {
                TIR::TContractKind::Pre => "Pre",
                TIR::TContractKind::Post => "Post",
            };
            let report = contract_semantics::jet_contract_report(
                keyword,
                &message,
                &contract.file,
                contract.line,
            );
            if let Some(sink) = self.sink.as_ref() {
                let mut sink = sink.lock().expect("evaluator sink poisoned");
                sink.stderr.push_str(&report.rendered);
                sink.exit_code = Some(report.exit_code);
                return Err(crate::Sema::Diagnostics::soft_exit(
                    report.exit_code.to_string(),
                    report.what,
                    Some(contract.span),
                ));
            }
            return Err(crate::Sema::Diagnostics::render_registered(
                "E3005",
                report.what,
                report.why,
                report.fix,
                Some(contract.span),
            ));
        }
        Ok(())
    }

    pub(super) fn burn(&mut self) -> Result<(), Diagnostic> {
        if self.fuel == 0 {
            // Dev/REPL fragments use E2202; pure comptime uses E0952.
            let (code, what, why, fix) = if self.sink.is_some() || self.repl_mode {
                (
                    "E2202",
                    "this program ran too many steps without finishing".to_string(),
                    format!(
                        "`jet dev` interprets your program to give instant feedback, but it ran out of steps without finishing — this usually means a loop that never ends"
                    ),
                    "check the loop near here for a condition that never becomes false; run `jet run` to execute the real build with no step limit"
                        .to_string(),
                )
            } else {
                (
                    "E0952",
                    "compile-time evaluation ran out of steps".to_string(),
                    "a loop or recursion in this expression doesn't finish".to_string(),
                    "simplify the expression, or move the work to runtime".to_string(),
                )
            };
            return Err(crate::Sema::Diagnostics::render_registered(
                code,
                what,
                why,
                fix,
                Some(self.span()),
            ));
        }
        self.fuel -= 1;
        Ok(())
    }

    pub(super) fn enter_source_nesting(&mut self) -> Result<(), Diagnostic> {
        let span = self.span();
        enter_source_nesting(&mut self.source_nesting, span)
    }

    pub(super) fn leave_source_nesting(&mut self) {
        self.source_nesting -= 1;
    }

    pub(crate) fn run_func(
        &mut self,
        func: &'a TFunc,
        args: Vec<CtValue>,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        // D-MEMO1=A: the interpreter adapter marshals the argument tuple to the
        // same Prelude store used by emitted Rust. Sema has already proved the
        // tuple safe to cache; this path never rechecks purity or hashability.
        if let Some(bound) = func.memo_bound {
            let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
            let mut memos = runtime.memos.lock().expect("memo state poisoned");
            let memo = memos
                .entry(func.name.clone())
                .or_insert_with(|| crate::memo::JetMemo::with_bound(bound));
            if let Some(value) = memo.get(&args) {
                return Ok(value);
            }
        }
        if self.call_depth >= jet_foundation::Outcome::JET_RUNTIME_STACK_LIMIT {
            if self.runtime_execution {
                return Err(self.runtime_stop(
                    "E3012",
                    func.line as u32,
                    &jet_foundation::Outcome::jet_stack_overflow_message(&func.name),
                ));
            }
            self.fuel = 0;
            self.burn()?;
            unreachable!("burn with fuel 0 always errors");
        }
        self.call_depth += 1;
        let previous_source_nesting = std::mem::replace(&mut self.source_nesting, 0);
        let previous_span = std::mem::replace(&mut self.current_span, func.source_span);
        let previous_fn = std::mem::replace(&mut self.current_fn, func.name.clone());
        let guard_mark = self.scope_guards.len();
        self.local_cells.enter_frame();
        let _sentry = func.unsafe_gate.as_ref().map(|gate| {
            if gate.fenced {
                jet_foundation::MemSentry::jet_sentry_fenced_scope(
                    gate.enabled,
                    &gate.file,
                    gate.line,
                    "",
                )
            } else {
                jet_foundation::MemSentry::jet_sentry_scope(
                    gate.enabled,
                    &gate.file,
                    gate.line,
                    "",
                )
            }
        });
        for (i, (name, _, _)) in func.params.iter().enumerate() {
            let jet = name.strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX).unwrap_or(name.as_str());
            let value = args.get(i).cloned().unwrap_or(CtValue::Unit);
            scope.insert(jet.to_string(), value.clone());
            if jet != name {
                scope.insert(name.clone(), value);
            }
        }
        let result = match self.exec_stmts(&func.body, scope) {
                Ok(Flow::Return(v)) => Ok(v),
                Ok(Flow::Normal) => Ok(CtValue::Unit),
                Ok(other) => Err(unsupported(
                        &format!("control flow {other:?} escaping function"),
                        self.span(),
                    )),
                Err(error) => Err(error),
        };
        // Run scope.guard cleanups LIFO, matching Drop order in AOT/JIT.
        let guards: Vec<_> = self.scope_guards.drain(guard_mark..).rev().collect();
        let mut cleanup_result = Ok(());
        for lam in guards {
            if let Err(error) = self.eval_tlambda(lam, Vec::new(), scope) {
                cleanup_result = Err(error);
                break;
            }
        }
        let result = result.map(|mut value| {
            if let (
                Some(Type::Named(expected)),
                CtValue::Enum { type_name, .. } | CtValue::Struct { type_name, .. },
            ) = (&func.ret, &mut value)
            {
                let leaf = crate::Codegen::nominal_leaf(type_name);
                if leaf == crate::Codegen::nominal_leaf(expected) {
                    // A Core type is DECLARED by its leaf. `use core.encoding as
                    // encoding` spells the annotation `encoding.EncodingError`,
                    // but every Prelude table, show selector and nominal equality
                    // is keyed on that declared leaf, so adopting the alias
                    // spelling here leaves the returned value unequal to the
                    // identical literal and unreachable for the shared Core
                    // display. Resolve onto the leaf, the same direction the JIT
                    // resolves through `jit::types_meta::core_alias_leaf`. A user
                    // record owns its spelling and keeps the annotated one, which
                    // is what its own method keys use.
                    let core_leaf = expected.contains('.')
                        && !self.struct_fields.contains_key(leaf)
                        && crate::Codegen::core_rust_type_name(leaf).is_some();
                    let resolved = if core_leaf {
                        leaf.to_string()
                    } else {
                        expected.clone()
                    };
                    *type_name = resolved;
                }
            }
            value
        });
        let returned = result.as_ref().ok().cloned().unwrap_or(CtValue::Unit);
        self.local_cells.leave_frame(&returned);
        self.call_depth -= 1;
        self.source_nesting = previous_source_nesting;
        self.current_span = previous_span;
        self.current_fn = previous_fn;
        let final_result = match (result, cleanup_result) {
            (Err(error), _) | (Ok(_), Err(error)) => {
                Err(error)
            }
            (Ok(value), Ok(())) => Ok(value),
        };
        if let Some(bound) = func.memo_bound {
            if let Ok(value) = &final_result {
                let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
                let mut memos = runtime.memos.lock().expect("memo state poisoned");
                memos
                    .entry(func.name.clone())
                    .or_insert_with(|| crate::memo::JetMemo::with_bound(bound))
                    .put(args, value.clone());
            }
        }
        final_result
    }

    /// D-MEMO1=A: stats are a projection of the function's one shared store;
    /// an untouched function gets a zeroed store with its ratified bound.
    fn memo_stats(&self, name: &str) -> Result<CtValue, Diagnostic> {
        let Some(func) = self.funcs.get(name) else {
            return Err(unsupported("memoized function", self.span()));
        };
        let Some(bound) = func.memo_bound else {
            return Err(unsupported("memoized function statistics", self.span()));
        };
        let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let mut memos = runtime.memos.lock().expect("memo state poisoned");
        let stats = memos
            .entry(name.to_string())
            .or_insert_with(|| crate::memo::JetMemo::with_bound(bound))
            .stats();
        Ok(CtValue::Struct {
            type_name: crate::Syntax::TYPE_MEMO_STATS.to_string(),
            fields: vec![
                ("hits".to_string(), CtValue::Int(stats.hits)),
                ("misses".to_string(), CtValue::Int(stats.misses)),
                ("size".to_string(), CtValue::Int(stats.size)),
                ("bound".to_string(), CtValue::Str(stats.bound)),
            ],
        })
    }

    fn store_callable(&mut self, callable: EvalCallable<'a>) -> CtValue {
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let index = runtime.callables.len() as i64;
        runtime.callables.push(callable);
        CtValue::Struct {
            type_name: "__JetTirCallable".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(index))],
        }
    }

    fn callable_value(index: usize) -> CtValue {
        CtValue::Struct {
            type_name: "__JetTirCallable".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(index as i64))],
        }
    }

    fn callable_index(value: &CtValue) -> Option<usize> {
        let CtValue::Struct { type_name, fields } = value else {
            return None;
        };
        if type_name != "__JetTirCallable" {
            return None;
        }
        fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
            ("index", CtValue::Int(index)) => usize::try_from(*index).ok(),
            _ => None,
        })
    }

    pub(super) fn register_interrupt_callback(
        &mut self,
        callback: &'a TExpr,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        interrupt_runtime::jet_interrupt_arm().map_err(|message| {
            unsupported(&interrupt_runtime::jet_interrupt_core_error(&message), self.span())
        })?;
        let value = self.eval_expr(callback, scope)?;
        let index = Self::callable_index(&value)
            .ok_or_else(|| {
                unsupported(
                    interrupt_runtime::jet_interrupt_invalid_callback_value_error(),
                    self.span(),
                )
            })?;
        self.runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .interrupt_handlers
            .push(index);
        Ok(CtValue::Unit)
    }

    /// Register one process-edge callback. The callback stays in the shared
    /// callable arena until the whole-program boundary drains it.
    pub(super) fn register_atexit_callback(
        &mut self,
        callback: &'a TExpr,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let value = self.eval_expr(callback, scope)?;
        let index = Self::callable_index(&value)
            .ok_or_else(|| unsupported("invalid atexit callback value", self.span()))?;
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        jet_foundation::Outcome::jet_runtime_register_atexit(
            &mut runtime.atexit_handlers,
            index,
        );
        Ok(CtValue::Unit)
    }

    /// Drain callbacks at the one evaluator process boundary. Lexical
    /// cleanup has already run when this method is called.
    pub(super) fn run_atexit_handlers(&mut self) -> Result<(), Diagnostic> {
        let mut indexes = Vec::new();
        {
            let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
            jet_foundation::Outcome::jet_runtime_drain_atexit(
                &mut runtime.atexit_handlers,
                |index| indexes.push(index),
            );
        }
        for index in indexes {
            self.call_callable(&Self::callable_value(index), Vec::new())?;
        }
        Ok(())
    }

    pub(super) fn dispatch_pending_interrupts(
        &mut self,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        let handlers = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .interrupt_handlers
            .clone();
        let mut deferred_panic = None;
        let mut failure = None;
        interrupt_runtime::jet_interrupt_dispatch(&handlers, |index| {
            if failure.is_some() {
                return;
            }
            let value = Self::callable_value(*index);
            match self.call_callable(&value, Vec::new()) {
                Ok(_) => {}
                Err(error) if error.code == "SOFT_EXIT" => {
                    let panic_stop = self.sink.as_ref().is_some_and(|sink| {
                        sink.lock()
                            .expect("evaluator sink poisoned")
                            .exit_code
                            == Some(70)
                    });
                    if panic_stop {
                        deferred_panic.get_or_insert(error);
                    } else {
                        failure = Some(error);
                    }
                }
                Err(error) => failure = Some(error),
            }
        });
        if let Some(error) = failure {
            return Err(error);
        }
        if let Some(error) = deferred_panic {
            return Err(error);
        }
        let _ = scope;
        Ok(())
    }

    fn store_stream(&mut self, func: &'a TFunc, args: Vec<CtValue>) -> CtValue {
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let index = runtime.streams.len() as i64;
        runtime.streams.push(EvalStream {
            func,
            args,
            control: crate::scheduler::JetTaskControl::new(),
        });
        CtValue::Struct {
            type_name: "__JetTirStream".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(index))],
        }
    }

    fn stream_index(value: &CtValue) -> Option<usize> {
        let CtValue::Struct { type_name, fields } = value else {
            return None;
        };
        if type_name != "__JetTirStream" {
            return None;
        }
        fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
            ("index", CtValue::Int(index)) => usize::try_from(*index).ok(),
            _ => None,
        })
    }

    pub(super) fn call_callable(
        &mut self,
        value: &CtValue,
        args: Vec<CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let index = Self::callable_index(value)
            .ok_or_else(|| unsupported("calling this non-function value", self.span()))?;
        let target = {
            let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
            match runtime.callables.get(index) {
                Some(EvalCallable::Lambda { lambda, captured }) => {
                    EvalCallableSnapshot::Lambda {
                        lambda: *lambda,
                        captured: captured.clone(),
                    }
                }
                Some(EvalCallable::Named(name)) => EvalCallableSnapshot::Named(*name),
                Some(EvalCallable::ComputeTransform {
                    base,
                    method,
                    targets,
                    result_ty,
                }) => EvalCallableSnapshot::ComputeTransform {
                    base: base.clone(),
                    method: method.clone(),
                    targets: targets.clone(),
                    result_ty: result_ty.clone(),
                },
                Some(EvalCallable::ComputePull {
                    output,
                    anchor,
                    targets,
                    gradient_ty,
                }) => EvalCallableSnapshot::ComputePull {
                    output: output.clone(),
                    anchor: anchor.clone(),
                    targets: targets.clone(),
                    gradient_ty: gradient_ty.clone(),
                },
                Some(EvalCallable::ComputeGrads {
                    output,
                    anchor,
                    targets,
                    gradient_ty,
                }) => EvalCallableSnapshot::ComputeGrads {
                    output: output.clone(),
                    anchor: anchor.clone(),
                    targets: targets.clone(),
                    gradient_ty: gradient_ty.clone(),
                },
                None => return Err(unsupported("calling an unknown function value", self.span())),
            }
        };
        match target {
            EvalCallableSnapshot::Lambda {
                lambda,
                mut captured,
            } => {
                let result = self.eval_tlambda(lambda, args, &mut captured);
                if result.is_ok() {
                    if let Some(EvalCallable::Lambda {
                        captured: stored, ..
                    }) = self
                        .runtime
                        .lock()
                        .expect("evaluator runtime poisoned")
                        .callables
                        .get_mut(index)
                    {
                        *stored = captured;
                    }
                }
                result
            }
            EvalCallableSnapshot::Named(name) => {
                let func = self
                    .funcs
                    .get(name)
                    .copied()
                    .ok_or_else(|| unsupported(&format!("callable function `{name}`"), self.span()))?;
                let mut child = HashMap::new();
                self.run_func(func, args, &mut child)
            }
            EvalCallableSnapshot::ComputeTransform {
                base,
                method,
                targets,
                result_ty,
            } => self.eval_compute_transform(&method, base, args, targets, &result_ty),
            EvalCallableSnapshot::ComputePull {
                output,
                anchor,
                targets,
                gradient_ty,
            } => self.eval_compute_pull(output, anchor, args, targets, &gradient_ty),
            EvalCallableSnapshot::ComputeGrads {
                output,
                anchor,
                targets,
                gradient_ty,
            } => self.eval_compute_grads(output, anchor, args, targets, &gradient_ty),
        }
    }

    /// Call the canonical callable slot and publish mutable captures back to
    /// the active lexical scope. The runtime slot remains authoritative for
    /// returned/escaped closures; the scope write-back is the local-variable
    /// half of the same FnMut storage contract.
    pub(super) fn call_callable_in_scope(
        &mut self,
        value: &CtValue,
        args: Vec<CtValue>,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let result = self.call_callable(value, args);
        self.sync_callable_captures(value, scope);
        result
    }

    pub(super) fn sync_callable_captures(
        &self,
        value: &CtValue,
        scope: &mut HashMap<String, CtValue>,
    ) {
        let Some(index) = Self::callable_index(value) else {
            return;
        };
        let updates = {
            let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
            let Some(EvalCallable::Lambda { lambda, captured }) = runtime.callables.get(index)
            else {
                return;
            };
            lambda
                .captures
                .iter()
                .filter_map(|(source, runtime_name, _)| {
                    let updated = captured
                        .get(runtime_name)
                        .or_else(|| captured.get(source))?
                        .clone();
                    Some((source.clone(), updated))
                })
                .collect::<Vec<_>>()
        };
        for (source, updated) in updates {
            if scope.contains_key(&source) {
                scope.insert(source, updated);
            }
        }
    }

    /// Publish one *lent* local back out of a callable frame.
    ///
    /// A lending callback (`edit_disjoint`) never captures the owner it writes
    /// through: it holds a `__JetViewMut` that names the owner local, and the
    /// callable frame is a clone of the caller's scope, so the body's writes
    /// land on the frame's copy. `sync_callable_captures` walks the *lexical*
    /// capture list and cannot see that name, so the loan has to be published
    /// by name — otherwise the interpreter silently drops writes that AOT's
    /// `jet_edit_disjoint(&mut xs, …)` makes directly on the owner (I9).
    pub(super) fn sync_callable_lent_owner(
        &self,
        value: &CtValue,
        owner: &str,
        scope: &mut HashMap<String, CtValue>,
    ) {
        let Some(index) = Self::callable_index(value) else {
            return;
        };
        let updated = {
            let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
            let Some(EvalCallable::Lambda { captured, .. }) = runtime.callables.get(index) else {
                return;
            };
            captured.get(owner).cloned()
        };
        if let Some(updated) = updated {
            if scope.contains_key(owner) {
                scope.insert(owner.to_string(), updated);
            }
        }
    }
}

/// Do a capture's two spellings name ONE storage slot, or two?
///
/// `lower/lambdas.rs` builds two different capture packs and the difference is
/// load-bearing here. The clone pack (`reactive_capture_name`) REBINDS the body
/// to a fresh `__jet___cap_*` slot, so the body's own `TLocal::name` is that
/// place and the pair really is two slots. The lexical pack leaves the body on
/// the OUTER slot and records `env.rust_name_of(name)` — for a user local that
/// is `local_place(source)` (`__jet_x`), a spelling no engine keys a value by,
/// because `TLocal::user(name).name` is the bare `name`.
///
/// Treating the second kind as two slots invents a phantom the body never
/// writes, and copying that phantom back over the body's real write loses the
/// write with no diagnostic — `xs.each(n => { seen.push(n) })` then leaves
/// `seen` empty while AOT, which gets it from Rust's own `FnMut` borrow, fills
/// it. An already-generated outer slot reports its own name, so it lands on the
/// `source == place` side and needs no aliasing either.
pub(super) fn capture_is_one_slot(source: &str, place: &str) -> bool {
    place == source || place == TIR::local_place(source)
}

fn empty_cx() -> Cx {
    build_cx_items(&[], "", "<eval>", None, &HashMap::new())
}

fn seed_fragment_distinct_types(
    cx: &mut Cx,
    ranges: &HashMap<String, Option<(i64, i64)>>,
    bases: &HashMap<String, crate::AST::Type>,
) {
    for (name, base) in bases {
        cx.distinct_types
            .insert(name.clone(), (base.clone(), base.is_numeric()));
    }
    for (name, range) in ranges {
        if let Some(bounds) = range {
            cx.distinct_ranges.insert(name.clone(), *bounds);
        }
    }
}

fn seed_fragment_unit_families(cx: &mut Cx, families: &[UnitFamilyDef]) {
    if families.is_empty() {
        return;
    }
    // Reuse the bundle context registry. Fragment evaluation must not grow a
    // second unit-fact implementation just because it has no ProgramBundle.
    let items = families
        .iter()
        .cloned()
        .map(Item::UnitFamily)
        .collect::<Vec<_>>();
    let units = build_cx_items(&items, "", "<eval>", None, &HashMap::new());
    cx.type_names.extend(units.type_names);
    cx.local_type_names.extend(units.local_type_names);
    cx.distinct_types.extend(units.distinct_types);
    cx.distinct_ranges.extend(units.distinct_ranges);
    cx.unit_facts.extend(units.unit_facts);
    cx.unit_labels.extend(units.unit_labels);
}

fn seed_fragment_funcs(cx: &mut Cx, funcs: &HashMap<String, &Func>) {
    for (name, function) in funcs {
        cx.fn_param_names.insert(
            name.clone(),
            function.params.iter().map(|parameter| parameter.name.clone()).collect(),
        );
        cx.fn_type_params.insert(
            name.clone(),
            function
                .type_params
                .iter()
                .map(|parameter| parameter.name.clone())
                .collect(),
        );
        cx.fn_type_param_order.insert(
            name.clone(),
            function
                .type_params
                .iter()
                .map(|parameter| parameter.name.clone())
                .collect(),
        );
        cx.sigs.insert(
            name.clone(),
            function
                .params
                .iter()
                .map(|parameter| {
                    let ty = if parameter.variadic {
                        Type::List(Box::new(parameter.ty.clone()))
                    } else {
                        parameter.ty.clone()
                    };
                    (parameter.convention, ty)
                })
                .collect(),
        );
        cx.fn_types.insert(
            name.clone(),
            Type::Fn {
                params: function
                    .params
                    .iter()
                    .map(|parameter| {
                        if parameter.variadic {
                            Type::List(Box::new(parameter.ty.clone()))
                        } else {
                            parameter.ty.clone()
                        }
                    })
                    .collect(),
                ret: function.return_type.clone().map(Box::new),
                effect_bound: None, return_view_provenance: None,
                param_contract: (!function.params.is_empty()).then(|| {
                    function
                        .params
                        .iter()
                        .map(|parameter| (parameter.call_label().to_string(), parameter.zone))
                        .collect()
                }),
                call_metadata: Some(crate::AST::FunctionCallMetadata {
                    names: function.params.iter().map(|parameter| parameter.name.clone()).collect(),
                    defaults: function
                        .params
                        .iter()
                        .map(|parameter| parameter.default.as_deref().cloned())
                        .collect(),
                    variadic: function.params.iter().map(|parameter| parameter.variadic).collect(),
                    conventions: function
                        .params
                        .iter()
                        .map(|parameter| parameter.convention)
                        .collect(),
                    policies: crate::AST::CallablePolicyChain::default(),
                }),
            },
        );
    }
}

/// Fragment evaluation normally receives the free-function table from the
/// comptime driver and a separate semantic method table. Build the same
/// lookup surface that whole-program lowering has: methods are keyed by
/// Owner::method, while free functions retain their source keys.
fn merge_fragment_funcs<'a>(
    funcs: &'a HashMap<String, &'a Func>,
    methods: &'a HashMap<(String, String), &'a Func>,
) -> HashMap<String, &'a Func> {
    let mut merged = funcs.clone();
    for ((owner, name), function) in methods {
        merged
            .entry(format!("{owner}::{name}"))
            .or_insert(*function);
    }
    merged
}

fn seed_fragment_structs(
    cx: &mut Cx,
    structs: &HashMap<String, &crate::AST::StructDef>,
    methods: &HashMap<(String, String), &Func>,
    computed_fields: &HashMap<(String, String), &Expr>,
) {
    for (name, definition) in structs {
        cx.type_names.insert(name.clone());
        cx.struct_fields.insert(
            name.clone(),
            definition
                .reflection_fields()
                .map(|field| (field.name.clone(), field.ty.clone()))
                .collect(),
        );
        if !definition.type_params.is_empty() {
            let params = definition
                .type_params
                .iter()
                .map(|param| param.name.clone())
                .collect::<Vec<_>>();
            cx.struct_type_params
                .insert(name.clone(), params.iter().cloned().collect());
            cx.struct_type_param_order.insert(name.clone(), params);
        }
        let computed = definition
            .fields
            .iter()
            .filter(|field| field.computed.is_some())
            .map(|field| field.name.clone())
            .collect::<std::collections::HashSet<_>>();
        if !computed.is_empty() {
            cx.computed_fields.insert(name.clone(), computed);
        }
        let (memo_fields, memo_dependencies) =
            crate::Codegen::Context::memo_facts_for_struct(definition);
        if !memo_fields.is_empty() {
            cx.memo_fields.insert(name.clone(), memo_fields);
        }
        if !memo_dependencies.is_empty() {
            cx.memo_dependencies.insert(name.clone(), memo_dependencies);
        }
    }
    // Keep fragment lowering on the same recursive-layout fact as AOT/JIT.
    // A fragment Cx is intentionally assembled without the full item walk, so
    // it must ask the shared context helper for the boxed edges after all
    // fragment structs and fields are registered.
    for definition in structs.values() {
        cx.boxed_edges
            .extend(crate::Codegen::find_struct_box_edges(definition, cx));
    }
    // Preserve sema's qualified and short keys even if a fragment's StructDef
    // table is sparse.
    for ((owner, field), _) in computed_fields {
        cx.computed_fields
            .entry(owner.clone())
            .or_default()
            .insert(field.clone());
    }
    for ((owner, name), method) in methods {
        let key = (owner.clone(), name.clone());
        if let Some(self_param) = method
            .params
            .iter()
            .find(|param| param.name == crate::Syntax::KW_SELF)
        {
            cx.method_self_convs.insert(key.clone(), self_param.convention);
        }
        cx.method_sigs.insert(
            key.clone(),
            method
                .params
                .iter()
                .filter(|param| param.name != crate::Syntax::KW_SELF)
                .map(|param| {
                    let ty = if param.variadic {
                        Type::List(Box::new(param.ty.clone()))
                    } else {
                        param.ty.clone()
                    };
                    (param.convention, ty)
                })
                .collect(),
        );
        cx.method_rets.insert(key, method.return_type.clone());
    }
}

/// Lower one expression for the evaluator (comptime / REPL fragments).
pub fn lower_expr_for_eval(
    expr: &Expr,
    funcs: &HashMap<String, &Func>,
    methods: &HashMap<(String, String), &Func>,
    structs: &HashMap<String, &crate::AST::StructDef>,
    computed_fields: &HashMap<(String, String), &Expr>,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    distinct_ranges: &HashMap<String, Option<(i64, i64)>>,
    distinct_bases: &HashMap<String, crate::AST::Type>,
    unit_families: &[UnitFamilyDef],
) -> Result<(TExpr, Vec<TJitSpawnLambda>), Diagnostic> {
    let mut diagnostic = None;
    let mut foreign_struct_span = None;
    crate::Comptime::walk_expr_nodes_for_validation(expr, &mut |node| {
        if diagnostic.is_none() {
            diagnostic = crate::Sema::Diagnostics::validate_typed_boundary_before_lowering(node);
        }
        if foreign_struct_span.is_none()
            && matches!(
                node,
                Expr::StructLit {
                    import_ns: Some(_),
                    ..
                }
            )
        {
            foreign_struct_span = Some(node.span());
        }
    });
    if let Some(diagnostic) = diagnostic {
        return Err(diagnostic);
    }
    // Fragment evaluation has only the current module's struct registry. An
    // imported struct literal needs the bundle name ledger to resolve its
    // canonical owner, which this intentionally lightweight evaluator does
    // not carry. Decline the optional fold and leave the checked runtime
    // expression intact; attempting TIR lowering here would turn a normal
    // fold miss into an I3 ICE.
    if let Some(span) = foreign_struct_span {
        return Err(unsupported("an imported struct literal", span));
    }
    let mut cx = empty_cx();
    seed_fragment_structs(&mut cx, structs, methods, computed_fields);
    seed_fragment_distinct_types(&mut cx, distinct_ranges, distinct_bases);
    seed_fragment_unit_families(&mut cx, unit_families);
    seed_fragment_funcs(&mut cx, funcs);
    cx.const_values = globals.clone();
    for (name, value) in globals {
        cx.consts.insert(name.clone(), String::new());
        let _ = value;
    }
    cx.core_imports = core_imports.clone();
    let mut env = LowerEnv::new("__ct".into());
    for name in globals.keys() {
        env.bind(name, TLocal::user(name), None);
    }
    // Fragment eval: sema facts may still be incomplete (e.g. IndexKind::Unknown).
    // Try lower under catch_unwind; refuse with E0956 only when lower can't.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::Codegen::TIR::with_eval_fragment(|| TIR::lower_expr(expr, &cx, &mut env))
    })) {
        Ok(tir) => Ok((
            tir,
            std::mem::take(&mut *cx.jit_spawn_lambdas.borrow_mut()),
        )),
        Err(_) => Err(unsupported("this expression", Span::new(0, 0))),
    }
}

/// Lower a statement list for the evaluator.
pub fn lower_stmts_for_eval(
    stmts: &[Stmt],
    funcs: &HashMap<String, &Func>,
    methods: &HashMap<(String, String), &Func>,
    structs: &HashMap<String, &crate::AST::StructDef>,
    computed_fields: &HashMap<(String, String), &Expr>,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    distinct_ranges: &HashMap<String, Option<(i64, i64)>>,
    distinct_bases: &HashMap<String, crate::AST::Type>,
    unit_families: &[UnitFamilyDef],
) -> Result<(Vec<TStmt>, Vec<TJitSpawnLambda>), Diagnostic> {
    let mut diagnostic = None;
    crate::Comptime::walk_stmt_expr_nodes_for_validation(stmts, &mut |expr| {
        if diagnostic.is_none() {
            diagnostic = crate::Sema::Diagnostics::validate_typed_boundary_before_lowering(expr);
        }
    });
    if let Some(diagnostic) = diagnostic {
        return Err(diagnostic);
    }
    let mut cx = empty_cx();
    seed_fragment_structs(&mut cx, structs, methods, computed_fields);
    seed_fragment_distinct_types(&mut cx, distinct_ranges, distinct_bases);
    seed_fragment_unit_families(&mut cx, unit_families);
    seed_fragment_funcs(&mut cx, funcs);
    cx.const_values = globals.clone();
    for name in globals.keys() {
        cx.consts.insert(name.clone(), String::new());
    }
    cx.core_imports = core_imports.clone();
    let mut env = LowerEnv::new("__ct_block".into());
    for name in globals.keys() {
        env.bind(name, TLocal::user(name), None);
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::Codegen::TIR::with_eval_fragment(|| TIR::lower_stmts(stmts, &cx, &mut env))
    })) {
        Ok(tir) => Ok((
            tir,
            std::mem::take(&mut *cx.jit_spawn_lambdas.borrow_mut()),
        )),
        Err(_) => Err(unsupported("this statement", Span::new(0, 0))),
    }
}

pub fn lower_interp_program(bundle: &ProgramBundle) -> Option<JitProgram> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| TIR::lower_jit_program(bundle)))
    {
        Ok(program) => program,
        Err(_) => None,
    }
}

fn program_funcs(program: &JitProgram) -> HashMap<String, &TFunc> {
    program.funcs.iter().map(|f| (f.name.clone(), f)).collect()
}

fn serve_entry_value(ctx: &mut EvalCtx<'_>, value: CtValue) -> Result<CtValue, Diagnostic> {
    match value {
        CtValue::Failed(report) => Ok(CtValue::Failed(report)),
        CtValue::Present(app) => ctx.eval_app_method(&app, "serve", Vec::new()),
        app @ CtValue::Struct { .. } => ctx.eval_app_method(&app, "serve", Vec::new()),
        _ => Err(unsupported("App entry value", ctx.span())),
    }
}

fn struct_metadata_keys(
    bundle: &ProgramBundle,
    owner_idx: usize,
    type_name: &str,
) -> Vec<String> {
    let keys = if owner_idx == bundle.entry {
        vec![type_name.to_string()]
    } else {
        crate::Codegen::TIR::imported_type_owners(bundle, owner_idx)
            .into_iter()
            .map(|owner| crate::Codegen::TIR::imported_type_name(&owner, type_name))
            .collect()
    };
    keys
}

fn collect_struct_fields(bundle: &ProgramBundle) -> HashMap<String, Vec<(String, bool)>> {
    let mut out = HashMap::new();
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        for item in &module.items {
            if let crate::AST::Item::Struct(s) = item {
                let reflection_fields = jet_foundation::Reflection::fields(s);
                let fields = reflection_fields
                    .into_iter()
                    .map(|field| {
                        let redact = s
                            .fields
                            .iter()
                            .find(|candidate| candidate.name == field.name)
                            .is_some_and(|candidate| candidate.redact);
                        (field.name, redact)
                    })
                    .collect::<Vec<(String, bool)>>();
                for key in struct_metadata_keys(bundle, module_idx, &s.name) {
                    out.insert(key, fields.clone());
                }
            }
        }
    }
    out
}

fn collect_struct_field_types(
    bundle: &ProgramBundle,
) -> HashMap<String, Vec<(String, crate::AST::Type)>> {
    let mut out = HashMap::new();
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        for item in &module.items {
            if let crate::AST::Item::Struct(s) = item {
                let fields = s
                    .reflection_fields()
                    .map(|f| (f.name.clone(), f.ty.clone()))
                    .collect::<Vec<(String, crate::AST::Type)>>();
                for key in struct_metadata_keys(bundle, module_idx, &s.name) {
                    out.insert(key, fields.clone());
                }
            }
        }
    }
    insert_core_struct_field_types(&mut out);
    out
}

/// CORE ("Prelude") records the interpreter must know the shape of: structural
/// clone and Debug show read a field's declared type from here.
///
/// I9 / card 2021: the interpreter states the declaration ORDER of these rows
/// and nothing else. The field TYPES come from sema's one table, the same one
/// AOT lowering and the JIT read, because three private copies of a Core
/// record's shape is exactly how `ProcessResult.output` came to be a `String`
/// in sema and an `Int` in the emitter.
fn insert_core_struct_field_types(
    fields: &mut HashMap<String, Vec<(String, crate::AST::Type)>>,
) {
    const CORE_ROWS: &[(&str, &[&str])] = &[
        (
            crate::Syntax::TYPE_MEMO_STATS,
            &["hits", "misses", "size", "bound"],
        ),
        (
            crate::Syntax::TYPE_IO_CONTEXT,
            &["operation", "resource", "os_code", "cause"],
        ),
        ("TestSuite", &["iteration", "result"]),
        (
            "TLSCertificate",
            &[
                "der",
                "sha256",
                "spki_sha256",
                "dns_names",
                "valid_from_unix_ms",
                "valid_until_unix_ms",
                "subject",
                "issuer",
            ],
        ),
        (
            "TLSPeerIdentity",
            &[
                "verified_server_name",
                "leaf",
                "certificate_chain",
                "cipher_suite",
                "tls_version",
            ],
        ),
    ];
    for (type_name, names) in CORE_ROWS {
        // A row is inserted whole or not at all. A half-filled row would hand
        // structural clone and Debug show a record whose field list no longer
        // matches its declaration order, which prints the wrong field.
        let Some(row) = names
            .iter()
            .map(|field| {
                crate::Sema::core_struct_field_type(type_name, field, &[])
                    .map(|ty| ((*field).to_string(), ty))
            })
            .collect::<Option<Vec<(String, crate::AST::Type)>>>()
        else {
            continue;
        };
        fields.insert((*type_name).to_string(), row);
    }
}

fn normalize_struct_field_types(
    structs: &HashMap<String, &crate::AST::StructDef>,
) -> HashMap<String, Vec<(String, crate::AST::Type)>> {
    let mut out = structs
        .iter()
        .map(|(name, definition)| {
            (
                name.clone(),
                definition
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.ty.clone()))
                    .collect(),
            )
        })
        .collect();
    insert_core_struct_field_types(&mut out);
    out
}

fn program_struct_field_types(
    program: &JitProgram,
) -> HashMap<String, Vec<(String, crate::AST::Type)>> {
    let mut out = program
        .struct_field_types
        .iter()
        .filter_map(|(type_name, types)| {
            let names = program.struct_fields.get(type_name)?;
            Some((
                type_name.clone(),
                names
                    .iter()
                    .zip(types)
                    .map(|(name, ty)| {
                        (
                            name.strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX).unwrap_or(name).to_string(),
                            ty.clone(),
                        )
                    })
                    .collect(),
            ))
        })
        .collect();
    insert_core_struct_field_types(&mut out);
    out
}

pub fn run_program(
    program: &JitProgram,
    base_dir: &Path,
    sink: &mut DevSink,
    globals: HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    gates: jet_foundation::Policy::GateSet,
) -> Result<CtValue, Diagnostic> {
    run_program_with_structs(
        program,
        base_dir,
        sink,
        globals,
        core_imports,
        gates,
        HashMap::new(),
        HashMap::new(),
    )
}

fn validate_kernel_proofs(program: &JitProgram) -> Result<(), Diagnostic> {
    if let Some(func) = program
        .funcs
        .iter()
        .find(|func| func.kernel_proof.is_some_and(|proof| !proof.is_complete()))
    {
        return Err(crate::Sema::Diagnostics::render_registered(
            "E0956",
            format!("kernel proof for `{}` is incomplete", func.name),
            "the interpreter consumes sema's complete kernel proof before execution"
                .to_string(),
            "report this as a compiler bug".to_string(),
            Some(func.source_span),
        ));
    }
    Ok(())
}

pub fn run_program_with_structs(
    program: &JitProgram,
    base_dir: &Path,
    sink: &mut DevSink,
    globals: HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    gates: jet_foundation::Policy::GateSet,
    struct_fields: HashMap<String, Vec<(String, bool)>>,
    struct_field_types: HashMap<String, Vec<(String, crate::AST::Type)>>,
) -> Result<CtValue, Diagnostic> {
    run_program_with_structs_at_stage(
        program,
        base_dir,
        sink,
        globals,
        core_imports,
        gates,
        struct_fields,
        struct_field_types,
        Comptime::PurityStage::RunTime,
    )
}

pub fn run_program_with_structs_at_stage(
    program: &JitProgram,
    base_dir: &Path,
    sink: &mut DevSink,
    globals: HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    gates: jet_foundation::Policy::GateSet,
    struct_fields: HashMap<String, Vec<(String, bool)>>,
    struct_field_types: HashMap<String, Vec<(String, crate::AST::Type)>>,
    stage: Comptime::PurityStage,
) -> Result<CtValue, Diagnostic> {
    run_program_with_structs_at_stage_and_cli(
        program,
        base_dir,
        sink,
        globals,
        core_imports,
        gates,
        struct_fields,
        struct_field_types,
        stage,
        None,
        program.package_hardened,
    )
}

fn run_program_with_structs_at_stage_and_cli(
    program: &JitProgram,
    base_dir: &Path,
    sink: &mut DevSink,
    globals: HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    gates: jet_foundation::Policy::GateSet,
    struct_fields: HashMap<String, Vec<(String, bool)>>,
    struct_field_types: HashMap<String, Vec<(String, crate::AST::Type)>>,
    stage: Comptime::PurityStage,
    cli_bundle: Option<&ProgramBundle>,
    package_hardened: bool,
) -> Result<CtValue, Diagnostic> {
    let cli_dispatch = if let Some(bundle) = cli_bundle.filter(|_| {
        program.entry == crate::Codegen::mangle_generated("cli_main")
    }) {
        // A typed-CLI entry parses the *program's* argv. When no embedder
        // installed one (`jet dev`, an embedder holding a checked bundle, a
        // test harness), the program simply received no arguments — it did
        // not receive the compiler's own command line. Reading
        // `std::env::args()` here handed `jet dev … --watch=off` and libtest's
        // `--test-threads=…` to the user's parser, which rejected them and
        // exited 2 before the body ran. Fall back to the canonical shape a
        // bare `jet run <file>` installs instead: argv[0] = the entry path,
        // no program arguments. Parsing, help, and error text stay in the
        // Prelude Args kernel; this only supplies the argv it reads.
        let argv = Comptime::runtime_argv()
            .unwrap_or_else(|| vec![bundle.modules[bundle.entry].display.clone()]);
        match cli::prepare(bundle, &argv)? {
            dispatch @ (cli::Dispatch::Run(_)
            | cli::Dispatch::Direct { .. }
            | cli::Dispatch::Invoke { .. }) => Some(dispatch),
            cli::Dispatch::Version(version) => {
                sink.stdout.push_str(&cli_boundary::jet_cli_banner(&version));
                return Ok(CtValue::Unit);
            }
            cli::Dispatch::Help(help) => {
                sink.stdout.push_str(&cli_boundary::jet_cli_banner(&help));
                return Ok(CtValue::Unit);
            }
            cli::Dispatch::Error(error) => {
                sink.stderr.push_str(&cli_boundary::jet_cli_banner(&error));
                sink.exit_code = Some(2);
                return Ok(CtValue::Unit);
            }
        }
    } else {
        None
    };
    // The evaluator's exhaustive expression dispatcher is intentionally one
    // semantic spine, but its large Rust frame makes ordinary test/CLI stacks
    // too small for nested aggregate literals. Keep the public runtime seam
    // on a bounded worker stack; this changes no language semantics.
    //
    // Thread-locals across the spawn. `PACKAGE_EDITION` is established *inside*
    // the worker from the program's own edition fact, so `with_package_edition`
    // stays under the boundary rather than over it — the same discipline sema
    // states in `Sema/Bundle/Pipeline.rs` and the driver states in
    // `jet_driver::run_compiler_work`. Wrapping the caller instead leaves this
    // worker on the reverted "2026" default, which picks the unchecked
    // `core.data` surface for a program sema typed as checked: `line_text` then
    // yields a plain `Str` that matches neither the `.Ok` nor the `.Err` arm and
    // the arm table prints nothing. The comptime ambient hooks are the one piece
    // of caller-established state the run reads, so they are carried explicitly.
    let (ambient_core, ambient_handle) = crate::Comptime::ambient_hooks();
    let edition = program.edition.clone();
    // D-FAIL-CTX1 / I9: the E3002 journey belongs to the program, not to
    // whichever thread the evaluator needed for stack room. `?` pushes its hop
    // inside this worker (`eval/exprs.rs`, `TExprKind::Try`), while every
    // report edge — `Interpreter::run_checked`, `Interpreter::run_named_job`,
    // the JIT deopt boundary — calls `jet_journey_report` on the caller after
    // this join. Leaving the hops on the worker is why `jet run --interpret`
    // printed `Error: file not found` with no trail while AOT and the resident
    // tier printed the three hops: the report edge drained the caller's empty
    // list. Foundation still owns the hops, their collapse and their
    // rendering; this only carries them across a boundary the evaluator
    // created for itself.
    let (outcome, journey) = std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("jet-tir-eval".to_string())
            .stack_size(64 * 1024 * 1024)
            .spawn_scoped(scope, move || {
                let outcome = jet_foundation::PackageEdition::with_package_edition(&edition, || {
                crate::Comptime::with_ambient(ambient_core, ambient_handle, || {
                    run_program_with_structs_on_stack(
                        program,
                        base_dir,
                        sink,
                        globals,
                        core_imports,
                        gates,
                        struct_fields,
                        struct_field_types,
                        stage,
                        cli_dispatch,
                        package_hardened,
                    )
                })
                });
                (outcome, jet_foundation::Outcome::jet_journey_take_hops())
            })
            .expect("evaluator worker");
        worker.join().expect("evaluator worker panicked")
    });
    jet_foundation::Outcome::jet_journey_adopt(journey);
    outcome
}

fn run_program_with_structs_on_stack(
    program: &JitProgram,
    base_dir: &Path,
    sink: &mut DevSink,
    globals: HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    gates: jet_foundation::Policy::GateSet,
    struct_fields: HashMap<String, Vec<(String, bool)>>,
    mut struct_field_types: HashMap<String, Vec<(String, crate::AST::Type)>>,
    stage: Comptime::PurityStage,
    cli_dispatch: Option<cli::Dispatch>,
    package_hardened: bool,
) -> Result<CtValue, Diagnostic> {
    validate_kernel_proofs(program)?;
    jet_foundation::MemSentry::jet_sentry_reset();
    jet_foundation::MemSentry::jet_sentry_set_hardened(package_hardened);
    // Fresh EventLite stores per whole-program run (REPL / warm cache / workers).
    crate::Comptime::reset_event_lite();
    // A whole-program run starts with no interrupt pending: a SIGINT marked for
    // a previous dev/restart instance must not land on this one. This is the
    // run boundary, not `EvalRuntime::with_memos` — that also builds the
    // per-call runtime for mid-run deopt (`run_named_func_with_memos`), and
    // clearing there would drop a signal delivered to the JIT tier mid-run now
    // that both tiers share one count (#2027).
    interrupt_runtime::jet_interrupt_clear();
    let _browser_session = browser::SessionGuard::new();
    insert_core_struct_field_types(&mut struct_field_types);
    for (name, fields) in program_struct_field_types(program) {
        struct_field_types.entry(name).or_insert(fields);
    }
    let funcs = program_funcs(program);
    let entry_name = match &cli_dispatch {
        Some(cli::Dispatch::Run(_)) => "run".to_string(),
        Some(cli::Dispatch::Direct { function, .. })
        | Some(cli::Dispatch::Invoke { function, .. }) => function.clone(),
        Some(cli::Dispatch::Help(_))
        | Some(cli::Dispatch::Version(_))
        | Some(cli::Dispatch::Error(_)) => {
            return Err(crate::Sema::Diagnostics::render_registered(
                "E2201",
                "CLI control dispatch reached the evaluator".to_string(),
                "help and error dispatches must return before TIR execution".to_string(),
                "report this as a compiler bug".to_string(),
                None,
            ));
        }
        None => program.entry.clone(),
    };
    let entry = funcs.get(&entry_name).copied().ok_or_else(|| {
        crate::Sema::Diagnostics::render_registered(
            "E2201",
            format!("entry `{entry_name}` missing from lowered TIR"),
            "the interpreter needs the selected entry function in the TIR program".to_string(),
            "report this as a compiler bug".to_string(),
            None,
        )
    })?;
    let shared_sink = Arc::new(Mutex::new(std::mem::take(sink)));
    let mut ctx = EvalCtx {
        funcs,
        base_dir: base_dir.to_path_buf(),
        source_file: program.source_file.clone(),
        source_text: program.source_text.clone(),
        fuel: DEV_FUEL,
        sink: Some(shared_sink.clone()),
        core_imports,
        globals,
        sentry_places: HashMap::new(),
        next_sentry_allocator: 1,
        gates,
        // Whole-program runtime/deopt carries RunTime explicitly.  Comptime
        // purity still uses eval_expr/eval_block with build-time defaults.
        impure_depth: if matches!(stage, Comptime::PurityStage::RunTime) && gates.allows(jet_foundation::Policy::PolicyKey::Impure) {
            1
        } else {
            0
        },
        runtime_execution: matches!(stage, Comptime::PurityStage::RunTime),
        prefer_tir_calls: false,
        repl_mode: false,
        repl_grants: Vec::new(),
        repl_authorizer: None,
        pending_return: None,
        preserve_allocator_view: false,
        deferred_closes: Vec::new(),
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        source_nesting: 0,
        current_span: entry.source_span,
        current_fn: entry.name.clone(),
        embed_inputs: None,
        struct_fields,
        memo_dependencies: program.memo_dependencies.clone(),
        reflection_fields: program.reflection_fields.clone(),
        reflect_paths: program.reflect_paths.clone(),
        struct_type_params: program.struct_type_params.clone(),
        struct_field_types,
        codec_migrations: program.codec_migrations.clone(),
        distinct_bases: program.distinct_bases.clone(),
        distinct_ranges: program.distinct_ranges.clone(),
        switch_subject: None,
        runtime: Arc::new(Mutex::new(EvalRuntime::new())),
        local_cells: local_cell::EvalLocalCells::new(),
        shared_transactions: Vec::new(),
        spawn_lambdas: &program.spawn_lambdas,
        task_sender: None,
        task_cancel: None,
        task_paused: None,
        context_deadline: None,
        shield_depth: 0,
        yield_consumer: None,
        yield_scope: None,
        scope_guards: Vec::new(),
        shared_guards: Vec::new(),
        txn_stack: Vec::new(),
        place_loans: Vec::new(),
        loan_scopes: Vec::new(),
    };
    let mut scope = HashMap::new();
    let entry_args = match cli_dispatch {
        Some(cli::Dispatch::Run(args)) => vec![args],
        Some(cli::Dispatch::Direct { args, .. }) => args,
        Some(cli::Dispatch::Invoke {
            receiver, args, ..
        }) => {
            if let Some(receiver) = receiver {
                scope.insert("self".to_string(), receiver);
            }
            args
        }
        None => Vec::new(),
        Some(cli::Dispatch::Help(_))
        | Some(cli::Dispatch::Version(_))
        | Some(cli::Dispatch::Error(_)) => Vec::new(),
    };
    let result = ctx.with_task_dispatcher(|ctx| {
        let result = ctx.run_func(entry, entry_args, &mut scope);
        if entry.ret.as_ref().is_some_and(crate::AST::type_is_app) {
            result.and_then(|value| serve_entry_value(ctx, value))
        } else {
            result
        }
    });
    let result = match (result, ctx.run_atexit_handlers()) {
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        (Ok(value), Ok(())) => Ok(value),
    };
    *sink = std::mem::take(
        &mut *shared_sink.lock().expect("evaluator sink poisoned"),
    );
    result
}

/// Run one named function through the canonical TIR evaluator (#778 deopt).
pub fn run_named_func(
    program: &JitProgram,
    name: &str,
    args: Vec<CtValue>,
    sink: &mut DevSink,
) -> Result<CtValue, Diagnostic> {
    run_named_func_with_memos(program, name, args, sink, new_memo_state())
}

/// Run one named function with a caller-owned Prelude memo carrier. The
/// mixed-tier JIT uses this form so repeated deopt calls see one store.
pub fn run_named_func_with_memos(
    program: &JitProgram,
    name: &str,
    args: Vec<CtValue>,
    sink: &mut DevSink,
    memos: MemoState,
) -> Result<CtValue, Diagnostic> {
    // Deopt runs on the JIT's own thread, which never entered a caller's
    // `with_package_edition` scope, so the edition is established here from the
    // program's carried bundle fact — the same reason `package_hardened` rides
    // the program instead of being reparsed or inherited ambiently.
    jet_foundation::PackageEdition::with_package_edition(&program.edition, || {
        run_named_func_on_program_edition(program, name, args, sink, memos)
    })
}

fn run_named_func_on_program_edition(
    program: &JitProgram,
    name: &str,
    args: Vec<CtValue>,
    sink: &mut DevSink,
    memos: MemoState,
) -> Result<CtValue, Diagnostic> {
    validate_kernel_proofs(program)?;
    jet_foundation::MemSentry::jet_sentry_reset();
    jet_foundation::MemSentry::jet_sentry_set_hardened(program.package_hardened);
    let _browser_session = browser::SessionGuard::new();
    let funcs = program_funcs(program);
    let func = funcs.get(name).copied().ok_or_else(|| {
        crate::Sema::Diagnostics::render_registered(
            "E2201",
            format!("function `{name}` missing from lowered TIR"),
            "the deopt tier needs the named function in the TIR program".to_string(),
            "report this as a compiler bug".to_string(),
            None,
        )
    })?;
    let core_imports = HashMap::new();
    let shared_sink = Arc::new(Mutex::new(std::mem::take(sink)));
    let mut ctx = EvalCtx {
        funcs,
        base_dir: PathBuf::from("."),
        source_file: program.source_file.clone(),
        source_text: program.source_text.clone(),
        fuel: DEV_FUEL,
        sink: Some(shared_sink.clone()),
        core_imports: &core_imports,
        globals: HashMap::new(),
        sentry_places: HashMap::new(),
        next_sentry_allocator: 1,
        gates: jet_foundation::Policy::GateSet::allow(jet_foundation::Policy::PolicyKey::Impure),
        // Runtime deopt is not comptime: open Tier-2 ambient I/O so `jet run`
        // matches AOT for env/fs/process (D-LENS-RUN2 / #778).
        // parity: guard tests/dev_default_parity.rs::dev_default_matches_compiled_binary
        impure_depth: 1,
        runtime_execution: true,
        prefer_tir_calls: program.canonical_calls.contains(name),
        repl_mode: false,
        repl_grants: Vec::new(),
        repl_authorizer: None,
        pending_return: None,
        preserve_allocator_view: false,
        deferred_closes: Vec::new(),
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        source_nesting: 0,
        current_span: func.source_span,
        current_fn: func.name.clone(),
        embed_inputs: None,
        struct_fields: HashMap::new(),
        memo_dependencies: program.memo_dependencies.clone(),
        reflection_fields: program.reflection_fields.clone(),
        reflect_paths: program.reflect_paths.clone(),
        struct_type_params: program.struct_type_params.clone(),
        struct_field_types: program_struct_field_types(program),
        codec_migrations: program.codec_migrations.clone(),
        distinct_bases: program.distinct_bases.clone(),
        distinct_ranges: program.distinct_ranges.clone(),
        switch_subject: None,
        runtime: Arc::new(Mutex::new(EvalRuntime::with_memos(memos))),
        local_cells: local_cell::EvalLocalCells::new(),
        shared_transactions: Vec::new(),
        spawn_lambdas: &program.spawn_lambdas,
        task_sender: None,
        task_cancel: None,
        task_paused: None,
        context_deadline: None,
        shield_depth: 0,
        yield_consumer: None,
        yield_scope: None,
        scope_guards: Vec::new(),
        shared_guards: Vec::new(),
        txn_stack: Vec::new(),
        place_loans: Vec::new(),
        loan_scopes: Vec::new(),
    };
    let mut scope = HashMap::new();
    let result = ctx.with_task_dispatcher(|ctx| ctx.run_func(func, args, &mut scope));
    *sink = std::mem::take(
        &mut *shared_sink.lock().expect("evaluator sink poisoned"),
    );
    result
}

static INSTALLED: OnceLock<()> = OnceLock::new();

pub fn install_comptime_bridge() {
    INSTALLED.get_or_init(|| {
        Comptime::TirBridge::install(Comptime::TirBridge::Hooks {
            run_bundle,
            run_bundle_at_stage,
            eval_expr: eval_expr_hook,
            eval_block: eval_block_hook,
        });
    });
}

fn run_bundle(
    bundle: &ProgramBundle,
    sink: &mut DevSink,
    gates: jet_foundation::Policy::GateSet,
) -> Result<CtValue, Diagnostic> {
    run_bundle_at_stage(bundle, sink, gates, Comptime::PurityStage::RunTime)
}

fn run_bundle_at_stage(
    bundle: &ProgramBundle,
    sink: &mut DevSink,
    gates: jet_foundation::Policy::GateSet,
    stage: Comptime::PurityStage,
) -> Result<CtValue, Diagnostic> {
    // The edition is a package fact, not an ambient one, and it is established
    // where the work runs: TIR lowering re-establishes it from `bundle.edition`
    // (`Codegen/TIR/mod.rs`), and the lowered program carries the same fact to
    // the evaluator's worker and to named deopt. Wrapping this caller instead
    // would sit *over* the evaluator's `std::thread::scope` spawn, which is how
    // a checked-surface program ran against the reverted "2026" default.
    //
    // Two unrelated failures used to arrive here as one `None` and both were
    // reported as a missing `run` (card #2001). They are separated now:
    //
    //  * the program really has no runnable entry — a user error, and the one
    //    case E2201's text describes, so it keeps that text verbatim;
    //  * lowering failed on a program that DOES have an entry — a compiler
    //    defect. Telling that reader to add a function they already wrote sent
    //    them to fix something that is not wrong and hid every defect reaching
    //    this path, so it takes the branded internal-error rail with the real
    //    reason and exits 101 (I2) instead of any user diagnostic.
    let program = match lower_interp_program(bundle) {
        Some(program) => program,
        None => {
            let reason = TIR::lower_jit_program_fail_reason(bundle);
            if reason == TIR::NO_RUNNABLE_ENTRY || reason == TIR::CLI_ENTRY_MISSING_RUN {
                return Err(crate::Sema::Diagnostics::render_registered(
                    "E2201",
                    "`jet dev` needs a `run` function to run".to_string(),
                    "`jet dev` runs a program; a library with no `run` has nothing to execute"
                        .to_string(),
                    "add `fn run() { … }`, or use `jet check <file>`".to_string(),
                    None,
                ));
            }
            jet_foundation::ice!(
                None,
                "whole-program TIR lowering produced no program for the dev interpreter ({reason}) — compiler bug (I2/R7)"
            )
        }
    };
    let mut globals = HashMap::new();
    let mut core_imports = HashMap::new();
    let persist = jet_foundation::Persist::prepare_bundle(bundle);
    for msg in &persist.messages {
        // Dev-tier only: surface reset / migration notes on stderr.
        eprintln!("{msg}");
    }
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        for item in &module.items {
            if let crate::AST::Item::Const(c) = item {
                let value = if c.is_persist {
                    persist.by_name.get(&c.name).cloned()
                } else {
                    None
                }
                .or_else(|| {
                    c.ct.clone().or_else(|| match &c.value {
                        crate::AST::Expr::Int(v, _, _, _) => Some(CtValue::Int(*v)),
                        crate::AST::Expr::Bool(v, _) => Some(CtValue::Bool(*v)),
                        _ => None,
                    })
                });
                if let Some(v) = value {
                    globals.entry(c.name.clone()).or_insert_with(|| v.clone());
                    // ConstRef sometimes carries the Rust-mangled spelling.
                    globals
                        .entry(mangle(&c.name))
                        .or_insert(v);
                }
            }
        }
        for imp in &module.imports {
            if matches!(imp.kind, crate::AST::ImportKind::Unqualified { .. }) {
                for binding in imp.walk_bindings() {
                    let Some(_original) = binding.original else {
                        continue;
                    };
                    let local = binding.local;
                    if let Some(binding) = bundle.name_ledger.effective_alias(module_idx, &local) {
                        if binding.target == "core" || binding.target.starts_with("core.") {
                            core_imports
                                .entry(local)
                                .or_insert_with(|| binding.target.clone());
                        }
                    }
                }
            } else if let Some(core_module) = imp.core_module_path() {
                let alias = imp.import_alias();
                if bundle.name_ledger.effective_alias(module_idx, &alias).is_some() {
                    core_imports.entry(alias).or_insert(core_module);
                }
            }
        }
    }
    run_program_with_structs_at_stage_and_cli(
        &program,
        &bundle.project_root,
        sink,
        globals,
        &core_imports,
        gates,
        collect_struct_fields(bundle),
        collect_struct_field_types(bundle),
        stage,
        Some(bundle),
        bundle.package_guarantees.harden,
    )
}

fn eval_expr_hook(
    req: &mut Comptime::TirBridge::ExprEvalRequest<'_>,
) -> Result<CtValue, Diagnostic> {
    let fragment_funcs = merge_fragment_funcs(req.funcs, req.methods);
    let (tir, mut spawn_lambdas) = lower_expr_for_eval(
        req.expr,
        &fragment_funcs,
        req.methods,
        req.structs,
        req.computed_fields,
        req.globals,
        req.core_imports,
        req.distinct_ranges,
        req.distinct_bases,
        req.unit_families,
    )?;
    let mut cx = empty_cx();
    seed_fragment_structs(&mut cx, req.structs, req.methods, req.computed_fields);
    seed_fragment_distinct_types(&mut cx, req.distinct_ranges, req.distinct_bases);
    seed_fragment_unit_families(&mut cx, req.unit_families);
    seed_fragment_funcs(&mut cx, &fragment_funcs);
    cx.struct_fields = normalize_struct_field_types(req.structs);
    cx.type_names.extend(req.structs.keys().cloned());
    cx.core_imports = req.core_imports.clone();
    cx.jit_spawn_site_base = spawn_lambdas.len();
    let lowered: Vec<TFunc> = fragment_funcs
        .iter()
        .filter_map(|(name, f)| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::Codegen::TIR::with_eval_fragment(|| {
                    let mut lowered = match name.rsplit_once("::") {
                        Some((owner, method))
                            if req
                                .methods
                                .contains_key(&(owner.to_string(), method.to_string())) =>
                        {
                            TIR::lower_method(f, owner, &cx)
                        }
                        Some((owner, "encode")) => {
                            TIR::lower_trait_method(f, owner, &cx, crate::Generics::ENCODE)
                        }
                        Some((owner, "decode")) => {
                            TIR::lower_trait_method(f, owner, &cx, crate::Generics::DECODE)
                        }
                        _ => TIR::lower_func(f, &cx),
                    };
                    lowered.name = name.clone();
                    lowered
                })
            }))
            .ok()
        })
        .collect();
    spawn_lambdas.extend(std::mem::take(&mut *cx.jit_spawn_lambdas.borrow_mut()));
    let funcs: HashMap<String, &TFunc> = lowered.iter().map(|f| (f.name.clone(), f)).collect();
    let base_dir = req.base_dir.to_path_buf();
    let fuel = req.fuel;
    let core_imports = req.core_imports;
    let globals = req.globals.clone();
    let gates = req.gates;
    let impure_depth = req.initial_impure_depth;
    let repl_mode = req.repl_mode;
    let repl_grants = req.repl_grants.to_vec();
    let repl_authorizer = reborrow_repl_authorizer(&mut req.repl_authorizer);
    let source_span = req.expr.span();
    let mut sink_target = req.sink.take();
    let mutated_out = req.mutated.take();
    let sink = sink_target
        .as_deref_mut()
        .map(|sink| Arc::new(Mutex::new(std::mem::take(sink))));
    let embed_inputs = req.embed_inputs.take();
    let mut ctx = EvalCtx {
        funcs,
        base_dir,
        source_file: String::new(),
        source_text: String::new(),
        fuel,
        sink,
        core_imports,
        globals: globals.clone(),
        sentry_places: HashMap::new(),
        next_sentry_allocator: 1,
        gates,
        impure_depth,
        runtime_execution: false,
        prefer_tir_calls: false,
        repl_mode,
        repl_grants,
        repl_authorizer,
        pending_return: None,
        preserve_allocator_view: false,
        deferred_closes: Vec::new(),
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        source_nesting: 0,
        current_span: source_span,
        current_fn: String::new(),
        embed_inputs,
        struct_fields: HashMap::new(),
        memo_dependencies: HashMap::new(),
        reflection_fields: HashMap::new(),
        reflect_paths: HashMap::new(),
        struct_type_params: HashMap::new(),
        struct_field_types: normalize_struct_field_types(req.structs),
        codec_migrations: HashMap::new(),
        distinct_bases: HashMap::new(),
        distinct_ranges: HashMap::new(),
        switch_subject: None,
        runtime: Arc::new(Mutex::new(EvalRuntime::new())),
        local_cells: local_cell::EvalLocalCells::new(),
        shared_transactions: Vec::new(),
        spawn_lambdas: &spawn_lambdas,
        task_sender: None,
        task_cancel: None,
        task_paused: None,
        context_deadline: None,
        shield_depth: 0,
        yield_consumer: None,
        yield_scope: None,
        scope_guards: Vec::new(),
        shared_guards: Vec::new(),
        txn_stack: Vec::new(),
        place_loans: Vec::new(),
        loan_scopes: Vec::new(),
    };
    let mut scope = globals;
    let result = if spawn_lambdas.is_empty() {
        ctx.eval_expr(&tir, &mut scope)
    } else {
        ctx.with_task_dispatcher(|ctx| ctx.eval_expr(&tir, &mut scope))
    }
    .map(|value| ctx.pending_return.take().unwrap_or(value));
    if let (Some(target), Some(shared)) = (sink_target, ctx.sink.as_ref()) {
        *target = std::mem::take(&mut *shared.lock().expect("evaluator sink poisoned"));
    }
    // Hand back the bindings the expression left behind: a mutating receiver
    // (`reader.read_u8()`, `cursor.skip_ws()`) advances state the next
    // statement must observe.
    if let Some(out) = mutated_out {
        *out = scope;
    }
    result
}

fn eval_block_hook(
    req: &mut Comptime::TirBridge::BlockEvalRequest<'_>,
) -> Result<Comptime::TirBridge::StmtOutcome, Diagnostic> {
    let fragment_funcs = merge_fragment_funcs(req.funcs, req.methods);
    let (tir, mut spawn_lambdas) = lower_stmts_for_eval(
        req.stmts,
        &fragment_funcs,
        req.methods,
        req.structs,
        req.computed_fields,
        req.globals,
        req.core_imports,
        req.distinct_ranges,
        req.distinct_bases,
        req.unit_families,
    )?;
    let mut cx = empty_cx();
    seed_fragment_structs(&mut cx, req.structs, req.methods, req.computed_fields);
    seed_fragment_distinct_types(&mut cx, req.distinct_ranges, req.distinct_bases);
    seed_fragment_unit_families(&mut cx, req.unit_families);
    seed_fragment_funcs(&mut cx, &fragment_funcs);
    cx.core_imports = req.core_imports.clone();
    cx.jit_spawn_site_base = spawn_lambdas.len();
    let lowered: Vec<TFunc> = fragment_funcs
        .iter()
        .filter_map(|(name, f)| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::Codegen::TIR::with_eval_fragment(|| {
                    let mut lowered = match name.rsplit_once("::") {
                        Some((owner, method))
                            if req
                                .methods
                                .contains_key(&(owner.to_string(), method.to_string())) =>
                        {
                            TIR::lower_method(f, owner, &cx)
                        }
                        _ => TIR::lower_func(f, &cx),
                    };
                    lowered.name = name.clone();
                    lowered
                })
            }))
            .ok()
        })
        .collect();
    spawn_lambdas.extend(std::mem::take(&mut *cx.jit_spawn_lambdas.borrow_mut()));
    let funcs: HashMap<String, &TFunc> = lowered.iter().map(|f| (f.name.clone(), f)).collect();
    let base_dir = req.base_dir.to_path_buf();
    let fuel = req.fuel;
    let core_imports = req.core_imports;
    let globals = req.globals.clone();
    let gates = req.gates;
    let impure_depth = req.impure_depth;
    let repl_mode = req.repl_mode;
    let repl_grants = req.repl_grants.to_vec();
    let repl_authorizer = reborrow_repl_authorizer(&mut req.repl_authorizer);
    let source_span = req
        .stmts
        .first()
        .map(crate::AST::Stmt::span)
        .unwrap_or_else(|| Span::new(0, 0));
    let mut sink_target = req.sink.take();
    let sink = sink_target
        .as_deref_mut()
        .map(|sink| Arc::new(Mutex::new(std::mem::take(sink))));
    let embed_inputs = req.embed_inputs.take();
    let mut ctx = EvalCtx {
        funcs,
        base_dir,
        source_file: String::new(),
        source_text: String::new(),
        fuel,
        sink,
        core_imports,
        globals: globals.clone(),
        sentry_places: HashMap::new(),
        next_sentry_allocator: 1,
        gates,
        impure_depth,
        runtime_execution: false,
        prefer_tir_calls: false,
        repl_mode,
        repl_grants,
        repl_authorizer,
        pending_return: None,
        preserve_allocator_view: false,
        deferred_closes: Vec::new(),
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        source_nesting: 0,
        current_span: source_span,
        current_fn: String::new(),
        embed_inputs,
        struct_fields: HashMap::new(),
        memo_dependencies: HashMap::new(),
        reflection_fields: HashMap::new(),
        reflect_paths: HashMap::new(),
        struct_type_params: HashMap::new(),
        struct_field_types: normalize_struct_field_types(req.structs),
        codec_migrations: HashMap::new(),
        distinct_bases: HashMap::new(),
        distinct_ranges: HashMap::new(),
        switch_subject: None,
        runtime: Arc::new(Mutex::new(EvalRuntime::new())),
        local_cells: local_cell::EvalLocalCells::new(),
        shared_transactions: Vec::new(),
        spawn_lambdas: &spawn_lambdas,
        task_sender: None,
        task_cancel: None,
        task_paused: None,
        context_deadline: None,
        shield_depth: 0,
        yield_consumer: None,
        yield_scope: None,
        scope_guards: Vec::new(),
        shared_guards: Vec::new(),
        txn_stack: Vec::new(),
        place_loans: Vec::new(),
        loan_scopes: Vec::new(),
    };
    let mut scope = globals;
    let outcome = if spawn_lambdas.is_empty() {
        ctx.exec_stmts(&tir, &mut scope)
    } else {
        ctx.with_task_dispatcher(|ctx| ctx.exec_stmts(&tir, &mut scope))
    }?;
    let outcome = match outcome {
        Flow::Normal => Ok(Comptime::TirBridge::StmtOutcome::Done(scope)),
        Flow::Return(value) => Ok(Comptime::TirBridge::StmtOutcome::Returned { value, scope }),
        other => Err(unsupported(
            &format!("control flow {other:?} in statement fragment"),
            ctx.span(),
        )),
    };
    if let (Some(target), Some(shared)) = (sink_target, ctx.sink.as_ref()) {
        *target = std::mem::take(&mut *shared.lock().expect("evaluator sink poisoned"));
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::{
        mpsc, select_eval_tasks, unsupported, Arc, CtValue, EvalTask,
        EvalTaskCompletion, OnceLock, Span,
    };
    use crate::task_group::JetTaskSelectMode;
    use crate::scheduler::JetTaskControl;

    fn ready_task(
        order: u64,
        result: Result<CtValue, crate::Diagnostics::Diagnostic>,
    ) -> EvalTask {
        let (sender, completion) = mpsc::sync_channel(1);
        let completion_order = Arc::new(OnceLock::new());
        completion_order.set(order).unwrap();
        sender.send(EvalTaskCompletion { result }).unwrap();
        EvalTask {
            completion,
            completion_order,
            completion_wait: crate::scheduler::ParkSlot::new(),
            control: JetTaskControl::new(),
        }
    }

    #[test]
    fn selection_keeps_every_already_ready_completion() {
        let all = select_eval_tasks(
            vec![
                ready_task(1, Ok(CtValue::Int(10))),
                ready_task(0, Ok(CtValue::Int(20))),
            ],
            JetTaskSelectMode::All,
            Span::new(0, 0),
            || Ok(()),
        )
        .unwrap();
        assert_eq!(all, [CtValue::Int(10), CtValue::Int(20)]);

        let race = select_eval_tasks(
            vec![
                ready_task(
                    0,
                    Err(unsupported("first completion failed", Span::new(0, 0))),
                ),
                ready_task(1, Ok(CtValue::Int(22))),
            ],
            JetTaskSelectMode::Race,
            Span::new(0, 0),
            || Ok(()),
        )
        .unwrap();
        assert_eq!(race, [CtValue::Int(22)]);
    }
}
