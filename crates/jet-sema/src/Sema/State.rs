//! Typestate (D-STATE1 / D-STATE-DECL / D-STATE-REQ / D-STATE-TRANS).
//!
//! A value moves through a named set of *states*. Operations declare the state they
//! need and the state they leave the value in, via two fn markers:
//!
//!   - `#State(S) fn m(self, …)` — a **require-state** guard: `m` is valid only
//!     when its receiver is currently in state `S`. Calling it in any other state
//!     is **E0150**. The state is unchanged by the call.
//!   - `#Transition(From -> To) fn m(self, …) -> T` — a **transition**: it consumes
//!     a value in state `From` and yields one in state `To`. A call requires the
//!     receiver be in `From` (E0150 otherwise) and **advances** it to `To`. The
//!     from-state may be `_` (an *entry* transition: a constructor that produces the
//!     initial state from nothing — e.g. `#Transition(_ -> Pending) fn new() -> R`).
//!
//! D-STATE-DECL (ratified 2026-06-25, option B): states are declared in a dedicated
//! block `state TypeName { Pending, Confirmed, CheckedIn }`. When present:
//!   - `#State(X)` / `#Transition(A -> B)` on `TypeName::*` must reference declared
//!     state names; an unknown name is **E0151** (typo against the set).
//!   - A declared state with no outgoing `#Transition(S -> …)` is a dead-end warning
//!     **L0151** (a half-built machine still compiles).
//!   - The set erases (compile-time only, no runtime discriminant).
//!
//! The current state of a value is a **compile-time fact** threaded by intraprocedural
//! forward dataflow over locals. Nothing about the state reaches codegen (I3, zero
//! runtime cost). When a value's state cannot be tracked precisely (it escapes into a
//! field, a non-local receiver, a loop-carried position), the checker is **silent**
//! rather than guessing (P1 — beginners never see a spurious error).

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{Call, ElseBranch, Expr, Func, IfStmt, Item, LValue, Stmt};
use std::collections::{HashMap, HashSet};

/// Program-wide typestate metadata, collected once before any body is walked.
#[derive(Default)]
pub struct StateTable {
    /// `Type::method` → required state (`#State(S)`). The receiver must be in this
    /// state at the call.
    requires: HashMap<String, String>,
    /// `Type::method` → (from-state, to-state) for a `#Transition(From -> To)`.
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
    /// D-STATE-DECL: type name → declared state labels with their spans. When a type
    /// has a `state TypeName { … }` block, every `#State(X)` / `#Transition(A -> B)`
    /// marker on its methods must reference a name from this set (else E0151).
    declared: HashMap<String, Vec<(String, Span)>>,
}

impl StateTable {
    /// Build the table from a program's items.
    pub fn build(items: &[Item]) -> Self {
        let mut t = StateTable::default();
        t.add_items(items);
        t
    }

