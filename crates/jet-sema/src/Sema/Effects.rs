//! Effect system (D-EFF1, D-QUAL1, D-EFF2, D-EFF3).
//!
//! Every function carries an inferred **effect set** — the categories of
//! ambient power its body exercises (network, filesystem, clock, …). The set is
//! inferred per-function, propagated along the call graph (Koka-style rows), and
//! **fully erased in codegen** (I3): effects are a compile-time proof with no
//! runtime value, handler, or monad. A function whose inferred set is empty is
//! pure (the ⊥ of the lattice).
//!
//! This module owns the effect vocabulary, the per-function summary the checker
//! accumulates during its walk, the shared fact-row traversal that turns those
//! summaries into transitive reachability facts, and the boundary diagnostics
//! (E0740 out-of-set against a declared effect-arrow bound). Casing is
//! PascalCase per D-CASING1.
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
//! D-EFFTREE1 (ratified 2026-07-03) amends D-EFF4/5: the 29 declared names below
//! become tree **roots**. A user-written effect name may now be a dotted path
//! rooted at one of them (`FS.Read`, `Net.HTTP.Get`) — the root is validated
//! against the closed vocabulary (E0119 otherwise); further segments are an
//! open, user-chosen leaf path with no fixed vocabulary of children (the same
//! shape as D-TAG1's tag-tree dotted paths). An `EffectSet` element is now the
//! canonical dotted string (`"FS"`, `"FS.Read"`) rather than a bare `Effect`.
//! **Ancestor matching is subsumption**: a bound entry covers any effect at or
//! below it in the tree (`effect_covers`) — the same ancestor-subtree rule as
//! D-TAG1's nested variant groups (CheckerCore.rs's switch-arm coverage:
//! `variant.starts_with(&format!("{c}."))`). `Effect` itself stays the closed
//! 29-root enum, used for root validation/
//! classification and by the small set of call sites (D-TXN2, D-TAINT1,
//! D-WASM1) that only ever care about a whole root regardless of leaf.

