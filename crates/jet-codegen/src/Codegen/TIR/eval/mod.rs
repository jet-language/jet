//! Canonical TIR evaluator — reference semantics (D-ONECORE1=A / #777).

mod builtins;
mod browser;
mod closure_ops;
mod data_calls;
mod event_ops;
mod exprs;
mod handles;
mod regex_ops;
mod stmts;

mod range_semantics {
    use jet_foundation::StructuralDebug::jet_debug_range;
    include!("../../../Prelude/Core/RangeBounds.rs");
}

#[allow(dead_code)]
mod uninit_semantics {
    include!("../../../Prelude/Uninit.rs");
}

#[allow(dead_code)]
mod shared_protocol {
    include!("../../../Prelude/SharedProtocol.rs");
}

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, OnceLock};

use crate::AST::{Expr, Func, ProgramBundle, Stmt, Type};
use super::Cx;
use crate::Codegen::TIR::{
    self, JitProgram, LowerEnv, TExpr, TFunc, TJitSpawnBody, TJitSpawnLambda, TLocal, TStmt,
};
use super::build_cx_items;
use crate::Comptime::{self, CtValue, DevSink};
use crate::Diagnostics::{Diagnostic, Span};

/// Cross-tier hook: Cranelift-native functions callable from the TIR evaluator (#778).
pub type NativeCallHook = fn(&str, &[CtValue]) -> Option<Result<CtValue, Diagnostic>>;

thread_local! {
    static NATIVE_CALL_HOOK: Cell<Option<NativeCallHook>> = const { Cell::new(None) };
}

pub fn set_native_call_hook(hook: Option<NativeCallHook>) {
    NATIVE_CALL_HOOK.with(|slot| slot.set(hook));
}

pub(super) fn native_call_hook() -> Option<NativeCallHook> {
    NATIVE_CALL_HOOK.with(Cell::get)
}

pub(super) fn raw_place_local(expr: &TExpr) -> Option<&TLocal> {
    match &expr.kind {
        TIR::TExprKind::Local(local) => Some(local),
        TIR::TExprKind::Borrow { place, .. } => raw_place_local(place),
        TIR::TExprKind::DistinctCtor { arg, .. } => raw_place_local(arg),
        _ => None,
    }
}

pub(super) fn unsupported(what: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        format!("{what} can't run at compile time yet"),
        "the canonical TIR evaluator doesn't cover this construct yet".to_string(),
        "use a simpler form, or run via `jet build` / `jet run`".to_string(),
        Some(span),
    )
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
    let exclusive = matches!(field("exclusive"), Some(CtValue::Bool(true)));
    range_semantics::jet_range_bounds(start, end, exclusive, len as i64)
        .ok_or_else(|| unsupported("Range window bounds", span))
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

