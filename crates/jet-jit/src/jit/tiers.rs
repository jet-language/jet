//! Per-function tier classification and `--trace-tiers` (D-LENS-RUN2=A).

use std::cell::RefCell;
use std::collections::HashSet;
use std::time::Instant;

use jet_codegen::Codegen::TIR::{JitProgram, TFunc, TFuncKind};
use jet_foundation::AST::{ProgramBundle, Type};

use super::api_debug::{cranelift_host_supported, classify_jit_gap};
use super::safety::{resident_safe_func_detail, resident_safe_spawn_lambda};
use super::gap::JitGap;

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

thread_local! {
    static TRACE_TIERS: RefCell<bool> = const { RefCell::new(false) };
    static LAST_TRACE: RefCell<Vec<TierRow>> = const { RefCell::new(Vec::new()) };
}

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
    if trace_tiers_enabled() {
        for row in &rows {
            let tier = match row.tier {
                Tier::Native => "tier1 native",
                Tier::Interp => "tier0 interp",
            };
            if row.reason.is_empty() {
                eprintln!(
                    "{:<24} {tier} ({:.3}ms)",
                    row.function, row.millis
                );
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
        Some(ty) => marshallable_ty(ty) || matches!(ty, Type::Named(n) if n == "Unit"),
    }
}

fn marshallable_ty(ty: &Type) -> bool {
    match ty {
        Type::Int | Type::IntN { .. } | Type::String | Type::Bool | Type::Char => true,
        Type::Named(n) if matches!(n.as_str(), "Int" | "String" | "Bool" | "Char" | "Unit") => {
            true
        }
        _ => false,
    }
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
    let has_contracts = program
        .funcs
        .iter()
        .any(|f| !f.pre_contracts.is_empty() || !f.post_contracts.is_empty());
    let mut native = HashSet::new();
    let mut deopt = Vec::new();
    let mut rows = Vec::new();

    for f in &program.funcs {
        if program.canonical_deopt.contains(&f.name) {
            let reason = "typed decode uses the canonical TIR migration plan".to_string();
            deopt.push((f.name.clone(), reason.clone()));
            rows.push(TierRow {
                function: f.name.clone(),
                tier: Tier::Interp,
                reason,
                millis: 0.0,
            });
            continue;
        }
        match resident_safe_func_detail(f, &names) {
            None => {
                native.insert(f.name.clone());
                rows.push(TierRow {
                    function: f.name.clone(),
                    tier: Tier::Native,
                    reason: String::new(),
                    millis: 0.0,
                });
            }
            Some(reason) => {
                deopt.push((f.name.clone(), reason.clone()));
                rows.push(TierRow {
                    function: f.name.clone(),
                    tier: Tier::Interp,
                    reason,
                    millis: 0.0,
                });
            }
        }
    }

    let entry_native = if program.entry == jet_foundation::Names::mangle_generated("cli_main") {
        // Host trampoline; user `run` is the resident body.
        native.contains("run")
    } else {
        native.contains(&program.entry)
    };
    let deopt_ok = deopt.iter().all(|(name, _)| {
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
        program.funcs.iter().any(|f| {
            f.name == "run"
                && f.params.len() == 1
                && (f.ret.is_none()
                    || matches!(&f.ret, Some(Type::Result { ok, err })
                        if matches!(ok.as_ref(), Type::Named(n) if n == "Unit")
                            && matches!(err.as_ref(), Type::String | Type::Named(_))))
        })
    } else {
        program.funcs.iter().any(|f| {
            f.name == program.entry
                && f.params.is_empty()
                && (f.ret.is_none()
                    || matches!(&f.ret, Some(Type::Result { ok, err })
                        if matches!(ok.as_ref(), Type::Named(n) if n == "Unit")
                            && matches!(err.as_ref(), Type::String | Type::Named(_))))
        })
    };

    // Mixed: entry native + every deopted helper marshallable + spawn lambdas covered.
    let mixed = entry_native && entry_shape_ok && deopt_ok && spawn_ok && !deopt.is_empty();
    let all_native = deopt.is_empty() && spawn_ok && entry_native && entry_shape_ok;
    // Contracts are executable TIR facts. Keep the whole call graph on the
    // canonical interpreter so a contract cannot disappear at a deopt ABI
    // boundary or be evaluated by a second engine-specific implementation.
    let whole_interp = has_contracts || (!all_native && !mixed);

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
            Some(classify_jit_gap(bundle))
        } else {
            None
        },
    }
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}
