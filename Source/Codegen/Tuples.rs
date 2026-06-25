use super::*;
use crate::AST::{
    ElseBranch, EnumLitArg, Expr,
    ForKind, IfStmt, Item, LambdaBody, OrFallback, Stmt, StrPart, Type, VariantPayload,
};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
pub(crate) fn tuple_struct_name(fields: &[(String, Type)]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (name, ty) in fields {
        name.hash(&mut hasher);
        ty.name().hash(&mut hasher);
    }
    format!("JetTup_{:x}", hasher.finish())
}

pub(crate) fn tuple_fields_plain(fields: &[(String, Box<Type>)]) -> Vec<(String, Type)> {
    fields
        .iter()
        .map(|(n, t)| (n.clone(), (**t).clone()))
        .collect()
}

fn collect_tuple_shapes_from_type(ty: &Type, out: &mut BTreeMap<String, Vec<(String, Type)>>) {
    if let Type::Tuple(fields) = ty {
        let plain = tuple_fields_plain(fields);
        out.insert(tuple_struct_name(&plain), plain);
        for (_, t) in fields {
            collect_tuple_shapes_from_type(t, out);
        }
        return;
    }
    match ty {
        Type::List(inner) | Type::Option(inner) | Type::Shared(inner) => {
            collect_tuple_shapes_from_type(inner, out);
        }
        Type::Map { key, value } => {
            collect_tuple_shapes_from_type(key, out);
            collect_tuple_shapes_from_type(value, out);
        }
        Type::Result { ok, err } => {
            collect_tuple_shapes_from_type(ok, out);
            collect_tuple_shapes_from_type(err, out);
        }
        Type::Fn { params, ret } => {
            for p in params {
                collect_tuple_shapes_from_type(p, out);
            }
            if let Some(r) = ret {
                collect_tuple_shapes_from_type(r, out);
            }
        }
        Type::Apply { args, .. } => {
            for a in args {
                collect_tuple_shapes_from_type(a, out);
            }
        }
        _ => {}
    }
}

fn collect_tuple_shapes_from_expr(expr: &Expr, out: &mut BTreeMap<String, Vec<(String, Type)>>) {
    match expr {
        Expr::TupleLit(_, _, Some(ty)) => collect_tuple_shapes_from_type(ty, out),
        Expr::Str(parts, _) => {
            for p in parts {
                if let StrPart::Interp(e) = p {
                    collect_tuple_shapes_from_expr(e, out);
                }
            }
        }
        Expr::ListLit(elems, _) => {
            for e in elems {
                collect_tuple_shapes_from_expr(e, out);
            }
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, e) in fields {
                collect_tuple_shapes_from_expr(e, out);
            }
        }
        Expr::MapLit(entries, _) => {
            for (k, v) in entries {
                collect_tuple_shapes_from_expr(k, out);
                collect_tuple_shapes_from_expr(v, out);
            }
        }
        Expr::Index { base, index, .. } => {
            collect_tuple_shapes_from_expr(base, out);
            collect_tuple_shapes_from_expr(index, out);
        }
        Expr::Slice { base, start, end, .. } => {
            collect_tuple_shapes_from_expr(base, out);
            collect_tuple_shapes_from_expr(start, out);
            collect_tuple_shapes_from_expr(end, out);
        }
        Expr::Ident(_, _)
        | Expr::Int(_, _, _)
        | Expr::Float(_, _, _)
        | Expr::Bool(_, _)
        | Expr::Char(_, _)
        | Expr::Absent(_)
        | Expr::Todo { .. } => {}
        Expr::Call(c) => {
            for a in &c.args {
                collect_tuple_shapes_from_expr(&a.expr, out);
            }
        }
        Expr::Unary(_, inner, _)
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Tainted(inner, _) // D-TAINT1: tag erased; recurse into the value.
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _) => collect_tuple_shapes_from_expr(inner, out),
        Expr::Binary(_, l, r, _) => {
            collect_tuple_shapes_from_expr(l, out);
            collect_tuple_shapes_from_expr(r, out);
        }
        Expr::Field(inner, _, _) | Expr::OptField { base: inner, .. } => {
            collect_tuple_shapes_from_expr(inner, out);
        }
        Expr::MethodCall { receiver, args, resolved_ret, .. } => {
            collect_tuple_shapes_from_expr(receiver, out);
            for a in args {
                collect_tuple_shapes_from_expr(&a.expr, out);
            }
            // D-ITER1: enumerate/zip/partition return named-tuple types. Sema stores
            // the resolved return type in `resolved_ret`; collect any tuple shapes
            // it contains so the JetTup_ struct declarations are emitted.
            if let Some(ty) = resolved_ret {
                collect_tuple_shapes_from_type(ty, out);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, e) in fields {
                collect_tuple_shapes_from_expr(e, out);
            }
        }
        Expr::EnumLit { args, .. } => {
            for a in args {
                match a {
                    EnumLitArg::Positional(e) => collect_tuple_shapes_from_expr(e, out),
                    EnumLitArg::Named { expr, .. } => collect_tuple_shapes_from_expr(expr, out),
                }
            }
        }
        Expr::PatternTest { subject, .. } => collect_tuple_shapes_from_expr(subject, out),
        Expr::OrFallback { value, fallback, .. } => {
            collect_tuple_shapes_from_expr(value, out);
            match fallback {
                OrFallback::Value(v) => collect_tuple_shapes_from_expr(v, out),
                OrFallback::Return(Some(v), _) => collect_tuple_shapes_from_expr(v, out),
                OrFallback::Return(None, _) => {}
                OrFallback::Panic { args, .. } => {
                    for a in args {
                        collect_tuple_shapes_from_expr(&a.expr, out);
                    }
                }
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
            collect_tuple_shapes_from_expr(cond, out);
            for s in then_body {
                collect_tuple_shapes_from_stmt(s, out);
            }
            collect_tuple_shapes_from_expr(then_value, out);
            for s in else_body {
                collect_tuple_shapes_from_stmt(s, out);
            }
            collect_tuple_shapes_from_expr(else_value, out);
        }
        Expr::Lambda(l) => match &l.body {
            LambdaBody::Expr(e) => collect_tuple_shapes_from_expr(e, out),
            LambdaBody::Block(stmts) => {
                for s in stmts {
                    collect_tuple_shapes_from_stmt(s, out);
                }
            }
        },
        Expr::CallValue { callee, args, .. } => {
            collect_tuple_shapes_from_expr(callee, out);
            for a in args {
                collect_tuple_shapes_from_expr(&a.expr, out);
            }
        }
        Expr::FanOut { callee, items, .. } => {
            collect_tuple_shapes_from_expr(callee, out);
            for item in items {
                collect_tuple_shapes_from_expr(item, out);
            }
        }
        Expr::PtrFromAddr { addr, .. } => collect_tuple_shapes_from_expr(addr, out),
    }
}

