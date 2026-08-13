use crate::AST::{AccessConvention, Type};
use crate::Diagnostics::Span;
use crate::Sema::Registration::already_defined;
use crate::Sema::{Checker, LocalInfo};
use std::collections::{HashMap, HashSet};
impl<'a> Checker<'a> {
        pub(crate) fn push_scope(&mut self) {
            self.flow.enter_scope();
            self.concrete_unit_values.push(HashMap::new());
            self.lambda_mut_borrow_stack.push(HashSet::new());
            self.ct_scopes.push(HashMap::new());
        }

        pub(crate) fn pop_scope(&mut self) {
            self.lint_unjoined_tasks_in_current_scope();
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
            self.flow.bindings.set_at(name, depth, info);
            self.flow.sendability.set_at(name, depth, true);
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
    
        pub(crate) fn current_ct_globals(&self) -> HashMap<String, crate::Comptime::CtValue> {
            let mut globals = self.ct_globals.clone();
            for scope in &self.ct_scopes {
                for (name, value) in scope {
                    globals.insert(name.clone(), value.clone());
                }
            }
            globals
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

        /// Bindings the fold changed under it. A binding that advances a
        /// receiver — `magic :: reader.read_u32_le()?` — only folds correctly
        /// if the emitted runtime copy of that receiver advances too, and a
        /// baked literal never does. The implicit fold path drops both the
        /// answer and the receiver instead (D-VERDICT-1308-1: silent decline).
        pub(crate) fn ct_mutated_names(
            before: &HashMap<String, crate::Comptime::CtValue>,
            after: &HashMap<String, crate::Comptime::CtValue>,
        ) -> Vec<String> {
            before
                .iter()
                .filter(|(name, value)| after.get(*name).is_some_and(|now| now != *value))
                .map(|(name, _)| name.clone())
                .collect()
        }

        /// Take names back out of the comptime world so later folds read them
        /// as unknown rather than stale.
        pub(crate) fn forget_ct_bindings(&mut self, names: &[String]) {
            for scope in &mut self.ct_scopes {
                for name in names {
                    scope.remove(name);
                }
            }
        }

        pub(crate) fn evaluate_constant(&self, expr: &crate::AST::Expr) -> Option<crate::Comptime::CtValue> {
            let mut globals = self.current_ct_globals();
            for (name, info) in self.flow.bindings.all() {
                if let Some(value) = &info.constant_value {
                    globals.insert(name.to_string(), value.clone());
                }
            }
            crate::Comptime::evaluate_owned_with_imports_opts(
                expr,
                self.ct_funcs,
                self.ct_externs,
                self.ct_base_dir,
                &globals,
                self.core_imports,
                self.gates,
                0,
            )
            .ok()
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
                (Some(declared), Some(narrowed)) if narrowed >= declared => {
                    self.flow.narrow.get(name)
                }
                (Some(_), _) => self.flow.bindings.get(name),
                (None, Some(_)) => self.flow.narrow.get(name),
                (None, None) => None,
            }
        }

        pub(crate) fn sendability_for(&self, name: &str) -> bool {
            let declared = self.flow.bindings.depth_of(name);
            let narrowed = self.flow.narrow.depth_of(name);
            let depth = match (declared, narrowed) {
                (Some(declared), Some(narrowed)) if narrowed >= declared => narrowed,
                (Some(declared), _) => declared,
                (None, Some(narrowed)) => narrowed,
                (None, None) => return true,
            };
            self.flow
                .sendability
                .get_at(name, depth)
                .copied()
                .unwrap_or(true)
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
            self.flow.bindings.set_at(name, depth, info);
            self.flow.sendability.set_at(name, depth, true);
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
            if self.lookup(&name).is_some()
                || self.consts.contains_key(&name)
                || self.loop_labels.iter().any(|label| label == &name)
            {
                self.diags.push(already_defined(&name, name_span));
            } else {
                let depth = self.flow.depth;
                self.flow.bindings.set_at(
                    &name,
                    depth,
                    LocalInfo {
                        def_span: name_span,
                        ty: ty.clone(),
                        mutable: false,
                        param_conv: None,
                        decl_loop_depth: self.loop_depth,
                        interrupt_sendable: false,
                        reactive_local: false,
                        reactive_shared: false,
                        task_lint_span: None,
                        single_use_span: None,
                        constant_value: None,
                        invalid: false,
                    },
                );
                self.flow.sendability.set_at(&name, depth, true);
            }
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