/// Resolve a `__JetViewMut { base, start, end }` handle to the inclusive window List.
pub(super) fn materialize_view_mut_window(
    fields: &[(String, CtValue)],
    scope: &HashMap<String, CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
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
    let (base, start, end) = match (base, start, end) {
        (Some(b), Some(s), Some(e)) => (b, s, e),
        _ => return Err(unsupported("view-mut fields", span)),
    };
    let Some(CtValue::List(items)) = scope.get(&base) else {
        return Err(unsupported("view-mut owner", span));
    };
    if start < 0
        || end < start - 1
        || (end >= start && end as usize >= items.len())
        || start as usize > items.len()
    {
        return Err(unsupported("view-mut bounds", span));
    }
    if end < start {
        Ok(CtValue::List(Vec::new()))
    } else {
        Ok(CtValue::List(items[start as usize..=end as usize].to_vec()))
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
    pub(super) fuel: u64,
    pub(super) sink: Option<Arc<Mutex<DevSink>>>,
    #[allow(dead_code)]
    pub(super) core_imports: &'a HashMap<String, String>,
    pub(super) globals: HashMap<String, CtValue>,
    #[allow(dead_code)]
    pub(super) allow_impure: bool,
    #[allow(dead_code)]
    pub(super) impure_depth: usize,
    /// True only for an actual dev/JIT runtime execution. Comptime may permit
    /// explicit I/O, but it must never open a Browser session while compiling.
    pub(super) runtime_execution: bool,
    /// Keep calls inside a codec-sensitive named deopt on canonical TIR.
    pub(super) prefer_tir_calls: bool,
    pub(super) repl_mode: bool,
    pub(super) pending_return: Option<CtValue>,
    /// `defer close(^…)` exprs scheduled in the current eval frame (LIFO).
    pub(super) deferred_closes: Vec<&'a TExpr>,
    /// Control emitted by an inline loop expression that targets an enclosing
    /// loop. The containing statement list consumes and propagates it.
    pub(super) pending_flow: Option<Flow>,
    /// Compiler-private eager List sinks for raw comptime yielding-loop
    /// fragments. Fully checked programs rewrite these sends to `List.push`.
    pub(super) collecting_items: Vec<Vec<CtValue>>,
    pub(super) call_depth: usize,
    pub(super) emitted_fragments: Option<&'a mut Vec<String>>,
    pub(super) embed_inputs: Option<&'a mut Vec<crate::AST::ComptimeInput>>,
    /// `TypeName -> [(field, redact)]` for JetDebug formatting (D-DISPLAYDBG).
    pub(super) struct_fields: HashMap<String, Vec<(String, bool)>>,
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
    shared_transactions: Vec<Vec<EvalSharedDelta<'a>>>,
    /// Spawn bodies are lowered separately because native tiers compile them as
    /// independent functions. The evaluator records each outcome behind a task
    /// handle, then observes it only at join/group boundaries.
    spawn_lambdas: &'a [TJitSpawnLambda],
    task_sender: Option<mpsc::Sender<EvalTaskJob<'a>>>,
    task_cancel: Option<Arc<AtomicBool>>,
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

enum EvalCallable<'a> {
    Lambda {
        lambda: &'a TIR::TLambda,
        captured: HashMap<String, CtValue>,
    },
    Named(&'a str),
}

struct EvalStream<'a> {
    func: &'a TFunc,
    args: Vec<CtValue>,
}

struct EvalRuntime<'a> {
    callables: Vec<EvalCallable<'a>>,
    streams: Vec<EvalStream<'a>>,
    shared_values: Vec<Arc<EvalSharedState>>,
    shared_guards: Vec<Arc<shared_protocol::JetSharedPermit>>,
    shared_conditions: Vec<Arc<shared_protocol::JetConditionProtocol>>,
    clocks: Vec<i64>,
    task_groups: Vec<Vec<usize>>,
    tasks: Vec<Option<EvalTask>>,
    completion_order: AtomicU64,
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
        self.protocol.acquire(editable, || {
            cancel.is_some_and(|cancel| cancel.load(Ordering::Acquire))
        })
    }
}

struct EvalConditionWaiter {
    notified: Mutex<bool>,
    wake: Condvar,
    cancel: Option<Arc<AtomicBool>>,
}

impl EvalConditionWaiter {
    fn new(cancel: Option<Arc<AtomicBool>>) -> Self {
        Self {
            notified: Mutex::new(false),
            wake: Condvar::new(),
            cancel,
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
}

struct EvalTask {
    completion: mpsc::Receiver<EvalTaskCompletion>,
    completion_order: Arc<OnceLock<u64>>,
    cancel: Arc<AtomicBool>,
}

struct EvalTaskCompletion {
    result: Result<CtValue, Diagnostic>,
}

struct EvalTaskJob<'a> {
    lambda: &'a TJitSpawnLambda,
    captured: HashMap<String, CtValue>,
    completion: mpsc::SyncSender<EvalTaskCompletion>,
    completion_order: Arc<OnceLock<u64>>,
    cancel: Arc<AtomicBool>,
}

fn cancel_and_drain_eval_tasks(tasks: Vec<EvalTask>) {
    for task in &tasks {
        task.cancel.store(true, Ordering::Release);
    }
    for task in tasks {
        let _ = task.completion.recv();
    }
}

fn select_eval_tasks(
    mut tasks: Vec<Option<EvalTask>>,
    mode: crate::task_group::JetTaskSelectMode,
    span: Span,
    mut wait_check: impl FnMut() -> Result<(), Diagnostic>,
) -> Result<Vec<CtValue>, Diagnostic> {
    let mut pending = std::iter::repeat_with(|| None)
        .take(tasks.len())
        .collect::<Vec<Option<EvalTaskCompletion>>>();
    let mut policy = crate::task_group::JetTaskSelectPolicy::new(mode, tasks.len());
    loop {
        wait_check()?;
        for (index, task) in tasks.iter().enumerate() {
            let Some(task) = task else { continue };
            if pending[index].is_some() {
                continue;
            }
            match task.completion.try_recv() {
                Ok(completion) => pending[index] = Some(completion),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(unsupported("task completion", span));
                }
            }
        }

        let next = tasks
            .iter()
            .enumerate()
            .filter_map(|(index, task)| {
                task.as_ref()
                    .and_then(|task| task.completion_order.get().copied())
                    .map(|order| (order, index))
            })
            .min();
        if let Some((order, index)) = next {
            if let Some(completion) = pending[index].take() {
                tasks[index] = None;
                if let crate::task_group::JetTaskDecision::Finish(result) =
                    policy.settle(order.into(), index, completion.result)
                {
                    for (index, completion) in pending.iter_mut().enumerate() {
                        if completion.take().is_some() {
                            tasks[index] = None;
                        }
                    }
                    cancel_and_drain_eval_tasks(tasks.into_iter().flatten().collect());
                    return result;
                }
            }
        }
        std::thread::yield_now();
    }
}