fn collect_tuple_shapes_from_stmt(stmt: &Stmt, out: &mut BTreeMap<String, Vec<(String, Type)>>) {
    match stmt {
        Stmt::Expr(e) => collect_tuple_shapes_from_expr(e, out),
        Stmt::Val(b) => {
            if let Some(ty) = &b.ty {
                collect_tuple_shapes_from_type(ty, out);
            }
            collect_tuple_shapes_from_expr(&b.init, out);
        }
        Stmt::Assign { value, .. } => collect_tuple_shapes_from_expr(value, out),
        Stmt::Return(Some(e), _) => collect_tuple_shapes_from_expr(e, out),
        Stmt::Return(None, _) => {}
        Stmt::If(i) => collect_tuple_shapes_from_if(i, out),
        Stmt::While { cond, body, .. } => {
            collect_tuple_shapes_from_expr(cond, out);
            for s in body {
                collect_tuple_shapes_from_stmt(s, out);
            }
        }
        Stmt::For { kind, body, .. } => {
            match kind {
                ForKind::Range { start, end, step } => {
                    collect_tuple_shapes_from_expr(start, out);
                    collect_tuple_shapes_from_expr(end, out);
                    if let Some(s) = step {
                        collect_tuple_shapes_from_expr(s, out);
                    }
                }
                ForKind::In { collection } => collect_tuple_shapes_from_expr(collection, out),
            }
            for s in body {
                collect_tuple_shapes_from_stmt(s, out);
            }
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => {
            collect_tuple_shapes_from_expr(subject, out);
            for a in arms {
                collect_tuple_shapes_from_expr(&a.cond, out);
                for s in &a.body {
                    collect_tuple_shapes_from_stmt(s, out);
                }
            }
            if let Some(body) = else_body {
                for s in body {
                    collect_tuple_shapes_from_stmt(s, out);
                }
            }
        }
        // D-REGION1: a region body is real code — collect tuple shapes from it.
        // D-EFF1: a `#Caps` region body is likewise real code.
        Stmt::Region { body, .. } | Stmt::Caps { body, .. } | Stmt::Grant { body, .. } | Stmt::Transact { body, .. } | Stmt::AssumeDet { body, .. } => {
            for s in body {
                collect_tuple_shapes_from_stmt(s, out);
            }
        }
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) | Stmt::Loop { .. } | Stmt::Unsafe { .. } => {}
        // D-CTX1: collect tuple shapes from context block fields and body.
        Stmt::ContextBlock { fields, body, .. } => {
            for (_, e, _) in fields {
                collect_tuple_shapes_from_expr(e, out);
            }
            for s in body {
                collect_tuple_shapes_from_stmt(s, out);
            }
        }
        // D-TERM1 (ratified 2026-06-22): collect tuple shapes from live block body.
        Stmt::Live { body, .. } => {
            for s in body {
                collect_tuple_shapes_from_stmt(s, out);
            }
        }
        // D-WHEN1: collect tuple shapes from both arms (conservative).
        Stmt::ComptimeIf { cond, then_body, else_body, .. } => {
            collect_tuple_shapes_from_expr(cond, out);
            for s in then_body {
                collect_tuple_shapes_from_stmt(s, out);
            }
            if let Some(eb) = else_body {
                for s in eb {
                    collect_tuple_shapes_from_stmt(s, out);
                }
            }
        }
    }
}

