use super::*;
use crate::AST::{
    AccessConvention,
    Expr, Lambda, Type, VariantPayload,
};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::is_type_var_name;
use crate::Syntax;
use std::collections::{HashMap, HashSet};

impl<'a> Checker<'a> {
    /// Whether `e` may be returned through `-> view T` (reference-safe).
    pub(crate) fn expr_ok_for_view_return(&self, e: &Expr) -> bool {
        match e {
            Expr::Ident(name, _) => {
                if self.consts.contains_key(name) {
                    return true;
                }
                if let Some(info) = self.lookup(name) {
                    return info.ty.is_scalar() || info.param_conv.is_some();
                }
                false
            }
            // E2-M5 (generic / zero-copy cell): a `view` may point *into* a
            // field of something the caller already owns — a parameter (incl.
            // a generic-typed one) or a const. The caller keeps owning the
            // root for as long as the returned view lives, so the borrow of a
            // stored field is sound. A field path rooted at a *local* is the
            // E2301 case (handled by `view_return_local_owner`, not here).
            //
            // Index/slice are deliberately *not* here: the list/string slice
            // helpers build a fresh owned value, so handing one back as a view
            // would borrow a temporary. Those land in E2304, below.
            Expr::Field(..) => {
                let Some(root) = expr_root_ident(e) else {
                    return false;
                };
                if self.consts.contains_key(root) {
                    return true;
                }
                match self.lookup(root) {
                    Some(info) => info.param_conv.is_some(),
                    None => false,
                }
            }
            _ => false,
        }
    }

    /// If `e` reads into a *field* (or index/slice) of a function-local value,
    /// return the owning local's name. A view into that field would outlive the
    /// owner — the E2301 ("what owns this?") case. Returns `None` when the root
    /// is a parameter or const (the caller owns it; that source outlives the
    /// call) or when `e` isn't a field/index access at all.
    pub(crate) fn view_return_local_owner(&self, e: &Expr) -> Option<String> {
        // Only field / index / slice access can borrow *into* an owner.
        if !matches!(
            e,
            Expr::Field(..) | Expr::Index { .. } | Expr::Slice { .. }
        ) {
            return None;
        }
        let root = expr_root_ident(e)?;
        if self.consts.contains_key(root) {
            return None;
        }
        let info = self.lookup(root)?;
        // Parameters are owned by the caller and outlive the call.
        if info.param_conv.is_some() {
            return None;
        }
        Some(root.to_string())
    }

    /// E2302: a struct literal that fills a `ref` field from a source that
    /// won't outlive the struct stores a view that would dangle. Read-only —
    /// `check_struct_lit` owns the literal's own elaboration; this only
    /// inspects the already-inferred init expression at its binding site.
    pub(crate) fn check_stored_ref_fields(&mut self, init: &Expr) {
        let Expr::StructLit {
            type_name,
            import_ns,
            fields,
            ..
        } = init
        else {
            return;
        };
        let Some(owner_mod) = self.struct_owner_module(type_name, import_ns.as_deref()) else {
            return;
        };
        let ref_fields: Vec<String> = match self.struct_fields_of(owner_mod, type_name) {
            Some(defs) => defs
                .iter()
                .filter(|(_, _, _, is_ref, _)| *is_ref)
                .map(|(n, ..)| n.clone())
                .collect(),
            None => return,
        };
        if ref_fields.is_empty() {
            return;
        }
        for (fname, fspan, fexpr) in fields {
            if !ref_fields.contains(fname) {
                continue;
            }
            if let Some(why_short) = self.ref_source_dangles(fexpr) {
                self.diags.push(Diagnostic::error(
                    "E2302",
                    format!(
                        "the `ref` field `{}` would point at something that dies first",
                        fname
                    ),
                    format!(
                        "a `ref` field stores a view, not its own copy, so its source has to outlive the struct — but {} doesn't live long enough to promise that here",
                        why_short
                    ),
                    "store an owned value: drop `ref` so the struct keeps its own copy (or `.clone()` into it)".to_string(),
                    Some(*fspan),
                ));
            }
        }
    }

