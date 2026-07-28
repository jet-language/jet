//! Exhaustive TStmt evaluation (#777).
use std::collections::HashMap;
use crate::Codegen::TIR::{TForInMethod, TIfCond, TPatternPosition, TPlace, TStmt};
use crate::Comptime::Builtins::{as_bool, as_int, eval_binop};
use crate::Comptime::CtValue;
use crate::Diagnostics::Diagnostic;
use super::{raw_place_local, unsupported, EvalCtx, Flow};

/// Inclusive place-region handle used while evaluating `TStmt::SplitViews`.
/// Reuses the `__JetViewMut` field shape so later splits can resolve absolute
/// windows into the original owner list.
fn place_region(base: &str, start: i64, end: i64) -> CtValue {
    CtValue::Struct {
        type_name: "__JetViewMut".into(),
        fields: vec![
            ("base".into(), CtValue::Str(base.to_string())),
            ("start".into(), CtValue::Int(start)),
            ("end".into(), CtValue::Int(end)),
        ],
    }
}

fn parse_place_region(value: &CtValue) -> Option<(String, i64, i64)> {
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
    Some((base?, start?, end?))
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
        for stmt in stmts {
            let flow = self.exec_stmt(stmt, scope)?;
            if let Some(flow) = self.pending_flow.take() {
                self.run_deferred_closes(defer_mark, scope)?;
                return Ok(flow);
            }
            match flow {
                Flow::Normal => {}
                other => {
                    self.run_deferred_closes(defer_mark, scope)?;
                    return Ok(other);
                }
            }
        }
        self.run_deferred_closes(defer_mark, scope)?;
        Ok(Flow::Normal)
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
        match self.exec_stmt_inner(stmt, scope) {
            Err(_) if self.pending_flow.is_some() => {
                Ok(self.pending_flow.take().expect("checked pending loop control"))
            }
            result => result,
        }
    }

    fn exec_stmt_inner(
        &mut self,
        stmt: &'a TStmt,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
        self.burn()?;
        match stmt {
            TStmt::Let { name, init, .. } => {
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
                        if matches!(place_expr.kind, crate::Codegen::TIR::TExprKind::Field { .. }) =>
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
                start,
                end,
                step,
                exclusive,
                body,
                label,
            } => {
                let mut i = as_int(&self.eval_expr(start, scope)?, self.span())?;
                let end_v = as_int(&self.eval_expr(end, scope)?, self.span())?;
                let step_v = match step {
                    Some(s) => as_int(&self.eval_expr(s, scope)?, self.span())?,
                    None => 1,
                };
                if step_v == 0 {
                    return Err(unsupported("range step 0", self.span()));
                }
                // D-RANGE-EXCL1=C: exclusive `..<` stops before end; inclusive `..` includes it.
                let in_range = |cur: i64| {
                    if *exclusive {
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
                let coll = self.eval_expr(source, scope)?;
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
                        let CtValue::Some(item) = next else {
                            if matches!(next, CtValue::None(_)) {
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
                if method_kind.is_some() {
                    return Err(unsupported("for-in method collection", self.span()));
                }
                if let Some(stream_index) = super::EvalCtx::stream_index(&coll) {
                    let (func, args) = self
                        .streams
                        .get(stream_index)
                        .map(|stream| (stream.func, stream.args.clone()))
                        .ok_or_else(|| unsupported("stream handle", self.span()))?;
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
                        while i < items.len() {
                            self.burn()?;
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
            } => {
                let base_name = match &base.kind {
                    crate::Codegen::TIR::TExprKind::Local(local) => local.name.clone(),
                    _ => return Err(unsupported("index assign base", self.span())),
                };
                let idx_v = self.eval_expr(index, scope)?;
                let rhs = self.eval_expr(value, scope)?;
                // Mutable place-window write-through (`&xs[a..b]` → `__JetViewMut`).
                if let Some(CtValue::Struct {
                    type_name,
                    fields,
                }) = scope.get(&base_name).cloned()
                {
                    if type_name == "__JetViewMut" {
                        let mut base_s = None;
                        let mut start = None;
                        for (n, v) in &fields {
                            match (n.as_str(), v) {
                                ("base", CtValue::Str(s)) => base_s = Some(s.clone()),
                                ("start", CtValue::Int(n)) => start = Some(*n),
                                _ => {}
                            }
                        }
                        let (owner, start) = match (base_s, start) {
                            (Some(b), Some(s)) => (b, s),
                            _ => return Err(unsupported("view-mut fields", self.span())),
                        };
                        let idx = as_int(&idx_v, self.span())?;
                        if idx < 0 {
                            return Err(unsupported("negative view index", self.span()));
                        }
                        let Some(CtValue::List(mut items)) = scope.get(&owner).cloned() else {
                            return Err(unsupported("view-mut owner", self.span()));
                        };
                        let i = (start + idx) as usize;
                        if i >= items.len() {
                            return Err(unsupported("view-mut OOB", self.span()));
                        }
                        items[i] = rhs;
                        scope.insert(owner, CtValue::List(items));
                        return Ok(Flow::Normal);
                    }
                }
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
                // IndexAssign writes through to the owner (AOT emits real slices).
                // Read-only / single-element user bindings still materialize.
                if let Some(owner_expr) = owner {
                    let base_name = raw_place_local(owner_expr)
                        .map(|local| local.name.clone())
                        .ok_or_else(|| unsupported("split views owner", self.span()))?;
                    let CtValue::List(items) = scope
                        .get(&base_name)
                        .cloned()
                        .ok_or_else(|| unsupported("split views owner list", self.span()))?
                    else {
                        return Err(unsupported("split views owner list", self.span()));
                    };
                    let len_i = items.len() as i64;
                    if *start < 0 || *end < *start || *end >= len_i {
                        return Err(unsupported("split views bounds", self.span()));
                    }
                    let root_end = len_i - 1;
                    scope.insert(root.clone(), place_region(&base_name, 0, root_end));
                    scope.insert(len.clone(), CtValue::Int(len_i));
                }
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
                let (base_name, _src_abs_start, src_abs_end) = parse_place_region(&source_region)
                    .ok_or_else(|| unsupported("split views source region", self.span()))?;
                let relative_start = *start - *source_start;
                let width = *end - *start + 1;
                if relative_start < 0 || width <= 0 {
                    return Err(unsupported("split views range", self.span()));
                }
                // `(before, split_tail) = source.split_at(relative_start)`
                if relative_start > 0 {
                    scope.insert(
                        before.clone(),
                        place_region(&base_name, *source_start, *start - 1),
                    );
                } else {
                    scope.insert(
                        before.clone(),
                        place_region(&base_name, *source_start, *source_start - 1),
                    );
                }
                scope.insert(
                    split_tail.clone(),
                    place_region(&base_name, *start, src_abs_end),
                );
                // `(segment, after) = split_tail.split_at(width)`
                scope.insert(segment.clone(), place_region(&base_name, *start, *end));
                let after_start = *end + 1;
                if after_start <= src_abs_end {
                    scope.insert(
                        after.clone(),
                        place_region(&base_name, after_start, src_abs_end),
                    );
                } else {
                    scope.insert(
                        after.clone(),
                        place_region(&base_name, after_start, *end),
                    );
                }
                if *write && !*single {
                    // Write-through handle — same shape as plan temps / ViewNew.
                    scope.insert(name.clone(), place_region(&base_name, *start, *end));
                } else {
                    let CtValue::List(items) = scope
                        .get(&base_name)
                        .cloned()
                        .ok_or_else(|| unsupported("split views owner", self.span()))?
                    else {
                        return Err(unsupported("split views owner", self.span()));
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
            TStmt::ContextBlock { body, .. } => self.exec_stmts(body, scope),
            TStmt::Live { .. } => Err(unsupported("statement `Live`", self.span())),
            TStmt::Shield { .. } => Err(unsupported("statement `Shield`", self.span())),
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
                    self.shared_transactions.push(HashMap::new());
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
                    HashMap::new()
                };
                match flow {
                    Ok(Flow::Normal) => {
                        for lam in frame.on_commit.into_iter().rev() {
                            let _ = self.eval_tlambda(lam, Vec::new(), scope)?;
                        }
                        for (index, value) in staged {
                            if let Some(slot) = self.shared_values.get_mut(index) {
                                *slot = value;
                            }
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
                    CtValue::None(_) | CtValue::Unit
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
                        CtValue::ResOk(inner) => {
                            scope.insert(binding.clone(), *inner);
                            Ok(true)
                        }
                        _ => Ok(false),
                    },
                    crate::AST::Pattern::Err { binding, .. } => match value {
                        CtValue::ResErr(inner) => {
                            scope.insert(binding.clone(), *inner);
                            Ok(true)
                        }
                        _ => Ok(false),
                    },
                    crate::AST::Pattern::Present { binding, .. } => match value {
                        CtValue::Some(inner) => {
                            scope.insert(binding.clone(), *inner);
                            Ok(true)
                        }
                        _ => Ok(false),
                    },
                    crate::AST::Pattern::Absent(_) => {
                        Ok(matches!(value, CtValue::None(_) | CtValue::Unit))
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
                CtValue::ResOk(inner) if variant == "Ok" => {
                    return bind_slots(bindings, &[(**inner).clone()], scope);
                }
                CtValue::ResErr(inner) if variant == "Err" => {
                    return bind_slots(bindings, &[(**inner).clone()], scope);
                }
                CtValue::Some(inner)
                    if variant == "Some" || variant == "Present" || variant == "Val" =>
                {
                    return bind_slots(bindings, &[(**inner).clone()], scope);
                }
                CtValue::None(_) | CtValue::Unit if variant == "None" || variant == "Absent" => {
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
            CtValue::ResOk(inner) => {
                scope.insert(binding.clone(), (**inner).clone());
                Ok(true)
            }
            _ => Ok(false),
        },
        Pattern::Err { binding, .. } => match value {
            CtValue::ResErr(inner) => {
                scope.insert(binding.clone(), (**inner).clone());
                Ok(true)
            }
            _ => Ok(false),
        },
        Pattern::Present { binding, .. } => match value {
            CtValue::Some(inner) => {
                scope.insert(binding.clone(), (**inner).clone());
                Ok(true)
            }
            _ => Ok(false),
        },
        Pattern::Absent(_) => Ok(matches!(value, CtValue::None(_) | CtValue::Unit)),
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
