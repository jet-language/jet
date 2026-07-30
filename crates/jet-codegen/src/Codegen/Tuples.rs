use super::*;
use crate::AST::{
    ElseBranch, EnumLitArg, Expr, ForKind, Func, IfStmt, Item, LambdaBody, OrFallback, Stmt,
    StrPart, Type, VariantPayload,
};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

pub(crate) struct CollectedTypeShapes {
    pub(crate) tuples: BTreeMap<String, Vec<(String, Type)>>,
    abstract_params: Vec<String>,
}

fn with_type_params(
    out: &mut CollectedTypeShapes,
    params: Vec<String>,
    collect: impl FnOnce(&mut CollectedTypeShapes),
) {
    let previous = std::mem::replace(&mut out.abstract_params, params);
    collect(out);
    out.abstract_params = previous;
}

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

fn collect_tuple_shapes_from_type(ty: &Type, out: &mut CollectedTypeShapes) {
    if let Type::Tuple(fields) = ty {
        let plain = tuple_fields_plain(fields);
        out.tuples.insert(tuple_struct_name(&plain), plain);
        for (_, t) in fields {
            collect_tuple_shapes_from_type(t, out);
        }
        return;
    }
    match ty {
        Type::List(inner) | Type::Option(inner) | Type::Shared(inner) => {
            collect_tuple_shapes_from_type(inner, out);
        }
        Type::Map { key, value, .. } => {
            let fields = vec![
                ("key".to_string(), (**key).clone()),
                ("value".to_string(), (**value).clone()),
            ];
            out.tuples.insert(tuple_struct_name(&fields), fields);
            collect_tuple_shapes_from_type(key, out);
            collect_tuple_shapes_from_type(value, out);
        }
        Type::Result { ok, err } => {
            collect_tuple_shapes_from_type(ok, out);
            collect_tuple_shapes_from_type(err, out);
        }
        Type::Fn { params, ret, .. } => {
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
        Type::Union(members) => {
            for m in members {
                collect_tuple_shapes_from_type(m, out);
            }
        }
        _ => {}
    }
}

fn collect_tuple_shapes_from_expr(expr: &Expr, out: &mut CollectedTypeShapes) {
    match expr {
        Expr::TupleLit(_, _, Some(ty)) => collect_tuple_shapes_from_type(ty, out),
        Expr::Str(parts, _) => {
            for p in parts {
                if let StrPart::Interp(e, _) = p {
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
        Expr::Slice { base, start, end, range, .. } => {
            collect_tuple_shapes_from_expr(base, out);
            if let Some(range) = range {
                collect_tuple_shapes_from_expr(range, out);
            } else {
                collect_tuple_shapes_from_expr(start, out);
                collect_tuple_shapes_from_expr(end, out);
            }
        }
        Expr::Range { start, end, .. } => {
            collect_tuple_shapes_from_expr(start, out);
            collect_tuple_shapes_from_expr(end, out);
        }
        Expr::Ident(_, _)
        | Expr::Int(_, _, _, _)
        | Expr::Float(_, _, _)
        | Expr::Bool(_, _)
        | Expr::Char(_, _)
        | Expr::Absent(_)
        | Expr::ReduceMarker(_, _)
        | Expr::Todo { .. }
        | Expr::UnitLit { .. }
        | Expr::ComptimeSplice { .. }
        // D-SHIFT1 (c7shift) / D-BINPAT1 (card #506 follow-up): a leaf
        // literal, no nested tuple shapes.
        | Expr::StrMatchLit(_, _)
        | Expr::BinMatchLit(_, _) => {}
        Expr::Call(c) => {
            for a in &c.args {
                collect_tuple_shapes_from_expr(&a.expr, out);
            }
        }
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Tainted(inner, _, _) // D-TAINT1: tag erased; recurse into the value.
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _) => collect_tuple_shapes_from_expr(inner, out),
        Expr::Binary(_, l, r, _) => {
            collect_tuple_shapes_from_expr(l, out);
            collect_tuple_shapes_from_expr(r, out);
        }
        Expr::CompareChain { operands, .. } => {
            for e in operands {
                collect_tuple_shapes_from_expr(e, out);
            }
        }
        Expr::Field(inner, _, _) | Expr::OptField { base: inner, .. } => {
            collect_tuple_shapes_from_expr(inner, out);
        }
        Expr::MethodCall {
            receiver,
            type_args,
            args,
            resolved_ret,
            ..
        } => {
            collect_tuple_shapes_from_expr(receiver, out);
            if let Expr::Ident(type_name, _) = receiver.as_ref() {
                if !type_args.is_empty()
                    && type_name.chars().next().is_some_and(char::is_uppercase)
                {
                    collect_tuple_shapes_from_type(
                        &Type::Apply {
                            name: type_name.clone(),
                            args: type_args.clone(),
                        },
                        out,
                    );
                }
            }
            for ty in type_args {
                collect_tuple_shapes_from_type(ty, out);
            }
            for a in args {
                collect_tuple_shapes_from_expr(&a.expr, out);
            }
            // D-ITER1: indexed/zip/partition return named-tuple types. Sema stores
            // the resolved return type in `resolved_ret`; collect any tuple shapes
            // it contains so the JetTup_ struct declarations are emitted.
            if let Some(ty) = resolved_ret {
                collect_tuple_shapes_from_type(ty, out);
            }
        }
        Expr::StructLit {
            type_name,
            type_args,
            fields,
            ..
        } => {
            if !type_args.is_empty() {
                collect_tuple_shapes_from_type(
                    &Type::Apply {
                        name: type_name.clone(),
                        args: type_args.clone(),
                    },
                    out,
                );
            }
            for (_, _, e) in fields {
                collect_tuple_shapes_from_expr(e, out);
            }
        }
        Expr::TypedLit { head, body, .. } => {
            if let Some(h) = head {
                collect_tuple_shapes_from_type(h, out);
            }
            body.for_each_expr(|e| collect_tuple_shapes_from_expr(e, out));
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
                OrFallback::Break(_)
                | OrFallback::Continue(_)
                | OrFallback::BreakLabel(..)
                | OrFallback::ContinueLabel(..) => {}
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
        Expr::Paren(inner, _) => collect_tuple_shapes_from_expr(inner, out),
        Expr::Spread(inner, _) => collect_tuple_shapes_from_expr(inner, out),
    }
}

fn collect_tuple_shapes_from_stmt(stmt: &Stmt, out: &mut CollectedTypeShapes) {
    match stmt {
        Stmt::Expr(e) | Stmt::Yield(e, _) => collect_tuple_shapes_from_expr(e, out),
        Stmt::Val(b) => {
            if let Some(ty) = &b.ty {
                collect_tuple_shapes_from_type(ty, out);
            }
            collect_tuple_shapes_from_expr(&b.init, out);
        }
        Stmt::Assign { value, .. } => collect_tuple_shapes_from_expr(value, out),
        Stmt::Return(Some(e), _) => collect_tuple_shapes_from_expr(e, out),
        Stmt::BreakValue(e, _) | Stmt::BreakLabelValue(_, _, e, _) => {
            collect_tuple_shapes_from_expr(e, out)
        }
        Stmt::Return(None, _) => {}
        Stmt::If(i) => collect_tuple_shapes_from_if(i, out),
        Stmt::While { cond, body, .. } | Stmt::CountedLoop { cond, body, .. } => {
            collect_tuple_shapes_from_expr(cond, out);
            for s in body {
                collect_tuple_shapes_from_stmt(s, out);
            }
        }
        Stmt::For { kind, body, .. } => {
            match kind {
                ForKind::Range { start, end, step, exclusive: _ } => {
                    collect_tuple_shapes_from_expr(start, out);
                    collect_tuple_shapes_from_expr(end, out);
                    if let Some(s) = step {
                        collect_tuple_shapes_from_expr(s, out);
                    }
                }
                ForKind::In { collection, step } => {
                    collect_tuple_shapes_from_expr(collection, out);
                    if let Some(step) = step { collect_tuple_shapes_from_expr(step, out); }
                }
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
        }
        | Stmt::ComptimeSwitch {
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
        Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Off { body, .. }
        | Stmt::DebugOnly { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::AssumeDet { body, .. } => {
            for s in body {
                collect_tuple_shapes_from_stmt(s, out);
            }
        }
        Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::BreakLabel(..)
        | Stmt::ContinueLabel(..)
        | Stmt::Loop { .. }
        | Stmt::Unsafe { .. }
        | Stmt::Impure { .. }
        | Stmt::Reactive { .. } => {}
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
        // D-DOTSCOPE1: collect tuple shapes from a scope-member region body.
        Stmt::ScopeMember { body, .. } => {
            for s in body {
                collect_tuple_shapes_from_stmt(s, out);
            }
        }
        // D-CTMARKER1: comptime block erases; no tuple shapes in emitted Rust.
        Stmt::ComptimeBlock { .. } => {}
        // D-WHEN1: collect tuple shapes from both arms (conservative).
        Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            ..
        } => {
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

fn collect_tuple_shapes_from_if(i: &IfStmt, out: &mut CollectedTypeShapes) {
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

fn collect_func_shapes(f: &Func, inherited: &[String], out: &mut CollectedTypeShapes) {
    let mut params = inherited.to_vec();
    params.extend(f.type_params.iter().map(|param| param.name.clone()));
    with_type_params(out, params, |out| {
        for param in &f.params {
            collect_tuple_shapes_from_type(&param.ty, out);
        }
        if let Some(ret) = &f.return_type {
            collect_tuple_shapes_from_type(ret, out);
        }
        for stmt in &f.body {
            collect_tuple_shapes_from_stmt(stmt, out);
        }
    });
}

pub(crate) fn collect_type_shapes(items: &[Item]) -> CollectedTypeShapes {
    let owner_params: BTreeMap<String, Vec<String>> = items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(def) => Some((&def.name, &def.type_params)),
            Item::Enum(def) => Some((&def.name, &def.type_params)),
            _ => None,
        })
        .map(|(name, params)| {
            (
                name.clone(),
                params.iter().map(|param| param.name.clone()).collect(),
            )
        })
        .collect();
    let mut out = CollectedTypeShapes {
        tuples: BTreeMap::new(),
        abstract_params: Vec::new(),
    };
    for item in items {
        match item {
            Item::Func(f) => collect_func_shapes(f, &[], &mut out),
            Item::Struct(s) => {
                let params = owner_params.get(&s.name).cloned().unwrap_or_default();
                with_type_params(&mut out, params.clone(), |out| {
                    for field in &s.fields {
                        collect_tuple_shapes_from_type(&field.ty, out);
                    }
                });
                for m in &s.methods {
                    collect_func_shapes(m, &params, &mut out);
                }
            }
            Item::Enum(e) => {
                let params = owner_params.get(&e.name).cloned().unwrap_or_default();
                with_type_params(&mut out, params, |out| {
                    for v in &e.variants {
                        match &v.payload {
                            VariantPayload::Unit => {}
                            VariantPayload::Single(t, _) => collect_tuple_shapes_from_type(t, out),
                            VariantPayload::Named(fs) => {
                                for f in fs {
                                    collect_tuple_shapes_from_type(&f.ty, out);
                                }
                            }
                        }
                    }
                });
            }
            Item::Const(c) => {
                collect_tuple_shapes_from_expr(&c.value, &mut out);
            }
            Item::Impl(i) => {
                let params = owner_params.get(&i.type_name).cloned().unwrap_or_default();
                for m in &i.methods {
                    collect_func_shapes(m, &params, &mut out);
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
            Item::EffectDecl(_)
            | Item::Trait(_) | Item::ExternRust(_) | Item::Module(_) | Item::CModule(_)
            | Item::CodeModule(_) | Item::Distinct(_) | Item::TypeAlias(_) | Item::UnitFamily(_) | Item::ErrorConv(_)
            | Item::Tag(_) // D-QUAL2: tags erase
            | Item::Migration(_) // D-MIGRATE1
            | Item::StateDecl(_) // D-STATE-DECL: erases
            | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2: erases
            | Item::UserDerive(_) // D-METADERIVE1=A: erase (expanded in sema)
            | Item::GenericModule(_) // D-GENMOD2=A: template — erases
            | Item::ModuleAlias(_) => {} // D-GENMOD2=A: alias — erases after expansion
        }
    }
    out
}

pub(crate) fn collect_tuple_shapes(items: &[Item]) -> BTreeMap<String, Vec<(String, Type)>> {
    collect_type_shapes(items).tuples
}

/// A linear Cell guard field must move out of a one-shot tuple, so the tuple
/// itself cannot derive `Clone` (D-LOCALCELL1=A).
fn is_move_only_cell_guard(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Apply { name, .. }
            if matches!(name.as_str(), "CellReadGuard" | "CellEditGuard")
    )
}

fn emit_tuple_struct(cx: &Cx, name: &str, fields: &[(String, Type)], out: &mut String) {
    // Tuples are structural types with no type-parameter scope of their own.
    let no_params: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut derives = Vec::new();
    if fields
        .iter()
        .all(|(_, t)| {
            !cx.type_contains_shared_guard(t)
                && !is_move_only_cell_guard(t)
                && field_type_cloneable(t, &cx.type_names, &no_params)
        })
    {
        derives.push("Clone");
    }
    if fields
        .iter()
        .all(|(_, t)| {
            !cx.type_contains_shared_guard(t)
                && !is_move_only_cell_guard(t)
                && field_type_comparable(t, &cx.type_names, &no_params)
        })
    {
        derives.push("PartialEq");
    }
    let view_lifetime = fields
        .iter()
        .any(|(_, ty)| cx.type_contains_view(ty))
        .then_some("<'__jet_view>")
        .unwrap_or("");
    if !derives.is_empty() {
        out.push_str(&format!("#[derive({})]\n", derives.join(", ")));
    }
    out.push_str(&format!("struct {}{} {{\n", name, view_lifetime));
    for (fname, fty) in fields {
        out.push_str(&format!(
            "    pub {}: {},\n",
            mangle(fname),
            if cx.type_contains_view(fty) {
                cx.rust_type_with_view_lifetime(fty)
            } else {
                cx.rust_type(fty)
            }
        ));
    }
    out.push_str("}\n\n");
}

pub(crate) fn emit_tuple_structs(
    cx: &Cx,
    shapes: &BTreeMap<String, Vec<(String, Type)>>,
    out: &mut String,
) {
    for (name, fields) in shapes {
        emit_tuple_struct(cx, name, fields, out);
    }
}
