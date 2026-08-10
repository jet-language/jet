//! Type inference: fallible forms (`ok`/`err`/`try`/`or`-fallback).
//!
//! Split out of the original `CheckerInfer.rs`; behavior unchanged.

use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{Call, Expr, LambdaBody, OrFallback, Stmt, TryConvert, Type};

impl<'a> Checker<'a> {
    pub(crate) fn infer_ok(&mut self, inner: &mut Box<Expr>, span: Span) -> Option<Type> {
        let payload = self.infer(inner)?;
        if let Some(expected) = self.expected_type.clone() {
            if let Some((ok_ty, err_ty)) = expected.unwrap_result() {
                let ok_payload = payload == *ok_ty
                    || matches!(ok_ty, Type::Union(members) if members.iter().any(|m| m == &payload));
                if !ok_payload {
                    self.diags.push(Diagnostic::error(
                        "E0108",
                        format!(
                            "this `{}` holds {}, but {} was expected",
                            Syntax::LIT_OK,
                            payload.show(),
                            ok_ty.show()
                        ),
                        "the success value must match the result's value type".to_string(),
                        type_fix_hint(ok_ty, &payload),
                        Some(span),
                    ));
                }
                return Some(Type::Result {
                    ok: Box::new(ok_ty.clone()),
                    err: Box::new(err_ty.clone()),
                });
            }
        }
        self.diags.push(Diagnostic::error(
            "E0404",
            format!("`{}(...)` only fits where a fallible result is expected", Syntax::LIT_OK),
            format!(
                "`{}` builds the success side of a `T ? E` result",
                Syntax::LIT_OK
            ),
            "use it in a `T ? E` return type, a `T ? E` binding annotation, or a call that expects one"
                .to_string(),
            Some(span),
        ));
        None
    }

    pub(crate) fn infer_err(&mut self, inner: &mut Box<Expr>, span: Span) -> Option<Type> {
        let mut payload = self.infer(inner)?;
        if let Some(expected) = self.expected_type.clone() {
            if let Some((ok_ty, err_ty)) = expected.unwrap_result() {
                if is_default_error(err_ty) && payload == Type::String {
                    let message = std::mem::replace(inner.as_mut(), Expr::Absent(span));
                    let call = Call {
                        name: Syntax::LIT_ERR.to_string(),
                        name_span: span,
                        type_args: Vec::new(),
                        args: vec![crate::AST::CallArg {
                            convention: crate::AST::AccessConvention::Read,
                            span,
                            expr: message,
                            flags: Default::default(),
                            label: None,
                            spread: false,
                        }],
                        resolved_ret: None,
                        range_checked: false,
                        widen_approx: false,
                    };
                    *inner = Box::new(self.default_err_value(call, false));
                    payload = Type::Named(Syntax::TYPE_ERR.to_string());
                }
                let ok_payload = payload == *err_ty
                    || matches!(err_ty, Type::Union(members) if members.iter().any(|m| m == &payload));
                if !ok_payload {
                    self.diags.push(Diagnostic::error(
                        "E0108",
                        format!(
                            "this `{}` holds {}, but {} was expected",
                            Syntax::LIT_ERR,
                            payload.show(),
                            err_ty.show()
                        ),
                        "the failure value must match the result's error type".to_string(),
                        type_fix_hint(err_ty, &payload),
                        Some(span),
                    ));
                }
                return Some(Type::Result {
                    ok: Box::new(ok_ty.clone()),
                    err: Box::new(err_ty.clone()),
                });
            }
        }
        self.diags.push(Diagnostic::error(
            "E0404",
            format!("`{}(...)` only fits where a fallible result is expected", Syntax::LIT_ERR),
            format!(
                "`{}` builds the failure side of a `T ? E` result",
                Syntax::LIT_ERR
            ),
            "use it in a `T ? E` return type, a `T ? E` binding annotation, or a call that expects one"
                .to_string(),
            Some(span),
        ));
        None
    }

