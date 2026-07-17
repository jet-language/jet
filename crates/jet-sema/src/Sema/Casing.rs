use crate::AST::{ElseBranch, Func, Item, ProgramBundle, Stmt, TraitMethodSig, VariantPayload};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax::{self, NameCase};

pub(super) fn validate_bundle(bundle: &ProgramBundle) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for module in &bundle.modules {
        for item in &module.items { item_names(item, &mut out); }
    }
    out
}

fn check(name: &str, span: Span, category: &str, case: NameCase, out: &mut Vec<Diagnostic>) {
    // Compiler-reserved names have their own D-SHAPE-DUNDER2 diagnostic.
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
    for segment in name.split('.') { check(segment, span, category, NameCase::Pascal, out); }
}

fn snake(name: &str, span: Span, category: &str, out: &mut Vec<Diagnostic>) {
    for segment in name.split(['.', '-']) { check(segment, span, category, NameCase::Snake, out); }
}

fn func_names(f: &Func, category: &str, out: &mut Vec<Diagnostic>) {
    snake(&f.name, f.name_span, category, out);
    for p in &f.type_params { pascal(&p.name, p.name_span, "type parameter", out); }
    for p in &f.params { if p.name != Syntax::KW_SELF { snake(&p.name, p.name_span, "parameter", out); } }
    stmt_names(&f.body, out);
}

fn trait_method_names(m: &TraitMethodSig, out: &mut Vec<Diagnostic>) {
    snake(&m.name, m.name_span, "method", out);
    for p in &m.params { if p.name != Syntax::KW_SELF { snake(&p.name, p.name_span, "parameter", out); } }
    if let Some(body) = &m.default_body { stmt_names(body, out); }
}

fn item_names(item: &Item, out: &mut Vec<Diagnostic>) {
    match item {
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
            for g in &e.groups { pascal(&g.path, g.name_span, "enum variant group", out); }
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
            for (name, span) in &u.members { snake(name, *span, "unit member", out); }
        }
        Item::Trait(t) => {
            pascal(&t.name, t.name_span, "trait", out);
            for (name, span) in &t.assoc_types { pascal(name, *span, "associated type", out); }
            for m in &t.methods { trait_method_names(m, out); }
        }
        Item::Tag(t) => {
            pascal(&t.name, t.name_span, "tag", out);
            for m in &t.methods { trait_method_names(m, out); }
        }
        Item::Impl(i) => for m in &i.methods { func_names(m, "method", out); },
        Item::Const(c) => snake(&c.name, c.name_span, "constant", out),
        Item::Test(t) => { for p in &t.params { snake(&p.name, p.name_span, "parameter", out); } stmt_names(&t.body, out); }
        Item::Bench(b) => stmt_names(&b.body, out),
        Item::Module(m) => snake(&m.name, m.name_span, "module", out),
        Item::CodeModule(m) => {
            snake(&m.name, m.name_span, "module", out);
            if let Some(body) = &m.body { for item in body { item_names(item, out); } }
        }
        Item::GenericModule(m) => {
            snake(&m.name, m.name_span, "generic module", out);
            for p in &m.params {
                if matches!(p, crate::AST::GenericModuleParam::Bare { .. }) {
                    pascal(p.name(), p.name_span(), "type parameter", out);
                }
            }
            for item in &m.body { item_names(item, out); }
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
        for n in pattern.names() { snake(&n.name, n.span, "local", out); }
    } else { snake(&b.name, b.name_span, if b.is_comptime { "local constant" } else { "local" }, out); }
}

fn stmt_names(stmts: &[Stmt], out: &mut Vec<Diagnostic>) {
    for stmt in stmts {
        match stmt {
            Stmt::Val(b) => binding_name(b, out),
            Stmt::If(i) => {
                stmt_names(&i.then_body, out);
                match &i.else_branch { Some(ElseBranch::ElseIf(i)) => stmt_names(std::slice::from_ref(&Stmt::If((**i).clone())), out), Some(ElseBranch::Else(b)) => stmt_names(b, out), None => {} }
            }
            Stmt::While { body, label, .. } | Stmt::Loop { body, label, .. } => {
                if let Some((name, span)) = label { snake(name, *span, "loop label", out); }
                stmt_names(body, out);
            }
            Stmt::For { var, var_span, var2, body, label, .. } => {
                snake(var, *var_span, "local", out);
                if let Some((name, span)) = var2 { snake(name, *span, "local", out); }
                if let Some((name, span)) = label { snake(name, *span, "loop label", out); }
                stmt_names(body, out);
            }
            Stmt::Switch { arms, else_body, .. } | Stmt::ComptimeSwitch { arms, else_body, .. } => {
                for arm in arms { stmt_names(&arm.body, out); }
                if let Some(body) = else_body { stmt_names(body, out); }
            }
            Stmt::CountedLoop { init, step, body, label, .. } => {
                binding_name(init, out);
                stmt_names(std::slice::from_ref(step), out);
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
            Stmt::ComptimeIf { then_body, else_body, .. } => {
                stmt_names(then_body, out); if let Some(body) = else_body { stmt_names(body, out); }
            }
            Stmt::Unsafe { body, .. } | Stmt::Impure { body, .. } | Stmt::Reactive { body, .. }
            | Stmt::Shield { body, .. } | Stmt::Off { body, .. } | Stmt::DebugOnly { body, .. }
            | Stmt::Policy { body, .. } | Stmt::Caps { body, .. } | Stmt::ComptimeBlock { body, .. }
            | Stmt::ContextBlock { body, .. } | Stmt::Live { body, .. } | Stmt::AssumeDet { body, .. }
            | Stmt::ScopeMember { body, .. } => stmt_names(body, out),
            _ => {}
        }
    }
}
