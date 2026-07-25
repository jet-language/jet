use super::*;
/// such call names so registration, body-checking, and codegen all agree.
/// Only callee names are rewritten (the unambiguous case); a sibling referenced
/// as a value resolves through normal name lookup and yields a clean Jet error
/// rather than leaking to rustc.
pub(crate) fn mangle_inline_sibling_calls(bundle: &mut ProgramBundle) {
    for module in bundle.modules.iter_mut() {
        for item in module.items.iter_mut() {
            let Item::CodeModule(cm) = item else { continue };
            let Some(body) = &mut cm.body else { continue };
            let siblings: HashSet<String> = body
                .iter()
                .filter_map(|i| match i {
                    Item::Func(f) => Some(f.name.clone()),
                    _ => None,
                })
                .collect();
            if siblings.is_empty() {
                continue;
            }
            for inner in body.iter_mut() {
                if let Item::Func(f) = inner {
                    rewrite_inline_calls_stmts(&mut f.body, &siblings, &cm.name);
                }
            }
        }
    }
}

pub(crate) fn rewrite_inline_calls_stmts(
    stmts: &mut [Stmt],
    siblings: &HashSet<String>,
    modname: &str,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(e) | Stmt::Yield(e, _) => rewrite_inline_calls_expr(e, siblings, modname),
            Stmt::Val(b) => rewrite_inline_calls_expr(&mut b.init, siblings, modname),
            Stmt::Assign { value, .. } => rewrite_inline_calls_expr(value, siblings, modname),
            Stmt::Return(Some(e), _) => rewrite_inline_calls_expr(e, siblings, modname),
            Stmt::Return(None, _)
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::BreakLabel(..)
            | Stmt::ContinueLabel(..) => {}
            Stmt::If(ifs) => rewrite_inline_calls_if(ifs, siblings, modname),
            Stmt::While { cond, body, .. } => {
                rewrite_inline_calls_expr(cond, siblings, modname);
                rewrite_inline_calls_stmts(body, siblings, modname);
            }
            Stmt::For { kind, body, .. } => {
                match kind {
                    ForKind::Range { start, end, step } => {
                        rewrite_inline_calls_expr(start, siblings, modname);
                        rewrite_inline_calls_expr(end, siblings, modname);
                        if let Some(step) = step {
                            rewrite_inline_calls_expr(step, siblings, modname);
                        }
                    }
                    ForKind::In { collection, step } => {
                        rewrite_inline_calls_expr(collection, siblings, modname);
                        if let Some(step) = step {
                            rewrite_inline_calls_expr(step, siblings, modname);
                        }
                    }
                }
                rewrite_inline_calls_stmts(body, siblings, modname);
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
                rewrite_inline_calls_expr(subject, siblings, modname);
                for a in arms.iter_mut() {
                    rewrite_inline_calls_expr(&mut a.cond, siblings, modname);
                    rewrite_inline_calls_stmts(&mut a.body, siblings, modname);
                }
                if let Some(eb) = else_body {
                    rewrite_inline_calls_stmts(eb, siblings, modname);
                }
            }
            Stmt::CountedLoop {
                init,
                cond,
                body: inner,
                ..
            } => {
                rewrite_inline_calls_expr(&mut init.init, siblings, modname);
                rewrite_inline_calls_expr(cond, siblings, modname);
                rewrite_inline_calls_stmts(inner, siblings, modname);
            }
            Stmt::Loop { body: inner, .. }
            | Stmt::Unsafe { body: inner, .. }
            | Stmt::Impure { body: inner, .. }
            | Stmt::Reactive { body: inner, .. }
            | Stmt::Shield { body: inner, .. }
            | Stmt::Off { body: inner, .. }
            | Stmt::DebugOnly { body: inner, .. }
            | Stmt::Region { body: inner, .. }
            | Stmt::Policy { body: inner, .. }
            | Stmt::TaskGroup { body: inner, .. }
            | Stmt::Layout { body: inner, .. }
            | Stmt::Caps { body: inner, .. }
            | Stmt::Grant { body: inner, .. }
            | Stmt::Transact { body: inner, .. }
            | Stmt::AssumeDet { body: inner, .. } => {
                rewrite_inline_calls_stmts(inner, siblings, modname);
            }
            // D-CTMARKER1: rewrite inline calls in comptime block body.
            Stmt::ComptimeBlock { body, .. } => rewrite_inline_calls_stmts(body, siblings, modname),
            // D-WHEN1: rewrite calls in both arms so sibling resolution works
            // regardless of which arm is selected at comptime.
            Stmt::ComptimeIf {
                cond,
                then_body,
                else_body,
                ..
            } => {
                rewrite_inline_calls_expr(cond, siblings, modname);
                rewrite_inline_calls_stmts(then_body, siblings, modname);
                if let Some(eb) = else_body {
                    rewrite_inline_calls_stmts(eb, siblings, modname);
                }
            }
            // D-CTX1: rewrite inline calls in field values and body.
            Stmt::ContextBlock { fields, body, .. } => {
                for (_, e, _) in fields.iter_mut() {
                    rewrite_inline_calls_expr(e, siblings, modname);
                }
                rewrite_inline_calls_stmts(body, siblings, modname);
            }
            // D-TERM1 (ratified 2026-06-22): rewrite inline calls in live block body.
            Stmt::Live { body, .. } => {
                rewrite_inline_calls_stmts(body, siblings, modname);
            }
            // D-DOTSCOPE1: rewrite inline calls in a scope-member region body.
            Stmt::ScopeMember { body, .. } => {
                rewrite_inline_calls_stmts(body, siblings, modname);
            }
        }
    }
}

