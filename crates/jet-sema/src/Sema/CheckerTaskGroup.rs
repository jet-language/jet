use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{CallArg, Expr, Stmt};
use std::collections::HashSet;

/// A child task spawned inside the active `taskgroup` scope.
pub(crate) struct PendingTaskSpawn {
    pub binding: Option<String>,
    pub span: Span,
    pub consumed: bool,
}

/// D-TASKBORROW1=A: one borrowed place a child of this group holds. Loans open
/// before the child launches and close when the group joins.
pub(crate) struct ScopedBorrow {
    pub name: String,
    pub place: ViewPlace,
    pub access: ViewAccess,
}

pub(crate) struct TaskGroupCtx {
    pub name: String,
    pub origin: TaskGroupOrigin,
    pub pending: Vec<PendingTaskSpawn>,
    /// Borrowed places already lent to earlier children of this group.
    pub borrows: Vec<ScopedBorrow>,
    /// Where this group's handle is declared. A borrowed owner declared after
    /// this point lives inside the group's own scope and would be dropped
    /// before the group joins, so it can never be lent to a child.
    pub handle_span: Span,
    synth_counter: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskGroupOrigin {
    Lexical,
    Parameter,
}

impl TaskGroupCtx {
    pub(crate) fn new(name: String, handle_span: Span) -> Self {
        Self {
            name,
            origin: TaskGroupOrigin::Lexical,
            pending: Vec::new(),
            borrows: Vec::new(),
            handle_span,
            synth_counter: 0,
        }
    }

    pub(crate) fn parameter(name: String) -> Self {
        Self {
            name,
            origin: TaskGroupOrigin::Parameter,
            pending: Vec::new(),
            borrows: Vec::new(),
            handle_span: Span::new(0, 0),
            synth_counter: 0,
        }
    }

    fn next_synth(&mut self) -> String {
        let n = self.synth_counter;
        self.synth_counter += 1;
        format!("__jet_tg_{n}")
    }
}

impl<'a> Checker<'a> {
    pub(crate) fn active_taskgroup(&self) -> Option<&TaskGroupCtx> {
        self.taskgroup_stack.last()
    }

    pub(crate) fn active_taskgroup_name(&self) -> Option<&str> {
        self.taskgroup_stack
            .iter()
            .rev()
            .find(|g| g.origin == TaskGroupOrigin::Lexical)
            .map(|g| g.name.as_str())
    }

    pub(crate) fn taskgroup_receiver_ok(&mut self, receiver: &Expr, span: Span) -> bool {
        let Expr::Ident(name, rspan) = receiver else {
            self.diags.push(Diagnostic::error(
                "E1110",
                "`.task => …` must be called on a taskgroup handle".to_string(),
                "structured spawning goes through a lexical taskgroup or a `TaskGroup` parameter"
                    .to_string(),
                "write `g.task => …` where `g` is a taskgroup name or parameter".to_string(),
                Some(span),
            ));
            return false;
        };
        if self.taskgroup_stack.iter().rev().any(|group| {
            group.name == *name
                && (group.origin == TaskGroupOrigin::Parameter
                    || self.active_taskgroup_name() == Some(name.as_str()))
        }) {
            return true;
        }
        if let Some(active) = self.active_taskgroup_name() {
            self.diags.push(Diagnostic::error(
                "E1110",
                format!(
                    "`.task` must be called on the active taskgroup handle `{}`, not `{}`",
                    active, name
                ),
                "each lexical `taskgroup` block owns spawns on its bound handle; a helper can instead receive `TaskGroup` as a parameter".to_string(),
                format!(
                    "write `{active}.task => …`, or pass `{active}` to `fn helper(group: TaskGroup)`"
                ),
                Some(*rspan),
            ));
        } else {
            self.diags.push(Diagnostic::error(
                "E1110",
                "`.task => …` needs a taskgroup handle".to_string(),
                "structured spawning is scoped to a lexical taskgroup or a `TaskGroup` parameter"
                    .to_string(),
                "wrap the call in `taskgroup g { … }`, or add `group: TaskGroup` to this helper"
                    .to_string(),
                Some(*rspan),
            ));
        }
        false
    }

    /// True when `name` is a `&place` write borrow.
    pub(crate) fn is_write_borrow(&self, name: &str) -> bool {
        self.view_fact(name)
            .is_some_and(|fact| matches!(fact.access, ViewAccess::Write))
    }

