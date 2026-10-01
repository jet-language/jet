use crate::AST::{AccessConvention, Expr, Type};
use crate::Diagnostics::Span;
use crate::Sema::Registration::already_defined;
use crate::Sema::{Checker, LocalInfo};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

impl<'a> Checker<'a> {
    pub(crate) fn push_scope(&mut self) {
        self.flow.enter_scope();
        self.concrete_unit_values.push(HashMap::new());
        self.lambda_mut_borrow_stack.push(HashSet::new());
        self.ct_scopes.push(HashMap::new());
    }

    pub(crate) fn pop_scope(&mut self) {
        self.check_single_use_consumed_in_current_scope();
        self.drop_scope_no_obligation_checks();
    }

    pub(crate) fn drop_scope_no_obligation_checks(&mut self) {
        // D-FACT-FLOW1: every scope-lived plane — bindings, flow narrowing
        // and open windows — leaves the one store together.
        self.flow.leave_scope();
        self.concrete_unit_values.pop();
        self.lambda_mut_borrow_stack.pop();
        self.ct_scopes.pop();
    }

    /// Number of open scopes. A fact recorded now leaves at this depth.
    pub(crate) fn scope_depth(&self) -> usize {
        self.flow.depth
    }

    /// Record a binding in the scope that is open now, with none of
    /// `declare`'s redefinition checks. Parameters and loop variables use
    /// this: their own caller already decided the name is free.
    pub(crate) fn declare_in_scope(&mut self, name: &str, info: LocalInfo) {
        let depth = self.flow.depth;
        if !info.invalid {
            self.note_unused_binding(name, info.def_span, info.param_conv.is_some());
        }
        // D-FACT-OWN1: every binding enters the shared crossing plane with
        // the ownership prover's answer. Do not give parameters and other
        // scope-only bindings a private/default sendability bit.
        let sendable = self.sendability_problem(&info.ty, true).is_none();
        self.flow.bindings.set_at(name, depth, info);
        self.flow.sendability.set_at(name, depth, sendable);
        self.flow.path_strings.remove_at(name, depth);
        self.flow
            .origins
            .set_at(name, depth, crate::Sema::FlowFacts::OriginFact::untracked());
    }

    /// Every name a binding is known by here, at any depth. Callers use it
    /// to match spellings, so the order carries no meaning.
    pub(crate) fn visible_names(&self) -> Vec<String> {
        self.flow
            .bindings
            .all()
            .map(|(name, _)| name.to_string())
            .collect()
    }

    pub(crate) fn lambda_mut_borrow_active(&self, name: &str) -> bool {
        self.lambda_mut_borrow_stack
            .iter()
            .any(|s| s.contains(name))
    }

