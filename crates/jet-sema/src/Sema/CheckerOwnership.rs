use super::*;
use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Sema::Diagnostics::{is_cloneable, type_requires_owned_iteration};
use crate::Generics::{is_type_var_name, substitute_type};
use crate::Collections;
use crate::Syntax;
use crate::AST::{
    AccessConvention, BinOp, Expr, ForKind, Lambda, LambdaBody, LValue, Pattern, Stmt, Type, UnOp,
    VariantPayload,
};
use std::collections::{HashMap, HashSet};

/// D-FACT-OWN1: the borrow checker is a prover, not a plane. The view plane
/// stores the windows; deciding when a window dies stays here, with the prover
/// that opened it.
pub(crate) fn invalidate_view_owner(
    views: &mut ViewStore,
    owner: &ViewOwnerId,
    verb: &str,
    span: Span,
) {
    for facts in views.all_mut() {
        for fact in facts {
            if &fact.place.owner == owner && fact.invalidated.is_none() {
                fact.invalidated = Some((verb.to_string(), span));
            }
        }
    }
}

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

fn carries_lending_view(expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::Ident(found, _) => found == name,
        Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Paren(inner, _)
        | Expr::Spread(inner, _) => carries_lending_view(inner, name),
        Expr::ListLit(items, _) => items.iter().any(|item| carries_lending_view(item, name)),
        Expr::TupleLit(fields, _, _) => fields
            .iter()
            .any(|(_, value)| carries_lending_view(value, name)),
        Expr::MapLit(entries, _) => entries.iter().any(|(key, value)| {
            carries_lending_view(key, name) || carries_lending_view(value, name)
        }),
        Expr::StructLit { fields, .. } => fields
            .iter()
            .any(|(_, _, value)| carries_lending_view(value, name)),
        Expr::TypedLit { .. }
        | Expr::If { .. }
        | Expr::OrFallback { .. } => crate::Sema::Captures::expr_refs_name(expr, name),
        Expr::EnumLit { args, .. } => args.iter().any(|arg| match arg {
            crate::AST::EnumLitArg::Positional(value) => carries_lending_view(value, name),
            crate::AST::EnumLitArg::Named { expr, .. } => carries_lending_view(expr, name),
        }),
        Expr::Ok(inner, _) | Expr::Err(inner, _) | Expr::Present(inner, _) => {
            carries_lending_view(inner, name)
        }
        _ => false,
    }
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

fn cell_guard_projection_path(expr: &Expr, parameter: &str) -> Option<Vec<String>> {
    match expr {
        Expr::Ident(name, _) if name == parameter => Some(Vec::new()),
        Expr::Field(base, field, _) => {
            let mut path = cell_guard_projection_path(base, parameter)?;
            path.push(field.clone());
            Some(path)
        }
        Expr::Paren(base, _) => cell_guard_projection_path(base, parameter),
        _ => None,
    }
}

fn cell_guard_projection_path_from_arg(arg: &crate::AST::CallArg) -> Option<Vec<String>> {
    let Expr::Lambda(lambda) = &arg.expr else {
        return None;
    };
    let [parameter] = lambda.params.as_slice() else {
        return None;
    };
    let crate::AST::LambdaBody::Expr(expr) = &lambda.body else {
        return None;
    };
    cell_guard_projection_path(expr, &parameter.name)
}

fn record_cell_guard_projection_path(
    arg: &mut crate::AST::CallArg,
    path: Option<Vec<String>>,
) {
    if let Expr::Lambda(lambda) = &mut arg.expr {
        lambda.meta.cell_projection_path = path;
    }
}

