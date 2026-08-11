//! Exhaustive TStmt evaluation (#777).
use std::collections::HashMap;
use std::sync::{mpsc, Arc};
use crate::AST::Type;
use crate::Codegen::TIR::{TForInMethod, TIfCond, TPatternPosition, TPlace, TStmt};
use crate::Comptime::Builtins::{as_bool, as_int, eval_binop};
use crate::Comptime::{CtReport, CtValue};
use crate::Diagnostics::Diagnostic;
use super::{
    encode_view_mut_path, load_view_mut_owner_list, parse_view_mut_path, raw_place_local,
    progress_elapsed, progress_emit, progress_iter_parts, progress_no_color, progress_now,
    progress_source_has_exact_total, store_view_mut_owner_list, unsupported, EvalCtx, Flow,
    ViewMutPathStep,
};
use crate::Codegen::TIR::{TExpr, TExprKind, THandleOp};

mod progress_semantics {
    include!("../../../Prelude/Core/Progress.rs");
}

/// Inclusive place-region handle used while evaluating `TStmt::SplitViews`.
/// Reuses the `__JetViewMut` field shape so later splits can resolve absolute
/// windows into the original owner list (local, field, or nested index).
fn place_region(base: &str, path: &[ViewMutPathStep], start: i64, end: i64) -> CtValue {
    let mut fields = vec![
        ("base".into(), CtValue::Str(base.to_string())),
        ("start".into(), CtValue::Int(start)),
        ("end".into(), CtValue::Int(end)),
    ];
    if !path.is_empty() {
        fields.push(("path".into(), encode_view_mut_path(path)));
    }
    CtValue::Struct {
        type_name: "__JetViewMut".into(),
        fields,
    }
}

