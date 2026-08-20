use crate::AST::{AccessConvention, Expr, Lambda, LambdaBody, Stmt, Type};
use crate::Diagnostics::{Diagnostic, TextEdit};
use crate::Sema::Captures::{lambda_body_refs_name, lambda_collect_captures};
use crate::Sema::CheckerInfer::is_reactive_handle_ty;
use crate::Sema::Diagnostics::{is_cloneable, type_fix_hint};
use crate::Sema::{
    Checker, LocalInfo, SendCrossing, SendProblemKind, SendabilityProblem, ViewAccess,
};
use crate::Syntax;
use std::collections::HashSet;
    impl<'a> Checker<'a> {
        pub(crate) fn check_lambda(
            &mut self,
            lam: &mut Lambda,
            expected: Option<&Type>,
        ) -> Option<Type> {
            let collecting_loop = lam.meta.collecting_loop;
            let result_loop = lam.meta.result_loop;
            let inline_loop = collecting_loop || result_loop;
            let (exp_params, exp_ret, exp_contract, exp_metadata) = match expected {
                Some(Type::Fn {
                    params,
                    ret,
                    param_contract,
                    call_metadata,
                    ..
                }) => (
                    Some(params.as_slice()),
                    ret.as_ref(),
                    param_contract.as_ref(),
                    call_metadata.as_ref(),
                ),
                _ => (None, None, None, None),
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
            lam.meta.materialized_captures.clear();
            lam.meta.frozen_captures.clear();
    
            let param_names: HashSet<String> = lam.params.iter().map(|p| p.name.clone()).collect();
            let take_set: HashSet<String> = lam.take_names.iter().map(|(n, _)| n.clone()).collect();
    
            let mut read_caps = HashSet::new();
            let mut mut_caps = HashSet::new();
            if !inline_loop {
                lambda_collect_captures(
                    &lam.body,
                    &param_names,
                    &mut read_caps,
                    &mut mut_caps,
                );
            }
            // D-CONC-FREEZE1=A: explicit `task ^name` captures are captures
            // even when the body never reads the name directly. The move is
            // checked by the same crossing loop below.
            read_caps.extend(lam.take_names.iter().map(|(name, _)| name.clone()));
    
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
            // D-CONC-SHARE1=A: a write through a `Shared<T>` handle is one
            // locked edit of the CELL, not a change to the handle binding.
            // `check_stmt` rewrites `handle.field += v` into
            // `handle.edit(payload => { payload.field += v })`
            // (`Sema/SharedAccess.rs`), a body that only READS the handle — the
            // very shape the retired `handle.edit(…)` closure spelling had, and
            // the reason `shared_capture` below lets a handle cross a task with
            // no `^`. Capture collection above runs before that rewrite, so it
            // still sees the projection write and reports the handle as
            // mutated. Forget that stale fact for an immutable handle, so the
            // refusal below, `needs_fn_mut`, and `mut_captures` all describe
            // the body this lambda actually lowers. Nothing is lost: replacing
            // the handle itself (`handle = other`) is refused where it is
            // written, by the `Stmt::Assign` rail in
            // `CheckerCore/statements.rs`, with the same E0111 code. A `:=`
            // handle keeps its entry, so a genuine rebind still asks for
            // `FnMut`.
            mut_caps.retain(|name| {
                !self.lookup(name).is_some_and(|info| {
                    !info.mutable && matches!(&info.ty, Type::Shared(_))
                })
            });
    
            for name in &mut_caps {
                if param_names.contains(name) || take_set.contains(name) {
                    continue;
                }
                // D-TASKBORROW1=A: a write borrow (`&place`) is already a
                // changeable place. Inside a task group child it stays one; the
                // disjointness proof below is what keeps the writes safe.
                if self.in_taskgroup_spawn && self.is_write_borrow(name) {
                    continue;
                }
                if let Some(info) = self.lookup(name) {
                    if !info.mutable {
                        let mut diagnostic = Diagnostic::error(
                            "E0111",
                            format!("`{}` can't be changed inside this lambda", name),
                            "changing a value inside a short function requires a `:=` binding"
                                .to_string(),
                            format!(
                                "declare `{name} := …` instead of `{name} :: …`"
                            ),
                            Some(lam.span),
                        );
                        if let Some(sigil_span) = info.binding_sigil_span {
                            diagnostic = diagnostic.with_edit(TextEdit {
                                span: sigil_span,
                                new_text: Syntax::SIGIL_BIND_MUT.to_string(),
                            });
                        }
                        self.diags.push(diagnostic);
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
                        .map(|i| (i.ty.clone(), i.param_conv))
                        .or_else(|| self.consts.get(name).map(|t| (t.clone(), None)));
                    let Some((cap_ty, cap_conv)) = cap else {
                        continue;
                    };
                    if self.interrupt_callback_depth > 0 {
                        let problem = if matches!(&cap_ty, Type::Fn { .. }) {
                            let callback_safe = self
                                .lookup(name)
                                .map(|info| {
                                    info.param_conv.is_none() && info.interrupt_sendable
                                })
                                .unwrap_or_else(|| {
                                    self.funcs.contains_key(name)
                                        || self.unqualified.contains_key(name)
                                        || self.unqualified_file.contains_key(name)
                                });
                            if callback_safe {
                                None
                            } else {
                                self.crossing_problem(&cap_ty, SendCrossing::InterruptCallback, false).or(Some(
                                    SendabilityProblem {
                                        root: None,
                                        path: Vec::new(),
                                        kind: SendProblemKind::ClosureCaptures,
                                    },
                                ))
                            }
                        } else {
                            self.crossing_problem_for_name(
                                name,
                                &cap_ty,
                                SendCrossing::InterruptCallback,
                                true,
                            )
                        };
                        if let Some(problem) = problem {
                            self.report_unsendable(
                                name,
                                &cap_ty,
                                problem,
                                SendCrossing::InterruptCallback,
                                lam.span,
                            );
                            continue;
                        }
                    }
                    if self.is_task_spawn && self.type_contains_cell_guard(&cap_ty) {
                        let problem = self
                            .crossing_problem(&cap_ty, SendCrossing::TaskCapture, true)
                            .expect("Cell guards are thread-confined");
                        self.report_unsendable(
                            name,
                            &cap_ty,
                            problem,
                            SendCrossing::TaskCapture,
                            lam.span,
                        );
                        continue;
                    }
                    if self.type_contains_cell_guard(&cap_ty) {
                        self.report_cell_guard_storage(
                            format!("Cell guard `{name}` cannot be captured by a lambda"),
                            lam.span,
                        );
                        continue;
                    }
                    if matches!(&cap_ty, Type::Named(ty) if ty == Syntax::TYPE_TASKGROUP) {
                        self.diags.push(Diagnostic::error(
                            "E1110",
                            format!("`{name}` is a `Group` and cannot escape in a lambda"),
                            "a task group is a scoped spawn authority that may flow only through a direct function or method parameter"
                                .to_string(),
                            format!(
                                "move this work to a function or method that takes `{name}: Group` and call it directly"
                            ),
                            Some(lam.span),
                        ));
                        continue;
                    }
                    let taken = take_set.contains(name);
                    let frozen_site = self.frozen_for(name);
                    if self.is_task_spawn && frozen_site.is_some() {
                        if !lam.meta.frozen_captures.iter().any(|capture| capture == name) {
                            lam.meta.frozen_captures.push(name.clone());
                        }
                    }
                    let cloneable = is_cloneable(&cap_ty, self.registry);
                    // D-TASKBORROW1=A: a `task.group` child is joined by its group,
                    // so it may borrow places the owner still holds. Reads are free;
                    // writes need proven-disjoint places. Detached tasks, channels,
                    // and detached tasks keep the ownership-only rules below.
                    if self.in_taskgroup_spawn && !taken && frozen_site.is_none() {
                        let fallback = match cap_conv {
                            Some(AccessConvention::Write) => Some(ViewAccess::Write),
                            Some(AccessConvention::Read) => Some(ViewAccess::Read),
                            _ => None,
                        };
                        match self.admit_scoped_borrow(name, fallback, lam.span) {
                            Some(true) => {
                                if self.is_task_spawn {
                                    if let Some(binding) = self
                                        .current_binding_name
                                        .as_ref()
                                        .or(self.task_spawn_binding_name.as_ref())
                                    {
                                        self.view_borrow_escape_tasks.insert(binding.clone());
                                    }
                                }
                                continue;
                            }
                            Some(false) => continue,
                            None => {}
                        }
                    }
                    if !cap_ty.is_scalar()
                        && !cloneable
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
                            "this function can access the parameter, but it does not own the value; capture requires the move marker `^`"
                                .to_string(),
                            format!(
                                "make the parameter owned with the move marker `^`: `{}: {}{}`",
                                name,
                                Syntax::SIGIL_MOVE,
                                cap_ty.name()
                            ),
                            Some(lam.span),
                        ));
                        continue;
                    }
                    if self.is_view(name) {
                        let read_only = self
                            .view_facts(name)
                            .iter()
                            .all(|fact| fact.access == ViewAccess::Read);
                        let copy_ty = crate::Sema::Diagnostics::owned_type_for_read_view(&cap_ty)
                            .or_else(|| {
                                (self.is_string_view(name) && matches!(cap_ty, Type::String))
                                    .then_some(Type::String)
                            });
                        if !self.is_task_spawn
                            && !self.copies_explicit()
                            && read_only
                            && copy_ty
                                .as_ref()
                                .is_some_and(|ty| is_cloneable(ty, self.registry))
                        {
                            // The capture is an owning slot. Record the same
                            // materialization fact for AOT/JIT/interpreter;
                            // codegen must not turn a view into another view
                            // with `Clone`.
                            if !lam
                                .meta
                                .materialized_captures
                                .iter()
                                .any(|capture| capture == name)
                            {
                                lam.meta.materialized_captures.push(name.clone());
                            }
                            if !lam
                                .meta
                                .cloned_captures
                                .iter()
                                .any(|capture| capture == name)
                            {
                                lam.meta.cloned_captures.push(name.clone());
                            }
                            continue;
                        }
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
                    // D-CONC-FREEZE1=A: a bare mutable capture is rejected by
                    // the same E1101 rail as a body write. A consuming `^`
                    // capture or a frozen snapshot removes the outer alias.
                    //
                    // A `Condition` has no outer alias to remove. `JetCondition`
                    // is `Arc<JetConditionProtocol>` behind `#[derive(Clone)]`
                    // (`Prelude/CoreLib/JetStd/MathTaskMem.rs`), so a clone
                    // aliases the ONE waiter queue, and the protocol's own
                    // `Mutex` plus epoch counter order the crossing — the same
                    // reason `Shared<T>` crosses. A wait set exists precisely so
                    // `guard.wait` in one task and `notify_all` in another
                    // rendezvous, so refusing it AT the task boundary refuses
                    // its only purpose. It is not put on the reactive rail:
                    // `is_reactive_handle_ty` also carries the `#Local` pin and
                    // the reactive-upgrade note, neither of which says anything
                    // about a wait set. Genuine sendability is still checked
                    // below by `crossing_problem_for_name`.
                    let shared_capture = matches!(&cap_ty, Type::Shared(_))
                        || matches!(&cap_ty, Type::Named(ty_name) if ty_name == Syntax::TYPE_CONDITION)
                        || is_reactive_handle_ty(&cap_ty)
                        || self.type_contains_cell_guard(&cap_ty);
                    let mutable_capture = self.lookup(name).is_some_and(|info| info.mutable);
                    if self.is_task_spawn
                        && !taken
                        && frozen_site.is_none()
                        && !shared_capture
                        && (mut_caps.contains(name) || mutable_capture)
                    {
                        self.report_concurrent_write(name, "task capture", lam.span);
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
                        let moves_capture = taken || !cloneable;
                        let problem = self.crossing_problem_for_name(
                            name,
                            &cap_ty,
                            SendCrossing::TaskCapture,
                            moves_capture,
                        );
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
                    if self.is_resource_type(&cap_ty) || !cloneable {
                        // D-ARROW-CONTROL1: escaping closures infer ownership.
                        // Owned non-Copy captures move at closure creation.
                        if !taken {
                            lam.meta.moved_captures.push(name.clone());
                        }
                    } else if is_reactive_handle_ty(&cap_ty) || matches!(cap_ty, Type::Shared(_)) {
                        // D-REACT1=B / D-DATARACE1=C: a reactive handle is an Arc-backed
                        // shared cell — capturing a "copy" shares the same reactive cell.
                        // The capture is recorded as a clone so codegen moves an Arc
                        // clone into the closure. Lock-ordered storage makes the clone
                        // Send without leaning on rustc.
                        // D-MEM1 S6 (D-SHARED-API1=A): `Shared<T>` is the same shape — an
                                // Arc-backed "copyable door" meant to be captured freely across
                                // `task` closures with no `take`; suppress the same lint.
                        if is_reactive_handle_ty(&cap_ty) {
                            if let Some(info) = self.lookup(name) {
                                if !info.reactive_local {
                                    let crossing = if info.reactive_shared {
                                        "#Shared pin + task"
                                    } else {
                                        "task"
                                    };
                                    self.note_reactive_upgrade(name, &cap_ty, crossing);
                                }
                            } else {
                                self.note_reactive_upgrade(name, &cap_ty, "task");
                            }
                        }
                        lam.meta.cloned_captures.push(name.clone());
                    } else if !taken {
                        lam.meta.cloned_captures.push(name.clone());
                    }
                }
            }
    
            self.push_scope();
            let saved_param_names = std::mem::replace(
                &mut self.current_param_names,
                lam.params.iter().map(|param| param.name.clone()).collect(),
            );
            let saved_return_view_provenance = self.return_view_provenance.take();
            for (p, pty) in lam.params.iter().zip(param_types.iter()) {
                self.declare_in_scope(
                    &p.name,
                    LocalInfo {
                        def_span: p.name_span,
                        binding_sigil_span: None,
                        ty: pty.clone(),
                        // D-MEM1 S6: `Shared<T>.edit(f)`'s closure param is the one
                        // builtin-closure shape that binds its param mutable with no
                        // `&` sigil — the exclusive write lock IS the API contract.
                        mutable: self.lambda_param_mutable,
                        param_conv: Some(AccessConvention::Read),
                        decl_loop_depth: self.loop_depth,
                        interrupt_sendable: false,
                        reactive_local: false,
                        reactive_shared: false,
                        single_use_span: None,
                        constant_value: None,
                        invalid: false,
                    },
                );
            }
            let lending_params = if self.lambda_params_are_lending_views {
                lam.params
                    .iter()
                    .zip(param_types.iter())
                    .filter(|(_, ty)| {
                        matches!(
                            ty,
                            Type::Apply { name, args }
                                if name == "ViewMut" && args.len() == 1
                        )
                    })
                    .filter_map(|(param, _)| {
                        self.lending_view_loop_vars
                            .insert(param.name.clone())
                            .then(|| param.name.clone())
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
    
            // D-TXN2: a lambda body is a deferred execution context (it runs later —
            // e.g. an `on_commit` hook fires only post-commit). Effects inside it are
            // NOT rejected by the enclosing `#Transact` block, so zero the depth here
            // and restore it after the body. This is exactly why `name.on_commit(() =>
            // { fs.write(…) })` is the D-TXN2 fix-it: the irreversible work moves into
            // a lambda, off the block's direct path.
            let saved_txn_depth = self.txn_depth;
            let saved_txn_wall_depth = self.txn_wall_depth;
            if !inline_loop {
                self.txn_depth = 0;
                self.txn_wall_depth = 0;
            }
            let saved_expected = self.expected_type.clone();
            self.expected_type = exp_ret.map(|ret| (**ret).clone());
            // A block-bodied lambda's `return` belongs to the lambda, not to the
            // enclosing named function. Give statement checking the expected
            // callback result while the lambda body is active.
            let saved_ret = self.ret.clone();
            if let Some(ret) = exp_ret {
                self.ret = Some((**ret).clone());
            }
            let saved_in_lambda_body = self.in_lambda_body;
            let saved_inferred_mut_captures =
                std::mem::take(&mut self.inferred_lambda_mut_captures);
            // D-CONC-SPAWN1: the propagation fact is per lambda body — a `?`
            // inside a nested closure early-returns that closure, never this
            // one. Scope the working flag and harvest it below.
            let saved_task_body_propagates =
                std::mem::replace(&mut self.task_body_propagates, false);
            self.in_lambda_body = true;
            if matches!(
                exp_ret.map(|ret| ret.as_ref()),
                Some(Type::Apply { name, .. }) if name == "View"
            ) {
                if let LambdaBody::Expr(expr) = &mut lam.body {
                    if !matches!(expr.as_ref(), Expr::Copy(..) | Expr::Place(..))
                        && self.place_from_expr(expr).is_some()
                    {
                        let span = expr.span();
                        let inner = std::mem::replace(expr, Box::new(Expr::Absent(span)));
                        *expr = Box::new(Expr::Place(
                            inner,
                            crate::AST::PlaceAccess::Read,
                            span,
                        ));
                    }
                }
            }
            if inline_loop {
                if let Some((label, span)) = lam.meta.loop_label.clone() {
                    install_inline_loop_label(&mut lam.body, &label, span);
                }
            }
            if collecting_loop {
                self.collect_item_types.push(None);
                self.pending_loop_value = Some((
                    crate::Sema::LoopValueKind::Collecting,
                    lam.meta.loop_label.as_ref().map(|(name, _)| name.clone()),
                ));
            } else if result_loop {
                self.pending_loop_value = Some((
                    crate::Sema::LoopValueKind::Result,
                    lam.meta.loop_label.as_ref().map(|(name, _)| name.clone()),
                ));
            }
            let mut body_ret = match &mut lam.body {
                LambdaBody::Expr(e) => {
                    if self.is_task_spawn {
                        self.borrow_ctx = true;
                    }
                    // S46 one-line bodies: `() => transfer(...)` is the brace-free
                    // form of `() => { transfer(...) }`. When no value is expected
                    // (() / () ? E callback, or inferred spawn body), treat the
                    // call as a statement so void functions do not trip E0116.
                    let needs_value = match exp_ret.map(|r| r.as_ref()) {
                        Some(Type::Named(name)) if name == "Unit" => false,
                        Some(Type::Result { ok, .. })
                            if matches!(ok.as_ref(), Type::Named(name) if name == "Unit") =>
                        {
                            false
                        }
                        None => false,
                        Some(_) => true,
                    };
                    if needs_value {
                        self.infer(e)
                    } else {
                        self.infer_fallible_stmt(e)
                    }
                }
                LambdaBody::Block(stmts) => {
                    // The body may never run (a spawned task, a callback), so a
                    // `return` inside it is a conditional return of the enclosing
                    // function, not an unconditional one (card #2006).
                    self.check_conditional_block(stmts, false);
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
            if collecting_loop {
                let item_ty = self.collect_item_types.pop().flatten();
                let item_ty = item_ty.unwrap_or_else(|| {
                    self.diags.push(Diagnostic::error(
                        "E0073",
                        "this collecting loop path produces no item".to_string(),
                        "every accepted iteration must contribute one non-unit value unless `next` omits it".to_string(),
                        "return a value on this path, or use `next` to omit the item; remove `->` if the loop only performs effects".to_string(),
                        Some(lam.span),
                    ));
                    Type::Int
                });
                if let LambdaBody::Block(stmts) = &mut lam.body {
                    lower_collecting_loop(stmts, &item_ty, lam.span);
                }
                lam.meta.collect_item_type = Some(item_ty.clone());
                // The compiler invokes this closure immediately. It does not
                // create an escape or ownership boundary, so reads stay in
                // the current lexical environment.
                lam.meta.cloned_captures.clear();
                lam.meta.moved_captures.clear();
                body_ret = Some(Type::List(Box::new(item_ty)));
            } else if result_loop {
                let result_ty = self.last_loop_result_type.take().unwrap_or_else(|| {
                    self.diags.push(Diagnostic::error(
                        "E0073",
                        "this loop has no final break value".to_string(),
                        "a loop used as a value must return one value through `break value`"
                            .to_string(),
                        "add `break value`, or use the loop only for effects".to_string(),
                        Some(lam.span),
                    ));
                    Type::Int
                });
                lam.meta.loop_result_type = Some(result_ty.clone());
                if lam.meta.requires_exhaustion_route && !lam.meta.exhaustion_route_attached {
                    self.diags.push(Diagnostic::error(
                        "E0078",
                        "this finite value loop needs a written exhaustion route".to_string(),
                        "the source can end without a matching break, so the expression must state what exhaustion means".to_string(),
                        "add `?? fallback` after the closing `}`, or use the labeled loop form for `next` or `break`".to_string(),
                        lam.meta.exhaustion_span.or(Some(lam.span)),
                    ));
                }
                body_ret = Some(result_ty);
            }

            let inferred_mut_caps = std::mem::replace(
                &mut self.inferred_lambda_mut_captures,
                saved_inferred_mut_captures,
            )
            .into_iter()
            .filter(|name| read_caps.contains(name) || mut_caps.contains(name))
            .collect::<HashSet<_>>();
            // The builtin table is the authority for mutating receivers. Fold
            // its inferred roots into the same metadata explicit assignments
            // use, so `xs.push(x)` is FnMut just like `xs += [x]`.
            mut_caps.extend(inferred_mut_caps);
            lam.meta.needs_fn_mut = !mut_caps.is_empty();
            lam.meta.mut_captures = mut_caps
                .iter()
                .filter(|name| !take_set.contains(*name) && !param_names.contains(*name))
                .cloned()
                .collect();
            // D-CONC-SPAWN1: record this body's own propagation fact, then
            // restore the enclosing body's.
            lam.meta.fallible_propagation = self.task_body_propagates;
            self.task_body_propagates = saved_task_body_propagates;
            self.in_lambda_body = saved_in_lambda_body;
            self.ret = saved_ret;
            self.expected_type = saved_expected;
            self.txn_depth = saved_txn_depth;
            self.txn_wall_depth = saved_txn_wall_depth;
            for name in lending_params {
                self.lending_view_loop_vars.remove(&name);
            }
    
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
                                        "`{}` was not moved here, so the lambda cannot take it with the move marker `^`",
                                        name
                                    ),
                                    "this function has read access only and does not own the value; the move marker `^` is required"
                                        .to_string(),
                                    format!(
                                        "take ownership in this function with the move marker `^`: `{}: {}{}`",
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
                if let LambdaBody::Expr(expr) = &lam.body {
                    self.check_aggregate_view_return(expr);
                }
            }
            let lambda_return_view_provenance = self.return_view_provenance.take();
            lam.meta.return_view_provenance = lambda_return_view_provenance.clone();
            self.return_view_provenance = saved_return_view_provenance;
            self.current_param_names = saved_param_names;
    
            Some(Type::Fn {
                params: param_types,
                ret: ret_ty.map(Box::new),
                // A lambda value is a concrete callback, not a demand for one.
                // A body sema proves effect-free publishes the empty bound so it
                // satisfies `=[]=>` positions; anything else stays unbounded and
                // the call-site D-EFF2 obligation solver decides.
                effect_bound: crate::Sema::foreign_thread_safe_lambda(lam).then(Vec::new),
                param_contract: exp_contract.cloned(),
                call_metadata: exp_metadata.cloned(),
                return_view_provenance: lambda_return_view_provenance,
            })
        }
    
}

fn install_inline_loop_label(
    body: &mut LambdaBody,
    label: &str,
    span: crate::Diagnostics::Span,
) {
    let LambdaBody::Block(stmts) = body else {
        return;
    };
    let Some(root) = stmts.first_mut() else {
        return;
    };
    let old = match root {
        Stmt::Loop {
            label: root_label, ..
        }
        | Stmt::While {
            label: root_label, ..
        }
        | Stmt::For {
            label: root_label, ..
        }
        | Stmt::CountedLoop {
            label: root_label, ..
        } => root_label
            .replace((label.to_string(), span))
            .map(|(name, _)| name),
        _ => None,
    };
    if let Some(old) = old {
        rewrite_inline_loop_target(stmts, &old, label);
    }
}

fn rewrite_inline_loop_target(stmts: &mut [Stmt], old: &str, new: &str) {
    for stmt in stmts {
        match stmt {
            Stmt::BreakLabel(name, _)
            | Stmt::ContinueLabel(name, _)
            | Stmt::BreakLabelValue(name, _, _, _)
                if name == old =>
            {
                *name = new.to_string();
            }
            Stmt::Switch {
                arms, else_body, ..
            }
            | Stmt::ComptimeSwitch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    rewrite_inline_loop_target(&mut arm.body, old, new);
                }
                if let Some(body) = else_body {
                    rewrite_inline_loop_target(body, old, new);
                }
            }
            Stmt::Loop { body, .. }
            | Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::CountedLoop { body, .. }
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
            | Stmt::ComptimeBlock { body, .. }
            | Stmt::ContextBlock { body, .. }
            | Stmt::Live { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::ScopeMember { body, .. } => rewrite_inline_loop_target(body, old, new),
            Stmt::ComptimeIf {
                then_body,
                else_body,
                ..
            } => {
                rewrite_inline_loop_target(then_body, old, new);
                if let Some(body) = else_body {
                    rewrite_inline_loop_target(body, old, new);
                }
            }
            _ => {}
        }
    }
}

fn lower_collecting_loop(stmts: &mut Vec<Stmt>, item_ty: &Type, span: crate::Diagnostics::Span) {
    let target = jet_foundation::Names::mangle(&format!("collect_{}", span.start));
    rewrite_collect_yields(stmts, &target);
    stmts.insert(
        0,
        Stmt::Val(crate::AST::Binding {
            mutable: true,
            markers: Vec::new(),
            reactive_upgrade: false,
            meta: None,
            name: target.clone(),
            name_span: span,
            sigil_span: None,
            pattern: None,
            ty: Some(Type::List(Box::new(item_ty.clone()))),
            ty_span: Some(span),
            init: crate::AST::Expr::ListLit(Vec::new(), span),
            is_comptime: false,
            ct: None,
            uninit: false,
            arena_view: false,
            string_view: false,
            gc_promotion: None,
            gc_transferred: false,
        }),
    );
    stmts.push(Stmt::Expr(crate::AST::Expr::Ident(target, span)));
}

fn rewrite_collect_yields(stmts: &mut [Stmt], target: &str) {
    for stmt in stmts {
        match stmt {
            Stmt::Yield(value, span) if span.start == span.end => {
                let value = std::mem::replace(
                    value,
                    crate::AST::Expr::Int(0, *span, None, None),
                );
                *stmt = Stmt::Expr(crate::AST::Expr::MethodCall {
                    receiver: Box::new(crate::AST::Expr::Ident(target.to_string(), *span)),
                    method: "push".to_string(),
                    method_span: *span,
                    owner_type_args: Vec::new(),
                    type_args: Vec::new(),
                    args: vec![crate::AST::CallArg {
                        convention: AccessConvention::Read,
                        expr: value,
                        span: *span,
                        flags: crate::AST::CallArgFlags::default(),
                        label: None,
                        spread: false,
                    }],
                    recv_type: None,
                    resolved_ret: Some(Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string())),
                    checked_widen: false,
                });
            }
            Stmt::Loop { body, .. }
            | Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::CountedLoop { body, .. }
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
            | Stmt::ComptimeBlock { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::ScopeMember { body, .. } => rewrite_collect_yields(body, target),
            Stmt::Switch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    rewrite_collect_yields(&mut arm.body, target);
                }
                if let Some(body) = else_body {
                    rewrite_collect_yields(body, target);
                }
            }
            _ => {}
        }
    }
}
