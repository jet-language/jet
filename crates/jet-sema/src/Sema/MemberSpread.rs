//! D-SPREAD1=A: desugar `prefix.[a, b]` to field lists before inference.

use crate::AST::{
    EnumLitArg, Expr, ForKind, Item, LambdaBody, ProgramBundle, Stmt, StrPart, TypedLitBody,
};
use crate::Diagnostics::Span;

/// Expand every `MemberSpread` in the bundle before registration/checking.
pub fn desugar_member_spreads(bundle: &mut ProgramBundle) {
    for module in &mut bundle.modules {
        for item in &mut module.items {
            match item {
                Item::Func(f) => desugar_stmts(&mut f.body),
                Item::Const(c) => desugar_expr(&mut c.value),
                Item::Test(t) => {
                    if let Some(e) = &mut t.name_expr {
                        desugar_expr(e);
                    }
                    desugar_stmts(&mut t.body);
                }
                Item::Bench(b) => {
                    desugar_expr(&mut b.name_expr);
                    desugar_stmts(&mut b.body);
                }
                Item::Struct(s) => {
                    for m in &mut s.methods {
                        desugar_stmts(&mut m.body);
                    }
                    for b in &mut s.trait_impls {
                        for m in &mut b.methods {
                            desugar_stmts(&mut m.body);
                        }
                    }
                }
                Item::Enum(e) => {
                    for m in &mut e.methods {
                        desugar_stmts(&mut m.body);
                    }
                }
                Item::Impl(i) => {
                    for m in &mut i.methods {
                        desugar_stmts(&mut m.body);
                    }
                }
                _ => {}
            }
        }
    }
}

fn desugar_stmts(stmts: &mut [Stmt]) {
    for stmt in stmts.iter_mut() {
        desugar_stmt(stmt);
    }
}

fn desugar_stmt(stmt: &mut Stmt) {
    match stmt {
        Stmt::Expr(e) | Stmt::Yield(e, _) | Stmt::BreakValue(e, _) => desugar_expr(e),
        Stmt::BreakLabelValue(_, _, e, _) => desugar_expr(e),
        Stmt::Val(b) => desugar_expr(&mut b.init),
        Stmt::Assign { value, .. } => desugar_expr(value),
        Stmt::Return(Some(e), _) => desugar_expr(e),
        Stmt::While { cond, body, .. } => {
            desugar_expr(cond);
            desugar_stmts(body);
        }
        Stmt::For { kind, body, .. } => {
            match kind {
                ForKind::Range {
                    start, end, step, ..
                } => {
                    desugar_expr(start);
                    desugar_expr(end);
                    if let Some(s) = step {
                        desugar_expr(s);
                    }
                }
                ForKind::In { collection, step } => {
                    desugar_expr(collection);
                    if let Some(s) = step {
                        desugar_expr(s);
                    }
                }
            }
            desugar_stmts(body);
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
            desugar_expr(subject);
            for arm in arms {
                desugar_expr(&mut arm.cond);
                desugar_stmts(&mut arm.body);
            }
            if let Some(eb) = else_body {
                desugar_stmts(eb);
            }
        }
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            desugar_expr(&mut init.init);
            desugar_expr(cond);
            if let Some(s) = step {
                desugar_stmt(s);
            }
            desugar_stmts(body);
        }
        Stmt::Loop { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::Live { body, .. } => desugar_stmts(body),
        Stmt::Unsafe {
            audit_expr, body, ..
        } => {
            if let Some(e) = audit_expr {
                desugar_expr(e);
            }
            desugar_stmts(body);
        }
        Stmt::Impure {
            reason_expr, body, ..
        } => {
            if let Some(e) = reason_expr {
                desugar_expr(e);
            }
            desugar_stmts(body);
        }
        Stmt::AssumeDet {
            reason_expr, body, ..
        } => {
            desugar_expr(reason_expr);
            desugar_stmts(body);
        }
        Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            ..
        } => {
            desugar_expr(cond);
            desugar_stmts(then_body);
            if let Some(eb) = else_body {
                desugar_stmts(eb);
            }
        }
        Stmt::ContextBlock { fields, body, .. } => {
            for (_, value, _) in fields {
                desugar_expr(value);
            }
            desugar_stmts(body);
        }
        Stmt::ScopeMember { args, body, .. } => {
            for a in args {
                desugar_expr(a);
            }
            desugar_stmts(body);
        }
        _ => {}
    }
}

fn fields_from_spread(base: &Expr, members: &[(String, Span)], span: Span) -> Vec<Expr> {
    members
        .iter()
        .map(|(name, member_span)| {
            Expr::Field(
                Box::new(base.clone()),
                name.clone(),
                Span::new(span.start, member_span.end),
            )
        })
        .collect()
}

