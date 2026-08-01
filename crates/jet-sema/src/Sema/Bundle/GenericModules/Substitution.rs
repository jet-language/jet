use super::*;

fn ct_value_expr(value: &crate::AST::CtValue, span: crate::Diagnostics::Span) -> Expr {
    match value {
        crate::AST::CtValue::Bool(v) => Expr::Bool(*v, span),
        crate::AST::CtValue::Int(v) => Expr::Int(*v, span, None, None),
        crate::AST::CtValue::Char(v) => Expr::Char(*v, span),
        crate::AST::CtValue::Str(v) => Expr::Str(vec![StrPart::Lit(v.clone())], span),
        crate::AST::CtValue::Enum {
            type_name,
            variant,
            args,
        } if args.is_empty() => Expr::EnumLit {
            type_name: type_name.clone(),
            variant: variant.clone(),
            args: Vec::new(),
            span,
        },
        _ => unreachable!("generic-module value domain was checked before substitution"),
    }
}
pub(super) fn substitute_meta(
    meta: &mut Option<crate::AST::MetaAttr>,
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) {
    let Some(meta) = meta else {
        return;
    };
    for field in &mut meta.fields {
        match field {
            crate::AST::MetaField::Category { value, .. }
            | crate::AST::MetaField::Maturity { value, .. } => {
                substitute_expr(value, types, values)
            }
            crate::AST::MetaField::Unknown {
                value: Some(value),
                ..
            } => {
                substitute_expr(value, types, values)
            }
            crate::AST::MetaField::Tunable { .. }
            | crate::AST::MetaField::Unknown { value: None, .. } => {}
        }
    }
}

