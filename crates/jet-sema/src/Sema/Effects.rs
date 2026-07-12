//! Effect system (D-EFF1, D-QUAL1, D-EFF2, D-EFF3).
//!
//! Every function carries an inferred **effect set** — the categories of
//! ambient power its body exercises (network, filesystem, clock, …). The set is
//! inferred per-function, propagated along the call graph (Koka-style rows), and
//! **fully erased in codegen** (I3): effects are a compile-time proof with no
//! runtime value, handler, or monad. A `@Pure fn` is the function whose inferred
//! set is empty (the ⊥ of the lattice).
//!
//! This module owns the effect vocabulary, the per-function summary the checker
//! accumulates during its walk, the whole-program fixpoint that turns those
//! summaries into transitive inferred sets, and the boundary diagnostics
//! (E0740 out-of-set against a declared `#(…)` bound; E0745 a non-empty bound on
//! a `@Pure fn`). Casing is PascalCase per D-CASING1.
//!
//! D-WASM1=A amends D-EFF4 with `Browser` — DOM / browser API use (c123).
//!
//! U13 (D-JPK-SECRETCRYPTO1, card c9jetpackgates) amends D-EFF4/5 with
//! `Secret` — reading a decrypted repo secret (`core.vault.get`). Modeled as a
//! bare root, not a leaf under an existing root: the compiler's own
//! Core-call-to-effect inference (`core_effect`) only ever tags a call with a
//! bare `Effect` (leaf precision is a user-declared-contract concept, never
//! inferred from a real call — see the D-EFFTREE1 note below), so a genuinely
//! *inferred* new effect can only ever be a new root, the same shape D-WASM1
//! already used for `Browser`. Unlike every other root, reaching it is denied
//! by default even with a matching declared bound absent — see
//! `check_secret_grants`.
//!
//! D-EFFTREE1 (ratified 2026-07-03) amends D-EFF4/5: the ten flat names below
//! become tree **roots**. A user-written effect name may now be a dotted path
//! rooted at one of them (`Fs.Read`, `Net.Http.Get`) — the root is validated
//! against the closed vocabulary (E0119 otherwise); further segments are an
//! open, user-chosen leaf path with no fixed vocabulary of children (the same
//! shape as D-TAG1's tag-tree dotted paths). An `EffectSet` element is now the
//! canonical dotted string (`"Fs"`, `"Fs.Read"`) rather than a bare `Effect`.
//! **Ancestor matching is subsumption**: a bound entry covers any effect at or
//! below it in the tree (`effect_covers`) — the same ancestor-subtree rule as
//! D-TAG1's nested variant groups (CheckerCore.rs's switch-arm coverage:
//! `variant.starts_with(&format!("{c}."))`). `Effect` itself stays the closed
//! twelve-root enum (U13 added `Secret`), used for root validation/
//! classification and by the small set of call sites (D-TXN2, D-TAINT1,
//! D-WASM1) that only ever care about a whole root regardless of leaf.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
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
    /// D-WASM1=A: browser/DOM API use — implies JS partition for web targets.
    Browser,
    /// U13 (D-JPK-SECRETCRYPTO1): reading a decrypted repo secret
    /// (`core.vault.get`). Denied by default even with no declared bound at
    /// all — see `check_secret_grants` — and always denied in a comptime
    /// build-tier context (E1265), with no `#Impure` escape hatch.
    Secret,
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
            Effect::Browser => "Browser",
            Effect::Secret => "Secret",
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
            "Browser" => Effect::Browser,
            "Secret" => Effect::Secret,
            _ => return None,
        })
    }

    /// Every effect — the maximal set, used for foreign (`extern`) calls whose
    /// body the compiler cannot inspect and for escaping function values. Each
    /// entry is a bare root, which (D-EFFTREE1 ancestor subsumption) covers
    /// its whole subtree — so this is still the true maximal set.
    pub fn all() -> EffectSet {
        [
            Effect::Net,
            Effect::Fs,
            Effect::Io,
            Effect::Db,
            Effect::Time,
            Effect::Rand,
            Effect::Env,
            Effect::Exec,
            Effect::Log,
            Effect::Gpu,
            Effect::Browser,
            Effect::Secret,
        ]
        .into_iter()
        .map(|e| e.name().to_string())
        .collect()
    }
}

/// D-EFFTREE1: an effect set's elements are canonical dotted paths (`"Fs"`,
/// `"Fs.Read"`) rather than bare `Effect` roots — see the module doc.
pub type EffectSet = BTreeSet<String>;

