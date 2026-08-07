use crate::AST::{
    EnumLitArg, Expr, ForKind, Func, GenericModuleParam, ImportKind, Item,
    LambdaBody, LValue, OrFallback, Pattern, ProgramBundle, Stmt, StrPart, StructPatField,
    TraitMethodSig, Type, VariantPayload,
};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax::{self, NameCase};
use std::collections::HashSet;

pub(super) fn validate_bundle(bundle: &ProgramBundle) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut traits = HashSet::new();
    for module in &bundle.modules {
        for item in &module.items { trait_names(item, &mut traits); }
    }
    for module in &bundle.modules {
        for import in &module.imports {
            if !import.alias.is_empty() { snake(&import.alias, import.alias_span, "module alias", &mut out); }
            match &import.kind {
                ImportKind::Module(name, span) => snake(name, *span, "module", &mut out),
                ImportKind::Unqualified { module_alias, module_alias_span, .. } =>
                    snake(module_alias, *module_alias_span, "module alias", &mut out),
                ImportKind::File(..) => {}
            }
        }
        for item in &module.items { item_names(item, &traits, &mut out); }
    }
    out
}

fn trait_names(item: &Item, out: &mut HashSet<String>) {
    match item {
        Item::Trait(t) => { out.insert(t.name.clone()); }
        Item::CodeModule(m) => if let Some(body) = &m.body { for item in body { trait_names(item, out); } },
        Item::GenericModule(m) => for item in &m.body { trait_names(item, out); },
        _ => {}
    }
}

fn check(name: &str, span: Span, category: &str, out: &mut Vec<Diagnostic>) {
    // Compiler-reserved names have their own D-SHAPE-DUNDER2 diagnostic.
    let case = Syntax::name_case_for_category(category)
        .unwrap_or_else(|| jet_foundation::ice!(
            Some(span),
            "identifier category `{category}` is missing from Syntax::NAME_CASE_CATEGORIES"
        ));
    if name.is_empty() || name.starts_with("__") || Syntax::name_has_case(name, case) { return; }
    let expected = match case { NameCase::Pascal => "PascalCase", NameCase::Snake => "snake_case" };
    let fixed = Syntax::canonical_name_case(name, case);
    out.push(Diagnostic::error(
        "E0357",
        format!("{category} `{name}` must be {expected}"),
        "Jet uses one machine-enforced type-like/value-like casing law (D-SHAPE-CASE1)".to_string(),
        format!("rename it to `{fixed}`"),
        Some(span),
    ));
}

fn pascal(name: &str, span: Span, category: &str, out: &mut Vec<Diagnostic>) {
    debug_assert_eq!(Syntax::name_case_for_category(category), Some(NameCase::Pascal));
    for segment in name.split('.') { check(segment, span, category, out); }
}

fn snake(name: &str, span: Span, category: &str, out: &mut Vec<Diagnostic>) {
    debug_assert_eq!(Syntax::name_case_for_category(category), Some(NameCase::Snake));
    for segment in name.split(['.', '-']) { check(segment, span, category, out); }
}

fn func_names(f: &Func, category: &str, out: &mut Vec<Diagnostic>) {
    snake(&f.name, f.name_span, category, out);
    for p in &f.type_params { pascal(&p.name, p.name_span, "type parameter", out); }
    for p in &f.params {
        if p.name != Syntax::KW_SELF { snake(&p.name, p.name_span, "parameter", out); }
        if let Some(default) = &p.default { expr_names(default, out); }
    }
    stmt_names(&f.body, out);
}

fn trait_method_names(m: &TraitMethodSig, out: &mut Vec<Diagnostic>) {
    snake(&m.name, m.name_span, "method", out);
    for p in &m.params {
        if p.name != Syntax::KW_SELF { snake(&p.name, p.name_span, "parameter", out); }
        if let Some(default) = &p.default { expr_names(default, out); }
    }
    if let Some(body) = &m.default_body { stmt_names(body, out); }
}