pub(super) fn substitute_expr(
    expr: &mut Expr,
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) {
    if let Expr::Ident(name, span) = expr {
        if let Some(value) = values.get(name) {
            *expr = ct_value_expr(value, *span);
            return;
        }
    }
    match expr {
        Expr::Ident(..)
        | Expr::Char(..)
        | Expr::Int(..)
        | Expr::Float(..)
        | Expr::Bool(..)
        | Expr::Absent(..)
        | Expr::ReduceMarker(..)
        | Expr::Todo { .. }
        | Expr::UnitLit { .. }
        | Expr::ComptimeSplice { .. }
        | Expr::StrMatchLit(..)
        | Expr::BinMatchLit(..) => {}
        Expr::Str(parts, _) => parts.iter_mut().for_each(|part| {
            if let StrPart::Interp(inner, _) = part {
                substitute_expr(inner, types, values);
            }
        }),
        Expr::Call(call) => {
            call.args
                .iter_mut()
                .for_each(|arg| substitute_expr(&mut arg.expr, types, values));
        }
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Field(inner, _, _)
        | Expr::Tainted(inner, _, _)
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _)
        | Expr::Paren(inner, _)
        | Expr::Spread(inner, _) => substitute_expr(inner, types, values),
        Expr::MemberSpread { base, .. } => substitute_expr(base, types, values),
        Expr::OptField { base, .. } => substitute_expr(base, types, values),
        Expr::Range { start, end, .. } => {
            substitute_expr(start, types, values);
            substitute_expr(end, types, values);
        }
        Expr::MethodCall {
            receiver,
            method,
            method_span,
            type_args,
            args,
            recv_type,
            resolved_ret,
            ..
        } => {
            if let Expr::Ident(name, _) = receiver.as_mut() {
                if let Some(Type::Named(resolved)) = types.get(name) {
                    *name = resolved.clone();
                }
            }
            substitute_expr(receiver, types, values);
            for ty in type_args {
                *ty = crate::Generics::substitute_type(ty, types);
            }
            if let Some(ty) = resolved_ret {
                *ty = crate::Generics::substitute_type(ty, types);
            }
            args.iter_mut()
                .for_each(|arg| substitute_expr(&mut arg.expr, types, values));
            // Rewrite the sema-recorded receiver type through the same subst so
            // monomorphized JIT/AOT bodies see `DBConnection::query` rather than
            // a leftover type-parameter owner (`T::query`) after `T` is gone.
            if let Some(name) = recv_type.as_mut() {
                match types.get(name) {
                    Some(Type::Named(resolved)) => *name = resolved.clone(),
                    Some(Type::Apply { name: resolved, .. }) => *name = resolved.clone(),
                    _ => {}
                }
            }
            let primitive = recv_type
                .as_ref()
                .and_then(|name| types.get(name))
                .is_some_and(|ty| {
                    matches!(
                        ty,
                        Type::Int | Type::Float | Type::Bool | Type::Char | Type::String
                    )
                });
            let op = match method.as_str() {
                "add" => Some(crate::AST::BinOp::Add),
                "sub" => Some(crate::AST::BinOp::Sub),
                "mul" => Some(crate::AST::BinOp::Mul),
                "div" => Some(crate::AST::BinOp::Div),
                "equal" => Some(crate::AST::BinOp::Eq),
                _ => None,
            };
            if primitive && args.len() == 1 {
                if let Some(op) = op {
                    *expr = Expr::Binary(
                        op,
                        receiver.clone(),
                        Box::new(args[0].expr.clone()),
                        *method_span,
                    );
                }
            }
        }
        Expr::StructLit {
            type_name,
            type_args,
            fields,
            ..
        } => {
            if let Some(Type::Named(resolved)) = types.get(type_name) {
                *type_name = resolved.clone();
            }
            for ty in type_args {
                *ty = crate::Generics::substitute_type(ty, types);
            }
            fields
                .iter_mut()
                .for_each(|(_, _, value)| substitute_expr(value, types, values));
        }
        Expr::TypedLit { head, body, .. } => {
            if let Some(h) = head {
                *h = crate::Generics::substitute_type(h, types);
            }
            body.for_each_expr_mut(|e| substitute_expr(e, types, values));
        }
        Expr::EnumLit {
            type_name, args, ..
        } => {
            if let Some(Type::Named(resolved)) = types.get(type_name) {
                *type_name = resolved.clone();
            }
            args.iter_mut().for_each(|arg| match arg {
                EnumLitArg::Positional(value) | EnumLitArg::Named { expr: value, .. } => {
                    substitute_expr(value, types, values)
                }
            });
        }
        Expr::OrFallback {
            value, fallback, ..
        } => {
            substitute_expr(value, types, values);
            match fallback {
                OrFallback::Value(value) | OrFallback::Return(Some(value), _) => {
                    substitute_expr(value, types, values)
                }
                OrFallback::Panic { args, .. } => args
                    .iter_mut()
                    .for_each(|arg| substitute_expr(&mut arg.expr, types, values)),
                OrFallback::Return(None, _)
                | OrFallback::Break(_)
                | OrFallback::Continue(_)
                | OrFallback::BreakLabel(..)
                | OrFallback::ContinueLabel(..) => {}
            }
        }
        Expr::PatternTest { subject, .. } => substitute_expr(subject, types, values),
        Expr::Binary(_, left, right, _) => {
            substitute_expr(left, types, values);
            substitute_expr(right, types, values);
        }
        Expr::CompareChain { operands, .. } | Expr::ListLit(operands, _) => operands
            .iter_mut()
            .for_each(|value| substitute_expr(value, types, values)),
        Expr::TupleLit(fields, _, inferred) => {
            fields
                .iter_mut()
                .for_each(|(_, value)| substitute_expr(value, types, values));
            if let Some(ty) = inferred {
                *ty = crate::Generics::substitute_type(ty, types);
            }
        }
        Expr::MapLit(entries, _) => entries.iter_mut().for_each(|(key, value)| {
            substitute_expr(key, types, values);
            substitute_expr(value, types, values);
        }),
        Expr::Index { base, index, .. } => {
            substitute_expr(base, types, values);
            substitute_expr(index, types, values);
        }
        Expr::Slice {
            base, start, end, range, ..
        } => {
            substitute_expr(base, types, values);
            if let Some(range) = range {
                substitute_expr(range, types, values);
            } else {
                substitute_expr(start, types, values);
                substitute_expr(end, types, values);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            substitute_expr(callee, types, values);
            args.iter_mut()
                .for_each(|arg| substitute_expr(&mut arg.expr, types, values));
        }
        Expr::Lambda(lambda) => {
            for param in &mut lambda.params {
                if let Some(ty) = &mut param.ty {
                    *ty = crate::Generics::substitute_type(ty, types);
                }
            }
            match &mut lambda.body {
                LambdaBody::Expr(value) => substitute_expr(value, types, values),
                LambdaBody::Block(body) => substitute_stmts(body, types, values),
            }
        }
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            substitute_expr(cond, types, values);
            substitute_stmts(then_body, types, values);
            substitute_expr(then_value, types, values);
            substitute_stmts(else_body, types, values);
            substitute_expr(else_value, types, values);
        }
        Expr::PtrFromAddr { elem, addr, .. } => {
            *elem = crate::Generics::substitute_type(elem, types);
            substitute_expr(addr, types, values);
        }
    }
}

