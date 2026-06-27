//! Type inference: calls, lambdas, method calls, and call checking.
//!
//! Split out of the original `CheckerInfer.rs`; behavior unchanged.

use super::*;
use crate::AST::{
    AccessConvention, BinOp, Call, EnumLitArg, Expr, Lambda, LambdaBody, Stmt, Type,
};
use crate::Collections;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::{e0901, e0904};
use crate::Syntax;
use std::collections::{HashMap, HashSet};

impl<'a> Checker<'a> {
    pub(crate) fn infer_call_value(
        &mut self,
        callee: &mut Box<Expr>,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        let callee_ty = self.infer(callee)?;
        let Type::Fn { params, ret, .. } = callee_ty.clone() else {
            self.diags.push(Diagnostic::error(
                "E0803",
                format!("this is {}, not a function", callee_ty.show()),
                "only a function value can be called with `(…)`".to_string(),
                "call a defined `fn` by name, or store a lambda in a binding first".to_string(),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        };
        if args.len() != params.len() {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "this function wants {} argument{}, got {}",
                    params.len(),
                    if params.len() == 1 { "" } else { "s" },
                    args.len()
                ),
                "every argument must match a parameter".to_string(),
                "check how many values this function expects".to_string(),
                Some(span),
            ));
        }
        for (i, arg) in args.iter_mut().enumerate() {
            if let Some(param_ty) = params.get(i) {
                let saved = self.expected_type.clone();
                self.expected_type = Some(param_ty.clone());
                let got = self.infer(&mut arg.expr);
                self.expected_type = saved;
                if let Some(got) = got {
                    if got != *param_ty {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "argument {} should be {}, not {}",
                                i + 1,
                                param_ty.show(),
                                got.show()
                            ),
                            "every argument must match its parameter's type".to_string(),
                            type_fix_hint(param_ty, &got),
                            Some(arg.expr.span()),
                        ));
                    }
                }
            } else {
                self.infer(&mut arg.expr);
            }
        }
        ret.map(|r| *r)
    }

    pub(crate) fn check_lambda(&mut self, lam: &mut Lambda, expected: Option<&Type>) -> Option<Type> {
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
                    .map(|i| (i.ty.clone(), i.sendable))
                    .or_else(|| self.consts.get(name).map(|t| (t.clone(), true)));
                let Some((cap_ty, cap_sendable)) = cap else {
                    continue;
                };
                let taken = take_set.contains(name);
                if self.is_task_spawn {
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
                                "give the task its own copy (`{}.clone()`) or hand it over with `take({})`",
                                name, name
                            ),
                            Some(lam.span),
                        ));
                    }
                    continue; // taken by move into closure via mut borrow path
                }
                if mut_caps.contains(name) {
                    continue;
                }
                if !is_cloneable(&cap_ty, self.registry, self.structs) {
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
                } else if is_reactive_handle_ty(&cap_ty) {
                    // D-REACT1=B: a reactive `Signal`/`Derived` is an Rc-backed shared
                    // handle — capturing a "copy" shares the same reactive cell (that is
                    // the whole point: a derived/effect reads the live signal, and the
                    // outer code still `.set`s it). No silent-data-copy to warn about, so
                    // L0801 is suppressed. The capture is still recorded as a clone so
                    // codegen moves an Rc clone into the closure.
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
                            "use `take({}) (…) => …` to move `{}`, or `.clone()` at the call site to copy on purpose",
                            name, name
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
                    ty: pty.clone(),
                    mutable: false,
                    param_conv: None,
                    decl_loop_depth: self.loop_depth,
                    sendable: true,
                    task_lint_span: None,
                    single_use_span: None,
                    task_has_view_capture: false,
                },
            );
        }

        // D-TXN2: a lambda body is a deferred execution context (it runs later —
        // e.g. an `on_commit` hook fires only post-commit). Effects inside it are
        // NOT rejected by the enclosing `#Transact` block, so zero the depth here
        // and restore it after the body. This is exactly why `name.on_commit(() =>
        // { fs.write(…) })` is the D-TXN2 fix-it: the irreversible work moves into
        // a lambda, off the block's direct path.
        let saved_txn_depth = self.txn_depth;
        self.txn_depth = 0;
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
                                "this function has read access only and does not own the value".to_string(),
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

        Some(Type::Fn {
            params: param_types,
            ret: ret_ty.map(Box::new),
            // A lambda value is a concrete callback, not a demand for one — it
            // carries no effect bound (D-EFF2 bounds ride callback *parameter*
            // types, checked against this value at the call site).
            effect_bound: None,
        })
    }

    pub(crate) fn finish_builtin_method(
        &mut self,
        receiver: &Expr,
        method: &str,
        recv_ty: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
        ret: Option<Type>,
    ) -> Option<Type> {
        if Collections::builtin_needs_mut_receiver(recv_ty, method) {
            if let Some(root) = expr_root_ident(receiver) {
                let root = root.to_string();
                let rspan = receiver.span();
                if self.iter_borrowed.contains(&root) {
                    self.diags.push(collection_changed_in_loop(&root, rspan));
                }
                if let Some(info) = self.lookup(&root) {
                    if !info.mutable {
                        let (what, fix) = if root == Syntax::KW_SELF {
                            (
                                format!(
                                    "`.{}()` edits `{}`, but this method has read access only",
                                    method,
                                    Syntax::KW_SELF
                                ),
                                format!(
                                    "declare the enclosing method with `{}{}`",
                                    Syntax::SIGIL_MUTATE,
                                    Syntax::KW_SELF
                                ),
                            )
                        } else {
                            (
                                format!(
                                    "cannot write to `{}` — it does not have edit access (`~`); required before calling `.{}()`",
                                    root,
                                    method
                                ),
                                format!("declare `{} {} ...`", root, Syntax::SIGIL_BIND_MUT),
                            )
                        };
                        self.diags.push(Diagnostic::error(
                            "E0202",
                            what,
                            "this method edits the collection in place; write access (`~`) is required".to_string(),
                            fix,
                            Some(rspan),
                        ));
                    }
                }
            }
        }
        if let Type::Apply { name, .. } = recv_ty {
            match (name.as_str(), method) {
                ("Task", "join") => {
                    self.consume_builtin_receiver(receiver, method);
                    let _ = span;
                    return ret;
                }
                ("Task", "detach") => {
                    // D-DETACH1: consume the Task handle (marks it moved → L1101 won't fire).
                    // Two error cases:
                    //   E1106: task captured a `view` borrow — a detached task can outlive
                    //          the borrow; fix-it is to pass an owned `copy`/`share`.
                    //   E1103: task had a general sendability failure at spawn (E1102 already
                    //          fired); detaching an unsound task is doubly dangerous.
                    if let Expr::Ident(name, _) = receiver {
                        if self.view_borrow_escape_tasks.contains(name.as_str()) {
                            self.diags.push(Diagnostic::error(
                                "E1106",
                                format!(
                                    "can't detach task `{}` — it captured a `view` borrow that may not live long enough",
                                    name
                                ),
                                "a detached task runs unsupervised and may outlive the caller; a captured `view` would dangle".to_string(),
                                "pass an owned `copy` or `share` to the task instead of a `view`".to_string(),
                                Some(span),
                            ));
                        } else if self.view_capture_tasks.contains(name.as_str()) {
                            self.diags.push(Diagnostic::error(
                                "E1103",
                                format!(
                                    "can't detach task `{}` — it captured a value that can't cross a thread boundary",
                                    name
                                ),
                                "a detached task runs unsupervised; it must only hold values it owns cleanly".to_string(),
                                "fix the E1102 error at the spawn site first, then `.detach()` is safe".to_string(),
                                Some(span),
                            ));
                        }
                    }
                    self.consume_builtin_receiver(receiver, method);
                    let _ = span;
                    return None; // detach() returns nothing
                }
                ("Sender", "send") => {
                    return self.finish_sender_send(recv_ty, args, span);
                }
                _ => {}
            }
        }
        let mut refined_ret = ret.clone();
        if let Some(expected) = Collections::builtin_method_arg_types(recv_ty, method) {
            for (i, arg) in args.iter_mut().enumerate() {
                let saved_esc = self.lambda_escapes;
                if Collections::is_closure_method(method) {
                    self.lambda_escapes = false;
                }
                let saved_exp = self.expected_type.clone();
                if let Some(et) = expected.get(i) {
                    self.expected_type = Some(et.clone());
                }
                let got = self.infer(&mut arg.expr);
                self.expected_type = saved_exp;
                self.lambda_escapes = saved_esc;
                if let (Some(et), Some(gt)) = (expected.get(i), got) {
                    if Collections::is_closure_method(method) && i == 0 && method == "map" {
                        if let Type::Fn {
                            ret: Some(ref r), ..
                        } = gt
                        {
                            if let Type::List(inner) = recv_ty {
                                refined_ret = Some(Type::List(Box::new((**r).clone())));
                                let _ = inner;
                            }
                        }
                    }
                    // D-FAILCOMP1: filter_map(f: T->V?E) → [V]; refine from closure's ok type.
                    if Collections::is_closure_method(method) && i == 0 && method == "filter_map" {
                        if let Type::Fn { ret: Some(ref r), .. } = gt {
                            if let Type::Result { ok, .. } = r.as_ref() {
                                refined_ret = Some(Type::List(Box::new(*ok.clone())));
                            }
                        }
                    }
                    // D-AUTOPAR1=A: par_map → [V]; refine V from closure's return type.
                    if Collections::is_closure_method(method) && i == 0 && method == "par_map" {
                        if let Type::Fn { ret: Some(ref r), .. } = gt {
                            refined_ret = Some(Type::List(Box::new((**r).clone())));
                        }
                    }
                    // D-AUTOPAR1=A: par_fold → acc; refine from closure's return type.
                    if Collections::is_closure_method(method) && i == 1 && method == "par_fold" {
                        if let Type::Fn { ret: Some(ref r), .. } = gt {
                            refined_ret = Some((**r).clone());
                        }
                    }
                    if method == "reduce" && i == 1 {
                        if let Type::Fn {
                            ret: Some(ref r), ..
                        } = gt
                        {
                            refined_ret = Some((**r).clone());
                        }
                    }
                    // Skip E0108 for closure methods with ret: None (open return type).
                    let open_ret = matches!(et, Type::Fn { ret: None, .. })
                        && matches!(gt, Type::Fn { .. });
                    if !open_ret && !fn_types_compatible(et, &gt) && gt != *et {
                        self.diags.push(Diagnostic::error(
                            "E0108",
                            format!(
                                "argument {} to `.{}()` should be {}, not {}",
                                i + 1,
                                method,
                                et.show(),
                                gt.show()
                            ),
                            "built-in methods need arguments of the right type".to_string(),
                            type_fix_hint(et, &gt),
                            Some(arg.expr.span()),
                        ));
                    }
                }
            }
        } else {
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
        }
        let _ = span;
        refined_ret
    }

    /// D-DET-CAPAPI: the generic `Rng` draws — `rng.pick(list) -> T?` (uniform
    /// choice; null on empty) and `rng.shuffle(~list)` (in-place Fisher–Yates).
    /// Both advance the stream, so the `rng` receiver must have edit access (`~`);
    /// `shuffle` edits its list in place, so the list arg must be `~` too. Mirrors
    /// the ambient `random.pick`/`random.shuffle` (CheckerCoreLib).
    fn finish_rng_generic(
        &mut self,
        receiver: &Expr,
        method: &str,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        // The receiver must be a `~Rng` (every draw advances the stream).
        if let Some(root) = expr_root_ident(receiver) {
            let root = root.to_string();
            if let Some(info) = self.lookup(&root) {
                if !info.mutable {
                    self.diags.push(Diagnostic::error(
                        "E0202",
                        format!(
                            "cannot draw from `{}` — it does not have edit access (`~`); required before calling `.{}()`",
                            root, method
                        ),
                        "every `Rng` draw advances the stream, so the receiver needs write access (`~`)".to_string(),
                        format!("declare `{} {} ...` (or pass the rng as `~{}`)", root, Syntax::SIGIL_BIND_MUT, root),
                        Some(receiver.span()),
                    ));
                }
            }
        }
        if args.len() != 1 {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!("`.{}()` takes one list, got {}", method, args.len()),
                "this `Rng` draw operates on a single list".to_string(),
                format!("call `rng.{}(items)`", method),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return if method == "pick" { Some(Type::Option(Box::new(Type::Int))) } else { None };
        }
        // `shuffle` edits the list in place — the list arg needs `~`.
        if method == "shuffle" && args[0].convention != AccessConvention::Write {
            self.diags.push(Diagnostic::error(
                "E0202",
                "`shuffle` edits its list in place".to_string(),
                "write access (`~`) is required; the list must be passed with `~`".to_string(),
                "write `rng.shuffle(~items)`".to_string(),
                Some(args[0].span),
            ));
        }
        let ty = self.infer(&mut args[0].expr)?;
        let Type::List(inner) = ty else {
            self.diags.push(Diagnostic::error(
                "E0112",
                format!("`{}` needs a list, not {}", method, ty.show()),
                format!("rng.{} operates on a `[T]`", method),
                "pass a `[T]` value".to_string(),
                Some(args[0].expr.span()),
            ));
            return if method == "pick" { Some(Type::Option(Box::new(Type::Int))) } else { None };
        };
        // `pick` returns `T?`; `shuffle` returns nothing.
        if method == "pick" {
            Some(Type::Option(inner))
        } else {
            None
        }
    }


    pub(crate) fn infer_method_call(
        &mut self,
        receiver: &mut Box<Expr>,
        method: &str,
        span: Span,
        type_args: &[Type],
        args: &mut Vec<crate::AST::CallArg>,
        recv_type_out: &mut Option<String>,
        resolved_ret_out: &mut Option<Type>,
    ) -> Option<Type> {
        // D-PATHFS1 / E0340: `read_dir` is not a Jet API — teach the typed path path.
        if method == "read_dir" {
            self.infer(receiver); // still type-check the receiver
            for a in args.iter_mut() { self.infer(&mut a.expr); }
            self.diags.push(Diagnostic::error(
                "E0340",
                "`read_dir` is not a method in Jet".to_string(),
                "Jet uses typed paths; raw-string directory helpers are not exposed".to_string(),
                "write `Path.from(path).walk()` to list a directory recursively".to_string(),
                Some(span),
            ));
            return None;
        }
        if method == "clone" {
            self.borrow_ctx = true;
            return self.infer(receiver);
        }
        // D-DIST3 (ratified 2026-06-20): `.raw()` unwraps a distinct type.
        if method == crate::Syntax::METHOD_DISTINCT_RAW {
            self.borrow_ctx = true;
            let recv_ty = self.infer(receiver)?;
            if let Type::Named(ref n) = recv_ty {
                if let Some(base) = self.registry.distinct_base(n).cloned() {
                    if !args.is_empty() {
                        self.diags.push(Diagnostic::error(
                            "E0103",
                            format!("`.{}()` takes no arguments", crate::Syntax::METHOD_DISTINCT_RAW),
                            "`.raw()` simply unwraps the base value — no arguments needed".to_string(),
                            "write `.raw()` with no arguments".to_string(),
                            Some(span),
                        ));
                    }
                    return Some(base);
                }
            }
            self.diags.push(Diagnostic::error(
                "E0311",
                format!("`.{}()` is only valid on a distinct type value", crate::Syntax::METHOD_DISTINCT_RAW),
                "`.raw()` unwraps a distinct type to its base representation".to_string(),
                format!("only call `.raw()` on a value whose type was declared with `{} distinct`", crate::Syntax::SIGIL_BIND_IMMUT),
                Some(span),
            ));
            return None;
        }
        // D-TOOL4 (E2-M11): `expect(x).snapshot()` — the special snapshot
        // assertion. Recognized by checking the receiver type.
        if method == Syntax::BUILTIN_SNAPSHOT {
            let recv_ty = self.infer(receiver);
            if recv_ty.as_ref().map(|t| t == &Type::Named("__JetExpect__".to_string())).unwrap_or(false) {
                // Valid: snapshot assertion — void, no return type.
                return None;
            }
            // Not from expect() — error.
            self.diags.push(Diagnostic::error(
                "E2901",
                format!("`.{}()` is only valid on the result of `{}(…)`", Syntax::BUILTIN_SNAPSHOT, Syntax::BUILTIN_EXPECT),
                "snapshot testing: call `expect(value).snapshot()` in a test block".to_string(),
                format!("e.g. `{}(my_result).snapshot()`", Syntax::BUILTIN_EXPECT),
                Some(span),
            ));
            return None;
        }
        if let Expr::Ident(root, _) = &**receiver {
            if root == "File" && method == Syntax::FOREIGN_OPEN {
                self.diags.push(Diagnostic::error(
                    "E0038",
                    "`File.open` is not the M10 file API".to_string(),
                    "M10 uses whole-file helpers in `core.fs`; file handles are out of scope"
                        .to_string(),
                    "import `core.fs as fs` and call `fs.read(path)` or `fs.write(path, text)`"
                        .to_string(),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
        }
        // D-ENC1: nested-namespace access — `encoding.json.to_string(x)` where `encoding`
        // is a library alias (`use core.encoding`) and `json` a registered submodule. The
        // method-call receiver is `Field(Ident(alias), leaf)`; resolve to the submodule
        // `<ns>.<leaf>` as a core call. Guarded by `is_known_core_module`, so it fires only
        // for real submodules (e.g. `core.encoding.json`), never plain field access.
        if let Expr::Field(base, leaf, _) = &**receiver {
            if let Expr::Ident(alias, alias_span) = &**base {
                if let Some(ns) = self.core_imports.get(alias).cloned() {
                    let submodule = format!("{}.{}", ns, leaf);
                    if crate::Syntax::is_known_core_module(&submodule) {
                        let ret =
                            self.infer_core_call(&submodule, method, *alias_span, span, type_args, args);
                        if is_polymorphic_core_special(&submodule, method) {
                            *resolved_ret_out = ret.clone();
                        }
                        return ret;
                    }
                }
            }
        }
        if let Expr::Ident(alias, alias_span) = &**receiver {
            if let Some(module) = self.core_imports.get(alias).cloned() {
                let ret = self.infer_core_call(&module, method, *alias_span, span, type_args, args);
                // c109 Phase 20: write the resolved return type back onto the node
                // for the polymorphic core specials whose type is arg-dependent and
                // NOT in `core_fixed_sig` (so the TIR can read it totally — I3). The
                // monomorphic calls (in `core_fixed_sig`) get their type from that
                // table at lowering, so leave `resolved_ret = None` for them.
                if is_polymorphic_core_special(&module, method) {
                    *resolved_ret_out = ret.clone();
                }
                return ret;
            }
            if let Some(&mod_idx) = self.imports.get(alias) {
                return self.infer_import_call(mod_idx, method, *alias_span, span, args);
            }
            // D-MOD2: inline code module call — `math.double(x)` where `math` is an
            // inline `module math { … }` in this file. Resolve via mangled name.
            if self.code_modules.contains_key(alias.as_str()) {
                let mangled = format!("{}__{}", alias, method);
                return self.infer_code_module_call(alias, &mangled, *alias_span, span, args);
            }
        }
        if let Expr::Ident(type_name, _) = &**receiver {
            // D-ENC-DYN1=A+: `Data`/`Json`/`Toml`/`Yaml`/`Csv` name the one dynamic value;
            // they are reserved core type names (a user type may not redefine them).
            if is_json_type_name(type_name) {
                if let Some(ret) = self.check_core_json_lit(method, args, span) {
                    return Some(ret);
                }
            }
            {
                let has_variant = self.resolve_enum_variants_cloned(type_name)
                    .map(|v| v.contains_key(method))
                    .unwrap_or(false);
                if has_variant {
                    let saved: Vec<Expr> = args
                        .iter_mut()
                        .map(|a| std::mem::replace(&mut a.expr, Expr::Int(0, a.span, None)))
                        .collect();
                    let mut enum_args: Vec<EnumLitArg> =
                        saved.into_iter().map(EnumLitArg::Positional).collect();
                    let ty = self.check_enum_lit(type_name, method, &mut enum_args, span);
                    for (a, ea) in args.iter_mut().zip(enum_args) {
                        if let EnumLitArg::Positional(e) = ea {
                            a.expr = e;
                        }
                    }
                    return Some(ty);
                }
            }
            if self.registry.method(type_name, method).is_some() {
                return self.check_static_method(type_name, method, span, args);
            }
            if let Some(ty) = builtin_type_from_ident(type_name) {
                if let Some(ret) = Collections::builtin_method_return(&ty, method, args.len(), true)
                {
                    return self.finish_builtin_method(receiver, method, &ty, args, span, ret);
                }
            }
            // D-COLLBREADTH1=A: `Set.from([...])` → `Set<T>`.
            // T is inferred from the list argument's element type.
            if type_name == "Set" && method == "from" && args.len() == 1 {
                let arg_ty = self.infer(&mut args[0].expr);
                let elem_ty = match arg_ty {
                    Some(Type::List(inner)) => *inner,
                    _ => Type::Int,
                };
                if !Collections::is_hashable_type(&elem_ty) {
                    self.diags.push(Diagnostic::error(
                        "E0506",
                        format!("`Set<{}>` is not valid — `{}` is not hashable", elem_ty.name(), elem_ty.name()),
                        "Set elements must implement Hash and Eq; use Int, Bool, String, Char, or a named type".to_string(),
                        format!("change the element type to a hashable type, or use a `[{}]` list instead", elem_ty.name()),
                        Some(span),
                    ));
                }
                return Some(Type::Apply { name: "Set".to_string(), args: vec![elem_ty] });
            }
            // D-COLLBREADTH1=A: `Deque.new()` → `Deque<T>`.
            // T is inferred from the type annotation's expected type.
            if type_name == "Deque" && method == "new" && args.is_empty() {
                let elem_ty = match &self.expected_type {
                    Some(Type::Apply { name, args, .. }) if name == "Deque" && !args.is_empty() => {
                        args[0].clone()
                    }
                    _ => Type::Int,
                };
                return Some(Type::Apply { name: "Deque".to_string(), args: vec![elem_ty] });
            }
            // D-PATHFS1: `Path.from(str)` — typed path constructor.
            if type_name == "Path" && method == "from" && !self.registry.contains("Path") {
                if args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!("`Path.from` takes one string argument, got {}", args.len()),
                        "`Path.from` constructs a typed path from a string".to_string(),
                        "write `Path.from(some_string)`".to_string(),
                        Some(span),
                    ));
                }
                for a in args.iter_mut() { self.infer(&mut a.expr); }
                return Some(Type::Named("Path".to_string()));
            }
            // D-SIMD2 / D-LINALG1: a STATIC method on a built-in math type —
            // `F32x4.splat(x)` / `Vec3.from_array([…])`. The arg is elaborated
            // against the method's expected type (so a literal becomes the
            // component type / the `[T#N]` bridge).
            if is_math_type(type_name) && !self.registry.contains(type_name) {
                if let Some(ret) = math_static_return(type_name, method, args.len()) {
                    if let Some(want) = math_static_arg_ty(type_name, method) {
                        if let Some(arg) = args.first_mut() {
                            let old = self.expected_type.replace(want.clone());
                            let got = self.infer(&mut arg.expr);
                            self.expected_type = old;
                            if let Some(g) = got {
                                if g != want {
                                    self.diags.push(Diagnostic::error(
                                        "E0128",
                                        format!("`{}.{}()` expects a `{}`, got `{}`", type_name, method, want.name(), g.name()),
                                        format!("`{}.{}` builds a `{}` from a `{}`", type_name, method, type_name, want.name()),
                                        format!("pass a `{}` value", want.name()),
                                        Some(arg.expr.span()),
                                    ));
                                }
                            }
                        }
                    }
                    return Some(ret);
                }
            }
        }
        self.borrow_ctx = true;
        let recv_ty = self.infer(receiver)?;
        // E0964: length-changing methods are forbidden on a fixed-size [T#N].
        if let Type::FixedList { .. } = &recv_ty {
            if matches!(method, "push" | "pop" | "insert" | "remove" | "clear") {
                self.diags.push(Diagnostic::error(
                    "E0964",
                    format!(
                        "`{}` changes a list's length, but this is a fixed-size {}",
                        method,
                        recv_ty.show()
                    ),
                    "the length of `[T#N]` is fixed at compile time and cannot change".to_string(),
                    "bind a growable list with `:=` (e.g. `r := [...]`) if you need to change its length".to_string(),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
        }
        // D-TXN3/D-TXN4: `<handle>.on_commit(() => { … })` on a `#Transact`
        // transaction handle. Same shape as `scope.guard`: a zero-parameter
        // lambda, Drop-backed, run LIFO on a clean commit and dropped on a
        // `?`-failure/rollback. Returns a guard handle (`TransactionGuard`),
        // bound or discarded like a `scope.guard`. Sets `recv_type` so codegen
        // routes the node to the commit-guard lowering (I3).
        if let Type::Named(handle_ty) = &recv_ty {
            if handle_ty == crate::Syntax::TXN_HANDLE_TYPE && method == crate::Syntax::TXN_ON_COMMIT {
                if args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!(
                            "`{}` takes one lambda, got {}",
                            crate::Syntax::TXN_ON_COMMIT,
                            args.len()
                        ),
                        "a post-commit hook registers a single cleanup lambda".to_string(),
                        format!("write `{}.{}(() => {{ … }})`", "<handle>", crate::Syntax::TXN_ON_COMMIT),
                        Some(span),
                    ));
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    *recv_type_out = Some(handle_ty.clone());
                    return Some(Type::Named("TransactionGuard".to_string()));
                }
                let lam_ty = self.infer(&mut args[0].expr);
                match &lam_ty {
                    Some(Type::Fn { params, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(Diagnostic::error(
                                "E0104",
                                format!(
                                    "`{}` needs a zero-parameter lambda, got {} parameter{}",
                                    crate::Syntax::TXN_ON_COMMIT,
                                    params.len(),
                                    if params.len() == 1 { "" } else { "s" }
                                ),
                                "the hook body takes no arguments — it captures what it needs via closure".to_string(),
                                format!("write `<handle>.{}(() => {{ … }})` with no parameters", crate::Syntax::TXN_ON_COMMIT),
                                Some(args[0].expr.span()),
                            ));
                        }
                    }
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{}` needs a lambda, not {}", crate::Syntax::TXN_ON_COMMIT, other.show()),
                            "a post-commit hook runs a lambda only after the transaction commits".to_string(),
                            format!("write `<handle>.{}(() => {{ … }})`", crate::Syntax::TXN_ON_COMMIT),
                            Some(args[0].expr.span()),
                        ));
                    }
                    None => {}
                }
                *recv_type_out = Some(handle_ty.clone());
                return Some(Type::Named("TransactionGuard".to_string()));
            }
        }
        // D-TXN-ROLLBACK (layer 3): `<handle>.on_rollback(() => { … })` on a
        // `#Transact` handle — the exact mirror of `on_commit`. A zero-parameter
        // lambda, Drop-backed, run LIFO on a `?`-failure/rollback and dropped on a
        // clean commit. Returns the same `TransactionGuard` handle.
        if let Type::Named(handle_ty) = &recv_ty {
            if handle_ty == crate::Syntax::TXN_HANDLE_TYPE && method == crate::Syntax::TXN_ON_ROLLBACK {
                if args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!(
                            "`{}` takes one lambda, got {}",
                            crate::Syntax::TXN_ON_ROLLBACK,
                            args.len()
                        ),
                        "a rollback hook registers a single undo lambda".to_string(),
                        format!("write `{}.{}(() => {{ … }})`", "<handle>", crate::Syntax::TXN_ON_ROLLBACK),
                        Some(span),
                    ));
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    *recv_type_out = Some(handle_ty.clone());
                    return Some(Type::Named("TransactionGuard".to_string()));
                }
                let lam_ty = self.infer(&mut args[0].expr);
                match &lam_ty {
                    Some(Type::Fn { params, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(Diagnostic::error(
                                "E0104",
                                format!(
                                    "`{}` needs a zero-parameter lambda, got {} parameter{}",
                                    crate::Syntax::TXN_ON_ROLLBACK,
                                    params.len(),
                                    if params.len() == 1 { "" } else { "s" }
                                ),
                                "the hook body takes no arguments — it captures what it needs via closure".to_string(),
                                format!("write `<handle>.{}(() => {{ … }})` with no parameters", crate::Syntax::TXN_ON_ROLLBACK),
                                Some(args[0].expr.span()),
                            ));
                        }
                    }
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{}` needs a lambda, not {}", crate::Syntax::TXN_ON_ROLLBACK, other.show()),
                            "a rollback hook runs a lambda only when the transaction rolls back".to_string(),
                            format!("write `<handle>.{}(() => {{ … }})`", crate::Syntax::TXN_ON_ROLLBACK),
                            Some(args[0].expr.span()),
                        ));
                    }
                    None => {}
                }
                *recv_type_out = Some(handle_ty.clone());
                return Some(Type::Named("TransactionGuard".to_string()));
            }
        }
        // E2-M7: method calls on streaming file handles (D-IO2).
        if let Type::Named(handle_ty) = &recv_ty {
            if let Some(ret) = file_handle_method_return(handle_ty, method, args.len(), span, &mut self.diags) {
                for a in args.iter_mut() { self.infer(&mut a.expr); }
                *recv_type_out = Some(handle_ty.clone());
                return ret;
            }
        }
        // E2-M10: method calls on net/http opaque types.
        if let Type::Named(handle_ty) = &recv_ty {
            if let Some(ret) = net_method_return(handle_ty, method, args.len(), span, &mut self.diags) {
                for a in args.iter_mut() { self.infer(&mut a.expr); }
                *recv_type_out = Some(handle_ty.clone());
                return ret;
            }
        }
        // D-PATHFS1: method calls on `Path` typed handle.
        if let Type::Named(handle_ty) = &recv_ty {
            if handle_ty == "Path" {
                if let Some(ret) = path_method_return(handle_ty, method, args.len(), span, &mut self.diags) {
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    *recv_type_out = Some("Path".to_string());
                    return ret;
                }
            }
        }
        // D-PENDING1=B: method calls on `Loadable<T,E>` handle.
        if let Some(ret) = loadable_method_return(&recv_ty, method, args.len()) {
            for a in args.iter_mut() { self.infer(&mut a.expr); }
            *recv_type_out = Some("Loadable".to_string());
            return ret;
        }
        // D-APPROX1=A: method calls on sketch data structures.
        if let Some(ret) = sketch_method_return(&recv_ty, method, args) {
            for a in args.iter_mut() { self.infer(&mut a.expr); }
            *recv_type_out = Some(sketch_type_name(&recv_ty).unwrap_or("Sketch").to_string());
            return ret;
        }
        // D-ALLOC1/D-ALLOC-C/D-ALLOC-D (ratified 2026-06-19): method calls on
        // Arena/Bump/Pool/Fixed allocators. E3104: use-after-free/reset.
        if let Type::Named(handle_ty) = &recv_ty {
            let handle_ty_s = handle_ty.clone();
            if let Some(ret) = alloc_method_return(&handle_ty_s, method, args, span, &mut self.diags) {
                // E3104: check for use-after-free/reset before inferring args.
                let recv_name = if let Expr::Ident(n, _) = &**receiver { Some(n.clone()) } else { None };
                // D-ALLOC-D: E3104 — `alloc` after `free` is always wrong (the allocator
                // is consumed). After `reset`, further `alloc` is valid (buffer is reused).
                if method == "alloc" {
                    if let Some(ref name) = recv_name {
                        if self.freed_allocators.contains_key(name.as_str()) {
                            self.diags.push(e3104(name, "free", span));
                        }
                    }
                }
                // Mark the allocator as freed only on `free`. `reset` keeps it alive.
                if method == "free" {
                    if let Some(ref name) = recv_name {
                        self.freed_allocators.insert(name.clone(), "free".to_string());
                    }
                }
                // D-ALLOC2: `reset`/`free` invalidate every value previously
                // allocated in this arena. Any view of it used afterward is
                // E0632 (use-after-reset/free) — the runtime `&mut self`/`self`
                // signatures would also reject, so Jet rejects first (I2).
                if method == "reset" || method == "free" {
                    if let Some(ref name) = recv_name {
                        self.kill_views_of_arena(name, method, span);
                    }
                }
                *recv_type_out = Some(handle_ty_s.clone());
                // For `alloc`, infer the argument and return its type.
                if method == "alloc" {
                    if let Some(arg) = args.get_mut(0) {
                        let inferred = self.infer(&mut arg.expr);
                        return inferred;
                    }
                    return None;
                }
                for a in args.iter_mut() { self.infer(&mut a.expr); }
                return ret;
            }
        }
        // D-ARGS1: method calls on ArgsSpec / ParsedArgs (builder and result types).
        if let Type::Named(handle_ty) = &recv_ty {
            if handle_ty == "ArgsSpec" {
                if let Some(ret) = args_spec_method_return(method, args.len(), span, &mut self.diags) {
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    *recv_type_out = Some("ArgsSpec".to_string());
                    return ret;
                }
            }
            if handle_ty == "ParsedArgs" {
                if let Some(ret) = parsed_args_method_return(method, args.len(), span, &mut self.diags) {
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    *recv_type_out = Some("ParsedArgs".to_string());
                    return ret;
                }
            }
        }
        // D-DET1: methods on the deterministic injected Clock/Rng capability (and
        // Stopwatch). Reading time/randomness THROUGH the handle is reproducible.
        // Set `recv_type_out` so codegen routes the call to the handle-method op
        // (TIR shape (h)) rather than failing the typed-IR subset check.
        if let Type::Named(handle_ty) = &recv_ty {
            // D-DET-CAPAPI: `rng.pick(list)` / `rng.shuffle(~list)` are GENERIC — the
            // element type comes from the `[T]` arg, mirroring the ambient
            // `random.pick`/`random.shuffle`. Resolve element-aware here (the
            // `builtin_method_return` table only carries Int placeholders).
            if handle_ty == crate::Syntax::RNG_TYPE && matches!(method, "pick" | "shuffle") {
                let handle_ty = handle_ty.clone();
                let result = self.finish_rng_generic(receiver, method, args, span);
                *recv_type_out = Some(handle_ty);
                return result;
            }
            if matches!(handle_ty.as_str(), "Clock" | "Rng" | "Stopwatch" | "Duration") {
                if let Some(ret) = Collections::builtin_method_return(&recv_ty, method, args.len(), false) {
                    let handle_ty = handle_ty.clone();
                    let result = self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                    *recv_type_out = Some(handle_ty);
                    return result;
                }
            }
        }
        // D-REACT1=B: methods on the reactive handle types `Signal<T>`/`Derived<T>`
        // (`.get()`, `.set(v)`). Set `recv_type_out` to the handle name so codegen
        // routes the call to the reactive-method shape (keyed on recv_type, not the
        // bare method name — `get`/0 would otherwise alias a list `get`).
        if let Type::Apply { name, .. } = &recv_ty {
            if matches!(name.as_str(), "Signal" | "Derived") {
                if let Some(ret) =
                    Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                {
                    let handle_ty = name.clone();
                    let result =
                        self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                    *recv_type_out = Some(handle_ty);
                    return result;
                }
            }
        }
        // D-HONESTNUM1=A: methods on `Measurement<Float>` (value ± uncertainty).
        // `.add/sub/mul/div(m)` → Measurement<Float>; `.value()/.uncertainty()` → Float.
        // Operator overloading is NOT extended here — I8 closed-family rule.
        if let Type::Apply { name, .. } = &recv_ty {
            if name == crate::Syntax::TYPE_MEASUREMENT {
                let meas_ty = recv_ty.clone();
                let ret = match (method, args.len()) {
                    ("add" | "sub" | "mul" | "div", 1) => {
                        let arg_ty = self.infer(&mut args[0].expr);
                        if let Some(got) = &arg_ty {
                            if got != &meas_ty {
                                self.diags.push(Diagnostic::error(
                                    "E0128",
                                    format!(
                                        "`.{}()` expects a `Measurement<Float>`, got `{}`",
                                        method, got.name()
                                    ),
                                    "Measurement arithmetic requires both operands to have the same type".to_string(),
                                    "wrap the value with `M.from(value, uncertainty)` first".to_string(),
                                    Some(args[0].expr.span()),
                                ));
                            }
                        }
                        Some(meas_ty.clone())
                    }
                    ("value" | "uncertainty", 0) => Some(Type::Float),
                    _ => None,
                };
                if let Some(r) = ret {
                    *recv_type_out = Some(crate::Syntax::TYPE_MEASUREMENT.to_string());
                    return Some(r);
                }
            }
        }
        // D-SIMD2 / D-LINALG1: methods on the built-in math value types
        // (`v.dot(w)`, `v.length()`, `v.sum()`, `v.reduce(#Max)`, `m.matmul(n)`).
        // Operator overloading on this closed family is blessed; named methods are
        // the rest of the surface. Set `recv_type_out` so codegen routes to the
        // math-method op (TIR handle-method path).
        if let Type::Named(math_ty) = &recv_ty {
            if is_math_type(math_ty) && !self.registry.contains(math_ty) {
                let math_ty = math_ty.clone();
                // `reduce(#Op)` — the sole arg is a reduce-op marker, validated here.
                if method == "reduce" && is_simd_lane_type(&math_ty) {
                    if args.len() != 1 {
                        self.diags.push(Diagnostic::error(
                            "E2510",
                            format!("`reduce` takes one reduce-op marker, got {}", args.len()),
                            "a lane reduction names its operation with a marker".to_string(),
                            "write `v.reduce(#Add)`, `#Mul`, `#Min`, or `#Max`".to_string(),
                            Some(span),
                        ));
                    } else if let Expr::ReduceMarker(op, mspan) = &args[0].expr {
                        if !simd_reduce_markers().contains(&op.as_str()) {
                            self.diags.push(Diagnostic::error(
                                "E2510",
                                format!("`#{}` isn't a reduce operation", op),
                                "a lane reduction folds the lanes with one of a fixed set of operations".to_string(),
                                "use `#Add`, `#Mul`, `#Min`, or `#Max`".to_string(),
                                Some(*mspan),
                            ));
                        }
                    } else {
                        self.diags.push(Diagnostic::error(
                            "E2510",
                            "`reduce` takes a reduce-op marker, not a value".to_string(),
                            "the operation is named with a marker so the fold is explicit".to_string(),
                            "write `v.reduce(#Add)`, `#Mul`, `#Min`, or `#Max`".to_string(),
                            Some(args[0].expr.span()),
                        ));
                        self.infer(&mut args[0].expr);
                    }
                    *recv_type_out = Some(math_ty.clone());
                    return Some(math_scalar_ty(&math_ty));
                }
                if let Some(ret) = math_method_return(&math_ty, method, args.len()) {
                    // Check each arg against its expected type (so a literal
                    // elaborates), then return the method's result type.
                    for arg in args.iter_mut() {
                        let want = math_method_arg_ty(&math_ty, method);
                        let old = self.expected_type.take();
                        if let Some(w) = &want { self.expected_type = Some(w.clone()); }
                        let got = self.infer(&mut arg.expr);
                        self.expected_type = old;
                        if let (Some(w), Some(g)) = (&want, &got) {
                            if g != w {
                                self.diags.push(Diagnostic::error(
                                    "E0128",
                                    format!("`.{}()` on `{}` expects a `{}`, got `{}`", method, math_ty, w.name(), g.name()),
                                    format!("`{}.{}(…)` operates on a `{}`", math_ty, method, w.name()),
                                    format!("pass a `{}` value", w.name()),
                                    Some(arg.expr.span()),
                                ));
                            }
                        }
                    }
                    *recv_type_out = Some(math_ty.clone());
                    return Some(ret);
                }
                // A math type but an unknown method — fall through to the generic
                // "no such method" path below, which prints a teaching diagnostic.
            }
        }
        if let Type::Named(n) = &recv_ty {
            if let Some(param) = self.type_param_scope.iter().find(|p| p.name == *n) {
                for (trait_name, info) in &self.trait_reg.traits {
                    if let Some(msig) = info.methods.get(method) {
                        if !param.bounds.iter().any(|b| b == trait_name) {
                            self.diags.push(e0901(method, trait_name, span));
                        }
                        *recv_type_out = Some(n.clone());
                        for arg in args.iter_mut() {
                            self.infer(&mut arg.expr);
                        }
                        return msig.return_type.clone();
                    }
                }
            }
        }
        // D-SOA1: a `[S]` of a `#layout(columnar)` struct is stored
        // struct-of-arrays. v1 supports the core list surface (construct,
        // index-read, field-read, `len`, `is_empty`, `push`, iterate); the rest
        // is deferred — reject the method with a clear message rather than
        // silently miscompiling on columnar storage.
        if let Type::List(inner) = &recv_ty {
            if let Type::Named(elem) = inner.as_ref() {
                if self.registry.is_columnar(elem)
                    && !matches!(method, "len" | "is_empty" | "push")
                    && Collections::builtin_method_return(&recv_ty, method, args.len(), false).is_some()
                {
                    self.diags.push(Diagnostic::error(
                        "E1108",
                        format!(
                            "`.{}()` isn't supported on a columnar list `{}` yet",
                            method,
                            recv_ty.show()
                        ),
                        "`#layout(columnar)` lists support the core surface in v1: indexing, field access, `len`, `is_empty`, `push`, and iteration".to_string(),
                        format!(
                            "drop `#layout(columnar)` from `{}` to use `.{}()`, or rewrite the loop with indexing",
                            elem, method
                        ),
                        Some(span),
                    ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
            }
        }
        // D-SERDE-ACCESS=B: accessor methods on Data/Json/DataTree.
        if let Type::Named(ref tn) = recv_ty {
            if is_json_type_name(tn) {
                if let Some(ret) = datatree_method_return(method, args.len()) {
                    let json_ret = match ret {
                        Type::Result { ok, err } => Type::Result {
                            ok: if matches!(*ok, Type::Named(ref n) if n == "DataTree") {
                                Box::new(json_ty())
                            } else { ok },
                            err,
                        },
                        other => other,
                    };
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    *recv_type_out = Some(tn.clone());
                    return Some(json_ret);
                }
            }
            if tn == "DataTree" {
                if let Some(ret) = datatree_method_return(method, args.len()) {
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    *recv_type_out = Some("DataTree".to_string());
                    return Some(ret);
                }
            }
        }
        if let Some(ret) = Collections::builtin_method_return(&recv_ty, method, args.len(), false) {
            // D-NUMOPS1: hand codegen the receiver's numeric width so it picks the
            // same widening/narrowing form sema just chose for the return type.
            if recv_ty.is_numeric() {
                *recv_type_out = Some(recv_ty.name());
            }
            let result = self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
            // D-ITER1: enumerate/zip/partition return named-tuple types. Store the
            // resolved return type in `resolved_ret_out` so Tuples.rs can collect
            // the JetTup_ shape and the TIR lowering pass can read the field names.
            if let Some(ref ty) = result {
                if contains_tuple_type(ty) {
                    *resolved_ret_out = Some(ty.clone());
                }
            }
            return result;
        }
        if let Type::TraitObject(trait_name) = &recv_ty {
            let sig = self
                .trait_reg
                .traits
                .get(trait_name)
                .and_then(|t| t.methods.get(method));
            if let Some(msig) = sig {
                *recv_type_out = Some(trait_name.clone());
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return msig.return_type.clone();
            }
            self.diags.push(Diagnostic::error(
                "E0102",
                format!("trait `{trait_name}` has no method `{method}`"),
                "check the method name on this trait value".to_string(),
                format!("add `fn {method}(…)` to `trait {trait_name}`"),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        let type_name = match &recv_ty {
            Type::Named(n) => n.clone(),
            Type::Option(inner) => match inner.as_ref() {
                Type::Named(n) => n.clone(),
                _ => {
                    self.diags.push(Diagnostic::error(
                        "E0311",
                        format!("`{}` isn't a method on this value", method),
                        "instance methods belong to struct or enum values".to_string(),
                        format!(
                            "call it on the type: `{}.{method}(...)` if it's static",
                            recv_ty.name()
                        ),
                        Some(span),
                    ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
            },
            _ => {
                self.diags.push(Diagnostic::error(
                    "E0311",
                    format!("`{}` isn't a method on this value", method),
                    "only struct and enum values have instance methods".to_string(),
                    format!("check the spelling of `{}`", method),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
        };
        if let Some(fields) = self.registry.struct_fields(&type_name) {
            if let Some((_, _, field_ty, _, _)) =
                fields.iter().find(|(fname, _, _, _, _)| fname == method)
            {
                if matches!(field_ty, Type::Fn { .. }) {
                    *recv_type_out = Some(type_name.clone());
                    let mut callee =
                        Box::new(Expr::Field(receiver.clone(), method.to_string(), span));
                    let end = args.last().map(|a| a.expr.span().end).unwrap_or(span.end);
                    let call_span = Span::new(span.start, end);
                    return self.infer_call_value(&mut callee, args, call_span);
                }
            }
        }
        let Some(msig) = self.registry.method(&type_name, method).cloned() else {
            self.diags.push(Diagnostic::error(
                "E0102",
                format!("`{}` has no method `{}`", type_name, method),
                "check the method name on this type".to_string(),
                format!("define it inside `struct {type_name}` or `impl {type_name}`"),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        };
        if msig.is_static {
            self.diags.push(Diagnostic::error(
                "E0311",
                format!("`{}` is a static method on `{}`", method, type_name),
                "static methods belong to the type name, not a value".to_string(),
                format!("write `{}.{method}(...)` instead", type_name),
                Some(span),
            ));
        }
        *recv_type_out = Some(type_name.clone());
        // `mut self` methods change the receiver: it must be changeable,
        // free of an active `for` borrow, and not aliased by an argument.
        if msig.self_conv == Some(AccessConvention::Write) {
            if let Some(root) = expr_root_ident(receiver) {
                let root = root.to_string();
                if self.iter_borrowed.contains(&root) {
                    self.diags.push(collection_changed_in_loop(&root, span));
                }
                if let Some(info) = self.lookup(&root) {
                    if !info.mutable {
                        let (what, fix) = if root == Syntax::KW_SELF {
                            (
                                format!(
                                    "`.{}()` edits `{}`, but this method has read access only",
                                    method,
                                    Syntax::KW_SELF
                                ),
                                format!(
                                    "declare the enclosing method with `{}{}`",
                                    Syntax::SIGIL_MUTATE,
                                    Syntax::KW_SELF
                                ),
                            )
                        } else {
                            (
                                format!(
                                    "cannot write to `{}` — it does not have edit access (`~`); required before calling `.{}()`",
                                    root,
                                    method
                                ),
                                format!("declare `{} {} ...`", root, Syntax::SIGIL_BIND_MUT),
                            )
                        };
                        self.diags.push(Diagnostic::error(
                            "E0202",
                            what,
                            "this method edits the value it's called on; write access (`~`) is required".to_string(),
                            fix,
                            Some(span),
                        ));
                    }
                }
                for arg in args.iter() {
                    if matches!(&arg.expr, Expr::Ident(n, _) if *n == root) {
                        self.diags.push(aliasing_while_mut(&root, arg.expr.span()));
                    }
                }
            }
        }
        if msig.self_conv == Some(AccessConvention::Move) {
            if let Expr::Ident(n, nspan) = &**receiver {
                // A borrowed parameter can't be consumed (the generated Rust
                // would move out of a `&T`/`&mut T`).
                if let Some(info) = self.lookup(n) {
                    if !type_is_copy(&info.ty)
                        && matches!(
                            info.param_conv,
                            Some(AccessConvention::Read) | Some(AccessConvention::Write)
                        )
                    {
                        self.diags.push(Diagnostic::error(
                            "E0120",
                            format!(
                                "`{}` was not moved here, so `.{}()` cannot take it (`^`)",
                                n, method
                            ),
                            "this function has read access only and does not own the value".to_string(),
                            format!(
                                "call it on a copy: `{}.clone().{}(...)` — or take ownership with `{}: {}{}`",
                                n,
                                method,
                                n,
                                Syntax::SIGIL_MOVE,
                                info.ty.name()
                            ),
                            Some(*nspan),
                        ));
                    }
                }
                self.mark_moved(n.clone(), *nspan);
            }
        }
        self.check_method_args(&type_name, method, &msig, args, span)?;
        msig.return_type.clone()
    }


    /// Check a call. Returns:
    ///   None             — problem already reported
    ///   Some(None)       — fine, no value handed back
    ///   Some(Some(ty))   — fine, hands back `ty`
    /// D-NUMOPS1: type a `wrapping`/`saturating`/`checked` opt-in. The single
    /// argument must be one integer `+`/`-`/`*`/`/`; `wrapping`/`saturating`
    /// return the operand width, `checked` returns it optional (`null` on
    /// overflow). E1005 otherwise.
    fn check_overflow_opt_in(&mut self, call: &mut Call) -> Option<Type> {
        let kind = call.name.clone();
        if call.args.len() != 1 {
            let mut ty = None;
            for a in call.args.iter_mut() {
                ty = ty.or(self.infer(&mut a.expr));
            }
            self.diags.push(overflow_opt_in_error(&kind, call.name_span));
            // Hand back a plausible type so the use site doesn't cascade.
            return ty.filter(Type::is_integer).or(Some(Type::Int));
        }
        let arg_ty = self.infer(&mut call.args[0].expr);
        let is_arith = matches!(
            &call.args[0].expr,
            Expr::Binary(op, _, _, _)
                if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div)
        );
        let int_ok = arg_ty.as_ref().is_some_and(|t| t.is_integer());
        if !is_arith || !int_ok {
            self.diags.push(overflow_opt_in_error(&kind, call.name_span));
            return arg_ty.filter(Type::is_integer).or(Some(Type::Int));
        }
        let t = arg_ty.unwrap();
        if kind == Syntax::BUILTIN_CHECKED {
            Some(Type::Option(Box::new(t)))
        } else {
            Some(t)
        }
    }

    pub(crate) fn check_call(&mut self, call: &mut Call, _as_value: bool) -> Option<Option<Type>> {
        // D-NUMOPS1: `wrapping`/`saturating`/`checked` opt-ins wrap a single integer
        // `+`/`-`/`*`/`/`. A user-defined function of the same name shadows them.
        if matches!(
            call.name.as_str(),
            Syntax::BUILTIN_WRAPPING | Syntax::BUILTIN_SATURATING | Syntax::BUILTIN_CHECKED
        ) && !self.funcs.contains_key(&call.name)
        {
            return Some(self.check_overflow_opt_in(call));
        }
        // D-EFF1: an ambient builtin (`print`/`input`) contributes the `Io`
        // effect, unless a user function of the same name shadows it (in which
        // case the edge to that user function is recorded below).
        if !self.funcs.contains_key(&call.name) {
            if let Some(e) = builtin_effect(&call.name) {
                self.record_effect(e);
            }
        }
        if call.name == Syntax::FOREIGN_PRINTLN || call.name == Syntax::FOREIGN_EPRINTLN {
            let target = if call.name == Syntax::FOREIGN_EPRINTLN {
                "io.eprint"
            } else {
                Syntax::BUILTIN_PRINT
            };
            self.diags.push(Diagnostic::error(
                "E0037",
                format!(
                    "{} calls it `{}`, not `{}`",
                    Syntax::LANG_NAME,
                    target,
                    call.name
                ),
                "`print` writes to stdout; `io.eprint` is the stderr twin in `core.io`".to_string(),
                format!("replace `{}` with `{}`", call.name, target),
                Some(call.name_span),
            ));
            for arg in call.args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        }

        if call.name == Syntax::FOREIGN_OPEN {
            self.diags.push(Diagnostic::error(
                "E0038",
                "`open` is not the M10 file API".to_string(),
                "M10 uses whole-file helpers in `core.fs`; file handles are out of scope"
                    .to_string(),
                "import `core.fs as fs` and call `fs.read(path)` or `fs.write(path, text)`"
                    .to_string(),
                Some(call.name_span),
            ));
            for arg in call.args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        }

        if call.name == Syntax::FOREIGN_GETENV {
            self.diags.push(Diagnostic::error(
                "E0039",
                "`getenv` is written `env.get` in Jet".to_string(),
                "environment access lives in the `core.env` module".to_string(),
                "import `core.env as env` and call `env.get(name)`".to_string(),
                Some(call.name_span),
            ));
            for arg in call.args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        }

        if matches!(
            call.name.as_str(),
            Syntax::FOREIGN_ASYNC | Syntax::FOREIGN_AWAIT
        ) {
            self.diags.push(Diagnostic::error(
                "E0040",
                format!("`{}` is not in Jet; use `tasks.spawn` instead", call.name),
                "Jet uses blocking tasks and channels, not async/await — simpler and race-free"
                    .to_string(),
                "import `core.tasks as tasks` and call `tasks.spawn(() => your_work())`".to_string(),
                Some(call.name_span),
            ));
            for a in call.args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }

        if matches!(
            call.name.as_str(),
            Syntax::FOREIGN_MUTEX | Syntax::FOREIGN_LOCK | "RwLock" | "mutex"
        ) {
            self.diags.push(Diagnostic::error(
                "E0041",
                format!(
                    "`{}` is not in Jet; share data through channels",
                    call.name
                ),
                "Jet avoids shared mutable state: tasks communicate by sending messages, not sharing memory"
                    .to_string(),
                "import `core.tasks as tasks`, create a channel, and use `sender.send`/`channel.receive`"
                    .to_string(),
                Some(call.name_span),
            ));
            for a in call.args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }

        if call.name == Syntax::BUILTIN_PRINT {
            if call.args.len() != 1 {
                self.diags.push(Diagnostic::error(
                    "E0103",
                    format!(
                        "`{}` needs exactly one thing to print",
                        Syntax::BUILTIN_PRINT
                    ),
                    "printing nothing isn't meaningful".to_string(),
                    format!("e.g. {}(\"hello\")", Syntax::BUILTIN_PRINT),
                    Some(call.name_span),
                ));
                for arg in call.args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            }
            let arg = &mut call.args[0];
            self.borrow_ctx = true; // print borrows via `.jet_show()`
            if let Some(t) = self.infer(&mut arg.expr) {
                if !is_printable(&t, self.registry) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{}` doesn't know how to show {}",
                            Syntax::BUILTIN_PRINT,
                            t.show()
                        ),
                        "print shows values that have a display".to_string(),
                        "print one of its parts instead".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
            }
            return Some(None);
        }

        // D-PRELUDE1 = B: `input` is ambient — no `use core.io` needed.
        // Resolves to the same semantics as `io.input`: optional String prompt,
        // returns Result(String, IoError). Shadowed by any user-defined `input`.
        if call.name == Syntax::BUILTIN_INPUT
            && self.funcs.get(Syntax::BUILTIN_INPUT).is_none()
            && self.lookup(Syntax::BUILTIN_INPUT).is_none()
        {
            if call.args.len() > 1 {
                self.diags
                    .push(wrong_core_arity(Syntax::BUILTIN_INPUT, 1, call.args.len(), call.name_span));
            }
            if let Some(arg) = call.args.get_mut(0) {
                self.expect_core_arg(Syntax::BUILTIN_INPUT, 0, &Type::String, arg);
            }
            return Some(Some(result_ty(Type::String, io_error_ty())));
        }

        if call.name == Syntax::BUILTIN_PANIC {
            self.check_panic_call(call);
            return Some(None);
        }

        if call.name == Syntax::BUILTIN_REQUIRE {
            self.check_require_call(call);
            return Some(None);
        }

        if call.name == Syntax::BUILTIN_REQUIRE_EQ {
            self.check_require_eq_call(call);
            return Some(None);
        }

        // D-LIN1-DROP (ratified 2026-06-25): `drop(x)` deliberately discards a
        // value by moving it to nowhere — its `Drop` runs. The blessed use is to
        // satisfy a `#SingleUse` value's consume duty when there is genuinely no
        // job left to do; that decision must be audited, so `drop` of a
        // `#SingleUse` value is legal only inside an `#Unsafe("reason")`
        // region/fn (the reason IS the audit note) — otherwise E0143. Shadowed
        // by any user `drop` fn or local of that name.
        if call.name == Syntax::BUILTIN_DROP
            && self.funcs.get(Syntax::BUILTIN_DROP).is_none()
            && self.lookup(Syntax::BUILTIN_DROP).is_none()
        {
            if call.args.len() != 1 {
                self.diags.push(Diagnostic::error(
                    "E0103",
                    format!("`{}` discards exactly one value", Syntax::BUILTIN_DROP),
                    "`drop` throws a single value away, running its cleanup".to_string(),
                    format!("e.g. {}(x)", Syntax::BUILTIN_DROP),
                    Some(call.name_span),
                ));
                for a in call.args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(None);
            }
            self.infer(&mut call.args[0].expr);
            if let Expr::Ident(name, span) = &call.args[0].expr {
                let single_use = self
                    .lookup(name)
                    .map(|info| info.single_use_span.is_some())
                    .unwrap_or(false);
                if single_use && !self.in_unsafe {
                    self.diags.push(e0143_drop_unaudited(name, *span));
                }
                // The value is given away for real — discharges the consume duty
                // (E0140/E0141) and prevents any later reuse (E0121). Mark it
                // consumed even on the E0143 path so the unaudited-drop error is
                // not buried under a cascade E0140 "unconsumed" at scope end.
                self.mark_moved(name.clone(), *span);
            }
            return Some(None);
        }

        // D-TOOL4 (E2-M11): `expect(x)` — test-only builtin that wraps a value
        // for snapshot testing. The expression `expect(x).snapshot()` is the
        // full form; `.snapshot()` is handled in the method-call path below.
        if call.name == Syntax::BUILTIN_EXPECT {
            if call.args.len() != 1 {
                self.diags.push(Diagnostic::error(
                    "E2901",
                    format!("`{}` needs exactly one value to test", Syntax::BUILTIN_EXPECT),
                    "snapshot testing wraps one value at a time".to_string(),
                    format!("e.g. {}(my_value).snapshot()", Syntax::BUILTIN_EXPECT),
                    Some(call.name_span),
                ));
            } else {
                self.infer(&mut call.args[0].expr);
            }
            // Returns a Named type marker so the `.snapshot()` call can detect it.
            return Some(Some(Type::Named("__JetExpect__".to_string())));
        }

        if self.funcs.get(&call.name).is_none() {
            if let Some(info) = self.lookup(&call.name) {
                if matches!(info.ty, Type::Fn { .. }) {
                    let name_span = call.name_span;
                    let mut callee = Box::new(Expr::Ident(call.name.clone(), name_span));
                    let mut args = std::mem::take(&mut call.args);
                    let end = args
                        .last()
                        .map(|a| a.expr.span().end)
                        .unwrap_or(name_span.end);
                    let span = Span::new(name_span.start, end);
                    let ret = self.infer_call_value(&mut callee, &mut args, span);
                    call.args = args;
                    return Some(ret);
                }
            }
            // D-MOD3: check unqualified inline-module imports (e.g. `use math.clamp`).
            if let Some(mangled) = self.unqualified.get(&call.name).cloned() {
                let alias = mangled.split("__").next().unwrap_or(&mangled).to_string();
                let result = self.infer_code_module_call(&alias, &mangled, call.name_span, call.name_span, &mut call.args);
                return Some(result);
            }
            // D-MOD3: check unqualified file-module imports (e.g. `use math.clamp` for a file module).
            if let Some((fn_name, mod_idx)) = self.unqualified_file.get(&call.name).cloned() {
                let result = self.infer_import_call(mod_idx, &fn_name, call.name_span, call.name_span, &mut call.args);
                return Some(result);
            }
        }

        // D-SIMD2 / D-LINALG1: `F32x4(a,b,c,d)` / `Vec3(x,y,z)` / `Mat3(…)` —
        // positional construction of a built-in math value type. Each argument is
        // elaborated against the component type (so `1.0` becomes `F32` for a
        // `F32x4`) and checked in order; arity is fixed by the type.
        if self.funcs.get(&call.name).is_none() && !self.registry.contains(&call.name) {
            if let Some(arg_types) = math_constructor_arg_types(&call.name) {
                let arity = arg_types.len();
                if call.args.len() != arity {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        format!(
                            "`{}` takes exactly {} component{}, got {}",
                            call.name,
                            arity,
                            if arity == 1 { "" } else { "s" },
                            call.args.len()
                        ),
                        format!(
                            "`{}` is a built-in {} type — construct it from its {} components",
                            call.name,
                            if is_simd_lane_type(&call.name) { "SIMD lane" } else { "linear-algebra" },
                            arity
                        ),
                        format!("write `{}({})`", call.name, vec!["…"; arity].join(", ")),
                        Some(call.name_span),
                    ));
                    for a in call.args.iter_mut() { self.infer(&mut a.expr); }
                    return Some(Some(Type::Named(call.name.clone())));
                }
                for (i, want) in arg_types.iter().enumerate() {
                    let old = self.expected_type.replace(want.clone());
                    let got = self.infer(&mut call.args[i].expr);
                    self.expected_type = old;
                    if let Some(at) = got {
                        if at != *want {
                            self.diags.push(Diagnostic::error(
                                "E0128",
                                format!(
                                    "component {} of `{}` must be `{}`, got `{}`",
                                    i + 1, call.name, want.name(), at.name()
                                ),
                                format!(
                                    "every component of `{}` is a `{}`", call.name, want.name()
                                ),
                                format!("write a `{}` value here", want.name()),
                                Some(call.args[i].expr.span()),
                            ));
                        }
                    }
                }
                return Some(Some(Type::Named(call.name.clone())));
            }
        }

        // D-DIST3 (ratified 2026-06-20): `DistinctType(expr)` — construct a distinct value.
        if self.funcs.get(&call.name).is_none() {
            if let Some(base_ty) = self.registry.distinct_base(&call.name).cloned() {
                if call.args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        format!(
                            "`{}` takes exactly one argument, got {}",
                            call.name,
                            call.args.len()
                        ),
                        format!("`{}` is a distinct type; construct it with `{}(value)`", call.name, call.name),
                        format!("write `{}(expr)` with a single value of type `{}`", call.name, base_ty.name()),
                        Some(call.name_span),
                    ));
                    for a in call.args.iter_mut() { self.infer(&mut a.expr); }
                    return None;
                }
                let old_expected = self.expected_type.replace(base_ty.clone());
                let arg_ty = self.infer(&mut call.args[0].expr);
                self.expected_type = old_expected;
                if let Some(at) = arg_ty {
                    if at != base_ty {
                        self.diags.push(Diagnostic::error(
                            "E0128",
                            format!(
                                "a `{}` can't be used where a `{}` is expected",
                                at.name(), call.name
                            ),
                            format!(
                                "`{}` and `{}` are different types — even though `{}` is built on `{}`, one is never accepted in place of the other",
                                call.name, at.name(), call.name, base_ty.name()
                            ),
                            format!("construct a `{}`: `{}({})`", call.name, call.name, "expr"),
                            Some(call.args[0].expr.span()),
                        ));
                        return None;
                    }
                }
                return Some(Some(Type::Named(call.name.clone())));
            }
        }

        let Some(sig) = self.funcs.get(&call.name).cloned() else {
            let mut fix = format!(
                "define it first ({} {}() {{ ... }}), or call one that exists",
                Syntax::KW_FN,
                call.name
            );
            let mut best: Option<(&str, usize)> = None;
            for cand in self
                .funcs
                .keys()
                .map(|s| s.as_str())
                .chain(Syntax::PRELUDE_IDENTS.iter().copied())
            {
                let d = edit_distance(&call.name, cand);
                if d <= 2 && best.map_or(true, |(_, bd)| d < bd) {
                    best = Some((cand, d));
                }
            }
            if let Some((cand, _)) = best {
                fix = format!("did you mean `{}`?", cand);
            }
            self.diags.push(Diagnostic::error(
                "E0102",
                format!("nothing named `{}` exists here", call.name),
                format!(
                    "only functions that have been defined (or built in, like `{}` / `{}`) can be called",
                    Syntax::BUILTIN_PRINT, Syntax::BUILTIN_INPUT
                ),
                fix,
                Some(call.name_span),
            ));
            for arg in call.args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        };

        // D-EFF1: record the call-graph edge for transitive effect inference.
        // A foreign (`extern`) callee has an un-inspectable body, so it forces
        // the maximal effect set; a Jet callee's effects flow in via its edge.
        if sig.is_extern {
            self.record_maximal();
        } else {
            self.record_edge(call.name.clone());
        }

        // E3103 (S58): an `#Unsafe fn` is a whole-function contract; callers
        // must take responsibility inside their own `#Unsafe` block.
        if sig.is_unsafe && !self.in_unsafe {
            self.diags.push(Diagnostic::error(
                "E3103",
                format!("`{}` is an `#Unsafe` function", call.name),
                "its contract can't be checked by the compiler, so the caller must vouch for it"
                    .to_string(),
                format!(
                    "call it inside `#{}(\"…\") {{ … }}`",
                    Syntax::KW_UNSAFE
                ),
                Some(call.name_span),
            ));
        }

        // D-NARG-D4 (S61, E0125): label validation — if a call arg has
        // `name: val`, verify it matches the parameter name at that position.
        // Labels never reorder.
        if !sig.param_info.is_empty() {
            let all_param_names: Vec<&str> =
                sig.param_info.iter().map(|(n, _)| n.as_str()).collect();
            for (i, arg) in call.args.iter().enumerate() {
                if let Some((label, label_span)) = &arg.label {
                    if let Some((param_name, _)) = sig.param_info.get(i) {
                        if label != param_name {
                            // Is the label a real param name at a different position?
                            if all_param_names.contains(&label.as_str()) {
                                // Transposed: label names a real param, but wrong position.
                                self.diags.push(Diagnostic::error(
                                    "E0125",
                                    format!(
                                        "label `{}:` doesn't match the parameter `{}` here",
                                        label, param_name
                                    ),
                                    "labels are checked documentation — each names the parameter at its own position, and arguments stay in the order they're declared".to_string(),
                                    format!(
                                        "write `{}:` here, or drop the label",
                                        param_name
                                    ),
                                    Some(*label_span),
                                ));
                            } else {
                                // Unknown: label doesn't name any parameter.
                                self.diags.push(Diagnostic::error(
                                    "E0125",
                                    format!(
                                        "`{}` has no parameter named `{}`",
                                        call.name, label
                                    ),
                                    format!(
                                        "a label must name the parameter at its position; `{}` takes {}",
                                        call.name,
                                        all_param_names.join(", ")
                                    ),
                                    format!(
                                        "use one of `{}`'s parameter names, or drop the label",
                                        call.name
                                    ),
                                    Some(*label_span),
                                ));
                            }
                        }
                    }
                }
            }
            // L2401: advisory lint — public API has a positional Bool parameter.
            // (Only warn on the callee definition side, not every call site.)
        }

        // D-NARG-D2 (S61): default-value filling — append defaults for omitted
        // trailing params. Earlier-param refs in defaults are substituted with
        // the supplied argument expression so codegen never sees an unresolved
        // identifier (invariant I2).
        if call.args.len() < sig.params.len() && !sig.defaults.is_empty() {
            let provided = call.args.len();
            let required: usize = sig
                .defaults
                .iter()
                .take_while(|d| d.is_none())
                .count();
            if provided >= required {
                // fill trailing omitted params with their defaults. We build
                // `earlier_names` incrementally so a default like `d: Int = h`
                // can reference an earlier-defaulted param `h` that was already
                // filled (and is now in call.args at position 1).
                let all_param_names: Vec<String> =
                    sig.param_info.iter().map(|(n, _)| n.clone()).collect();
                for i in provided..sig.params.len() {
                    if let Some(Some(default_expr)) = sig.defaults.get(i) {
                        // earlier_names covers all params up to (not including) i.
                        let earlier_names: Vec<String> =
                            all_param_names.iter().take(i).cloned().collect();
                        // Substitute any earlier-param idents with the supplied arg.
                        let resolved = super::substitute_param_refs(
                            default_expr.clone(),
                            &earlier_names,
                            &call.args,
                        );
                        call.args.push(crate::AST::CallArg {
                            convention: sig.params[i].0,
                            expr: resolved,
                            span: call.name_span,
                            flags: Default::default(),
                            label: None,
                        });
                    }
                }
            }
        }

        if call.args.len() != sig.params.len() {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`{}` expects {} argument{}, got {}",
                    call.name,
                    sig.params.len(),
                    if sig.params.len() == 1 { "" } else { "s" },
                    call.args.len()
                ),
                "every argument must match a parameter".to_string(),
                format!("check the definition of `{}`", call.name),
                Some(call.name_span),
            ));
        }

        let fn_type_params = self
            .trait_reg
            .fn_params
            .get(&call.name)
            .cloned()
            .unwrap_or_default();
        let mut generic_subst = HashMap::new();
        let mut pre_inferred: Vec<Option<Type>> = Vec::new();
        if !fn_type_params.is_empty() {
            for arg in call.args.iter_mut() {
                pre_inferred.push(self.infer(&mut arg.expr));
            }
            let arg_types: Vec<Type> = pre_inferred.iter().filter_map(|t| t.clone()).collect();
            if arg_types.len() == call.args.len() {
                match self.trait_reg.infer_fn_subst(
                    &sig,
                    &arg_types,
                    &fn_type_params,
                    self.expected_type.as_ref(),
                ) {
                    Ok(s) => generic_subst = s,
                    Err(p) => self.diags.push(e0904(call.name_span, &p)),
                }
            }
        }
        let effective_params: Vec<(AccessConvention, Type)> = if generic_subst.is_empty() {
            sig.params.clone()
        } else {
            sig.params
                .iter()
                .map(|(c, t)| (*c, self.trait_reg.instantiate_type(t, &generic_subst)))
                .collect()
        };
        let args_pre_inferred = !generic_subst.is_empty() && pre_inferred.len() == call.args.len();

        let mut mut_borrowed: HashSet<String> = HashSet::new();
        let mut read_borrowed: HashSet<String> = HashSet::new();

        for (i, arg) in call.args.iter_mut().enumerate() {
            if let Expr::Ident(name, span) = &arg.expr {
                if mut_borrowed.contains(name) {
                    self.diags.push(aliasing_while_mut(name, *span));
                } else if arg.convention == AccessConvention::Write && read_borrowed.contains(name)
                {
                    self.diags.push(aliasing_mut_after_read(name, *span));
                }
            }
            if !sig.is_extern {
                if let Some((AccessConvention::Read, pty)) = effective_params.get(i) {
                    if !pty.is_scalar() {
                        self.borrow_ctx = true;
                    }
                }
            } else if let Some((_, pty)) = effective_params.get(i) {
                if !pty.is_scalar() {
                    arg.flags.implicit_clone = true;
                }
            }
            let saved_exp = self.expected_type.clone();
            let saved_esc = self.lambda_escapes;
            if let Some((param_conv, param_ty)) = effective_params.get(i) {
                if matches!(param_ty, Type::Fn { .. }) {
                    self.expected_type = Some(param_ty.clone());
                    self.lambda_escapes = matches!(param_conv, AccessConvention::Move);
                } else if matches!(param_ty, Type::IntN { .. } | Type::Float32) {
                    // D-SG9: let a fixed-width literal argument adopt the parameter's
                    // width and be range-checked at the literal.
                    self.expected_type = Some(param_ty.clone());
                }
            }
            // D-EFF2 (callback param bound): snapshot the effect accumulator
            // before walking a function-typed argument so the callback's own
            // effect contribution (the delta) can be checked against the
            // parameter's declared bound after the walk.
            let cb_bound: Option<Vec<(String, Span)>> = match effective_params.get(i) {
                Some((_, Type::Fn { effect_bound: Some(b), .. })) => Some(b.clone()),
                _ => None,
            };
            let cb_snapshot = cb_bound.as_ref().map(|_| {
                (
                    self.fx_direct.clone(),
                    self.fx_edges.clone(),
                    self.fx_maximal,
                )
            });
            let arg_ty = if args_pre_inferred {
                pre_inferred.get(i).and_then(|t| t.clone())
            } else {
                self.infer(&mut arg.expr)
            };
            self.expected_type = saved_exp;
            self.lambda_escapes = saved_esc;
            let Some((param_conv, param_ty)) = effective_params.get(i) else {
                continue;
            };
            // D-EFF2: a function value passed to a function-typed parameter flows
            // its effects through to this caller (transparent flow-through).
            if matches!(param_ty, Type::Fn { .. }) {
                self.attribute_fn_arg(&arg.expr);
            }
            // D-EFF2 (callback param bound): record the obligation now that the
            // callback's effects are in the accumulator (including the edge added
            // by `attribute_fn_arg` for a named-fn callback). Checked post-solve.
            if let (Some(bound), Some((bd, be, bm))) = (&cb_bound, &cb_snapshot) {
                self.record_callback_obligation(bound, bd, be, *bm, arg.expr.span());
            }
            if arg.convention == AccessConvention::Write && !matches!(arg.expr, Expr::Ident(_, _))
            {
                self.diags.push(Diagnostic::error(
                    "E0202",
                    format!(
                        "`{}` needs a plain named binding after it",
                        Syntax::SIGIL_MUTATE
                    ),
                    "write access (`~`) can only be granted to a named binding, not an expression".to_string(),
                    format!(
                        "bind the value first: `x {} ...` then pass `{}x`",
                        Syntax::SIGIL_BIND_MUT,
                        Syntax::SIGIL_MUTATE
                    ),
                    Some(arg.span),
                ));
            }

            if let Some(arg_ty) = &arg_ty {
                let param_ty = self.resolve_type(param_ty.clone());
                let arg_ty = self.resolve_type(arg_ty.clone());
                let reported = self.check_type_assignable(&param_ty, &arg_ty, arg.expr.span());
                // D-FIXARR1: [T#N] widens to [T] at a call site — compatible but codegen
                // will emit .to_vec() on the argument.
                let fixed_widens = matches!((&param_ty, &arg_ty),
                    (Type::List(pe), Type::FixedList { elem: ae, .. }) if pe == ae);
                let compatible = arg_ty == param_ty
                    || fixed_widens
                    || (matches!(&param_ty, Type::Fn { .. })
                        && matches!(&arg_ty, Type::Fn { .. })
                        && fn_types_compatible(&param_ty, &arg_ty));
                if !reported && !compatible {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{}` wants {} for argument {}, but this is {}",
                            call.name,
                            param_ty.show(),
                            i + 1,
                            arg_ty.show()
                        ),
                        "every argument must match its parameter's type".to_string(),
                        type_fix_hint(&param_ty, &arg_ty),
                        Some(arg.expr.span()),
                    ));
                }
            }

            // D-LIN1 / E0142: a `#SingleUse` value may only be moved/consumed. If
            // it reaches a parameter that does not take ownership (`^`), the call
            // would borrow it (`&`/`view`/read) or copy it (an implicit clone) —
            // both are forbidden, since the value has exactly one use to give.
            if !matches!(param_conv, AccessConvention::Move) {
                if let Expr::Ident(name, span) = &arg.expr {
                    let is_single_use = self
                        .lookup(name)
                        .map(|info| info.single_use_span.is_some())
                        .unwrap_or(false);
                    if is_single_use {
                        self.diags
                            .push(e0142_aliased(name, &call.name, *span));
                        continue;
                    }
                }
            }

            match (param_conv, arg.convention) {
                (AccessConvention::Move, AccessConvention::Read) => {
                    if let Expr::Ident(name, span) = &arg.expr {
                        if is_cloneable(param_ty, self.registry, self.structs) {
                            arg.flags.implicit_clone = true;
                            // D-L0201: only warn when the value is dead after
                            // this call (a wasteful clone).
                            if !self.is_name_live_after(name) {
                                self.diags.push(Diagnostic::lint(
                                    "L0201",
                                    format!(
                                        "implicit clone of `{}`; write `{}{}` to transfer ownership or `.clone()` to silence this warning",
                                        name,
                                        Syntax::SIGIL_MOVE,
                                        name
                                    ),
                                    format!(
                                        "`{}` expects to take ownership of this value",
                                        call.name
                                    ),
                                    format!(
                                        "write `{}{}` to move, or `{}.clone()` to copy explicitly",
                                        Syntax::SIGIL_MOVE,
                                        name,
                                        name
                                    ),
                                    Some(*span),
                                ));
                            }
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0201",
                                format!(
                                    "`{}` needs `{}` here — this value can't be copied",
                                    call.name,
                                    Syntax::SIGIL_MOVE
                                ),
                                format!(
                                    "parameter {} takes ownership (`^`); passing `{}` without `{}` would have to copy it, but this type can't be copied",
                                    i + 1,
                                    name,
                                    Syntax::SIGIL_MOVE
                                ),
                                format!(
                                    "write `{}{}` to move ownership to `{}`",
                                    Syntax::SIGIL_MOVE,
                                    name,
                                    call.name
                                ),
                                Some(*span),
                            ));
                        }
                    }
                }
                (AccessConvention::Move, AccessConvention::Move) => {
                    // The value is given away for real.
                    if let Expr::Ident(name, span) = &arg.expr {
                        if !param_ty.is_scalar() {
                            self.mark_moved(name.clone(), *span);
                        }
                    }
                }
                (AccessConvention::Write, AccessConvention::Read) => {
                    if let Expr::Ident(name, span) = &arg.expr {
                        self.diags.push(Diagnostic::error(
                            "E0202",
                            format!(
                                "parameter `{}` requires write access (`~`) at the call site",
                                name
                            ),
                            format!(
                                "`{}` needs to edit (`~`) this value; passing it without `{}` grants only read access",
                                call.name,
                                Syntax::SIGIL_MUTATE
                            ),
                            format!(
                                "write `{}{}` when calling `{}`",
                                Syntax::SIGIL_MUTATE,
                                name,
                                call.name
                            ),
                            Some(*span),
                        ));
                    }
                }
                (AccessConvention::Write, AccessConvention::Write) => {
                    // `mut x` at the call site: x itself must be changeable.
                    if let Expr::Ident(name, span) = &arg.expr {
                        if let Some(info) = self.lookup(name) {
                            if !info.mutable {
                                self.diags.push(Diagnostic::error(
                                    "E0111",
                                    format!(
                                        "`{}` was made with `{}`, so it can't be changed",
                                        name,
                                        Syntax::SIGIL_BIND_IMMUT
                                    ),
                                    format!(
                                        "`{}` will change this value, so it must be mutable (`{}`)",
                                        call.name,
                                        Syntax::SIGIL_BIND_MUT
                                    ),
                                    format!("declare it with `{} {} ...`", name, Syntax::SIGIL_BIND_MUT),
                                    Some(*span),
                                ));
                            }
                        }
                    }
                }
                (AccessConvention::Read | AccessConvention::Write, AccessConvention::Move) => {
                    self.diags.push(Diagnostic::error(
                        "E0203",
                        format!(
                            "`{}` passed to a parameter that does not consume",
                            Syntax::SIGIL_MOVE
                        ),
                        "only move (`^`) parameters accept a moved value at the call site".to_string(),
                        format!(
                            "remove `{}` or change the parameter to take ownership (`{}`)",
                            Syntax::SIGIL_MOVE,
                            Syntax::SIGIL_MOVE
                        ),
                        Some(arg.span),
                    ));
                }
                _ => {}
            }

            if arg.convention == AccessConvention::Write {
                if let Expr::Ident(name, _) = &arg.expr {
                    mut_borrowed.insert(name.clone());
                }
            }
            if let (Some((param_conv, param_ty)), Expr::Ident(name, _)) =
                (effective_params.get(i), &arg.expr)
            {
                if matches!(param_conv, AccessConvention::Read)
                    && arg.convention == AccessConvention::Read
                    && !param_ty.is_scalar()
                {
                    read_borrowed.insert(name.clone());
                }
            }

            if self.loop_depth > 0 {
                if let Expr::Ident(name, span) = &arg.expr {
                    if let Some(info) = self.lookup(name) {
                        if matches!(info.ty, Type::Shared(_)) {
                            arg.flags.shared_auto_clone = true;
                            self.diags.push(Diagnostic::lint(
                                "L0202",
                                format!(
                                    "auto-cloned `{}` inside a loop; consider hoisting or caching",
                                    name
                                ),
                                "shared handles are cloned when used across a loop boundary"
                                    .to_string(),
                                format!("hoist `{}` before the loop, or clone once outside", name),
                                Some(*span),
                            ));
                        }
                    }
                }
            }
        }

        Some(sig.return_type.as_ref().map(|t| {
            if generic_subst.is_empty() {
                t.clone()
            } else {
                self.trait_reg.instantiate_type(t, &generic_subst)
            }
        }))
    }
}