fn cell_guard_projection_paths_overlap(first: &[String], second: &[String]) -> bool {
    first.starts_with(second) || second.starts_with(first)
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
        invalidate_view_owner(&mut self.flow.views, &owner, verb, span);
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
            // D-PIN1=A: the no-move promise ends with the pin. Letting the pin
            // outlive the pinned place would keep the promise alive after the
            // storage it describes is gone.
            ViewKind::Pin => self.diags.push(Diagnostic::error(
                "E2305",
                format!(
                    "`{}` cannot be shared — the pin does not live long enough to {}",
                    name, what
                ),
                format!(
                    "`{}` pins `{}`; sharing it outside `{}`'s scope would let the no-move promise outlive the storage it protects",
                    name, fact.place.owner.name, fact.place.owner.name
                ),
                format!(
                    "keep `{}` inside `{}`'s scope, or pin the place again where it is needed",
                    name, fact.place.owner.name
                ),
                Some(span),
            )),
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
        self.record_view_with_exemptions(
            name,
            output_path,
            place,
            kind,
            access,
            span,
            None,
            None,
        );
    }

    fn record_view_with_exemptions(
        &mut self,
        name: &str,
        output_path: Vec<String>,
        place: ViewPlace,
        kind: ViewKind,
        access: ViewAccess,
        span: Span,
        transfer_from: Option<&str>,
        enum_variants: Option<&HashSet<String>>,
    ) {
        let conflicts = self
            .flow
            .views
            .all()
            .flat_map(|(existing_name, facts)| facts.iter().map(move |fact| (existing_name, fact)))
            .any(|(existing_name, fact)| {
            if !self.view_is_live_now(existing_name) || fact.invalidated.is_some() {
                return false;
            }
            if transfer_from == Some(existing_name) {
                return false;
            }
            // Several candidates for one logical output slot are alternatives,
            // not simultaneous borrows. Enum variants are likewise mutually
            // exclusive even though their payload paths differ.
            if existing_name == name
                && (fact.output_path == output_path
                    || enum_variants.is_some_and(|variants| {
                        Self::view_paths_are_enum_alternatives(
                            variants,
                            &fact.output_path,
                            &output_path,
                        )
                    }))
            {
                return false;
            }
            // D-PIN2=A / D-PIN3=A: reaching a declared `Pin<U>` field through a
            // live pin is structural projection, not a second borrow — parent
            // and child are one contract. Only nesting is exempt; two pins on
            // unrelated-but-overlapping places still conflict.
            if kind == ViewKind::Pin
                && fact.kind == ViewKind::Pin
                && (place.extends(&fact.place) || fact.place.extends(&place))
            {
                return false;
            }
            // D-PIN1=A + D-ALLOC2 / D-FIXED-BACKING1: `mem.pin(&alloc)` names the
            // same exclusive write window the arena/Fixed alloc already opened.
            // The pin binding adds the address-stability promise; it is not a
            // second overlapping write into the allocator's storage.
            if kind == ViewKind::Pin
                && fact.kind == ViewKind::Arena
                && fact.place.overlaps(&place)
            {
                return false;
            }
            (fact.access == ViewAccess::Write || access == ViewAccess::Write)
                && fact.place.overlaps(&place)
        });
        if conflicts {
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
            scope_len: self.scope_depth(),
            seq: 0,
            invalidated: None,
        };
        crate::Sema::push_view_fact(&mut self.flow.views, name, fact);
    }

    fn view_paths_are_enum_alternatives(
        variants: &HashSet<String>,
        left: &[String],
        right: &[String],
    ) -> bool {
        let (Some(left_variant), Some(right_variant)) = (left.first(), right.first()) else {
            return false;
        };
        if left_variant == right_variant {
            return false;
        }
        variants.contains(left_variant) && variants.contains(right_variant)
    }

    pub(crate) fn view_fact(&self, name: &str) -> Option<&ViewFact> {
        let binding = self.lookup(name)?;
        crate::Sema::current_view_fact_for_binding(&self.flow.views, name, binding.def_span)
    }

    fn view_facts(&self, name: &str) -> Vec<&ViewFact> {
        let Some(binding) = self.lookup(name) else {
            return Vec::new();
        };
        crate::Sema::view_facts_for_binding(&self.flow.views, name, binding.def_span)
    }

    fn view_fact_at_path(&self, name: &str, output_path: &[String]) -> Option<&ViewFact> {
        self.view_facts_at_path(name, output_path).into_iter().next()
    }

    fn view_facts_at_path(&self, name: &str, output_path: &[String]) -> Vec<&ViewFact> {
        let candidates: Vec<_> = self
            .view_facts(name)
            .into_iter()
            .filter(|fact| output_path.starts_with(&fact.output_path))
            .collect();
        let Some(longest) = candidates.iter().map(|fact| fact.output_path.len()).max() else {
            return Vec::new();
        };
        candidates
            .into_iter()
            .filter(|fact| fact.output_path.len() == longest)
            .collect()
    }

    fn compose_view_source_places(
        &self,
        actual: &Expr,
        projections: &[crate::AST::ViewSourceProjection],
        span: Span,
    ) -> Vec<ViewPlace> {
        let actual = actual.without_parens();
        let leading_fields: Vec<String> = projections
            .iter()
            .map_while(|projection| match projection {
                crate::AST::ViewSourceProjection::Field(name) => Some(name.clone()),
                _ => None,
            })
            .collect();
        if let Expr::Ident(name, _) = actual {
            let facts = self.view_facts_at_path(name, &leading_fields);
            if !facts.is_empty() {
                return facts
                    .into_iter()
                    .map(|fact| {
                        let mut place = fact.place.clone();
                        append_view_source_projections(
                            &mut place,
                            &projections[fact.output_path.len()..],
                            span,
                        );
                        place
                    })
                    .collect();
            }
        }
        let Some(mut place) = self.place_from_expr(actual) else {
            return Vec::new();
        };
        append_view_source_projections(&mut place, projections, span);
        vec![place]
    }

    fn view_is_live_now(&self, name: &str) -> bool {
        if self.view_kind(name) == Some(ViewKind::FixedBacking) {
            // A Fixed handle still owns cleanup work even after its last source
            // read. Its exclusive backing borrow ends only on consuming close
            // (or lexical scope exit), not ordinary last-use shortening.
            return !self.flow.moved.contains(name);
        }
        self.views_used_in_stmt.contains(name) || self.is_name_live_after(name)
    }

    pub(crate) fn view_kind(&self, name: &str) -> Option<ViewKind> {
        self.view_fact(name).map(|fact| fact.kind)
    }

    fn view_kind_for_place(&self, place: &ViewPlace) -> ViewKind {
        if let Some((_, fact)) = crate::Sema::view_facts_newest_first(&self.flow.views)
            .into_iter()
            .find(|(_, fact)| {
                fact.place.owner.def_span == place.owner.def_span
                    && fact.place.projections == place.projections
            })
        {
            return fact.kind;
        }
        // D-PIN2=A / D-PIN3=A: a place reached through a live pin is still
        // pinned. Only `Pin` inherits this way — every other window kind keeps
        // its own owner-shaped classification below.
        if crate::Sema::view_facts_newest_first(&self.flow.views)
            .into_iter()
            .any(|(_, fact)| fact.kind == ViewKind::Pin && place.extends(&fact.place))
        {
            return ViewKind::Pin;
        }
        match self.lookup(&place.owner.name).map(|info| &info.ty) {
            Some(Type::Named(name)) if name == Syntax::TYPE_BYTES => ViewKind::Buffer,
            Some(Type::Apply { name, .. })
                if matches!(name.as_str(), "Vec" | "Matrix" | "Tensor") =>
            {
                ViewKind::Matrix
            }
            Some(Type::Named(name)) if name == "Tensor" => ViewKind::Matrix,
            _ => ViewKind::List,
        }
    }

    pub(crate) fn place_name(place: &ViewPlace) -> String {
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
            .find(|(candidate, _, _)| candidate == field)
            .map(|(_, _, ty)| substitute_type(ty, &subst))
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
        let moved = lambda
            .meta
            .moved_captures
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
            } else if (moved.contains(name)
                || (!lambda.meta.needs_fn_mut || lambda.meta.escapes))
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
                Expr::Slice {
                    start, end, range, ..
                } => {
                    if let Some(range) = range {
                        self.collect_evaluated_expr_accesses(range, mode, bound, out);
                    } else {
                        self.collect_evaluated_expr_accesses(start, mode, bound, out);
                        self.collect_evaluated_expr_accesses(end, mode, bound, out);
                    }
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
            Expr::CallValue { callee, args, .. }
                if matches!(
                    callee.as_ref(),
                    Expr::Lambda(lambda)
                        if lambda.meta.collecting_loop || lambda.meta.result_loop
                ) =>
            {
                let Expr::Lambda(lambda) = callee.as_ref() else {
                    unreachable!("collecting loop guard requires a lambda")
                };
                match &lambda.body {
                    LambdaBody::Expr(body) => {
                        self.collect_evaluated_expr_accesses(body, mode, bound, out);
                    }
                    LambdaBody::Block(body) => {
                        let mut body_bound = bound.clone();
                        self.collect_evaluated_stmt_accesses(
                            body,
                            mode,
                            &mut body_bound,
                            out,
                        );
                    }
                }
                for arg in args {
                    self.collect_evaluated_expr_accesses(&arg.expr, mode, bound, out);
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
            | Expr::Paren(inner, _) => {
                self.collect_evaluated_expr_accesses(inner, mode, bound, out);
            }
            Expr::Try(inner, _, _, note) => {
                self.collect_evaluated_expr_accesses(inner, mode, bound, out);
                if let Some(note) = note {
                    self.collect_evaluated_expr_accesses(note, mode, bound, out);
                }
            }
            Expr::Field(base, _, _) | Expr::OptField { base, .. } => {
                self.collect_evaluated_expr_accesses(base, mode, bound, out);
            }
            Expr::MemberSpread { base, .. } => {
                self.collect_evaluated_expr_accesses(base, mode, bound, out);
            }
            Expr::Index { base, index, .. } => {
                self.collect_evaluated_expr_accesses(base, mode, bound, out);
                self.collect_evaluated_expr_accesses(index, mode, bound, out);
            }
            Expr::Slice {
                base,
                start,
                end,
                range,
                ..
            } => {
                self.collect_evaluated_expr_accesses(base, mode, bound, out);
                if let Some(range) = range {
                    self.collect_evaluated_expr_accesses(range, mode, bound, out);
                } else {
                    self.collect_evaluated_expr_accesses(start, mode, bound, out);
                    self.collect_evaluated_expr_accesses(end, mode, bound, out);
                }
            }
            Expr::Range { start, end, .. } => {
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
        | Expr::NoElse(_)
            | Expr::ReduceMarker(..)
            | Expr::ComptimeName { .. }
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

    fn collect_evaluated_stmt_accesses(
        &self,
        body: &[Stmt],
        mode: AccessWalkMode,
        bound: &mut HashSet<String>,
        out: &mut Vec<EvaluatedAccess>,
    ) {
        for stmt in body {
            match stmt {
                Stmt::Expr(expr)
                | Stmt::Yield(expr, _)
                | Stmt::DeferClose { close: expr, .. } => {
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
                Stmt::BreakValue(expr, _) | Stmt::BreakLabelValue(_, _, expr, _) => {
                    self.collect_evaluated_expr_accesses(expr, mode, bound, out);
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
                // D-CANVASSTATE1=D: an `#Off` body never reaches runtime.
                Stmt::Switched { marker, .. } if crate::AST::switched_off(marker) => {}
                Stmt::Loop { body, .. }
                | Stmt::Unsafe { body, .. }
                | Stmt::Impure { body, .. }
                | Stmt::Shield { body, .. }
                | Stmt::Switched { body, .. }
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
                Stmt::ComptimeIf { .. }
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
                base,
                start,
                end,
                range,
                ..
            } => {
                self.collect_call_projection_accesses(base, accesses);
                if let Some(range) = range {
                    self.collect_evaluated_expr_accesses(
                        range,
                        AccessWalkMode::EvaluateNow,
                        &HashSet::new(),
                        accesses,
                    );
                } else {
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
        self.report_lending_view_escape(expr, "be passed to a call");
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

    pub(crate) fn report_lending_view_escape(&mut self, expr: &Expr, action: &str) {
        let lent = self
            .lending_view_loop_vars
            .iter()
            .find(|name| carries_lending_view(expr, name))
            .cloned();
        let Some(name) = lent else {
            return;
        };
        self.diags.push(Diagnostic::error(
            "E0212",
            format!("the lent mutable view `{name}` cannot {action}"),
            "the view is valid only while the current iteration or callback invocation is active"
                .to_string(),
            "use the view inside that scope, and keep only owned values outside it".to_string(),
            Some(expr.span()),
        ));
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
            Expr::Slice {
                base,
                start,
                end,
                range,
                span,
            } => {
                let mut place = self.place_from_expr(base)?;
                let (start, end) = match range.as_deref() {
                    Some(Expr::Range {
                        start,
                        end,
                        exclusive,
                        ..
                    }) => {
                        let start = const_place_int(start);
                        let mut end = const_place_int(end);
                        if *exclusive {
                            end = end.and_then(|value| value.checked_sub(1));
                        }
                        (start, end)
                    }
                    Some(_) => (None, None),
                    None => (const_place_int(start), const_place_int(end)),
                };
                place.projections.push(ViewProjection::Range {
                    start,
                    end,
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

    /// True when this frame owns `place` outright, so a loop that hands out the
    /// elements themselves may take the whole collection.
    ///
    /// A read or write parameter, anything reached through one, and any live
    /// view all name storage the caller still owns. Moving out of those is
    /// `E0507` in the generated Rust, which reaches the user as an internal
    /// compiler error rather than a diagnostic (I2). An expression with no root
    /// name is a temporary, which the loop is always free to consume.
    pub(crate) fn frame_owns_place(&self, place: &Expr) -> bool {
        let Some(root) = expr_root_ident(place) else {
            return true;
        };
        if self.is_view(root) {
            return false;
        }
        !self.lookup(root).is_some_and(|info| {
            matches!(
                info.param_conv,
                Some(AccessConvention::Read) | Some(AccessConvention::Write)
            )
        })
    }

    /// True when a by-value loop may take `place` as the collection itself.
    ///
    /// Only a bare owned local or an rvalue is consumable. A field, index, or
    /// slice still names storage inside another value — moving elements out of
    /// that storage is `E0507`/`E0508` in the generated Rust (I2), even when the
    /// root local is owned. Infer may wrap an owning field read in `Copy` so
    /// rustc never sees a partial move; peel that wrap so the loop still sees
    /// the projection and rejects it instead of cloning a task handle.
    pub(crate) fn frame_can_consume_collection(&self, place: &Expr) -> bool {
        match place {
            Expr::Ident(..) => self.frame_owns_place(place),
            Expr::Paren(inner, _) | Expr::Copy(inner, _) => {
                self.frame_can_consume_collection(inner)
            }
            _ if expr_root_ident(place).is_none() => true,
            _ => false,
        }
    }

    pub(crate) fn is_write_view(&self, name: &str) -> bool {
        self.view_fact(name)
            .is_some_and(|fact| fact.access == ViewAccess::Write)
    }

    pub(crate) fn is_edit_shared_guard(&self, name: &str) -> bool {
        self.lookup(name).is_some_and(|info| {
            matches!(
                &info.ty,
                Type::Tagged { marker, inner }
                    if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::SharedGuardEdit))
                        && matches!(
                            inner.as_ref(),
                            Type::Apply { name, .. }
                                if name == crate::Syntax::TYPE_SHARED_GUARD
                        )
            )
        })
    }

    pub(crate) fn is_read_shared_guard(&self, name: &str) -> bool {
        self.lookup(name).is_some_and(|info| {
            matches!(
                &info.ty,
                Type::Apply { name, .. }
                    if name == crate::Syntax::TYPE_SHARED_GUARD
            ) || matches!(
                &info.ty,
                Type::Tagged { marker, inner }
                    if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::SharedGuardRead))
                        && matches!(
                            inner.as_ref(),
                            Type::Apply { name, .. }
                                if name == crate::Syntax::TYPE_SHARED_GUARD
                        )
            )
        })
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
        let Some(info) = self.lookup(&root).cloned() else {
            return;
        };
        if self.reject_read_shared_guard_write(expr, span) {
            return;
        }
        if info.mutable
            || info.param_conv == Some(AccessConvention::Write)
            || self.is_edit_shared_guard(&root)
        {
            return;
        }
        let (code, why, fix) = if info.param_conv.is_some() {
            (
                "E0205",
                "an unmarked parameter gives read access only; a write window needs the write-capability marker `&`".to_string(),
                format!(
                    "change the parameter to `{}: {}{}` with the write-capability marker `&`",
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
        let Some(root) = expr_root_ident(receiver).map(str::to_string) else {
            self.check_expr_change(receiver, &format!("be changed by `.{method}()`"), span);
            return;
        };
        if self.is_edit_shared_guard(&root) {
            return;
        }
        if self.is_read_shared_guard(&root) {
            self.diags.push(Diagnostic::error(
                "E0205",
                format!(
                    "cannot edit through `{root}` — this `SharedGuard` has read access only"
                ),
                "a guard from `guard_read()` may inspect the shared value but cannot change it"
                    .to_string(),
                "use `guard_edit()` when this scope must change the shared value".to_string(),
                Some(span),
            ));
            return;
        }
        self.check_expr_change(receiver, &format!("be changed by `.{method}()`"), span);
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
                            "declare the enclosing method with the write-capability marker `&`: `{}{}`",
                            Syntax::SIGIL_WRITE,
                            Syntax::KW_SELF
                        ),
                    )
                } else {
                    (
                        format!(
                            "cannot write to `{}` — it does not have the write-capability marker `&`; required before calling `.{}()`",
                            root, method
                        ),
                        format!("declare `{} {} ...`", root, Syntax::SIGIL_BIND_MUT),
                    )
                };
                self.diags.push(Diagnostic::error(
                    "E0202",
                    what,
                    "this method edits the value it's called on; the write-capability marker `&` is required"
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

    pub(crate) fn validate_shared_guard_lvalue(&mut self, target: &LValue) {
        match target {
            LValue::Local { .. } => {}
            LValue::Index { base, span, .. } => {
                self.reject_read_shared_guard_write(base, *span);
            }
            LValue::Field { base, field, span } => {
                let target = Expr::Field(base.clone(), field.clone(), *span);
                self.reject_read_shared_guard_write(&target, *span);
            }
        }
    }

    /// Reject moves, replacement, and storage-changing methods while any view
    /// into the owner remains live. Exclusive-window reads of the owner are
    /// rejected by `check_place_read` (E0220); facts vanish on scope exit.
    pub(crate) fn check_owner_change(&mut self, owner: &str, action: &str, span: Span) {
        let owner_place = ViewPlace {
            owner: self.owner_id(owner),
            projections: Vec::new(),
        };
        self.check_place_change(&owner_place, action, span);
    }

    /// Card #1361 / I2: reject a read of a place while an exclusive window into
    /// it is live. Reading *through* that window (the pin / ViewMut binding) is
    /// the legal path; reading the owner beside it reaches rustc E0503.
    pub(crate) fn check_place_read(&mut self, expr: &Expr, span: Span) {
        let Some(place) = self.place_from_expr(expr) else {
            return;
        };
        if let Some(root) = crate::Sema::Diagnostics::expr_root_ident(expr) {
            if self.is_write_view(root) {
                return;
            }
        }
        let Some((view, place_name, kind)) = crate::Sema::view_facts_newest_first(&self.flow.views)
            .into_iter()
            .find(|(name, fact)| {
                self.view_is_live_now(name)
                    && fact.access == ViewAccess::Write
                    && fact.place.overlaps(&place)
                    && fact.invalidated.is_none()
            })
            .map(|(name, fact)| (name.to_string(), Self::place_name(&fact.place), fact.kind))
        else {
            return;
        };
        let read_name = Self::place_name(&place);
        let window = if kind == ViewKind::Pin {
            "pin"
        } else {
            "exclusive write window"
        };
        self.diags.push(Diagnostic::error(
            "E0220",
            format!("`{read_name}` cannot be read while `{view}` has a live {window} into it"),
            format!(
                "`{view}` is an exclusive window into `{place_name}`; reading the owner beside that window would be rejected after lowering"
            ),
            format!("read or edit through `{view}` instead of `{read_name}`"),
            Some(span),
        ));
    }

    fn check_place_change(&mut self, changed: &ViewPlace, action: &str, span: Span) {
        // A task group loan outlives the borrow binding's last lexical use: the
        // child still holds it until the group joins. Check that first — the
        // view scan below would already have let this place go.
        if self.report_scoped_loan_conflict(changed, action, span) {
            return;
        }
        let Some((view, access, place, kind)) = crate::Sema::view_facts_newest_first(&self.flow.views)
            .into_iter()
            .find(|(name, fact)| {
                self.view_is_live_now(name)
                    && fact.place.overlaps(changed)
                    && fact.invalidated.is_none()
            })
            .map(|(name, fact)| {
                (
                    name.to_string(),
                    fact.access,
                    Self::place_name(&fact.place),
                    fact.kind,
                )
            })
        else {
            return;
        };
        // D-PIN1=A: a pin is not an aliasing complaint — it is a promise the
        // pinned storage keeps its address. Say that instead of E0212's view
        // wording so the fix names the pin, not "make an owned copy".
        if kind == ViewKind::Pin {
            let changed_name = Self::place_name(changed);
            self.diags.push(Diagnostic::error(
                "E0219",
                format!("`{changed_name}` cannot {action} while it is pinned"),
                format!(
                    "`{view}` pinned `{place}`, which promises that storage keeps its address; moving or replacing it would leave every stored address pointing at the old place"
                ),
                format!(
                    "finish using `{view}` before changing `{changed_name}`, or narrow the pin's scope"
                ),
                Some(span),
            ));
            return;
        }
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
        let init = init.without_parens();
        if let Expr::Copy(inner, _) | Expr::Try(inner, _, _, _) = init {
            return self.view_call_sources(inner);
        }
        if let Expr::Ident(name, _) = init {
            return self
                .view_facts(name)
                .into_iter()
                .map(|fact| {
                    (
                        fact.output_path.clone(),
                        fact.place.clone(),
                        fact.kind,
                        fact.access,
                    )
                })
                .collect();
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
        if let Expr::ListLit(elements, _) = init {
            let mut sources = Vec::new();
            for element in elements {
                for (mut path, place, kind, access) in self.view_call_sources(element) {
                    path.insert(0, "[]".to_string());
                    sources.push((path, place, kind, access));
                }
            }
            return sources;
        }
        if let Expr::TupleLit(fields, ..) = init {
            let mut sources = Vec::new();
            for (field, value) in fields {
                for (mut path, place, kind, access) in self.view_call_sources(value) {
                    path.insert(0, field.clone());
                    sources.push((path, place, kind, access));
                }
            }
            return sources;
        }
        if let Expr::StructLit { fields, .. } = init {
            let mut sources = Vec::new();
            for (field, _, value) in fields {
                for (mut path, place, kind, access) in self.view_call_sources(value) {
                    path.insert(0, field.clone());
                    sources.push((path, place, kind, access));
                }
            }
            return sources;
        }
        if let Expr::EnumLit { variant, args, .. } = init {
            let mut sources = Vec::new();
            for (index, arg) in args.iter().enumerate() {
                let (slot, value) = match arg {
                    crate::AST::EnumLitArg::Positional(value) => (index.to_string(), value),
                    crate::AST::EnumLitArg::Named { label, expr } => (label.clone(), expr),
                };
                for (mut path, place, kind, access) in self.view_call_sources(value) {
                    path.insert(0, slot.clone());
                    path.insert(0, variant.clone());
                    sources.push((path, place, kind, access));
                }
            }
            return sources;
        }
        if let Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Tainted(inner, _, _) = init
        {
            return self.view_call_sources(inner);
        }
        if let Expr::Index { base, .. } = init {
            let projected: Vec<_> = self
                .view_call_sources(base)
                .into_iter()
                .filter_map(|(mut path, place, kind, access)| {
                    (path.first().map(String::as_str) == Some("[]")).then(|| {
                        path.remove(0);
                        (path, place, kind, access)
                    })
                })
                .collect();
            if !projected.is_empty() {
                return projected;
            }
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
            if let Some(Type::Fn {
                params,
                ret,
                return_view_provenance,
                ..
            }) = self.lookup(&call.name).map(|info| info.ty.clone())
            {
                if !ret
                    .as_deref()
                    .is_some_and(|ty| self.type_contains_view_boundary(ty))
                {
                    return Vec::new();
                }
                let map = return_view_provenance.unwrap_or_else(|| {
                    ret.as_deref()
                        .map(|ret| self.conservative_callback_view_provenance(&params, ret))
                        .unwrap_or_default()
                });
                let mut sources = Vec::new();
                for (output_path, provenance) in map {
                    for source in provenance.sources {
                        let crate::AST::ViewSource::Parameter(index) = source.source else {
                            continue;
                        };
                        let Some(actual) = call.args.get(index).map(|arg| &arg.expr) else {
                            continue;
                        };
                        for place in self.compose_view_source_places(
                            actual,
                            &source.projections,
                            init.span(),
                        ) {
                            let kind = self.view_kind_for_place(&place);
                            let access = if provenance.mutable {
                                ViewAccess::Write
                            } else {
                                ViewAccess::Read
                            };
                            sources.push((output_path.clone(), place, kind, access));
                        }
                    }
                }
                return sources;
            }
            let Some(sig) = self.funcs.get(&call.name) else {
                return Vec::new();
            };
            let Some(map) = sig.return_view_provenance.get() else {
                return Vec::new();
            };
            let string_view = sig.return_type.as_ref().is_some_and(|ty| {
                matches!(
                    ty,
                    Type::Apply { name, args }
                        if name == "View"
                            && matches!(args.as_slice(), [Type::Named(inner)] if inner == "str")
                ) || matches!(ty, Type::Named(name) if self.registry.struct_fields(name).is_some_and(|fields| {
                    fields.iter().any(|(_, _, field_ty)| matches!(
                        field_ty,
                        Type::Apply { name, args }
                            if name == "View"
                                && matches!(args.as_slice(), [Type::Named(inner)] if inner == "str")
                    ))
                }))
            });
            let mut sources = Vec::new();
            for (output_path, provenance) in map {
                for source in provenance.sources {
                    let crate::AST::ViewSource::Parameter(index) = source.source else {
                        continue;
                    };
                    let Some(actual) = call.args.get(index).map(|arg| &arg.expr) else {
                        continue;
                    };
                    let places = self.compose_view_source_places(
                        actual,
                        &source.projections,
                        init.span(),
                    );
                    if places.is_empty() {
                        self.report_temporary_view_source(actual.span(), string_view);
                        continue;
                    }
                    let access = if provenance.mutable {
                        ViewAccess::Write
                    } else {
                        ViewAccess::Read
                    };
                    for place in places {
                        let kind = self.view_kind_for_place(&place);
                        sources.push((output_path.clone(), place, kind, access));
                    }
                }
            }
            return sources;
        }
        if let Expr::CallValue { callee, args, .. } = init {
            let Some(Type::Fn {
                params,
                ret,
                return_view_provenance,
                ..
            }) = (match callee.as_ref() {
                Expr::Ident(name, _) => self.lookup(name).map(|info| info.ty.clone()),
                _ => None,
            }) else {
                return Vec::new();
            };
            if !ret
                .as_deref()
                .is_some_and(|ty| self.type_contains_view_boundary(ty))
            {
                return Vec::new();
            }
            let map = return_view_provenance.unwrap_or_else(|| {
                ret.as_deref()
                    .map(|ret| self.conservative_callback_view_provenance(&params, ret))
                    .unwrap_or_default()
            });
            let mut sources = Vec::new();
            for (output_path, provenance) in map {
                for source in provenance.sources {
                    let crate::AST::ViewSource::Parameter(index) = source.source else {
                        continue;
                    };
                    let Some(actual) = args.get(index).map(|arg| &arg.expr) else {
                        continue;
                    };
                    for place in self.compose_view_source_places(
                        actual,
                        &source.projections,
                        init.span(),
                    ) {
                        let kind = self.view_kind_for_place(&place);
                        let access = if provenance.mutable {
                            ViewAccess::Write
                        } else {
                            ViewAccess::Read
                        };
                        sources.push((output_path.clone(), place, kind, access));
                    }
                }
            }
            return sources;
        }
        if let Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } = init
        {
            if let Expr::Ident(enum_name, _) = receiver.as_ref().without_parens() {
                if let Some(payload) = self
                    .resolve_enum_variants_cloned(enum_name)
                    .and_then(|variants| variants.get(method).cloned())
                    .map(|(_, payload)| payload)
                {
                    let mut sources = Vec::new();
                    for (index, arg) in args.iter().enumerate() {
                        let slot = match &payload {
                            VariantPayload::Unit => continue,
                            VariantPayload::Single(_, _) => index.to_string(),
                            VariantPayload::Named(fields) => arg
                                .label
                                .as_ref()
                                .map(|(label, _)| label.clone())
                                .or_else(|| fields.get(index).map(|field| field.name.clone()))
                                .unwrap_or_else(|| index.to_string()),
                        };
                        for (mut path, place, kind, access) in
                            self.view_call_sources(&arg.expr)
                        {
                            path.insert(0, slot.clone());
                            path.insert(0, method.clone());
                            sources.push((path, place, kind, access));
                        }
                    }
                    return sources;
                }
            }
        }
        // D-PIN1=A: `mem.pin(&place)` opens the address-stability window on
        // `place`. The recorded fact IS the contract: `check_place_change`
        // rejects every move, replacement, and resize of that place while the
        // pin is live, and the fact disappears with the pin binding's scope.
        if let Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } = init
        {
            if method == Syntax::MEM_PIN
                && matches!(receiver.as_ref().without_parens(), Expr::Ident(alias, _)
                    if self.core_imports.get(alias).is_some_and(|m| m == Syntax::CORE_MEM_MODULE))
            {
                return args
                    .first()
                    .and_then(|arg| self.place_from_expr(&arg.expr))
                    .map(|place| vec![(Vec::new(), place, ViewKind::Pin, ViewAccess::Write)])
                    .unwrap_or_default();
            }
        }
        if let Expr::MethodCall {
            receiver,
            method,
            args,
            method_span,
            ..
        } = init
        {
            if method == "split_write" || method == "get_disjoint_write" {
                let Some(place) = self.place_from_expr(receiver) else {
                    return Vec::new();
                };
                let kind = self.view_kind_for_place(&place);
                let source = |output_path: Vec<String>, proof_span: Span| {
                    let mut proved = place.clone();
                    proved.projections.push(ViewProjection::Fresh(proof_span));
                    (output_path, proved, kind, ViewAccess::Write)
                };
                if method == "split_write" {
                    let right_span = args
                        .first()
                        .map(|arg| arg.expr.span())
                        .unwrap_or_else(|| init.span());
                    return vec![
                        source(vec!["left".to_string()], *method_span),
                        source(vec!["right".to_string()], right_span),
                    ];
                }
                let proof_spans = args
                    .first()
                    .and_then(|arg| match &arg.expr {
                        Expr::ListLit(elements, _) => {
                            Some(elements.iter().map(Expr::span).collect::<Vec<_>>())
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| vec![init.span()]);
                return proof_spans
                    .into_iter()
                    .map(|span| source(vec!["[]".to_string()], span))
                    .collect();
            }
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
            else {
                return Vec::new();
            };
            let mut sources = Vec::new();
            for (output_path, provenance) in map {
                for source in provenance.sources {
                    let actual = match source.source {
                        crate::AST::ViewSource::Receiver => receiver.as_ref(),
                        crate::AST::ViewSource::Parameter(index) => {
                            let Some(arg) = args.get(index) else { continue };
                            &arg.expr
                        }
                        crate::AST::ViewSource::Static { .. } => continue,
                    };
                    let places = self.compose_view_source_places(
                        actual,
                        &source.projections,
                        init.span(),
                    );
                    if places.is_empty() {
                        continue;
                    }
                    let access = if provenance.mutable {
                        ViewAccess::Write
                    } else {
                        ViewAccess::Read
                    };
                    for place in places {
                        let kind = self.view_kind_for_place(&place);
                        sources.push((output_path.clone(), place, kind, access));
                    }
                }
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
        let receiver = receiver.as_ref().without_parens();
        let Some(mut place) = self.place_from_expr(receiver) else {
            return Vec::new();
        };
        let kind = match receiver {
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

    pub(crate) fn check_list_view_element_aliases(
        &mut self,
        elements: &[Expr],
        element_ty: &Type,
    ) {
        let direct_mutable =
            matches!(element_ty, Type::Apply { name, .. } if name == "ViewMut");
        let mut previous: Vec<(ViewPlace, ViewAccess)> = Vec::new();
        for element in elements {
            let mut sources: Vec<_> = self
                .view_call_sources(element)
                .into_iter()
                .map(|(_, place, _, access)| (place, access))
                .collect();
            if sources.is_empty() && direct_mutable {
                if let Some(place) = self.place_from_expr(element) {
                    sources.push((place, ViewAccess::Write));
                }
            }
            for (place, access) in sources {
                if previous.iter().any(|(existing_place, existing_access)| {
                    (*existing_access == ViewAccess::Write || access == ViewAccess::Write)
                        && existing_place.overlaps(&place)
                }) {
                    let place_name = Self::place_name(&place);
                    self.diags.push(Diagnostic::error(
                        "E0212",
                        format!("list elements create overlapping views of `{place_name}`"),
                        "list elements coexist, so an exclusive mutable view cannot overlap another element's view"
                            .to_string(),
                        "use disjoint ranges, keep only one mutable view, or store owned values"
                            .to_string(),
                        Some(element.span()),
                    ));
                }
                previous.push((place, access));
            }
        }
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
        binding_ty: &Type,
        transfer_from: Option<&str>,
    ) {
        let enum_variants = match binding_ty {
            Type::Named(type_name) | Type::Apply { name: type_name, .. } => self
                .resolve_enum_variants_cloned(type_name)
                .map(|variants| variants.into_keys().collect::<HashSet<_>>()),
            _ => None,
        };
        self.record_view_with_exemptions(
            name,
            output_path,
            place,
            kind,
            access,
            span,
            transfer_from,
            enum_variants.as_ref(),
        );
    }

    /// Transfer the hidden view relation from a matched aggregate into the
    /// names bound by that pattern. Call this only after the arm bindings have
    /// entered their lexical scope.
    pub(crate) fn record_condition_view_bindings(&mut self, condition: &Expr) {
        match condition {
            Expr::PatternTest {
                subject, pattern, ..
            } => self.record_pattern_view_bindings(subject, pattern),
            Expr::Binary(BinOp::And, left, right, _) => {
                self.record_condition_view_bindings(left);
                self.record_condition_view_bindings(right);
            }
            _ => {}
        }
    }

    pub(crate) fn record_pattern_view_bindings(
        &mut self,
        subject: &Expr,
        pattern: &Pattern,
    ) {
        let sources = self.view_call_sources(subject);
        let transfer_from = sources
            .iter()
            .any(|(_, _, _, access)| *access == ViewAccess::Write)
            .then(|| match subject {
                Expr::Ident(name, _) => Some(name.as_str()),
                _ => None,
            })
            .flatten();
        let mut bound_slots = Vec::new();

        match pattern {
            Pattern::Present {
                binding,
                binding_span,
                ..
            }
            | Pattern::Ok {
                binding,
                binding_span,
                ..
            }
            | Pattern::Err {
                binding,
                binding_span,
                ..
            } => {
                bound_slots.push((binding.clone(), *binding_span, Vec::new()));
            }
            Pattern::Variant {
                variant,
                bindings: slots,
                ..
            } => {
                let slot_names = self.pattern_variant_slot_names(subject, variant, slots.len());
                for (index, slot) in slots.iter().enumerate() {
                    if let crate::AST::PatSlot::Bind { name, span } = slot {
                        let slot_name = slot_names
                            .get(index)
                            .cloned()
                            .unwrap_or_else(|| index.to_string());
                        bound_slots.push((
                            name.clone(),
                            *span,
                            vec![variant.clone(), slot_name],
                        ));
                    }
                }
            }
            Pattern::Struct { fields, .. } => {
                for field in fields {
                    if let crate::AST::StructPatField::Bind {
                        field,
                        local,
                        local_span,
                        ..
                    } = field
                    {
                        bound_slots.push((
                            local.clone(),
                            *local_span,
                            vec![field.clone()],
                        ));
                    }
                }
            }
            Pattern::Or(alternatives, _) => {
                for alternative in alternatives {
                    self.record_pattern_view_bindings(subject, alternative);
                }
                return;
            }
            Pattern::Absent(_)
            | Pattern::Range { .. }
            | Pattern::StrMatch { .. }
            | Pattern::BinMatch { .. } => {}
        }

        for (name, span, source_prefix) in bound_slots {
            let binding_span = self
                .lookup(&name)
                .map(|info| info.def_span)
                .unwrap_or(span);
            let mut matched = false;
            for (path, place, kind, access) in &sources {
                if !path.starts_with(&source_prefix) {
                    continue;
                }
                matched = true;
                self.record_view_with_exemptions(
                    &name,
                    path[source_prefix.len()..].to_vec(),
                    place.clone(),
                    *kind,
                    *access,
                    binding_span,
                    transfer_from,
                    None,
                );
            }
            if matched {
                continue;
            }

            // A view-bearing aggregate parameter has no concrete owner fact
            // inside its callee. Treat the matched payload as a projection of
            // that parameter; call-site composition later reconnects the
            // abstract path to every concrete source in the argument.
            let Some(base) = self.place_from_expr(subject) else {
                continue;
            };
            let Some(binding_ty) = self.lookup(&name).map(|info| info.ty.clone()) else {
                continue;
            };
            for (output_path, access) in self.view_leaf_paths(&binding_ty) {
                let mut place = base.clone();
                for projection in source_prefix.iter().chain(output_path.iter()) {
                    place.projections.push(if projection == "[]" {
                        ViewProjection::Index {
                            value: None,
                            span,
                        }
                    } else {
                        ViewProjection::Field(projection.clone())
                    });
                }
                let kind = self.view_kind_for_place(&place);
                self.record_view_with_exemptions(
                    &name,
                    output_path,
                    place,
                    kind,
                    access,
                    binding_span,
                    transfer_from,
                    None,
                );
            }
        }
    }

    pub(crate) fn view_leaf_paths(&self, ty: &Type) -> Vec<(Vec<String>, ViewAccess)> {
        fn walk(
            checker: &Checker<'_>,
            ty: &Type,
            path: &mut Vec<String>,
            seen: &mut HashSet<String>,
            out: &mut Vec<(Vec<String>, ViewAccess)>,
        ) {
            match ty {
                // D-PIN1=A: a pin is a write window, so it is a view leaf too.
                Type::Apply { name, .. }
                    if name == "View" || name == "ViewMut" || name == Syntax::TYPE_PIN =>
                {
                    out.push((
                        path.clone(),
                        if name == "View" {
                            ViewAccess::Read
                        } else {
                            ViewAccess::Write
                        },
                    ));
                }
                Type::List(inner) | Type::FixedList { elem: inner, .. } => {
                    path.push("[]".to_string());
                    walk(checker, inner, path, seen, out);
                    path.pop();
                }
                Type::Tuple(fields) => {
                    for (field, field_ty) in fields {
                        path.push(field.clone());
                        walk(checker, field_ty, path, seen, out);
                        path.pop();
                    }
                }
                Type::Option(inner) | Type::Tagged { inner, .. } => {
                    walk(checker, inner, path, seen, out);
                }
                Type::Result { ok, err } => {
                    walk(checker, ok, path, seen, out);
                    walk(checker, err, path, seen, out);
                }
                Type::Named(name) | Type::Apply { name, .. } if seen.insert(name.clone()) => {
                    if let Some(fields) = checker.registry.struct_fields(name) {
                        for (field, _, field_ty) in fields {
                            path.push(field.clone());
                            walk(checker, field_ty, path, seen, out);
                            path.pop();
                        }
                    } else if let Some(variants) = checker.resolve_enum_variants_cloned(name) {
                        for (variant, (_, payload)) in variants {
                            match payload {
                                VariantPayload::Unit => {}
                                VariantPayload::Single(inner, _) => {
                                    path.push(variant);
                                    path.push("0".to_string());
                                    walk(checker, &inner, path, seen, out);
                                    path.pop();
                                    path.pop();
                                }
                                VariantPayload::Named(fields) => {
                                    for field in fields {
                                        path.push(variant.clone());
                                        path.push(field.name);
                                        walk(checker, &field.ty, path, seen, out);
                                        path.pop();
                                        path.pop();
                                    }
                                }
                            }
                        }
                    }
                    seen.remove(name);
                }
                _ => {}
            }
        }

        let mut out = Vec::new();
        walk(
            self,
            ty,
            &mut Vec::new(),
            &mut HashSet::new(),
            &mut out,
        );
        out
    }

    fn conservative_callback_view_provenance(
        &self,
        params: &[Type],
        ret: &Type,
    ) -> crate::AST::ViewProvenanceMap {
        let sources = params
            .iter()
            .enumerate()
            .filter(|(_, ty)| !ty.is_scalar())
            .map(|(index, _)| crate::AST::ViewSourcePath {
                source: crate::AST::ViewSource::Parameter(index),
                projections: Vec::new(),
            })
            .collect::<std::collections::BTreeSet<_>>();
        self.view_leaf_paths(ret)
            .into_iter()
            .map(|(output_path, access)| {
                (
                    output_path,
                    crate::AST::ViewProvenance {
                        sources: sources.clone(),
                        mutable: access == ViewAccess::Write,
                    },
                )
            })
            .collect()
    }

    fn pattern_variant_slot_names(
        &self,
        subject: &Expr,
        variant: &str,
        slot_count: usize,
    ) -> Vec<String> {
        fn subject_type(checker: &Checker<'_>, subject: &Expr) -> Option<Type> {
            match subject {
                Expr::Ident(name, _) => checker.lookup(name).map(|info| info.ty.clone()),
                Expr::Copy(inner, _) | Expr::Paren(inner, _) => subject_type(checker, inner),
                _ => None,
            }
        }

        let named = subject_type(self, subject)
            .and_then(|ty| match ty {
                Type::Named(name) | Type::Apply { name, .. } => Some(name),
                _ => None,
            })
            .and_then(|name| self.resolve_enum_variants_cloned(&name))
            .and_then(|variants| variants.get(variant).cloned())
            .and_then(|(_, payload)| match payload {
                VariantPayload::Named(fields) => {
                    Some(fields.into_iter().map(|field| field.name).collect())
                }
                _ => None,
            });

        named.unwrap_or_else(|| (0..slot_count).map(|index| index.to_string()).collect())
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

    /// Moving a mutable view out of a view-bearing aggregate transfers the
    /// exclusive loan. Retire the carrier name without treating that transfer
    /// as a mutation of the underlying owner.
    pub(crate) fn mutable_view_aggregate_root(&self, expr: &Expr) -> Option<String> {
        let expr = match expr {
            Expr::Place(inner, _, _) | Expr::Paren(inner, _) => inner.as_ref(),
            _ => expr,
        };
        let (Expr::Field(..) | Expr::Index { .. }) = expr else {
            return None;
        };
        let root = expr_root_ident(expr)?;
        self.view_facts(root)
            .iter()
            .any(|fact| fact.access == ViewAccess::Write)
            .then(|| root.to_string())
    }

    pub(crate) fn finish_mutable_view_aggregate_transfer(
        &mut self,
        root: Option<&str>,
        span: Span,
    ) {
        if let Some(root) = root {
            self.flow.moved.set(root, span);
        }
    }

    pub(crate) fn transfer_mutable_pattern_subject(&mut self, subject: &Expr) -> bool {
        let Expr::Ident(name, span) = subject else {
            return false;
        };
        if self
            .view_facts(name)
            .iter()
            .all(|fact| fact.access == ViewAccess::Read)
        {
            return false;
        }
        self.flow.moved.set(name, *span);
        true
    }

    /// True if `name` is currently a live `View<T>` binding.
    pub(crate) fn is_list_view(&self, name: &str) -> bool {
        self.view_kind(name).is_some_and(ViewKind::is_named_window)
    }

    pub(crate) fn type_contains_view_boundary(&self, ty: &Type) -> bool {
        fn payload_contains(
            registry: &TypeRegistry,
            payload: &VariantPayload,
            seen: &mut HashSet<String>,
        ) -> bool {
            match payload {
                VariantPayload::Unit => false,
                VariantPayload::Single(ty, _) => contains(registry, ty, seen),
                VariantPayload::Named(fields) => fields
                    .iter()
                    .any(|field| contains(registry, &field.ty, seen)),
            }
        }

        fn named_contains(
            registry: &TypeRegistry,
            name: &str,
            seen: &mut HashSet<String>,
        ) -> bool {
            if !seen.insert(name.to_string()) {
                return false;
            }
            let found = registry.struct_fields(name).is_some_and(|fields| {
                fields
                    .iter()
                    .any(|(_, _, field_ty)| contains(registry, field_ty, seen))
            }) || registry.enum_variants(name).is_some_and(|variants| {
                variants
                    .values()
                    .any(|(_, payload)| payload_contains(registry, payload, seen))
            });
            seen.remove(name);
            found
        }

        fn contains(
            registry: &TypeRegistry,
            ty: &Type,
            seen: &mut HashSet<String>,
        ) -> bool {
            match ty {
                // D-PIN1=A: `Pin<T>` is a borrowed window like `View`/`ViewMut`,
                // so it crosses the same provenance boundary — a returned or
                // stored pin must name the owner it borrows from.
                Type::Apply { name, args }
                    if matches!(name.as_str(), "View" | "ViewMut" | Syntax::TYPE_PIN)
                        && args.len() == 1 =>
                {
                    true
                }
                Type::Named(name) => named_contains(registry, name, seen),
                Type::Apply { name, args } => {
                    args.iter().any(|arg| contains(registry, arg, seen))
                        || named_contains(registry, name, seen)
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
    /// the canonical identity. Compatible return paths form a source union.
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
        let source = crate::AST::ViewSourcePath {
            source,
            projections,
        };
        let map = self.return_view_provenance.get_or_insert_with(Default::default);
        let provenance = map
            .entry(output_path)
            .or_insert_with(|| crate::AST::ViewProvenance {
                sources: Default::default(),
                mutable: access == ViewAccess::Write,
            });
        if provenance.mutable != (access == ViewAccess::Write) {
            self.diags.push(Diagnostic::error(
                "E2305",
                "returned view paths disagree about read or write access".to_string(),
                "one public output slot cannot sometimes be a read view and sometimes be an exclusive write view".to_string(),
                "return the same view capability on every path".to_string(),
                Some(span),
            ));
            return;
        }
        provenance.sources.insert(source);
    }

    pub(crate) fn check_named_view_binding_return(&mut self, name: &str, span: Span) {
        let facts: Vec<_> = self.view_facts(name).into_iter().cloned().collect();
        if facts.is_empty() {
            self.report_view_return_boundary(span);
            return;
        }
        for fact in facts {
            self.check_named_view_return(&fact.place, fact.access, fact.output_path, span);
        }
    }

    pub(crate) fn check_named_string_view_binding_return(&mut self, name: &str, span: Span) {
        let facts: Vec<_> = self.view_facts(name).into_iter().cloned().collect();
        if facts.is_empty() {
            self.report_string_view_boundary(span);
            return;
        }
        for fact in facts {
            if !matches!(
                fact.place.owner.origin,
                ViewOwnerOrigin::Receiver | ViewOwnerOrigin::Parameter(_)
            ) {
                self.report_string_view_unsupported_use(name, "be returned", span);
                return;
            }
            self.check_named_view_return(&fact.place, fact.access, fact.output_path, span);
        }
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
                Expr::ListLit(elements, _) => {
                    path.push("[]".to_string());
                    for value in elements {
                        walk(checker, value, path);
                    }
                    path.pop();
                }
                Expr::EnumLit { variant, args, .. } => {
                    path.push(variant.clone());
                    for (index, arg) in args.iter().enumerate() {
                        let (slot, value) = match arg {
                            crate::AST::EnumLitArg::Positional(value) => {
                                (index.to_string(), value)
                            }
                            crate::AST::EnumLitArg::Named { label, expr } => {
                                (label.clone(), expr)
                            }
                        };
                        path.push(slot);
                        walk(checker, value, path);
                        path.pop();
                    }
                    path.pop();
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
            "each public `View`/`ViewMut` slot must name a bounded set of receiver, parameter, or static sources that stay live; this return did not prove those sources"
                .to_string(),
            "return a view derived from parameters or the receiver on every path, keep the view local, or return an owned copy with `~`"
                .to_string(),
            Some(span),
        ));
    }

    pub(crate) fn report_string_view_boundary(&mut self, span: Span) {
        self.diags.push(Diagnostic::error(
            "E2307",
            "returned string views need a stable owner relationship".to_string(),
            "each public `View<str>` slot must name a bounded set of receiver, parameter, or static `String` sources that stay live; this return did not prove those sources"
                .to_string(),
            "return a view derived from parameters or the receiver on every path, or return an owned `String` copy with `~`"
                .to_string(),
            Some(span),
        ));
    }

    /// #1164 / #1163 teaching: a plain owned `String` place is not a `View<str>`.
    /// Only tracked string-view bindings / `.trim()`/`.after()`/`.before()` fill
    /// that slot under today's string-view rules.
    pub(crate) fn report_owned_string_as_view_str(&mut self, span: Span) {
        self.diags.push(Diagnostic::error(
            "E2307",
            "an owned `String` cannot fill a `View<str>` slot".to_string(),
            "`View<str>` needs a zero-copy window from `.trim()` / `.after()` / `.before()` (or an already-tracked string-view binding); a plain `String` place owns its buffer instead"
                .to_string(),
            "bind a window with `.trim()`/`.after()`/`.before()`, return a `View` of the owning element and read the field through it, or store an owned `String` field and copy with `~`"
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
        let owner = crate::Sema::current_view_fact(&self.flow.views, name)
            .map(|fact| fact.place.owner.name.clone())
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

    /// D-LIN1 / D-CONC-JOIN1: true when `ty` is a `#SingleUse` value or carries
    /// a task duty, checking the local registry first and then any imported
    /// module that exposes the type publicly. Such a value must be consumed
    /// exactly once and may not be aliased.
    pub(crate) fn type_is_single_use(&self, ty: &Type) -> bool {
        if type_requires_owned_iteration(ty) {
            return true;
        }
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

    fn place_reads_untagged_or_read_shared_guard(&self, expr: &Expr) -> bool {
        if expr_root_ident(expr).is_some_and(|name| {
            self.lookup(name)
                .is_some_and(|info| info.param_conv == Some(AccessConvention::Write))
        }) {
            return false;
        }
        match expr {
            Expr::Field(base, field, _) => {
                if field == "value" {
                    if let Some(ty) = self.place_expr_type(base) {
                        if matches!(
                            &ty,
                            Type::Apply { name, .. }
                                if name == crate::Syntax::TYPE_SHARED_GUARD
                        ) || matches!(
                            &ty,
                            Type::Tagged { marker, .. }
                                if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::SharedGuardRead))
                        ) {
                            return true;
                        }
                    }
                }
                self.place_reads_untagged_or_read_shared_guard(base)
            }
            Expr::Index { base, .. } | Expr::Paren(base, _) | Expr::Place(base, _, _) => {
                self.place_reads_untagged_or_read_shared_guard(base)
            }
            _ => false,
        }
    }

    fn reject_read_shared_guard_write(&mut self, expr: &Expr, span: Span) -> bool {
        if !self.place_reads_untagged_or_read_shared_guard(expr) {
            return false;
        }
        self.diags.push(Diagnostic::error(
            "E0205",
            "cannot edit through this read-only `SharedGuard` view".to_string(),
            "a public guard type outside its acquisition site preserves read access unless a helper explicitly receives it with write access".to_string(),
            "keep the edit at the acquisition site, or pass the guard to a helper with the write-capability marker `&`, such as `&guard: SharedGuard<T>`".to_string(),
            Some(span),
        ));
        true
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
        self.flow
            .moved
            .retain(|place, _| !Self::contains_place(name, place));
    }

    pub(crate) fn clear_moved_expr(&mut self, expr: &Expr) {
        let Some(place) = self.capture_place_from_expr(expr) else {
            return;
        };
        let place = Self::place_name(&place);
        self.flow
            .moved
            .retain(|moved, _| !Self::contains_place(&place, moved));
    }

    pub(crate) fn reject_moved_expr_use(&mut self, expr: &Expr, span: Span) -> bool {
        let Some(place) = self.capture_place_from_expr(expr) else {
            return false;
        };
        let place_name = Self::place_name(&place);
        let moved = if matches!(expr, Expr::Ident(..)) && self.suppress_partial_move_root_read {
            self.flow
                .moved
                .get(&place_name)
                .copied()
                .map(|at| (place_name.clone(), at))
        } else {
            self.flow
                .moved
                .iter()
                .filter(|(moved, _)| Self::move_keys_overlap(&place_name, moved))
                .min_by_key(|(moved, _)| moved.len())
                .map(|(moved, at)| (moved.to_string(), *at))
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
        } else if moved_ty.as_ref().is_some_and(is_one_pass_source) {
            match moved_ty.as_ref().and_then(one_pass_materializer) {
                Some(method) => format!(
                    "`{moved_place}` is one-pass — materialize it first with `{moved_place}{method}`, or create a fresh source for the second drive"
                ),
                None => format!(
                    "`{moved_place}` is one-pass and cannot be copied — create a fresh source for the second drive"
                ),
            }
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
            "after a value moves to another name, the old name no longer gives access to it".to_string(),
            fix,
            Some(span),
        ));
        self.flow.moved.remove(&moved_place);
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
        self.flow.moved.set(&Self::place_name(&place), span);
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
                "change the helper to accept a list window, then pass a range write window with the write-capability marker `&`, such as `{}xs[a..b]`",
                Syntax::SIGIL_WRITE,
            )
        } else {
            format!(
                "bind the value first: `x {} ...` then pass `{}x` with the write-capability marker `&`",
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

    /// D-LIN1 (ratified 2026-06-21): E0140 — a `#SingleUse` value that owns the
    /// consume duty (`single_use_span` set) but is not in `moved` when its scope
    /// ends was dropped without being used. It looks only at the innermost
    /// (just-closing) scope. The branch-divergence
    /// case (consumed on one path, dropped on the other) is E0141, raised in
    /// `check_if`.
    pub(crate) fn check_single_use_consumed_in_current_scope(&mut self) {
        let pending: Vec<(String, Span, bool)> = self
            .flow
            .bindings
            .iter_at(self.scope_depth())
            .filter_map(|(name, info)| {
                let span = info.single_use_span?;
                if self.flow.moved.contains(name) {
                    None
                } else {
                    Some((name.to_string(), span, is_task_type(&info.ty)))
                }
            })
            .collect();
        // Deterministic order (HashMap iteration is unordered): by span, then name.
        let mut pending = pending;
        pending.sort_by(|a, b| a.1.start.cmp(&b.1.start).then(a.0.cmp(&b.0)));
        for (name, span, is_task) in pending {
            if is_task {
                self.diags.push(l1101_unjoined_task(
                    &format!("`{name}`"),
                    "the program may end before this task finishes",
                    span,
                ));
            } else {
                self.diags.push(e0140_unconsumed(&name, span));
            }
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
            let cap = self
                .lookup(name)
                .map(|i| (i.ty.clone(), self.sendability_for(name)))
                .or_else(|| self.consts.get(name).map(|t| (t.clone(), true)));
            let Some((cap_ty, cap_sendable)) = cap else {
                continue;
            };
            let moves_capture = take_set.contains(name) || !is_cloneable(&cap_ty, self.registry);
            if !cap_sendable
                || self
                    .sendability_problem(&cap_ty, moves_capture)
                    .is_some()
            {
                return false;
            }
        }
        if let Type::Fn { ret: Some(ret), .. } = fn_ty {
            self.sendability_problem(ret, false).is_none()
        } else {
            true
        }
    }

    /// A callback retained by `core.os.on_interrupt` needs a stronger fact than
    /// an ordinary escaping closure. In particular, `Type::Fn` normally lowers
    /// to `Rc`, and moving that value is not enough for the native `Arc<dyn
    /// Fn() + Send + Sync>` boundary. Named functions and aliases that were
    /// already proven to contain such a representation carry the dedicated
    /// local fact; every other function capture is rejected here.
    pub(crate) fn lambda_interrupt_sendable(&self, lam: &Lambda, fn_ty: &Type) -> bool {
        if lam.meta.needs_fn_mut {
            return false;
        }
        let param_names: HashSet<String> = lam.params.iter().map(|p| p.name.clone()).collect();
        let mut read_caps = HashSet::new();
        let mut mut_caps = HashSet::new();
        lambda_collect_captures(&lam.body, &param_names, &mut read_caps, &mut mut_caps);
        for name in read_caps.iter().chain(mut_caps.iter()) {
            if param_names.contains(name) {
                continue;
            }
            let Some(info) = self.lookup(name) else {
                // A top-level function or module item is emitted as a static
                // wrapper, not as a captured Rc local.
                continue;
            };
            if matches!(&info.ty, Type::Fn { .. }) {
                if info.param_conv.is_some() || !info.interrupt_sendable {
                    return false;
                }
                continue;
            }
            if !self.sendability_for(name)
                || self
                    .sendability_problem(&info.ty, true)
                    .is_some()
                || matches!(
                    info.param_conv,
                    Some(AccessConvention::Read) | Some(AccessConvention::Write)
                )
            {
                return false;
            }
        }
        match fn_ty {
            Type::Fn { ret: Some(ret), .. } => self.sendability_problem(ret, true).is_none(),
            Type::Fn { .. } => true,
            _ => false,
        }
    }

    /// Whether an expression already has the one representation that may
    /// cross `core.os.on_interrupt`. This is intentionally narrower than
    /// ordinary function typing: arbitrary function-producing expressions
    /// must not reach codegen as an unexamined `Rc` value.
    pub(crate) fn interrupt_callback_expr_sendable(&self, expr: &Expr, ty: &Type) -> bool {
        if !matches!(ty, Type::Fn { .. }) {
            return false;
        }
        match expr {
            Expr::Ident(name, _) => self
                .lookup(name)
                .map(|info| info.param_conv.is_none() && info.interrupt_sendable)
                .unwrap_or_else(|| {
                    self.funcs.contains_key(name)
                        || self.unqualified.contains_key(name)
                        || self.unqualified_file.contains_key(name)
                }),
            Expr::Paren(inner, _) => self.interrupt_callback_expr_sendable(inner, ty),
            Expr::Lambda(lam) => self.lambda_interrupt_sendable(lam, ty),
            _ => false,
        }
    }

    /// Reject a local function value that would otherwise reach the callback
    /// host as an ordinary Rc. Direct named functions and callback-safe aliases
    /// are admitted; function parameters and all other local function values
    /// receive the normal E1102 product diagnostic before codegen.
    pub(crate) fn check_interrupt_callback_expr(&mut self, expr: &Expr, ty: &Type) {
        if self.interrupt_callback_depth == 0 || !matches!(ty, Type::Fn { .. }) {
            return;
        }
        fn ident(expr: &Expr) -> Option<&str> {
            match expr {
                Expr::Ident(name, _) => Some(name),
                Expr::Paren(inner, _) => ident(inner),
                _ => None,
            }
        }
        fn lambda(expr: &Expr) -> bool {
            match expr {
                Expr::Lambda(_) => true,
                Expr::Paren(inner, _) => lambda(inner),
                _ => false,
            }
        }
        fn needs_fn_mut(expr: &Expr) -> bool {
            match expr {
                Expr::Lambda(lam) => lam.meta.needs_fn_mut,
                Expr::Paren(inner, _) => needs_fn_mut(inner),
                _ => false,
            }
        }
        fn lambda_span(expr: &Expr) -> Option<Span> {
            match expr {
                Expr::Lambda(lam) => Some(lam.span),
                Expr::Paren(inner, _) => lambda_span(inner),
                _ => None,
            }
        }
        let Some(name) = ident(expr) else {
            if lambda(expr) {
                // Lambda capture checking runs while the interrupt callback
                // depth is active. It owns the detailed Send/'static proof.
                // A mutable capture is the one callback-specific fact that
                // capture sendability alone cannot express: it lowers to
                // `FnMut`, while the retained ABI is `Fn() + Send + Sync`.
                // Reject it here, before the Arc coercion reaches rustc.
                if needs_fn_mut(expr)
                    && !lambda_span(expr).is_some_and(|span| {
                        self.diags
                            .iter()
                            .any(|diag| diag.code == "E1102" && diag.span == Some(span))
                    })
                {
                    self.report_unsendable(
                        "this callback",
                        ty,
                        SendabilityProblem {
                            root: None,
                            path: Vec::new(),
                            kind: SendProblemKind::ClosureCaptures,
                        },
                        SendCrossing::InterruptCallback,
                        expr.span(),
                    );
                }
                return;
            }
            self.report_unsendable(
                "this callback",
                ty,
                SendabilityProblem {
                    root: None,
                    path: Vec::new(),
                    kind: SendProblemKind::ClosureCaptures,
                },
                SendCrossing::InterruptCallback,
                expr.span(),
            );
            return;
        };
        if self
            .lookup(name)
            .map(|info| info.param_conv.is_none() && info.interrupt_sendable)
            .unwrap_or_else(|| {
                self.funcs.contains_key(name)
                    || self.unqualified.contains_key(name)
                    || self.unqualified_file.contains_key(name)
            })
        {
            return;
        }
        let problem = self.sendability_problem(ty, false).unwrap_or(SendabilityProblem {
            root: None,
            path: Vec::new(),
            kind: SendProblemKind::ClosureCaptures,
        });
        self.report_unsendable(
            name,
            ty,
            problem,
            SendCrossing::InterruptCallback,
            expr.span(),
        );
    }

    pub(crate) fn sendability_problem(
        &self,
        ty: &Type,
        closure_taken: bool,
    ) -> Option<SendabilityProblem> {
        let mut seen = HashSet::new();
        self.sendability_problem_inner(ty, closure_taken, &mut seen)
    }

    pub(crate) fn type_contains_local_cell(&self, ty: &Type) -> bool {
        self.type_contains_local_cell_inner(ty, &mut HashSet::new())
    }

    pub(crate) fn type_contains_cell_guard(&self, ty: &Type) -> bool {
        self.type_contains_cell_guard_inner(ty, &mut HashSet::new())
    }

    pub(crate) fn cell_guard_storage_is_unsupported(&self, ty: &Type) -> bool {
        match ty {
            Type::Apply { name, .. }
                if matches!(name.as_str(), "CellReadGuard" | "CellEditGuard") =>
            {
                false
            }
            Type::Tuple(fields) => fields
                .iter()
                .any(|(_, field)| self.cell_guard_storage_is_unsupported(field)),
            _ => self.type_contains_cell_guard(ty),
        }
    }

    pub(crate) fn report_cell_guard_storage(
        &mut self,
        what: String,
        span: Span,
    ) {
        self.diags.push(Diagnostic::error(
            "E0217",
            what,
            "a Cell guard is a temporary loan handle; storing it inside another value could keep the loan after its local scope ends"
                .to_string(),
            "keep the guard in a local name or a tuple, and use `.map(...)` or `.split(...)` for projections"
                .to_string(),
            Some(span),
        ));
    }

    fn type_contains_cell_guard_inner(
        &self,
        ty: &Type,
        seen: &mut HashSet<String>,
    ) -> bool {
        match ty {
            Type::Apply { name, .. }
                if matches!(name.as_str(), "CellReadGuard" | "CellEditGuard") =>
            {
                true
            }
            Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => {
                self.type_contains_cell_guard_inner(inner, seen)
            }
            Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
                self.type_contains_cell_guard_inner(key, seen)
                    || self.type_contains_cell_guard_inner(value, seen)
            }
            Type::Fn { params, ret, .. } => {
                params
                    .iter()
                    .any(|param| self.type_contains_cell_guard_inner(param, seen))
                    || ret
                        .as_deref()
                        .is_some_and(|ret| self.type_contains_cell_guard_inner(ret, seen))
            }
            Type::Tuple(fields) => fields
                .iter()
                .any(|(_, field)| self.type_contains_cell_guard_inner(field, seen)),
            Type::FixedList { elem, .. } | Type::Tagged { inner: elem, .. } => {
                self.type_contains_cell_guard_inner(elem, seen)
            }
            Type::Union(members) => members
                .iter()
                .any(|member| self.type_contains_cell_guard_inner(member, seen)),
            Type::Named(name) => self.named_type_contains_cell_guard(name, &[], seen),
            Type::Apply { name, args } => {
                self.named_type_contains_cell_guard(name, args, seen)
            }
            Type::Int
            | Type::Float
            | Type::Bool
            | Type::String
            | Type::Char
            | Type::TraitObject(_)
            | Type::IntN { .. }
            | Type::Float32 => false,
            Type::Quantity { .. } => false,
            Type::ComputeDim(_) => false,
        }
    }

    fn named_type_contains_cell_guard(
        &self,
        name: &str,
        args: &[Type],
        seen: &mut HashSet<String>,
    ) -> bool {
        if !seen.insert(name.to_string()) {
            return false;
        }
        let subst = if args.is_empty() {
            HashMap::new()
        } else {
            self.struct_subst(name, args)
        };
        let found = match self.registry.types.get(name) {
            Some(TypeDef::Struct { fields, .. }) => fields.iter().any(|(_, _, ty)| {
                let actual = self.trait_reg.instantiate_type(ty, &subst);
                self.type_contains_cell_guard_inner(&actual, seen)
            }),
            Some(TypeDef::Enum { variants, .. }) => variants.values().any(|(_, payload)| {
                match payload {
                    VariantPayload::Unit => false,
                    VariantPayload::Single(ty, _) => {
                        let actual = self.trait_reg.instantiate_type(ty, &subst);
                        self.type_contains_cell_guard_inner(&actual, seen)
                    }
                    VariantPayload::Named(fields) => fields.iter().any(|field| {
                        let actual = self.trait_reg.instantiate_type(&field.ty, &subst);
                        self.type_contains_cell_guard_inner(&actual, seen)
                    }),
                }
            }),
            Some(TypeDef::Alias { target, .. }) => {
                let actual = self.trait_reg.instantiate_type(target, &subst);
                self.type_contains_cell_guard_inner(&actual, seen)
            }
            Some(TypeDef::Distinct { .. }) | None => false,
        };
        seen.remove(name);
        found
    }

    fn type_contains_local_cell_inner(
        &self,
        ty: &Type,
        seen: &mut HashSet<String>,
    ) -> bool {
        match ty {
            Type::Apply { name, .. }
                if matches!(
                    name.as_str(),
                    "Cell" | "CellReadGuard" | "CellEditGuard"
                ) =>
            {
                true
            }
            Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => {
                self.type_contains_local_cell_inner(inner, seen)
            }
            Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
                self.type_contains_local_cell_inner(key, seen)
                    || self.type_contains_local_cell_inner(value, seen)
            }
            Type::Fn { params, ret, .. } => {
                params
                    .iter()
                    .any(|param| self.type_contains_local_cell_inner(param, seen))
                    || ret
                        .as_deref()
                        .is_some_and(|ret| self.type_contains_local_cell_inner(ret, seen))
            }
            Type::Tuple(fields) => fields
                .iter()
                .any(|(_, field)| self.type_contains_local_cell_inner(field, seen)),
            Type::FixedList { elem, .. } | Type::Tagged { inner: elem, .. } => {
                self.type_contains_local_cell_inner(elem, seen)
            }
            Type::Union(members) => members
                .iter()
                .any(|member| self.type_contains_local_cell_inner(member, seen)),
            Type::Named(name) => self.named_type_contains_local_cell(name, &[], seen),
            Type::Apply { name, args } => {
                self.named_type_contains_local_cell(name, args, seen)
            }
            Type::Int
            | Type::Float
            | Type::Bool
            | Type::String
            | Type::Char
            | Type::TraitObject(_)
            | Type::IntN { .. }
            | Type::Float32 => false,
            Type::Quantity { .. } => false,
            Type::ComputeDim(_) => false,
        }
    }

    fn named_type_contains_local_cell(
        &self,
        name: &str,
        args: &[Type],
        seen: &mut HashSet<String>,
    ) -> bool {
        if !seen.insert(name.to_string()) {
            return false;
        }
        let subst = if args.is_empty() {
            HashMap::new()
        } else {
            self.struct_subst(name, args)
        };
        let found = match self.registry.types.get(name) {
            Some(TypeDef::Struct { fields, .. }) => fields.iter().any(|(_, _, ty)| {
                let actual = self.trait_reg.instantiate_type(ty, &subst);
                self.type_contains_local_cell_inner(&actual, seen)
            }),
            Some(TypeDef::Enum { variants, .. }) => variants.values().any(|(_, payload)| {
                match payload {
                    VariantPayload::Unit => false,
                    VariantPayload::Single(ty, _) => {
                        let actual = self.trait_reg.instantiate_type(ty, &subst);
                        self.type_contains_local_cell_inner(&actual, seen)
                    }
                    VariantPayload::Named(fields) => fields.iter().any(|field| {
                        let actual = self.trait_reg.instantiate_type(&field.ty, &subst);
                        self.type_contains_local_cell_inner(&actual, seen)
                    }),
                }
            }),
            Some(TypeDef::Alias { target, .. }) => {
                let actual = self.trait_reg.instantiate_type(target, &subst);
                self.type_contains_local_cell_inner(&actual, seen)
            }
            Some(TypeDef::Distinct { .. }) | None => false,
        };
        seen.remove(name);
        found
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
                        | "BrowserFrame"
                        | "BrowserLocator"
                        | "BrowserIntercept"
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
            Type::Apply { name, .. } if name == crate::Syntax::TYPE_SHARED_GUARD => {
                Some(SendabilityProblem {
                    root: None,
                    path: Vec::new(),
                    kind: SendProblemKind::ThreadConfined(
                        crate::Syntax::TYPE_SHARED_GUARD.to_string(),
                    ),
                })
            }
            Type::Apply { name, args }
                if matches!(name.as_str(), "Task" | "Channel" | "Sender") =>
            {
                args.iter()
                    .find_map(|arg| self.sendability_problem_inner(arg, true, seen))
            }
            // D-LOCALCELL1=A: local cells and every guard derived from them retain
            // single-threaded runtime borrow state. They cannot cross any task,
            // channel, Shared, or parallel boundary.
            Type::Apply { name, .. }
                if matches!(
                    name.as_str(),
                    "Cell" | "CellReadGuard" | "CellEditGuard"
                ) =>
            {
                Some(SendabilityProblem {
                    root: None,
                    path: Vec::new(),
                    kind: SendProblemKind::ThreadConfined(name.clone()),
                })
            }
            // D-DYNARRAY1 (E2303, reported as E1102): a `View<T>` is a borrow into
            // its owner's backing storage — it can never cross a task/channel
            // boundary, the same rule a `-> view`/`ref` value already gets.
            // D-PIN1=A: a pin is a borrow into one owner's storage too, and the
            // no-move promise only holds inside the owner's thread.
            Type::Apply { name, .. }
                if matches!(name.as_str(), "View" | "ViewMut" | Syntax::TYPE_PIN) =>
            {
                Some(SendabilityProblem {
                    root: None,
                    path: Vec::new(),
                    kind: SendProblemKind::ViewBorrow,
                })
            }
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
            Type::Quantity { base, .. } => self.sendability_problem_inner(base, closure_taken, seen),
            Type::ComputeDim(_) => None,
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
                for (field_name, _, field_ty) in fields {
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
                if !self.sendability_for(name) {
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
            (SendCrossing::InterruptCallback, _) => {
                format!(
                    "{} can't be registered as an interrupt callback because `{}` isn't sendable",
                    value_text, type_name
                )
            }
        };
        let why = if matches!(crossing, SendCrossing::InterruptCallback) {
            "core.os.on_interrupt retains callbacks until signal delivery, so the callback and its captured state must be owned and thread-safe".to_string()
        } else {
            format!(
                "{}; tasks and channels move owned values between threads",
                describe_sendability_problem(&problem)
            )
        };
        let local_cell = matches!(
            &problem.kind,
            SendProblemKind::ThreadConfined(name)
                if matches!(
                    name.as_str(),
                    "Cell" | "CellReadGuard" | "CellEditGuard"
                )
        );
        let fix = match (local_cell, crossing) {
            (true, SendCrossing::ChannelSend) => {
                "send the owned value instead, or use `Shared<T>` for synchronized state"
            }
            (true, SendCrossing::TaskCapture | SendCrossing::TaskResult) => {
                "create the `Cell<T>` inside the task, or use `Shared<T>` for synchronized state"
            }
            (true, SendCrossing::InterruptCallback) => {
                "pass a named function, or capture only owned sendable values in the callback"
            }
            (false, SendCrossing::ChannelSend) => {
                "send plain owned data instead, or rebuild the value as an owned copy before calling `.send()`"
            }
            (false, SendCrossing::TaskCapture | SendCrossing::TaskResult) => {
                "give the task plain owned data, or rebuild the value as an owned copy before spawning"
            }
            (false, SendCrossing::InterruptCallback) => {
                "pass a named function, or capture only owned sendable values in the callback"
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
            if let Some(name) = self
                .current_binding_name
                .as_ref()
                .or(self.task_spawn_binding_name.as_ref())
            {
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

    /// E0120: `loop h, hs { … }` over a list of handles that cannot be copied
    /// hands out each handle itself, which takes the list. The collection must
    /// be a bare owned local or a temporary — a borrowed name, view, field, or
    /// index cannot give the list away.
    pub(crate) fn report_borrowed_loop_consume(
        &mut self,
        collection: &Expr,
        coll_ty: &Option<Type>,
    ) {
        let list_ty = coll_ty
            .as_ref()
            .map(Type::show)
            .unwrap_or_else(|| "[Task<T>]".to_string());
        let why = "each step hands you the handle itself, and a task handle cannot be copied — so the loop takes the whole list with the move-capability marker `^`"
            .to_string();
        // Infer may wrap an owning field/index read in `Copy`; report against
        // the underlying place so the fix names the projection, not a clone.
        let place = match collection {
            Expr::Paren(inner, _) | Expr::Copy(inner, _) => inner.as_ref(),
            other => other,
        };
        match place {
            // Bare name: the fix names the list. Suggest `^` only when it is a
            // parameter; the canonical task combinators are written at spawn.
            Expr::Ident(name, _) => {
                let info = self.lookup(name);
                let is_param = info.as_ref().is_some_and(|info| info.param_conv.is_some());
                let is_task_list = info.as_ref().is_some_and(|info| {
                    matches!(
                        &info.ty,
                        Type::List(inner) | Type::FixedList { elem: inner, .. }
                            if type_requires_owned_iteration(inner)
                    )
                });
                let fix = if is_param && is_task_list {
                    format!(
                        "take the list with the move-capability marker `^`: `{name}: {}{list_ty}`, or collect the work in a canonical `task.all` or `task.group` block",
                        Syntax::SIGIL_MOVE
                    )
                } else if is_task_list {
                    format!(
                        "move the list into a local this scope owns before the loop consumes its task handles"
                    )
                } else {
                    "move the list into a local this scope owns first".to_string()
                };
                self.diags.push(Diagnostic::error(
                    "E0120",
                    format!("`{name}` was not moved here, so this loop can't take its handles"),
                    why,
                    fix,
                    Some(collection.span()),
                ));
            }
            // Field / index / slice: the root's type is not the list, so do not
            // rewrite it as `root: ^[Task<…>]`; the field or index itself must
            // first be copied into a local this scope owns.
            _ => {
                self.diags.push(Diagnostic::error(
                    "E0120",
                    "this loop can't take handles out of a field or index".to_string(),
                    why,
                    "bind the list into a local this scope owns first (`hs := …`), then write `loop h, hs { … }`"
                        .to_string(),
                    Some(collection.span()),
                ));
            }
        }
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
                            "call it on a copy, or take ownership with the move-capability marker `^`: `{}: {}{}`",
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
                "`{name}` is used again after this call, so the move-capability marker `^` (`{}{name}`) would break that later use — write the copy marker `~` (`{}{name}`) to pass a copy, or reorder so this call is `{name}`'s last use and write the move-capability marker `^` (`{}{name}`)",
                Syntax::SIGIL_MOVE,
                Syntax::SIGIL_COPY,
                Syntax::SIGIL_MOVE,
            )
        } else {
            format!(
                "write the move-capability marker `^` (`{}{name}`) to move it — this is `{name}`'s last use — or the copy marker `~` (`{}{name}`) to keep a copy",
                Syntax::SIGIL_MOVE,
                Syntax::SIGIL_COPY,
            )
        };
        Diagnostic::error("E0209", what, why, fix, Some(span))
    }

    /// D-MEMPROVENANCE3=A: `word: View<str> from corpus` — the argument's view
    /// owners must stay inside the places of the named sibling parameters.
    pub(crate) fn check_param_view_from_requirements(
        &mut self,
        sig: &crate::AST::FuncSig,
        args: &[crate::AST::CallArg],
    ) {
        if sig.param_view_from_names.iter().all(|n| n.is_none()) {
            return;
        }
        for (index, required_names) in sig.param_view_from_names.iter().enumerate() {
            let Some(required_names) = required_names else {
                continue;
            };
            let Some(arg) = args.get(index) else {
                continue;
            };
            let actual_places = self.compose_view_source_places(&arg.expr, &[], arg.expr.span());
            if actual_places.is_empty() {
                continue;
            }
            let mut allowed: Vec<ViewPlace> = Vec::new();
            for name in required_names {
                let Some(src_index) = sig.param_info.iter().position(|(n, _)| n == name) else {
                    self.diags.push(Diagnostic::error(
                        "E2305",
                        format!("`from {name}` does not name a parameter"),
                        "a parameter `from` clause names sibling parameters (or `self`) that own the view".to_string(),
                        "use a parameter name from this function's signature".to_string(),
                        Some(arg.expr.span()),
                    ));
                    continue;
                };
                let Some(src_arg) = args.get(src_index) else {
                    continue;
                };
                allowed.extend(self.compose_view_source_places(
                    &src_arg.expr,
                    &[],
                    src_arg.expr.span(),
                ));
            }
            if allowed.is_empty() {
                continue;
            }
            let ok = actual_places.iter().all(|actual| {
                allowed.iter().any(|req| {
                    actual.owner == req.owner
                        && actual.projections.starts_with(req.projections.as_slice())
                })
            });
            if !ok {
                let param_label = sig
                    .param_info
                    .get(index)
                    .map(|(n, _)| n.as_str())
                    .unwrap_or("argument");
                self.diags.push(Diagnostic::error(
                    "E2305",
                    format!("`{param_label}` must borrow from its declared `from` sources"),
                    "this argument's view owners are outside the sources named after `from`".to_string(),
                    "pass a view derived from those sources, or widen the `from` clause".to_string(),
                    Some(arg.expr.span()),
                ));
            }
        }
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
                    if type_is_copy(param_ty) {
                        // Copy values cross an owning parameter by bits.
                    } else if !crate::Sema::Diagnostics::is_secret_bearing_crypto_type(param_ty)
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
                                "`{}` needs the move-capability marker `^` here — this value can't be copied",
                                call_name,
                            ),
                            format!(
                                "parameter {} takes ownership through the move-capability marker `^`; passing `{}` without that marker would have to copy it, but this type can't be copied",
                                idx + 1,
                                name
                            ),
                            format!("write the move-capability marker `^` (`{}{}`) to move ownership to `{}`", Syntax::SIGIL_MOVE, name, call_name),
                            Some(*span),
                        ));
                    }
                }
            }
            AccessConvention::Move => {
                if let Expr::Ident(name, span) = &arg.expr {
                    if !type_is_copy(param_ty) {
                        self.mark_moved(name.clone(), *span);
                    }
                }
            }
            AccessConvention::Write => {}
        }
    }

    /// Apply the declaration-side capability contract to a function-value
    /// argument. Function values carry the same convention metadata as named
    /// functions, so their calls must not silently collapse to read-only.
    pub(crate) fn check_callable_argument_ownership(
        &mut self,
        call_name: &str,
        index: usize,
        param_conv: AccessConvention,
        param_ty: &Type,
        arg: &mut crate::AST::CallArg,
    ) {
        if param_conv != AccessConvention::Move {
            if let Expr::Ident(name, span) = &arg.expr {
                if self
                    .lookup(name)
                    .is_some_and(|info| info.single_use_span.is_some())
                {
                    self.diags.push(e0142_aliased(name, call_name, *span));
                    return;
                }
            }
        }

        if arg.convention == AccessConvention::Write
            && !matches!(arg.expr, Expr::Ident(_, _))
        {
            self.diags.push(Diagnostic::error(
                "E0202",
                "the write-capability marker `&` needs a plain named binding after it".to_string(),
                "write access from the write-capability marker `&` can only be granted to a named binding, not an expression"
                    .to_string(),
                self.non_name_write_argument_fix(&arg.expr),
                Some(arg.span),
            ));
        }

        match (param_conv, arg.convention) {
            (AccessConvention::Move, AccessConvention::Read) => {
                if let Expr::Ident(name, span) = &arg.expr {
                    if type_is_copy(param_ty) {
                        // Copy values cross an owning parameter by bits.
                    } else if !self.is_resource_type(param_ty)
                        && is_cloneable(param_ty, self.registry)
                    {
                        arg.flags.implicit_clone = true;
                        let diagnostic = self.e0209_implicit_clone(
                            format!("implicit clone of `{name}`"),
                            format!("`{call_name}` expects to take ownership of this value"),
                            name,
                            *span,
                        );
                        self.diags.push(diagnostic);
                    } else {
                        self.diags.push(Diagnostic::error(
                            "E0201",
                            format!(
                                "`{call_name}` needs the move-capability marker `^` here — this value can't be copied"
                            ),
                            format!(
                                "parameter {} takes ownership through the move-capability marker `^`; passing `{name}` without that marker would have to copy it, but this type can't be copied",
                                index + 1
                            ),
                            format!(
                                "write the move-capability marker `^` (`{}{name}`) to move ownership to `{call_name}`",
                                Syntax::SIGIL_MOVE
                            ),
                            Some(*span),
                        ));
                    }
                }
            }
            (AccessConvention::Move, AccessConvention::Move) => {
                if let Expr::Ident(name, span) = &arg.expr {
                    if !type_is_copy(param_ty) {
                        self.mark_moved(name.clone(), *span);
                    }
                }
            }
            (AccessConvention::Write, AccessConvention::Read) => {
                if let Expr::Ident(name, span) = &arg.expr {
                    self.diags.push(Diagnostic::error(
                        "E0202",
                        format!(
                            "parameter `{name}` requires the write-capability marker `&` at the call site"
                        ),
                        format!(
                            "`{call_name}` needs to edit this value with the write-capability marker `&`; passing it without that marker grants only read access"
                        ),
                        format!(
                            "write the write-capability marker `&` (`{}{name}`) when calling `{call_name}`",
                            Syntax::SIGIL_WRITE
                        ),
                        Some(*span),
                    ));
                }
            }
            (AccessConvention::Write, AccessConvention::Write) => {
                if let Expr::Ident(name, span) = &arg.expr {
                    if let Some(info) = self.lookup(name) {
                        if !info.mutable {
                            let mut diagnostic = Diagnostic::error(
                                "E0111",
                                format!(
                                    "`{name}` was made with `{}`, so it can't be changed",
                                    Syntax::SIGIL_BIND_IMMUT
                                ),
                                format!(
                                    "`{call_name}` will change this value, so it must be mutable (`{}`)",
                                    Syntax::SIGIL_BIND_MUT
                                ),
                                format!(
                                    "declare it with `{} {name} ...`",
                                    Syntax::SIGIL_BIND_MUT
                                ),
                                Some(*span),
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
            }
            (AccessConvention::Read | AccessConvention::Write, AccessConvention::Move) => {
                self.diags.push(Diagnostic::error(
                    "E0203",
                    "a value was passed with the move-capability marker `^` to a parameter that does not consume".to_string(),
                    "only parameters declared with the move-capability marker `^` accept a moved value at the call site".to_string(),
                    "remove the move-capability marker `^`, or declare the parameter with that marker to take ownership".to_string(),
                    Some(arg.span),
                ));
            }
            _ => {}
        }

        self.check_write_arg_change(arg);
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
            let got = self.widen_numeric_argument(
                &mut arg.expr,
                got,
                &elem_ty,
                AccessConvention::Move,
            );
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
                                    crate::Syntax::MARKER_LOCAL
                                ),
                                format!(
                                    "`#{}` keeps `{}` in the fast one-thread form",
                                    crate::Syntax::MARKER_LOCAL,
                                    got.name()
                                ),
                                format!(
                                    "remove `#{}`, or send an owned copy of the value",
                                    crate::Syntax::MARKER_LOCAL
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
        self.finish_shared_closure("read", inner, args, span, false, false, None)
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
        self.finish_shared_closure("edit", inner, args, span, true, false, None)
    }

    pub(crate) fn finish_cell_get(&mut self, inner: &Type, span: Span) -> Option<Type> {
        if !is_cloneable(inner, self.registry) {
            self.diags.push(Diagnostic::error(
                "E0112",
                format!("`Cell<{}>.get()` cannot copy its value", inner.show()),
                "`get` returns an independent owned value, so the stored type must support copying".to_string(),
                "use `.read(value => ...)` to inspect it without making a copy".to_string(),
                Some(span),
            ));
        }
        Some(inner.clone())
    }

    pub(crate) fn finish_cell_write(
        &mut self,
        method: &str,
        inner: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        if args.len() != 1 {
            self.diags
                .push(crate::Sema::CheckerCoreLib::wrong_core_arity(
                    method,
                    1,
                    args.len(),
                    span,
                ));
            for arg in args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return (method == "replace").then(|| inner.clone());
        }
        let arg = &mut args[0];
        let saved = self.expected_type.clone();
        self.expected_type = Some(inner.clone());
        let got = self.infer(&mut arg.expr);
        self.expected_type = saved;
        if let Some(got) = got {
            let got = self.widen_numeric_argument(
                &mut arg.expr,
                got,
                inner,
                AccessConvention::Move,
            );
            self.check_type_assignable(inner, &got, arg.expr.span());
        }
        self.check_take_arg_ownership(method, 0, inner, arg);
        (method == "replace").then(|| inner.clone())
    }

    pub(crate) fn finish_cell_get_or_set(
        &mut self,
        inner: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        let Type::Option(value) = inner else {
            self.diags.push(Diagnostic::error(
                "E0112",
                format!("`Cell<{}>` is not an optional cell", inner.show()),
                "`get_or_set` initializes an empty `Cell<T?>` and returns its `T` value".to_string(),
                "use `Cell<T?>`, or use `.get()` for a cell that always has a value".to_string(),
                Some(span),
            ));
            for arg in args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return Some(inner.clone());
        };
        if args.len() != 1 {
            self.diags
                .push(crate::Sema::CheckerCoreLib::wrong_core_arity(
                    "get_or_set",
                    1,
                    args.len(),
                    span,
                ));
            for arg in args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return Some((**value).clone());
        }
        if !matches!(args[0].expr, Expr::Lambda(_)) {
            self.diags.push(Diagnostic::error(
                "E0112",
                "`get_or_set` needs a zero-parameter lambda".to_string(),
                "the initializer runs only when the cell is empty".to_string(),
                "write `.get_or_set(() => value)`".to_string(),
                Some(args[0].expr.span()),
            ));
        }
        let expected = Type::Fn {
            params: vec![],
            ret: Some(value.clone()),
            effect_bound: None,
            param_contract: None,
                call_metadata: None,
            return_view_provenance: None,
        };
        let saved_expected = self.expected_type.clone();
        let saved_escapes = self.lambda_escapes;
        self.expected_type = Some(expected);
        self.lambda_escapes = false;
        self.infer(&mut args[0].expr);
        self.lambda_escapes = saved_escapes;
        self.expected_type = saved_expected;
        if !is_cloneable(value, self.registry) {
            self.diags.push(Diagnostic::error(
                "E0112",
                format!("`Cell<{}?>.get_or_set()` cannot copy its value", value.show()),
                "`get_or_set` returns an independent owned value, so `T` must support copying".to_string(),
                "use `.edit(value => ...)` when the cached value cannot be copied".to_string(),
                Some(span),
            ));
        }
        Some((**value).clone())
    }

    pub(crate) fn finish_cell_read(
        &mut self,
        inner: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        self.finish_shared_closure("read", inner, args, span, false, false, None)
    }

    pub(crate) fn finish_cell_edit(
        &mut self,
        inner: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        self.finish_shared_closure("edit", inner, args, span, true, false, None)
    }

    pub(crate) fn finish_cell_guard_map(
        &mut self,
        guard: &str,
        inner: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        let editable = guard == "CellEditGuard";
        let projection = args
            .first()
            .and_then(cell_guard_projection_path_from_arg);
        if let Some(arg) = args.first_mut() {
            record_cell_guard_projection_path(arg, projection.clone());
        }
        if args.first().is_some_and(|_| projection.is_none()) {
            self.diags.push(Diagnostic::error(
                "E0112",
                "`guard.map` needs a field projection".to_string(),
                "a mapped guard must point into the value covered by its original dynamic loan".to_string(),
                "write `.map(value => value.field)`".to_string(),
                Some(args[0].expr.span()),
            ));
        }
        let projected =
            self.finish_shared_closure("map", inner, args, span, editable, false, None)?;
        Some(Type::Apply {
            name: guard.to_string(),
            args: vec![projected],
        })
    }

    pub(crate) fn finish_cell_guard_split(
        &mut self,
        guard: &str,
        inner: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        if args.len() != 2 {
            self.diags
                .push(crate::Sema::CheckerCoreLib::wrong_core_arity(
                    "split",
                    2,
                    args.len(),
                    span,
                ));
            for arg in args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        }
        let paths = [
            cell_guard_projection_path_from_arg(&args[0]),
            cell_guard_projection_path_from_arg(&args[1]),
        ];
        for (arg, path) in args.iter_mut().zip(paths.iter()) {
            record_cell_guard_projection_path(arg, path.clone());
            if path.is_none() {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    "`guard.split` needs two field projections".to_string(),
                    "each split guard must point into the value covered by the original dynamic loan".to_string(),
                    "write `.split(value => value.left, value => value.right)`".to_string(),
                    Some(arg.expr.span()),
                ));
            }
        }
        let editable = guard == "CellEditGuard";
        if editable {
            if let (Some(first), Some(second)) = (&paths[0], &paths[1]) {
                if cell_guard_projection_paths_overlap(first, second) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        "`guard.split` projections overlap".to_string(),
                        "each edit guard must cover a disjoint field of the original value"
                            .to_string(),
                        "project two different fields, or keep one guard and edit through it"
                            .to_string(),
                        Some(args[1].expr.span()),
                    ));
                }
            }
        }
        let (first_arg, second_arg) = args.split_at_mut(1);
        let first =
            self.finish_shared_closure("split", inner, first_arg, span, editable, false, None)?;
        let second =
            self.finish_shared_closure("split", inner, second_arg, span, editable, false, None)?;
        Some(Type::Tuple(vec![
            (
                "first".to_string(),
                Box::new(Type::Apply {
                    name: guard.to_string(),
                    args: vec![first],
                }),
            ),
            (
                "second".to_string(),
                Box::new(Type::Apply {
                    name: guard.to_string(),
                    args: vec![second],
                }),
            ),
        ]))
    }

    pub(crate) fn finish_expiring_secret_with(
        &mut self,
        inner: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        let loan = crate::Sema::Diagnostics::expiring_secret_loan_type(inner.clone());
        let result = self.finish_shared_closure("with", &loan, args, span, false, true, None);
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
        expected_return: Option<Type>,
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
            ret: expected_return.map(Box::new),
            effect_bound: None, return_view_provenance: None,
            param_contract: None,
                call_metadata: None,
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

    pub(crate) fn finish_shared_guard_method(
        &mut self,
        receiver: &Expr,
        guard_ty: &Type,
        method: &str,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Option<Type>> {
        let Type::Tagged { marker, inner } = guard_ty else {
            return None;
        };
        let Type::Apply {
            name,
            args: guard_args,
        } = inner.as_ref()
        else {
            return None;
        };
        if name != Syntax::TYPE_SHARED_GUARD || guard_args.len() != 1 {
            return None;
        }
        let value_ty = guard_args[0].clone();
        let editable = matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::SharedGuardEdit));
        let tagged = |ty: Type| Type::Tagged {
            marker: marker.clone(),
            inner: Box::new(Type::Apply {
                name: Syntax::TYPE_SHARED_GUARD.to_string(),
                args: vec![ty],
            }),
        };

        match method {
            "map" => {
                if args.len() != 1 {
                    return Some(self.finish_shared_closure(
                        "map",
                        &value_ty,
                        args,
                        span,
                        editable,
                        false,
                        None,
                    ));
                }
                let projection = shared_guard_projection(&args[0].expr).filter(|path| {
                    self.shared_guard_projection_is_stored(&value_ty, path)
                });
                if projection.is_none() {
                    self.diags.push(Diagnostic::error(
                        "E0215",
                        "`SharedGuard.map` needs a stored field projection".to_string(),
                        "a mapped guard must keep a stable stored place inside the value protected by the original lock; computed fields are values, not places".to_string(),
                        "write a direct projection such as `guard.map(value => value.field)`"
                            .to_string(),
                        Some(args[0].expr.span()),
                    ));
                }
                if let Expr::Lambda(lambda) = &mut args[0].expr {
                    lambda.meta.guard_projection = projection;
                }
                let projected = self.finish_shared_closure(
                    "map",
                    &value_ty,
                    args,
                    span,
                    editable,
                    false,
                    None,
                );
                self.consume_builtin_receiver(receiver, method);
                Some(projected.map(tagged))
            }
            "split" => {
                if args.len() != 2 {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!("`split` expects 2 arguments, got {}", args.len()),
                        "splitting a guard needs two field projections".to_string(),
                        "write `guard.split(value => value.left, value => value.right)`"
                            .to_string(),
                        Some(span),
                    ));
                    for arg in args {
                        self.infer(&mut arg.expr);
                    }
                    return Some(None);
                }
                let first_path = shared_guard_projection(&args[0].expr).filter(|path| {
                    self.shared_guard_projection_is_stored(&value_ty, path)
                });
                let second_path = shared_guard_projection(&args[1].expr).filter(|path| {
                    self.shared_guard_projection_is_stored(&value_ty, path)
                });
                if first_path.is_none()
                    || second_path.is_none()
                    || first_path == second_path
                    || first_path
                        .as_ref()
                        .zip(second_path.as_ref())
                        .is_some_and(|(a, b)| a.starts_with(b) || b.starts_with(a))
                {
                    self.diags.push(Diagnostic::error(
                        "E0216",
                        "`SharedGuard.split` needs two disjoint field projections".to_string(),
                        "two editable guards may never point at the same field or at an enclosing and nested field".to_string(),
                        "project sibling fields, such as `value.left` and `value.right`"
                            .to_string(),
                        Some(span),
                    ));
                }
                if let Expr::Lambda(lambda) = &mut args[0].expr {
                    lambda.meta.guard_projection = first_path;
                }
                if let Expr::Lambda(lambda) = &mut args[1].expr {
                    lambda.meta.guard_projection = second_path;
                }
                let (first, second) = args.split_at_mut(1);
                let first_ty = self.finish_shared_closure(
                    "split",
                    &value_ty,
                    first,
                    span,
                    editable,
                    false,
                    None,
                );
                let second_ty = self.finish_shared_closure(
                    "split",
                    &value_ty,
                    second,
                    span,
                    editable,
                    false,
                    None,
                );
                self.consume_builtin_receiver(receiver, method);
                Some(first_ty.zip(second_ty).map(|(first, second)| {
                    Type::Tuple(vec![
                        ("first".to_string(), Box::new(tagged(first))),
                        ("second".to_string(), Box::new(tagged(second))),
                    ])
                }))
            }
            "wait" => {
                if !editable {
                    self.diags.push(Diagnostic::error(
                        "E0205",
                        "`SharedGuard.wait` needs an edit guard".to_string(),
                        "waiting releases and reacquires exclusive access before the predicate is checked again".to_string(),
                        "create this guard with `guard_edit()`".to_string(),
                        Some(span),
                    ));
                }
                if args.len() != 2 {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!("`wait` expects 2 arguments, got {}", args.len()),
                        "waiting needs one Condition and one predicate".to_string(),
                        "write `guard.wait(condition, value => predicate)`".to_string(),
                        Some(span),
                    ));
                    for arg in args {
                        self.infer(&mut arg.expr);
                    }
                    return Some(None);
                }
                let condition_ty = self.infer(&mut args[0].expr);
                if !matches!(
                    condition_ty,
                    Some(Type::Named(ref name)) if name == Syntax::TYPE_CONDITION
                ) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        "`SharedGuard.wait` needs a Condition first".to_string(),
                        "the Condition owns the waiter set notified by `notify_one` and `notify_all`".to_string(),
                        "pass a value created with `Condition.new()`".to_string(),
                        Some(args[0].expr.span()),
                    ));
                }
                let predicate = self.finish_shared_closure(
                    "wait",
                    &value_ty,
                    &mut args[1..],
                    span,
                    false,
                    false,
                    Some(Type::Bool),
                );
                if predicate != Some(Type::Bool) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        "`SharedGuard.wait` predicate must return Bool".to_string(),
                        "the guard sleeps until this predicate becomes true".to_string(),
                        "return a Bool condition from the predicate".to_string(),
                        Some(args[1].expr.span()),
                    ));
                }
                Some(Some(Type::Result {
                    ok: Box::new(Type::Named("Unit".to_string())),
                    err: Box::new(Type::String),
                }))
            }
            _ => None,
        }
    }

    fn shared_guard_projection_is_stored(&self, root: &Type, path: &[String]) -> bool {
        let mut owner = root.clone();
        for field in path {
            if self.field_is_computed(&owner, field) {
                return false;
            }
            let Some(next) = self.projected_field_type(owner, field) else {
                // Ordinary field inference reports an unknown field. This
                // helper only adds the place-vs-value law for computed fields.
                return true;
            };
            owner = next;
        }
        true
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
            let got = self.widen_numeric_argument(
                &mut arg.expr,
                got,
                &elem_ty,
                AccessConvention::Move,
            );
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

fn shared_guard_projection(expr: &Expr) -> Option<Vec<String>> {
    let Expr::Lambda(lambda) = expr else {
        return None;
    };
    let [param] = lambda.params.as_slice() else {
        return None;
    };
    let crate::AST::LambdaBody::Expr(body) = &lambda.body else {
        return None;
    };

    let mut cursor = body.as_ref();
    let mut fields = Vec::new();
    while let Expr::Field(base, field, _) = cursor {
        fields.push(field.clone());
        cursor = base;
    }
    if !matches!(cursor, Expr::Ident(name, _) if name == &param.name) || fields.is_empty() {
        return None;
    }
    fields.reverse();
    Some(fields)
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

/// D-LIN1 (ratified 2026-06-21) / D-FACT-WORD1=A: E0140 — a `#SingleUse` value
/// reached the end of its scope without being consumed. One duty voice: the
/// value still owes its job; consuming it discharges the duty, and the
/// audited `consume` gate (E0143) is the only written word that lets it go
/// undone.
pub(crate) fn e0140_unconsumed(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0140",
        format!("`{}` still owes `consume`", name),
        "this value's type is `#SingleUse`, so it carries a job that has to be done — dropping it without doing that job leaves the work undone (an unjoined task, an unreleased lock)".to_string(),
        format!(
            "consume it exactly once: move it to a parameter with the move-capability marker `^`, or `return` it — or write `#{}(\"reason\") {{ consume({}) }}` to discard it deliberately",
            Syntax::KW_UNSAFE, name
        ),
        Some(span),
    )
}

/// Same duty voice as [`e0140_unconsumed`], for the `_ :: value` discard case
/// (D-FACT-WORD1=A): binding a `#SingleUse` value to `_` skips its job before
/// it's done.
pub(crate) fn e0140_discarded_wildcard(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0140",
        "this value still owes `consume`".to_string(),
        "this value's type is `#SingleUse`, so it carries a job that has to be done — discarding it into `_` skips that job before it's done".to_string(),
        format!(
            "bind it to a name, then consume it exactly once — or write `#{}(\"reason\") {{ consume(...) }}` to discard it deliberately",
            Syntax::KW_UNSAFE
        ),
        Some(span),
    )
}

/// D-CONC-JOIN1 / D-FACT-WORD1=A: one duty voice for every task-owes-join
/// site. `#SingleUse`'s duty and a task handle's duty are the same law
/// — the value still owes `join`, and `.detach()` is the one written word that
/// lets it go free.
pub(crate) fn l1101_unjoined_task(subject: &str, why: &str, span: Span) -> Diagnostic {
    Diagnostic::lint(
        "L1101",
        format!("{subject} still owes `join`"),
        why.to_string(),
        "join it with `.join()`, or write `.detach()` to let it go free".to_string(),
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
            "move it with the move-capability marker `^` (`{}{}`) to give it away, or rework the call so it takes ownership through that marker",
            Syntax::SIGIL_MOVE,
            name,
        ),
        Some(span),
    )
}
