use crate::AST::{Expr, Type};
use crate::Collections;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::substitute_type;
use crate::Sema::Captures::{lambda_body_refs_name, lambda_collect_captures};
use crate::Sema::Bundle::fn_types_compatible;
use crate::Sema::Checker;
use crate::Sema::CheckerCoreLib::wrong_core_arity;
use crate::Sema::Diagnostics::{collection_changed_in_loop, expr_root_ident, type_fix_hint};
use crate::Syntax;
use std::collections::HashSet;
impl<'a> Checker<'a> {
        fn para_type_is_transferable(&self, ty: &Type) -> bool {
            fn transferable(
                checker: &Checker<'_>,
                ty: &Type,
                owner_hint: Option<usize>,
                seen: &mut HashSet<(usize, String)>,
            ) -> bool {
                // D-DATARACE1=C: reactive handles are lock-ordered Arc and may cross
                // parallel adapters; `#Local` pins are rejected at capture time.
                if checker.sendability_problem(ty, true).is_some() {
                    return false;
                }
                match ty {
                    Type::Fn { .. } | Type::TraitObject(_) => false,
                    Type::List(inner)
                    | Type::Shared(inner)
                    | Type::Option(inner)
                    | Type::Tagged { inner, .. } => transferable(checker, inner, owner_hint, seen),
                    Type::Result { ok, err } => transferable(checker, ok, owner_hint, seen)
                        && transferable(checker, err, owner_hint, seen),
                    Type::Map { key, value, .. } => transferable(checker, key, owner_hint, seen)
                        && transferable(checker, value, owner_hint, seen),
                    Type::Tuple(fields) => fields
                        .iter()
                        .all(|(_, field_ty)| transferable(checker, field_ty, owner_hint, seen)),
                    Type::FixedList { elem, .. } => transferable(checker, elem, owner_hint, seen),
                    Type::Named(name) | Type::Apply { name, .. } => {
                        let args = match ty {
                            Type::Apply { args, .. } => args.as_slice(),
                            _ => &[],
                        };
                        if !args
                            .iter()
                            .all(|arg| transferable(checker, arg, owner_hint, seen))
                        {
                            return false;
                        }
                        let (import_ns, leaf) = name
                            .rsplit_once('.')
                            .map_or((None, name.as_str()), |(alias, leaf)| (Some(alias), leaf));
                        let hinted_owner = owner_hint.filter(|&owner| {
                            if owner == checker.module_idx {
                                checker.registry.contains(leaf)
                            } else {
                                checker
                                    .modules
                                    .and_then(|modules| modules.get(owner))
                                    .is_some_and(|module| module.registry.contains(leaf))
                            }
                        });
                        let Some(owner) = hinted_owner
                            .or_else(|| checker.struct_owner_module(leaf, import_ns))
                        else {
                            return true;
                        };
                        let key = (owner, leaf.to_string());
                        if !seen.insert(key.clone()) {
                            return true;
                        }
                        let (registry, trait_reg) = if owner == checker.module_idx {
                            (checker.registry, checker.trait_reg)
                        } else {
                            let Some(module) = checker
                                .modules
                                .and_then(|modules| modules.get(owner))
                            else {
                                return false;
                            };
                            (&module.registry, &module.trait_reg)
                        };
                        let subst = registry
                            .type_alias(leaf)
                            .map(|(params, _)| params)
                            .or_else(|| trait_reg.struct_params.get(leaf).map(Vec::as_slice))
                            .unwrap_or(&[])
                            .iter()
                            .zip(args.iter())
                            .map(|(param, arg)| (param.name.clone(), arg.clone()))
                            .collect();
                        let safe = match registry.types.get(leaf) {
                            Some(crate::Sema::TypeDef::Struct { fields, .. }) => fields.iter().all(
                                |(_, _, field_ty, _)| {
                                    let actual = trait_reg.instantiate_type(field_ty, &subst);
                                    transferable(checker, &actual, Some(owner), seen)
                                },
                            ),
                            Some(crate::Sema::TypeDef::Enum { variants, .. }) => variants
                                .values()
                                .all(|(_, payload)| match payload {
                                    crate::AST::VariantPayload::Unit => true,
                                    crate::AST::VariantPayload::Single(payload, _) => {
                                        let actual = trait_reg.instantiate_type(payload, &subst);
                                        transferable(checker, &actual, Some(owner), seen)
                                    }
                                    crate::AST::VariantPayload::Named(fields) => fields.iter().all(
                                        |field| {
                                            let actual =
                                                trait_reg.instantiate_type(&field.ty, &subst);
                                            transferable(checker, &actual, Some(owner), seen)
                                        },
                                    ),
                                }),
                            Some(crate::Sema::TypeDef::Alias { target, .. }) => {
                                let actual = substitute_type(target, &subst);
                                transferable(checker, &actual, Some(owner), seen)
                            }
                            Some(crate::Sema::TypeDef::Distinct { base, .. }) => {
                                transferable(checker, base, Some(owner), seen)
                            }
                            None => true,
                        };
                        seen.remove(&key);
                        safe
                    }
                    _ => true,
                }
            }
            transferable(self, ty, None, &mut HashSet::new())
        }

