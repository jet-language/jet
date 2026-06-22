//! The tree-walking interpreter: `Interp` struct, fuel, control-flow `Flow`,
//! the `DevSink` output buffer, and the statement/expression evaluation spine.
//! Method dispatch (`eval_call`/`eval_method`/…) continues in `methods.rs`.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use crate::AST::{BinOp, BindPattern, EnumLitArg, Expr, Func, Stmt, StrPart, Type, UnOp};
use crate::Diagnostics::{Diagnostic, Span};

use super::Builtins::{as_bool, as_int, eval_binop};
use super::Diagnostics::{
    comptime_panic, index_oob, map_missing, overflow, slice_oob, unsupported, unsupported_expr,
};
use super::Value::{CtKey, CtValue};

/// Default step budget per binding (S26 rule 3). Compiler-internal, not a
/// user knob (philosophy: minimal configuration).
pub(super) const FUEL_BUDGET: u64 = 10_000_000;

/// Step budget for a whole-program `jet dev` interpretation (E2-M4). Larger
/// than the per-binding comptime budget because a `main()` does real work,
/// but still finite so a runaway loop surfaces as E2202 instead of hanging
/// the watch loop. Compiler-internal, not a user knob.
pub(super) const DEV_FUEL_BUDGET: u64 = 1_000_000_000;

/// Step budget for a single `jet repl` input (D-REPL-FUEL=A). Tighter than
/// `jet dev` because REPL snippets should be short demos; a runaway loop
/// surfaces as E1801 instead of hanging the session. The user can bypass
/// with `:run` (unbounded but still finite — this constant then applies to
/// the `:run`-spawned one-shot compile path, not the interpreter).
pub const REPL_FUEL_BUDGET: u64 = 10_000_000;

/// Control-flow signal threaded through statement execution.
pub(super) enum Flow {
    Normal,
    Break,
    Continue,
    Return(CtValue),
}

/// Where a [`Interp`] running in whole-program "dev" mode sends program
/// output. In pure comptime mode this is `None` and `print`/`eprint` never
/// reach the evaluator (the purity check rejects them as E0951 first).
///
/// The dev interpreter (E2-M4 `jet dev`) buffers stdout/stderr so the
/// watch loop can stream them; the bytes are produced exactly as the
/// compiled program would (`jet_show()` + a trailing newline), which the
/// differential battery enforces.
pub struct DevSink {
    pub stdout: String,
    pub stderr: String,
}

impl DevSink {
    pub fn new() -> Self {
        DevSink {
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}

impl Default for DevSink {
    fn default() -> Self {
        DevSink::new()
    }
}

pub(super) struct Interp<'a> {
    pub(super) funcs: &'a HashMap<String, &'a Func>,
    pub(super) base_dir: &'a Path,
    pub(super) fuel: u64,
    /// `Some` in whole-program dev mode (E2-M4): `print`/`eprint` write here
    /// instead of being rejected. `None` in pure comptime mode (M9.5).
    pub(super) sink: Option<&'a mut DevSink>,
}

impl<'a> Interp<'a> {
    pub(super) fn burn(&mut self, span: Span) -> Result<(), Diagnostic> {
        if self.fuel == 0 {
            // E2202 in whole-program dev mode (E2-M4): the interpreter ran out
            // of fuel, which almost always means an unbounded loop. E0952 in
            // pure comptime mode (M9.5).
            if self.sink.is_some() {
                return Err(Diagnostic::error(
                    "E2202",
                    "this program ran too long for `jet dev` to keep interpreting".to_string(),
                    format!(
                        "`jet dev` interprets your program to give instant feedback, but it ran more than {} steps without finishing — this usually means a loop that never ends",
                        DEV_FUEL_BUDGET
                    ),
                    "check the loop near here for a condition that never becomes false; run `jet run` to execute the real build with no step limit"
                        .to_string(),
                    Some(span),
                ));
            }
            return Err(Diagnostic::error(
                "E0952",
                "comptime evaluation used up its budget".to_string(),
                format!(
                    "this `comptime` value ran more than {} steps before compilation gave up — it may loop forever",
                    FUEL_BUDGET
                ),
                "make the computation finish in fewer steps, or compute it at runtime instead"
                    .to_string(),
                Some(span),
            ));
        }
        self.fuel -= 1;
        Ok(())
    }

