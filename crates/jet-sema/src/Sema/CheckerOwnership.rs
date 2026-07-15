use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::is_type_var_name;
use crate::Syntax;
use crate::AST::{AccessConvention, Expr, Lambda, Type, VariantPayload};
use std::collections::{HashMap, HashSet};

impl<'a> Checker<'a> {
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
    pub(crate) fn record_arena_view(&mut self, name: &str, arena: String) {
        let scope_len = self.scopes.len();
        self.arena_views.insert(
            name.to_string(),
            ArenaViewInfo {
                arena,
                scope_len,
                dead: None,
            },
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
                "keep `{}` inside the `{}` region, or copy what you need out with `{}` before it leaves",
                name, arena, Syntax::SIGIL_COPY
            ),
            Some(span),
        ));
    }

    /// True if `name` is currently a live arena view. Used to gate escape checks
    /// at the use sites (return / bind / move-arg / struct field).
    pub(crate) fn is_arena_view(&self, name: &str) -> bool {
        self.arena_views.contains_key(name)
    }

    // ──────────────────────────────────────────────────────────────────────
    // D-DYNARRAY1 (ratified 2026-07-01): `View<T>` zero-copy windows
    // (`list.view(a..b)`).
    //
    // A view's Rust value is a genuine borrowed slice (`&[T]`, elided
    // lifetime — see `Context::rust_type`'s `View` arm) so it is exactly as
    // sound as an arena view; the tracking below is the same shape as
    // `arena_views` (E0631/E0632, above) reusing its *reasoning*: the owner
    // list is made and freed inside this function, so a view into it can't
    // outlive that scope. Kept as a separate map (not a refactor of
    // `arena_views`) so the well-tested arena mechanism's wording/drop-
    // tracking stays untouched — I8's "one owner-tracking shape," not one
    // shared struct.
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
    pub(crate) fn view_call_source(&self, init: &Expr) -> Option<String> {
        let Expr::MethodCall {
            receiver, method, ..
        } = init
        else {
            return None;
        };
        if method != Syntax::METHOD_VIEW {
            return None;
        }
        let Expr::Ident(name, _) = receiver.as_ref() else {
            return None;
        };
        if self.consts.contains_key(name) {
            return None;
        }
        if let Some(info) = self.lookup(name) {
            if info.param_conv.is_some() {
                return None;
            }
        }
        Some(name.clone())
    }

    /// Record `name` as a view into `owner`, declared at the current scope depth.
    pub(crate) fn record_list_view(&mut self, name: &str, owner: String) {
        let scope_len = self.scopes.len();
        self.list_views
            .insert(name.to_string(), ListViewInfo { owner, scope_len });
    }

    /// True if `name` is currently a live `View<T>` binding.
    pub(crate) fn is_list_view(&self, name: &str) -> bool {
        self.list_views.contains_key(name)
    }

    /// E2305: a `View<T>` named `name` is escaping the scope of the list it
    /// borrows from. `what` describes the escape site for the message.
    pub(crate) fn report_list_view_escape(&mut self, name: &str, what: &str, span: Span) {
        let owner = self
            .list_views
            .get(name)
            .map(|v| v.owner.clone())
            .unwrap_or_else(|| "its owner".to_string());
        self.diags.push(Diagnostic::error(
            "E2305",
            format!(
                "`{}` cannot be shared — it does not live long enough to {}",
                name, what
            ),
            format!(
                "`{}` is a view into `{}` (`.view(...)`); sharing it outside `{}`'s scope would let it outlive the list and point into freed memory",
                name, owner, owner
            ),
            format!(
                "keep `{}` inside `{}`'s scope, or copy what you need with `.map(...)` or indexing before it leaves",
                name, owner
            ),
            Some(span),
        ));
    }

    /// E2305: `return list.view(a..b)` made fresh right in the `return` — `owner`
    /// (`list`) is made in this function and freed when it returns. Mirrors
    /// E2301's exact wording (`this view points into X, which this function
    /// owns`) for the "fresh call in return" shape; `report_list_view_escape`
    /// above covers the "already-bound name" shape.
    pub(crate) fn report_view_owns_return(&mut self, owner: &str, span: Span) {
        self.diags.push(Diagnostic::error(
            "E2305",
            format!(
                "this view points into `{}`, which this function owns",
                owner
            ),
            format!(
                "`{}` is made here and freed when the function returns, so a view into it (`.view(...)`) would outlive what owns it — there'd be nothing left to look at",
                owner
            ),
            "return an owned copy (drop `.view(...)` for a copying slice `[a..b]`, or `.map(...)` it into an owned list), or accept the source as a parameter so the caller keeps owning it".to_string(),
            Some(span),
        ));
    }

    // ──────────────────────────────────────────────────────────────────────
    // D-MEM1 stage S5 (2026-07-04): string `view`s — `s.trim()` / `s.after(sep)`
    // / `s.before(sep)` bound to a local name return a zero-copy `&str` window
    // into `s` instead of an owned `String`. Unlike `View<T>` (a distinct Jet
    // type), `String` stays ONE type end to end (D-MEM1 gallery: "one String
    // type") — so the view-ness lives on the *binding* (`Binding::string_view`,
    // set below), not on the value's static type. Tracking mirrors `list_views`
    // exactly (same shape, same E-code family): E2307 fires on escape
    // (returned, rebound, stored in a struct field); crossing a task boundary
    // is caught separately at the capture-check site (`CheckerInfer/calls.rs`)
    // since a plain `Type::String` carries no view marker for the general
    // sendability check to key off.
    // ──────────────────────────────────────────────────────────────────────

    /// If `init` is `s.trim()` / `s.after(sep)` / `s.before(sep)` on a plain
    /// local `String` name, return `s`'s name — but only when `s` is a genuine
    /// local (dies at this function's return/scope exit). A parameter or const
    /// outlives the call, so a view into it is sound without tracking (mirrors
    /// `view_call_source`'s exemption).
    pub(crate) fn string_view_call_source(&self, init: &Expr) -> Option<String> {
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
        if info.param_conv.is_some() {
            return None;
        }
        Some(name.clone())
    }

    /// Record `name` as a string view into `owner`, declared at the current
    /// scope depth.
    pub(crate) fn record_string_view(&mut self, name: &str, owner: String) {
        let scope_len = self.scopes.len();
        self.string_views
            .insert(name.to_string(), ListViewInfo { owner, scope_len });
    }

    /// True if `name` is currently a live string-view binding.
    pub(crate) fn is_string_view(&self, name: &str) -> bool {
        self.string_views.contains_key(name)
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
            .string_views
            .get(name)
            .map(|v| v.owner.clone())
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

    /// D-MUSTUSE1 (c18iwxqx): true when `ty` is a `@MustUse` struct/enum or a
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

    /// D-MUSTUSE1 (c18iwxqx): name of a `@MustUse` fn/method call when `expr` is
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
            "a `@MustUse` result carries work or a resource that is lost when nothing checks it"
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

    pub(crate) fn mark_moved(&mut self, name: String, span: Span) {
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
        self.moved.insert(name, span);
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
            Type::Apply { name, .. } if name == "View" => Some(SendabilityProblem {
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
            AccessConvention::Read => {
                if let Expr::Ident(name, span) = &arg.expr {
                    if is_cloneable(param_ty, self.registry) {
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
        self.finish_shared_closure("read", inner, args, span, false)
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
        self.finish_shared_closure("edit", inner, args, span, true)
    }

    fn finish_shared_closure(
        &mut self,
        method: &str,
        inner: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
        param_mutable: bool,
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
        let saved_esc = self.lambda_escapes;
        self.lambda_escapes = false;
        let fn_ty = self.infer(&mut args[0].expr);
        self.lambda_escapes = saved_esc;
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