        fn reject_para_type(&mut self, role: &str, ty: &Type, span: Span) {
            if !self.para_type_is_transferable(ty) {
                self.diags.push(Diagnostic::error(
                    "E1111",
                    format!("parallel {role} cannot use `{}` across workers", ty.name()),
                    "parallel collection workers may only share or transfer thread-safe owned values".to_string(),
                    "copy the data into a plain owned value, or keep this operation sequential".to_string(),
                    Some(span),
                ));
            }
        }

        fn check_para_lambda(&mut self, expr: &Expr) {
            let Expr::Lambda(lam) = expr else {
                if matches!(expr, Expr::Ident(name, _) if self.funcs.contains_key(name)) {
                    return;
                }
                self.diags.push(Diagnostic::error(
                    "E1111",
                    "parallel callback does not expose worker-sharing facts".to_string(),
                    "stored or imported callbacks may hide captures that are not safe to share between workers".to_string(),
                    "pass a top-level function or write the callback directly as a lambda".to_string(),
                    Some(expr.span()),
                ));
                return;
            };
            for name in &lam.meta.mut_captures {
                self.diags.push(Diagnostic::error(
                    "E1111",
                    format!("parallel callback cannot change captured `{name}`"),
                    "parallel workers need private accumulator state; changing caller-owned state would race or require a hidden merge rule".to_string(),
                    "return the extra data, use `.para_partition(...)`, or use `.para_fold(seed, step, merge)`".to_string(),
                    Some(lam.span),
                ));
            }

            let params = lam.params.iter().map(|param| param.name.clone()).collect::<HashSet<_>>();
            let mut read = HashSet::new();
            let mut changed = HashSet::new();
            lambda_collect_captures(&lam.body, &params, &mut read, &mut changed);
            // Direct-call syntax carries its callee as `Call::name`, rather
            // than an `Expr::Ident`. Add only matching outer bindings here so
            // top-level/builtin calls stay non-captures while stored function
            // values receive the same worker-sharing check as every other read.
            for name in self.scopes.iter().flat_map(|scope| scope.keys()) {
                if !params.contains(name) && lambda_body_refs_name(&lam.body, name) {
                    read.insert(name.clone());
                }
            }
            for name in read {
                if changed.contains(&name)
                    || self.imports.contains_key(&name)
                    || self.core_imports.contains_key(&name)
                {
                    continue;
                }
                let captured = self
                    .lookup(&name)
                    .map(|info| (info.ty.clone(), info.sendable))
                    .or_else(|| self.consts.get(&name).cloned().map(|ty| (ty, true)));
                let Some((ty, sendable)) = captured else { continue };
                if let Some(info) = self.lookup(&name) {
                    if info.reactive_local && super::super::is_reactive_handle_ty(&ty) {
                        self.diags.push(Diagnostic::error(
                            "E1102",
                            format!(
                                "`{name}` is pinned `#{}` and can't cross into a parallel worker",
                                Syntax::ATTR_LOCAL
                            ),
                            format!(
                                "`#{}` keeps `{}` in the fast one-thread form",
                                Syntax::ATTR_LOCAL,
                                ty.name()
                            ),
                            format!(
                                "remove `#{}`, or keep the reactive graph off the parallel path",
                                Syntax::ATTR_LOCAL
                            ),
                            Some(lam.span),
                        ));
                        continue;
                    }
                }
                if super::super::is_reactive_handle_ty(&ty) {
                    self.note_reactive_upgrade(&name, &ty, "parallel");
                }
                if !sendable || self.is_view(&name) || !self.para_type_is_transferable(&ty)
                {
                    self.diags.push(Diagnostic::error(
                        "E1111",
                        format!("parallel callback cannot share captured `{name}`"),
                        "this capture is not safe to read from several worker threads".to_string(),
                        "copy plain owned data into the callback, or keep this operation sequential".to_string(),
                        Some(lam.span),
                    ));
                }
            }
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
            let mut call_access = self.call_access_frame();
            let receiver_convention = if Collections::builtin_needs_mut_receiver(recv_ty, method) {
                crate::AST::AccessConvention::Write
            } else if Collections::is_iter_type(recv_ty) {
                crate::AST::AccessConvention::Move
            } else {
                crate::AST::AccessConvention::Read
            };
            self.with_call_access(&mut call_access, |checker| {
                checker.record_call_receiver_access(receiver, receiver_convention, span);
            });
            if Collections::builtin_needs_mut_receiver(recv_ty, method) {
                if let Some(root) = expr_root_ident(receiver) {
                    let root = root.to_string();
                    if self.in_lambda_body {
                        self.inferred_lambda_mut_captures.insert(root.clone());
                    }
                    let rspan = receiver.span();
                    self.check_owner_change(
                        &root,
                        &format!("be changed by `.{method}()`"),
                        rspan,
                    );
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
                                        Syntax::SIGIL_WRITE,
                                        Syntax::KW_SELF
                                    ),
                                )
                            } else {
                                (
                                    format!(
                                        "cannot write to `{}` — it does not have edit access (`&`); required before calling `.{}()`",
                                        root,
                                        method
                                    ),
                                    format!("declare `{} {} ...`", root, Syntax::SIGIL_BIND_MUT),
                                )
                            };
                            self.diags.push(Diagnostic::error(
                                "E0202",
                                what,
                                "this method edits the collection in place; write access (`&`) is required".to_string(),
                                fix,
                                Some(rspan),
                            ));
                        }
                    }
                }
            }
            if let Type::Apply { name, .. } = recv_ty {
                match (name.as_str(), method) {
                    ("Task", "join") | ("Task", "wait") => {
                        if let Expr::Ident(name, _) = receiver {
                            self.mark_taskgroup_spawn_consumed(name);
                        }
                        self.consume_builtin_receiver(receiver, method);
                        let _ = span;
                        return ret;
                    }
                    ("Task", "detach") => {
                        // D-DETACH1: consume the Task handle (marks it moved → L1101 won't fire).
                        // Two error cases:
                        //   E1106: task captured a `view` borrow — a detached task can outlive
                        //          the borrow; fix-it is to pass an owned `copy` or `Shared<T>`.
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
                                    "pass an owned `copy`, or a `Shared<T>` handle, to the task instead of a `view`".to_string(),
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
                    // D-MEM1 S6 (D-POOLID-API1=A): `Pool<T>` — generational arena.
                    ("Pool", "add") => {
                        return self.finish_pool_add(recv_ty, args, span);
                    }
                    ("Pool", "remove") => {
                        return self.finish_pool_remove(recv_ty, args, span);
                    }
                    ("Pool", "ids") => {
                        if !args.is_empty() {
                            self.diags
                                .push(wrong_core_arity("ids", 0, args.len(), span));
                            for a in args.iter_mut() {
                                self.with_call_access(&mut call_access, |checker| {
                                    let inferred = checker.infer(&mut a.expr);
                                    checker.check_call_argument_captures(&a.expr);
                                    inferred
                                });
                            }
                        }
                        let elem_ty = match recv_ty {
                            Type::Apply { name, args } if name == "Pool" => {
                                args.first().cloned().unwrap_or(Type::Int)
                            }
                            _ => Type::Int,
                        };
                        let id_ty = Type::Apply {
                            name: "Id".to_string(),
                            args: vec![elem_ty],
                        };
                        return Some(Type::List(Box::new(id_ty)));
                    }
                    _ => {}
                }
            }
            // D-ITERTOOLS1=A: every method on `Iter<T>` consumes the view (move).
            // Driving a consumed lazy value twice is E0121, not a runtime throw.
            if Collections::is_iter_type(recv_ty) {
                self.consume_builtin_receiver(receiver, method);
            }
            // D-MEM1 S6 (D-SHARED-API1=A): `Shared<T>` is `Type::Shared`, not
            // `Type::Apply` (it predates this stage — see the type's own doc
            // comment) — a separate receiver match, same shape as the block above.
            if let Type::Shared(inner) = recv_ty {
                match method {
                    "read" => return self.finish_shared_read(inner, args, span),
                    "edit" => return self.finish_shared_edit(inner, args, span),
                    _ => {}
                }
            }
            // D-SHAPE-DURATION1=A: all type-owned unit constructors share one
            // numeric contract. Int stays exact; Float is checked at runtime for
            // finiteness and range after unit scaling.
            if matches!(recv_ty, Type::Named(n) if n == Syntax::DURATION_TYPE)
                && Syntax::DURATION_CONSTRUCTORS.contains(&method)
            {
                for arg in args.iter_mut() {
                    let got = self.with_call_access(&mut call_access, |checker| {
                        let inferred = checker.infer(&mut arg.expr);
                        checker.check_call_argument_captures(&arg.expr);
                        inferred
                    });
                    if !matches!(got, Some(Type::Int | Type::Float)) {
                        self.diags.push(Diagnostic::error(
                            "E0108",
                            format!(
                                "argument to `Duration.{method}()` should be Int or Float"
                            ),
                            "runtime duration construction accepts a numeric value and checks the scaled result".to_string(),
                            "pass an Int or Float value".to_string(),
                            Some(arg.expr.span()),
                        ));
                    }
                }
                return ret;
            }
            let mut refined_ret = ret.clone();
            if let Some(mut expected) = Collections::builtin_method_arg_types(recv_ty, method) {
                let inferred_seed = if matches!(
                    method,
                    "reduce" | "fold" | "scan"
                ) {
                    let saved_exp = self.expected_type.take();
                    let seed = args.first_mut().and_then(|arg| {
                        self.with_call_access(&mut call_access, |checker| {
                            let inferred = checker.infer(&mut arg.expr);
                            checker.check_call_argument_captures(&arg.expr);
                            inferred
                        })
                    });
                    self.expected_type = saved_exp;
                    if let Some(seed_ty) = &seed {
                        if let Some(slot) = expected.first_mut() {
                            *slot = seed_ty.clone();
                        }
                        if let Some(Type::Fn { params, ret, .. }) = expected.get_mut(1) {
                            if let Some(acc) = params.first_mut() {
                                *acc = seed_ty.clone();
                            }
                            *ret = Some(Box::new(seed_ty.clone()));
                        }
                        refined_ret = Some(if method == "scan" {
                            Collections::iter_ty(seed_ty.clone())
                        } else {
                            seed_ty.clone()
                        });
                    }
                    seed
                } else {
                    None
                };
                for (i, arg) in args.iter_mut().enumerate() {
                    let saved_esc = self.lambda_escapes;
                    if Collections::is_closure_method(method) {
                        self.lambda_escapes = false;
                    }
                    let saved_exp = self.expected_type.clone();
                    if let Some(et) = expected.get(i) {
                        self.expected_type = Some(et.clone());
                    }
                    let got = if i == 0 && inferred_seed.is_some() {
                        inferred_seed.clone()
                    } else {
                        self.with_call_access(&mut call_access, |checker| {
                            let inferred = checker.infer(&mut arg.expr);
                            checker.check_call_argument_captures(&arg.expr);
                            inferred
                        })
                    };
                    self.expected_type = saved_exp;
                    self.lambda_escapes = saved_esc;
                    if Syntax::PARA_METHODS.contains(&method) {
                        self.check_para_lambda(&arg.expr);
                    }
                    if method == "para_fold" && i == 0 {
                        if let Some(Type::Fn { ret: Some(acc), .. }) = &got {
                            let acc = (**acc).clone();
                            if let Some(Type::Fn { params, ret, .. }) = expected.get_mut(1) {
                                params[0] = acc.clone();
                                *ret = Some(Box::new(acc.clone()));
                            }
                            if let Some(Type::Fn { params, ret, .. }) = expected.get_mut(2) {
                                params[0] = acc.clone();
                                params[1] = acc.clone();
                                *ret = Some(Box::new(acc.clone()));
                            }
                            refined_ret = Some(acc);
                        }
                    }
                    if method == "zip" && i == 0 {
                        let recv_elem = match recv_ty {
                            Type::List(inner) | Type::FixedList { elem: inner, .. } => {
                                Some((**inner).clone())
                            }
                            Type::Apply { name, args }
                                if name == Syntax::TYPE_ITER && args.len() == 1 =>
                            {
                                Some(args[0].clone())
                            }
                            _ => None,
                        };
                        let arg_elem = match &got {
                            Some(Type::List(inner)) | Some(Type::FixedList { elem: inner, .. }) => {
                                Some((**inner).clone())
                            }
                            Some(Type::Apply { name, args })
                                if name == Syntax::TYPE_ITER && args.len() == 1 =>
                            {
                                Some(args[0].clone())
                            }
                            _ => None,
                        };
                        if let (Some(a), Some(b)) = (recv_elem, arg_elem) {
                            refined_ret = Some(Collections::iter_ty(Collections::zip_elem_ty(&a, &b)));
                        }
                    }
                    if let (Some(et), Some(gt)) = (expected.get(i), got) {
                        if Collections::is_closure_method(method) && i == 0 && method == "map" {
                            if let Type::Fn {
                                ret: Some(ref r), ..
                            } = gt
                            {
                                match recv_ty {
                                    Type::List(inner) => {
                                        refined_ret = Some(Collections::iter_ty((**r).clone()));
                                        let _ = inner;
                                    }
                                    Type::Apply { name, .. }
                                        if name == Syntax::TYPE_ITER =>
                                    {
                                        refined_ret = Some(Collections::iter_ty((**r).clone()));
                                    }
                                    // D-HOLE1: `opt.map(f: T -> R) -> R?`.
                                    Type::Option(_) => {
                                        refined_ret = Some(Type::Option(Box::new((**r).clone())));
                                    }
                                    // D-DYNARRAY1: `view.map(f: T -> R) -> [R]` — map-to-owned;
                                    // the result is a fresh owned list, never another View.
                                    Type::Apply { name, .. }
                                        if matches!(name.as_str(), "View" | "ViewMut") => {
                                        refined_ret = Some(Type::List(Box::new((**r).clone())));
                                    }
                                    _ => {}
                                }
                            }
                        }
                        // D-FAILCOMP1: filter_map(f: T->V?E) → Iter<V>; refine from closure's ok type.
                        if Collections::is_closure_method(method) && i == 0 && method == "filter_map" {
                            if let Type::Fn {
                                ret: Some(ref r), ..
                            } = gt
                            {
                                if let Type::Result { ok, .. } = r.as_ref() {
                                    refined_ret = Some(Collections::iter_ty(*ok.clone()));
                                }
                            }
                        }
                        if Collections::is_closure_method(method)
                            && i == 0
                            && method == "flat_map"
                        {
                            if let Type::Fn {
                                ret: Some(ref r), ..
                            } = gt
                            {
                                if let Type::List(inner) | Type::FixedList { elem: inner, .. } =
                                    r.as_ref()
                                {
                                    refined_ret = Some(Collections::iter_ty((**inner).clone()));
                                }
                            }
                        }
                        // D-PARCAPTURE1=D: para_map → [V].
                        if Collections::is_closure_method(method) && i == 0 && method == "para_map" {
                            if let Type::Fn {
                                ret: Some(ref r), ..
                            } = gt
                            {
                                refined_ret = Some(Type::List(Box::new((**r).clone())));
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
                        if Collections::is_closure_method(method)
                            && i == 0
                            && matches!(method, "group_by" | "count_by")
                        {
                            if let Type::Fn {
                                ret: Some(ref r), ..
                            } = gt
                            {
                                let value = if method == "group_by" {
                                    match recv_ty {
                                        Type::List(inner) | Type::FixedList { elem: inner, .. } => {
                                            Type::List(Box::new((**inner).clone()))
                                        }
                                        _ => Type::List(Box::new(Type::Int)),
                                    }
                                } else {
                                    Type::Int
                                };
                                refined_ret = Some(Type::Map {
                                    key: Box::new((**r).clone()),
                                    key_span: None,
                                    value: Box::new(value),
                                });
                            }
                        }
                        // Skip E0108 for closure methods with ret: None (open return type).
                        let open_ret =
                            matches!(et, Type::Fn { ret: None, .. }) && matches!(gt, Type::Fn { .. });
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
                if Syntax::PARA_METHODS.contains(&method) {
                    let item = match recv_ty {
                        Type::List(inner) | Type::FixedList { elem: inner, .. } => {
                            Some((**inner).clone())
                        }
                        _ => None,
                    };
                    if let Some(item) = item {
                        self.reject_para_type("item", &item, span);
                    }
                    if let Some(result) = refined_ret.clone() {
                        self.reject_para_type("result", &result, span);
                    }
                }
            } else {
                for a in args.iter_mut() {
                    self.with_call_access(&mut call_access, |checker| {
                        let inferred = checker.infer(&mut a.expr);
                        checker.check_call_argument_captures(&a.expr);
                        inferred
                    });
                }
            }
            let _ = span;
            refined_ret
        }
    
}
