//! Exhaustive TExprKind evaluation (#777).
use std::collections::HashMap;
use crate::AST::{CtFloat, Type, UnOp};
use crate::Codegen::TIR::{TCallArg, TExpr, TExprKind, TPlace, TStrPart};
use crate::Comptime::Builtins::{as_bool, as_int, eval_binop};
use crate::Comptime::{apply_core_call, apply_impure_core_call, CtValue};
use crate::Diagnostics::Diagnostic;
use super::builtins::eval_builtin;
use super::handles::eval_handle;
use super::{unsupported, EvalCtx, Flow};

impl EvalCtx<'_> {
    pub(crate) fn eval_expr(
        &mut self,
        expr: &TExpr,
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
                                crate::AST::StrFormat::Debug => self.debug_value(&v),
                                crate::AST::StrFormat::Display => self.show_value(&v, scope)?,
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
                self.write_print(&v.jet_show(), false)?;
                Ok(CtValue::Unit)
            }
            TExprKind::Drop(inner) | TExprKind::Close(inner) => {
                let _ = self.eval_expr(inner, scope)?;
                Ok(CtValue::Unit)
            }
            TExprKind::Binary { op, lhs, rhs, .. } => {
                let l = self.eval_expr(lhs, scope)?;
                let r = self.eval_expr(rhs, scope)?;
                eval_binop(*op, l, r, self.span())
            }
            TExprKind::Unary { op, operand } => {
                let v = self.eval_expr(operand, scope)?;
                match (*op, v) {
                    (UnOp::Neg, CtValue::Int(n)) => Ok(CtValue::Int(-n)),
                    (UnOp::Neg, CtValue::Float(n)) => Ok(CtValue::Float(CtFloat::f64(-n.as_f64()))),
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
                    let part = eval_binop(*op, vals[i].clone(), vals[i + 1].clone(), self.span())?;
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
                            return Err(unsupported(
                                &format!("if-expr then {other:?}"),
                                self.span(),
                            ))
                        }
                    }
                    self.eval_expr(then_value, scope)
                } else {
                    match self.exec_stmts(else_body, scope)? {
                        Flow::Return(v) => return Ok(v),
                        Flow::Normal => {}
                        other => {
                            return Err(unsupported(
                                &format!("if-expr else {other:?}"),
                                self.span(),
                            ))
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
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(a, scope)?);
                }
                let result = eval_builtin(op, &mut r, argv, self.span())?;
                self.write_back_place(recv, r, scope)?;
                Ok(result)
            }
            TExprKind::HandleMethod { recv, op, args } => {
                let mut r = self.eval_expr(recv, scope)?;
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(a, scope)?);
                }
                let result = eval_handle(op, &mut r, argv, self.span())?;
                self.write_back_place(recv, r, scope)?;
                Ok(result)
            }
            TExprKind::CoreCall {
                module,
                method,
                args,
                ..
            } => {
                if module == "core.data" {
                    return self.eval_core_data_call(method, args, &expr.ty, scope);
                }
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(a, scope)?);
                }
                // Runtime deopt / `jet run` sets impure_depth>0 so Tier-2
                // ambient I/O matches AOT (env/fs/process). Pure comptime
                // keeps depth 0 and stays on apply_core_call (E3410).
                if self.impure_depth > 0 {
                    apply_impure_core_call(
                        module,
                        method,
                        argv,
                        self.span(),
                        &self.base_dir,
                        self.sink.as_deref_mut(),
                        self.repl_mode,
                        None,
                        None,
                    )
                } else {
                    apply_core_call(module, method, argv, self.span(), self.repl_mode)
                }
            }
            TExprKind::StructLit { fields, .. } => {
                let mut out = Vec::with_capacity(fields.len());
                for (name, val, _) in fields {
                    out.push((name.clone(), self.eval_expr(val, scope)?));
                }
                let type_name = match &expr.ty {
                    crate::AST::Type::Named(n) => n.clone(),
                    crate::AST::Type::Apply { name, .. } => name.clone(),
                    _ => "struct".into(),
                };
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
                let b = self.eval_expr(base, scope)?;
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
                        _ => Err(unsupported("index recv", self.span())),
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
                if let Ok(v) = crate::Comptime::Builtins::apply_method(
                    &r,
                    &method.name,
                    argv.clone(),
                    self.span(),
                ) {
                    return Ok(v);
                }
                const MUTATING: &[&str] = &[
                    "push", "pop", "add", "add_new", "insert", "remove", "clear", "reverse",
                    "sort",
                ];
                if MUTATING.contains(&method.name.as_str()) {
                    let ret = crate::Comptime::Builtins::apply_mutating(
                        &mut r,
                        &method.name,
                        argv,
                        self.span(),
                    )?;
                    self.write_back_place(recv, r, scope)?;
                    return Ok(ret);
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
                        let argv_for_params = if matches!(
                            &func.kind,
                            crate::Codegen::TIR::TFuncKind::Method {
                                self_conv: Some(_),
                                ..
                            }
                        ) {
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
            TExprKind::Try { inner, .. } => {
                let v = self.eval_expr(inner, scope)?;
                match v {
                    CtValue::ResOk(inner) | CtValue::Some(inner) => Ok(*inner),
                    CtValue::ResErr(e) => {
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
                let miss = if *is_option {
                    matches!(v, CtValue::None(_))
                } else {
                    matches!(v, CtValue::ResErr(_))
                };
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
            TExprKind::HostCall(..) => Err(unsupported("expr `HostCall`", self.span())),
            TExprKind::DataEntriesToMap(..) => Err(unsupported("expr `DataEntriesToMap`", self.span())),
            TExprKind::DistinctCtor { .. } => Err(unsupported("expr `DistinctCtor`", self.span())),
            TExprKind::RangeCheckedCtor { .. } => {
                Err(unsupported("expr `RangeCheckedCtor`", self.span()))
            }
            TExprKind::DistinctConvert { .. } => {
                Err(unsupported("expr `DistinctConvert`", self.span()))
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
                    &format!("math `{type_name}.{func}`"),
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
            TExprKind::ResourceNew(..) => Err(unsupported("expr `ResourceNew`", self.span())),
            TExprKind::ResourceTake(..) => Err(unsupported("expr `ResourceTake`", self.span())),
            TExprKind::AmbientInput { .. } => Err(unsupported("expr `AmbientInput`", self.span())),
            TExprKind::RequireStop { .. } => Err(unsupported("expr `RequireStop`", self.span())),
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
            TExprKind::Deref(inner) => self.eval_expr(inner, scope),
            TExprKind::RawOf(inner) => self.eval_expr(inner, scope),
            TExprKind::AllocNew { .. } => Err(unsupported("expr `AllocNew`", self.span())),
            TExprKind::JsonLit { .. } => Err(unsupported("expr `JsonLit`", self.span())),
            TExprKind::DbValueLit { .. } => Err(unsupported("expr `DbValueLit`", self.span())),
            TExprKind::ListSpread { .. } => Err(unsupported("expr `ListSpread`", self.span())),
            TExprKind::ColumnarListLit { .. } => {
                Err(unsupported("expr `ColumnarListLit`", self.span()))
            }
            TExprKind::ColumnarGather { .. } => {
                Err(unsupported("expr `ColumnarGather`", self.span()))
            }
            TExprKind::ColumnarColumnRead { .. } => {
                Err(unsupported("expr `ColumnarColumnRead`", self.span()))
            }
            TExprKind::PoolSlot { .. } => Err(unsupported("expr `PoolSlot`", self.span())),
            TExprKind::IndexHook { .. } => Err(unsupported("expr `IndexHook`", self.span())),
            TExprKind::MathLaneIndex { .. } => Err(unsupported("expr `MathLaneIndex`", self.span())),
            TExprKind::MathSwizzleRead { .. } => {
                Err(unsupported("expr `MathSwizzleRead`", self.span()))
            }
            TExprKind::FnFieldCall { .. } => Err(unsupported("expr `FnFieldCall`", self.span())),
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
                        if let Some(res) = crate::Comptime::Builtins::apply_static_type_method(
                            type_name,
                            &method.name,
                            argv.clone(),
                            self.span(),
                        ) {
                            return res;
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
            TExprKind::Todo { .. } => Err(unsupported("expr `Todo`", self.span())),
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
            TExprKind::Lambda(..) => Err(unsupported("expr `Lambda`", self.span())),
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
            TExprKind::NumericMethod { .. } => {
                Err(unsupported("expr `NumericMethod`", self.span()))
            }
            TExprKind::OverflowOpt { .. } => Err(unsupported("expr `OverflowOpt`", self.span())),
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
            TExprKind::FnValue { .. } => Err(unsupported("expr `FnValue`", self.span())),
            TExprKind::ModuleCall { .. } => Err(unsupported("expr `ModuleCall`", self.span())),
            TExprKind::ExternCall { .. } => Err(unsupported("expr `ExternCall`", self.span())),
        }
    }

    pub(crate) fn write_back_place(
        &mut self,
        place: &TExpr,
        value: CtValue,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        match &place.kind {
            TExprKind::Local(local) => {
                scope.insert(local.name.clone(), value);
                Ok(())
            }
            TExprKind::Borrow { place, .. } => self.write_back_place(place, value, scope),
            _ => Ok(()),
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
        args: &[TCallArg],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let mut argv = Vec::with_capacity(args.len());
        for a in args {
            argv.push(self.eval_expr(&a.value, scope)?);
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
        let mut child = HashMap::new();
        self.run_func(func, argv, &mut child)
    }

    pub(super) fn show_value(
        &mut self,
        v: &CtValue,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<String, Diagnostic> {
        let _ = scope;
        if let CtValue::Struct { type_name, .. } | CtValue::Enum { type_name, .. } = v {
            let key = format!("{type_name}::display");
            if let Some(func) = self.funcs.get(&key).copied() {
                let mut child = HashMap::new();
                if let CtValue::Str(s) = self.run_func(func, vec![v.clone()], &mut child)? {
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
