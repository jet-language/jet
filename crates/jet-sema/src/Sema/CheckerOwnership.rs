use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::{is_type_var_name, substitute_type};
use crate::Collections;
use crate::Syntax;
use crate::AST::{
    AccessConvention, ElseBranch, Expr, ForKind, Lambda, LambdaBody, LValue, Pattern, Stmt, Type,
    UnOp, VariantPayload,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct EvaluatedAccess {
    place: ViewPlace,
    capture_place: ViewPlace,
    capture_ty: Option<Type>,
    capture_is_view: bool,
    access: ViewAccess,
    span: Span,
    through_call: bool,
    moves_owner: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AccessWalkMode {
    EvaluateNow,
    ConstructCaptures,
    CaptureRequirements,
}

fn const_place_int(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Int(value, _, _, _) => Some(*value),
        Expr::Unary(UnOp::Neg, inner, _) => const_place_int(inner)?.checked_neg(),
        Expr::Paren(inner, _) => const_place_int(inner),
        _ => None,
    }
}

fn named_view_field_path(expr: &Expr, fields: &mut Vec<String>) -> Option<String> {
    match expr {
        Expr::Ident(name, _) => Some(name.clone()),
        Expr::Field(base, field, _) => {
            let name = named_view_field_path(base, fields)?;
            fields.push(field.clone());
            Some(name)
        }
        _ => None,
    }
}

fn append_view_source_projections(
    place: &mut ViewPlace,
    projections: &[crate::AST::ViewSourceProjection],
    span: Span,
) {
    for projection in projections {
        place.projections.push(match projection {
            crate::AST::ViewSourceProjection::Field(name) => {
                ViewProjection::Field(name.clone())
            }
            crate::AST::ViewSourceProjection::Index => ViewProjection::Index {
                value: None,
                span,
            },
            crate::AST::ViewSourceProjection::Range => ViewProjection::Range {
                start: None,
                end: None,
                span,
            },
        });
    }
}

impl<'a> Checker<'a> {
    pub(crate) fn is_resource_type(&self, ty: &Type) -> bool {
        matches!(
            ty,
            Type::Named(name) | Type::Apply { name, .. }
                if self.trait_reg.implements_trait(name, Syntax::TRAIT_CLOSE)
        )
    }

    // ──────────────────────────────────────────────────────────────────────
    // D-ALLOC2 / D-REGION1 (ratified 2026-06-21): scope-bound arena `view`s.
    //
    // `x :: arena.alloc(v)` makes `x` a *view* into `arena`'s storage — Rust
    // `&'arena mut T`. The view is sound only while it stays inside its region
    // (the lexical scope of the `arena` binding / an explicit `region`) and only
    // until `arena` is reset or closed. Two diagnostics enforce that, both
    // *strictly* at least as strict as Rust's borrow checker, so every
    // Jet-accepted program is rustc-accepted (I2: Jet rejects first):
    //   * E0631 — the view escapes its region (returned, stored in a binding /
    //     ref / struct field, passed where ownership/`mut` is taken, captured by
    //     an escaping closure).
    //   * E0632 — the view is used after the backing arena was reset.
    //
    // v1 restriction (I8): views are non-reassignable, non-escaping locals; we
    // reject anything the analysis can't prove with a teaching error rather than
    // attempt a clever lowering.
    // ──────────────────────────────────────────────────────────────────────

    /// If `init` is `arena.alloc(value)` on a name, return the arena's name.
    pub(crate) fn arena_alloc_source(&self, init: &Expr) -> Option<String> {
        if let Expr::MethodCall {
            receiver, method, ..
        } = init
        {
            if method == Syntax::MEM_ALLOC_ALLOC {
                if let Expr::Ident(arena, _) = &**receiver {
                    if self.lookup(arena).is_some_and(|i| is_allocator_type(&i.ty)) {
                        return Some(arena.clone());
                    }
                }
            }
        }
        None
    }

    /// Record `name` as a view into `arena`, declared at the current scope depth.
    pub(crate) fn record_arena_view(&mut self, name: &str, arena: String, span: Span) {
        let place = ViewPlace {
            owner: self.owner_id(&arena),
            projections: vec![ViewProjection::Fresh(span)],
        };
        self.record_view(name, Vec::new(), place, ViewKind::Arena, ViewAccess::Write, span);
    }

    /// Return the user buffer borrowed by `Fixed.over`, or a synthetic local
    /// owner for `Fixed.new`'s compiler-generated inline backing.
    pub(crate) fn fixed_backing_source(&self, init: &Expr) -> Option<String> {
        let Expr::MethodCall { receiver, method, args, .. } = init else {
            return None;
        };
        if !matches!(&**receiver, Expr::Field(_, name, _) if name == "Fixed") {
            return None;
        }
        match method.as_str() {
            "new" => Some("<inline Fixed backing>".to_string()),
            "over" => args.first().and_then(|arg| match &arg.expr {
                Expr::Ident(name, _) => Some(name.clone()),
                _ => None,
            }),
            _ => None,
        }
    }

    pub(crate) fn record_fixed_backing(&mut self, name: &str, owner: String, span: Span) {
        let place = ViewPlace {
            owner: if owner == "<inline Fixed backing>" {
                ViewOwnerId {
                    name: owner,
                    def_span: span,
                    origin: ViewOwnerOrigin::Local,
                }
            } else {
                self.owner_id(&owner)
            },
            projections: Vec::new(),
        };
        self.record_view(
            name,
            Vec::new(),
            place,
            ViewKind::FixedBacking,
            ViewAccess::Write,
            span,
        );
    }

    /// E0632: when `arena` is reset, every live view into it dies.
    pub(crate) fn kill_views_of_arena(&mut self, arena: &str, verb: &str, span: Span) {
        let owner = self.owner_id(arena);
        self.view_facts.invalidate_owner(&owner, verb, span);
    }

    /// E0632: reading a view whose arena was already reset.
    pub(crate) fn check_view_use(&mut self, name: &str, span: Span) {
        self.views_used_in_stmt.insert(name.to_string());
        if let Some(info) = self.view_fact(name) {
            if let Some((verb, _kill_span)) = &info.invalidated {
                let arena = info.place.owner.name.clone();
                let verb = verb.clone();
                self.diags.push(Diagnostic::error(
                    "E0632",
                    format!("`{}` was {} here, so the value `{}` points into is gone", arena, verb, name),
                    format!(
                        "`{}` is a view into `{}`; `{}.{}()` invalidated everything stored in `{}`, so reading `{}` now would read freed memory",
                        name, arena, arena, verb, arena, name
                    ),
                    format!(
                        "use `{}` before `{}.{}()`, or re-`alloc` after the {} to get a fresh value",
                        name, arena, verb, verb
                    ),
                    Some(span),
                ));
            }
        }
    }

    /// E0631: a view named `name` is escaping its region (used where it would
    /// outlive `arena`). `what` describes the escape site for the message.
    pub(crate) fn report_view_escape(&mut self, name: &str, what: &str, span: Span) {
        let Some(fact) = self.view_fact(name).cloned() else {
            return;
        };
        match fact.kind {
            ViewKind::Arena => self.diags.push(Diagnostic::error(
                "E0631",
                format!("`{}` cannot be shared — it does not live long enough to {}", name, what),
                format!(
                    "`{}` is a view into `{}`; sharing it outside the region would let it outlive `{}` and point into freed memory",
                    name, fact.place.owner.name, fact.place.owner.name
                ),
                format!(
                    "keep `{}` inside the `{}` region, or copy what you need out with `{}` before it leaves",
                    name, fact.place.owner.name, Syntax::SIGIL_COPY
                ),
                Some(span),
            )),
            ViewKind::FixedBacking => self.diags.push(Diagnostic::error(
                "E0631",
                format!("`{}` cannot be shared — its fixed backing does not live long enough to {}", name, what),
                format!(
                    "`{}` exclusively borrows `{}`; moving or capturing the handle could outlive that inline storage",
                    name, fact.place.owner.name
                ),
                "keep the Fixed handle in its declaring lexical scope and close it after its last allocation view".to_string(),
                Some(span),
            )),
            ViewKind::String => self.report_string_view_unsupported_use(name, what, span),
            ViewKind::List | ViewKind::Buffer | ViewKind::Matrix => {
                self.diags.push(Diagnostic::error(
                    "E2305",
                    format!(
                        "`{}` cannot be shared — it does not live long enough to {}",
                        name, what
                    ),
                    format!(
                        "`{}` is a view into `{}`; sharing it outside `{}`'s scope would let it outlive its owner and point into freed storage",
                        name, fact.place.owner.name, fact.place.owner.name
                    ),
                    format!(
                        "keep `{}` inside `{}`'s scope, or copy what you need before it leaves",
                        name, fact.place.owner.name
                    ),
                    Some(span),
                ));
            }
        }
    }

    /// True if `name` is currently a live arena view. Used to gate escape checks
    /// at the use sites (return / bind / move-arg / struct field).
    pub(crate) fn is_arena_view(&self, name: &str) -> bool {
        self.view_kind(name) == Some(ViewKind::Arena)
    }

    pub(crate) fn is_fixed_backing_view(&self, name: &str) -> bool {
        self.view_kind(name) == Some(ViewKind::FixedBacking)
    }

    pub(crate) fn reject_fixed_storage(&mut self, expr: &Expr, what: &str) {
        if let Expr::Ident(name, span) = expr {
            if self.is_fixed_backing_view(name) {
                self.report_view_escape(name, what, *span);
            }
        }
    }

    fn owner_id(&self, owner: &str) -> ViewOwnerId {
        let def_span = self
            .lookup(owner)
            .map(|info| info.def_span)
            .unwrap_or_else(|| Span::new(0, 0));
        if owner == Syntax::KW_SELF {
            ViewOwnerId {
                name: owner.to_string(),
                def_span,
                origin: ViewOwnerOrigin::Receiver,
            }
        } else if self.consts.contains_key(owner) {
            ViewOwnerId {
                name: owner.to_string(),
                def_span,
                origin: ViewOwnerOrigin::Static,
            }
        } else if self.lookup(owner).is_some_and(|info| info.param_conv.is_some()) {
            let index = self
                .current_param_names
                .iter()
                .position(|name| name == owner)
                .unwrap_or(0);
            ViewOwnerId {
                name: owner.to_string(),
                def_span,
                origin: ViewOwnerOrigin::Parameter(index),
            }
        } else {
            ViewOwnerId {
                name: owner.to_string(),
                def_span,
                origin: ViewOwnerOrigin::Local,
            }
        }
    }

    fn record_view(
        &mut self,
        name: &str,
        output_path: Vec<String>,
        place: ViewPlace,
        kind: ViewKind,
        access: ViewAccess,
        span: Span,
    ) {
        if self.view_facts.bindings.iter().any(|(name, fact)| {
            self.view_is_live_now(name)
                && fact.invalidated.is_none()
                && (fact.access == ViewAccess::Write || access == ViewAccess::Write)
                && fact.place.overlaps(&place)
        }) {
            let place_name = Self::place_name(&place);
            self.diags.push(Diagnostic::error(
                "E0212",
                format!("`{place_name}` already has a live view that conflicts with `{name}`"),
                "many read views may overlap, but an exclusive mutable view cannot overlap any other live view".to_string(),
                "finish using the earlier view before creating this one, or make an owned copy".to_string(),
                Some(span),
            ));
        }
        let fact = ViewFact {
            binding_span: span,
            output_path,
            place,
            kind,
            access,
            scope_len: self.scopes.len(),
            invalidated: None,
        };
        self.view_facts.push(name.to_string(), fact);
    }

    fn view_fact(&self, name: &str) -> Option<&ViewFact> {
        let binding = self.lookup(name)?;
        self.view_facts.current_for_binding(name, binding.def_span)
    }

    fn view_facts(&self, name: &str) -> Vec<&ViewFact> {
        let Some(binding) = self.lookup(name) else {
            return Vec::new();
        };
        self.view_facts.all_for_binding(name, binding.def_span)
    }

    fn view_fact_at_path(&self, name: &str, output_path: &[String]) -> Option<&ViewFact> {
        self.view_facts(name)
            .into_iter()
            .filter(|fact| output_path.starts_with(&fact.output_path))
            .max_by_key(|fact| fact.output_path.len())
    }

    fn compose_view_source_place(
        &self,
        actual: &Expr,
        projections: &[crate::AST::ViewSourceProjection],
        span: Span,
    ) -> Option<ViewPlace> {
        let leading_fields: Vec<String> = projections
            .iter()
            .map_while(|projection| match projection {
                crate::AST::ViewSourceProjection::Field(name) => Some(name.clone()),
                _ => None,
            })
            .collect();
        if let Expr::Ident(name, _) = actual {
            if let Some(fact) = self.view_fact_at_path(name, &leading_fields) {
                let mut place = fact.place.clone();
                append_view_source_projections(
                    &mut place,
                    &projections[fact.output_path.len()..],
                    span,
                );
                return Some(place);
            }
        }
        let mut place = self.place_from_expr(actual)?;
        append_view_source_projections(&mut place, projections, span);
        Some(place)
    }

    fn view_is_live_now(&self, name: &str) -> bool {
        if self.view_kind(name) == Some(ViewKind::FixedBacking) {
            // A Fixed handle still owns cleanup work even after its last source
            // read. Its exclusive backing borrow ends only on consuming close
            // (or lexical scope exit), not ordinary last-use shortening.
            return !self.moved.contains_key(name);
        }
        self.views_used_in_stmt.contains(name) || self.is_name_live_after(name)
    }

    pub(crate) fn view_kind(&self, name: &str) -> Option<ViewKind> {
        self.view_fact(name).map(|fact| fact.kind)
    }

    fn view_kind_for_place(&self, place: &ViewPlace) -> ViewKind {
        if let Some((_, fact)) = self
            .view_facts
            .bindings
            .iter()
            .rev()
            .find(|(_, fact)| {
                fact.place.owner.def_span == place.owner.def_span
                    && fact.place.projections == place.projections
            })
        {
            return fact.kind;
        }
        match self.lookup(&place.owner.name).map(|info| &info.ty) {
            Some(Type::Named(name)) if name == Syntax::TYPE_BYTE_BUFFER => ViewKind::Buffer,
            Some(Type::Apply { name, .. }) if matches!(name.as_str(), "Matrix" | "Tensor") => {
                ViewKind::Matrix
            }
            _ => ViewKind::List,
        }
    }

    fn place_name(place: &ViewPlace) -> String {
        let mut out = place.owner.name.clone();
        for projection in &place.projections {
            match projection {
                ViewProjection::Field(field) => {
                    out.push('.');
                    out.push_str(field);
                }
                ViewProjection::Index { span, .. } => {
                    let _ = span.start;
                    out.push_str("[…]");
                }
                ViewProjection::Range { span, .. } => {
                    let _ = span.start;
                    out.push_str("[…]");
                }
                ViewProjection::Fresh(span) => {
                    let _ = span.start;
                    out.push_str("[fresh]");
                }
            }
        }
        out
    }

    pub(crate) fn call_access_frame(&self) -> CallAccessFrame {
        CallAccessFrame::default()
    }

    /// Make one call's accumulated loans visible while its next expression is
    /// checked. The frame lives in the caller between arguments and is always
    /// restored by this closure boundary, including ordinary early returns.
    pub(crate) fn with_call_access<T>(
        &mut self,
        frame: &mut CallAccessFrame,
        check: impl FnOnce(&mut Self) -> T,
    ) -> T {
        self.call_access_frames.push(std::mem::take(frame));
        let result = check(self);
        *frame = self.call_access_frames.pop().unwrap_or_default();
        result
    }

    /// Lambda bodies are deferred execution. Keep calls inside the body scoped
    /// to that body; the enclosing call sees the post-inference capture summary.
    pub(crate) fn with_deferred_call_access<T>(
        &mut self,
        check: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let active = std::mem::take(&mut self.call_access_frames);
        let result = check(self);
        self.call_access_frames = active;
        result
    }

    fn check_call_place_access(
        &mut self,
        place: &ViewPlace,
        access: ViewAccess,
        span: Span,
    ) {
        self.check_call_place_access_kind(place, access, false, span);
    }

    fn check_call_place_access_kind(
        &mut self,
        place: &ViewPlace,
        access: ViewAccess,
        reserved: bool,
        span: Span,
    ) {
        let conflict = self
            .call_access_frames
            .iter()
            .flat_map(|frame| frame.accesses.iter())
            .rev()
            .find(|active| {
                active.place.overlaps(place)
                    && match (active.access, active.reserved, access, reserved) {
                        (ViewAccess::Write, true, ViewAccess::Read, false) => false,
                        (ViewAccess::Write, ..) | (_, _, ViewAccess::Write, _) => true,
                        _ => false,
                    }
            })
            .map(|active| active.access);
        let Some(active) = conflict else {
            return;
        };
        let name = Self::place_name(place);
        self.diags.push(if active == ViewAccess::Write {
            crate::Sema::Diagnostics::aliasing_while_mut(&name, span)
        } else {
            crate::Sema::Diagnostics::aliasing_mut_after_read(&name, span)
        });
    }

    fn record_call_place_access(&mut self, place: ViewPlace, access: ViewAccess) {
        self.call_access_frames
            .last_mut()
            .map(|frame| frame.accesses.push(CallPlaceAccess {
                place,
                access,
                reserved: false,
            }));
    }

    fn record_call_place_reservation(&mut self, place: ViewPlace) {
        self.call_access_frames
            .last_mut()
            .map(|frame| frame.accesses.push(CallPlaceAccess {
                place,
                access: ViewAccess::Write,
                reserved: true,
            }));
    }

    fn push_evaluated_access(
        &self,
        expr: &Expr,
        access: ViewAccess,
        bound: &HashSet<String>,
        out: &mut Vec<EvaluatedAccess>,
    ) {
        let Some(place) = self.place_from_expr(expr) else {
            return;
        };
        let (capture_place, capture_ty) = self
            .capture_place_info(expr)
            .map(|(place, ty, _)| (place, ty))
            .unwrap_or_else(|| (place.clone(), self.place_expr_type(expr)));
        if bound.contains(&capture_place.owner.name) {
            return;
        }
        if let Some(existing) = out.iter_mut().find(|existing| {
            existing.place.owner == place.owner
                && existing.place.projections == place.projections
                && existing.capture_place.owner == capture_place.owner
                && existing.capture_place.projections == capture_place.projections
        }) {
            if access == ViewAccess::Write {
                existing.access = ViewAccess::Write;
                existing.span = expr.span();
            }
            return;
        }
        out.push(EvaluatedAccess {
            place,
            capture_place,
            capture_ty,
            capture_is_view: expr_root_ident(expr).is_some_and(|name| self.is_view(name)),
            access,
            span: expr.span(),
            through_call: false,
            moves_owner: false,
        });
    }

    fn place_expr_type(&self, expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Ident(name, _) => self.lookup(name).map(|info| info.ty.clone()),
            Expr::Paren(inner, _) | Expr::Place(inner, _, _) => self.place_expr_type(inner),
            Expr::Field(base, field, _) => self
                .place_expr_type(base)
                .and_then(|owner| self.projected_field_type(owner, field)),
            Expr::Index { base, .. } => match self.place_expr_type(base)? {
                Type::List(elem) | Type::FixedList { elem, .. } => Some(*elem),
                Type::Map { value, .. } => Some(*value),
                _ => None,
            },
            _ => None,
        }
    }

    fn projected_field_type(&self, mut owner: Type, field: &str) -> Option<Type> {
        while let Type::Tagged { inner, .. } = owner {
            owner = *inner;
        }
        let (name, subst) = match owner {
            Type::Named(name) => (name, HashMap::new()),
            Type::Apply { name, args } => {
                let params = self.trait_reg.struct_params.get(&name)?;
                let subst = params
                    .iter()
                    .zip(args)
                    .map(|(param, arg)| (param.name.clone(), arg))
                    .collect();
                (name, subst)
            }
            _ => return None,
        };
        self.registry
            .struct_fields(&name)?
            .iter()
            .find(|(candidate, _, _, _)| candidate == field)
            .map(|(_, _, ty, _)| substitute_type(ty, &subst))
    }

    fn method_receiver_access(&self, receiver: &Expr, method: &str) -> ViewAccess {
        let Some(ty) = self.place_expr_type(receiver) else {
            return ViewAccess::Read;
        };
        if Collections::builtin_method_mutates(&ty, method) {
            return ViewAccess::Write;
        }
        if let Type::TraitObject(names) = &ty {
            let receiver = names.iter().find_map(|name| {
                self.trait_reg
                    .traits
                    .get(name)
                    .and_then(|trait_info| trait_info.methods.get(method))
                    .and_then(|sig| sig.params.first())
            });
            return if receiver.is_some_and(|receiver| {
                receiver.name == Syntax::KW_SELF
                    && receiver.convention == AccessConvention::Write
            }) {
                ViewAccess::Write
            } else {
                ViewAccess::Read
            };
        }
        let type_name = match &ty {
            Type::Named(name) | Type::Apply { name, .. } => name,
            _ => return ViewAccess::Read,
        };
        if self
            .resolve_method_sig(type_name, method)
            .is_some_and(|(_, sig)| sig.self_conv == Some(AccessConvention::Write))
        {
            ViewAccess::Write
        } else {
            ViewAccess::Read
        }
    }

    fn collect_lvalue_access(
        &self,
        target: &LValue,
        bound: &HashSet<String>,
        out: &mut Vec<EvaluatedAccess>,
    ) {
        match target {
            LValue::Local { name, name_span } => {
                let target = Expr::Ident(name.clone(), *name_span);
                self.push_evaluated_access(&target, ViewAccess::Write, bound, out);
            }
            LValue::Field { base, field, span } => {
                let target = Expr::Field(base.clone(), field.clone(), *span);
                self.push_evaluated_access(&target, ViewAccess::Write, bound, out);
            }
            LValue::Index {
                base, index, span, ..
            } => {
                let target = Expr::Index {
                    base: base.clone(),
                    index: index.clone(),
                    span: *span,
                    kind: Default::default(),
                };
                self.push_evaluated_access(&target, ViewAccess::Write, bound, out);
                self.collect_evaluated_expr_accesses(
                    index,
                    AccessWalkMode::EvaluateNow,
                    bound,
                    out,
                );
            }
        }
    }

    fn collect_lambda_requirement_events(
        &self,
        lambda: &Lambda,
        out: &mut Vec<EvaluatedAccess>,
    ) {
        let params = lambda
            .params
            .iter()
            .map(|param| param.name.clone())
            .collect::<HashSet<_>>();
        let mut events = Vec::new();
        match &lambda.body {
            LambdaBody::Expr(body) => self.collect_evaluated_expr_accesses(
                body,
                AccessWalkMode::CaptureRequirements,
                &params,
                &mut events,
            ),
            LambdaBody::Block(body) => {
                let mut bound = params;
                self.collect_evaluated_stmt_accesses(
                    body,
                    AccessWalkMode::CaptureRequirements,
                    &mut bound,
                    &mut events,
                );
            }
        }
        // `take` captures move at construction even when the body never names
        // them. All other capture roots come from the binding-aware semantic
        // walker above; the legacy name-only collector is not authoritative.
        for (name, span) in &lambda.take_names {
            let capture = Expr::Ident(name.clone(), *span);
            self.push_evaluated_access(
                &capture,
                ViewAccess::Read,
                &HashSet::new(),
                &mut events,
            );
        }
        let taken = lambda
            .take_names
            .iter()
            .map(|(name, _)| name)
            .collect::<HashSet<_>>();
        let cloned = lambda
            .meta
            .cloned_captures
            .iter()
            .collect::<HashSet<_>>();
        let has_capture_write = events
            .iter()
            .any(|event| event.access == ViewAccess::Write);
        let retains = !lambda.meta.escapes && has_capture_write;
        for event in &mut events {
            let name = &event.capture_place.owner.name;
            if cloned.contains(name) {
                // Lowering clones the whole source root before constructing
                // the move closure; body projections and writes target the clone.
                event.place.projections.clear();
                event.access = ViewAccess::Read;
            } else if (!lambda.meta.needs_fn_mut || lambda.meta.escapes)
                && !event.capture_is_view
                && !event.capture_ty.as_ref().is_some_and(type_is_copy)
                && self
                    .lookup(name)
                    .is_some_and(|info| info.param_conv.is_none())
            {
                // Rust 2021 move closures capture the syntactic place precisely.
                // Keep source-resolved places for loans, but move the field the
                // generated closure captures rather than its whole owner.
                event.place = event.capture_place.clone();
                event.access = ViewAccess::Write;
                event.moves_owner = true;
            }
            event.through_call = (event.capture_is_view
                || (retains && !taken.contains(name)))
                && !event.moves_owner;
        }
        for event in events {
            if let Some(existing) = out.iter_mut().find(|existing| {
                existing.place.owner == event.place.owner
                    && existing.place.projections == event.place.projections
            }) {
                if event.access == ViewAccess::Write {
                    existing.access = ViewAccess::Write;
                    existing.span = event.span;
                }
                existing.through_call |= event.through_call;
                existing.moves_owner |= event.moves_owner;
            } else {
                out.push(event);
            }
        }
    }

    fn collect_evaluated_expr_accesses(
        &self,
        expr: &Expr,
        mode: AccessWalkMode,
        bound: &HashSet<String>,
        out: &mut Vec<EvaluatedAccess>,
    ) {
        if let Expr::IncDec { operand, .. } = expr {
            self.push_evaluated_access(operand, ViewAccess::Write, bound, out);
            return;
        }
        if let Some(place) = self.place_from_expr(expr) {
            if mode != AccessWalkMode::ConstructCaptures
                && !bound.contains(&place.owner.name)
            {
                let access = if matches!(
                    expr,
                    Expr::Place(_, crate::AST::PlaceAccess::Write, _)
                ) {
                    ViewAccess::Write
                } else {
                    ViewAccess::Read
                };
                self.push_evaluated_access(expr, access, bound, out);
            }
            match expr {
                Expr::Index { index, .. } => {
                    self.collect_evaluated_expr_accesses(index, mode, bound, out);
                }
                Expr::Slice { start, end, .. } => {
                    self.collect_evaluated_expr_accesses(start, mode, bound, out);
                    self.collect_evaluated_expr_accesses(end, mode, bound, out);
                }
                _ => {}
            }
            return;
        }
        match expr {
            Expr::Lambda(lambda) => {
                if mode != AccessWalkMode::EvaluateNow {
                    self.collect_lambda_requirement_events(lambda, out);
                }
            }
            Expr::Call(call) => {
                if mode == AccessWalkMode::CaptureRequirements {
                    for arg in &call.args {
                        self.collect_evaluated_expr_accesses(
                            &arg.expr,
                            mode,
                            bound,
                            out,
                        );
                    }
                }
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                if mode == AccessWalkMode::CaptureRequirements {
                    let access = self.method_receiver_access(receiver, method);
                    self.push_evaluated_access(receiver, access, bound, out);
                    for arg in args {
                        self.collect_evaluated_expr_accesses(
                            &arg.expr,
                            mode,
                            bound,
                            out,
                        );
                    }
                }
            }
            Expr::CallValue { callee, args, .. } => {
                if mode == AccessWalkMode::CaptureRequirements {
                    self.collect_evaluated_expr_accesses(callee, mode, bound, out);
                    for arg in args {
                        self.collect_evaluated_expr_accesses(
                            &arg.expr,
                            mode,
                            bound,
                            out,
                        );
                    }
                }
            }
            Expr::Binary(_, left, right, _) => {
                self.collect_evaluated_expr_accesses(left, mode, bound, out);
                self.collect_evaluated_expr_accesses(right, mode, bound, out);
            }
            Expr::CompareChain { operands, .. } | Expr::ListLit(operands, _) => {
                for operand in operands {
                    self.collect_evaluated_expr_accesses(operand, mode, bound, out);
                }
            }
            Expr::Unary(_, inner, _)
            | Expr::Spread(inner, _)
            | Expr::Deref(inner, _)
            | Expr::RawOf(inner, _)
            | Expr::Copy(inner, _)
            | Expr::Place(inner, _, _)
            | Expr::Tainted(inner, _, _)
            | Expr::Present(inner, _)
            | Expr::Ok(inner, _)
            | Expr::Err(inner, _)
            | Expr::Try(inner, _, _)
            | Expr::Paren(inner, _) => {
                self.collect_evaluated_expr_accesses(inner, mode, bound, out);
            }
            Expr::Field(base, _, _) | Expr::OptField { base, .. } => {
                self.collect_evaluated_expr_accesses(base, mode, bound, out);
            }
            Expr::Index { base, index, .. } => {
                self.collect_evaluated_expr_accesses(base, mode, bound, out);
                self.collect_evaluated_expr_accesses(index, mode, bound, out);
            }
            Expr::Slice {
                base, start, end, ..
            } => {
                self.collect_evaluated_expr_accesses(base, mode, bound, out);
                self.collect_evaluated_expr_accesses(start, mode, bound, out);
                self.collect_evaluated_expr_accesses(end, mode, bound, out);
            }
            Expr::MapLit(entries, _) => {
                for (key, value) in entries {
                    self.collect_evaluated_expr_accesses(key, mode, bound, out);
                    self.collect_evaluated_expr_accesses(value, mode, bound, out);
                }
            }
            Expr::StructLit { fields, .. } => {
                for (_, _, value) in fields {
                    self.collect_evaluated_expr_accesses(value, mode, bound, out);
                }
            }
            Expr::TypedLit { body, .. } => {
                body.for_each_expr(|value| {
                    self.collect_evaluated_expr_accesses(value, mode, bound, out)
                });
            }
            Expr::TupleLit(fields, _, _) => {
                for (_, value) in fields {
                    self.collect_evaluated_expr_accesses(value, mode, bound, out);
                }
            }
            Expr::EnumLit { args, .. } => {
                for arg in args {
                    let value = match arg {
                        crate::AST::EnumLitArg::Positional(value)
                        | crate::AST::EnumLitArg::Named { expr: value, .. } => value,
                    };
                    self.collect_evaluated_expr_accesses(value, mode, bound, out);
                }
            }
            Expr::Str(parts, _) => {
                for part in parts {
                    if let crate::AST::StrPart::Interp(value, _) = part {
                        self.collect_evaluated_expr_accesses(value, mode, bound, out);
                    }
                }
            }
            Expr::PatternTest {
                subject, pattern, ..
            } => {
                self.collect_evaluated_expr_accesses(subject, mode, bound, out);
                self.collect_pattern_value_accesses(pattern, mode, bound, out);
            }
            Expr::OrFallback {
                value, fallback, ..
            } => {
                self.collect_evaluated_expr_accesses(value, mode, bound, out);
                match fallback {
                    crate::AST::OrFallback::Value(value)
                    | crate::AST::OrFallback::Return(Some(value), _) => {
                        self.collect_evaluated_expr_accesses(value, mode, bound, out);
                    }
                    crate::AST::OrFallback::Panic { args, .. } => {
                        for arg in args {
                            self.collect_evaluated_expr_accesses(
                                &arg.expr,
                                mode,
                                bound,
                                out,
                            );
                        }
                    }
                    _ => {}
                }
            }
            Expr::If {
                cond,
                then_body,
                then_value,
                else_body,
                else_value,
                ..
            } => {
                self.collect_evaluated_expr_accesses(cond, mode, bound, out);
                let mut then_bound = bound.clone();
                if let Expr::PatternTest { pattern, .. } = cond.as_ref() {
                    Self::bind_pattern_names(pattern, &mut then_bound);
                }
                self.collect_evaluated_stmt_accesses(
                    then_body,
                    mode,
                    &mut then_bound,
                    out,
                );
                self.collect_evaluated_expr_accesses(
                    then_value,
                    mode,
                    &then_bound,
                    out,
                );
                let mut else_bound = bound.clone();
                self.collect_evaluated_stmt_accesses(
                    else_body,
                    mode,
                    &mut else_bound,
                    out,
                );
                self.collect_evaluated_expr_accesses(
                    else_value,
                    mode,
                    &else_bound,
                    out,
                );
            }
            Expr::PtrFromAddr { addr, .. } => {
                self.collect_evaluated_expr_accesses(addr, mode, bound, out);
            }
            Expr::FanOut { callee, items, .. } => {
                self.collect_evaluated_expr_accesses(callee, mode, bound, out);
                for item in items {
                    self.collect_evaluated_expr_accesses(item, mode, bound, out);
                }
            }
            Expr::StrMatchLit(..)
            | Expr::BinMatchLit(..)
            | Expr::Int(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::Char(..)
            | Expr::Ident(..)
            | Expr::UnitLit { .. }
            | Expr::Absent(..)
            | Expr::Todo { .. }
            | Expr::ReduceMarker(..)
            | Expr::ComptimeSplice { .. }
            | Expr::IncDec { .. } => {}
        }
    }

    fn bind_pattern_names(pattern: &Pattern, bound: &mut HashSet<String>) {
        match pattern {
            Pattern::Ok { binding, .. }
            | Pattern::Err { binding, .. }
            | Pattern::Present { binding, .. } => {
                bound.insert(binding.clone());
            }
            Pattern::Variant { bindings, .. } => {
                for slot in bindings {
                    if let crate::AST::PatSlot::Bind { name, .. } = slot {
                        bound.insert(name.clone());
                    }
                }
            }
            Pattern::Struct { fields, .. } => {
                for field in fields {
                    if let crate::AST::StructPatField::Bind { local, .. } = field {
                        bound.insert(local.clone());
                    }
                }
            }
            Pattern::Or(alts, _) => {
                if let Some(first) = alts.first() {
                    Self::bind_pattern_names(first, bound);
                }
            }
            Pattern::StrMatch { parts, .. } => {
                for part in parts {
                    if let crate::AST::StrMatchPart::Hole { name, .. } = part {
                        bound.insert(name.clone());
                    }
                }
            }
            Pattern::BinMatch { parts, .. } => {
                for part in parts {
                    if let crate::AST::BinMatchPart::Hole { name, .. } = part {
                        bound.insert(name.clone());
                    }
                }
            }
            Pattern::Absent(_) | Pattern::Range { .. } => {}
        }
    }

    fn collect_pattern_value_accesses(
        &self,
        pattern: &Pattern,
        mode: AccessWalkMode,
        bound: &HashSet<String>,
        out: &mut Vec<EvaluatedAccess>,
    ) {
        match pattern {
            Pattern::Struct { fields, .. } => {
                for field in fields {
                    if let crate::AST::StructPatField::Value { value, .. } = field {
                        self.collect_evaluated_expr_accesses(value, mode, bound, out);
                    }
                }
            }
            Pattern::Or(alternatives, _) => {
                for alternative in alternatives {
                    self.collect_pattern_value_accesses(
                        alternative,
                        mode,
                        bound,
                        out,
                    );
                }
            }
            _ => {}
        }
    }

    fn collect_if_stmt_accesses(
        &self,
        branch: &crate::AST::IfStmt,
        mode: AccessWalkMode,
        bound: &HashSet<String>,
        out: &mut Vec<EvaluatedAccess>,
    ) {
        self.collect_evaluated_expr_accesses(&branch.cond, mode, bound, out);
        let mut then_bound = bound.clone();
        if let Expr::PatternTest { pattern, .. } = &branch.cond {
            Self::bind_pattern_names(pattern, &mut then_bound);
        }
        self.collect_evaluated_stmt_accesses(
            &branch.then_body,
            mode,
            &mut then_bound,
            out,
        );
        match &branch.else_branch {
            Some(ElseBranch::ElseIf(branch)) => {
                self.collect_if_stmt_accesses(branch, mode, bound, out);
            }
            Some(ElseBranch::Else(body)) => {
                let mut else_bound = bound.clone();
                self.collect_evaluated_stmt_accesses(body, mode, &mut else_bound, out);
            }
            None => {}
        }
    }

    fn collect_evaluated_stmt_accesses(
        &self,
        body: &[Stmt],
        mode: AccessWalkMode,
        bound: &mut HashSet<String>,
        out: &mut Vec<EvaluatedAccess>,
    ) {
        for stmt in body {
            match stmt {
                Stmt::Expr(expr) | Stmt::Yield(expr, _) => {
                    self.collect_evaluated_expr_accesses(expr, mode, bound, out);
                }
                Stmt::Val(binding) => {
                    self.collect_evaluated_expr_accesses(&binding.init, mode, bound, out);
                    if let Some(pattern) = &binding.pattern {
                        for name in pattern.names() {
                            bound.insert(name.local_name().to_string());
                        }
                    } else {
                        bound.insert(binding.name.clone());
                    }
                }
                Stmt::Assign { target, value, .. } => {
                    self.collect_lvalue_access(target, bound, out);
                    self.collect_evaluated_expr_accesses(value, mode, bound, out);
                }
                Stmt::Return(Some(expr), _) => {
                    self.collect_evaluated_expr_accesses(expr, mode, bound, out);
                }
                Stmt::If(branch) => {
                    self.collect_if_stmt_accesses(branch, mode, bound, out);
                }
                Stmt::While { cond, body, .. } => {
                    self.collect_evaluated_expr_accesses(cond, mode, bound, out);
                    let mut body_bound = bound.clone();
                    self.collect_evaluated_stmt_accesses(
                        body,
                        mode,
                        &mut body_bound,
                        out,
                    );
                }
                Stmt::For {
                    var,
                    var2,
                    kind,
                    body,
                    ..
                } => {
                    match kind {
                        ForKind::Range {
                            start, end, step, ..
                        } => {
                            self.collect_evaluated_expr_accesses(start, mode, bound, out);
                            self.collect_evaluated_expr_accesses(end, mode, bound, out);
                            if let Some(step) = step {
                                self.collect_evaluated_expr_accesses(
                                    step, mode, bound, out,
                                );
                            }
                        }
                        ForKind::In { collection, step } => {
                            self.collect_evaluated_expr_accesses(
                                collection, mode, bound, out,
                            );
                            if let Some(step) = step {
                                self.collect_evaluated_expr_accesses(
                                    step, mode, bound, out,
                                );
                            }
                        }
                    }
                    let mut body_bound = bound.clone();
                    body_bound.insert(var.clone());
                    if let Some((name, _)) = var2 {
                        body_bound.insert(name.clone());
                    }
                    self.collect_evaluated_stmt_accesses(
                        body,
                        mode,
                        &mut body_bound,
                        out,
                    );
                }
                Stmt::Switch {
                    subject,
                    arms,
                    else_body,
                    ..
                } => {
                    self.collect_evaluated_expr_accesses(subject, mode, bound, out);
                    let mut switch_bound = bound.clone();
                    switch_bound.insert(Syntax::KW_IT.to_string());
                    for arm in arms {
                        self.collect_evaluated_expr_accesses(
                            &arm.cond,
                            mode,
                            &switch_bound,
                            out,
                        );
                        let mut arm_bound = switch_bound.clone();
                        if let Expr::PatternTest { pattern, .. } = &arm.cond {
                            Self::bind_pattern_names(pattern, &mut arm_bound);
                        }
                        self.collect_evaluated_stmt_accesses(
                            &arm.body,
                            mode,
                            &mut arm_bound,
                            out,
                        );
                    }
                    if let Some(else_body) = else_body {
                        let mut else_bound = bound.clone();
                        self.collect_evaluated_stmt_accesses(
                            else_body,
                            mode,
                            &mut else_bound,
                            out,
                        );
                    }
                }
                Stmt::CountedLoop {
                    init,
                    cond,
                    step,
                    body,
                    ..
                } => {
                    self.collect_evaluated_expr_accesses(&init.init, mode, bound, out);
                    let mut loop_bound = bound.clone();
                    loop_bound.insert(init.name.clone());
                    self.collect_evaluated_expr_accesses(cond, mode, &loop_bound, out);
                    self.collect_evaluated_stmt_accesses(
                        body,
                        mode,
                        &mut loop_bound,
                        out,
                    );
                    if let Some(step) = step {
                        self.collect_evaluated_stmt_accesses(
                            std::slice::from_ref(step.as_ref()),
                            mode,
                            &mut loop_bound,
                            out,
                        );
                    }
                }
                Stmt::Loop { body, .. }
                | Stmt::Unsafe { body, .. }
                | Stmt::Impure { body, .. }
                | Stmt::Shield { body, .. }
                | Stmt::DebugOnly { body, .. }
                | Stmt::Region { body, .. }
                | Stmt::Policy { body, .. }
                | Stmt::TaskGroup { body, .. }
                | Stmt::Layout { body, .. }
                | Stmt::Caps { body, .. }
                | Stmt::Grant { body, .. }
                | Stmt::Transact { body, .. }
                | Stmt::AssumeDet { body, .. }
                | Stmt::Live { body, .. } => {
                    let mut body_bound = bound.clone();
                    self.collect_evaluated_stmt_accesses(
                        body,
                        mode,
                        &mut body_bound,
                        out,
                    );
                }
                Stmt::Reactive { body, .. } => {
                    let mut body_bound = bound.clone();
                    let mut captures = Vec::new();
                    self.collect_evaluated_stmt_accesses(
                        body,
                        AccessWalkMode::CaptureRequirements,
                        &mut body_bound,
                        &mut captures,
                    );
                    // Reactive registration clones every free capture. Body
                    // writes run later against the clones; construction only
                    // reads each source value now.
                    for mut capture in captures {
                        capture.access = ViewAccess::Read;
                        capture.through_call = false;
                        capture.moves_owner = false;
                        capture.place.projections.clear();
                        capture.capture_place = capture.place.clone();
                        capture.capture_ty = self
                            .lookup(&capture.place.owner.name)
                            .map(|info| info.ty.clone());
                        capture.capture_is_view = false;
                        for existing in out
                            .iter()
                            .filter(|event| event.place.owner == capture.place.owner)
                        {
                            if existing.access == ViewAccess::Write {
                                capture.access = ViewAccess::Write;
                                capture.span = existing.span;
                            }
                            capture.through_call |= existing.through_call;
                            capture.moves_owner |= existing.moves_owner;
                        }
                        out.retain(|event| event.place.owner != capture.place.owner);
                        out.push(capture);
                    }
                }
                Stmt::ContextBlock { fields, body, .. } => {
                    for (_, expr, _) in fields {
                        self.collect_evaluated_expr_accesses(expr, mode, bound, out);
                    }
                    let mut body_bound = bound.clone();
                    self.collect_evaluated_stmt_accesses(
                        body,
                        mode,
                        &mut body_bound,
                        out,
                    );
                }
                Stmt::ScopeMember { args, body, .. } => {
                    for arg in args {
                        self.collect_evaluated_expr_accesses(arg, mode, bound, out);
                    }
                    let mut body_bound = bound.clone();
                    self.collect_evaluated_stmt_accesses(
                        body,
                        mode,
                        &mut body_bound,
                        out,
                    );
                }
                // These forms erase before runtime evaluation. `Off` never
                // runs; comptime bodies run in the separate interpreter.
                Stmt::Off { .. }
                | Stmt::ComptimeIf { .. }
                | Stmt::ComptimeSwitch { .. }
                | Stmt::ComptimeBlock { .. }
                | Stmt::Break(_)
                | Stmt::Continue(_)
                | Stmt::BreakLabel(..)
                | Stmt::ContinueLabel(..)
                | Stmt::Return(None, _) => {}
            }
        }
    }

    fn check_transient_accesses(&mut self, accesses: Vec<EvaluatedAccess>) {
        for access in accesses {
            self.check_call_place_access(&access.place, access.access, access.span);
        }
    }


    /// Rust activates a two-phase receiver borrow only after its arguments
    /// finish evaluating. Only loans retained through the call can conflict at
    /// that point; transient reads have already ended.
    pub(crate) fn activate_call_reservations(
        &mut self,
        frame: &CallAccessFrame,
        span: Span,
    ) {
        let conflicts: Vec<_> = frame
            .accesses
            .iter()
            .filter(|access| access.reserved)
            .filter_map(|reservation| {
                frame
                    .accesses
                    .iter()
                    .filter(|access| !access.reserved)
                    .find(|access| access.place.overlaps(&reservation.place))
                    .map(|access| (reservation.place.clone(), access.access))
            })
            .collect();
        for (place, access) in conflicts {
            let name = Self::place_name(&place);
            self.diags.push(if access == ViewAccess::Read {
                crate::Sema::Diagnostics::aliasing_mut_after_read(&name, span)
            } else {
                crate::Sema::Diagnostics::aliasing_while_mut(&name, span)
            });
        }
    }

    /// A nested call's frame ends when it returns, but a returned View keeps
    /// the source loan alive as an argument of the enclosing call.
    pub(crate) fn record_call_result_views(&mut self, expr: &Expr) {
        let mut loans: Vec<(ViewPlace, ViewAccess)> = Vec::new();
        for (_, place, _, access) in self.view_call_sources(expr) {
            if let Some((_, seen_access)) = loans.iter_mut().find(|(seen, _)| {
                seen.owner == place.owner
                    && seen.projections == place.projections
            }) {
                if access == ViewAccess::Write {
                    *seen_access = ViewAccess::Write;
                }
            } else {
                loans.push((place, access));
            }
        }
        for (place, access) in loans {
            self.check_call_place_access(&place, access, expr.span());
            self.record_call_place_access(place, access);
        }
    }

    fn collect_call_projection_accesses(
        &self,
        expr: &Expr,
        accesses: &mut Vec<EvaluatedAccess>,
    ) {
        match expr {
            Expr::Index { base, index, .. } => {
                self.collect_call_projection_accesses(base, accesses);
                self.collect_evaluated_expr_accesses(
                    index,
                    AccessWalkMode::EvaluateNow,
                    &HashSet::new(),
                    accesses,
                );
            }
            Expr::Slice {
                base, start, end, ..
            } => {
                self.collect_call_projection_accesses(base, accesses);
                self.collect_evaluated_expr_accesses(
                    start,
                    AccessWalkMode::EvaluateNow,
                    &HashSet::new(),
                    accesses,
                );
                self.collect_evaluated_expr_accesses(
                    end,
                    AccessWalkMode::EvaluateNow,
                    &HashSet::new(),
                    accesses,
                );
            }
            Expr::Field(base, _, _)
            | Expr::Place(base, _, _)
            | Expr::Paren(base, _)
            | Expr::Deref(base, _) => self.collect_call_projection_accesses(base, accesses),
            _ => {}
        }
    }

    /// Check one evaluated argument against every active outer/current call,
    /// then retain exactly the borrow that generated Rust keeps until this call
    /// returns. Call-form checkers supply only signature metadata.
    pub(crate) fn check_call_argument_access(
        &mut self,
        arg: &crate::AST::CallArg,
        param_conv: AccessConvention,
        param_ty: &Type,
        borrowed_read: bool,
    ) {
        let Some(place) = self.place_from_expr(&arg.expr) else {
            let mut accesses = Vec::new();
            self.collect_evaluated_expr_accesses(
                &arg.expr,
                AccessWalkMode::EvaluateNow,
                &HashSet::new(),
                &mut accesses,
            );
            self.check_transient_accesses(accesses);
            return;
        };
        let mut projection_accesses = Vec::new();
        self.collect_call_projection_accesses(&arg.expr, &mut projection_accesses);
        self.check_transient_accesses(projection_accesses);
        let access = if arg.convention == AccessConvention::Write {
            ViewAccess::Write
        } else {
            ViewAccess::Read
        };
        self.check_call_place_access(&place, access, arg.expr.span());
        let generated = if arg.convention == AccessConvention::Write {
            Some(ViewAccess::Write)
        } else if arg.convention == AccessConvention::Read
            && param_conv == AccessConvention::Read
            && borrowed_read
            && !param_ty.is_scalar()
        {
            Some(ViewAccess::Read)
        } else {
            None
        };
        if let Some(access) = generated {
            self.record_call_place_access(place, access);
        }
    }

    /// Lambda bodies run later, but constructing a lambda evaluates its free
    /// captures now. Only nonescaping FnMut closures retain capture borrows.
    pub(crate) fn check_call_argument_captures(&mut self, expr: &Expr) {
        let mut accesses = Vec::new();
        self.collect_evaluated_expr_accesses(
            expr,
            AccessWalkMode::ConstructCaptures,
            &HashSet::new(),
            &mut accesses,
        );
        for access in accesses {
            self.check_call_place_access(&access.place, access.access, access.span);
            if access.through_call {
                self.record_call_place_access(access.place, access.access);
            } else if access.moves_owner {
                self.mark_moved_place(access.place, access.span);
            }
        }
        self.record_call_result_views(expr);
    }

    /// Receiver evaluation is a read even before method resolution. This catches
    /// a nested receiver that overlaps an outer call's active write loan.
    pub(crate) fn check_call_receiver_evaluation(&mut self, receiver: &Expr, span: Span) {
        if let Some(place) = self.place_from_expr(receiver) {
            let mut projection_accesses = Vec::new();
            self.collect_call_projection_accesses(receiver, &mut projection_accesses);
            self.check_transient_accesses(projection_accesses);
            self.check_call_place_access(&place, ViewAccess::Read, span);
        } else {
            let mut accesses = Vec::new();
            self.collect_evaluated_expr_accesses(
                receiver,
                AccessWalkMode::EvaluateNow,
                &HashSet::new(),
                &mut accesses,
            );
            self.check_transient_accesses(accesses);
        }
    }

    pub(crate) fn record_call_receiver_access(
        &mut self,
        receiver: &Expr,
        convention: AccessConvention,
        span: Span,
    ) {
        let Some(place) = self.place_from_expr(receiver) else {
            return;
        };
        let access = if convention == AccessConvention::Write {
            ViewAccess::Write
        } else {
            ViewAccess::Read
        };
        self.check_call_place_access(&place, access, span);
        if convention != AccessConvention::Move {
            self.record_call_place_access(place, access);
        }
    }

    pub(crate) fn record_call_receiver_reservation(&mut self, receiver: &Expr, span: Span) {
        let Some(place) = self.place_from_expr(receiver) else {
            return;
        };
        self.check_call_place_access_kind(&place, ViewAccess::Write, true, span);
        self.record_call_place_reservation(place);
    }

    pub(crate) fn place_from_expr(&self, expr: &Expr) -> Option<ViewPlace> {
        let mut output_path = Vec::new();
        if let Some(name) = named_view_field_path(expr, &mut output_path) {
            if let Some(fact) = self.view_fact_at_path(&name, &output_path) {
                let mut place = fact.place.clone();
                place.projections.extend(
                    output_path[fact.output_path.len()..]
                        .iter()
                        .cloned()
                        .map(ViewProjection::Field),
                );
                return Some(place);
            }
        }
        match expr {
            Expr::Ident(name, _) => (self.lookup(name).is_some()
                || self.consts.contains_key(name))
            .then(|| ViewPlace {
                owner: self.owner_id(name),
                projections: Vec::new(),
            }),
            Expr::Field(base, field, _) => {
                let mut place = self.place_from_expr(base)?;
                place.projections.push(ViewProjection::Field(field.clone()));
                Some(place)
            }
            Expr::Index { base, index, span, .. } => {
                let mut place = self.place_from_expr(base)?;
                place.projections.push(ViewProjection::Index {
                    value: const_place_int(index),
                    span: *span,
                });
                Some(place)
            }
            Expr::Slice { base, start, end, span } => {
                let mut place = self.place_from_expr(base)?;
                place.projections.push(ViewProjection::Range {
                    start: const_place_int(start),
                    end: const_place_int(end),
                    span: *span,
                });
                Some(place)
            }
            Expr::Place(inner, _, _) | Expr::Paren(inner, _) => self.place_from_expr(inner),
            _ => None,
        }
    }

    /// Rust 2021 capture prefix and its type from the written closure
    /// expression. Indexing and slicing stop precision: fields before them are
    /// capturable, while anything after them belongs to the captured container.
    fn capture_place_info(&self, expr: &Expr) -> Option<(ViewPlace, Option<Type>, bool)> {
        match expr {
            Expr::Ident(name, _) => (self.lookup(name).is_some()
                || self.consts.contains_key(name))
            .then(|| {
                (
                    ViewPlace {
                        owner: self.owner_id(name),
                        projections: Vec::new(),
                    },
                    self.lookup(name).map(|info| info.ty.clone()),
                    false,
                )
            }),
            Expr::Field(base, field, _) => {
                let (mut place, ty, stopped) = self.capture_place_info(base)?;
                if stopped {
                    return Some((place, ty, true));
                }
                place.projections.push(ViewProjection::Field(field.clone()));
                let ty = ty.and_then(|owner| self.projected_field_type(owner, field));
                Some((place, ty, false))
            }
            Expr::Index { base, .. } | Expr::Slice { base, .. } => self
                .capture_place_info(base)
                .map(|(place, ty, _)| (place, ty, true)),
            Expr::Place(inner, _, _) | Expr::Paren(inner, _) => {
                self.capture_place_info(inner)
            }
            _ => None,
        }
    }

    /// Place Rust 2021 captures from the written closure expression. Unlike
    /// `place_from_expr`, this deliberately does not chase a View alias to its
    /// source; generated Rust captures the alias value itself.
    fn capture_place_from_expr(&self, expr: &Expr) -> Option<ViewPlace> {
        self.capture_place_info(expr).map(|(place, _, _)| place)
    }

    pub(crate) fn is_view(&self, name: &str) -> bool {
        self.view_fact(name).is_some()
    }

    pub(crate) fn is_write_view(&self, name: &str) -> bool {
        self.view_fact(name)
            .is_some_and(|fact| fact.access == ViewAccess::Write)
    }

    pub(crate) fn validate_write_place(&mut self, expr: &Expr, span: Span) {
        let Some(root) = expr_root_ident(expr).map(str::to_string) else {
            return;
        };
        if let Some(fact) = self.view_fact(&root) {
            if fact.access == ViewAccess::Write {
                return;
            }
            self.diags.push(Diagnostic::error(
                "E0205",
                format!("cannot edit through `{root}` — it has read access only"),
                "a read window may inspect its place, but it cannot change the owner"
                    .to_string(),
                "take a write window from a mutable owner instead".to_string(),
                Some(span),
            ));
            return;
        }
        if self.consts.contains_key(&root) {
            self.diags.push(Diagnostic::error(
                "E0111",
                format!("`{root}` is a const and can never change"),
                "a const is fixed for the whole program".to_string(),
                format!(
                    "use a `{}` binding if it needs to change",
                    Syntax::SIGIL_BIND_MUT
                ),
                Some(span),
            ));
            return;
        }
        let Some(info) = self.lookup(&root) else {
            return;
        };
        if info.mutable || info.param_conv == Some(AccessConvention::Write) {
            return;
        }
        let (code, why, fix) = if info.param_conv.is_some() {
            (
                "E0205",
                "an unmarked parameter gives read access only; a write window needs write access (`&`)".to_string(),
                format!(
                    "change the parameter to `{}: {}{}`",
                    root,
                    Syntax::SIGIL_WRITE,
                    info.ty.name()
                ),
            )
        } else {
            (
                "E0202",
                "a write window can change its owner, so the owner must be mutable".to_string(),
                format!("declare `{} {} ...`", root, Syntax::SIGIL_BIND_MUT),
            )
        };
        self.diags.push(Diagnostic::error(
            code,
            format!("cannot take a write window into `{root}`"),
            why,
            fix,
            Some(span),
        ));
    }

    pub(crate) fn check_expr_change(&mut self, expr: &Expr, action: &str, span: Span) {
        if let Some(name) = expr_root_ident(expr) {
            if self.reject_expiring_secret_loan_change(name, action, span) {
                return;
            }
        }
        if let Some(place) = self.place_from_expr(expr) {
            self.check_place_change(&place, action, span);
        }
    }

    pub(crate) fn check_mutating_method_receiver(
        &mut self,
        receiver: &Expr,
        method: &str,
        span: Span,
    ) {
        if self.in_lambda_body {
            if let Some(root) = expr_root_ident(receiver) {
                self.inferred_lambda_mut_captures
                    .insert(root.to_string());
            }
        }
        self.check_expr_change(receiver, &format!("be changed by `.{method}()`"), span);
        let Some(root) = expr_root_ident(receiver).map(str::to_string) else {
            return;
        };
        if self.iter_borrowed.contains(&root) {
            self.diags.push(
                crate::Sema::Diagnostics::collection_changed_in_loop(&root, span),
            );
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
                            root, method
                        ),
                        format!("declare `{} {} ...`", root, Syntax::SIGIL_BIND_MUT),
                    )
                };
                self.diags.push(Diagnostic::error(
                    "E0202",
                    what,
                    "this method edits the value it's called on; write access (`&`) is required"
                        .to_string(),
                    fix,
                    Some(span),
                ));
            }
        }
    }

    pub(crate) fn check_write_arg_change(&mut self, arg: &crate::AST::CallArg) {
        if arg.convention == AccessConvention::Write {
            self.check_expr_change(&arg.expr, "be passed with write access", arg.span);
        }
    }

    pub(crate) fn check_lvalue_change(&mut self, target: &LValue, action: &str) {
        let through_write_view = match target {
            LValue::Index { base, .. } | LValue::Field { base, .. } => {
                expr_root_ident(base)
                    .and_then(|name| self.view_fact(name))
                    .is_some_and(|fact| fact.access == ViewAccess::Write)
            }
            LValue::Local { name, .. } => self.is_write_view(name),
        };
        if through_write_view {
            return;
        }
        let place = match target {
            LValue::Local { name, .. } => Some(ViewPlace {
                owner: self.owner_id(name),
                projections: Vec::new(),
            }),
            LValue::Index { base, index, span, .. } => self.place_from_expr(base).map(|mut place| {
                place.projections.push(ViewProjection::Index {
                    value: const_place_int(index),
                    span: *span,
                });
                place
            }),
            LValue::Field { base, field, .. } => self.place_from_expr(base).map(|mut place| {
                place.projections.push(ViewProjection::Field(field.clone()));
                place
            }),
        };
        if let Some(place) = place {
            self.check_place_change(&place, action, target.span());
        }
    }

    /// Reject moves, replacement, and storage-changing methods while any view
    /// into the owner remains live. Reads stay legal; facts vanish on scope exit.
    pub(crate) fn check_owner_change(&mut self, owner: &str, action: &str, span: Span) {
        let owner_place = ViewPlace {
            owner: self.owner_id(owner),
            projections: Vec::new(),
        };
        self.check_place_change(&owner_place, action, span);
    }

    fn check_place_change(&mut self, changed: &ViewPlace, action: &str, span: Span) {
        let Some((view, access, place)) = self
            .view_facts
            .bindings
            .iter()
            .rev()
            .find(|(name, fact)| {
                self.view_is_live_now(name)
                    && fact.place.overlaps(changed)
                    && fact.invalidated.is_none()
            })
            .map(|(name, fact)| (name.clone(), fact.access, Self::place_name(&fact.place)))
        else {
            return;
        };
        let access = if access == ViewAccess::Write {
            "exclusive mutable view"
        } else {
            "read view"
        };
        let changed_name = Self::place_name(changed);
        self.diags.push(Diagnostic::error(
            "E0212",
            format!("`{changed_name}` cannot {action} while `{view}` is still looking into it"),
            format!(
                "`{view}` is a live {access} into `{place}`; changing or moving the owner could invalidate that view"
            ),
            format!(
                "finish using `{view}` before changing `{changed_name}`, narrow the view's scope, or make an owned copy"
            ),
            Some(span),
        ));
    }

    // ──────────────────────────────────────────────────────────────────────
    // D-DYNARRAY1 (ratified 2026-07-01): `View<T>` zero-copy windows
    // (`list.view(a..b)`).
    //
    // A view's Rust value is a genuine borrowed slice (`&[T]`, elided
    // lifetime — see `Context::rust_type`'s `View` arm) so it is exactly as
    // sound as an arena view. List, arena, string, buffer, and matrix windows
    // share one provenance graph; only product diagnostics and lowering flags
    // differ.
    //
    // E2305 fires when a view escapes: returned, rebound to another local, or
    // stored in a struct field. Crossing a task/channel boundary is instead
    // caught by the general sendability check (`SendProblemKind::ViewBorrow`
    // on `Type::Apply { name: "View", .. }`, reported as E1102) — no tracking
    // needed there, the value's TYPE alone is enough.
    // ──────────────────────────────────────────────────────────────────────

    /// If `init` is `list.view(a..b)` on a plain local name, return the list's
    /// name — but only when that list is a genuine local (dies at this
    /// function's return/scope exit). A parameter or const outlives the call,
    /// so a view into it is sound without tracking (mirrors `E2301`'s
    /// param/const exemption in `view_return_local_owner`).
    pub(crate) fn view_call_sources(
        &mut self,
        init: &Expr,
    ) -> Vec<(Vec<String>, ViewPlace, ViewKind, ViewAccess)> {
        if let Expr::Copy(inner, _) | Expr::Paren(inner, _) = init {
            return self.view_call_sources(inner);
        }
        if let Expr::If {
            then_value,
            else_value,
            ..
        } = init
        {
            let mut sources = self.view_call_sources(then_value);
            sources.extend(self.view_call_sources(else_value));
            return sources;
        }
        if let Expr::OrFallback {
            value, fallback, ..
        } = init
        {
            let mut sources = self.view_call_sources(value);
            if let crate::AST::OrFallback::Value(value) = fallback {
                sources.extend(self.view_call_sources(value));
            }
            return sources;
        }
        if let Expr::Field(base, field, _) = init {
            let projected: Vec<_> = self
                .view_call_sources(base)
                .into_iter()
                .filter_map(|(mut path, place, kind, access)| {
                    (path.first() == Some(field)).then(|| {
                        path.remove(0);
                        (path, place, kind, access)
                    })
                })
                .collect();
            if !projected.is_empty() {
                return projected;
            }

            let mut output_path = Vec::new();
            if let Some(name) = named_view_field_path(init, &mut output_path) {
                if let Some((kind, access)) = self
                    .view_fact_at_path(&name, &output_path)
                    .map(|fact| (fact.kind, fact.access))
                {
                    if let Some(place) = self.place_from_expr(init) {
                        return vec![(Vec::new(), place, kind, access)];
                    }
                }
            }
        }
        if let Expr::Call(call) = init {
            let Some(sig) = self.funcs.get(&call.name) else {
                return Vec::new();
            };
            let Some(map) = sig.return_view_provenance.get().cloned() else {
                return Vec::new();
            };
            let string_view = sig.return_type.as_ref().is_some_and(|ty| {
                matches!(
                    ty,
                    Type::Apply { name, args }
                        if name == "View"
                            && matches!(args.as_slice(), [Type::Named(inner)] if inner == "str")
                ) || matches!(ty, Type::Named(name) if self.registry.struct_fields(name).is_some_and(|fields| {
                    fields.iter().any(|(_, _, field_ty, _)| matches!(
                        field_ty,
                        Type::Apply { name, args }
                            if name == "View"
                                && matches!(args.as_slice(), [Type::Named(inner)] if inner == "str")
                    ))
                }))
            });
            let mut sources = Vec::new();
            for (output_path, provenance) in map {
                let crate::AST::ViewSource::Parameter(index) = provenance.source else {
                    continue;
                };
                let Some(actual) = call.args.get(index).map(|arg| &arg.expr) else {
                    continue;
                };
                let Some(place) = self.compose_view_source_place(
                    actual,
                    &provenance.projections,
                    init.span(),
                ) else {
                    self.report_temporary_view_source(actual.span(), string_view);
                    continue;
                };
                let kind = self.view_kind_for_place(&place);
                let access = if provenance.mutable {
                    ViewAccess::Write
                } else {
                    ViewAccess::Read
                };
                sources.push((output_path, place, kind, access));
            }
            return sources;
        }
        if let Expr::MethodCall {
            receiver,
            method,
            args,
            recv_type: Some(recv_type),
            ..
        } = init
        {
            let Some(map) = self
                .registry
                .method(recv_type, method)
                .and_then(|sig| sig.return_view_provenance.get())
                .or_else(|| {
                    self.trait_reg
                        .traits
                        .get(recv_type)
                        .and_then(|info| info.methods.get(method))
                        .and_then(|sig| sig.return_view_provenance.get())
                })
                .cloned()
            else {
                return Vec::new();
            };
            let mut sources = Vec::new();
            for (output_path, provenance) in map {
                let actual = match provenance.source {
                    crate::AST::ViewSource::Receiver => receiver.as_ref(),
                    crate::AST::ViewSource::Parameter(index) => {
                        let Some(arg) = args.get(index) else { continue };
                        &arg.expr
                    }
                    crate::AST::ViewSource::Static { .. } => continue,
                };
                let Some(place) = self.compose_view_source_place(
                    actual,
                    &provenance.projections,
                    init.span(),
                ) else {
                    continue;
                };
                let kind = self.view_kind_for_place(&place);
                let access = if provenance.mutable {
                    ViewAccess::Write
                } else {
                    ViewAccess::Read
                };
                sources.push((output_path, place, kind, access));
            }
            return sources;
        }
        if let Expr::Place(inner, access, _) = init {
            let Some(place) = self.place_from_expr(inner) else {
                return Vec::new();
            };
            let kind = self.view_kind_for_place(&place);
            let access = match access {
                crate::AST::PlaceAccess::Read => ViewAccess::Read,
                crate::AST::PlaceAccess::Write => ViewAccess::Write,
            };
            return vec![(Vec::new(), place, kind, access)];
        }
        let Expr::MethodCall {
            receiver, method, ..
        } = init
        else {
            return Vec::new();
        };
        if method != Syntax::METHOD_VIEW {
            return Vec::new();
        }
        let Some(mut place) = self.place_from_expr(receiver) else {
            return Vec::new();
        };
        let kind = match receiver.as_ref() {
            Expr::Ident(name, _) => self
                .view_kind(name)
                .unwrap_or_else(|| self.view_kind_for_place(&place)),
            _ => self.view_kind_for_place(&place),
        };
        place.projections.push(ViewProjection::Range {
            start: None,
            end: None,
            span: init.span(),
        });
        vec![(Vec::new(), place, kind, ViewAccess::Read)]
    }

    fn report_temporary_view_source(&mut self, span: Span, string_view: bool) {
        self.diags.push(Diagnostic::error(
            if string_view { "E2307" } else { "E2305" },
            "a returned view cannot borrow from a temporary argument".to_string(),
            "the temporary owner is dropped at the end of this statement, while the returned view remains live"
                .to_string(),
            "store the owner in a named binding first, then pass that binding to the view-returning call"
                .to_string(),
            Some(span),
        ));
    }

    /// Record `name` as a view into `owner`, declared at the current scope depth.
    pub(crate) fn record_list_view(
        &mut self,
        name: &str,
        output_path: Vec<String>,
        place: ViewPlace,
        kind: ViewKind,
        access: ViewAccess,
        span: Span,
    ) {
        self.record_view(name, output_path, place, kind, access, span);
    }

    pub(crate) fn transfer_named_view(&mut self, target: &str, source: &str, span: Span) -> bool {
        let facts: Vec<_> = self.view_facts(source).into_iter().cloned().collect();
        if facts.is_empty() {
            return false;
        }
        for fact in facts {
            self.record_view(
                target,
                fact.output_path,
                fact.place,
                fact.kind,
                fact.access,
                span,
            );
        }
        true
    }

    /// True if `name` is currently a live `View<T>` binding.
    pub(crate) fn is_list_view(&self, name: &str) -> bool {
        self.view_kind(name).is_some_and(ViewKind::is_named_window)
    }

    pub(crate) fn type_contains_view_boundary(&self, ty: &Type) -> bool {
        fn contains(
            registry: &TypeRegistry,
            ty: &Type,
            seen: &mut HashSet<String>,
        ) -> bool {
            match ty {
                Type::Apply { name, args }
                    if matches!(name.as_str(), "View" | "ViewMut") && args.len() == 1 =>
                {
                    true
                }
                Type::Named(name) | Type::Apply { name, .. } => {
                    if !seen.insert(name.clone()) {
                        return false;
                    }
                    registry.struct_fields(name).is_some_and(|fields| {
                        fields
                            .iter()
                            .any(|(_, _, field_ty, _)| contains(registry, field_ty, seen))
                    })
                }
                Type::Option(inner)
                | Type::List(inner)
                | Type::Shared(inner)
                | Type::Tagged { inner, .. } => contains(registry, inner, seen),
                Type::Result { ok, err } => {
                    contains(registry, ok, seen) || contains(registry, err, seen)
                }
                Type::Map { key, value, .. } => {
                    contains(registry, key, seen) || contains(registry, value, seen)
                }
                Type::Tuple(fields) => fields
                    .iter()
                    .any(|(_, field_ty)| contains(registry, field_ty, seen)),
                Type::FixedList { elem, .. } => contains(registry, elem, seen),
                Type::Fn { params, ret, .. } => {
                    params.iter().any(|param| contains(registry, param, seen))
                        || ret
                            .as_deref()
                            .is_some_and(|ret| contains(registry, ret, seen))
                }
                _ => false,
            }
        }
        contains(self.registry, ty, &mut HashSet::new())
    }

    pub(crate) fn named_view_has_stable_owner(&self, name: &str) -> bool {
        self.view_fact(name).is_some_and(|fact| {
            matches!(
                fact.place.owner.origin,
                ViewOwnerOrigin::Receiver
                    | ViewOwnerOrigin::Parameter(_)
                    | ViewOwnerOrigin::Static
            )
        })
    }

    /// E2305: `return list[a..b]` made fresh right in the `return` — `owner`
    /// (`list`) is made in this function and freed when it returns. Mirrors
    /// E2301's exact wording (`this view points into X, which this function
    /// owns`) for the "fresh call in return" shape; `report_list_view_escape`
    /// above covers the "already-bound name" shape.
    pub(crate) fn report_view_owns_return(&mut self, place: &ViewPlace, span: Span) {
        let owner = &place.owner.name;
        self.diags.push(Diagnostic::error(
            "E2305",
            format!(
                "this view points into `{}`, which this function owns",
                owner
            ),
            format!(
                "`{}` is made here and freed when the function returns, so a read window into it would outlive what owns it — there'd be nothing left to look at",
                owner
            ),
            "return an owned copy with `~place` (for example `~value[a..b]`), or accept the source as a parameter so the caller keeps owning it".to_string(),
            Some(span),
        ));
    }

    /// D-MEM-VIEWRET1=B: accept a returned named view only when its source is
    /// stable at the public boundary. Parameter position, never spelling, is
    /// the canonical identity. Multiple return paths must agree exactly.
    pub(crate) fn check_named_view_return(
        &mut self,
        place: &ViewPlace,
        access: ViewAccess,
        output_path: Vec<String>,
        span: Span,
    ) {
        let source = match place.owner.origin {
            ViewOwnerOrigin::Receiver => crate::AST::ViewSource::Receiver,
            ViewOwnerOrigin::Parameter(index) => crate::AST::ViewSource::Parameter(index),
            _ => {
                self.report_view_owns_return(place, span);
                return;
            }
        };
        let mut projections = Vec::new();
        for projection in &place.projections {
            projections.push(match projection {
                ViewProjection::Field(name) => crate::AST::ViewSourceProjection::Field(name.clone()),
                ViewProjection::Index { .. } => crate::AST::ViewSourceProjection::Index,
                ViewProjection::Range { .. } => crate::AST::ViewSourceProjection::Range,
                ViewProjection::Fresh(_) => {
                    self.report_view_return_boundary(span);
                    return;
                }
            });
        }
        let provenance = crate::AST::ViewProvenance {
            source,
            projections,
            mutable: access == ViewAccess::Write,
        };
        let map = self.return_view_provenance.get_or_insert_with(Default::default);
        if map.get(&output_path).is_some_and(|existing| existing != &provenance) {
            self.diags.push(Diagnostic::error(
                "E2305",
                "returned view paths disagree about their owner".to_string(),
                "each public output slot must name one stable owner source on every return path".to_string(),
                "for this output field, return views derived from the same parameter and place shape on every path, or return an owned copy".to_string(),
                Some(span),
            ));
            return;
        }
        map.insert(output_path, provenance);
    }

    pub(crate) fn check_named_view_binding_return(&mut self, name: &str, span: Span) {
        let Some(fact) = self.view_fact(name).cloned() else {
            self.report_view_return_boundary(span);
            return;
        };
        self.check_named_view_return(&fact.place, fact.access, Vec::new(), span);
    }

    pub(crate) fn check_named_string_view_binding_return(&mut self, name: &str, span: Span) {
        let Some(fact) = self.view_fact(name).cloned() else {
            self.report_string_view_boundary(span);
            return;
        };
        if !matches!(
            fact.place.owner.origin,
            ViewOwnerOrigin::Receiver | ViewOwnerOrigin::Parameter(_)
        ) {
            self.report_string_view_unsupported_use(name, "be returned", span);
            return;
        }
        self.check_named_view_return(&fact.place, fact.access, Vec::new(), span);
    }

    pub(crate) fn check_aggregate_view_return(&mut self, expr: &Expr) {
        fn walk(checker: &mut Checker<'_>, expr: &Expr, path: &mut Vec<String>) {
            match expr {
                Expr::StructLit { fields, .. } => {
                    for (field, _, value) in fields {
                        path.push(field.clone());
                        walk(checker, value, path);
                        path.pop();
                    }
                }
                Expr::TypedLit { body, .. } => {
                    body.for_each_expr(|value| walk(checker, value, path));
                }
                Expr::TupleLit(fields, ..) => {
                    for (field, value) in fields {
                        path.push(field.clone());
                        walk(checker, value, path);
                        path.pop();
                    }
                }
                Expr::Present(inner, _)
                | Expr::Ok(inner, _)
                | Expr::Err(inner, _)
                | Expr::Paren(inner, _) => walk(checker, inner, path),
                Expr::Ident(name, span) => {
                    let facts: Vec<_> = checker.view_facts(name).into_iter().cloned().collect();
                    for fact in facts {
                        let mut output_path = path.clone();
                        output_path.extend(fact.output_path);
                        checker.check_named_view_return(
                            &fact.place,
                            fact.access,
                            output_path,
                            *span,
                        );
                    }
                }
                _ => {
                    for (suffix, place, _, access) in checker.view_call_sources(expr) {
                        let mut output_path = path.clone();
                        output_path.extend(suffix);
                        checker.check_named_view_return(
                            &place,
                            access,
                            output_path,
                            expr.span(),
                        );
                    }
                }
            }
        }
        walk(self, expr, &mut Vec::new());
    }

    pub(crate) fn report_view_return_boundary(&mut self, span: Span) {
        self.diags.push(Diagnostic::error(
            "E2305",
            "returned views need a stable owner relationship".to_string(),
            "public view provenance is not carried through compiler APIs and TIR yet, so this return could outlive its owner".to_string(),
            "keep the view local, or return an owned copy instead".to_string(),
            Some(span),
        ));
    }

    pub(crate) fn report_string_view_boundary(&mut self, span: Span) {
        self.diags.push(Diagnostic::error(
            "E2307",
            "returned string views need a stable owner relationship".to_string(),
            "the compiler could not prove which caller-owned `String` keeps this `View<str>` alive"
                .to_string(),
            "return a view derived from one parameter or receiver on every path, or return an owned `String` copy"
                .to_string(),
            Some(span),
        ));
    }

    // ──────────────────────────────────────────────────────────────────────
    // D-MEM1 stage S5 (2026-07-04): string `view`s — `s.trim()` / `s.after(sep)`
    // / `s.before(sep)` bound to a local name return a zero-copy `&str` window
    // into `s` instead of an owned `String`. Unlike `View<T>` (a distinct Jet
    // type), `String` stays ONE type end to end (D-MEM1 gallery: "one String
    // type") — so the view-ness lives on the *binding* (`Binding::string_view`,
    // set below), not on the value's static type. The shared fact graph reports
    // E2307 on escape
    // (returned, rebound, stored in a struct field); crossing a task boundary
    // is caught separately at the capture-check site (`CheckerInfer/calls.rs`)
    // since a plain `Type::String` carries no view marker for the general
    // sendability check to key off.
    // ──────────────────────────────────────────────────────────────────────

    /// If `init` is `s.trim()` / `s.after(sep)` / `s.before(sep)` on a plain
    /// local `String` name, return `s`'s name — but only when `s` is a genuine
    /// local or parameter. Parameters must remain tracked so a `View<str>`
    /// return can publish their stable source provenance.
    pub(crate) fn string_view_call_source(&self, init: &Expr) -> Option<ViewPlace> {
        let Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } = init
        else {
            return None;
        };
        let arity_ok = match method.as_str() {
            "trim" => args.is_empty(),
            "after" | "before" => args.len() == 1,
            _ => return None,
        };
        if !arity_ok {
            return None;
        }
        let Expr::Ident(name, _) = receiver.as_ref() else {
            return None;
        };
        if self.consts.contains_key(name) {
            return None;
        }
        let Some(info) = self.lookup(name) else {
            return None;
        };
        if !matches!(info.ty, Type::String) {
            return None;
        }
        self.place_from_expr(receiver)
    }

    /// Record `name` as a string view into `owner`, declared at the current
    /// scope depth.
    pub(crate) fn record_string_view(&mut self, name: &str, place: ViewPlace, span: Span) {
        self.record_view(name, Vec::new(), place, ViewKind::String, ViewAccess::Read, span);
    }

    /// True if `name` is currently a live string-view binding.
    pub(crate) fn is_string_view(&self, name: &str) -> bool {
        self.view_kind(name) == Some(ViewKind::String)
    }

    /// E2307: a string view named `name` was used somewhere only its full
    /// `String` representation supports — this is the ONE call site for the
    /// whole class of unsupported uses (return, rebind, struct field, call
    /// argument, list/tuple literal element, an arbitrary builtin/user
    /// method, …): the general `Expr::Ident` inference arm calls this
    /// whenever it reads a live string-view name outside the two positions
    /// its bare `&str` Rust place actually supports (chaining another
    /// `.trim()`/`.after()`/`.before()`, or `copy`'s operand). This is a
    /// representation limit, not a lifetime one — the view DOES live long
    /// enough; it just isn't the value shape the destination needs — so the
    /// wording says so rather than claiming it "doesn't live long enough"
    /// (unlike `View<T>`, which needs its own escape check since a `List`
    /// re-assignment/return is otherwise silent). `what` describes the use site.
    pub(crate) fn report_string_view_unsupported_use(
        &mut self,
        name: &str,
        what: &str,
        span: Span,
    ) {
        let owner = self
            .view_facts
            .bindings
            .iter()
            .rev()
            .find_map(|(binding, fact)| {
                (binding == name).then(|| fact.place.owner.name.clone())
            })
            .unwrap_or_else(|| "its owner".to_string());
        self.diags.push(Diagnostic::error(
            "E2307",
            format!("`{}` can't {} yet", name, what),
            format!(
                "`{}` is a zero-copy view into `{}` (`.trim()`/`.after()`/`.before()`); only chaining another `.trim()`/`.after()`/`.before()` on it works directly — other methods and calls need the full owned value",
                name, owner
            ),
            format!("write `{}{}` first to get an owned `String`, then use that", Syntax::SIGIL_COPY, name),
            Some(span),
        ));
    }

    // No "fresh call made right in the return" shape exists for string views
    // (unlike `View<T>`, which is type-driven everywhere `.view()` appears):
    // view-ness here lives on the *binding*, set only for `Stmt::Val`, so
    // `return s.after(sep)` written directly (no intermediate binding) always
    // lowers as an ordinary owned `String` — safe, nothing to catch.

    /// D-LIN1 (ratified 2026-06-21): true when `ty` is a `#SingleUse` struct/enum,
    /// checking the local registry first and then any imported module that exposes
    /// the type publicly. A `#SingleUse` value must be consumed exactly once and
    /// may not be aliased.
    pub(crate) fn type_is_single_use(&self, ty: &Type) -> bool {
        let Some(name) = ty.base_name() else {
            return false;
        };
        if self.registry.is_single_use(name) {
            return true;
        }
        if let Some(mods) = self.modules {
            for &idx in self.imports.values() {
                if self.type_is_pub_in(idx, name) && mods[idx].registry.is_single_use(name) {
                    return true;
                }
            }
        }
        false
    }

    /// D-MUSTUSE1 (c18iwxqx): true when `ty` is a `#MustUse` struct/enum or a
    /// built-in guard/handle type whose result must not be silently ignored.
    pub(crate) fn type_is_must_use(&self, ty: &Type) -> bool {
        let Some(name) = ty.base_name() else {
            return false;
        };
        if super::CheckerCoreLib::core_must_use_type(name) {
            return true;
        }
        if self.registry.is_must_use(name) {
            return true;
        }
        if let Some(mods) = self.modules {
            for &idx in self.imports.values() {
                if self.type_is_pub_in(idx, name) && mods[idx].registry.is_must_use(name) {
                    return true;
                }
            }
        }
        false
    }

    /// D-MUSTUSE1 (c18iwxqx): name of a `#MustUse` fn/method call when `expr` is
    /// that call, else `None`.
    pub(crate) fn ignored_must_use_call_target(&self, expr: &Expr) -> Option<String> {
        if let Expr::Call(call) = expr {
            if self.funcs.get(&call.name).is_some_and(|s| s.is_must_use) {
                return Some(call.name.clone());
            }
        }
        if let Expr::MethodCall {
            receiver, method, ..
        } = expr
        {
            if let Some(type_name) = self.receiver_type_name(receiver) {
                if self.method_is_must_use(&type_name, method) {
                    return Some(format!("{type_name}.{method}"));
                }
            }
        }
        None
    }

    fn receiver_type_name(&self, receiver: &Expr) -> Option<String> {
        match receiver {
            Expr::Ident(name, _) => self
                .lookup(name)
                .and_then(|info| info.ty.base_name().map(str::to_string)),
            _ => None,
        }
    }

    fn method_is_must_use(&self, type_name: &str, method: &str) -> bool {
        if self
            .registry
            .method(type_name, method)
            .is_some_and(|m| m.must_use)
        {
            return true;
        }
        if let Some(mods) = self.modules {
            for &idx in self.imports.values() {
                if self.type_is_pub_in(idx, type_name) {
                    if let Some(m) = mods[idx].registry.method(type_name, method) {
                        if m.must_use {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// D-MUSTUSE1 (c18iwxqx): emit E0419 when a must-use result is dropped as a
    /// bare expression statement.
    pub(crate) fn check_ignored_must_use(&mut self, expr: &Expr, ty: &Type, span: Span) {
        if self.suppress_must_use || is_task_type(ty) {
            return;
        }
        let target = if let Some(name) = self.ignored_must_use_call_target(expr) {
            name
        } else if matches!(ty, Type::Named(n) if n == "Unit") {
            return;
        } else if self.type_is_must_use(ty) {
            ty.name()
        } else {
            return;
        };
        self.diags.push(Diagnostic::error(
            "E0419",
            format!("`{target}` must be used — it was dropped as a bare statement"),
            "a `#MustUse` result carries work or a resource that is lost when nothing checks it"
                .to_string(),
            format!(
                "bind it (`x := …`), use `{}`, `{} …`, or `.drop(\"reason\")` to discard intentionally",
                Syntax::OP_TRY_SUFFIX,
                Syntax::BUILTIN_PANIC
            ),
            Some(span),
        ));
    }

    pub(crate) fn has_core_mem_import(&self) -> bool {
        self.core_imports
            .values()
            .any(|m| m == Syntax::CORE_MEM_MODULE)
    }

    /// c26 / arena-inference lint: heap growth in a loop after `use core.mem`.
    /// c26 / allocation-boundary lint: growable calls inside `#Context` without allocator.
    pub(crate) fn lint_allocation_hints(&mut self, method: &str, span: Span) {
        let grows_heap = matches!(method, "push" | "append" | "insert" | "add" | "add_new");
        if !grows_heap {
            return;
        }
        if self.loop_depth > 0 && self.has_core_mem_import() {
            self.diags.push(Diagnostic::lint(
                "L0505",
                "heap growth inside a loop — consider an arena".to_string(),
                "each `push` may allocate on the global heap; in a hot loop that adds up".to_string(),
                "bind `arena :: mem.Arena.new()` outside the loop and use `arena.alloc(…)` for scratch data".to_string(),
                Some(span),
            ));
        }
        if self.context_depth > 0 && !self.context_allocator_active {
            self.diags.push(Diagnostic::lint(
                "L0506",
                "hidden allocation inside `#Context` without an allocator".to_string(),
                "this call may allocate on the global heap while a scoped context is active".to_string(),
                "add `allocator: …` to the `#Context(…)` fields, or move the allocation outside the block".to_string(),
                Some(span),
            ));
        }
    }

    fn move_keys_overlap(left: &str, right: &str) -> bool {
        Self::contains_place(left, right) || Self::contains_place(right, left)
    }

    fn contains_place(parent: &str, child: &str) -> bool {
        child.strip_prefix(parent).is_some_and(|suffix| {
            suffix.is_empty() || suffix.starts_with('.') || suffix.starts_with('[')
        })
    }

    pub(crate) fn clear_moved_binding(&mut self, name: &str) {
        self.moved
            .retain(|place, _| !Self::contains_place(name, place));
    }

    pub(crate) fn clear_moved_expr(&mut self, expr: &Expr) {
        let Some(place) = self.capture_place_from_expr(expr) else {
            return;
        };
        let place = Self::place_name(&place);
        self.moved
            .retain(|moved, _| !Self::contains_place(&place, moved));
    }

    pub(crate) fn reject_moved_expr_use(&mut self, expr: &Expr, span: Span) -> bool {
        let Some(place) = self.capture_place_from_expr(expr) else {
            return false;
        };
        let place_name = Self::place_name(&place);
        let moved = if matches!(expr, Expr::Ident(..)) && self.suppress_partial_move_root_read {
            self.moved
                .get(&place_name)
                .copied()
                .map(|at| (place_name.clone(), at))
        } else {
            self.moved
                .iter()
                .filter(|(moved, _)| Self::move_keys_overlap(&place_name, moved))
                .min_by_key(|(moved, _)| moved.len())
                .map(|(moved, at)| (moved.clone(), *at))
        };
        let Some((moved_place, _moved_at)) = moved else {
            return false;
        };
        let root = place.owner.name;
        let moved_ty = self.lookup(&root).map(|info| info.ty.clone());
        let fix = if moved_ty
            .as_ref()
            .is_some_and(|ty| self.is_resource_type(ty))
        {
            format!(
                "acquire a new `{}` resource; closed resources cannot be copied or reused",
                moved_ty.as_ref().map(Type::show).unwrap_or_default()
            )
        } else {
            format!(
                "give away a copy instead (`{}{}`) where it moved",
                Syntax::SIGIL_COPY,
                moved_place
            )
        };
        self.diags.push(Diagnostic::error(
            "E0121",
            format!("`{moved_place}` was given away earlier, so it can't be used here"),
            "after a value moves somewhere else, the old name no longer holds it".to_string(),
            fix,
            Some(span),
        ));
        self.moved.remove(&moved_place);
        true
    }

    pub(crate) fn mark_moved_place(&mut self, place: ViewPlace, span: Span) {
        let name = place.owner.name.clone();
        if !tracks_named_move(&name) {
            return;
        }
        if self.reject_expiring_secret_loan_change(&name, "be moved", span) {
            return;
        }
        self.check_owner_change(&name, "be moved", span);
        if let Some(info) = self.lookup(&name) {
            if info.decl_loop_depth < self.loop_depth {
                self.diags.push(Diagnostic::error(
                    "E0121",
                    format!("`{}` is given away inside a loop that may run again", name),
                    "after a value is given away it's gone, but the next time around the loop would need it again".to_string(),
                    format!("give away a copy instead: `{}{}`", Syntax::SIGIL_COPY, name),
                    Some(span),
                ));
                return;
            }
        }
        self.moved.insert(Self::place_name(&place), span);
    }

    pub(crate) fn mark_moved(&mut self, name: String, span: Span) {
        self.mark_moved_place(
            ViewPlace {
                owner: self.owner_id(&name),
                projections: Vec::new(),
            },
            span,
        );
    }

    pub(crate) fn non_name_write_argument_fix(&self, expr: &Expr) -> String {
        let indexes_list = matches!(expr, Expr::Index { .. })
            && expr_root_ident(expr)
                .and_then(|name| self.lookup(name))
                .is_some_and(|info| {
                    matches!(&info.ty, Type::List(_) | Type::FixedList { .. })
                });
        if indexes_list {
            format!(
                "change the helper to accept a list window, then pass a range write window such as `{}xs[a..b]`",
                Syntax::SIGIL_WRITE,
            )
        } else {
            format!(
                "bind the value first: `x {} ...` then pass `{}x`",
                Syntax::SIGIL_BIND_MUT,
                Syntax::SIGIL_WRITE,
            )
        }
    }

    fn reject_expiring_secret_loan_change(
        &mut self,
        name: &str,
        action: &str,
        span: Span,
    ) -> bool {
        if !self.lookup(name).is_some_and(|info| {
            crate::Sema::Diagnostics::contains_expiring_secret_loan(&info.ty)
        }) {
            return false;
        }
        self.diags.push(Diagnostic::error(
            "E0201",
            format!("ExpiringSecret loan `{name}` cannot {action}"),
            "the callback receives temporary read access; moving, changing, or dropping it could let the credential escape its expiry boundary".to_string(),
            "use the loan only for read-only operations inside this callback".to_string(),
            Some(span),
        ));
        true
    }

    /// `x = y` / `a :: y` / `return y` where `y` is a plain name of a
    /// non-scalar type gives the value away (assignment moves, see C1).
    pub(crate) fn note_move_if_direct_ident(&mut self, e: &Expr) {
        if let Expr::Ident(n, span) = e {
            if let Some(info) = self.lookup(n) {
                if !info.ty.is_scalar() && info.param_conv.is_none() {
                    self.mark_moved(n.clone(), *span);
                }
            }
        }
    }

    pub(crate) fn lint_unjoined_tasks_in_current_scope(&mut self) {
        let Some(scope) = self.scopes.last() else {
            return;
        };
        let pending: Vec<(String, Span)> = scope
            .iter()
            .filter_map(|(name, info)| {
                let span = info.task_lint_span?;
                if self.moved.contains_key(name) {
                    None
                } else {
                    Some((name.clone(), span))
                }
            })
            .collect();
        for (name, span) in pending {
            self.diags.push(Diagnostic::lint(
                "L1101",
                format!("task `{}` is dropped without `.join()`", name),
                "the program may end before this task finishes".to_string(),
                "call `.join()` on the task before it goes out of scope, or call `.detach()` if fire-and-forget is intentional".to_string(),
                Some(span),
            ));
        }
    }

    /// D-LIN1 (ratified 2026-06-21): E0140 — a `#SingleUse` value that owns the
    /// consume duty (`single_use_span` set) but is not in `moved` when its scope
    /// ends was dropped without being used. Mirrors the unjoined-task check: it
    /// looks only at the innermost (just-closing) scope. The branch-divergence
    /// case (consumed on one path, dropped on the other) is E0141, raised in
    /// `check_if`.
    pub(crate) fn check_single_use_consumed_in_current_scope(&mut self) {
        let Some(scope) = self.scopes.last() else {
            return;
        };
        let pending: Vec<(String, Span)> = scope
            .iter()
            .filter_map(|(name, info)| {
                let span = info.single_use_span?;
                if self.moved.contains_key(name) {
                    None
                } else {
                    Some((name.clone(), span))
                }
            })
            .collect();
        // Deterministic order (HashMap iteration is unordered): by span, then name.
        let mut pending = pending;
        pending.sort_by(|a, b| a.1.start.cmp(&b.1.start).then(a.0.cmp(&b.0)));
        for (name, span) in pending {
            self.diags.push(e0140_unconsumed(&name, span));
        }
    }

    pub(crate) fn lambda_value_sendable(&self, lam: &Lambda, fn_ty: &Type) -> bool {
        let param_names: HashSet<String> = lam.params.iter().map(|p| p.name.clone()).collect();
        let take_set: HashSet<String> = lam.take_names.iter().map(|(n, _)| n.clone()).collect();
        let mut read_caps = HashSet::new();
        let mut mut_caps = HashSet::new();
        lambda_collect_captures(&lam.body, &param_names, &mut read_caps, &mut mut_caps);
        for name in read_caps.iter().chain(mut_caps.iter()) {
            if param_names.contains(name) {
                continue;
            }
            let taken = take_set.contains(name);
            if mut_caps.contains(name) && !taken {
                return false;
            }
            let cap = self
                .lookup(name)
                .map(|i| (i.ty.clone(), i.sendable))
                .or_else(|| self.consts.get(name).map(|t| (t.clone(), true)));
            let Some((cap_ty, cap_sendable)) = cap else {
                continue;
            };
            if !cap_sendable || self.sendability_problem(&cap_ty, taken).is_some() {
                return false;
            }
        }
        if let Type::Fn { ret: Some(ret), .. } = fn_ty {
            self.sendability_problem(ret, false).is_none()
        } else {
            true
        }
    }

    pub(crate) fn sendability_problem(
        &self,
        ty: &Type,
        closure_taken: bool,
    ) -> Option<SendabilityProblem> {
        let mut seen = HashSet::new();
        self.sendability_problem_inner(ty, closure_taken, &mut seen)
    }

    pub(crate) fn sendability_problem_inner(
        &self,
        ty: &Type,
        closure_taken: bool,
        seen: &mut HashSet<String>,
    ) -> Option<SendabilityProblem> {
        match ty {
            Type::Int | Type::Float | Type::Bool | Type::String | Type::Char => None,
            Type::IntN { .. } | Type::Float32 => None,
            Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => {
                self.sendability_problem_inner(inner, true, seen)
            }
            Type::Map { key, value, .. } => self
                .sendability_problem_inner(key, true, seen)
                .or_else(|| self.sendability_problem_inner(value, true, seen)),
            Type::Result { ok, err } => self
                .sendability_problem_inner(ok, true, seen)
                .or_else(|| self.sendability_problem_inner(err, true, seen)),
            Type::Fn { .. } => {
                if closure_taken {
                    None
                } else {
                    Some(SendabilityProblem {
                        root: None,
                        path: Vec::new(),
                        kind: SendProblemKind::ClosureNeedsTake,
                    })
                }
            }
            // #648: allocator handles own interior-mutability and raw backing
            // storage. They are deliberately thread-confined; values allocated
            // from them may never race reset/close on another task.
            Type::Named(name) if crate::Syntax::alloc_handle_rust_type(name).is_some() => {
                Some(SendabilityProblem {
                    root: None,
                    path: Vec::new(),
                    kind: SendProblemKind::ThreadConfined(name.clone()),
                })
            }
            // D-BROWSER-AUTO1=A: Browser protocol objects retain one
            // Rc/RefCell-backed session and are deliberately thread-confined.
            Type::Named(name)
                if matches!(
                    name.as_str(),
                    "Browser"
                        | "BrowserContext"
                        | "BrowserPage"
                        | "BrowserLocator"
                        | "BrowserProtocol"
                ) =>
            {
                Some(SendabilityProblem {
                    root: None,
                    path: Vec::new(),
                    kind: SendProblemKind::ThreadConfined(name.clone()),
                })
            }
            Type::Named(name) if is_type_var_name(name) || core_type_known(name) => None,
            Type::Named(name) => self.named_sendability_problem(name, &[], seen),
            Type::Apply { name, args }
                if matches!(name.as_str(), "Task" | "Channel" | "Sender") =>
            {
                args.iter()
                    .find_map(|arg| self.sendability_problem_inner(arg, true, seen))
            }
            // D-DYNARRAY1 (E2303, reported as E1102): a `View<T>` is a borrow into
            // its owner's backing storage — it can never cross a task/channel
            // boundary, the same rule a `-> view`/`ref` value already gets.
            Type::Apply { name, .. } if matches!(name.as_str(), "View" | "ViewMut") => Some(SendabilityProblem {
                root: None,
                path: Vec::new(),
                kind: SendProblemKind::ViewBorrow,
            }),
            Type::Apply { name, args } => self.named_sendability_problem(name, args, seen),
            Type::TraitObject(names) => Some(SendabilityProblem {
                root: None,
                path: Vec::new(),
                kind: SendProblemKind::TraitValue(names.join(" + ")),
            }),
            Type::Tuple(fields) => fields
                .iter()
                .find_map(|(_, t)| self.sendability_problem_inner(t, true, seen)),
            Type::FixedList { elem, .. } => self.sendability_problem_inner(elem, true, seen),
            Type::Tagged { inner, .. } => {
                self.sendability_problem_inner(inner, closure_taken, seen)
            }
            Type::Union(members) => members
                .iter()
                .find_map(|m| self.sendability_problem_inner(m, closure_taken, seen)),
        }
    }

    pub(crate) fn named_sendability_problem(
        &self,
        name: &str,
        args: &[Type],
        seen: &mut HashSet<String>,
    ) -> Option<SendabilityProblem> {
        if !seen.insert(name.to_string()) {
            return None;
        }
        let subst = if args.is_empty() {
            HashMap::new()
        } else {
            self.struct_subst(name, args)
        };
        let found = match self.registry.types.get(name) {
            Some(TypeDef::Struct { fields, .. }) => {
                for (field_name, _, field_ty, _) in fields {
                    let actual_ty = self.trait_reg.instantiate_type(field_ty, &subst);
                    if let Some(problem) = self.sendability_problem_inner(&actual_ty, true, seen) {
                        return Some(prepend_send_path(name, field_name, problem));
                    }
                }
                None
            }
            Some(TypeDef::Enum { variants, .. }) => {
                for (_, payload) in variants.values() {
                    let problem = match payload {
                        VariantPayload::Unit => None,
                        VariantPayload::Single(ty, _) => {
                            let actual_ty = self.trait_reg.instantiate_type(ty, &subst);
                            self.sendability_problem_inner(&actual_ty, true, seen)
                        }
                        VariantPayload::Named(fields) => fields.iter().find_map(|field| {
                            let actual_ty = self.trait_reg.instantiate_type(&field.ty, &subst);
                            self.sendability_problem_inner(&actual_ty, true, seen)
                                .map(|p| prepend_send_path(name, &field.name, p))
                        }),
                    };
                    if let Some(problem) = problem {
                        return Some(problem);
                    }
                }
                None
            }
            // D-DIST1: distinct types wrap a scalar; they are always Send.
            Some(TypeDef::Distinct { .. }) | Some(TypeDef::Alias { .. }) | None => None,
        };
        seen.remove(name);
        found
    }

    pub(crate) fn expr_sendability_problem(
        &self,
        expr: &Expr,
        ty: &Type,
        closure_taken: bool,
    ) -> Option<SendabilityProblem> {
        if let Expr::Ident(name, _) = expr {
            if let Some(info) = self.lookup(name) {
                if !info.sendable {
                    return self
                        .sendability_problem(&info.ty, closure_taken)
                        .or_else(|| {
                            Some(SendabilityProblem {
                                root: None,
                                path: Vec::new(),
                                kind: SendProblemKind::ClosureCaptures,
                            })
                        });
                }
            }
        }
        self.sendability_problem(ty, closure_taken)
    }

    pub(crate) fn note_reactive_upgrade(&mut self, name: &str, ty: &Type, crossing: &str) {
        self.reactive_upgrade_names.insert(name.to_string());
        let line = format!(
            "{name}: {} synchronized for {crossing} crossing",
            ty.show()
        );
        if !self.reactive_upgrades.iter().any(|existing| existing == &line) {
            self.reactive_upgrades.push(line);
        }
    }

    pub(crate) fn report_unsendable(
        &mut self,
        value: &str,
        ty: &Type,
        problem: SendabilityProblem,
        crossing: SendCrossing,
        span: Span,
    ) {
        let type_name = ty.name();
        let value_text = if value == "this value" {
            "this value".to_string()
        } else {
            format!("`{}`", value)
        };
        let what = match (crossing, &problem.kind) {
            (SendCrossing::TaskCapture, SendProblemKind::ViewBorrow) => {
                format!(
                    "{} cannot be shared into a task — it is a view that does not live long enough",
                    value_text
                )
            }
            (SendCrossing::TaskResult, SendProblemKind::ViewBorrow) => {
                "this task returns a view that cannot be shared — it does not live long enough to cross into a task".to_string()
            }
            (SendCrossing::ChannelSend, SendProblemKind::ViewBorrow) => {
                format!("{} cannot be shared across channels — it is a view that does not live long enough", value_text)
            }
            (SendCrossing::TaskCapture, _) => {
                format!(
                    "{} can't cross into a task because `{}` isn't sendable",
                    value_text, type_name
                )
            }
            (SendCrossing::TaskResult, _) => {
                format!("this task returns `{}`, which isn't sendable", type_name)
            }
            (SendCrossing::ChannelSend, _) => {
                format!(
                    "{} can't be sent because `{}` isn't sendable",
                    value_text, type_name
                )
            }
        };
        let why = format!(
            "{}; tasks and channels move owned values between threads",
            describe_sendability_problem(&problem)
        );
        let fix = match crossing {
            SendCrossing::ChannelSend => {
                "send plain owned data instead, or rebuild the value as an owned copy before calling `.send()`"
            }
            SendCrossing::TaskCapture | SendCrossing::TaskResult => {
                "give the task plain owned data, or rebuild the value as an owned copy before spawning"
            }
        };
        // D-DETACH1: if this E1102 fires in a task spawn context, record the task
        // binding name so the right detach diagnostic fires when `.detach()` is called:
        //   - ViewBorrow → E1106 ("pass an owned copy/share"); view can outlive the borrow
        //   - other sendability failures → E1103 (general unsound-detach)
        if matches!(
            crossing,
            SendCrossing::TaskCapture | SendCrossing::TaskResult
        ) {
            if let Some(name) = &self.current_binding_name {
                if matches!(problem.kind, SendProblemKind::ViewBorrow) {
                    self.view_borrow_escape_tasks.insert(name.clone());
                } else {
                    self.view_capture_tasks.insert(name.clone());
                }
            }
        }
        self.diags.push(Diagnostic::error(
            "E1102",
            what,
            why,
            fix.to_string(),
            Some(span),
        ));
    }

    pub(crate) fn consume_builtin_receiver(&mut self, receiver: &Expr, method: &str) {
        if let Expr::Ident(name, span) = receiver {
            if let Some(info) = self.lookup(name) {
                if !type_is_copy(&info.ty)
                    && matches!(
                        info.param_conv,
                        Some(AccessConvention::Read) | Some(AccessConvention::Write)
                    )
                {
                    self.diags.push(Diagnostic::error(
                        "E0120",
                        format!(
                            "`{}` was not moved here, so `.{}()` can't take it",
                            name, method
                        ),
                        "this function has read access only and does not own the value".to_string(),
                        format!(
                            "call it on a copy, or take ownership with `{}: {}{}`",
                            name,
                            Syntax::SIGIL_MOVE,
                            info.ty.name()
                        ),
                        Some(*span),
                    ));
                    return;
                }
                if !info.ty.is_scalar() {
                    self.mark_moved(name.clone(), *span);
                }
            }
        }
    }

    /// D-MEM1/S2: no clone is ever silent (I8) — the former D-L0201 lint's
    /// cloneable carve-out is now a hard error (E0209, was `L0201`). `what`/
    /// `why` are call-site-specific; this builds the shared, liveness-aware
    /// fix menu: `^name` when this call is `name`'s last use (safe to move),
    /// or `~name` (D-SHAPE-COPY1, supersedes D-CAP2/S4)/reorder when `name`
    /// is still used afterward (moving now would break that later use).
    pub(crate) fn e0209_implicit_clone(
        &self,
        what: String,
        why: String,
        name: &str,
        span: Span,
    ) -> Diagnostic {
        let fix = if self.is_name_live_after(name) {
            format!(
                "`{name}` is used again after this call, so `{}{name}` would break that later use — write `{}{name}` to pass a copy, or reorder so this call is `{name}`'s last use and write `{}{name}`",
                Syntax::SIGIL_MOVE,
                Syntax::SIGIL_COPY,
                Syntax::SIGIL_MOVE,
            )
        } else {
            format!(
                "write `{}{name}` to move it — this is `{name}`'s last use — or `{}{name}` to keep a copy",
                Syntax::SIGIL_MOVE,
                Syntax::SIGIL_COPY,
            )
        };
        Diagnostic::error("E0209", what, why, fix, Some(span))
    }

    pub(crate) fn check_take_arg_ownership(
        &mut self,
        call_name: &str,
        idx: usize,
        param_ty: &Type,
        arg: &mut crate::AST::CallArg,
    ) {
        // D-ALLOC2: E0631 — a view passed to a `take`/`mut` (out) parameter
        // would let the callee keep a borrow past the arena's region. Reading
        // it (Read convention) is fine; only ownership/`mut` transfer escapes.
        if matches!(
            arg.convention,
            AccessConvention::Move | AccessConvention::Write
        ) {
            if let Expr::Ident(name, span) = &arg.expr {
                if self.is_view(name) && !self.is_string_view(name) {
                    let verb = if matches!(arg.convention, AccessConvention::Move) {
                        "be given away"
                    } else {
                        "be lent out for mutation"
                    };
                    self.report_view_escape(name, verb, *span);
                }
            }
        }
        match arg.convention {
            AccessConvention::Read => {
                if let Expr::Ident(name, span) = &arg.expr {
                    if !crate::Sema::Diagnostics::is_secret_bearing_crypto_type(param_ty)
                        && !self.is_resource_type(param_ty)
                        && is_cloneable(param_ty, self.registry)
                    {
                        arg.flags.implicit_clone = true;
                        // D-MEM1/S2 (was D-L0201 lint): passing a named binding to
                        // a Move param without `^` is always a hard error now.
                        let diag = self.e0209_implicit_clone(
                            format!("implicit clone of `{}`", name),
                            format!("`{}` expects to take ownership of this value", call_name),
                            name,
                            *span,
                        );
                        self.diags.push(diag);
                    } else {
                        self.diags.push(Diagnostic::error(
                            "E0201",
                            format!(
                                "`{}` needs `{}` here — this value can't be copied",
                                call_name,
                                Syntax::SIGIL_MOVE
                            ),
                            format!(
                                "parameter {} takes ownership (`^`); passing `{}` without `{}` would have to copy it, but this type can't be copied",
                                idx + 1,
                                name,
                                Syntax::SIGIL_MOVE
                            ),
                            format!("write `{}{}` to move ownership to `{}`", Syntax::SIGIL_MOVE, name, call_name),
                            Some(*span),
                        ));
                    }
                }
            }
            AccessConvention::Move => {
                if let Expr::Ident(name, span) = &arg.expr {
                    if !param_ty.is_scalar() {
                        self.mark_moved(name.clone(), *span);
                    }
                }
            }
            AccessConvention::Write => {}
        }
    }

    pub(crate) fn finish_sender_send(
        &mut self,
        recv_ty: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        let elem_ty = match recv_ty {
            Type::Apply { name, args } if name == "Sender" => {
                args.first().cloned().unwrap_or(Type::Int)
            }
            _ => Type::Int,
        };
        if args.len() != 1 {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!("`send` expects 1 argument, got {}", args.len()),
                "sending needs exactly one value".to_string(),
                "call `.send(value)` with the value to send".to_string(),
                Some(span),
            ));
        }
        let Some(arg) = args.get_mut(0) else {
            return None;
        };
        let saved_exp = self.expected_type.clone();
        self.expected_type = Some(elem_ty.clone());
        let got = self.infer(&mut arg.expr);
        self.expected_type = saved_exp;
        let mut sendability_failed = false;
        if let Some(got) = got {
            let reported = self.check_type_assignable(&elem_ty, &got, arg.expr.span());
            if !reported && got != elem_ty {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!(
                        "`send` wants {} for argument 1, but this is {}",
                        elem_ty.show(),
                        got.show()
                    ),
                    "a sender can only send values of its channel's element type".to_string(),
                    type_fix_hint(&elem_ty, &got),
                    Some(arg.expr.span()),
                ));
            }
            if let Some(problem) = self.expr_sendability_problem(
                &arg.expr,
                &got,
                matches!(arg.convention, AccessConvention::Move),
            ) {
                let value_name = match &arg.expr {
                    Expr::Ident(name, _) => name.as_str(),
                    _ => "this value",
                };
                self.report_unsendable(
                    value_name,
                    &got,
                    problem,
                    SendCrossing::ChannelSend,
                    arg.expr.span(),
                );
                sendability_failed = true;
            } else if crate::Sema::CheckerInfer::is_reactive_handle_ty(&got) {
                match &arg.expr {
                    Expr::Ident(name, _) => {
                        if self.lookup(name).is_some_and(|info| info.reactive_local) {
                            self.diags.push(Diagnostic::error(
                                "E1102",
                                format!(
                                    "`{name}` is pinned `#{}` and can't be sent on a channel",
                                    crate::Syntax::ATTR_LOCAL
                                ),
                                format!(
                                    "`#{}` keeps `{}` in the fast one-thread form",
                                    crate::Syntax::ATTR_LOCAL,
                                    got.name()
                                ),
                                format!(
                                    "remove `#{}`, or send an owned copy of the value",
                                    crate::Syntax::ATTR_LOCAL
                                ),
                                Some(arg.expr.span()),
                            ));
                            sendability_failed = true;
                        } else {
                            self.note_reactive_upgrade(name, &got, "channel");
                        }
                    }
                    _ => self.note_reactive_upgrade("this value", &got, "channel"),
                }
            }
        }
        if !sendability_failed {
            self.check_take_arg_ownership("send", 0, &elem_ty, arg);
        }
        None
    }

    /// D-MEM1 S6 (D-SHARED-API1=A): `shared.read(f)` — read-locked closure
    /// callback; `f`'s param is a read-only view of the wrapped `T` (bare
    /// access, no sigil). The call's own type is whatever `f` returns.
    pub(crate) fn finish_shared_read(
        &mut self,
        inner: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        self.finish_shared_closure("read", inner, args, span, false, false)
    }

    /// D-MEM1 S6 (D-SHARED-API1=A): `shared.edit(f)` — write-locked closure
    /// callback, exclusive; `f`'s param is a mutable view of the wrapped `T`
    /// with no `&` sigil written (the lock IS the write-access grant, scoped
    /// to the closure body — no guard object ever escapes it).
    pub(crate) fn finish_shared_edit(
        &mut self,
        inner: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        self.finish_shared_closure("edit", inner, args, span, true, false)
    }

    pub(crate) fn finish_expiring_secret_with(
        &mut self,
        inner: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        let loan = crate::Sema::Diagnostics::expiring_secret_loan_type(inner.clone());
        let result = self.finish_shared_closure("with", &loan, args, span, false, true);
        let escaped = result
            .as_ref()
            .is_some_and(crate::Sema::Diagnostics::contains_expiring_secret_loan)
            || matches!(&result, Some(Type::Fn { .. }));
        if escaped {
            self.diags.push(Diagnostic::error(
                "E0201",
                "an ExpiringSecret loan cannot escape its callback".to_string(),
                "the callback receives a temporary read-only loan; returning a secret would create another owned credential outside the expiry boundary".to_string(),
                "return a non-secret result such as a signature, public key, status, or response".to_string(),
                Some(span),
            ));
            return Some(Type::Named("Unit".to_string()));
        }
        result
    }

    fn finish_shared_closure(
        &mut self,
        method: &str,
        inner: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
        param_mutable: bool,
        param_is_secret_loan: bool,
    ) -> Option<Type> {
        if args.len() != 1 {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!("`{}` expects 1 argument, got {}", method, args.len()),
                format!("`.{}` takes exactly one closure", method),
                format!("call `.{}(value => ...)` with a closure", method),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        if !matches!(args[0].expr, Expr::Lambda(_)) {
            self.diags.push(Diagnostic::error(
                "E0112",
                format!("`{}` needs a lambda, not a value", method),
                format!(
                    "`.{}` runs a short function against the locked value",
                    method
                ),
                format!("write `.{}(value => …)`", method),
                Some(args[0].expr.span()),
            ));
            self.infer(&mut args[0].expr);
            return None;
        }
        let expected = Type::Fn {
            params: vec![inner.clone()],
            ret: None,
            effect_bound: None,
        };
        let saved_exp = self.expected_type.clone();
        self.expected_type = Some(expected);
        let saved_mut = self.lambda_param_mutable;
        self.lambda_param_mutable = param_mutable;
        let saved_loan = self.lambda_param_is_secret_loan;
        self.lambda_param_is_secret_loan = param_is_secret_loan;
        let saved_esc = self.lambda_escapes;
        self.lambda_escapes = false;
        let fn_ty = self.infer(&mut args[0].expr);
        self.lambda_escapes = saved_esc;
        self.lambda_param_is_secret_loan = saved_loan;
        self.lambda_param_mutable = saved_mut;
        self.expected_type = saved_exp;
        match fn_ty {
            Some(Type::Fn { ret: Some(r), .. }) => Some(*r),
            _ => Some(Type::Named("Unit".to_string())),
        }
    }

    /// D-MEM1 S6 (D-POOLID-API1=A): `pool.add(val)` — inserts, consumes `val` (a
    /// fresh literal passes with no ceremony; a named binding needs `^`, same
    /// discipline as `Sender.send` above), returns the new `Id<T>` handle.
    pub(crate) fn finish_pool_add(
        &mut self,
        recv_ty: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        let elem_ty = match recv_ty {
            Type::Apply { name, args } if name == "Pool" => {
                args.first().cloned().unwrap_or(Type::Int)
            }
            _ => Type::Int,
        };
        if args.len() != 1 {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!("`add` expects 1 argument, got {}", args.len()),
                "adding to a pool needs exactly one value".to_string(),
                "call `.add(value)` with the value to store".to_string(),
                Some(span),
            ));
        }
        let Some(arg) = args.get_mut(0) else {
            return Some(Type::Apply {
                name: "Id".to_string(),
                args: vec![elem_ty],
            });
        };
        let saved_exp = self.expected_type.clone();
        self.expected_type = Some(elem_ty.clone());
        let got = self.infer(&mut arg.expr);
        self.expected_type = saved_exp;
        if let Some(got) = got {
            let reported = self.check_type_assignable(&elem_ty, &got, arg.expr.span());
            if !reported && got != elem_ty {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!(
                        "`add` wants {} for argument 1, but this is {}",
                        elem_ty.show(),
                        got.show()
                    ),
                    "a pool only stores values of its own element type".to_string(),
                    type_fix_hint(&elem_ty, &got),
                    Some(arg.expr.span()),
                ));
            }
        }
        self.check_take_arg_ownership("add", 0, &elem_ty, arg);
        Some(Type::Apply {
            name: "Id".to_string(),
            args: vec![elem_ty],
        })
    }

    /// D-MEM1 S6 (D-POOLID-API1=A): `pool.remove(id)` — removes the slot `id`
    /// names, bumping its generation so any other copy of `id` becomes stale.
    /// `Id<T>` is plain copyable data (like `Int`); no move ceremony on the
    /// argument. Returns `T?` — mirrors `Map.remove`'s `Option<V>` convention
    /// (the stdlib's existing "remove that might miss" shape), not `List`/
    /// `Set.remove`'s unit return (those remove by position/value, always
    /// present by construction at the call site).
    pub(crate) fn finish_pool_remove(
        &mut self,
        recv_ty: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        let elem_ty = match recv_ty {
            Type::Apply { name, args } if name == "Pool" => {
                args.first().cloned().unwrap_or(Type::Int)
            }
            _ => Type::Int,
        };
        let id_ty = Type::Apply {
            name: "Id".to_string(),
            args: vec![elem_ty.clone()],
        };
        if args.len() != 1 {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!("`remove` expects 1 argument, got {}", args.len()),
                "removing from a pool needs exactly one `Id<T>`".to_string(),
                "call `.remove(id)` with the `Id<T>` to remove".to_string(),
                Some(span),
            ));
        }
        let Some(arg) = args.get_mut(0) else {
            return Some(Type::Option(Box::new(elem_ty)));
        };
        let saved_exp = self.expected_type.clone();
        self.expected_type = Some(id_ty.clone());
        let got = self.infer(&mut arg.expr);
        self.expected_type = saved_exp;
        if let Some(got) = got {
            let reported = self.check_type_assignable(&id_ty, &got, arg.expr.span());
            if !reported && got != id_ty {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!(
                        "`remove` wants {} for argument 1, but this is {}",
                        id_ty.show(),
                        got.show()
                    ),
                    "a pool is only removed from by the `Id<T>` its own `.add()` returned"
                        .to_string(),
                    type_fix_hint(&id_ty, &got),
                    Some(arg.expr.span()),
                ));
            }
        }
        Some(Type::Option(Box::new(elem_ty)))
    }
}

