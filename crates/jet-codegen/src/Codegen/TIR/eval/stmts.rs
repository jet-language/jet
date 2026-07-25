//! Exhaustive TStmt evaluation (#777).
use std::collections::HashMap;
use crate::Codegen::TIR::{TIfCond, TPlace, TStmt};
use crate::Comptime::Builtins::{as_bool, as_int, eval_binop};
use crate::Comptime::CtValue;
use crate::Diagnostics::Diagnostic;
use super::{unsupported, EvalCtx, Flow};

impl EvalCtx<'_> {
    pub(super) fn exec_stmts(
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
                while if step_v > 0 { i <= end_v } else { i >= end_v } {
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
            TStmt::Inline(body) | TStmt::DebugOnly(body) | TStmt::Unsafe(body) | TStmt::Region(body) => {
                self.exec_stmts(body, scope)
            }
            TStmt::LineMarker(_) => Ok(Flow::Normal),
            TStmt::DeferClose { .. } => Ok(Flow::Normal),
            TStmt::ForIn { .. } => Err(unsupported("for-in", self.span())),
            TStmt::EnumMatch { .. } => Err(unsupported("enum match", self.span())),
            TStmt::RangeSwitch { .. } => Err(unsupported("range switch", self.span())),
            TStmt::MixedSwitch { .. } => Err(unsupported("mixed switch", self.span())),
            TStmt::IndexAssign { .. } => Err(unsupported("index assign", self.span())),
            TStmt::IndexFieldAssign(_) => Err(unsupported("index field assign", self.span())),
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
            TIfCond::IfLet { .. } => Err(unsupported("if-let", self.span())),
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