/// The root segment of a dotted effect path (`"Fs.Read"` → `"Fs"`; a bare
/// name is its own root). D-EFFTREE1.
pub fn effect_root(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

/// D-EFFTREE1: validate a user-written effect path — bare (`Fs`) or dotted
/// (`Fs.Read`, `Net.Http.Get`). The root must be one of the closed ten
/// D-EFF4/5 names (the caller reports E0119 on `None`); further segments are
/// an open, user-chosen leaf path with no fixed vocabulary — mirrors D-TAG1's
/// tag-tree dotted paths. Returns the path unchanged (as the canonical form)
/// when the root is known.
pub fn parse_effect_name(name: &str) -> Option<String> {
    Effect::parse(effect_root(name))?;
    Some(name.to_string())
}

/// D-EFFTREE1: does `bound` (one entry of a declared/granted/prohibited set)
/// cover `e`? Exact match, or `bound` is a dot-path ancestor of `e` — ancestor
/// subsumption, the same rule as D-TAG1's tag-tree subtree matching. A
/// root-only bound (`Fs`) covers every leaf under it; a leaf bound
/// (`Fs.Read`) covers only itself and any deeper path under it — a sibling
/// (`Fs.Write`) is never covered.
pub fn effect_covers(bound: &str, e: &str) -> bool {
    e == bound || e.starts_with(&format!("{bound}."))
}

/// The subset of `inferred` NOT covered by any entry of `bound_set` — the
/// tree-aware replacement for a flat `BTreeSet::difference` now that ancestor
/// entries subsume their whole subtree (D-EFFTREE1). Used at every "is the
/// inferred set within its declared bound" check (E0740/E0741/E0712/E0747/E0742).
pub fn effects_uncovered(inferred: &EffectSet, bound_set: &EffectSet) -> EffectSet {
    inferred
        .iter()
        .filter(|e| !bound_set.iter().any(|b| effect_covers(b, e)))
        .cloned()
        .collect()
}

/// The subset of `inferred` covered by any entry of `set` — used for
/// prohibition matching (D-PROP1 + D-EFFTREE1): a prohibited root prohibits
/// its whole subtree, the symmetric reading of ancestor-subsumption.
pub fn effects_covered(inferred: &EffectSet, set: &EffectSet) -> EffectSet {
    inferred
        .iter()
        .filter(|e| set.iter().any(|b| effect_covers(b, e)))
        .cloned()
        .collect()
}

/// True when `set` contains an effect rooted at `root` (the bare root itself,
/// or any leaf under it) — for the few call sites that only care about a
/// whole root regardless of leaf precision (D-WASM1's `Browser` web-partition
/// check).
pub fn effect_set_has_root(set: &EffectSet, root: Effect) -> bool {
    set.iter().any(|e| effect_root(e) == root.name())
}

impl<'a> super::Checker<'a> {
    /// D-EFF1: record an effect this function reaches directly — into the
    /// function's set and every open `#Caps(…)` region (which must account for
    /// effects reached inside it, E0741).
    pub(crate) fn record_effect(&mut self, e: &str) {
        self.fx_direct.insert(e.to_string());
        for r in &mut self.region_stack {
            r.direct.insert(e.to_string());
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
    /// D-EFF2 (callback param bound): record an obligation that the callback just
    /// walked (whose effect contribution is the delta of `fx_direct`/`fx_edges`/
    /// `fx_maximal` between `before` and now) satisfies the parameter's declared
    /// bound. `bound_names` is the raw `@Pure`/`#(…)` list off the parameter type
    /// (empty = `@Pure`); names are validated here (E0119) and the obligation is
    /// checked against the resolved callback effects in the post-pass (E0747).
    pub(crate) fn record_callback_obligation(
        &mut self,
        bound_names: &[(String, Span)],
        before_direct: &EffectSet,
        before_edges: &BTreeSet<String>,
        before_maximal: bool,
        span: Span,
    ) {
        let mut bound: EffectSet = EffectSet::new();
        for (name, nspan) in bound_names {
            match parse_effect_name(name) {
                Some(e) => {
                    bound.insert(e);
                }
                None => {
                    self.diags.push(unknown_effect(name, *nspan));
                    return; // a bad name leaves the bound incomplete; skip the check
                }
            }
        }
        let direct: EffectSet = self.fx_direct.difference(before_direct).cloned().collect();
        let edges: BTreeSet<String> = self.fx_edges.difference(before_edges).cloned().collect();
        let maximal = self.fx_maximal && !before_maximal;
        self.fx_callback_obligations.push(CallbackObligation {
            bound,
            direct,
            edges,
            maximal,
            span,
        });
    }

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

/// Render a set as `Net, Fs.Read` (canonical order) for diagnostics.
pub fn show_set(set: &EffectSet) -> String {
    set.iter().cloned().collect::<Vec<_>>().join(", ")
}

/// The effect carried by a Core call `module.method`, or `None` if pure.
/// Grounded in the real Core API surface (CheckerCoreLib). The `module` is the
/// fully-resolved name (`core.files`, `core.http`, …); legacy internal ring
/// keys are normalized through the foundation resolver before matching.
pub fn core_effect(module: &str, method: &str) -> Option<Effect> {
    let normalized_module =
        Syntax::normalize_core_module(module).unwrap_or_else(|| module.to_string());
    let module = normalized_module.as_str();
    // D-DET1: the deterministic capability constructors carry NO ambient effect —
    // `time.clock(seed)` / `random.rng(seed)` build a reproducible `Clock`/`Rng`
    // from a caller-supplied seed (a pure value). Reading time/randomness THROUGH
    // the resulting handle (`clock.now()` / `rng.int(…)`) is a method call on a
    // value, not a module call, so it never reaches `core_effect`. This lets a
    // `@Pure fn` take and use an injected `Clock`/`Rng` while ambient `time.now()`
    // / `random.int(…)` stay rejected (E3403).
    // D-DET-CAPAPI: duration/civil constructors mint deterministic values — a
    // deterministic value constructor, so (like `time.clock`) it carries no effect.
    if matches!(
        (module, method),
        (
            "core.time",
            "clock"
                | "ms"
                | "secs"
                | "seconds"
                | "minutes"
                | "hours"
                | "local_time"
                | "parse_time"
                | "period"
                | "period_days"
                | "period_months"
                | "period_years"
                | "utc"
                | "zoned"
                | "zoned_local"
        ) | ("core.random", "rng")
    ) {
        return None;
    }
    if module == "core.watcher" {
        return match method {
            "files" => Some(Effect::Fs),
            "process_pid" => Some(Effect::Exec),
            "port" => Some(Effect::Net),
            "set" => None,
            _ => None,
        };
    }
    Some(match module {
        "core.files" => Effect::Fs,
        "core.net" | "core.tls" | "jet.http" | "core.http.client" | "core.http.server" => Effect::Net,
        // D-RAYLIB1=A: windowing/drawing/input/audio bridge.
        "core.raylib" => Effect::Gpu,
        "core.time" => Effect::Time,
        "core.random" | "core.crypto.random" => Effect::Rand,
        "core.env" => Effect::Env,
        "core.process" => Effect::Exec,
        "core.io" => Effect::Io,
        "jet.db" | "jet.sql" => Effect::Db,
        // D-DEP-WASM1=A (c81): loading a sandboxed plugin executes foreign
        // code, even though the sandbox makes it memory-safe — same bucket as
        // `core.process` (an effects-budget `deny: [Exec]` also denies plugins).
        "core.plugin" | "jet.plugin" => Effect::Exec,
        "jet.log" => Effect::Log,
        "core.ui" | "core.web" | "core.web.storage.local" | "core.web.storage.session" => {
            Effect::Browser
        }
        // U13 (D-JPK-SECRETCRYPTO1): `core.vault.get` reads a decrypted repo
        // secret — never `core.secrets` (D-TTLVAL1's in-memory
        // Expiring/Rotting<T> wrapper already owns that module name).
        "core.vault" => Effect::Secret,
        _ => return None,
    })
}

/// D-TXN2: the irreversible effects — a network, filesystem, or subprocess
/// effect that, once performed, cannot be rolled back. These are rejected when
/// reached directly inside a `#Transact { … }` block (E0746). The remaining
/// effects (Io/Time/Rand/Env/Db/Log/Gpu) are reversible-or-benign for this
/// purpose: reads, clock/RNG reads, and logging leave no committed external
/// state a rollback must undo, and Db rollback is the transaction's own job.
pub fn is_irreversible_effect(e: Effect) -> bool {
    matches!(e, Effect::Net | Effect::Fs | Effect::Exec)
}

/// E0746 (D-TXN2): an irreversible effect (Net/Fs/Exec) used directly inside a
/// `#Transact { … }` block. Points at the offending call; the fix is to move it
/// after the block or register it via `name.on_commit(() => { … })`.
pub fn e0746(api: &str, e: Effect, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0746",
        format!(
            "`{}` has the `{}` effect, which can't be rolled back inside a `#{}` block",
            api, e.name(), crate::Syntax::KW_TRANSACT
        ),
        format!(
            "a `#{}` block undoes its work on a `?`-failure; a network, file, or subprocess effect (`{}`) leaves committed external state a rollback can't take back",
            crate::Syntax::KW_TRANSACT, e.name()
        ),
        format!(
            "move this call after the block, or register it with `<handle>.{}(() => {{ … }})` so it runs only on a clean commit",
            crate::Syntax::TXN_ON_COMMIT
        ),
        Some(span),
    )
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
    /// D-EFF2 (callback param bound): obligations recorded at each call to a
    /// higher-order fn whose function-typed parameter carries an effect bound
    /// (`@Pure fn(…)` / `#(E) fn(…)`). Checked against the actual callback's
    /// resolved effects in the post-pass — E0747.
    pub callback_obligations: Vec<CallbackObligation>,
}

/// D-SEMINDEX1: per-function effect summaries and the solved transitive sets,
/// captured during `check_bundle` for the public semantic-index API.
#[derive(Debug, Clone, Default)]
pub struct SemIndexEffectFacts {
    pub summaries: HashMap<String, EffectSummary>,
    pub solved: HashMap<String, EffectSet>,
    /// Reference targets proven by sema/name resolution. Key is
    /// `(module path, reference start, reference end)`. Tooling may copy these
    /// facts but must never independently resolve by spelling or proximity.
    pub reference_anchors: HashMap<(String, usize, usize), DefinitionAnchorFact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionAnchorFact {
    pub module_path: String,
    pub kind: String,
    pub def_span: Span,
}

/// D-EFF2 (callback param bound): one obligation that a callback argument passed
/// to a `@Pure fn(…)` / `#(E) fn(…)` parameter satisfies the declared bound. The
/// callback's own effect contribution is captured as the delta of the function's
/// effect accumulator across the argument walk (its direct effects, its
/// call-graph edges, and whether it forced the maximal set). Edges are resolved
/// against the whole-program `solved` map in the post-pass.
#[derive(Debug, Clone)]
pub struct CallbackObligation {
    /// The declared bound — the most effects the callback may carry.
    pub bound: EffectSet,
    /// Effects the callback reaches directly (delta of `fx_direct`).
    pub direct: EffectSet,
    /// Call-graph edges the callback introduces (delta of `fx_edges`).
    pub edges: BTreeSet<String>,
    /// The callback forced the maximal set (delta of `fx_maximal`) — an unknown /
    /// escaping callback value (D-EFF2 sound default).
    pub maximal: bool,
    /// Span of the callback argument, for the diagnostic.
    pub span: Span,
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
    /// D-SCAP1: true for a `#Grant(…)` region (authorizes the listed effects via
    /// a handle), false for a `#Caps(…)` region (restricts to the listed set).
    /// Both share the subset machinery; the flag selects the diagnostic — an
    /// out-of-set effect is E0712 (no capability) for a grant, E0741 (out of the
    /// restriction) for caps.
    pub grant: bool,
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
    /// Span of the `#Caps(…)` / `#Grant(…)` list, for the diagnostic.
    pub caps_span: Span,
    /// D-SCAP1: a `#Grant(…)` region (E0712 on overflow) vs `#Caps(…)` (E0741).
    pub grant: bool,
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
    // Worklist fixpoint. The lattice is finite (≤11 effects per node), so a
    // simple round-robin until no set grows terminates quickly.
    let mut changed = true;
    while changed {
        changed = false;
        for (k, s) in summaries {
            let mut add: EffectSet = BTreeSet::new();
            for callee in &s.edges {
                if let Some(cs) = sets.get(callee) {
                    for e in cs {
                        add.insert(e.clone());
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

/// D-EFF2 (`#(via f)` pass-through): seed each via-fn's effect summary with the
/// declared bound of its callback parameter `f`, so its published effect set is a
/// tight pass-through of `f` (the set that holds even when the callback value
/// escapes — the conservative flow-through can't see a callback the body stores
/// or returns). Runs over the assembled summaries **before** the fixpoint solve.
///
/// - `f` must be a parameter of this function whose type is a function type
///   (`Type::Fn`), else E0748.
/// - `f`'s declared bound (`@Pure` → empty, `#(E)` → that set) is the pass-through
///   set. An **unbounded** callback param (`f: fn(T)`) publishes the maximal set
///   (sound: an unconstrained callback may do anything).
/// - `#(via f)` and a `#(…)` effect list are mutually exclusive at parse time, so
///   there is no interaction with E0740 here.
pub fn apply_effect_via(
    items: &[crate::AST::Item],
    summaries: &mut HashMap<String, EffectSummary>,
    diags: &mut Vec<Diagnostic>,
) {
    use crate::AST::{Item, Type};
    fn one(
        f: &crate::AST::Func,
        owner: Option<&str>,
        summaries: &mut HashMap<String, EffectSummary>,
        diags: &mut Vec<Diagnostic>,
    ) {
        let Some((param, via_span)) = &f.effect_via else {
            return;
        };
        let Some(p) = f.params.iter().find(|p| &p.name == param) else {
            diags.push(e0748_unknown_param(&f.name, param, *via_span));
            return;
        };
        let Type::Fn { effect_bound, .. } = &p.ty else {
            diags.push(e0748_not_callback(&f.name, param, p.ty.name(), *via_span));
            return;
        };
        // The pass-through set: the param's declared bound, or maximal if unbounded.
        let mut via_set = EffectSet::new();
        let mut maximal = false;
        match effect_bound {
            Some(names) => {
                for (n, ns) in names {
                    match parse_effect_name(n) {
                        Some(e) => {
                            via_set.insert(e);
                        }
                        None => diags.push(unknown_effect(n, *ns)),
                    }
                }
            }
            None => maximal = true,
        }
        let key = super::effect_key(owner, &f.name);
        let entry = summaries.entry(key).or_default();
        entry.direct.extend(via_set);
        entry.maximal |= maximal;
    }
    for item in items {
        match item {
            Item::Func(f) => one(f, None, summaries, diags),
            Item::Impl(i) => {
                for m in &i.methods {
                    one(m, Some(&i.type_name), summaries, diags);
                }
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    one(m, Some(&s.name), summaries, diags);
                }
                for block in &s.trait_impls {
                    for m in &block.methods {
                        one(m, Some(&s.name), summaries, diags);
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    one(m, Some(&e.name), summaries, diags);
                }
            }
            _ => {}
        }
    }
}

/// E0748 (D-EFF2): `#(via f)` names `f`, which is not a parameter of this function.
pub fn e0748_unknown_param(fn_name: &str, param: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0748",
        format!("`#(via {})` on `{}` names no such parameter", param, fn_name),
        format!(
            "`#(via f)` publishes the effects of a callback parameter `f`; `{}` has no parameter called `{}`",
            fn_name, param
        ),
        format!("name one of `{}`'s callback parameters after `via`", fn_name),
        Some(span),
    )
}

/// E0748 (D-EFF2): `#(via f)` names a parameter that is not a function type.
pub fn e0748_not_callback(fn_name: &str, param: &str, ty: String, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0748",
        format!("`#(via {})` on `{}` names a parameter that isn't a callback", param, fn_name),
        format!(
            "`#(via f)` publishes the effects of a *function* parameter; `{}` is `{}`, not a `fn(…)` type",
            param, ty
        ),
        format!("point `via` at a parameter whose type is a function, or drop the `#(via {})` annotation", param),
        Some(span),
    )
}

/// D-EFF2: check every callback-bound obligation across the program. For each
/// obligation, resolve the callback's effects (its direct effects, plus the
/// maximal set if it escaped, plus the transitive effects of every edge from
/// `solved`) and verify the result is a subset of the declared bound. Any effect
/// beyond the bound is E0747.
pub fn check_callback_bounds(
    summaries: &HashMap<String, EffectSummary>,
    solved: &HashMap<String, EffectSet>,
    diags: &mut Vec<Diagnostic>,
) {
    for summary in summaries.values() {
        for ob in &summary.callback_obligations {
            let mut cb = ob.direct.clone();
            if ob.maximal {
                cb.extend(Effect::all());
            }
            for callee in &ob.edges {
                if let Some(cs) = solved.get(callee) {
                    cb.extend(cs.iter().cloned());
                }
            }
            let over: EffectSet = effects_uncovered(&cb, &ob.bound);
            if !over.is_empty() {
                diags.push(e0747(&over, &ob.bound, ob.span));
            }
        }
    }
}

/// D-REPLAY1: `#Replayable` functions may not reach ambient
/// Time/Rand/Net/Io. Deterministic handles (`time.clock(seed)`,
/// `random.rng(seed)`, mockable capability objects) stay valid because they do
/// not enter the ambient Core-call effect graph.
pub fn check_replayable_effects(
    items: &[crate::AST::Item],
    solved: &HashMap<String, EffectSet>,
    diags: &mut Vec<Diagnostic>,
) {
    use crate::AST::Item;

    fn replay_forbidden(effects: &EffectSet) -> EffectSet {
        let roots = [Effect::Time, Effect::Rand, Effect::Net, Effect::Io];
        effects
            .iter()
            .filter(|effect| roots.iter().any(|root| effect_root(effect) == root.name()))
            .cloned()
            .collect()
    }

    fn check_one(
        f: &crate::AST::Func,
        owner: Option<&str>,
        solved: &HashMap<String, EffectSet>,
        diags: &mut Vec<Diagnostic>,
    ) {
        if !f.is_replayable {
            return;
        }
        let inferred = solved
            .get(&super::effect_key(owner, &f.name))
            .cloned()
            .unwrap_or_default();
        let forbidden = replay_forbidden(&inferred);
        if !forbidden.is_empty() {
            diags.push(e0725(
                &f.name,
                &forbidden,
                f.replayable_span.unwrap_or(f.name_span),
            ));
        }
    }

    for item in items {
        match item {
            Item::Func(f) => check_one(f, None, solved, diags),
            Item::Impl(i) => {
                for m in &i.methods {
                    check_one(m, Some(&i.type_name), solved, diags);
                }
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    check_one(m, Some(&s.name), solved, diags);
                }
                for block in &s.trait_impls {
                    for m in &block.methods {
                        check_one(m, Some(&s.name), solved, diags);
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    check_one(m, Some(&e.name), solved, diags);
                }
            }
            _ => {}
        }
    }
}

/// E0725 (D-REPLAY1): a `#Replayable` function reaches ambient nondeterminism.
pub fn e0725(fn_name: &str, effects: &EffectSet, span: Span) -> Diagnostic {
    let effect_list = show_set(effects);
    Diagnostic::error(
        "E0725",
        format!(
            "`{}` is `#Replayable` but reaches `{}`",
            fn_name, effect_list
        ),
        "`#Replayable` code must replay from explicit inputs; ambient time, randomness, network, or console IO would make the same replay diverge"
            .to_string(),
        "inject a deterministic clock/RNG or mockable capability, pass recorded data in, or move the ambient effect outside the replayable function"
            .to_string(),
        Some(span),
    )
}

/// E0747 (D-EFF2): a callback argument carries an effect the parameter's bound
/// doesn't allow — a `@Pure fn(…)` parameter handed an impure callback, or a
/// `#(E) fn(…)` parameter handed one that reaches an effect outside `E`.
pub fn e0747(over: &EffectSet, bound: &EffectSet, span: Span) -> Diagnostic {
    let over_list = show_set(over);
    let bound_desc = if bound.is_empty() {
        format!(
            "the parameter is `@{} fn(…)`, so the callback must be pure",
            crate::Syntax::KW_PURE
        )
    } else {
        format!(
            "the parameter is `#({}) fn(…)`, so the callback may use only those effects",
            show_set(bound)
        )
    };
    let fix = if bound.is_empty() {
        format!(
            "pass a `@{} fn` (or a lambda that uses no effects), or widen the parameter's bound",
            crate::Syntax::KW_PURE
        )
    } else {
        format!(
            "pass a callback within `#({})`, or add `{}` to the parameter's bound",
            show_set(bound),
            over_list
        )
    };
    Diagnostic::error(
        "E0747",
        format!(
            "this callback uses the effect `{}`, which the parameter doesn't allow",
            over_list
        ),
        format!("{}; `{}` is outside that bound", bound_desc, over_list),
        fix,
        Some(span),
    )
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
        format!(
            "add `{}` to the effect list, or stop using it in `{}`",
            over_list, fn_name
        ),
        Some(span),
    )
}

/// E0749 (D-PROP1=A): a function's reachable call graph uses a prohibited
/// effect. `reached` is the actual offending effect(s); `prohibited` is the
/// declared `#(!…)` set that covers them — D-EFFTREE1: since ancestor
/// prohibits descendant, `reached` may name a leaf (`Fs.Write`) under a
/// broader declared root (`Fs`), so the two are shown separately rather than
/// assuming they're always the same text (they always were, pre-D-EFFTREE1).
pub fn e0749(fn_name: &str, reached: &EffectSet, prohibited: &EffectSet, span: Span) -> Diagnostic {
    let reached_list = show_set(reached);
    let decl_list = show_set(prohibited);
    Diagnostic::error(
        "E0749",
        format!(
            "`{}` reaches the `{}` effect, which it prohibits with `#(!{})`",
            fn_name, reached_list, decl_list
        ),
        format!(
            "a `#(!{})` annotation means the function and every callee it can reach must not use `{}`",
            decl_list, reached_list
        ),
        format!(
            "remove the call that introduces `{}`, or drop the `#(!{})` prohibition",
            reached_list, decl_list
        ),
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
                    inferred.extend(cs.iter().cloned());
                }
            }
            let over: EffectSet = effects_uncovered(&inferred, &region.caps);
            if !over.is_empty() {
                if region.grant {
                    diags.push(e0712(&over, &region.caps, region.caps_span));
                } else {
                    diags.push(e0741(&over, &region.caps, region.caps_span));
                }
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

/// D-SCAP1: detect whether the capability handle `handle` bound by a `#Grant(…)`
/// region escapes its block — returned, stored, passed, captured, or otherwise
/// used as a value that outlives the scope. Returns the span of the first escape,
/// or `None` if the handle is only ever used in place (as the receiver of a
/// method call / field access / `?` on itself). The receiver of `handle.read(…)`
/// is an in-place use (performing the granted effect); everything else — `return
/// handle`, `x :: handle`, `f(handle)`, `[handle]`, a struct field, an `or`
/// fallback — lets the revoked authority leak past the grant (E0711).
pub fn grant_handle_escape(body: &[crate::AST::Stmt], handle: &str) -> Option<Span> {
    body.iter().find_map(|s| stmt_handle_escape(s, handle))
}

fn stmt_handle_escape(stmt: &crate::AST::Stmt, handle: &str) -> Option<Span> {
    use crate::AST::{ForKind, Stmt};
    let block = |b: &[Stmt]| b.iter().find_map(|s| stmt_handle_escape(s, handle));
    match stmt {
        Stmt::Expr(e) | Stmt::Yield(e, _) => expr_handle_escape(e, handle),
        Stmt::Val(b) => expr_handle_escape(&b.init, handle),
        Stmt::Assign { value, .. } => expr_handle_escape(value, handle),
        Stmt::Return(Some(e), _) => expr_handle_escape(e, handle),
        Stmt::If(i) => expr_handle_escape(&i.cond, handle)
            .or_else(|| block(&i.then_body))
            .or_else(|| {
                i.else_branch
                    .as_ref()
                    .and_then(|e| else_handle_escape(e, handle))
            }),
        Stmt::While { cond, body, .. } => expr_handle_escape(cond, handle).or_else(|| block(body)),
        Stmt::For { kind, body, .. } => {
            let coll = match kind {
                ForKind::Range { start, end, step } => expr_handle_escape(start, handle)
                    .or_else(|| expr_handle_escape(end, handle))
                    .or_else(|| step.as_ref().and_then(|s| expr_handle_escape(s, handle))),
                ForKind::In { collection } => expr_handle_escape(collection, handle),
            };
            coll.or_else(|| block(body))
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        }
        | Stmt::ComptimeSwitch {
            subject,
            arms,
            else_body,
            ..
        } => expr_handle_escape(subject, handle)
            .or_else(|| {
                arms.iter()
                    .find_map(|a| expr_handle_escape(&a.cond, handle).or_else(|| block(&a.body)))
            })
            .or_else(|| else_body.as_ref().and_then(|b| block(b))),
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => expr_handle_escape(&init.init, handle)
            .or_else(|| expr_handle_escape(cond, handle))
            .or_else(|| block(body))
            .or_else(|| stmt_handle_escape(step, handle)),
        Stmt::Loop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::DebugOnly { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::ScopeMember { body, .. }
        | Stmt::Live { body, .. } => block(body),
        Stmt::Off { .. } => None,
        // D-CTMARKER1: comptime block erases; no handle can escape a build-time block.
        Stmt::ComptimeBlock { .. } => None,
        Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            ..
        } => expr_handle_escape(cond, handle)
            .or_else(|| block(then_body))
            .or_else(|| else_body.as_ref().and_then(|b| block(b))),
        Stmt::ContextBlock { fields, body, .. } => fields
            .iter()
            .find_map(|(_, e, _)| expr_handle_escape(e, handle))
            .or_else(|| block(body)),
        Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::BreakLabel(..)
        | Stmt::ContinueLabel(..)
        | Stmt::Return(None, _) => None,
    }
}

fn else_handle_escape(e: &crate::AST::ElseBranch, handle: &str) -> Option<Span> {
    use crate::AST::ElseBranch;
    match e {
        ElseBranch::Else(stmts) => stmts.iter().find_map(|s| stmt_handle_escape(s, handle)),
        ElseBranch::ElseIf(i) => expr_handle_escape(&i.cond, handle)
            .or_else(|| {
                i.then_body
                    .iter()
                    .find_map(|s| stmt_handle_escape(s, handle))
            })
            .or_else(|| {
                i.else_branch
                    .as_ref()
                    .and_then(|e| else_handle_escape(e, handle))
            }),
    }
}

/// True when `e` is the bare handle ident (the root of an in-place chain). A bare
/// reference to the handle is a *value-use* (escape) on its own; but as the
/// receiver of a method call / field access / `?` / index it is an in-place use
/// of the capability that does NOT carry it out of scope.
fn is_bare_handle(e: &crate::AST::Expr, handle: &str) -> bool {
    matches!(e, crate::AST::Expr::Ident(n, _) if n == handle)
}

/// The span of the first escaping use of `handle` in `e`, or `None`. A bare
/// reference to the handle in value position is an escape; a method-call /
/// field-access / `?` chain rooted at the handle is an in-place use, but the
/// chain's arguments and indices are still scanned (they can carry it out).
fn expr_handle_escape(e: &crate::AST::Expr, handle: &str) -> Option<Span> {
    use crate::AST::{EnumLitArg, Expr, OrFallback, StrPart};
    match e {
        // A bare value-position reference to the handle escapes.
        Expr::Ident(n, span) if n == handle => Some(*span),
        Expr::Ident(_, _) => None,
        // A method call / field / `?` / deref / index whose receiver is the bare
        // handle is an in-place use (performing the granted effect) — NOT an
        // escape; but the call's args and the index are still scanned.
        Expr::MethodCall { receiver, args, .. } if is_bare_handle(receiver, handle) => {
            args.iter().find_map(|a| expr_handle_escape(&a.expr, handle))
        }
        Expr::Field(receiver, _, _) if is_bare_handle(receiver, handle) => None,
        Expr::OptField { base, .. } if is_bare_handle(base, handle) => None,
        Expr::Try(receiver, _, _) if is_bare_handle(receiver, handle) => None,
        Expr::Deref(receiver, _) if is_bare_handle(receiver, handle) => None,
        Expr::Index { base, index, .. } if is_bare_handle(base, handle) => {
            expr_handle_escape(index, handle)
        }
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Tainted(inner, _) // D-TAINT1: tag erased; recurse into the value.
        | Expr::Present(inner, _) | Expr::Ok(inner, _) | Expr::Err(inner, _)
        | Expr::Try(inner, _, _) | Expr::Field(inner, _, _) => expr_handle_escape(inner, handle),
        Expr::OptField { base, .. } => expr_handle_escape(base, handle),
        Expr::Binary(_, l, r, _) => {
            expr_handle_escape(l, handle).or_else(|| expr_handle_escape(r, handle))
        }
        Expr::CompareChain { operands, .. } => {
            operands.iter().find_map(|e| expr_handle_escape(e, handle))
        }
        Expr::Call(c) => c.args.iter().find_map(|a| expr_handle_escape(&a.expr, handle)),
        Expr::CallValue { callee, args, .. } => expr_handle_escape(callee, handle)
            .or_else(|| args.iter().find_map(|a| expr_handle_escape(&a.expr, handle))),
        Expr::MethodCall { receiver, args, .. } => expr_handle_escape(receiver, handle)
            .or_else(|| args.iter().find_map(|a| expr_handle_escape(&a.expr, handle))),
        Expr::Index { base, index, .. } => {
            expr_handle_escape(base, handle).or_else(|| expr_handle_escape(index, handle))
        }
        Expr::Slice { base, start, end, .. } => expr_handle_escape(base, handle)
            .or_else(|| expr_handle_escape(start, handle))
            .or_else(|| expr_handle_escape(end, handle)),
        Expr::ListLit(elems, _) => elems.iter().find_map(|el| expr_handle_escape(el, handle)),
        Expr::TupleLit(fields, _, _) => {
            fields.iter().find_map(|(_, e)| expr_handle_escape(e, handle))
        }
        Expr::MapLit(entries, _) => entries
            .iter()
            .find_map(|(k, v)| expr_handle_escape(k, handle).or_else(|| expr_handle_escape(v, handle))),
        Expr::StructLit { fields, .. } => {
            fields.iter().find_map(|(_, _, f)| expr_handle_escape(f, handle))
        }
        Expr::EnumLit { args, .. } => args.iter().find_map(|a| match a {
            EnumLitArg::Positional(e) => expr_handle_escape(e, handle),
            EnumLitArg::Named { expr, .. } => expr_handle_escape(expr, handle),
        }),
        Expr::OrFallback { value, fallback, .. } => expr_handle_escape(value, handle).or_else(|| {
            match fallback {
                OrFallback::Value(e) => expr_handle_escape(e, handle),
                OrFallback::Return(Some(e), _) => expr_handle_escape(e, handle),
                _ => None,
            }
        }),
        Expr::PatternTest { subject, .. } => expr_handle_escape(subject, handle),
        Expr::Str(parts, _) => parts.iter().find_map(|p| match p {
            StrPart::Interp(e, _) => expr_handle_escape(e, handle),
            _ => None,
        }),
        Expr::If { cond, then_body, then_value, else_body, else_value, .. } => {
            expr_handle_escape(cond, handle)
                .or_else(|| expr_handle_escape(then_value, handle))
                .or_else(|| expr_handle_escape(else_value, handle))
                .or_else(|| then_body.iter().find_map(|s| stmt_handle_escape(s, handle)))
                .or_else(|| else_body.iter().find_map(|s| stmt_handle_escape(s, handle)))
        }
        Expr::FanOut { callee, items, .. } => expr_handle_escape(callee, handle)
            .or_else(|| items.iter().find_map(|e| expr_handle_escape(e, handle))),
        Expr::PtrFromAddr { addr, .. } => expr_handle_escape(addr, handle),
        // A lambda that captures the handle smuggles it out — the closure can
        // outlive the grant. Count a reference unless a lambda param shadows the
        // handle name (then it's a different binding, not a capture).
        Expr::Lambda(l) => {
            let shadowed = l.params.iter().any(|p| p.name == handle)
                || l.take_names.iter().any(|(n, _)| n == handle);
            if !shadowed && super::lambda_body_refs_name(&l.body, handle) {
                Some(l.span)
            } else {
                None
            }
        }
        Expr::Int(..)
        | Expr::Float(..)
        | Expr::Bool(..)
        | Expr::Char(..)
        | Expr::Absent(_)
        | Expr::ReduceMarker(_, _)
        | Expr::Todo { .. }
        | Expr::UnitLit { .. }
        | Expr::ComptimeSplice { .. }
        // D-SHIFT1 (c7shift): a leaf literal, no nested `Expr` to recurse into.
        | Expr::StrMatchLit(_, _) => None,
        Expr::Paren(inner, _) => expr_handle_escape(inner, handle),
        Expr::Spread(inner, _) => expr_handle_escape(inner, handle),
    }
}

/// E0712 (D-SCAP1): an effect used inside a `#Grant(…)` region that the grant
/// doesn't authorize — there is no capability in scope backing it. The dual of
/// E0741: `#Grant(…)` *authorizes* exactly the listed effects through its handle,
/// so an effect reached inside (even through a call) that the grant omits has no
/// capability to perform it.
pub fn e0712(over: &EffectSet, caps: &EffectSet, span: Span) -> Diagnostic {
    let over_list = show_set(over);
    let caps_list = if caps.is_empty() {
        "no effects".to_string()
    } else {
        format!("`{}`", show_set(caps))
    };
    Diagnostic::error(
        "E0712",
        format!(
            "this `#{}` region uses the effect `{}`, which it has no capability for",
            crate::Syntax::KW_GRANT, over_list
        ),
        format!(
            "`#{}(…)` grants only {}; an effect reached inside — even through a call — needs a capability in scope to perform it",
            crate::Syntax::KW_GRANT, caps_list
        ),
        format!(
            "add `{}` to the `#{}(…)` list, or move that work outside the grant",
            over_list, crate::Syntax::KW_GRANT
        ),
        Some(span),
    )
}

/// E0711 (D-SCAP1): the capability handle bound by a `#Grant(…)` region escapes
/// its scope — returned, stored in an outer binding, or captured by an escaping
/// value. The capability is revoked at scope end (RAII, S63), so a reference that
/// outlives the block would name a revoked authority.
pub fn e0711(handle: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0711",
        format!("the capability `{}` can't escape its `#{}` block", handle, crate::Syntax::KW_GRANT),
        format!(
            "`#{}(…)` revokes the capability at the end of its block (RAII); returning, storing, or sharing `{}` would let a revoked authority outlive the grant",
            crate::Syntax::KW_GRANT, handle
        ),
        format!("use `{}` only inside the `#{}` block, or perform the work there", handle, crate::Syntax::KW_GRANT),
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

/// E0742 (D-EFF3): an impl of a trait method uses effects beyond the upper
/// bound the trait method declares (`@Pure fn …` / `fn … #(Gpu)`).
pub fn e0742(
    trait_name: &str,
    method: &str,
    over: &EffectSet,
    bound: &EffectSet,
    span: Span,
) -> Diagnostic {
    let over_list = show_set(over);
    let bound_desc = if bound.is_empty() {
        format!(
            "`{}` declares `{}` `@{}`, so impls must be pure",
            trait_name,
            method,
            crate::Syntax::KW_PURE
        )
    } else {
        format!(
            "`{}` bounds `{}` to `#({})`, so impls may use only those",
            trait_name,
            method,
            show_set(bound)
        )
    };
    Diagnostic::error(
        "E0742",
        format!(
            "this `{}` impl uses the effect `{}`, which the trait doesn't allow",
            method, over_list
        ),
        format!("{}; `{}` is outside that bound", bound_desc, over_list),
        format!(
            "remove the `{}` work from this impl, or widen the bound on the trait method",
            over_list
        ),
        Some(span),
    )
}

/// D-EFF3: enforce trait-method effect upper bounds against each impl's inferred
/// effects. `trait_bounds[(trait, method)]` is `Some(bound)` when the trait
/// method declares one (`@Pure` → empty set). Impl methods are keyed
/// `Type::method` in `solved`. An impl whose inferred set exceeds the bound is
/// E0742.
pub fn check_trait_obligations(
    impls: &[(String, String, String, Span)],
    trait_bounds: &HashMap<(String, String), EffectSet>,
    solved: &HashMap<String, EffectSet>,
    diags: &mut Vec<Diagnostic>,
) {
    for (trait_name, type_name, method, span) in impls {
        let Some(bound) = trait_bounds.get(&(trait_name.clone(), method.clone())) else {
            continue;
        };
        let key = format!("{type_name}::{method}");
        let inferred = solved.get(&key).cloned().unwrap_or_default();
        let over: EffectSet = effects_uncovered(&inferred, bound);
        if !over.is_empty() {
            diags.push(e0742(trait_name, method, &over, bound, *span));
        }
    }
}

/// U13 (D-JPK-SECRETCRYPTO1): unlike every other effect, `Secret` is denied
/// even with no declared bound at all — `core.vault.get` demands an explicit
/// `#(Secret)` (or a wider declared bound covering it) on the calling
/// function; a bare `fn` with no `#(…)` list, or one that omits `Secret`, is
/// E1264. A helper fn two calls deep from the actual `core.vault.get` still
/// requires the grant on itself — the same call-graph reach E0740 checks —
/// but this deliberately does **not** use the general `solved` (whole-program)
/// set: `solve()` inflates a function's set to the maximal (all-effects) set
/// the instant it reaches *any* un-inspectable foreign (`extern rust`) call
/// (`fx_maximal`), which would force `#(Secret)` onto every FFI-using function
/// in the program regardless of whether it goes near a secret. So this walks
/// its own small fixpoint over `direct`/`edges` only — real, syntactically
/// traceable `core.vault.get` reaches, the same thing `apply_effect_via`/
/// `record_edge` already track, just excluding the `maximal` blanket.
pub(crate) fn check_secret_grants(
    items: &[crate::AST::Item],
    summaries: &HashMap<String, EffectSummary>,
    diags: &mut Vec<Diagnostic>,
) {
    let reaches_secret = solve_secret_reach(summaries);
    fn declared_set(f: &crate::AST::Func) -> EffectSet {
        let mut declared = EffectSet::new();
        let Some(list) = &f.declared_effects else {
            return declared;
        };
        for (name, _span) in list {
            if let Some(base) = name.strip_prefix('!') {
                // Prohibitions don't grant anything.
                let _ = base;
                continue;
            }
            if let Some(e) = parse_effect_name(name) {
                declared.insert(e);
            }
        }
        declared
    }
    fn check_one(
        f: &crate::AST::Func,
        owner: Option<&str>,
        reaches_secret: &std::collections::HashSet<String>,
        diags: &mut Vec<Diagnostic>,
    ) {
        let key = super::effect_key(owner, &f.name);
        if !reaches_secret.contains(&key) {
            return;
        }
        let declared = declared_set(f);
        if !declared
            .iter()
            .any(|e| effect_root(e) == Effect::Secret.name())
        {
            let span = f
                .declared_effects
                .as_ref()
                .and_then(|list| list.first())
                .map(|(_, s)| *s)
                .unwrap_or(f.name_span);
            diags.push(e1264(&f.name, span));
        }
    }
    use crate::AST::Item;
    for item in items {
        match item {
            Item::Func(f) => check_one(f, None, &reaches_secret, diags),
            Item::Impl(i) => {
                for m in &i.methods {
                    check_one(m, Some(&i.type_name), &reaches_secret, diags);
                }
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    check_one(m, Some(&s.name), &reaches_secret, diags);
                }
                for block in &s.trait_impls {
                    for m in &block.methods {
                        check_one(m, Some(&s.name), &reaches_secret, diags);
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    check_one(m, Some(&e.name), &reaches_secret, diags);
                }
            }
            _ => {}
        }
    }
}

/// U13: the set of function keys whose body reaches `Secret` through a real,
/// syntactically traceable path — direct effects and call-graph edges only,
/// deliberately ignoring `EffectSummary.maximal` (an opaque `extern rust`
/// call is not "reading a secret" for this specific mandatory-grant purpose;
/// see `check_secret_grants`'s doc comment).
fn solve_secret_reach(
    summaries: &HashMap<String, EffectSummary>,
) -> std::collections::HashSet<String> {
    let mut reaches: std::collections::HashSet<String> = summaries
        .iter()
        .filter(|(_, s)| effect_set_has_root(&s.direct, Effect::Secret))
        .map(|(k, _)| k.clone())
        .collect();
    let mut changed = true;
    while changed {
        changed = false;
        for (k, s) in summaries {
            if reaches.contains(k) {
                continue;
            }
            if s.edges.iter().any(|callee| reaches.contains(callee)) {
                reaches.insert(k.clone());
                changed = true;
            }
        }
    }
    reaches
}

/// E1264 (U13, D-JPK-SECRETCRYPTO1): a function reaches `core.vault.get`
/// (transitively) without `Secret` in its own declared `#(…)` bound.
pub fn e1264(fn_name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1264",
        format!(
            "`{}` reads a secret but doesn't declare the `Secret` effect",
            fn_name
        ),
        "reading a secret (`core.vault.get`) always requires an explicit grant — unlike other \
         effects, there is no silently-inferred default here, so a function must opt in even with \
         no other `#(…)` bound at all."
            .to_string(),
        format!(
            "add `#(Secret)` to `{}`'s signature (or widen an existing `#(…)` list to cover it)",
            fn_name
        ),
        Some(span),
    )
}

/// E0745: a `@Pure fn` also carries a non-empty `#(…)` bound — a contradiction.
pub fn e0745(fn_name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0745",
        format!("`{}` is `@{}` but also declares effects", fn_name, crate::Syntax::KW_PURE),
        format!(
            "`@{}` means the empty effect set; a `#(…)` list on the same function asks for both empty and non-empty",
            crate::Syntax::KW_PURE
        ),
        format!("drop the `#(…)` list to keep `{}` pure, or remove `@{}`", fn_name, crate::Syntax::KW_PURE),
        Some(span),
    )
}