use crate::Diagnostics::{Diagnostic, Span};
/// D-META-EFFECT1: the effect facts live in `jet-foundation` so both stages
/// read one table. Sema keeps the solver, the diagnostics, and the checks.
pub use jet_foundation::Effects::{
    builtin_effect, core_effect, is_irreversible_effect, Effect, EffectSet,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// D-SHAPE8 open-row entry (`..E`). The parser stores row variables beside
/// concrete effects so every consumer can preserve the exact source spelling.
/// A row variable is instantiated by the callback passed at each call site;
/// it is never a concrete effect root of its own.
pub fn effect_row_var(name: &str) -> Option<&str> {
    name.strip_prefix("..").filter(|name| !name.is_empty())
}

/// The root segment of a dotted effect path (`"FS.Read"` → `"FS"`; a bare
/// name is its own root). D-EFFTREE1.
pub fn effect_root(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

/// D-EFFTREE1: validate a user-written effect path — bare (`FS`) or dotted
/// (`FS.Read`, `Net.HTTP.Get`). The root must be one of the closed 29
/// D-EFF4/5 names (the caller reports E0119 on `None`); further segments are
/// an open, user-chosen leaf path with no fixed vocabulary — mirrors D-TAG1's
/// tag-tree dotted paths. Returns the path unchanged (as the canonical form)
/// when the root is known.
pub fn parse_effect_name(name: &str) -> Option<String> {
    Effect::parse(effect_root(name))?;
    Some(name.to_string())
}

pub fn resolve_effect_name(
    name: &str,
    facts: &jet_foundation::Facts::FactRegistry,
) -> Result<String, Option<String>> {
    let root = effect_root(name);
    Effect::parse(root).ok_or(None)?;
    let Some(member) = name.strip_prefix(root).and_then(|suffix| suffix.strip_prefix('.')) else {
        return Ok(name.to_string());
    };
    let Some(declaration) =
        facts.get(jet_foundation::Facts::FactKind::Effect, root)
    else {
        return Ok(name.to_string());
    };
    if declaration.members.is_empty() || declaration.members.contains(member) {
        return Ok(name.to_string());
    }
    let suggestion = declaration
        .members
        .iter()
        .map(|candidate| {
            (
                crate::Syntax::edit_distance(member, candidate),
                format!("{root}.{candidate}"),
            )
        })
        .min_by(|left, right| left.cmp(right))
        .filter(|(distance, _)| *distance <= 3)
        .map(|(_, candidate)| candidate);
    Err(suggestion)
}

pub fn undeclared_effect(name: &str, suggestion: Option<&str>, span: Option<Span>) -> Diagnostic {
    Diagnostic::error(
        "E0750",
        format!("`{name}` isn't a declared effect"),
        format!(
            "the `{}` root has declared leaves, so every leaf under it must resolve to one declared name",
            effect_root(name)
        ),
        suggestion
            .map(|candidate| format!("did you mean `{candidate}`?"))
            .unwrap_or_else(|| format!("declare it with `effect {name}`, or use a declared leaf")),
        span,
    )
}

pub fn effect_leaf_required(root: &str, span: Option<Span>) -> Diagnostic {
    Diagnostic::error(
        "E0750",
        format!("`{root}` names an effect root, not a leaf"),
        "an effect declaration must name one leaf beneath its root".to_string(),
        format!("write `effect {root}.Name`"),
        span,
    )
}

/// D-EFFTREE1: does `bound` (one entry of a declared/granted/prohibited set)
/// cover `e`? Exact match, or `bound` is a dot-path ancestor of `e` — ancestor
/// subsumption, the same rule as D-TAG1's tag-tree subtree matching. A
/// root-only bound (`FS`) covers every leaf under it; a leaf bound
/// (`FS.Read`) covers only itself and any deeper path under it — a sibling
/// (`FS.Write`) is never covered.
pub fn effect_covers(bound: &str, e: &str) -> bool {
    jet_foundation::Facts::fact_covers(bound, e)
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
    /// D-BOUND-UNDO1=A: foreign calls are an explicit transaction boundary.
    /// Keep the policy here so every call-resolution route uses the same check;
    /// codegen only consumes the proven inverse fact.
    pub(crate) fn check_foreign_transaction_call(
        &mut self,
        sig: &crate::AST::FuncSig,
        api: &str,
        span: Span,
    ) {
        if !sig.is_extern {
            return;
        }
        if sig.foreign_effect_root.is_none() {
            self.record_effect(Effect::FFI.name(), span);
        }
        if self.txn_depth > 0 && sig.undo.is_none() {
            self.diags.push(e0746(api, Effect::FFI, span));
        }
    }

    /// D-EFF1: record an effect this function reaches directly — into the
    /// function's set and every open `#Caps(…)` region (which must account for
    /// effects reached inside it, E0741).
    pub(crate) fn record_effect(&mut self, e: &str, span: Span) {
        self.fx_direct.insert(e.to_string());
        self.fx_direct_spans.entry(e.to_string()).or_insert(span);
        for r in &mut self.region_stack {
            r.direct.insert(e.to_string());
        }
    }

    /// D-EFF1: record a call-graph edge to a user function `name` — into the
    /// function's edges and every open `#Caps(…)` region.
    pub(crate) fn record_edge(&mut self, name: String, span: Span) {
        self.record_edge_with_executions(name, span, self.memory_control_multiplier);
    }

    fn record_edge_with_executions(
        &mut self,
        name: String,
        span: Span,
        executions: Option<u64>,
    ) {
        for r in &mut self.region_stack {
            r.edges.insert(name.clone());
        }
        for r in &mut self.memory_policy_stack {
            r.edges.insert(name.clone());
        }
        let call = super::MemoryFacts::MemoryCall {
            callee: name.clone(),
            span,
            source: self.module_path.to_string(),
            executions,
        };
        for r in &mut self.memory_policy_stack {
            r.calls.push(call.clone());
        }
        self.fx_memory_calls.push(call);
        self.fx_edges.insert(name);
    }

    /// D-EFF1: record that a foreign (`extern`) call was reached — forcing the
    /// maximal set on the function and every open `#Caps(…)` region.
    pub(crate) fn record_maximal(&mut self, span: Span) {
        self.fx_maximal = true;
        self.fx_maximal_span.get_or_insert(span);
        self.record_open_memory_dispatch(span, "foreign or dynamically selected function body");
        for r in &mut self.region_stack {
            r.maximal = true;
        }
    }

    pub(crate) fn record_open_memory_dispatch(&mut self, span: Span, reason: &str) {
        let dispatch = super::MemoryFacts::OpenMemoryDispatch {
            span,
            source: self.module_path.to_string(),
            reason: reason.to_string(),
        };
        for region in &mut self.memory_policy_stack {
            region.open_dispatches.push(dispatch.clone());
        }
        self.fx_memory_open.push(dispatch);
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
    ///   The expert levers `fn(…) =[E]=>` param types and `#(via f)` tighten this.
    /// D-EFF2 (callback param bound): record an obligation that the callback just
    /// walked (whose effect contribution is the delta of `fx_direct`/`fx_edges`/
    /// `fx_maximal` between `before` and now) satisfies the parameter's declared
    /// bound. `bound_names` is the raw `#Pure`/`#(…)` list off the parameter type
    /// (empty = `#Pure`); names are validated here (E0119) and the obligation is
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
        let mut open = false;
        for (name, nspan) in bound_names {
            if effect_row_var(name).is_some() {
                open = true;
                continue;
            }
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
        // `..E` instantiates to the callback's remaining effects. Treating it
        // as a closed bound would produce a false E0747.
        if open {
            return;
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
                // The callee is sealed, but the higher-order function may
                // invoke this callback any number of times.
                self.record_edge_with_executions(name.clone(), arg.span(), None);
            }
            _ => self.record_maximal(arg.span()),
        }
    }

    pub(crate) fn record_memory_event(&mut self, mut event: super::MemoryFacts::MemoryEvent) {
        event.executions = self.memory_control_multiplier;
        for region in &mut self.memory_policy_stack {
            region.events.push(event.clone());
        }
        self.fx_memory_events.push(event);
    }

    pub(crate) fn enter_memory_policy_region(
        &mut self,
        declarations: Vec<crate::Policy::PolicyDeclaration>,
        span: Span,
    ) {
        let mut inherited = self
            .memory_policy_stack
            .last()
            .map(|region| region.declarations.clone())
            .unwrap_or_default();
        inherited.extend(declarations);
        self.memory_policy_stack.push(super::MemoryFacts::MemoryPolicyRegion {
            declarations: inherited,
            span,
            events: Vec::new(),
            edges: BTreeSet::new(),
            open_dispatches: Vec::new(),
            unbounded_control: Vec::new(),
            calls: Vec::new(),
        });
    }

    pub(crate) fn exit_memory_policy_region(&mut self) {
        if let Some(region) = self.memory_policy_stack.pop() {
            self.fx_memory_regions.push(region);
        }
    }
}

/// Render a set as `Net, FS.Read` (canonical order) for diagnostics.
pub fn show_set(set: &EffectSet) -> String {
    set.iter().cloned().collect::<Vec<_>>().join(", ")
}
/// E0746 (D-TXN2/D-BOUND-UNDO1): an irreversible effect (Net/FS/Exec/FFI) used directly inside a
/// `#Transact { … }` block. Points at the offending call; the fix is to move it
/// after the block or register it via `name.on_commit(() => { … })`.
pub fn e0746(api: &str, e: Effect, span: Span) -> Diagnostic {
    if e == Effect::FFI {
        return Diagnostic::error(
            "E0746",
            format!(
                "`{api}` has the `FFI` effect, which can't be rolled back inside a `#Transact` block"
            ),
            "a foreign function can change external state that the compiler cannot undo automatically"
                .to_string(),
            "move this call after the block, or declare `#Undo(inverse)` on the foreign binding so rollback can call the inverse"
                .to_string(),
            Some(span),
        );
    }
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


/// Per-function summary the checker accumulates during its walk: the effects the
/// body reaches directly, the user functions it calls (edges for transitivity),
/// and whether it touches a foreign body (forcing the maximal set).
#[derive(Debug, Clone)]
pub struct AutodiffObligation {
    pub method: String,
    pub target: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ComputeCallFact {
    pub method: String,
    pub span: Span,
}

#[derive(Debug, Clone, Default)]
pub struct EffectSummary {
    pub direct: EffectSet,
    /// Source witnesses for direct effects. Transitive provenance follows
    /// `edges` and the source-spanned memory-call records below.
    pub direct_spans: HashMap<String, Span>,
    /// Bare names of user functions called in the body (call-graph edges).
    pub edges: BTreeSet<String>,
    /// A foreign (`extern`) call was reached: the body's effects are maximal.
    pub maximal: bool,
    /// Source witness for the open-world operation that forced `maximal`.
    pub maximal_span: Option<Span>,
    /// This synthetic node is a trait method with no declared dispatch bound.
    /// Any caller that promises an effect ceiling receives E0743.
    pub unbounded_trait_dispatch: bool,
    /// D-EFF1: `#Caps(…)` restriction regions found in this body (checked against
    /// their transitive inferred set in the post-pass — E0741).
    pub regions: Vec<RegionSummary>,
    /// D-EFF2 (callback param bound): obligations recorded at each call to a
    /// higher-order fn whose function-typed parameter carries an effect bound
    /// (`fn(…) =[]=>` / `fn(…) =[E]=>`). Checked against the actual callback's
    /// resolved effects in the post-pass — E0747.
    pub callback_obligations: Vec<CallbackObligation>,
    /// D-AUTODIFF1: named functions passed to an autodiff transform. Their
    /// solved rows are checked after the shared effect graph closes, while
    /// compute's own GPU capability remains admissible for pure Tensor work.
    pub autodiff_obligations: Vec<AutodiffObligation>,
    /// Core compute consumers seen in each body; the autodiff post-pass uses
    /// this to reject operations that cannot preserve a Tensor trace.
    pub compute_calls: Vec<ComputeCallFact>,
    /// D-MEM-FACTS1 shares this already-complete call graph instead of growing
    /// a parallel reachability mechanism.
    pub memory: super::MemoryFacts::MemorySummary,
}

/// D-AUTODIFF1: reject a named differentiated function when its solved effect
/// row reaches an ambient effect. `GPU` is the capability used by pure
/// `core.compute` Tensor operations and is therefore allowed here.
pub fn check_autodiff_purity(
    summaries: &HashMap<String, EffectSummary>,
    solved: &HashMap<String, EffectSet>,
    diags: &mut Vec<Diagnostic>,
) {
    for summary in summaries.values() {
        for obligation in &summary.autodiff_obligations {
            let Some(effects) = solved.get(&obligation.target) else {
                continue;
            };
            let forbidden = effects
                .iter()
                .filter(|effect| effect_root(effect) != Effect::GPU.name())
                .cloned()
                .collect::<EffectSet>();
            if forbidden.is_empty() {
                if let Some(target_summary) = summaries.get(&obligation.target) {
                    for call in &target_summary.compute_calls {
                        if !matches!(
                            call.method.as_str(),
                            "get"
                                | "set"
                                | "to_list"
                                | "det"
                                | "inv"
                                | "solve"
                                | "fft"
                                | "serialize"
                                | "mse_loss"
                                | "sgd_step"
                                | "to_sparse"
                                | "sparse_mv"
                                | "matmul_f32_tile"
                        ) {
                            continue;
                        }
                        diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`compute.{}` cannot trace `compute.{}`",
                                obligation.method, call.method
                            ),
                            "this operation consumes, mutates, serializes, or has no registered Tensor derivative rule".to_string(),
                            "keep the differentiated function on pure differentiable Tensor operations".to_string(),
                            Some(call.span),
                        ));
                    }
                }
                continue;
            }
            diags.push(Diagnostic::error(
                "E0112",
                format!(
                    "`compute.{}` needs a pure Tensor function",
                    obligation.method
                ),
                format!(
                    "`{}` reaches {}; autodiff records only pure Tensor operations",
                    obligation.target,
                    show_set(&forbidden)
                ),
                format!(
                    "remove the effectful operation from `{}`, or differentiate a pure Tensor function",
                    obligation.target
                ),
                Some(obligation.span),
            ));
        }
    }
}

/// D-SEMINDEX1: per-function effect summaries and the solved transitive sets,
/// captured during `check_bundle` for the public semantic-index API.
#[derive(Debug, Clone, Default)]
pub struct SemIndexEffectFacts {
    pub summaries: HashMap<String, EffectSummary>,
    pub solved: HashMap<String, EffectSet>,
    /// Every inter-function reachability fact projected from the same graph.
    pub reachability: jet_foundation::Facts::ReachabilityResult,
    /// D-MEM-FACTS1 inspect/API/semver surface. These are the exact effective
    /// declarations and checker projections from the same graph used for E0921.
    pub memory_declarations: Vec<super::MemoryFacts::MemoryFactDeclaration>,
    pub memory_projections:
        HashMap<(String, super::MemoryFacts::MemoryFact), super::MemoryFacts::MemoryProjection>,
    /// One name ledger produced by sema. Tooling reads it and never resolves
    /// names independently from spelling or proximity.
    pub name_ledger: jet_foundation::Names::NameLedger,
    /// D-WEBAPP1=D: statically known `fn app()` application graph (Tower #438).
    pub web_app: Option<jet_foundation::WebApp::WebAppGraph>,
    /// D-FACTMODEL1=A: the one checked registry used by tag, effect, state,
    /// diagnostics, semantic tooling, and reflection consumers.
    pub fact_registry: jet_foundation::Facts::FactRegistry,
}

/// D-EFF2 (callback param bound): one obligation that a callback argument passed
/// to a `fn(…) =[]=>` / `fn(…) =[E]=>` parameter satisfies the declared bound. The
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

fn add_seed(seeds: &mut BTreeMap<String, BTreeSet<String>>, node: &str, fact: &str) {
    seeds
        .entry(node.to_string())
        .or_default()
        .insert(fact.to_string());
}

fn resolve_seed_nodes(
    graph: &BTreeMap<String, BTreeSet<String>>,
    name: &str,
) -> Vec<String> {
    let suffix = format!("::{name}");
    graph
        .keys()
        .filter(move |candidate| candidate.as_str() == name || candidate.ends_with(&suffix))
        .cloned()
        .collect()
}

/// Project effects, panic, taint, secret, and the `calls-exec` proof row from
/// one edge set. New reachability facts belong as seed rows here; they do not
/// get another recursive walker.
pub fn solve_reachability(
    summaries: &HashMap<String, EffectSummary>,
    taint_seeds: &HashMap<String, BTreeSet<String>>,
) -> jet_foundation::Facts::ReachabilityResult {
    let graph: BTreeMap<String, BTreeSet<String>> = summaries
        .iter()
        .map(|(name, summary)| (name.clone(), summary.edges.clone()))
        .collect();

    let mut effects = BTreeMap::new();
    let mut calls_exec = BTreeMap::new();
    let mut secret = BTreeMap::new();
    for (name, summary) in summaries {
        let mut direct = summary.direct.clone();
        if summary.maximal {
            direct.extend(Effect::all());
        }
        if effect_set_has_root(&direct, Effect::Exec) {
            add_seed(&mut calls_exec, name, Effect::Exec.name());
        }
        if effect_set_has_root(&summary.direct, Effect::Secret) {
            add_seed(&mut secret, name, Effect::Secret.name());
        }
        if !direct.is_empty() {
            effects.insert(name.clone(), direct);
        }
    }

    let mut panic = BTreeMap::new();
    add_seed(&mut panic, "__jet_panic__", "panic");

    let mut taint = BTreeMap::new();
    for (name, facts) in taint_seeds {
        for node in resolve_seed_nodes(&graph, name) {
            taint.entry(node).or_insert_with(BTreeSet::new).extend(facts.clone());
        }
    }

    jet_foundation::Facts::project_reachability(
        &graph,
        [
            jet_foundation::Facts::ReachabilityRow::new("effects", effects),
            jet_foundation::Facts::ReachabilityRow::new("panic", panic),
            jet_foundation::Facts::ReachabilityRow::new("taint", taint),
            jet_foundation::Facts::ReachabilityRow::new("secret", secret),
            jet_foundation::Facts::ReachabilityRow::new("calls-exec", calls_exec),
        ],
    )
}

/// D-EFF3: a call through a trait value sees the trait method's declared
/// dispatch bound, not any one implementation body. Seed those contract nodes
/// into the same graph concrete methods use.
pub(crate) fn seed_trait_dispatch_effects(
    items: &[crate::AST::Item],
    summaries: &mut HashMap<String, EffectSummary>,
) {
    use crate::AST::Item;

    for item in items {
        let Item::Trait(trait_def) = item else { continue };
        for method in &trait_def.methods {
            let mut summary = EffectSummary::default();
            match &method.declared_effects {
                Some(row) => {
                    for (name, span) in row {
                        let base = name.strip_prefix('!').unwrap_or(name);
                        if effect_row_var(base).is_some() {
                            summary.maximal = true;
                            summary.maximal_span.get_or_insert(*span);
                        } else if !name.starts_with('!') {
                            if let Some(effect) = parse_effect_name(name) {
                                summary.direct.insert(effect.clone());
                                summary.direct_spans.entry(effect).or_insert(*span);
                            }
                        }
                    }
                }
                None if !method.is_pure => {
                    summary.maximal = true;
                    summary.maximal_span = Some(method.name_span);
                    summary.unbounded_trait_dispatch = true;
                }
                None => {}
            }
            summaries.insert(
                super::effect_key(Some(&trait_def.name), &method.name),
                summary,
            );
        }
    }
}

fn inferred_purity_display_name<'a>(module_alias: &str, name: &'a str) -> &'a str {
    name.strip_prefix(module_alias)
        .and_then(|name| name.strip_prefix("::"))
        .unwrap_or(name)
}