    /// D-TASKBORROW1=A: admit a borrowed capture in a `taskgroup` child.
    ///
    /// Reads are admitted freely; a write is admitted only when its place is
    /// provably disjoint from every place a sibling already holds. A group can
    /// only lend what outlives its own join, so two shapes are never lent:
    /// a group reached through a `TaskGroup` parameter (its join runs in
    /// another frame) and an owner declared inside the group's own block (it
    /// drops before the group joins).
    ///
    /// `None` means this capture is not a borrowed place the group can lend, so
    /// the caller keeps its ordinary ownership rules. `Some(false)` means the
    /// borrow was rejected and a diagnostic was reported.
    pub(crate) fn admit_scoped_borrow(
        &mut self,
        name: &str,
        fallback: Option<ViewAccess>,
        span: Span,
    ) -> Option<bool> {
        let Some(active) = self.taskgroup_stack.last() else {
            return None;
        };
        if active.origin == TaskGroupOrigin::Parameter {
            return None;
        }
        let handle_start = active.handle_span.start;
        let (place, access) = match self.view_fact(name) {
            Some(fact) => (fact.place.clone(), fact.access),
            // A borrowed parameter is stack data the caller still owns. It has
            // no projection fact, so the whole binding is the borrowed place.
            None => match (fallback, self.lookup(name)) {
                (Some(access), Some(info)) => (
                    ViewPlace {
                        owner: ViewOwnerId {
                            name: name.to_string(),
                            def_span: info.def_span,
                            origin: ViewOwnerOrigin::Local,
                        },
                        projections: Vec::new(),
                    },
                    access,
                ),
                _ => return None,
            },
        };
        // The owner must already exist where the group handle is declared.
        // Anything created inside the block drops before the group joins.
        if place.owner.def_span.start > handle_start {
            return None;
        }
        // Every live group lends at the same time, so an inner group's child
        // races an outer group's child just as two siblings do.
        let conflict = self
            .taskgroup_stack
            .iter()
            .flat_map(|group| group.borrows.iter())
            .find(|held| {
                (matches!(access, ViewAccess::Write) || matches!(held.access, ViewAccess::Write))
                    && held.place.overlaps(&place)
            })
            .map(|held| held.name.clone());
        if let Some(other) = conflict {
            let what = if other == name {
                format!("`{name}` is already lent to another task in this group")
            } else {
                format!("`{name}` and `{other}` can reach the same place at the same time")
            };
            self.diags.push(Diagnostic::error(
                "E1101",
                what,
                "a taskgroup runs its children at the same time, so two children may borrow one place only when the compiler can prove the places never overlap"
                    .to_string(),
                "borrow separate fields or constant indexes, give each task its own owned copy, or send results back through a channel"
                    .to_string(),
                Some(span),
            ));
            return Some(false);
        }
        if let Some(group) = self.taskgroup_stack.last_mut() {
            group.borrows.push(ScopedBorrow {
                name: name.to_string(),
                place,
                access,
            });
        }
        Some(true)
    }

    /// D-TASKBORROW1=A: a loan to a taskgroup child opens before the child
    /// launches and closes only when the group joins. Until then the parent may
    /// not move, drop, or write the lent place.
    ///
    /// The ordinary view rules cannot see this. They end a borrow at the view
    /// binding's last lexical use, which is right, but a child holds its loan
    /// past that point — it is still running. `spawn_scoped` erases the lifetime
    /// inside a vetted-unsafe region, so rustc will not catch it either (I1).
    ///
    /// Returns true when a conflict was reported.
    pub(crate) fn report_scoped_loan_conflict(
        &mut self,
        changed: &ViewPlace,
        action: &str,
        span: Span,
    ) -> bool {
        // Inside the child's own body the loan is what makes the access legal.
        if self.in_taskgroup_spawn {
            return false;
        }
        let Some((lent, group)) = self.taskgroup_stack.iter().find_map(|group| {
            group
                .borrows
                .iter()
                .find(|held| held.place.overlaps(changed))
                .map(|held| (held.name.clone(), group.name.clone()))
        }) else {
            return false;
        };
        let changed_name = Self::place_name(changed);
        self.diags.push(Diagnostic::error(
            "E1101",
            format!("`{changed_name}` cannot {action} while `{lent}` is lent to a task in `{group}`"),
            format!(
                "a taskgroup joins its children at the end of the block, so `{lent}` stays borrowed until `{group}` joins — changing `{changed_name}` now would race a running task or free memory it still reads"
            ),
            format!(
                "do this after the `{group}` block ends, or give the task its own owned copy instead of a borrow"
            ),
            Some(span),
        ));
        true
    }

