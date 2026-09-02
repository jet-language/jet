//! Per-function tier classification and `--trace-tiers` (D-LENS-RUN2=A).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use jet_codegen::Codegen::TIR::{
    JitProgram, TCallArg, TCoreClosureKind, TEnumPayload, TExpr, TExprKind, TFnValueKind,
    THostArg, THostCall, TIfCond, TFunc, TFuncKind, TModuleCallForm, TPlace, TRequireKind,
    TStmt, TLambda, TLambdaBody,
};
use jet_foundation::AST::{ProgramBundle, Type};
use jet_foundation::JSON::json_escape;

use super::api_debug::{classify_jit_gap, cranelift_host_supported};
use super::gap::JitGap;
use super::safety::{
    entry_return_supported, resident_safe_func_detail, resident_safe_spawn_lambda,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Cranelift native.
    Native,
    /// Canonical TIR interpreter (reference semantics).
    Interp,
}

#[derive(Debug, Clone)]
pub struct TierRow {
    pub function: String,
    pub tier: Tier,
    pub reason: String,
    pub millis: f64,
}

/// All tier plans recorded by one compiler process.
///
/// `LAST_TRACE` remains the one-plan thread-local handoff used by existing
/// callers. This process aggregate is separate so a scheduled plan or a
/// fallback plan cannot be hidden by a later plan on the same process.
#[derive(Debug, Clone, Default)]
pub struct TierTraceAggregate {
    pub rows: Vec<TierRow>,
    pub native_rows: usize,
    pub interp_rows: usize,
    pub whole_program_deopt: bool,
}

impl TierTraceAggregate {
    fn append(&mut self, rows: &[TierRow]) {
        self.native_rows += rows
            .iter()
            .filter(|row| matches!(row.tier, Tier::Native))
            .count();
        self.interp_rows += rows
            .iter()
            .filter(|row| matches!(row.tier, Tier::Interp))
            .count();
        // A whole-program interpreter plan has no native row. Keep this bit
        // sticky across every plan recorded in the process.
        if !rows.is_empty() && rows.iter().all(|row| matches!(row.tier, Tier::Interp)) {
            self.whole_program_deopt = true;
        }
        self.rows.extend(rows.iter().cloned());
    }

    pub fn to_json(&self) -> String {
        let rows = self
            .rows
            .iter()
            .map(tier_row_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"rows\":[{rows}],\"native_rows\":{},\"interp_rows\":{},\"whole_program_deopt\":{}}}",
            self.native_rows, self.interp_rows, self.whole_program_deopt
        )
    }
}

thread_local! {
    static TRACE_TIERS: RefCell<bool> = const { RefCell::new(false) };
    static LAST_TRACE: RefCell<Vec<TierRow>> = const { RefCell::new(Vec::new()) };
}

static TRACE_AGGREGATE: LazyLock<Mutex<TierTraceAggregate>> =
    LazyLock::new(|| Mutex::new(TierTraceAggregate::default()));

fn trace_aggregate() -> &'static Mutex<TierTraceAggregate> {
    &TRACE_AGGREGATE
}

fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::Native => "native",
        Tier::Interp => "interp",
    }
}

fn tier_row_json(row: &TierRow) -> String {
    let reason = if row.reason.is_empty() {
        "null".to_string()
    } else {
        format!("\"{}\"", json_escape(&row.reason))
    };
    let millis = if row.millis.is_finite() {
        row.millis.to_string()
    } else {
        "null".to_string()
    };
    format!(
        "{{\"function\":\"{}\",\"tier\":\"{}\",\"reason\":{reason},\"millis\":{millis}}}",
        json_escape(&row.function),
        tier_name(row.tier),
    )
}

