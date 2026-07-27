//! Exhaustive TExprKind evaluation (#777).
use std::collections::HashMap;
use crate::AST::{CtFloat, Type, UnOp};
use crate::Codegen::TIR::{
    ListSpreadPart, TCallArg, TCoreClosureKind, TExpr, TExprKind, TFnValueKind, TPlace, TStrPart,
};
use crate::Comptime::Builtins::{as_bool, as_int, eval_binop};
use crate::Comptime::{apply_core_call, apply_impure_core_call, CtValue};
use crate::Diagnostics::Diagnostic;
use super::builtins::eval_builtin;
use super::handles::eval_handle;
use super::{materialize_view_mut_window, unsupported, EvalCallable, EvalCtx, Flow};

fn handle_index(value: &CtValue, type_name: &str) -> Option<usize> {
    let CtValue::Struct {
        type_name: actual,
        fields,
    } = value
    else {
        return None;
    };
    (actual == type_name).then_some(())?;
    fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("index", CtValue::Int(index)) => Some(*index as usize),
        _ => None,
    })
}

fn struct_int(value: &CtValue, field: &str) -> Option<i64> {
    let CtValue::Struct { fields, .. } = value else {
        return None;
    };
    fields.iter().find_map(|(name, value)| match value {
        CtValue::Int(value) if name == field => Some(*value),
        _ => None,
    })
}

fn show_typed_value(value: &CtValue, ty: &Type, debug: bool) -> Option<String> {
    match (value, ty) {
        (CtValue::Int(value), ty) => {
            let (signed, _) = crate::Comptime::MathLayout::integer_type_layout(ty)?;
            Some(crate::Comptime::MathLayout::integer_show(*value, signed))
        }
        (CtValue::Some(value), Type::Option(inner)) => {
            Some(show_typed_value(value, inner, debug).unwrap_or_else(|| {
                if debug {
                    value.debug_rust()
                } else {
                    value.jet_show()
                }
            }))
        }
        (CtValue::None(_), Type::Option(_)) => Some("null".to_string()),
        (CtValue::List(values), Type::List(inner) | Type::FixedList { elem: inner, .. }) => {
            let parts = values
                .iter()
                .map(|value| {
                    show_typed_value(value, inner, debug).unwrap_or_else(|| {
                        if debug {
                            value.debug_rust()
                        } else {
                            value.jet_show()
                        }
                    })
                })
                .collect::<Vec<_>>();
            Some(format!("[{}]", parts.join(", ")))
        }
        _ => None,
    }
}