fn tracks_named_move(name: &str) -> bool {
    name != "_"
}

#[cfg(test)]
mod discard_tests {
    use super::tracks_named_move;

    #[test]
    fn discard_binding_never_enters_move_tracking() {
        assert!(!tracks_named_move("_"));
        assert!(tracks_named_move("value"));
    }
}

/// D-LIN1 (ratified 2026-06-21): E0140 — a `#SingleUse` value reached the end of
/// its scope without being consumed. The fix names the three legal exits.
pub(crate) fn e0140_unconsumed(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0140",
        format!("`{}` must be used exactly once, but it is never used", name),
        "this value's type is `#SingleUse`, so it carries a job that has to be done — dropping it without doing that job leaves the work undone (an unjoined task, an unreleased lock)".to_string(),
        format!(
            "give it away exactly once: move it to a `{}` parameter, or `return` it",
            Syntax::SIGIL_MOVE
        ),
        Some(span),
    )
}

/// D-LIN1 (ratified 2026-06-21): E0141 — a `#SingleUse` value is consumed on one
/// branch of an `if` but not the other, so some paths leave it unused.
pub(crate) fn e0141_unconsumed_branch(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0141",
        format!("`{}` must be used exactly once, but one path through this `if` leaves it unused", name),
        "a `#SingleUse` value has to be used on every path — here it is consumed on one branch but not the other, so the program could reach the end of its scope without doing its job".to_string(),
        format!(
            "use `{}` exactly once on every branch: consume it in the missing arm, or move it out before the `if`",
            name
        ),
        Some(span),
    )
}

