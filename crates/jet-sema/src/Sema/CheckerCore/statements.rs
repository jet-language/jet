use crate::Diagnostics::{Diagnostic, Severity, TextEdit};
use crate::Generics::substitute_type;
use crate::Sema::CheckerCoreLib::{
    is_swizzleable_math_type, parse_swizzle_member, swizzle_write_overlaps, SwizzleParse,
};
use crate::Sema::CheckerTaskGroup::{TaskGroupCtx, TaskGroupOrigin};
use crate::Sema::Diagnostics::{
    aliasing_while_mut, collection_changed_in_loop, collection_root_name,
    computed_field_not_settable, expr_root_ident, is_cloneable, is_task_type, loop_control_outside,
    type_fix_hint, type_requires_owned_iteration, undefined_loop_label,
};
use crate::Sema::Effects::{grant_handle_escape, unknown_effect};
use crate::Sema::Registration::already_defined;
use crate::Sema::{type_is_copy, Checker, LocalInfo, LoopValueFrame, LoopValueKind, ViewAccess};
use crate::Syntax;
use crate::AST::{AccessConvention, Expr, ForKind, IndexKind, LValue, Stmt, StrPart, Type};
use std::collections::HashMap;

/// D-INTDIV1=A: `/` answers the true quotient, so it hands back a Float even
/// for two whole numbers. When that Float is being stored into a whole-number
/// binding, the useful advice is the operator that keeps a whole number — not
/// the generic "drop the decimal part". `n /= 2` reaches here as `n = n / 2`,
/// because compound assignment is desugared before checking, so both spellings
/// get the same fix.
fn division_fix_hint(want: &Type, got: &Type, value: &Expr) -> String {
    let divides = matches!(value, Expr::Binary(crate::AST::BinOp::Div, _, _, _));
    if divides && *want == Type::Int && matches!(got, Type::Float | Type::Float32) {
        // `n /= 2` and `n = n / 2` both reach here, and the two spellings want
        // different repairs, so name each one.
        return "use `/%` to divide and round down (`/%=` in place), \
                or make the binding a Float"
            .to_string();
    }
    type_fix_hint(want, got)
}

fn encoding_reader_item_type(name: &str) -> Option<Type> {
    match name {
        "JSONReader" | "CBORReader" => Some(Type::Named("DataEvent".to_string())),
        "JSONLReader" | "XMLReader" => Some(Type::Named("DataTree".to_string())),
        "CSVReader" => Some(Type::List(Box::new(Type::String))),
        _ => None,
    }
}

fn stream_element_type(ty: Option<&Type>) -> Option<Type> {
    match ty {
        Some(Type::Apply { name, args })
            if name == Syntax::TYPE_STREAM && args.len() == 1 => Some(args[0].clone()),
        _ => None,
    }
}

use super::helpers::layout_constraint_fingerprint;
use std::collections::HashSet;

fn exits_current_block(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::Return(..)
            | Stmt::Break(..)
            | Stmt::BreakValue(..)
            | Stmt::BreakLabel(..)
            | Stmt::BreakLabelValue(..)
            | Stmt::Continue(..)
            | Stmt::ContinueLabel(..)
    ) || matches!(
        stmt,
        Stmt::Expr(expr)
            if matches!(expr.without_parens(), Expr::Todo { .. })
                || matches!(
                    expr.without_parens(),
                    Expr::Call(call) if call.name == Syntax::BUILTIN_PANIC
                )
    )
}

/// A canonical `while handles.len() > 0` drain proves a linear collection is
/// empty when control reaches the next statement. Keep this proof structural:
/// the first body statement must pop that same collection, and neither its
/// fallback nor any later straight-line statement may touch the collection.
fn drained_collection<'a>(
    cond: &'a Expr,
    body: &'a [Stmt],
) -> Option<(&'a str, crate::Diagnostics::Span)> {
    let Expr::Binary(crate::AST::BinOp::Gt, lhs, rhs, _) = cond.without_parens() else {
        return None;
    };
    let Expr::MethodCall {
        receiver,
        method,
        args,
        ..
    } = lhs.without_parens()
    else {
        return None;
    };
    let Expr::Ident(name, name_span) = receiver.without_parens() else {
        return None;
    };
    if method != "len"
        || !args.is_empty()
        || !matches!(rhs.without_parens(), Expr::Int(0, ..))
        || !body
            .iter()
            .all(|stmt| matches!(stmt, Stmt::Val(_) | Stmt::Expr(_)))
        || body
            .iter()
            .skip(1)
            .any(|stmt| crate::Sema::Captures::stmt_refs_name(stmt, name))
    {
        return None;
    }
    let Stmt::Val(binding) = body.first()? else {
        return None;
    };
    let value = match binding.init.without_parens() {
        Expr::OrFallback {
            value, fallback, ..
        } => {
            if crate::Sema::Captures::fallback_refs_name(fallback, name) {
                return None;
            }
            value.without_parens()
        }
        value => value,
    };
    let Expr::MethodCall {
        receiver: popped,
        method,
        args,
        ..
    } = value
    else {
        return None;
    };
    if method == "pop"
        && args.is_empty()
        && matches!(popped.without_parens(), Expr::Ident(popped, _) if popped == name)
    {
        Some((name, *name_span))
    } else {
        None
    }
}

impl<'a> Checker<'a> {
    fn push_loop_value_frame(&mut self, label: Option<&(String, crate::Diagnostics::Span)>) {
        let (kind, pending_label) = self
            .pending_loop_value
            .take()
            .unwrap_or((LoopValueKind::Effect, None));
        self.loop_value_frames.push(LoopValueFrame {
            label: pending_label.or_else(|| label.map(|(name, _)| name.clone())),
            kind,
            ty: None,
        });
    }

    fn pop_loop_value_frame(&mut self) {
        if let Some(frame) = self.loop_value_frames.pop() {
            if frame.kind == LoopValueKind::Result {
                self.last_loop_result_type = frame.ty;
            }
        }
    }

    fn push_loop_break_frame(&mut self) {
        self.loop_break_flows.push(Vec::new());
    }

    fn record_break_flow(&mut self, frame_index: usize) {
        let flow = self.flow.clone();
        if let Some(paths) = self.loop_break_flows.get_mut(frame_index) {
            paths.push(flow);
        }
    }

    fn join_loop_flow(
        &mut self,
        before_loop: &crate::Sema::FlowFacts::FlowFacts,
        after_body: &crate::Sema::FlowFacts::FlowFacts,
        may_skip: bool,
    ) {
        let mut break_paths = self
            .loop_break_flows
            .pop()
            .expect("loop break frame matches loop value frame");
        let depth = self.flow.depth;
        for path in &mut break_paths {
            while path.depth > depth {
                path.leave_scope();
            }
        }
        if may_skip {
            self.flow = crate::Sema::FlowFacts::FlowFacts::after_loop_with_breaks(
                before_loop,
                after_body,
                &break_paths,
            );
        } else {
            break_paths.retain(|path| path.reachable);
            self.flow = if break_paths.is_empty() {
                let mut exited = before_loop.clone();
                exited.reachable = false;
                exited
            } else {
                crate::Sema::FlowFacts::FlowFacts::merge_paths(before_loop, &break_paths)
            };
        }
    }

    fn check_break_value(
        &mut self,
        target: Option<(&str, crate::Diagnostics::Span)>,
        value: &mut Expr,
        span: crate::Diagnostics::Span,
    ) {
        let frame_index = match target {
            Some((name, name_span)) => {
                let found = self
                    .loop_value_frames
                    .iter()
                    .rposition(|frame| frame.label.as_deref() == Some(name));
                if found.is_none() {
                    self.diags
                        .push(undefined_loop_label(name, &self.loop_labels, name_span));
                    self.infer(value);
                    return;
                }
                found
            }
            None => self.loop_value_frames.len().checked_sub(1),
        };
        let Some(frame_index) = frame_index else {
            self.diags
                .push(loop_control_outside(Syntax::KW_BREAK, span));
            self.infer(value);
            return;
        };
        let kind = self.loop_value_frames[frame_index].kind;
        let got = self.infer(value).map(|ty| self.resolve_type(ty));
        self.note_move_if_direct_ident(value);
        match kind {
                LoopValueKind::Collecting => self.diags.push(Diagnostic::error(
                    "E0075",
                    "this collecting loop cannot use a break payload".to_string(),
                    "its result is already the accumulated List, so a second payload would give one exit two result channels".to_string(),
                    "write `break` to return the accumulated list".to_string(),
                    Some(span),
                )),
                LoopValueKind::Effect => self.diags.push(Diagnostic::error(
                    "E0079",
                    "this effect-only loop uses a result exit".to_string(),
                    "a break payload makes an effect-only loop a value expression".to_string(),
                    "bind the loop with `::`, or remove the payload".to_string(),
                    Some(span),
                )),
                LoopValueKind::Result => {
                    if let Some(got) = got {
                        let frame = &mut self.loop_value_frames[frame_index];
                        if let Some(expected) = &frame.ty {
                            if expected != &got {
                                self.diags.push(Diagnostic::error(
                                    "E0076",
                                    format!(
                                        "this loop breaks with {}, but another exit uses {}",
                                        got.show(),
                                        expected.show()
                                    ),
                                    "an ordinary loop has one final result type".to_string(),
                                    "make every break payload use the same type".to_string(),
                                    Some(value.span()),
                                ));
                            }
                        } else {
                            frame.ty = Some(got);
                        }
                    }
                }
            }
        self.record_break_flow(frame_index);
    }

    pub(in crate::Sema) fn check_break_without_value(
        &mut self,
        target: Option<(&str, crate::Diagnostics::Span)>,
        span: crate::Diagnostics::Span,
    ) {
        let frame_index = match target {
            Some((name, name_span)) => {
                let found = self
                    .loop_value_frames
                    .iter()
                    .rposition(|frame| frame.label.as_deref() == Some(name));
                if found.is_none() {
                    self.diags
                        .push(undefined_loop_label(name, &self.loop_labels, name_span));
                    return;
                }
                found
            }
            None => self.loop_value_frames.len().checked_sub(1),
        };
        let Some(frame_index) = frame_index else {
            self.diags
                .push(loop_control_outside(Syntax::KW_BREAK, span));
            return;
        };
        let frame = &self.loop_value_frames[frame_index];
        if frame.kind == LoopValueKind::Result {
            self.diags.push(Diagnostic::error(
                "E0076",
                "this result loop exits without a value".to_string(),
                "every exit from a loop used as a value must provide the same result type"
                    .to_string(),
                "add a break payload, or target an inner effect-only loop".to_string(),
                Some(span),
            ));
        }
        self.record_break_flow(frame_index);
    }

    fn compound_owner_field_type(&self, owner: &Type, field: &str) -> Option<Type> {
        let (owner_name, subst) = match owner {
            Type::Named(name) => (name.as_str(), HashMap::new()),
            Type::Apply { name, args } => {
                let params = self.trait_reg.struct_params.get(name)?;
                let subst = params
                    .iter()
                    .zip(args)
                    .map(|(param, arg)| (param.name.clone(), arg.clone()))
                    .collect();
                (name.as_str(), subst)
            }
            Type::Tagged { inner, .. } => return self.compound_owner_field_type(inner, field),
            _ => return None,
        };
        self.registry
            .struct_fields(owner_name)?
            .iter()
            .find(|(name, _, _)| name == field)
            .map(|(_, _, ty)| substitute_type(ty, &subst))
    }