impl<'a> EvalCtx<'a> {
    pub(crate) fn eval_expr(
        &mut self,
        expr: &'a TExpr,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        self.burn()?;
        match &expr.kind {
            TExprKind::IntLit(n, _) => Ok(CtValue::Int(*n)),
            TExprKind::FloatLit(v) => {
                let is_f32 = matches!(&expr.ty, Type::Float32);
                Ok(CtValue::Float(CtFloat::literal(*v, is_f32)))
            }
            TExprKind::BoolLit(b) => Ok(CtValue::Bool(*b)),
            TExprKind::CharLit(c) => Ok(CtValue::Char(*c)),
            TExprKind::StrLit(parts) => {
                let mut out = String::new();
                for part in parts {
                    match part {
                        TStrPart::Lit(s) => out.push_str(s),
                        TStrPart::Interp(e, fmt) => {
                            let v = self.eval_expr(e, scope)?;
                            let text = match fmt {
                                crate::AST::StrFormat::Debug => {
                                    show_typed_value(&v, &e.ty, true)
                                        .unwrap_or_else(|| self.debug_value(&v))
                                }
                                crate::AST::StrFormat::Display => {
                                    show_typed_value(&v, &e.ty, false)
                                        .unwrap_or(self.show_value(&v, scope)?)
                                }
                            };
                            out.push_str(&text);
                        }
                    }
                }
                Ok(CtValue::Str(out))
            }
            TExprKind::Local(local) => scope
                .get(&local.name)
                .cloned()
                .or_else(|| self.globals.get(&local.name).cloned())
                .ok_or_else(|| unsupported(&format!("unbound `{}`", local.name), self.span())),
            TExprKind::InlineBlock(stmts) => {
                // Raw comptime fragments reach TIR before sema rewrites the
                // private yielding-loop sends to `List.push`. Collect them
                // here; checked runtime programs use the ordinary List path.
                let raw_collecting = matches!(&expr.ty, Type::List(_))
                    && matches!(
                        stmts.last(),
                        Some(
                            crate::Codegen::TIR::TStmt::ForIn { .. }
                                | crate::Codegen::TIR::TStmt::Range { .. }
                                | crate::Codegen::TIR::TStmt::CountedLoop { .. }
                        )
                    );
                if raw_collecting {
                    self.collecting_items.push(Vec::new());
                    let flow = self.exec_stmts(stmts, scope);
                    let items = self
                        .collecting_items
                        .pop()
                        .expect("raw collecting loop installs one item sink");
                    return match flow? {
                        Flow::Normal => Ok(CtValue::List(items)),
                        Flow::Return(value) => {
                            self.pending_return = Some(value);
                            Ok(CtValue::Unit)
                        }
                        other => {
                            self.pending_flow = Some(other);
                            Err(unsupported("pending loop control", self.span()))
                        }
                    };
                }
                let Some((tail, prefix)) = stmts.split_last() else {
                    return Ok(CtValue::Unit);
                };
                match self.exec_stmts(prefix, scope)? {
                    Flow::Normal => {}
                    Flow::Return(value) => {
                        self.pending_return = Some(value);
                        return Ok(CtValue::Unit);
                    }
                    other => {
                        self.pending_flow = Some(other);
                        return Err(unsupported("pending loop control", self.span()));
                    }
                }
                if let crate::Codegen::TIR::TStmt::Loop { label, body } = tail {
                    return self.exec_loop_value(label.as_deref(), body, scope);
                }
                match tail {
                    crate::Codegen::TIR::TStmt::ExprStmt(value) => self.eval_expr(value, scope),
                    crate::Codegen::TIR::TStmt::Return(value) => {
                        let value = match value {
                            Some(value) => self.eval_expr(value, scope)?,
                            None => CtValue::Unit,
                        };
                        self.pending_return = Some(value);
                        Ok(CtValue::Unit)
                    }
                    _ => match self.exec_stmt(tail, scope)? {
                        Flow::Normal => Ok(CtValue::Unit),
                        Flow::Return(value) => {
                            self.pending_return = Some(value);
                            Ok(CtValue::Unit)
                        }
                        other => {
                            self.pending_flow = Some(other);
                            Err(unsupported("pending loop control", self.span()))
                        }
                    },
                }
            }
            TExprKind::Unit | TExprKind::DefaultLit | TExprKind::Uninit => Ok(CtValue::Unit),
            TExprKind::CtLit(v) => Ok(v.clone()),
            TExprKind::ConstRef(name) => self
                .globals
                .get(name)
                .cloned()
                .ok_or_else(|| unsupported(&format!("const `{name}`"), self.span())),
            TExprKind::Print(inner) => {
                let v = self.eval_expr(inner, scope)?;
                if self.pending_return.is_some() {
                    return Ok(CtValue::Unit);
                }
                let shown = match show_typed_value(&v, &inner.ty, false) {
                    Some(shown) => shown,
                    None => self.show_value(&v, scope)?,
                };
                self.write_print(&shown, false)?;
                Ok(CtValue::Unit)
            }
            TExprKind::Drop(inner) | TExprKind::Close(inner) => {
                let _ = self.eval_expr(inner, scope)?;
                Ok(CtValue::Unit)
            }
            TExprKind::Binary { op, lhs, rhs, .. } => {
                let l = self.eval_expr(lhs, scope)?;
                let r = self.eval_expr(rhs, scope)?;
                if let Type::IntN { signed, bits } = &lhs.ty {
                    let a = as_int(&l, self.span())?;
                    let b = as_int(&r, self.span())?;
                    let right_signed =
                        crate::Comptime::MathLayout::integer_type_layout(&rhs.ty)
                            .map(|(signed, _)| signed)
                            .unwrap_or(true);
                    return crate::Comptime::MathLayout::integer_binop(
                        *op,
                        a,
                        b,
                        *signed,
                        *bits,
                        right_signed,
                        self.span(),
                    );
                }
                eval_binop(*op, l, r, self.span())
            }
            TExprKind::Unary { op, operand } => {
                let v = self.eval_expr(operand, scope)?;
                match (*op, v) {
                    (UnOp::Neg, CtValue::Int(n))
                        if matches!(&operand.ty, Type::IntN { signed: true, .. }) =>
                    {
                        let (_, bits) =
                            crate::Comptime::MathLayout::integer_type_layout(&operand.ty)
                                .expect("IntN layout");
                        crate::Comptime::MathLayout::integer_neg(n, bits, self.span())
                    }
                    (UnOp::Neg, CtValue::Int(n)) => n
                        .checked_neg()
                        .map(CtValue::Int)
                        .ok_or_else(|| unsupported("integer negation overflow", self.span())),
                    (UnOp::Neg, CtValue::Float(n)) => Ok(CtValue::Float(n.neg())),
                    (UnOp::Neg, CtValue::BigInt(n)) => Ok(CtValue::BigInt(n.neg())),
                    (UnOp::Not, CtValue::Bool(b)) => Ok(CtValue::Bool(!b)),
                    _ => Err(unsupported("unary form", self.span())),
                }
            }
            TExprKind::CompareChain { operands, ops, .. } => {
                let mut vals = Vec::with_capacity(operands.len());
                for o in operands {
                    vals.push(self.eval_expr(o, scope)?);
                }
                for (i, op) in ops.iter().enumerate() {
                    let part = if let Type::IntN { signed, bits } = &operands[i].ty {
                        let right_signed =
                            crate::Comptime::MathLayout::integer_type_layout(&operands[i + 1].ty)
                                .map(|(signed, _)| signed)
                                .unwrap_or(true);
                        crate::Comptime::MathLayout::integer_binop(
                            *op,
                            as_int(&vals[i], self.span())?,
                            as_int(&vals[i + 1], self.span())?,
                            *signed,
                            *bits,
                            right_signed,
                            self.span(),
                        )?
                    } else {
                        eval_binop(*op, vals[i].clone(), vals[i + 1].clone(), self.span())?
                    };
                    if !as_bool(&part, self.span())? {
                        return Ok(CtValue::Bool(false));
                    }
                }
                Ok(CtValue::Bool(true))
            }
            TExprKind::Call { name, args } => self.eval_call(name, args, scope),
            TExprKind::IfExpr {
                cond,
                then_body,
                then_value,
                else_body,
                else_value,
            } => {
                if self.eval_if_cond(cond, scope)? {
                    match self.exec_stmts(then_body, scope)? {
                        Flow::Return(v) => return Ok(v),
                        Flow::Normal => {}
                        other => {
                            self.pending_flow = Some(other);
                            return Err(unsupported("pending loop control", self.span()));
                        }
                    }
                    self.eval_expr(then_value, scope)
                } else {
                    match self.exec_stmts(else_body, scope)? {
                        Flow::Return(v) => return Ok(v),
                        Flow::Normal => {}
                        other => {
                            self.pending_flow = Some(other);
                            return Err(unsupported("pending loop control", self.span()));
                        }
                    }
                    self.eval_expr(else_value, scope)
                }
            }
            TExprKind::BuiltinMethod { recv, op, args } => {
                if matches!(op, crate::Codegen::TIR::TBuiltinOp::ViewMutNew { .. }) {
                    let base_name = match &recv.kind {
                        TExprKind::Local(local) => local.name.clone(),
                        TExprKind::Borrow { place, .. } => match &place.kind {
                            TExprKind::Local(local) => local.name.clone(),
                            _ => {
                                return Err(unsupported("view-mut base", self.span()));
                            }
                        },
                        _ => return Err(unsupported("view-mut base", self.span())),
                    };
                    let start = as_int(&self.eval_expr(&args[0], scope)?, self.span())?;
                    let end = as_int(&self.eval_expr(&args[1], scope)?, self.span())?;
                    let CtValue::List(xs) = scope
                        .get(&base_name)
                        .cloned()
                        .ok_or_else(|| unsupported("view-mut unbound base", self.span()))?
                    else {
                        return Err(unsupported("view-mut list base", self.span()));
                    };
                    if start < 0 || end < start || end as usize >= xs.len() {
                        return Err(unsupported("view-mut bounds", self.span()));
                    }
                    return Ok(CtValue::Struct {
                        type_name: "__JetViewMut".into(),
                        fields: vec![
                            ("base".into(), CtValue::Str(base_name)),
                            ("start".into(), CtValue::Int(start)),
                            ("end".into(), CtValue::Int(end)),
                        ],
                    });
                }
                let mut r = self.eval_expr(recv, scope)?;
                // `__JetViewMut` is a write-through handle; read builtins see the
                // inclusive window as a List (same surface as View after ViewNew).
                // Do not write the temporary List back over the ViewMut binding.
                let mut skip_view_mut_wb = false;
                if let CtValue::Struct {
                    type_name,
                    fields,
                } = &r
                {
                    if type_name == "__JetViewMut"
                        && matches!(
                            *op,
                            crate::Codegen::TIR::TBuiltinOp::LenList
                                | crate::Codegen::TIR::TBuiltinOp::IsEmpty
                                | crate::Codegen::TIR::TBuiltinOp::GetList
                                | crate::Codegen::TIR::TBuiltinOp::First
                                | crate::Codegen::TIR::TBuiltinOp::Last
                                | crate::Codegen::TIR::TBuiltinOp::Contains
                                | crate::Codegen::TIR::TBuiltinOp::IndexOf
                                | crate::Codegen::TIR::TBuiltinOp::JoinSep
                        )
                    {
                        r = materialize_view_mut_window(fields, scope, self.span())?;
                        skip_view_mut_wb = true;
                    }
                }
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(a, scope)?);
                }
                let result = eval_builtin(op, &mut r, argv, self.span())?;
                if !skip_view_mut_wb {
                    self.write_back_place(recv, r, scope)?;
                }
                Ok(result)
            }
            TExprKind::HandleMethod { recv, op, args } => {
                let mut r = self.eval_expr(recv, scope)?;
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(a, scope)?);
                }
                if let Some(index) = handle_index(&r, "__JetTirClock") {
                    let delta = argv.first().and_then(|value| match value {
                        CtValue::Int(value) => Some(*value),
                        CtValue::Struct { type_name, fields }
                            if type_name == crate::Syntax::DURATION_TYPE
                                || type_name == "Duration" =>
                        {
                            fields.iter().find_map(|(name, v)| match (name.as_str(), v) {
                                ("ms", CtValue::Int(ms)) => Some(*ms),
                                _ => None,
                            })
                        }
                        _ => None,
                    });
                    let span = self.span();
                    let Some(clock) = self.clocks.get_mut(index) else {
                        return Err(unsupported("clock handle", span));
                    };
                    let result = match op {
                        crate::Codegen::TIR::THandleOp::ClockNow => CtValue::Int(*clock),
                        // D-DET-CAPAPI: `advance(to_ms)` sets an absolute instant;
                        // `tick` / `wait` advance relatively. Match AOT `jet_clock_*`.
                        crate::Codegen::TIR::THandleOp::ClockAdvance => {
                            let Some(to_ms) = delta else {
                                return Err(unsupported("clock advance target", span));
                            };
                            *clock = to_ms;
                            CtValue::Int(*clock)
                        }
                        crate::Codegen::TIR::THandleOp::ClockTick
                        | crate::Codegen::TIR::THandleOp::ClockWait => {
                            let Some(delta) = delta else {
                                return Err(unsupported("clock delta", span));
                            };
                            *clock = clock.saturating_add(delta);
                            CtValue::Int(*clock)
                        }
                        _ => return Err(unsupported("clock method", self.span())),
                    };
                    return Ok(result);
                }
                if matches!(
                    op,
                    crate::Codegen::TIR::THandleOp::ExpiringMethod { .. }
                ) {
                    let deadline = struct_int(&r, "deadline")
                        .ok_or_else(|| unsupported("expiring deadline", self.span()))?;
                    let clock_index = argv
                        .first()
                        .and_then(|clock| handle_index(clock, "__JetTirClock"))
                        .ok_or_else(|| unsupported("expiring clock", self.span()))?;
                    let now = *self
                        .clocks
                        .get(clock_index)
                        .ok_or_else(|| unsupported("expiring clock handle", self.span()))?;
                    let valid = now <= deadline;
                    let result = match op {
                        crate::Codegen::TIR::THandleOp::ExpiringMethod { method }
                            if method == "is_valid" =>
                        {
                            CtValue::Bool(valid)
                        }
                        crate::Codegen::TIR::THandleOp::ExpiringMethod { method }
                            if method == "get" =>
                        {
                            let value = if valid {
                                let CtValue::Struct { fields, .. } = &r else {
                                    unreachable!();
                                };
                                fields
                                    .iter()
                                    .find_map(|(name, value)| {
                                        (name == "value").then(|| value.clone())
                                    })
                                    .unwrap_or(CtValue::Unit)
                            } else {
                                CtValue::Str("expired".to_string())
                            };
                            if valid {
                                CtValue::ResOk(Box::new(value))
                            } else {
                                CtValue::ResErr(Box::new(value))
                            }
                        }
                        _ => return Err(unsupported("expiring method", self.span())),
                    };
                    return Ok(result);
                }
                let result = eval_handle(op, &mut r, &mut argv, self.span())?;
                self.write_back_place(recv, r, scope)?;
                // `Rng.shuffle(&list)` mutates the list arg in place. Fragment
                // lowering may keep `&deck` as Local (Write convention on the
                // AST CallArg) rather than wrapping TExprKind::Borrow.
                let force_arg_wb = matches!(*op, crate::Codegen::TIR::THandleOp::RngShuffle);
                for (place, value) in args.iter().zip(argv.into_iter()) {
                    if force_arg_wb {
                        self.write_back_place(place, value, scope)?;
                        continue;
                    }
                    if matches!(place.kind, TExprKind::Borrow { .. }) {
                        self.write_back_place(place, value, scope)?;
                    }
                }
                Ok(result)
            }
            TExprKind::CoreCall {
                module,
                method,
                args,
                source_span,
                ..
            } => {
                if module == "core.data" {
                    return self.eval_core_data_call(method, args, &expr.ty, scope);
                }
                if module == "core.mem" && method == "volatile_write" && args.len() == 2 {
                    let pointer = self.eval_expr(&args[0], scope)?;
                    let value = self.eval_expr(&args[1], scope)?;
                    let CtValue::Struct { type_name, fields } = pointer else {
                        return Err(unsupported("raw pointer carrier", self.span()));
                    };
                    if type_name != "__JetRawLocal" {
                        return Err(unsupported("raw pointer target", self.span()));
                    }
                    let name = fields.iter().find_map(|(field, value)| {
                        match (field.as_str(), value) {
                            ("name", CtValue::Str(name)) => Some(name.clone()),
                            _ => None,
                        }
                    });
                    let Some(name) = name else {
                        return Err(unsupported("raw pointer local", self.span()));
                    };
                    scope.insert(name, value);
                    return Ok(CtValue::Unit);
                }
                if module == "core.mem" && method == "volatile_read" && args.len() == 1 {
                    let pointer = self.eval_expr(&args[0], scope)?;
                    let CtValue::Struct { type_name, fields } = pointer else {
                        return Err(unsupported("raw pointer carrier", self.span()));
                    };
                    if type_name != "__JetRawLocal" {
                        return Err(unsupported("raw pointer target", self.span()));
                    }
                    let name = fields.iter().find_map(|(field, value)| {
                        match (field.as_str(), value) {
                            ("name", CtValue::Str(name)) => Some(name.as_str()),
                            _ => None,
                        }
                    });
                    return name
                        .and_then(|name| scope.get(name).cloned())
                        .ok_or_else(|| unsupported("raw pointer local", self.span()));
                }
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(a, scope)?);
                }
                if module == "jet.crypto"
                    && method == "__signing_generate"
                    && argv.is_empty()
                {
                    return Ok(CtValue::ResOk(Box::new(CtValue::Int(1))));
                }
                if module == "jet.crypto"
                    && method == "__signing_public"
                    && argv.len() == 1
                {
                    return Ok(argv.remove(0));
                }
                if module == "core.browser" && self.runtime_execution {
                    return super::browser::core_call(method, argv, *source_span);
                }
                if !self.runtime_execution && module == "core.net" && method == "fetch" {
                    return crate::Comptime::eval_net_fetch(
                        &argv,
                        self.embed_inputs.as_deref_mut(),
                        *source_span,
                    );
                }
                if !self.runtime_execution && module == "core.vault" {
                    return Err(crate::Comptime::vault_comptime_denied(
                        module,
                        method,
                        *source_span,
                    ));
                }
                let is_tier2 =
                    crate::Comptime::is_tier2_core_call(module, method, self.repl_mode);
                if !is_tier2 {
                    return apply_core_call(module, method, argv, *source_span, self.repl_mode);
                }
                // Runtime deopt / `jet run` sets impure_depth>0 so Tier-2
                // ambient I/O matches AOT (env/fs/process). Pure comptime
                // keeps depth 0 and stays on apply_core_call (E3410).
                if self.impure_depth > 0 && self.allow_impure {
                    apply_impure_core_call(
                        module,
                        method,
                        argv,
                        *source_span,
                        &self.base_dir,
                        self.sink.as_deref_mut(),
                        self.repl_mode,
                        None,
                        None,
                    )
                } else if self.impure_depth == 0 {
                    apply_core_call(module, method, argv, *source_span, self.repl_mode)
                } else {
                    Err(Diagnostic::error(
                        "E3411",
                        format!(
                            "`{module}.{method}()` inside `#Impure` gate, but `--allow-impure` was not passed"
                        ),
                        "the `#Impure` block opts in to ambient comptime I/O, but the build flag is required so CI can audit builds that touch the host".to_string(),
                        "add `--allow-impure` to your `jet build` / `jet run` invocation".to_string(),
                        Some(*source_span),
                    ))
                }
            }
            TExprKind::StructLit {
                fields, as_trait, ..
            } => {
                let mut out = Vec::with_capacity(fields.len());
                for (name, val, _) in fields {
                    out.push((name.clone(), self.eval_expr(val, scope)?));
                }
                let type_name = as_trait
                    .as_ref()
                    .map(|(_, concrete)| concrete.clone())
                    .unwrap_or_else(|| match &expr.ty {
                        crate::AST::Type::Named(n) => n.clone(),
                        crate::AST::Type::Apply { name, .. } => name.clone(),
                        _ => "struct".into(),
                    });
                Ok(CtValue::Struct {
                    type_name,
                    fields: out,
                })
            }
            TExprKind::Field { recv, field, .. } => {
                let r = self.eval_expr(recv, scope)?;
                match r {
                    CtValue::Struct { fields, .. } => fields
                        .into_iter()
                        .find(|(n, _)| n == field)
                        .map(|(_, v)| v)
                        .ok_or_else(|| unsupported(&format!("field `{field}`"), self.span())),
                    _ => Err(unsupported("field recv", self.span())),
                }
            }
            TExprKind::ListLit(elems) => {
                let mut out = Vec::with_capacity(elems.len());
                for e in elems {
                    out.push(self.eval_expr(e, scope)?);
                }
                Ok(CtValue::List(out))
            }
            TExprKind::Clone(inner) => self.eval_expr(inner, scope),
            TExprKind::Present(inner) => {
                Ok(CtValue::Some(Box::new(self.eval_expr(inner, scope)?)))
            }
            TExprKind::Absent => Ok(CtValue::None(expr.ty.clone())),
            TExprKind::Ok(inner) => Ok(CtValue::ResOk(Box::new(self.eval_expr(inner, scope)?))),
            TExprKind::Err(inner) => Ok(CtValue::ResErr(Box::new(self.eval_expr(inner, scope)?))),
            TExprKind::TupleLit { fields, .. } => {
                let mut out = Vec::with_capacity(fields.len());
                for (name, e) in fields {
                    out.push((name.clone(), self.eval_expr(e, scope)?));
                }
                Ok(CtValue::Struct {
                    type_name: "tuple".into(),
                    fields: out,
                })
            }
            TExprKind::MapLit(entries) => {
                let mut m = std::collections::BTreeMap::new();
                for (k, v) in entries {
                    let key = crate::AST::CtKey::from_value(self.eval_expr(k, scope)?)
                        .ok_or_else(|| unsupported("map key", self.span()))?;
                    m.insert(key, self.eval_expr(v, scope)?);
                }
                Ok(CtValue::Map(m))
            }
            TExprKind::Index {
                base,
                index,
                is_map,
                ..
            } => {
                let b = match self.eval_expr(base, scope)? {
                    CtValue::ResOk(inner) | CtValue::Some(inner) => *inner,
                    other => other,
                };
                let i = self.eval_expr(index, scope)?;
                if *is_map {
                    let key = crate::AST::CtKey::from_value(i)
                        .ok_or_else(|| unsupported("map index key", self.span()))?;
                    match b {
                        CtValue::Map(m) => m
                            .get(&key)
                            .cloned()
                            .ok_or_else(|| unsupported("missing map key", self.span())),
                        _ => Err(unsupported("map index recv", self.span())),
                    }
                } else {
                    let idx = as_int(&i, self.span())?;
                    match b {
                        CtValue::List(xs) => {
                            if idx < 0 || idx as usize >= xs.len() {
                                Err(unsupported("list index oob", self.span()))
                            } else {
                                Ok(xs[idx as usize].clone())
                            }
                        }
                        CtValue::Bytes(bs) => {
                            if idx < 0 || idx as usize >= bs.len() {
                                Err(unsupported("bytes index oob", self.span()))
                            } else {
                                Ok(CtValue::Int(bs[idx as usize] as i64))
                            }
                        }
                        CtValue::Str(s) => {
                            let ch = s
                                .chars()
                                .nth(idx as usize)
                                .ok_or_else(|| unsupported("string index oob", self.span()))?;
                            Ok(CtValue::Char(ch))
                        }
                        other => {
                            if let Some(r) =
                                crate::Comptime::MathLayout::lane_at(&other, idx, self.span())
                            {
                                r
                            } else {
                                Err(unsupported("index recv", self.span()))
                            }
                        }
                    }
                }
            }
            TExprKind::Slice {
                base, start, end, ..
            } => {
                let b = self.eval_expr(base, scope)?;
                let a = as_int(&self.eval_expr(start, scope)?, self.span())?;
                let z = as_int(&self.eval_expr(end, scope)?, self.span())?;
                match b {
                    CtValue::List(xs) => {
                        if a < 0 || z < a || z as usize >= xs.len() {
                            Err(unsupported("slice bounds", self.span()))
                        } else {
                            Ok(CtValue::List(xs[a as usize..=z as usize].to_vec()))
                        }
                    }
                    CtValue::Str(s) => {
                        let chars: Vec<char> = s.chars().collect();
                        if a < 0 || z < a || z as usize >= chars.len() {
                            Err(unsupported("slice bounds", self.span()))
                        } else {
                            Ok(CtValue::Str(
                                chars[a as usize..=z as usize].iter().collect(),
                            ))
                        }
                    }
                    _ => Err(unsupported("slice recv", self.span())),
                }
            }
            TExprKind::Borrow { place, .. } => self.eval_expr(place, scope),
            TExprKind::MaterializeView(inner) => self.eval_expr(inner, scope),
            TExprKind::MethodCall {
                recv,
                method,
                args,
                source_first_string_literal,
                ..
            } => {
                let mut r = self.eval_expr(recv, scope)?;
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(&a.value, scope)?);
                }
                if method.name == "clone" {
                    return Ok(r);
                }
                if method.name == "apply" {
                    if let (
                        CtValue::Struct {
                            type_name,
                            fields,
                        },
                        Some(CtValue::Struct {
                            type_name: patch_name,
                            fields: patch_fields,
                        }),
                    ) = (&r, argv.first())
                    {
                        if patch_name == &format!("{type_name}.Patch") {
                            let fields = fields
                                .iter()
                                .map(|(name, old)| {
                                    let value = patch_fields
                                        .iter()
                                        .find_map(|(patch_name, value)| {
                                            (patch_name == name).then_some(value)
                                        })
                                        .and_then(|value| match value {
                                            CtValue::Some(value) => Some((**value).clone()),
                                            _ => None,
                                        })
                                        .unwrap_or_else(|| old.clone());
                                    (name.clone(), value)
                                })
                                .collect();
                            return Ok(CtValue::Struct {
                                type_name: type_name.clone(),
                                fields,
                            });
                        }
                    }
                }
                if method.name == "merge" {
                    if let (
                        CtValue::Struct {
                            type_name,
                            fields,
                        },
                        Some(CtValue::Struct {
                            type_name: other_name,
                            fields: other_fields,
                        }),
                    ) = (&r, argv.first())
                    {
                        if type_name.ends_with(".Patch") && type_name == other_name {
                            let fields = fields
                                .iter()
                                .map(|(name, current)| {
                                    let incoming = other_fields
                                        .iter()
                                        .find_map(|(other_name, value)| {
                                            (other_name == name).then_some(value)
                                        })
                                        .filter(|value| matches!(value, CtValue::Some(_)))
                                        .cloned()
                                        .unwrap_or_else(|| current.clone());
                                    (name.clone(), incoming)
                                })
                                .collect();
                            return Ok(CtValue::Struct {
                                type_name: type_name.clone(),
                                fields,
                            });
                        }
                    }
                }
                let span = self.span();
                let base_dir = self.base_dir.clone();
                if let Some(result) =
                    crate::Comptime::Build::eval_program_build_input_method(
                        &r,
                        &method.name,
                        &argv,
                        source_first_string_literal.as_deref(),
                        &base_dir,
                        self.embed_inputs.as_deref_mut(),
                        span,
                    )
                {
                    return result;
                }
                if let Some(result) = crate::Comptime::Build::eval_program_build_method(
                    &r,
                    &method.name,
                    argv.clone(),
                    self.span(),
                    self.impure_depth > 0,
                ) {
                    return result;
                }
                const MUTATING: &[&str] = &[
                    "push", "pop", "add", "add_new", "insert", "remove", "clear", "reverse",
                    "sort", "tick", "advance", "wait", "int", "float", "float_range", "bool",
                    "normal", "exponential", "bytes", "split", "pick", "weighted_pick", "sample",
                    "shuffle", "require",
                ];
                let try_mutating = MUTATING.contains(&method.name.as_str())
                    || matches!(
                        &r,
                        CtValue::Struct { type_name, .. }
                            if type_name == crate::Syntax::CLOCK_TYPE
                                || type_name == crate::Syntax::RNG_TYPE
                                || type_name == crate::Syntax::SOLVER_TYPE
                                || (type_name == crate::Syntax::MEM_POOL
                                    && matches!(method.name.as_str(), "add" | "remove"))
                    );
                // Mutating dispatch first — `apply_method` for Pool.add returns the
                // Id but drops the updated arena (write-back lives in apply_mutating).
                if try_mutating {
                    if method.name == "shuffle" {
                        if let CtValue::Struct { type_name, fields } = &r {
                            if type_name == crate::Syntax::RNG_TYPE {
                                let mut state = fields
                                    .iter()
                                    .find_map(|(name, value)| match (name.as_str(), value) {
                                        ("state", CtValue::Int(state)) => Some(*state as u64),
                                        _ => None,
                                    })
                                    .unwrap_or(0);
                                let ret = crate::Comptime::apply_seeded_rng_method(
                                    &mut state,
                                    "shuffle",
                                    &mut argv,
                                    self.span(),
                                )?;
                                r = CtValue::Struct {
                                    type_name: crate::Syntax::RNG_TYPE.to_string(),
                                    fields: vec![("state".to_string(), CtValue::Int(state as i64))],
                                };
                                self.write_back_place(recv, r, scope)?;
                                for (a, v) in args.iter().zip(argv.into_iter()) {
                                    let place_like = matches!(
                                        &a.value.kind,
                                        TExprKind::Borrow { .. }
                                            | TExprKind::Local(_)
                                            | TExprKind::Field { .. }
                                    );
                                    if place_like {
                                        self.write_back_place(&a.value, v, scope)?;
                                    }
                                }
                                return Ok(ret);
                            }
                        }
                    }
                    if let Ok(ret) = crate::Comptime::Builtins::apply_mutating(
                        &mut r,
                        &method.name,
                        argv.clone(),
                        self.span(),
                    ) {
                        self.write_back_place(recv, r, scope)?;
                        return Ok(ret);
                    }
                }
                if method.name == "compare" {
                    if let (CtValue::Int(lhs), [CtValue::Int(rhs)]) = (&r, argv.as_slice()) {
                        let variant = match lhs.cmp(rhs) {
                            std::cmp::Ordering::Less => "Less",
                            std::cmp::Ordering::Equal => "Equal",
                            std::cmp::Ordering::Greater => "Greater",
                        };
                        return Ok(CtValue::Enum {
                            type_name: "Ordering".to_string(),
                            variant: variant.to_string(),
                            args: Vec::new(),
                        });
                    }
                }
                if let Ok(v) = crate::Comptime::Builtins::apply_method(
                    &r,
                    &method.name,
                    argv.clone(),
                    self.span(),
                ) {
                    return Ok(v);
                }
                let mut names = vec![method.name.clone()];
                if method.mangled {
                    names.push(format!("user_{}", method.name));
                }
                if let CtValue::Struct { type_name, .. } = &r {
                    names.push(format!("{type_name}::{}", method.name));
                }
                for name in names {
                    if let Some(func) = self.funcs.get(&name).copied() {
                        let mut child = HashMap::new();
                        // Instance methods lower `self` into the env, not `params`.
                        let has_receiver = matches!(
                            &func.kind,
                            crate::Codegen::TIR::TFuncKind::Method {
                                self_conv: Some(_),
                                ..
                            }
                                | crate::Codegen::TIR::TFuncKind::TraitMethod { .. }
                        );
                        let argv_for_params = if has_receiver {
                            child.insert("self".to_string(), r.clone());
                            argv
                        } else {
                            let mut full = vec![r.clone()];
                            full.extend(argv);
                            full
                        };
                        let result = self.run_func(func, argv_for_params, &mut child)?;
                        if matches!(
                            &func.kind,
                            crate::Codegen::TIR::TFuncKind::Method {
                                self_conv: Some(crate::AST::AccessConvention::Write),
                                ..
                            }
                        ) {
                            if let Some(updated) = child.get("self") {
                                self.write_back_place(recv, updated.clone(), scope)?;
                            }
                        }
                        return Ok(result);
                    }
                }
                Err(unsupported(
                    &format!("method `{}`", method.name),
                    self.span(),
                ))
            }
            TExprKind::Try {
                inner,
                convert: _,
                file,
                line,
                fn_name,
            } => {
                let v = self.eval_expr(inner, scope)?;
                match v {
                    CtValue::ResOk(inner) | CtValue::Some(inner) => Ok(*inner),
                    CtValue::ResErr(e) => {
                        // D-ERRCTX1: match AOT `jet_trace_err` / JIT host (dev builds).
                        let file = file.trim_matches('"');
                        let fn_name = fn_name.trim_matches('"');
                        let frame = format!(
                            "error propagated from: {fn_name} ({file}:{line}) via ?\n"
                        );
                        if let Some(sink) = self.sink.as_mut() {
                            let skip = sink
                                .stderr
                                .ends_with(&frame);
                            if !skip {
                                sink.stderr.push_str(&frame);
                            }
                        }
                        // Propagate as a function return of the error value.
                        self.pending_return = Some(CtValue::ResErr(e));
                        Ok(CtValue::Unit)
                    }
                    CtValue::None(_) => {
                        self.pending_return = Some(CtValue::None(crate::AST::Type::Int));
                        Ok(CtValue::Unit)
                    }
                    other => Ok(other),
                }
            }
            TExprKind::OrFallback {
                value,
                fallback,
                is_option,
            } => {
                let v = self.eval_expr(value, scope)?;
                // Always treat `None` as a miss: fragment lowering can leave
                // `is_option=false` when the Option return type is unknown, and
                // `??` must still unwrap (SortedSet.first() ?? -1, etc.).
                let miss = matches!(v, CtValue::None(_))
                    || (!*is_option && matches!(v, CtValue::ResErr(_)));
                if !miss {
                    return match v {
                        CtValue::Some(inner) | CtValue::ResOk(inner) => Ok(*inner),
                        other => Ok(other),
                    };
                }
                match fallback {
                    crate::Codegen::TIR::TOrFallback::Value(fb) => self.eval_expr(fb, scope),
                    crate::Codegen::TIR::TOrFallback::Return(Some(fb)) => {
                        let ret = self.eval_expr(fb, scope)?;
                        self.pending_return = Some(ret);
                        Ok(CtValue::Unit)
                    }
                    crate::Codegen::TIR::TOrFallback::Return(None) => {
                        self.pending_return = Some(CtValue::Unit);
                        Ok(CtValue::Unit)
                    }
                    crate::Codegen::TIR::TOrFallback::Panic { .. } => {
                        Err(unsupported("or-fallback panic", self.span()))
                    }
                    _ => Err(unsupported("or-fallback form", self.span())),
                }
            }
            TExprKind::EnumLit {
                enum_type,
                variant,
                payload,
            } => {
                // Positional payloads keep `label: None` so `jet_show` matches
                // AOT `user_Wrap(user_Num(1))` Debug shape (I2 / #777).
                let args = match payload {
                    crate::Codegen::TIR::TEnumPayload::Unit => Vec::new(),
                    crate::Codegen::TIR::TEnumPayload::Positional(pos) => {
                        let mut out = Vec::with_capacity(pos.len());
                        for a in pos {
                            out.push((None, self.eval_expr(&a.value, scope)?));
                        }
                        out
                    }
                    crate::Codegen::TIR::TEnumPayload::Named(named) => {
                        let mut out = Vec::with_capacity(named.len());
                        for (name, a) in named {
                            out.push((Some(name.clone()), self.eval_expr(&a.value, scope)?));
                        }
                        out
                    }
                };
                Ok(CtValue::Enum {
                    type_name: enum_type.clone(),
                    variant: variant.clone(),
                    args,
                })
            }
            TExprKind::HostCall(host) => match host.as_ref() {
                crate::Codegen::TIR::THostCall::ExpectSnapshot { value, .. } => {
                    // Comptime/transcript: evaluate the wrapped value; snapshot I/O
                    // is an AOT harness concern.
                    let _ = self.eval_expr(value, scope)?;
                    Ok(CtValue::Unit)
                }
                crate::Codegen::TIR::THostCall::NumericBounds { ty, member } => {
                    use crate::AST::Type;
                    match (ty, member.as_str()) {
                        (Type::Float32, "MAX") => {
                            Ok(CtValue::Float(CtFloat::literal(f32::MAX as f64, true)))
                        }
                        (Type::Float32, "MIN") => {
                            Ok(CtValue::Float(CtFloat::literal(f32::MIN as f64, true)))
                        }
                        (Type::Float32, "NAN") => {
                            Ok(CtValue::Float(CtFloat::literal(f32::NAN as f64, true)))
                        }
                        (Type::Float32, "INFINITY") => {
                            Ok(CtValue::Float(CtFloat::literal(f32::INFINITY as f64, true)))
                        }
                        (Type::Float32, "NEG_INFINITY") => Ok(CtValue::Float(CtFloat::literal(
                            f32::NEG_INFINITY as f64,
                            true,
                        ))),
                        (Type::Float32, "EPSILON") => {
                            Ok(CtValue::Float(CtFloat::literal(f32::EPSILON as f64, true)))
                        }
                        (Type::Float, "MAX") => {
                            Ok(CtValue::Float(CtFloat::literal(f64::MAX, false)))
                        }
                        (Type::Float, "MIN") => {
                            Ok(CtValue::Float(CtFloat::literal(f64::MIN, false)))
                        }
                        (Type::Float, "NAN") => {
                            Ok(CtValue::Float(CtFloat::literal(f64::NAN, false)))
                        }
                        (Type::Float, "INFINITY") => {
                            Ok(CtValue::Float(CtFloat::literal(f64::INFINITY, false)))
                        }
                        (Type::Float, "NEG_INFINITY") => {
                            Ok(CtValue::Float(CtFloat::literal(f64::NEG_INFINITY, false)))
                        }
                        (Type::Float, "EPSILON") => {
                            Ok(CtValue::Float(CtFloat::literal(f64::EPSILON, false)))
                        }
                        (Type::Int, "MAX") => Ok(CtValue::Int(i64::MAX)),
                        (Type::Int, "MIN") => Ok(CtValue::Int(i64::MIN)),
                        (Type::IntN { signed: false, bits: 8 }, "MAX") => {
                            Ok(CtValue::Int(u8::MAX as i64))
                        }
                        (Type::IntN { signed: false, bits: 8 }, "MIN") => Ok(CtValue::Int(0)),
                        (Type::IntN { signed: true, bits: 8 }, "MAX") => {
                            Ok(CtValue::Int(i8::MAX as i64))
                        }
                        (Type::IntN { signed: true, bits: 8 }, "MIN") => {
                            Ok(CtValue::Int(i8::MIN as i64))
                        }
                        (Type::IntN { signed: false, bits: 16 }, "MAX") => {
                            Ok(CtValue::Int(u16::MAX as i64))
                        }
                        (Type::IntN { signed: false, bits: 16 }, "MIN") => Ok(CtValue::Int(0)),
                        (Type::IntN { signed: true, bits: 16 }, "MAX") => {
                            Ok(CtValue::Int(i16::MAX as i64))
                        }
                        (Type::IntN { signed: true, bits: 16 }, "MIN") => {
                            Ok(CtValue::Int(i16::MIN as i64))
                        }
                        (Type::IntN { signed: false, bits: 32 }, "MAX") => {
                            Ok(CtValue::Int(u32::MAX as i64))
                        }
                        (Type::IntN { signed: false, bits: 32 }, "MIN") => Ok(CtValue::Int(0)),
                        (Type::IntN { signed: true, bits: 32 }, "MAX") => {
                            Ok(CtValue::Int(i32::MAX as i64))
                        }
                        (Type::IntN { signed: true, bits: 32 }, "MIN") => {
                            Ok(CtValue::Int(i32::MIN as i64))
                        }
                        (Type::IntN { signed, bits }, "MAX") => Ok(CtValue::Int(
                            crate::Comptime::MathLayout::integer_bound(*signed, *bits, true),
                        )),
                        (Type::IntN { signed, bits }, "MIN") => Ok(CtValue::Int(
                            crate::Comptime::MathLayout::integer_bound(*signed, *bits, false),
                        )),
                        _ => Err(unsupported(
                            &format!("numeric bounds `{member}`"),
                            self.span(),
                        )),
                    }
                }
                crate::Codegen::TIR::THostCall::FixedListIndex { base, index } => {
                    let b = self.eval_expr(base, scope)?;
                    let idx = as_int(&self.eval_expr(index, scope)?, self.span())?;
                    match b {
                        CtValue::List(xs) => {
                            if idx < 0 || idx as usize >= xs.len() {
                                Err(unsupported("fixed-list index oob", self.span()))
                            } else {
                                Ok(xs[idx as usize].clone())
                            }
                        }
                        other => {
                            if let Some(r) =
                                crate::Comptime::MathLayout::lane_at(&other, idx, self.span())
                            {
                                r
                            } else {
                                Err(unsupported("fixed-list index recv", self.span()))
                            }
                        }
                    }
                }
                crate::Codegen::TIR::THostCall::SwitchSubjectField { field } => {
                    let CtValue::Struct { fields, .. } = self
                        .switch_subject
                        .as_ref()
                        .ok_or_else(|| unsupported("switch subject field outside switch", self.span()))?
                    else {
                        return Err(unsupported("switch subject is not a struct", self.span()));
                    };
                    fields
                        .iter()
                        .find_map(|(name, value)| (name == field).then(|| value.clone()))
                        .ok_or_else(|| unsupported(&format!("switch subject field `{field}`"), self.span()))
                }
                crate::Codegen::TIR::THostCall::Method { recv, method, args } => {
                    let mut r = self.eval_expr(recv, scope)?;
                    if matches!(&r, CtValue::Struct { type_name, .. } if type_name == "__JetTirExpiring")
                        && method == "with"
                    {
                        let clock_index = struct_int(&r, "clock")
                            .ok_or_else(|| unsupported("expiring secret clock", self.span()))?
                            as usize;
                        let deadline = struct_int(&r, "deadline")
                            .ok_or_else(|| unsupported("expiring secret deadline", self.span()))?;
                        let valid = self
                            .clocks
                            .get(clock_index)
                            .is_some_and(|now| *now <= deadline);
                        if !valid {
                            return Ok(CtValue::ResErr(Box::new(CtValue::Str(
                                "expired".to_string(),
                            ))));
                        }
                        let CtValue::Struct { fields, .. } = &r else {
                            unreachable!();
                        };
                        let value = fields
                            .iter()
                            .find_map(|(name, value)| (name == "value").then(|| value.clone()))
                            .unwrap_or(CtValue::Unit);
                        let Some(TExpr {
                            kind: TExprKind::Lambda(lambda),
                            ..
                        }) = args.first()
                        else {
                            return Err(unsupported("expiring secret lambda", self.span()));
                        };
                        let result = self.eval_tlambda(lambda, vec![value], scope)?;
                        return Ok(CtValue::ResOk(Box::new(result)));
                    }
                    if matches!(&r, CtValue::Struct { type_name, .. } if type_name == "__JetTirShared") {
                        let CtValue::Struct { fields, .. } = &r else {
                            unreachable!();
                        };
                        let index = fields
                            .iter()
                            .find_map(|(name, value)| match (name.as_str(), value) {
                                ("index", CtValue::Int(index)) => Some(*index as usize),
                                _ => None,
                            })
                            .ok_or_else(|| unsupported("shared handle", self.span()))?;
                        let transactional = method == "edit_txn";
                        let current = if transactional {
                            self.shared_transactions
                                .last()
                                .and_then(|transaction| transaction.get(&index))
                                .cloned()
                                .or_else(|| self.shared_values.get(index).cloned())
                        } else {
                            self.shared_values.get(index).cloned()
                        }
                        .ok_or_else(|| unsupported("shared value", self.span()))?;
                        let Some(TExpr {
                            kind: TExprKind::Lambda(lambda),
                            ..
                        }) = args.first()
                        else {
                            return Err(unsupported("shared method lambda", self.span()));
                        };
                        let (result, updated) =
                            self.eval_tlambda_mut_arg(lambda, current, scope)?;
                        if transactional {
                            let Some(transaction) = self.shared_transactions.last_mut() else {
                                return Err(unsupported(
                                    "Shared.edit_txn outside #Transact",
                                    self.span(),
                                ));
                            };
                            transaction.insert(index, updated);
                        } else if method == "edit" {
                            self.shared_values[index] = updated;
                        }
                        return Ok(result);
                    }
                    let mut argv = Vec::with_capacity(args.len());
                    for a in args {
                        argv.push(self.eval_expr(a, scope)?);
                    }
                    let result = match crate::Comptime::Builtins::apply_mutating(
                        &mut r,
                        method,
                        argv.clone(),
                        self.span(),
                    ) {
                        Ok(v) => v,
                        Err(_) => crate::Comptime::Builtins::apply_method(
                            &r,
                            method,
                            argv,
                            self.span(),
                        )?,
                    };
                    self.write_back_place(recv, r, scope)?;
                    Ok(result)
                }
                crate::Codegen::TIR::THostCall::YieldSend { value } => {
                    let yielded = self.eval_expr(value, scope)?;
                    if let Some(items) = self.collecting_items.last_mut() {
                        items.push(yielded);
                        return Ok(CtValue::Unit);
                    }
                    let consumer = self
                        .yield_consumer
                        .clone()
                        .ok_or_else(|| unsupported("yield outside a stream consumer", self.span()))?;
                    let mut consumer_scope = self
                        .yield_scope
                        .take()
                        .ok_or_else(|| unsupported("stream consumer scope", self.span()))?;
                    consumer_scope.insert(consumer.var, yielded);
                    let result = self.exec_stmts(consumer.body, &mut consumer_scope);
                    self.yield_scope = Some(consumer_scope);
                    match result? {
                        Flow::Normal | Flow::Continue => Ok(CtValue::Unit),
                        Flow::Break => {
                            self.pending_return = Some(CtValue::Unit);
                            Ok(CtValue::Unit)
                        }
                        other => Err(unsupported(
                            &format!("stream consumer control flow {other:?}"),
                            self.span(),
                        )),
                    }
                }
                crate::Codegen::TIR::THostCall::Helper { helper, args } => {
                    let leaf = helper
                        .rsplit("::")
                        .next()
                        .unwrap_or(helper.as_str());
                    let mut argv = Vec::with_capacity(args.len());
                    for a in args {
                        match a {
                            crate::Codegen::TIR::THostArg::Expr(e)
                            | crate::Codegen::TIR::THostArg::Borrow(e) => {
                                argv.push(self.eval_expr(e, scope)?);
                            }
                            crate::Codegen::TIR::THostArg::Lambda(_) => {
                                return Err(unsupported(
                                    "expr `HostCall` helper lambda",
                                    self.span(),
                                ));
                            }
                        }
                    }
                    if leaf == "jet_std_clock_new" || leaf.ends_with("jet_std_clock_new") {
                        let seed = match argv.first() {
                            Some(CtValue::Int(n)) => *n,
                            _ => {
                                return Err(unsupported(
                                    "Clock.new expects an Int seed",
                                    self.span(),
                                ));
                            }
                        };
                        let index = self.clocks.len();
                        self.clocks.push(seed);
                        return Ok(CtValue::Struct {
                            type_name: "__JetTirClock".to_string(),
                            fields: vec![("index".to_string(), CtValue::Int(index as i64))],
                        });
                    }
                    if leaf == "jet_std_clock_system" || leaf.ends_with("jet_std_clock_system") {
                        return Ok(CtValue::Struct {
                            type_name: crate::Syntax::CLOCK_TYPE.to_string(),
                            fields: vec![("now".to_string(), CtValue::Int(0))],
                        });
                    }
                    // D-ERRCTX1: `.context(msg)` — prepend message on Err only.
                    if leaf == "jet_context" || leaf.ends_with("jet_context") {
                        let msg = match argv.get(1) {
                            Some(CtValue::Str(s)) => s.clone(),
                            Some(other) => other.jet_show(),
                            None => String::new(),
                        };
                        return Ok(match argv.first() {
                            Some(CtValue::ResOk(v)) => CtValue::ResOk(v.clone()),
                            Some(CtValue::ResErr(err)) => {
                                CtValue::ResErr(Box::new(CtValue::Str(format!(
                                    "{}: {}",
                                    msg,
                                    err.jet_show()
                                ))))
                            }
                            Some(other) => other.clone(),
                            None => CtValue::Unit,
                        });
                    }
                    Err(unsupported(
                        &format!("expr `HostCall` helper `{leaf}`"),
                        self.span(),
                    ))
                }
                crate::Codegen::TIR::THostCall::ExpiringValueNew {
                    value,
                    duration,
                    clock,
                }
                | crate::Codegen::TIR::THostCall::ExpiringSecretNew {
                    value,
                    duration,
                    clock,
                    ..
                } => {
                    let value = self.eval_expr(value, scope)?;
                    let duration = self.eval_expr(duration, scope)?;
                    let duration = struct_int(&duration, "ms")
                        .ok_or_else(|| unsupported("expiring duration", self.span()))?;
                    let clock = self.eval_expr(clock, scope)?;
                    let clock_index = handle_index(&clock, "__JetTirClock")
                        .ok_or_else(|| unsupported("expiring clock", self.span()))?;
                    let now = *self
                        .clocks
                        .get(clock_index)
                        .ok_or_else(|| unsupported("expiring clock handle", self.span()))?;
                    Ok(CtValue::Struct {
                        type_name: "__JetTirExpiring".to_string(),
                        fields: vec![
                            ("value".to_string(), value),
                            (
                                "deadline".to_string(),
                                CtValue::Int(now.saturating_add(duration)),
                            ),
                            ("clock".to_string(), CtValue::Int(clock_index as i64)),
                        ],
                    })
                }
                other => {
                    let tag = match other {
                        crate::Codegen::TIR::THostCall::Helper { .. } => "Helper",
                        crate::Codegen::TIR::THostCall::Method { .. } => "Method",
                        crate::Codegen::TIR::THostCall::FixedListIndex { .. } => "FixedListIndex",
                        crate::Codegen::TIR::THostCall::TypedText { .. } => "TypedText",
                        crate::Codegen::TIR::THostCall::FnName(_) => "FnName",
                        crate::Codegen::TIR::THostCall::GcEdit { .. } => "GcEdit",
                        crate::Codegen::TIR::THostCall::GcRead { .. } => "GcRead",
                        crate::Codegen::TIR::THostCall::OptionProbe { .. } => "OptionProbe",
                        crate::Codegen::TIR::THostCall::StrMatchScan { .. } => "StrMatchScan",
                        crate::Codegen::TIR::THostCall::BinMatchScan { .. } => "BinMatchScan",
                        crate::Codegen::TIR::THostCall::TupleIndex { .. } => "TupleIndex",
                        crate::Codegen::TIR::THostCall::SwitchSubjectField { .. } => {
                            "SwitchSubjectField"
                        }
                        crate::Codegen::TIR::THostCall::YieldSend { .. } => unreachable!(),
                        crate::Codegen::TIR::THostCall::TypedTextInterp { .. } => "TypedTextInterp",
                        crate::Codegen::TIR::THostCall::ExpectSnapshot { .. } => "ExpectSnapshot",
                        crate::Codegen::TIR::THostCall::EnvSet { .. } => "EnvSet",
                        _ => "Other",
                    };
                    Err(unsupported(
                        &format!("expr `HostCall` {tag}"),
                        self.span(),
                    ))
                }
            },
            TExprKind::DataEntriesToMap(local) => scope
                .get(&local.name)
                .cloned()
                .or_else(|| self.globals.get(&local.name).cloned())
                .ok_or_else(|| unsupported(&format!("unbound `{}`", local.name), self.span())),
            TExprKind::DistinctCtor { name: _, arg, base: _ } => {
                // Distinct is a zero-cost nominal wrapper over its base scalar.
                self.eval_expr(arg, scope)
            }
            TExprKind::RangeCheckedCtor { name, arg } => {
                let v = self.eval_expr(arg, scope)?;
                Ok(CtValue::ResOk(Box::new(v)))
                // Range bounds are enforced by sema for literals; dynamic checks
                // reuse the same ok-wrapping Result shape as AOT try_new.
                .map(|ok| {
                    let _ = name;
                    ok
                })
            }
            TExprKind::DistinctConvert {
                name: _,
                arg,
                op,
                range,
                fallible,
            } => {
                let v = self.eval_expr(arg, scope)?;
                let converted = self.eval_numeric_op(&v, op, &arg.ty, &expr.ty)?;
                let inner = match converted {
                    CtValue::ResOk(v) => *v,
                    CtValue::ResErr(e) if *fallible => return Ok(CtValue::ResErr(e)),
                    other if !*fallible => other,
                    other => other,
                };
                if let Some((lo, hi)) = range {
                    let CtValue::Int(n) = &inner else {
                        return Err(unsupported("distinct range check on non-Int", self.span()));
                    };
                    if *n < *lo || *n > *hi {
                        let err = CtValue::Str(format!("value doesn't fit in range {lo}..{hi}"));
                        return Ok(if *fallible {
                            CtValue::ResErr(Box::new(err))
                        } else {
                            return Err(unsupported("distinct out of range", self.span()));
                        });
                    }
                }
                Ok(if *fallible {
                    CtValue::ResOk(Box::new(inner))
                } else {
                    inner
                })
            }
            TExprKind::UnitConvert { .. } => Err(unsupported("expr `UnitConvert`", self.span())),
            TExprKind::MathBuiltin {
                type_name,
                func,
                args,
            } => {
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(a, scope)?);
                }
                if let Some(res) = crate::Comptime::Builtins::apply_static_type_method(
                    type_name,
                    func,
                    argv.clone(),
                    self.span(),
                ) {
                    return res;
                }
                // Instance-style math ops arrive as MathBuiltin with the receiver
                // already folded into `args` for free-function emit; try method form.
                if let Some((recv, rest)) = argv.split_first() {
                    match crate::Comptime::Builtins::apply_method(
                        recv,
                        func,
                        rest.to_vec(),
                        self.span(),
                    ) {
                        Ok(v) => return Ok(v),
                        Err(_) => {}
                    }
                }
                Err(unsupported(
                    &format!("`{type_name}.{func}`"),
                    self.span(),
                ))
            }
            TExprKind::PreciseBuiltin {
                type_name,
                func,
                args,
            } => {
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(a, scope)?);
                }
                eval_precise_builtin(type_name, func, argv, self.span())
            }
            TExprKind::ResourceNew(inner) => self.eval_expr(inner, scope),
            TExprKind::ResourceTake(place) => scope
                .get(place)
                .cloned()
                .or_else(|| place.strip_prefix("user_").and_then(|name| scope.get(name).cloned()))
                .or_else(|| self.globals.get(place).cloned())
                .ok_or_else(|| unsupported(&format!("resource `{place}`"), self.span())),
            TExprKind::AmbientInput { .. } => Err(unsupported("expr `AmbientInput`", self.span())),
            TExprKind::RequireStop {
                kind,
                loc,
                always_stops,
            } => {
                let failed = if *always_stops {
                    true
                } else {
                    match kind {
                        crate::Codegen::TIR::TRequireKind::Require { cond, .. } => {
                            !as_bool(&self.eval_expr(cond, scope)?, self.span())?
                        }
                        crate::Codegen::TIR::TRequireKind::RequireEq { left, right, .. } => {
                            self.eval_expr(left, scope)? != self.eval_expr(right, scope)?
                        }
                        crate::Codegen::TIR::TRequireKind::Panic { .. } => true,
                    }
                };
                if !failed {
                    return Ok(CtValue::Unit);
                }
                let msg = match kind {
                    crate::Codegen::TIR::TRequireKind::Require { msg: Some(msg), .. }
                    | crate::Codegen::TIR::TRequireKind::Panic { msg } => {
                        self.eval_expr(msg, scope)?.jet_show()
                    }
                    crate::Codegen::TIR::TRequireKind::Require { msg: None, .. } => {
                        "requirement failed".to_string()
                    }
                    crate::Codegen::TIR::TRequireKind::RequireEq { .. } => {
                        "values are not equal".to_string()
                    }
                };
                let file = loc.file.trim_matches('"');
                let fn_name = loc.fn_name.trim_matches('"');
                let src_line = loc.src_line.trim_matches('"');
                let line_s = loc.line.to_string();
                let margin = line_s.len();
                let pad = " ".repeat(margin);
                let col_offset = loc.col.saturating_sub(1) as usize;
                let caret = "^".repeat(loc.caret.max(1) as usize);
                let rendered = format!(
                    "panic: {msg}\n  --> {file}:{} in {fn_name}\n   {pad}|\n{line_s} | {src_line}\n   {pad}| {}{caret}\n",
                    loc.line,
                    " ".repeat(col_offset)
                );
                if let Some(sink) = self.sink.as_mut() {
                    sink.stderr.push_str(&rendered);
                    sink.exit_code = Some(70);
                    return Err(Diagnostic::error(
                        "SOFT_EXIT",
                        "70".to_string(),
                        "require/panic stop".to_string(),
                        String::new(),
                        Some(self.span()),
                    ));
                }
                Err(unsupported("require/panic stop", self.span()))
            }
            TExprKind::LayoutCompare { .. } => Err(unsupported("expr `LayoutCompare`", self.span())),
            TExprKind::LayoutLit { .. } => Err(unsupported("expr `LayoutLit`", self.span())),
            TExprKind::IncDec {
                op,
                place,
                postfix,
                ..
            } => {
                let TPlace::Local(local) = place else {
                    return Err(unsupported("inc/dec place", self.span()));
                };
                let key = local.name.clone();
                let cur = scope
                    .get(&key)
                    .cloned()
                    .or_else(|| self.globals.get(&key).cloned())
                    .unwrap_or(CtValue::Unit);
                let n = as_int(&cur, self.span())?;
                let next = match op {
                    crate::AST::IncDecOp::Inc => n.wrapping_add(1),
                    crate::AST::IncDecOp::Dec => n.wrapping_sub(1),
                };
                scope.insert(key, CtValue::Int(next));
                Ok(if *postfix {
                    CtValue::Int(n)
                } else {
                    CtValue::Int(next)
                })
            }
            TExprKind::PtrFromAddr { .. } => Err(unsupported("expr `PtrFromAddr`", self.span())),
            TExprKind::Deref(inner) => {
                let pointer = self.eval_expr(inner, scope)?;
                let CtValue::Struct { type_name, fields } = pointer else {
                    return Err(unsupported("raw pointer carrier", self.span()));
                };
                if type_name != "__JetRawLocal" {
                    return Err(unsupported("raw pointer target", self.span()));
                }
                if let Some(value) = fields.iter().find_map(|(field, value)| {
                    (field == "value").then(|| value.clone())
                }) {
                    return Ok(value);
                }
                let name = fields.iter().find_map(|(field, value)| match (field.as_str(), value) {
                    ("name", CtValue::Str(name)) => Some(name.as_str()),
                    _ => None,
                });
                name.and_then(|name| scope.get(name).cloned())
                    .ok_or_else(|| unsupported("raw pointer local", self.span()))
            }
            TExprKind::RawOf(inner) => {
                if matches!(
                    &inner.ty,
                    Type::Apply { name, args }
                        if name == crate::Syntax::TYPE_PTR && args.len() == 1
                ) {
                    return self.eval_expr(inner, scope);
                }
                let local = super::raw_place_local(inner);
                let fields = if let Some(local) = local {
                    vec![("name".to_string(), CtValue::Str(local.name.clone()))]
                } else {
                    vec![("value".to_string(), self.eval_expr(inner, scope)?)]
                };
                Ok(CtValue::Struct {
                    type_name: "__JetRawLocal".to_string(),
                    fields,
                })
            }
            TExprKind::AllocNew { ctor } => Ok(CtValue::Struct {
                type_name: "__JetTirAllocator".to_string(),
                fields: vec![("ctor".to_string(), CtValue::Str(ctor.clone()))],
            }),
            TExprKind::JsonLit { variant, arg } => {
                let payload = match arg {
                    Some(inner) => Some(self.eval_expr(&inner.0, scope)?),
                    None => None,
                };
                Ok(CtValue::Enum {
                    type_name: "Json".to_string(),
                    variant: variant.clone(),
                    args: match payload {
                        Some(v) => vec![(None, v)],
                        None => Vec::new(),
                    },
                })
            }
            TExprKind::DbValueLit { .. } => Err(unsupported("expr `DbValueLit`", self.span())),
            TExprKind::ListSpread { parts } => {
                let mut values = Vec::new();
                for part in parts {
                    match part {
                        ListSpreadPart::Elem(expr) => {
                            values.push(self.eval_expr(expr, scope)?);
                        }
                        ListSpreadPart::Spread(expr) => {
                            let CtValue::List(items) = self.eval_expr(expr, scope)? else {
                                return Err(unsupported("list spread operand", self.span()));
                            };
                            values.extend(items);
                        }
                    }
                }
                Ok(CtValue::List(values))
            }
            TExprKind::ColumnarListLit { .. } => {
                Err(unsupported("expr `ColumnarListLit`", self.span()))
            }
            TExprKind::ColumnarGather { .. } => {
                Err(unsupported("expr `ColumnarGather`", self.span()))
            }
            TExprKind::ColumnarColumnRead { .. } => {
                Err(unsupported("expr `ColumnarColumnRead`", self.span()))
            }
            TExprKind::PoolSlot {
                pool,
                id,
                field,
                ..
            } => {
                let pool_value = self.eval_expr(pool, scope)?;
                let id_value = self.eval_expr(id, scope)?;
                let Some((index, generation)) = pool_id_parts(&id_value) else {
                    return Err(pool_stale_diagnostic());
                };
                let CtValue::Struct { fields, .. } = pool_value else {
                    return Err(pool_stale_diagnostic());
                };
                let slots = fields.iter().find_map(|(name, value)| match value {
                    CtValue::List(slots) if name == "slots" => Some(slots),
                    _ => None,
                });
                let Some(CtValue::Enum {
                    variant,
                    args,
                    ..
                }) = slots.and_then(|slots| slots.get(index))
                else {
                    return Err(pool_stale_diagnostic());
                };
                if variant != "Occupied"
                    || !matches!(args.first(), Some((_, CtValue::Int(found))) if *found == generation)
                {
                    return Err(pool_stale_diagnostic());
                }
                let Some((_, mut value)) = args.get(1).cloned() else {
                    return Err(pool_stale_diagnostic());
                };
                if let Some(field) = field {
                    let CtValue::Struct { fields, .. } = value else {
                        return Err(unsupported("Pool field on a non-struct", self.span()));
                    };
                    value = fields
                        .into_iter()
                        .find_map(|(name, value)| (name == *field).then_some(value))
                        .ok_or_else(|| unsupported(&format!("Pool field `{field}`"), self.span()))?;
                }
                Ok(value)
            }
            TExprKind::IndexHook {
                type_name,
                base,
                index,
                ..
            } => {
                let recv = self.eval_expr(base, scope)?;
                let key = self.eval_expr(index, scope)?;
                let func = self
                    .funcs
                    .get(&format!("{type_name}::get"))
                    .copied()
                    .ok_or_else(|| unsupported("Index.get", self.span()))?;
                let mut child = HashMap::new();
                child.insert("self".to_string(), recv);
                match self.run_func(func, vec![key], &mut child)? {
                    CtValue::Some(value) => Ok(*value),
                    CtValue::None(_) => Err(unsupported("index miss", self.span())),
                    _ => Err(unsupported("Index.get result", self.span())),
                }
            }
            TExprKind::MathLaneIndex { base, index, .. } => {
                let b = self.eval_expr(base, scope)?;
                let i = as_int(&self.eval_expr(index, scope)?, self.span())?;
                match crate::Comptime::MathLayout::lane_at(&b, i, self.span()) {
                    Some(r) => r,
                    None => Err(unsupported("expr `MathLaneIndex`", self.span())),
                }
            }
            TExprKind::MathSwizzleRead { .. } => {
                Err(unsupported("expr `MathSwizzleRead`", self.span()))
            }
            TExprKind::FnFieldCall { recv, field, args } => {
                let value = self.eval_expr(recv, scope)?;
                let CtValue::Struct { fields, .. } = value else {
                    return Err(unsupported("function field receiver", self.span()));
                };
                let callable = fields
                    .into_iter()
                    .find_map(|(name, value)| (name == *field).then_some(value))
                    .ok_or_else(|| unsupported(&format!("function field `{field}`"), self.span()))?;
                let mut argv = Vec::with_capacity(args.len());
                for arg in args {
                    argv.push(self.eval_expr(&arg.value, scope)?);
                }
                self.call_callable(&callable, argv)
            }
            TExprKind::StaticCall {
                owner,
                method,
                args,
                ..
            } => {
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(&a.value, scope)?);
                }
                match owner {
                    crate::Codegen::TIR::TStaticOwner::User(type_name) => {
                        if method.name == "diff" && argv.len() == 2 {
                            if let (
                                CtValue::Struct {
                                    type_name: new_name,
                                    fields: new_fields,
                                },
                                CtValue::Struct {
                                    type_name: old_name,
                                    fields: old_fields,
                                },
                            ) = (&argv[0], &argv[1])
                            {
                                if new_name == type_name && old_name == type_name {
                                    let fields = new_fields
                                        .iter()
                                        .map(|(name, new_value)| {
                                            let changed = old_fields
                                                .iter()
                                                .find_map(|(old_name, old_value)| {
                                                    (old_name == name).then_some(old_value)
                                                })
                                                != Some(new_value);
                                            (
                                                name.clone(),
                                                if changed {
                                                    CtValue::Some(Box::new(new_value.clone()))
                                                } else {
                                                    CtValue::None(new_value.jet_type())
                                                },
                                            )
                                        })
                                        .collect();
                                    return Ok(CtValue::Struct {
                                        type_name: format!("{type_name}.Patch"),
                                        fields,
                                    });
                                }
                            }
                        }
                        if let Some(res) = crate::Comptime::Builtins::apply_static_type_method(
                            type_name,
                            &method.name,
                            argv.clone(),
                            self.span(),
                        ) {
                            return res;
                        }
                        // Core-import alias may still lower as StaticCall when
                        // function bodies were typed before imports propagated.
                        if let Some(module) = self.core_imports.get(type_name) {
                            return apply_core_call(
                                module,
                                &method.name,
                                argv,
                                self.span(),
                                self.repl_mode,
                            );
                        }
                        let candidates = [
                            format!("{type_name}::{}", method.name),
                            format!("{type_name}.{}", method.name),
                            method.name.clone(),
                            format!("user_{}", method.name),
                        ];
                        for name in candidates {
                            if let Some(func) = self.funcs.get(&name).copied() {
                                let mut child = HashMap::new();
                                return self.run_func(func, argv, &mut child);
                            }
                        }
                        Err(unsupported(
                            &format!("static `{type_name}.{}`", method.name),
                            self.span(),
                        ))
                    }
                    crate::Codegen::TIR::TStaticOwner::Prelude { path, .. } => {
                        if path == "jet_std::JetShared" && method.name == "new" && argv.len() == 1 {
                            let index = self.shared_values.len();
                            self.shared_values.push(argv.remove(0));
                            return Ok(CtValue::Struct {
                                type_name: "__JetTirShared".to_string(),
                                fields: vec![("index".to_string(), CtValue::Int(index as i64))],
                            });
                        }
                        if let Some(res) = crate::Comptime::Builtins::apply_static_type_method(
                            path,
                            &method.name,
                            argv,
                            self.span(),
                        ) {
                            res
                        } else {
                            Err(unsupported(
                                &format!("prelude static `{path}.{}`", method.name),
                                self.span(),
                            ))
                        }
                    }
                }
            }
            TExprKind::Todo { expected_type, .. } => Err(unsupported(&format!("expr Todo ({expected_type})"), self.span())),
            TExprKind::DistinctRaw(inner) => self.eval_expr(inner, scope),
            TExprKind::OptField {
                base,
                member,
                flatten,
            } => {
                let v = self.eval_expr(base, scope)?;
                match v {
                    CtValue::None(_) => Ok(CtValue::None(expr.ty.clone())),
                    CtValue::Some(inner) => {
                        let field = match *inner {
                            CtValue::Struct { fields, .. } => fields
                                .into_iter()
                                .find(|(n, _)| n == member)
                                .map(|(_, v)| v)
                                .ok_or_else(|| {
                                    unsupported(&format!("opt field `{member}`"), self.span())
                                })?,
                            _ => {
                                return Err(unsupported("opt-field recv", self.span()));
                            }
                        };
                        if *flatten {
                            Ok(field)
                        } else {
                            Ok(CtValue::Some(Box::new(field)))
                        }
                    }
                    CtValue::Struct { fields, .. } => fields
                        .into_iter()
                        .find(|(n, _)| n == member)
                        .map(|(_, v)| {
                            if *flatten {
                                v
                            } else {
                                CtValue::Some(Box::new(v))
                            }
                        })
                        .ok_or_else(|| unsupported(&format!("opt field `{member}`"), self.span())),
                    _ => Err(unsupported("opt-field recv", self.span())),
                }
            }
            TExprKind::Lambda(lambda) => Ok(self.store_callable(EvalCallable::Lambda {
                lambda,
                captured: scope.clone(),
            })),
            TExprKind::PatternMatches { .. } => {
                Err(unsupported("expr `PatternMatches`", self.span()))
            }
            TExprKind::OptionLift2 { .. } => Err(unsupported("expr `OptionLift2`", self.span())),
            TExprKind::ClosureMethod { recv, op, args } => {
                self.eval_closure_method(recv, op, args, scope)
            }
            TExprKind::HostBorrowCallback { .. } => {
                Err(unsupported("expr `HostBorrowCallback`", self.span()))
            }
            TExprKind::NumericMethod { recv, op } => {
                let v = self.eval_expr(recv, scope)?;
                self.eval_numeric_op(&v, op, &recv.ty, &expr.ty)
            }
            TExprKind::OverflowOpt {
                prefix,
                op,
                lhs,
                rhs,
            } => {
                let l = self.eval_expr(lhs, scope)?;
                let r = self.eval_expr(rhs, scope)?;
                let a = as_int(&l, self.span())?;
                let b = as_int(&r, self.span())?;
                let width_ty = match &expr.ty {
                    Type::Option(inner) => inner.as_ref(),
                    other => other,
                };
                let (signed, bits) = match width_ty {
                    Type::IntN { signed, bits } => (*signed, *bits),
                    Type::Int => (true, 64),
                    Type::Named(n) => match n.as_str() {
                        "U8" => (false, 8),
                        "I8" => (true, 8),
                        "U16" => (false, 16),
                        "I16" => (true, 16),
                        "U32" => (false, 32),
                        "I32" => (true, 32),
                        "U64" => (false, 64),
                        "I64" | "Int" => (true, 64),
                        _ => match &lhs.ty {
                            Type::IntN { signed, bits } => (*signed, *bits),
                            Type::Int => (true, 64),
                            _ => (true, 64),
                        },
                    },
                    _ => match &lhs.ty {
                        Type::IntN { signed, bits } => (*signed, *bits),
                        Type::Int => (true, 64),
                        _ => (true, 64),
                    },
                };
                let bin = match *op {
                    "add" => crate::AST::BinOp::Add,
                    "sub" => crate::AST::BinOp::Sub,
                    "mul" => crate::AST::BinOp::Mul,
                    "div" => crate::AST::BinOp::Div,
                    other => {
                        return Err(unsupported(
                            &format!("OverflowOpt op `{other}`"),
                            self.span(),
                        ));
                    }
                };
                crate::Comptime::MathLayout::overflow_opt(
                    prefix,
                    bin,
                    a,
                    b,
                    signed,
                    bits,
                    self.span(),
                )
            }
            TExprKind::CoreClosureCall {
                kind: TCoreClosureKind::Spawn { .. },
            } => self.eval_spawn(scope),
            TExprKind::CoreClosureCall {
                kind: TCoreClosureKind::Guard { executable, .. },
            } => {
                self.scope_guards.push(executable.as_ref());
                Ok(CtValue::Unit)
            }
            TExprKind::CoreClosureCall {
                kind: TCoreClosureKind::OnCommit { executable, .. },
            } => {
                let Some(frame) = self.txn_stack.last_mut() else {
                    return Err(unsupported("on_commit outside transaction", self.span()));
                };
                frame.on_commit.push(executable.as_ref());
                Ok(CtValue::Unit)
            }
            TExprKind::CoreClosureCall {
                kind: TCoreClosureKind::OnRollback { executable, .. },
            } => {
                let Some(frame) = self.txn_stack.last_mut() else {
                    return Err(unsupported("on_rollback outside transaction", self.span()));
                };
                frame.on_rollback.push(executable.as_ref());
                Ok(CtValue::Unit)
            }
            TExprKind::CoreClosureCall { .. } => {
                Err(unsupported("expr `CoreClosureCall`", self.span()))
            }
            TExprKind::TaskGroupAll { .. } => Err(unsupported("expr `TaskGroupAll`", self.span())),
            TExprKind::TaskGroupRace { .. } => Err(unsupported("expr `TaskGroupRace`", self.span())),
            TExprKind::TaskGroupAny { .. } => Err(unsupported("expr `TaskGroupAny`", self.span())),
            TExprKind::SelectStart => Err(unsupported("expr `SelectStart`", self.span())),
            TExprKind::SelectRecv { .. } => Err(unsupported("expr `SelectRecv`", self.span())),
            TExprKind::SelectAfter { .. } => Err(unsupported("expr `SelectAfter`", self.span())),
            TExprKind::SelectRead { .. } => Err(unsupported("expr `SelectRead`", self.span())),
            TExprKind::SelectWait { .. } => Err(unsupported("expr `SelectWait`", self.span())),
            TExprKind::FnValue { kind } => match kind {
                TFnValueKind::NamedFn {
                    name: Some(name), ..
                } => Ok(self.store_callable(EvalCallable::Named(name))),
                TFnValueKind::NamedFn { name: None, .. } => {
                    Err(unsupported("rendered function coercion", self.span()))
                }
                TFnValueKind::Call { callee, args } => {
                    let callable = self.eval_expr(callee, scope)?;
                    let mut argv = Vec::with_capacity(args.len());
                    for arg in args {
                        argv.push(self.eval_expr(&arg.value, scope)?);
                    }
                    self.call_callable(&callable, argv)
                }
            },
            TExprKind::ModuleCall { .. } => Err(unsupported("expr `ModuleCall`", self.span())),
            TExprKind::ExternCall { .. } => Err(unsupported("expr `ExternCall`", self.span())),
        }
    }

    pub(crate) fn write_back_place(
        &mut self,
        place: &'a TExpr,
        value: CtValue,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        match &place.kind {
            TExprKind::Local(local) => {
                scope.insert(local.name.clone(), value);
                Ok(())
            }
            TExprKind::Borrow { place, .. } => self.write_back_place(place, value, scope),
            TExprKind::Deref(inner) => {
                let pointer = self.eval_expr(inner, scope)?;
                let CtValue::Struct { type_name, fields } = pointer else {
                    return Err(unsupported("raw pointer carrier", self.span()));
                };
                if type_name != "__JetRawLocal" {
                    return Err(unsupported("raw pointer target", self.span()));
                }
                let name = fields.iter().find_map(|(field, value)| match (field.as_str(), value) {
                    ("name", CtValue::Str(name)) => Some(name.clone()),
                    _ => None,
                });
                let Some(name) = name else {
                    return Err(unsupported("raw pointer local", self.span()));
                };
                scope.insert(name, value);
                Ok(())
            }
            TExprKind::Field { recv, field, .. } => {
                let mut base_val = self.eval_expr(recv, scope)?;
                match &mut base_val {
                    CtValue::Struct { fields, .. } => {
                        if let Some((_, slot)) = fields.iter_mut().find(|(n, _)| n == field) {
                            *slot = value;
                        } else {
                            fields.push((field.clone(), value));
                        }
                    }
                    _ => {
                        return Err(unsupported("field write-back on a non-struct", self.span()));
                    }
                }
                self.write_back_place(recv, base_val, scope)
            }
            TExprKind::PoolSlot {
                pool,
                id,
                field,
                ..
            } => {
                let mut pool_value = self.eval_expr(pool, scope)?;
                let id_value = self.eval_expr(id, scope)?;
                let Some((index, generation)) = pool_id_parts(&id_value) else {
                    return Err(pool_stale_diagnostic());
                };
                let CtValue::Struct { fields, .. } = &mut pool_value else {
                    return Err(pool_stale_diagnostic());
                };
                let slots = fields.iter_mut().find_map(|(name, value)| match value {
                    CtValue::List(slots) if name == "slots" => Some(slots),
                    _ => None,
                });
                let Some(CtValue::Enum {
                    variant,
                    args,
                    ..
                }) = slots.and_then(|slots| slots.get_mut(index))
                else {
                    return Err(pool_stale_diagnostic());
                };
                if variant != "Occupied"
                    || !matches!(args.first(), Some((_, CtValue::Int(found))) if *found == generation)
                {
                    return Err(pool_stale_diagnostic());
                }
                let Some((_, payload)) = args.get_mut(1) else {
                    return Err(pool_stale_diagnostic());
                };
                if let Some(field) = field {
                    let CtValue::Struct { fields, .. } = payload else {
                        return Err(unsupported("Pool field on a non-struct", self.span()));
                    };
                    let slot = fields
                        .iter_mut()
                        .find_map(|(name, value)| (name == field).then_some(value))
                        .ok_or_else(|| unsupported(&format!("Pool field `{field}`"), self.span()))?;
                    *slot = value;
                } else {
                    *payload = value;
                }
                self.write_back_place(pool, pool_value, scope)
            }
            _ => Ok(()),
        }
    }

    fn eval_numeric_op(
        &self,
        v: &CtValue,
        op: &crate::Codegen::TIR::TNumericOp,
        recv_ty: &crate::AST::Type,
        result_ty: &crate::AST::Type,
    ) -> Result<CtValue, Diagnostic> {
        let _ = recv_ty;
        use crate::Codegen::TIR::TNumericOp;
        match op {
            TNumericOp::BitCount { method, width } => {
                let CtValue::Int(value) = v else {
                    return Err(unsupported("numeric bit-count recv", self.span()));
                };
                crate::Comptime::MathLayout::integer_bit_count(*value, *width, method)
                    .map(CtValue::Int)
                    .ok_or_else(|| unsupported(&format!("numeric `{method}`"), self.span()))
            }
            TNumericOp::ToShow => Ok(CtValue::Str(
                show_typed_value(v, recv_ty, false).unwrap_or_else(|| v.jet_show()),
            )),
            TNumericOp::Predicate(method) => {
                crate::Comptime::Builtins::apply_method(v, method, vec![], self.span())
            }
            TNumericOp::Origin(origin) => Ok(CtValue::Str(
                origin.clone().unwrap_or_else(|| "untracked".to_string()),
            )),
            TNumericOp::CastAs { .. } => {
                // Width casts keep the same CtValue scalar representation.
                Ok(v.clone())
            }
            TNumericOp::TryFrom {
                dst_spelling,
                host_kind,
                ..
            } => {
                let CtValue::Int(n) = v else {
                    return Err(unsupported("TryFrom expects Int", self.span()));
                };
                let (lo, hi) = match *host_kind {
                    0 => (i8::MIN as i64, i8::MAX as i64),
                    1 => (i16::MIN as i64, i16::MAX as i64),
                    2 => (i32::MIN as i64, i32::MAX as i64),
                    3 => (i64::MIN, i64::MAX),
                    4 => (0, u8::MAX as i64),
                    5 => (0, u16::MAX as i64),
                    6 => (0, u32::MAX as i64),
                    7 => (0, i64::MAX), // U64 in i64 domain for pure-parity
                    _ => (i64::MIN, i64::MAX),
                };
                if *n < lo || *n > hi {
                    return Ok(CtValue::ResErr(Box::new(CtValue::Str(format!(
                        "value doesn't fit in {dst_spelling}"
                    )))));
                }
                let _ = result_ty;
                Ok(CtValue::ResOk(Box::new(CtValue::Int(*n))))
            }
            TNumericOp::FloatToInt {
                dst_spelling,
                lower,
                upper_exclusive,
                ..
            } => {
                let CtValue::Float(f) = v else {
                    return Err(unsupported("FloatToInt expects Float", self.span()));
                };
                let lo: f64 = lower.parse().unwrap_or(f64::NEG_INFINITY);
                let hi: f64 = upper_exclusive.parse().unwrap_or(f64::INFINITY);
                if f.is_finite() && f.as_f64() >= lo && f.as_f64() < hi {
                    Ok(CtValue::ResOk(Box::new(CtValue::Int(f.as_f64().trunc() as i64))))
                } else {
                    Ok(CtValue::ResErr(Box::new(CtValue::Str(format!(
                        "value doesn't fit in {dst_spelling}"
                    )))))
                }
            }
            TNumericOp::FloatNarrow { dst_spelling } => {
                let CtValue::Float(f) = v else {
                    return Err(unsupported("FloatNarrow expects Float", self.span()));
                };
                let n = f.as_f64();
                if n.is_finite() && n >= -(f32::MAX as f64) && n <= f32::MAX as f64 {
                    Ok(CtValue::ResOk(Box::new(CtValue::Float(
                        crate::AST::CtFloat::f32(n as f32),
                    ))))
                } else {
                    Ok(CtValue::ResErr(Box::new(CtValue::Str(format!(
                        "value doesn't fit in {dst_spelling}"
                    )))))
                }
            }
        }
    }

    fn write_print(&mut self, text: &str, to_stderr: bool) -> Result<(), Diagnostic> {
        let Some(sink) = self.sink.as_mut() else {
            return Err(unsupported("print at comptime", self.span()));
        };
        if to_stderr {
            sink.stderr.push_str(text);
            sink.stderr.push('\n');
        } else {
            sink.stdout.push_str(text);
            sink.stdout.push('\n');
        }
        Ok(())
    }

    fn eval_call(
        &mut self,
        name: &str,
        args: &'a [TCallArg],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let mut argv = Vec::with_capacity(args.len());
        for a in args {
            let mut v = self.eval_expr(&a.value, scope)?;
            // D-UNIONTYPE1=A: member → union inject at the call boundary (mirrors emit).
            if let Some(Type::Union(members)) = &a.widen_to_union {
                let tag = crate::AST::union_member_tag(&a.value.ty);
                if members.iter().any(|m| m == &a.value.ty) {
                    v = CtValue::Enum {
                        type_name: crate::AST::union_enum_name(members),
                        variant: tag,
                        args: vec![(None, v)],
                    };
                }
            }
            argv.push(v);
            // Try/`?` may set pending_return mid-arg; abort the call (don't print Unit).
            if self.pending_return.is_some() {
                return Ok(CtValue::Unit);
            }
        }
        if name == "print" {
            let text = argv.first().map(|v| v.jet_show()).unwrap_or_default();
            self.write_print(&text, false)?;
            return Ok(CtValue::Unit);
        }
        if name == "eprint" {
            let text = argv.first().map(|v| v.jet_show()).unwrap_or_default();
            self.write_print(&text, true)?;
            return Ok(CtValue::Unit);
        }
        // D-TOOL4: `expect(x)` — wrap Display text for `.snapshot()`.
        if name == crate::Syntax::BUILTIN_EXPECT && self.funcs.get(name).is_none() {
            if argv.len() != 1 {
                return Err(unsupported("`expect` needs exactly one value", self.span()));
            }
            let shown = argv[0].jet_show();
            return Ok(CtValue::Struct {
                type_name: "__JetExpect__".to_string(),
                fields: vec![("value".into(), CtValue::Str(shown))],
            });
        }
        if name == "consume" && self.funcs.get(name).is_none() {
            if argv.len() != 1 {
                return Err(unsupported("`consume` discards exactly one value", self.span()));
            }
            return Ok(CtValue::Unit);
        }
        // D-METADERIVE1=A: `emit(source_string)` — push a re-entry fragment.
        if name == "emit" {
            let Some(CtValue::Str(s)) = argv.into_iter().next() else {
                return Err(unsupported("`emit` argument must be a string", self.span()));
            };
            let fragment = crate::Comptime::apply_dollar_splices(&s, scope);
            if let Some(out) = self.emitted_fragments.as_mut() {
                out.push(fragment);
            }
            return Ok(CtValue::Unit);
        }
        // #778: prefer Cranelift-native callees when the deopt tier installed a hook.
        if let Some(hook) = super::native_call_hook() {
            if let Some(result) = hook(name, &argv) {
                return result;
            }
        }
        let Some(func) = self.funcs.get(name).copied() else {
            return Err(unsupported(&format!("call `{name}`"), self.span()));
        };
        if matches!(
            &func.ret,
            Some(Type::Apply { name, .. }) if name == crate::Syntax::TYPE_STREAM
        ) {
            return Ok(self.store_stream(func, argv));
        }
        let mut child = HashMap::new();
        let result = self.run_func(func, argv, &mut child)?;
        // CtValue params are copy-in/copy-out. Fragment lowering often lacks
        // `cx.sigs`, so call-site `borrow`/`mut_borrow` flags may be false —
        // use the callee's own param conventions instead (#722).
        for ((pname, pty, conv), carg) in func.params.iter().zip(args.iter()) {
            let needs_wb = match conv {
                crate::AST::AccessConvention::Write => true,
                crate::AST::AccessConvention::Read if !pty.is_scalar() => true,
                _ => false,
            };
            if !needs_wb {
                continue;
            }
            let jet = pname.strip_prefix("user_").unwrap_or(pname.as_str());
            if let Some(updated) = child.get(jet) {
                self.write_back_place(&carg.value, updated.clone(), scope)?;
            }
        }
        Ok(result)
    }

    pub(super) fn show_value(
        &mut self,
        v: &CtValue,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<String, Diagnostic> {
        let _ = scope;
        if let Some(text) = crate::Comptime::display_core_pure_value(v) {
            return Ok(text);
        }
        if let CtValue::Struct { type_name, .. } | CtValue::Enum { type_name, .. } = v {
            let key = format!("{type_name}::display");
            if let Some(func) = self.funcs.get(&key).copied() {
                let mut child = HashMap::new();
                child.insert("self".to_string(), v.clone());
                if let CtValue::Str(s) = self.run_func(func, Vec::new(), &mut child)? {
                    return Ok(s);
                }
            }
        }
        Ok(v.jet_show())
    }

    pub(super) fn debug_value(&self, v: &CtValue) -> String {
        match v {
            CtValue::Struct { type_name, fields } => {
                let ty = type_name.strip_prefix("user_").unwrap_or(type_name);
                let Some(defs) = self.struct_fields.get(ty) else {
                    return v.debug_rust();
                };
                if defs.is_empty() {
                    return format!("{ty} {{}}");
                }
                let parts: Vec<String> = defs
                    .iter()
                    .map(|(name, redact)| {
                        if *redact {
                            format!("{name}: [redacted]")
                        } else {
                            let rendered = fields
                                .iter()
                                .find(|(n, _)| n == name || n.strip_prefix("user_") == Some(name.as_str()))
                                .map(|(_, value)| value.debug_rust())
                                .unwrap_or_else(|| CtValue::Unit.debug_rust());
                            format!("{name}: {rendered}")
                        }
                    })
                    .collect();
                format!("{ty} {{ {} }}", parts.join(", "))
            }
            CtValue::Enum {
                type_name,
                variant,
                args,
            } => {
                let ty = type_name.strip_prefix("user_").unwrap_or(type_name);
                let var = variant.strip_prefix("user_").unwrap_or(variant);
                if args.is_empty() {
                    format!("{ty}.{var}")
                } else if args.iter().all(|(label, _)| label.is_some()) {
                    let parts: Vec<String> = args
                        .iter()
                        .map(|(label, val)| {
                            format!(
                                "{}: {}",
                                label.as_deref().unwrap_or(""),
                                self.debug_value(val)
                            )
                        })
                        .collect();
                    format!("{ty}.{var} {{ {} }}", parts.join(", "))
                } else {
                    let parts: Vec<String> = args
                        .iter()
                        .map(|(_, val)| self.debug_value(val))
                        .collect();
                    format!("{ty}.{var}({})", parts.join(", "))
                }
            }
            _ => v.debug_rust(),
        }
    }

}