    pub(crate) fn current_ct_globals(&self) -> Cow<'_, HashMap<String, crate::Comptime::CtValue>> {
        // Most semantic expressions do not open a comptime scope. Borrow the
        // immutable module globals instead of cloning every known value for
        // each node; overlays still receive the exact old merged snapshot.
        if self.ct_scopes.iter().all(|scope| scope.is_empty()) {
            return Cow::Borrowed(self.ct_globals);
        }
        let mut globals = self.ct_globals.clone();
        for scope in &self.ct_scopes {
            for (name, value) in scope {
                globals.insert(name.clone(), value.clone());
            }
        }
        Cow::Owned(globals)
    }

    /// Types for names visible to the compile-time evaluator. CtValue erases
    /// nominal text facts, so folding must carry the checker-owned declaration
    /// type alongside the value. Only names with values are exposed: the MIR
    /// fragment receives each typed name as an argument.
    pub(crate) fn current_ct_binding_types(
        &self,
        values: &HashMap<String, crate::Comptime::CtValue>,
    ) -> HashMap<String, Type> {
        values
            .keys()
            .filter_map(|name| {
                self.flow
                    .bindings
                    .get(name)
                    .map(|info| (name.clone(), info.ty.clone()))
                    .or_else(|| self.consts.get(name).map(|ty| (name.clone(), ty.clone())))
            })
            .collect()
    }

    /// Record what a folded initializer changed about earlier bindings.
    /// `r :: Reader.over(bytes)` followed by `magic :: r.read_u32_le()?`
    /// advances `r`; without writing that back, every later fold reads a
    /// reader still parked at position zero. Only names a comptime scope
    /// already holds are updated.
    pub(crate) fn apply_ct_mutations(
        &mut self,
        mutated: HashMap<String, crate::Comptime::CtValue>,
    ) {
        for (name, value) in mutated {
            if let Some(slot) = self
                .ct_scopes
                .iter_mut()
                .rev()
                .find_map(|scope| scope.get_mut(&name))
            {
                *slot = value;
            }
        }
    }

    /// Whether `expr` is an explicit constant, so a diagnostic may evaluate
    /// it. Compile time is always explicit, so only literal forms qualify:
    /// literals (including unit literals and their elaborated constructor),
    /// type-, enum- or module-qualified members such as `Method.Get` or
    /// `math.PI`, negation and arithmetic over those, module constants with
    /// a known value (a `prep` constant or a literal initializer), and local
    /// `prep` bindings whose value is live in the compile-time frame. An
    /// ordinary binding or a function call never counts as a constant here.
    pub(crate) fn is_explicit_constant(
        &self,
        expr: &Expr,
        values: &HashMap<String, crate::Comptime::CtValue>,
    ) -> bool {
        let constant = |inner: &Expr| self.is_explicit_constant(inner, values);
        match expr {
            Expr::Int(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::Char(..)
            | Expr::Unit(_)
            | Expr::Absent(_)
            | Expr::UnitLit { .. }
            | Expr::ComptimeName { .. } => true,
            Expr::Str(parts, _) => parts.iter().all(|part| match part {
                crate::AST::StrPart::Lit(_) => true,
                crate::AST::StrPart::Interp(inner, _) => constant(inner),
            }),
            Expr::Paren(inner, _) | Expr::Unary(_, inner, _) | Expr::Present(inner, _) => {
                constant(inner)
            }
            Expr::Binary(_, left, right, _) => constant(left) && constant(right),
            Expr::CompareChain { operands, .. } | Expr::ListLit(operands, _) => {
                operands.iter().all(constant)
            }
            Expr::TupleLit(fields, ..) => fields.iter().all(|(_, value)| constant(value)),
            Expr::MapLit(entries, _) => entries
                .iter()
                .all(|(key, value)| constant(key) && constant(value)),
            Expr::EnumLit { args, .. } => args.iter().all(|arg| match arg {
                crate::AST::EnumLitArg::Positional(value)
                | crate::AST::EnumLitArg::Named { expr: value, .. } => constant(value),
            }),
            Expr::Ident(name, _) => self.names_explicit_constant(name, values),
            Expr::Field(base, ..) => self.qualifies_explicit_constant(base, values),
            // Sema elaborates a unit literal in place: `5s` becomes
            // `Duration.nanoseconds(5 * scale) ?? panic(…)`, `2.5px` becomes
            // `Px.float(2.5)` and `4i` becomes `Complex(0.0, 4.0)`.
            Expr::OrFallback {
                value,
                fallback: crate::AST::OrFallback::Panic { .. },
                ..
            } => self.is_elaborated_unit_literal(value) && constant(value),
            Expr::MethodCall { args, .. } => {
                self.is_elaborated_unit_literal(expr) && args.iter().all(|arg| constant(&arg.expr))
            }
            Expr::Call(call) if call.name == crate::Syntax::TYPE_COMPLEX => {
                call.args.iter().all(|arg| constant(&arg.expr))
            }
            _ => false,
        }
    }

    /// The constructor call sema writes in place of a unit literal.
    fn is_elaborated_unit_literal(&self, expr: &Expr) -> bool {
        let Expr::MethodCall {
            receiver, method, ..
        } = expr
        else {
            return false;
        };
        let Expr::Ident(owner, _) = receiver.as_ref() else {
            return false;
        };
        if self.lookup(owner).is_some() {
            return false;
        }
        (owner == crate::Syntax::DURATION_TYPE && method == "nanoseconds")
            || (self.registry.is_distinct(owner)
                && crate::Syntax::numeric_conversion_method("Float") == Some(method.as_str()))
    }

    /// The base of a member read `base.member` that keeps it constant: a
    /// type, enum or module name (`Method.Get`, `math.PI`), or a value that
    /// is itself an explicit constant.
    fn qualifies_explicit_constant(
        &self,
        base: &Expr,
        values: &HashMap<String, crate::Comptime::CtValue>,
    ) -> bool {
        match base {
            Expr::Field(inner, ..) => self.qualifies_explicit_constant(inner, values),
            Expr::Ident(name, _) if self.lookup(name).is_none() && !self.consts.contains_key(name) => {
                true
            }
            _ => self.is_explicit_constant(base, values),
        }
    }

    /// A name read is constant when it is a module constant with a known
    /// value or a local `prep` binding whose value is live.
    fn names_explicit_constant(
        &self,
        name: &str,
        values: &HashMap<String, crate::Comptime::CtValue>,
    ) -> bool {
        if self.lookup(name).is_none() {
            // A module-level name: only a constant with a known value.
            return values.contains_key(name);
        }
        let Some(depth) = self.flow.bindings.depth_of(name) else {
            return false;
        };
        // Flow depth counts open scopes, so the function scope is depth 1
        // while its compile-time frame is `ct_scopes[0]`.
        let Some(local_value) = depth
            .checked_sub(1)
            .and_then(|index| self.ct_scopes.get(index))
            .and_then(|scope| scope.get(name))
        else {
            // This lexical local shadows any same-named outer value, but no
            // live compile-time value belongs to this binding.
            return false;
        };
        values.get(name) == Some(local_value)
    }

    pub(crate) fn evaluate_constant(
        &self,
        expr: &crate::AST::Expr,
    ) -> Option<crate::Comptime::CtValue> {
        if self.defer_ct_evaluation {
            return None;
        }
        let globals = self.current_ct_globals().into_owned();
        if !self.is_explicit_constant(expr, &globals) {
            return None;
        }
        let binding_types = self.current_ct_binding_types(&globals);
        let checked_nominals = self.checked_comptime_nominals();
        crate::Comptime::evaluate_owned_with_imports_opts_collecting_items(
            expr,
            self.ct_checked_funcs_for_evaluation(),
            self.ct_externs,
            self.ct_base_dir,
            &globals,
            &binding_types,
            self.core_imports,
            self.gates,
            0,
            &[],
            checked_nominals,
            None,
        )
        .ok()
        .map(|(value, _)| value)
    }


    /// What the checker knows about a name here: the innermost declaration,
    /// or the flow-narrowed refinement of it when a proven test recorded one
    /// at the same depth or deeper (D-FLOWTYPE1).
    pub(crate) fn lookup(&self, name: &str) -> Option<&LocalInfo> {
        if name == "_" {
            return None;
        }
        let declared = self.flow.bindings.depth_of(name);
        let narrowed = self.flow.narrow.depth_of(name);
        match (declared, narrowed) {
            (Some(declared), Some(narrowed)) if narrowed >= declared => self.flow.narrow.get(name),
            (Some(_), _) => self.flow.bindings.get(name),
            (None, Some(_)) => self.flow.narrow.get(name),
            (None, None) => None,
        }
    }

    /// Module-level mutable bindings are the one module-level write target.
    /// Keep the permission fact on the declaration; do not create a second
    /// mutable-binding table beside `consts`.
    pub(crate) fn is_global_mutable_binding(&self, name: &str) -> bool {
        self.items.iter().any(|item| {
            matches!(
                item,
                crate::AST::Item::Const(c)
                    if c.name == name && c.mutable && !c.is_comptime
            )
        })
    }

    pub(crate) fn sendability_for(&self, name: &str) -> bool {
        let Some(depth) = self.binding_fact_depth(name) else {
            return true;
        };
        self.flow
            .sendability
            .get_at(name, depth)
            .copied()
            .unwrap_or(true)
    }

    /// D-CONC-FREEZE1=A: read the one frozen proof attached to the active
    /// binding/refinement. A missing row means the value is not frozen.
    pub(crate) fn frozen_for(&self, name: &str) -> Option<Span> {
        let declared = self.flow.bindings.depth_of(name);
        let narrowed = self.flow.narrow.depth_of(name);
        match (declared, narrowed) {
            (Some(declared), Some(narrowed)) if narrowed >= declared => self
                .flow
                .frozen
                .get_at(name, narrowed)
                .or_else(|| self.flow.frozen.get_at(name, declared))
                .copied(),
            (Some(declared), _) => self.flow.frozen.get_at(name, declared).copied(),
            (None, Some(narrowed)) => self.flow.frozen.get_at(name, narrowed).copied(),
            (None, None) => None,
        }
    }

    pub(crate) fn binding_fact_depth(&self, name: &str) -> Option<usize> {
        let declared = self.flow.bindings.depth_of(name);
        let narrowed = self.flow.narrow.depth_of(name);
        match (declared, narrowed) {
            (Some(declared), Some(narrowed)) if narrowed >= declared => Some(narrowed),
            (Some(declared), _) => Some(declared),
            (None, Some(narrowed)) => Some(narrowed),
            (None, None) => None,
        }
    }

    /// D-CONC-FREEZE1=A: find the proof that a place expression is backed
    /// by a frozen value. Deep immutability follows every place projection.
    pub(crate) fn frozen_expr_site(&self, expr: &Expr) -> Option<Span> {
        match expr {
            Expr::Ident(name, _) => self.frozen_for(name),
            Expr::Field(base, ..)
            | Expr::Index { base, .. }
            | Expr::Slice { base, .. }
            | Expr::Place(base, ..)
            | Expr::Paren(base, _) => self.frozen_expr_site(base),
            Expr::Call(call) if call.name == crate::Syntax::KW_FREEZE && call.args.len() == 1 => {
                Some(call.name_span)
            }
            _ => None,
        }
    }

    /// Update the callback representation fact after a function-valued
    /// local is assigned. The fact is deliberately attached to the
    /// binding, not inferred again at the eventual host call: an unsafe
    /// reassignment must not leave a stale `Send` proof behind.
    pub(crate) fn set_interrupt_sendable(&mut self, name: &str, sendable: bool) {
        if let Some(info) = self.flow.bindings.get_mut(name) {
            info.interrupt_sendable = sendable && info.param_conv.is_none();
        }
        if let Some(info) = self.flow.narrow.get_mut(name) {
            info.interrupt_sendable = sendable && info.param_conv.is_none();
        }
    }

    /// A binding is borrowed (a `view`) when it is a `Read` parameter of a
    /// non-scalar type — in v1 those lower to `&T`, so the value can't be moved
    /// out of it. Used to decide where a consuming use must clone (B1).
    pub(crate) fn is_borrowed_binding(&self, name: &str) -> bool {
        self.lookup(name)
            .map(|info| {
                matches!(info.param_conv, Some(AccessConvention::Read)) && !info.ty.is_scalar()
            })
            .unwrap_or(false)
    }

    pub(crate) fn declare(&mut self, name: &str, name_span: Span, info: LocalInfo) {
        if name == "_" {
            return;
        }
        if !info.invalid {
            self.note_unused_binding(name, info.def_span, info.param_conv.is_some());
        }
        if self.lookup(name).is_some()
            || self.consts.contains_key(name)
            || self.loop_labels.iter().any(|label| label == name)
        {
            self.diags.push(already_defined(name, name_span));
        }
        self.clear_moved_binding(name);
        let depth = self.flow.depth;
        // A fresh declaration replaces any refinement recorded for the same
        // name in this scope: the new binding is what the name now means.
        self.flow.narrow.remove_at(name, depth);
        self.flow.sendability.remove_at(name, depth);
        self.flow.frozen.remove_at(name, depth);
        self.flow.path_strings.remove_at(name, depth);
        self.flow.bindings.set_at(name, depth, info);
        self.flow.sendability.set_at(name, depth, true);
        self.flow
            .origins
            .set_at(name, depth, crate::Sema::FlowFacts::OriginFact::untracked());
    }

    pub(crate) fn declare_with_sendability(
        &mut self,
        name: &str,
        name_span: Span,
        info: LocalInfo,
        sendable: bool,
    ) {
        self.declare(name, name_span, info);
        let depth = self.flow.depth;
        self.flow.sendability.set_at(name, depth, sendable);
    }

    pub(crate) fn declare_loop_var(&mut self, name: String, name_span: Span, ty: &Type) {
        if name == "_" {
            return;
        }
        // D-LOOP-SUBJECT1=A: an inner bindingless loop cannot shadow its
        // outer implicit subject; its diagnostic teaches named bindings.
        if name == crate::Syntax::KW_IT && self.implicit_loop_subject_depth > 0 {
            return;
        }
        if self.lookup(&name).is_some()
            || self.consts.contains_key(&name)
            || self.loop_labels.iter().any(|label| label == &name)
        {
            self.diags.push(already_defined(&name, name_span));
        } else {
            let depth = self.flow.depth;
            self.note_unused_binding(&name, name_span, false);
            // A fresh loop binding is a new value: a move of an earlier
            // binding with the same name (an earlier loop's variable) is gone.
            self.clear_moved_binding(&name);
            self.flow.bindings.set_at(
                &name,
                depth,
                LocalInfo {
                    def_span: name_span,
                    binding_sigil_span: None,
                    ty: ty.clone(),
                    mutable: false,
                    param_conv: None,
                    decl_loop_depth: self.loop_depth,
                    interrupt_sendable: false,
                    reactive_local: false,
                    reactive_shared: false,
                    single_use_span: None,
                    invalid: false,
                },
            );
            self.flow.sendability.set_at(&name, depth, true);
            self.flow.path_strings.remove_at(&name, depth);
            self.flow.origins.set_at(
                &name,
                depth,
                crate::Sema::FlowFacts::OriginFact::untracked(),
            );
        }
    }

    /// Error recovery for a loop whose source failed to check: its item type
    /// is unknown, so bind each loop variable as invalid. A use then stays
    /// silent (the source already reported) instead of cascading into E0107
    /// "nothing named `row`" reports that suggest an unrelated similar name.
    pub(crate) fn declare_invalid_loop_vars(
        &mut self,
        var: &str,
        var_span: Span,
        var2: Option<&(String, Span)>,
    ) {
        let names = std::iter::once((var, var_span))
            .chain(var2.map(|(name, span)| (name.as_str(), *span)));
        for (name, name_span) in names {
            if name == "_" || self.lookup(name).is_some() {
                continue;
            }
            let depth = self.flow.depth;
            self.clear_moved_binding(name);
            self.flow.bindings.set_at(
                name,
                depth,
                LocalInfo {
                    def_span: name_span,
                    binding_sigil_span: None,
                    ty: Type::Named(String::new()),
                    mutable: false,
                    param_conv: None,
                    decl_loop_depth: self.loop_depth,
                    interrupt_sendable: false,
                    reactive_local: false,
                    reactive_shared: false,
                    single_use_span: None,
                    invalid: true,
                },
            );
        }
    }

    /// Replace the active binding's origin with the scoped "not tracked" row.
    /// Removing it would make an outer shadowed origin visible again.
    pub(crate) fn clear_origin(&mut self, name: &str) {
        let depth = self
            .flow
            .origins
            .depth_of(name)
            .or_else(|| self.binding_fact_depth(name))
            .unwrap_or_else(|| self.scope_depth());
        self.flow
            .origins
            .set_at(name, depth, crate::Sema::FlowFacts::OriginFact::untracked());
    }

    pub(crate) fn declare_loop_label(&mut self, name: &str, name_span: Span) {
        if self.lookup(name).is_some()
            || self.consts.contains_key(name)
            || self.loop_labels.iter().any(|label| label == name)
        {
            self.diags.push(already_defined(name, name_span));
        }
        self.loop_labels.push(name.to_string());
    }
}