#[derive(Clone)]
struct EvalTaskConfig<'a> {
    funcs: HashMap<String, &'a TFunc>,
    base_dir: PathBuf,
    sink: Option<Arc<Mutex<DevSink>>>,
    core_imports: &'a HashMap<String, String>,
    globals: HashMap<String, CtValue>,
    allow_impure: bool,
    impure_depth: usize,
    runtime_execution: bool,
    prefer_tir_calls: bool,
    repl_mode: bool,
    struct_fields: HashMap<String, Vec<(String, bool)>>,
    struct_field_types: HashMap<String, Vec<(String, Type)>>,
    codec_migrations: HashMap<String, TIR::TCodecMigrationPlan>,
    distinct_bases: HashMap<String, Type>,
    distinct_ranges: HashMap<String, (i64, i64)>,
    spawn_lambdas: &'a [TJitSpawnLambda],
    runtime: Arc<Mutex<EvalRuntime<'a>>>,
}

impl EvalRuntime<'_> {
    fn new() -> Self {
        Self {
            callables: Vec::new(),
            streams: Vec::new(),
            shared_values: Vec::new(),
            shared_guards: Vec::new(),
            shared_conditions: Vec::new(),
            clocks: Vec::new(),
            task_groups: Vec::new(),
            tasks: Vec::new(),
            completion_order: AtomicU64::new(0),
        }
    }
}

#[derive(Clone)]
struct YieldConsumer<'a> {
    var: String,
    body: &'a [TStmt],
}

