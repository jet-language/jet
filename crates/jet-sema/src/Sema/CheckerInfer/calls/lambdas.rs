use crate::AST::{AccessConvention, Lambda, LambdaBody, Stmt, Type};
use crate::Diagnostics::Diagnostic;
use crate::Sema::Captures::{lambda_body_refs_name, lambda_collect_captures};
use crate::Sema::CheckerInfer::is_reactive_handle_ty;
use crate::Sema::Diagnostics::{is_cloneable, type_fix_hint};
use crate::Sema::{Checker, LocalInfo, SendCrossing, SendProblemKind, SendabilityProblem};
use crate::Syntax;
use std::collections::HashSet;
impl<'a> Checker<'a> {
        pub(crate) fn check_lambda(
            &mut self,
            lam: &mut Lambda,
            expected: Option<&Type>,
        ) -> Option<Type> {
            let (exp_params, exp_ret) = match expected {
                Some(Type::Fn { params, ret, .. }) => (Some(params.as_slice()), ret.as_ref()),
                _ => (None, None),
            };
    
            if let Some(ep) = exp_params {
                if lam.params.len() != ep.len() {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!(
                            "this lambda has {} parameter{}, but {} {} expected",
                            lam.params.len(),
                            if lam.params.len() == 1 { "" } else { "s" },
                            ep.len(),
                            if ep.len() == 1 { "was" } else { "were" }
                        ),
                        "parameter count must match the function type at this spot".to_string(),
                        "add or remove parameters, or fix the surrounding type".to_string(),
                        Some(lam.span),
                    ));
                }
            }
    
            let mut param_types = Vec::new();
            for (i, p) in lam.params.iter_mut().enumerate() {
                let pty = if let Some(ty) = &p.ty {
                    self.check_declared_type(ty, p.ty_span.unwrap_or(p.name_span));
                    ty.clone()
                } else if let Some(ep) = exp_params.and_then(|ps| ps.get(i)) {
                    ep.clone()
                } else {
                    self.diags.push(Diagnostic::error(
                        "E0801",
                        format!("tell me the type of `{}`", p.name),
                        "this lambda parameter has no type to go on".to_string(),
                        format!("write `({}: Int) => …` (or whatever type fits)", p.name),
                        Some(p.name_span),
                    ));
                    Type::Int
                };
                param_types.push(pty);
            }
    
            if let Some(binding) = &self.lambda_binding {
                if lambda_body_refs_name(&lam.body, binding) {
                    self.diags.push(Diagnostic::error(
                        "E0804",
                        format!("a lambda can't call itself as `{}`", binding),
                        "short functions stored in a binding can't recurse in v1".to_string(),
                        format!(
                            "write a named `{}` instead of assigning the lambda to `{}`",
                            Syntax::KW_FN,
                            binding
                        ),
                        Some(lam.span),
                    ));
                }
            }
    
            let escapes = self.lambda_escapes;
            lam.meta.escapes = escapes;
    
            let param_names: HashSet<String> = lam.params.iter().map(|p| p.name.clone()).collect();
            let take_set: HashSet<String> = lam.take_names.iter().map(|(n, _)| n.clone()).collect();
    
            let mut read_caps = HashSet::new();
            let mut mut_caps = HashSet::new();
            lambda_collect_captures(&lam.body, &param_names, &mut read_caps, &mut mut_caps);
    
            for name in read_caps.iter().chain(mut_caps.iter()) {
                if take_set.contains(name) || param_names.contains(name) {
                    continue;
                }
                // Module aliases (imports and core_imports) are always in scope in
                // lambdas — they're not local variables but they're valid references.
                // Don't report them as unknown names; the body check validates calls.
                if self.imports.contains_key(name) || self.core_imports.contains_key(name) {
                    continue;
                }
                if self.is_known_enum(name) {
                    continue;
                }
                if self.lookup(name).is_none() && !self.consts.contains_key(name) {
                    self.unknown_name(name, lam.span);
                }
            }
    
            for name in &mut_caps {
                if param_names.contains(name) || take_set.contains(name) {
                    continue;
                }
                if let Some(info) = self.lookup(name) {
                    if !info.mutable {
                        self.diags.push(Diagnostic::error(
                            "E0111",
                            format!("`{}` can't be changed inside this lambda", name),
                            "changing a value inside a short function requires a `var` binding"
                                .to_string(),
                            format!("declare `var {}: …` instead of `val`", name),
                            Some(lam.span),
                        ));
                    }
                }
            }
    
            lam.meta.needs_fn_mut = !mut_caps.is_empty();
            lam.meta.mut_captures = mut_caps
                .iter()
                .filter(|n| !take_set.contains(*n) && !param_names.contains(*n))
                .cloned()
                .collect();
    
            if escapes {
                let mut seen_caps: HashSet<String> = HashSet::new();
                for name in read_caps.iter().chain(mut_caps.iter()) {
                    if !seen_caps.insert(name.clone()) {
                        continue; // already processed this capture
                    }
                    if param_names.contains(name) {
                        continue;
                    }
                    let cap = self
                        .lookup(name)
                        .map(|i| (i.ty.clone(), i.sendable, i.param_conv))
                        .or_else(|| self.consts.get(name).map(|t| (t.clone(), true, None)));
                    let Some((cap_ty, cap_sendable, cap_conv)) = cap else {
                        continue;
                    };
                    let taken = take_set.contains(name);
                    if !cap_ty.is_scalar()
                        && matches!(
                            cap_conv,
                            Some(AccessConvention::Read) | Some(AccessConvention::Write)
                        )
                    {
                        let destination = if self.is_task_spawn {
                            "a spawned task"
                        } else {
                            "a stored lambda"
                        };
                        self.diags.push(Diagnostic::error(
                            "E0120",
                            format!(
                                "`{}` was not moved here, so it cannot be captured by {}",
                                name, destination
                            ),
                            "this function can access the parameter, but it does not own the value"
                                .to_string(),
                            format!(
                                "copy it into an owned local first: `owned {} {}{}`",
                                Syntax::SIGIL_BIND_IMMUT,
                                Syntax::SIGIL_COPY,
                                name
                            ),
                            Some(lam.span),
                        ));
                        continue;
                    }
                    if self.is_view(name) {
                        if self.is_task_spawn {
                            self.report_unsendable(
                                name,
                                &cap_ty,
                                SendabilityProblem {
                                    root: None,
                                    path: Vec::new(),
                                    kind: SendProblemKind::ViewBorrow,
                                },
                                SendCrossing::TaskCapture,
                                lam.span,
                            );
                        } else {
                            self.report_view_escape(name, "be captured by a stored lambda", lam.span);
                        }
                        continue;
                    }
                    if self.is_task_spawn {
                        // D-MEM1 stage S5: a string view (`Binding.string_view`)
                        // is `Type::String` at the type level — the general
                        // sendability check above sees a plain `String` and finds
                        // nothing wrong, unlike `View<T>` (a distinct type it
                        // already flags). Check the NAME here instead, mirroring
                        // the same `ViewBorrow` verdict `View<T>` gets, so a view
                        // can't cross into a spawned task's `'static` closure any
                        // more than a `View<T>` can (I2: this must be caught here,
                        // never surface as a real rustc lifetime rejection).
                        let problem = if !cap_sendable {
                            self.sendability_problem(&cap_ty, taken).or_else(|| {
                                Some(SendabilityProblem {
                                    root: None,
                                    path: Vec::new(),
                                    kind: SendProblemKind::ClosureCaptures,
                                })
                            })
                        } else {
                            self.sendability_problem(&cap_ty, taken)
                        };
                        if let Some(problem) = problem {
                            self.report_unsendable(
                                name,
                                &cap_ty,
                                problem,
                                SendCrossing::TaskCapture,
                                lam.span,
                            );
                            continue;
                        }
                    }
                    if mut_caps.contains(name) && !taken {
                        if self.is_task_spawn {
                            self.diags.push(Diagnostic::error(
                                "E1101",
                                format!(
                                    "`{}` is a mutable value — the new task might outlive this scope",
                                    name
                                ),
                                "tasks run concurrently; a `var` binding can't be shared between tasks".to_string(),
                                format!(
                                    "give the task its own copy (`{}{}`) or hand it over with `take({})`",
                                    Syntax::SIGIL_COPY, name, name
                                ),
                                Some(lam.span),
                            ));
                        }
                        continue; // taken by move into closure via mut borrow path
                    }
                    if mut_caps.contains(name) {
                        continue;
                    }
                    if self.is_resource_type(&cap_ty) || !is_cloneable(&cap_ty, self.registry) {
                        if !taken {
                            if self.is_task_spawn {
                                self.diags.push(Diagnostic::error(
                                    "E1101",
                                    format!(
                                        "`{}` can't be copied into a task — the task might outlive this scope",
                                        name
                                    ),
                                    "a spawned task must own everything it captures".to_string(),
                                    format!(
                                        "use `take({})` on the lambda to move `{}` into the task",
                                        name, name
                                    ),
                                    Some(lam.span),
                                ));
                            } else {
                                self.diags.push(Diagnostic::error(
                                    "E0802",
                                    format!("`{}` can't be copied into a stored lambda", name),
                                    "a lambda that outlives this line must own its captures"
                                        .to_string(),
                                    format!(
                                        "prefix the lambda with `take({})` to move `{}` in",
                                        name, name
                                    ),
                                    Some(lam.span),
                                ));
                            }
                        }
                    } else if is_reactive_handle_ty(&cap_ty) || matches!(cap_ty, Type::Shared(_)) {
                        // D-REACT1=B: a reactive `Signal`/`Derived` is an Rc-backed shared
                        // handle — capturing a "copy" shares the same reactive cell (that is
                        // the whole point: a derived/effect reads the live signal, and the
                        // outer code still `.set`s it). No silent-data-copy to warn about, so
                        // L0801 is suppressed. The capture is still recorded as a clone so
                        // codegen moves an Rc clone into the closure.
                        // D-MEM1 S6 (D-SHARED-API1=A): `Shared<T>` is the same shape — an
                        // Arc-backed "copyable door" meant to be captured freely across
                        // `tasks.spawn` closures with no `take`; suppress the same lint.
                        lam.meta.cloned_captures.push(name.clone());
                    } else if !taken {
                        lam.meta.cloned_captures.push(name.clone());
                        self.diags.push(Diagnostic::lint(
                            "L0801",
                            format!(
                                "lambda stored a copy of `{}`; write `take({})` on the lambda to move it instead",
                                name, name
                            ),
                            "a stored lambda owns its captures — clonable values are copied silently"
                                .to_string(),
                            format!(
                                "use `take({}) (…) => …` to move `{}`, or `{}{}` at the call site to copy on purpose",
                                name, name, Syntax::SIGIL_COPY, name
                            ),
                            Some(lam.span),
                        ));
                    }
                }
            }
    
            self.push_scope();
            for (p, pty) in lam.params.iter().zip(param_types.iter()) {
                self.scopes.last_mut().unwrap().insert(
                    p.name.clone(),
                    LocalInfo {
                        def_span: p.name_span,
                        ty: pty.clone(),
                        // D-MEM1 S6: `Shared<T>.edit(f)`'s closure param is the one
                        // builtin-closure shape that binds its param mutable with no
                        // `&` sigil — the exclusive write lock IS the API contract.
                        mutable: self.lambda_param_mutable,
                        param_conv: None,
                        decl_loop_depth: self.loop_depth,
                        sendable: true,
                        task_lint_span: None,
                        single_use_span: None,
                    },
                );
            }
    
            // D-TXN2: a lambda body is a deferred execution context (it runs later —
            // e.g. an `on_commit` hook fires only post-commit). Effects inside it are
            // NOT rejected by the enclosing `@Transact` block, so zero the depth here
            // and restore it after the body. This is exactly why `name.on_commit(() =>
            // { fs.write(…) })` is the D-TXN2 fix-it: the irreversible work moves into
            // a lambda, off the block's direct path.
            let saved_txn_depth = self.txn_depth;
            self.txn_depth = 0;
            let saved_expected = self.expected_type.clone();
            self.expected_type = exp_ret.map(|ret| (**ret).clone());
            let saved_in_lambda_body = self.in_lambda_body;
            self.in_lambda_body = true;
            let body_ret = match &mut lam.body {
                LambdaBody::Expr(e) => {
                    if self.is_task_spawn {
                        self.borrow_ctx = true;
                    }
                    self.infer(e)
                }
                LambdaBody::Block(stmts) => {
                    self.check_block(stmts, false);
                    let mut last_ret = None;
                    for s in stmts.iter_mut().rev() {
                        match s {
                            Stmt::Return(Some(e), _) => {
                                last_ret = self.infer(e);
                                break;
                            }
                            Stmt::Expr(e) => {
                                last_ret = self.infer_fallible_stmt(e);
                                break;
                            }
                            _ => {}
                        }
                    }
                    last_ret
                }
            };
            self.in_lambda_body = saved_in_lambda_body;
            self.expected_type = saved_expected;
            self.txn_depth = saved_txn_depth;
    
            self.pop_scope();
    
            if escapes {
                for (name, span) in &lam.take_names {
                    if let Some(info) = self.lookup(name) {
                        if !info.ty.is_scalar() {
                            if matches!(
                                info.param_conv,
                                Some(AccessConvention::Read) | Some(AccessConvention::Write)
                            ) {
                                self.diags.push(Diagnostic::error(
                                    "E0120",
                                    format!(
                                        "`{}` was not moved here, so the lambda cannot take it (`^`)",
                                        name
                                    ),
                                    "this function has read access only and does not own the value"
                                        .to_string(),
                                    format!(
                                        "take ownership in this function with `{}: {}{}`",
                                        name,
                                        Syntax::SIGIL_MOVE,
                                        info.ty.name()
                                    ),
                                    Some(*span),
                                ));
                            } else {
                                self.mark_moved(name.clone(), *span);
                            }
                        }
                    }
                }
            }
    
            let ret_ty = if let Some(er) = exp_ret {
                if let Some(br) = &body_ret {
                    if br != er.as_ref() {
                        self.diags.push(Diagnostic::error(
                            "E0113",
                            format!("this lambda should return {}, not {}", er.show(), br.show()),
                            "the lambda's return type must match what's expected here".to_string(),
                            type_fix_hint(er, br),
                            Some(lam.span),
                        ));
                    }
                }
                Some((**er).clone())
            } else {
                body_ret
            };
            if ret_ty
                .as_ref()
                .is_some_and(|ty| self.type_contains_view_boundary(ty))
            {
                self.diags.push(Diagnostic::error(
                    "E2305",
                    "a lambda cannot return a stored or borrowed view".to_string(),
                    "a lambda value has no stable public owner slot for the returned view, and captured owners may move with the closure"
                        .to_string(),
                    "use a named function or method whose returned-view provenance can be inferred and published"
                        .to_string(),
                    Some(lam.span),
                ));
            }
    
            Some(Type::Fn {
                params: param_types,
                ret: ret_ty.map(Box::new),
                // A lambda value is a concrete callback, not a demand for one — it
                // carries no effect bound (D-EFF2 bounds ride callback *parameter*
                // types, checked against this value at the call site).
                effect_bound: None,
            })
        }
    
}