    pub(super) fn exec_block(
        &mut self,
        stmts: &[Stmt],
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

    /// S74: bind the names of a destructuring target from a comptime value.
    pub(super) fn bind_pattern(
        &mut self,
        pat: &BindPattern,
        value: CtValue,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        match pat {
            BindPattern::Struct { fields, .. } => {
                let CtValue::Struct { fields: vals, .. } = value else {
                    return Err(comptime_panic(
                        "this value isn't a struct, so it can't be destructured with `{ }`",
                        pat.span(),
                    ));
                };
                for f in fields {
                    let v = vals
                        .iter()
                        .find(|(n, _)| n == &f.name)
                        .map(|(_, v)| v.clone())
                        .ok_or_else(|| {
                            comptime_panic(
                                &format!("this value has no field `{}`", f.name),
                                f.span,
                            )
                        })?;
                    scope.insert(f.name.clone(), v);
                }
            }
            BindPattern::List { elems, span } => {
                let CtValue::List(xs) = value else {
                    return Err(comptime_panic(
                        "this value isn't a list, so it can't be destructured with `[ ]`",
                        *span,
                    ));
                };
                if xs.len() != elems.len() {
                    return Err(comptime_panic(
                        &format!(
                            "this pattern needs exactly {} item{}, but the list has {}",
                            elems.len(),
                            if elems.len() == 1 { "" } else { "s" },
                            xs.len()
                        ),
                        *span,
                    ));
                }
                for (e, v) in elems.iter().zip(xs) {
                    scope.insert(e.name.clone(), v);
                }
            }
            BindPattern::Tuple { span, .. } => {
                return Err(comptime_panic(
                    "tuple destructuring isn't supported in comptime yet",
                    *span,
                ));
            }
        }
        Ok(())
    }

    pub(super) fn exec_stmt(
        &mut self,
        stmt: &Stmt,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
        match stmt {
            Stmt::Expr(e) => {
                self.eval(e, scope)?;
                Ok(Flow::Normal)
            }
            Stmt::Val(b) => {
                let v = self.eval(&b.init, scope)?;
                // S74: a destructuring target binds each field/element.
                if let Some(pat) = &b.pattern {
                    self.bind_pattern(pat, v, scope)?;
                } else {
                    scope.insert(b.name.clone(), v);
                }
                Ok(Flow::Normal)
            }
            Stmt::Assign {
                target,
                op,
                value,
                op_span,
            } => {
                self.exec_assign(target, *op, value, *op_span, scope)?;
                Ok(Flow::Normal)
            }
            Stmt::Return(e, _) => {
                let v = match e {
                    Some(e) => self.eval(e, scope)?,
                    None => CtValue::Unit,
                };
                Ok(Flow::Return(v))
            }
            Stmt::If(ifs) => self.exec_if(ifs, scope),
            Stmt::While { cond, body, span, .. } => {
                loop {
                    self.burn(*span)?;
                    let c = self.eval(cond, scope)?;
                    if !as_bool(&c, cond.span())? {
                        break;
                    }
                    match self.exec_block(body, scope)? {
                        Flow::Break => break,
                        Flow::Continue | Flow::Normal => {}
                        ret @ Flow::Return(_) => return Ok(ret),
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::For {
                var,
                var2,
                kind,
                body,
                span,
                ..
            } => self.exec_for(var, var2.as_ref(), kind, body, *span, scope),
            Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            } => {
                // Sema rewrites enum/equality arms into ordinary Bool
                // conditions, so a switch is a first-true-arm chain.
                let _ = self.eval(subject, scope)?;
                for arm in arms {
                    let c = self.eval(&arm.cond, scope)?;
                    if as_bool(&c, arm.cond.span())? {
                        return self.exec_block(&arm.body, scope);
                    }
                }
                if let Some(body) = else_body {
                    return self.exec_block(body, scope);
                }
                Ok(Flow::Normal)
            }
            Stmt::Break(_) => Ok(Flow::Break),
            Stmt::Continue(_) => Ok(Flow::Continue),
            Stmt::Loop { body, span, .. } => {
                loop {
                    self.burn(*span)?;
                    match self.exec_block(body, scope)? {
                        Flow::Break => break,
                        Flow::Continue | Flow::Normal => {}
                        ret @ Flow::Return(_) => return Ok(ret),
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::Unsafe { span, .. } => Err(unsupported("an `#Unsafe` block", *span)),
            // D-REGION1: allocation regions are a runtime/codegen construct; the
            // comptime interpreter has no arenas, so a `region` block is declined.
            Stmt::Region { span, .. } => Err(unsupported("a `region` block", *span)),
            Stmt::Caps { span, .. } => Err(unsupported("a `#Caps` block", *span)),
            // D-CTX1: the smart-context block is a runtime/codegen construct; the
            // comptime interpreter declines it (no thread-local context at compile time).
            Stmt::ContextBlock { span, .. } => Err(unsupported("a `#Context` block", *span)),
            // D-LABEL1: labeled `break @name`/`continue @name` need the compiled
            // backend's multi-level loop control; the interpreter declines them
            // honestly (like `@unsafe`) rather than approximate them.
            Stmt::BreakLabel(_, span) | Stmt::ContinueLabel(_, span) => {
                Err(unsupported("a labeled `break`/`continue`", *span))
            }
            // D-WHEN1 (ratified 2026-06-19): sema already selected the arm
            // (selected_then = Some(…)); execute only that arm. If sema didn't
            // run (None), skip both — matching codegen's dumb-emit behaviour.
            Stmt::ComptimeIf {
                then_body,
                else_body,
                selected_then,
                ..
            } => match selected_then {
                Some(true) => self.exec_block(then_body, scope),
                Some(false) => {
                    if let Some(eb) = else_body {
                        self.exec_block(eb, scope)
                    } else {
                        Ok(Flow::Normal)
                    }
                }
                None => Ok(Flow::Normal),
            },
        }
    }

    pub(super) fn exec_if(
        &mut self,
        ifs: &crate::AST::IfStmt,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
        let c = self.eval(&ifs.cond, scope)?;
        if as_bool(&c, ifs.cond.span())? {
            return self.exec_block(&ifs.then_body, scope);
        }
        match &ifs.else_branch {
            Some(crate::AST::ElseBranch::ElseIf(inner)) => self.exec_if(inner, scope),
            Some(crate::AST::ElseBranch::Else(body)) => self.exec_block(body, scope),
            None => Ok(Flow::Normal),
        }
    }

    fn exec_for(
        &mut self,
        var: &str,
        var2: Option<&(String, Span)>,
        kind: &crate::AST::ForKind,
        body: &[Stmt],
        span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
        match kind {
            crate::AST::ForKind::Range { start, end, step } => {
                let a = as_int(&self.eval(start, scope)?, start.span())?;
                let b = as_int(&self.eval(end, scope)?, end.span())?;
                // S22 (D-SG8): `a..b` is inclusive; `step n` strides by n
                // (sema guarantees a positive Int).
                let stride = match step {
                    Some(step) => as_int(&self.eval(step, scope)?, step.span())?.max(1),
                    None => 1,
                };
                let mut i = a;
                while i <= b {
                    self.burn(span)?;
                    scope.insert(var.to_string(), CtValue::Int(i));
                    match self.exec_block(body, scope)? {
                        Flow::Break => break,
                        Flow::Continue | Flow::Normal => {}
                        ret @ Flow::Return(_) => return Ok(ret),
                    }
                    i += stride;
                }
                Ok(Flow::Normal)
            }
            crate::AST::ForKind::In { collection } => {
                let c = self.eval(collection, scope)?;
                match c {
                    CtValue::List(items) => {
                        for item in items {
                            self.burn(span)?;
                            scope.insert(var.to_string(), item);
                            match self.exec_block(body, scope)? {
                                Flow::Break => break,
                                Flow::Continue | Flow::Normal => {}
                                ret @ Flow::Return(_) => return Ok(ret),
                            }
                        }
                        Ok(Flow::Normal)
                    }
                    CtValue::Map(m) => {
                        for (k, v) in m {
                            self.burn(span)?;
                            if let Some((v2name, _)) = var2 {
                                scope.insert(var.to_string(), k.to_value());
                                scope.insert(v2name.clone(), v);
                            } else {
                                scope.insert(var.to_string(), k.to_value());
                            }
                            match self.exec_block(body, scope)? {
                                Flow::Break => break,
                                Flow::Continue | Flow::Normal => {}
                                ret @ Flow::Return(_) => return Ok(ret),
                            }
                        }
                        Ok(Flow::Normal)
                    }
                    CtValue::Str(s) => {
                        // `for c in s.chars()` lowers the receiver to the string.
                        for ch in s.chars() {
                            self.burn(span)?;
                            scope.insert(var.to_string(), CtValue::Char(ch));
                            match self.exec_block(body, scope)? {
                                Flow::Break => break,
                                Flow::Continue | Flow::Normal => {}
                                ret @ Flow::Return(_) => return Ok(ret),
                            }
                        }
                        Ok(Flow::Normal)
                    }
                    _ => Err(unsupported("looping over this value", span)),
                }
            }
        }
    }

    fn exec_assign(
        &mut self,
        target: &crate::AST::LValue,
        op: Option<BinOp>,
        value: &Expr,
        op_span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        let rhs = self.eval(value, scope)?;
        match target {
            crate::AST::LValue::Local { name, name_span } => {
                let new = match op {
                    None => rhs,
                    Some(op) => {
                        let cur = scope
                            .get(name)
                            .cloned()
                            .ok_or_else(|| unsupported("this assignment", *name_span))?;
                        eval_binop(op, cur, rhs, op_span)?
                    }
                };
                scope.insert(name.clone(), new);
                Ok(())
            }
            crate::AST::LValue::Index {
                base, index, span, ..
            } => {
                // Only `name[key] = v` is supported (the common case).
                let Expr::Ident(bname, _) = base.as_ref() else {
                    return Err(unsupported("this indexed assignment", *span));
                };
                let key = self.eval(index, scope)?;
                let mut container = scope
                    .get(bname)
                    .cloned()
                    .ok_or_else(|| unsupported("this indexed assignment", *span))?;
                match &mut container {
                    CtValue::List(xs) => {
                        let i = as_int(&key, index.span())?;
                        let new = match op {
                            None => rhs,
                            Some(op) => {
                                let cur = list_get(xs, i, *span)?;
                                eval_binop(op, cur, rhs, op_span)?
                            }
                        };
                        if i < 0 || i as usize >= xs.len() {
                            return Err(index_oob(xs.len(), i, *span));
                        }
                        xs[i as usize] = new;
                    }
                    CtValue::Map(m) => {
                        let k = CtKey::from_value(key)
                            .ok_or_else(|| unsupported("this map key type", index.span()))?;
                        let new = match op {
                            None => rhs,
                            Some(op) => {
                                let cur = m.get(&k).cloned().unwrap_or(CtValue::Int(0));
                                eval_binop(op, cur, rhs, op_span)?
                            }
                        };
                        m.insert(k, new);
                    }
                    _ => return Err(unsupported("this indexed assignment", *span)),
                }
                scope.insert(bname.clone(), container);
                Ok(())
            }
        }
    }

    pub(super) fn eval(
        &mut self,
        e: &Expr,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        self.burn(e.span())?;
        match e {
            Expr::Int(n, _, _) => Ok(CtValue::Int(*n)),
            Expr::Float(f, _) => Ok(CtValue::Float(*f)),
            Expr::Bool(b, _) => Ok(CtValue::Bool(*b)),
            Expr::Char(c, _) => Ok(CtValue::Char(*c)),
            Expr::Str(parts, _) => {
                let mut s = String::new();
                for part in parts {
                    match part {
                        StrPart::Lit(t) => s.push_str(t),
                        StrPart::Interp(e) => s.push_str(&self.eval(e, scope)?.jet_show()),
                    }
                }
                Ok(CtValue::Str(s))
            }
            Expr::ListLit(elems, _) => {
                let mut xs = Vec::with_capacity(elems.len());
                for el in elems {
                    xs.push(self.eval(el, scope)?);
                }
                Ok(CtValue::List(xs))
            }
            Expr::MapLit(entries, _) => {
                let mut m = BTreeMap::new();
                for (k, v) in entries {
                    let key = CtKey::from_value(self.eval(k, scope)?)
                        .ok_or_else(|| unsupported("this map key type", k.span()))?;
                    let val = self.eval(v, scope)?;
                    m.insert(key, val);
                }
                Ok(CtValue::Map(m))
            }
            Expr::Ident(name, span) => scope
                .get(name)
                .cloned()
                .ok_or_else(|| unsupported(&format!("the name `{}`", name), *span)),
            Expr::Unary(op, inner, span) => {
                let v = self.eval(inner, scope)?;
                match (op, v) {
                    (UnOp::Neg, CtValue::Int(n)) => n
                        .checked_neg()
                        .map(CtValue::Int)
                        .ok_or_else(|| overflow("negate", *span)),
                    (UnOp::Neg, CtValue::Float(f)) => Ok(CtValue::Float(-f)),
                    (UnOp::Not, CtValue::Bool(b)) => Ok(CtValue::Bool(!b)),
                    _ => Err(unsupported("this operation", *span)),
                }
            }
            Expr::Binary(op, l, r, span) => {
                // Short-circuit && / || to match runtime.
                if matches!(op, BinOp::And | BinOp::Or) {
                    let lv = as_bool(&self.eval(l, scope)?, l.span())?;
                    if matches!(op, BinOp::And) && !lv {
                        return Ok(CtValue::Bool(false));
                    }
                    if matches!(op, BinOp::Or) && lv {
                        return Ok(CtValue::Bool(true));
                    }
                    let rv = as_bool(&self.eval(r, scope)?, r.span())?;
                    return Ok(CtValue::Bool(rv));
                }
                let lv = self.eval(l, scope)?;
                let rv = self.eval(r, scope)?;
                eval_binop(*op, lv, rv, *span)
            }
            Expr::Index {
                base, index, span, ..
            } => {
                let b = self.eval(base, scope)?;
                let i = self.eval(index, scope)?;
                match b {
                    CtValue::List(xs) => list_get(&xs, as_int(&i, index.span())?, *span),
                    CtValue::Map(m) => {
                        let k = CtKey::from_value(i)
                            .ok_or_else(|| unsupported("this map key type", index.span()))?;
                        m.get(&k).cloned().ok_or_else(|| map_missing(*span))
                    }
                    _ => Err(unsupported("indexing this value", *span)),
                }
            }
            Expr::Field(inner, member, span) => {
                if let Expr::Ident(type_name, _) = inner.as_ref() {
                    return Ok(CtValue::Enum {
                        type_name: type_name.clone(),
                        variant: member.clone(),
                        args: Vec::new(),
                    });
                }
                let v = self.eval(inner, scope)?;
                match v {
                    CtValue::Struct { fields, .. } => fields
                        .into_iter()
                        .find(|(name, _)| name == member)
                        .map(|(_, value)| value)
                        .ok_or_else(|| unsupported(&format!("the field `.{}`", member), *span)),
                    _ => Err(unsupported("field access on this value", *span)),
                }
            }
            Expr::StructLit {
                type_name, fields, ..
            } => {
                let mut out = Vec::with_capacity(fields.len());
                for (name, _, expr) in fields {
                    out.push((name.clone(), self.eval(expr, scope)?));
                }
                Ok(CtValue::Struct {
                    type_name: type_name.clone(),
                    fields: out,
                })
            }
            Expr::TupleLit(_, span, _) => Err(unsupported("tuple literals in comptime", *span)),
            Expr::EnumLit {
                type_name,
                variant,
                args,
                ..
            } => {
                let mut out = Vec::with_capacity(args.len());
                for arg in args {
                    match arg {
                        EnumLitArg::Positional(expr) => out.push((None, self.eval(expr, scope)?)),
                        EnumLitArg::Named { label, expr } => {
                            out.push((Some(label.clone()), self.eval(expr, scope)?))
                        }
                    }
                }
                Ok(CtValue::Enum {
                    type_name: type_name.clone(),
                    variant: variant.clone(),
                    args: out,
                })
            }
            Expr::Slice {
                base,
                start,
                end,
                span,
            } => {
                let b = self.eval(base, scope)?;
                let a = as_int(&self.eval(start, scope)?, start.span())?;
                let z = as_int(&self.eval(end, scope)?, end.span())?;
                match b {
                    CtValue::List(xs) => slice_inclusive(&xs, a, z, *span).map(CtValue::List),
                    CtValue::Str(s) => {
                        let chars: Vec<char> = s.chars().collect();
                        let n = chars.len() as i64;
                        if a < 0 || z < 0 || a > z || z >= n {
                            return Err(slice_oob(n, a, z, *span));
                        }
                        Ok(CtValue::Str(
                            chars[a as usize..=z as usize].iter().collect(),
                        ))
                    }
                    _ => Err(unsupported("slicing this value", *span)),
                }
            }
            Expr::Present(inner, _) => Ok(CtValue::Some(Box::new(self.eval(inner, scope)?))),
            Expr::Absent(_) => Ok(CtValue::None(Type::Int)),
            Expr::Call(call) => self.eval_call(&call.name, call.name_span, &call.args, scope),
            Expr::MethodCall {
                receiver,
                method,
                method_span,
                args,
                ..
            } => self.eval_method(receiver, method, *method_span, args, scope),
            Expr::If {
                cond,
                then_body,
                then_value,
                else_body,
                else_value,
                ..
            } => {
                let c = self.eval(cond, scope)?;
                if as_bool(&c, cond.span())? {
                    match self.exec_block(then_body, scope)? {
                        Flow::Return(v) => Ok(v),
                        _ => self.eval(then_value, scope),
                    }
                } else {
                    match self.exec_block(else_body, scope)? {
                        Flow::Return(v) => Ok(v),
                        _ => self.eval(else_value, scope),
                    }
                }
            }
            Expr::FanOut { callee, items, span } => self.eval_fan_out(callee, items, *span, scope),
            other => Err(unsupported_expr(other)),
        }
    }
}

// --- value helpers --------------------------------------------------------

fn list_get(xs: &[CtValue], i: i64, span: Span) -> Result<CtValue, Diagnostic> {
    if i < 0 || i as usize >= xs.len() {
        return Err(index_oob(xs.len(), i, span));
    }
    Ok(xs[i as usize].clone())
}

fn slice_inclusive(xs: &[CtValue], a: i64, b: i64, span: Span) -> Result<Vec<CtValue>, Diagnostic> {
    let n = xs.len() as i64;
    if a < 0 || b < 0 || a > b || b >= n {
        return Err(slice_oob(n, a, b, span));
    }
    Ok(xs[a as usize..=b as usize].to_vec())
}