/// Atomically publish compiler-owned tier facts for the completed process.
pub fn write_trace_sidecar(
    path: &Path,
    aggregate: &TierTraceAggregate,
) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("tier-trace.json");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temporary = parent.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        nonce
    ));
    let result = fs::write(&temporary, aggregate.to_json())
        .and_then(|_| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Drain all process plans while leaving the latest thread-local trace alone.
pub fn take_trace_aggregate() -> TierTraceAggregate {
    let mut aggregate = trace_aggregate()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    std::mem::take(&mut *aggregate)
}

// Keep the latest-plan state and the process aggregate independent.

/// Expert flag: `jet run --trace-tiers` / `jet dev --trace-tiers`.
pub fn set_trace_tiers(enabled: bool) {
    TRACE_TIERS.with(|slot| *slot.borrow_mut() = enabled);
}

pub fn trace_tiers_enabled() -> bool {
    TRACE_TIERS.with(|slot| *slot.borrow())
}

pub fn take_last_trace() -> Vec<TierRow> {
    LAST_TRACE.with(|slot| std::mem::take(&mut *slot.borrow_mut()))
}

/// Move tier rows from a compiler worker back to its caller thread.
pub fn publish_trace(rows: Vec<TierRow>) {
    LAST_TRACE.with(|slot| *slot.borrow_mut() = rows);
}

pub fn record_trace(rows: Vec<TierRow>) {
    {
        let mut aggregate = trace_aggregate()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        aggregate.append(&rows);
    }
    if trace_tiers_enabled() {
        for row in &rows {
            let tier = match row.tier {
                Tier::Native => "tier1 native",
                Tier::Interp => "tier0 interp",
            };
            if row.reason.is_empty() {
                eprintln!("{:<24} {tier} ({:.3}ms)", row.function, row.millis);
            } else {
                eprintln!(
                    "{:<24} {tier} ({}) ({:.3}ms)",
                    row.function, row.reason, row.millis
                );
            }
        }
    }
    LAST_TRACE.with(|slot| *slot.borrow_mut() = rows);
}

/// True when a deopted function's ABI can round-trip through host i64 slots.
pub(crate) fn deopt_marshallable(tir: &TFunc) -> bool {
    if !matches!(tir.kind, TFuncKind::TopLevel) {
        return false;
    }
    if !tir.params.iter().all(|(_, ty, _)| marshallable_ty(ty)) {
        return false;
    }
    match &tir.ret {
        None => true,
        Some(ty) => marshallable_return_ty(ty),
    }
}

fn marshallable_ty(ty: &Type) -> bool {
    match ty {
        Type::Int
        | Type::IntN { .. }
        | Type::InlineRange { .. }
        | Type::String
        | Type::Bool
        | Type::Char => true,
        Type::Named(n) if matches!(n.as_str(), "Int" | "String" | "Bool" | "Char" | "Unit") => true,
        _ => false,
    }
}

fn marshallable_return_ty(ty: &Type) -> bool {
    marshallable_ty(ty)
        || matches!(
            ty,
            Type::Result { ok, err }
                if marshallable_ty(ok)
                    && matches!(
                        err.as_ref(),
                        Type::Named(name)
                            if matches!(
                                name.as_str(),
                                jet_foundation::Syntax::TYPE_ERR
                                    | jet_foundation::Syntax::TYPE_NEVER
                            )
                    )
        )
}

fn nominal_type_key(ty: &Type) -> Option<String> {
    match ty {
        Type::Named(name) => Some(name.clone()),
        Type::Apply { .. } => Some(ty.name()),
        Type::Tagged { inner, .. } => nominal_type_key(inner),
        _ => None,
    }
}
const SERDE_DEMAND_PREFIX: &str = "\0jet-serde-demand:";

fn serde_demand_marker(owner: &Type, method: &str) -> Option<String> {
    if !matches!(owner, Type::Named(_) | Type::Apply { .. }) {
        return None;
    }
    Some(format!(
        "{SERDE_DEMAND_PREFIX}{}",
        jet_codegen::Codegen::TIR::generic_method_instance_key(owner, method, &[])
    ))
}

fn insert_core_serde_demand_markers(expr: &TExpr, calls: &mut HashSet<String>) {
    let TExprKind::CoreCall {
        module,
        method,
        args,
        ..
    } = &expr.kind
    else {
        return;
    };
    let encoding = matches!(
        module.as_str(),
        "core.sys"
            | "core.encoding.json"
            | "core.encoding.toml"
            | "core.encoding.yaml"
            | "core.encoding.csv"
            | "core.encoding.cbor"
    );
    if !encoding {
        return;
    }
    if matches!(
        method.as_str(),
        "to_string" | "to_string_pretty" | "to_bytes" | "to_bytes_canonical"
    ) {
        if let Some(arg) = args.first() {
            if let Some(marker) = serde_demand_marker(&arg.ty, "encode") {
                calls.insert(marker);
            }
        }
    }
    if matches!(method.as_str(), "decode" | "query") {
        if let Type::Result { ok, .. } = &expr.ty {
            let decode_ty = if method == "query" {
                match ok.as_ref() {
                    Type::List(inner) => inner.as_ref(),
                    other => other,
                }
            } else {
                ok.as_ref()
            };
            if let Some(marker) = serde_demand_marker(decode_ty, "decode") {
                calls.insert(marker);
            }
        }
    }
}

/// One canonical predicate for codec bodies retained in the TIR function graph.
///
/// Both compiler-written structural codecs and user-written `impl T.Encode` /
/// `impl T.Decode` methods carry the trait-method marker. Generic structural
/// codecs are lowered as synthetic owner methods, where the owner-qualified
/// key is the remaining provenance fact. Codec-demand edges must retain either
/// kind; filtering to synthetic bodies makes a generated parent silently lose
/// its explicit hand-codec child.
fn is_serde_codec(func: &TFunc) -> bool {
    match &func.kind {
        TFuncKind::TraitMethod {
            serde: Some(
                jet_codegen::Codegen::TIR::SerdeCodec::Encode
                | jet_codegen::Codegen::TIR::SerdeCodec::Decode,
            ),
            ..
        } => true,
        TFuncKind::Method { owner_type, .. } => {
            if !func.synthetic {
                return false;
            }
            let Some((_, method)) = func.name.rsplit_once("::") else {
                return false;
            };
            matches!(method, "encode" | "decode")
                && func.name
                    == jet_codegen::Codegen::TIR::generic_method_instance_key(
                        owner_type,
                        method,
                        &[],
                    )
        }
        _ => false,
    }
}

fn collect_call_args(args: &[TCallArg], calls: &mut HashSet<String>) {
    for arg in args {
        collect_expr_callees(&arg.value, calls);
    }
}

fn collect_place_callees(place: &TPlace, calls: &mut HashSet<String>) {
    if let TPlace::Expr(expr) = place {
        collect_expr_callees(expr, calls);
    }
}

fn collect_lambda_callees(lambda: &TLambda, calls: &mut HashSet<String>) {
    match &lambda.executable {
        TLambdaBody::Expr(expr) => collect_expr_callees(expr, calls),
        TLambdaBody::Block(body) => {
            for stmt in body.iter() {
                collect_stmt_callees(stmt, calls);
            }
        }
        TLambdaBody::SharedBlock(body) => {
            for stmt in body.iter() {
                collect_stmt_callees(stmt, calls);
            }
        }
    }
}

fn collect_host_arg_callees(arg: &THostArg, calls: &mut HashSet<String>) {
    match arg {
        THostArg::Expr(expr) | THostArg::Borrow(expr) => collect_expr_callees(expr, calls),
        THostArg::Lambda(lambda) => collect_lambda_callees(lambda, calls),
    }
}

fn collect_host_callees(host: &THostCall, calls: &mut HashSet<String>) {
    match host {
        THostCall::Helper { args, .. } => {
            for arg in args {
                collect_host_arg_callees(arg, calls);
            }
        }
        THostCall::Method { recv, args, .. } => {
            collect_expr_callees(recv, calls);
            for arg in args {
                collect_expr_callees(arg, calls);
            }
        }
        THostCall::CarrierFact { recv, .. }
        | THostCall::OptionProbe { inner: recv, .. }
        | THostCall::TupleIndex { base: recv, .. }
        | THostCall::YieldSend { value: recv }
        | THostCall::ExpectSnapshot { value: recv, .. }
        | THostCall::TypedText { arg: recv, .. }
        | THostCall::CellGuardProject { recv, .. } => collect_expr_callees(recv, calls),
        THostCall::FixedListIndex { base, index, .. } => {
            collect_expr_callees(base, calls);
            collect_expr_callees(index, calls);
        }
        THostCall::StrMatchScan { subject, .. } | THostCall::BinMatchScan { subject, .. } => {
            collect_expr_callees(subject, calls)
        }
        THostCall::GcEdit {
            edit, index_temp, ..
        } => {
            collect_expr_callees(edit, calls);
            if let Some((_, expr)) = index_temp {
                collect_expr_callees(expr, calls);
            }
        }
        THostCall::FnName(name) => {
            calls.insert(name.clone());
        }
        THostCall::TypedTextInterp { holes, .. } => {
            for hole in holes {
                collect_expr_callees(hole, calls);
            }
        }
        THostCall::EnvSet { name, value, .. } => {
            collect_expr_callees(name, calls);
            collect_expr_callees(value, calls);
        }
        THostCall::ExpiringSecretNew {
            value,
            duration,
            clock,
            ..
        }
        | THostCall::ExpiringValueNew {
            value,
            duration,
            clock,
        } => {
            collect_expr_callees(value, calls);
            collect_expr_callees(duration, calls);
            collect_expr_callees(clock, calls);
        }
        THostCall::CCallback { lambda, .. } => collect_lambda_callees(lambda, calls),
        THostCall::GcRead { .. }
        | THostCall::SwitchSubjectField { .. }
        | THostCall::SwitchSubjectValue
        | THostCall::NumericBounds { .. } => {}
    }
}

fn collect_require_callees(kind: &TRequireKind, calls: &mut HashSet<String>) {
    match kind {
        TRequireKind::Require { cond, msg } => {
            collect_expr_callees(cond, calls);
            if let Some(msg) = msg {
                collect_expr_callees(msg, calls);
            }
        }
        TRequireKind::RequireEq { left, right } => {
            collect_expr_callees(left, calls);
            collect_expr_callees(right, calls);
        }
        TRequireKind::Panic { msg } => collect_expr_callees(msg, calls),
    }
}

fn collect_if_cond_callees(cond: &TIfCond, calls: &mut HashSet<String>) {
    match cond {
        TIfCond::Plain(expr)
        | TIfCond::IfLet { subj: expr, .. }
        | TIfCond::IsNone { subj: expr }
        | TIfCond::Matches { subj: expr, .. } => collect_expr_callees(expr, calls),
        TIfCond::And { left, right } => {
            collect_if_cond_callees(left, calls);
            collect_if_cond_callees(right, calls);
        }
        TIfCond::WithPrelude { prelude, cond } => {
            for stmt in prelude {
                collect_stmt_callees(stmt, calls);
            }
            collect_if_cond_callees(cond, calls);
        }
    }
}

fn collect_contract_callees(contract: &jet_codegen::Codegen::TIR::TContract, calls: &mut HashSet<String>) {
    collect_expr_callees(&contract.condition, calls);
    collect_expr_callees(&contract.message, calls);
}

fn collect_core_closure_callees(kind: &TCoreClosureKind, calls: &mut HashSet<String>) {
    match kind {
        TCoreClosureKind::Spawn {
            group,
            label,
            executable,
            ..
        } => {
            if let Some(label) = label {
                calls.insert(label.clone());
            }
            if let Some(group) = group {
                collect_expr_callees(group, calls);
            }
            collect_lambda_callees(executable, calls);
        }
        TCoreClosureKind::Serve { addr, .. } => collect_expr_callees(addr, calls),
        TCoreClosureKind::OnInterrupt { callback } => collect_expr_callees(callback, calls),
        TCoreClosureKind::Guard { executable, .. }
        | TCoreClosureKind::OnCommit { executable, .. }
        | TCoreClosureKind::OnRollback { executable, .. }
        | TCoreClosureKind::ReactiveDerived { executable, .. }
        | TCoreClosureKind::ReactiveEffect { executable, .. }
        | TCoreClosureKind::UiReactiveRender { executable, .. } => {
            collect_lambda_callees(executable, calls)
        }
        TCoreClosureKind::UiButtonOnClick {
            label, executable, ..
        } => {
            collect_expr_callees(label, calls);
            collect_lambda_callees(executable, calls);
        }
    }
}

fn collect_fn_value_callees(kind: &TFnValueKind, calls: &mut HashSet<String>) {
    match kind {
        TFnValueKind::NamedFn {
            name, lambda, ..
        } => {
            if let Some(name) = name {
                calls.insert(name.clone());
            }
            if let Some(lambda) = lambda {
                collect_lambda_callees(lambda, calls);
            }
        }
        TFnValueKind::Policy {
            policy_args, callee, ..
        } => {
            collect_call_args(policy_args, calls);
            collect_expr_callees(callee, calls);
        }
        TFnValueKind::Call { callee, args } => {
            collect_expr_callees(callee, calls);
            collect_call_args(args, calls);
        }
        TFnValueKind::Interrupt { value } => collect_expr_callees(value, calls),
    }
}

fn collect_enum_payload_callees(payload: &TEnumPayload, calls: &mut HashSet<String>) {
    match payload {
        TEnumPayload::Unit => {}
        TEnumPayload::Positional(args) => {
            for arg in args {
                collect_expr_callees(&arg.value, calls);
            }
        }
        TEnumPayload::Named(fields) => {
            for (_, arg) in fields {
                collect_expr_callees(&arg.value, calls);
            }
        }
    }
}

fn collect_expr_callees(expr: &TExpr, calls: &mut HashSet<String>) {
    // Keep the graph aligned with lower_demanded_generic_methods: every
    // concrete Apply expression seeds both codec instances, while build_callers
    // admits these markers only for an already-designated canonical owner.
    if matches!(&expr.ty, Type::Apply { .. }) {
        for method in ["encode", "decode"] {
            if let Some(marker) = serde_demand_marker(&expr.ty, method) {
                calls.insert(marker);
            }
        }
    }
    match &expr.kind {
        TExprKind::IntLit(..)
        | TExprKind::FloatLit(_)
        | TExprKind::BoolLit(_)
        | TExprKind::CharLit(_)
        | TExprKind::Local(_)
        | TExprKind::Unit
        | TExprKind::DefaultLit
        | TExprKind::Uninit
        | TExprKind::CtLit(_)
        | TExprKind::ConstRef(_)
        | TExprKind::DataEntriesToMap(_)
        | TExprKind::ResourceTake(_)
        | TExprKind::AllocNew { .. }
        | TExprKind::SelectStart
        | TExprKind::Todo { .. }
        | TExprKind::Unreachable { .. }
        | TExprKind::Absent => {}
        TExprKind::StrLit(parts) => {
            for part in parts {
                if let jet_codegen::Codegen::TIR::TStrPart::Interp(expr, _) = part {
                    collect_expr_callees(expr, calls);
                }
            }
        }
        TExprKind::InlineBlock(body) => {
            for stmt in body {
                collect_stmt_callees(stmt, calls);
            }
        }
        TExprKind::HostCall(host) => collect_host_callees(host, calls),
        TExprKind::Call { name, args, .. } => {
            calls.insert(name.clone());
            collect_call_args(args, calls);
        }
        TExprKind::DistinctCtor { arg, .. }
        | TExprKind::RangeCheckedCtor { arg, .. }
        | TExprKind::DistinctConvert { arg, .. }
        | TExprKind::Print(arg)
        | TExprKind::Drop(arg)
        | TExprKind::Close(arg)
        | TExprKind::ResourceNew(arg)
        | TExprKind::Deref(arg)
        | TExprKind::RawOf(arg)
        | TExprKind::DistinctRaw(arg)
        | TExprKind::Present(arg)
        | TExprKind::Ok(arg)
        | TExprKind::Err(arg)
        | TExprKind::Clone(arg)
        | TExprKind::ExplicitCopy(arg)
        | TExprKind::MaterializeView(arg)
        | TExprKind::LayoutLit { inner: arg } => collect_expr_callees(arg, calls),
        TExprKind::UnitConvert {
            arg, rounding, ..
        } => {
            collect_expr_callees(arg, calls);
            if let Some((_, rounding)) = rounding {
                collect_expr_callees(rounding, calls);
            }
        }
        TExprKind::MathBuiltin { args, .. }
        | TExprKind::PreciseBuiltin { args, .. } => {
            for arg in args {
                collect_expr_callees(arg, calls);
            }
        }
        TExprKind::AmbientInput { prompt } => {
            if let Some(prompt) = prompt {
                collect_expr_callees(prompt, calls);
            }
        }
        TExprKind::RequireStop { kind, .. } => collect_require_callees(kind, calls),
        TExprKind::Binary { lhs, rhs, .. } | TExprKind::LayoutCompare { lhs, rhs, .. } => {
            collect_expr_callees(lhs, calls);
            collect_expr_callees(rhs, calls);
        }
        TExprKind::CompareChain { operands, .. } => {
            for operand in operands {
                collect_expr_callees(operand, calls);
            }
        }
        TExprKind::Unary { operand, .. } => collect_expr_callees(operand, calls),
        TExprKind::IncDec { place, .. } => collect_place_callees(place, calls),
        TExprKind::StructLit { fields, .. } => {
            for (_, value, _) in fields {
                collect_expr_callees(value, calls);
            }
        }
        TExprKind::Field { recv, .. }
        | TExprKind::SharedGuardValue { guard: recv, .. }
        | TExprKind::SharedGuardMap { guard: recv, .. }
        | TExprKind::ConditionNotify { condition: recv, .. }
        | TExprKind::NumericMethod { recv, .. } => collect_expr_callees(recv, calls),
        TExprKind::SharedGuardSplit { guard, .. } => collect_expr_callees(guard, calls),
        TExprKind::SharedGuardWait {
            guard,
            condition,
            predicate,
        } => {
            collect_expr_callees(guard, calls);
            collect_expr_callees(condition, calls);
            collect_lambda_callees(predicate, calls);
        }
        TExprKind::PtrFromAddr { addr, .. } => collect_expr_callees(addr, calls),
        TExprKind::EnumLit { payload, .. } => collect_enum_payload_callees(payload, calls),
        TExprKind::JSONLit { arg, .. } | TExprKind::DBValueLit { arg, .. } => {
            if let Some((arg, _)) = arg.as_deref() {
                collect_expr_callees(arg, calls);
            }
        }
        TExprKind::ListLit(items) => {
            for item in items {
                collect_expr_callees(item, calls);
            }
        }
        TExprKind::ListSpread { parts } => {
            for part in parts {
                let item = match part {
                    jet_codegen::Codegen::TIR::ListSpreadPart::Elem(item)
                    | jet_codegen::Codegen::TIR::ListSpreadPart::Spread(item) => item,
                };
                collect_expr_callees(item, calls);
            }
        }
        TExprKind::ColumnarListLit { elems, .. } => {
            for item in elems {
                collect_expr_callees(item, calls);
            }
        }
        TExprKind::ColumnarGather { base, index, .. }
        | TExprKind::ColumnarColumnRead { base, index, .. }
        | TExprKind::Index { base, index, .. }
        | TExprKind::IndexHook { base, index, .. }
        | TExprKind::MathLaneIndex { base, index, .. } => {
            collect_expr_callees(base, calls);
            collect_expr_callees(index, calls);
        }
        TExprKind::PoolSlot { pool, id, .. } => {
            collect_expr_callees(pool, calls);
            collect_expr_callees(id, calls);
        }
        TExprKind::TupleLit { fields, .. } => {
            for (_, value) in fields {
                collect_expr_callees(value, calls);
            }
        }
        TExprKind::MapLit(entries) => {
            for (key, value) in entries {
                collect_expr_callees(key, calls);
                collect_expr_callees(value, calls);
            }
        }
        TExprKind::MathSwizzleRead { recv, .. } => collect_expr_callees(recv, calls),
        TExprKind::Slice {
            base,
            start,
            end,
            range,
            ..
        } => {
            collect_expr_callees(base, calls);
            collect_expr_callees(start, calls);
            collect_expr_callees(end, calls);
            if let Some(range) = range {
                collect_expr_callees(range, calls);
            }
        }
        TExprKind::Borrow { place, .. } => collect_expr_callees(place, calls),
        TExprKind::MethodCall {
            recv, method, args, ..
        } => {
            if let Some(owner) = nominal_type_key(&recv.ty) {
                calls.insert(format!("{owner}::{}", method.name));
            }
            collect_expr_callees(recv, calls);
            collect_call_args(args, calls);
        }
        TExprKind::FnFieldCall { recv, args, .. } => {
            collect_expr_callees(recv, calls);
            collect_call_args(args, calls);
        }
        TExprKind::StaticCall {
            owner,
            owner_type,
            method,
            args,
            ..
        } => {
            let owner_name = owner_type
                .as_ref()
                .and_then(nominal_type_key)
                .or_else(|| match owner {
                    jet_codegen::Codegen::TIR::TStaticOwner::User(name) => Some(name.clone()),
                    jet_codegen::Codegen::TIR::TStaticOwner::Prelude { .. } => None,
                });
            if let Some(owner_name) = owner_name {
                calls.insert(format!("{owner_name}::{}", method.name));
            }
            collect_call_args(args, calls);
        }
        TExprKind::DecodeUnder { segment, inner } => {
            collect_expr_callees(segment, calls);
            collect_expr_callees(inner, calls);
        }
        TExprKind::BuiltinMethod { recv, args, .. } => {
            collect_expr_callees(recv, calls);
            for arg in args {
                collect_expr_callees(arg, calls);
            }
        }
        TExprKind::CoreCall { args, .. } => {
            insert_core_serde_demand_markers(expr, calls);
            for arg in args {
                collect_expr_callees(arg, calls);
            }
        }
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            collect_if_cond_callees(cond, calls);
            for stmt in then_body.iter().chain(else_body.iter()) {
                collect_stmt_callees(stmt, calls);
            }
            collect_expr_callees(then_value, calls);
            collect_expr_callees(else_value, calls);
        }
        TExprKind::Try { inner, note, .. } => {
            collect_expr_callees(inner, calls);
            if let Some(note) = note {
                collect_expr_callees(note, calls);
            }
        }
        TExprKind::OrFallback { value, fallback } => {
            collect_expr_callees(value, calls);
            match fallback {
                jet_codegen::Codegen::TIR::TOrFallback::Value(expr)
                | jet_codegen::Codegen::TIR::TOrFallback::Return(Some(expr))
                | jet_codegen::Codegen::TIR::TOrFallback::Panic { msg: expr, .. } => {
                    collect_expr_callees(expr, calls)
                }
                jet_codegen::Codegen::TIR::TOrFallback::Return(None)
                | jet_codegen::Codegen::TIR::TOrFallback::Break
                | jet_codegen::Codegen::TIR::TOrFallback::Continue
                | jet_codegen::Codegen::TIR::TOrFallback::BreakLabel(_)
                | jet_codegen::Codegen::TIR::TOrFallback::ContinueLabel(_) => {}
            }
        }
        TExprKind::OptField { base, .. } => collect_expr_callees(base, calls),
        TExprKind::Lambda(lambda) => collect_lambda_callees(lambda, calls),
        TExprKind::PatternMatches { subj, .. } => collect_expr_callees(subj, calls),
        TExprKind::OptionLift2 { f, a, b } => {
            collect_expr_callees(f, calls);
            collect_expr_callees(a, calls);
            collect_expr_callees(b, calls);
        }
        TExprKind::ClosureMethod { recv, args, .. } => {
            collect_expr_callees(recv, calls);
            for arg in args {
                collect_expr_callees(arg, calls);
            }
        }
        TExprKind::HostBorrowCallback { callable, .. } => {
            collect_expr_callees(callable, calls);
        }
        TExprKind::NumericBinaryMethod { recv, arg, .. } => {
            collect_expr_callees(recv, calls);
            collect_expr_callees(arg, calls);
        }
        TExprKind::OverflowOpt { lhs, rhs, .. } => {
            collect_expr_callees(lhs, calls);
            collect_expr_callees(rhs, calls);
        }
        TExprKind::HandleMethod { recv, op, args } => {
            match op {
                jet_codegen::Codegen::TIR::THandleOp::SerdeEncode => {
                    if let Some(marker) = serde_demand_marker(&recv.ty, "encode") {
                        calls.insert(marker);
                    }
                }
                jet_codegen::Codegen::TIR::THandleOp::DataTreeDecode(target) => {
                    if let Some(marker) = serde_demand_marker(target, "decode") {
                        calls.insert(marker);
                    }
                }
                _ => {}
            }
            collect_expr_callees(recv, calls);
            for arg in args {
                collect_expr_callees(arg, calls);
            }
        }
        TExprKind::CoreClosureCall { kind } => collect_core_closure_callees(kind, calls),
        TExprKind::TaskGroupAll { tasks }
        | TExprKind::TaskGroupRace { tasks }
        | TExprKind::TaskGroupAny { tasks } => collect_expr_callees(tasks, calls),
        TExprKind::SelectRecv { builder, channel } => {
            collect_expr_callees(builder, calls);
            collect_expr_callees(channel, calls);
        }
        TExprKind::SelectAfter {
            builder,
            duration,
            value,
        } => {
            collect_expr_callees(builder, calls);
            collect_expr_callees(duration, calls);
            if let Some(value) = value {
                collect_expr_callees(value, calls);
            }
        }
        TExprKind::SelectWait { builder, .. } => collect_expr_callees(builder, calls),
        TExprKind::FnValue { kind } => collect_fn_value_callees(kind, calls),
        TExprKind::ModuleCall { form, args, .. } => {
            let target = match form {
                TModuleCallForm::Qualified { rust_mod, rust_fn } => {
                    format!("{rust_mod}::{rust_fn}")
                }
                TModuleCallForm::InlineMangled { mangled } => mangled.clone(),
            };
            calls.insert(target);
            collect_call_args(args, calls);
        }
        TExprKind::ExternCall { args, .. } => {
            for arg in args {
                collect_expr_callees(&arg.value, calls);
            }
        }
    }
}