impl<'a> EvalCtx<'a> {
    fn task_wait_cancel_check(&self) -> Result<(), Diagnostic> {
        if self
            .task_cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(Ordering::Acquire))
        {
            Err(Diagnostic::error(
                "TASK_CANCELLED",
                "task cancelled".to_string(),
                "the owning taskgroup stopped this task".to_string(),
                String::new(),
                Some(self.span()),
            ))
        } else {
            Ok(())
        }
    }

    fn task_config(&self) -> EvalTaskConfig<'a> {
        EvalTaskConfig {
            funcs: self.funcs.clone(),
            base_dir: self.base_dir.clone(),
            sink: self.sink.clone(),
            core_imports: self.core_imports,
            globals: self.globals.clone(),
            allow_impure: self.allow_impure,
            impure_depth: self.impure_depth,
            runtime_execution: self.runtime_execution,
            prefer_tir_calls: self.prefer_tir_calls,
            repl_mode: self.repl_mode,
            struct_fields: self.struct_fields.clone(),
            struct_field_types: self.struct_field_types.clone(),
            codec_migrations: self.codec_migrations.clone(),
            distinct_bases: self.distinct_bases.clone(),
            distinct_ranges: self.distinct_ranges.clone(),
            spawn_lambdas: self.spawn_lambdas,
            runtime: self.runtime.clone(),
        }
    }

    fn run_task_job(config: EvalTaskConfig<'a>, job: EvalTaskJob<'a>) {
        let mut ctx = EvalCtx {
            funcs: config.funcs,
            base_dir: config.base_dir,
            fuel: DEV_FUEL,
            sink: config.sink,
            core_imports: config.core_imports,
            globals: config.globals,
            allow_impure: config.allow_impure,
            impure_depth: config.impure_depth,
            runtime_execution: config.runtime_execution,
            prefer_tir_calls: config.prefer_tir_calls,
            repl_mode: config.repl_mode,
            pending_return: None,
            deferred_closes: Vec::new(),
            pending_flow: None,
            collecting_items: Vec::new(),
            call_depth: 0,
            emitted_fragments: None,
            embed_inputs: None,
            struct_fields: config.struct_fields,
            struct_field_types: config.struct_field_types,
            codec_migrations: config.codec_migrations,
            distinct_bases: config.distinct_bases,
            distinct_ranges: config.distinct_ranges,
            switch_subject: None,
            runtime: config.runtime.clone(),
            shared_transactions: Vec::new(),
            spawn_lambdas: config.spawn_lambdas,
            task_sender: None,
            task_cancel: Some(job.cancel),
            yield_consumer: None,
            yield_scope: None,
            scope_guards: Vec::new(),
            shared_guards: Vec::new(),
            txn_stack: Vec::new(),
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
        };
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
                    .ok_or_else(|| unsupported("taskgroup handle", self.span()))?,
            ),
            None => None,
        };
        let lam = self
            .spawn_lambdas
            .get(site)
            .ok_or_else(|| unsupported("spawn body", self.span()))?;
        let mut child = HashMap::new();
        for capture in &lam.captures {
            child.insert(
                capture.name.clone(),
                scope
                    .get(&capture.name)
                    .cloned()
                    .or_else(|| self.globals.get(&capture.name).cloned())
                    .unwrap_or(CtValue::Unit),
            );
        }
        let sender = self
            .task_sender
            .as_ref()
            .ok_or_else(|| unsupported("spawn outside a taskgroup", self.span()))?;
        let (completion, receiver) = mpsc::sync_channel(1);
        let completion_order = Arc::new(OnceLock::new());
        let cancel = Arc::new(AtomicBool::new(false));
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let task = runtime.tasks.len();
        runtime.tasks.push(Some(EvalTask {
            completion: receiver,
            completion_order: completion_order.clone(),
            cancel: cancel.clone(),
        }));
        if let Some(group) = group {
            runtime.task_groups[group].push(task);
        }
        drop(runtime);
        sender
            .send(EvalTaskJob {
                lambda: lam,
                captured: child,
                completion,
                completion_order,
                cancel,
            })
            .map_err(|_| unsupported("closed taskgroup", self.span()))?;
        Ok(CtValue::Struct {
            type_name: "__JetTirTask".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(task as i64))],
        })
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

    fn new_taskgroup(&mut self) -> CtValue {
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let index = runtime.task_groups.len();
        runtime.task_groups.push(Vec::new());
        CtValue::Struct {
            type_name: "__JetTirTaskGroup".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(index as i64))],
        }
    }

    fn take_task_entry(&mut self, value: &CtValue) -> Result<EvalTask, Diagnostic> {
        let index = Self::task_index(value)
            .ok_or_else(|| unsupported("task receiver", self.span()))?;
        self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .tasks
            .get_mut(index)
            .and_then(Option::take)
            .ok_or_else(|| unsupported("task already joined", self.span()))
    }

    pub(super) fn take_task(&mut self, value: &CtValue) -> Result<CtValue, Diagnostic> {
        self.task_wait_cancel_check()?;
        let task = self.take_task_entry(value)?;
        let result = task.completion
            .recv()
            .map_err(|_| unsupported("task completion", self.span()))?
            .result;
        self.task_wait_cancel_check()?;
        result
    }

    pub(super) fn task_select(
        &mut self,
        values: &[CtValue],
        mode: crate::task_group::JetTaskSelectMode,
    ) -> Result<CtValue, Diagnostic> {
        let tasks = values
            .iter()
            .map(|value| self.take_task_entry(value).map(Some))
            .collect::<Result<Vec<_>, _>>()?;
        if tasks.is_empty() {
            return Err(unsupported("empty taskgroup combinator", self.span()));
        }
        select_eval_tasks(tasks, mode, self.span(), || self.task_wait_cancel_check()).map(
            |mut values| {
                if matches!(mode, crate::task_group::JetTaskSelectMode::All) {
                    CtValue::List(values)
                } else {
                    values.pop().expect("race/any result missing")
                }
            },
        )
    }

    fn close_taskgroup(&mut self, index: usize) -> Result<(), Diagnostic> {
        let span = self.span();
        let children = {
            let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
            std::mem::take(
                runtime
                    .task_groups
                    .get_mut(index)
                    .ok_or_else(|| unsupported("taskgroup handle", span))?,
            )
        };
        let mut first = None;
        {
            let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
            for child in &children {
                if let Some(Some(task)) = runtime.tasks.get(*child) {
                    task.cancel.store(true, Ordering::Release);
                }
            }
        }
        for child in children {
            let task = self
                .runtime
                .lock()
                .expect("evaluator runtime poisoned")
                .tasks
                .get_mut(child)
                .and_then(Option::take);
            let result = task.and_then(|task| task.completion.recv().ok());
            if let Some(EvalTaskCompletion {
                result: Err(error),
                ..
            }) = result
            {
                if first.is_none() {
                    first = Some(error);
                }
            }
        }
        match first {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub(crate) fn span(&self) -> Span {
        Span::new(0, 0)
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
            return Err(Diagnostic::error(code, what, why, fix, Some(self.span())));
        }
        self.fuel -= 1;
        Ok(())
    }

    pub(crate) fn run_func(
        &mut self,
        func: &'a TFunc,
        args: Vec<CtValue>,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        if self.call_depth > 64 {
            self.fuel = 0;
            self.burn()?;
            unreachable!("burn with fuel 0 always errors");
        }
        self.call_depth += 1;
        let guard_mark = self.scope_guards.len();
        for (i, (name, _, _)) in func.params.iter().enumerate() {
            let jet = name.strip_prefix("user_").unwrap_or(name.as_str());
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
        self.call_depth -= 1;
        match (result, cleanup_result) {
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            (Ok(value), Ok(())) => Ok(value),
        }
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

    fn store_stream(&mut self, func: &'a TFunc, args: Vec<CtValue>) -> CtValue {
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let index = runtime.streams.len() as i64;
        runtime.streams.push(EvalStream { func, args });
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
        let (lambda, named, mut captured) = {
            let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
            match runtime.callables.get(index) {
                Some(EvalCallable::Lambda { lambda, captured }) => {
                    (Some(*lambda), None, captured.clone())
                }
                Some(EvalCallable::Named(name)) => (None, Some(*name), HashMap::new()),
                None => return Err(unsupported("calling an unknown function value", self.span())),
            }
        };
        if let Some(lambda) = lambda {
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
            return result;
        }
        let name = named.expect("callable target");
        let func = self
            .funcs
            .get(name)
            .copied()
            .ok_or_else(|| unsupported(&format!("callable function `{name}`"), self.span()))?;
        let mut child = HashMap::new();
        self.run_func(func, args, &mut child)
    }
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

fn seed_fragment_funcs(cx: &mut Cx, funcs: &HashMap<String, &Func>) {
    for (name, function) in funcs {
        cx.fn_type_params.insert(
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
                effect_bound: None,
            },
        );
    }
}

/// Lower one expression for the evaluator (comptime / REPL fragments).
pub fn lower_expr_for_eval(
    expr: &Expr,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    distinct_ranges: &HashMap<String, Option<(i64, i64)>>,
    distinct_bases: &HashMap<String, crate::AST::Type>,
) -> Result<TExpr, Diagnostic> {
    let mut cx = empty_cx();
    seed_fragment_distinct_types(&mut cx, distinct_ranges, distinct_bases);
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
        Ok(tir) => Ok(tir),
        Err(_) => Err(unsupported("this expression", Span::new(0, 0))),
    }
}

/// Lower a statement list for the evaluator.
pub fn lower_stmts_for_eval(
    stmts: &[Stmt],
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    distinct_ranges: &HashMap<String, Option<(i64, i64)>>,
    distinct_bases: &HashMap<String, crate::AST::Type>,
) -> Result<Vec<TStmt>, Diagnostic> {
    let mut cx = empty_cx();
    seed_fragment_distinct_types(&mut cx, distinct_ranges, distinct_bases);
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
        Ok(tir) => Ok(tir),
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

fn collect_struct_fields(bundle: &ProgramBundle) -> HashMap<String, Vec<(String, bool)>> {
    let mut out = HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let crate::AST::Item::Struct(s) = item {
                out.insert(
                    s.name.clone(),
                    s.fields
                        .iter()
                        .map(|f| (f.name.clone(), f.redact))
                        .collect(),
                );
            }
        }
    }
    out
}

