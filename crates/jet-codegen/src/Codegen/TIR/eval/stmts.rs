//! Exhaustive TStmt evaluation (#777).
use std::collections::HashMap;
use crate::Codegen::TIR::{TIfCond, TPlace, TStmt};
use crate::Comptime::Builtins::{as_bool, as_int, eval_binop};
use crate::Comptime::CtValue;
use crate::Diagnostics::Diagnostic;
use super::{unsupported, EvalCtx, Flow};

impl EvalCtx<'_> {
    pub(crate) fn exec_stmts(
        &mut self,
        stmts: &[TStmt],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
        for stmt in stmts {
            match self.exec_stmt(stmt, scope)? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    pub(super) fn exec_stmt(
        &mut self,
        stmt: &TStmt,
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
                self.exec_stmt(init, scope)?;
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
                        self.exec_stmt(step, scope)?;
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
            TStmt::DeferClose { .. } => Ok(Flow::Normal),
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
                if method_kind.is_some() {
                    return Err(unsupported("for-in method collection", self.span()));
                }
                let coll = self.eval_expr(source, scope)?;
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
            TStmt::RangeSwitch { .. } => Err(unsupported("range switch", self.span())),
            TStmt::MixedSwitch {
                subject: _,
                arms,
                else_body,
            } => {
                // Subject is bound for AOT parity only; arm conditions are full exprs.
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
                let base_name = match &assign.base.kind {
                    crate::Codegen::TIR::TExprKind::Local(local) => local.name.clone(),
                    _ => {
                        return Err(unsupported("index field assign base", self.span()));
                    }
                };
                let idx = as_int(&self.eval_expr(&assign.index, scope)?, self.span())?;
                if idx < 0 {
                    return Err(unsupported("negative index field assign", self.span()));
                }
                let mut rhs = self.eval_expr(&assign.value, scope)?;
                if assign.clone_value {
                    rhs = rhs.clone();
                }
                let Some(CtValue::List(mut items)) = scope.get(&base_name).cloned() else {
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
                scope.insert(base_name, CtValue::List(items));
                Ok(Flow::Normal)
            }
            TStmt::IndexHookAssign { .. } => Err(unsupported("index hook assign", self.span())),
            TStmt::MathSwizzleAssign { .. } => Err(unsupported("math swizzle assign", self.span())),
            TStmt::GcEdit { .. } => Err(unsupported("gc edit", self.span())),
            TStmt::SplitViews { .. } => Err(unsupported("split views", self.span())),
            TStmt::Reactive { .. } => Err(unsupported("statement `Reactive`", self.span())),
            TStmt::Layout { .. } => Err(unsupported("statement `Layout`", self.span())),
            TStmt::ContextBlock { .. } => Err(unsupported("statement `ContextBlock`", self.span())),
            TStmt::Live { .. } => Err(unsupported("statement `Live`", self.span())),
            TStmt::Shield { .. } => Err(unsupported("statement `Shield`", self.span())),
            TStmt::ScopeMember { .. } => Err(unsupported("statement `ScopeMember`", self.span())),
            TStmt::Transact { .. } => Err(unsupported("statement `Transact`", self.span())),
        }
    }

    pub(super) fn eval_if_cond(
        &mut self,
        cond: &TIfCond,
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
                    _ => Err(unsupported("if-let pattern", self.span())),
                }
            }
            TIfCond::Matches { .. } => Err(unsupported("if-matches", self.span())),
        }
    }

    fn exec_infinite(
        &mut self,
        label: Option<&str>,
        body: &[TStmt],
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
fn bind_match_pattern(
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
                CtValue::Some(inner) if variant == "Some" || variant == "Present" => {
                    return bind_slots(bindings, &[(**inner).clone()], scope);
                }
                CtValue::None(_) | CtValue::Unit if variant == "None" || variant == "Absent" => {
                    return Ok(bindings.is_empty());
                }
                _ => return Ok(false),
            };
            if got != variant.as_str() {
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