#[cfg(test)]
mod inferred_purity_display_tests {
    use super::inferred_purity_display_name;

    #[test]
    fn strips_only_the_current_module_prefix() {
        assert_eq!(inferred_purity_display_name("app", "app::leaky"), "leaky");
        assert_eq!(
            inferred_purity_display_name("app", "dependency::leaky"),
            "dependency::leaky"
        );
    }
}

#[cfg(test)]
mod reachability_tests {
    use super::{solve_reachability, EffectSummary};
    use std::collections::{BTreeSet, HashMap};

    #[test]
    fn one_graph_projects_all_reachability_rows() {
        let summaries = HashMap::from([
            (
                "root".to_string(),
                EffectSummary {
                    edges: BTreeSet::from(["mid".to_string()]),
                    ..Default::default()
                },
            ),
            (
                "mid".to_string(),
                EffectSummary {
                    edges: BTreeSet::from(["leaf".to_string()]),
                    ..Default::default()
                },
            ),
            (
                "leaf".to_string(),
                EffectSummary {
                    direct: BTreeSet::from([
                        "Exec".to_string(),
                        "Secret".to_string(),
                    ]),
                    edges: BTreeSet::from(["__jet_panic__".to_string()]),
                    ..Default::default()
                },
            ),
        ]);
        let taint = HashMap::from([(
            "leaf".to_string(),
            BTreeSet::from(["Credential".to_string()]),
        )]);

        let result = solve_reachability(&summaries, &taint);

        assert!(result.nodes_with("effects", "Exec").contains("root"));
        assert!(result.nodes_with("secret", "Secret").contains("root"));
        assert!(result.nodes_with("panic", "panic").contains("root"));
        assert!(result.nodes_with("taint", "Credential").contains("root"));
        assert!(result.nodes_with("calls-exec", "Exec").contains("root"));
    }
}