    /// A child holding a WRITE loan runs at the same time as the parent, so a
    /// parent read of that place races the child's write. Sequentially a read
    /// beside a live write view is fine — nothing runs concurrently — which is
    /// why this rule belongs to taskgroups and not to the general view model.
    pub(crate) fn check_scoped_loan_read(&mut self, expr: &Expr) {
        if self.scoped_loan_read_reported || self.in_taskgroup_spawn {
            return;
        }
        // Cheap early-out: almost no statement runs under a live write loan.
        if !self
            .taskgroup_stack
            .iter()
            .any(|group| group.borrows.iter().any(|held| held.access == ViewAccess::Write))
        {
            return;
        }
        if !matches!(expr, Expr::Index { .. } | Expr::Field(..)) {
            return;
        }
        let Some(place) = self.place_from_expr(expr) else {
            return;
        };
        let Some((lent, group)) = self.taskgroup_stack.iter().find_map(|group| {
            group
                .borrows
                .iter()
                .find(|held| held.access == ViewAccess::Write && held.place.overlaps(&place))
                .map(|held| (held.name.clone(), group.name.clone()))
        }) else {
            return;
        };
        self.scoped_loan_read_reported = true;
        let read_name = Self::place_name(&place);
        self.diags.push(Diagnostic::error(
            "E1101",
            format!("`{read_name}` cannot be read while `{lent}` is lent to a task in `{group}`"),
            format!(
                "`{lent}` is an exclusive write borrow held by a running child, so reading `{read_name}` here would race that task's writes"
            ),
            format!(
                "read `{read_name}` after the `{group}` block ends, or have the task send the value back through a channel"
            ),
            Some(expr.span()),
        ));
    }

    pub(crate) fn register_taskgroup_spawn(
        &mut self,
        receiver: &str,
        binding: Option<String>,
        span: Span,
    ) {
        if let Some(ctx) = self
            .taskgroup_stack
            .iter_mut()
            .rev()
            .find(|group| group.name == receiver)
        {
            ctx.pending.push(PendingTaskSpawn {
                binding,
                span,
                consumed: false,
            });
        }
    }

    pub(crate) fn mark_taskgroup_spawn_consumed(&mut self, name: &str) {
        for ctx in self.taskgroup_stack.iter_mut().rev() {
            for p in ctx.pending.iter_mut().rev() {
                if p.binding.as_deref() == Some(name) {
                    p.consumed = true;
                    return;
                }
            }
        }
    }

    pub(crate) fn taskgroup_spawn_from_expr(expr: &Expr) -> Option<(&Expr, Span)> {
        match expr {
            Expr::MethodCall {
                receiver,
                method,
                method_span,
                ..
            } if method == Syntax::TASKGROUP_SPAWN_METHOD => Some((receiver, *method_span)),
            _ => None,
        }
    }

    pub(crate) fn rewrite_anonymous_taskgroup_spawn(&mut self, stmt: &mut Stmt) -> bool {
        let Stmt::Expr(expr) = stmt else {
            return false;
        };
        let (receiver, mspan) = match Self::taskgroup_spawn_from_expr(expr) {
            Some(v) => v,
            None => return false,
        };
        if self.active_taskgroup().is_none() {
            return false;
        }
        if !self.taskgroup_receiver_ok(receiver, mspan) {
            let _ = self.infer(expr);
            return false;
        }
        let synth = self
            .taskgroup_stack
            .last_mut()
            .expect("checked above")
            .next_synth()
            .clone();
        let span = expr.span();
        let init = std::mem::replace(
            expr,
            Expr::Int(0, span, None, None), // placeholder; replaced below
        );
        *stmt = Stmt::Val(crate::AST::Binding {
            mutable: false,
            markers: Vec::new(),
                reactive_upgrade: false,
            meta: None,
            name: synth.clone(),
            name_span: span,
            pattern: None,
            ty: None,
            ty_span: None,
            init,
            is_comptime: false,
            ct: None,
            uninit: false,
            arena_view: false,
                string_view: false,
                gc_promotion: None,
                gc_transferred: false,
        });
        true
    }