    /// If the expression filling a `ref` field won't *provably* outlive the
    /// struct, return a short noun phrase describing the source (for the E2302
    /// *why*). `None` means the source is `'static` (a const), the only thing a
    /// stored `ref` can safely point at in v1.
    ///
    /// E2-M5 soundness note: a parameter outlives the *call*, but the struct it
    /// fills can be returned or stored past the call, and the generated Rust
    /// struct has no lifetime to name that borrow against. There is no sound
    /// lowering for a `ref` field bound to a parameter or local without arenas
    /// (D-REF2, OPEN). So only a const source survives; everything else is
    /// rejected here rather than handed to rustc as an ICE (I2).
    pub(crate) fn ref_source_dangles(&self, e: &Expr) -> Option<String> {
        match e {
            // A fresh literal has no owner at all that outlives the struct.
            Expr::Str(..) => Some("freshly made text".to_string()),
            // A `'static` const is the one source a stored `ref` may point at.
            Expr::Ident(name, _) => {
                if self.consts.contains_key(name) {
                    return None;
                }
                let info = self.lookup(name)?;
                if info.param_conv.is_some() {
                    Some(format!("the borrowed `{}`", name))
                } else {
                    Some(format!("the local `{}`", name))
                }
            }
            Expr::Field(..) | Expr::Index { .. } | Expr::Slice { .. } => {
                let root = expr_root_ident(e)?;
                if self.consts.contains_key(root) {
                    return None;
                }
                let info = self.lookup(root)?;
                if info.param_conv.is_some() {
                    Some(format!("the borrowed `{}`", root))
                } else {
                    Some(format!("the local `{}`", root))
                }
            }
            // Elaboration may wrap a `ref` field's source in an auto `.clone()`
            // (record-literal path). A clone produces a fresh owned value, so a
            // `ref` (which needs a borrow) can't be filled from it — look
            // through to the receiver so we still name the real source.
            Expr::MethodCall { receiver, .. } => self.ref_source_dangles(receiver),
            // Anything else computed here (a call result, an operator, a fresh
            // collection) is a temporary with no lifetime to name — there is no
            // sound `ref`-field lowering for it in v1 (arenas, D-REF2, OPEN).
            _ => Some("a value computed here".to_string()),
        }
    }

    // ──────────────────────────────────────────────────────────────────────
    // D-ALLOC2 / D-REGION1 (ratified 2026-06-21): scope-bound arena `view`s.
    //
    // `x :: arena.alloc(v)` makes `x` a *view* into `arena`'s storage — Rust
    // `&'arena mut T`. The view is sound only while it stays inside its region
    // (the lexical scope of the `arena` binding / an explicit `region`) and only
    // until `arena` is `reset`/`free`d. Two diagnostics enforce that, both
    // *strictly* at least as strict as Rust's borrow checker, so every
    // Jet-accepted program is rustc-accepted (I2: Jet rejects first):
    //   * E0631 — the view escapes its region (returned, stored in a binding /
    //     ref / struct field, passed where ownership/`mut` is taken, captured by
    //     an escaping closure).
    //   * E0632 — the view is used after the backing arena was `reset`/`free`d.
    //
    // v1 restriction (I8): views are non-reassignable, non-escaping locals; we
    // reject anything the analysis can't prove with a teaching error rather than
    // attempt a clever lowering.
    // ──────────────────────────────────────────────────────────────────────