fn item_names(item: &Item, traits: &HashSet<String>, out: &mut Vec<Diagnostic>) {
    match item {
        Item::EffectDecl(_) | Item::MarkerDecl(_) => {}
        Item::Func(f) => func_names(f, "function", out),
        Item::Struct(s) => {
            pascal(&s.name, s.name_span, "struct", out);
            for p in &s.type_params { pascal(&p.name, p.name_span, "type parameter", out); }
            for f in &s.fields { snake(&f.name, f.name_span, "field", out); }
            for m in &s.methods { func_names(m, "method", out); }
            for b in &s.trait_impls { for m in &b.methods { func_names(m, "method", out); } }
            stmt_names(&s.validate_block, out);
        }
        Item::Enum(e) => {
            pascal(&e.name, e.name_span, "enum", out);
            for p in &e.type_params { pascal(&p.name, p.name_span, "type parameter", out); }
            for g in &e.groups { pascal(&g.path, g.name_span, "variant group", out); }
            for v in &e.variants {
                pascal(&v.name, v.name_span, "enum variant", out);
                if let VariantPayload::Named(fields) = &v.payload {
                    for f in fields { snake(&f.name, f.name_span, "variant field", out); }
                }
            }
            for m in &e.methods { func_names(m, "method", out); }
            for b in &e.trait_impls { for m in &b.methods { func_names(m, "method", out); } }
        }
        Item::Distinct(d) => pascal(&d.name, d.name_span, "distinct type", out),
        Item::TypeAlias(a) => {
            pascal(&a.name, a.name_span, "type alias", out);
            for p in &a.type_params { pascal(&p.name, p.name_span, "type parameter", out); }
        }
        Item::UnitFamily(u) => {
            pascal(&u.family, u.family_span, "unit family", out);
            for member in &u.members {
                // D-UNIT-SCALE-PROVENANCE1: SI-accepted `mmHg` keeps its
                // published symbol; all other user members follow snake_case.
                if member.name != "mmHg" {
                    snake(&member.name, member.name_span, "unit member", out);
                }
            }
        }
        Item::Trait(t) => {
            pascal(&t.name, t.name_span, "trait", out);
            for (name, span) in &t.assoc_types { pascal(name, *span, "associated type", out); }
            for m in &t.methods { trait_method_names(m, out); }
        }
        Item::Tag(t) => {
            pascal(&t.name, t.name_span, "tag", out);
        }
        Item::Impl(i) => for m in &i.methods { func_names(m, "method", out); },
        Item::Const(c) => {
            snake(&c.name, c.name_span, "constant", out);
            expr_names(&c.value, out);
        }
        Item::Test(t) => { for p in &t.params { snake(&p.name, p.name_span, "parameter", out); } stmt_names(&t.body, out); }
        Item::Bench(b) => stmt_names(&b.body, out),
        Item::Module(m) => {
            snake(&m.name, m.name_span, "module", out);
            for source in &m.sources {
                snake(&source.name, source.name_span, "config name", out);
            }
            for import in &m.imports { expr_names(import, out); }
            for member in &m.members { expr_names(member, out); }
        }
        Item::CodeModule(m) => {
            snake(&m.name, m.name_span, "module", out);
            if let Some(body) = &m.body { for item in body { item_names(item, traits, out); } }
        }
        Item::GenericModule(m) => {
            snake(&m.name, m.name_span, "generic module", out);
            for p in &m.params {
                match p {
                    GenericModuleParam::Bare { .. } =>
                        pascal(p.name(), p.name_span(), "type parameter", out),
                    GenericModuleParam::Annotated { annotation, .. } => {
                        let type_param = matches!(annotation, Type::Named(name)
                            if traits.contains(name) || crate::Generics::is_builtin_trait(name));
                        if type_param { pascal(p.name(), p.name_span(), "type parameter", out); }
                        else { snake(p.name(), p.name_span(), "value parameter", out); }
                    }
                }
            }
            for item in &m.body { item_names(item, traits, out); }
        }
        Item::ModuleAlias(m) => snake(&m.name, m.name_span, "module alias", out),
        Item::StateDecl(s) => {
            pascal(&s.type_name, s.type_name_span, "state type", out);
            for (name, span) in &s.states { pascal(name, *span, "state", out); }
        }
        Item::ProtocolDecl(p) => {
            pascal(&p.name, p.name_span, "protocol", out);
            for m in &p.messages {
                pascal(&m.name, m.name_span, "protocol message", out);
                for (name, _) in &m.fields { snake(name, m.span, "message field", out); }
            }
        }
        Item::UserDerive(d) => {
            pascal(&d.trait_name, d.trait_span, "trait", out);
            stmt_names(&d.body, out);
        }
        // D-SHAPE-CASE2=A: foreign declarations are exempt zones. Other
        // declaration families carry no user-defined identifier category.
        Item::CModule(_) | Item::ExternRust(_) | Item::ErrorConv(_) | Item::Migration(_) => {}
    }
}