/// Expand `MemberSpread`; splice fields into parent lists.
fn desugar_expr(expr: &mut Expr) {
    // List splice must see MemberSpread before it is rewritten to ListLit.
    if let Expr::ListLit(elems, _) = expr {
        let mut out = Vec::new();
        for elem in std::mem::take(elems) {
            match elem {
                Expr::MemberSpread {
                    mut base,
                    members,
                    span,
                } => {
                    desugar_expr(&mut base);
                    out.extend(fields_from_spread(&base, &members, span));
                }
                mut other => {
                    desugar_expr(&mut other);
                    out.push(other);
                }
            }
        }
        *elems = out;
        return;
    }

    if let Expr::MemberSpread {
        base,
        members,
        span,
    } = expr
    {
        desugar_expr(base);
        let fields = fields_from_spread(base, members, *span);
        *expr = Expr::ListLit(fields, *span);
        return;
    }

    match expr {
        Expr::Spread(inner, _)
        | Expr::Paren(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Unary(_, inner, _)
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _)
        | Expr::Tainted(inner, _, _)
        | Expr::Field(inner, _, _)
        | Expr::Place(inner, _, _) => desugar_expr(inner),
        Expr::Binary(_, a, b, _) => {
            desugar_expr(a);
            desugar_expr(b);
        }
        Expr::CompareChain { operands, .. } => {
            for op in operands {
                desugar_expr(op);
            }
        }
        Expr::OptField { base, .. } => desugar_expr(base),
        Expr::Call(call) => {
            for arg in &mut call.args {
                desugar_expr(&mut arg.expr);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            desugar_expr(receiver);
            for arg in args {
                desugar_expr(&mut arg.expr);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            desugar_expr(callee);
            for arg in args {
                desugar_expr(&mut arg.expr);
            }
        }
        Expr::Index { base, index, .. } => {
            desugar_expr(base);
            desugar_expr(index);
        }
        Expr::Slice {
            base,
            start,
            end,
            range,
            ..
        } => {
            desugar_expr(base);
            desugar_expr(start);
            desugar_expr(end);
            if let Some(r) = range {
                desugar_expr(r);
            }
        }
        Expr::Range { start, end, .. } => {
            desugar_expr(start);
            desugar_expr(end);
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, value) in fields {
                desugar_expr(value);
            }
        }
        Expr::TypedLit { body, .. } => match body {
            TypedLitBody::Fields(fields) => {
                for (_, _, value) in fields {
                    desugar_expr(value);
                }
            }
            TypedLitBody::Elements(elems) => {
                for e in elems {
                    desugar_expr(e);
                }
            }
            TypedLitBody::Entries(entries) => {
                for (k, v) in entries {
                    desugar_expr(k);
                    desugar_expr(v);
                }
            }
            TypedLitBody::Value(inner) => desugar_expr(inner),
            TypedLitBody::Empty => {}
        },
        Expr::EnumLit { args, .. } => {
            for arg in args {
                match arg {
                    EnumLitArg::Positional(e) => desugar_expr(e),
                    EnumLitArg::Named { expr, .. } => desugar_expr(expr),
                }
            }
        }
        Expr::MapLit(entries, _) => {
            for (k, v) in entries {
                desugar_expr(k);
                desugar_expr(v);
            }
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, value) in fields {
                desugar_expr(value);
            }
        }
        Expr::Lambda(lam) => match &mut lam.body {
            LambdaBody::Block(body) => desugar_stmts(body),
            LambdaBody::Expr(e) => desugar_expr(e),
        },
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            desugar_expr(cond);
            desugar_stmts(then_body);
            desugar_expr(then_value);
            desugar_stmts(else_body);
            desugar_expr(else_value);
        }
        Expr::OrFallback { value, fallback, .. } => {
            desugar_expr(value);
            match fallback {
                crate::AST::OrFallback::Value(e) | crate::AST::OrFallback::Return(Some(e), _) => {
                    desugar_expr(e);
                }
                crate::AST::OrFallback::Panic { args, .. } => {
                    for arg in args {
                        desugar_expr(&mut arg.expr);
                    }
                }
                _ => {}
            }
        }
        Expr::PatternTest { subject, .. } => desugar_expr(subject),
        Expr::Str(parts, _) => {
            for part in parts {
                if let StrPart::Interp(inner, _) = part {
                    desugar_expr(inner);
                }
            }
        }
        Expr::PtrFromAddr { addr, .. } => desugar_expr(addr),
        _ => {}
    }
}
