//! Typestate (D-STATE1 / D-STATE-HOME1 / D-STATE-REQ / D-STATE-TRANS).
//!
//! A value moves through a named set of *states*. Operations declare the state they
//! need and the state they leave the value in, via two fn markers:
//!
//!   - `#State(S) fn m(self, …)` — a **require-state** guard: `m` is valid only
//!     when its receiver is currently in state `S`. Calling it in any other state
//!     is **E0150**. The state is unchanged by the call.
//!   - `#Transition(From, To) fn m(self, …) T ->` — a **transition**: it consumes
//!     a value in state `From` and yields one in state `To`. A call requires the
//!     receiver be in `From` (E0150 otherwise) and **advances** it to `To`. The
//!     from-state may be `_` (an *entry* transition: a constructor that produces the
//!     initial state from nothing — e.g. `#Transition(_, Pending) fn new() R ->`).
//!
//! D-STATE-HOME1=A: states are declared in one dedicated section
//! `state { Pending, Confirmed, CheckedIn }` inside the owning struct.
//! When present:
//!   - `#State(X)` / `#Transition(A, B)` on `TypeName::*` must reference declared
//!     state names; an unknown name is **E0151** (typo against the set).
//!   - A declared state with no outgoing `#Transition(S, …)` is a terminal state.
//!     The checked graph stores that fact for reflection and semantic tools.
//!   - The set erases (compile-time only, no runtime discriminant).
//!
//! The current state of a value is a **compile-time fact** threaded by intraprocedural
//! forward dataflow over locals. Nothing about the state reaches codegen (I3, zero
//! runtime cost). When a value's state cannot be tracked precisely (it escapes into a
//! field, a non-local receiver, a loop-carried position), the checker is **silent**
//! rather than guessing (P1 — beginners never see a spurious error).

use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Sema::FlowFacts::{FlowFacts, Plane};
use crate::Sema::{KnowledgeGate, KnowledgePlane};
use crate::Syntax::edit_distance;
use crate::AST::{Call, Expr, Func, Item, LValue, Stmt, Type};
use std::collections::{HashMap, HashSet, VecDeque};

/// D-STATE1: the state a value is in. One plane of the checker's flow facts —
/// this file supplies the join rule and nothing else about merging.
pub(crate) enum Typestate {}

const TASK_RUNNING: &str = "Running";
const TASK_JOINED: &str = "Joined";
const TASK_DETACHED: &str = "Detached";

impl Plane for Typestate {
    type Fact = String;

    /// A state holds after paths meet only when every path agrees. Paths that
    /// disagree leave the value untracked, and say so (L0152).
    fn join(left: Option<&String>, right: Option<&String>) -> Option<String> {
        match (left, right) {
            (Some(left), Some(right)) if left == right => Some(left.clone()),
            _ => None,
        }
    }

    const REPORTS_DIVERGENCE: bool = true;
}

/// Program-wide typestate metadata, collected once before any body is walked.
#[derive(Default)]
pub struct StateTable {
    facts: jet_foundation::Facts::FactRegistry,
    /// `Type::method` → required state (`#State(S)`). The receiver must be in this
    /// state at the call.
    requires: HashMap<String, String>,
    /// `Type::method` → (from-state, to-state) for a `#Transition(From, To)`.
    /// `from` is `None` for an entry transition.
    transitions: HashMap<String, (Option<String>, String)>,
    /// Free-function name → required state / transition (typestate on a free fn
    /// whose first parameter is the tracked value).
    fn_requires: HashMap<String, String>,
    fn_transitions: HashMap<String, (Option<String>, String)>,
    /// Type name → the to-state of its entry transition(s) keyed by the producing
    /// method name (`Type::method` → to-state). Lets a binding `r := Type.ctor()`
    /// seed `r`'s initial state.
    entry_ctors: HashMap<String, String>,
    /// D-STATE-HOME1=A: type name → declared state labels with their spans. When a type
    /// has a nested `state { … }` section, every `#State(X)` / `#Transition(A, B)`
    /// marker on its methods must reference a name from this set (else E0151).
    declared: HashMap<String, Vec<(String, Span)>>,
}

fn methods_for_type<'a>(items: &'a [Item], type_name: &str) -> Vec<&'a Func> {
    items
        .iter()
        .flat_map(|item| match item {
            Item::Impl(i) if i.type_name == type_name => i.methods.iter().collect(),
            Item::Struct(s) if s.name == type_name => {
                let mut methods: Vec<&Func> = s.methods.iter().collect();
                for block in &s.trait_impls {
                    methods.extend(block.methods.iter());
                }
                methods
            }
            Item::Enum(e) if e.name == type_name => e.methods.iter().collect(),
            _ => Vec::new(),
        })
        .collect()
}

