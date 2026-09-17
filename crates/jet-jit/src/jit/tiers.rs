//! MIR-native tier classification and trace publication.

use super::gap::JitGap;
use super::safety::{artifact_entry, function_name};
use jet_foundation::JSON::json_escape;
use jet_foundation::MIR::{
    MirArtifactId, MirCallee, MirDecisionDisposition, MirDecisionKind, MirDecisionRow,
    MirFunctionId, MirLoopSourceKind, MirOperation, MirProgram,
};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Native,
    Interp,
}

#[derive(Debug, Clone)]
pub struct TierRow {
    pub function: MirFunctionId,
    pub function_name: String,
    pub tier: Tier,
    pub reason: String,
    pub millis: f64,
}

#[derive(Debug, Clone, Default)]
pub struct TierTraceAggregate {
    pub rows: Vec<TierRow>,
    pub native_rows: usize,
    pub interp_rows: usize,
    pub whole_program_deopt: bool,
}

impl TierTraceAggregate {
    fn append(&mut self, rows: &[TierRow]) {
        self.native_rows += rows.iter().filter(|row| row.tier == Tier::Native).count();
        self.interp_rows += rows.iter().filter(|row| row.tier == Tier::Interp).count();
        self.rows.extend(rows.iter().cloned());
    }

    pub fn to_json(&self) -> String {
        let rows = self
            .rows
            .iter()
            .map(|row| {
                let tier = match row.tier {
                    Tier::Native => "native",
                    Tier::Interp => "interp",
                };
                let reason = if row.reason.is_empty() {
                    "null".to_string()
                } else {
                    format!("\"{}\"", json_escape(&row.reason))
                };
                format!(
                    "{{\"function_id\":{},\"function\":\"{}\",\"tier\":\"{}\",\"reason\":{},\"millis\":{}}}",
                    row.function.0,
                    json_escape(&row.function_name),
                    tier,
                    reason,
                    if row.millis.is_finite() { row.millis.to_string() } else { "null".into() },
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"rows\":[{rows}],\"native_rows\":{},\"interp_rows\":{},\"whole_program_deopt\":{}}}",
            self.native_rows, self.interp_rows, self.whole_program_deopt
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct MirTierPlan {
    pub rows: Vec<TierRow>,
    pub native: BTreeSet<MirFunctionId>,
    pub deopt: Vec<(MirFunctionId, String)>,
    pub whole_program_deopt: bool,
    pub gap: Option<JitGap>,
}

impl MirTierPlan {
    /// Convert the planner's own tier/deopt records to shared decision rows.
    /// The inspect plane only renders these rows; it never replans a function.
    pub fn decision_ledger_rows(&self, program: &MirProgram) -> Vec<MirDecisionRow> {
        let mut rows = Vec::new();
        for tier in &self.rows {
            let span = program
                .functions
                .iter()
                .find(|function| function.id == tier.function)
                .map(|function| function.span)
                .unwrap_or_else(|| jet_foundation::Diagnostics::Span::new(0, 0));
            let (disposition, reason) = match tier.tier {
                Tier::Native => (
                    MirDecisionDisposition::Selected,
                    if tier.reason.is_empty() {
                        "the native tier was selected".to_string()
                    } else {
                        tier.reason.clone()
                    },
                ),
                Tier::Interp => (
                    MirDecisionDisposition::Selected,
                    if tier.reason.is_empty() {
                        "the interpreter tier was selected".to_string()
                    } else {
                        tier.reason.clone()
                    },
                ),
            };
            rows.push(MirDecisionRow::new(
                MirDecisionKind::Tier,
                disposition,
                Some(tier.function),
                tier.function_name.clone(),
                span,
                match tier.tier {
                    Tier::Native => "cranelift",
                    Tier::Interp => "interpreter",
                },
                reason,
                "jit.tier-planner",
                "tier-selection-plan",
            ));
            if tier.tier == Tier::Interp && !tier.reason.is_empty() {
                rows.push(MirDecisionRow::new(
                    MirDecisionKind::Deopt,
                    MirDecisionDisposition::Selected,
                    Some(tier.function),
                    tier.function_name.clone(),
                    span,
                    "tier-fallback",
                    tier.reason.clone(),
                    "jit.tier-planner",
                    "tier-fallback-plan",
                ));
            }
        }
        rows.sort_by(|left, right| {
            left.span
                .start
                .cmp(&right.span.start)
                .then_with(|| left.kind.cmp(&right.kind))
                .then_with(|| left.function_name.cmp(&right.function_name))
                .then_with(|| left.id.cmp(&right.id))
        });
        rows
    }
}

/// Convert tier rows emitted after an invocation into actual runtime
/// decisions.  Planning rows remain separate: a static tier choice is not
/// evidence that the function executed.
pub(crate) fn runtime_decision_rows(
    program: &MirProgram,
    observed_rows: &[TierRow],
) -> Vec<MirDecisionRow> {
    let mut rows = Vec::with_capacity(observed_rows.len().saturating_mul(2));
    for observed in observed_rows {
        let span = program
            .functions
            .iter()
            .find(|function| function.id == observed.function)
            .map(|function| function.span)
            .unwrap_or_else(|| jet_foundation::Diagnostics::Span::new(0, 0));
        let tier = match observed.tier {
            Tier::Native => "native",
            Tier::Interp => "interpreter",
        };
        let reason = if observed.reason.is_empty() {
            format!("the {tier} tier executed")
        } else {
            format!("the {tier} tier executed: {}", observed.reason)
        };
        let mut tier_row = MirDecisionRow::new(
            MirDecisionKind::Tier,
            MirDecisionDisposition::Selected,
            Some(observed.function),
            observed.function_name.clone(),
            span,
            "runtime-tier",
            reason,
            "jit.runtime",
            "runtime-tier-observation",
        );
        tier_row.evidence_method =
            Some(jet_foundation::Facts::DerivationMethod::RecordedExecution);
        rows.push(tier_row);
        if observed.tier == Tier::Interp && !observed.reason.is_empty() {
            let mut deopt_row = MirDecisionRow::new(
                MirDecisionKind::Deopt,
                MirDecisionDisposition::Selected,
                Some(observed.function),
                observed.function_name.clone(),
                span,
                "runtime-deopt",
                observed.reason.clone(),
                "jit.runtime",
                "runtime-deopt-observation",
            );
            deopt_row.evidence_method =
                Some(jet_foundation::Facts::DerivationMethod::RecordedExecution);
            rows.push(deopt_row);
        }
    }
    rows
}


thread_local! {
    static TRACE_TIERS: RefCell<bool> = const { RefCell::new(false) };
    static LAST_TRACE: RefCell<Vec<TierRow>> = const { RefCell::new(Vec::new()) };
}

static TRACE_AGGREGATE: LazyLock<Mutex<TierTraceAggregate>> =
    LazyLock::new(|| Mutex::new(TierTraceAggregate::default()));

pub fn set_trace_tiers(enabled: bool) {
    TRACE_TIERS.with(|slot| *slot.borrow_mut() = enabled);
}

pub fn trace_tiers_enabled() -> bool {
    TRACE_TIERS.with(|slot| *slot.borrow())
}

pub fn take_last_trace() -> Vec<TierRow> {
    LAST_TRACE.with(|slot| std::mem::take(&mut *slot.borrow_mut()))
}

pub fn record_trace(rows: Vec<TierRow>) {
    if let Ok(mut aggregate) = TRACE_AGGREGATE.lock() {
        aggregate.append(&rows);
    }
    if trace_tiers_enabled() {
        for row in &rows {
            let tier = match row.tier {
                Tier::Native => "tier1 native",
                Tier::Interp => "tier0 interp",
            };
            if row.reason.is_empty() {
                eprintln!("{:<24} {tier} ({:.3}ms)", row.function_name, row.millis);
            } else {
                eprintln!(
                    "{:<24} {tier} ({}) ({:.3}ms)",
                    row.function_name, row.reason, row.millis
                );
            }
        }
    } else {
        for row in &rows {
            if row.tier == Tier::Interp && !row.reason.is_empty() {
                eprintln!(
                    "deopt: function={} tier=interp reason={}",
                    row.function_name, row.reason
                );
            }
        }
    }
    LAST_TRACE.with(|slot| *slot.borrow_mut() = rows);
}

pub fn take_trace_aggregate() -> TierTraceAggregate {
    TRACE_AGGREGATE
        .lock()
        .map(|mut aggregate| std::mem::take(&mut *aggregate))
        .unwrap_or_default()
}

pub fn write_trace_sidecar(path: &Path, aggregate: &TierTraceAggregate) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("tier-trace.json");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let temporary = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), nonce));
    let result = fs::write(&temporary, aggregate.to_json()).and_then(|_| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Return a direct MIR function target.  The artifact planner must follow
/// checked user calls and closure constructors, but never infer a target from
/// a rendered symbol or from an opaque indirect call.
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
        _ => None,
    }
}

fn insert_symbol_target(
    program: &MirProgram,
    selected: &mut BTreeSet<MirFunctionId>,
    symbol: &str,
) {
    if let Some(function) = program
        .functions
        .iter()
        .find(|function| function.key == symbol)
    {
        selected.insert(function.id);
    }
}

/// Seed an artifact's callable roots.  Functions outside these roots are
/// metadata, helper declarations for other targets, or otherwise unreachable
/// from this invocation and must not poison its tier plan.
fn artifact_roots(program: &MirProgram, artifact: MirArtifactId) -> BTreeSet<MirFunctionId> {
    let Some(plan) = program.artifacts.iter().find(|plan| plan.id == artifact) else {
        return BTreeSet::new();
    };
    let mut selected = BTreeSet::new();
    if let Some(entry) = &plan.entry {
        if let Some(function) = entry.function {
            selected.insert(function);
        }
        if let Some(cli) = &entry.cli {
            selected.extend(cli.commands.iter().map(|command| command.function));
        }
    }
    selected.extend(plan.exports.iter().map(|export| export.function));
    for job_id in &plan.jobs {
        if let Some(job) = program.jobs.iter().find(|job| job.id == *job_id) {
            selected.insert(job.function);
        }
    }
    if let Some(harness_id) = plan.harness {
        if let Some(harness) = program
            .harnesses
            .iter()
            .find(|harness| harness.id == harness_id)
        {
            let test_ids = harness
                .selected_test
                .into_iter()
                .chain(harness.tests.iter().copied());
            for test_id in test_ids {
                if let Some(test) = program.tests.iter().find(|test| test.id == test_id) {
                    selected.insert(test.function);
                    if let Some(eligibility) = test.eligibility {
                        selected.insert(eligibility);
                    }
                }
            }
            selected.extend(
                harness
                    .output_checks
                    .iter()
                    .map(|check| check.function),
            );
            selected.extend(
                harness
                    .coverage_points
                    .iter()
                    .map(|point| point.function),
            );
        }
    }
    for setup in &program.facts.hardware_setups {
        if let jet_foundation::MIR::MirHardwareSetup::InterruptBind {
            handler_symbol, ..
        } = setup
        {
            insert_symbol_target(program, &mut selected, handler_symbol);
        }
    }
    selected
}

/// Add function targets that are encoded in operation metadata rather than a
/// normal MIR call.  These are still checked, transitive artifact edges:
/// iterable hooks are named by their source fact and typed CSV calls resolve a
/// concrete Decode method from their row type.
fn append_operation_targets(
    program: &MirProgram,
    function: &jet_foundation::MIR::MirFunction,
    selected: &mut BTreeSet<MirFunctionId>,
) {
    for instruction in function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
    {
        if let Some(callee) = direct_function_call(&instruction.operation) {
            selected.insert(callee);
        }
        match &instruction.operation {
            MirOperation::LoopIterInit {
                source_kind:
                    MirLoopSourceKind::Iterable {
                        iter_symbol,
                        next_symbol,
                        ..
                    },
                ..
            } => {
                insert_symbol_target(program, selected, iter_symbol);
                insert_symbol_target(program, selected, next_symbol);
            }
            MirOperation::CoreCall {
                call, type_args, ..
            } if type_args.len() == 1 => {
                let Some(core) = program.core_calls.iter().find(|core| core.id == *call) else {
                    continue;
                };
                if !matches!(
                    (core.module.as_str(), core.member.as_str()),
                    ("core.data", "csv") | ("core.encoding.csv", "decode" | "query")
                ) {
                    continue;
                }
                for candidate in &program.functions {
                    if candidate.is_decode_for(&type_args[0]) {
                        selected.insert(candidate.id);
                    }
                }
            }
            _ => {}
        }
    }
}

fn selected_function_ids(program: &MirProgram, artifact: MirArtifactId) -> BTreeSet<MirFunctionId> {
    let mut selected = artifact_roots(program, artifact);
    let mut changed = true;
    while changed {
        changed = false;
        let current = selected.iter().copied().collect::<Vec<_>>();
        for function_id in current {
            let Some(function) = program
                .functions
                .iter()
                .find(|function| function.id == function_id)
            else {
                continue;
            };
            let before = selected.len();
            append_operation_targets(program, function, &mut selected);
            changed |= selected.len() != before;
        }
    }
    selected
}

/// Plan only the transitive function closure rooted in the requested artifact.
/// A function rejected by sema remains a named per-function interpreter row;
/// unrelated functions never turn a resident run into a whole-program deopt.
pub fn plan_mir_tiers(program: &MirProgram, artifact: MirArtifactId) -> MirTierPlan {
    let mut plan = MirTierPlan::default();
    let selected = selected_function_ids(program, artifact);
    for function in program
        .functions
        .iter()
        .filter(|function| selected.contains(&function.id))
    {
        if function.target_applicability.cranelift {
            plan.native.insert(function.id);
            plan.rows.push(TierRow {
                function: function.id,
                function_name: function.key.clone(),
                tier: Tier::Native,
                reason: String::new(),
                millis: 0.0,
            });
        } else {
            let reason = "upstream MIR target applicability excludes Cranelift".to_string();
            plan.deopt.push((function.id, reason.clone()));
            plan.rows.push(TierRow {
                function: function.id,
                function_name: function.key.clone(),
                tier: Tier::Interp,
                reason,
                millis: 0.0,
            });
        }
    }
    plan
}

pub(crate) fn mir_function_ids(plan: &MirTierPlan) -> impl Iterator<Item = MirFunctionId> + '_ {
    plan.native.iter().copied()
}