    pub(crate) fn infer_try(
        &mut self,
        inner: &mut Box<Expr>,
        span: Span,
        convert: &mut TryConvert,
    ) -> Option<Type> {
        if let Expr::Call(call) = inner.as_mut() {
            if self.registry.distinct_range(&call.name).is_some() {
                call.range_checked = true;
            }
        }
        let inner_ty = self.infer(inner)?;
        match inner_ty {
            Type::Result { ok, err } => {
                let ret = self.resolve_type(self.ret.clone().unwrap_or(Type::Int));
                match &ret {
                    // E2-M7: error types match — propagate and unwrap the Ok value.
                    // The Ok types (`ret_ok` and `ok`) do NOT need to be equal: `?`
                    // only propagates the error; the unwrapped Ok value may have any
                    // type (it is bound by the caller, not returned unchanged).
                    Type::Result { err: ret_err, .. } if *ret_err == err => {
                        Some((*ok).clone())
                    }
                    // D-FAIL-ERROR1=A: older String-returning callees cross into
                    // the default family through one explicit Prelude conversion.
                    Type::Result { err: ret_err, .. }
                        if is_default_error(ret_err)
                            && matches!(err.as_ref(), Type::String) =>
                    {
                        *convert = TryConvert::DefaultErr;
                        Some((*ok).clone())
                    }
                    // D-UNIONTYPE1=A: member error widens into the return's union.
                    Type::Result { err: ret_err, .. }
                        if ret_err.union_contains(err.as_ref()) =>
                    {
                        let members = ret_err
                            .unwrap_union()
                            .expect("union_contains implies Union");
                        *convert = TryConvert::WidenUnion {
                            enum_name: crate::AST::union_enum_name(members),
                            tag: crate::AST::union_member_tag(&err),
                        };
                        Some((*ok).clone())
                    }
                    Type::Result { err: ret_err, .. } => {
                        let err_type_name = err.name();
                        let ret_err_name = ret_err.name();

                        // D-ERR-CONV: check if a declared `impl Source => Target` conversion exists.
                        if self.trait_reg.has_error_conv(&err_type_name, &ret_err_name) {
                            let fn_name = error_conv_fn_name(&err_type_name, &ret_err_name);
                            *convert = TryConvert::Typed(fn_name);
                            return Some((*ok).clone());
                        }

                        // S80/D-LIB3: check if the error type implements `Fallible`
                        // and the return error is the default `Err`.
                        if is_default_error(ret_err) {
                            if self
                                .trait_reg
                                .implements_trait(&err_type_name, Syntax::TRAIT_FALLIBLE)
                            {
                                // Mark the Try node for Fallible conversion in codegen.
                                *convert = TryConvert::Fallible;
                                return Some((*ok).clone());
                            }
                            // E2402: return is `Err` but the error type has no Fallible impl.
                            let err_name = err.name();
                            self.diags.push(Diagnostic::error(
                                "E2402",
                                format!(
                                    "`?` can't convert `{}` into `{}`",
                                    err_name,
                                    Syntax::TYPE_ERR
                                ),
                                format!(
                                    "`{}` has no path to `{}`; implement `impl {}: {}` to enable conversion",
                                    err_name,
                                    Syntax::TYPE_ERR,
                                    err_name,
                                    Syntax::TRAIT_FALLIBLE
                                ),
                                format!(
                                    "add `impl {}: {} {{ fn to_error(self) => {} {{ … }} }}`, or change the return type",
                                    err_name,
                                    Syntax::TRAIT_FALLIBLE,
                                    Syntax::TYPE_ERR
                                ),
                                Some(span),
                            ));
                            return None;
                        }
                        // E2404: no declared conversion between these two typed error types.
                        self.diags.push(Diagnostic::error(
                            "E2404",
                            format!(
                                "`?` can't turn a `{}` into a `{}` here",
                                err_type_name, ret_err_name
                            ),
                            format!(
                                "`?` only changes an error's type when you've declared how; \
                                 there's no declared way to turn `{}` into `{}`",
                                err_type_name, ret_err_name
                            ),
                            format!(
                                "add `impl {} => {} {{ … }}` before this function",
                                err_type_name, ret_err_name
                            ),
                            Some(span),
                        ));
                        None
                    }
                    _ => {
                        self.diags.push(Diagnostic::error(
                            "E0403",
                            format!(
                                "`{}` only works inside a function that returns a fallible result",
                                Syntax::OP_TRY_SUFFIX
                            ),
                            "propagation early-returns the failure to the caller".to_string(),
                            format!(
                                "add `=> ... ? {}` to this function, or handle the result with `{}`",
                                err.name(),
                                Syntax::OP_FALLBACK
                            ),
                            Some(span),
                        ));
                        None
                    }
                }
            }
            Type::Option(ref inner) => {
                let ret = self.ret.clone().unwrap_or(Type::Int);
                if let Type::Option(ret_inner) = &ret {
                    if **ret_inner == **inner {
                        return Some((**inner).clone());
                    }
                }
                self.diags.push(Diagnostic::error(
                    "E0403",
                    format!(
                        "`{}` on `{}` needs a function that returns the same optional type",
                        Syntax::OP_TRY_SUFFIX,
                        inner_ty.name()
                    ),
                    format!(
                        "propagation passes `{}` back to the caller",
                        Syntax::LIT_NULL
                    ),
                    format!(
                        "add `=> {}` to this function, or handle it with `{}`",
                        inner_ty.name(),
                        Syntax::OP_FALLBACK
                    ),
                    Some(span),
                ));
                None
            }
            other => {
                self.diags.push(Diagnostic::error(
                    "E0403",
                    format!(
                        "`{}` only works on a fallible value, not {}",
                        Syntax::OP_TRY_SUFFIX,
                        other.show()
                    ),
                    "postfix `?` unwraps success or returns early with the failure".to_string(),
                    format!(
                        "call something that returns `T ? E` or an optional value, or remove `{}`",
                        Syntax::OP_TRY_SUFFIX
                    ),
                    Some(span),
                ));
                None
            }
        }
    }

