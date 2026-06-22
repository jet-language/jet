//! Effect system (D-EFF1, D-QUAL1, D-EFF2, D-EFF3).
//!
//! Every function carries an inferred **effect set** — the categories of
//! ambient power its body exercises (network, filesystem, clock, …). The set is
//! inferred per-function, propagated along the call graph (Koka-style rows), and
//! **fully erased in codegen** (I3): effects are a compile-time proof with no
//! runtime value, handler, or monad. A `#Pure fn` is the function whose inferred
//! set is empty (the ⊥ of the lattice).
//!
//! This module owns the effect vocabulary, the per-function summary the checker
//! accumulates during its walk, the whole-program fixpoint that turns those
//! summaries into transitive inferred sets, and the boundary diagnostics
//! (E0740 out-of-set against a declared `#(…)` bound; E0745 a non-empty bound on
//! a `#Pure fn`). Casing is PascalCase per D-CASING1.

use crate::Diagnostics::{Diagnostic, Span};
use std::collections::{BTreeSet, HashMap};

/// A primitive effect. Closed, compiler-known set; each Core operation
/// contributes exactly one. Ordered for deterministic diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Effect {
    Net,
    Fs,
    Io,
    Db,
    Time,
    Rand,
    Env,
    Exec,
    Log,
    Gpu,
}

impl Effect {
    /// The PascalCase surface spelling (D-CASING1).
    pub fn name(self) -> &'static str {
        match self {
            Effect::Net => "Net",
            Effect::Fs => "Fs",
            Effect::Io => "Io",
            Effect::Db => "Db",
            Effect::Time => "Time",
            Effect::Rand => "Rand",
            Effect::Env => "Env",
            Effect::Exec => "Exec",
            Effect::Log => "Log",
            Effect::Gpu => "Gpu",
        }
    }

    /// Parse a user-written effect name; `None` if it is not a known effect.
    pub fn parse(s: &str) -> Option<Effect> {
        Some(match s {
            "Net" => Effect::Net,
            "Fs" => Effect::Fs,
            "Io" => Effect::Io,
            "Db" => Effect::Db,
            "Time" => Effect::Time,
            "Rand" => Effect::Rand,
            "Env" => Effect::Env,
            "Exec" => Effect::Exec,
            "Log" => Effect::Log,
            "Gpu" => Effect::Gpu,
            _ => return None,
        })
    }

    /// Every effect — the maximal set, used for foreign (`extern`) calls whose
    /// body the compiler cannot inspect and for escaping function values.
    pub fn all() -> EffectSet {
        [
            Effect::Net, Effect::Fs, Effect::Io, Effect::Db, Effect::Time,
            Effect::Rand, Effect::Env, Effect::Exec, Effect::Log, Effect::Gpu,
        ]
        .into_iter()
        .collect()
    }
}

pub type EffectSet = BTreeSet<Effect>;

/// Render a set as `Net, Fs` (canonical order) for diagnostics.
pub fn show_set(set: &EffectSet) -> String {
    set.iter().map(|e| e.name()).collect::<Vec<_>>().join(", ")
}

/// The effect carried by a Core call `module.method`, or `None` if pure.
/// Grounded in the real Core API surface (CheckerCoreLib). The `module` is the
/// fully-resolved name (`core.fs`, `jet.http`, …).
pub fn core_effect(module: &str, _method: &str) -> Option<Effect> {
    Some(match module {
        "core.fs" | "core.files" => Effect::Fs,
        "core.net" | "jet.http" => Effect::Net,
        "core.time" | "jet.time" => Effect::Time,
        "core.random" => Effect::Rand,
        "core.env" => Effect::Env,
        "core.process" => Effect::Exec,
        "core.io" => Effect::Io,
        "jet.db" | "jet.sql" => Effect::Db,
        "jet.log" => Effect::Log,
        _ => return None,
    })
}

/// The effect carried by an ambient builtin call (`print`, `input`, …).
pub fn builtin_effect(name: &str) -> Option<Effect> {
    if crate::Syntax::IMPURE_BUILTINS.contains(&name) {
        Some(Effect::Io)
    } else {
        None
    }
}

/// Per-function summary the checker accumulates during its walk: the effects the
/// body reaches directly, the user functions it calls (edges for transitivity),
/// and whether it touches a foreign body (forcing the maximal set).
#[derive(Debug, Clone, Default)]
pub struct EffectSummary {
    pub direct: EffectSet,
    /// Bare names of user functions called in the body (call-graph edges).
    pub edges: BTreeSet<String>,
    /// A foreign (`extern`) call was reached: the body's effects are maximal.
    pub maximal: bool,
}

/// Whole-program fixpoint: turn per-function summaries into transitive inferred
/// effect sets. `summaries` is keyed by function identity (bare name for
/// top-level functions; `Type::method` for methods). Edges name bare functions,
/// resolved against the same map. Iterates to a fixed point so mutual recursion
/// converges.
pub fn solve(summaries: &HashMap<String, EffectSummary>) -> HashMap<String, EffectSet> {
    let mut sets: HashMap<String, EffectSet> = HashMap::new();
    for (k, s) in summaries {
        let mut init = s.direct.clone();
        if s.maximal {
            init.extend(Effect::all());
        }
        sets.insert(k.clone(), init);
    }
    // Worklist fixpoint. The lattice is finite (≤10 effects per node), so a
    // simple round-robin until no set grows terminates quickly.
    let mut changed = true;
    while changed {
        changed = false;
        for (k, s) in summaries {
            let mut add: EffectSet = BTreeSet::new();
            for callee in &s.edges {
                if let Some(cs) = sets.get(callee) {
                    for e in cs {
                        add.insert(*e);
                    }
                }
            }
            let cur = sets.get_mut(k).expect("seeded above");
            let before = cur.len();
            cur.extend(add);
            if cur.len() != before {
                changed = true;
            }
        }
    }
    sets
}

/// E0740: a function's inferred effects exceed its declared `#(…)` bound.
pub fn e0740(fn_name: &str, over: &EffectSet, declared: &EffectSet, span: Span) -> Diagnostic {
    let over_list = show_set(over);
    let decl = if declared.is_empty() {
        "no effects".to_string()
    } else {
        format!("`#({})`", show_set(declared))
    };
    Diagnostic::error(
        "E0740",
        format!(
            "`{}` uses the effect `{}`, which its signature doesn't allow",
            fn_name, over_list
        ),
        format!(
            "the signature declares {}, so the body may only use those; `{}` is outside that set",
            decl, over_list
        ),
        format!("add `{}` to the effect list, or stop using it in `{}`", over_list, fn_name),
        Some(span),
    )
}

/// E0745: a `#Pure fn` also carries a non-empty `#(…)` bound — a contradiction.
pub fn e0745(fn_name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0745",
        format!("`{}` is `#{}` but also declares effects", fn_name, crate::Syntax::KW_PURE),
        format!(
            "`#{}` means the empty effect set; a `#(…)` list on the same function asks for both empty and non-empty",
            crate::Syntax::KW_PURE
        ),
        format!("drop the `#(…)` list to keep `{}` pure, or remove `#{}`", fn_name, crate::Syntax::KW_PURE),
        Some(span),
    )
}