fn collect_stmt_callees(stmt: &TStmt, calls: &mut HashSet<String>) {
    match stmt {
        TStmt::Contract { contract } => collect_contract_callees(contract, calls),
        TStmt::ContractScope {
            pre, body, post, ..
        } => {
            for contract in pre.iter().chain(post) {
                collect_contract_callees(contract, calls);
            }
            for stmt in body {
                collect_stmt_callees(stmt, calls);
            }
        }
        TStmt::Let { init, .. }
        | TStmt::TupleDestructure { init, .. }
        | TStmt::StructDestructure { init, .. }
        | TStmt::ListDestructure { init, .. }
        | TStmt::ExprStmt(init)
        | TStmt::Return(Some(init))
        | TStmt::BreakValue { value: init, .. } => collect_expr_callees(init, calls),
        TStmt::Return(None) => {}
        TStmt::RefutableBind { init, fallback, .. } => {
            collect_expr_callees(init, calls);
            for stmt in fallback {
                collect_stmt_callees(stmt, calls);
            }
        }
        TStmt::GcEdit {
            index_temp, stmt, ..
        } => {
            if let Some((_, expr)) = index_temp {
                collect_expr_callees(expr, calls);
            }
            collect_stmt_callees(stmt, calls);
        }
        TStmt::SplitViews { owner, .. } => {
            if let Some(owner) = owner {
                collect_expr_callees(owner, calls);
            }
        }
        TStmt::Assign { place, value, .. } => {
            collect_place_callees(place, calls);
            collect_expr_callees(value, calls);
        }
        TStmt::TaskGroup { limit, body, .. } => {
            if let Some(limit) = limit {
                collect_expr_callees(limit, calls);
            }
            for stmt in body {
                collect_stmt_callees(stmt, calls);
            }
        }
        TStmt::DeferClose { close, .. } => collect_expr_callees(close, calls),
        TStmt::If {
            cond,
            then_body,
            else_body,
            ..
        } => {
            collect_if_cond_callees(cond, calls);
            for stmt in then_body {
                collect_stmt_callees(stmt, calls);
            }
            if let Some(else_body) = else_body {
                for stmt in else_body {
                    collect_stmt_callees(stmt, calls);
                }
            }
        }
        TStmt::Loop { body, .. }
        | TStmt::Inline(body)
        | TStmt::DebugOnly(body)
        | TStmt::Unsafe { body, .. }
        | TStmt::SentryPolicy { body, .. }
        | TStmt::Impure(body)
        | TStmt::Region(body)
        | TStmt::Live { body }
        | TStmt::Shield { body } => {
            for stmt in body {
                collect_stmt_callees(stmt, calls);
            }
        }
        TStmt::While { cond, body, .. } => {
            collect_expr_callees(cond, calls);
            for stmt in body {
                collect_stmt_callees(stmt, calls);
            }
        }
        TStmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            collect_stmt_callees(init, calls);
            collect_expr_callees(cond, calls);
            if let Some(step) = step {
                collect_stmt_callees(step, calls);
            }
            for stmt in body {
                collect_stmt_callees(stmt, calls);
            }
        }
        TStmt::Range {
            source,
            start,
            end,
            step,
            body,
            ..
        } => {
            if let Some(source) = source {
                collect_expr_callees(source, calls);
            }
            collect_expr_callees(start, calls);
            collect_expr_callees(end, calls);
            if let Some(step) = step {
                collect_expr_callees(step, calls);
            }
            for stmt in body {
                collect_stmt_callees(stmt, calls);
            }
        }
        TStmt::EnumMatch {
            scrutinee,
            arms,
            else_body,
            ..
        } => {
            collect_expr_callees(scrutinee, calls);
            for arm in arms {
                for stmt in &arm.body {
                    collect_stmt_callees(stmt, calls);
                }
            }
            if let Some(else_body) = else_body {
                for stmt in else_body {
                    collect_stmt_callees(stmt, calls);
                }
            }
        }
        TStmt::RangeSwitch {
            subject,
            arms,
            else_body,
        } => {
            collect_expr_callees(subject, calls);
            for (_, _, body) in arms {
                for stmt in body {
                    collect_stmt_callees(stmt, calls);
                }
            }
            for stmt in else_body {
                collect_stmt_callees(stmt, calls);
            }
        }
        TStmt::IndexAssign {
            base, index, value, ..
        } => {
            collect_expr_callees(base, calls);
            collect_expr_callees(index, calls);
            collect_expr_callees(value, calls);
        },
        TStmt::IndexFieldAssign(assign) => {
            collect_expr_callees(&assign.base, calls);
            collect_expr_callees(&assign.index, calls);
            collect_expr_callees(&assign.value, calls);
        }
        TStmt::IndexHookAssign {
            base, index, value, ..
        } => {
            collect_expr_callees(base, calls);
            collect_expr_callees(index, calls);
            collect_expr_callees(value, calls);
        }
        TStmt::MathSwizzleAssign { base, value, .. } => {
            collect_expr_callees(base, calls);
            collect_expr_callees(value, calls);
        }
        TStmt::ForIn {
            source,
            collection,
            step,
            body,
            ..
        } => {
            collect_expr_callees(source, calls);
            collect_expr_callees(collection, calls);
            if let Some(step) = step {
                collect_expr_callees(step, calls);
            }
            for stmt in body {
                collect_stmt_callees(stmt, calls);
            }
        }
        TStmt::MixedSwitch {
            subject,
            arms,
            else_body,
            ..
        } => {
            collect_expr_callees(subject, calls);
            for (cond, body) in arms {
                collect_expr_callees(cond, calls);
                for stmt in body {
                    collect_stmt_callees(stmt, calls);
                }
            }
            if let Some(else_body) = else_body {
                for stmt in else_body {
                    collect_stmt_callees(stmt, calls);
                }
            }
        }
        TStmt::ScopeMember { kind, body } => {
            if let jet_codegen::Codegen::TIR::ScopeMemberKind::Timeout(duration) = kind {
                collect_expr_callees(duration, calls);
            }
            for stmt in body {
                collect_stmt_callees(stmt, calls);
            }
        }
        TStmt::Reactive { executable, .. } => collect_lambda_callees(executable, calls),
        TStmt::Layout { body, .. } => {
            for stmt in body {
                collect_stmt_callees(stmt, calls);
            }
        }
        TStmt::ContextBlock { guards, body } => {
            for (_, guard) in guards {
                collect_expr_callees(guard, calls);
            }
            for stmt in body {
                collect_stmt_callees(stmt, calls);
            }
        }
        TStmt::Transact { body, .. } => {
            for stmt in body {
                collect_stmt_callees(stmt, calls);
            }
        }
        TStmt::Break(_)
        | TStmt::Continue(_)
        | TStmt::LineMarker(_)
        | TStmt::SourceSpan(_) => {}
    }
}