    /// D-CHOOSE-FIND1=A: a finite value loop is not a fallible expression.
    /// Attach its written exhaustion route to the compiler-private result-loop
    /// carrier, then erase the surface `??` before ordinary lowering. Every
    /// execution tier therefore consumes the same result-loop AST.
    pub(crate) fn infer_value_loop_fallback(&mut self, expr: &mut Expr) -> Option<Type> {
        let span = expr.span();
        let Expr::OrFallback {
            value, fallback, ..
        } = std::mem::replace(expr, Expr::Absent(span))
        else {
            unreachable!("value-loop fallback dispatch only accepts OrFallback")
        };
        let mut value = value;
        mark_value_loop_route_attached(&mut value);
        let value_ty = match self.infer(&mut value) {
            Some(ty) => ty,
            None => {
                *expr = *value;
                return None;
            }
        };
        self.reject_borrowed_param_subplace(
            value.as_ref(),
            Some(&value_ty),
            "supply an owned exhaustion-route payload",
        );
        let Some(label) = value_loop_route_label(&mut value) else {
            *expr = *value;
            return Some(value_ty);
        };

        match fallback {
            OrFallback::Value(mut fallback_expr) => {
                let fallback_span = fallback_expr.span();
                let saved = self.expected_type.clone();
                self.expected_type = Some(value_ty.clone());
                let fallback_ty = self.infer(&mut fallback_expr);
                self.expected_type = saved;
                if let Some(fallback_ty) = fallback_ty {
                    if fallback_ty != value_ty {
                        self.diags.push(Diagnostic::error(
                            "E0405",
                            format!(
                                "the fallback is {}, but the loop value is {}",
                                fallback_ty.show(),
                                value_ty.show()
                            ),
                            format!(
                                "both sides of `{}` must be the same type",
                                Syntax::OP_FALLBACK
                            ),
                            type_fix_hint(&value_ty, &fallback_ty),
                            Some(fallback_span),
                        ));
                    }
                }
                attach_value_loop_route(
                    &mut value,
                    Stmt::BreakLabelValue(
                        label,
                        span,
                        *fallback_expr,
                        fallback_span,
                    ),
                );
            }
            OrFallback::Return(mut ret_expr, ret_span) => {
                let ret = self.ret.clone();
                match (&ret, ret_expr.as_mut()) {
                    (Some(ret_ty), Some(e)) => {
                        let saved = self.expected_type.clone();
                        self.expected_type = Some(ret_ty.clone());
                        let expr_ty = self.infer(e);
                        self.expected_type = saved;
                        if let Some(expr_ty) = expr_ty {
                            self.check_type_assignable(ret_ty, &expr_ty, e.span());
                        }
                    }
                    (Some(ret_ty), None) => {
                        self.diags.push(Diagnostic::error(
                            "E0405",
                            format!("`{} return` here needs a value", Syntax::OP_FALLBACK),
                            format!(
                                "a bare `return` needs a value here because the function returns {}",
                                ret_ty.show()
                            ),
                            format!(
                                "give a fallback value: `{} return <value>`",
                                Syntax::OP_FALLBACK
                            ),
                            Some(ret_span),
                        ));
                    }
                    (None, Some(e)) => {
                        self.diags.push(Diagnostic::error(
                            "E0405",
                            format!("`{} return` can't return a value here", Syntax::OP_FALLBACK),
                            "this function returns nothing, so `return` can't carry a value"
                                .to_string(),
                            "drop the value, or add `=> Type` to the function".to_string(),
                            Some(e.span()),
                        ));
                    }
                    (None, None) => {}
                }
                attach_value_loop_route(
                    &mut value,
                    Stmt::Return(ret_expr.map(|e| *e), span),
                );
            }
            OrFallback::Panic { name_span, args } => {
                let mut call = Call {
                    name: Syntax::BUILTIN_PANIC.to_string(),
                    name_span,
                    type_args: Vec::new(),
                    args,
                    resolved_ret: None,
                    range_checked: false,
                    widen_approx: false,
                };
                self.check_panic_call(&mut call);
                attach_value_loop_route(&mut value, Stmt::Expr(Expr::Call(call)));
            }
            route @ (OrFallback::Break(_) | OrFallback::Continue(_)) => {
                let (route, route_span) = match &route {
                    OrFallback::Break(span) => ("break".to_string(), *span),
                    OrFallback::Continue(span) => ("next".to_string(), *span),
                    OrFallback::Value(_) | OrFallback::Return(..) | OrFallback::Panic { .. }
                    | OrFallback::BreakLabel(..)
                    | OrFallback::ContinueLabel(..) => {
                        unreachable!("matched immediate loop-control fallback")
                    }
                };
                mark_value_loop_route_unattached(&mut value);
                self.diags.push(Diagnostic::error(
                    "E0078",
                    format!(
                        "this finite value loop cannot use `{} {}` after its closing brace",
                        Syntax::OP_FALLBACK,
                        route
                    ),
                    "the route would control the loop that just closed; use a labeled loop to name the target".to_string(),
                    "write a labeled search such as `found :: loop { ... break(found, value) }`".to_string(),
                    Some(route_span),
                ));
            }
            OrFallback::BreakLabel(name, route_span) => {
                if self.loop_depth == 0 {
                    mark_value_loop_route_unattached(&mut value);
                    self.diags
                        .push(loop_control_outside(Syntax::KW_BREAK, route_span));
                } else if !self.loop_labels.iter().any(|label| label == &name) {
                    mark_value_loop_route_unattached(&mut value);
                    self.diags.push(crate::Sema::Diagnostics::undefined_loop_label(
                        &name,
                        &self.loop_labels,
                        route_span,
                    ));
                } else {
                    self.check_break_without_value(Some((&name, route_span)), route_span);
                    attach_value_loop_route(&mut value, Stmt::BreakLabel(name, route_span));
                }
            }
            OrFallback::ContinueLabel(name, route_span) => {
                if self.loop_depth == 0 {
                    mark_value_loop_route_unattached(&mut value);
                    self.diags
                        .push(loop_control_outside(Syntax::KW_NEXT, route_span));
                } else if !self.loop_labels.iter().any(|label| label == &name) {
                    mark_value_loop_route_unattached(&mut value);
                    self.diags.push(crate::Sema::Diagnostics::undefined_loop_label(
                        &name,
                        &self.loop_labels,
                        route_span,
                    ));
                } else {
                    attach_value_loop_route(&mut value, Stmt::ContinueLabel(name, route_span));
                }
            }
        }
        *expr = *value;
        Some(value_ty)
    }

