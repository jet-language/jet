use crate::AST::{ElseBranch, IfStmt, Stmt, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::CheckerOwnership::e0141_unconsumed_branch;
use crate::Sema::{Checker, LocalInfo};
use std::collections::HashMap;
impl<'a> Checker<'a> {
        pub(crate) fn check_if(&mut self, ifs: &mut IfStmt) {
            let before = self.moved.clone();
            let mut after = before.clone();
            // D-UNINIT1 engine (reused by D-UNINIT-SENTINEL1): definite-assignment
            // merge. A `:= uninit` name is initialized after the `if` only if it is
            // written on *every* path; it stays uninit if still-uninit in any branch
            // (or, with no `else`, on the fall-through).
            let before_u = self.uninit.clone();
            let mut after_u: HashMap<String, Span> = HashMap::new();
            let bindings = self.check_condition_with_bindings(&mut ifs.cond);
            self.push_scope();
            for (name, ty) in bindings {
                self.declare(
                    &name,
                    ifs.span,
                    LocalInfo {
                        def_span: ifs.span,
                        ty,
                        mutable: false,
                        param_conv: None,
                        decl_loop_depth: self.loop_depth,
                        sendable: true,
                        task_lint_span: None,
                        single_use_span: None,
                        constant_value: None,
                    },
                );
            }
            // D-LIN1: the `@SingleUse` bindings that outlive this `if` — declared in an
            // enclosing scope (not the `if`-cond scope just pushed) and not already
            // consumed before the `if`. These are the candidates for the
            // consumed-on-one-branch check (E0141).
            let single_use_live: Vec<(String, Span)> = self
                .scopes
                .iter()
                .rev()
                .skip(1) // skip the if-cond scope pushed above
                .flat_map(|s| s.iter())
                .filter_map(|(name, info)| {
                    let span = info.single_use_span?;
                    if before.contains_key(name) {
                        None
                    } else {
                        Some((name.clone(), span))
                    }
                })
                .collect();
    
            self.check_block(&mut ifs.then_body, false);
            self.pop_scope();
            let then_moved = self.moved.clone();
            for (k, v) in self.moved.drain() {
                after.entry(k).or_insert(v);
            }
            for (k, v) in std::mem::take(&mut self.uninit) {
                after_u.entry(k).or_insert(v);
            }
            self.moved = before.clone();
            self.uninit = before_u.clone();
            // `None` else: the cond-false path consumes nothing.
            let mut else_moved = before.clone();
            match &mut ifs.else_branch {
                None => {
                    // The cond-false path runs no branch, so everything stays uninit.
                    for (k, v) in &before_u {
                        after_u.entry(k.clone()).or_insert(*v);
                    }
                }
                Some(ElseBranch::Else(else_body)) => {
                    self.check_block(else_body, true);
                    else_moved = self.moved.clone();
                    for (k, v) in self.moved.drain() {
                        after.entry(k).or_insert(v);
                    }
                    for (k, v) in std::mem::take(&mut self.uninit) {
                        after_u.entry(k).or_insert(v);
                    }
                }
                Some(ElseBranch::ElseIf(next)) => {
                    self.check_if(next);
                    else_moved = self.moved.clone();
                    for (k, v) in self.moved.drain() {
                        after.entry(k).or_insert(v);
                    }
                    for (k, v) in std::mem::take(&mut self.uninit) {
                        after_u.entry(k).or_insert(v);
                    }
                }
            }
            // D-LIN1 / E0141: a `@SingleUse` binding consumed on exactly one branch
            // leaves the other path with the value unused. Report it once, on the
            // branch where it WAS consumed (asymmetry is the bug), pointing at the
            // binding. (Both-consumed → fine; neither → falls through to E0140.)
            let mut diverged: Vec<(String, Span)> = single_use_live
                .into_iter()
                .filter(|(name, _)| then_moved.contains_key(name) != else_moved.contains_key(name))
                .collect();
            diverged.sort_by(|a, b| a.1.start.cmp(&b.1.start).then(a.0.cmp(&b.0)));
            for (name, span) in diverged {
                self.diags.push(e0141_unconsumed_branch(&name, span));
            }
            self.moved = after;
            self.uninit = after_u;
        }
    
        /// D-WHEN1/D-WHEN2 (ratified 2026-06-19): check a `comptime if` statement.
        ///
        /// Steps:
        /// 1. Evaluate the condition with the comptime interpreter. The condition
        ///    must be Bool and comptime-evaluable (else E0989).
        /// 2. Select the arm whose condition is true.
        /// 3. Full type-check + lower the selected arm (`check_block`).
        /// 4. D-WHEN2: run a name-resolution-only pass over the dropped arm —
        /// D-CTMARKER1 (ratified 2026-06-25, piece 2): run a `comptime { … }` block at
        /// build time via the comptime interpreter (D-CTCORE1 pure path). Any error
        /// (E0951 impurity / E0953 panic / E0956 unsupported) is surfaced as a diagnostic.
        pub(crate) fn check_comptime_block(&mut self, stmt: &mut Stmt) {
            let Stmt::ComptimeBlock { body, .. } = stmt else {
                return;
            };
            let globals = self.current_ct_globals();
            match crate::Comptime::run_block_with_imports(
                body,
                self.ct_funcs,
                self.ct_externs,
                self.ct_base_dir,
                &globals,
                self.core_imports,
            ) {
                Ok(scope) => {
                    let current = self.ct_scopes.last_mut().unwrap();
                    for (name, value) in scope {
                        current.insert(name, value);
                    }
                }
                Err(d) => self.diags.push(d),
            }
        }
    
        ///    we enter `in_dropped_comptime_arm` mode, call `check_block`, but
        ///    then keep only `E0107` (unknown name) diagnostics from that pass.
        ///    This catches typos in dead code without requiring the arm to
        ///    type-check against the current context.
        pub(crate) fn check_comptime_if(&mut self, stmt: &mut Stmt) {
            let Stmt::ComptimeIf {
                cond,
                cond_span,
                then_body,
                else_body,
                selected_then,
                ..
            } = stmt
            else {
                return;
            };
            // Step 1: evaluate the condition at comptime.
            // D-CTCORE1: pass core_imports for whitelisted pure Core calls.
            let globals = self.current_ct_globals();
            let selected = match crate::Comptime::evaluate_owned_with_imports_opts(
                cond,
                self.ct_funcs,
                self.ct_externs,
                self.ct_base_dir,
                &globals,
                self.core_imports,
                self.allow_impure && self.ct_impure_depth > 0,
                self.ct_impure_depth,
            ) {
                Ok(crate::Comptime::CtValue::Bool(b)) => b,
                Ok(_) => {
                    // The condition evaluated but isn't Bool — fall back to E0989
                    // (non-Bool condition is the same as non-comptime for users).
                    self.diags.push(Diagnostic::error(
                        "E0989",
                        format!(
                            "a `comptime if` condition must be {}, not another type",
                            Type::Bool.show()
                        ),
                        "the condition selects a branch at compile time — it must be true or false"
                            .to_string(),
                        "write a Bool comptime expression, like `comptime if flag { … }`".to_string(),
                        Some(*cond_span),
                    ));
                    return;
                }
                Err(_d) => {
                    // Condition failed to evaluate — not comptime-evaluable (E0989).
                    self.diags.push(Diagnostic::error(
                        "E0989",
                        "this `comptime if` condition can't be known at compile time".to_string(),
                        "a `comptime if` condition must be a comptime expression — a `comptime` binding, a literal, or a pure function call with comptime arguments (D-WHEN1)".to_string(),
                        "use a `comptime` binding: `comptime flag = …; comptime if flag { … }`"
                            .to_string(),
                        Some(*cond_span),
                    ));
                    return;
                }
            };
            *selected_then = Some(selected);
    
            // Step 3: full type-check on the selected arm.
            if selected {
                self.check_block(then_body, true);
            } else if let Some(eb) = else_body {
                self.check_block(eb, true);
            }
    
            // Step 4: D-WHEN2 — name-resolution-only pass on the dropped arm.
            let dropped_arm: Option<&mut Vec<Stmt>> = if selected {
                else_body.as_mut()
            } else {
                Some(then_body)
            };
            if let Some(dropped) = dropped_arm {
                let diag_before = self.diags.len();
                let prev_flag = self.in_dropped_comptime_arm;
                self.in_dropped_comptime_arm = true;
                self.check_block(dropped, true);
                self.in_dropped_comptime_arm = prev_flag;
                // D-WHEN2: keep name-resolution diagnostics from the dropped arm
                // (unknown variable E0107, unknown function E0102) so typos still
                // teach, but drop all type-checking diagnostics.
                let dropped_diags: Vec<Diagnostic> = self.diags.drain(diag_before..).collect();
                for d in dropped_diags {
                    if d.code == "E0107" || d.code == "E0102" {
                        self.diags.push(d);
                    }
                }
            }
        }
    
}