fn collect_struct_field_types(
    bundle: &ProgramBundle,
) -> HashMap<String, Vec<(String, crate::AST::Type)>> {
    let mut out = HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let crate::AST::Item::Struct(s) = item {
                out.insert(
                    s.name.clone(),
                    s.fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect(),
                );
            }
        }
    }
    out
}

fn normalize_struct_field_types(
    structs: &HashMap<String, &crate::AST::StructDef>,
) -> HashMap<String, Vec<(String, crate::AST::Type)>> {
    structs
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
        .collect()
}

fn program_struct_field_types(
    program: &JitProgram,
) -> HashMap<String, Vec<(String, crate::AST::Type)>> {
    program
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
                            name.strip_prefix("user_").unwrap_or(name).to_string(),
                            ty.clone(),
                        )
                    })
                    .collect(),
            ))
        })
        .collect()
}

pub fn run_program(
    program: &JitProgram,
    base_dir: &Path,
    sink: &mut DevSink,
    globals: HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
) -> Result<CtValue, Diagnostic> {
    run_program_with_structs(
        program,
        base_dir,
        sink,
        globals,
        core_imports,
        allow_impure,
        HashMap::new(),
        HashMap::new(),
    )
}

pub fn run_program_with_structs(
    program: &JitProgram,
    base_dir: &Path,
    sink: &mut DevSink,
    globals: HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
    struct_fields: HashMap<String, Vec<(String, bool)>>,
    mut struct_field_types: HashMap<String, Vec<(String, crate::AST::Type)>>,
) -> Result<CtValue, Diagnostic> {
    // Fresh EventLite stores per whole-program run (REPL / warm cache / workers).
    crate::Comptime::reset_event_lite();
    let _browser_session = browser::SessionGuard::new();
    for (name, fields) in program_struct_field_types(program) {
        struct_field_types.entry(name).or_insert(fields);
    }
    let funcs = program_funcs(program);
    let entry = funcs.get(&program.entry).copied().ok_or_else(|| {
        Diagnostic::error(
            "E2201",
            format!("entry `{}` missing from lowered TIR", program.entry),
            "the interpreter needs the selected entry function in the TIR program".to_string(),
            "report this as a compiler bug".to_string(),
            None,
        )
    })?;
    let shared_sink = Arc::new(Mutex::new(std::mem::take(sink)));
    let mut ctx = EvalCtx {
        funcs,
        base_dir: base_dir.to_path_buf(),
        fuel: DEV_FUEL,
        sink: Some(shared_sink.clone()),
        core_imports,
        globals,
        allow_impure,
        // Runtime `run_bundle` / deopt: ambient Tier-2 I/O matches AOT `jet run`.
        // Comptime purity still uses eval_expr/eval_block with explicit depths.
        impure_depth: if allow_impure { 1 } else { 0 },
        runtime_execution: true,
        prefer_tir_calls: false,
        repl_mode: false,
        pending_return: None,
        deferred_closes: Vec::new(),
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        emitted_fragments: None,
        embed_inputs: None,
        struct_fields,
        struct_field_types,
        codec_migrations: program.codec_migrations.clone(),
        distinct_bases: program.distinct_bases.clone(),
        distinct_ranges: program.distinct_ranges.clone(),
        switch_subject: None,
        runtime: Arc::new(Mutex::new(EvalRuntime::new())),
        shared_transactions: Vec::new(),
        spawn_lambdas: &program.spawn_lambdas,
        task_sender: None,
        task_cancel: None,
        yield_consumer: None,
        yield_scope: None,
        scope_guards: Vec::new(),
        shared_guards: Vec::new(),
        txn_stack: Vec::new(),
    };
    let mut scope = HashMap::new();
    let result = ctx.run_func(entry, Vec::new(), &mut scope);
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
    let _browser_session = browser::SessionGuard::new();
    let funcs = program_funcs(program);
    let func = funcs.get(name).copied().ok_or_else(|| {
        Diagnostic::error(
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
        fuel: DEV_FUEL,
        sink: Some(shared_sink.clone()),
        core_imports: &core_imports,
        globals: HashMap::new(),
        allow_impure: true,
        // Runtime deopt is not comptime: open Tier-2 ambient I/O so `jet run`
        // matches AOT for env/fs/process (D-LENS-RUN2 / #778).
        impure_depth: 1,
        runtime_execution: true,
        prefer_tir_calls: program.canonical_calls.contains(name),
        repl_mode: false,
        pending_return: None,
        deferred_closes: Vec::new(),
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        emitted_fragments: None,
        embed_inputs: None,
        struct_fields: HashMap::new(),
        struct_field_types: program_struct_field_types(program),
        codec_migrations: program.codec_migrations.clone(),
        distinct_bases: program.distinct_bases.clone(),
        distinct_ranges: program.distinct_ranges.clone(),
        switch_subject: None,
        runtime: Arc::new(Mutex::new(EvalRuntime::new())),
        shared_transactions: Vec::new(),
        spawn_lambdas: &program.spawn_lambdas,
        task_sender: None,
        task_cancel: None,
        yield_consumer: None,
        yield_scope: None,
        scope_guards: Vec::new(),
        shared_guards: Vec::new(),
        txn_stack: Vec::new(),
    };
    let mut scope = HashMap::new();
    let result = ctx.run_func(func, args, &mut scope);
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
            eval_expr: eval_expr_hook,
            eval_block: eval_block_hook,
        });
    });
}