    pub(crate) fn infer_or_fallback(
        &mut self,
        value: &mut Box<Expr>,
        fallback: &mut OrFallback,
        span: Span,
        is_option: &mut bool,
    ) -> Option<Type> {
        let val_ty = self.infer(value)?;
        *is_option = matches!(val_ty, Type::Option(_));
        let payload = match &val_ty {
            Type::Result { ok, .. } if !*is_option => (**ok).clone(),
            Type::Option(inner) if *is_option => (**inner).clone(),
            other => {
                self.diags.push(Diagnostic::error(
                    "E0405",
                    format!(
                        "`{}` only works on a fallible value, not {}",
                        Syntax::OP_FALLBACK,
                        other.show()
                    ),
                    "the left side must be a `Result` or optional value".to_string(),
                    format!(
                        "call something that can fail, then write `... {} fallback`",
                        Syntax::OP_FALLBACK
                    ),
                    Some(span),
                ));
                return None;
            }
        };
        self.reject_borrowed_param_subplace(
            value,
            Some(&payload),
            "supply an owned fallback payload",
        );
        match fallback {
            OrFallback::Value(e) => {
                // Infer in place: sema rewrites inside the fallback (index
                // kinds, S25 distribution, field clones) must reach codegen.
                // D-SG9: the fallback shares the success type, so a fixed-width
                // literal fallback (`x ?? 0` where `x` is `U8?`) elaborates to it.
                let saved = self.expected_type.clone();
                self.expected_type = Some(payload.clone());
                let ft = self.infer(e);
                self.expected_type = saved;
                let ft = ft?;
                if ft != payload {
                    self.diags.push(Diagnostic::error(
                        "E0405",
                        format!(
                            "the fallback is {}, but the success value is {}",
                            ft.show(),
                            payload.show()
                        ),
                        format!(
                            "both sides of `{}` must be the same type",
                            Syntax::OP_FALLBACK
                        ),
                        type_fix_hint(&payload, &ft),
                        Some(e.span()),
                    ));
                }
                Some(payload)
            }
            OrFallback::Return(ret_expr, ret_span) => {
                let ret = self.ret.clone();
                match (&ret, ret_expr) {
                    // `?? return value` in a value-returning fn — the value must match.
                    (Some(rt), Some(e)) => {
                        let saved = self.expected_type.clone();
                        self.expected_type = Some(rt.clone());
                        let et = self.infer(e);
                        self.expected_type = saved;
                        if let Some(et) = et {
                            let espan = e.span();
                            self.check_type_assignable(rt, &et, espan);
                        }
                    }
                    // Bare `?? return` in a value-returning fn — rustc would reject the
                    // emitted `return;` (E0069). Reject cleanly: the fn owes a value.
                    (Some(rt), None) => {
                        self.diags.push(Diagnostic::error(
                            "E0405",
                            format!(
                                "`{} return` here needs a value",
                                Syntax::OP_FALLBACK
                            ),
                            format!(
                                "a bare `return` needs a value here because the function returns {}",
                                rt.show()
                            ),
                            format!(
                                "give a fallback value: `{} return <value>`",
                                Syntax::OP_FALLBACK
                            ),
                            Some(*ret_span),
                        ));
                    }
                    // `?? return value` in a unit fn — there's nothing to return.
                    (None, Some(e)) => {
                        self.diags.push(Diagnostic::error(
                            "E0405",
                            format!("`{} return` can't return a value here", Syntax::OP_FALLBACK),
                            "this function returns nothing, so `return` can't carry a value"
                                .to_string(),
                            "drop the value, or add `=> Type` to the function".to_string(),
                            Some(e.span()),
                        ));
                    }
                    // Bare `?? return` in a unit fn — rustc accepts the emitted `return;`.
                    (None, None) => {}
                }
                Some(payload)
            }
            OrFallback::Panic { name_span, args } => {
                let mut call = Call {
                    name: Syntax::BUILTIN_PANIC.to_string(),
                    name_span: *name_span,
                    type_args: Vec::new(),
                    args: std::mem::take(args),
                    resolved_ret: None,
                    range_checked: false,
                    widen_approx: false,
                };
                self.check_panic_call(&mut call);
                *args = call.args;
                Some(payload)
            }
            // D-ORRETURN-ERG1=B: `?? break` / `?? next` — loop-only.
            OrFallback::Break(kw_span) => {
                if self.loop_depth == 0 {
                    self.diags
                        .push(loop_control_outside(Syntax::KW_BREAK, *kw_span));
                }
                Some(payload)
            }
            OrFallback::Continue(kw_span) => {
                if self.loop_depth == 0 {
                    self.diags
                        .push(loop_control_outside(Syntax::KW_NEXT, *kw_span));
                }
                Some(payload)
            }
            OrFallback::BreakLabel(name, span) => {
                if self.loop_depth == 0 {
                    self.diags
                        .push(loop_control_outside(Syntax::KW_BREAK, *span));
                } else if !self.loop_labels.iter().any(|label| label == name) {
                    self.diags.push(crate::Sema::Diagnostics::undefined_loop_label(
                        name,
                        &self.loop_labels,
                        *span,
                    ));
                } else {
                    self.check_break_without_value(Some((name, *span)), *span);
                }
                Some(payload)
            }
            OrFallback::ContinueLabel(name, span) => {
                if self.loop_depth == 0 {
                    self.diags
                        .push(loop_control_outside(Syntax::KW_NEXT, *span));
                } else if !self.loop_labels.iter().any(|label| label == name) {
                    self.diags.push(crate::Sema::Diagnostics::undefined_loop_label(
                        name,
                        &self.loop_labels,
                        *span,
                    ));
                }
                Some(payload)
            }
        }
    }