fn build_callers(program: &JitProgram) -> HashMap<String, HashSet<String>> {
    let names: HashSet<String> = program.funcs.iter().map(|func| func.name.clone()).collect();
    let serde_codecs: HashSet<String> = program
        .funcs
        .iter()
        .filter(|func| is_serde_codec(func))
        .map(|func| func.name.clone())
        .collect();
    let mut callers: HashMap<String, HashSet<String>> = HashMap::new();
    for func in &program.funcs {
        let mut callees = HashSet::new();
        for stmt in &func.body {
            collect_stmt_callees(stmt, &mut callees);
        }
        for callee in callees {
            let target = if let Some(codec) = callee.strip_prefix(SERDE_DEMAND_PREFIX) {
                let canonical_owner = program.canonical_deopt.contains(&func.name);
                let serde_owner = serde_codecs.contains(&func.name);
                if (!canonical_owner && !serde_owner)
                    || (canonical_owner && !deopt_marshallable(func))
                    || !serde_codecs.contains(codec)
                {
                    continue;
                }
                codec
            } else {
                callee.as_str()
            };
            if names.contains(target) {
                callers
                    .entry(target.to_string())
                    .or_default()
                    .insert(func.name.clone());
            }
        }
    }
    callers
}

fn has_marshallable_top_level_caller(
    function: &str,
    callers: &HashMap<String, HashSet<String>>,
    program: &JitProgram,
) -> bool {
    let mut seen = HashSet::new();
    let mut pending = VecDeque::from([function.to_string()]);
    while let Some(callee) = pending.pop_front() {
        let Some(callee_callers) = callers.get(&callee) else {
            continue;
        };
        for caller in callee_callers {
            if !seen.insert(caller.clone()) {
                continue;
            }
            if program
                .funcs
                .iter()
                .find(|func| func.name == *caller)
                .is_some_and(deopt_marshallable)
            {
                return true;
            }
            pending.push_back(caller.clone());
        }
    }
    false
}
#[derive(Debug, Clone)]
pub struct TierPlan {
    pub rows: Vec<TierRow>,
    /// Functions that stay on Cranelift.
    pub native: HashSet<String>,
    /// Functions bound to the interpreter (named reason).
    pub deopt: Vec<(String, String)>,
    /// Whole program must run in the interpreter (entry gap, unmarshallable, lower fail).
    pub whole_interp: bool,
    pub gap: Option<JitGap>,
}