fn pool_id_parts(value: &CtValue) -> Option<(usize, i64)> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "Id" {
        return None;
    }
    let int_field = |wanted: &str| {
        fields.iter().find_map(|(name, value)| match value {
            CtValue::Int(value) if name == wanted => Some(*value),
            _ => None,
        })
    };
    Some((usize::try_from(int_field("index")?).ok()?, int_field("generation")?))
}

fn pool_stale_diagnostic() -> Diagnostic {
    Diagnostic::error(
        "E0953",
        "your comptime code stopped the build".to_string(),
        "while computing this value at compile time, the program panicked: this Id no longer refers to a live value — its pool slot was removed".to_string(),
        "this is the sanctioned way to validate at compile time — fix the input the check rejects"
            .to_string(),
        None,
    )
}

fn eval_precise_builtin(
    type_name: &str,
    func: &str,
    args: Vec<CtValue>,
    span: crate::Diagnostics::Span,
) -> Result<CtValue, Diagnostic> {
    use jet_foundation::Numeric::{CtBigInt, CtDecimal};
    match (type_name, func) {
        ("BigInt", "from_int") => match args.into_iter().next() {
            Some(CtValue::Int(n)) => Ok(CtValue::BigInt(CtBigInt::from_int(n))),
            _ => Err(unsupported("`BigInt.from_int`", span)),
        },
        ("BigInt", "from_str") => match args.into_iter().next() {
            Some(CtValue::Str(s)) => CtBigInt::from_str(&s)
                .map(CtValue::BigInt)
                .map_err(|_| unsupported(&format!("`BigInt(\"{s}\")`"), span)),
            _ => Err(unsupported("`BigInt.from_str`", span)),
        },
        ("Decimal", "from_str") => match args.into_iter().next() {
            Some(CtValue::Str(s)) => CtDecimal::from_str(&s)
                .map(|d| d.to_value())
                .map_err(|_| unsupported(&format!("`Decimal(\"{s}\")`"), span)),
            _ => Err(unsupported("`Decimal.from_str`", span)),
        },
        ("BigInt" | "Decimal", "add" | "sub" | "mul" | "neg" | "to_string") => {
            let mut it = args.into_iter();
            let Some(recv) = it.next() else {
                return Err(unsupported(&format!("`{type_name}.{func}`"), span));
            };
            let rest: Vec<_> = it.collect();
            crate::Comptime::Builtins::apply_method(&recv, func, rest, span)
        }
        _ => Err(unsupported(
            &format!("precise `{type_name}.{func}`"),
            span,
        )),
    }
}