    pub(crate) fn infer_fallible_stmt(&mut self, expr: &mut Expr) -> Option<Type> {
        self.normalize_imported_core_expr(expr);
        self.normalize_prelude_expr(expr);
        match expr {
            Expr::Call(call) => match self.check_call(call, false) {
                Some(Some(t)) => Some(t),
                _ => None,
            },
            Expr::MethodCall { .. } => self.infer(expr),
            _ => self.infer(expr),
        }
    }
}

pub(crate) fn value_loop_requires_route(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::CallValue { callee, args, .. }
            if args.is_empty()
                && matches!(
                    callee.as_ref(),
                    Expr::Lambda(lam)
                        if lam.meta.result_loop
                            && lam.meta.requires_exhaustion_route
                            && !lam.meta.exhaustion_route_attached
                )
    )
}

fn mark_value_loop_route_attached(expr: &mut Expr) {
    let Expr::CallValue { callee, args, .. } = expr else {
        return;
    };
    if !args.is_empty() {
        return;
    }
    if let Expr::Lambda(lam) = callee.as_mut() {
        lam.meta.exhaustion_route_attached = true;
    }
}

fn mark_value_loop_route_unattached(expr: &mut Expr) {
    let Expr::CallValue { callee, args, .. } = expr else {
        return;
    };
    if !args.is_empty() {
        return;
    }
    if let Expr::Lambda(lam) = callee.as_mut() {
        lam.meta.exhaustion_route_attached = false;
    }
}

fn value_loop_route_label(expr: &mut Expr) -> Option<String> {
    let Expr::CallValue { callee, args, .. } = expr else {
        return None;
    };
    if !args.is_empty() {
        return None;
    }
    let Expr::Lambda(lam) = callee.as_mut() else {
        return None;
    };
    let LambdaBody::Block(stmts) = &mut lam.body else {
        return None;
    };
    let Some(Stmt::Loop {
        label: Some((label, _)),
        ..
    }) = stmts.first()
    else {
        return None;
    };
    Some(label.clone())
}

fn attach_value_loop_route(expr: &mut Expr, route: Stmt) -> bool {
    let Expr::CallValue { callee, args, .. } = expr else {
        return false;
    };
    if !args.is_empty() {
        return false;
    }
    let Expr::Lambda(lam) = callee.as_mut() else {
        return false;
    };
    let LambdaBody::Block(stmts) = &mut lam.body else {
        return false;
    };
    let Some(Stmt::Loop { body, .. }) = stmts.first_mut() else {
        return false;
    };
    body.push(route);
    true
}