/// D-EFFECT-OMIT1: an explicit empty row proves the *inferred* body row is
/// empty. Callees need not repeat `=[]=>`; their solved row is the authority.
/// D-CRYPTO-DIAG1 defers E2702 facts until this solved-effect phase completes.
pub fn check_inferred_purity(
    items: &[crate::AST::Item],
    module_alias: &str,
    summaries: &HashMap<String, EffectSummary>,
    solved: &HashMap<String, EffectSet>,
    reachability: &jet_foundation::Facts::ReachabilityResult,
    diags: &mut Vec<Diagnostic>,
) {
    fn check_one(
        f: &crate::AST::Func,
        owner: Option<&str>,
        identity: Option<&str>,
        module_alias: &str,
        summaries: &HashMap<String, EffectSummary>,
        solved: &HashMap<String, EffectSet>,
        reachability: &jet_foundation::Facts::ReachabilityResult,
        diags: &mut Vec<Diagnostic>,
    ) {
        if !f.is_pure {
            return;
        }
        let identity = identity
            .map(str::to_owned)
            .unwrap_or_else(|| super::effect_key(owner, &f.name));
        let key = format!("{module_alias}::{identity}");
        if solved.get(&key).map_or(true, EffectSet::is_empty) {
            return;
        }
        let Some(proof) = solved.get(&key).and_then(|effects| {
            effects.iter().find_map(|effect| {
                let path = reachability.path("effects", &key, effect)?;
                (path.len() > 1).then(|| path.to_vec())
            })
        }) else {
            // Direct ambient operations are diagnosed while checking the body,
            // where their precise call span and API spelling are available.
            return;
        };
        let chain = &proof[1..];
        let Some(first) = chain.first() else { return };
        let summary = summaries.get(&key);
        let span = summary
            .and_then(|summary| summary.memory.calls.iter().find(|call| &call.callee == first))
            .map(|call| call.span)
            .unwrap_or(f.name_span);
        let call_name = chain.last().expect("non-empty chain");
        if summaries
            .get(call_name)
            .is_some_and(|summary| summary.unbounded_trait_dispatch)
        {
            // `check_effect_boundaries` emits E0743 once for the explicit
            // ceiling. Do not stack a generic E3401 on the same call path.
            return;
        }
        let path = if chain.len() > 1 {
            std::iter::once(f.name.clone())
                .chain(
                    chain[..chain.len() - 1]
                        .iter()
                        .map(|name| inferred_purity_display_name(module_alias, name).to_string()),
                )
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        diags.push(crate::Sema::e3401(
            &f.name,
            inferred_purity_display_name(module_alias, call_name),
            &path,
            span,
        ));
    }

    use crate::AST::Item;
    for item in items {
        match item {
            Item::Func(f) => check_one(f, None, None, module_alias, summaries, solved, reachability, diags),
            Item::Impl(i) => for f in &i.methods { check_one(f, Some(&i.type_name), None, module_alias, summaries, solved, reachability, diags); },
            Item::Struct(s) => {
                for f in &s.methods { check_one(f, Some(&s.name), None, module_alias, summaries, solved, reachability, diags); }
                for block in &s.trait_impls {
                    for f in &block.methods { check_one(f, Some(&s.name), None, module_alias, summaries, solved, reachability, diags); }
                }
            }
            Item::Enum(e) => for f in &e.methods { check_one(f, Some(&e.name), None, module_alias, summaries, solved, reachability, diags); },
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    for item in body {
                        if let Item::Func(f) = item {
                            let identity = jet_foundation::Names::member_name(&module.name, &f.name);
                            check_one(
                                f,
                                None,
                                Some(&identity),
                                module_alias,
                                summaries,
                                solved,
                                reachability,
                                diags,
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn e0743(trait_method: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0743",
        format!("dynamic call `{trait_method}` has no effect bound"),
        "a trait value can select any implementation at runtime, so an enclosing effect ceiling needs the trait method's declared upper bound".to_string(),
        format!("declare an effect row on `{trait_method}`, such as `=[]=>` for pure dispatch, or move this dynamic call outside the bounded function"),
        Some(span),
    )
}

/// D-EFF2 (`#(via f)` pass-through): seed each via-fn's effect summary with the
/// declared bound of its callback parameter `f`, so its published effect set is a
/// tight pass-through of `f` (the set that holds even when the callback value
/// escapes — the conservative flow-through can't see a callback the body stores
/// or returns). Runs over the assembled summaries **before** the shared
/// reachability projection.
///
/// - `f` must be a parameter of this function whose type is a function type
///   (`Type::Fn`), else E0748.
/// - `f`'s declared bound (`#Pure` → empty, `#(E)` → that set) is the pass-through
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
        let mut via_spans = HashMap::new();
        let mut maximal = false;
        match effect_bound {
            Some(names) => {
                for (n, ns) in names {
                    if effect_row_var(n).is_some() {
                        // The caller contributes the concrete callback edge in
                        // `attribute_fn_arg`; making the generic callee maximal
                        // here would lose that substitution and invent every
                        // effect for even a pure callback.
                        continue;
                    }
                    match parse_effect_name(n) {
                        Some(e) => {
                            via_spans.entry(e.clone()).or_insert(*ns);
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
        entry.direct_spans.extend(via_spans);
        entry.maximal |= maximal;
        if maximal {
            entry.maximal_span.get_or_insert(*via_span);
        }
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

/// E0748 (D-EFF2/D-SHAPE8): `=[via f]=>` names no callback parameter.
pub fn e0748_unknown_param(fn_name: &str, param: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0748",
        format!("`=[via {}]=>` on `{}` names no such parameter", param, fn_name),
        format!(
            "`=[via f]=>` publishes the effects of a callback parameter `f`; `{}` has no parameter called `{}`",
            fn_name, param
        ),
        format!("name one of `{}`'s callback parameters after `via`", fn_name),
        Some(span),
    )
}

/// E0748 (D-EFF2/D-SHAPE8): `=[via f]=>` names a non-callback parameter.
pub fn e0748_not_callback(fn_name: &str, param: &str, ty: String, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0748",
        format!("`=[via {}]=>` on `{}` names a parameter that isn't a callback", param, fn_name),
        format!(
            "`=[via f]=>` publishes the effects of a *function* parameter; `{}` is `{}`, not a `fn(…)` type",
            param, ty
        ),
        format!("point `via` at a parameter whose type is a function, or drop the `=[via {}]=>` annotation", param),
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
    failed_diagnostic_phases: &mut HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    for (key, summary) in summaries {
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
                failed_diagnostic_phases.insert(key.clone());
                diags.push(e0747(&over, &ob.bound, ob.span));
            }
        }
    }
}

/// D-REPLAY1: `#Replayable` functions may not reach ambient
/// Time/Rand/Net/IO. Deterministic handles (`Clock.new(seed)`,
/// `random.rng(seed)`, mockable capability objects) stay valid because they do
/// not enter the ambient Core-call effect graph.
pub fn check_replayable_effects(
    items: &[crate::AST::Item],
    solved: &HashMap<String, EffectSet>,
    diags: &mut Vec<Diagnostic>,
) {
    use crate::AST::Item;

    fn replay_forbidden(effects: &EffectSet) -> EffectSet {
        let roots = [Effect::Time, Effect::Rand, Effect::Net, Effect::IO];
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
/// doesn't allow — a `fn(…) =[]=>` parameter handed an impure callback, or a
/// `fn(…) =[E]=>` parameter handed one that reaches an effect outside `E`.
pub fn e0747(over: &EffectSet, bound: &EffectSet, span: Span) -> Diagnostic {
    let over_list = show_set(over);
    let bound_desc = if bound.is_empty() {
        "the parameter is `fn(…) =[]=>`, so the callback must be pure".to_string()
    } else {
        format!(
            "the parameter is `fn(…) =[{}]=>`, so the callback may use only those effects",
            show_set(bound)
        )
    };
    let fix = if bound.is_empty() {
        "pass a `fn(…) =[]=>` callback (or a lambda that uses no effects), or widen the parameter's bound".to_string()
    } else {
        format!(
            "pass a callback within `=[{}]=>`, or add `{}` to the parameter's bound",
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

/// E0740: a function's inferred effects exceed its declared effect-row bound.
pub fn e0740(fn_name: &str, over: &EffectSet, declared: &EffectSet, span: Span) -> Diagnostic {
    let over_list = show_set(over);
    let decl = if declared.is_empty() {
        "no effects".to_string()
    } else {
        format!("`=[{}]=>`", show_set(declared))
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
/// prohibits descendant, `reached` may name a leaf (`FS.Write`) under a
/// broader declared root (`FS`), so the two are shown separately rather than
/// assuming they're always the same text (they always were, pre-D-EFFTREE1).
pub fn e0749(fn_name: &str, reached: &EffectSet, prohibited: &EffectSet, span: Span) -> Diagnostic {
    let reached_list = show_set(reached);
    let decl_list = show_set(prohibited);
    Diagnostic::error(
        "E0749",
        format!(
            "`{}` reaches the `{}` effect, which it prohibits with `=[!{}]=>`",
            fn_name, reached_list, decl_list
        ),
        format!(
            "a `=[!{}]=>` row means the function and every callee it can reach must not use `{}`",
            decl_list, reached_list
        ),
        format!(
            "remove the call that introduces `{}`, or drop the `=[!{}]=>` prohibition",
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
    failed_diagnostic_phases: &mut HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    for (key, summary) in summaries {
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
                failed_diagnostic_phases.insert(key.clone());
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
        Stmt::BreakValue(e, _) | Stmt::BreakLabelValue(_, _, e, _) => {
            expr_handle_escape(e, handle)
        }
        Stmt::While { cond, body, .. } => expr_handle_escape(cond, handle).or_else(|| block(body)),
        Stmt::For { kind, body, .. } => {
            let coll = match kind {
                ForKind::Range { start, end, step, exclusive: _ } => expr_handle_escape(start, handle)
                    .or_else(|| expr_handle_escape(end, handle))
                    .or_else(|| step.as_ref().and_then(|s| expr_handle_escape(s, handle))),
                ForKind::In { collection, step } => expr_handle_escape(collection, handle)
                    .or_else(|| step.as_ref().and_then(|s| expr_handle_escape(s, handle))),
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
            .or_else(|| step.as_ref().and_then(|step| stmt_handle_escape(step, handle))),
        // D-CANVASSTATE1=D: an `#Off` body never runs, so nothing escapes it.
        Stmt::Switched { marker, .. } if crate::AST::switched_off(marker) => None,
        Stmt::Loop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::ScopeMember { body, .. }
        | Stmt::Live { body, .. } => block(body),
        // D-META-STAGE1=B (formerly D-CTMARKER1): comptime block erases; no handle can escape a build-time block.
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
        Expr::Try(receiver, _, _, note) if is_bare_handle(receiver, handle) => {
            note.as_deref().and_then(|note| expr_handle_escape(note, handle))
        }
        Expr::Deref(receiver, _) if is_bare_handle(receiver, handle) => None,
        Expr::Index { base, index, .. } if is_bare_handle(base, handle) => {
            expr_handle_escape(index, handle)
        }
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Tainted(inner, _, _) // D-TAINT1: tag erased; recurse into the value.
        | Expr::Present(inner, _) | Expr::Ok(inner, _) | Expr::Err(inner, _)
        | Expr::Field(inner, _, _) => expr_handle_escape(inner, handle),
        Expr::Try(inner, _, _, note) => expr_handle_escape(inner, handle).or_else(|| {
            note.as_deref().and_then(|note| expr_handle_escape(note, handle))
        }),
        Expr::MemberSpread { base, .. } => expr_handle_escape(base, handle),
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
        Expr::Slice { base, start, end, range, .. } => expr_handle_escape(base, handle)
            .or_else(|| {
                range.as_deref().map_or_else(
                    || {
                        expr_handle_escape(start, handle)
                            .or_else(|| expr_handle_escape(end, handle))
                    },
                    |range| expr_handle_escape(range, handle),
                )
            }),
        Expr::Range { start, end, .. } => {
            expr_handle_escape(start, handle).or_else(|| expr_handle_escape(end, handle))
        }
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
        Expr::TypedLit { body, .. } => {
            let mut found = None;
            body.for_each_expr(|f| {
                if found.is_none() {
                    found = expr_handle_escape(f, handle);
                }
            });
            found
        }
        Expr::EnumLit { args, .. } => args.iter().find_map(|a| match a {
            EnumLitArg::Positional(e) => expr_handle_escape(e, handle),
            EnumLitArg::Named { expr, .. } => expr_handle_escape(expr, handle),
        }),
        Expr::OrFallback { value, fallback, .. } => expr_handle_escape(value, handle).or_else(|| {
            match fallback {
                OrFallback::Value(e) => expr_handle_escape(e, handle),
                OrFallback::Block { body, value, .. } => body
                    .iter()
                    .find_map(|stmt| stmt_handle_escape(stmt, handle))
                    .or_else(|| expr_handle_escape(value, handle)),
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
        | Expr::NoElse(_)
        | Expr::UnitLit { .. }
        | Expr::ComptimeName { .. }
        // D-SHIFT1 (c7shift) / D-BINPAT1 (card #506 follow-up): a leaf
        // literal, no nested `Expr` to recurse into.
        | Expr::StrMatchLit(_, _)
        | Expr::BinMatchLit(_, _) => None,
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
        "an effect list names compiler-known effects like `Net`, `FS`, `IO`, `DB`, or `Time`"
            .to_string(),
        "use one of the known effect names, or remove it from the list".to_string(),
        Some(span),
    )
}

/// E0742 (D-EFF3): an impl of a trait method uses effects beyond the upper
/// bound the trait method declares (`#Pure fn …` / `fn … #(GPU)`).
pub fn e0742(
    trait_name: &str,
    method: &str,
    over: &EffectSet,
    bound: &EffectSet,
    span: Span,
) -> Diagnostic {
    let over_list = show_set(over);
    let bound_desc = if bound.is_empty() {
        format!("`{}` bounds `{}` to `=[]=>`, so impls must be pure", trait_name, method)
    } else {
        format!(
            "`{}` bounds `{}` to `=[{}]=>`, so impls may use only those",
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
/// method declares one (`#Pure` → empty set). Impl methods are keyed
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
/// but this deliberately uses the dedicated `secret` row rather than the
/// general `effects` row: foreign calls seed every effect, but do not prove a
/// secret read.
pub(crate) fn check_secret_grants(
    items: &[crate::AST::Item],
    module_alias: &str,
    reaches_secret: &BTreeSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
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
        module_alias: &str,
        reaches_secret: &BTreeSet<String>,
        diags: &mut Vec<Diagnostic>,
    ) {
        let key = format!("{module_alias}::{}", super::effect_key(owner, &f.name));
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
            Item::Func(f) => check_one(f, None, module_alias, reaches_secret, diags),
            Item::Impl(i) => {
                for m in &i.methods {
                    check_one(m, Some(&i.type_name), module_alias, reaches_secret, diags);
                }
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    check_one(m, Some(&s.name), module_alias, reaches_secret, diags);
                }
                for block in &s.trait_impls {
                    for m in &block.methods {
                        check_one(m, Some(&s.name), module_alias, reaches_secret, diags);
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    check_one(m, Some(&e.name), module_alias, reaches_secret, diags);
                }
            }
            _ => {}
        }
    }
}

/// E1264 (U13, D-JPK-SECRETCRYPTO1): a function reaches `core.vault.get`
/// (transitively) without `Secret` in its own declared effect row.
pub fn e1264(fn_name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1264",
        format!(
            "`{}` reads a secret but doesn't declare the `Secret` effect",
            fn_name
        ),
        "reading a secret (`core.vault.get`) always requires an explicit grant — unlike other \
         effects, there is no silently-inferred default here, so a function must opt in even with \
         no other explicit effect row at all."
            .to_string(),
        format!(
            "add `=[Secret]=>` to `{}`'s signature (or widen an existing effect row to cover it)",
            fn_name
        ),
        Some(span),
    )
}