fn binding_name(b: &crate::AST::Binding, out: &mut Vec<Diagnostic>) {
    if let Some(pattern) = &b.pattern {
        for n in pattern.names() {
            let span = n.rename.as_ref().map_or(n.span, |(_, span)| *span);
            snake(n.local_name(), span, "local", out);
        }
    } else { snake(&b.name, b.name_span, if b.is_comptime { "local constant" } else { "local" }, out); }
}

fn stmt_names(stmts: &[Stmt], out: &mut Vec<Diagnostic>) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(e) | Stmt::Yield(e, _) => expr_names(e, out),
            Stmt::Val(b) => { binding_name(b, out); expr_names(&b.init, out); }
            Stmt::Assign { target, value, .. } => {
                match target {
                    LValue::Index { base, index, .. } => { expr_names(base, out); expr_names(index, out); }
                    LValue::Field { base, .. } => expr_names(base, out),
                    LValue::Local { .. } => {}
                }
                expr_names(value, out);
            }
            Stmt::Return(value, _) => if let Some(value) = value { expr_names(value, out); },
            Stmt::While { cond, body, label, .. } => {
                expr_names(cond, out);
                if let Some((name, span)) = label { snake(name, *span, "loop label", out); }
                stmt_names(body, out);
            }
            Stmt::Loop { body, label, .. } => {
                if let Some((name, span)) = label { snake(name, *span, "loop label", out); }
                stmt_names(body, out);
            }
            Stmt::For { var, var_span, var2, kind, body, label, .. } => {
                snake(var, *var_span, "local", out);
                if let Some((name, span)) = var2 { snake(name, *span, "local", out); }
                match kind {
                    ForKind::Range { start, end, step, exclusive: _ } => {
                        expr_names(start, out); expr_names(end, out);
                        if let Some(step) = step { expr_names(step, out); }
                    }
                    ForKind::In { collection, step } => {
                        expr_names(collection, out);
                        if let Some(step) = step { expr_names(step, out); }
                    }
                }
                if let Some((name, span)) = label { snake(name, *span, "loop label", out); }
                stmt_names(body, out);
            }
            Stmt::Switch { subject, arms, else_body, .. }
            | Stmt::ComptimeSwitch { subject, arms, else_body, .. } => {
                expr_names(subject, out);
                for arm in arms { expr_names(&arm.cond, out); stmt_names(&arm.body, out); }
                if let Some(body) = else_body { stmt_names(body, out); }
            }
            Stmt::CountedLoop { init, cond, step, body, label, .. } => {
                binding_name(init, out);
                expr_names(&init.init, out);
                expr_names(cond, out);
                if let Some(step) = step { stmt_names(std::slice::from_ref(step), out); }
                if let Some((name, span)) = label { snake(name, *span, "loop label", out); }
                stmt_names(body, out);
            }
            Stmt::Region { name, name_span, body, .. }
            | Stmt::TaskGroup { name, name_span, body, .. }
            | Stmt::Layout { name, name_span, body, .. } => {
                snake(name, *name_span, "local", out); stmt_names(body, out);
            }
            Stmt::Grant { binding, binding_span, body, .. } => {
                snake(binding, *binding_span, "local", out); stmt_names(body, out);
            }
            Stmt::Transact { name, name_span, body, .. } => {
                if let (Some(name), Some(span)) = (name, name_span) { snake(name, *span, "local", out); }
                stmt_names(body, out);
            }
            Stmt::ComptimeIf { cond, then_body, else_body, .. } => {
                expr_names(cond, out);
                stmt_names(then_body, out); if let Some(body) = else_body { stmt_names(body, out); }
            }
            Stmt::ContextBlock { fields, body, .. } => {
                for (_, value, _) in fields { expr_names(value, out); }
                stmt_names(body, out);
            }
            Stmt::ScopeMember { args, body, .. } => {
                for arg in args { expr_names(arg, out); }
                stmt_names(body, out);
            }
            Stmt::Unsafe { body, .. } | Stmt::Impure { body, .. } | Stmt::Reactive { body, .. }
            | Stmt::Shield { body, .. } | Stmt::Off { body, .. } | Stmt::DebugOnly { body, .. }
            | Stmt::Policy { body, .. } | Stmt::Caps { body, .. } | Stmt::ComptimeBlock { body, .. }
            | Stmt::Live { body, .. } | Stmt::AssumeDet { body, .. } => stmt_names(body, out),
            Stmt::BreakValue(value, _) | Stmt::BreakLabelValue(_, _, value, _) => {
                expr_names(value, out)
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {}
        }
    }
}