    /// Register every typestate marker in `items` into this table. Methods key as
    /// `Type::method`; entry transitions (`_ -> To`) also register under
    /// `entry_ctors` so a constructor call can seed a local's initial state.
    /// D-STATE-DECL: `state TypeName { … }` blocks are also registered here so
    /// `validate_declarations` can check markers against declared sets.
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
                // D-STATE-DECL: register the bounded state-set for this type.
                Item::StateDecl(sd) => {
                    self.declared
                        .insert(sd.type_name.clone(), sd.states.clone());
                }
                _ => {}
            }
        }
    }

    /// D-STATE-DECL: validate that every `#State(X)` / `#Transition(A -> B)` marker
    /// on methods of a type that has a `state TypeName { … }` declaration references
    /// a state in the declared set. Unknown state → E0151. Also warns (L0151) about
    /// declared states with no outgoing `#Transition(S -> …)` (dead-end states).
    pub fn validate_declarations(&self, items: &[Item], diags: &mut Vec<Diagnostic>) {
        for (type_name, decl_states) in &self.declared {
            let state_names: HashSet<&str> = decl_states.iter().map(|(n, _)| n.as_str()).collect();

            // Collect all method markers for this type.
            let mut outgoing: HashSet<String> = HashSet::new();

            // Helper to check a single state name against the declared set.
            let check_state = |state: &str, span: Span, diags: &mut Vec<Diagnostic>| {
                if !state_names.contains(state) {
                    let candidates: Vec<&str> = state_names
                        .iter()
                        .filter(|&&s| edit_distance(state, s) <= 2)
                        .copied()
                        .collect();
                    diags.push(e0151(state, type_name, &candidates, span));
                }
            };

            // Walk all method markers on this type.
            for item in items {
                let methods: Vec<&Func> = match item {
                    Item::Impl(i) if i.type_name == *type_name => i.methods.iter().collect(),
                    Item::Struct(s) if s.name == *type_name => {
                        let mut ms: Vec<&Func> = s.methods.iter().collect();
                        for block in &s.trait_impls {
                            ms.extend(block.methods.iter());
                        }
                        ms
                    }
                    Item::Enum(e) if e.name == *type_name => e.methods.iter().collect(),
                    _ => continue,
                };
                for m in methods {
                    if let Some((state, span)) = &m.state_requires {
                        check_state(state, *span, diags);
                    }
                    if let Some(tr) = &m.state_transition {
                        if let Some(from) = &tr.from {
                            check_state(from, tr.span, diags);
                            outgoing.insert(from.clone());
                        }
                        check_state(&tr.to, tr.span, diags);
                    }
                }
            }

            // L0151: dead-end state — declared but no outgoing transition.
            // D-PROTO2: protocol handle terminal states are intentional completion points.
            let protocol_handle = type_name.contains('.')
                && (type_name.ends_with(".Client") || type_name.ends_with(".Server"));
            for (i, (state, span)) in decl_states.iter().enumerate() {
                if !outgoing.contains(state.as_str()) {
                    if protocol_handle && i + 1 == decl_states.len() {
                        continue;
                    }
                    diags.push(l0151(state, type_name, *span));
                }
            }
        }
    }

    fn add_method(&mut self, type_name: &str, m: &Func) {
        let key = format!("{type_name}::{}", m.name);
        if let Some((state, _)) = &m.state_requires {
            self.requires.insert(key.clone(), state.clone());
        }
        if let Some(tr) = &m.state_transition {
            self.transitions
                .insert(key, (tr.from.clone(), tr.to.clone()));
            if tr.from.is_none() {
                self.entry_ctors
                    .insert(format!("{type_name}::{}", m.name), tr.to.clone());
            }
        }
    }

    fn add_free_fn(&mut self, f: &Func) {
        if let Some((state, _)) = &f.state_requires {
            self.fn_requires.insert(f.name.clone(), state.clone());
        }
        if let Some(tr) = &f.state_transition {
            self.fn_transitions
                .insert(f.name.clone(), (tr.from.clone(), tr.to.clone()));
        }
    }

    /// True when the program declares no typestate at all — lets the caller skip
    /// the per-body walk and declaration validation entirely.
    pub fn is_empty(&self) -> bool {
        self.requires.is_empty()
            && self.transitions.is_empty()
            && self.fn_requires.is_empty()
            && self.fn_transitions.is_empty()
            && self.declared.is_empty()
    }
}

/// Per-function typestate analyzer. Tracks each tracked local's current state.
struct StateCtx<'a> {
    tbl: &'a StateTable,
    /// local name → current state tag.
    states: HashMap<String, String>,
    diags: Vec<Diagnostic>,
}

impl<'a> StateCtx<'a> {
    fn new(tbl: &'a StateTable) -> Self {
        StateCtx {
            tbl,
            states: HashMap::new(),
            diags: Vec::new(),
        }
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

    fn check_block(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.check_stmt(s);
        }
    }