/// D-DROP-WORD1: E0143 — `consume(x)` deliberately discards a
/// `#SingleUse` value, but the discard wasn't audited. Throwing away a value
/// whose whole point is "this job must be done" needs a written justification,
/// so `consume` of a `#SingleUse` value is legal only inside an `#Unsafe("reason")`
/// region/fn — the reason IS the audit note (reuses D-UNSAFE2's audited gate).
pub(crate) fn e0143_drop_unaudited(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0143",
        format!("`{}` is `#SingleUse` — discarding it with `consume` needs an audited reason", name),
        "this value's type is `#SingleUse`, so it carries a job that has to be done; deliberately throwing it away skips that job, which is exactly the kind of decision that has to be written down".to_string(),
        format!(
            "wrap it in an audited region: `#{}(\"why discarding this is fine\") {{ consume({}) }}`",
            Syntax::KW_UNSAFE,
            name
        ),
        Some(span),
    )
}

/// D-LIN1 (ratified 2026-06-21): E0142 — a `#SingleUse` value was passed somewhere
/// that would borrow or copy it. Such values may only be moved/consumed.
pub(crate) fn e0142_aliased(name: &str, call: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0142",
        format!("`{}` can't be shared — it must be used exactly once", name),
        format!(
            "this value's type is `#SingleUse`, so it can only be moved (handed over for good); `{}` would borrow or copy it, and a `#SingleUse` value has no second use to give",
            call
        ),
        format!(
            "move it with `{}{}` to give it away, or rework the call so it takes ownership (`{}`)",
            Syntax::SIGIL_MOVE,
            name,
            Syntax::SIGIL_MOVE
        ),
        Some(span),
    )
}