fn run_bundle(
    bundle: &ProgramBundle,
    sink: &mut DevSink,
    allow_impure: bool,
) -> Result<CtValue, Diagnostic> {
    let program = lower_interp_program(bundle).ok_or_else(|| {
        Diagnostic::error(
            "E2201",
            "`jet dev` needs a `run` function to run".to_string(),
            "`jet dev` runs a program; a library with no `run` has nothing to execute".to_string(),
            "add `fn run() { … }`, or use `jet check <file>`".to_string(),
            None,
        )
    })?;
    let mut globals = HashMap::new();
    let mut core_imports = HashMap::new();
    let persist = jet_foundation::Persist::prepare_bundle(bundle);
    for msg in &persist.messages {
        // Dev-tier only: surface reset / migration notes on stderr.
        eprintln!("{msg}");
    }
    for module in &bundle.modules {
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
                        .entry(format!("user_{}", c.name))
                        .or_insert(v);
                }
            }
        }
        for imp in &module.imports {
            if let Some(core_module) = imp.core_module_path() {
                core_imports
                    .entry(imp.import_alias())
                    .or_insert(core_module);
            }
        }
    }
    run_program_with_structs(
        &program,
        &bundle.project_root,
        sink,
        globals,
        &core_imports,
        allow_impure,
        collect_struct_fields(bundle),
        collect_struct_field_types(bundle),
    )
}

