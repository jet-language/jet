use super::*;
use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use crate::Traits::TraitRegistry;
use crate::AST::{
    CodeModule, ConstAttr, ElseBranch, EnumDef, EnumLitArg, Expr, ForKind, Func, GenericModuleDef,
    GenericModuleParam, IfStmt, ImportKind, Item, LValue, LambdaBody, ModuleAliasDef, ModuleArg,
    OrFallback, Param, ProgramBundle, RustConstKind, Stmt, StrPart, Type, VariantPayload,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// D-CLIFLAG1: what `fn run`'s single parameter type turned out to be.
enum CliEntryShape {
    /// A `@[Cli]`-derived struct — flags come straight from its fields.
    Struct,
    /// An `enum` whose every variant carries a `@[Cli]` struct payload.
    Enum,
    /// An `enum` parameter with at least one non-`@[Cli]` variant (E1307).
    EnumBadVariants(Vec<Diagnostic>),
    /// Neither of the above (E1308).
    Invalid,
}

/// D-CLIFLAG1: classify `fn run`'s parameter type against the entry file's
/// own struct/enum definitions. Only looks at the entry module (matching the
/// rest of the entry-point machinery, which only ever inspects the entry
/// file — same scope as the `main`/E0101 checks above).
fn cli_entry_param_shape(items: &[Item], ty: &Type, reg: &TraitRegistry) -> CliEntryShape {
    let Type::Named(name) = ty else {
        return CliEntryShape::Invalid;
    };
    if reg.implements_trait(name, "Cli") {
        return CliEntryShape::Struct;
    }
    let enum_def: Option<&EnumDef> = items.iter().find_map(|i| match i {
        Item::Enum(e) if &e.name == name => Some(e),
        _ => None,
    });
    let Some(e) = enum_def else {
        return CliEntryShape::Invalid;
    };
    let mut bad = Vec::new();
    for v in &e.variants {
        let ok = matches!(
            &v.payload,
            VariantPayload::Single(Type::Named(p), _) if reg.implements_trait(p, "Cli")
        );
        if !ok {
            bad.push(e1307(&v.name, v.name_span));
        }
    }
    if bad.is_empty() {
        CliEntryShape::Enum
    } else {
        CliEntryShape::EnumBadVariants(bad)
    }
}

/// E0101: the entry file has no canonical `fn run`.
fn no_run_error() -> Diagnostic {
    Diagnostic::error(
        "E0101",
        "this program has no `run` function".to_string(),
        "running a program starts at `fn run`, and the entry file doesn't define one".to_string(),
        "add `fn run() { ... }` to the entry file".to_string(),
        None,
    )
}

fn package_scope_for(path: &Path, project_root: &Path) -> PathBuf {
    let norm_path = normalize_sem_path(path);
    let norm_root = normalize_sem_path(project_root);
    if norm_path.starts_with(&norm_root) {
        return norm_root;
    }
    norm_path
        .parent()
        .map(normalize_sem_path)
        .unwrap_or(norm_path)
}

fn normalize_sem_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// D-MOD2: inside an inline `module M { … }`, a call to a sibling function
/// `helper(x)` must lower to the mangled `M__helper`. This pre-pass rewrites
/// such call names so registration, body-checking, and codegen all agree.
/// Only callee names are rewritten (the unambiguous case); a sibling referenced
/// as a value resolves through normal name lookup and yields a clean Jet error
/// rather than leaking to rustc.
// ---------------------------------------------------------------------------
// D-GENMOD2=A: generic module expansion (R11 pre-pass)
// ---------------------------------------------------------------------------
//
// `module StringCache32 = Lru<String, 32>` expands into a synthetic
// `CodeModule` with the same body as the generic template, with every
// TypeParam name substituted by the supplied type arg. The original
// GenericModule/ModuleAlias items are then erased. This runs before
// `mangle_inline_sibling_calls` so the expanded body is visible to that pass.

/// Substitute every occurrence of `param_name` in `ty` with `replacement`.
fn substitute_type(ty: Type, param_name: &str, replacement: &Type) -> Type {
    match ty {
        Type::Named(ref n) if n == param_name => replacement.clone(),
        Type::Named(_) => ty,
        Type::List(inner) => Type::List(Box::new(substitute_type(*inner, param_name, replacement))),
        Type::Map { key, value } => Type::Map {
            key: Box::new(substitute_type(*key, param_name, replacement)),
            value: Box::new(substitute_type(*value, param_name, replacement)),
        },
        Type::Shared(inner) => {
            Type::Shared(Box::new(substitute_type(*inner, param_name, replacement)))
        }
        Type::Option(inner) => {
            Type::Option(Box::new(substitute_type(*inner, param_name, replacement)))
        }
        Type::Result { ok, err } => Type::Result {
            ok: Box::new(substitute_type(*ok, param_name, replacement)),
            err: Box::new(substitute_type(*err, param_name, replacement)),
        },
        Type::Fn {
            params,
            ret,
            effect_bound,
        } => Type::Fn {
            params: params
                .into_iter()
                .map(|t| substitute_type(t, param_name, replacement))
                .collect(),
            ret: ret.map(|r| Box::new(substitute_type(*r, param_name, replacement))),
            effect_bound,
        },
        Type::Apply { name, args } => Type::Apply {
            name,
            args: args
                .into_iter()
                .map(|t| substitute_type(t, param_name, replacement))
                .collect(),
        },
        Type::Tuple(parts) => Type::Tuple(
            parts
                .into_iter()
                .map(|(n, t)| (n, Box::new(substitute_type(*t, param_name, replacement))))
                .collect(),
        ),
        Type::FixedList { elem, len } => Type::FixedList {
            elem: Box::new(substitute_type(*elem, param_name, replacement)),
            len,
        },
        Type::Tagged { marker, inner } => Type::Tagged {
            marker,
            inner: Box::new(substitute_type(*inner, param_name, replacement)),
        },
        // Primitives and opaque variants carry no nested Type.
        other => other,
    }
}

fn substitute_type_in_param(mut p: Param, param_name: &str, replacement: &Type) -> Param {
    p.ty = substitute_type(p.ty, param_name, replacement);
    p
}

fn substitute_type_in_func(mut f: Func, param_name: &str, replacement: &Type) -> Func {
    f.params = f
        .params
        .into_iter()
        .map(|p| substitute_type_in_param(p, param_name, replacement))
        .collect();
    if let Some(ret) = f.return_type {
        f.return_type = Some(substitute_type(ret, param_name, replacement));
    }
    // Body stmt-level substitution is not done here — the sema checker resolves
    // type names from declarations, so only signature types need substitution.
    f
}

fn apply_type_args_to_func(f: Func, params: &[GenericModuleParam], args: &[ModuleArg]) -> Func {
    let mut out = f;
    for (param, arg) in params.iter().zip(args.iter()) {
        match (param, arg) {
            (GenericModuleParam::TypeParam { name, .. }, ModuleArg::Type(ty, _)) => {
                out = substitute_type_in_func(out, name, ty);
            }
            _ => {} // value params are left as-is for now (would need const-eval)
        }
    }
    out
}

/// A cloned-and-filtered view of a generic module template.
/// `non_fn_kinds` records item kinds that were dropped (non-Func),
/// so E0854 can fire at alias-expansion time.
struct TemplateInfo {
    def: GenericModuleDef,
    non_fn_kinds: Vec<&'static str>,
}

fn expand_alias(
    alias: &ModuleAliasDef,
    templates: &std::collections::HashMap<String, TemplateInfo>,
    diags: &mut Vec<Diagnostic>,
) -> Option<CodeModule> {
    let info = match templates.get(&alias.target) {
        Some(t) => t,
        None => {
            diags.push(Diagnostic::error(
                "E0850",
                format!("generic module `{}` not found in this scope", alias.target),
                "check the module template name and make sure it is defined in the same file"
                    .to_string(),
                format!("example: `module {} = MyTemplate<String>`", alias.name),
                Some(alias.target_span),
            ));
            return None;
        }
    };
    let template = &info.def;
    if alias.args.len() != template.params.len() {
        diags.push(Diagnostic::error(
            "E0851",
            format!(
                "module alias `{}` passes {} argument(s) but `{}` expects {}",
                alias.name,
                alias.args.len(),
                alias.target,
                template.params.len(),
            ),
            "the number of type/value arguments must match the template parameter list".to_string(),
            format!(
                "example: `module {} = {}<{}>` with {} arg(s)",
                alias.name,
                alias.target,
                template
                    .params
                    .iter()
                    .map(|p| match p {
                        GenericModuleParam::TypeParam { name, .. } => name.as_str(),
                        GenericModuleParam::ValueParam { name, .. } => name.as_str(),
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
                template.params.len(),
            ),
            Some(alias.span),
        ));
        return None;
    }
    // Emit E0854 for each non-Func item kind that was in the original template.
    for kind in &info.non_fn_kinds {
        diags.push(Diagnostic::error(
            "E0854",
            format!(
                "generic module `{}` contains a `{}` item, which cannot be instantiated yet",
                template.name, kind
            ),
            "generic module bodies currently support only `fn` items".to_string(),
            "move structs/enums outside the generic module and pass them as type params"
                .to_string(),
            Some(alias.span),
        ));
    }
    if !info.non_fn_kinds.is_empty() {
        return None;
    }
    // Substitute type args into each function signature in the template body.
    let body: Vec<Item> = template
        .body
        .iter()
        .filter_map(|item| {
            if let Item::Func(f) = item {
                let expanded = apply_type_args_to_func(f.clone(), &template.params, &alias.args);
                Some(Item::Func(expanded))
            } else {
                None // already reported above
            }
        })
        .collect();
    Some(CodeModule {
        name: alias.name.clone(),
        name_span: alias.name_span,
        is_pub: alias.is_pub,
        is_package_pub: alias.is_package_pub,
        body: Some(body),
        web_target: None,
        span: alias.span,
    })
}

/// D-GENMOD2=A: expand every `ModuleAlias` in each module's item list into a
/// concrete `CodeModule` using the corresponding `GenericModule` template.
/// Templates and aliases are removed from the item list after expansion.
pub(crate) fn expand_generic_module_aliases(
    bundle: &mut ProgramBundle,
    diags: &mut Vec<Diagnostic>,
) {
    for module in bundle.modules.iter_mut() {
        // Collect templates by name, recording non-Func item kinds for E0854.
        let templates: std::collections::HashMap<String, TemplateInfo> = module
            .items
            .iter()
            .filter_map(|i| {
                if let Item::GenericModule(gm) = i {
                    let mut non_fn_kinds: Vec<&'static str> = Vec::new();
                    let fn_items: Vec<Item> = gm
                        .body
                        .iter()
                        .filter_map(|item| match item {
                            Item::Func(f) => Some(Item::Func(f.clone())),
                            Item::Struct(_) => {
                                non_fn_kinds.push("struct");
                                None
                            }
                            Item::Enum(_) => {
                                non_fn_kinds.push("enum");
                                None
                            }
                            Item::Const(_) => {
                                non_fn_kinds.push("const");
                                None
                            }
                            _ => {
                                non_fn_kinds.push("item");
                                None
                            }
                        })
                        .collect();
                    Some((
                        gm.name.clone(),
                        TemplateInfo {
                            def: GenericModuleDef {
                                name: gm.name.clone(),
                                name_span: gm.name_span,
                                is_pub: gm.is_pub,
                                is_package_pub: gm.is_package_pub,
                                params: gm.params.clone(),
                                body: fn_items,
                                span: gm.span,
                            },
                            non_fn_kinds,
                        },
                    ))
                } else {
                    None
                }
            })
            .collect();

        // Expand aliases into CodeModules, collect separately.
        let mut expansions: Vec<(usize, CodeModule)> = Vec::new();
        for (idx, item) in module.items.iter().enumerate() {
            if let Item::ModuleAlias(alias) = item {
                if let Some(cm) = expand_alias(alias, &templates, diags) {
                    expansions.push((idx, cm));
                }
            }
        }

        // Replace/erase: iterate in reverse to preserve indices.
        // For each alias, replace it with the expanded CodeModule.
        // GenericModule items are erased (replaced with nothing).
        // We need to:
        // 1. Replace each ModuleAlias with its CodeModule expansion (collected above)
        // 2. Remove all GenericModule items
        for (idx, cm) in expansions {
            module.items[idx] = Item::CodeModule(cm);
        }
        module
            .items
            .retain(|i| !matches!(i, Item::GenericModule(_)));
    }
}

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
                    ForKind::In { collection } => {
                        rewrite_inline_calls_expr(collection, siblings, modname);
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
            | Stmt::SuppressMustUse { body: inner, .. }
            | Stmt::Region { body: inner, .. }
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
        Expr::Ident(_, _)
        | Expr::Char(_, _)
        | Expr::Int(_, _, _)
        | Expr::Float(_, _, _)
        | Expr::Bool(_, _)
        | Expr::Absent(_)
        | Expr::ReduceMarker(_, _)
        | Expr::Todo { .. }
        | Expr::UnitLit { .. }
        | Expr::ComptimeSplice { .. }
        // D-SHIFT1 (c7shift): a leaf literal, no nested `Expr` to recurse into.
        | Expr::StrMatchLit(_, _) => {}
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
        | Expr::Field(inner, _, _)
        | Expr::Tainted(inner, _) // D-TAINT1: tag erased; recurse into the value.
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
                | OrFallback::Continue(_) => {}
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

pub fn check_bundle(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    check_bundle_opts(bundle, mode, false, false).0
}

/// Like `check_bundle` but also returns effect facts for D-SEMINDEX1.
pub fn check_bundle_with_effect_facts(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    check_bundle_opts(bundle, mode, false, false)
}

/// Like `check_bundle` but with extra build options (E2-M15).
pub fn check_bundle_freestanding(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    check_bundle_opts(bundle, mode, true, false).0
}

/// Like `check_bundle` but with D-CTEFFECT1 `--allow-impure` flag.
pub fn check_bundle_allow_impure(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    check_bundle_opts(bundle, mode, false, true).0
}

pub(crate) fn check_bundle_opts(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    freestanding: bool,
    allow_impure: bool,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    let mut diags = Vec::new();
    // D-OSTARGET2=B (ratified 2026-07-03): fold every `comptime if build.os == {
    // … }` switch to the arm matching this build's active OS *before* any other
    // pass sees a body — so OS-gating checks, the type-checker, and codegen only
    // meet the taken arm. Rewrites into a `comptime if` chain (reuses D-WHEN1).
    diags.extend(super::desugar_os_switches(bundle));
    // D-MIGRATE4: desugar each `change … via { (old) => … }` converter on a
    // decodable `@PublishedSchema` type into a synthetic top-level converter
    // function, so the runtime migration step (codegen) can call it. Runs before
    // registration/checking so those synthetic functions are type-checked and
    // lowered through the normal pipeline. Sets `conv_fn` on the `change` op.
    super::desugar_migrations(bundle);
    // D-GENMOD2=A: expand module aliases into concrete CodeModules before any
    // sibling-call mangling or registration sees the items.
    expand_generic_module_aliases(bundle, &mut diags);
    // D-MOD2: rewrite inline-module sibling calls to their mangled names before any
    // registration/checking/codegen sees the bodies.
    mangle_inline_sibling_calls(bundle);
    // D-CAP8 (= C): resolve unmarked (`Infer`) parameter capabilities from body usage
    // before registration/checking/codegen — they then see resolved conventions, never
    // `Infer`. Deterministic; mutates the AST param conventions in place.
    super::Capability::resolve_capabilities(bundle);
    let mut states: Vec<ModuleState> = bundle
        .modules
        .iter()
        .map(|m| ModuleState {
            package_scope: package_scope_for(&m.path, &bundle.project_root),
            funcs: HashMap::new(),
            func_pub: HashMap::new(),
            func_pkg_pub: HashMap::new(),
            type_pub: HashMap::new(),
            type_pkg_pub: HashMap::new(),
            method_pub: HashMap::new(),
            method_pkg_pub: HashMap::new(),
            field_pub: HashMap::new(),
            field_pkg_pub: HashMap::new(),
            registry: TypeRegistry {
                types: HashMap::new(),
                ref_field_labels: HashMap::new(),
                computed_fields: HashMap::new(),
            },
            structs: HashMap::new(),
            consts: HashMap::new(),
            imports: HashMap::new(),
            core_imports: HashMap::new(),
            tests: HashMap::new(),
            trait_reg: TraitRegistry::default(),
            code_modules: HashMap::new(),
            unqualified: HashMap::new(),
            unqualified_file: HashMap::new(),
            reexports: HashMap::new(),
        })
        .collect();

    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        super::Protocol::expand_module_protocols(&mut module.items, &mut diags);
        // D-DOTSCOPE1: validate contextual `.member { … }` scope statements
        // against each marker's declared vocabulary (E0614/E0615/E0616/E0617/E0618).
        diags.extend(super::ScopeMembers::check(&module.items));
        // D-FIELDPOL1: computed-field cycle check (E0338) + `self.field`
        // rewrite + synthesized getter methods, before anything else.
        process_computed_fields(&mut module.items, &mut diags);
        // D-PATCH1: synthetic `T.Patch` before struct registration.
        inject_patchable_types(&mut module.items, &mut diags);
        let st = &mut states[idx];
        for item in &module.items {
            match item {
                Item::Func(f) => register_func_item(f, st, &mut diags),
                Item::Struct(s) => {
                    register_struct(
                        s,
                        &mut st.registry,
                        &mut st.structs,
                        &mut diags,
                        &st.funcs,
                        &st.consts,
                    );
                    st.type_pub
                        .insert(s.name.clone(), s.is_pub && !s.is_package_pub);
                    st.type_pkg_pub.insert(s.name.clone(), s.is_package_pub);
                    for fld in &s.fields {
                        st.field_pub.insert(
                            (s.name.clone(), fld.name.clone()),
                            fld.is_pub && !fld.is_package_pub,
                        );
                        st.field_pkg_pub
                            .insert((s.name.clone(), fld.name.clone()), fld.is_package_pub);
                    }
                    for m in &s.methods {
                        st.method_pub.insert(
                            (s.name.clone(), m.name.clone()),
                            m.is_pub && !m.is_package_pub,
                        );
                        st.method_pkg_pub
                            .insert((s.name.clone(), m.name.clone()), m.is_package_pub);
                    }
                }
                Item::Enum(e) => {
                    register_enum(e, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub
                        .insert(e.name.clone(), e.is_pub && !e.is_package_pub);
                    st.type_pkg_pub.insert(e.name.clone(), e.is_package_pub);
                    for m in &e.methods {
                        st.method_pub.insert(
                            (e.name.clone(), m.name.clone()),
                            m.is_pub && !m.is_package_pub,
                        );
                        st.method_pkg_pub
                            .insert((e.name.clone(), m.name.clone()), m.is_package_pub);
                    }
                }
                Item::Impl(i) => {
                    if !i.type_name.contains('.') && !st.registry.contains(&i.type_name) {
                        diags.push(Diagnostic::error(
                            "E0301",
                            format!("`impl {}` names a type that doesn't exist", i.type_name),
                            format!("`{}` hasn't been defined as a struct or enum", i.type_name),
                            format!(
                                "define `struct {}` or `enum {}` first",
                                i.type_name, i.type_name
                            ),
                            Some(i.type_span),
                        ));
                    } else if !i.type_name.contains('.') {
                        for m in &i.methods {
                            st.method_pub.insert(
                                (i.type_name.clone(), m.name.clone()),
                                m.is_pub && !m.is_package_pub,
                            );
                            st.method_pkg_pub
                                .insert((i.type_name.clone(), m.name.clone()), m.is_package_pub);
                        }
                    }
                }
                Item::Const(c) => {
                    register_const(c, &mut st.consts, &mut diags, &st.funcs, &st.registry)
                }
                Item::Distinct(d) => {
                    register_distinct(d, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub
                        .insert(d.name.clone(), d.is_pub && !d.is_package_pub);
                    st.type_pkg_pub.insert(d.name.clone(), d.is_package_pub);
                }
                Item::TypeAlias(a) => {
                    register_type_alias(a, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub
                        .insert(a.name.clone(), a.is_pub && !a.is_package_pub);
                    st.type_pkg_pub.insert(a.name.clone(), a.is_package_pub);
                }
                // D-QUAL3: a unit family lowers to one `@Numeric` distinct type
                // per member, each erasing to `Float`.
                Item::UnitFamily(uf) => {
                    for d in uf.distinct_defs() {
                        register_distinct(&d, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                        st.type_pub
                            .insert(d.name.clone(), d.is_pub && !d.is_package_pub);
                        st.type_pkg_pub.insert(d.name.clone(), d.is_package_pub);
                    }
                }
                Item::Test(t) => {
                    if name_defined(&t.name, &st.funcs, &st.registry, &st.consts)
                        || st.tests.contains_key(&t.name)
                    {
                        diags.push(defined_twice(
                            &t.name,
                            "every test needs a unique name so failures are easy to find",
                            t.name_span,
                        ));
                    } else {
                        st.tests.insert(t.name.clone(), t.name_span);
                    }
                }
                // D-BENCH1: `#Bench` blocks define no referenceable name; codegen
                // discovers them straight from the AST, so registration is a no-op.
                Item::Bench(_) => {}
                Item::ExternRust(block) => {
                    if check_extern_block(block, &st.registry, &mut diags) {
                        for ef in &block.functions {
                            register_extern_fn(
                                ef,
                                &mut st.funcs,
                                &st.registry,
                                &st.consts,
                                &mut diags,
                            );
                        }
                    }
                }
                Item::CModule(cm) => {
                    if check_c_module(cm, &st.registry, &mut diags) {
                        for ef in &cm.functions {
                            register_extern_fn(
                                ef,
                                &mut st.funcs,
                                &st.registry,
                                &st.consts,
                                &mut diags,
                            );
                            // C FFI functions are callable across the `use c.<lib>`
                            // alias — expose them like any pub item.
                            st.func_pub.insert(ef.name.clone(), true);
                        }
                    }
                }
                Item::Trait(_) => {}
                // D-QUAL2: a tag is a marker; it registers no callable items.
                Item::Tag(_) => {}
                Item::Module(_) => {}
                Item::CodeModule(cm) => {
                    if let Some(body) = &cm.body {
                        // D-MOD2: register inline module functions under mangled names
                        // (`math__double`) so call-site sema can check them.
                        st.code_modules.insert(cm.name.clone(), cm.name.clone());
                        for inner in body {
                            if let Item::Func(f) = inner {
                                let mangled = format!("{}__{}", cm.name, f.name);
                                st.funcs.insert(mangled.clone(), func_to_sig(f));
                                st.func_pub.insert(mangled, f.is_pub && !f.is_package_pub);
                                st.func_pkg_pub
                                    .insert(format!("{}__{}", cm.name, f.name), f.is_package_pub);
                            }
                        }
                    }
                }
                Item::ErrorConv(_) => {}
                // D-MIGRATE1: migration decls are handled by the schema diff pass; no registration needed.
                Item::Migration(_) => {}
                // D-STATE-DECL: state-set decls are sema-only (I3); no type to register.
                Item::StateDecl(_) => {}
                // D-PROTO1/D-PROTO2: expanded before registration; declaration erases.
                Item::ProtocolDecl(_) => {}
                // D-METADERIVE1=A: user-authored derive blocks are expanded below; skip here.
                Item::UserDerive(_) => {}
                // D-GENMOD2=A: templates/aliases already expanded; erase.
                Item::GenericModule(_) | Item::ModuleAlias(_) => {}
            }
        }
        // D-METADERIVE1=A: user-derive expansion — run after struct/func registration so
        // derive bodies can call helper functions and access TypeInfo. Re-entry (D-CTCODEGEN1=A):
        // emitted fragments go through the full lexer→parser pipeline and are appended as items.
        {
            let user_derives: Vec<(String, String, Vec<crate::AST::Stmt>)> = module
                .items
                .iter()
                .filter_map(|i| {
                    if let Item::UserDerive(d) = i {
                        Some((d.trait_name.clone(), d.type_param.clone(), d.body.clone()))
                    } else {
                        None
                    }
                })
                .collect();

            if !user_derives.is_empty() {
                let struct_infos: Vec<&crate::AST::StructDef> = module
                    .items
                    .iter()
                    .filter_map(|i| {
                        if let Item::Struct(s) = i {
                            Some(s)
                        } else {
                            None
                        }
                    })
                    .collect();

                let actual_funcs: HashMap<String, &Func> = module
                    .items
                    .iter()
                    .filter_map(|i| {
                        if let Item::Func(f) = i {
                            Some((f.name.clone(), f))
                        } else {
                            None
                        }
                    })
                    .collect();

                let mut new_items: Vec<Item> = Vec::new();

                for s in &struct_infos {
                    for (derive_name, derive_span) in &s.derives {
                        // D-METADERIVE1=A orphan rule: a UserDerive defined and applied
                        // in the same imported module violates the orphan rule (E2711).
                        // Expansion is only allowed in the entry module (idx == 0).
                        if idx > 0 && user_derives.iter().any(|(tn, _, _)| tn == derive_name) {
                            diags.push(Diagnostic::error(
                                "E2711",
                                format!(
                                    "derive orphan rule: `derive T.{}` and `{}` are both in an imported module",
                                    derive_name, s.name
                                ),
                                "user-derive expansion can only run in the entry module; both the derive block and the struct are from the same imported file".to_string(),
                                format!(
                                    "move both `derive {}` and `struct {}` to your entry module; derive expansion only runs there",
                                    derive_name, s.name
                                ),
                                None,
                            ));
                            continue;
                        }
                        if let Some((_, type_param, body)) =
                            user_derives.iter().find(|(tn, _, _)| tn == derive_name)
                        {
                            let type_info = crate::Comptime::build_struct_type_info(s);

                            match crate::Comptime::evaluate_derive_body(
                                body,
                                type_param,
                                type_info,
                                &actual_funcs,
                                &bundle.project_root,
                            ) {
                                Ok(fragments) => {
                                    for fragment in fragments {
                                        let (toks, lex_diags) = crate::Lexer::lex(&fragment);
                                        if !lex_diags.is_empty() {
                                            diags.extend(lex_diags);
                                            continue;
                                        }
                                        match crate::Parser::parse(&toks) {
                                            Ok(mut prog) => new_items.extend(prog.items.drain(..)),
                                            Err(parse_diags) => diags.extend(parse_diags),
                                        }
                                    }
                                }
                                // E2710: derive body failed at comptime. Wrap with context
                                // pointing at the #[TraitName] trigger on the struct.
                                Err(inner) => diags.push(Diagnostic::error(
                                    "E2710",
                                    format!(
                                        "`derive T.{}` body failed while expanding `#[{}]` on `{}`",
                                        derive_name, derive_name, s.name
                                    ),
                                    inner.what.clone(),
                                    "fix the `derive` body so it generates valid Jet at compile time".to_string(),
                                    Some(*derive_span),
                                )),
                            }
                        }
                    }
                }

                // Register new items before synthesis so they go through
                // the normal sema pipeline.
                for item in &new_items {
                    match item {
                        Item::Func(f) => register_func_item(f, st, &mut diags),
                        Item::Impl(i) => {
                            for m in &i.methods {
                                st.method_pub
                                    .insert((i.type_name.clone(), m.name.clone()), m.is_pub);
                            }
                        }
                        _ => {}
                    }
                }
                module.items.extend(new_items);
            }
        }

        // S62 + D-LIB2: synthesis must happen before register_impl_methods
        // so the synthesised Func nodes appear in the type registry.
        synthesize_impls(&mut module.items);
        register_type_methods(&module.items, &mut st.registry, &mut diags);
        register_patchable_methods(&module.items, &mut st.registry);
        register_impl_methods(&module.items, &mut st.registry, &mut diags);
        // D-TXN-ROLLBACK layer 2: ensure Rollback is known before user impl blocks.
        st.trait_reg.register_synthetic_rollback();
        st.trait_reg.register_synthetic_display_debug();
        st.trait_reg.register_synthetic_iter_index();
        st.trait_reg.register_items(&module.items, &mut diags);
        // D-SERDE: validate `@[Codable]`/`@[Encode]`/`@[Decode]` markers (E2407–E2412)
        // now that the trait registry resolves field/variant types — keeps the emitted
        // `impl`s rustc-clean (I2).
        diags.extend(validate_serde_items(&module.items, &st.trait_reg));
        // D-CLIFLAG1: validate `@[Cli]`-derived structs (E1305/E1306), same
        // timing as the serde pass above (trait registry must be built so
        // `Cli` is visible on `s.derives`).
        diags.extend(validate_cli_items(&module.items, &st.trait_reg));
        // D-MIGRATE1: schema diff pass (E0910) — runs after struct registration (I3).
        diags.extend(check_schema_migrations(&module.items, &bundle.project_root));
        // c129 (D-CAP4/D-CAP6/D-CAP8): capability-freeze drift pass (E0912). Runs
        // after `Capability::resolve_capabilities` (above) so it diffs the resolved
        // signature against the frozen `.api` contract. No-op without a frozen
        // snapshot (inferred-default library / first release).
        diags.extend(check_capability_freeze(&module.items, &bundle.project_root));
    }

    // S62 E2401: delegation validation — check field exists and implements trait.
    // Runs after all m9 registrations so implements_trait is populated.
    for (idx, module) in bundle.modules.iter().enumerate() {
        let st = &states[idx];
        for item in &module.items {
            if let Item::Impl(i) = item {
                if let (Some(trait_name), Some(field_name)) = (&i.trait_name, &i.delegation_field) {
                    if let Some(fields) = st.registry.struct_fields(&i.type_name) {
                        if let Some((_, _, field_ty, _, _)) =
                            fields.iter().find(|(n, _, _, _, _)| n == field_name)
                        {
                            let field_type_name = field_ty.name();
                            if !st.trait_reg.implements_trait(&field_type_name, trait_name) {
                                diags.push(Diagnostic::error(
                                    "E2401",
                                    format!(
                                        "`{}` doesn't implement `{}`, so it can't delegate",
                                        field_type_name, trait_name
                                    ),
                                    format!(
                                        "`impl {}.{} using {}` forwards `{}` methods to the `{}` field, but `{}` doesn't implement `{}`",
                                        i.type_name, trait_name, field_name,
                                        trait_name, field_name,
                                        field_type_name, trait_name
                                    ),
                                    format!(
                                        "implement `impl {}: {}` on the field's type, or choose a different field",
                                        field_type_name, trait_name
                                    ),
                                    Some(i.type_span),
                                ));
                            }
                        } else {
                            diags.push(Diagnostic::error(
                                "E2401",
                                format!("`{}` has no field `{}`", i.type_name, field_name),
                                format!(
                                    "`impl {}.{} using {}` needs `{}` to have a field named `{}`",
                                    i.type_name, trait_name, field_name, i.type_name, field_name
                                ),
                                format!("add `{}: Type` to `struct {}`", field_name, i.type_name),
                                Some(i.type_span),
                            ));
                        }
                    }
                }
            }
        }
    }

    // S57 (M9.5): evaluate comptime bindings per module. `embed_file` paths
    // resolve against each module file's own directory (S16 convention).
    // D-CTCORE1: pre-collect core_imports (alias→module) per module so the
    // comptime interpreter can evaluate whitelisted pure Core calls. Build a
    // SEPARATE local map — not `states[idx].core_imports` — so the duplicate
    // import check in the full import-resolution loop (below) is unaffected.
    let ct_core_imports: Vec<HashMap<String, String>> = bundle
        .modules
        .iter()
        .map(|module| {
            module
                .imports
                .iter()
                .filter_map(|imp| {
                    let path = imp.core_module_path()?;
                    let alias = imp.import_alias();
                    Some((alias, path))
                })
                .collect()
        })
        .collect();
    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        let base = module
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        eval_comptime_items(
            &mut module.items,
            &mut states[idx].consts,
            &base,
            &mut diags,
            &ct_core_imports[idx],
        );
    }

    // D-MOD3/4: Unqualified imports (`use alias.Item`) are processed in a
    // dedicated pass *after* file-module aliases land in `st.imports` below.

    for (idx, module) in bundle.modules.iter().enumerate() {
        let st = &mut states[idx];
        for imp in &module.imports {
            // Unqualified imports are handled in the dedicated pass below.
            if matches!(&imp.kind, ImportKind::Unqualified { .. }) {
                continue;
            }
            let alias = imp.import_alias();
            if st.imports.contains_key(&alias) {
                diags.push(Diagnostic::error(
                    "E0105",
                    format!("the import name `{}` is used twice", alias),
                    "each import needs a unique namespace name in this file".to_string(),
                    format!("rename one with `{} alias`", Syntax::KW_AS),
                    Some(imp.alias_span),
                ));
                continue;
            }
            if st.core_imports.contains_key(&alias) {
                diags.push(Diagnostic::error(
                    "E0105",
                    format!("the import name `{}` is used twice", alias),
                    "each import needs a unique namespace name in this file".to_string(),
                    format!("rename one with `{} alias`", Syntax::KW_AS),
                    Some(imp.alias_span),
                ));
                continue;
            }
            if let ImportKind::Module(name, _) = &imp.kind {
                if crate::Syntax::is_legacy_std_import(name) {
                    diags.push(Diagnostic::error(
                        "E0019",
                        format!("`{name}` is the old standard-library import spelling"),
                        "the standard library module was renamed to `core`".to_string(),
                        format!(
                            "use `import {}` or `import {}.fs as fs`",
                            Syntax::CORE_SHORT,
                            Syntax::CORE_SHORT
                        ),
                        Some(imp.span),
                    ));
                    continue;
                }
                // D-CORENS1 / E0341: old `jet.<ring>` spelling → teach the new `core.<ring>`.
                if let Some(ring) = name.strip_prefix("jet.") {
                    if crate::Syntax::is_ring_module(ring) {
                        diags.push(Diagnostic::error(
                            "E0341",
                            format!("`use jet.{ring}` is the old first-party library spelling"),
                            "first-party libraries moved to the `core.*` namespace (D-CORENS1)"
                                .to_string(),
                            format!("write `use core.{ring}` instead"),
                            Some(imp.span),
                        ));
                        continue;
                    }
                }
            }
            if let Some(module) = imp.core_module_path() {
                if !crate::Syntax::is_known_core_module(&module) {
                    diags.push(Diagnostic::error(
                        "E1001",
                        format!("there is no core module `{}`", module),
                        "`core` is compiler-known in M10, and only the frozen core modules exist"
                            .to_string(),
                        format!("import one of: {}", crate::Syntax::core_modules_list()),
                        Some(imp.span),
                    ));
                    continue;
                }
                // D-RINGLAYER1=A: infer minimum layer and enforce optional ceiling.
                if let Some(mod_layer) = crate::Syntax::core_module_layer(&module) {
                    if let Some(ceiling) = bundle.layer_ceiling {
                        if mod_layer > ceiling {
                            diags.push(crate::Syntax::layer_ceiling_exceeded(
                                &module,
                                mod_layer,
                                ceiling,
                                Some(imp.span),
                                Some(&format!("`use {module}`")),
                            ));
                            continue;
                        }
                    }
                    if mod_layer > bundle.inferred_layer {
                        bundle.inferred_layer = mod_layer;
                    }
                }
                st.core_imports.insert(alias, module);
                continue;
            }
            // S59 (E2-M14): C `use` forms bind to a synthetic merged module
            // resolved by `CFFI::assemble` (E3204 already reported there).
            if imp.is_c_import() {
                if let Some(target) = bundle.cffi.target_for(idx, &alias) {
                    st.imports.insert(alias, target);
                }
                continue;
            }
            if let Some(target) = bundle.import_targets.get(&(idx, imp.span)).copied() {
                st.imports.insert(alias, target);
            }
        }
    }

    // D-MOD3/4: process `use alias.Item` unqualified imports now that file-module
    // aliases are registered in `st.imports`. `pub use` additionally re-exports the
    // item onto this module's public surface (`reexports`).
    for (idx, module) in bundle.modules.iter().enumerate() {
        for imp in &module.imports {
            let ImportKind::Unqualified {
                module_alias,
                module_alias_span,
                items,
                ..
            } = &imp.kind
            else {
                continue;
            };
            let st = &mut states[idx];
            if st.code_modules.contains_key(module_alias.as_str()) {
                // Inline module: items are mangled as `{alias}__{item}`.
                for (orig, alias_opt) in items {
                    let local = alias_opt.as_deref().unwrap_or(orig.as_str());
                    let mangled = format!("{}__{}", module_alias, orig);
                    if !st.funcs.contains_key(&mangled) {
                        diags.push(Diagnostic::error(
                            "E0611",
                            format!("`{}` is not defined in module `{}`", orig, module_alias),
                            "check the module body for the item you're importing".to_string(),
                            "make sure the name is spelled correctly".to_string(),
                            Some(*module_alias_span),
                        ));
                    } else if !st.func_pub.get(&mangled).copied().unwrap_or(false)
                        && !st.func_pkg_pub.get(&mangled).copied().unwrap_or(false)
                    {
                        diags.push(Diagnostic::error(
                            "E0609",
                            format!("`{}` is private in module `{}`", orig, module_alias),
                            "only `pub` items can be brought into scope with `use`".to_string(),
                            format!(
                                "add `pub` before `fn {}` in module `{}`",
                                orig, module_alias
                            ),
                            Some(*module_alias_span),
                        ));
                    } else {
                        st.unqualified.insert(local.to_string(), mangled.clone());
                        if imp.is_pub {
                            st.reexports.insert(local.to_string(), (mangled, idx));
                        }
                    }
                }
            } else if module_alias == "core" || module_alias == "jet" {
                // Std namespace prefix: `use core.mem` → bind each item as a Core import.
                // Each item `x` becomes `core.x` in the known-modules table.
                let st = &mut states[idx];
                for (orig, alias_opt) in items {
                    let local = alias_opt.as_deref().unwrap_or(orig.as_str());
                    let full = format!("core.{}", orig);
                    if !crate::Syntax::is_known_core_module(&full) {
                        diags.push(Diagnostic::error(
                            "E1001",
                            format!("there is no core module `{}`", full),
                            "`core` is compiler-known in M10, and only the frozen core modules exist".to_string(),
                            format!("import one of: {}", crate::Syntax::core_modules_list()),
                            Some(*module_alias_span),
                        ));
                    } else if st.core_imports.contains_key(local) {
                        diags.push(Diagnostic::error(
                            "E0105",
                            format!("the import name `{}` is used twice", local),
                            "each import needs a unique namespace name in this file".to_string(),
                            format!("rename one with `{} alias`", Syntax::KW_AS),
                            Some(imp.alias_span),
                        ));
                    } else {
                        // D-RINGLAYER1=A M2: unqualified `use core.X` obeys the same layer rules.
                        if let Some(mod_layer) = crate::Syntax::core_module_layer(&full) {
                            if let Some(ceiling) = bundle.layer_ceiling {
                                if mod_layer > ceiling {
                                    diags.push(crate::Syntax::layer_ceiling_exceeded(
                                        &full,
                                        mod_layer,
                                        ceiling,
                                        Some(*module_alias_span),
                                        Some(&format!("`use core.{orig}`")),
                                    ));
                                    continue;
                                }
                            }
                            if mod_layer > bundle.inferred_layer {
                                bundle.inferred_layer = mod_layer;
                            }
                        }
                        st.core_imports.insert(local.to_string(), full);
                    }
                }
            } else if st.imports.contains_key(module_alias.as_str()) {
                // File module: look up items in the target module's state.
                let target_idx = st.imports[module_alias.as_str()];
                let is_reexport = imp.is_pub;
                for (orig, alias_opt) in items {
                    let local = alias_opt.as_deref().unwrap_or(orig.as_str());
                    let same_pkg = states[target_idx].package_scope == states[idx].package_scope;
                    let is_pub = states[target_idx]
                        .func_pub
                        .get(orig.as_str())
                        .copied()
                        .unwrap_or(false)
                        || (same_pkg
                            && states[target_idx]
                                .func_pkg_pub
                                .get(orig.as_str())
                                .copied()
                                .unwrap_or(false));
                    let exists = states[target_idx].funcs.contains_key(orig.as_str());
                    if !exists {
                        diags.push(Diagnostic::error(
                            "E0611",
                            format!("`{}` is not defined in module `{}`", orig, module_alias),
                            "check the module for the item you're importing".to_string(),
                            "make sure the name is spelled correctly".to_string(),
                            Some(*module_alias_span),
                        ));
                    } else if !is_pub {
                        diags.push(Diagnostic::error(
                            "E0609",
                            format!("`{}` is private in module `{}`", orig, module_alias),
                            "only `pub` items can be brought into scope with `use`".to_string(),
                            format!("add `pub` before `fn {}` in the imported file", orig),
                            Some(*module_alias_span),
                        ));
                    } else {
                        states[idx]
                            .unqualified_file
                            .insert(local.to_string(), (orig.clone(), target_idx));
                        if is_reexport {
                            states[idx]
                                .reexports
                                .insert(local.to_string(), (orig.clone(), target_idx));
                        }
                    }
                }
            } else {
                // Module alias not found — E0610.
                diags.push(Diagnostic::error(
                    "E0610",
                    format!("no module named `{}` in scope", module_alias),
                    "the alias must refer to a module imported earlier in this file".to_string(),
                    format!("add `import … as {}`  before this `use`", module_alias),
                    Some(*module_alias_span),
                ));
            }
        }
    }

    for idx in 0..bundle.modules.len() {
        for item in &bundle.modules[idx].items {
            let Item::Impl(i) = item else { continue };
            if !i.type_name.contains('.') {
                continue;
            }
            if !impl_type_exists(
                &i.type_name,
                &states[idx].registry,
                &states[idx].imports,
                Some(&states),
            ) {
                diags.push(Diagnostic::error(
                    "E0301",
                    format!("`impl {}` names a type that doesn't exist", i.type_name),
                    format!("`{}` hasn't been defined as a struct or enum", i.type_name),
                    format!(
                        "define `struct {}` or `enum {}` first",
                        i.type_name, i.type_name
                    ),
                    Some(i.type_span),
                ));
            } else {
                for m in &i.methods {
                    states[idx]
                        .method_pub
                        .insert((i.type_name.clone(), m.name.clone()), m.is_pub);
                }
            }
        }
    }

    // Parity with the single-file path: `@static` and address-taken consts
    // must lower to Rust `static` in bundle mode too.
    for module in bundle.modules.iter_mut() {
        let const_names: Vec<String> = module
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Const(c) => Some(c.name.clone()),
                _ => None,
            })
            .collect();
        let mut address_taken: HashSet<String> = HashSet::new();
        for item in &module.items {
            match item {
                Item::Func(f) => {
                    walk_stmts_for_const_refs(&f.body, &const_names, &mut address_taken)
                }
                Item::Struct(s) => {
                    for m in &s.methods {
                        walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                    }
                }
                Item::Enum(e) => {
                    for m in &e.methods {
                        walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                    }
                }
                Item::Impl(i) => {
                    for m in &i.methods {
                        walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                    }
                }
                Item::Test(t) => {
                    walk_stmts_for_const_refs(&t.body, &const_names, &mut address_taken)
                }
                Item::Bench(b) => {
                    walk_stmts_for_const_refs(&b.body, &const_names, &mut address_taken)
                }
                Item::Const(_)
            | Item::ExternRust(_)
            | Item::Trait(_)
            | Item::Tag(_) // D-QUAL2: tags erase
            | Item::Module(_)
            | Item::Distinct(_)
            | Item::TypeAlias(_) // D-TYPEALIAS1: erases
            | Item::UnitFamily(_) // D-QUAL3: lowered to distinct types
            | Item::CModule(_) | Item::CodeModule(_)
            | Item::ErrorConv(_)
            | Item::Migration(_) // D-MIGRATE1
            | Item::StateDecl(_) // D-STATE-DECL: erases
            | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2: erases
            | Item::UserDerive(_) // D-METADERIVE1=A: already expanded above
            | Item::GenericModule(_) // D-GENMOD2=A: template — erases
            | Item::ModuleAlias(_) => {} // D-GENMOD2=A: alias — erases after expansion
            }
        }
        for item in &mut module.items {
            if let Item::Const(c) = item {
                let force_static = c.attrs.contains(&ConstAttr::ForceStatic);
                c.rust_kind = if force_static || address_taken.contains(&c.name) {
                    RustConstKind::Static
                } else {
                    RustConstKind::Const
                };
            }
        }
    }

    // Each non-entry module becomes a Rust `mod user_<alias>`; a type in the
    // entry file with the same name would collide in the type namespace.
    for (idx, m) in bundle.modules.iter().enumerate() {
        if idx == bundle.entry {
            continue;
        }
        if states[bundle.entry].registry.contains(&m.alias) {
            diags.push(Diagnostic::error(
                "E0105",
                format!(
                    "the type `{}` clashes with the imported file `{}`",
                    m.alias, m.display
                ),
                "a type and an imported module can't share a name".to_string(),
                format!(
                    "rename the type, or import with `{} other_name`",
                    Syntax::KW_AS
                ),
                None,
            ));
        }
    }

    let entry = &states[bundle.entry];
    if mode == CompileMode::Run || mode == CompileMode::Eval {
        let entry_items = &bundle.modules[bundle.entry].items;
        if let Some(run_fn) = entry_items.iter().find_map(|i| match i {
            Item::Func(f) if f.name == "run" => Some(f),
            _ => None,
        }) {
            // S12/D-CLIFLAG1: `run` is the only program entry name. It is either
            // zero-arg, or one typed CLI-spec parameter (`@[Cli]` struct / enum).
            if run_fn.params.is_empty() {
                if mode == CompileMode::Run && run_fn.return_type.is_some() {
                    diags.push(Diagnostic::error(
                        "E0122",
                        "`run` returns a value".to_string(),
                        "`run` is where running starts; in run mode there is no caller waiting for a value"
                            .to_string(),
                        "write it as: fn run() { ... }".to_string(),
                        Some(run_fn.name_span),
                    ));
                }
            } else if run_fn.params.len() == 1 {
                let param = &run_fn.params[0];
                match cli_entry_param_shape(entry_items, &param.ty, &entry.trait_reg) {
                    CliEntryShape::Struct | CliEntryShape::Enum => {}
                    CliEntryShape::EnumBadVariants(bad) => diags.extend(bad),
                    CliEntryShape::Invalid => diags.push(e1308(Some(param.ty_span))),
                }
            } else {
                diags.push(e1308(Some(run_fn.name_span)));
            }
        } else {
            diags.push(no_run_error());
        }
    }
    match mode {
        CompileMode::Test if entry.tests.is_empty() => {
            diags.push(Diagnostic::error(
                "E0601",
                format!("no `#{}` blocks found to run", Syntax::KW_TEST),
                format!(
                    "add at least one top-level block: #{} \"describes what this checks\" {{ ... }}",
                    Syntax::KW_TEST
                ),
                format!(
                    "use `{}` and `{}` inside the block to check results",
                    Syntax::BUILTIN_REQUIRE,
                    Syntax::BUILTIN_REQUIRE_EQ
                ),
                None,
            ));
        }
        // `jet bench` checks the AST for `#Bench` blocks before entering Bench
        // mode and falls back to whole-program timing otherwise, so an empty
        // bench set is never an error here.
        CompileMode::Bench
        | CompileMode::Test
        | CompileMode::Run
        | CompileMode::Check
        | CompileMode::Eval => {}
    }

    // D-EFF1: collect effect summaries across every module, then run the
    // whole-program fixpoint and enforce each `#(…)` bound once.
    // D-CTEFFECT1 Tier-1: accumulate embed inputs from all module checks.
    // Use a temporary to avoid simultaneous &mut borrows of `bundle`.
    let mut embed_inputs = std::mem::take(&mut bundle.comptime_inputs);
    let mut effect_summaries: HashMap<String, EffectSummary> = HashMap::new();
    // D-METHODMACRO1=A: top-level function names whose address was taken
    // anywhere in the bundle, accumulated across every module below; the
    // `@InlineAlways` address-taken pass (E0918) runs after the loop, once
    // this set is complete across the whole bundle.
    let mut global_addr_taken: HashSet<String> = HashSet::new();
    // D-EXPANDCLI1 (card #183): resolved `&T` stored-ref owner facts,
    // accumulated across every module for `jet expand --facts refs`.
    let mut ref_facts: Vec<Facts::RefFact> = Vec::new();
    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        diags.extend(check_module_bodies(
            module,
            idx,
            &states,
            mode,
            freestanding,
            allow_impure,
            &mut effect_summaries,
            &mut embed_inputs,
            &mut global_addr_taken,
            &mut ref_facts,
        ));
    }
    bundle.comptime_inputs = embed_inputs;
    // D-METHODMACRO1=A: E0918 (address-taken) needs every module's function
    // bodies checked first. Methods can't appear in `global_addr_taken`
    // (Jet's grammar has no way to read a method's bare name as a value), so
    // this only ever fires for top-level functions.
    for module in &bundle.modules {
        for item in &module.items {
            if let Item::Func(f) = item {
                if f.is_inline_always && global_addr_taken.contains(&f.name) {
                    diags.push(e0918_address_taken(
                        &f.name,
                        f.inline_span.unwrap_or(f.name_span),
                    ));
                }
            }
        }
    }
    // D-EFF2 (`#(via f)`): seed each via-fn's summary with its callback's bound
    // before the fixpoint, so its published effect set is a tight pass-through.
    for module in &bundle.modules {
        apply_effect_via(&module.items, &mut effect_summaries, &mut diags);
    }
    let solved = solve(&effect_summaries);
    for module in &bundle.modules {
        check_effect_boundaries(&module.items, &solved, &mut diags);
    }
    check_region_caps(&effect_summaries, &solved, &mut diags);
    // D-EFF2: callback param effect bounds (E0747).
    check_callback_bounds(&effect_summaries, &solved, &mut diags);

    // D-WASM1=A (c123 M1): JS/WASM partition inference and boundary checks.
    diags.extend(check_web_partition(bundle, &effect_summaries, &solved));

    // D-OSTARGET1=A (ratified 2026-07-01, c134): native OS platform gating —
    // mixed-axis conflicts and unmatched cross-gate calls.
    diags.extend(check_os_target(bundle));

    // D-TAINT1: taint tracking across every module. `#Sanitizer fn`s are
    // collected program-wide (a sanitizer in one module clears taint at a call in
    // another); each module's bodies are checked against its own Core aliases so
    // a sink call (Db/Exec/Net effect) resolves correctly. Erased in codegen (I3).
    let mut sanitizers: std::collections::HashSet<String> = std::collections::HashSet::new();
    for module in &bundle.modules {
        collect_sanitizers(&module.items, &mut sanitizers);
    }
    for (idx, module) in bundle.modules.iter().enumerate() {
        let core_imports = &states[idx].core_imports;
        for item in &module.items {
            taint_check_item(item, &sanitizers, core_imports, &mut diags);
        }
    }

    // D-STATE1 / D-STATE-DECL: typestate across the whole bundle. State-set
    // declarations are collected program-wide, then declarations validated (E0151,
    // L0151) and per-body forward dataflow checked (E0150). Erased in codegen (I3).
    let mut state_tbl = crate::Sema::StateTable::default();
    for module in &bundle.modules {
        state_tbl.add_items(&module.items);
    }
    if !state_tbl.is_empty() {
        for module in &bundle.modules {
            state_tbl.validate_declarations(&module.items, &mut diags);
            crate::Sema::check_items_state(&module.items, &state_tbl, &mut diags);
        }
    }

    let (mut used_core, usage_spans) = collect_used_core(bundle, &states);
    // D-CLIFLAG1: a `@[Cli]`-derived struct's generated `__jet_cli_spec_*`/
    // `__jet_cli_decode_*` functions (and the synthesized `fn main` for a
    // typed `fn run`) call straight into `core.args`'s `JetArgsSpec`/
    // `JetParsedArgs` prelude — but they're pure codegen text, not a Jet
    // method call `collect_used_core` can see by walking function bodies.
    // Force the same `CORELIB_PRELUDE` inclusion a hand-written
    // `use core.args` would trigger (any key works; the caller only checks
    // "is this set empty").
    if bundle
        .modules
        .iter()
        .any(|m| m.items.iter().any(|i| matches!(i, Item::Struct(s) if s.derives.iter().any(|(t, _)| t == "Cli"))))
    {
        used_core.insert("core.args::spec".to_string());
    }
    bundle.used_core = used_core;
    apply_helper_layer_inference(bundle, &states, &usage_spans, &mut diags);
    (
        diags,
        super::Effects::SemIndexEffectFacts {
            summaries: effect_summaries,
            solved,
            refs: ref_facts,
        },
    )
}

/// D-TAINT1: run the taint pass over one item's function/method bodies in the
/// bundle path, using `core_imports` to classify sink calls.
fn taint_check_item(
    item: &Item,
    sanitizers: &std::collections::HashSet<String>,
    core_imports: &HashMap<String, String>,
    diags: &mut Vec<Diagnostic>,
) {
    match item {
        Item::Func(f) => diags.extend(check_func_taint(&f.body, sanitizers, core_imports)),
        Item::Impl(i) => {
            for m in &i.methods {
                diags.extend(check_func_taint(&m.body, sanitizers, core_imports));
            }
        }
        Item::Struct(s) => {
            for m in &s.methods {
                diags.extend(check_func_taint(&m.body, sanitizers, core_imports));
            }
            for block in &s.trait_impls {
                for m in &block.methods {
                    diags.extend(check_func_taint(&m.body, sanitizers, core_imports));
                }
            }
        }
        Item::Enum(e) => {
            for m in &e.methods {
                diags.extend(check_func_taint(&m.body, sanitizers, core_imports));
            }
        }
        Item::Test(t) => diags.extend(check_func_taint(&t.body, sanitizers, core_imports)),
        Item::ErrorConv(ec) => diags.extend(check_func_taint(&ec.body, sanitizers, core_imports)),
        _ => {}
    }
}

pub(crate) fn register_func_item(f: &Func, st: &mut ModuleState, diags: &mut Vec<Diagnostic>) {
    if f.name == Syntax::BUILTIN_PRINT
        || f.name == Syntax::BUILTIN_PANIC
        || f.name == Syntax::BUILTIN_REQUIRE
        || f.name == Syntax::BUILTIN_REQUIRE_EQ
    {
        diags.push(Diagnostic::error(
            "E0106",
            format!("the name `{}` is built in and can't be redefined", f.name),
            format!("`{}` is provided by the language itself", f.name),
            "choose a different name for this function".to_string(),
            Some(f.name_span),
        ));
        return;
    }
    if name_defined(&f.name, &st.funcs, &st.registry, &st.consts) {
        diags.push(Diagnostic::error(
            "E0105",
            format!("`{}` is defined twice", f.name),
            "every function needs a unique name so calls aren't ambiguous".to_string(),
            "rename or remove one of the definitions".to_string(),
            Some(f.name_span),
        ));
        return;
    }
    // L2401: advisory — public fn with a positional Bool parameter.
    if f.is_pub {
        for p in &f.params {
            if matches!(p.ty, Type::Bool) && p.name != Syntax::KW_SELF && p.default.is_none() {
                diags.push(Diagnostic::lint(
                    "L2401",
                    format!(
                        "public function `{}` has a positional `Bool` parameter `{}`",
                        f.name, p.name
                    ),
                    "positional booleans are easy to transpose at the call site".to_string(),
                    format!(
                        "callers can write `{}: true` to make the intent clear (S61 labels)",
                        p.name
                    ),
                    Some(p.name_span),
                ));
            }
        }
    }
    // D-NARG-D2 (E0126): check defaults don't ref later params.
    check_default_forward_refs(&f.params, &f.name, diags);
    st.func_pub
        .insert(f.name.clone(), f.is_pub && !f.is_package_pub);
    st.func_pkg_pub.insert(f.name.clone(), f.is_package_pub);
    st.funcs.insert(f.name.clone(), func_to_sig(f));
}

pub(crate) fn collect_used_core(
    bundle: &ProgramBundle,
    states: &[ModuleState],
) -> (HashSet<String>, HashMap<String, crate::Diagnostics::Span>) {
    let mut used = HashSet::new();
    let mut spans = HashMap::new();
    for (idx, module) in bundle.modules.iter().enumerate() {
        let imports = &states[idx].core_imports;
        for item in &module.items {
            match item {
                Item::Func(f) => collect_core_stmts(&f.body, imports, &mut used, &mut spans),
                Item::Struct(s) => {
                    for m in &s.methods {
                        collect_core_stmts(&m.body, imports, &mut used, &mut spans);
                    }
                }
                Item::Enum(e) => {
                    for m in &e.methods {
                        collect_core_stmts(&m.body, imports, &mut used, &mut spans);
                    }
                }
                Item::Impl(i) => {
                    for m in &i.methods {
                        collect_core_stmts(&m.body, imports, &mut used, &mut spans);
                    }
                }
                Item::Test(t) => collect_core_stmts(&t.body, imports, &mut used, &mut spans),
                Item::Bench(b) => collect_core_stmts(&b.body, imports, &mut used, &mut spans),
                Item::Const(c) => collect_core_expr(&c.value, imports, &mut used, &mut spans),
                Item::Trait(_)
                | Item::Tag(_) // D-QUAL2: tags use no core imports
                | Item::ExternRust(_)
                | Item::Module(_)
                | Item::Distinct(_)
                | Item::TypeAlias(_) // D-TYPEALIAS1: erases
                | Item::UnitFamily(_) // D-QUAL3: lowered to distinct types
                | Item::CModule(_) | Item::CodeModule(_)
                | Item::ErrorConv(_)
                | Item::Migration(_) // D-MIGRATE1
                | Item::StateDecl(_) // D-STATE-DECL: uses no core imports
                | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2: erases
            | Item::UserDerive(_) // D-METADERIVE1=A: already expanded
            | Item::GenericModule(_) // D-GENMOD2=A: template — erases
            | Item::ModuleAlias(_) => {} // D-GENMOD2=A: alias — erases after expansion
            }
        }
    }
    (used, spans)
}

fn note_core_usage(
    used: &mut HashSet<String>,
    spans: &mut HashMap<String, crate::Diagnostics::Span>,
    key: impl Into<String>,
    span: Option<crate::Diagnostics::Span>,
) {
    let key = key.into();
    used.insert(key.clone());
    if let Some(s) = span {
        spans.entry(key).or_insert(s);
    }
}

/// D-RINGLAYER1=A M2: bump inferred layer from emitted helper usage and enforce ceiling.
fn apply_helper_layer_inference(
    bundle: &mut ProgramBundle,
    states: &[ModuleState],
    usage_spans: &HashMap<String, crate::Diagnostics::Span>,
    diags: &mut Vec<Diagnostic>,
) {
    let core_imports: HashMap<String, String> = states
        .iter()
        .flat_map(|st| st.core_imports.iter().map(|(a, m)| (a.clone(), m.clone())))
        .collect();
    for usage in &bundle.used_core {
        let Some(mod_layer) = crate::Syntax::core_usage_layer(usage) else {
            continue;
        };
        if mod_layer > bundle.inferred_layer {
            bundle.inferred_layer = mod_layer;
        }
        let Some(ceiling) = bundle.layer_ceiling else {
            continue;
        };
        if mod_layer <= ceiling {
            continue;
        }
        let span = usage_spans.get(usage).copied();
        let chain = helper_import_chain(usage, &core_imports);
        diags.push(crate::Syntax::layer_ceiling_exceeded(
            usage,
            mod_layer,
            ceiling,
            span,
            Some(&chain),
        ));
    }
}

fn helper_import_chain(usage: &str, core_imports: &HashMap<String, String>) -> String {
    if usage == "core.io::input" {
        return format!("ambient `input()` (helper `{usage}`)");
    }
    if let Some((module, _)) = usage.split_once("::") {
        if let Some((alias, imported)) = core_imports.iter().find(|(_, m)| m.as_str() == module) {
            return format!("`use {imported} as {alias}` → `{usage}`");
        }
        return format!("prelude helper `{usage}`");
    }
    format!("prelude helper `{usage}`")
}

pub(crate) fn collect_core_stmts(
    stmts: &[Stmt],
    imports: &HashMap<String, String>,
    used: &mut HashSet<String>,
    spans: &mut HashMap<String, crate::Diagnostics::Span>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(e) | Stmt::Yield(e, _) => collect_core_expr(e, imports, used, spans),
            Stmt::Val(b) => collect_core_expr(&b.init, imports, used, spans),
            Stmt::Assign { target, value, .. } => {
                collect_core_lvalue(target, imports, used, spans);
                collect_core_expr(value, imports, used, spans);
            }
            Stmt::Return(Some(e), _) => collect_core_expr(e, imports, used, spans),
            Stmt::Return(None, _) => {}
            Stmt::If(ifs) => collect_core_if(ifs, imports, used, spans),
            Stmt::While { cond, body, .. } => {
                collect_core_expr(cond, imports, used, spans);
                collect_core_stmts(body, imports, used, spans);
            }
            Stmt::For { kind, body, .. } => {
                match kind {
                    ForKind::Range { start, end, step } => {
                        collect_core_expr(start, imports, used, spans);
                        collect_core_expr(end, imports, used, spans);
                        if let Some(step) = step {
                            collect_core_expr(step, imports, used, spans);
                        }
                    }
                    ForKind::In { collection } => {
                        collect_core_expr(collection, imports, used, spans)
                    }
                }
                collect_core_stmts(body, imports, used, spans);
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
                collect_core_expr(subject, imports, used, spans);
                for arm in arms {
                    collect_core_expr(&arm.cond, imports, used, spans);
                    collect_core_stmts(&arm.body, imports, used, spans);
                }
                if let Some(body) = else_body {
                    collect_core_stmts(body, imports, used, spans);
                }
            }
            Stmt::CountedLoop {
                init,
                cond,
                step,
                body,
                ..
            } => {
                collect_core_expr(&init.init, imports, used, spans);
                collect_core_expr(cond, imports, used, spans);
                collect_core_stmts(body, imports, used, spans);
                collect_core_stmts(std::slice::from_ref(step.as_ref()), imports, used, spans);
            }
            Stmt::Loop { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::SuppressMustUse { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::AssumeDet { body, .. } => collect_core_stmts(body, imports, used, spans),
            // D-REACTCORE1: reactive blocks implicitly use `core.reactive`.
            Stmt::Reactive { body, span, .. } => {
                note_core_usage(used, spans, "core.reactive", Some(*span));
                collect_core_stmts(body, imports, used, spans);
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {
            }
            // D-CTMARKER1: collect Core usage from comptime block body.
            Stmt::ComptimeBlock { body, .. } => collect_core_stmts(body, imports, used, spans),
            // D-WHEN1: collect Core usage from both arms (we don't know which is
            // selected until sema runs; over-collecting is harmless here).
            Stmt::ComptimeIf {
                cond,
                then_body,
                else_body,
                ..
            } => {
                collect_core_expr(cond, imports, used, spans);
                collect_core_stmts(then_body, imports, used, spans);
                if let Some(eb) = else_body {
                    collect_core_stmts(eb, imports, used, spans);
                }
            }
            // D-CTX1: collect Core usage from context block fields and body.
            Stmt::ContextBlock { fields, body, .. } => {
                for (_, e, _) in fields {
                    collect_core_expr(e, imports, used, spans);
                }
                collect_core_stmts(body, imports, used, spans);
            }
            // D-TERM1 (ratified 2026-06-22): collect Core usage from live block body.
            // The live block implicitly uses `core.term` (jet_term_enter/leave), so
            // we mark it as used here.
            Stmt::Live { body, span, .. } => {
                note_core_usage(used, spans, "core.term", Some(*span));
                collect_core_stmts(body, imports, used, spans);
            }
            // D-DOTSCOPE1: collect core usage in a scope-member region body.
            Stmt::ScopeMember { body, .. } => {
                collect_core_stmts(body, imports, used, spans);
            }
        }
    }
}

pub(crate) fn collect_core_if(
    ifs: &IfStmt,
    imports: &HashMap<String, String>,
    used: &mut HashSet<String>,
    spans: &mut HashMap<String, crate::Diagnostics::Span>,
) {
    collect_core_expr(&ifs.cond, imports, used, spans);
    collect_core_stmts(&ifs.then_body, imports, used, spans);
    match &ifs.else_branch {
        Some(ElseBranch::Else(body)) => collect_core_stmts(body, imports, used, spans),
        Some(ElseBranch::ElseIf(next)) => collect_core_if(next, imports, used, spans),
        None => {}
    }
}

pub(crate) fn collect_core_lvalue(
    lv: &LValue,
    imports: &HashMap<String, String>,
    used: &mut HashSet<String>,
    spans: &mut HashMap<String, crate::Diagnostics::Span>,
) {
    match lv {
        LValue::Local { .. } => {}
        LValue::Index { base, index, .. } => {
            collect_core_expr(base, imports, used, spans);
            collect_core_expr(index, imports, used, spans);
        }
        // D-MUTSELF1: `place.field = v` — the base place may use a core import.
        LValue::Field { base, .. } => collect_core_expr(base, imports, used, spans),
    }
}

pub(crate) fn collect_core_expr(
    expr: &Expr,
    imports: &HashMap<String, String>,
    used: &mut HashSet<String>,
    spans: &mut HashMap<String, crate::Diagnostics::Span>,
) {
    // D-SIMD2 / D-LINALG1: a built-in math type used anywhere (constructor, static
    // method, or instance method on a math-typed receiver-by-name) pulls in the
    // CoreLib prelude that defines the `jet_math_*` helpers. Detect it syntactically;
    // a math-type *constructor* call and a static `T.method(...)` both surface the
    // type NAME, which is enough to require the prelude.
    match expr {
        Expr::Call(c) if is_math_type(&c.name) => {
            note_core_usage(used, spans, "core.math::__mathtypes__", Some(c.name_span));
        }
        Expr::Call(c)
            if c.name == crate::Syntax::TYPE_BIGINT || c.name == crate::Syntax::TYPE_DECIMAL =>
        {
            note_core_usage(used, spans, "core.numeric::__precise__", Some(c.name_span));
        }
        Expr::MethodCall {
            receiver,
            method_span,
            ..
        } => {
            if let Expr::Ident(n, _) = receiver.as_ref() {
                if is_math_type(n) {
                    note_core_usage(used, spans, "core.math::__mathtypes__", Some(*method_span));
                }
                // D-PATHFS1: `Path.from(...)` or any Path static call triggers path prelude.
                if n == "Path" {
                    note_core_usage(used, spans, "core.path::__pathapi__", Some(*method_span));
                }
            }
        }
        _ => {}
    }
    match expr {
        Expr::PtrFromAddr { addr, .. } => collect_core_expr(addr, imports, used, spans),
        Expr::MethodCall {
            receiver,
            method,
            method_span,
            args,
            recv_type,
            ..
        } => {
            if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
                && method == Syntax::TASKGROUP_SPAWN_METHOD
            {
                note_core_usage(
                    used,
                    spans,
                    "core.tasks::spawn",
                    Some(*method_span),
                );
            }
            if matches!(
                recv_type.as_deref(),
                Some(crate::Syntax::TYPE_BIGINT) | Some(crate::Syntax::TYPE_DECIMAL)
            ) {
                note_core_usage(
                    used,
                    spans,
                    "core.numeric::__precise__",
                    Some(*method_span),
                );
            }
            if matches!(receiver.as_ref(), Expr::Ident(n, _) if is_json_type_name(n)) {
                note_core_usage(used, spans, "core::json", Some(*method_span));
            }
            if matches!(
                method.as_str(),
                "bytes" | "from_bytes" | "to_u8" | "elapsed_millis"
            ) {
                note_core_usage(used, spans, format!("core::{method}"), Some(*method_span));
            }
            if let Expr::Ident(alias, _) = receiver.as_ref() {
                if let Some(module) = imports.get(alias) {
                    note_core_usage(
                        used,
                        spans,
                        format!("{module}::{method}"),
                        Some(*method_span),
                    );
                }
            }
            // D-ENC1: nested-namespace core call `<alias>.<leaf>.method(...)` (e.g.
            // `encoding.json.to_string(x)`). Record `<ns>.<leaf>::method` so the CoreLib
            // prelude is emitted and the backing helper is in scope.
            if let Expr::Field(base, leaf, _) = receiver.as_ref() {
                if let Expr::Ident(alias, _) = base.as_ref() {
                    if let Some(ns) = imports.get(alias) {
                        let submodule = format!("{ns}.{leaf}");
                        if crate::Syntax::is_known_core_module(&submodule) {
                            note_core_usage(
                                used,
                                spans,
                                format!("{submodule}::{method}"),
                                Some(*method_span),
                            );
                        }
                    }
                }
            }
            collect_core_expr(receiver, imports, used, spans);
            for arg in args {
                collect_core_expr(&arg.expr, imports, used, spans);
            }
        }
        Expr::Call(c) => {
            // D-PRELUDE1 = B: bare `input(...)` is prelude-ambient; mark core.io so
            // CORELIB_PRELUDE is emitted and jet_std_io_input is in scope for codegen.
            if c.name == Syntax::BUILTIN_INPUT {
                note_core_usage(used, spans, "core.io::input", Some(c.name_span));
            }
            for arg in &c.args {
                collect_core_expr(&arg.expr, imports, used, spans);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            collect_core_expr(callee, imports, used, spans);
            for arg in args {
                collect_core_expr(&arg.expr, imports, used, spans);
            }
        }
        Expr::Field(inner, member, span) => {
            if matches!(inner.as_ref(), Expr::Ident(n, _) if is_json_type_name(n))
                && member == "Null"
            {
                note_core_usage(used, spans, "core::json", Some(*span));
            }
            collect_core_expr(inner, imports, used, spans);
        }
        Expr::OptField { base, .. } => collect_core_expr(base, imports, used, spans),
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Tainted(inner, _) // D-TAINT1: tag erased; recurse into the value.
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _) => collect_core_expr(inner, imports, used, spans),
        Expr::Binary(_, lhs, rhs, _)
        | Expr::Index {
            base: lhs,
            index: rhs,
            ..
        } => {
            collect_core_expr(lhs, imports, used, spans);
            collect_core_expr(rhs, imports, used, spans);
        }
        Expr::CompareChain { operands, .. } => {
            for e in operands.iter() {
                collect_core_expr(e, imports, used, spans);
            }
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            collect_core_expr(base, imports, used, spans);
            collect_core_expr(start, imports, used, spans);
            collect_core_expr(end, imports, used, spans);
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let StrPart::Interp(e, _) = part {
                    collect_core_expr(e, imports, used, spans);
                }
            }
        }
        Expr::ListLit(items, _) => {
            for e in items {
                collect_core_expr(e, imports, used, spans);
            }
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, e) in fields {
                collect_core_expr(e, imports, used, spans);
            }
        }
        Expr::MapLit(items, _) => {
            for (k, v) in items {
                collect_core_expr(k, imports, used, spans);
                collect_core_expr(v, imports, used, spans);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, e) in fields {
                collect_core_expr(e, imports, used, spans);
            }
        }
        Expr::EnumLit { args, .. } => {
            for arg in args {
                match arg {
                    EnumLitArg::Positional(e) => collect_core_expr(e, imports, used, spans),
                    EnumLitArg::Named { expr, .. } => collect_core_expr(expr, imports, used, spans),
                }
            }
        }
        Expr::PatternTest { subject, .. } => collect_core_expr(subject, imports, used, spans),
        Expr::OrFallback {
            value, fallback, ..
        } => {
            collect_core_expr(value, imports, used, spans);
            match fallback {
                OrFallback::Value(e) => collect_core_expr(e, imports, used, spans),
                OrFallback::Return(Some(e), _) => collect_core_expr(e, imports, used, spans),
                OrFallback::Return(None, _) => {}
                OrFallback::Panic { args, .. } => {
                    for arg in args {
                        collect_core_expr(&arg.expr, imports, used, spans);
                    }
                }
                OrFallback::Break(_) | OrFallback::Continue(_) => {}
            }
        }
        Expr::Lambda(lam) => match &lam.body {
            LambdaBody::Expr(e) => collect_core_expr(e, imports, used, spans),
            LambdaBody::Block(stmts) => collect_core_stmts(stmts, imports, used, spans),
        },
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            collect_core_expr(cond, imports, used, spans);
            collect_core_stmts(then_body, imports, used, spans);
            collect_core_expr(then_value, imports, used, spans);
            collect_core_stmts(else_body, imports, used, spans);
            collect_core_expr(else_value, imports, used, spans);
        }
        Expr::FanOut { callee, items, .. } => {
            collect_core_expr(callee, imports, used, spans);
            for item in items {
                collect_core_expr(item, imports, used, spans);
            }
        }
        Expr::Int(_, _, _)
        | Expr::Float(_, _, _)
        | Expr::Bool(_, _)
        | Expr::Char(_, _)
        | Expr::Ident(_, _)
        | Expr::Absent(_)
        | Expr::ReduceMarker(_, _)
        | Expr::Todo { .. }
        | Expr::UnitLit { .. }
        | Expr::ComptimeSplice { .. }
        // D-SHIFT1 (c7shift): a leaf literal, no nested `Expr` to recurse into.
        | Expr::StrMatchLit(_, _) => {}
        Expr::Paren(inner, _) => collect_core_expr(inner, imports, used, spans),
        Expr::Spread(inner, _) => collect_core_expr(inner, imports, used, spans),
    }
}

pub(crate) fn check_module_bodies(
    module: &mut crate::AST::LoadedModule,
    module_idx: usize,
    states: &[ModuleState],
    mode: CompileMode,
    freestanding: bool,
    allow_impure: bool,
    summaries: &mut HashMap<String, EffectSummary>,
    embed_inputs_out: &mut Vec<crate::AST::ComptimeInput>,
    global_addr_taken: &mut HashSet<String>,
    ref_facts_out: &mut Vec<Facts::RefFact>,
) -> Vec<Diagnostic> {
    let st = &states[module_idx];
    let mut diags = Vec::new();
    let (ct_funcs, ct_externs, ct_globals) = comptime_context_from_items(&module.items);
    let ct_base_dir = module
        .path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    for item in &mut module.items {
        match item {
            Item::Func(f) => {
                diags.extend(check_func_body_bundle(
                    f,
                    module_idx,
                    states,
                    None,
                    &ct_funcs,
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                    freestanding,
                    allow_impure,
                    summaries,
                    embed_inputs_out,
                    global_addr_taken,
                    ref_facts_out,
                ));
            }
            Item::Struct(s) => {
                for m in &mut s.methods {
                    diags.extend(check_func_body_bundle(
                        m,
                        module_idx,
                        states,
                        Some(&s.name),
                        &ct_funcs,
                        &ct_externs,
                        &ct_base_dir,
                        &ct_globals,
                        freestanding,
                        allow_impure,
                        summaries,
                        embed_inputs_out,
                        global_addr_taken,
                        ref_facts_out,
                    ));
                }
            }
            Item::Enum(e) => {
                for m in &mut e.methods {
                    diags.extend(check_func_body_bundle(
                        m,
                        module_idx,
                        states,
                        Some(&e.name),
                        &ct_funcs,
                        &ct_externs,
                        &ct_base_dir,
                        &ct_globals,
                        freestanding,
                        allow_impure,
                        summaries,
                        embed_inputs_out,
                        global_addr_taken,
                        ref_facts_out,
                    ));
                }
            }
            Item::Impl(i) => {
                for m in &mut i.methods {
                    diags.extend(check_func_body_bundle(
                        m,
                        module_idx,
                        states,
                        Some(&i.type_name),
                        &ct_funcs,
                        &ct_externs,
                        &ct_base_dir,
                        &ct_globals,
                        freestanding,
                        allow_impure,
                        summaries,
                        embed_inputs_out,
                        global_addr_taken,
                        ref_facts_out,
                    ));
                }
            }
            Item::Test(t) if mode == CompileMode::Test => {
                // D-TEST1: a parameterized `#Test fn` is a property test — its
                // params must be generatable types so the runner can synthesize
                // inputs. Validate before checking the body so the error points at
                // the offending param type.
                for p in &t.params {
                    if let Some(d) = property_param_unsupported(&p.ty, p.ty_span) {
                        diags.push(d);
                    }
                }
                let mut synthetic = Func {
                    is_pub: false,
                    is_package_pub: false,
                    external_type: None,
                    name: format!("__test_{}", t.name),
                    name_span: t.name_span,
                    type_params: Vec::new(),
                    params: t.params.clone(),
                    return_type: None,
                    is_view_return: false,
                    is_unsafe: false,
                    is_pure: false,
                    is_reactive: false,
                    is_must_use: false,
                    must_use_span: None,
                    is_inline: false,
                    is_inline_always: false,
                    inline_span: None,
                    is_sanitizer: false,
                    declared_effects: None,
                    effect_via: None,
                    state_requires: None,
                    state_transition: None,
                    web_marker: None,
                    pre: Vec::new(),
                    post: Vec::new(),
                    body: std::mem::take(&mut t.body),
                };
                diags.extend(check_func_body_bundle(
                    &mut synthetic,
                    module_idx,
                    states,
                    None,
                    &ct_funcs,
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                    freestanding,
                    allow_impure,
                    summaries,
                    embed_inputs_out,
                    global_addr_taken,
                    ref_facts_out,
                ));
                t.body = synthetic.body;
            }
            // D-BENCH1: a `#Bench` body type-checks exactly like a `#Test` body
            // (a bare statement list, no params, unit context) — only the mode
            // gate differs.
            Item::Bench(b) if mode == CompileMode::Bench => {
                let mut synthetic = Func {
                    is_pub: false,
                    is_package_pub: false,
                    external_type: None,
                    name: format!("__bench_{}", b.name),
                    name_span: b.name_span,
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: None,
                    is_view_return: false,
                    is_unsafe: false,
                    is_pure: false,
                    is_reactive: false,
                    is_must_use: false,
                    must_use_span: None,
                    is_inline: false,
                    is_inline_always: false,
                    inline_span: None,
                    is_sanitizer: false,
                    declared_effects: None,
                    effect_via: None,
                    state_requires: None,
                    state_transition: None,
                    web_marker: None,
                    pre: Vec::new(),
                    post: Vec::new(),
                    body: std::mem::take(&mut b.body),
                };
                diags.extend(check_func_body_bundle(
                    &mut synthetic,
                    module_idx,
                    states,
                    None,
                    &ct_funcs,
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                    freestanding,
                    allow_impure,
                    summaries,
                    embed_inputs_out,
                    global_addr_taken,
                    ref_facts_out,
                ));
                b.body = synthetic.body;
            }
            Item::CodeModule(cm) => {
                // D-MOD2: type-check inline-module function bodies. Sibling calls were
                // already rewritten to mangled names by `mangle_inline_sibling_calls`,
                // and the mangled signatures are registered in `st.funcs`.
                if let Some(body) = &mut cm.body {
                    for inner in body.iter_mut() {
                        if let Item::Func(f) = inner {
                            diags.extend(check_func_body_bundle(
                                f,
                                module_idx,
                                states,
                                None,
                                &ct_funcs,
                                &ct_externs,
                                &ct_base_dir,
                                &ct_globals,
                                freestanding,
                                allow_impure,
                                summaries,
                                embed_inputs_out,
                                global_addr_taken,
                                ref_facts_out,
                            ));
                        }
                    }
                }
            }
            Item::ErrorConv(ec) => {
                // D-ERR-CONV: type-check the conversion body in the bundle path.
                let st = &states[module_idx];
                diags.extend(crate::Sema::Registration::check_error_conv_body(
                    ec,
                    &st.funcs,
                    &st.registry,
                    &st.structs,
                    &st.consts,
                    &st.trait_reg,
                    &ct_funcs,
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                ));
            }
            _ => {}
        }
    }
    let _ = st;
    diags
}

pub(crate) fn check_func_body_bundle(
    f: &mut Func,
    module_idx: usize,
    states: &[ModuleState],
    owner_type: Option<&str>,
    ct_funcs: &HashMap<String, Func>,
    ct_externs: &HashSet<String>,
    ct_base_dir: &std::path::Path,
    ct_globals: &HashMap<String, crate::Comptime::CtValue>,
    freestanding: bool,
    allow_impure: bool,
    summaries: &mut HashMap<String, EffectSummary>,
    embed_inputs_out: &mut Vec<crate::AST::ComptimeInput>,
    global_addr_taken: &mut HashSet<String>,
    ref_facts_out: &mut Vec<Facts::RefFact>,
) -> Vec<Diagnostic> {
    let st = &states[module_idx];
    let mut ck = Checker {
        funcs: &st.funcs,
        registry: &st.registry,
        structs: &st.structs,
        consts: &st.consts,
        modules: Some(states),
        module_idx,
        imports: &st.imports,
        core_imports: &st.core_imports,
        code_modules: &st.code_modules,
        unqualified: &st.unqualified,
        unqualified_file: &st.unqualified_file,
        func_pub: &st.func_pub,
        func_pkg_pub: &st.func_pkg_pub,
        diags: Vec::new(),
        scopes: vec![HashMap::new()],
        moved: HashMap::new(),
        loop_depth: 0,
        loop_labels: Vec::new(),
        fx_direct: std::collections::BTreeSet::new(),
        fx_edges: std::collections::BTreeSet::new(),
        fx_maximal: false,
        region_stack: Vec::new(),
        fx_regions: Vec::new(),
        fx_callback_obligations: Vec::new(),
        txn_depth: 0,
        det_suppress: 0,
        context_depth: 0,
        context_allocator_active: false,
        // S58 (E2-M13): an `@unsafe fn` body is itself an audited region — its
        // statements may use low-level ops directly without a nested `@unsafe`
        // block. Calling such a fn is gated separately (E3103).
        in_unsafe: f.is_unsafe,
        suppress_must_use: false,
        in_pure: f.is_pure,
        in_pre_clause: false,
        in_comptime: false,
        ret: f.return_type.clone(),
        view_return: f.is_view_return,
        fn_name: f.name.clone(),
        expected_type: None,
        iter_borrowed: HashSet::new(),
        freed_allocators: HashMap::new(),
        arena_views: HashMap::new(),
        list_views: HashMap::new(),
        uninit: HashMap::new(),
        borrow_ctx: false,
        lambda_escapes: true,
        is_task_spawn: false,
        view_capture_tasks: HashSet::new(),
        view_borrow_escape_tasks: HashSet::new(),
        current_binding_name: None,
        lambda_binding: None,
        lambda_mut_borrow_stack: vec![HashSet::new()],
        trait_reg: &st.trait_reg,
        ct_funcs,
        ct_externs,
        ct_base_dir,
        ct_globals,
        ct_scopes: vec![HashMap::new()],
        type_param_scope: f.type_params.clone(),
        freestanding,
        allow_impure,
        ct_impure_depth: 0,
        ct_embed_inputs: Vec::new(),
        in_dropped_comptime_arm: false,
        stmt_tail_ptr: std::ptr::null(),
        stmt_tail_len: 0,
        liveness_frames: Vec::new(),
        taskgroup_stack: Vec::new(),
        in_taskgroup_spawn: false,
        inline_addr_taken: HashSet::new(),
        ref_facts: Vec::new(),
    };
    ck.check_params_and_body(f, owner_type);
    // S60 (E2-M16): purity enforcement for `pure fn` bodies.
    if f.is_pure {
        ck.diags.extend(check_pure_fn(f, &st.funcs));
    }
    // D-METHODMACRO1=A: the local half of the `@InlineAlways` check (self-
    // recursion E0917 + size ceiling E0919); roll this function's
    // address-taken names into the whole-program accumulator so the E0918
    // pass after the full bundle check can see them.
    if f.is_inline_always {
        ck.diags.extend(check_inline_always_fn(f));
    }
    global_addr_taken.extend(std::mem::take(&mut ck.inline_addr_taken));
    // D-EXPANDCLI1 (card #183): roll this function's resolved ref-owner facts
    // into the whole-bundle accumulator for `jet expand --facts refs`.
    ref_facts_out.extend(std::mem::take(&mut ck.ref_facts));
    // D-CTEFFECT1 Tier-1: drain embed inputs into the caller's accumulator.
    embed_inputs_out.extend(std::mem::take(&mut ck.ct_embed_inputs));
    // D-EFF1: record this function's effect summary for the whole-program fixpoint.
    // D-PROP1=A: a declared positive effect is part of the function's contract —
    // callers that prohibit that effect must see it transitively even if the body
    // is currently empty. Seed `direct` with declared positives so solve() propagates them.
    let mut direct = std::mem::take(&mut ck.fx_direct);
    if let Some(declared_list) = &f.declared_effects {
        for (name, _) in declared_list {
            if !name.starts_with('!') {
                if let Some(e) = Effect::parse(name.as_str()) {
                    direct.insert(e);
                }
            }
        }
    }
    summaries.insert(
        effect_key(owner_type, &f.name),
        EffectSummary {
            direct,
            edges: std::mem::take(&mut ck.fx_edges),
            maximal: ck.fx_maximal,
            regions: std::mem::take(&mut ck.fx_regions),
            callback_obligations: std::mem::take(&mut ck.fx_callback_obligations),
        },
    );
    ck.diags
}

pub(crate) fn func_sig_to_fn_type(sig: &FuncSig) -> Type {
    Type::Fn {
        params: sig.params.iter().map(|(_, t)| t.clone()).collect(),
        ret: sig.return_type.clone().map(Box::new),
        effect_bound: None,
    }
}

pub(crate) fn fn_types_compatible(want: &Type, got: &Type) -> bool {
    let (
        Type::Fn {
            params: wp,
            ret: wr,
            ..
        },
        Type::Fn {
            params: gp,
            ret: gr,
            ..
        },
    ) = (want, got)
    else {
        return false;
    };
    if wp.len() != gp.len() {
        return false;
    }
    for (a, b) in wp.iter().zip(gp.iter()) {
        if a != b {
            return false;
        }
    }
    match (wr, gr) {
        (None, None) => true,
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// D-TEST1: which parameter types the property-test runner can synthesize inputs
/// for. The generator (codegen) covers the scalar value types plus `[T]` and
/// `T?` of a generatable element. Anything else (user structs/enums, `Map`,
/// functions, trait objects) has no automatic generator yet, so reject it with a
/// clear error rather than miscompile (I3 — checking lives in sema).
fn property_param_generatable(ty: &Type) -> bool {
    match ty {
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::Float32
        | Type::IntN { .. } => true,
        Type::List(inner) | Type::Option(inner) => property_param_generatable(inner),
        Type::FixedList { elem, .. } => property_param_generatable(elem),
        _ => false,
    }
}

/// E0613: a property-test parameter type with no automatic value generator.
fn property_param_unsupported(ty: &Type, span: Span) -> Option<Diagnostic> {
    if property_param_generatable(ty) {
        return None;
    }
    Some(Diagnostic::error(
        "E0613",
        format!(
            "a property test can't generate values of type `{}`",
            ty.name()
        ),
        format!(
            "a parameterized `#{} fn` is a property test (D-TEST1): {} generates inputs from each parameter's type, but this type has no built-in generator",
            Syntax::KW_TEST,
            Syntax::LANG_NAME
        ),
        "use a generatable type (Int, Float, Bool, String, Char, a sized integer, or a list/optional of those), or write a plain `#Test \"name\" { … }` block and construct the value yourself".to_string(),
        Some(span),
    ))
}