pub fn plan_tiers(bundle: &ProgramBundle, program: Option<&JitProgram>) -> TierPlan {
    let started = Instant::now();
    if !cranelift_host_supported() {
        let gap = classify_jit_gap(bundle);
        return TierPlan {
            rows: vec![TierRow {
                function: gap.function.clone(),
                tier: Tier::Interp,
                reason: gap.reason.clone(),
                millis: elapsed_ms(started),
            }],
            native: HashSet::new(),
            deopt: vec![(gap.function.clone(), gap.reason.clone())],
            whole_interp: true,
            gap: Some(gap),
        };
    }
    let Some(program) = program else {
        let gap = classify_jit_gap(bundle);
        return TierPlan {
            rows: vec![TierRow {
                function: gap.function.clone(),
                tier: Tier::Interp,
                reason: gap.reason.clone(),
                millis: elapsed_ms(started),
            }],
            native: HashSet::new(),
            deopt: vec![(gap.function.clone(), gap.reason.clone())],
            whole_interp: true,
            gap: Some(gap),
        };
    };

    let names: HashSet<String> = program.funcs.iter().map(|f| f.name.clone()).collect();
    let mut unsafe_names = HashSet::new();
    let mut unsafe_roots = Vec::new();
    let mut reasons = HashMap::new();

    for f in &program.funcs {
        // D-MEMO1=A: the resident engine is an adapter here. Memoized calls
        // cross the deopt boundary so the canonical TIR evaluator can call
        // the shared Prelude store; no cache policy is reimplemented in JIT.
        let reason = if f.memo_bound.is_some() {
            Some("memoized function uses the canonical Prelude cache".to_string())
        } else if program.canonical_deopt.contains(&f.name) {
            Some("typed decode uses the canonical TIR migration plan".to_string())
        } else {
            resident_safe_func_detail(f, &names)
        };
        if let Some(reason) = reason {
            unsafe_names.insert(f.name.clone());
            reasons.insert(f.name.clone(), reason);
            if !deopt_marshallable(f) {
                unsafe_roots.push(f.name.clone());
            }
        }
    }

    // A method or trait method cannot cross the named deopt ABI. Start from
    // those non-boundary roots, taint callers until the first marshallable
    // top-level owner, and stop there. Keeping a nested method as an
    // independent deopt row would promise a stub that lower_deopt_stub rejects.
    let callers = build_callers(program);
    let mut pending = VecDeque::from(unsafe_roots.clone());
    while let Some(callee) = pending.pop_front() {
        let Some(callee_callers) = callers.get(&callee) else {
            continue;
        };
        let mut callee_callers: Vec<&String> = callee_callers.iter().collect();
        callee_callers.sort_unstable();
        for caller in callee_callers {
            let marshallable = program
                .funcs
                .iter()
                .find(|func| func.name == *caller)
                .is_some_and(deopt_marshallable);
            let newly_unsafe = unsafe_names.insert(caller.clone());
            if newly_unsafe {
                reasons.insert(
                    caller.clone(),
                    format!("calls non-resident function `{callee}`"),
                );
                if !marshallable {
                    pending.push_back(caller.clone());
                }
            }
        }
    }
    let orphaned_root = unsafe_roots
        .iter()
        .any(|root| !has_marshallable_top_level_caller(root, &callers, program));

    let mut native = HashSet::new();
    let mut deopt = Vec::new();
    let mut rows = Vec::new();
    for f in &program.funcs {
        if let Some(reason) = reasons.get(&f.name) {
            rows.push(TierRow {
                function: f.name.clone(),
                tier: Tier::Interp,
                reason: reason.clone(),
                millis: 0.0,
            });
            // Only a marshallable top-level function is a legal named deopt
            // boundary. Unsafe nested methods remain evaluator-only helpers.
            if deopt_marshallable(f) {
                deopt.push((f.name.clone(), reason.clone()));
            }
        } else {
            native.insert(f.name.clone());
            rows.push(TierRow {
                function: f.name.clone(),
                tier: Tier::Native,
                reason: String::new(),
                millis: 0.0,
            });
        }
    }

    let entry_native = if program.entry == jet_foundation::Names::mangle_generated("cli_main") {
        // Host trampoline; user `run` is the resident body.
        native.contains("run")
    } else {
        native.contains(&program.entry)
    };
    let deopt_ok = !orphaned_root
        && deopt.iter().all(|(name, _)| {
            program
                .funcs
                .iter()
                .find(|f| f.name == *name)
                .is_some_and(deopt_marshallable)
        });
    let spawn_ok = {
        let sites = super::safety::count_spawn_sites(program);
        sites == program.spawn_lambdas.len()
            && program
                .spawn_lambdas
                .iter()
                .all(|lam| resident_safe_spawn_lambda(lam, &names))
    };
    let entry_shape_ok = if program.entry == jet_foundation::Names::mangle_generated("cli_main") {
        program
            .funcs
            .iter()
            .any(|f| f.name == "run" && entry_return_supported(f.ret.as_ref()))
    } else {
        program.funcs.iter().any(|f| {
            f.name == program.entry && f.params.is_empty() && entry_return_supported(f.ret.as_ref())
        })
    };

    // Mixed: entry native + every deopted helper marshallable + spawn lambdas covered.
    let mixed = entry_native && entry_shape_ok && deopt_ok && spawn_ok && !deopt.is_empty();
    let all_native = unsafe_names.is_empty() && spawn_ok && entry_native && entry_shape_ok;

    // Keep the exact rejected mixed-tier predicate in the plan's gap. The
    // caller already renders TierPlan with `Debug`; this avoids guessing from
    // rows when a future boundary check rejects an otherwise useful split.
    let mixed_gate_reason = if mixed {
        None
    } else {
        let mut failures = Vec::new();
        if !entry_native {
            failures.push("entry_native");
        }
        if !entry_shape_ok {
            failures.push("entry_shape_ok");
        }
        if !deopt_ok {
            failures.push("deopt_ok");
        }
        if !spawn_ok {
            failures.push("spawn_ok");
        }
        if deopt.is_empty() {
            failures.push("deopt_empty");
        }
        Some(failures.join(","))
    };
    let whole_interp = !all_native && !mixed;

    let ms = elapsed_ms(started);
    for row in &mut rows {
        row.millis = ms;
    }

    TierPlan {
        rows,
        native,
        deopt,
        whole_interp,
        gap: if whole_interp || mixed {
            let mut gap = classify_jit_gap(bundle);
            if let Some(reason) = mixed_gate_reason.as_deref() {
                gap.reason = format!("tier plan gate failed: {reason}; {}", gap.reason);
            }
            Some(gap)
        } else {
            None
        },
    }
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_aggregate_keeps_an_earlier_interpreter_plan() {
        let mut aggregate = TierTraceAggregate::default();
        aggregate.append(&[TierRow {
            function: "scheduled".to_string(),
            tier: Tier::Interp,
            reason: "scheduled deopt".to_string(),
            millis: 1.0,
        }]);
        aggregate.append(&[TierRow {
            function: "main".to_string(),
            tier: Tier::Native,
            reason: String::new(),
            millis: 2.0,
        }]);

        assert_eq!(aggregate.native_rows, 1);
        assert_eq!(aggregate.interp_rows, 1);
        assert!(aggregate.whole_program_deopt);
        assert_eq!(aggregate.rows.len(), 2);
        let json = aggregate.to_json();
        assert!(json.contains("\"native_rows\":1"));
        assert!(json.contains("\"interp_rows\":1"));
        assert!(json.contains("\"whole_program_deopt\":true"));
        assert!(json.find("\"scheduled\"").unwrap() < json.find("\"main\"").unwrap());
    }
}