    pub(in crate::Sema) fn compound_expr_type(&self, expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Ident(name, _) => self.lookup(name).map(|info| info.ty.clone()),
            Expr::Field(base, field, _) => self.compound_field_type(base, field),
            Expr::Index { base, .. } => match self.compound_expr_type(base)? {
                Type::List(elem) | Type::FixedList { elem, .. } => Some(*elem),
                Type::Map { value, .. } => Some(*value),
                _ => None,
            },
            _ => None,
        }
    }

    /// Place type for assignment expected-type flow (`holder.value = .{…}`).
    fn lvalue_type(&self, target: &LValue) -> Option<Type> {
        match target {
            LValue::Local { name, .. } => {
                self.lookup(name).map(|info| info.ty.clone()).or_else(|| {
                    self.is_persist_binding(name)
                        .then(|| self.consts.get(name).cloned())
                        .flatten()
                })
            }
            LValue::Field { base, field, .. } => self.compound_field_type(base, field),
            LValue::Index { base, .. } => match self.compound_expr_type(base)? {
                Type::List(elem) | Type::FixedList { elem, .. } => Some(*elem),
                Type::Map { value, .. } => Some(*value),
                _ => None,
            },
        }
    }

    fn compound_field_type(&self, base: &Expr, field: &str) -> Option<Type> {
        let owner = self.compound_expr_type(base)?;
        self.compound_owner_field_type(&owner, field)
    }

    fn compound_type_implements(&self, ty: &Type, trait_name: &str) -> bool {
        self.trait_reg.type_implements_trait(ty, trait_name)
            || matches!(ty, Type::Named(name)
            if self.type_param_scope.iter().any(|param| {
                param.name == *name && param.bounds.iter().any(|bound| bound == trait_name)
            }))
    }

    /// True when compound assign on `target` is rejected (E0164 / E0362), so
    /// L0503 must not recommend it.
    fn compound_assign_rejected(&self, target: &LValue, op: crate::AST::BinOp) -> bool {
        match target {
            LValue::Index { .. } => true,
            LValue::Local { .. } => false,
            LValue::Field { base, field, .. } => {
                let trait_name = match op {
                    crate::AST::BinOp::Add => Some(Syntax::TRAIT_ADD),
                    crate::AST::BinOp::Sub => Some(Syntax::TRAIT_SUB),
                    crate::AST::BinOp::Mul => Some(Syntax::TRAIT_MUL),
                    crate::AST::BinOp::Div => Some(Syntax::TRAIT_DIV),
                    _ => None,
                };
                trait_name.is_some_and(|trait_name| {
                    self.compound_field_type(base, field).is_some_and(|ty| {
                        self.compound_type_implements(&ty, trait_name)
                            && match base.as_ref() {
                                Expr::Ident(..) => false,
                                Expr::Index { .. } => {
                                    matches!(ty, Type::Named(_) | Type::Apply { .. })
                                }
                                _ => true,
                            }
                    })
                })
            }
        }
    }

    /// Check two alternative branches with independent move states, then
    /// keep the union (a value moved in either branch counts as gone).
    pub(crate) fn check_stmt(&mut self, stmt: &mut Stmt) {
        if !self.enter_source_nesting(stmt.span()) {
            return;
        }
        let before = self.flow.clone();
        let diagnostics_start = self.diags.len();
        let allows_start = self.statement_lint_allows.len();
        self.check_stmt_inner(stmt);
        let allows = self.statement_lint_allows.split_off(allows_start);
        if !allows.is_empty() {
            let retained = self.diags.split_off(diagnostics_start);
            self.diags.extend(retained.into_iter().filter(|diagnostic| {
                diagnostic.severity != Severity::Lint
                    || allows.iter().all(|name| {
                        jet_foundation::LintPolicy::name_for_code(&diagnostic.code)
                            != Some(name.as_str())
                    })
            }));
        }
        if !before.reachable {
            // Unreachable source is still checked for diagnostics, but it
            // is not a path that may contribute facts to a later join.
            self.flow = before;
        } else if exits_current_block(stmt) {
            self.flow.reachable = false;
        }
        self.leave_source_nesting();
    }

    pub(crate) fn check_return_expr(
        &mut self,
        expr: &mut Option<Expr>,
        span: &crate::Diagnostics::Span,
        return_type: Option<Type>,
    ) {
        // D-ENC-DYN1=A+: the declared return type may be a `Data` alias
        // (`JSON`/`TOML`/…); canonicalize it so it unifies with the returned value.
        let resolved_ret = return_type
            .clone()
            .or_else(|| self.ret.clone())
            .map(|t| self.resolve_type(t));
        // D-STREAMYIELD1: a generator (`Stream<T> ->`) yields values; `return`
        // only ever ends the stream early — bare `return;` is fine, `return
        // value;` is E0806 (a generator body yields, it doesn't return a value).
        if stream_element_type(
            return_type
                .as_ref()
                .or(self.declared_return_type.as_ref()),
        )
        .is_some()
        {
            if let Some(e) = expr {
                self.infer(e);
                self.diags.push(Diagnostic::error(
                    "E0806",
                    format!(
                        "`{}` yields values, so `return` can't carry one",
                        self.fn_name
                    ),
                    "a generator body produces values with `yield`; `return` only ends the stream early".to_string(),
                    "write `yield ...;` to hand back a value, or a bare `return;` to end the stream".to_string(),
                    Some(e.span()),
                ));
            }
            return;
        }
        match (&mut *expr, resolved_ret) {
            (Some(e), Some(rt)) => {
                // A direct `return value?` in a fallible function returns
                // the propagated success value. Desugar it through the
                // existing `Ok` constructor so inference checks the success
                // payload while `infer_try` still records the error conversion.
                if matches!(&rt, Type::Result { .. }) && matches!(e.without_parens(), Expr::Try(..))
                {
                    let span = e.span();
                    let inner = std::mem::replace(e, Expr::Absent(span));
                    *e = Expr::Ok(Box::new(inner), span);
                }
                let string_view_return = matches!(
                    &rt,
                    Type::Apply { name, args }
                        if name == "View"
                            && matches!(args.as_slice(), [Type::Named(inner)] if inner == "str")
                );
                // D-SHAPE-PLACE1=A: a bare maximal place is a read
                // window. At a named `View<T>` return boundary, make
                // that local acquisition explicit in the AST before
                // inference; E2305 then checks today's provenance gate.
                let bare_place_return = {
                    let transparent = e.without_parens();
                    matches!(&rt, Type::Apply { name, .. } if name == "View")
                        && !string_view_return
                        && !matches!(transparent, Expr::Copy(..) | Expr::Place(..))
                        && self.place_from_expr(transparent).is_some()
                };
                if bare_place_return {
                    let span = e.span();
                    let inner = std::mem::replace(e, Expr::Absent(span));
                    *e = Expr::Place(Box::new(inner), crate::AST::PlaceAccess::Read, span);
                }
                let saved_expected = self.expected_type.clone();
                // D-FAILURE-FOUNDATION1=A: a direct fallible call in a return
                // position uses the ordinary transparent-carrier route. Give
                // the call its success expectation so `infer` elaborates the
                // existing `Try` node and applies the declared error
                // conversion, then the return checker lifts that success
                // value back into the caller's carrier.
                let contextual_result_constructor = matches!(
                    e.without_parens(),
                    Expr::Call(call)
                        if !self.funcs.contains_key(&call.name)
                            && matches!(call.name.as_str(), Syntax::LIT_OK | Syntax::LIT_ERR)
                );
                let call_success_type = (!contextual_result_constructor
                    && matches!(
                        e.without_parens(),
                        Expr::Call(..) | Expr::MethodCall { .. } | Expr::CallValue { .. }
                    ))
                .then(|| match &rt {
                    Type::Result { ok, .. } => ok.as_ref().clone(),
                    _ => rt.clone(),
                });
                self.expected_type = Some(call_success_type.unwrap_or_else(|| rt.clone()));
                // Spawned task returns are checked separately by E1102.
                self.borrow_ctx = self.is_task_spawn;
                let saved_string_view_read = self.allow_string_view_read;
                if string_view_return {
                    self.allow_string_view_read = true;
                }
                // D-ALLOC2 / E0631: capture the returned name BEFORE inferring.
                // An arena view carries an allocator-view type, so returning it
                // where the declared type is the payload rewrites `e` through a
                // deref coercion and it stops being an `Ident`. The escape check
                // below then silently missed every arena view that escaped.
                let returned_ident = match &*e {
                    Expr::Ident(name, span) => Some((name.clone(), *span)),
                    _ => None,
                };
                let mut et = self.infer(e);
                        // D-FAILURE-FOUNDATION1=A: the public return contract
                        // is the shared Result carrier, but source authors
                        // return its success payload directly. Preserve an
                        // already-carried result (calls, `Ok`, `Err`, and
                        // explicit propagation); lift every matching payload
                        // through the existing `Ok` constructor.
                        if let (Type::Result { ok, .. }, Some(actual)) = (&rt, et.as_ref()) {
                            let payload_matches = actual == ok.as_ref()
                                || matches!(ok.as_ref(), Type::Union(members) if members.iter().any(|member| member == actual))
                                || actual.numeric_widening_to(ok).is_some();
                            if payload_matches && !matches!(actual, Type::Result { .. }) {
                                let value_span = e.span();
                                let value = std::mem::replace(e, Expr::Absent(value_span));
                                *e = Expr::Ok(Box::new(value), value_span);
                                self.expected_type = Some(rt.clone());
                                // The payload was already inferred against the
                                // carrier's success type above. The `Ok` node is
                                // compiler-generated, so re-inferring it would
                                // spend one extra source-nesting level at the
                                // published boundary.
                                et = Some(rt.clone());
                            }
                        }
                        self.report_lending_view_escape(e, "be returned");
                self.allow_string_view_read = saved_string_view_read;
                self.expected_type = saved_expected;
                if let Some(source) = et.as_ref() {
                    if source != &rt && source.numeric_widening_to(&rt).is_some() {
                        let source = source.clone();
                        self.widen_numeric_expr(e, &source, &rt);
                        et = Some(rt.clone());
                    }
                }
                // #1164: direct `View`/`ViewMut` returns use the
                // dedicated path below. Aggregates that contain view
                // fields need the walk. Non-view returns must not
                // re-check string-view idents as view escapes — the
                // E2307 "needs owned String" path already teaches.
                let direct_view_return = matches!(
                    &rt,
                    Type::Apply { name, .. }
                        if matches!(name.as_str(), "View" | "ViewMut")
                );
                if !direct_view_return && self.type_contains_view_boundary(&rt) {
                    self.check_aggregate_view_return(e);
                }
                // D-ALLOC2: E0631 — returning an arena `view` would let
                // it outlive the arena (the arena drops at scope end).
                if let Some((n, nspan)) = &returned_ident {
                    if self.is_arena_view(n) || self.is_fixed_backing_view(n) {
                        self.report_view_escape(n, "be returned", *nspan);
                    }
                }
                // D-DYNARRAY1: E2305 — returning a `View<T>` whose owner
                // list is local to this function would outlive it. Two
                // shapes: an already-bound view name (`return window`),
                // and a fresh range place made right in the
                // `return` (`return incidents[0..2]`) — the latter
                // needs `view_call_source` directly.
                if string_view_return {
                    if let Expr::Ident(n, nspan) = &*e {
                        if self.is_string_view(n) {
                            self.check_named_string_view_binding_return(n, *nspan);
                        } else {
                            self.report_string_view_boundary(e.span());
                        }
                    } else {
                        let sources: Vec<_> = self
                            .view_call_sources(e)
                            .into_iter()
                            .filter(|(path, ..)| path.is_empty())
                            .collect();
                        if sources.is_empty() {
                            self.report_string_view_boundary(e.span());
                        } else {
                            for (_, place, _, access) in sources {
                                self.check_named_view_return(&place, access, Vec::new(), e.span());
                            }
                        }
                    }
                } else if matches!(&rt, Type::Apply { name, .. } if matches!(name.as_str(), "View" | "ViewMut"))
                {
                    if let Expr::Ident(n, nspan) = &*e {
                        if self.is_list_view(n) {
                            self.check_named_view_binding_return(n, *nspan);
                        } else {
                            self.report_view_return_boundary(e.span());
                        }
                    } else {
                        let sources: Vec<_> = self
                            .view_call_sources(e)
                            .into_iter()
                            .filter(|(path, ..)| path.is_empty())
                            .collect();
                        if sources.is_empty() {
                            self.report_view_return_boundary(e.span());
                        } else {
                            for (_, place, _, access) in sources {
                                self.check_named_view_return(&place, access, Vec::new(), e.span());
                            }
                        }
                    }
                }
                // D-MEM-COPYSEM1: default owning returns have already
                // become `Expr::Copy` in `self.infer(e)`. Declared
                // view returns still use the boundary checks above;
                // an explicit-copy policy leaves the original read
                // in place so its registered E2307 refusal remains.
                // Returning a borrowed parameter would move out of a
                // borrow in the generated Rust (I2) — require a copy.
                self.reject_borrowed_param_subplace(
                    e,
                    et.as_ref(),
                    "be returned as an owned value",
                );
                if et
                    .as_ref()
                    .is_some_and(crate::Sema::Diagnostics::contains_expiring_secret_loan)
                {
                    self.diags.push(Diagnostic::error(
                        "E0201",
                        "an ExpiringSecret loan cannot be returned".to_string(),
                        "the callback parameter is valid only while `.with` is running".to_string(),
                        "return a non-secret result computed from the loan".to_string(),
                        Some(e.span()),
                    ));
                }
                if let Expr::Ident(n, nspan) = &*e {
                    if let Some(info) = self.lookup(n) {
                        if !info.ty.is_scalar()
                            && !matches!(
                                &info.ty,
                                Type::Named(name)
                                    if self
                                        .type_param_scope
                                        .iter()
                                        .any(|param| &param.name == name)
                            )
                            && matches!(
                                info.param_conv,
                                Some(AccessConvention::Read) | Some(AccessConvention::Write)
                            )
                        {
                            let display_type = self.display_type(&info.ty).name();
                            self.diags.push(Diagnostic::error(
                                            "E0120",
                                            format!(
                                                "`{}` was not moved here, so it cannot be returned as-is",
                                                n
                                            ),
                                            "this function has read access only and does not own the value"
                                                .to_string(),
                                            format!(
                                                "return a copy: `return {}{};` — or take ownership with the move marker `^`: `{}: {}{}`. \
                                                 There's no borrow-return in v1 — to share the value without a full \
                                                 copy, store an owned field, or reach for `Shared<T>`/`Id<T>` \
                                                 once a real program needs shared ownership",
                                                Syntax::SIGIL_COPY,
                                                n,
                                                n,
                                                Syntax::SIGIL_MOVE,
                                                display_type
                                            ),
                                            Some(*nspan),
                                        ));
                        }
                    }
                }
                if !string_view_return {
                    self.note_move_if_direct_ident(e);
                }
                if let Some(et) = et {
                    let http_handler_lambda = matches!(
                        (&rt, &et),
                        (Type::Named(name), Type::Fn { params, ret: Some(ret), .. })
                            if name == "HTTPHandler"
                                && params == &vec![Type::Named("HTTPRequest".to_string())]
                                && ret.as_ref() == &Type::Result {
                                    ok: Box::new(Type::Named("HTTPResponse".to_string())),
                                    err: Box::new(Type::Named("HTTPError".to_string())),
                                }
                    );
                    let string_view_compatible = string_view_return && et == Type::String;
                    let union_member_widen = matches!(
                        &rt,
                        Type::Union(members) if members.iter().any(|m| m == &et)
                    );
                    // D-APILABEL1=A: every return contract uses
                    // shared assignability, including qualified
                    // nominal types.
                    let reported = self.check_type_assignable(&rt, &et, e.span());
                    if et != rt
                        && !reported
                        && !http_handler_lambda
                        && !string_view_compatible
                        && !union_member_widen
                    {
                        let display_return = self.display_type(&rt);
                        let display_actual = self.display_type(&et);
                        self.diags.push(Diagnostic::error(
                            "E0113",
                            format!(
                                "`{}` promises to return {}, but this returns {}",
                                self.fn_name,
                                display_return.show(),
                                display_actual.show()
                            ),
                            "the value handed back must match the declared return type".to_string(),
                            type_fix_hint(&display_return, &display_actual),
                            Some(e.span()),
                        ));
                    }
                }
            }
            (Some(e), None) => {
                let ty_name = self.infer_name_or(e, "Int");
                self.diags.push(Diagnostic::error(
                                "E0113",
                                format!("`{}` doesn't return a value", self.fn_name),
                                "a function only hands back a value if it declares a return type before `->`"
                                    .to_string(),
                                format!(
                                    "remove the value (`return;`), or declare `{}` as the return type before `->`",
                                    ty_name
                                ),
                                Some(e.span()),
                            ));
            }
            (None, Some(rt)) => {
                // D-FAIL-EXIT1: implicit `fn run` has unit success and the
                // default `Err` failure contract.
                // A bare `return` is successful exit, same as falling off
                // the end of the body.
                let fallible_void = matches!(
                    rt,
                    Type::Result { ref ok, .. }
                        if matches!(ok.as_ref(), Type::Named(n) if n == Syntax::INTERNAL_UNIT_TYPE)
                );
                if !fallible_void {
                    self.diags.push(Diagnostic::error(
                        "E0113",
                        format!(
                            "`{}` promises to return {}, but this `return` is empty",
                            self.fn_name,
                            rt.show()
                        ),
                        "the value handed back must match the declared return type".to_string(),
                        "add the value: `return ...;`".to_string(),
                        Some(*span),
                    ));
                }
            }
            (None, None) => {}
        }
    }

    fn check_stmt_inner(&mut self, stmt: &mut Stmt) {
        // D-CONC-SHARE1=A (card #1561): a plain field write on a
        // `Shared<T>` handle is one atomic locked step. Rewrite before the
        // compound-assignment expansion below, so a read-modify-write
        // stays inside one lock and cannot lose an update.
        if self.desugar_shared_field_write(stmt) {
            self.check_stmt_inner(stmt);
            return;
        }
        if let Some(mut marker) = self.take_statement_rule_fact(stmt.span()) {
            let allow_names = if marker.name == Syntax::MARKER_ALLOW {
                marker
                    .args
                    .iter()
                    .filter_map(|argument| match argument {
                        Expr::Ident(name, _) => Some(name.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            if let Some(arguments) = self.validate_rule_signature(&mut marker) {
                if marker.name == Syntax::MARKER_ALLOW {
                    self.statement_lint_allows.extend(allow_names);
                }
                let text = match arguments.constant_for_source(0) {
                    Some(crate::Comptime::CtValue::Str(value)) => Some(value.clone()),
                    _ => None,
                };
                match stmt {
                    Stmt::Unsafe {
                        audit, audit_expr, ..
                    } if marker.name == Syntax::KW_UNSAFE => {
                        *audit = text;
                        *audit_expr = marker.args.into_iter().next();
                    }
                    Stmt::Impure {
                        reason,
                        reason_expr,
                        ..
                    } if marker.name == Syntax::KW_IMPURE => {
                        *reason = text;
                        *reason_expr = marker.args.into_iter().next();
                    }
                    Stmt::AssumeDet {
                        reason,
                        reason_expr,
                        ..
                    } if marker.name == Syntax::MARKER_NONDETERMINISTIC => {
                        if let Some(text) = text {
                            *reason = text;
                        }
                        if let Some(argument) = marker.args.into_iter().next() {
                            *reason_expr = argument;
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Stmt::While { span, .. }
        | Stmt::For { span, .. }
        | Stmt::Loop { span, .. }
        | Stmt::CountedLoop { span, .. } = stmt
        {
            self.fx_memory_unbounded_control.push(*span);
            for region in &mut self.memory_policy_stack {
                region.unbounded_control.push(*span);
            }
        }
        if let Stmt::Assign {
            target: LValue::Field { base, field, span },
            op: Some(op),
            value,
            ..
        } = stmt
        {
            let trait_name = match op {
                crate::AST::BinOp::Add => Some(Syntax::TRAIT_ADD),
                crate::AST::BinOp::Sub => Some(Syntax::TRAIT_SUB),
                crate::AST::BinOp::Mul => Some(Syntax::TRAIT_MUL),
                crate::AST::BinOp::Div => Some(Syntax::TRAIT_DIV),
                _ => None,
            };
            let nested_hook = trait_name.is_some_and(|trait_name| {
                self.compound_field_type(base, field).is_some_and(|ty| {
                    self.compound_type_implements(&ty, trait_name)
                        && match base.as_ref() {
                            Expr::Ident(..) => false,
                            Expr::Index { .. } => {
                                matches!(ty, Type::Named(_) | Type::Apply { .. })
                            }
                            _ => true,
                        }
                })
            });
            if nested_hook {
                self.diags.push(Diagnostic::error(
                    "E0362",
                    "compound assignment can't target a nested operator field".to_string(),
                    "the current operator-place lowering supports one named owner and one field"
                        .to_string(),
                    "bind the inner value, update it, then assign the whole inner value back"
                        .to_string(),
                    Some(*span),
                ));
                let saved_expected = self.expected_type.clone();
                if let Some(place_ty) = self.compound_field_type(base, field) {
                    self.expected_type = Some(place_ty);
                }
                self.infer(value);
                self.expected_type = saved_expected;
                return;
            }
        }
        // S17 / L0503: prefer `place += …` over `place = place + …`.
        // Must run before the hooked compound rewrite below, which expands
        // intentional `+=` into `place = place + …` and would false-positive.
        if let Stmt::Assign {
            target,
            op: None,
            op_span,
            value,
        } = &*stmt
        {
            if let Some((place, bin_op, compound)) = prefer_compound_assign(target, value) {
                if !self.compound_assign_rejected(target, bin_op) {
                    self.diags.push(Diagnostic::from_row(
                        "L0503",
                        &[("place", place.as_str()), ("op", compound)],
                        Some(*op_span),
                    ));
                }
            }
        }
        // D-OPDEF1=A: compound arithmetic is the same hook as its binary
        // form. Rewrite before checking so TIR/JIT see one MethodCall spine.
        let compound = match &*stmt {
            Stmt::Assign {
                target: LValue::Local { name, name_span },
                op: Some(op),
                ..
            } => self
                .lookup(name)
                .map(|info| info.ty.clone())
                .or_else(|| {
                    self.is_persist_binding(name)
                        .then(|| self.consts.get(name).cloned())
                        .flatten()
                })
                .and_then(|ty| {
                    let trait_name = match op {
                        crate::AST::BinOp::Add => Some(Syntax::TRAIT_ADD),
                        crate::AST::BinOp::Sub => Some(Syntax::TRAIT_SUB),
                        crate::AST::BinOp::Mul => Some(Syntax::TRAIT_MUL),
                        crate::AST::BinOp::Div => Some(Syntax::TRAIT_DIV),
                        _ => None,
                    }?;
                    self.compound_type_implements(&ty, trait_name).then_some((
                        Expr::Ident(name.clone(), *name_span),
                        *name_span,
                        *op,
                    ))
                }),
            Stmt::Assign {
                target: LValue::Field { base, field, span },
                op: Some(op),
                ..
            } => (|| {
                if !matches!(base.as_ref(), Expr::Ident(..) | Expr::Index { .. }) {
                    return None;
                }
                let field_ty = self.compound_field_type(base, field)?;
                if matches!(base.as_ref(), Expr::Index { .. })
                    && matches!(&field_ty, Type::Named(_) | Type::Apply { .. })
                {
                    return None;
                }
                let trait_name = match op {
                    crate::AST::BinOp::Add => Some(Syntax::TRAIT_ADD),
                    crate::AST::BinOp::Sub => Some(Syntax::TRAIT_SUB),
                    crate::AST::BinOp::Mul => Some(Syntax::TRAIT_MUL),
                    crate::AST::BinOp::Div => Some(Syntax::TRAIT_DIV),
                    _ => None,
                }?;
                self.compound_type_implements(&field_ty, trait_name)
                    .then_some((Expr::Field(base.clone(), field.clone(), *span), *span, *op))
            })(),
            _ => None,
        };
        if let Some((left, span, binary_op)) = compound {
            if let Stmt::Assign { op, value, .. } = stmt {
                let rhs = std::mem::replace(value, Expr::Absent(span));
                *value = Expr::Binary(binary_op, Box::new(left), Box::new(rhs), span);
                *op = None;
            }
        }
        match stmt {
            Stmt::Val(b) => {
                self.check_binding(b);
                crate::Sema::Effects::record_authority_alias(self, b);
            }
            Stmt::Assign {
                target,
                op,
                op_span: _,
                value,
            } => {
                let is_compound = op.is_some();
                if let (Some(compound_op), LValue::Index { span, .. }) = (op.as_ref(), &*target) {
                    self.diags.push(Diagnostic::from_row(
                        "E0164",
                        &[("op", (*compound_op).spell())],
                        Some(*span),
                    ));
                    let saved_expected = self.expected_type.clone();
                    if let Some(place_ty) = self.lvalue_type(target) {
                        self.expected_type = Some(place_ty);
                    }
                    self.infer(value);
                    self.expected_type = saved_expected;
                    return;
                }
                let origin_write_root = match &*target {
                    LValue::Local { name, .. } => Some(name.clone()),
                    LValue::Field { base, .. } | LValue::Index { base, .. } => {
                        expr_root_ident(base).map(str::to_string)
                    }
                };
                if let LValue::Local { name, .. } = &*target {
                    self.mark_local_write(name);
                }
                self.check_lvalue_change(target, "be assigned");
                self.validate_shared_guard_lvalue(target);
                // Beginner magic: place type feeds `.{…}` / `.Variant` on the RHS.
                let saved_expected = self.expected_type.clone();
                let place_ty = self.lvalue_type(target);
                if let Some(place_ty) = &place_ty {
                    self.expected_type = Some(place_ty.clone());
                }
                let mut vt = self.infer(value);
                self.expected_type = saved_expected;
                self.report_lending_view_escape(value, "replace another value");
                if let (Some(source), Some(target_ty)) = (vt.as_ref(), place_ty.as_ref()) {
                    if source != target_ty && source.numeric_widening_to(target_ty).is_some() {
                        let source = source.clone();
                        self.widen_numeric_expr(value, &source, target_ty);
                        vt = Some(target_ty.clone());
                    }
                }
                if !is_compound {
                    if vt
                        .as_ref()
                        .is_some_and(crate::Sema::Diagnostics::contains_expiring_secret_loan)
                    {
                        self.diags.push(Diagnostic::error(
                                "E0201",
                                "an ExpiringSecret loan cannot replace an owned value".to_string(),
                                "the callback parameter is a temporary read-only loan that ends when `.with` returns".to_string(),
                                "use the loan inside the callback and assign only a non-secret result".to_string(),
                                Some(value.span()),
                            ));
                    }
                    self.reject_borrowed_param_subplace(
                        value,
                        vt.as_ref(),
                        "replace an owned value",
                    );
                    if let Expr::Ident(name, span) = value {
                        let borrowed = self.lookup(name).is_some_and(|info| {
                            !type_is_copy(&info.ty)
                                && matches!(
                                    info.param_conv,
                                    Some(AccessConvention::Read) | Some(AccessConvention::Write)
                                )
                        });
                        if borrowed {
                            self.diags.push(Diagnostic::error(
                                    "E0120",
                                    format!(
                                        "`{name}` was not moved here, so it cannot replace an owned value"
                                    ),
                                    "this function has read access only and does not own the value"
                                        .to_string(),
                                    format!("copy it explicitly with `{}{name}`", Syntax::SIGIL_COPY),
                                    Some(*span),
                                ));
                        }
                    }
                }
                self.note_move_if_direct_ident(value);
                // D-UNINIT-SENTINEL2: a plain `name = …` initializes a
                // `Type.{ uninit }` binding (clears the not-yet-written flag);
                // a compound `name += …` reads it first, so it's a
                // read-before-write.
                if let LValue::Local { name, name_span } = &*target {
                    if self.flow.uninit.contains(name) {
                        if is_compound {
                            self.diags.push(Diagnostic::error(
                                    "E0420",
                                    format!("`{}` may be read before it is given a value", name),
                                    format!(
                                        "`{}+=` reads `{}` first, but it was declared with `Type.{{ uninit }}` and has no value yet",
                                        name, name
                                    ),
                                    format!("give `{}` a value with `{} = …` before updating it", name, name),
                                    Some(*name_span),
                                ));
                        }
                        self.flow.uninit.remove(name);
                    }
                }
                match target {
                    LValue::Local { name, name_span } => {
                        let name_span = *name_span;
                        if self.lambda_mut_borrow_active(name) {
                            self.diags.push(aliasing_while_mut(name, name_span));
                        }
                        let Some(info) = self.lookup(name).cloned() else {
                            if self.is_persist_binding(name) {
                                // D-PERSIST-DEVSTATE1=A: the marker makes
                                // this module binding the one legal global
                                // write target. Its type and assignment
                                // rules were already checked above.
                                return;
                            }
                            if self.consts.contains_key(name.as_str()) {
                                self.diags.push(Diagnostic::error(
                                    "E0111",
                                    format!("`{}` is a const and can never change", name),
                                    "a const is fixed for the whole program".to_string(),
                                    format!(
                                        "use a `{}` binding if it needs to change",
                                        Syntax::SIGIL_BIND_MUT
                                    ),
                                    Some(name_span),
                                ));
                            } else {
                                self.unknown_name(name, name_span);
                            }
                            return;
                        };
                        let target_was_frozen = self.frozen_for(name).is_some();
                        if !info.mutable
                            && !self.is_write_view(name)
                            && self.frozen_for(name).is_none()
                        {
                            let what = if info.param_conv.is_some() {
                                format!("the parameter `{}` can't be changed here", name)
                            } else {
                                format!(
                                    "`{}` was made with `{}`, so it can't change",
                                    name,
                                    Syntax::SIGIL_BIND_IMMUT
                                )
                            };
                            let fix = if info.param_conv.is_some() {
                                format!(
                                        "mark the parameter `{}: {}{}` with the write-access marker `&` if the function should change it",
                                        name,
                                        Syntax::SIGIL_WRITE,
                                        info.ty.name()
                                    )
                            } else {
                                format!(
                                    "declare it with `{} {} ...` instead",
                                    name,
                                    Syntax::SIGIL_BIND_MUT
                                )
                            };
                            let mut diagnostic = Diagnostic::error(
                                    "E0111",
                                    what,
                                    format!(
                                        "only `{}` bindings and parameters marked with the write-access marker `&` can be changed",
                                        Syntax::SIGIL_BIND_MUT,
                                    ),
                                    fix,
                                    Some(name_span),
                                );
                            if let Some(sigil_span) = info.binding_sigil_span {
                                diagnostic = diagnostic.with_edit(TextEdit {
                                    span: sigil_span,
                                    new_text: Syntax::SIGIL_BIND_MUT.to_string(),
                                });
                            }
                            self.diags.push(diagnostic);
                        }
                        self.clear_moved_binding(name);
                        // D-CONC-FREEZE1=A: rebinding a local replaces its
                        // frozen proof only after the write check above.
                        // A rejected write through a frozen target keeps
                        // the original freeze site intact.
                        if !target_was_frozen && !is_compound && vt.is_some() {
                            let depth = self
                                .binding_fact_depth(name)
                                .unwrap_or_else(|| self.scope_depth());
                            if let Some(site) = self.frozen_expr_site(value) {
                                self.flow.frozen.set_at(name, depth, site);
                            } else {
                                self.flow.frozen.remove_at(name, depth);
                            }
                        }
                        if matches!(&info.ty, Type::Fn { .. }) {
                            let sendable = !is_compound
                                && vt.as_ref().is_some_and(|value_ty| {
                                    self.interrupt_callback_expr_sendable(value, value_ty)
                                })
                                && info.param_conv.is_none();
                            self.set_interrupt_sendable(name, sendable);
                        }
                        if let (Some(vt), false) =
                            (vt.clone(), info.ty == Type::Named(String::new()))
                        {
                            if crate::Sema::Diagnostics::is_deterministic_clock_type(&info.ty)
                                && !crate::Sema::Diagnostics::is_deterministic_clock_type(&vt)
                            {
                                self.diags.push(crate::Sema::e3403(
                                    "replacing a deterministic Clock with an unproven Clock",
                                    Some(value.span()),
                                ));
                            }
                            if vt != info.ty {
                                self.diags.push(Diagnostic::error(
                                    "E0108",
                                    format!(
                                        "`{}` holds {}, but this value is {}",
                                        name,
                                        info.ty.show(),
                                        vt.show()
                                    ),
                                    "a binding keeps one type for its whole life".to_string(),
                                    division_fix_hint(&info.ty, &vt, value),
                                    Some(value.span()),
                                ));
                            }
                        }
                    }
                    LValue::Index {
                        base,
                        index,
                        span,
                        kind,
                    } => {
                        let fixed_uninit = if !is_compound {
                            match base.as_ref() {
                                Expr::Ident(name, _) => self
                                    .flow
                                    .uninit
                                    .remove(name)
                                    .map(|state| (name.clone(), state)),
                                _ => None,
                            }
                        } else {
                            None
                        };
                        self.borrow_ctx = true;
                        let base_ty = self.infer(base);
                        let idx_ty = self.infer(index);
                        match &base_ty {
                            Some(Type::Map { .. }) => *kind = IndexKind::Map,
                            // S76/D-FIXARR1: `buf[i] = v` on a fixed-size `[T#N]` indexes
                            // exactly like a growable `[T]` — same gate as `infer_index`'s
                            // read-side `Type::FixedList` arm. Missing this left `kind` at
                            // its `IndexKind::Unknown` default, which the TIR subset gate
                            // (`stmt_in_subset`) reads as "sema did not resolve it" and
                            // excludes the whole function — an I2 ICE since TIR is the
                            // only codegen path (R7).
                            Some(Type::List(_)) | Some(Type::FixedList { .. }) => {
                                *kind = IndexKind::List
                            }
                            Some(Type::Apply { name, .. }) if name == "ViewMut" => {
                                *kind = IndexKind::List
                            }
                            Some(Type::Named(n)) if self.trait_reg.index_types.contains_key(n) => {
                                *kind = IndexKind::User(n.clone());
                            }
                            // D-MEM1 S6: `pool[id] = v` — generation-checked write.
                            Some(Type::Apply {
                                name,
                                args: pool_args,
                            }) if name == "Pool" => {
                                *kind = IndexKind::Pool;
                                let is_matching_id = matches!(
                                    &idx_ty,
                                    Some(Type::Apply { name, args: id_args })
                                        if name == "Id" && id_args.first() == pool_args.first()
                                );
                                if !is_matching_id {
                                    self.diags.push(Diagnostic::error(
                                            "E0112",
                                            format!(
                                                "`Pool` indexes need a matching `Id<T>`, not {}",
                                                idx_ty
                                                    .as_ref()
                                                    .map(|t| t.show())
                                                    .unwrap_or_else(|| "this".to_string())
                                            ),
                                            "a pool slot is only reached through the `Id<T>` its own `.add()` returned".to_string(),
                                            "index with the `Id<T>` handle from `.add(...)`".to_string(),
                                            Some(index.span()),
                                        ));
                                }
                            }
                            _ => {}
                        }
                        // D-SOA1: index-WRITE through a columnar list (`xs[i] = …`)
                        // is deferred — v1 supports index-READ, field-read, `len`,
                        // `is_empty`, `push`, and iteration. Reject rather than
                        // miscompile (the columns type has no `IndexMut`).
                        if let Some(Type::List(inner)) = &base_ty {
                            if let Type::Named(elem) = inner.as_ref() {
                                if self.registry.is_columnar(elem) {
                                    self.diags.push(Diagnostic::error(
                                            "E1108",
                                            format!(
                                                "writing through `[ ]` isn't supported on a columnar list `{}` yet",
                                                Type::List(inner.clone()).show()
                                            ),
                                            "`#Layout(columnar)` lists support reading in v1 (indexing, field access, `len`, `is_empty`, `push`, iteration); index-write is deferred".to_string(),
                                            format!(
                                                "drop `#Layout(columnar)` from `{}` to assign through `[ ]`, or rebuild the list with `push`",
                                                elem
                                            ),
                                            Some(*span),
                                        ));
                                    return;
                                }
                            }
                        }
                        // Writing through `[ ]` changes the owner: the root
                        // name must be changeable and not under a `for` borrow.
                        let base_is_pool =
                            matches!(&base_ty, Some(Type::Apply { name, .. }) if name == "Pool");
                        let base_is_view_mut =
                            matches!(&base_ty, Some(Type::Apply { name, .. }) if name == "ViewMut");
                        let base_has_write_view =
                            expr_root_ident(base).is_some_and(|name| self.is_write_view(name));
                        if base_is_pool
                            || base_is_view_mut
                            || matches!(
                                base_ty,
                                Some(Type::Map { .. })
                                    | Some(Type::List(_))
                                    | Some(Type::FixedList { .. })
                            )
                        {
                            if let Some(root) = expr_root_ident(base) {
                                let root = root.to_string();
                                if self.iter_borrowed.contains(&root) {
                                    self.diags.push(collection_changed_in_loop(&root, *span));
                                }
                                if let Some(info) = self.lookup(&root) {
                                    if !base_is_view_mut
                                        && !base_has_write_view
                                        && !info.mutable
                                        && self.frozen_for(&root).is_none()
                                    {
                                        let (code, why, fix) = if matches!(
                                            info.param_conv,
                                            Some(AccessConvention::Read)
                                        ) {
                                            (
                                                    "E0205",
                                                    "an unmarked parameter gives read access only; assigning into it needs the write-access marker `&`".to_string(),
                                                    format!(
                                                        "change the parameter to `{}: {}{}` with the write-access marker `&`",
                                                        root,
                                                        Syntax::SIGIL_WRITE,
                                                        info.ty.name()
                                                    ),
                                                )
                                        } else {
                                            (
                                                    "E0202",
                                                    "assigning into a collection edits it; the binding must be declared mutable".to_string(),
                                                    format!(
                                                        "declare `{} {} ...`",
                                                        root,
                                                        Syntax::SIGIL_BIND_MUT
                                                    ),
                                                )
                                        };
                                        self.diags.push(Diagnostic::error(
                                                code,
                                                format!(
                                                    "cannot write to `{}` — it does not have the write-access marker `&`",
                                                    root
                                                ),
                                                why,
                                                fix,
                                                Some(*span),
                                            ));
                                    }
                                }
                            }
                        }
                        if !matches!(idx_ty.as_ref(), Some(Type::Int | Type::InlineRange { .. }))
                            && !matches!(base_ty, Some(Type::Map { .. }))
                        {
                            if let Some(ref it) = idx_ty {
                                self.diags.push(Diagnostic::error(
                                    "E0505",
                                    format!(
                                        "list indexes must be {}, not {}",
                                        Type::Int.show(),
                                        it.show()
                                    ),
                                    "count positions with a whole number starting at 0".to_string(),
                                    "use an Int index, like `items[0]`".to_string(),
                                    Some(index.span()),
                                ));
                            }
                        }
                        if let Some(Type::Map {
                            key,
                            value: map_val_ty,
                            ..
                        }) = base_ty
                        {
                            if let Some(kt) = idx_ty {
                                if kt != *key {
                                    self.diags.push(Diagnostic::error(
                                        "E0505",
                                        format!(
                                            "this map holds keys of type {}, not {}",
                                            key.show(),
                                            kt.show()
                                        ),
                                        "the key in `map[key]` must match the map's key type"
                                            .to_string(),
                                        format!("use a {} key here", key.name()),
                                        Some(index.span()),
                                    ));
                                }
                            }
                            if let Some(vt) = vt {
                                if vt != *map_val_ty
                                    && !self.nominal_type_identity(&map_val_ty, &vt)
                                {
                                    self.diags.push(Diagnostic::error(
                                        "E0108",
                                        format!(
                                            "this map holds values of type {}, not {}",
                                            map_val_ty.show(),
                                            vt.show()
                                        ),
                                        "every value stored in a map must have the same type"
                                            .to_string(),
                                        type_fix_hint(&map_val_ty, &vt),
                                        Some(value.span()),
                                    ));
                                }
                            }
                        } else if let Some(
                            Type::List(elem_ty) | Type::FixedList { elem: elem_ty, .. },
                        ) = base_ty
                        {
                            if let Some(vt) = vt {
                                if vt != *elem_ty {
                                    self.diags.push(Diagnostic::error(
                                        "E0108",
                                        format!(
                                            "this list holds {}, not {}",
                                            elem_ty.show(),
                                            vt.show()
                                        ),
                                        "every item stored in a list must have the same type"
                                            .to_string(),
                                        type_fix_hint(&elem_ty, &vt),
                                        Some(value.span()),
                                    ));
                                }
                            }
                        } else if let Some(Type::Apply { name, args }) = base_ty {
                            if name == "ViewMut" {
                                if let (Some(elem_ty), Some(vt)) = (args.first(), vt) {
                                    if vt != *elem_ty {
                                        self.diags.push(Diagnostic::error(
                                                "E0108",
                                                format!(
                                                    "this view holds {}, not {}",
                                                    elem_ty.show(),
                                                    vt.show()
                                                ),
                                                "every item written through a view must keep the owner's element type".to_string(),
                                                type_fix_hint(elem_ty, &vt),
                                                Some(value.span()),
                                            ));
                                    }
                                }
                            }
                        } else if let Some(Type::Named(n)) = &base_ty {
                            if let Some((key_ty, value_ty)) = self.trait_reg.index_types.get(n) {
                                if !self.trait_reg.index_mutable.contains(n) {
                                    self.diags.push(Diagnostic::error(
                                            "E0505",
                                            format!(
                                                "`{}` can be read with `[ ]` but not written — it has no `IndexMut` impl",
                                                n
                                            ),
                                            "bracket assignment needs `impl Type.IndexMut { fn set(&self, k, v) }` with the write-access marker `&`"
                                                .to_string(),
                                            format!(
                                                "use `.set(key, value)` instead, or add `impl {n}.IndexMut`"
                                            ),
                                            Some(*span),
                                        ));
                                }
                                if let Some(kt) = idx_ty {
                                    if kt != *key_ty {
                                        self.diags.push(Diagnostic::error(
                                            "E0505",
                                            format!(
                                                "this value indexes with {}, not {}",
                                                key_ty.show(),
                                                kt.show()
                                            ),
                                            "the key must match the type's `Index` key".to_string(),
                                            format!("use a {} key here", key_ty.name()),
                                            Some(index.span()),
                                        ));
                                    }
                                }
                                if let Some(vt) = vt {
                                    if vt != *value_ty {
                                        self.diags.push(Diagnostic::error(
                                            "E0108",
                                            format!(
                                                "this value holds {}, not {}",
                                                value_ty.show(),
                                                vt.show()
                                            ),
                                            "the stored value must match the type's `Index` value"
                                                .to_string(),
                                            type_fix_hint(value_ty, &vt),
                                            Some(value.span()),
                                        ));
                                    }
                                }
                                if let Some(root) = expr_root_ident(base) {
                                    let root = root.to_string();
                                    if let Some(info) = self.lookup(&root) {
                                        if !info.mutable && !self.is_write_view(&root) {
                                            self.diags.push(Diagnostic::error(
                                                    "E0202",
                                                    format!(
                                                        "cannot write to `{}` — it does not have the write-access marker `&`",
                                                        root
                                                    ),
                                                    "assigning through `[ ]` edits the value; the binding must be declared mutable"
                                                        .to_string(),
                                                    format!(
                                                        "declare `{} {} ...`",
                                                        root,
                                                        Syntax::SIGIL_BIND_MUT
                                                    ),
                                                    Some(*span),
                                                ));
                                        }
                                    }
                                }
                            }
                        } else if let Some(Type::String) = base_ty {
                            self.diags.push(Diagnostic::error(
                                    "E0503",
                                    "strings aren't indexed with `[ ]`".to_string(),
                                    "text is counted in characters — walk them with `.chars()` or take a piece with `.slice(start..end)`".to_string(),
                                    "e.g. `loop c in s.chars() { }` or `s.slice(0..2)`".to_string(),
                                    Some(*span),
                                ));
                        }
                        if let Some((name, mut state)) = fixed_uninit {
                            let completely_initialized =
                                if let (Some(len), Expr::Int(index, _, _, _)) =
                                    (state.fixed_len, index.as_ref())
                                {
                                    if *index >= 0 && (*index as u64) < len {
                                        state.initialized_indexes.insert(*index as u64);
                                    }
                                    state.initialized_indexes.len() as u64 == len
                                } else {
                                    false
                                };
                            if !completely_initialized {
                                self.flow.uninit.set(&name, state);
                            }
                        }
                    }
                    // D-MUTSELF1: a field-assignment `place.field [op]= v`. The place
                    // must be a CHANGEABLE place: a `mut self` receiver, or a `:=`/`mut`
                    // local. A non-`mut` `self` (shared-read receiver) or an immutable/shared
                    // binding is E0205, pointed at the assignment, with a "write the
                    // receiver as `mut self`" / "make it changeable" fix (owner Q1).
                    LValue::Field { base, field, span } => {
                        self.borrow_ctx = true;
                        // D-SOA1: `xs[i].field = …` where `xs` is a columnar list
                        // would write into a throwaway gathered value, not the
                        // column — reject (field-WRITE on a columnar element is
                        // deferred; reads are supported). Detected off the index
                        // base's root binding type (the common `local[i].f` form).
                        if let Expr::Index { base: ib, .. } = base.as_ref() {
                            let columnar_elem = expr_root_ident(ib)
                                .and_then(|root| self.lookup(root))
                                .and_then(|info| match &info.ty {
                                    Type::List(inner) => match inner.as_ref() {
                                        Type::Named(elem) if self.registry.is_columnar(elem) => {
                                            Some(elem.clone())
                                        }
                                        _ => None,
                                    },
                                    _ => None,
                                });
                            if let Some(elem) = columnar_elem {
                                self.diags.push(Diagnostic::error(
                                        "E1108",
                                        format!(
                                            "writing `{}[i].{}` isn't supported on a columnar list yet",
                                            expr_root_ident(ib).unwrap_or("xs"),
                                            field
                                        ),
                                        "`#Layout(columnar)` lists support reading a field (`xs[i].f`) in v1; writing one is deferred".to_string(),
                                        format!(
                                            "drop `#Layout(columnar)` from `{}` to write fields in place, or rebuild the element with `push`",
                                            elem
                                        ),
                                        Some(*span),
                                    ));
                                return;
                            }
                        }
                        let suppress = self.suppress_partial_move_root_read;
                        if !is_compound {
                            self.suppress_partial_move_root_read = true;
                        }
                        let base_ty = self.infer(base);
                        self.suppress_partial_move_root_read = suppress;
                        // D-FIELDPOL1: `s.computed_field = v` — a computed field is
                        // never stored, so a plain assignment has nothing to write.
                        if let Some(bt) = &base_ty {
                            if self.field_is_computed(bt, field) {
                                self.diags.push(computed_field_not_settable(field, *span));
                                return;
                            }
                        }
                        // D-SWIZZLE1: overlapping write swizzles (`v.xx = …`) are rejected.
                        if let Some(Type::Named(type_name)) = &base_ty {
                            if is_swizzleable_math_type(type_name)
                                && !self.registry.contains(type_name)
                            {
                                if let SwizzleParse::Ok(lanes) =
                                    parse_swizzle_member(field, type_name)
                                {
                                    if swizzle_write_overlaps(&lanes) {
                                        self.diags.push(Diagnostic::error(
                                                "E3111",
                                                format!(
                                                    "write swizzle `{}` repeats a lane on `{}`",
                                                    field, type_name
                                                ),
                                                "each lane may be written at most once — overlapping patterns like `v.xx` have no single meaning"
                                                    .to_string(),
                                                format!(
                                                    "assign each lane once, e.g. `{}.xy = …` instead of `{}.{} = …`",
                                                    expr_root_ident(base).unwrap_or("v"),
                                                    expr_root_ident(base).unwrap_or("v"),
                                                    field
                                                ),
                                                Some(*span),
                                            ));
                                        return;
                                    }
                                }
                            }
                        }
                        // Validate the field exists and get its type (emits E0302 on a
                        // bad field). The value's type must match the field type (E0108).
                        if let Some(bt) = &base_ty {
                            if let Some(ft) = self.field_type(bt, field, *span) {
                                if self.type_contains_view_boundary(&ft) {
                                    self.diags.push(Diagnostic::error(
                                            "E2305",
                                            format!("view field `{field}` cannot be assigned a new source"),
                                            "a stored view field has one stabilized owner relationship; overwriting it would erase or change that public provenance"
                                                .to_string(),
                                            "construct a new value with the new view, or keep the original field source unchanged"
                                                .to_string(),
                                            Some(*span),
                                        ));
                                    return;
                                }
                                if let Some(vt) = &vt {
                                    if *vt != ft && ft != Type::Named(String::new()) {
                                        self.diags.push(Diagnostic::error(
                                            "E0108",
                                            format!(
                                                "field `{}` holds {}, but this value is {}",
                                                field,
                                                ft.show(),
                                                vt.show()
                                            ),
                                            "a field keeps one type for its whole life".to_string(),
                                            type_fix_hint(&ft, vt),
                                            Some(value.span()),
                                        ));
                                    }
                                }
                            }
                        }
                        // The root place must be changeable. The headline is `self`:
                        // a `mut self` receiver (param_conv == Mutate) may be mutated;
                        // a shared-read `self`, or any non-`mut` local, may not.
                        if let Some(root) = expr_root_ident(base) {
                            let root = root.to_string();
                            if let Some(info) = self.lookup(&root) {
                                if !info.mutable
                                    && !self.is_write_view(&root)
                                    && !self.is_edit_shared_guard(&root)
                                    && self.frozen_for(&root).is_none()
                                {
                                    let is_self = root == Syntax::KW_SELF;
                                    let what = if is_self {
                                        format!(
                                                "cannot edit `{}` — `{}` has read access only; the write-access marker `&` is required",
                                                field,
                                                Syntax::KW_SELF
                                            )
                                    } else {
                                        format!("cannot edit `{}` — `{}` does not have the write-access marker `&`", field, root)
                                    };
                                    let fix = if is_self {
                                        format!(
                                                "write the receiver with the write-access marker `&`: `{}{}` to grant write access",
                                                Syntax::SIGIL_WRITE,
                                                Syntax::KW_SELF
                                            )
                                    } else if info.param_conv.is_some() {
                                        format!(
                                                "mark the parameter with the write-access marker `&`: `{}: {}{}` to grant write access",
                                                root,
                                                Syntax::SIGIL_WRITE,
                                                info.ty.name()
                                            )
                                    } else {
                                        format!(
                                            "declare it with `{} {} ...` to give it write access",
                                            root,
                                            Syntax::SIGIL_BIND_MUT
                                        )
                                    };
                                    self.diags.push(Diagnostic::error(
                                            "E0205",
                                            what,
                                            "editing a field requires the write-access marker `&` on the owning place".to_string(),
                                            fix,
                                            Some(*span),
                                        ));
                                }
                            }
                        }
                        if !is_compound && base_ty.is_some() {
                            let target = Expr::Field(base.clone(), field.clone(), *span);
                            self.clear_moved_expr(&target);
                        }
                    }
                }
                // D-TRACK-ORIGIN1=A: any successful write into a tracked
                // binding, including a field or index place, ends the old
                // value's provenance. Reads on the RHS above still observe
                // the pre-write fact.
                if let Some(root) = origin_write_root.as_deref() {
                    self.clear_origin(root);
                }
            }
            Stmt::Expr(_) => {
                if self.rewrite_anonymous_taskgroup_spawn(stmt) {
                    if let Stmt::Val(b) = stmt {
                        self.check_binding(b);
                        crate::Sema::Effects::record_authority_alias(self, b);
                    }
                    return;
                }
                let Stmt::Expr(expr) = stmt else {
                    return;
                };
                // D-IGNORERET2=A: `.drop("reason")` is the blessed explicit-discard
                // terminal. When recognized, infer the *receiver* (for side effects),
                // validate the reason is a non-empty string literal, and suppress E0402.
                if let Expr::MethodCall {
                    receiver,
                    method,
                    method_span,
                    args,
                    ..
                } = expr
                {
                    if method == Syntax::METHOD_DROP {
                        let recv_ty = self.infer_fallible_stmt(receiver);
                        // Validate reason argument — must be a non-empty string literal.
                        match args.first() {
                            Some(a) => match &a.expr {
                                Expr::Str(parts, _)
                                    if parts.len() == 1
                                        && matches!(&parts[0], StrPart::Lit(s) if s.is_empty()) =>
                                {
                                    self.diags.push(Diagnostic::error(
                                            "E0407",
                                            "`.drop()` reason must not be empty".to_string(),
                                            "the reason documents why this result is intentionally discarded".to_string(),
                                            "write `.drop(\"why this is fine to ignore\")` with a real explanation".to_string(),
                                            Some(*method_span),
                                        ));
                                }
                                Expr::Str(parts, _)
                                    if parts.iter().all(|p| matches!(p, StrPart::Lit(_))) =>
                                {
                                    // Non-empty plain string literal — valid.
                                }
                                _ => {
                                    self.diags.push(Diagnostic::error(
                                        "E0407",
                                        "`.drop()` requires a string literal reason".to_string(),
                                        "the reason must be a compile-time string, not a variable"
                                            .to_string(),
                                        "write `.drop(\"why this is fine to ignore\")`".to_string(),
                                        Some(*method_span),
                                    ));
                                }
                            },
                            None => {
                                self.diags.push(Diagnostic::error(
                                        "E0407",
                                        "`.drop()` requires a reason argument".to_string(),
                                        "the reason documents why this result is intentionally discarded".to_string(),
                                        "write `.drop(\"why this is fine to ignore\")`".to_string(),
                                        Some(*method_span),
                                    ));
                            }
                        }
                        // E0402 is suppressed — that is the entire point of `.drop()`.
                        // Task drop is still rejected (L1101) because `.drop()` on a task
                        // doesn't actually join it; use `.detach()` for fire-and-forget.
                        if let Some(ty) = recv_ty {
                            if is_task_type(&ty) {
                                self.diags.push(crate::Sema::CheckerOwnership::l1101_unjoined_task(
                                        "this task",
                                        "`.drop()` discards the handle without joining it, so the task may outlive the function",
                                        expr.span(),
                                    ));
                            }
                        }
                        // Short-circuit: don't fall through to the generic E0402 path.
                        return;
                    }
                }
                // A statement consumes the call's success value. An enclosing
                // value-if may leave its Result carrier in `expected_type`,
                // but that expectation belongs only to the branch value, not
                // to this statement.
                let must_use_call_target = self.ignored_must_use_call_target(expr);
                let saved_expected = self.expected_type.take();
                let inferred = self.infer_statement_expr(expr);
                self.expected_type = saved_expected;
                if let Some(ty) = inferred {
                    if ty.is_fallible() && !self.suppress_must_use {
                        self.diags.push(Diagnostic::error(
                                "E0402",
                                "this call can fail and nothing checks it".to_string(),
                                "a fallible result can't be ignored — handle it or say failure is impossible"
                                    .to_string(),
                                format!(
                                    "use `{}`, `{}`, `{} ...`, or `.drop(\"reason\")` to intentionally discard",
                                    Syntax::OP_TRY_SUFFIX,
                                    Syntax::OP_FALLBACK,
                                    Syntax::BUILTIN_PANIC
                                ),
                                Some(expr.span()),
                            ));
                    } else if !self.suppress_must_use {
                        self.check_ignored_must_use(
                            expr,
                            &ty,
                            expr.span(),
                            must_use_call_target,
                        );
                    }
                    if self.arrow_loop_body && !self.is_unit_type(&ty) {
                        self.diags.push(Diagnostic::lint(
                                "L0508",
                                "this arrow loop body computes a value and drops it".to_string(),
                                "a statement-position loop arrow discards its body's value; the loop takes write access (&) for items when it updates the source".to_string(),
                                "bind the loop with `::` to collect its values, or write `loop v in &values -> v *= 2` so it takes write access (&) for items".to_string(),
                                Some(expr.span()),
                            ));
                    }
                    // An unbound spawn belongs to its enclosing scope; only a
                    // named handle carries the join duty checked at scope end.
                }
            }
            Stmt::DeferClose { close, .. } => {
                let saved_expected = self.expected_type.take();
                self.infer_fallible_stmt(close);
                self.expected_type = saved_expected;
            }
            Stmt::Return(expr, span) => {
                self.check_return_expr(expr, span, None);
            }
            // D-STREAMYIELD1: `yield expr` — legal only in a function whose return
            // type is `Stream<T>` (E0805 otherwise); `expr: T` (E0807 on mismatch).
            Stmt::Yield(e, span) => {
                // Yielding-loop collection uses a compiler-private zero-width
                // marker. A real `yield` nested in that loop still belongs to
                // an enclosing Stream generator.
                if span.start == span.end && !self.collect_item_types.is_empty() {
                    let saved_expected = self.expected_type.clone();
                    let expected = self.collect_item_types.last().and_then(Clone::clone);
                    self.expected_type = expected.clone();
                    let got = self.infer(e).map(|ty| self.resolve_type(ty));
                    self.expected_type = saved_expected;
                    if let Some(got) = got {
                        if matches!(&got, Type::Named(name) if name == Syntax::INTERNAL_UNIT_TYPE) {
                            self.diags.push(Diagnostic::error(
                                    "E0073",
                                    "this collecting loop path produces no item".to_string(),
                                    "every accepted iteration must contribute one non-unit value unless `next` omits it".to_string(),
                                    "return a value on this path, or remove `->`".to_string(),
                                    Some(e.span()),
                                ));
                        } else if let Some(want) = expected {
                            // D-NUMJOIN1=A: a numeric item widens into the
                            // item type the loop is already building.
                            if got != want && got.numeric_widening_to(&want).is_some() {
                                // Keep the same sema widening path used by
                                // calls, returns, and ordinary lists. It
                                // gates range erasure before the item is
                                // stored in the collected carrier.
                                let source = got.clone();
                                self.widen_numeric_expr(e, &source, &want);
                            } else if got != want {
                                self.diags.push(Diagnostic::error(
                                        "E0074",
                                        "this collecting loop produces incompatible item types".to_string(),
                                        format!("one collecting loop builds one `[{}]`, but this item is {}", want.show(), got.show()),
                                        "convert every item to one type, or split the loops".to_string(),
                                        Some(e.span()),
                                    ));
                            }
                        } else {
                            *self.collect_item_types.last_mut().expect("checked above") = Some(got);
                        }
                    }
                    return;
                }
                let elem_ty = stream_element_type(self.declared_return_type.as_ref())
                    .map(|ty| self.resolve_type(ty));
                let Some(elem_ty) = elem_ty else {
                    self.diags.push(Diagnostic::error(
                            "E0805",
                            format!("`{}` outside a generator", Syntax::KW_YIELD),
                            "`yield` hands a value to a `Stream<T>` consumer — only a function declared `Stream<T> ->` may use it".to_string(),
                            format!("declare `{}<T> ->` on this function, or remove the `{}`", Syntax::TYPE_STREAM, Syntax::KW_YIELD),
                            Some(*span),
                        ));
                    self.infer(e);
                    return;
                };
                let saved_expected = self.expected_type.clone();
                self.expected_type = Some(elem_ty.clone());
                let got = self.infer(e);
                self.expected_type = saved_expected;
                if let Some(got) = got {
                    let got = self.resolve_type(got);
                    if got != elem_ty {
                        self.diags.push(Diagnostic::error(
                            "E0807",
                            format!(
                                "this yields {}, but the stream is `{}<{}>`",
                                got.show(),
                                Syntax::TYPE_STREAM,
                                elem_ty.show()
                            ),
                            "every `yield` in a generator must hand back the stream's element type"
                                .to_string(),
                            type_fix_hint(&elem_ty, &got),
                            Some(e.span()),
                        ));
                    }
                }
            }
            // D-META-STAGE1=B (formerly D-CTMARKER1, ratified 2026-06-25, piece 2): build-time execution block.
            Stmt::ComptimeBlock { .. } => self.check_comptime_block(stmt),
            // D-WHEN1/D-WHEN2 (ratified 2026-06-19): compile-time conditional.
            Stmt::ComptimeIf { .. } => self.check_comptime_if(stmt),
            Stmt::While {
                cond,
                body,
                span: _,
                arrow_body,
                label,
            } => {
                let memory_multiplier = self.memory_control_multiplier;
                self.memory_control_multiplier = None;
                self.require_bool(cond, "a `while` condition");
                if let Some((n, label_span)) = label {
                    self.declare_loop_label(n, *label_span);
                }
                self.push_loop_value_frame(label.as_ref());
                self.push_loop_break_frame();
                self.loop_depth += 1;
                // D-FACT-FLOW1: one loop rule for every plane — a body may run zero
                // times, so the facts after the loop join the zero-turn path with the
                // path one walk of the body left behind.
                let before_loop = self.flow.clone();
                let previous_arrow_loop_body = self.arrow_loop_body;
                self.arrow_loop_body = *arrow_body;
                self.check_block(body, true);
                self.arrow_loop_body = previous_arrow_loop_body;
                let after_body = self.flow.clone();
                self.join_loop_flow(
                    &before_loop,
                    &after_body,
                    !matches!(cond.without_parens(), Expr::Bool(true, _)),
                );
                self.loop_depth -= 1;
                if after_body.reachable {
                    if let Some((name, span)) = drained_collection(cond, body) {
                        let name = name.to_string();
                        let carries_duty = self
                            .lookup(&name)
                            .is_some_and(|info| self.type_is_single_use(&info.ty));
                        if carries_duty {
                            self.mark_moved(name, span);
                        }
                    }
                }
                self.pop_loop_value_frame();
                if label.is_some() {
                    self.loop_labels.pop();
                }
                self.memory_control_multiplier = memory_multiplier;
            }
            Stmt::For {
                var,
                var_span,
                var2,
                kind,
                body,
                span: _,
                arrow_body,
                label,
                auto_vectorization,
            } => {
                let auto_vectorization_kind = kind.clone();
                let memory_multiplier = self.memory_control_multiplier;
                let loop_multiplier = memory_multiplier.and_then(|outer| {
                    statically_bounded_for_iterations(kind)
                        .and_then(|iterations| outer.checked_mul(iterations))
                });
                self.memory_control_multiplier = memory_multiplier;
                if let Some((n, label_span)) = label {
                    self.declare_loop_label(n, *label_span);
                }
                // D-FACT-FLOW1: one loop rule for every plane — a body may run
                // zero times, so the facts after the loop join the zero-turn
                // path with the path one walk of the body left behind.
                let before_loop = self.flow.clone();
                self.push_loop_value_frame(label.as_ref());
                self.push_loop_break_frame();
                match kind {
                    ForKind::Range {
                        start,
                        end,
                        step,
                        exclusive,
                    } => {
                        for (e, which) in [(&mut *start, "start"), (&mut *end, "end")] {
                            let t = self.infer(e);
                            if let Some(t) = t {
                                if !matches!(&t, Type::Int | Type::InlineRange { .. }) {
                                    self.diags.push(Diagnostic::error(
                                            "E0109",
                                            format!(
                                                "the {} of a `for` range must be {}, not {}",
                                                which,
                                                Type::Int.show(),
                                                t.show()
                                            ),
                                            "`loop` counts whole numbers between two ends (inclusive `..` includes both, S22; exclusive `..<` stops before the end)"
                                                .to_string(),
                                            "use Int values for both ends, like `1..10` or `0..<n`".to_string(),
                                            Some(e.span()),
                                        ));
                                }
                            }
                        }
                        if let Some(step) = step {
                            // D-LOOP-ADVANCE2=A: the stride must be a positive Int.
                            let t = self.infer(step);
                            if let Some(t) = t {
                                if !matches!(&t, Type::Int | Type::InlineRange { .. }) {
                                    self.diags.push(Diagnostic::error(
                                        "E0123",
                                        format!(
                                            "a range loop stride must be {}, not {}",
                                            Type::Int.show(),
                                            t.show()
                                        ),
                                        "the stride is how far to count each turn, so it must be a whole number"
                                            .to_string(),
                                        "use an Int stride, like `loop i in 0..10, 2 { ... }`".to_string(),
                                        Some(step.span()),
                                    ));
                                }
                            }
                            if let Expr::Int(n, sp, _, _) = step {
                                if *n <= 0 {
                                    self.diags.push(Diagnostic::error(
                                            "E0123",
                                            format!("a range loop stride must be positive, not {}", n),
                                            "a zero or negative stride would never reach the end"
                                                .to_string(),
                                            "use a stride of 1 or more, like `loop i in 0..10, 2 { ... }`".to_string(),
                                            Some(*sp),
                                        ));
                                }
                            }
                        }
                        self.loop_depth += 1;
                        self.push_scope();
                        let vs = *var_span;
                        let v = var.clone();
                        if self.lookup(&v).is_some() || self.consts.contains_key(&v) {
                            self.diags.push(already_defined(&v, vs));
                        }
                        self.declare_in_scope(
                            &v,
                            LocalInfo {
                                def_span: vs,
                                binding_sigil_span: None,
                                ty: Type::Int,
                                mutable: false,
                                param_conv: None,
                                decl_loop_depth: self.loop_depth,
                                interrupt_sendable: false,
                                reactive_local: false,
                                reactive_shared: false,
                                single_use_span: None,
                                constant_value: None,
                                invalid: false,
                            },
                        );
                        self.memory_control_multiplier = loop_multiplier;
                        let previous_arrow_loop_body = self.arrow_loop_body;
                        self.arrow_loop_body = *arrow_body;
                        let before_auto_direct = self.fx_direct.clone();
                        let before_auto_edges = self.fx_edges.clone();
                        let before_auto_maximal = self.fx_maximal;
                        for s in body.iter_mut() {
                            self.check_stmt(s);
                        }
                        self.arrow_loop_body = previous_arrow_loop_body;
                        *auto_vectorization = self.prove_auto_vectorization_loop(
                            &auto_vectorization_kind,
                            var,
                            body,
                            &before_auto_direct,
                            &before_auto_edges,
                            before_auto_maximal,
                        );
                        // D-RANGE-EXCL1=C: teach when inclusive `….xs.len()` indexes that same xs
                        // with this loop's index name (the provable 0..len trap).
                        if !*exclusive {
                            if let Some(root) = range_end_len_root(end) {
                                if stmts_index_root_with(body, &root, var) {
                                    let end_span = end.span();
                                    self.diags.push(Diagnostic::error(
                                            "E0364",
                                            format!(
                                                "this range includes `{root}.len()`, one past the last index"
                                            ),
                                            format!(
                                                "`{root}.len()` is the count of items, so inclusive `..` runs one step too far when the body indexes `{root}`"
                                            ),
                                            format!(
                                                "write `loop (i, item) in {root}` — or `loop i in {root}.indexes` — or `0..<{root}.len()`"
                                            ),
                                            Some(end_span),
                                        ));
                                }
                            }
                        }
                        self.pop_scope();
                        self.loop_depth -= 1;
                    }
                    ForKind::In { collection, step } => {
                        if let Some(step) = step {
                            if let Some(ty) = self.infer(step) {
                                if !matches!(&ty, Type::Int | Type::InlineRange { .. }) {
                                    self.diags.push(Diagnostic::error(
                                            "E0123",
                                            format!("a source loop stride must be {}, not {}", Type::Int.show(), ty.show()),
                                            "the stride counts source pulls, so it must be a positive whole number".to_string(),
                                            "use an Int stride of 1 or more".to_string(),
                                            Some(step.span()),
                                        ));
                                }
                            }
                            if matches!(step, Expr::Int(n, ..) if *n <= 0) {
                                self.diags.push(Diagnostic::error(
                                    "E0123",
                                    "a source loop stride must be positive".to_string(),
                                    "zero or a negative value cannot select a later source item"
                                        .to_string(),
                                    "use a stride of 1 or more".to_string(),
                                    Some(step.span()),
                                ));
                            }
                        }
                        let coll_ty = self.infer(collection);
                        let bindingless = var == Syntax::KW_IT && var2.is_none();
                        let nested_bindingless =
                            bindingless && self.implicit_loop_subject_depth > 0;
                        if nested_bindingless {
                            self.diags.push(Diagnostic::from_row(
                                "E0380",
                                &[],
                                Some(collection.span()),
                            ));
                        }
                        let borrowed = collection_root_name(collection);
                        // A collection iterated by value is consumed: each
                        // step hands you the element itself. Match codegen's
                        // `by_value` predicate exactly — task-handle lists
                        // (including nested `[[Task<…>]]`), Stream, Iter, and
                        // HTTPBodyChunks. Two-binding loops consume too.
                        let task_list_consume = matches!(
                            &coll_ty,
                            Some(Type::List(inner) | Type::FixedList { elem: inner, .. })
                                if type_requires_owned_iteration(inner)
                        );
                        let streaming_consume = matches!(
                            &coll_ty,
                            Some(Type::Apply { name, .. })
                                if name == crate::Syntax::TYPE_STREAM
                                    || name == Syntax::TYPE_ITER
                        ) || matches!(
                            &coll_ty,
                            Some(Type::Named(name)) if name == "HTTPBodyChunks"
                        );
                        let consumes_collection = task_list_consume || streaming_consume;
                        let owns_collection =
                            !consumes_collection || self.frame_can_consume_collection(collection);
                        if !owns_collection {
                            if task_list_consume {
                                self.report_borrowed_loop_consume(collection, &coll_ty);
                            } else {
                                // Stream / Iter / HTTPBodyChunks. Peel
                                // Paren/Copy so a wrapped Ident still hits
                                // the move helper; field/index must get a
                                // diagnostic — consume_builtin_receiver is
                                // Ident-only and codegen still iterates by
                                // value (I2).
                                let place = match &*collection {
                                    Expr::Paren(inner, _) | Expr::Copy(inner, _) => inner.as_ref(),
                                    other => other,
                                };
                                if matches!(place, Expr::Ident(..)) {
                                    self.consume_builtin_receiver(place, "loop");
                                } else {
                                    self.diags.push(Diagnostic::error(
                                            "E0120",
                                            "this loop can't take a stream or iterator out of a field or index"
                                                .to_string(),
                                            "each step pulls from the source itself, so the loop must take a whole value this scope owns"
                                                .to_string(),
                                            "bind it into a local this scope owns first (`src := …`), then write `loop x in src { … }`"
                                                .to_string(),
                                            Some(collection.span()),
                                        ));
                                }
                            }
                        }
                        let lending_var = match (&coll_ty, var2.as_ref()) {
                            (
                                Some(Type::List(inner) | Type::FixedList { elem: inner, .. }),
                                None,
                            ) if matches!(
                                inner.as_ref(),
                                Type::Apply { name, .. } if name == "ViewMut"
                            ) =>
                            {
                                Some(var.clone())
                            }
                            (
                                Some(Type::List(inner) | Type::FixedList { elem: inner, .. }),
                                Some((value, _)),
                            ) if matches!(
                                inner.as_ref(),
                                Type::Apply { name, .. } if name == "ViewMut"
                            ) =>
                            {
                                Some(value.clone())
                            }
                            _ => None,
                        };
                        self.loop_depth += 1;
                        if let Some(n) = borrowed.clone() {
                            self.iter_borrowed.insert(n);
                        }
                        self.push_scope();
                        match &coll_ty {
                            Some(Type::List(inner)) | Some(Type::FixedList { elem: inner, .. }) => {
                                // D-RANGE-EXCL1=C: two bindings are index then item; one binding stays item-only.
                                if let Some((v2, v2s)) = var2.as_ref() {
                                    self.declare_loop_var(var.clone(), *var_span, &Type::Int);
                                    self.declare_loop_var(v2.clone(), *v2s, inner);
                                } else {
                                    self.declare_loop_var(var.clone(), *var_span, inner);
                                }
                            }
                            Some(Type::Apply { name, args })
                                if name == Syntax::TYPE_ITER && args.len() == 1 =>
                            {
                                self.declare_loop_var(var.clone(), *var_span, &args[0]);
                            }
                            Some(Type::Map { key, value, .. }) => {
                                if let Some((v2, v2s)) = var2.as_ref() {
                                    self.declare_loop_var(var.clone(), *var_span, key);
                                    self.declare_loop_var(v2.clone(), *v2s, value);
                                } else {
                                    let entry = Type::Tuple(vec![
                                        ("key".to_string(), Box::new((**key).clone())),
                                        ("value".to_string(), Box::new((**value).clone())),
                                    ]);
                                    self.declare_loop_var(var.clone(), *var_span, &entry);
                                }
                            }
                            // E2-M7: `loop line in handle.lines()` — streaming line iterator.
                            Some(Type::Named(n)) if n == "FileLines" => {
                                self.declare_loop_var(var.clone(), *var_span, &Type::String);
                            }
                            // D-STDIN1=A: `loop line in io.stdin().lines()` — streaming stdin iterator.
                            Some(Type::Named(n)) if n == "StdinLines" => {
                                self.declare_loop_var(var.clone(), *var_span, &Type::String);
                            }
                            // `loop line in child.stdout.lines()` /
                            // `child.stderr.lines()` — streaming subprocess output.
                            Some(Type::Named(n)) if n == "ProcessLines" => {
                                self.declare_loop_var(var.clone(), *var_span, &Type::String);
                            }
                            Some(Type::Named(n)) if n == Syntax::TYPE_RANGE => {
                                if var2.is_some() {
                                    self.diags.push(Diagnostic::error(
                                        "E0109",
                                        "a Range loop has one value per step".to_string(),
                                        "Range values yield whole numbers, not key-value pairs"
                                            .to_string(),
                                        "use one loop name".to_string(),
                                        Some(collection.span()),
                                    ));
                                }
                                self.declare_loop_var(var.clone(), *var_span, &Type::Int);
                            }
                            Some(Type::Named(n)) if n == "HTTPBodyChunks" => {
                                self.declare_loop_var(
                                    var.clone(),
                                    *var_span,
                                    &Type::Result {
                                        ok: Box::new(Type::List(Box::new(Type::Named(
                                            "U8".to_string(),
                                        )))),
                                        err: Box::new(Type::Named("HTTPError".to_string())),
                                    },
                                );
                            }
                            Some(Type::Named(n)) if encoding_reader_item_type(n).is_some() => {
                                if var2.is_some() {
                                    self.diags.push(Diagnostic::error(
                                        "E0109",
                                        format!(
                                            "`loop (x, y) in` on `{}` needs one loop name, not two",
                                            n
                                        ),
                                        "a codec reader yields one item per step".to_string(),
                                        format!("write `loop item in {n}`").to_string(),
                                        Some(collection.span()),
                                    ));
                                } else if let Some(item_ty) = encoding_reader_item_type(n) {
                                    self.declare_loop_var(var.clone(), *var_span, &item_ty);
                                }
                            }
                            Some(Type::Named(n))
                                if self.trait_reg.iterable_items.contains_key(n) =>
                            {
                                if var2.is_some() {
                                    self.diags.push(Diagnostic::error(
                                        "E0109",
                                        format!(
                                            "`loop (x, y) in` on `{}` needs one loop name, not two",
                                            n
                                        ),
                                        "a custom iterable yields one item per step".to_string(),
                                        format!("write `loop item in {n}`").to_string(),
                                        Some(collection.span()),
                                    ));
                                } else {
                                    let item_ty = self
                                        .trait_reg
                                        .iterable_items
                                        .get(n)
                                        .cloned()
                                        .unwrap_or(Type::Int);
                                    self.declare_loop_var(var.clone(), *var_span, &item_ty);
                                }
                            }
                            // D-STREAMYIELD1: `loop x in a_stream { }` — pull one value
                            // at a time from a generator's `Stream<T>`, blocking until
                            // the producer yields (or ends the stream by returning).
                            Some(Type::Apply { name, args })
                                if name == crate::Syntax::TYPE_STREAM && args.len() == 1 =>
                            {
                                self.declare_loop_var(var.clone(), *var_span, &args[0]);
                            }
                            // D-CONC-CHAN1=A: a receiver is a pull source. The loop
                            // ends on the channel's closed sentinel.
                            Some(Type::Apply { name, args })
                                if name == crate::Syntax::TYPE_RECEIVER && args.len() == 1 =>
                            {
                                self.declare_loop_var(var.clone(), *var_span, &args[0]);
                            }
                            // D-DYNARRAY1 / D-RANGE-EXCL1=C: `loop x in window` — View iterates
                            // elements; two bindings are index then item.
                            Some(Type::Apply { name, args })
                                if matches!(name.as_str(), "View" | "ViewMut")
                                    && args.len() == 1 =>
                            {
                                if let Some((v2, v2s)) = var2.as_ref() {
                                    self.declare_loop_var(var.clone(), *var_span, &Type::Int);
                                    self.declare_loop_var(v2.clone(), *v2s, &args[0]);
                                } else {
                                    self.declare_loop_var(var.clone(), *var_span, &args[0]);
                                }
                            }
                            Some(other) => {
                                if bindingless {
                                    if !nested_bindingless {
                                        self.diags.push(Diagnostic::from_row(
                                            "E0380",
                                            &[],
                                            Some(collection.span()),
                                        ));
                                    }
                                } else {
                                    self.diags.push(Diagnostic::error(
                                            "E0109",
                                            format!(
                                                "`for x in` needs a list or map, not {}",
                                                other.show()
                                            ),
                                            "walk items with `loop item in items { }` or characters with `loop c in s.chars() { }`".to_string(),
                                            "use a `List`, `Map`, or `s.chars()`".to_string(),
                                            Some(collection.span()),
                                        ));
                                }
                            }
                            None => {}
                        }
                        if let Some(name) = &lending_var {
                            self.lending_view_loop_vars.insert(name.clone());
                        }
                        self.memory_control_multiplier = loop_multiplier;
                        let previous_arrow_loop_body = self.arrow_loop_body;
                        self.arrow_loop_body = *arrow_body;
                        if bindingless && !nested_bindingless {
                            self.implicit_loop_subject_depth += 1;
                        }
                        for s in body.iter_mut() {
                            self.check_stmt(s);
                        }
                        if bindingless && !nested_bindingless {
                            self.implicit_loop_subject_depth -= 1;
                        }
                        self.arrow_loop_body = previous_arrow_loop_body;
                        if let Some(name) = &lending_var {
                            self.lending_view_loop_vars.remove(name);
                        }
                        self.pop_scope();
                        if let Some(n) = borrowed.clone() {
                            self.iter_borrowed.remove(&n);
                        }
                        self.loop_depth -= 1;
                        // A by-value collection cannot be iterated by copy, so
                        // the loop takes it. Record the move after leaving the
                        // loop: the collection is consumed by this loop, not
                        // inside it, so a later use is ordinary use-after-move
                        // (E0121) instead of a rustc rejection (I2).
                        // `consumes_collection` above matches the codegen
                        // `by_value` predicate. A collection this frame could
                        // not take already reported E0120, so do not also
                        // record a move it never made.
                        if consumes_collection && owns_collection {
                            if let Some(name) = borrowed {
                                self.mark_moved(name, collection.span());
                            }
                        }
                    }
                }
                self.pop_loop_value_frame();
                let after_body = self.flow.clone();
                self.join_loop_flow(&before_loop, &after_body, true);
                if label.is_some() {
                    self.loop_labels.pop();
                }
                self.memory_control_multiplier = memory_multiplier;
            }
            Stmt::Switch {
                subject,
                arms,
                else_body,
                span,
            }
            | Stmt::ComptimeSwitch {
                subject,
                arms,
                else_body,
                span,
            } => self.check_switch(subject, arms, else_body, *span),
            Stmt::Break(span) => {
                self.check_break_without_value(None, *span);
            }
            Stmt::BreakValue(value, span) => {
                self.check_break_value(None, value, *span);
            }
            Stmt::Continue(span) => {
                if self.loop_depth == 0 {
                    self.diags
                        .push(loop_control_outside(Syntax::KW_NEXT, *span));
                }
            }
            // D-ARROW-CONTROL1: `break(name)` / `next(name)`.
            Stmt::BreakLabel(name, span) => {
                self.check_break_without_value(Some((name, *span)), *span);
            }
            Stmt::BreakLabelValue(name, name_span, value, span) => {
                self.check_break_value(Some((name, *name_span)), value, *span);
            }
            Stmt::ContinueLabel(name, span) => {
                if self.loop_depth == 0 {
                    self.diags
                        .push(loop_control_outside(Syntax::KW_NEXT, *span));
                } else if !self.loop_labels.iter().any(|l| l == name) {
                    self.diags
                        .push(undefined_loop_label(name, &self.loop_labels, *span));
                }
            }
            Stmt::CountedLoop {
                init,
                cond,
                body,
                step,
                label,
                arrow_body,
                ..
            } => {
                let memory_multiplier = self.memory_control_multiplier;
                self.memory_control_multiplier = None;
                if let Some((n, label_span)) = label {
                    self.declare_loop_label(n, *label_span);
                }
                self.push_scope();
                self.check_binding(init);
                crate::Sema::Effects::record_authority_alias(self, init);
                self.require_bool(cond, "a counted loop condition");
                self.push_loop_value_frame(label.as_ref());
                self.push_loop_break_frame();
                self.loop_depth += 1;
                let before_loop = self.flow.clone();
                let previous_arrow_loop_body = self.arrow_loop_body;
                self.arrow_loop_body = *arrow_body;
                self.check_block(body, true);
                self.arrow_loop_body = previous_arrow_loop_body;
                if let Some(step) = step {
                    self.check_stmt(step.as_mut());
                }
                let after_body = self.flow.clone();
                self.join_loop_flow(
                    &before_loop,
                    &after_body,
                    !matches!(cond.without_parens(), Expr::Bool(true, _)),
                );
                self.loop_depth -= 1;
                self.pop_loop_value_frame();
                self.pop_scope();
                if label.is_some() {
                    self.loop_labels.pop();
                }
                self.memory_control_multiplier = memory_multiplier;
            }
            Stmt::Loop {
                body: inner,
                label,
                arrow_body,
                ..
            } => {
                let memory_multiplier = self.memory_control_multiplier;
                self.memory_control_multiplier = None;
                if let Some((n, label_span)) = label {
                    self.declare_loop_label(n, *label_span);
                }
                self.push_loop_value_frame(label.as_ref());
                self.push_loop_break_frame();
                self.loop_depth += 1;
                let before_loop = self.flow.clone();
                let previous_arrow_loop_body = self.arrow_loop_body;
                self.arrow_loop_body = *arrow_body;
                self.check_block(inner, true);
                self.arrow_loop_body = previous_arrow_loop_body;
                let after_body = self.flow.clone();
                self.join_loop_flow(&before_loop, &after_body, false);
                self.loop_depth -= 1;
                self.pop_loop_value_frame();
                if label.is_some() {
                    self.loop_labels.pop();
                }
                self.memory_control_multiplier = memory_multiplier;
            }
            Stmt::Unsafe { body, .. } => {
                let prev = self.in_unsafe;
                self.in_unsafe = true;
                self.check_block(body, true);
                self.in_unsafe = prev;
            }
            // D-CTEFFECT1: `#Impure("reason") { … }` — the Tier-2 comptime effect
            // gate. At runtime (which is what sema is checking here), this block is
            // semantically a plain block: it has no runtime significance. The gate is
            // enforced only inside the comptime interpreter. L3102 fires when no
            // reason was given.
            Stmt::Impure {
                reason, body, span, ..
            } => {
                let policy_allowed =
                    self.audited_gate_allowed(crate::Policy::PolicyKey::Impure, *span);
                if reason.is_none() {
                    self.diags.push(Diagnostic::lint(
                        "L3102",
                        "this `#Impure` block has no reason".to_string(),
                        "every comptime effect gate records why ambient I/O is needed".to_string(),
                        "add the reason: `#Impure(\"reading build config\") { … }`".to_string(),
                        Some(*span),
                    ));
                }
                if policy_allowed {
                    self.ct_impure_depth += 1;
                }
                self.check_block(body, true);
                if policy_allowed {
                    self.ct_impure_depth -= 1;
                }
            }
            // D-SHIELDNAME1=A: `#Shield { … }` — a cancellation-shield region.
            // Legal anywhere ordinary statements are; a no-op outside a task.
            // Semantically a plain block: check the body, no effects, no gate.
            Stmt::Shield { body, .. } => {
                self.check_block(body, true);
            }
            // D-REACTCORE1: `#Reactive { … }` — a reactive effect scope.
            Stmt::Reactive { body, span } => {
                if self.in_comptime {
                    self.diags.push(Diagnostic::error(
                            "E2914",
                            "`#Reactive` can't run at comptime".to_string(),
                            "reactive effects subscribe to runtime signals and re-run when they change (D-REACTCORE1)"
                                .to_string(),
                            "move `#Reactive { … }` out of the `comptime` block".to_string(),
                            Some(*span),
                        ));
                }
                let mut captures = crate::Sema::block_free_var_reads(body)
                    .into_iter()
                    .collect::<Vec<_>>();
                captures.sort();
                for name in captures {
                    if self.is_view(&name) {
                        let read_only = self
                            .view_facts(&name)
                            .iter()
                            .all(|fact| fact.access == ViewAccess::Read);
                        let copy_ty =
                            self.lookup(&name)
                                .map(|info| info.ty.clone())
                                .and_then(|ty| {
                                    crate::Sema::Diagnostics::owned_type_for_read_view(&ty).or_else(
                                        || {
                                            (self.is_string_view(&name)
                                                && matches!(ty, Type::String))
                                            .then_some(Type::String)
                                        },
                                    )
                                });
                        if !self.copies_explicit()
                            && read_only
                            && copy_ty
                                .as_ref()
                                .is_some_and(|ty| is_cloneable(ty, self.registry))
                        {
                            continue;
                        }
                        self.report_view_escape(&name, "be captured by a reactive effect", *span);
                    }
                }
                // The body runs later in its own move closure. Mutating a
                // cloned reactive capture must not make an enclosing
                // callback FnMut; only construction-time clone reads cross
                // that callback boundary.
                let enclosing_mut_captures = std::mem::take(&mut self.inferred_lambda_mut_captures);
                self.check_block(body, true);
                self.inferred_lambda_mut_captures = enclosing_mut_captures;
            }
            Stmt::Switched { marker, body, .. } if crate::AST::switched_off(marker) => {
                let flow = self.flow.clone();
                let fx_direct = self.fx_direct.clone();
                let fx_direct_spans = self.fx_direct_spans.clone();
                let fx_edges = self.fx_edges.clone();
                let fx_maximal = self.fx_maximal;
                let fx_maximal_span = self.fx_maximal_span;
                let region_stack = self.region_stack.clone();
                let fx_regions = self.fx_regions.clone();
                let fx_authority_delegations = self.fx_authority_delegations.clone();
                let fx_callback_obligations = self.fx_callback_obligations.clone();
                let fx_memory_events = self.fx_memory_events.clone();
                let fx_memory_open = self.fx_memory_open.clone();
                let memory_policy_stack = self.memory_policy_stack.clone();
                let fx_memory_regions = self.fx_memory_regions.clone();
                let fx_memory_unbounded_control = self.fx_memory_unbounded_control.clone();
                let fx_memory_calls = self.fx_memory_calls.clone();
                let memory_control_multiplier = self.memory_control_multiplier;
                let prev_suppress = self.suppress_must_use;
                self.suppress_must_use = true;
                self.push_scope();
                for stmt in body {
                    self.check_stmt(stmt);
                }
                self.drop_scope_no_obligation_checks();
                self.suppress_must_use = prev_suppress;
                self.flow = flow;
                self.fx_direct = fx_direct;
                self.fx_direct_spans = fx_direct_spans;
                self.fx_edges = fx_edges;
                self.fx_maximal = fx_maximal;
                self.fx_maximal_span = fx_maximal_span;
                self.region_stack = region_stack;
                self.fx_regions = fx_regions;
                self.fx_authority_delegations = fx_authority_delegations;
                self.fx_callback_obligations = fx_callback_obligations;
                self.fx_memory_events = fx_memory_events;
                self.fx_memory_open = fx_memory_open;
                self.memory_policy_stack = memory_policy_stack;
                self.fx_memory_regions = fx_memory_regions;
                self.fx_memory_unbounded_control = fx_memory_unbounded_control;
                self.fx_memory_calls = fx_memory_calls;
                self.memory_control_multiplier = memory_control_multiplier;
            }
            Stmt::Switched { body, .. } => {
                self.check_block(body, true);
            }
            // D-REGION1 (opt B): an explicit `region r { … }`. A fresh lexical
            // scope: arena `view`s allocated inside cannot escape it (the
            // E0631 escape rule is enforced against the scope floor, identical
            // to the implicit scope-inferred region of opt A). The region name
            // is documentary in v1 — it labels the scope for the reader; the
            // bound is the scope itself.
            Stmt::Region { body, .. } => {
                self.check_block(body, true);
            }
            Stmt::Policy {
                declarations,
                body,
                span,
            } => {
                self.enter_memory_policy_region(declarations.clone(), *span);
                self.check_block(body, true);
                self.exit_memory_policy_region();
            }
            // D-WRAP-SCOPE1=A: one lexical fixed-width arithmetic mode. The
            // mode is a sema fact only; codegen sees the ordinary shared
            // checked/wrapping/saturating operation after expression typing.
            Stmt::ScopeMember {
                name,
                args,
                body,
                span,
                ..
            } if name == crate::Syntax::MARKER_ARITHMETIC => {
                let Some(mode) = args.first().and_then(crate::AST::ArithmeticMode::from_expr)
                else {
                    self.diags.push(crate::Policy::marker_argument_shape_error(
                        crate::Syntax::MARKER_ARITHMETIC,
                        *span,
                    ));
                    self.check_block(body, true);
                    return;
                };
                self.arithmetic_policy_stack
                    .push(crate::AST::ArithmeticPolicyFact {
                        mode,
                        scope_span: *span,
                        operation_span: *span,
                        operation: None,
                    });
                self.check_block(body, true);
                self.arithmetic_policy_stack.pop();
            }
            // D-CONC-SPAWN1=D: `task.group g { … }` — structured task scope.
            Stmt::TaskGroup {
                name,
                name_span,
                limit,
                body,
                ..
            } => {
                if let Some(limit) = limit {
                    if let Some(limit_ty) = self.infer(limit) {
                        if !matches!(limit_ty, Type::InlineRange { .. }) {
                            self.check_type_assignable(&Type::Int, &limit_ty, limit.span());
                        }
                    }
                }
                self.push_scope();
                self.declare(
                    name,
                    *name_span,
                    LocalInfo {
                        def_span: *name_span,
                        binding_sigil_span: None,
                        ty: Type::Named(Syntax::TYPE_TASKGROUP.to_string()),
                        mutable: false,
                        param_conv: None,
                        decl_loop_depth: self.loop_depth,
                        interrupt_sendable: false,
                        reactive_local: false,
                        reactive_shared: false,
                        single_use_span: None,
                        constant_value: None,
                        invalid: false,
                    },
                );
                // The group name is the lexical handle that owns this scope;
                // `task` calls inside the body are rewritten to it even when
                // the source never spells the handle again. Count that
                // implicit ownership use so the label is not reported as an
                // unused local.
                self.mark_local_name_reference(name);
                self.taskgroup_stack
                    .push(TaskGroupCtx::new(name.clone(), *name_span));
                self.check_block(body, false);
                self.mark_taskgroup_spawns_owned(TaskGroupOrigin::Lexical);
                self.taskgroup_stack.pop();
                self.pop_scope();
            }
            // D-LAYOUT1: `layout NAME { … }` — a
            // Cassowary-style constraint block. Unlike `region`/`task.group`,
            // `name` is declared in the CURRENT scope (not pushed/popped
            // around it) so the handle outlives the block — later code reads
            // solved values (`NAME.value(v)`) or calls `NAME.suggest(...)`.
            // The parser already desugared every `box.anchor` read into a
            // `NAME.h(box, anchor)`/`NAME.v(box, anchor)` call, so every line
            // is an ordinary expression checked by the general GATE-1/GATE-2
            // machinery (`infer_binary`'s layout block, `layout_method_return`)
            // — the only layout-specific rule left to enforce here is that
            // each line's RESULT is a `Constraint` (E2933 otherwise).
            Stmt::Layout {
                name,
                name_span,
                body,
                ..
            } => {
                self.declare(
                    name,
                    *name_span,
                    LocalInfo {
                        def_span: *name_span,
                        binding_sigil_span: None,
                        ty: Type::Named(Syntax::LAYOUT_TYPE.to_string()),
                        mutable: false,
                        param_conv: None,
                        decl_loop_depth: self.loop_depth,
                        interrupt_sendable: false,
                        reactive_local: false,
                        reactive_shared: false,
                        single_use_span: None,
                        constant_value: None,
                        invalid: false,
                    },
                );
                self.push_scope();
                // D-LAYOUT1: E2934 (lint) — a constraint line that is a
                // byte-for-byte structural duplicate of an earlier one in the
                // SAME block is almost always a copy-paste mistake (a real,
                // if narrow, notion of "redundant": exact duplicates only —
                // proving general LP redundancy, i.e. "implied by the
                // others", is a much larger problem than this lint needs).
                let mut seen_constraints: HashSet<String> = HashSet::new();
                for stmt in body.iter_mut() {
                    if let Stmt::Expr(_) = stmt {
                        let Stmt::Expr(e) = stmt else { unreachable!() };
                        let fp = layout_constraint_fingerprint(e);
                        let t = self.infer(e);
                        let is_constraint = matches!(&t, Some(Type::Named(n)) if n == Syntax::LAYOUT_CONSTRAINT_TYPE);
                        if !is_constraint && t.is_some() {
                            self.diags.push(Diagnostic::error(
                                    "E2933",
                                    format!(
                                        "this element inside `{} {} {}.{{ … }}` doesn't produce a constraint (found `{}`)",
                                        name,
                                        Syntax::SIGIL_BIND_IMMUT,
                                        Syntax::LAYOUT_TYPE,
                                        t.as_ref().map(|ty| ty.name()).unwrap_or_default()
                                    ),
                                    format!(
                                        "every element inside a `{}.{{ … }}` body must be a `>=`/`<=`/`==` comparison of layout values (a `Constraint`), comma-separated",
                                        Syntax::LAYOUT_TYPE
                                    ),
                                    "write a comparison, e.g. `label.width >= 80.0`".to_string(),
                                    Some(e.span()),
                                ));
                        } else if is_constraint && !seen_constraints.insert(fp) {
                            self.diags.push(Diagnostic::lint(
                                    "E2934",
                                    format!(
                                        "this constraint repeats one already written in this `{}.{{ … }}` body",
                                        Syntax::LAYOUT_TYPE
                                    ),
                                    "an exact duplicate constraint doesn't tighten the layout — it's almost always a copy-paste leftover".to_string(),
                                    "remove the duplicate line, or change it if a different constraint was meant".to_string(),
                                    Some(e.span()),
                                ));
                        }
                    } else if let Stmt::Val(_) = stmt {
                        let fp = if let Stmt::Val(b) = stmt {
                            layout_constraint_fingerprint(&b.init)
                        } else {
                            unreachable!()
                        };
                        self.check_stmt(stmt);
                        if let Stmt::Val(b) = stmt {
                            let bname = b.name.clone();
                            let name_span = b.name_span;
                            let is_constraint = self
                                    .lookup(&bname)
                                    .map(|info| {
                                        matches!(&info.ty, Type::Named(n) if n == Syntax::LAYOUT_CONSTRAINT_TYPE)
                                    })
                                    .unwrap_or(false);
                            if !is_constraint {
                                self.diags.push(Diagnostic::error(
                                        "E2933",
                                        format!(
                                            "this binding inside `{} {} {}.{{ … }}` doesn't capture a constraint",
                                            name,
                                            Syntax::SIGIL_BIND_IMMUT,
                                            Syntax::LAYOUT_TYPE
                                        ),
                                        format!(
                                            "every element inside a `{}.{{ … }}` body must be a `>=`/`<=`/`==` comparison of layout values (a `Constraint`), optionally captured with `::`",
                                            Syntax::LAYOUT_TYPE
                                        ),
                                        "bind a comparison: `c :: label.width >= 80.0`".to_string(),
                                        Some(name_span),
                                    ));
                            } else if !seen_constraints.insert(fp) {
                                self.diags.push(Diagnostic::lint(
                                        "E2934",
                                        format!(
                                            "this constraint repeats one already written in this `{}.{{ … }}` body",
                                            Syntax::LAYOUT_TYPE
                                        ),
                                        "an exact duplicate constraint doesn't tighten the layout — it's almost always a copy-paste leftover".to_string(),
                                        "remove the duplicate line, or change it if a different constraint was meant".to_string(),
                                        Some(name_span),
                                    ));
                            }
                        }
                    } else {
                        self.diags.push(Diagnostic::error(
                                "E2933",
                                format!(
                                    "only constraint elements belong directly inside a `{}.{{ … }}` body",
                                    Syntax::LAYOUT_TYPE
                                ),
                                format!(
                                    "every element inside a `{}.{{ … }}` body must be a `>=`/`<=`/`==` comparison of layout values (a `Constraint`), optionally captured with `::`",
                                    Syntax::LAYOUT_TYPE
                                ),
                                "write a comparison, e.g. `label.width >= 80.0`".to_string(),
                                Some(stmt.span()),
                            ));
                    }
                }
                self.pop_scope();
            }
            // D-EFF1 / D-ABILITY-NAME2: bare `#FX(Net, DB) { … }`
            // restricts effects. A named `#FX(grant: FS, Net) { … }`
            // also binds a scoped Authority handle and uses the same
            // subset check for both forms.
            Stmt::AuthorityScope {
                caps,
                caps_span,
                binding,
                binding_span,
                body,
                span,
                ..
            } => {
                let mut cap_set = crate::Sema::EffectSet::new();
                let mut bad = false;
                for (name, span) in caps.iter() {
                    match crate::Sema::resolve_effect_name(name, self.effect_facts) {
                        Ok(e) => {
                            if let Some(diagnostic) =
                                crate::Sema::reject_positive_deny_only_effect(&e, *span)
                            {
                                self.diags.push(diagnostic);
                                bad = true;
                            } else {
                                cap_set.insert(e);
                            }
                        }
                        Err(suggestion) => {
                            if crate::Sema::parse_effect_name(name).is_some() {
                                self.diags.push(crate::Sema::undeclared_effect(
                                    name,
                                    suggestion.as_deref(),
                                    Some(*span),
                                ));
                            } else {
                                self.diags.push(unknown_effect(name, *span));
                            }
                            bad = true;
                        }
                    }
                }
                // D-AUTHORITY-MODEL1: a nested scope may attenuate the
                // enclosing grant, but it cannot mint a right the outer
                // scope did not hold. Check the declaration boundary before
                // walking the body so an empty nested block cannot widen by
                // accident.
                if let Some(outer) = self.region_stack.last() {
                    let widening = crate::Sema::effects_uncovered(&cap_set, &outer.caps);
                    if !widening.is_empty() {
                        self.diags.push(crate::Sema::e0712(
                            &widening,
                            &outer.caps,
                            *caps_span,
                            crate::Syntax::KW_FX,
                        ));
                        bad = true;
                    }
                }
                if let Some(binding) = binding {
                    let binding_span = binding_span
                        .as_ref()
                        .copied()
                        .expect("named #FX binding has a span");
                    self.push_scope();
                    self.declare_loop_var(
                        binding.clone(),
                        binding_span,
                        &Type::Named(crate::Syntax::AUTHORITY_HANDLE_TYPE.to_string()),
                    );
                }
                self.region_stack.push(crate::Sema::RegionAccum {
                    binding: binding.clone(),
                    aliases: std::collections::BTreeMap::new(),
                    caps: cap_set,
                    caps_span: *caps_span,
                    direct: crate::Sema::EffectSet::new(),
                    edges: std::collections::BTreeSet::new(),
                    maximal: false,
                });
                self.check_block(body, true);
                let acc = self.region_stack.pop().expect("pushed above");
                if let Some(binding) = binding {
                    for delegation in crate::Sema::authority_delegations(body, binding, *span) {
                        if !self.fx_authority_delegations.iter().any(|existing| {
                            existing.span == delegation.span
                                && existing.resource == delegation.resource
                                && existing.operation == delegation.operation
                        }) {
                            self.fx_authority_delegations.push(delegation);
                        }
                    }
                    if let Some(escape_span) = grant_handle_escape(body, binding) {
                        self.diags.push(crate::Sema::e0711(
                            binding,
                            crate::Syntax::KW_FX,
                            escape_span,
                        ));
                    }
                    self.pop_scope();
                }
                // Skip the subset check when a cap name was invalid (the cap
                // set is incomplete) — E0119 is the real problem to fix.
                if !bad {
                    self.fx_regions.push(crate::Sema::RegionSummary {
                        caps: acc.caps,
                        direct: acc.direct,
                        edges: acc.edges,
                        maximal: acc.maximal,
                        caps_span: acc.caps_span,
                    });
                }
            }
            // `#Context(field: value) { … }`.
            // Type-check each field value: `allocator` must be an allocator
            // handle type; `deadline` must be an Int epoch-ms instant; `logger`
            // is currently unconstrained. E0762 on mismatch.
            // Q1 = A2: explicit allocator args at call sites override the
            // ambient — no static binding done here, only type validation and
            // block body checking. Q2 = Cβ: restore is per-block (RAII guard).
            // D-TERM1 (ratified 2026-06-22): `live { … }` — terminal direct-input
            // block. No type-checking beyond the body; the block is impure (IO
            // effect), so it is rejected inside `#Pure fn` (same rule as `io.input`).
            // `use core.term` is NOT required to write a `live` block — the block
            // is its own syntactic gate. `term.read_key()` does need the import.
            // E3301: freestanding builds have no terminal device.
            Stmt::Live { body, span } => {
                if self.in_pure {
                    self.diags.push(crate::Sema::e3401(
                        &self.fn_name.clone(),
                        "#Live { … }",
                        &[],
                        *span,
                    ));
                }
                if self.freestanding {
                    self.diags.push(crate::Sema::e3301(
                            "#Live { … }",
                            "Terminal I/O requires an OS terminal device. Build without `--freestanding`.",
                            *span,
                        ));
                }
                self.check_block(body, true);
            }
            // D-DOTSCOPE1: a scope-member statement (`.setup`/`.expect_fail`/
            // `.timeout`/`.skip` inside a `#Test` block). Member legality, args,
            // position, and nesting are validated by the `ScopeMembers` pass; here
            // the checker type-checks the region body's ordinary statements.
            // `.timeout` is the one member with a value argument: its canonical
            // Time literal or binding is inferred here, after `.setup` bindings
            // have entered scope, and must be a Duration.
            // `.setup` is init sugar: its bindings leak into the test scope (no new
            // scope), so the rest of the body can use them. Every other member is
            // its own region (a closure / block / dead branch in codegen), so its
            // bindings are scoped — referencing them later is a normal unknown-name
            // error, never reaching codegen.
            Stmt::ScopeMember {
                name, args, body, ..
            } => {
                if name == crate::Syntax::SCOPE_TEST_TIMEOUT {
                    if let [arg] = args.as_mut_slice() {
                        let expected = Type::Named(crate::Syntax::DURATION_TYPE.to_string());
                        if let Some(got) = self.infer_with_expected(arg, &expected) {
                            let reported = self.check_type_assignable(&expected, &got, arg.span());
                            if !reported && got != expected {
                                self.diags.push(Diagnostic::error(
                                    "E0112",
                                    format!(
                                        "`.timeout` wants {}, but this is {}",
                                        expected.show(),
                                        got.show()
                                    ),
                                    "a timeout budget is a Duration value".to_string(),
                                    type_fix_hint(&expected, &got),
                                    Some(arg.span()),
                                ));
                            }
                        }
                    }
                }
                // D-DSLBLOCK1: the optional SQL row header is a type
                // position, not a runtime expression. Validate it through
                // the ordinary declared-type rules so local, imported,
                // generic, alias, and unknown names all receive the same
                // visibility and teaching diagnostics as any other type.
                let has_type_parameter = crate::Policy::applied_rule(name).is_some_and(|row| {
                    row.signature
                        .params
                        .first()
                        .is_some_and(|parameter| parameter.source_type == "Type")
                });
                if has_type_parameter {
                    if let [Expr::Ident(type_name, type_span)] = args.as_slice() {
                        self.check_declared_type_rules(&Type::Named(type_name.clone()), *type_span);
                    }
                }
                let leak = name == crate::Syntax::SCOPE_TEST_SETUP;
                self.check_block(body, !leak);
            }
            // D-DET1 (ratified 2026-06-22): `assume_deterministic { … }` — the
            // expert determinism-escape. Raise the suppression depth so the
            // determinism rejections inside a `#Pure fn` (E3403 non-deterministic
            // Core call / E3401 impure Core call) are suspended for the body. This
            // does NOT relax memory/type safety — only the determinism check. A
            // lexical scope; erased in codegen (I3 — a plain Rust block).
            Stmt::AssumeDet { body, span, .. } => {
                let policy_allowed =
                    self.audited_gate_allowed(crate::Policy::PolicyKey::Nondeterministic, *span);
                if policy_allowed {
                    self.det_suppress += 1;
                }
                self.check_block(body, true);
                if policy_allowed {
                    self.det_suppress -= 1;
                }
            }
            // `#Transact(name) { … }`.
            // Bind the user-chosen handle `name` (typed `Transaction`) so
            // `name.on_commit(() -> { … })` resolves inside the block, then check
            // the body with the transaction depth raised: an irreversible Core
            // effect (Net/FS/Exec) reached directly in the block is E0746
            // (D-TXN2) at its call site. A lexical scope; erased in codegen (I3).
            Stmt::Transact {
                name,
                name_span,
                body,
                implicit,
                ..
            } => {
                let implicit = *implicit;
                self.push_scope();
                if let (Some(name), Some(name_span)) = (name, name_span) {
                    self.declare_loop_var(
                        name.clone(),
                        *name_span,
                        &Type::Named(crate::Syntax::TXN_HANDLE_TYPE.to_string()),
                    );
                }
                self.txn_depth += 1;
                // D-CONC-SHARE1=A: only a transaction the author opened
                // raises the D-TXN2 effect wall. A synthesized
                // one-statement commit carries the commit plane alone.
                if !implicit {
                    self.txn_wall_depth += 1;
                }
                self.check_block(body, true);
                if !implicit {
                    self.txn_wall_depth -= 1;
                }
                self.txn_depth -= 1;
                self.pop_scope();
            }
            Stmt::ContextBlock { fields, body, span } => {
                let rule_fact = self.take_rule_fact(crate::Syntax::CTX_BLOCK, *span);
                let signature_checked = rule_fact.is_some();
                let validated = rule_fact.and_then(|mut marker| {
                    for (argument, (_, value, _)) in marker.args.iter_mut().zip(fields.iter_mut()) {
                        std::mem::swap(argument, value);
                    }
                    let validated = self.validate_rule_signature(&mut marker);
                    for (argument, (_, value, _)) in marker.args.iter_mut().zip(fields.iter_mut()) {
                        std::mem::swap(argument, value);
                    }
                    validated
                });
                let signature_valid = !signature_checked || validated.is_some();
                for (source_index, (field_name, value_expr, field_span)) in
                    fields.iter_mut().enumerate()
                {
                    let ty = if signature_checked {
                        validated.as_ref().and_then(|arguments| {
                            let binding = arguments
                                .bindings
                                .iter()
                                .find(|binding| binding.source_index == source_index)?;
                            if binding.ty == crate::Policy::RuleArgType::Any {
                                self.infer(value_expr)
                            } else {
                                arguments.type_for_source(source_index)
                            }
                        })
                    } else {
                        self.infer(value_expr)
                    };
                    match field_name.as_str() {
                        crate::Syntax::CTX_FIELD_ALLOCATOR => {
                            if !signature_valid {
                                continue;
                            }
                            // Must be one of the known allocator handle types.
                            let ok = match &ty {
                                Some(Type::Named(n)) => {
                                    crate::Syntax::alloc_handle_rust_type(n).is_some()
                                }
                                _ => false,
                            };
                            if !ok {
                                let got = ty
                                    .as_ref()
                                    .map(|t| t.show())
                                    .unwrap_or_else(|| "unknown".to_string());
                                self.diags.push(Diagnostic::error(
                                        "E0762",
                                        format!("`allocator` needs an allocator, got {}", got),
                                        "the `allocator` field takes an `Arena`, `Bump`, `Pool`, or `Fixed` value".to_string(),
                                        "pass an allocator, e.g. `mem.Arena.new()`".to_string(),
                                        Some(*field_span),
                                    ));
                            }
                        }
                        crate::Syntax::CTX_FIELD_LOGGER => {
                            // v1: any value accepted for logger; a future Logger
                            // type will narrow this. No E0762 for logger yet.
                            let _ = ty;
                        }
                        crate::Syntax::CTX_FIELD_DEADLINE => {
                            if !signature_valid {
                                continue;
                            }
                            if !matches!(ty, Some(Type::Int | Type::InlineRange { .. })) {
                                let got = ty
                                    .as_ref()
                                    .map(|t| t.show())
                                    .unwrap_or_else(|| "unknown".to_string());
                                self.diags.push(Diagnostic::error(
                                        "E0762",
                                        format!("`deadline` needs an Int epoch-millis instant, got {}", got),
                                        "the `deadline` field carries an absolute time budget in milliseconds".to_string(),
                                        "pass an Int, e.g. `time.now() + 200`".to_string(),
                                        Some(*field_span),
                                    ));
                            }
                        }
                        _ => {
                            // Parser already rejected unknown fields (E0761);
                            // this arm is unreachable in practice.
                        }
                    }
                }
                let has_allocator = fields
                    .iter()
                    .any(|(n, _, _)| n == crate::Syntax::CTX_FIELD_ALLOCATOR);
                let saved_depth = self.context_depth;
                let saved_alloc = self.context_allocator_active;
                self.context_depth += 1;
                if has_allocator {
                    self.context_allocator_active = true;
                }
                self.check_block(body, true);
                self.context_allocator_active = saved_alloc;
                self.context_depth = saved_depth;
            }
        }
    }
}

fn statically_bounded_for_iterations(kind: &ForKind) -> Option<u64> {
    match kind {
        ForKind::Range {
            start,
            end,
            step,
            exclusive,
        } => {
            let Expr::Int(start, _, _, _) = start else {
                return None;
            };
            let Expr::Int(end, _, _, _) = end else {
                return None;
            };
            let step = match step {
                Some(Expr::Int(step, _, _, _)) if *step > 0 => *step as i128,
                None => 1,
                _ => return None,
            };
            // Inclusive `a..b` is empty when b < a; exclusive `a..<b` when a >= b.
            if *exclusive {
                if *end <= *start {
                    return Some(0);
                }
                let iterations = ((*end as i128 - *start as i128) + step - 1) / step;
                return u64::try_from(iterations).ok();
            }
            if end < start {
                return Some(0);
            }
            let iterations = ((*end as i128 - *start as i128) / step) + 1;
            u64::try_from(iterations).ok()
        }
        ForKind::In {
            collection: Expr::ListLit(items, _),
            step,
        } => {
            let len = u64::try_from(items.len()).ok()?;
            let stride = match step {
                Some(Expr::Int(stride, _, _, _)) if *stride > 0 => *stride as u64,
                None => 1,
                _ => return None,
            };
            Some(len.div_ceil(stride))
        }
        ForKind::In { .. } => None,
    }
}

/// D-RANGE-EXCL1=C: only the exact shape `….len()` (no args) ends a range.
fn range_end_len_root(end: &Expr) -> Option<String> {
    match end {
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } if method == "len" && args.is_empty() => {
            collection_root_name(receiver).or_else(|| expr_root_ident(receiver).map(str::to_string))
        }
        _ => None,
    }
}

/// True when any statement in `body` indexes the named root with `index_var`.
fn stmts_index_root_with(body: &[Stmt], root: &str, index_var: &str) -> bool {
    body.iter()
        .any(|s| stmt_indexes_root_with(s, root, index_var))
}

fn stmt_indexes_root_with(stmt: &Stmt, root: &str, index_var: &str) -> bool {
    match stmt {
        Stmt::Expr(e) | Stmt::DeferClose { close: e, .. } | Stmt::Return(Some(e), _) => {
            expr_indexes_root_with(e, root, index_var)
        }
        Stmt::Val(b) => expr_indexes_root_with(&b.init, root, index_var),
        Stmt::Assign { target, value, .. } => {
            lvalue_indexes_root_with(target, root, index_var)
                || expr_indexes_root_with(value, root, index_var)
        }
        Stmt::While { cond, body, .. } => {
            expr_indexes_root_with(cond, root, index_var)
                || stmts_index_root_with(body, root, index_var)
        }
        Stmt::For { kind, body, .. } => {
            let kind_hits = match kind {
                ForKind::Range {
                    start, end, step, ..
                } => {
                    expr_indexes_root_with(start, root, index_var)
                        || expr_indexes_root_with(end, root, index_var)
                        || step
                            .as_ref()
                            .is_some_and(|s| expr_indexes_root_with(s, root, index_var))
                }
                ForKind::In { collection, step } => {
                    expr_indexes_root_with(collection, root, index_var)
                        || step
                            .as_ref()
                            .is_some_and(|s| expr_indexes_root_with(s, root, index_var))
                }
            };
            kind_hits || stmts_index_root_with(body, root, index_var)
        }
        Stmt::Loop { body, .. } | Stmt::Unsafe { body, .. } | Stmt::Region { body, .. } => {
            stmts_index_root_with(body, root, index_var)
        }
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            expr_indexes_root_with(&init.init, root, index_var)
                || expr_indexes_root_with(cond, root, index_var)
                || step
                    .as_ref()
                    .is_some_and(|s| stmt_indexes_root_with(s, root, index_var))
                || stmts_index_root_with(body, root, index_var)
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
            expr_indexes_root_with(subject, root, index_var)
                || arms.iter().any(|a| {
                    expr_indexes_root_with(&a.cond, root, index_var)
                        || stmts_index_root_with(&a.body, root, index_var)
                })
                || else_body
                    .as_ref()
                    .is_some_and(|b| stmts_index_root_with(b, root, index_var))
        }
        _ => false,
    }
}

fn lvalue_indexes_root_with(lv: &LValue, root: &str, index_var: &str) -> bool {
    match lv {
        LValue::Index { base, index, .. } => {
            (expr_root_ident(base) == Some(root) && expr_is_ident(index, index_var))
                || expr_indexes_root_with(base, root, index_var)
                || expr_indexes_root_with(index, root, index_var)
        }
        LValue::Local { .. } => false,
        _ => false,
    }
}

fn expr_is_ident(expr: &Expr, name: &str) -> bool {
    matches!(expr, Expr::Ident(n, _) if n == name)
}

fn expr_indexes_root_with(expr: &Expr, root: &str, index_var: &str) -> bool {
    match expr {
        Expr::Index { base, index, .. } => {
            (expr_root_ident(base) == Some(root) && expr_is_ident(index, index_var))
                || expr_indexes_root_with(base, root, index_var)
                || expr_indexes_root_with(index, root, index_var)
        }
        Expr::Str(parts, _) => parts.iter().any(|p| match p {
            StrPart::Interp(inner, _) => expr_indexes_root_with(inner, root, index_var),
            StrPart::Lit(_) => false,
        }),
        Expr::Call(c) => c
            .args
            .iter()
            .any(|a| expr_indexes_root_with(&a.expr, root, index_var)),
        Expr::CallValue { callee, args, .. } => {
            expr_indexes_root_with(callee, root, index_var)
                || args
                    .iter()
                    .any(|a| expr_indexes_root_with(&a.expr, root, index_var))
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_indexes_root_with(receiver, root, index_var)
                || args
                    .iter()
                    .any(|a| expr_indexes_root_with(&a.expr, root, index_var))
        }
        Expr::Field(inner, _, _)
        | Expr::Spread(inner, _)
        | Expr::Copy(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Present(inner, _)
        | Expr::Tainted(inner, _, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _) => expr_indexes_root_with(inner, root, index_var),
        Expr::Try(inner, _, _, note) => {
            expr_indexes_root_with(inner, root, index_var)
                || note
                    .as_deref()
                    .is_some_and(|note| expr_indexes_root_with(note, root, index_var))
        }
        Expr::Binary(_, left, right, _) => {
            expr_indexes_root_with(left, root, index_var)
                || expr_indexes_root_with(right, root, index_var)
        }
        Expr::ListLit(items, _) => items
            .iter()
            .any(|e| expr_indexes_root_with(e, root, index_var)),
        Expr::TupleLit(items, _, _) => items
            .iter()
            .any(|(_, e)| expr_indexes_root_with(e, root, index_var)),
        Expr::Slice {
            base,
            start,
            end,
            range,
            ..
        } => {
            expr_indexes_root_with(base, root, index_var)
                || range.as_deref().map_or_else(
                    || {
                        expr_indexes_root_with(start, root, index_var)
                            || expr_indexes_root_with(end, root, index_var)
                    },
                    |range| expr_indexes_root_with(range, root, index_var),
                )
        }
        Expr::OrFallback { value, .. } => expr_indexes_root_with(value, root, index_var),
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            expr_indexes_root_with(cond, root, index_var)
                || stmts_index_root_with(then_body, root, index_var)
                || expr_indexes_root_with(then_value, root, index_var)
                || stmts_index_root_with(else_body, root, index_var)
                || expr_indexes_root_with(else_value, root, index_var)
        }
        _ => false,
    }
}

/// S17 / L0503: `place = place op rhs` → prefer `place op= rhs`.
/// Indexed places are excluded (compound assign there is E0164).
fn prefer_compound_assign(
    target: &LValue,
    value: &Expr,
) -> Option<(String, crate::AST::BinOp, &'static str)> {
    if matches!(target, LValue::Index { .. }) {
        return None;
    }
    let Expr::Binary(op, left, _right, _) = value.without_parens() else {
        return None;
    };
    let compound = op.compound_spell()?;
    if !lvalue_same_place(target, left) {
        return None;
    }
    Some((lvalue_spell(target)?, *op, compound))
}

fn lvalue_same_place(lv: &LValue, expr: &Expr) -> bool {
    match (lv, expr.without_parens()) {
        (LValue::Local { name, .. }, Expr::Ident(n, _)) => name == n,
        (LValue::Field { base, field, .. }, Expr::Field(b, f, _)) => {
            field == f && expr_same_place(base, b)
        }
        _ => false,
    }
}

fn expr_same_place(a: &Expr, b: &Expr) -> bool {
    match (a.without_parens(), b.without_parens()) {
        (Expr::Ident(n1, _), Expr::Ident(n2, _)) => n1 == n2,
        (Expr::Field(b1, f1, _), Expr::Field(b2, f2, _)) => f1 == f2 && expr_same_place(b1, b2),
        (
            Expr::Index {
                base: b1,
                index: i1,
                ..
            },
            Expr::Index {
                base: b2,
                index: i2,
                ..
            },
        ) => expr_same_place(b1, b2) && expr_same_place(i1, i2),
        _ => false,
    }
}

fn lvalue_spell(lv: &LValue) -> Option<String> {
    match lv {
        LValue::Local { name, .. } => Some(name.clone()),
        LValue::Field { base, field, .. } => Some(format!("{}.{}", expr_place_spell(base)?, field)),
        LValue::Index { .. } => None,
    }
}

fn expr_place_spell(expr: &Expr) -> Option<String> {
    match expr.without_parens() {
        Expr::Ident(name, _) => Some(name.clone()),
        Expr::Field(base, field, _) => Some(format!("{}.{}", expr_place_spell(base)?, field)),
        Expr::Index { base, index, .. } => Some(format!(
            "{}[{}]",
            expr_place_spell(base)?,
            expr_place_spell(index).unwrap_or_else(|| "…".to_string())
        )),
        _ => None,
    }
}