fn collect_tuple_shapes_from_if(i: &IfStmt, out: &mut BTreeMap<String, Vec<(String, Type)>>) {
    collect_tuple_shapes_from_expr(&i.cond, out);
    for s in &i.then_body {
        collect_tuple_shapes_from_stmt(s, out);
    }
    match &i.else_branch {
        Some(ElseBranch::ElseIf(nested)) => collect_tuple_shapes_from_if(nested, out),
        Some(ElseBranch::Else(stmts)) => {
            for s in stmts {
                collect_tuple_shapes_from_stmt(s, out);
            }
        }
        None => {}
    }
}

pub(crate) fn collect_tuple_shapes(items: &[Item]) -> BTreeMap<String, Vec<(String, Type)>> {
    let mut out = BTreeMap::new();
    for item in items {
        match item {
            Item::Func(f) => {
                for p in &f.params {
                    collect_tuple_shapes_from_type(&p.ty, &mut out);
                }
                if let Some(ret) = &f.return_type {
                    collect_tuple_shapes_from_type(ret, &mut out);
                }
                for s in &f.body {
                    collect_tuple_shapes_from_stmt(s, &mut out);
                }
            }
            Item::Struct(s) => {
                for field in &s.fields {
                    collect_tuple_shapes_from_type(&field.ty, &mut out);
                }
                for m in &s.methods {
                    for p in &m.params {
                        collect_tuple_shapes_from_type(&p.ty, &mut out);
                    }
                    if let Some(ret) = &m.return_type {
                        collect_tuple_shapes_from_type(ret, &mut out);
                    }
                    for s in &m.body {
                        collect_tuple_shapes_from_stmt(s, &mut out);
                    }
                }
            }
            Item::Enum(e) => {
                for v in &e.variants {
                    match &v.payload {
                        VariantPayload::Unit => {}
                        VariantPayload::Single(t, _) => collect_tuple_shapes_from_type(t, &mut out),
                        VariantPayload::Named(fs) => {
                            for f in fs {
                                collect_tuple_shapes_from_type(&f.ty, &mut out);
                            }
                        }
                    }
                }
            }
            Item::Const(c) => {
                collect_tuple_shapes_from_expr(&c.value, &mut out);
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    for p in &m.params {
                        collect_tuple_shapes_from_type(&p.ty, &mut out);
                    }
                    if let Some(ret) = &m.return_type {
                        collect_tuple_shapes_from_type(ret, &mut out);
                    }
                    for s in &m.body {
                        collect_tuple_shapes_from_stmt(s, &mut out);
                    }
                }
            }
            Item::Test(t) => {
                for s in &t.body {
                    collect_tuple_shapes_from_stmt(s, &mut out);
                }
            }
            Item::Bench(b) => {
                for s in &b.body {
                    collect_tuple_shapes_from_stmt(s, &mut out);
                }
            }
            Item::Trait(_) | Item::ExternRust(_) | Item::Module(_) | Item::CModule(_)
            | Item::CodeModule(_) | Item::Distinct(_) | Item::ErrorConv(_)
            | Item::Tag(_) // D-QUAL2: tags erase
            | Item::Migration(_) => {} // D-MIGRATE1
        }
    }
    out
}

fn emit_tuple_struct(cx: &Cx, name: &str, fields: &[(String, Type)], out: &mut String) {
    let mut derives = vec!["Clone"];
    if fields
        .iter()
        .all(|(_, t)| field_type_comparable(t, &cx.type_names))
    {
        derives.push("PartialEq");
    }
    out.push_str(&format!("#[derive({})]\nstruct {} {{\n", derives.join(", "), name));
    for (fname, fty) in fields {
        out.push_str(&format!(
            "    pub {}: {},\n",
            mangle(fname),
            cx.rust_type(fty)
        ));
    }
    out.push_str("}\n\n");
}

pub(crate) fn emit_tuple_structs(cx: &Cx, shapes: &BTreeMap<String, Vec<(String, Type)>>, out: &mut String) {
    for (name, fields) in shapes {
        emit_tuple_struct(cx, name, fields, out);
    }
}
