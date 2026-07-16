use crate::AST::{Expr, Type};
use crate::Collections;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Bundle::fn_types_compatible;
use crate::Sema::Checker;
use crate::Sema::CheckerCoreLib::wrong_core_arity;
use crate::Sema::Diagnostics::{collection_changed_in_loop, expr_root_ident, type_fix_hint};
use crate::Syntax;
impl<'a> Checker<'a> {
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
                                self.infer(&mut a.expr);
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
                    if method == "zip" && i == 0 {
                        let recv_elem = match recv_ty {
                            Type::List(inner) | Type::FixedList { elem: inner, .. } => {
                                Some((**inner).clone())
                            }
                            _ => None,
                        };
                        let arg_elem = match &got {
                            Some(Type::List(inner)) | Some(Type::FixedList { elem: inner, .. }) => {
                                Some((**inner).clone())
                            }
                            _ => None,
                        };
                        if let (Some(a), Some(b)) = (recv_elem, arg_elem) {
                            refined_ret = Some(Type::List(Box::new(Collections::zip_elem_ty(&a, &b))));
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
                                        refined_ret = Some(Type::List(Box::new((**r).clone())));
                                        let _ = inner;
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
                        // D-FAILCOMP1: filter_map(f: T->V?E) → [V]; refine from closure's ok type.
                        if Collections::is_closure_method(method) && i == 0 && method == "filter_map" {
                            if let Type::Fn {
                                ret: Some(ref r), ..
                            } = gt
                            {
                                if let Type::Result { ok, .. } = r.as_ref() {
                                    refined_ret = Some(Type::List(Box::new(*ok.clone())));
                                }
                            }
                        }
                        // D-AUTOPAR1=A: par_map → [V]; refine V from closure's return type.
                        if Collections::is_closure_method(method) && i == 0 && method == "par_map" {
                            if let Type::Fn {
                                ret: Some(ref r), ..
                            } = gt
                            {
                                refined_ret = Some(Type::List(Box::new((**r).clone())));
                            }
                        }
                        // D-AUTOPAR1=A: par_fold → acc; refine from closure's return type.
                        if Collections::is_closure_method(method) && i == 1 && method == "par_fold" {
                            if let Type::Fn {
                                ret: Some(ref r), ..
                            } = gt
                            {
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
            } else {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
            }
            let _ = span;
            refined_ret
        }
    
}