pub(crate) fn rewrite_inline_calls_if(ifs: &mut IfStmt, siblings: &HashSet<String>, modname: &str) {
    rewrite_inline_calls_expr(&mut ifs.cond, siblings, modname);
    rewrite_inline_calls_stmts(&mut ifs.then_body, siblings, modname);
    match &mut ifs.else_branch {
        Some(ElseBranch::Else(b)) => rewrite_inline_calls_stmts(b, siblings, modname),
        Some(ElseBranch::ElseIf(next)) => rewrite_inline_calls_if(next, siblings, modname),
        None => {}
    }
}

pub(crate) fn rewrite_inline_calls_expr(
    expr: &mut Expr,
    siblings: &HashSet<String>,
    modname: &str,
) {
    match expr {
        Expr::Call(c) => {
            if siblings.contains(&c.name) {
                c.name = format!("{}__{}", modname, c.name);
            }
            for a in c.args.iter_mut() {
                rewrite_inline_calls_expr(&mut a.expr, siblings, modname);
            }
        }
        Expr::PtrFromAddr { addr, .. } => rewrite_inline_calls_expr(addr, siblings, modname),
        Expr::Ident(name, _) => {
            if siblings.contains(name) {
                *name = modname.to_string();
            }
        }
        Expr::Char(_, _)
        | Expr::Int(_, _, _, _)
        | Expr::Float(_, _, _)
        | Expr::Bool(_, _)
        | Expr::Absent(_)
        | Expr::ReduceMarker(_, _)
        | Expr::Todo { .. }
        | Expr::UnitLit { .. }
        | Expr::ComptimeSplice { .. }
        // D-SHIFT1 (c7shift) / D-BINPAT1 (card #506 follow-up): a leaf
        // literal, no nested `Expr` to recurse into.
        | Expr::StrMatchLit(_, _)
        | Expr::BinMatchLit(_, _) => {}
        Expr::Str(parts, _) => {
            for p in parts.iter_mut() {
                if let StrPart::Interp(e, _) = p {
                    rewrite_inline_calls_expr(e, siblings, modname);
                }
            }
        }
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Field(inner, _, _)
        | Expr::Tainted(inner, _, _) // D-TAINT1: tag erased; recurse into the value.
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _) => rewrite_inline_calls_expr(inner, siblings, modname),
        Expr::OptField { base, .. } => rewrite_inline_calls_expr(base, siblings, modname),
        Expr::MethodCall { receiver, args, .. } => {
            rewrite_inline_calls_expr(receiver, siblings, modname);
            for a in args.iter_mut() {
                rewrite_inline_calls_expr(&mut a.expr, siblings, modname);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, e) in fields.iter_mut() {
                rewrite_inline_calls_expr(e, siblings, modname);
            }
        }
        Expr::TypedLit { body, .. } => {
            body.for_each_expr_mut(|e| rewrite_inline_calls_expr(e, siblings, modname));
        }
        Expr::EnumLit { args, .. } => {
            for a in args.iter_mut() {
                match a {
                    EnumLitArg::Positional(e) | EnumLitArg::Named { expr: e, .. } => {
                        rewrite_inline_calls_expr(e, siblings, modname);
                    }
                }
            }
        }
        Expr::OrFallback { value, fallback, .. } => {
            rewrite_inline_calls_expr(value, siblings, modname);
            match fallback {
                OrFallback::Value(e) => rewrite_inline_calls_expr(e, siblings, modname),
                OrFallback::Return(Some(e), _) => rewrite_inline_calls_expr(e, siblings, modname),
                OrFallback::Return(None, _)
                | OrFallback::Panic { .. }
                | OrFallback::Break(_)
                | OrFallback::Continue(_)
                | OrFallback::BreakLabel(..)
                | OrFallback::ContinueLabel(..) => {}
            }
        }
        Expr::PatternTest { subject, .. } => {
            rewrite_inline_calls_expr(subject, siblings, modname)
        }
        Expr::Binary(_, l, r, _) => {
            rewrite_inline_calls_expr(l, siblings, modname);
            rewrite_inline_calls_expr(r, siblings, modname);
        }
        Expr::CompareChain { operands, .. } => {
            for e in operands.iter_mut() {
                rewrite_inline_calls_expr(e, siblings, modname);
            }
        }
        Expr::ListLit(elems, _) => {
            for e in elems.iter_mut() {
                rewrite_inline_calls_expr(e, siblings, modname);
            }
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, e) in fields.iter_mut() {
                rewrite_inline_calls_expr(e, siblings, modname);
            }
        }
        Expr::MapLit(entries, _) => {
            for (k, v) in entries.iter_mut() {
                rewrite_inline_calls_expr(k, siblings, modname);
                rewrite_inline_calls_expr(v, siblings, modname);
            }
        }
        Expr::Index { base, index, .. } => {
            rewrite_inline_calls_expr(base, siblings, modname);
            rewrite_inline_calls_expr(index, siblings, modname);
        }
        Expr::Slice { base, start, end, .. } => {
            rewrite_inline_calls_expr(base, siblings, modname);
            rewrite_inline_calls_expr(start, siblings, modname);
            rewrite_inline_calls_expr(end, siblings, modname);
        }
        Expr::CallValue { callee, args, .. } => {
            rewrite_inline_calls_expr(callee, siblings, modname);
            for a in args.iter_mut() {
                rewrite_inline_calls_expr(&mut a.expr, siblings, modname);
            }
        }
        Expr::Lambda(lam) => match &mut lam.body {
            LambdaBody::Expr(e) => rewrite_inline_calls_expr(e, siblings, modname),
            LambdaBody::Block(stmts) => rewrite_inline_calls_stmts(stmts, siblings, modname),
        },
        Expr::If { cond, then_body, then_value, else_body, else_value, .. } => {
            rewrite_inline_calls_expr(cond, siblings, modname);
            rewrite_inline_calls_stmts(then_body, siblings, modname);
            rewrite_inline_calls_expr(then_value, siblings, modname);
            rewrite_inline_calls_stmts(else_body, siblings, modname);
            rewrite_inline_calls_expr(else_value, siblings, modname);
        }
        Expr::FanOut { callee, items, .. } => {
            rewrite_inline_calls_expr(callee, siblings, modname);
            for item in items.iter_mut() {
                rewrite_inline_calls_expr(item, siblings, modname);
            }
        }
        Expr::Paren(inner, _) => rewrite_inline_calls_expr(inner, siblings, modname),
        Expr::Spread(inner, _) => rewrite_inline_calls_expr(inner, siblings, modname),
    }
}