    pub(crate) fn append_taskgroup_auto_joins(&mut self, body: &mut Vec<Stmt>) {
        let Some(ctx) = self.taskgroup_stack.last() else {
            return;
        };
        for spawn in &ctx.pending {
            if spawn.consumed {
                continue;
            }
            let Some(name) = spawn.binding.clone() else {
                continue;
            };
            body.push(Stmt::Expr(cancel_call(&name, spawn.span)));
            body.push(Stmt::Expr(join_call(&name, spawn.span)));
            self.flow.moved.set(&name, spawn.span);
        }
    }

    pub(crate) fn infer_taskgroup_method(
        &mut self,
        receiver: &mut Box<Expr>,
        method: &str,
        span: Span,
        args: &mut Vec<CallArg>,
        recv_type_out: &mut Option<String>,
    ) -> Option<Type> {
        if !self.taskgroup_receiver_ok(receiver, span) {
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            *recv_type_out = Some(Syntax::TYPE_TASKGROUP.to_string());
            return None;
        }
        *recv_type_out = Some(Syntax::TYPE_TASKGROUP.to_string());
        let receiver_name = match receiver.as_ref() {
            Expr::Ident(name, _) => name.clone(),
            _ => unreachable!("taskgroup_receiver_ok accepted a non-identifier"),
        };
        match method {
            Syntax::TASKGROUP_SPAWN_METHOD => {
                self.infer_taskgroup_spawn(&receiver_name, args, span)
            }
            Syntax::TASKGROUP_ALL_METHOD => self.infer_taskgroup_all(args, span),
            Syntax::TASKGROUP_RACE_METHOD => self.infer_taskgroup_race(args, span),
            Syntax::TASKGROUP_ANY_METHOD => self.infer_taskgroup_any(args, span),
            Syntax::TASKGROUP_SELECT_METHOD => self.infer_taskgroup_select(args, span),
            other => {
                self.diags.push(Diagnostic::error(
                    "E0102",
                    format!("`TaskGroup` has no method `{other}`"),
                    "structured taskgroups support `.task => …`, `.all([…])`, `.race([…])`, `.any([…])`, and `.select()`"
                        .to_string(),
                    "write `g.task => work()`, `g.all([h1, h2])`, or `g.select().recv(ch).wait()`".to_string(),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                None
            }
        }
    }

    fn infer_taskgroup_spawn(
        &mut self,
        receiver: &str,
        args: &mut Vec<CallArg>,
        span: Span,
    ) -> Option<Type> {
        if args.len() != 1 {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`.task => …` takes one body, got {} argument{}",
                    args.len(),
                    if args.len() == 1 { "" } else { "s" }
                ),
                "a scoped task runs the `{ … }` body on a worker thread".to_string(),
                "write `g.task => your_work()`".to_string(),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        let saved_esc = self.lambda_escapes;
        let saved_task = self.is_task_spawn;
        let saved_tg = self.in_taskgroup_spawn;
        self.lambda_escapes = true;
        self.is_task_spawn = true;
        self.in_taskgroup_spawn = true;
        let lam_ty = self.infer(&mut args[0].expr);
        self.lambda_escapes = saved_esc;
        self.is_task_spawn = saved_task;
        self.in_taskgroup_spawn = saved_tg;

        let t = match lam_ty {
            Some(Type::Fn { params, ret, .. }) => {
                if !params.is_empty() {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!(
                            "`.task => …` needs a zero-parameter body, got {} parameter{}",
                            params.len(),
                            if params.len() == 1 { "" } else { "s" }
                        ),
                        "a scoped task body captures values from the enclosing scope — it takes no parameters"
                            .to_string(),
                        "move data in via capture instead of lambda parameters".to_string(),
                        Some(args[0].expr.span()),
                    ));
                }
                ret.map(|r| *r)
                    .unwrap_or_else(|| Type::Named("Unit".to_string()))
            }
            Some(other) => {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`.task` needs a block body, not {}", other.show()),
                    "a scoped task runs a block on a worker thread".to_string(),
                    "write `g.task => your_work()`".to_string(),
                    Some(args[0].expr.span()),
                ));
                Type::Named("Unit".to_string())
            }
            None => Type::Named("Unit".to_string()),
        };
        if let Some(problem) = self.sendability_problem(&t, false) {
            self.report_unsendable(
                "task result",
                &t,
                problem,
                SendCrossing::TaskResult,
                args[0].expr.span(),
            );
        }
        let binding = self.current_binding_name.clone();
        self.register_taskgroup_spawn(receiver, binding, span);
        Some(Type::Apply {
            name: "Task".to_string(),
            args: vec![t],
        })
    }

    fn infer_taskgroup_all(&mut self, args: &mut Vec<CallArg>, span: Span) -> Option<Type> {
        if args.len() != 1 {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`.all()` takes one list of task handles, got {} argument{}",
                    args.len(),
                    if args.len() == 1 { "" } else { "s" }
                ),
                "`.all([h1, h2, …])` waits for every handle and returns the results in order"
                    .to_string(),
                "write `g.all([h1, h2])`".to_string(),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        let arg_ty = self.infer(&mut args[0].expr);
        let elem = match arg_ty {
            Some(Type::List(inner)) => match *inner {
                Type::Apply {
                    ref name, ref args, ..
                } if name == "Task" && args.len() == 1 => args[0].clone(),
                other => {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`.all()` needs a list of task handles, not `[{}]`",
                            other.show()
                        ),
                        "each element must be a `Task<T>` handle returned from `g.task => …`"
                            .to_string(),
                        "write `g.all([h1, h2])` where each handle came from `g.task`".to_string(),
                        Some(args[0].expr.span()),
                    ));
                    return None;
                }
            },
            Some(other) => {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!(
                        "`.all()` needs a list of task handles, not {}",
                        other.show()
                    ),
                    "pass a `[Task<T>]` list of handles from `g.task => …`".to_string(),
                    "write `g.all([h1, h2])`".to_string(),
                    Some(args[0].expr.span()),
                ));
                return None;
            }
            None => return None,
        };
        self.mark_taskgroup_all_consumed(&args[0].expr);
        Some(Type::List(Box::new(elem)))
    }

    fn infer_taskgroup_race(&mut self, args: &mut Vec<CallArg>, span: Span) -> Option<Type> {
        self.infer_taskgroup_first_task(
            args,
            span,
            "`.race()`",
            "`.race([h1, h2, …])` returns the first completed result",
        )
    }

    fn infer_taskgroup_any(&mut self, args: &mut Vec<CallArg>, span: Span) -> Option<Type> {
        self.infer_taskgroup_first_task(
            args,
            span,
            "`.any()`",
            "`.any([h1, h2, …])` returns the first completed result",
        )
    }

    fn infer_taskgroup_first_task(
        &mut self,
        args: &mut Vec<CallArg>,
        span: Span,
        method_label: &str,
        why: &str,
    ) -> Option<Type> {
        if args.len() != 1 {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "{method_label} takes one list of task handles, got {} argument{}",
                    args.len(),
                    if args.len() == 1 { "" } else { "s" }
                ),
                why.to_string(),
                "write `g.race([h1, h2])`".to_string(),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        let arg_ty = self.infer(&mut args[0].expr);
        let elem = match arg_ty {
            Some(Type::List(inner)) => match *inner {
                Type::Apply {
                    ref name, ref args, ..
                } if name == "Task" && args.len() == 1 => args[0].clone(),
                other => {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "{method_label} needs a list of task handles, not `[{}]`",
                            other.show()
                        ),
                        "each element must be a `Task<T>` handle returned from `g.task => …`"
                            .to_string(),
                        "write `g.race([h1, h2])` where each handle came from `g.task`".to_string(),
                        Some(args[0].expr.span()),
                    ));
                    return None;
                }
            },
            Some(other) => {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!(
                        "{method_label} needs a list of task handles, not {}",
                        other.show()
                    ),
                    "pass a `[Task<T>]` list of handles from `g.task => …`".to_string(),
                    "write `g.race([h1, h2])`".to_string(),
                    Some(args[0].expr.span()),
                ));
                return None;
            }
            None => return None,
        };
        self.mark_taskgroup_all_consumed(&args[0].expr);
        Some(elem)
    }

    fn mark_taskgroup_all_consumed(&mut self, expr: &Expr) {
        let mut names = HashSet::new();
        collect_task_idents(expr, &mut names);
        for name in names {
            self.mark_moved(name.clone(), expr.span());
            self.mark_taskgroup_spawn_consumed(&name);
        }
    }

    fn infer_taskgroup_select(&mut self, args: &mut Vec<CallArg>, span: Span) -> Option<Type> {
        if !args.is_empty() {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`.select()` takes no arguments, got {} argument{}",
                    args.len(),
                    if args.len() == 1 { "" } else { "s" }
                ),
                "scoped select starts an empty fluent builder — add arms with `.recv`, `.read`, or `.after`"
                    .to_string(),
                "write `g.select().recv(ch).wait()`".to_string(),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        Some(Type::Apply {
            name: Syntax::TYPE_SELECT_BUILDER.to_string(),
            args: vec![],
        })
    }

    #[allow(dead_code)]
    pub(crate) fn infer_select_method(
        &mut self,
        receiver: &mut Box<Expr>,
        method: &str,
        span: Span,
        args: &mut Vec<CallArg>,
        recv_type_out: &mut Option<String>,
    ) -> Option<Type> {
        let recv_ty = self.infer(receiver)?;
        let elem = match &recv_ty {
            Type::Apply { name, args, .. }
                if name == Syntax::TYPE_SELECT_BUILDER && args.len() == 1 =>
            {
                Some(args[0].clone())
            }
            Type::Named(n) if n == Syntax::TYPE_SELECT_BUILDER => None,
            _ => None,
        };
        *recv_type_out = Some(match &elem {
            Some(t) => format!("{}<{}>", Syntax::TYPE_SELECT_BUILDER, t.show()),
            None => Syntax::TYPE_SELECT_BUILDER.to_string(),
        });
        match method {
            Syntax::SELECT_RECV_METHOD => self.infer_select_recv(args, span, elem.as_ref()),
            Syntax::SELECT_AFTER_METHOD => self.infer_select_after(args, span, elem.as_ref()),
            Syntax::SELECT_READ_METHOD => self.infer_select_read(args, span, elem.as_ref()),
            Syntax::SELECT_WAIT_METHOD => self.infer_select_wait(args, span, elem.as_ref()),
            other => {
                self.diags.push(Diagnostic::error(
                    "E0102",
                    format!("`SelectBuilder` has no method `{other}`"),
                    "scoped select supports `.recv(ch)`, `.read(stream)`, `.after(ms: …)`, and `.wait()`"
                        .to_string(),
                    "write `g.select().recv(ch).wait()`".to_string(),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                None
            }
        }
    }

    #[allow(dead_code)]
    fn infer_select_recv(
        &mut self,
        args: &mut Vec<CallArg>,
        span: Span,
        elem: Option<&Type>,
    ) -> Option<Type> {
        if args.len() != 1 {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`.recv()` takes one receiver, got {} argument{}",
                    args.len(),
                    if args.len() == 1 { "" } else { "s" }
                ),
                "a select receive arm waits on one typed receiver".to_string(),
                "write `.recv(rx)` where `rx` is a `Receiver<T>`".to_string(),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        let ch_ty = self.infer(&mut args[0].expr)?;
        let inner = match ch_ty {
            Type::Apply {
                ref name, ref args, ..
            } if name == "Receiver" && args.len() == 1 => args[0].clone(),
            other => {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`.recv()` needs a receiver, not {}", other.show()),
                    "each select receive arm waits on a `Receiver<T>`".to_string(),
                    "write `.recv(rx)` where `rx` came from `tasks.channel<T>()`".to_string(),
                    Some(args[0].expr.span()),
                ));
                return None;
            }
        };
        if let Some(prev) = elem {
            if prev.show() != inner.show() {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!(
                        "select receive arms must share one element type, got `{}` and `{}`",
                        prev.show(),
                        inner.show()
                    ),
                    "every `.recv()` in one select waits on the same `Receiver<T>` element type"
                        .to_string(),
                    "use channels with the same `T`, or split into separate selects".to_string(),
                    Some(args[0].expr.span()),
                ));
                return None;
            }
        }
        Some(Type::Apply {
            name: Syntax::TYPE_SELECT_BUILDER.to_string(),
            args: vec![inner],
        })
    }

    #[allow(dead_code)]
    fn infer_select_after(
        &mut self,
        args: &mut Vec<CallArg>,
        span: Span,
        elem: Option<&Type>,
    ) -> Option<Type> {
        if !(args.len() == 1 || args.len() == 2) {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`.after()` takes one `ms:` duration and an optional value, got {} argument{}",
                    args.len(),
                    if args.len() == 1 { "" } else { "s" }
                ),
                "a select timer arm fires after the given milliseconds; mixed recv/timer selects need a typed value"
                    .to_string(),
                "write `.after(ms: 100)` or `.after(ms: 100, value: fallback)`".to_string(),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        let ms_ty = self.infer(&mut args[0].expr)?;
        if !(matches!(ms_ty, Type::Int)
            || matches!(ms_ty, Type::Named(ref n) if n == "Int" || n == "I64" || n == "I32"))
        {
            self.diags.push(Diagnostic::error(
                "E0112",
                format!(
                    "`.after(ms: …)` needs an integer millisecond count, not {}",
                    ms_ty.show()
                ),
                "timer arms use whole milliseconds".to_string(),
                "write `.after(ms: 100)`".to_string(),
                Some(args[0].expr.span()),
            ));
        }
        let value_ty = if args.len() == 2 {
            Some(self.infer(&mut args[1].expr)?)
        } else {
            None
        };
        if let (Some(prev), Some(value)) = (elem, value_ty.as_ref()) {
            if prev.show() != value.show() {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!(
                        "select timer value must match receive element type, got `{}` and `{}`",
                        prev.show(),
                        value.show()
                    ),
                    "one select returns one type no matter which arm wins".to_string(),
                    "use a timer value with the same type as the receive arms".to_string(),
                    Some(args[1].expr.span()),
                ));
                return None;
            }
        }
        if elem.is_some() && value_ty.is_none() {
            self.diags.push(Diagnostic::error(
                "E0112",
                "a timer arm mixed with receive arms needs a typed value".to_string(),
                "otherwise the select would have no value to return when the timer wins"
                    .to_string(),
                "write `.after(ms: 100, value: fallback)`".to_string(),
                Some(span),
            ));
            return None;
        }
        Some(Type::Apply {
            name: Syntax::TYPE_SELECT_BUILDER.to_string(),
            args: elem
                .cloned()
                .or(value_ty)
                .map(|t| vec![t])
                .unwrap_or_default(),
        })
    }

    #[allow(dead_code)]
    fn infer_select_read(
        &mut self,
        args: &mut Vec<CallArg>,
        span: Span,
        elem: Option<&Type>,
    ) -> Option<Type> {
        if args.len() != 1 {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`.read()` takes one stream, got {} argument{}",
                    args.len(),
                    if args.len() == 1 { "" } else { "s" }
                ),
                "a select read arm waits until a TCP stream looks readable".to_string(),
                "write `.read(conn)` where `conn` is a `TcpStream`".to_string(),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        let _ = self.infer(&mut args[0].expr)?;
        Some(match elem {
            Some(t) => Type::Apply {
                name: Syntax::TYPE_SELECT_BUILDER.to_string(),
                args: vec![t.clone()],
            },
            None => Type::Apply {
                name: Syntax::TYPE_SELECT_BUILDER.to_string(),
                args: vec![],
            },
        })
    }

    #[allow(dead_code)]
    fn infer_select_wait(
        &mut self,
        args: &mut Vec<CallArg>,
        span: Span,
        elem: Option<&Type>,
    ) -> Option<Type> {
        if !args.is_empty() {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`.wait()` takes no arguments, got {} argument{}",
                    args.len(),
                    if args.len() == 1 { "" } else { "s" }
                ),
                "`.wait()` blocks until one select arm wins and deregisters the losers".to_string(),
                "write `g.select().recv(ch).wait()`".to_string(),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        Some(match elem {
            Some(t) => t.clone(),
            None => Type::Named("Unit".to_string()),
        })
    }
}

fn cancel_call(name: &str, span: Span) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(Expr::Ident(name.to_string(), span)),
        method: Syntax::METHOD_TASK_CANCEL.to_string(),
        method_span: span,
        owner_type_args: Vec::new(),
        type_args: Vec::new(),
        args: Vec::new(),
        recv_type: None,
        resolved_ret: None,
        checked_widen: false,
    }
}

fn join_call(name: &str, span: Span) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(Expr::Ident(name.to_string(), span)),
        method: "join".to_string(),
        method_span: span,
        owner_type_args: Vec::new(),
        type_args: Vec::new(),
        args: Vec::new(),
        recv_type: None,
        resolved_ret: None,
        checked_widen: false,
    }
}

fn collect_task_idents(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::Ident(name, _) => {
            out.insert(name.clone());
        }
        Expr::ListLit(items, _) => {
            for e in items {
                collect_task_idents(e, out);
            }
        }
        _ => {}
    }
}