/// Build checked state facts from one module's declarations and transitions.
/// Invalid marker references are omitted; the diagnostic pass reports them.
pub(crate) fn checked_state_graphs(
    items: &[Item],
) -> HashMap<String, jet_foundation::Facts::StateGraph> {
    let declarations: HashMap<String, Vec<String>> = items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(structure) => structure.state.as_ref().map(|state| {
                (
                    structure.name.clone(),
                    state.states.iter().map(|(name, _)| name.clone()).collect(),
                )
            }),
            _ => None,
        })
        .collect();

    declarations
        .into_iter()
        .map(|(type_name, states)| {
            let state_names: HashSet<&str> = states.iter().map(String::as_str).collect();
            let mut outgoing = HashSet::new();
            let mut edges: HashMap<String, Vec<String>> = HashMap::new();
            let mut entries = Vec::new();
            let mut transitions = Vec::new();

            for method in methods_for_type(items, &type_name) {
                let Some(transition) = &method.state_transition else {
                    continue;
                };
                let from = if let Some(raw) = &transition.from {
                    let Some(leaf) = StateTable::state_leaf(&type_name, raw) else {
                        continue;
                    };
                    if !state_names.contains(leaf) {
                        continue;
                    }
                    Some(leaf.to_string())
                } else {
                    None
                };
                let Some(to) = StateTable::state_leaf(&type_name, &transition.to)
                    .filter(|leaf| state_names.contains(leaf))
                    .map(str::to_string)
                else {
                    continue;
                };

                if let Some(from_state) = &from {
                    outgoing.insert(from_state.clone());
                    edges
                        .entry(from_state.clone())
                        .or_default()
                        .push(to.clone());
                } else {
                    entries.push(to.clone());
                }
                transitions.push(jet_foundation::Facts::StateTransition {
                    operation: method.name.clone(),
                    from,
                    to,
                });
            }

            let reachable = if entries.is_empty() {
                None
            } else {
                let mut seen = HashSet::new();
                let mut queue = VecDeque::from(entries);
                while let Some(state) = queue.pop_front() {
                    if !seen.insert(state.clone()) {
                        continue;
                    }
                    if let Some(next) = edges.get(&state) {
                        queue.extend(next.iter().cloned());
                    }
                }
                Some(seen)
            };

            let nodes = states
                .into_iter()
                .map(|name| jet_foundation::Facts::StateNode {
                    terminal: !outgoing.contains(&name),
                    reachable: reachable
                        .as_ref()
                        .map(|reachable| reachable.contains(&name)),
                    name,
                })
                .collect();
            (
                type_name,
                jet_foundation::Facts::StateGraph {
                    states: nodes,
                    transitions,
                },
            )
        })
        .collect()
}

impl StateTable {
    /// Build the state-only registry needed by early comptime and body
    /// checking. The full table still adds marker requirements below.
    pub fn declaration_facts(items: &[Item]) -> jet_foundation::Facts::FactRegistry {
        jet_foundation::Facts::FactRegistry::from_state_items(items)
    }