fn pattern_names(pattern: &Pattern, out: &mut Vec<Diagnostic>) {
    match pattern {
        Pattern::Variant { bindings, .. } => for binding in bindings {
            if let (Some(name), Some(span)) = (binding.as_bind(), binding.binding_span()) {
                snake(name, span, "pattern binding", out);
            }
        },
        Pattern::Present { binding, binding_span, .. }
        | Pattern::Ok { binding, binding_span, .. }
        | Pattern::Err { binding, binding_span, .. } =>
            snake(binding, *binding_span, "pattern binding", out),
        Pattern::Or(patterns, _) => for pattern in patterns { pattern_names(pattern, out); },
        Pattern::Struct { fields, .. } => for field in fields {
            match field {
                StructPatField::Bind { local, local_span, .. } =>
                    snake(local, *local_span, "pattern binding", out),
                StructPatField::Value { value, .. } => expr_names(value, out),
            }
        },
        Pattern::StrMatch { parts, .. } => for part in parts {
            if let crate::AST::StrMatchPart::Hole { name, span, .. } = part {
                snake(name, *span, "pattern binding", out);
            }
        },
        Pattern::BinMatch { parts, .. } => for part in parts {
            if let crate::AST::BinMatchPart::Hole { name, span, .. } = part {
                snake(name, *span, "pattern binding", out);
            }
        },
        Pattern::Absent(_) | Pattern::Range { .. } => {}
    }
}