fn progress_wrapper_parts(
    value: &CtValue,
) -> Option<(Vec<CtValue>, String, String, f64, Vec<usize>, usize, usize, bool)> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "__JetProgressIter" {
        return None;
    }
    let items = fields.iter().find_map(|(name, value)| {
        (name == "items").then(|| match value {
            CtValue::List(items) => Some(items.clone()),
            _ => None,
        })
    })??;
    let description = fields.iter().find_map(|(name, value)| {
        (name == "description").then(|| match value {
            CtValue::Str(value) => Some(value.clone()),
            _ => None,
        })
    })??;
    let format = fields.iter().find_map(|(name, value)| {
        (name == "format").then(|| match value {
            CtValue::Str(value) => Some(value.clone()),
            _ => None,
        })
    })??;
    let started_at = fields
        .iter()
        .find_map(|(name, value)| {
            (name == "started_at").then(|| match value {
                CtValue::Float(value) => Some(value.as_f64()),
                CtValue::Int(value) => Some(*value as f64),
                _ => None,
            })
        })
        .flatten()
        .unwrap_or_else(progress_now);
    let pulls = fields
        .iter()
        .find(|(name, _)| name == "pulls")
        .and_then(|(_, value)| match value {
            CtValue::List(values) => values
                .iter()
                .map(|value| match value {
                    CtValue::Int(value) => Some((*value).max(0) as usize),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>(),
            _ => None,
        })
        .unwrap_or_else(|| vec![1; items.len()]);
    let tail = fields
        .iter()
        .find_map(|(name, value)| {
            (name == "tail").then(|| match value {
                CtValue::Int(value) => Some((*value).max(0) as usize),
                _ => None,
            })
        })
        .flatten()
        .unwrap_or(0);
    let total = fields
        .iter()
        .find_map(|(name, value)| {
            (name == "total").then(|| match value {
                CtValue::Int(value) => Some((*value).max(0) as usize),
                _ => None,
            })
        })
        .flatten()
        .unwrap_or(items.len());
    let known_total = fields
        .iter()
        .find_map(|(name, value)| {
            (name == "known_total").then(|| match value {
                CtValue::Bool(value) => Some(*value),
                _ => None,
            })
        })
        .flatten()
        .unwrap_or(true);
    Some((
        items,
        description,
        format,
        started_at,
        pulls,
        tail,
        total,
        known_total,
    ))
}

fn parse_place_region(value: &CtValue) -> Option<(String, Vec<ViewMutPathStep>, i64, i64)> {
    let CtValue::Struct {
        type_name,
        fields,
    } = value
    else {
        return None;
    };
    if type_name != "__JetViewMut" {
        return None;
    }
    let mut base = None;
    let mut start = None;
    let mut end = None;
    for (name, v) in fields {
        match (name.as_str(), v) {
            ("base", CtValue::Str(s)) => base = Some(s.clone()),
            ("start", CtValue::Int(n)) => start = Some(*n),
            ("end", CtValue::Int(n)) => end = Some(*n),
            _ => {}
        }
    }
    Some((base?, parse_view_mut_path(fields), start?, end?))
}

fn owner_list_place(expr: &TExpr) -> Option<(String, Vec<ViewMutPathStep>)> {
    match &expr.kind {
        TExprKind::Local(local) => Some((local.name.clone(), Vec::new())),
        TExprKind::Borrow { place, .. } | TExprKind::Deref(place) => owner_list_place(place),
        TExprKind::Field { recv, field, .. } => {
            let (base, mut path) = owner_list_place(recv)?;
            path.push(ViewMutPathStep::Field(field.clone()));
            Some((base, path))
        }
        TExprKind::Index {
            base,
            index,
            is_map: false,
            ..
        } => {
            let (root, mut path) = owner_list_place(base)?;
            let TExprKind::IntLit(idx, _) = &index.kind else {
                return None;
            };
            path.push(ViewMutPathStep::Index(*idx));
            Some((root, path))
        }
        _ => raw_place_local(expr).map(|local| (local.name.clone(), Vec::new())),
    }
}

impl<'a> EvalCtx<'a> {
    pub(super) fn exec_loop_value(
        &mut self,
        label: Option<&str>,
        body: &'a [TStmt],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        loop {
            self.burn()?;
            match self.exec_stmts(body, scope)? {
                Flow::Normal | Flow::Continue => {}
                Flow::ContinueLabel(ref name) if label == Some(name.as_str()) => {}
                Flow::BreakValue(target, value)
                    if target.is_none() || target.as_deref() == label =>
                {
                    return Ok(value);
                }
                Flow::Return(value) => {
                    self.pending_return = Some(value);
                    return Ok(CtValue::Unit);
                }
                other => {
                    self.pending_flow = Some(other);
                    return Err(unsupported("pending loop control", self.span()));
                }
            }
        }
    }

    pub(crate) fn exec_stmts(
        &mut self,
        stmts: &'a [TStmt],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
        let defer_mark = self.deferred_closes.len();
        let guard_mark = self.shared_guards.len();
        for stmt in stmts {
            let flow = match self.exec_stmt(stmt, scope) {
                Ok(flow) => flow,
                Err(error) => {
                    let _ = self.finish_eval_scope(defer_mark, guard_mark, scope);
                    return Err(error);
                }
            };
            if let Some(flow) = self.pending_flow.take() {
                let preserved = returned_shared_guards(&flow);
                self.finish_eval_scope_preserving(
                    defer_mark,
                    guard_mark,
                    scope,
                    &preserved,
                )?;
                return Ok(flow);
            }
            match flow {
                Flow::Normal => {}
                other => {
                    let preserved = returned_shared_guards(&other);
                    self.finish_eval_scope_preserving(
                        defer_mark,
                        guard_mark,
                        scope,
                        &preserved,
                    )?;
                    return Ok(other);
                }
            }
        }
        self.finish_eval_scope(defer_mark, guard_mark, scope)?;
        Ok(Flow::Normal)
    }

    fn finish_eval_scope(
        &mut self,
        defer_mark: usize,
        guard_mark: usize,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        self.finish_eval_scope_preserving(defer_mark, guard_mark, scope, &[])
    }

    fn finish_eval_scope_preserving(
        &mut self,
        defer_mark: usize,
        guard_mark: usize,
        scope: &mut HashMap<String, CtValue>,
        preserve: &[usize],
    ) -> Result<(), Diagnostic> {
        let deferred_result = self.run_deferred_closes(defer_mark, scope);
        self.release_shared_guards_except(guard_mark, preserve);
        deferred_result
    }

    fn release_shared_guards_except(&mut self, mark: usize, preserve: &[usize]) {
        let (keep, guard_ids): (Vec<_>, Vec<_>) = self.shared_guards[mark..]
            .iter()
            .copied()
            .partition(|guard| preserve.contains(guard));
        self.shared_guards.truncate(mark);
        self.shared_guards.extend(keep);
        let leases = {
            let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
            guard_ids
                .into_iter()
                .filter_map(|index| runtime.shared_guards.get(index).cloned())
                .collect::<Vec<_>>()
        };
        for lease in leases.into_iter().rev() {
            lease.release();
        }
    }

    fn run_deferred_closes(
        &mut self,
        mark: usize,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        while self.deferred_closes.len() > mark {
            let close = self
                .deferred_closes
                .pop()
                .expect("deferred close above mark");
            self.eval_expr(close, scope)?;
        }
        Ok(())
    }

    pub(super) fn exec_stmt(
        &mut self,
        stmt: &'a TStmt,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
        self.enter_source_nesting()?;
        let result = match self.exec_stmt_inner(stmt, scope) {
            Err(_) if self.pending_flow.is_some() => {
                Ok(self.pending_flow.take().expect("checked pending loop control"))
            }
            result => result,
        };
        self.leave_source_nesting();
        result
    }

    fn exec_stmt_inner(
        &mut self,
        stmt: &'a TStmt,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
        self.burn()?;
        match stmt {
            TStmt::Let { name, init, .. } => {
                // D-MEM1 S9 / D-PIN1=A: a whole-place write window (`p :: &node`,
                // `pinned :: mem.pin(&node)`) is an alias in AOT and Cranelift,
                // so bind an alias handle here instead of a copy — otherwise
                // edits through the window vanish on this tier alone (I9).
                if let TExprKind::Borrow { place, mutable: true } = &init.kind {
                    if let Some((base, path)) = owner_list_place(place) {
                        // `x :: &x` would shadow its own owner and make the
                        // handle point at itself, so fall back to the value.
                        if &base != name && scope.contains_key(&base) {
                            scope.insert(
                                name.clone(),
                                super::place_mut_handle(&base, &path),
                            );
                            return Ok(Flow::Normal);
                        }
                    }
                }
                let v = self.eval_expr(init, scope)?;
                if let Some(ret) = self.pending_return.take() {
                    return Ok(Flow::Return(ret));
                }
                scope.insert(name.clone(), v);
                Ok(Flow::Normal)
            }
            TStmt::Assign {
                place,
                op,
                value,
                clone_value,
                ..
            } => {
                let mut rhs = self.eval_expr(value, scope)?;
                if let Some(ret) = self.pending_return.take() {
                    return Ok(Flow::Return(ret));
                }
                if *clone_value {
                    rhs = rhs.clone();
                }
                match place {
                    TPlace::Local(local) => {
                        let key = local.name.clone();
                        if let Some(CtValue::Struct {
                            type_name,
                            fields,
                        }) = scope.get(&key).cloned()
                        {
                            if type_name == "__JetViewMut" {
                                let mut start = None;
                                let mut end = None;
                                for (n, v) in &fields {
                                    match (n.as_str(), v) {
                                        ("start", CtValue::Int(n)) => start = Some(*n),
                                        ("end", CtValue::Int(n)) => end = Some(*n),
                                        _ => {}
                                    }
                                }
                                if let (Some(start), Some(end)) = (start, end) {
                                    if start == end {
                                        let mut items = load_view_mut_owner_list(
                                            &fields,
                                            scope,
                                            self.span(),
                                        )?;
                                        let i = start as usize;
                                        if i >= items.len() {
                                            return Err(unsupported(
                                                "view-mut OOB",
                                                self.span(),
                                            ));
                                        }
                                        let mut rhs = rhs;
                                        if let Some(binop) = op {
                                            rhs = eval_binop(
                                                *binop,
                                                items[i].clone(),
                                                rhs,
                                                self.span(),
                                            )?;
                                        }
                                        items[i] = rhs;
                                        store_view_mut_owner_list(
                                            &fields,
                                            scope,
                                            items,
                                            self.span(),
                                        )?;
                                        return Ok(Flow::Normal);
                                    }
                                }
                            }
                        }
                        let mut rhs = rhs;
                        if let Some(binop) = op {
                            let cur = scope.get(&key).cloned().unwrap_or(CtValue::Unit);
                            rhs = eval_binop(*binop, cur, rhs, self.span())?;
                        }
                        scope.insert(key, rhs);
                        Ok(Flow::Normal)
                    }
                    TPlace::Expr(place_expr)
                        if matches!(place_expr.kind, crate::Codegen::TIR::TExprKind::PoolSlot { .. }) =>
                    {
                        if let Some(binop) = op {
                            let current = self.eval_expr(place_expr, scope)?;
                            rhs = eval_binop(*binop, current, rhs, self.span())?;
                        }
                        self.write_back_place(place_expr, rhs, scope)?;
                        Ok(Flow::Normal)
                    }
                    TPlace::Expr(place_expr)
                        if matches!(
                            place_expr.kind,
                            crate::Codegen::TIR::TExprKind::Field { .. }
                                | crate::Codegen::TIR::TExprKind::SharedGuardValue { .. }
                        ) =>
                    {
                        if let Some(binop) = op {
                            let current = self.eval_expr(place_expr, scope)?;
                            rhs = eval_binop(*binop, current, rhs, self.span())?;
                        }
                        self.write_back_place(place_expr, rhs, scope)?;
                        Ok(Flow::Normal)
                    }
                    TPlace::Expr(place_expr)
                        if matches!(place_expr.kind, crate::Codegen::TIR::TExprKind::Deref(_)) =>
                    {
                        if let Some(binop) = op {
                            let current = self.eval_expr(place_expr, scope)?;
                            rhs = eval_binop(*binop, current, rhs, self.span())?;
                        }
                        self.write_back_place(place_expr, rhs, scope)?;
                        Ok(Flow::Normal)
                    }
                    TPlace::Expr(_) => Err(unsupported("complex assign place", self.span())),
                }
            }
            TStmt::Return(v) => {
                let val = match v {
                    Some(e) => {
                        let val = self.eval_expr(e, scope)?;
                        if let Some(ret) = self.pending_return.take() {
                            ret
                        } else {
                            val
                        }
                    }
                    None => CtValue::Unit,
                };
                Ok(Flow::Return(val))
            }
            TStmt::ExprStmt(e) => {
                let _ = self.eval_expr(e, scope)?;
                if let Some(ret) = self.pending_return.take() {
                    return Ok(Flow::Return(ret));
                }
                Ok(Flow::Normal)
            }
            TStmt::TaskGroup { group, limit, body } => {
                let mut run_body = |this: &mut Self| {
                    let value = this.new_taskgroup(limit.as_ref(), scope)?;
                    let index = Self::taskgroup_index(&value)
                        .expect("new task group always carries an evaluator index");
                    scope.insert(group.name.clone(), value);
                    let body_result = this.exec_stmts(body, scope);
                    let close_result = this.close_taskgroup(index);
                    scope.remove(&group.name);
                    match (body_result, close_result) {
                        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
                        (Ok(flow), Ok(())) => Ok(flow),
                    }
                };
                if self.task_sender.is_some() {
                    run_body(self)
                } else {
                    let config = Arc::new(self.task_config());
                    std::thread::scope(|threads| {
                        let (sender, receiver) = mpsc::channel();
                        let worker_config = config.clone();
                        let dispatcher = std::thread::Builder::new()
                            .name("jet-tir-task-dispatch".to_string())
                            .stack_size(8 * 1024 * 1024)
                            .spawn_scoped(threads, move || {
                                while let Ok(job) = receiver.recv() {
                                    let job_config = (*worker_config).clone();
                                    std::thread::Builder::new()
                                        .name("jet-tir-task".to_string())
                                        .stack_size(8 * 1024 * 1024)
                                        .spawn_scoped(threads, move || {
                                            Self::run_task_job(job_config, job)
                                        })
                                        .expect("evaluator task worker");
                                }
                            })
                            .expect("evaluator task dispatcher");
                        self.task_sender = Some(sender);
                        let result = run_body(self);
                        drop(self.task_sender.take());
                        dispatcher
                            .join()
                            .expect("evaluator task dispatcher panicked");
                        result
                    })
                }
            }
            TStmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                if self.eval_if_cond(cond, scope)? {
                    self.exec_stmts(then_body, scope)
                } else if let Some(else_body) = else_body {
                    self.exec_stmts(else_body, scope)
                } else {
                    Ok(Flow::Normal)
                }
            }
            TStmt::Loop { body, label } => self.exec_infinite(label.as_deref(), body, scope),
            TStmt::While { cond, body, label } => {
                loop {
                    self.burn()?;
                    std::thread::yield_now();
                    if !as_bool(&self.eval_expr(cond, scope)?, self.span())? {
                        break;
                    }
                    match self.exec_stmts(body, scope)? {
                        Flow::Normal | Flow::Continue => {}
                        Flow::Break => break,
                        Flow::BreakLabel(ref name) if label.as_deref() == Some(name.as_str()) => break,
                        Flow::ContinueLabel(ref name) if label.as_deref() == Some(name.as_str()) => {}
                        other => return Ok(other),
                    }
                }
                Ok(Flow::Normal)
            }
            TStmt::CountedLoop {
                init,
                cond,
                step,
                body,
                label,
            } => {
                match self.exec_stmt(init, scope)? {
                    Flow::Normal => {}
                    other => return Ok(other),
                }
                loop {
                    self.burn()?;
                    std::thread::yield_now();
                    if !as_bool(&self.eval_expr(cond, scope)?, self.span())? {
                        break;
                    }
                    match self.exec_stmts(body, scope)? {
                        Flow::Normal | Flow::Continue => {}
                        Flow::Break => break,
                        Flow::BreakLabel(ref name) if label.as_deref() == Some(name.as_str()) => break,
                        Flow::ContinueLabel(ref name) if label.as_deref() == Some(name.as_str()) => {}
                        other => return Ok(other),
                    }
                    if let Some(step) = step {
                        match self.exec_stmt(step, scope)? {
                            Flow::Normal => {}
                            other => return Ok(other),
                        }
                    }
                }
                Ok(Flow::Normal)
            }
            TStmt::Range {
                var,
                source,
                start,
                end,
                step,
                exclusive,
                body,
                label,
            } => {
                let (mut i, end_v, exclusive_v) = if let Some(source) = source {
                    let value = self.eval_expr(source, scope)?;
                    let CtValue::Struct { type_name, fields } = value else {
                        return Err(unsupported("Range loop source", self.span()));
                    };
                    if type_name != crate::Syntax::TYPE_RANGE {
                        return Err(unsupported("Range loop source type", self.span()));
                    }
                    let field = |name: &str| {
                        fields.iter().find(|(field, _)| field == name).map(|(_, value)| value)
                    };
                    let start = field("start")
                        .ok_or_else(|| unsupported("Range.start", self.span()))
                        .and_then(|value| as_int(value, self.span()))?;
                    let end = field("end")
                        .ok_or_else(|| unsupported("Range.end", self.span()))
                        .and_then(|value| as_int(value, self.span()))?;
                    let exclusive = matches!(field("exclusive"), Some(CtValue::Bool(true)));
                    (start, end, exclusive)
                } else {
                    (
                        as_int(&self.eval_expr(start, scope)?, self.span())?,
                        as_int(&self.eval_expr(end, scope)?, self.span())?,
                        *exclusive,
                    )
                };
                let step_v = match step {
                    Some(s) => as_int(&self.eval_expr(s, scope)?, self.span())?,
                    None => 1,
                };
                if step_v == 0 {
                    return Err(unsupported("range step 0", self.span()));
                }
                // D-RANGE-EXCL1=C: exclusive `..<` stops before end; inclusive `..` includes it.
                let in_range = |cur: i64| {
                    if exclusive_v {
                        if step_v > 0 { cur < end_v } else { cur > end_v }
                    } else if step_v > 0 {
                        cur <= end_v
                    } else {
                        cur >= end_v
                    }
                };
                while in_range(i) {
                    self.burn()?;
                    scope.insert(var.clone(), CtValue::Int(i));
                    match self.exec_stmts(body, scope)? {
                        Flow::Normal | Flow::Continue => {}
                        Flow::Break => break,
                        Flow::BreakLabel(ref name) if label.as_deref() == Some(name.as_str()) => break,
                        Flow::ContinueLabel(ref name) if label.as_deref() == Some(name.as_str()) => {}
                        other => return Ok(other),
                    }
                    i += step_v;
                }
                Ok(Flow::Normal)
            }
            TStmt::Break(label) => Ok(match label {
                Some(name) => Flow::BreakLabel(name.clone()),
                None => Flow::Break,
            }),
            TStmt::BreakValue { label, value } => Ok(Flow::BreakValue(
                label.clone(),
                self.eval_expr(value, scope)?,
            )),
            TStmt::Continue(label) => Ok(match label {
                Some(name) => Flow::ContinueLabel(name.clone()),
                None => Flow::Continue,
            }),
            TStmt::TupleDestructure { init, binds, .. } => {
                let v = self.eval_expr(init, scope)?;
                match v {
                    CtValue::List(items) => {
                        for (i, (local, _)) in binds.iter().enumerate() {
                            let key = strip_user(local);
                            scope.insert(key, items.get(i).cloned().unwrap_or(CtValue::Unit));
                        }
                        Ok(Flow::Normal)
                    }
                    CtValue::Struct { fields, .. } => {
                        for (i, (local, _)) in binds.iter().enumerate() {
                            let key = strip_user(local);
                            scope.insert(
                                key,
                                fields.get(i).map(|(_, v)| v.clone()).unwrap_or(CtValue::Unit),
                            );
                        }
                        Ok(Flow::Normal)
                    }
                    _ => Err(unsupported("tuple destructure non-aggregate", self.span())),
                }
            }
            TStmt::StructDestructure { init, binds, .. } => {
                let v = self.eval_expr(init, scope)?;
                let CtValue::Struct { fields, .. } = v else {
                    return Err(unsupported("struct destructure non-struct", self.span()));
                };
                for (local, field) in binds {
                    let key = strip_user(local);
                    let fkey = strip_user(field);
                    let val = fields
                        .iter()
                        .find(|(n, _)| n == &fkey || n == field)
                        .map(|(_, v)| v.clone())
                        .unwrap_or(CtValue::Unit);
                    scope.insert(key, val);
                }
                Ok(Flow::Normal)
            }
            TStmt::ListDestructure { init, elems, .. } => {
                let v = self.eval_expr(init, scope)?;
                let CtValue::List(items) = v else {
                    return Err(unsupported("list destructure non-list", self.span()));
                };
                for (i, local) in elems.iter().enumerate() {
                    scope.insert(strip_user(local), items.get(i).cloned().unwrap_or(CtValue::Unit));
                }
                Ok(Flow::Normal)
            }
            TStmt::Impure(body) => {
                let previous = self.impure_depth;
                self.impure_depth = previous.saturating_add(1);
                let result = self.exec_stmts(body, scope);
                self.impure_depth = previous;
                result
            }
            TStmt::Inline(body)
            | TStmt::DebugOnly(body)
            | TStmt::Unsafe(body)
            | TStmt::Region(body) => self.exec_stmts(body, scope),
            TStmt::LineMarker(_) => Ok(Flow::Normal),
            TStmt::SourceSpan(span) => {
                self.current_span = *span;
                Ok(Flow::Normal)
            }
            TStmt::DeferClose { close, .. } => {
                self.deferred_closes.push(close);
                Ok(Flow::Normal)
            }
            TStmt::ForIn {
                label,
                var,
                var2,
                source,
                step,
                method_kind,
                body,
                ..
            } => {
                let mut progress = None;
                let mut coll = if let TExprKind::CoreCall {
                    module,
                    method,
                    args,
                    ..
                } = &source.kind
                {
                    if module == "core.io"
                        && method == "progress"
                        && args.len() >= 1
                        && !matches!(args.first().map(|arg| &arg.ty), Some(Type::String))
                    {
                        let evaluated = self.eval_expr(&args[0], scope)?;
                        let (coll, iter_known_total) = match progress_iter_parts(&evaluated) {
                            Some((items, known_total)) => {
                                (CtValue::List(items), Some(known_total))
                            }
                            None => (evaluated, None),
                        };
                        let description = match args.get(1) {
                            Some(arg) => match self.eval_expr(arg, scope)? {
                                CtValue::Str(value) => value,
                                _ => return Err(unsupported("progress description", self.span())),
                            },
                            None => "Progress".to_string(),
                        };
                        let format = match args.get(2) {
                            Some(arg) => match self.eval_expr(arg, scope)? {
                                CtValue::Str(value) => value,
                                _ => return Err(unsupported("progress format", self.span())),
                            },
                            None => String::new(),
                        };
                        let total = match &coll {
                            CtValue::List(items) => items.len(),
                            _ => return Err(unsupported("progress source", self.span())),
                        };
                        let known_total = iter_known_total.unwrap_or_else(|| match args.first().map(|arg| &arg.ty) {
                            Some(Type::Apply { name, .. })
                                if name == crate::Syntax::TYPE_ITER => args
                                    .first()
                                    .is_some_and(progress_source_has_exact_total),
                            _ => true,
                        });
                        progress = Some((
                            description,
                            format,
                            progress_now(),
                            total,
                            vec![1; total],
                            0,
                            known_total,
                        ));
                        coll
                    } else {
                        self.eval_expr(source, scope)?
                    }
                } else {
                    self.eval_expr(source, scope)?
                };
                if let Some((items, _)) = progress_iter_parts(&coll) {
                    coll = CtValue::List(items);
                }
                if progress.is_none() {
                    if let Some((items, description, format, started_at, pulls, tail, total, known_total)) =
                        progress_wrapper_parts(&coll)
                    {
                        progress = Some((
                            description,
                            format,
                            started_at,
                            total,
                            pulls,
                            tail,
                            known_total,
                        ));
                        coll = CtValue::List(items);
                    }
                }
                if progress.is_none() {
                    if let Some(TForInMethod::Iterable {
                        coll_type,
                        iter_type,
                    }) = method_kind
                {
                    let iter_func = self
                        .funcs
                        .get(&format!("{coll_type}::iter"))
                        .copied()
                        .ok_or_else(|| unsupported("Iterable.iter", self.span()))?;
                    let next_func = self
                        .funcs
                        .get(&format!("{iter_type}::next"))
                        .copied()
                        .ok_or_else(|| unsupported("Iterator.next", self.span()))?;
                    let mut iter_scope = HashMap::new();
                    iter_scope.insert("self".to_string(), coll);
                    let mut iterator = self.run_func(iter_func, Vec::new(), &mut iter_scope)?;
                    loop {
                        self.burn()?;
                        let mut next_scope = HashMap::new();
                        next_scope.insert("self".to_string(), iterator);
                        let next = self.run_func(next_func, Vec::new(), &mut next_scope)?;
                        iterator = next_scope.remove("self").unwrap_or(CtValue::Unit);
                        let CtValue::Present(item) = next else {
                            if matches!(next, CtValue::Failed(CtReport::Clean(_))) {
                                break;
                            }
                            return Err(unsupported("Iterator.next result", self.span()));
                        };
                        scope.insert(var.clone(), *item);
                        match self.exec_stmts(body, scope)? {
                            Flow::Normal | Flow::Continue => {}
                            Flow::Break => break,
                            Flow::BreakLabel(ref name)
                                if label.as_deref() == Some(name.as_str()) =>
                            {
                                break
                            }
                            Flow::ContinueLabel(ref name)
                                if label.as_deref() == Some(name.as_str()) => {}
                            other => return Ok(other),
                        }
                    }
                    return Ok(Flow::Normal);
                }
                }
                if progress.is_none() {
                    if let Some(TForInMethod::Chars) = method_kind {
                        // I9: same semantics as AOT/JIT `({recv}).chars()` —
                        // iterate Unicode scalar values of the receiver string.
                        let CtValue::Str(s) = &coll else {
                            return Err(unsupported("chars() receiver", self.span()));
                        };
                        for ch in s.chars() {
                            self.burn()?;
                            scope.insert(var.clone(), CtValue::Char(ch));
                            match self.exec_stmts(body, scope)? {
                                Flow::Normal | Flow::Continue => {}
                                Flow::Break => break,
                                Flow::BreakLabel(ref name)
                                    if label.as_deref() == Some(name.as_str()) =>
                                {
                                    break
                                }
                                Flow::ContinueLabel(ref name)
                                    if label.as_deref() == Some(name.as_str()) => {}
                                other => return Ok(other),
                            }
                        }
                        return Ok(Flow::Normal);
                    }
                }
                if progress.is_none() {
                    if let Some(TForInMethod::EncodingReader { reader_type }) = method_kind {
                        let op = match reader_type.as_str() {
                            "JSONReader" => THandleOp::JSONReaderNext,
                            "JSONLReader" => THandleOp::JSONLReaderNext,
                            "CSVReader" => THandleOp::CSVReaderNext,
                            "XMLReader" => THandleOp::XMLReaderNext,
                            "CBORReader" => THandleOp::CBORReaderNext,
                            _ => {
                                return Err(unsupported(
                                    "unknown encoding reader",
                                    self.span(),
                                ))
                            }
                        };
                        let stride = match step {
                            Some(s) => as_int(&self.eval_expr(s, scope)?, self.span())?,
                            None => 1,
                        };
                        if stride <= 0 {
                            return Err(unsupported("for-in stride <= 0", self.span()));
                        }
                        let mut skipped = 0i64;
                        loop {
                            self.burn()?;
                            let mut args = [];
                            let next = super::handles::eval_handle(
                                &op,
                                &mut coll,
                                &mut args,
                                self.span(),
                            )?;
                            let item = match next {
                                CtValue::Present(value) => match *value {
                                    CtValue::Present(item) => *item,
                                    CtValue::Failed(CtReport::Clean(_)) => break,
                                    _ => {
                                        return Err(unsupported(
                                            "encoding reader next result",
                                            self.span(),
                                        ))
                                    }
                                },
                                CtValue::Failed(CtReport::Clean(_)) => break,
                                _ => {
                                    return Err(unsupported(
                                        "encoding reader next result",
                                        self.span(),
                                    ))
                                }
                            };
                            if skipped != 0 {
                                skipped -= 1;
                                continue;
                            }
                            scope.insert(var.clone(), item);
                            match self.exec_stmts(body, scope)? {
                                Flow::Normal | Flow::Continue => {}
                                Flow::Break => break,
                                Flow::BreakLabel(ref name)
                                    if label.as_deref() == Some(name.as_str()) =>
                                {
                                    break
                                }
                                Flow::ContinueLabel(ref name)
                                    if label.as_deref() == Some(name.as_str()) => {}
                                other => return Ok(other)
                            }
                            skipped = stride - 1;
                        }
                        return Ok(Flow::Normal);
                    }
                }
                if method_kind.is_some() {
                    return Err(unsupported("for-in method collection", self.span()));
                }
                if let Some(stream_index) = super::EvalCtx::stream_index(&coll) {
                    let (func, args) = {
                        let runtime =
                            self.runtime.lock().expect("evaluator runtime poisoned");
                        runtime
                            .streams
                            .get(stream_index)
                            .map(|stream| (stream.func, stream.args.clone()))
                            .ok_or_else(|| unsupported("stream handle", self.span()))?
                    };
                    let previous_consumer = self.yield_consumer.replace(super::YieldConsumer {
                        var: var.clone(),
                        body,
                    });
                    let previous_scope = self.yield_scope.replace(std::mem::take(scope));
                    let mut generator_scope = HashMap::new();
                    let result = self.run_func(func, args, &mut generator_scope);
                    *scope = self.yield_scope.take().unwrap_or_default();
                    self.yield_scope = previous_scope;
                    self.yield_consumer = previous_consumer;
                    result?;
                    return Ok(Flow::Normal);
                }
                let stride = match step {
                    Some(s) => as_int(&self.eval_expr(s, scope)?, self.span())?,
                    None => 1,
                };
                if stride <= 0 {
                    return Err(unsupported("for-in stride <= 0", self.span()));
                }
                match coll {
                    CtValue::List(items) => {
                        let mut i = 0usize;
                        let mut progress_count = 0usize;
                        let mut progress_yielded = 0usize;
                        let mut naturally_exhausted = true;
                        while i < items.len() {
                            self.burn()?;
                            let next_i = i.saturating_add(stride as usize).min(items.len());
                            if let Some((description, format, started_at, total, pulls, _, known_total)) = progress.as_ref() {
                                let requested = if progress_count == 0 {
                                    1
                                } else {
                                    stride as usize
                                };
                                let end = progress_yielded
                                    .saturating_add(requested)
                                    .min(pulls.len());
                                let raw_pulls: usize = pulls[progress_yielded..end].iter().sum();
                                progress_yielded = end;
                                let raw_pulls = if *known_total {
                                    raw_pulls.min(total.saturating_sub(progress_count))
                                } else {
                                    raw_pulls
                                };
                                if raw_pulls != 0 {
                                    for pulled in
                                        (progress_count + 1)..=(progress_count + raw_pulls)
                                    {
                                    let text = progress_semantics::jet_progress_render(
                                        description,
                                        format,
                                        pulled,
                                        (*known_total).then_some(*total),
                                        progress_elapsed(*started_at),
                                        progress_no_color(),
                                    );
                                    progress_emit(self.sink.as_ref(), &text);
                                    }
                                }
                                progress_count = progress_count.saturating_add(raw_pulls);
                            }
                            // D-RANGE-EXCL1=C: two bindings are index then item;
                            // one binding stays item-only.
                            if let Some(v2) = var2 {
                                scope.insert(var.clone(), CtValue::Int(i as i64));
                                scope.insert(v2.clone(), items[i].clone());
                            } else {
                                scope.insert(var.clone(), items[i].clone());
                            }
                            match self.exec_stmts(body, scope)? {
                                Flow::Normal | Flow::Continue => {}
                                Flow::Break => {
                                    naturally_exhausted = false;
                                    break;
                                }
                                Flow::BreakLabel(ref name)
                                    if label.as_deref() == Some(name.as_str()) =>
                                {
                                    naturally_exhausted = false;
                                    break
                                }
                                Flow::ContinueLabel(ref name)
                                    if label.as_deref() == Some(name.as_str()) => {}
                                other => return Ok(other),
                            }
                            i = next_i;
                        }
                        if naturally_exhausted {
                            if let Some((description, format, started_at, total, pulls, tail, known_total)) = progress.as_ref() {
                                let remaining = pulls[progress_yielded..].iter().sum::<usize>() + *tail;
                                let remaining = if *known_total {
                                    remaining.min(total.saturating_sub(progress_count))
                                } else {
                                    remaining
                                };
                                if remaining != 0 {
                                    for pulled in (progress_count + 1)..=(progress_count + remaining) {
                                    let text = progress_semantics::jet_progress_render(
                                        description,
                                        format,
                                        pulled,
                                        (*known_total).then_some(*total),
                                        progress_elapsed(*started_at),
                                        progress_no_color(),
                                    );
                                    progress_emit(self.sink.as_ref(), &text);
                                    }
                                }
                            }
                        }
                        Ok(Flow::Normal)
                    }
                    CtValue::Map(entries) => {
                        let pairs: Vec<(CtValue, CtValue)> = entries
                            .iter()
                            .map(|(k, v)| (k.to_value(), v.clone()))
                            .collect();
                        let mut i = 0usize;
                        while i < pairs.len() {
                            self.burn()?;
                            let (k, v) = &pairs[i];
                            if let Some(v2) = var2 {
                                scope.insert(var.clone(), k.clone());
                                scope.insert(v2.clone(), v.clone());
                            } else {
                                scope.insert(var.clone(), k.clone());
                            }
                            match self.exec_stmts(body, scope)? {
                                Flow::Normal | Flow::Continue => {}
                                Flow::Break => break,
                                Flow::BreakLabel(ref name)
                                    if label.as_deref() == Some(name.as_str()) =>
                                {
                                    break
                                }
                                Flow::ContinueLabel(ref name)
                                    if label.as_deref() == Some(name.as_str()) => {}
                                other => return Ok(other),
                            }
                            i = i.saturating_add(stride as usize);
                        }
                        Ok(Flow::Normal)
                    }
                    _ => Err(unsupported("for-in collection", self.span())),
                }
            }
            TStmt::EnumMatch {
                scrutinee,
                arms,
                else_body,
                ..
            } => {
                let value = self.eval_expr(scrutinee, scope)?;
                for arm in arms {
                    if bind_match_pattern(&arm.pattern.pattern, &value, scope)? {
                        return self.exec_stmts(&arm.body, scope);
                    }
                }
                if let Some(body) = else_body {
                    return self.exec_stmts(body, scope);
                }
                Ok(Flow::Normal)
            }
            TStmt::RangeSwitch {
                subject,
                arms,
                else_body,
            } => {
                let value = as_int(&self.eval_expr(subject, scope)?, self.span())?;
                for (lo, hi, body) in arms {
                    if value >= *lo && value <= *hi {
                        return self.exec_stmts(body, scope);
                    }
                }
                self.exec_stmts(else_body, scope)
            }
            TStmt::MixedSwitch {
                subject,
                arms,
                else_body,
                ..
            } => {
                let value = self.eval_expr(subject, scope)?;
                let saved = self.switch_subject.replace(value);
                let result = (|| {
                    for (cond, body) in arms {
                        let c = self.eval_expr(cond, scope)?;
                        if as_bool(&c, self.span())? {
                            return self.exec_stmts(body, scope);
                        }
                    }
                    if let Some(body) = else_body {
                        return self.exec_stmts(body, scope);
                    }
                    Ok(Flow::Normal)
                })();
                self.switch_subject = saved;
                result
            }
            TStmt::IndexAssign {
                base,
                index,
                is_map,
                value,
                ..
            } => {
                let base_value = self.eval_expr(base, scope)?;
                let idx_v = self.eval_expr(index, scope)?;
                let rhs = self.eval_expr(value, scope)?;
                // Mutable place-window write-through (`&xs[a..b]` → `__JetViewMut`).
                if let CtValue::Struct {
                    type_name,
                    fields,
                } = &base_value
                {
                    if type_name == "__JetViewMut" {
                        let mut start = None;
                        for (n, v) in fields {
                            if let ("start", CtValue::Int(n)) = (n.as_str(), v) {
                                start = Some(*n);
                            }
                        }
                        let start = start
                            .ok_or_else(|| unsupported("view-mut fields", self.span()))?;
                        let idx = as_int(&idx_v, self.span())?;
                        if idx < 0 {
                            return Err(unsupported("negative view index", self.span()));
                        }
                        let mut items =
                            load_view_mut_owner_list(fields, scope, self.span())?;
                        let i = (start + idx) as usize;
                        if i >= items.len() {
                            return Err(unsupported("view-mut OOB", self.span()));
                        }
                        items[i] = rhs;
                        store_view_mut_owner_list(fields, scope, items, self.span())?;
                        return Ok(Flow::Normal);
                    }
                }
                let base_name = match &base.kind {
                    crate::Codegen::TIR::TExprKind::Local(local) => local.name.clone(),
                    _ => return Err(unsupported("index assign base", self.span())),
                };
                if *is_map {
                    let Some(CtValue::Map(mut entries)) = scope.get(&base_name).cloned() else {
                        return Err(unsupported("index assign map", self.span()));
                    };
                    let key = crate::AST::CtKey::from_value(idx_v)
                        .ok_or_else(|| unsupported("index assign map key", self.span()))?;
                    entries.insert(key, rhs);
                    scope.insert(base_name, CtValue::Map(entries));
                } else {
                    let idx = as_int(&idx_v, self.span())?;
                    if idx < 0 {
                        return Err(unsupported("negative index assign", self.span()));
                    }
                    if let Some(mut carrier @ CtValue::Struct { .. }) =
                        scope.get(&base_name).cloned()
                    {
                        if super::uninit_fixed_write(&mut carrier, idx as usize, rhs.clone()) {
                            scope.insert(base_name, carrier);
                            return Ok(Flow::Normal);
                        }
                    }
                    let Some(CtValue::List(mut items)) = scope.get(&base_name).cloned() else {
                        return Err(unsupported("index assign list", self.span()));
                    };
                    let i = idx as usize;
                    if i >= items.len() {
                        return Err(unsupported("index assign OOB", self.span()));
                    }
                    items[i] = rhs;
                    scope.insert(base_name, CtValue::List(items));
                }
                Ok(Flow::Normal)
            }
            TStmt::IndexFieldAssign(assign) => {
                if assign.is_map {
                    return Err(unsupported("index field assign on map", self.span()));
                }
                let idx = as_int(&self.eval_expr(&assign.index, scope)?, self.span())?;
                if idx < 0 {
                    return Err(unsupported("negative index field assign", self.span()));
                }
                let mut rhs = self.eval_expr(&assign.value, scope)?;
                if assign.clone_value {
                    rhs = rhs.clone();
                }
                let CtValue::List(mut items) = self.eval_expr(&assign.base, scope)? else {
                    return Err(unsupported("index field assign list", self.span()));
                };
                let i = idx as usize;
                if i >= items.len() {
                    return Err(unsupported("index field assign OOB", self.span()));
                }
                let CtValue::Struct {
                    type_name,
                    mut fields,
                } = items[i].clone()
                else {
                    return Err(unsupported("index field assign elem", self.span()));
                };
                let mut found = false;
                for (name, val) in &mut fields {
                    if name == &assign.field {
                        if let Some(op) = assign.op {
                            *val = eval_binop(op, val.clone(), rhs, self.span())?;
                        } else {
                            *val = rhs;
                        }
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(unsupported(
                        &format!("field `{}`", assign.field),
                        self.span(),
                    ));
                }
                items[i] = CtValue::Struct { type_name, fields };
                self.write_back_place(&assign.base, CtValue::List(items), scope)?;
                Ok(Flow::Normal)
            }
            TStmt::IndexHookAssign {
                type_name,
                base,
                index,
                value,
            } => {
                let recv = self.eval_expr(base, scope)?;
                let key = self.eval_expr(index, scope)?;
                let rhs = self.eval_expr(value, scope)?;
                let func = self
                    .funcs
                    .get(&format!("{type_name}::set"))
                    .copied()
                    .ok_or_else(|| unsupported("IndexMut.set", self.span()))?;
                let mut child = HashMap::new();
                child.insert("self".to_string(), recv);
                self.run_func(func, vec![key, rhs], &mut child)?;
                if let Some(updated) = child.remove("self") {
                    self.write_back_place(base, updated, scope)?;
                }
                Ok(Flow::Normal)
            }
            TStmt::MathSwizzleAssign { .. } => Err(unsupported("math swizzle assign", self.span())),
            TStmt::GcEdit { .. } => Err(unsupported("gc edit", self.span())),
            TStmt::SplitViews {
                owner,
                root,
                len,
                source,
                source_start,
                before,
                split_tail,
                segment,
                after,
                name,
                start,
                end,
                single,
                write,
                elem_ty: _,
                line: _,
            } => {
                // D-SHAPE-PLACE1=A: mirror AOT `split_at_mut` planning with absolute
                // region handles. Mutable user windows stay as `__JetViewMut` so
                // IndexAssign / field writes reach the owner (AOT emits real slices).
                // Read-only windows still materialize.
                let owner_path = if let Some(owner_expr) = owner {
                    let (base_name, path) = owner_list_place(owner_expr)
                        .ok_or_else(|| unsupported("split views owner", self.span()))?;
                    let items = {
                        let probe = place_region(&base_name, &path, 0, 0);
                        let CtValue::Struct { fields, .. } = &probe else {
                            return Err(unsupported("split views owner", self.span()));
                        };
                        load_view_mut_owner_list(fields, scope, self.span())?
                    };
                    let len_i = items.len() as i64;
                    if *start < 0 || *end < *start || *end >= len_i {
                        return Err(unsupported("split views bounds", self.span()));
                    }
                    let root_end = len_i - 1;
                    scope.insert(root.clone(), place_region(&base_name, &path, 0, root_end));
                    scope.insert(len.clone(), CtValue::Int(len_i));
                    path
                } else {
                    let source_region = scope
                        .get(source)
                        .cloned()
                        .ok_or_else(|| unsupported("split views source", self.span()))?;
                    let (_, path, _, _) = parse_place_region(&source_region)
                        .ok_or_else(|| unsupported("split views source region", self.span()))?;
                    path
                };
                let len_i = match scope.get(len) {
                    Some(CtValue::Int(n)) => *n,
                    _ => return Err(unsupported("split views len", self.span())),
                };
                if *start < 0 || *end < *start || *end >= len_i {
                    return Err(unsupported("split views bounds", self.span()));
                }
                let source_region = scope
                    .get(source)
                    .cloned()
                    .ok_or_else(|| unsupported("split views source", self.span()))?;
                let (base_name, path, _src_abs_start, src_abs_end) =
                    parse_place_region(&source_region)
                        .ok_or_else(|| unsupported("split views source region", self.span()))?;
                let path = if owner.is_some() {
                    owner_path
                } else {
                    path
                };
                let relative_start = *start - *source_start;
                let width = *end - *start + 1;
                if relative_start < 0 || width <= 0 {
                    return Err(unsupported("split views range", self.span()));
                }
                // `(before, split_tail) = source.split_at(relative_start)`
                if relative_start > 0 {
                    scope.insert(
                        before.clone(),
                        place_region(&base_name, &path, *source_start, *start - 1),
                    );
                } else {
                    scope.insert(
                        before.clone(),
                        place_region(&base_name, &path, *source_start, *source_start - 1),
                    );
                }
                scope.insert(
                    split_tail.clone(),
                    place_region(&base_name, &path, *start, src_abs_end),
                );
                // `(segment, after) = split_tail.split_at(width)`
                scope.insert(
                    segment.clone(),
                    place_region(&base_name, &path, *start, *end),
                );
                let after_start = *end + 1;
                if after_start <= src_abs_end {
                    scope.insert(
                        after.clone(),
                        place_region(&base_name, &path, after_start, src_abs_end),
                    );
                } else {
                    scope.insert(
                        after.clone(),
                        place_region(&base_name, &path, after_start, *end),
                    );
                }
                if *write {
                    // Write-through handle — including single-element `&xs[i]` so
                    // `view.field = v` / `view = v` match AOT `&mut` semantics.
                    scope.insert(name.clone(), place_region(&base_name, &path, *start, *end));
                } else {
                    let items = {
                        let probe = place_region(&base_name, &path, *start, *end);
                        let CtValue::Struct { fields, .. } = &probe else {
                            return Err(unsupported("split views owner", self.span()));
                        };
                        load_view_mut_owner_list(fields, scope, self.span())?
                    };
                    let window = items[*start as usize..=*end as usize].to_vec();
                    if *single {
                        scope.insert(
                            name.clone(),
                            window.into_iter().next().unwrap_or(CtValue::Unit),
                        );
                    } else {
                        scope.insert(name.clone(), CtValue::List(window));
                    }
                }
                Ok(Flow::Normal)
            }
            TStmt::Reactive { .. } => Err(unsupported("statement `Reactive`", self.span())),
            TStmt::Layout { .. } => Err(unsupported("statement `Layout`", self.span())),
            TStmt::ContextBlock { guards, body } => {
                let saved_deadline = self.context_deadline;
                let result = (|| {
                    for (name, value) in guards {
                        if name == crate::Syntax::CTX_FIELD_DEADLINE {
                            self.context_deadline = Some(match self.eval_expr(value, scope)? {
                                CtValue::Int(deadline) => deadline,
                                _ => return Err(unsupported("context deadline", self.span())),
                            });
                        }
                    }
                    self.exec_stmts(body, scope)
                })();
                self.context_deadline = saved_deadline;
                result
            }
            TStmt::Live { .. } => Err(unsupported("statement `Live`", self.span())),
            TStmt::Shield { body } => {
                if self.task_cancel.is_some() {
                    crate::scheduler::jet_scheduler_shield_enter();
                }
                self.shield_depth += 1;
                let result = self.exec_stmts(body, scope);
                self.shield_depth -= 1;
                if self.task_cancel.is_some() {
                    let _ = crate::scheduler::jet_scheduler_shield_leave_status();
                }
                if self.shield_depth == 0 {
                    self.task_wait_cancel_check()?;
                }
                result
            }
            TStmt::ScopeMember { .. } => Err(unsupported("statement `ScopeMember`", self.span())),
            TStmt::Transact {
                snapshots,
                uses_stm,
                body,
                ..
            } => {
                let mut snaps = Vec::new();
                for (local, _) in snapshots {
                    let key = local.name.clone();
                    let value = scope
                        .get(&key)
                        .cloned()
                        .or_else(|| {
                            let mangled = format!("user_{key}");
                            scope.get(&mangled).cloned()
                        })
                        .unwrap_or(CtValue::Unit);
                    snaps.push((key, value));
                }
                self.txn_stack.push(super::EvalTxnFrame {
                    snapshots: snaps,
                    on_commit: Vec::new(),
                    on_rollback: Vec::new(),
                });
                if *uses_stm {
                    self.shared_transactions.push(Vec::new());
                }
                let flow = self.exec_stmts(body, scope);
                let frame = self.txn_stack.pop().unwrap_or(super::EvalTxnFrame {
                    snapshots: Vec::new(),
                    on_commit: Vec::new(),
                    on_rollback: Vec::new(),
                });
                let staged = if *uses_stm {
                    self.shared_transactions.pop().unwrap_or_default()
                } else {
                    Vec::new()
                };
                match flow {
                    Ok(Flow::Normal) => {
                        let slots = {
                            let runtime =
                                self.runtime.lock().expect("evaluator runtime poisoned");
                            staged
                                .iter()
                                .filter_map(|delta| {
                                    runtime
                                        .shared_values
                                        .get(delta.shared_index)
                                        .cloned()
                                        .map(|slot| (delta.shared_index, slot))
                                })
                                .collect::<Vec<_>>()
                        };
                        let permits = super::shared_protocol::jet_shared_acquire_ordered(
                            slots
                                .iter()
                                .map(|(_, slot)| slot.protocol.clone())
                                .collect(),
                        );
                        for mut delta in staged {
                            let slot = slots
                                .iter()
                                .find_map(|(index, slot)| {
                                    (*index == delta.shared_index).then_some(slot)
                                })
                                .expect("staged Shared delta has a locked slot");
                            let current = slot
                                .value
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .clone();
                            let (_, updated) = self.eval_tlambda_mut_arg(
                                delta.lambda,
                                current,
                                &mut delta.captured,
                            )?;
                            *slot
                                .value
                                .lock()
                                .unwrap_or_else(|error| error.into_inner()) = updated;
                        }
                        drop(permits);
                        for lam in frame.on_commit.into_iter().rev() {
                            let _ = self.eval_tlambda(lam, Vec::new(), scope)?;
                        }
                        Ok(Flow::Normal)
                    }
                    Ok(Flow::Return(v)) => {
                        // Auto-snapshot restore + rollback hooks on early return
                        // (including `return Err(...)` / `?`).
                        for (place, snap) in frame.snapshots.into_iter().rev() {
                            if scope.contains_key(&place) {
                                scope.insert(place, snap);
                            } else {
                                let mangled = format!("user_{place}");
                                if scope.contains_key(&mangled) {
                                    scope.insert(mangled, snap);
                                }
                            }
                        }
                        for lam in frame.on_rollback.into_iter().rev() {
                            let _ = self.eval_tlambda(lam, Vec::new(), scope)?;
                        }
                        Ok(Flow::Return(v))
                    }
                    other => other,
                }
            }
        }
    }

    pub(super) fn eval_if_cond(
        &mut self,
        cond: &'a TIfCond,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<bool, Diagnostic> {
        match cond {
            TIfCond::Plain(e) => as_bool(&self.eval_expr(e, scope)?, self.span()),
            TIfCond::And { left, right } => {
                Ok(self.eval_if_cond(left, scope)? && self.eval_if_cond(right, scope)?)
            }
            TIfCond::IsNone { subj } => {
                Ok(matches!(
                    self.eval_expr(subj, scope)?,
                    CtValue::Failed(CtReport::Clean(_)) | CtValue::Unit
                ))
            }
            TIfCond::IfLet { pattern, subj } => {
                let value = self.eval_expr(subj, scope)?;
                if let TPatternPosition::DataEntries { temp } = &pattern.position {
                    if let CtValue::Enum { args, .. } = &value {
                        if let Some((_, payload)) = args.first() {
                            scope.insert(temp.clone(), payload.clone());
                        }
                    }
                }
                match &pattern.pattern {
                    crate::AST::Pattern::Ok { binding, .. } => match value {
                        CtValue::Present(inner) => {
                            scope.insert(binding.clone(), *inner);
                            Ok(true)
                        }
                        _ => Ok(false),
                    },
                    crate::AST::Pattern::Err { binding, .. } => match value {
                        CtValue::Failed(CtReport::Told(inner)) => {
                            scope.insert(binding.clone(), *inner);
                            Ok(true)
                        }
                        _ => Ok(false),
                    },
                    crate::AST::Pattern::Present { binding, .. } => match value {
                        CtValue::Present(inner) => {
                            scope.insert(binding.clone(), *inner);
                            Ok(true)
                        }
                        _ => Ok(false),
                    },
                    crate::AST::Pattern::Absent(_) => {
                        Ok(matches!(value, CtValue::Failed(CtReport::Clean(_)) | CtValue::Unit))
                    }
                    _ => bind_match_pattern(&pattern.pattern, &value, scope),
                }
            }
            TIfCond::Matches { pattern, subj } => {
                let value = self.eval_expr(subj, scope)?;
                bind_match_pattern(&pattern.pattern, &value, scope)
            }
        }
    }

    fn exec_infinite(
        &mut self,
        label: Option<&str>,
        body: &'a [TStmt],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
        loop {
            self.burn()?;
            match self.exec_stmts(body, scope)? {
                Flow::Normal | Flow::Continue => {}
                Flow::Break => break,
                Flow::BreakLabel(ref name) if label == Some(name.as_str()) => break,
                Flow::ContinueLabel(ref name) if label == Some(name.as_str()) => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }
}

fn returned_shared_guards(flow: &Flow) -> Vec<usize> {
    let Flow::Return(value) = flow else {
        return Vec::new();
    };
    fn collect(value: &CtValue, out: &mut Vec<usize>) {
        match value {
            CtValue::Struct { type_name, fields }
                if type_name == "__JetTirSharedGuard" =>
            {
                if let Some(index) = fields.iter().find_map(|(name, value)| {
                    match (name.as_str(), value) {
                        ("lease", CtValue::Int(index)) => usize::try_from(*index).ok(),
                        _ => None,
                    }
                }) {
                    if !out.contains(&index) {
                        out.push(index);
                    }
                }
            }
            CtValue::Struct { fields, .. } => {
                for (_, value) in fields {
                    collect(value, out);
                }
            }
            CtValue::List(values) => {
                for value in values {
                    collect(value, out);
                }
            }
            CtValue::Map(entries) => {
                for value in entries.values() {
                    collect(value, out);
                }
            }
            CtValue::Enum { args, .. } => {
                for (_, value) in args {
                    collect(value, out);
                }
            }
            CtValue::Present(value) | CtValue::Failed(CtReport::Told(value)) => {
                collect(value, out);
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    collect(value, &mut out);
    out
}

fn strip_user(name: &str) -> String {
    name.strip_prefix("user_").unwrap_or(name).to_string()
}

/// Bind a match-arm pattern against `value`. Returns `true` when the arm matches
/// (and payload locals were inserted into `scope`).
pub(super) fn bind_match_pattern(
    pattern: &crate::AST::Pattern,
    value: &CtValue,
    scope: &mut HashMap<String, CtValue>,
) -> Result<bool, Diagnostic> {
    use crate::AST::{Pattern, StructPatField};
    match pattern {
        Pattern::Variant { variant, bindings, .. } => {
            let (got, args) = match value {
                CtValue::Enum { variant, args, .. } => (variant.as_str(), args.as_slice()),
                CtValue::Present(inner) if variant == "Ok" => {
                    return bind_slots(bindings, &[(**inner).clone()], scope);
                }
                CtValue::Failed(CtReport::Told(inner)) if variant == "Err" => {
                    return bind_slots(bindings, &[(**inner).clone()], scope);
                }
                CtValue::Present(inner)
                    if variant == "Some" || variant == "Present" || variant == "Val" =>
                {
                    return bind_slots(bindings, &[(**inner).clone()], scope);
                }
                CtValue::Failed(CtReport::Clean(_)) | CtValue::Unit if variant == "None" || variant == "Absent" => {
                    return Ok(bindings.is_empty());
                }
                _ => return Ok(false),
            };
            if got != variant.as_str()
                && got
                    .strip_prefix(variant)
                    .is_none_or(|suffix| !suffix.starts_with('.'))
            {
                return Ok(false);
            }
            let positional: Vec<CtValue> = args.iter().map(|(_, v)| v.clone()).collect();
            bind_slots(bindings, &positional, scope)
        }
        Pattern::Ok { binding, .. } => match value {
            CtValue::Present(inner) => {
                scope.insert(binding.clone(), (**inner).clone());
                Ok(true)
            }
            _ => Ok(false),
        },
        Pattern::Err { binding, .. } => match value {
            CtValue::Failed(CtReport::Told(inner)) => {
                scope.insert(binding.clone(), (**inner).clone());
                Ok(true)
            }
            _ => Ok(false),
        },
        Pattern::Present { binding, .. } => match value {
            CtValue::Present(inner) => {
                scope.insert(binding.clone(), (**inner).clone());
                Ok(true)
            }
            _ => Ok(false),
        },
        Pattern::Absent(_) => Ok(matches!(value, CtValue::Failed(CtReport::Clean(_)) | CtValue::Unit)),
        Pattern::Range { lo, hi, .. } => match value {
            CtValue::Int(n) => Ok(*n >= *lo && *n <= *hi),
            CtValue::Char(c) => {
                let n = *c as i64;
                Ok(n >= *lo && n <= *hi)
            }
            _ => Ok(false),
        },
        Pattern::Or(alts, _) => {
            for alt in alts {
                // Or-patterns share binding names; try each until one matches.
                let mut trial = scope.clone();
                if bind_match_pattern(alt, value, &mut trial)? {
                    *scope = trial;
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Pattern::Struct { fields, .. } => {
            let CtValue::Struct {
                fields: values, ..
            } = value
            else {
                return Ok(false);
            };
            for field in fields {
                match field {
                    StructPatField::Bind { field, local, .. } => {
                        let Some((_, v)) = values.iter().find(|(n, _)| n == field) else {
                            return Ok(false);
                        };
                        scope.insert(local.clone(), v.clone());
                    }
                    StructPatField::Value { .. } => {
                        // Equality guards need expr eval — not needed for current parity suite.
                        return Ok(false);
                    }
                }
            }
            Ok(true)
        }
        Pattern::StrMatch { .. } | Pattern::BinMatch { .. } => Ok(false),
    }
}

fn bind_slots(
    slots: &[crate::AST::PatSlot],
    args: &[CtValue],
    scope: &mut HashMap<String, CtValue>,
) -> Result<bool, Diagnostic> {
    use crate::AST::PatSlot;
    if slots.len() > args.len() {
        return Ok(false);
    }
    for (i, slot) in slots.iter().enumerate() {
        match slot {
            PatSlot::Wildcard => {}
            PatSlot::Bind { name, .. } => {
                scope.insert(name.clone(), args[i].clone());
            }
            PatSlot::Range { lo, hi } => match &args[i] {
                CtValue::Int(n) if *n >= *lo && *n <= *hi => {}
                CtValue::Char(c) => {
                    let n = *c as i64;
                    if n < *lo || n > *hi {
                        return Ok(false);
                    }
                }
                _ => return Ok(false),
            },
        }
    }
    Ok(true)
}