pub(super) fn substitute_stmts(
    stmts: &mut [Stmt],
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(value) | Stmt::Yield(value, _) => substitute_expr(value, types, values),
            Stmt::Val(binding) => {
                substitute_meta(&mut binding.meta, types, values);
                if let Some(ty) = &mut binding.ty {
                    *ty = specialize_module_type(ty, types, values);
                }
                substitute_expr(&mut binding.init, types, values);
            }
            Stmt::Assign { value, .. } | Stmt::Return(Some(value), _) => {
                substitute_expr(value, types, values)
            }
            Stmt::BreakValue(value, _) | Stmt::BreakLabelValue(_, _, value, _) => {
                substitute_expr(value, types, values)
            }
            Stmt::Return(None, _)
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::BreakLabel(..)
            | Stmt::ContinueLabel(..) => {}
            Stmt::While { cond, body, .. } => {
                substitute_expr(cond, types, values);
                substitute_stmts(body, types, values);
            }
            Stmt::For { kind, body, .. } => {
                match kind {
                    ForKind::Range { start, end, step, exclusive: _ } => {
                        substitute_expr(start, types, values);
                        substitute_expr(end, types, values);
                        if let Some(step) = step {
                            substitute_expr(step, types, values);
                        }
                    }
                    ForKind::In { collection, step } => {
                        substitute_expr(collection, types, values);
                        if let Some(step) = step { substitute_expr(step, types, values); }
                    }
                }
                substitute_stmts(body, types, values);
            }
            Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            }
            | Stmt::ComptimeSwitch {
                subject,
                arms,
                else_body,
                ..
            } => {
                substitute_expr(subject, types, values);
                for arm in arms {
                    substitute_expr(&mut arm.cond, types, values);
                    substitute_stmts(&mut arm.body, types, values);
                }
                if let Some(body) = else_body {
                    substitute_stmts(body, types, values);
                }
            }
            Stmt::CountedLoop {
                init,
                cond,
                step,
                body,
                ..
            } => {
                if let Some(ty) = &mut init.ty {
                    *ty = specialize_module_type(ty, types, values);
                }
                substitute_expr(&mut init.init, types, values);
                substitute_expr(cond, types, values);
                if let Some(step) = step {
                    substitute_stmts(std::slice::from_mut(step), types, values);
                }
                substitute_stmts(body, types, values);
            }
            Stmt::Loop { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Reactive { body, .. }
            | Stmt::Shield { body, .. }
            | Stmt::Off { body, .. }
            | Stmt::DebugOnly { body, .. }
            | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::ComptimeBlock { body, .. }
            | Stmt::Live { body, .. }
            | Stmt::ScopeMember { body, .. } => substitute_stmts(body, types, values),
            Stmt::ComptimeIf {
                cond,
                then_body,
                else_body,
                ..
            } => {
                substitute_expr(cond, types, values);
                substitute_stmts(then_body, types, values);
                if let Some(body) = else_body {
                    substitute_stmts(body, types, values);
                }
            }
            Stmt::ContextBlock { fields, body, .. } => {
                fields
                    .iter_mut()
                    .for_each(|(_, value, _)| substitute_expr(value, types, values));
                substitute_stmts(body, types, values);
            }
        }
    }
}