fn expr_names(expr: &Expr, out: &mut Vec<Diagnostic>) {
    match expr {
        Expr::Str(parts, _) => for part in parts {
            if let StrPart::Interp(value, _) = part { expr_names(value, out); }
        },
        Expr::StrMatchLit(parts, _) => for part in parts {
            if let crate::AST::StrMatchPart::Hole { name, span, .. } = part {
                snake(name, *span, "pattern binding", out);
            }
        },
        Expr::BinMatchLit(parts, _) => for part in parts {
            if let crate::AST::BinMatchPart::Hole { name, span, .. } = part {
                snake(name, *span, "pattern binding", out);
            }
        },
        Expr::Call(call) => for arg in &call.args { expr_names(&arg.expr, out); },
        Expr::Unary(_, inner, _) | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _) | Expr::RawOf(inner, _) | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _) | Expr::Field(inner, _, _) | Expr::Tainted(inner, _, _)
        | Expr::Present(inner, _) | Expr::Ok(inner, _) | Expr::Err(inner, _)
        | Expr::Try(inner, _, _) | Expr::Paren(inner, _) | Expr::Spread(inner, _) =>
            expr_names(inner, out),
        Expr::MemberSpread { base, .. } => expr_names(base, out),
        Expr::OptField { base, .. } => expr_names(base, out),
        Expr::MethodCall { receiver, args, .. } => {
            expr_names(receiver, out);
            for arg in args { expr_names(&arg.expr, out); }
        }
        Expr::StructLit { fields, .. } => for (_, _, value) in fields { expr_names(value, out); },
        Expr::TypedLit { body, .. } => body.for_each_expr(|value| expr_names(value, out)),
        Expr::EnumLit { args, .. } => for arg in args {
            match arg {
                EnumLitArg::Positional(value) | EnumLitArg::Named { expr: value, .. } =>
                    expr_names(value, out),
            }
        },
        Expr::OrFallback { value, fallback, .. } => {
            expr_names(value, out);
            match fallback {
                OrFallback::Value(value) | OrFallback::Return(Some(value), _) => expr_names(value, out),
                OrFallback::Panic { args, .. } => for arg in args { expr_names(&arg.expr, out); },
                OrFallback::Return(None, _)
                | OrFallback::Break(_)
                | OrFallback::Continue(_)
                | OrFallback::BreakLabel(..)
                | OrFallback::ContinueLabel(..) => {}
            }
        }
        Expr::PatternTest { subject, pattern, .. } => {
            expr_names(subject, out);
            pattern_names(pattern, out);
        }
        Expr::Binary(_, left, right, _) => { expr_names(left, out); expr_names(right, out); }
        Expr::CompareChain { operands, .. } | Expr::ListLit(operands, _) =>
            for value in operands { expr_names(value, out); },
        Expr::TupleLit(fields, _, _) => for (_, value) in fields { expr_names(value, out); },
        Expr::MapLit(entries, _) => for (key, value) in entries {
            expr_names(key, out); expr_names(value, out);
        },
        Expr::Index { base, index, .. } => { expr_names(base, out); expr_names(index, out); }
        Expr::Slice { base, start, end, range, .. } => {
            expr_names(base, out);
            if let Some(range) = range {
                expr_names(range, out);
            } else {
                expr_names(start, out); expr_names(end, out);
            }
        }
        Expr::Range { start, end, .. } => {
            expr_names(start, out); expr_names(end, out);
        }
        Expr::CallValue { callee, args, .. } => {
            expr_names(callee, out);
            for arg in args { expr_names(&arg.expr, out); }
        }
        Expr::Lambda(lambda) => {
            for param in &lambda.params { snake(&param.name, param.name_span, "lambda parameter", out); }
            match &lambda.body {
                LambdaBody::Expr(value) => expr_names(value, out),
                LambdaBody::Block(body) => stmt_names(body, out),
            }
        }
        Expr::If { cond, then_body, then_value, else_body, else_value, .. } => {
            expr_names(cond, out); stmt_names(then_body, out); expr_names(then_value, out);
            stmt_names(else_body, out); expr_names(else_value, out);
        }
        Expr::PtrFromAddr { addr, .. } => expr_names(addr, out),
        Expr::Char(..) | Expr::Int(..) | Expr::Float(..) | Expr::Bool(..)
        | Expr::Ident(..) | Expr::UnitLit { .. } | Expr::Absent(_)
        | Expr::Todo { .. } | Expr::NoElse(_) | Expr::ReduceMarker(..) | Expr::ComptimeSplice { .. } => {}
    }
}