    fn state_leaf<'a>(type_name: &str, state: &'a str) -> Option<&'a str> {
        if !state.contains('.') {
            return Some(state);
        }
        let mut parts = state.rsplitn(3, '.');
        let leaf = parts.next()?;
        let plane = parts.next()?;
        let owner = parts.next()?;
        (plane == "State" && owner == type_name).then_some(leaf)
    }

    fn state_for_owner(owner: Option<&str>, state: &str) -> String {
        owner
            .and_then(|owner| Self::state_leaf(owner, state))
            .map(str::to_string)
            .unwrap_or_else(|| state.to_string())
    }

    fn nominal_type_name(ty: &Type) -> Option<&str> {
        match ty {
            Type::Named(name) | Type::Apply { name, .. } => Some(name),
            Type::Tagged { inner, .. } => Self::nominal_type_name(inner),
            _ => None,
        }
    }

    fn free_fn_state_owner<'a>(f: &'a Func, entry: bool) -> Option<&'a str> {
        let param_owner = f
            .params
            .first()
            .and_then(|param| Self::nominal_type_name(&param.ty));
        let return_owner = f
            .return_type
            .as_ref()
            .and_then(|ty| Self::nominal_type_name(ty));
        if entry {
            return_owner.or(param_owner)
        } else {
            param_owner.or(return_owner)
        }
    }
    /// Register every typestate marker in `items` into this table. Methods key as
    /// `Type::method`; entry transitions (`_ -> To`) also register under
    /// `entry_ctors` so a constructor call can seed a local's initial state.
    /// D-STATE-HOME1=A: nested struct state sections are registered here so
    /// `validate_declarations` can check markers against the owning set.
    /// Idempotent across modules — call once per module for a bundle.
    pub fn add_items(&mut self, items: &[Item]) {
        for item in items {
            match item {
                Item::Func(f) => self.add_free_fn(f),
                Item::Impl(i) => {
                    for m in &i.methods {
                        self.add_method(&i.type_name, m);
                    }
                }
                Item::Struct(s) => {
                    if let Some(state) = &s.state {
                        self.facts.declare_state(
                            s.name.clone(),
                            state.states.iter().map(|(name, _)| name.clone()),
                        );
                        self.declared.insert(s.name.clone(), state.states.clone());
                    }
                    for m in &s.methods {
                        self.add_method(&s.name, m);
                    }
                    for block in &s.trait_impls {
                        for m in &block.methods {
                            self.add_method(&s.name, m);
                        }
                    }
                }
                Item::Enum(e) => {
                    for m in &e.methods {
                        self.add_method(&e.name, m);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn with_facts(facts: jet_foundation::Facts::FactRegistry) -> Self {
        let mut table = Self {
            facts,
            ..Self::default()
        };
        table.install_task_lifecycle();
        table
    }

    /// D-CONC-UNIT1: task ownership states are ordinary typestate rows. The
    /// row is compiler-owned, so the public task surface stays unchanged.
    fn install_task_lifecycle(&mut self) {
        self.facts.declare(
            jet_foundation::Facts::FactKind::State,
            format!("{}.State", crate::Syntax::TYPE_TASK),
            [TASK_RUNNING, TASK_JOINED, TASK_DETACHED]
                .into_iter()
                .map(str::to_string),
        );
        self.transitions.insert(
            format!("{}::{}", crate::Syntax::TYPE_TASK, crate::Syntax::TASK_JOIN),
            (Some(TASK_RUNNING.to_string()), TASK_JOINED.to_string()),
        );
        self.transitions.insert(
            format!(
                "{}::{}",
                crate::Syntax::TYPE_TASK,
                crate::Syntax::TASK_DETACH
            ),
            (Some(TASK_RUNNING.to_string()), TASK_DETACHED.to_string()),
        );
    }

    /// D-STATE-HOME1=A: validate that every `#State(X)` / `#Transition(A, B)` marker
    /// on methods of a type that has a nested `state { … }` section references
    /// a state in the declared set. Unknown state → E0151. The checked graph records
    /// terminal and entry reachability facts without adding a runtime policy.
    pub fn validate_declarations(&mut self, items: &[Item], diags: &mut Vec<Diagnostic>) {
        let graphs = checked_state_graphs(items);
        // `StateTable` is collected once for the whole bundle, while this
        // validator is called once per loaded module. Only the struct-owned
        // rows present in this module may produce declaration diagnostics or
        // graph facts here; otherwise an empty/duplicate row in one module is
        // repeated for every sibling module with a synthetic 0..0 span.
        let local_structs: HashSet<&str> = items
            .iter()
            .filter_map(|item| match item {
                Item::Struct(structure) => Some(structure.name.as_str()),
                _ => None,
            })
            .collect();
        for (type_name, decl_states) in self
            .declared
            .iter()
            .filter(|(type_name, _)| local_structs.contains(type_name.as_str()))
        {
            if decl_states.is_empty() {
                diags.push(e0169(type_name, self.state_span(items, type_name)));
            }
            let mut seen = HashSet::new();
            for (state, span) in decl_states {
                if !seen.insert(state) {
                    diags.push(e0166(state, type_name, *span));
                }
            }
            if let Some(structure) = items.iter().find_map(|item| match item {
                Item::Struct(s) if s.name == *type_name => Some(s),
                _ => None,
            }) {
                let members: HashSet<&str> =
                    structure
                        .fields
                        .iter()
                        .map(|field| field.name.as_str())
                        .chain(structure.methods.iter().map(|method| method.name.as_str()))
                        .chain(structure.trait_impls.iter().flat_map(|block| {
                            block.methods.iter().map(|method| method.name.as_str())
                        }))
                        .chain(items.iter().filter_map(|item| match item {
                            Item::Impl(implementation)
                                if implementation.type_name == structure.name => {
                                Some(implementation.methods.iter().map(|method| method.name.as_str()))
                            }
                            _ => None,
                        }).flatten())
                        .collect();
                for (state, span) in decl_states {
                    if members.contains(state.as_str()) {
                        diags.push(e0167(state, type_name, *span));
                    }
                }
            }
            let plane = format!("{type_name}.State");
            let state_names: HashSet<&str> = self
                .facts
                .get(jet_foundation::Facts::FactKind::State, &plane)
                .into_iter()
                .flat_map(|fact| fact.members.iter().map(String::as_str))
                .collect();

            // Helper to check a single state name against the declared set.
            let check_state = |state: &str, span: Span, diags: &mut Vec<Diagnostic>| {
                let leaf = Self::state_leaf(type_name, state);
                if leaf.is_none_or(|leaf| !state_names.contains(leaf)) {
                    let candidate_name = leaf.unwrap_or(state);
                    let candidates: Vec<&str> = state_names
                        .iter()
                        .filter(|&&s| edit_distance(candidate_name, s) <= 2)
                        .copied()
                        .collect();
                    diags.push(e0151(state, type_name, &candidates, span));
                }
            };

            // Walk all method markers on this type.
            for m in methods_for_type(items, type_name) {
                if let Some((state, span)) = &m.state_requires {
                    check_state(state, *span, diags);
                }
                if let Some(tr) = &m.state_transition {
                    if let Some(from) = &tr.from {
                        check_state(from, tr.span, diags);
                    }
                    check_state(&tr.to, tr.span, diags);
                }
            }

            if let Some(graph) = graphs.get(type_name).cloned() {
                for node in &graph.states {
                    if node.reachable == Some(false) {
                        let span = decl_states
                            .iter()
                            .find(|(state, _)| state == &node.name)
                            .map(|(_, span)| *span)
                            .unwrap_or_else(|| Span::new(0, 0));
                        diags.push(l0153(&node.name, type_name, span));
                    }
                }
                self.facts
                    .set_state_graph(format!("{type_name}.State"), graph);
            }
        }

        // A typestate marker on a struct/enum/impl without its owning state
        // section is a distinct error. Free functions keep their existing
        // marker shape; they have no nominal owner to attach a state set to.
        for item in items {
            let (type_name, methods): (&str, Vec<&Func>) = match item {
                Item::Impl(i) => (i.type_name.as_str(), i.methods.iter().collect()),
                Item::Struct(s) => {
                    let mut methods = s.methods.iter().collect::<Vec<_>>();
                    methods.extend(s.trait_impls.iter().flat_map(|b| b.methods.iter()));
                    (s.name.as_str(), methods)
                }
                Item::Enum(e) => (e.name.as_str(), e.methods.iter().collect()),
                _ => continue,
            };
            if type_name == crate::Syntax::TYPE_TASK || self.declared.contains_key(type_name) {
                continue;
            }
            for method in methods {
                if method.state_requires.is_some() || method.state_transition.is_some() {
                    diags.push(e0159(type_name, method.span));
                }
            }
        }
    }

    fn state_span(&self, items: &[Item], type_name: &str) -> Span {
        items
            .iter()
            .find_map(|item| match item {
                Item::Struct(s) if s.name == type_name => s.state.as_ref().map(|state| state.span),
                _ => None,
            })
            .unwrap_or_else(|| Span::new(0, 0))
    }

    fn add_method(&mut self, type_name: &str, m: &Func) {
        let key = format!("{type_name}::{}", m.name);
        if let Some((state, _)) = &m.state_requires {
            if let Some(state) = Self::state_leaf(type_name, state) {
                self.requires.insert(key.clone(), state.to_string());
            }
        }
        if let Some(tr) = &m.state_transition {
            let from = tr
                .from
                .as_deref()
                .and_then(|state| Self::state_leaf(type_name, state))
                .map(str::to_string);
            let Some(to) = Self::state_leaf(type_name, &tr.to).map(str::to_string) else {
                return;
            };
            self.transitions.insert(key, (from.clone(), to.clone()));
            if from.is_none() {
                self.entry_ctors
                    .insert(format!("{type_name}::{}", m.name), to);
            }
        }
    }

    fn add_free_fn(&mut self, f: &Func) {
        if let Some((state, _)) = &f.state_requires {
            let owner = Self::free_fn_state_owner(f, false);
            self.fn_requires
                .insert(f.name.clone(), Self::state_for_owner(owner, state));
        }
        if let Some(tr) = &f.state_transition {
            let owner = Self::free_fn_state_owner(f, tr.from.is_none());
            self.fn_transitions
                .insert(
                    f.name.clone(),
                    (
                        tr.from
                            .as_deref()
                            .map(|state| Self::state_for_owner(owner, state)),
                        Self::state_for_owner(owner, &tr.to),
                    ),
                );
        }
    }

    /// True when no typestate rows are registered. `with_facts` always installs
    /// the compiler-owned task lifecycle row, so task bodies use this pass too.
    pub fn is_empty(&self) -> bool {
        self.requires.is_empty()
            && self.transitions.is_empty()
            && self.fn_requires.is_empty()
            && self.fn_transitions.is_empty()
            && self.declared.is_empty()
    }

    pub fn into_facts(self) -> jet_foundation::Facts::FactRegistry {
        self.facts
    }
}

/// Per-function typestate analyzer. Tracks each tracked local's current state.
struct StateCtx<'a> {
    tbl: &'a StateTable,
    /// The typestate plane of the one flow-fact store.
    flow: FlowFacts,
    diags: Vec<Diagnostic>,
    /// Diagnostics CheckerCore already produced before this pass runs — read
    /// only to check whether a switch was already proven exhaustive
    /// (`FlowFacts::switch_proven_exhaustive`); never written here.
    existing_diags: &'a [Diagnostic],
}

impl<'a> StateCtx<'a> {
    fn new(tbl: &'a StateTable, existing_diags: &'a [Diagnostic]) -> Self {
        StateCtx {
            tbl,
            flow: FlowFacts::default(),
            diags: Vec::new(),
            existing_diags,
        }
    }

    /// Join every path that meets here, and report each value the paths left in
    /// different states instead of keeping whichever path was walked last.
    fn merge_states(&mut self, before: &FlowFacts, paths: &[FlowFacts], span: Span) {
        let mut diverged = Vec::new();
        self.flow = FlowFacts::merge_paths_with_state_diagnostics(before, paths, &mut diverged);
        self.report_divergence(diverged, span);
    }

    fn report_divergence(
        &mut self,
        mut diverged: Vec<crate::Sema::FlowFacts::Divergence<String>>,
        span: Span,
    ) {
        diverged.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then(a.left.cmp(&b.left))
                .then(a.right.cmp(&b.right))
        });
        diverged.dedup_by(|a, b| a.name == b.name);
        for split in diverged {
            self.diags
                .push(l0152(&split.name, &split.left, &split.right, span));
        }
    }

    /// Walk one path from the facts that reach it and report where it ends.
    fn walk_path(&mut self, before: &FlowFacts, body: &[Stmt]) -> FlowFacts {
        self.flow = before.clone();
        self.check_block(body);
        self.flow.clone()
    }

    /// The shared loop rule is stated once in `FlowFacts::after_loop`.
    fn check_loop_body(&mut self, body: &[Stmt], span: Span) {
        let before = self.flow.clone();
        self.check_block(body);
        let after_body = self.flow.clone();
        let mut diverged = Vec::new();
        self.flow =
            FlowFacts::after_loop_with_state_diagnostics(&before, &after_body, &mut diverged);
        self.report_divergence(diverged, span);
    }

    /// Resolve a static-method receiver (`Payment.Client.client()`) to a type name.
    fn static_method_type_name(receiver: &Expr) -> Option<String> {
        match receiver {
            Expr::Ident(name, _) => Some(name.clone()),
            Expr::Field(base, leaf, _) => {
                if let Expr::Ident(prefix, _) = base.as_ref() {
                    if prefix
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_uppercase())
                    {
                        return Some(format!("{prefix}.{leaf}"));
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// If `init` is a constructor call to an entry transition (`Type.ctor()`),
    /// return the to-state the produced value starts in.
    fn entry_state_of(&self, init: &Expr) -> Option<String> {
        match init {
            Expr::OrFallback { value, .. }
            | Expr::Copy(value, _)
            | Expr::Place(value, _, _)
            | Expr::Paren(value, _)
            | Expr::Ok(value, _)
            | Expr::Try(value, _, _, _) => self.entry_state_of(value),
            // D-CONC-UNIT1: the canonical spawn expression enters the task
            // lifecycle in `Running`.
            Expr::MethodCall { method, .. }
                if method == crate::Syntax::INTERNAL_TASK_SPAWN_METHOD =>
            {
                Some(TASK_RUNNING.to_string())
            }
            // `Type.ctor(…)` / `Ns.Type.ctor(…)` — static entry transition.
            Expr::MethodCall {
                receiver, method, ..
            } => {
                let type_name = Self::static_method_type_name(receiver)?;
                let key = format!("{type_name}::{method}");
                self.tbl.entry_ctors.get(&key).cloned()
            }
            // A free-function entry transition (`from = None`).
            Expr::Call(Call { name, .. }) => match self.tbl.fn_transitions.get(name) {
                Some((None, to)) => Some(to.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    fn seed_entry_state(&mut self, name: &str, init: &Expr) -> bool {
        let Some(state) = self.entry_state_of(init) else {
            return false;
        };
        if !crate::Sema::knowledge_gate_allows(
            KnowledgePlane::State,
            KnowledgeGate::StateTransition,
        ) {
            return false;
        }
        self.flow.states.set(name, state);
        true
    }

    fn check_block(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.check_stmt(s);
        }
    }

    fn check_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(e) | Stmt::Yield(e, _) | Stmt::DeferClose { close: e, .. } => {
                self.check_expr(e);
            }
            Stmt::BreakValue(e, _) | Stmt::BreakLabelValue(_, _, e, _) => {
                self.check_expr(e);
            }
            Stmt::Val(b) => {
                self.check_expr(&b.init);
                // A binding may rebind from a transition call: `r := r.confirm()`
                // gives `r` the call's to-state. Otherwise seed from an entry ctor.
                if !b.name.is_empty() {
                    if let Some(to) = self.result_state_of(&b.init) {
                        self.flow.states.set(&b.name, to);
                    } else if !self.seed_entry_state(&b.name, &b.init) {
                        // The binding takes on the state of the local it aliases, if
                        // any (`s := r`), else becomes untracked.
                        if let Expr::Ident(src, _) = &b.init {
                            if let Some(st) = self.flow.states.get(src).cloned() {
                                self.flow.states.set(&b.name, st);
                            } else {
                                self.flow.states.remove(&b.name);
                            }
                        } else {
                            self.flow.states.remove(&b.name);
                        }
                    }
                }
            }
            Stmt::Assign {
                target, value, op, ..
            } => {
                self.check_expr(value);
                if op.is_none() {
                    if let LValue::Local { name, .. } = target {
                        if let Some(to) = self.result_state_of(value) {
                            self.flow.states.set(name, to);
                        } else if !self.seed_entry_state(name, value) {
                            self.flow.states.remove(name);
                        }
                    }
                }
            }
            Stmt::Return(Some(e), _) => self.check_expr(e),
            Stmt::Return(None, _) => {}
            Stmt::While {
                cond, body, span, ..
            } => {
                self.check_expr(cond);
                self.check_loop_body(body, *span);
            }
            Stmt::For {
                kind, body, span, ..
            } => {
                if let crate::AST::ForKind::Range {
                    start,
                    end,
                    step,
                    exclusive: _,
                } = kind
                {
                    self.check_expr(start);
                    self.check_expr(end);
                    if let Some(s) = step {
                        self.check_expr(s);
                    }
                } else if let crate::AST::ForKind::In { collection, step } = kind {
                    self.check_expr(collection);
                    if let Some(step) = step {
                        self.check_expr(step);
                    }
                }
                self.check_loop_body(body, *span);
            }
            Stmt::Switch {
                subject,
                arms,
                else_body,
                span,
            }
            | Stmt::ComptimeSwitch {
                subject,
                arms,
                else_body,
                span,
            } => {
                self.check_expr(subject);
                let before = self.flow.clone();
                let mut paths = Vec::new();
                for a in arms {
                    self.flow = before.clone();
                    self.check_expr(&a.cond);
                    self.check_block(&a.body);
                    paths.push(self.flow.clone());
                }
                match else_body {
                    Some(b) => paths.push(self.walk_path(&before, b)),
                    // No `else`: skipping every arm is itself a path, unless
                    // CheckerCore already proved this pattern table exhaustive.
                    None if !crate::Sema::FlowFacts::switch_proven_exhaustive(
                        arms,
                        self.existing_diags,
                        *span,
                    ) =>
                    {
                        paths.push(before.clone());
                    }
                    None => {}
                }
                self.merge_states(&before, &paths, *span);
            }
            Stmt::CountedLoop {
                init,
                cond,
                step,
                body,
                span,
                ..
            } => {
                self.check_expr(&init.init);
                self.check_expr(cond);
                let before = self.flow.clone();
                self.check_block(body);
                if let Some(step) = step {
                    self.check_stmt(step);
                }
                let after_body = self.flow.clone();
                let mut diverged = Vec::new();
                self.flow = FlowFacts::after_loop_with_state_diagnostics(
                    &before,
                    &after_body,
                    &mut diverged,
                );
                self.report_divergence(diverged, *span);
            }
            Stmt::Loop { body, span, .. } => self.check_loop_body(body, *span),
            Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Reactive { body, .. }
            | Stmt::Shield { body, .. }
            | Stmt::Switched { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::Policy { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::AuthorityScope { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::ScopeMember { body, .. }
            | Stmt::Live { body, .. } => self.check_block(body),
            // D-META-STAGE1=B (formerly D-CTMARKER1): comptime block erases; walk body conservatively.
            Stmt::ComptimeBlock { body, .. } => self.check_block(body),
            Stmt::ComptimeIf {
                cond,
                then_body,
                else_body,
                span,
                ..
            } => {
                self.check_expr(cond);
                let before = self.flow.clone();
                let then_path = self.walk_path(&before, then_body);
                let other_path = match else_body {
                    Some(b) => self.walk_path(&before, b),
                    None => before.clone(),
                };
                self.merge_states(&before, &[then_path, other_path], *span);
            }
            Stmt::ContextBlock { fields, body, .. } => {
                for (_, e, _) in fields {
                    self.check_expr(e);
                }
                self.check_block(body);
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {
            }
        }
    }

    /// The to-state a call expression leaves its receiver/result in, if it is a
    /// tracked transition. Used to thread `r := r.confirm()` rebinding.
    fn result_state_of(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::OrFallback { value, .. }
            | Expr::Copy(value, _)
            | Expr::Place(value, _, _)
            | Expr::Paren(value, _)
            | Expr::Ok(value, _)
            | Expr::Try(value, _, _, _) => self.result_state_of(value),
            Expr::MethodCall {
                receiver,
                method,
                recv_type,
                ..
            } => {
                let static_ty = recv_type
                    .is_none()
                    .then(|| Self::static_method_type_name(receiver))
                    .flatten();
                let ty = recv_type.as_deref().or(static_ty.as_deref())?;
                // A task transition discharges the handle; its result is not
                // another task row.
                if ty == crate::Syntax::TYPE_TASK {
                    return None;
                }
                let key = format!("{ty}::{method}");
                let (_, to) = self.tbl.transitions.get(&key)?;
                if !crate::Sema::knowledge_gate_allows(
                    KnowledgePlane::State,
                    KnowledgeGate::StateTransition,
                ) {
                    return None;
                }
                // A transition's result carries the to-state even when the
                // receiver is not a tracked local (for example, a static
                // entry constructor).
                Some(to.clone())
            }
            Expr::Call(Call { name, .. }) => {
                let (_, to) = self.tbl.fn_transitions.get(name)?;
                if !crate::Sema::knowledge_gate_allows(
                    KnowledgePlane::State,
                    KnowledgeGate::StateTransition,
                ) {
                    return None;
                }
                // A transition's result carries the to-state regardless of
                // whether its first argument is a local binding.
                Some(to.clone())
            }
            _ => None,
        }
    }


    /// Walk an expression for typestate violations and apply in-place transitions
    /// (a transition call in expression-statement position advances the receiver).
    fn check_expr(&mut self, e: &Expr) {
        match e {
            Expr::MethodCall {
                receiver,
                method,
                method_span,
                recv_type,
                args,
                ..
            } => {
                self.check_expr(receiver);
                for a in args {
                    self.check_expr(&a.expr);
                }
                let Expr::Ident(local, _) = receiver.as_ref() else {
                    return;
                };
                // Built-in Task methods intentionally leave `recv_type` empty;
                // the task row is the type fact this pass already owns.
                let task_key = format!(
                    "{}::{}",
                    crate::Syntax::TYPE_TASK,
                    method
                );
                let implicit_task = recv_type.is_none()
                    && self.flow.states.get(local).is_some_and(|state| {
                        matches!(state.as_str(), TASK_RUNNING | TASK_JOINED | TASK_DETACHED)
                    })
                    && self.tbl.transitions.contains_key(&task_key);
                let Some(ty) = recv_type.as_deref().or_else(|| {
                    implicit_task.then_some(crate::Syntax::TYPE_TASK)
                }) else {
                    return;
                };
                let key = format!("{ty}::{method}");
                let cur = self.flow.states.get(local).cloned();
                // A require-state guard: receiver must currently be in `req`.
                if ty != crate::Syntax::TYPE_TASK {
                    if let Some(req) = self.tbl.requires.get(&key) {
                        self.check_state(local, cur.as_deref(), req, ty, method, *method_span);
                    }
                }
                // A task's lifecycle row records the transition; its join
                // duty owns the user-facing consumed-handle diagnostic.
                if let Some((from, to)) = self.tbl.transitions.get(&key) {
                    if ty != crate::Syntax::TYPE_TASK {
                        if let Some(req) = from {
                            self.check_state(local, cur.as_deref(), req, ty, method, *method_span);
                        }
                    }
                    if crate::Sema::knowledge_gate_allows(
                        KnowledgePlane::State,
                        KnowledgeGate::StateTransition,
                    ) {
                        self.flow.states.set(local, to.clone());
                    }
                }
            }
            Expr::Call(Call { name, args, .. }) => {
                for a in args {
                    self.check_expr(&a.expr);
                }
                // Free-fn typestate operates on its first argument when it is a local.
                let first_local = args.first().and_then(|a| match &a.expr {
                    Expr::Ident(n, _) => Some(n.clone()),
                    _ => None,
                });
                let Some(local) = first_local else { return };
                let span = args
                    .first()
                    .map(|a| a.expr.span())
                    .unwrap_or(Span::new(0, 0));
                let cur = self.flow.states.get(&local).cloned();
                if let Some(req) = self.tbl.fn_requires.get(name) {
                    self.check_state(&local, cur.as_deref(), req, &local, name, span);
                }
                if let Some((from, to)) = self.tbl.fn_transitions.get(name) {
                    if let Some(req) = from {
                        self.check_state(&local, cur.as_deref(), req, &local, name, span);
                    }
                    if crate::Sema::knowledge_gate_allows(
                        KnowledgePlane::State,
                        KnowledgeGate::StateTransition,
                    ) {
                        self.flow.states.set(&local, to.clone());
                    }
                }
            }
            Expr::Tainted(inner, _, _)
            | Expr::Unary(_, inner, _)
            | Expr::IncDec { operand: inner, .. }
            | Expr::Deref(inner, _)
            | Expr::RawOf(inner, _)
            | Expr::Copy(inner, _)
            | Expr::Place(inner, _, _)
            | Expr::Field(inner, _, _)
            | Expr::Present(inner, _)
            | Expr::Ok(inner, _)
            | Expr::Err(inner, _) => self.check_expr(inner),
            Expr::Try(inner, _, _, note) => {
                self.check_expr(inner);
                if let Some(note) = note {
                    self.check_expr(note);
                }
            }
            Expr::Binary(_, l, r, _) => {
                self.check_expr(l);
                self.check_expr(r);
            }
            Expr::CompareChain { operands, .. } => {
                for e in operands {
                    self.check_expr(e);
                }
            }
            Expr::CallValue { callee, args, .. } => {
                self.check_expr(callee);
                for a in args {
                    self.check_expr(&a.expr);
                }
            }
            Expr::OptField { base, .. } => self.check_expr(base),
            Expr::Index { base, index, .. } => {
                self.check_expr(base);
                self.check_expr(index);
            }
            Expr::Slice {
                base, start, end, range, ..
            } => {
                self.check_expr(base);
                if let Some(range) = range {
                    self.check_expr(range);
                } else {
                    self.check_expr(start);
                    self.check_expr(end);
                }
            }
            Expr::Range { start, end, .. } => {
                self.check_expr(start);
                self.check_expr(end);
            }
            Expr::ListLit(elems, _) => elems.iter().for_each(|el| self.check_expr(el)),
            Expr::MapLit(entries, _) => entries.iter().for_each(|(k, v)| {
                self.check_expr(k);
                self.check_expr(v);
            }),
            Expr::TupleLit(fields, _, _) => fields.iter().for_each(|(_, e)| self.check_expr(e)),
            Expr::StructLit { fields, .. } => {
                fields.iter().for_each(|(_, _, f)| self.check_expr(f))
            }
            Expr::TypedLit { body, .. } => {
                body.for_each_expr(|f| self.check_expr(f))
            }
            Expr::EnumLit { args, .. } => args.iter().for_each(|a| match a {
                crate::AST::EnumLitArg::Positional(e) => self.check_expr(e),
                crate::AST::EnumLitArg::Named { expr, .. } => self.check_expr(expr),
            }),
            Expr::Str(parts, _) => parts.iter().for_each(|p| {
                if let crate::AST::StrPart::Interp(e, _) = p {
                    self.check_expr(e);
                }
            }),
            Expr::If {
                cond,
                then_body,
                then_value,
                else_body,
                else_value,
                span,
            } => {
                self.check_expr(cond);
                let before = self.flow.clone();
                self.check_block(then_body);
                self.check_expr(then_value);
                let then_path = self.flow.clone();
                self.flow = before.clone();
                self.check_block(else_body);
                self.check_expr(else_value);
                let else_path = self.flow.clone();
                self.merge_states(&before, &[then_path, else_path], *span);
            }
            Expr::PatternTest { subject, .. } => self.check_expr(subject),
            Expr::PtrFromAddr { addr, .. } => self.check_expr(addr),
            Expr::OrFallback {
                value, fallback, ..
            } => {
                self.check_expr(value);
                match fallback {
                    crate::AST::OrFallback::Value(e) => self.check_expr(e),
                    crate::AST::OrFallback::Block { body, value, .. } => {
                        self.check_block(body);
                        if let Some(value) = value {
                            self.check_expr(value);
                        }
                    }
                    crate::AST::OrFallback::Return(Some(e), _) => self.check_expr(e),
                    _ => {}
                }
            }
            // Leaves and forms with no tracked sub-expression.
            Expr::Ident(..)
            | Expr::Int(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::Unit(..)
            | Expr::Char(..)
            | Expr::Absent(_)
            | Expr::ReduceMarker(_, _)
            | Expr::Todo { .. }
        | Expr::NoElse(_)
            | Expr::Lambda(_)
            | Expr::UnitLit { .. }
            | Expr::ComptimeName { .. }
            // D-SHIFT1 (c7shift) / D-BINPAT1 (card #506 follow-up): a leaf
            // literal, no nested `Expr` to recurse into.
            | Expr::StrMatchLit(_, _)
            | Expr::BinMatchLit(_, _) => {}
            Expr::Paren(inner, _) => self.check_expr(inner),
            Expr::Spread(inner, _) => self.check_expr(inner),
            Expr::MemberSpread { base, .. } => self.check_expr(base),
        }
    }

    /// Emit E0150 if the value's current state is known and differs from the
    /// required state. When the state is unknown (untracked value), stay silent —
    /// no false positive on code the dataflow can't follow.
    fn check_state(
        &mut self,
        local: &str,
        cur: Option<&str>,
        required: &str,
        owner: &str,
        op: &str,
        span: Span,
    ) {
        if let Some(cur) = cur {
            if cur != required {
                self.diags.push(e0150(
                    local,
                    owner,
                    op,
                    required,
                    cur,
                    &self.transition_hint(owner, required),
                    span,
                ));
            }
        }
    }

    /// Find a transition whose to-state is `required` on the same owner, to name in
    /// the fix-it ("call `<fn>` to reach `<state>`"). Returns the op name or "".
    fn transition_hint(&self, owner: &str, required: &str) -> String {
        // Method transitions: keys are `Owner::method`.
        for (key, (_, to)) in &self.tbl.transitions {
            if to == required {
                if let Some((ty, m)) = key.split_once("::") {
                    if ty == owner {
                        return m.to_string();
                    }
                }
            }
        }
        for (name, (_, to)) in &self.tbl.fn_transitions {
            if to == required {
                return name.clone();
            }
        }
        String::new()
    }
}

/// Run the typestate pass over one function body. The receiver's incoming state is
/// seeded from a `#State(S)`/`#Transition(S, _)` marker on `self` so a method body
/// that itself transitions starts from the declared state.
pub fn check_func_state(
    f: &Func,
    owner: Option<&str>,
    tbl: &StateTable,
    existing_diags: &[Diagnostic],
) -> Vec<Diagnostic> {
    let mut ctx = StateCtx::new(tbl, existing_diags);
    // Seed `self`'s incoming state from this function's own typestate marker so a
    // chain of self-transitions inside one body checks correctly.
    if f.self_param().is_some() {
        let incoming = f
            .state_requires
            .as_ref()
            .map(|(s, _)| StateTable::state_for_owner(owner, s))
            .or_else(|| {
                f.state_transition
                    .as_ref()
                    .and_then(|t| t.from.as_deref())
                    .map(|state| StateTable::state_for_owner(owner, state))
            });
        if let Some(s) = incoming {
            ctx.flow.states.set(crate::Syntax::KW_SELF, s);
        }
    }
    ctx.check_block(&f.body);
    ctx.diags
}

/// Run the typestate pass over every function/method body in a set of items.
pub fn check_items_state(items: &[Item], tbl: &StateTable, diags: &mut Vec<Diagnostic>) {
    for item in items {
        match item {
            Item::Func(f) => {
                let new = check_func_state(f, None, tbl, diags.as_slice());
                diags.extend(new);
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    let new = check_func_state(m, Some(&i.type_name), tbl, diags.as_slice());
                    diags.extend(new);
                }
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    let new = check_func_state(m, Some(&s.name), tbl, diags.as_slice());
                    diags.extend(new);
                }
                for block in &s.trait_impls {
                    for m in &block.methods {
                        let new = check_func_state(m, Some(&s.name), tbl, diags.as_slice());
                        diags.extend(new);
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    let new = check_func_state(m, Some(&e.name), tbl, diags.as_slice());
                    diags.extend(new);
                }
            }
            Item::Test(t) => {
                let new = {
                    let mut ctx = StateCtx::new(tbl, diags.as_slice());
                    ctx.check_block(&t.body);
                    ctx.diags
                };
                diags.extend(new);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_spawn_seeds_a_running_typestate_row() {
        let span = Span::new(0, 0);
        let spawn = Expr::MethodCall {
            receiver: Box::new(Expr::Ident(
                crate::Syntax::INTERNAL_TASK_RECEIVER.to_string(),
                span,
            )),
            method: crate::Syntax::INTERNAL_TASK_SPAWN_METHOD.to_string(),
            method_span: span,
            owner_type_args: Vec::new(),
            type_args: Vec::new(),
            args: Vec::new(),
            recv_type: Some(crate::Syntax::INTERNAL_TASK_SURFACE_TYPE.to_string()),
            resolved_ret: None,
            checked_widen: false,
        };
        let table = StateTable::with_facts(Default::default());
        let mut ctx = StateCtx::new(&table, &[]);
        assert!(ctx.seed_entry_state("handle", &spawn));

        assert_eq!(
            ctx.flow.states.get("handle").map(|state| state.as_str()),
            Some(TASK_RUNNING)
        );
        assert_eq!(
            table
                .transitions
                .get(&format!(
                    "{}::{}",
                    crate::Syntax::TYPE_TASK,
                    crate::Syntax::TASK_JOIN
                ))
                .map(|(_, to)| to.as_str()),
            Some(TASK_JOINED)
        );
    }
}

/// E0151 (D-STATE-HOME1=A): a `#State(X)` or `#Transition(A, B)` marker references a
/// state name that is not in the owning struct's `state { … }` section for that type.
/// Includes a typo suggestion when the edit distance is ≤ 2.
pub fn e0151(state: &str, type_name: &str, candidates: &[&str], span: Span) -> Diagnostic {
    let candidate_name = state.rsplit('.').next().unwrap_or(state);
    let mut best: Option<(&str, usize)> = None;
    let mut ambiguous = false;
    for &candidate in candidates {
        let distance = edit_distance(candidate_name, candidate);
        if distance == 0 || distance > 2 {
            continue;
        }
        match best {
            None => best = Some((candidate, distance)),
            Some((_, best_distance)) if distance < best_distance => {
                best = Some((candidate, distance));
                ambiguous = false;
            }
            Some((best_candidate, best_distance)) if distance == best_distance => {
                ambiguous |= best_candidate != candidate;
            }
            _ => {}
        }
    }
    let suggestion = best.filter(|_| !ambiguous).map(|(candidate, _)| candidate);
    let fix = if let Some(c) = suggestion {
        format!("did you mean `{c}`? Check the `struct {type_name} {{ state {{ … }} }}` declaration for valid names")
    } else {
        format!(
            "add `{state}` inside `struct {type_name} {{ state {{ … }} }}`, or correct the spelling"
        )
    };
    let mut diagnostic = Diagnostic::error(
        "E0151",
        format!("`{state}` is not a declared state of `{type_name}`"),
        format!(
            "typestate (D-STATE-HOME1=A): `struct {type_name} {{ state {{ … }} }}` defines the valid state labels; \
             `{state}` is not among them — a typo here would silently create a phantom state that no \
             transition reaches"
        ),
        fix,
        Some(span),
    );
    if let Some(candidate) = suggestion {
        diagnostic = diagnostic.with_edit(TextEdit {
            span,
            new_text: candidate.to_string(),
        });
    }
    diagnostic
}

/// E0159 (D-STATE-HOME1=A): a nominal owner uses typestate markers without
/// declaring the one state section that owns their fact set.
pub fn e0159(type_name: &str, span: Span) -> Diagnostic {
    Diagnostic::from_row("E0159", &[("type", type_name)], Some(span))
}

/// E0166: one struct state section repeats a state label.
pub fn e0166(state: &str, type_name: &str, span: Span) -> Diagnostic {
    Diagnostic::from_row(
        "E0166",
        &[("state", state), ("type", type_name)],
        Some(span),
    )
}

/// E0167: a state label would collide with a normal member of its owner.
pub fn e0167(state: &str, type_name: &str, span: Span) -> Diagnostic {
    Diagnostic::from_row(
        "E0167",
        &[("state", state), ("type", type_name)],
        Some(span),
    )
}

/// E0169: a state section has no labels.
pub fn e0169(type_name: &str, span: Span) -> Diagnostic {
    Diagnostic::from_row("E0169", &[("type", type_name)], Some(span))
}

/// L0153 (D-STATE-TERMINAL1): a declared state is unreachable from every entry
/// transition in a graph that declares an entry.
pub fn l0153(state: &str, type_name: &str, span: Span) -> Diagnostic {
    Diagnostic::from_row(
        "L0153",
        &[("state", state), ("type", type_name)],
        Some(span),
    )
}

/// L0152 (D-STATE1, D-FACT-FLOW1): two paths meet and leave one value in
/// different states, so from here the compiler can no longer say which state it
/// is in. A warning, not an error: the code may never need the state again.
pub fn l0152(value: &str, one: &str, other: &str, span: Span) -> Diagnostic {
    Diagnostic::from_row(
        "L0152",
        &[("value", value), ("one", one), ("other", other)],
        Some(span),
    )
}

/// E0150 (D-STATE1): a typestate operation is called on a value in the wrong state.
/// Names the operation, both states, and the transition that reaches the required
/// state.
pub fn e0150(
    local: &str,
    owner: &str,
    op: &str,
    required: &str,
    current: &str,
    transition: &str,
    span: Span,
) -> Diagnostic {
    let fix = if transition.is_empty() {
        format!("transition `{local}` into state `{required}` before calling `{op}`")
    } else {
        format!("transition it first: call `{transition}` to reach `{required}`")
    };
    Diagnostic::error(
        "E0150",
        format!("`{op}` needs `{owner}` in state `{required}`, but `{local}` is in state `{current}`"),
        format!(
            "typestate (D-STATE1): `{op}` is only valid in state `{required}`; calling it in `{current}` is the out-of-order-events bug it prevents"
        ),
        fix,
        Some(span),
    )
}