fn eval_expr_hook(
    req: &mut Comptime::TirBridge::ExprEvalRequest<'_>,
) -> Result<CtValue, Diagnostic> {
    let tir = lower_expr_for_eval(
        req.expr,
        req.funcs,
        req.globals,
        req.core_imports,
        req.distinct_ranges,
        req.distinct_bases,
    )?;
    let mut cx = empty_cx();
    seed_fragment_distinct_types(&mut cx, req.distinct_ranges, req.distinct_bases);
    seed_fragment_funcs(&mut cx, req.funcs);
    cx.struct_fields = normalize_struct_field_types(req.structs);
    cx.type_names.extend(req.structs.keys().cloned());
    cx.core_imports = req.core_imports.clone();
    let lowered: Vec<TFunc> = req
        .funcs
        .iter()
        .filter_map(|(name, f)| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::Codegen::TIR::with_eval_fragment(|| {
                    let mut lowered = match name.rsplit_once("::") {
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
    let funcs: HashMap<String, &TFunc> = lowered.iter().map(|f| (f.name.clone(), f)).collect();
    let base_dir = req.base_dir.to_path_buf();
    let fuel = req.fuel;
    let core_imports = req.core_imports;
    let globals = req.globals.clone();
    let allow_impure = req.allow_impure;
    let impure_depth = req.initial_impure_depth;
    let repl_mode = req.repl_mode;
    let mut sink_target = req.sink.take();
    let sink = sink_target
        .as_deref_mut()
        .map(|sink| Arc::new(Mutex::new(std::mem::take(sink))));
    let emitted_fragments = req.emitted_fragments.take();
    let embed_inputs = req.embed_inputs.take();
    let mut ctx = EvalCtx {
        funcs,
        base_dir,
        fuel,
        sink,
        core_imports,
        globals: globals.clone(),
        allow_impure,
        impure_depth,
        runtime_execution: false,
        prefer_tir_calls: false,
        repl_mode,
        pending_return: None,
        deferred_closes: Vec::new(),
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        emitted_fragments,
        embed_inputs,
        struct_fields: HashMap::new(),
        struct_field_types: normalize_struct_field_types(req.structs),
        codec_migrations: HashMap::new(),
        distinct_bases: HashMap::new(),
        distinct_ranges: HashMap::new(),
        switch_subject: None,
        runtime: Arc::new(Mutex::new(EvalRuntime::new())),
        shared_transactions: Vec::new(),
        spawn_lambdas: &[],
        task_sender: None,
        task_cancel: None,
        yield_consumer: None,
        yield_scope: None,
        scope_guards: Vec::new(),
        shared_guards: Vec::new(),
        txn_stack: Vec::new(),
    };
    let mut scope = globals;
    let result = ctx.eval_expr(&tir, &mut scope);
    if let (Some(target), Some(shared)) = (sink_target, ctx.sink.as_ref()) {
        *target = std::mem::take(&mut *shared.lock().expect("evaluator sink poisoned"));
    }
    result
}

fn eval_block_hook(
    req: &mut Comptime::TirBridge::BlockEvalRequest<'_>,
) -> Result<Comptime::TirBridge::StmtOutcome, Diagnostic> {
    let tir = lower_stmts_for_eval(
        req.stmts,
        req.funcs,
        req.globals,
        req.core_imports,
        req.distinct_ranges,
        req.distinct_bases,
    )?;
    let mut cx = empty_cx();
    seed_fragment_distinct_types(&mut cx, req.distinct_ranges, req.distinct_bases);
    seed_fragment_funcs(&mut cx, req.funcs);
    cx.core_imports = req.core_imports.clone();
    let lowered: Vec<TFunc> = req
        .funcs
        .iter()
        .filter_map(|(name, f)| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::Codegen::TIR::with_eval_fragment(|| {
                    let mut lowered = TIR::lower_func(f, &cx);
                    lowered.name = name.clone();
                    lowered
                })
            }))
            .ok()
        })
        .collect();
    let funcs: HashMap<String, &TFunc> = lowered.iter().map(|f| (f.name.clone(), f)).collect();
    let base_dir = req.base_dir.to_path_buf();
    let fuel = req.fuel;
    let core_imports = req.core_imports;
    let globals = req.globals.clone();
    let allow_impure = req.allow_impure;
    let impure_depth = req.impure_depth;
    let repl_mode = req.repl_mode;
    let mut sink_target = req.sink.take();
    let sink = sink_target
        .as_deref_mut()
        .map(|sink| Arc::new(Mutex::new(std::mem::take(sink))));
    let emitted_fragments = req.emitted_fragments.take();
    let embed_inputs = req.embed_inputs.take();
    let mut ctx = EvalCtx {
        funcs,
        base_dir,
        fuel,
        sink,
        core_imports,
        globals: globals.clone(),
        allow_impure,
        impure_depth,
        runtime_execution: false,
        prefer_tir_calls: false,
        repl_mode,
        pending_return: None,
        deferred_closes: Vec::new(),
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        emitted_fragments,
        embed_inputs,
        struct_fields: HashMap::new(),
        struct_field_types: normalize_struct_field_types(req.structs),
        codec_migrations: HashMap::new(),
        distinct_bases: HashMap::new(),
        distinct_ranges: HashMap::new(),
        switch_subject: None,
        runtime: Arc::new(Mutex::new(EvalRuntime::new())),
        shared_transactions: Vec::new(),
        spawn_lambdas: &[],
        task_sender: None,
        task_cancel: None,
        yield_consumer: None,
        yield_scope: None,
        scope_guards: Vec::new(),
        shared_guards: Vec::new(),
        txn_stack: Vec::new(),
    };
    let mut scope = globals;
    let outcome = match ctx.exec_stmts(&tir, &mut scope)? {
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
        mpsc, select_eval_tasks, unsupported, Arc, AtomicBool, CtValue, EvalTask,
        EvalTaskCompletion, OnceLock, Span,
    };
    use crate::task_group::JetTaskSelectMode;

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
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn selection_keeps_every_already_ready_completion() {
        let all = select_eval_tasks(
            vec![
                Some(ready_task(1, Ok(CtValue::Int(10)))),
                Some(ready_task(0, Ok(CtValue::Int(20)))),
            ],
            JetTaskSelectMode::All,
            Span::new(0, 0),
            || Ok(()),
        )
        .unwrap();
        assert_eq!(all, [CtValue::Int(10), CtValue::Int(20)]);

        let race = select_eval_tasks(
            vec![
                Some(ready_task(
                    0,
                    Err(unsupported("first completion failed", Span::new(0, 0))),
                )),
                Some(ready_task(1, Ok(CtValue::Int(22)))),
            ],
            JetTaskSelectMode::Race,
            Span::new(0, 0),
            || Ok(()),
        )
        .unwrap();
        assert_eq!(race, [CtValue::Int(22)]);
    }
}