    /// If `init` is `arena.alloc(value)` on a name, return the arena's name.
    pub(crate) fn arena_alloc_source(&self, init: &Expr) -> Option<String> {
        if let Expr::MethodCall { receiver, method, .. } = init {
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
    pub(crate) fn record_arena_view(&mut self, name: &str, arena: String) {
        let scope_len = self.scopes.len();
        self.arena_views.insert(
            name.to_string(),
            ArenaViewInfo { arena, scope_len, dead: None },
        );
    }

    /// E0632: when `arena` is `reset`/`free`d, every live view into it dies.
    pub(crate) fn kill_views_of_arena(&mut self, arena: &str, verb: &str, span: Span) {
        for v in self.arena_views.values_mut() {
            if v.arena == arena && v.dead.is_none() {
                v.dead = Some((verb.to_string(), span));
            }
        }
    }

    /// E0632: reading a view whose arena was already `reset`/`free`d.
    pub(crate) fn check_view_use(&mut self, name: &str, span: Span) {
        if let Some(info) = self.arena_views.get(name) {
            if let Some((verb, _kill_span)) = &info.dead {
                let arena = info.arena.clone();
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
        let arena = self
            .arena_views
            .get(name)
            .map(|v| v.arena.clone())
            .unwrap_or_else(|| "its arena".to_string());
        self.diags.push(Diagnostic::error(
            "E0631",
            format!("`{}` cannot be shared — it does not live long enough to {}", name, what),
            format!(
                "`{}` is a view into `{}`; sharing it outside the region would let it outlive `{}` and point into freed memory",
                name, arena, arena
            ),
            format!(
                "keep `{}` inside the `{}` region, or copy what you need out with `.clone()` before it leaves",
                name, arena
            ),
            Some(span),
        ));
    }

    /// True if `name` is currently a live arena view. Used to gate escape checks
    /// at the use sites (return / bind / move-arg / struct field).
    pub(crate) fn is_arena_view(&self, name: &str) -> bool {
        self.arena_views.contains_key(name)
    }

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
                if mods[idx].type_pub.get(name).copied().unwrap_or(false)
                    && mods[idx].registry.is_single_use(name)
                {
                    return true;
                }
            }
        }
        false
    }

    pub(crate) fn mark_moved(&mut self, name: String, span: Span) {
        if let Some(info) = self.lookup(&name) {
            if info.decl_loop_depth < self.loop_depth {
                self.diags.push(Diagnostic::error(
                    "E0121",
                    format!("`{}` is given away inside a loop that may run again", name),
                    "after a value is given away it's gone, but the next time around the loop would need it again".to_string(),
                    format!("give away a copy instead: `{}.clone()`", name),
                    Some(span),
                ));
                return;
            }
        }
        self.moved.insert(name, span);
    }

    /// `x = y;` / `val a = y;` / `return y;` where `y` is a plain name of a
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

    pub(crate) fn sendability_problem(&self, ty: &Type, closure_taken: bool) -> Option<SendabilityProblem> {
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
            Type::Map { key, value } => self
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
            Type::Named(name) if is_type_var_name(name) || core_type_known(name) => None,
            Type::Named(name) => self.named_sendability_problem(name, &[], seen),
            Type::Apply { name, args }
                if matches!(name.as_str(), "Task" | "Channel" | "Sender") =>
            {
                args.iter()
                    .find_map(|arg| self.sendability_problem_inner(arg, true, seen))
            }
            Type::Apply { name, args } => self.named_sendability_problem(name, args, seen),
            Type::TraitObject(name) => Some(SendabilityProblem {
                root: None,
                path: Vec::new(),
                kind: SendProblemKind::TraitValue(name.clone()),
            }),
            Type::Tuple(fields) => fields.iter().find_map(|(_, t)| {
                self.sendability_problem_inner(t, true, seen)
            }),
            Type::FixedList { elem, .. } => self.sendability_problem_inner(elem, true, seen),
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
                for (field_name, _, field_ty, is_ref, _) in fields {
                    if *is_ref {
                        return Some(SendabilityProblem {
                            root: Some(name.to_string()),
                            path: vec![field_name.clone()],
                            kind: SendProblemKind::RefField,
                        });
                    }
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
            Some(TypeDef::Distinct { .. }) | None => None,
        };
        seen.remove(name);
        found
    }

    pub(crate) fn expr_sendability_problem(
        &self,
        expr: &Expr,
        ty: &Type,
        closure_taken: bool,
        view_borrow: bool,
    ) -> Option<SendabilityProblem> {
        if view_borrow {
            return Some(SendabilityProblem {
                root: None,
                path: Vec::new(),
                kind: SendProblemKind::ViewBorrow,
            });
        }
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
                "send plain owned data instead, or rebuild the value without shared-view fields before calling `.send()`"
            }
            SendCrossing::TaskCapture | SendCrossing::TaskResult => {
                "give the task plain owned data, or remove the shared-view field before spawning"
            }
        };
        // D-DETACH1: if this E1102 fires in a task spawn context, record the task
        // binding name so the right detach diagnostic fires when `.detach()` is called:
        //   - ViewBorrow → E1106 ("pass an owned copy/share"); view can outlive the borrow
        //   - other sendability failures → E1103 (general unsound-detach)
        if matches!(crossing, SendCrossing::TaskCapture | SendCrossing::TaskResult) {
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
                if self.is_arena_view(name) {
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
            // D-CAP8/9: Infer follows Read (default pre-resolution); Share/Raw aren't
            // produced yet — ownership specializes them when their phases land.
            AccessConvention::Read
            | AccessConvention::Infer
            | AccessConvention::Share
            | AccessConvention::Raw => {
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
                                format!("`{}` expects to take ownership of this value", call_name),
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
        let view_borrow = self.is_view_call(&arg.expr);
        let saved_exp = self.expected_type.clone();
        self.expected_type = Some(elem_ty.clone());
        if view_borrow {
            self.borrow_ctx = true;
        }
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
                view_borrow,
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
            }
        }
        if !sendability_failed {
            self.check_take_arg_ownership("send", 0, &elem_ty, arg);
        }
        None
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
