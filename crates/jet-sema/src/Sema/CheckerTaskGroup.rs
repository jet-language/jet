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

pub(crate) struct TaskGroupCtx {
    pub name: String,
    pub pending: Vec<PendingTaskSpawn>,
    synth_counter: usize,
}

impl TaskGroupCtx {
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
            pending: Vec::new(),
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
        self.taskgroup_stack.last().map(|g| g.name.as_str())
    }

    pub(crate) fn taskgroup_receiver_ok(&mut self, receiver: &Expr, span: Span) -> bool {
        let Some(active) = self.active_taskgroup_name() else {
            self.diags.push(Diagnostic::error(
                "E1110",
                "`.task { … }` only works inside a `taskgroup` block".to_string(),
                "structured task spawning is scoped — a taskgroup owns child tasks and joins them at scope exit"
                    .to_string(),
                "wrap the spawn in `taskgroup g { … }` and call `g.task { … }`".to_string(),
                Some(span),
            ));
            return false;
        };
        match receiver {
            Expr::Ident(name, rspan) if name == active => true,
            Expr::Ident(name, rspan) => {
                self.diags.push(Diagnostic::error(
                    "E1110",
                    format!(
                        "`.task` must be called on the active taskgroup handle `{}`, not `{}`",
                        active, name
                    ),
                    "each `taskgroup` block owns spawns on its bound handle only".to_string(),
                    format!("write `{active}.task {{ … }}` inside `taskgroup {active} {{ … }}`"),
                    Some(*rspan),
                ));
                false
            }
            _ => {
                self.diags.push(Diagnostic::error(
                    "E1110",
                    "`.task { … }` must be called on the taskgroup handle".to_string(),
                    "structured spawning goes through the handle bound by `taskgroup g { … }`"
                        .to_string(),
                    "write `g.task { … }` where `g` is the taskgroup name".to_string(),
                    Some(span),
                ));
                false
            }
        }
    }

    pub(crate) fn register_taskgroup_spawn(&mut self, binding: Option<String>, span: Span) {
        if let Some(ctx) = self.taskgroup_stack.last_mut() {
            ctx.pending.push(PendingTaskSpawn {
                binding,
                span,
                consumed: false,
            });
        }
    }

    pub(crate) fn mark_taskgroup_spawn_consumed(&mut self, name: &str) {
        if let Some(ctx) = self.taskgroup_stack.last_mut() {
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
            track: false,
            track_span: None,
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
            self.moved.insert(name, spawn.span);
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
        match method {
            Syntax::TASKGROUP_SPAWN_METHOD => self.infer_taskgroup_spawn(args, span),
            Syntax::TASKGROUP_ALL_METHOD => self.infer_taskgroup_all(args, span),
            Syntax::TASKGROUP_RACE_METHOD => self.infer_taskgroup_race(args, span),
            Syntax::TASKGROUP_ANY_METHOD => self.infer_taskgroup_any(args, span),
            Syntax::TASKGROUP_SELECT_METHOD => self.infer_taskgroup_select(args, span),
            other => {
                self.diags.push(Diagnostic::error(
                    "E0102",
                    format!("`TaskGroup` has no method `{other}`"),
                    "structured taskgroups support `.task { … }`, `.all([…])`, `.race([…])`, `.any([…])`, and `.select()`"
                        .to_string(),
                    "write `g.task { work() }`, `g.all([h1, h2])`, or `g.select().recv(ch).wait()`".to_string(),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                None
            }
        }
    }

    fn infer_taskgroup_spawn(&mut self, args: &mut Vec<CallArg>, span: Span) -> Option<Type> {
        if args.len() != 1 {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`.task {{ … }}` takes one body, got {} argument{}",
                    args.len(),
                    if args.len() == 1 { "" } else { "s" }
                ),
                "a scoped task runs the `{ … }` body on a worker thread".to_string(),
                "write `g.task { your_work() }`".to_string(),
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
                            "`.task {{ … }}` needs a zero-parameter body, got {} parameter{}",
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
                    "write `g.task { your_work() }`".to_string(),
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
        self.register_taskgroup_spawn(binding, span);
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
                        "each element must be a `Task<T>` handle returned from `g.task { … }`"
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
                    "pass a `[Task<T>]` list of handles from `g.task { … }`".to_string(),
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
                        "each element must be a `Task<T>` handle returned from `g.task { … }`"
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
                    "pass a `[Task<T>]` list of handles from `g.task { … }`".to_string(),
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
        type_args: Vec::new(),
        args: Vec::new(),
        recv_type: None,
        resolved_ret: None,
    }
}

fn join_call(name: &str, span: Span) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(Expr::Ident(name.to_string(), span)),
        method: "join".to_string(),
        method_span: span,
        type_args: Vec::new(),
        args: Vec::new(),
        recv_type: None,
        resolved_ret: None,
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