    fn check_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(e) | Stmt::Yield(e, _) => {
                self.check_expr(e);
            }
            Stmt::Val(b) => {
                self.check_expr(&b.init);
                // A binding may rebind from a transition call: `r := r.confirm()`
                // gives `r` the call's to-state. Otherwise seed from an entry ctor.
                if !b.name.is_empty() {
                    if let Some(to) = self.result_state_of(&b.init) {
                        self.states.insert(b.name.clone(), to);
                    } else if let Some(to) = self.entry_state_of(&b.init) {
                        self.states.insert(b.name.clone(), to);
                    } else {
                        // The binding takes on the state of the local it aliases, if
                        // any (`s := r`), else becomes untracked.
                        if let Expr::Ident(src, _) = &b.init {
                            if let Some(st) = self.states.get(src).cloned() {
                                self.states.insert(b.name.clone(), st);
                            } else {
                                self.states.remove(&b.name);
                            }
                        } else {
                            self.states.remove(&b.name);
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
                        if let Some(to) = self
                            .result_state_of(value)
                            .or_else(|| self.entry_state_of(value))
                        {
                            self.states.insert(name.clone(), to);
                        } else {
                            self.states.remove(name);
                        }
                    }
                }
            }
            Stmt::Return(Some(e), _) => self.check_expr(e),
            Stmt::Return(None, _) => {}
            Stmt::If(ifs) => self.check_if(ifs),
            Stmt::While { cond, body, .. } => {
                self.check_expr(cond);
                self.check_block(body);
            }
            Stmt::For { kind, body, .. } => {
                if let crate::AST::ForKind::Range { start, end, step } = kind {
                    self.check_expr(start);
                    self.check_expr(end);
                    if let Some(s) = step {
                        self.check_expr(s);
                    }
                } else if let crate::AST::ForKind::In { collection } = kind {
                    self.check_expr(collection);
                }
                self.check_block(body);
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
            } => {
                self.check_expr(subject);
                for a in arms {
                    self.check_expr(&a.cond);
                    self.check_block(&a.body);
                }
                if let Some(b) = else_body {
                    self.check_block(b);
                }
            }
            Stmt::CountedLoop {
                init,
                cond,
                step,
                body,
                ..
            } => {
                self.check_expr(&init.init);
                self.check_expr(cond);
                self.check_block(body);
                self.check_stmt(step);
            }
            Stmt::Loop { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Reactive { body, .. }
            | Stmt::SuppressMustUse { body, .. }
            | Stmt::Off { body, .. }
            | Stmt::DebugOnly { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::ScopeMember { body, .. }
            | Stmt::Live { body, .. } => self.check_block(body),
            // D-CTMARKER1: comptime block erases; walk body conservatively.
            Stmt::ComptimeBlock { body, .. } => self.check_block(body),
            Stmt::ComptimeIf {
                cond,
                then_body,
                else_body,
                ..
            } => {
                self.check_expr(cond);
                self.check_block(then_body);
                if let Some(b) = else_body {
                    self.check_block(b);
                }
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

    fn check_if(&mut self, ifs: &IfStmt) {
        self.check_expr(&ifs.cond);
        // Branches are checked against the state at the `if`; transitions inside a
        // branch do not leak out (conservative — a value whose state diverges across
        // arms becomes untracked at the join, handled by `join_after`).
        let before = self.states.clone();
        self.check_block(&ifs.then_body);
        let after_then = std::mem::replace(&mut self.states, before.clone());
        if let Some(e) = &ifs.else_branch {
            self.check_else(e);
        }
        let after_else = std::mem::take(&mut self.states);
        self.states = join_after(before, after_then, after_else);
    }

    fn check_else(&mut self, e: &ElseBranch) {
        match e {
            ElseBranch::Else(stmts) => self.check_block(stmts),
            ElseBranch::ElseIf(ifs) => self.check_if(ifs),
        }
    }

    /// The to-state a call expression leaves its receiver/result in, if it is a
    /// tracked transition. Used to thread `r := r.confirm()` rebinding.
    fn result_state_of(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::OrFallback { value, .. } | Expr::Paren(value, _) => self.result_state_of(value),
            Expr::MethodCall {
                receiver,
                method,
                recv_type,
                ..
            } => {
                let ty = recv_type.as_ref()?;
                let key = format!("{ty}::{method}");
                let (_, to) = self.tbl.transitions.get(&key)?;
                // Only meaningful when the receiver is a tracked local.
                if let Expr::Ident(_, _) = receiver.as_ref() {
                    Some(to.clone())
                } else {
                    None
                }
            }
            Expr::Call(Call { name, args, .. }) => {
                let (_, to) = self.tbl.fn_transitions.get(name)?;
                // The first argument is the tracked value.
                if let Some(first) = args.first() {
                    if let Expr::Ident(_, _) = &first.expr {
                        return Some(to.clone());
                    }
                }
                None
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
                let Some(ty) = recv_type else { return };
                let Expr::Ident(local, _) = receiver.as_ref() else {
                    return;
                };
                let key = format!("{ty}::{method}");
                let cur = self.states.get(local).cloned();
                // A require-state guard: receiver must currently be in `req`.
                if let Some(req) = self.tbl.requires.get(&key) {
                    self.check_state(local, cur.as_deref(), req, ty, method, *method_span);
                }
                // A transition: require `from` (unless entry), then advance to `to`.
                if let Some((from, to)) = self.tbl.transitions.get(&key) {
                    if let Some(req) = from {
                        self.check_state(local, cur.as_deref(), req, ty, method, *method_span);
                    }
                    self.states.insert(local.clone(), to.clone());
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
                let cur = self.states.get(&local).cloned();
                if let Some(req) = self.tbl.fn_requires.get(name) {
                    self.check_state(&local, cur.as_deref(), req, name, name, span);
                }
                if let Some((from, to)) = self.tbl.fn_transitions.get(name) {
                    if let Some(req) = from {
                        self.check_state(&local, cur.as_deref(), req, name, name, span);
                    }
                    self.states.insert(local, to.clone());
                }
            }
            Expr::Tainted(inner, _)
            | Expr::Unary(_, inner, _)
            | Expr::IncDec { operand: inner, .. }
            | Expr::Deref(inner, _)
            | Expr::RawOf(inner, _)
            | Expr::Copy(inner, _)
            | Expr::Field(inner, _, _)
            | Expr::Present(inner, _)
            | Expr::Ok(inner, _)
            | Expr::Err(inner, _)
            | Expr::Try(inner, _, _) => self.check_expr(inner),
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
                base, start, end, ..
            } => {
                self.check_expr(base);
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
                ..
            } => {
                self.check_expr(cond);
                self.check_block(then_body);
                self.check_expr(then_value);
                self.check_block(else_body);
                self.check_expr(else_value);
            }
            Expr::FanOut { items, callee, .. } => {
                self.check_expr(callee);
                items.iter().for_each(|e| self.check_expr(e));
            }
            Expr::PatternTest { subject, .. } => self.check_expr(subject),
            Expr::PtrFromAddr { addr, .. } => self.check_expr(addr),
            Expr::OrFallback {
                value, fallback, ..
            } => {
                self.check_expr(value);
                match fallback {
                    crate::AST::OrFallback::Value(e) => self.check_expr(e),
                    crate::AST::OrFallback::Return(Some(e), _) => self.check_expr(e),
                    _ => {}
                }
            }
            // Leaves and forms with no tracked sub-expression.
            Expr::Ident(..)
            | Expr::Int(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::Char(..)
            | Expr::Absent(_)
            | Expr::ReduceMarker(_, _)
            | Expr::Todo { .. }
            | Expr::Lambda(_)
            | Expr::UnitLit { .. }
            | Expr::ComptimeSplice { .. }
            // D-SHIFT1 (c7shift): a leaf literal, no nested `Expr` to recurse into.
            | Expr::StrMatchLit(_, _) => {}
            Expr::Paren(inner, _) => self.check_expr(inner),
            Expr::Spread(inner, _) => self.check_expr(inner),
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

/// Join two branch state-maps back into one: a local keeps its state only when both
/// branches agree (and it had one before, or both produced the same). Disagreement →
/// untracked (no spurious guard error after a state-divergent `if`).
fn join_after(
    before: HashMap<String, String>,
    then_s: HashMap<String, String>,
    else_s: HashMap<String, String>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (k, v) in &then_s {
        if else_s.get(k) == Some(v) {
            out.insert(k.clone(), v.clone());
        }
    }
    // Keep pre-branch states for locals untouched by either branch.
    for (k, v) in before {
        if then_s.get(&k) == Some(&v) && else_s.get(&k) == Some(&v) {
            out.insert(k, v);
        }
    }
    out
}

/// Run the typestate pass over one function body. The receiver's incoming state is
/// seeded from a `#State(S)`/`#Transition(S -> _)` marker on `self` so a method body
/// that itself transitions starts from the declared state.
pub fn check_func_state(f: &Func, tbl: &StateTable) -> Vec<Diagnostic> {
    let mut ctx = StateCtx::new(tbl);
    // Seed `self`'s incoming state from this function's own typestate marker so a
    // chain of self-transitions inside one body checks correctly.
    if f.self_param().is_some() {
        let incoming = f
            .state_requires
            .as_ref()
            .map(|(s, _)| s.clone())
            .or_else(|| f.state_transition.as_ref().and_then(|t| t.from.clone()));
        if let Some(s) = incoming {
            ctx.states.insert(crate::Syntax::KW_SELF.to_string(), s);
        }
    }
    ctx.check_block(&f.body);
    ctx.diags
}

/// Run the typestate pass over every function/method body in a set of items.
pub fn check_items_state(items: &[Item], tbl: &StateTable, diags: &mut Vec<Diagnostic>) {
    for item in items {
        match item {
            Item::Func(f) => diags.extend(check_func_state(f, tbl)),
            Item::Impl(i) => {
                for m in &i.methods {
                    diags.extend(check_func_state(m, tbl));
                }
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    diags.extend(check_func_state(m, tbl));
                }
                for block in &s.trait_impls {
                    for m in &block.methods {
                        diags.extend(check_func_state(m, tbl));
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    diags.extend(check_func_state(m, tbl));
                }
            }
            Item::Test(t) => diags.extend({
                let mut ctx = StateCtx::new(tbl);
                ctx.check_block(&t.body);
                ctx.diags
            }),
            _ => {}
        }
    }
}

/// E0151 (D-STATE-DECL): a `#State(X)` or `#Transition(A -> B)` marker references a
/// state name that is not in the `state TypeName { … }` declaration for that type.
/// Includes a typo suggestion when the edit distance is ≤ 2.
pub fn e0151(state: &str, type_name: &str, candidates: &[&str], span: Span) -> Diagnostic {
    let fix = if let Some(c) = candidates.first() {
        format!("did you mean `{c}`?  Check the `state {type_name} {{ … }}` block for valid names")
    } else {
        format!(
            "add `{state}` to the `state {type_name} {{ … }}` declaration, or correct the spelling"
        )
    };
    Diagnostic::error(
        "E0151",
        format!("`{state}` is not a declared state of `{type_name}`"),
        format!(
            "typestate (D-STATE-DECL): `state {type_name} {{ … }}` defines the valid state labels; \
             `{state}` is not among them — a typo here would silently create a phantom state that no \
             transition reaches"
        ),
        fix,
        Some(span),
    )
}

/// L0151 (D-STATE-DECL): a declared state has no outgoing `#Transition(S -> …)`,
/// making it a dead end — a value in this state can never advance further.
/// This is a warning (not an error) so a half-built machine still compiles.
pub fn l0151(state: &str, type_name: &str, span: Span) -> Diagnostic {
    Diagnostic::lint(
        "L0151",
        format!("`{state}` (in `state {type_name}`) has no outgoing transition"),
        format!(
            "typestate (D-STATE-DECL): a state with no `#Transition({state} -> …)` is a dead end — \
             a value that reaches `{state}` can never advance to another state"
        ),
        format!(
            "add `#Transition({state} -> NextState) fn …` on `{type_name}`, or remove `{state}` from the declaration"
        ),
        Some(span),
    )
}

/// Simple Levenshtein distance (capped at 3) for state-name suggestions in E0151.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return n.min(3);
    }
    if n == 0 {
        return m.min(3);
    }
    let mut row: Vec<usize> = (0..=n).collect();
    for i in 1..=m {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            let next = (row[j] + 1).min(row[j - 1] + 1).min(prev + cost);
            prev = row[j];
            row[j] = next;
            if next >= 3 {
                break;
            }
        }
    }
    row[n]
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
