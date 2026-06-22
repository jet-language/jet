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

impl<'a> super::Checker<'a> {
    /// D-EFF1: record an effect this function reaches directly — into the
    /// function's set and every open `#Caps(…)` region (which must account for
    /// effects reached inside it, E0741).
    pub(crate) fn record_effect(&mut self, e: Effect) {
        self.fx_direct.insert(e);
        for r in &mut self.region_stack {
            r.direct.insert(e);
        }
    }

    /// D-EFF1: record a call-graph edge to a user function `name` — into the
    /// function's edges and every open `#Caps(…)` region.
    pub(crate) fn record_edge(&mut self, name: String) {
        for r in &mut self.region_stack {
            r.edges.insert(name.clone());
        }
        self.fx_edges.insert(name);
    }

    /// D-EFF1: record that a foreign (`extern`) call was reached — forcing the
    /// maximal set on the function and every open `#Caps(…)` region.
    pub(crate) fn record_maximal(&mut self) {
        self.fx_maximal = true;
        for r in &mut self.region_stack {
            r.maximal = true;
        }
    }

    /// D-EFF2 (transparent flow-through): a function value passed as an argument
    /// to a function-typed parameter contributes its effects to *this* function
    /// (the caller), so a callback's effects surface at the call site, not buried
    /// inside the higher-order callee.
    ///
    /// - A **lambda** argument is already walked inline into this function's set,
    ///   so it needs nothing here.
    /// - A **directly-named top-level function** flows through precisely (edge).
    /// - Any **other** function value — a local binding, a parameter passed
    ///   onward, a returned/stored callback — has an origin that isn't statically
    ///   known here, so it defaults to the maximal effect set (D-EFF2, sound).
    ///   The expert levers `#(E) fn(…)` param types and `#(via f)` tighten this.
    pub(crate) fn attribute_fn_arg(&mut self, arg: &crate::AST::Expr) {
        use crate::AST::Expr;
        match arg {
            Expr::Lambda(_) => {}
            Expr::Ident(name, _)
                if self.lookup(name).is_none() && self.funcs.contains_key(name) =>
            {
                self.record_edge(name.clone());
            }
            _ => self.record_maximal(),
        }
    }
}

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
    /// D-EFF1: `#Caps(…)` restriction regions found in this body (checked against
    /// their transitive inferred set in the post-pass — E0741).
    pub regions: Vec<RegionSummary>,
}

/// D-EFF1: an open `#Caps(…)` region's running accumulator while the checker
/// walks its body. Sealed into a `RegionSummary` when the region closes.
#[derive(Debug, Clone)]
pub struct RegionAccum {
    pub caps: EffectSet,
    pub caps_span: Span,
    pub direct: EffectSet,
    pub edges: BTreeSet<String>,
    pub maximal: bool,
}

/// D-EFF1: a `#Caps(…) { … }` region's accumulated effects, checked (transitively)
/// against the declared cap set in the post-pass.
#[derive(Debug, Clone)]
pub struct RegionSummary {
    /// The declared cap set — the only effects the region may use.
    pub caps: EffectSet,
    pub direct: EffectSet,
    pub edges: BTreeSet<String>,
    pub maximal: bool,
    /// Span of the `#Caps(…)` list, for the diagnostic.
    pub caps_span: Span,
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

/// D-EFF1: check every `#Caps(…)` region across the program against its
/// transitive inferred effect set (region.direct ∪ maximal ∪ ⋃ solved[edge]).
/// An effect used inside a region that its cap list omits is E0741.
pub fn check_region_caps(
    summaries: &HashMap<String, EffectSummary>,
    solved: &HashMap<String, EffectSet>,
    diags: &mut Vec<Diagnostic>,
) {
    for summary in summaries.values() {
        for region in &summary.regions {
            let mut inferred = region.direct.clone();
            if region.maximal {
                inferred.extend(Effect::all());
            }
            for callee in &region.edges {
                if let Some(cs) = solved.get(callee) {
                    inferred.extend(cs.iter().copied());
                }
            }
            let over: EffectSet = inferred.difference(&region.caps).copied().collect();
            if !over.is_empty() {
                diags.push(e0741(&over, &region.caps, region.caps_span));
            }
        }
    }
}

/// E0741: an effect used inside a `#Caps(…)` region is not in its cap list.
pub fn e0741(over: &EffectSet, caps: &EffectSet, span: Span) -> Diagnostic {
    let over_list = show_set(over);
    let caps_list = if caps.is_empty() {
        "no effects".to_string()
    } else {
        format!("`{}`", show_set(caps))
    };
    Diagnostic::error(
        "E0741",
        format!("this `#{}` region uses the effect `{}`, which it doesn't allow", crate::Syntax::KW_CAPS, over_list),
        format!(
            "`#{}(…)` restricts the region to {}; an effect reached inside — even through a call — must be in that list",
            crate::Syntax::KW_CAPS, caps_list
        ),
        format!("add `{}` to the `#{}(…)` list, or move that work outside the region", over_list, crate::Syntax::KW_CAPS),
        Some(span),
    )
}

/// E0119: a `#(…)` or `#Caps(…)` list names something that isn't a known effect.
pub fn unknown_effect(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0119",
        format!("`{}` isn't a known effect", name),
        "an effect list names compiler-known effects like `Net`, `Fs`, `Io`, `Db`, or `Time`"
            .to_string(),
        "use one of the known effect names, or remove it from the list".to_string(),
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
