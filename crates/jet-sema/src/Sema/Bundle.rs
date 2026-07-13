use super::*;
use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use crate::Traits::TraitRegistry;
use crate::AST::{
    CodeModule, ConstAttr, ElseBranch, EnumDef, EnumLitArg, Expr, ForKind, Func, GenericModuleDef,
    GenericModuleParam, IfStmt, ImportKind, Item, LValue, LambdaBody, ModuleAliasDef, ModuleArg,
    OrFallback, ProgramBundle, RustConstKind, Stmt, StrPart, Type, VariantPayload,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

fn is_fallible_void_entry_return(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Result { ok, err }
            if matches!(ok.as_ref(), Type::Named(n) if n == Syntax::TYPE_VOID)
                && matches!(err.as_ref(), Type::Named(n) if n == Syntax::TYPE_ERROR)
    )
}

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

fn ct_value_expr(value: &crate::AST::CtValue, span: crate::Diagnostics::Span) -> Expr {
    match value {
        crate::AST::CtValue::Bool(v) => Expr::Bool(*v, span),
        crate::AST::CtValue::Int(v) => Expr::Int(*v, span, None),
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

fn substitute_expr(
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
        | Expr::Field(inner, _, _)
        | Expr::Tainted(inner, _)
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _)
        | Expr::Paren(inner, _)
        | Expr::Spread(inner, _) => substitute_expr(inner, types, values),
        Expr::OptField { base, .. } => substitute_expr(base, types, values),
        Expr::MethodCall {
            receiver,
            type_args,
            args,
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
                | OrFallback::Continue(_) => {}
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
            base, start, end, ..
        } => {
            substitute_expr(base, types, values);
            substitute_expr(start, types, values);
            substitute_expr(end, types, values);
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
        Expr::FanOut { callee, items, .. } => {
            substitute_expr(callee, types, values);
            items
                .iter_mut()
                .for_each(|value| substitute_expr(value, types, values));
        }
    }
}

fn substitute_if(
    branch: &mut IfStmt,
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) {
    substitute_expr(&mut branch.cond, types, values);
    substitute_stmts(&mut branch.then_body, types, values);
    match &mut branch.else_branch {
        Some(ElseBranch::ElseIf(next)) => substitute_if(next, types, values),
        Some(ElseBranch::Else(body)) => substitute_stmts(body, types, values),
        None => {}
    }
}

fn substitute_stmts(
    stmts: &mut [Stmt],
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(value) | Stmt::Yield(value, _) => substitute_expr(value, types, values),
            Stmt::Val(binding) => {
                if let Some(ty) = &mut binding.ty {
                    *ty = specialize_module_type(ty, types, values);
                }
                substitute_expr(&mut binding.init, types, values);
            }
            Stmt::Assign { value, .. } | Stmt::Return(Some(value), _) => {
                substitute_expr(value, types, values)
            }
            Stmt::Return(None, _)
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::BreakLabel(..)
            | Stmt::ContinueLabel(..) => {}
            Stmt::If(branch) => substitute_if(branch, types, values),
            Stmt::While { cond, body, .. } => {
                substitute_expr(cond, types, values);
                substitute_stmts(body, types, values);
            }
            Stmt::For { kind, body, .. } => {
                match kind {
                    ForKind::Range { start, end, step } => {
                        substitute_expr(start, types, values);
                        substitute_expr(end, types, values);
                        if let Some(step) = step {
                            substitute_expr(step, types, values);
                        }
                    }
                    ForKind::In { collection } => substitute_expr(collection, types, values),
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
                substitute_stmts(std::slice::from_mut(step), types, values);
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

fn specialize_func(
    mut func: Func,
    params: &[ResolvedModuleParam],
    args: &[ResolvedModuleArg],
    definition_types: &HashMap<String, Type>,
    definition_values: &HashMap<String, crate::AST::CtValue>,
) -> Func {
    let mut types = definition_types.clone();
    let mut values = definition_values.clone();
    for (param, arg) in params.iter().zip(args) {
        match (param, arg) {
            (ResolvedModuleParam::Type { name, .. }, ResolvedModuleArg::Type(ty)) => {
                types.insert(name.clone(), ty.clone());
            }
            (ResolvedModuleParam::Value { name, .. }, ResolvedModuleArg::Value(value, bytes)) => {
                let _ = bytes;
                values.insert(name.clone(), value.clone());
            }
            _ => {}
        }
    }
    for param in &mut func.params {
        param.ty = specialize_module_type(&param.ty, &types, &values);
        if let Some(default) = &mut param.default {
            substitute_expr(default, &types, &values);
        }
    }
    if let Some(ret) = &mut func.return_type {
        *ret = specialize_module_type(ret, &types, &values);
    }
    substitute_stmts(&mut func.body, &types, &values);
    func
}

fn mapped_definition_name(name: &str, types: &HashMap<String, Type>) -> String {
    match types.get(name) {
        Some(Type::Named(mapped)) => mapped.clone(),
        _ => name.to_string(),
    }
}

fn clone_trait(source: &crate::AST::TraitDef) -> crate::AST::TraitDef {
    crate::AST::TraitDef {
        span: source.span,
        is_pub: source.is_pub,
        is_package_pub: source.is_package_pub,
        name: source.name.clone(),
        name_span: source.name_span,
        assoc_types: source.assoc_types.clone(),
        methods: source.methods.clone(),
    }
}

fn clone_tag(source: &crate::AST::TagDef) -> crate::AST::TagDef {
    crate::AST::TagDef { is_pub: source.is_pub, is_package_pub: source.is_package_pub,
        name: source.name.clone(), name_span: source.name_span, methods: source.methods.clone(), span: source.span }
}

fn specialize_tag(source: &crate::AST::TagDef, types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>) -> crate::AST::TagDef {
    let mut result = clone_tag(source);
    result.name = mapped_definition_name(&source.name, types);
    for method in &mut result.methods {
        for param in &mut method.params { param.ty = specialize_module_type(&param.ty, types, values); }
        if let Some(ret) = &mut method.return_type { *ret = specialize_module_type(ret, types, values); }
        if let Some(body) = &mut method.default_body { substitute_stmts(body, types, values); }
    }
    result
}

fn clone_impl(source: &crate::AST::ImplDef) -> crate::AST::ImplDef {
    crate::AST::ImplDef {
        span: source.span,
        type_name: source.type_name.clone(),
        type_span: source.type_span,
        trait_name: source.trait_name.clone(),
        trait_span: source.trait_span,
        methods: source.methods.clone(),
        delegation_field: source.delegation_field.clone(),
        assoc_type_impls: source.assoc_type_impls.clone(),
        is_generated_serde: source.is_generated_serde,
        os_target: source.os_target,
    }
}

fn clone_error_conv(source: &crate::AST::ErrorConvDef) -> crate::AST::ErrorConvDef {
    crate::AST::ErrorConvDef {
        from_ty: source.from_ty.clone(),
        from_span: source.from_span,
        to_ty: source.to_ty.clone(),
        to_span: source.to_span,
        body: source.body.clone(),
        body_span: source.body_span,
    }
}

fn clone_test(source: &crate::AST::TestDef) -> crate::AST::TestDef {
    crate::AST::TestDef { span: source.span, name: source.name.clone(), name_span: source.name_span,
        params: source.params.clone(), fn_keyword_span: source.fn_keyword_span, body: source.body.clone() }
}

fn clone_bench(source: &crate::AST::BenchDef) -> crate::AST::BenchDef {
    crate::AST::BenchDef { span: source.span, name: source.name.clone(), name_span: source.name_span,
        body: source.body.clone() }
}

fn specialize_test(source: &crate::AST::TestDef, alias: &str,
    types: &HashMap<String, Type>, values: &HashMap<String, crate::AST::CtValue>) -> crate::AST::TestDef {
    let mut result = clone_test(source);
    result.name = format!("{alias}__{}", source.name);
    for param in &mut result.params {
        param.ty = specialize_module_type(&param.ty, types, values);
        if let Some(default) = &mut param.default { substitute_expr(default, types, values); }
    }
    substitute_stmts(&mut result.body, types, values);
    result
}

fn specialize_bench(source: &crate::AST::BenchDef, alias: &str,
    types: &HashMap<String, Type>, values: &HashMap<String, crate::AST::CtValue>) -> crate::AST::BenchDef {
    let mut result = clone_bench(source);
    result.name = format!("{alias}__{}", source.name);
    substitute_stmts(&mut result.body, types, values);
    result
}

fn specialize_trait(
    source: &crate::AST::TraitDef,
    params: &[ResolvedModuleParam],
    args: &[ResolvedModuleArg],
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) -> crate::AST::TraitDef {
    let mut result = clone_trait(source);
    result.name = mapped_definition_name(&source.name, types);
    for method in &mut result.methods {
        for param in &mut method.params {
            param.ty = specialize_module_type(&param.ty, types, values);
            if let Some(default) = &mut param.default { substitute_expr(default, types, values); }
        }
        if let Some(ret) = &mut method.return_type {
            *ret = specialize_module_type(ret, types, values);
        }
        if let Some(body) = &mut method.default_body {
            substitute_stmts(body, types, values);
        }
    }
    let _ = (params, args);
    result
}

fn specialize_impl(
    source: &crate::AST::ImplDef,
    params: &[ResolvedModuleParam],
    args: &[ResolvedModuleArg],
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) -> crate::AST::ImplDef {
    let mut result = clone_impl(source);
    result.type_name = mapped_definition_name(&source.type_name, types);
    result.trait_name = source.trait_name.as_ref().map(|name| mapped_definition_name(name, types));
    result.methods = source.methods.iter().cloned()
        .map(|method| specialize_func(method, params, args, types, values)).collect();
    result.assoc_type_impls = source.assoc_type_impls.iter().map(|(name, span, ty)| {
        (name.clone(), *span, specialize_module_type(ty, types, values))
    }).collect();
    result
}

fn specialize_error_conv(
    source: &crate::AST::ErrorConvDef,
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) -> crate::AST::ErrorConvDef {
    let mut result = clone_error_conv(source);
    result.from_ty = mapped_definition_name(&source.from_ty, types);
    result.to_ty = mapped_definition_name(&source.to_ty, types);
    substitute_stmts(&mut result.body, types, values);
    result
}

fn specialize_module_type(
    ty: &Type,
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) -> Type {
    let mut resolved = crate::Generics::substitute_type(ty, types);
    fn lengths(ty: &mut Type, types: &HashMap<String, Type>, values: &HashMap<String, crate::AST::CtValue>) {
        match ty {
            Type::FixedList { elem, len, len_symbol } => {
                lengths(elem, types, values);
                if let Some((name, _)) = len_symbol.as_ref() {
                    if let Some(crate::AST::CtValue::Int(value)) = values.get(name) {
                        if *value >= 0 {
                            *len = *value as u64;
                            *len_symbol = None;
                        }
                    }
                }
            }
            Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => lengths(inner, types, values),
            Type::Map { key, value, .. } => { lengths(key, types, values); lengths(value, types, values); }
            Type::Result { ok, err } => { lengths(ok, types, values); lengths(err, types, values); }
            Type::Fn { params, ret, .. } => { for param in params { lengths(param, types, values); } if let Some(ret) = ret { lengths(ret, types, values); } }
            Type::Apply { args, .. } => args.iter_mut().for_each(|arg| lengths(arg, types, values)),
            Type::Tuple(fields) => fields.iter_mut().for_each(|(_, ty)| lengths(ty, types, values)),
            Type::Tagged { marker, inner } => {
                if let Some(Type::Named(mapped)) = types.get(marker) { *marker = mapped.clone(); }
                **inner = crate::Generics::substitute_type(inner, types);
                lengths(inner, types, values);
            }
            _ => {}
        }
    }
    lengths(&mut resolved, types, values);
    resolved
}

fn specialize_nested_code_module(
    module: &CodeModule,
    params: &[ResolvedModuleParam],
    args: &[ResolvedModuleArg],
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) -> CodeModule {
    let body = module.body.as_ref().map(|items| {
        items.iter().filter_map(|item| match item {
            Item::Func(def) => Some(Item::Func(specialize_func(def.clone(), params, args, types, values))),
            Item::Struct(def) => Some(Item::Struct(specialize_struct(def, "", params, args, types, values))),
            Item::Enum(def) => Some(Item::Enum(specialize_enum(def, "", params, args, types, values))),
            Item::Const(def) => {
                let mut value = def.value.clone();
                substitute_expr(&mut value, types, values);
                Some(Item::Const(crate::AST::ConstDef { span: def.span, name: def.name.clone(), name_span: def.name_span, value, meta: def.meta.clone(), attrs: def.attrs.clone(), rust_kind: def.rust_kind, is_comptime: def.is_comptime, ct: def.ct.clone(), ty: def.ty.as_ref().map(|ty| specialize_module_type(ty, types, values)), is_persist: def.is_persist, persist_span: def.persist_span }))
            }
            Item::CodeModule(child) => Some(Item::CodeModule(specialize_nested_code_module(child, params, args, types, values))),
            Item::Trait(def) => Some(Item::Trait(specialize_trait(def, params, args, types, values))),
            Item::Tag(def) => Some(Item::Tag(specialize_tag(def, types, values))),
            Item::Impl(def) => Some(Item::Impl(specialize_impl(def, params, args, types, values))),
            Item::ErrorConv(def) => Some(Item::ErrorConv(specialize_error_conv(def, types, values))),
            Item::Test(def) => Some(Item::Test(specialize_test(def, &module.name, types, values))),
            Item::Bench(def) => Some(Item::Bench(specialize_bench(def, &module.name, types, values))),
            _ => None,
        }).collect()
    });
    CodeModule { name: module.name.clone(), name_span: module.name_span, is_pub: module.is_pub, is_package_pub: module.is_package_pub, body, web_target: module.web_target, instance_identity: module.instance_identity.clone(), span: module.span }
}

fn specialize_struct(
    source: &crate::AST::StructDef,
    alias: &str,
    params: &[ResolvedModuleParam],
    args: &[ResolvedModuleArg],
    definition_types: &HashMap<String, Type>,
    definition_values: &HashMap<String, crate::AST::CtValue>,
) -> crate::AST::StructDef {
    let mut types = definition_types.clone();
    for (param, arg) in params.iter().zip(args) {
        if let (ResolvedModuleParam::Type { name, .. }, ResolvedModuleArg::Type(ty)) = (param, arg) {
            types.insert(name.clone(), ty.clone());
        }
    }
    let mut fields = source.fields.clone();
    for field in &mut fields {
        field.ty = specialize_module_type(&field.ty, &types, definition_values);
        if let Some(computed) = &mut field.computed {
            substitute_expr(computed, &types, definition_values);
        }
        for marker in &mut field.serde_markers {
            marker
                .args
                .iter_mut()
                .for_each(|arg| substitute_expr(arg, &types, definition_values));
        }
    }
    let methods = source
        .methods
        .iter()
        .cloned()
        .map(|method| {
            specialize_func(
                method,
                params,
                args,
                definition_types,
                definition_values,
            )
        })
        .collect();
    let trait_impls = source
        .trait_impls
        .iter()
        .map(|block| crate::AST::TraitImplBlock {
            trait_name: mapped_definition_name(&block.trait_name, &types),
            trait_span: block.trait_span,
            methods: block
                .methods
                .iter()
                .cloned()
                .map(|method| {
                    specialize_func(
                        method,
                        params,
                        args,
                        definition_types,
                        definition_values,
                    )
                })
                .collect(),
            assoc_type_impls: block
                .assoc_type_impls
                .iter()
                .map(|(name, span, ty)| {
                    (
                        name.clone(),
                        *span,
                        specialize_module_type(ty, &types, definition_values),
                    )
                })
                .collect(),
        })
        .collect();
    crate::AST::StructDef {
        span: source.span,
        is_pub: source.is_pub,
        is_package_pub: source.is_package_pub,
        name: if alias.is_empty() {
            source.name.clone()
        } else {
            format!("{alias}__{}", source.name)
        },
        name_span: source.name_span,
        type_params: source.type_params.clone(),
        fields,
        methods,
        trait_impls,
        derives: source.derives.iter().map(|(name, span)| (mapped_definition_name(name, &types), *span)).collect(),
        is_published_schema: source.is_published_schema,
        published_schema_span: source.published_schema_span,
        is_single_use: source.is_single_use,
        single_use_span: source.single_use_span,
        is_must_use: source.is_must_use,
        must_use_span: source.must_use_span,
        layout: source.layout.clone(),
        layout_span: source.layout_span,
        serde_markers: source.serde_markers.clone(),
        type_markers: source.type_markers.clone(),
        validate_block: source.validate_block.clone(),
        validate_span: source.validate_span,
    }
}

fn clone_struct(source: &crate::AST::StructDef) -> crate::AST::StructDef {
    specialize_struct(
        source,
        "",
        &[],
        &[],
        &HashMap::new(),
        &HashMap::new(),
    )
}

fn specialize_enum(
    source: &crate::AST::EnumDef,
    alias: &str,
    params: &[ResolvedModuleParam],
    args: &[ResolvedModuleArg],
    definition_types: &HashMap<String, Type>,
    definition_values: &HashMap<String, crate::AST::CtValue>,
) -> crate::AST::EnumDef {
    let mut types = definition_types.clone();
    for (param, arg) in params.iter().zip(args) {
        if let (ResolvedModuleParam::Type { name, .. }, ResolvedModuleArg::Type(ty)) = (param, arg) {
            types.insert(name.clone(), ty.clone());
        }
    }
    let variants = source
        .variants
        .iter()
        .map(|variant| {
            let payload = match &variant.payload {
                VariantPayload::Unit => VariantPayload::Unit,
                VariantPayload::Single(ty, span) => VariantPayload::Single(
                    specialize_module_type(ty, &types, definition_values),
                    *span,
                ),
                VariantPayload::Named(fields) => VariantPayload::Named(
                    fields
                        .iter()
                        .cloned()
                        .map(|mut field| {
                            field.ty = specialize_module_type(&field.ty, &types, definition_values);
                            field
                        })
                        .collect(),
                ),
            };
            let mut serde_markers = variant.serde_markers.clone();
            for marker in &mut serde_markers {
                marker.args.iter_mut().for_each(|arg| {
                    substitute_expr(arg, &types, definition_values);
                });
            }
            crate::AST::Variant {
                name: variant.name.clone(),
                name_span: variant.name_span,
                payload,
                discriminant: variant.discriminant,
                serde_markers,
            }
        })
        .collect();
    let methods = source
        .methods
        .iter()
        .cloned()
        .map(|method| {
            specialize_func(
                method,
                params,
                args,
                definition_types,
                definition_values,
            )
        })
        .collect();
    let trait_impls = source
        .trait_impls
        .iter()
        .map(|block| crate::AST::TraitImplBlock {
            trait_name: mapped_definition_name(&block.trait_name, &types),
            trait_span: block.trait_span,
            methods: block
                .methods
                .iter()
                .cloned()
                .map(|method| {
                    specialize_func(
                        method,
                        params,
                        args,
                        definition_types,
                        definition_values,
                    )
                })
                .collect(),
            assoc_type_impls: block
                .assoc_type_impls
                .iter()
                .map(|(name, span, ty)| {
                    (
                        name.clone(),
                        *span,
                        crate::Generics::substitute_type(ty, &types),
                    )
                })
                .collect(),
        })
        .collect();
    crate::AST::EnumDef {
        span: source.span,
        is_pub: source.is_pub,
        is_package_pub: source.is_package_pub,
        name: if alias.is_empty() {
            source.name.clone()
        } else {
            format!("{alias}__{}", source.name)
        },
        name_span: source.name_span,
        type_params: source.type_params.clone(),
        variants,
        methods,
        trait_impls,
        derives: source.derives.iter().map(|(name, span)| (mapped_definition_name(name, &types), *span)).collect(),
        is_single_use: source.is_single_use,
        single_use_span: source.single_use_span,
        is_must_use: source.is_must_use,
        must_use_span: source.must_use_span,
        serde_markers: source.serde_markers.clone(),
        type_markers: source.type_markers.clone(),
        groups: source.groups.clone(),
    }
}

fn clone_enum(source: &crate::AST::EnumDef) -> crate::AST::EnumDef {
    specialize_enum(
        source,
        "",
        &[],
        &[],
        &HashMap::new(),
        &HashMap::new(),
    )
}

#[derive(Clone)]
enum ResolvedModuleParam {
    Type { name: String, bound: Option<String> },
    Value { name: String, ty: Type },
    Invalid,
}

/// A cloned view of a generic module template.
struct TemplateInfo {
    def: GenericModuleDef,
    definition_id: String,
    params: Vec<ResolvedModuleParam>,
    source_module: usize,
    source_items: Vec<Item>,
    source_values: HashMap<String, crate::AST::CtValue>,
}

impl Clone for TemplateInfo {
    fn clone(&self) -> Self {
        Self {
            def: clone_generic_module_def(&self.def),
            definition_id: self.definition_id.clone(),
            params: self.params.clone(),
            source_module: self.source_module,
            source_items: clone_definition_items(&self.source_items),
            source_values: self.source_values.clone(),
        }
    }
}

fn clone_definition_items(items: &[Item]) -> Vec<Item> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Func(def) => Some(Item::Func(def.clone())),
            Item::Struct(def) => Some(Item::Struct(clone_struct(def))),
            Item::Enum(def) => Some(Item::Enum(clone_enum(def))),
            Item::Trait(def) => Some(Item::Trait(clone_trait(def))),
            Item::Tag(def) => Some(Item::Tag(clone_tag(def))),
            Item::Impl(def) => Some(Item::Impl(clone_impl(def))),
            Item::ErrorConv(def) => Some(Item::ErrorConv(clone_error_conv(def))),
            Item::Test(def) => Some(Item::Test(clone_test(def))),
            Item::Bench(def) => Some(Item::Bench(clone_bench(def))),
            Item::Const(def) => Some(Item::Const(crate::AST::ConstDef {
                span: def.span,
                name: def.name.clone(),
                name_span: def.name_span,
                value: def.value.clone(),
                meta: def.meta.clone(),
                attrs: def.attrs.clone(),
                rust_kind: def.rust_kind,
                is_comptime: def.is_comptime,
                ct: def.ct.clone(),
                ty: def.ty.clone(),
                is_persist: def.is_persist,
                persist_span: def.persist_span,
            })),
            _ => None,
        })
        .collect()
}

fn clone_generic_module_def(gm: &GenericModuleDef) -> GenericModuleDef {
    gm.clone()
}

struct AliasExpansion {
    module: CodeModule,
    declarations: Vec<Item>,
}

fn specialize_nested_template_outer(
    source: &GenericModuleDef,
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) -> GenericModuleDef {
    let mut result = clone_generic_module_def(source);
    result.body = result.body.into_iter().map(|item| match item {
        Item::Func(func) => Item::Func(specialize_func(func, &[], &[], types, values)),
        Item::Struct(def) => Item::Struct(specialize_struct(&def, "", &[], &[], types, values)),
        Item::Enum(def) => Item::Enum(specialize_enum(&def, "", &[], &[], types, values)),
        Item::Trait(def) => Item::Trait(specialize_trait(&def, &[], &[], types, values)),
        Item::Tag(def) => Item::Tag(specialize_tag(&def, types, values)),
        Item::Impl(def) => Item::Impl(specialize_impl(&def, &[], &[], types, values)),
        Item::ErrorConv(def) => Item::ErrorConv(specialize_error_conv(&def, types, values)),
        Item::Test(def) => Item::Test(specialize_test(&def, &source.name, types, values)),
        Item::Bench(def) => Item::Bench(specialize_bench(&def, &source.name, types, values)),
        Item::GenericModule(def) => Item::GenericModule(specialize_nested_template_outer(&def, types, values)),
        other => other,
    }).collect();
    result
}

fn specialize_nested_alias_outer(
    source: &ModuleAliasDef,
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) -> ModuleAliasDef {
    let mut args = source.args.clone();
    for arg in &mut args {
        match arg {
            ModuleArg::Type(ty, _) => *ty = specialize_module_type(ty, types, values),
            ModuleArg::Value(expr, _) => substitute_expr(expr, types, values),
        }
    }
    ModuleAliasDef { name: source.name.clone(), name_span: source.name_span,
        is_pub: source.is_pub, is_package_pub: source.is_package_pub, target: source.target.clone(),
        target_span: source.target_span, args, span: source.span }
}

#[derive(Clone)]
enum ResolvedModuleArg { Type(Type), Value(crate::AST::CtValue, Vec<u8>) }

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ModuleInstanceKey {
    definition_id: String,
    args: Vec<Vec<u8>>,
}

impl ModuleInstanceKey {
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.definition_id.len() as u64).to_be_bytes());
        out.extend_from_slice(self.definition_id.as_bytes());
        out.extend_from_slice(&(self.args.len() as u64).to_be_bytes());
        for arg in &self.args {
            out.extend_from_slice(&(arg.len() as u64).to_be_bytes());
            out.extend_from_slice(arg);
        }
        out
    }
}

fn instance_identity(key: &ModuleInstanceKey, template: &TemplateInfo) -> crate::AST::ModuleInstanceIdentity {
    let full_key = key.bytes();
    let mut input = full_key.clone();
    // Template/dependency semantics, excluding consumer alias spelling. The
    // resolved definition snapshot includes captured dependency declarations.
    input.extend_from_slice(&crate::CanonicalAST::canonical_fragment(&template.def.body));
    input.extend_from_slice(&crate::CanonicalAST::canonical_fragment(&template.source_items));
    crate::AST::ModuleInstanceIdentity {
        full_key,
        fingerprint: crate::SHA256::sha256_hex(&input),
    }
}

fn register_instance_fingerprint(
    registry: &mut HashMap<String, Vec<u8>>,
    identity: &crate::AST::ModuleInstanceIdentity,
    span: Span,
) {
    if let Some(previous) = registry.get(&identity.fingerprint) {
        if previous != &identity.full_key {
            let hex = |bytes: &[u8]| bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
            jet_foundation::ice!(
                Some(span),
                "E0859 generic module instance fingerprint collision: digest={} first-full-key={} second-full-key={}; compilation stopped before codegen",
                identity.fingerprint,
                hex(previous),
                hex(&identity.full_key),
            );
        }
    } else {
        registry.insert(identity.fingerprint.clone(), identity.full_key.clone());
    }
}

fn normalized_instance_type(ty: &Type, aliases: &HashMap<String, Type>) -> Type {
    fn go(ty: &Type, aliases: &HashMap<String, Type>, seen: &mut HashSet<String>) -> Type {
        if let Type::Named(name) = ty {
            if seen.insert(name.clone()) {
                if let Some(target) = aliases.get(name) {
                    let result = go(target, aliases, seen);
                    seen.remove(name);
                    return result;
                }
                seen.remove(name);
            }
        }
        crate::Generics::substitute_type(ty, &aliases.iter().map(|(name, target)| (name.clone(), target.clone())).collect())
    }
    go(ty, aliases, &mut HashSet::new())
}

fn instance_key(
    info: &TemplateInfo,
    args: &[ResolvedModuleArg],
    type_aliases: &HashMap<String, Type>,
) -> ModuleInstanceKey {
    let args = args.iter().map(|arg| match arg {
        ResolvedModuleArg::Type(ty) => {
            let name = normalized_instance_type(ty, type_aliases).name();
            let mut bytes = vec![0];
            bytes.extend_from_slice(&(name.len() as u64).to_be_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes
        }
        ResolvedModuleArg::Value(_, normalized) => {
            let mut bytes = vec![1];
            bytes.extend_from_slice(&(normalized.len() as u64).to_be_bytes());
            bytes.extend_from_slice(normalized);
            bytes
        }
    }).collect();
    ModuleInstanceKey { definition_id: info.definition_id.clone(), args }
}

fn resolve_params(
    def: &GenericModuleDef,
    traits: &TraitRegistry,
    enums: &HashMap<String, bool>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<ResolvedModuleParam> {
    def.params
        .iter()
        .map(|param| match param {
            GenericModuleParam::Bare { name, .. } => ResolvedModuleParam::Type {
                name: name.clone(),
                bound: None,
            },
            GenericModuleParam::Annotated {
                name,
                name_span,
                annotation,
            } => {
                if let Type::Named(bound) = annotation {
                    if traits.traits.contains_key(bound) {
                        return ResolvedModuleParam::Type {
                            name: name.clone(),
                            bound: Some(bound.clone()),
                        };
                    }
                }
                let allowed = matches!(annotation, Type::Bool | Type::Int | Type::Char | Type::String)
                    || matches!(annotation, Type::Named(n) if enums.get(n).copied() == Some(true));
                if allowed {
                    ResolvedModuleParam::Value {
                        name: name.clone(),
                        ty: annotation.clone(),
                    }
                } else {
                    diags.push(Diagnostic::error(
                        "E0856",
                        format!(
                            "generic module value parameter `{name}` uses unsupported type `{}`",
                            type_name(annotation)
                        ),
                        "value parameters admit only Bool, Int, Char, String, or a fieldless enum"
                            .to_string(),
                        "use a Tier-0 value type, or make this an unannotated type parameter"
                            .to_string(),
                        Some(*name_span),
                    ));
                    ResolvedModuleParam::Invalid
                }
            }
        })
        .collect()
}

fn type_name(ty:&Type)->String{match ty{Type::Int=>"Int".into(),Type::Bool=>"Bool".into(),Type::Char=>"Char".into(),Type::String=>"String".into(),Type::Named(n)=>n.clone(),Type::Apply{name,..}=>name.clone(),other=>format!("{other:?}")}}
fn value_type(value:&crate::AST::CtValue)->Option<Type>{match value{crate::AST::CtValue::Bool(_)=>Some(Type::Bool),crate::AST::CtValue::Int(_)=>Some(Type::Int),crate::AST::CtValue::Char(_)=>Some(Type::Char),crate::AST::CtValue::Str(_)=>Some(Type::String),crate::AST::CtValue::Enum{type_name,args,..}if args.is_empty()=>Some(Type::Named(type_name.clone())),_=>None}}
fn normalized_value(value:&crate::AST::CtValue)->Option<Vec<u8>>{let mut out=Vec::new();match value{crate::AST::CtValue::Bool(v)=>{out.extend_from_slice(&[1,u8::from(*v)]);},crate::AST::CtValue::Int(v)=>{out.push(2);out.extend_from_slice(&v.to_be_bytes());},crate::AST::CtValue::Char(v)=>{out.push(3);out.extend_from_slice(&(*v as u32).to_be_bytes());},crate::AST::CtValue::Str(v)=>{out.push(4);out.extend_from_slice(&(v.len() as u64).to_be_bytes());out.extend_from_slice(v.as_bytes());},crate::AST::CtValue::Enum{type_name,variant,args}if args.is_empty()=>{out.push(5);for text in [type_name,variant]{out.extend_from_slice(&(text.len() as u64).to_be_bytes());out.extend_from_slice(text.as_bytes());}},_=>return None}Some(out)}

fn module_arg_expr(arg:&ModuleArg)->Option<Expr>{match arg{ModuleArg::Value(expr,_)=>Some(expr.clone()),ModuleArg::Type(Type::Named(name),span)=>Some(Expr::Ident(name.clone(),*span)),_=>None}}

fn resolve_args(
    alias: &ModuleAliasDef,
    template: &TemplateInfo,
    traits: &TraitRegistry,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, crate::AST::CtValue>,
    enums: &HashMap<String, bool>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Vec<ResolvedModuleArg>> {
    let mut out = Vec::new();
    for (param, arg) in template.params.iter().zip(&alias.args) {
        match param {
            ResolvedModuleParam::Invalid => return None,
            ResolvedModuleParam::Type { name, bound } => {
                let ty = match arg {
                    ModuleArg::Type(ty, _) => ty.clone(),
                    ModuleArg::Value(Expr::Ident(n, _), _) => Type::Named(n.clone()),
                    _ => {
                        diags.push(Diagnostic::error(
                            "E0852",
                            format!("type argument for `{name}` does not satisfy its module bound"),
                            "this slot resolves to a type parameter, but the argument is a value expression".into(),
                            "pass a type that satisfies the declared bound".into(),
                            Some(arg.span()),
                        ));
                        return None;
                    }
                };
                if let Some(bound) = bound {
                    let identity = type_name(&ty);
                    if !traits.implements_trait(&identity, bound) {
                        diags.push(Diagnostic::error(
                            "E0852",
                            format!("type argument `{identity}` does not satisfy `{bound}`"),
                            format!("generic module parameter `{name}` requires the `{bound}` bound"),
                            format!("pass a type that implements `{bound}`"),
                            Some(arg.span()),
                        ));
                        return None;
                    }
                }
                out.push(ResolvedModuleArg::Type(ty));
            }
            ResolvedModuleParam::Value { name, ty } => {
                let Some(expr) = module_arg_expr(arg) else {
                    diags.push(Diagnostic::error(
                        "E0853",
                        format!("value argument for `{name}` has the wrong type"),
                        format!("this slot requires an exact `{}` Tier-0 value", type_name(ty)),
                        format!("pass a compile-time `{}` value without conversion", type_name(ty)),
                        Some(arg.span()),
                    ));
                    return None;
                };
                let value = match crate::Comptime::evaluate(
                    &expr,
                    funcs,
                    &HashSet::new(),
                    Path::new("."),
                    globals,
                ) {
                    Ok(value) => value,
                    Err(_) => {
                        diags.push(Diagnostic::error(
                            "E0857",
                            format!("value argument for `{name}` is not known at compile time"),
                            "generic module instances need one closed, deterministic Tier-0 value"
                                .to_string(),
                            format!("pass a literal or comptime `{}` value", type_name(ty)),
                            Some(arg.span()),
                        ));
                        return None;
                    }
                };
                let actual = value_type(&value);
                let allowed = matches!(ty, Type::Bool | Type::Int | Type::Char | Type::String)
                    || matches!(ty, Type::Named(n) if enums.get(n).copied() == Some(true));
                if !allowed || actual.as_ref() != Some(ty) {
                    diags.push(Diagnostic::error(
                        "E0853",
                        format!("value argument for `{name}` has the wrong type"),
                        format!(
                            "expected exact `{}`, found `{}`",
                            type_name(ty),
                            actual
                                .as_ref()
                                .map(type_name)
                                .unwrap_or_else(|| "non-Tier-0 value".into())
                        ),
                        format!("pass a compile-time `{}` value without conversion", type_name(ty)),
                        Some(arg.span()),
                    ));
                    return None;
                }
                let bytes = normalized_value(&value).expect("allowed Tier-0 value normalizes");
                out.push(ResolvedModuleArg::Value(value, bytes));
            }
        }
    }
    Some(out)
}

trait ModuleArgSpan{fn span(&self)->crate::Diagnostics::Span;}impl ModuleArgSpan for ModuleArg{fn span(&self)->crate::Diagnostics::Span{match self{ModuleArg::Type(_,s)|ModuleArg::Value(_,s)=>*s}}}

fn expand_alias(
    alias: &ModuleAliasDef,
    consumer_module: usize,
    templates: &std::collections::HashMap<String, TemplateInfo>,
    diags: &mut Vec<Diagnostic>,
    traits:&TraitRegistry,
    funcs:&HashMap<String,&Func>,
    globals:&HashMap<String,crate::AST::CtValue>,
    enums:&HashMap<String,bool>,
    resolved_args: Option<Vec<ResolvedModuleArg>>,
) -> Option<AliasExpansion> {
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
    let source_items: &[Item] = if info.source_module == consumer_module {
        &[]
    } else {
        &info.source_items
    };
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
                    .map(GenericModuleParam::name)
                    .collect::<Vec<_>>()
                    .join(", "),
                template.params.len(),
            ),
            Some(alias.span),
        ));
        return None;
    }
    let resolved_args = match resolved_args {
        Some(args) => args,
        None => resolve_args(alias, info, traits, funcs, globals, enums, diags)?,
    };
    let mut type_args = HashMap::new();
    let mut value_args = HashMap::new();
    for (param, arg) in info.params.iter().zip(&resolved_args) {
        match (param, arg) {
            (ResolvedModuleParam::Type { name, .. }, ResolvedModuleArg::Type(ty)) => {
                type_args.insert(name.clone(), ty.clone());
            }
            (ResolvedModuleParam::Value { name, .. }, ResolvedModuleArg::Value(value, _)) => {
                value_args.insert(name.clone(), value.clone());
            }
            _ => {}
        }
    }
    let mut definition_types = HashMap::new();
    for item in source_items {
        let name = match item {
            Item::Struct(def) => Some(&def.name),
            Item::Enum(def) => Some(&def.name),
            Item::Trait(def) => Some(&def.name),
            Item::Tag(def) => Some(&def.name),
            _ => None,
        };
        if let Some(name) = name {
            definition_types.insert(name.clone(), Type::Named(format!("{}__{}", alias.name, name)));
        }
    }
    for item in &template.body {
        let name = match item {
            Item::Struct(def) => Some(&def.name),
            Item::Enum(def) => Some(&def.name),
            Item::Trait(def) => Some(&def.name),
            Item::Tag(def) => Some(&def.name),
            _ => None,
        };
        if let Some(name) = name {
            definition_types.insert(
                name.clone(),
                Type::Named(format!("{}__{}", alias.name, name)),
            );
        }
    }
    definition_types.extend(type_args.clone());

    // Constants specialize in declaration order. Their evaluated definition-site
    // values are then available to later constants and every template function.
    let mut definition_values = if info.source_module == consumer_module {
        HashMap::new()
    } else {
        info.source_values.clone()
    };
    definition_values.extend(value_args);
    let mut declarations = Vec::new();
    for item in source_items {
        match item {
            Item::Struct(def) => declarations.push(Item::Struct(specialize_struct(
                def,
                &alias.name,
                &[],
                &[],
                &definition_types,
                &definition_values,
            ))),
            Item::Enum(def) => declarations.push(Item::Enum(specialize_enum(
                def,
                &alias.name,
                &[],
                &[],
                &definition_types,
                &definition_values,
            ))),
            Item::Trait(def) => declarations.push(Item::Trait(specialize_trait(
                def, &[], &[], &definition_types, &definition_values,
            ))),
            Item::Tag(def) => declarations.push(Item::Tag(specialize_tag(
                def, &definition_types, &definition_values,
            ))),
            Item::Impl(def) => declarations.push(Item::Impl(specialize_impl(
                def, &[], &[], &definition_types, &definition_values,
            ))),
            Item::ErrorConv(def) => declarations.push(Item::ErrorConv(specialize_error_conv(
                def, &definition_types, &definition_values,
            ))),
            _ => {}
        }
    }
    for item in &template.body {
        let Item::Const(source) = item else { continue };
        let mut value = source.value.clone();
        substitute_expr(&mut value, &definition_types, &definition_values);
        let evaluated = crate::Comptime::evaluate(
            &value,
            funcs,
            &HashSet::new(),
            Path::new("."),
            &definition_values,
        );
        if let Ok(evaluated) = evaluated {
            definition_values.insert(source.name.clone(), evaluated);
        }
        declarations.push(Item::Const(crate::AST::ConstDef {
            span: source.span,
            name: format!("{}__{}", alias.name, source.name),
            name_span: source.name_span,
            value,
            meta: source.meta.clone(),
            attrs: source.attrs.clone(),
            rust_kind: source.rust_kind,
            is_comptime: source.is_comptime,
            ct: source.ct.clone(),
            ty: source.ty.as_ref().map(|ty| specialize_module_type(ty, &definition_types, &definition_values)),
            is_persist: source.is_persist,
            persist_span: source.persist_span,
        }));
    }
    for item in &template.body {
        if let Item::Struct(def) = item {
            declarations.push(Item::Struct(specialize_struct(
                def,
                &alias.name,
                &info.params,
                &resolved_args,
                &definition_types,
                &definition_values,
            )));
        }
        if let Item::Enum(def) = item {
            declarations.push(Item::Enum(specialize_enum(
                def,
                &alias.name,
                &info.params,
                &resolved_args,
                &definition_types,
                &definition_values,
            )));
        }
        if let Item::Trait(def) = item {
            declarations.push(Item::Trait(specialize_trait(
                def, &info.params, &resolved_args, &definition_types, &definition_values,
            )));
        }
        if let Item::Tag(def) = item {
            declarations.push(Item::Tag(specialize_tag(
                def, &definition_types, &definition_values,
            )));
        }
        if let Item::Impl(def) = item {
            declarations.push(Item::Impl(specialize_impl(
                def, &info.params, &resolved_args, &definition_types, &definition_values,
            )));
        }
        if let Item::ErrorConv(def) = item {
            declarations.push(Item::ErrorConv(specialize_error_conv(
                def, &definition_types, &definition_values,
            )));
        }
        if let Item::Test(def) = item {
            declarations.push(Item::Test(specialize_test(
                def, &alias.name, &definition_types, &definition_values,
            )));
        }
        if let Item::Bench(def) = item {
            declarations.push(Item::Bench(specialize_bench(
                def, &alias.name, &definition_types, &definition_values,
            )));
        }
    }

    let mut body: Vec<Item> = source_items
        .iter()
        .filter_map(|item| match item {
            Item::Func(func) => Some(Item::Func(specialize_func(
                func.clone(),
                &[],
                &[],
                &definition_types,
                &definition_values,
            ))),
            _ => None,
        })
        .collect();
    body.extend(template
        .body
        .iter()
        .filter_map(|item| match item {
            Item::Func(func) => Some(Item::Func(specialize_func(
                func.clone(),
                &info.params,
                &resolved_args,
                &definition_types,
                &definition_values,
            ))),
            Item::CodeModule(module) => Some(Item::CodeModule(specialize_nested_code_module(module, &info.params, &resolved_args, &definition_types, &definition_values))),
            Item::Const(_) => None,
            _ => None,
        })
        .collect::<Vec<_>>());

    // BODY1: nested generic templates close over the outer instance. Resolve
    // their aliases now, while the outer type/value environment is concrete.
    let nested_defs: Vec<GenericModuleDef> = template.body.iter().filter_map(|item| {
        let Item::GenericModule(def) = item else { return None };
        Some(specialize_nested_template_outer(def, &definition_types, &definition_values))
    }).collect();
    if !nested_defs.is_empty() {
        let mut nested_traits = TraitRegistry::default();
        for def in &nested_defs { nested_traits.register_items(&def.body, diags); }
        let nested_enums: HashMap<String, bool> = nested_defs.iter().flat_map(|def| def.body.iter()).filter_map(|item| {
            let Item::Enum(def) = item else { return None };
            Some((def.name.clone(), def.variants.iter().all(|v| matches!(v.payload, VariantPayload::Unit))))
        }).collect();
        let nested_funcs: HashMap<String, &Func> = nested_defs.iter().flat_map(|def| def.body.iter()).filter_map(|item| {
            let Item::Func(def) = item else { return None }; Some((def.name.clone(), def))
        }).collect();
        let nested_templates: HashMap<String, TemplateInfo> = nested_defs.iter().map(|def| {
            (def.name.clone(), TemplateInfo { def: clone_generic_module_def(def), definition_id: format!("{}::{}", alias.name, def.name),
                params: resolve_params(def, &nested_traits, &nested_enums, diags),
                source_module: consumer_module, source_items: Vec::new(), source_values: definition_values.clone() })
        }).collect();
        let nested_alias_defs: Vec<ModuleAliasDef> = template.body.iter().filter_map(|item| {
            let Item::ModuleAlias(def) = item else { return None };
            Some(specialize_nested_alias_outer(def, &definition_types, &definition_values))
        }).collect();
        let nested_aliases: HashMap<String, &ModuleAliasDef> = nested_alias_defs.iter()
            .map(|def| (def.name.clone(), def)).collect();
        let mut ordered: Vec<&ModuleAliasDef> = nested_alias_defs.iter().collect();
        ordered.sort_by_key(|def| local_alias_depth(def, &nested_aliases));
        for nested_alias in ordered {
            let Some(mut resolved_alias) = resolve_local_alias(nested_alias, &nested_aliases, &nested_templates, diags) else { continue };
            resolved_alias.name = format!("{}__{}", alias.name, nested_alias.name);
            if let Some(expansion) = expand_alias(&resolved_alias, consumer_module, &nested_templates,
                diags, &nested_traits, &nested_funcs, &definition_values, &nested_enums, None) {
                body.push(Item::CodeModule(expansion.module));
                declarations.extend(expansion.declarations);
            }
        }
    }
    Some(AliasExpansion {
        module: CodeModule {
            name: alias.name.clone(),
            name_span: alias.name_span,
            is_pub: alias.is_pub,
            is_package_pub: alias.is_package_pub,
            body: Some(body),
            web_target: None,
            instance_identity: None,
            span: alias.span,
        },
        declarations,
    })
}

fn report_generic_module_cycles(items: &[Item], diags: &mut Vec<Diagnostic>) -> bool {
    fn collect_alias_edges(items: &[Item], edges: &mut Vec<(String, crate::Diagnostics::Span)>) {
        for item in items {
            match item {
                Item::ModuleAlias(alias) => {
                    edges.push((alias.target.clone(), alias.target_span));
                }
                Item::GenericModule(module) => collect_alias_edges(&module.body, edges),
                _ => {}
            }
        }
    }

    let mut graph: HashMap<String, Vec<(String, crate::Diagnostics::Span)>> = HashMap::new();
    for item in items {
        match item {
            Item::ModuleAlias(alias) => {
                graph
                    .entry(alias.name.clone())
                    .or_default()
                    .push((alias.target.clone(), alias.target_span));
            }
            Item::GenericModule(module) => {
                let mut edges = Vec::new();
                collect_alias_edges(&module.body, &mut edges);
                graph.insert(module.name.clone(), edges);
            }
            _ => {}
        }
    }
    for edges in graph.values_mut() {
        edges.sort_by(|a, b| a.0.cmp(&b.0));
    }

    fn visit(
        node: &str,
        graph: &HashMap<String, Vec<(String, crate::Diagnostics::Span)>>,
        state: &mut HashMap<String, u8>,
        stack: &mut Vec<String>,
        reported: &mut HashSet<String>,
        diags: &mut Vec<Diagnostic>,
    ) {
        state.insert(node.to_string(), 1);
        stack.push(node.to_string());
        if let Some(edges) = graph.get(node) {
            for (target, span) in edges {
                if !graph.contains_key(target) {
                    continue;
                }
                match state.get(target).copied().unwrap_or(0) {
                    0 => visit(target, graph, state, stack, reported, diags),
                    1 => {
                        let start = stack.iter().position(|name| name == target).unwrap_or(0);
                        let mut chain = stack[start..].to_vec();
                        chain.push(target.clone());
                        let text = chain.join(" -> ");
                        if reported.insert(text.clone()) {
                            diags.push(Diagnostic::error(
                                "E0855",
                                format!("generic module instantiation forms a cycle: {text}"),
                                "module aliases must form an acyclic dependency graph so specialization reaches one stable result".to_string(),
                                format!("break the cycle: {text}"),
                                Some(*span),
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        stack.pop();
        state.insert(node.to_string(), 2);
    }

    let mut nodes: Vec<String> = graph.keys().cloned().collect();
    nodes.sort();
    let mut state = HashMap::new();
    let mut stack = Vec::new();
    let mut reported = HashSet::new();
    for node in nodes {
        if state.get(&node).copied().unwrap_or(0) == 0 {
            visit(
                &node,
                &graph,
                &mut state,
                &mut stack,
                &mut reported,
                diags,
            );
        }
    }
    !reported.is_empty()
}

fn resolve_local_alias(
    alias: &ModuleAliasDef,
    aliases: &HashMap<String, &ModuleAliasDef>,
    templates: &HashMap<String, TemplateInfo>,
    diags: &mut Vec<Diagnostic>,
) -> Option<ModuleAliasDef> {
    let mut current = alias;
    while let Some(next) = aliases.get(&current.target).copied() {
        if !current.args.is_empty() {
            diags.push(Diagnostic::error(
                "E0851",
                format!(
                    "module alias `{}` passes {} argument(s) but alias `{}` expects 0",
                    current.name,
                    current.args.len(),
                    current.target
                ),
                "an alias-to-alias link reuses an already-bound module instance".to_string(),
                format!("remove the arguments and write `module {} = {}`", current.name, current.target),
                Some(current.span),
            ));
            return None;
        }
        current = next;
    }
    if !templates.contains_key(&current.target) {
        return Some(ModuleAliasDef {
            name: alias.name.clone(),
            name_span: alias.name_span,
            is_pub: alias.is_pub,
            is_package_pub: alias.is_package_pub,
            target: current.target.clone(),
            target_span: current.target_span,
            args: current.args.clone(),
            span: alias.span,
        });
    }
    Some(ModuleAliasDef {
        name: alias.name.clone(),
        name_span: alias.name_span,
        is_pub: alias.is_pub,
        is_package_pub: alias.is_package_pub,
        target: current.target.clone(),
        target_span: current.target_span,
        args: current.args.clone(),
        span: alias.span,
    })
}

fn local_alias_depth(alias: &ModuleAliasDef, aliases: &HashMap<String, &ModuleAliasDef>) -> usize {
    let mut depth = 0;
    let mut current = alias;
    while let Some(next) = aliases.get(&current.target).copied() {
        depth += 1;
        current = next;
    }
    depth
}

fn alias_chain_contains(
    alias: &ModuleAliasDef,
    aliases: &HashMap<String, &ModuleAliasDef>,
    names: &HashSet<String>,
) -> bool {
    let mut current = alias;
    while let Some(next) = aliases.get(&current.target).copied() {
        if names.contains(&next.name) {
            return true;
        }
        current = next;
    }
    false
}

/// D-GENMOD2=A: expand every `ModuleAlias` in each module's item list into a
/// concrete `CodeModule` using the corresponding `GenericModule` template.
/// Templates and aliases are removed from the item list after expansion.
pub(crate) fn expand_generic_module_aliases(
    bundle: &mut ProgramBundle,
    diags: &mut Vec<Diagnostic>,
) {
    let package_identity = bundle.project_root.to_string_lossy().replace('\\', "/");
    let template_snapshots: Vec<HashMap<String, TemplateInfo>> = bundle
        .modules
        .iter()
        .enumerate()
        .map(|(source_module, module)| {
            let mut traits = TraitRegistry::default();
            traits.register_items(&module.items, diags);
            let enums: HashMap<String, bool> = module
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Enum(def) => Some((
                        def.name.clone(),
                        def.variants
                            .iter()
                            .all(|variant| matches!(variant.payload, VariantPayload::Unit)),
                    )),
                    _ => None,
                })
                .collect();
            let funcs: HashMap<String, &Func> = module
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Func(def) => Some((def.name.clone(), def)),
                    _ => None,
                })
                .collect();
            let mut source_values = HashMap::new();
            for item in &module.items {
                if let Item::Const(def) = item {
                    if let Ok(value) = crate::Comptime::evaluate(
                        &def.value,
                        &funcs,
                        &HashSet::new(),
                        Path::new("."),
                        &source_values,
                    ) {
                        source_values.insert(def.name.clone(), value);
                    }
                }
            }
            let source_items = clone_definition_items(&module.items);
            module
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::GenericModule(gm) => {
                        Some((
                            gm.name.clone(),
                            TemplateInfo {
                                def: clone_generic_module_def(gm),
                                definition_id: format!("{}::{}::{}", package_identity, module.path.to_string_lossy().replace('\\', "/"), gm.name),
                                params: resolve_params(gm, &traits, &enums, diags),
                                source_module,
                                source_items: clone_definition_items(&source_items),
                                source_values: source_values.clone(),
                            },
                        ))
                    }
                    _ => None,
                })
                .collect()
        })
        .collect();
    let module_aliases: Vec<String> = bundle
        .modules
        .iter()
        .map(|module| module.alias.clone())
        .collect();
    // `use alias.Item` has its own span and therefore no `import_targets`
    // entry. Resolve it through the namespace import which established
    // `alias`, exactly like the later ordinary-import registration pass.
    let import_bindings: Vec<HashMap<String, usize>> = bundle
        .modules
        .iter()
        .enumerate()
        .map(|(module_idx, module)| {
            module
                .imports
                .iter()
                .filter(|import| !matches!(import.kind, ImportKind::Unqualified { .. }))
                .filter_map(|import| {
                    bundle
                        .import_targets
                        .get(&(module_idx, import.span))
                        .copied()
                        .map(|target| (import.import_alias(), target))
                })
                .collect()
        })
        .collect();

    let mut bundle_instances: HashMap<ModuleInstanceKey, String> = HashMap::new();
    let mut bundle_instance_nominals: HashMap<String, Vec<String>> = HashMap::new();
    let mut fingerprint_keys: HashMap<String, Vec<u8>> = HashMap::new();

    for (module_idx, module) in bundle.modules.iter_mut().enumerate() {
        if report_generic_module_cycles(&module.items, diags) {
            continue;
        }
        let mut traits=TraitRegistry::default();traits.register_items(&module.items,diags);
        let enums:HashMap<String,bool>=module.items.iter().filter_map(|item|if let Item::Enum(def)=item{Some((def.name.clone(),def.variants.iter().all(|v|matches!(v.payload,VariantPayload::Unit))))}else{None}).collect();
        let funcs:HashMap<String,&Func>=module.items.iter().filter_map(|item|if let Item::Func(f)=item{Some((f.name.clone(),f))}else{None}).collect();
        let mut globals:HashMap<String,crate::AST::CtValue>=HashMap::new();for item in &module.items{if let Item::Const(c)=item{if let Ok(value)=crate::Comptime::evaluate(&c.value,&funcs,&HashSet::new(),Path::new("."),&globals){globals.insert(c.name.clone(),value);}}}
        // Parameter declarations were resolved once in the immutable prepass.
        // Reuse that result locally so invalid declarations emit one diagnostic.
        let mut templates = template_snapshots[module_idx].clone();
        let mut denied_templates = HashSet::new();

        for import in &mut module.imports {
            let ImportKind::Unqualified {
                module_alias,
                items,
                ..
            } = &mut import.kind
            else {
                continue;
            };
            let Some(source_idx) = import_bindings[module_idx].get(module_alias).copied() else {
                continue;
            };
            let mut consumed = HashSet::new();
            for (original, alias) in items.iter() {
                let Some(source) = template_snapshots[source_idx].get(original) else {
                    continue;
                };
                consumed.insert(original.clone());
                let local = alias.as_deref().unwrap_or(original);
                if !source.def.is_pub && !source.def.is_package_pub {
                    denied_templates.insert(local.to_string());
                    diags.push(Diagnostic::error(
                        "E0609",
                        format!("`{original}` is private in module `{}`", module_aliases[source_idx]),
                        "only `pub` items can be brought into scope with `use`".to_string(),
                        format!("add `pub` before `module {original}` in the defining file"),
                        Some(import.span),
                    ));
                    continue;
                }
                templates.insert(local.to_string(), source.clone());
            }
            // Generic templates are compile-time namespace inputs, not runtime
            // values for the ordinary unqualified-import pass below.
            items.retain(|(original, _)| !consumed.contains(original));
        }

        let aliases: HashMap<String, &ModuleAliasDef> = module
            .items
            .iter()
            .filter_map(|item| match item {
                Item::ModuleAlias(alias) => Some((alias.name.clone(), alias)),
                _ => None,
            })
            .collect();
        let type_aliases: HashMap<String, Type> = module.items.iter().filter_map(|item| {
            let Item::TypeAlias(alias) = item else { return None };
            Some((alias.name.clone(), alias.target.clone()))
        }).collect();
        let mut projections = HashMap::new();
        for alias in aliases.values().copied() {
            if aliases.contains_key(&alias.target) {
                let mut terminal = aliases[&alias.target];
                while let Some(next) = aliases.get(&terminal.target).copied() {
                    terminal = next;
                }
                projections.insert(alias.name.clone(), terminal.name.clone());
            }
        }

        // Expand aliases into CodeModules, collect separately.
        let mut expansions: Vec<(usize, AliasExpansion)> = Vec::new();
        let mut ordered_aliases: Vec<(usize, &ModuleAliasDef)> = module
            .items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| match item {
                Item::ModuleAlias(alias) => Some((idx, alias)),
                _ => None,
            })
            .collect();
        ordered_aliases.sort_by_key(|(_, alias)| local_alias_depth(alias, &aliases));
        let mut invalid_aliases = HashSet::new();
        for (idx, alias) in ordered_aliases {
            if alias_chain_contains(alias, &aliases, &invalid_aliases) {
                continue;
            }
                let Some(resolved) = resolve_local_alias(alias, &aliases, &templates, diags) else {
                    invalid_aliases.insert(alias.name.clone());
                    continue;
                };
                if denied_templates.contains(&resolved.target) {
                    invalid_aliases.insert(alias.name.clone());
                    continue;
                }
                // A valid forward alias is a projection of the already-bound
                // terminal instance, not a second specialization.
                if aliases.contains_key(&alias.target) {
                    continue;
                }
                let Some(info) = templates.get(&resolved.target) else {
                    invalid_aliases.insert(alias.name.clone());
                    continue;
                };
                let Some(args) = resolve_args(&resolved, info, &traits, &funcs, &globals, &enums, diags) else {
                    invalid_aliases.insert(alias.name.clone());
                    continue;
                };
                let key = instance_key(info, &args, &type_aliases);
                if let Some(canonical) = bundle_instances.get(&key) {
                    projections.insert(alias.name.clone(), canonical.clone());
                    continue;
                }
                if let Some(mut cm) = expand_alias(&resolved, module_idx, &templates, diags,&traits,&funcs,&globals,&enums, Some(args)) {
                    let identity = instance_identity(&key, info);
                    register_instance_fingerprint(&mut fingerprint_keys, &identity, alias.span);
                    cm.module.instance_identity = Some(identity);
                    bundle_instance_nominals.insert(alias.name.clone(), cm.declarations.iter().filter_map(|item| match item {
                        Item::Struct(def) => Some(def.name.clone()),
                        Item::Enum(def) => Some(def.name.clone()),
                        _ => None,
                    }).collect());
                    bundle_instances.insert(key, alias.name.clone());
                    expansions.push((idx, cm));
                } else {
                    invalid_aliases.insert(alias.name.clone());
                }
        }

        // Replace/erase: iterate in reverse to preserve indices.
        // For each alias, replace it with the expanded CodeModule.
        // GenericModule items are erased (replaced with nothing).
        // We need to:
        // 1. Replace each ModuleAlias with its CodeModule expansion (collected above)
        // 2. Remove all GenericModule items
        let mut declarations = Vec::new();
        for (idx, expansion) in expansions {
            module.items[idx] = Item::CodeModule(expansion.module);
            declarations.extend(expansion.declarations);
        }
        // Collapse forward-alias chains through the applicative canonical
        // instance selected above.
        for alias in projections.clone().keys() {
            let mut canonical = projections[alias].clone();
            let mut seen = HashSet::new();
            while seen.insert(canonical.clone()) {
                let Some(next) = projections.get(&canonical) else { break };
                canonical = next.clone();
            }
            projections.insert(alias.clone(), canonical);
        }
        // Resolve projected nominal spellings before registration/codegen. No
        // duplicate declaration or zero-parameter surface alias leaks out.
        let projection_types: HashMap<String, Type> = projections.iter().flat_map(|(alias, canonical)| {
            let prefix = format!("{canonical}__");
            bundle_instance_nominals.get(canonical).into_iter().flatten().filter_map(move |canonical_name| {
                canonical_name.strip_prefix(&prefix).map(|suffix| {
                    (format!("{alias}__{suffix}"), Type::Named(canonical_name.clone()))
                })
            })
        }).collect();
        for (alias, canonical) in &projections {
            let names = HashSet::from([alias.clone()]);
            for item in &mut module.items {
                if let Item::Func(func) = item {
                    rewrite_inline_calls_stmts(&mut func.body, &names, canonical);
                }
            }
        }
        for item in &mut module.items {
            if let Item::Func(func) = item {
                for param in &mut func.params {
                    param.ty = crate::Generics::substitute_type(&param.ty, &projection_types);
                }
                if let Some(ret) = &mut func.return_type {
                    *ret = crate::Generics::substitute_type(ret, &projection_types);
                }
                substitute_stmts(&mut func.body, &projection_types, &HashMap::new());
            }
        }
        module
            .items
            .retain(|i| !matches!(i, Item::GenericModule(_) | Item::ModuleAlias(_)));
        module.items.extend(declarations);
        debug_assert!(!module.items.iter().any(|item| matches!(item, Item::ModuleAlias(_))));
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
            | Stmt::Shield { body: inner, .. }
            | Stmt::Off { body: inner, .. }
            | Stmt::DebugOnly { body: inner, .. }
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
        Expr::Ident(name, _) => {
            if siblings.contains(name) {
                *name = modname.to_string();
            }
        }
        Expr::Char(_, _)
        | Expr::Int(_, _, _)
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
    let mut diags = super::BudgetSpecs::validate_bundle(bundle);
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
    let mut states: Vec<ModuleState> = bundle
        .modules
        .iter()
        .map(|m| ModuleState {
            module_path: m.display.clone(),
            func_spans: HashMap::new(),
            const_spans: HashMap::new(),
            import_spans: HashMap::new(),
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

    // Generic-instance declarations have one AST/codegen owner, while every
    // consumer registry receives the same nominal metadata. This is not a
    // declaration clone: generated Rust/TIR still sees the owner item once.
    let shared_instance_nominals: Vec<(usize, Item)> = bundle.modules.iter().enumerate().flat_map(|(owner, module)| {
        let prefixes: Vec<String> = module.items.iter().filter_map(|item| match item {
            Item::CodeModule(cm) => Some(format!("{}__", cm.name)),
            _ => None,
        }).collect();
        module.items.iter().filter_map(move |item| match item {
            Item::Struct(def) if prefixes.iter().any(|prefix| def.name.starts_with(prefix)) => Some((owner, Item::Struct(clone_struct(def)))),
            Item::Enum(def) if prefixes.iter().any(|prefix| def.name.starts_with(prefix)) => Some((owner, Item::Enum(clone_enum(def)))),
            _ => None,
        })
    }).collect();
    for (owner, item) in &shared_instance_nominals {
        for (consumer, st) in states.iter_mut().enumerate() {
            if consumer == *owner { continue; }
            match item {
                Item::Struct(def) => {
                    register_struct(def, &mut st.registry, &mut st.structs, &mut diags, &st.funcs, &st.consts);
                    st.type_pub.insert(def.name.clone(), def.is_pub && !def.is_package_pub);
                    st.type_pkg_pub.insert(def.name.clone(), def.is_package_pub);
                }
                Item::Enum(def) => {
                    register_enum(def, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub.insert(def.name.clone(), def.is_pub && !def.is_package_pub);
                    st.type_pkg_pub.insert(def.name.clone(), def.is_package_pub);
                }
                _ => unreachable!(),
            }
        }
    }

    // D-METADERIVE1=A orphan law needs a bundle-wide provider view: a derive
    // may be supplied by the entry module for an imported type, or imported
    // for an entry-local type.  Clone provider bodies/helpers before mutating
    // modules so expansion can attach generated items beside the target type.
    let derive_providers: Vec<(
        usize,
        String,
        String,
        Vec<crate::AST::Stmt>,
        HashMap<String, Func>,
    )> = bundle
        .modules
        .iter()
        .enumerate()
        .flat_map(|(origin, module)| {
            let helpers: HashMap<String, Func> = module
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Func(f) => Some((f.name.clone(), f.clone())),
                    _ => None,
                })
                .collect();
            module.items.iter().filter_map(move |item| match item {
                Item::UserDerive(d) => Some((
                    origin,
                    d.trait_name.clone(),
                    d.type_param.clone(),
                    d.body.clone(),
                    helpers.clone(),
                )),
                _ => None,
            })
        })
        .collect();

    // D-MARK-VOCAB1 (card #518): the dynamic half of the `@` contract-plane
    // vocabulary — every `derive T.Name { … }` provider in the bundle, not
    // just this module's own, per the same bundle-wide orphan-rule view as
    // `derive_providers` above.
    let known_derive_names: HashSet<String> =
        derive_providers.iter().map(|(_, name, _, _, _)| name.clone()).collect();

    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        super::Protocol::expand_module_protocols(&mut module.items, &mut diags);
        // D-DOTSCOPE1: validate contextual `.member { … }` scope statements
        // against each marker's declared vocabulary (E0614/E0615/E0616/E0617/E0618).
        diags.extend(super::ScopeMembers::check(&module.items));
        // D-FIELDPOL1: computed-field cycle check (E0338) + `self.field`
        // rewrite + synthesized getter methods, before anything else.
        process_computed_fields(&mut module.items, &mut diags);
        // D-VALIDATE1 (card #506): `validate { … }` block shape check +
        // synthesized `Type.validate(value)`, same pre-registration timing.
        process_validate_blocks(&mut module.items, &mut diags);
        // D-PATCH1: synthetic `T.Patch` before struct registration.
        inject_patchable_types(&mut module.items, &mut diags);
        // Card #436: `CFFI::assemble` (jetpack crate) drains every
        // `#Extern`/`#Bindgen module` out of its declaring file and re-homes
        // it in a synthetic per-lib module (`<c.lib>`) with an empty
        // registry of its own — so a struct/enum/distinct declared in an
        // ordinary file was NEVER visible to `is_c_abi_type`'s `Type::Named`
        // lookup (`c_named_type_ok`, Sema/FFI.rs), and every named type was
        // silently rejected at the C boundary regardless of its shape. Real
        // modules are always processed before any synthetic one (assemble
        // only appends), so by this iteration every preceding module's
        // registry is already fully populated; merge them once here so a
        // same-project named type resolves. Type names are unique
        // program-wide (a duplicate definition is its own error elsewhere),
        // so this union is sound.
        let ffi_named_types: Option<HashMap<String, TypeDef>> = if module
            .items
            .iter()
            .any(|i| matches!(i, Item::CModule(_)))
        {
            Some(
                states[..idx]
                    .iter()
                    .flat_map(|s| s.registry.types.iter())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )
        } else {
            None
        };
        let st = &mut states[idx];
        for import in &module.imports {
            if !matches!(import.kind, crate::AST::ImportKind::Unqualified { .. }) {
                st.import_spans.insert(import.import_alias(), import.alias_span);
            }
        }
        for item in &module.items {
            match item {
                Item::Func(f) => {
                    st.func_spans.insert(f.name.clone(), f.name_span);
                }
                Item::Const(c) => {
                    st.const_spans.insert(c.name.clone(), c.name_span);
                }
                _ => {}
            }
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
                    if let Some(meta) = &c.meta {
                        diags.extend(CheckerCore::check_meta_attr_fields(meta));
                    }
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
                                false,
                            );
                        }
                    }
                }
                Item::CModule(cm) => {
                    // Card #436: check named C-ABI types (struct/enum/distinct)
                    // against the merged cross-file view built above, not the
                    // synthetic module's own (empty) registry. See the comment
                    // at `ffi_named_types`'s construction.
                    let merged_registry = ffi_named_types.as_ref().map(|extra| {
                        let mut types = st.registry.types.clone();
                        for (k, v) in extra {
                            types.entry(k.clone()).or_insert_with(|| v.clone());
                        }
                        TypeRegistry {
                            types,
                            computed_fields: st.registry.computed_fields.clone(),
                        }
                    });
                    let check_registry = merged_registry.as_ref().unwrap_or(&st.registry);
                    if check_c_module(cm, check_registry, &mut diags) {
                        for ef in &cm.functions {
                            register_extern_fn(
                                ef,
                                &mut st.funcs,
                                check_registry,
                                &st.consts,
                                &mut diags,
                                true,
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
                                st.func_spans.insert(mangled.clone(), f.name_span);
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
                Item::GenericModule(_) | Item::ModuleAlias(_) => {}
            }
        }
        // D-METADERIVE1=A: user-derive expansion — run after struct/func registration so
        // derive bodies can call helper functions and access TypeInfo. Re-entry (D-CTCODEGEN1=A):
        // emitted fragments go through the full lexer→parser pipeline and are appended as items.
        {
            if !derive_providers.is_empty() {
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

                let mut new_items: Vec<Item> = Vec::new();

                for s in &struct_infos {
                    for (derive_name, derive_span) in &s.derives {
                        // Prefer an entry-local provider, then one beside the target.
                        // Remaining imported/imported pairs violate the orphan law:
                        // either provider or target must be entry-local.
                        let provider = derive_providers
                            .iter()
                            .filter(|(_, name, _, _, _)| name == derive_name)
                            .min_by_key(|(origin, _, _, _, _)| {
                                if *origin == 0 {
                                    0
                                } else if *origin == idx {
                                    1
                                } else {
                                    2
                                }
                            });
                        let Some((provider_idx, _, type_param, body, helper_funcs)) = provider else {
                            continue;
                        };
                        if idx > 0 && *provider_idx > 0 {
                            diags.push(Diagnostic::error(
                                "E2711",
                                format!(
                                    "derive orphan rule: neither `derive T.{}` nor `{}` is local",
                                    derive_name, s.name
                                ),
                                "a generated implementation is owned locally only when the derive provider or target type lives in the entry module".to_string(),
                                format!(
                                    "define `derive T.{}` or `{}` in the entry module",
                                    derive_name, s.name
                                ),
                                // The violating marker belongs to an imported source file;
                                // the bundled diagnostic currently renders against the entry
                                // file, so omit a misleading entry-file caret.
                                None,
                            ));
                            continue;
                        }
                        let actual_funcs: HashMap<String, &Func> = helper_funcs
                            .iter()
                            .map(|(name, func)| (name.clone(), func))
                            .collect();
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
                                            let detail = lex_diags
                                                .first()
                                                .map(|d| d.what.as_str())
                                                .unwrap_or("the generated text could not be read");
                                            diags.push(Diagnostic::error(
                                                "E2710",
                                                format!(
                                                    "`derive T.{}` generated invalid Jet while expanding `#[{}]` on `{}`",
                                                    derive_name, derive_name, s.name
                                                ),
                                                format!(
                                                    "generated source did not pass the ordinary lexer and parser: {detail}"
                                                ),
                                                "fix the `derive` body so every emitted fragment is valid Jet source".to_string(),
                                                Some(*derive_span),
                                            ));
                                            continue;
                                        }
                                        match crate::Parser::parse(&toks) {
                                            Ok(mut prog) => new_items.extend(prog.items.drain(..)),
                                            Err(parse_diags) => {
                                                let detail = parse_diags
                                                    .first()
                                                    .map(|d| d.what.as_str())
                                                    .unwrap_or("the generated text was not valid Jet");
                                                diags.push(Diagnostic::error(
                                                    "E2710",
                                                    format!(
                                                        "`derive T.{}` generated invalid Jet while expanding `#[{}]` on `{}`",
                                                        derive_name, derive_name, s.name
                                                    ),
                                                    format!(
                                                        "generated source did not pass the ordinary lexer and parser: {detail}"
                                                    ),
                                                    "fix the `derive` body so every emitted fragment is valid Jet source".to_string(),
                                                    Some(*derive_span),
                                                ));
                                            }
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

        // Defaults must exist before serde source expansion so Decode bodies
        // embed the evaluated value rather than re-evaluating at runtime.
        let serde_core_imports: HashMap<String, String> = module
            .imports
            .iter()
            .filter_map(|imp| Some((imp.import_alias(), imp.core_module_path()?)))
            .collect();
        let serde_base = module
            .path
            .parent()
            .map(|path| path.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        eval_default_markers(
            &mut module.items,
            &serde_base,
            &mut diags,
            &serde_core_imports,
        );
        // D-SERDE2=A/R11: built-in codecs re-enter as ordinary Jet source in
        // bundle builds too; this is the production multi-file path.
        super::Registration::expand_builtin_serde_items(&mut module.items, &mut diags);

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
        // D-MARK-VOCAB1 (card #518): a marker name outside the registered
        // `@`/`#` plane vocabulary is E0927, instead of silently doing
        // nothing (the parser accepts any PascalCase name structurally).
        diags.extend(check_marker_vocabulary(&module.items, &known_derive_names));
        // D-CLIFLAG1: validate `@[Cli]`-derived structs (E1305/E1306), same
        // timing as the serde pass above (trait registry must be built so
        // `Cli` is visible on `s.derives`).
        diags.extend(validate_cli_items(&module.items, &st.trait_reg));
        // D-MIGRATE1: schema diff pass (E0910) — runs after struct registration (I3).
        diags.extend(check_schema_migrations(
            &module.items,
            &bundle.project_root,
            &st.trait_reg,
        ));
    }

    // S62 E2401: delegation validation — check field exists and implements trait.
    // Runs after all m9 registrations so implements_trait is populated.
    for (idx, module) in bundle.modules.iter().enumerate() {
        let st = &states[idx];
        for item in &module.items {
            if let Item::Impl(i) = item {
                if let (Some(trait_name), Some(field_name)) = (&i.trait_name, &i.delegation_field) {
                    if let Some(fields) = st.registry.struct_fields(&i.type_name) {
                        if let Some((_, _, field_ty, _)) =
                            fields.iter().find(|(n, _, _, _)| n == field_name)
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
    let mut top_level_embed_inputs = Vec::new();
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
            Some(&mut top_level_embed_inputs),
        );
    }
    bundle.comptime_inputs.extend(top_level_embed_inputs);

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
            if let Some(canonical) = st.code_modules.get(module_alias.as_str()) {
                // Inline module: items are mangled as `{alias}__{item}`.
                for (orig, alias_opt) in items {
                    let local = alias_opt.as_deref().unwrap_or(orig.as_str());
                    let mangled = format!("{}__{}", canonical, orig);
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
            // S12/D-S80-RUN1/D-CLIFLAG1: `run` is the only program entry name.
            // It is zero-arg (optionally `-> Void ?`), or one typed CLI-spec
            // parameter (`@[Cli]` struct / enum).
            if run_fn.params.is_empty() {
                if mode == CompileMode::Run
                    && run_fn
                        .return_type
                        .as_ref()
                        .is_some_and(|ret| !is_fallible_void_entry_return(ret))
                {
                    diags.push(Diagnostic::error(
                        "E0122",
                        "`run` returns the wrong kind of value".to_string(),
                        "`run` is where running starts; it either returns nothing or reports top-level errors with `Void ?`"
                            .to_string(),
                        "write `fn run() { ... }`, or `fn run() -> Void ? { ... }` if the entry uses `?`"
                            .to_string(),
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
    let mut reference_anchors = HashMap::new();
    let mut module_effect_summaries: Vec<(String, HashMap<String, EffectSummary>)> = Vec::new();
    // D-METHODMACRO1=A: top-level function names whose address was taken
    // anywhere in the bundle, accumulated across every module below; the
    // `@InlineAlways` address-taken pass (E0918) runs after the loop, once
    // this set is complete across the whole bundle.
    let mut global_addr_taken: HashSet<String> = HashSet::new();
    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        let mut local_summaries = HashMap::new();
        diags.extend(check_module_bodies(
            module,
            idx,
            &states,
            mode,
            freestanding,
            allow_impure,
            &mut local_summaries,
            &mut embed_inputs,
            &mut global_addr_taken,
            &mut reference_anchors,
        ));
        apply_effect_via(&module.items, &mut local_summaries, &mut Vec::new());
        effect_summaries.extend(local_summaries.clone());
        module_effect_summaries.push((module.alias.clone(), local_summaries));
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
        check_replayable_effects(&module.items, &solved, &mut diags);
    }
    check_region_caps(&effect_summaries, &solved, &mut diags);
    // D-EFF2: callback param effect bounds (E0747).
    check_callback_bounds(&effect_summaries, &solved, &mut diags);
    // U13 (D-JPK-SECRETCRYPTO1): a `core.vault.get` reach requires `Secret` in
    // the reaching function's own declared `#(…)` bound — E1264.
    for module in &bundle.modules {
        check_secret_grants(&module.items, &effect_summaries, &mut diags);
    }

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
    if bundle.modules.iter().any(|m| {
        m.items
            .iter()
            .any(|i| matches!(i, Item::Struct(s) if s.derives.iter().any(|(t, _)| t == "Cli")))
    }) {
        used_core.insert("core.args::spec".to_string());
    }
    // D-MEM1 S6: `Shared<T>`/`Pool<T>`/`Id<T>` need `CORELIB_PRELUDE`'s `jet_std`
    // module (`JetShared`/`JetPool`/`JetId`), but need no `use core.X` import to
    // reach them (unlike `tasks.spawn` etc.) — `collect_used_core` only walks
    // import aliases, so it never sees them. Same forced-insert shape as
    // D-CLIFLAG1 above; a cheap source-text scan is deliberately over-eager (a
    // false positive just includes the prelude when it wasn't strictly needed —
    // harmless, `#![allow(warnings)]` covers the unused code).
    if bundle.modules.iter().any(|m| {
        m.source.contains("Pool<")
            || m.source.contains("Shared<")
            || m.source.contains("Shared.new(")
            || m.source.contains("Id<")
    }) {
        used_core.insert("core.mem::pool_shared".to_string());
    }
    // D-VALIDATE1 (card #506): a `validate { … }` block synthesizes
    // `Type.validate(value)`, which returns `jet_std::FieldError` — same
    // forced-insert shape as D-CLIFLAG1/D-MEM1 S6 above, since declaring the
    // block needs no `use core.X` import to reach `CORELIB_PRELUDE`.
    if bundle.modules.iter().any(|m| {
        m.items
            .iter()
            .any(|i| matches!(i, Item::Struct(s) if !s.validate_block.is_empty()))
    }) {
        used_core.insert("core.validate::field_error".to_string());
    }
    bundle.used_core = used_core;
    apply_helper_layer_inference(bundle, &states, &usage_spans, &mut diags);
    let (public_summaries, public_solved) = qualified_effect_facts(&module_effect_summaries);
    (
        diags,
        super::Effects::SemIndexEffectFacts {
            summaries: public_summaries,
            solved: public_solved,
            reference_anchors,
        },
    )
}

fn qualified_effect_facts(
    modules: &[(String, HashMap<String, EffectSummary>)],
) -> (HashMap<String, EffectSummary>, HashMap<String, EffectSet>) {
    let mut locations = HashMap::<String, Vec<String>>::new();
    let aliases = modules.iter().map(|(alias, _)| alias.as_str()).collect::<HashSet<_>>();
    for (alias, summaries) in modules {
        for key in summaries.keys() {
            locations.entry(key.clone()).or_default().push(format!("{alias}::{key}"));
        }
    }
    let mut qualified = HashMap::new();
    for (alias, summaries) in modules {
        for (key, summary) in summaries {
            let mut summary = summary.clone();
            summary.edges = summary.edges.iter().map(|edge| {
                if edge == "__jet_panic__" { return edge.clone(); }
                if summaries.contains_key(edge) { return format!("{alias}::{edge}"); }
                if let Some((module, symbol)) = edge.split_once('.') {
                    if aliases.contains(module) { return format!("{module}::{symbol}"); }
                }
                locations.get(edge).and_then(|values| (values.len() == 1).then(|| values[0].clone())).unwrap_or_else(|| edge.clone())
            }).collect();
            qualified.insert(format!("{alias}::{key}"), summary);
        }
    }
    let mut solved = solve(&qualified);
    for (short, values) in locations.iter().filter(|(_, values)| values.len() == 1) {
        let qualified_key = &values[0];
        if let Some(summary) = qualified.get(qualified_key).cloned() {
            qualified.insert(short.clone(), summary);
        }
        if let Some(effects) = solved.get(qualified_key).cloned() {
            solved.insert(short.clone(), effects);
        }
    }
    (qualified, solved)
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
        // D-ENCSTREAM-SURFACE1=A: core.encoding now exports runtime value and
        // opaque handle types. A program may use those solely in annotations,
        // without a core method call for the expression walker to observe; the
        // generated Rust still needs the Core prelude that defines them.
        if imports.values().any(|module| module == "core.encoding" || module.starts_with("core.encoding.")) {
            used.insert("core.encoding::types".to_string());
        }
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
            | Stmt::Off { body, .. }
            | Stmt::DebugOnly { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::AssumeDet { body, .. } => collect_core_stmts(body, imports, used, spans),
            // D-SHIELDNAME1=A: parsed syntax, not raw source text, owns the
            // scheduler-prelude capability. This recognizes legal whitespace
            // such as `# Shield` and cannot be fooled by comments or strings.
            Stmt::Shield { body, span } => {
                note_core_usage(used, spans, "core.concurrency::shield", Some(*span));
                collect_core_stmts(body, imports, used, spans);
            }
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
            if recv_type.as_deref() == Some(crate::Syntax::SOLVER_TYPE) {
                note_core_usage(
                    used,
                    spans,
                    format!("{}::{method}", crate::Syntax::CORE_SOLVE_MODULE),
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
        | Expr::Copy(inner, _)
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
        // D-SHIFT1 (c7shift) / D-BINPAT1 (card #506 follow-up): a leaf
        // literal, no nested `Expr` to recurse into.
        | Expr::StrMatchLit(_, _)
        | Expr::BinMatchLit(_, _) => {}
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
    reference_anchors: &mut HashMap<(String, usize, usize), DefinitionAnchorFact>,
) -> Vec<Diagnostic> {
    let st = &states[module_idx];
    let mut diags = Vec::new();
    // D-MEM1/S7 (D-NOALLOC-SEM1=A): captured once — every function body check
    // below for this module gets the same file-scoped `policy no_alloc` state.
    let no_alloc = module.no_alloc_policy.is_some();
    let no_prelude = module.no_prelude;
    let (ct_funcs, ct_externs, ct_globals) = comptime_context_from_items(&module.items);
    let invalid_serde_impls = invalid_serde_derive_impls(&module.items, &st.trait_reg);
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
                    no_alloc,
                no_prelude,
                reference_anchors,
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
                        no_alloc,
                    no_prelude,
                    reference_anchors,
                    ));
                }
                // Trait impls nested in a struct are real method bodies too.
                // They inherit the struct's generic parameters, just as the
                // Rust impl emitted for them does.  Temporarily expose those
                // parameters to the ordinary body checker while preserving the
                // parsed method signature for codegen.
                for block in &mut s.trait_impls {
                    if matches!(
                        block.trait_name.as_str(),
                        crate::Generics::COMPARABLE | crate::Generics::EQUATABLE
                    ) {
                        // E0903 already rejected this built-in impl. Its body is
                        // not a valid checking context, so don't emit cascades.
                        continue;
                    }
                    for m in &mut block.methods {
                        let own_params = std::mem::take(&mut m.type_params);
                        m.type_params = if own_params.is_empty() {
                            s.type_params.clone()
                        } else {
                            own_params.clone()
                        };
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
                            no_alloc,
                            no_prelude,
                            reference_anchors,
                        ));
                        // Generated serde methods temporarily carry inherited,
                        // inferred bounds solely for sema. Their Rust generics
                        // belong on the enclosing impl, not on the method.
                        m.type_params = if matches!(block.trait_name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE) {
                            Vec::new()
                        } else {
                            own_params
                        };
                    }
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
                        no_alloc,
                    no_prelude,
                    reference_anchors,
                    ));
                }
                for block in &mut e.trait_impls {
                    if matches!(
                        block.trait_name.as_str(),
                        crate::Generics::COMPARABLE | crate::Generics::EQUATABLE
                    ) {
                        continue;
                    }
                    for m in &mut block.methods {
                        let own_params = std::mem::take(&mut m.type_params);
                        m.type_params = if own_params.is_empty() { e.type_params.clone() } else { own_params.clone() };
                        diags.extend(check_func_body_bundle(
                            m, module_idx, states, Some(&e.name), &ct_funcs, &ct_externs,
                            &ct_base_dir, &ct_globals, freestanding, allow_impure, summaries,
                            embed_inputs_out, global_addr_taken, no_alloc, no_prelude,
                            reference_anchors,
                        ));
                        m.type_params = if matches!(block.trait_name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE) { Vec::new() } else { own_params };
                    }
                }
            }
            Item::Impl(i) => {
                if i.trait_name.as_deref().is_some_and(|trait_name| {
                    matches!(
                        trait_name,
                        crate::Generics::COMPARABLE | crate::Generics::EQUATABLE
                    ) || (i.is_generated_serde
                        && invalid_serde_impls
                            .contains(&(i.type_name.clone(), trait_name.to_string())))
                }) {
                    continue;
                }
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
                        no_alloc,
                    no_prelude,
                    reference_anchors,
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
                    span: t.name_span,
                    is_pub: false,
                    is_package_pub: false,
                    external_type: None,
                    name: format!("__test_{}", t.name),
                    name_span: t.name_span,
                    meta: None,
                    type_params: Vec::new(),
                    params: t.params.clone(),
                    return_type: None,
                    return_type_span: None,
                    is_unsafe: false,
                    unsafe_reason: None,
                    unsafe_span: None,
                    is_pure: false,
                    is_reactive: false,
                    is_replayable: false,
                    replayable_span: None,
                    is_task: false,
                    task_span: None,
                    every: None,
                    is_must_use: false,
                    must_use_span: None,
                    maturity: None,
                    maturity_span: None,
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
                    inline_foreign: None,
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
                    no_alloc,
                no_prelude,
                reference_anchors,
                ));
                t.body = synthetic.body;
            }
            // D-BENCH1: a `#Bench` body type-checks exactly like a `#Test` body
            // (a bare statement list, no params, unit context) — only the mode
            // gate differs.
            Item::Bench(b) if mode == CompileMode::Bench => {
                let mut synthetic = Func {
                    span: b.name_span,
                    is_pub: false,
                    is_package_pub: false,
                    external_type: None,
                    name: format!("__bench_{}", b.name),
                    name_span: b.name_span,
                    meta: None,
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: None,
                    return_type_span: None,
                    is_unsafe: false,
                    unsafe_reason: None,
                    unsafe_span: None,
                    is_pure: false,
                    is_reactive: false,
                    is_replayable: false,
                    replayable_span: None,
                    is_task: false,
                    task_span: None,
                    every: None,
                    is_must_use: false,
                    must_use_span: None,
                    maturity: None,
                    maturity_span: None,
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
                    inline_foreign: None,
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
                    no_alloc,
                no_prelude,
                reference_anchors,
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
                                no_alloc,
                            no_prelude,
                            reference_anchors,
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
                    no_alloc,
                    no_prelude,
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
    // D-MEM1/S7 (D-NOALLOC-SEM1=A): this module's `policy no_alloc` state.
    no_alloc: bool,
    // D-PRELUDEX1=A: this file's `#NoPrelude` state.
    no_prelude: bool,
    reference_anchors: &mut HashMap<(String, usize, usize), DefinitionAnchorFact>,
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
        module_path: &st.module_path,
        reference_anchors,
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
        // S58 (E2-M13): an `#Unsafe fn` body is itself an audited region — its
        // statements may use low-level ops directly without a nested `#Unsafe`
        // block. Calling such a fn is gated separately (E3103).
        in_unsafe: f.is_unsafe,
        suppress_must_use: false,
        in_pure: f.is_pure,
        no_alloc,
        no_prelude,
        in_pre_clause: false,
        in_comptime: false,
        ret: f.return_type.clone(),
        fn_name: f.name.clone(),
        expected_type: None,
        iter_borrowed: HashSet::new(),
        freed_allocators: HashMap::new(),
        arena_views: HashMap::new(),
        list_views: HashMap::new(),
        string_views: HashMap::new(),
        uninit: HashMap::new(),
        borrow_ctx: false,
        allow_string_view_read: false,
        lambda_escapes: true,
        is_task_spawn: false,
        lambda_param_mutable: false,
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
    // D-SCHEDULE1 (card #505): a bad `#Every(…)` value is E0926.
    ck.diags.extend(check_every_marker(f));
    global_addr_taken.extend(std::mem::take(&mut ck.inline_addr_taken));
    // D-EXPANDCLI1 (card #183): roll this function's resolved ref-owner facts
    // into the whole-bundle accumulator for `jet inspect expand --facts refs`.
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
                if let Some(e) = parse_effect_name(name.as_str()) {
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

#[cfg(test)]
mod instance_collision_tests {
    use super::*;

    #[test]
    #[should_panic(expected = "internal compiler error: E0859 generic module instance fingerprint collision")]
    fn different_full_keys_with_same_digest_fail_closed_before_codegen() {
        let mut registry = HashMap::new();
        let first = crate::AST::ModuleInstanceIdentity { full_key: vec![1], fingerprint: "forced-digest".into() };
        let second = crate::AST::ModuleInstanceIdentity { full_key: vec![2], fingerprint: "forced-digest".into() };
        register_instance_fingerprint(&mut registry, &first, Span::new(1, 2));
        register_instance_fingerprint(&mut registry, &second, Span::new(3, 4));
    }

    #[test]
    fn generic_template_snapshot_never_filters_parser_admitted_items() {
        let source = r#"
module Everything<T> {
    const answer = 42
    tag Marked;
    trait Show { fn show(self) -> T }
    struct Boxed { value: T }
    enum Maybe { Empty Value(T) }
    impl Boxed.Show { fn show(self) -> T { return self.value } }
    fn id(value: T) -> T { return copy value }
    module Nested { fn nested() {} }
    module Inner<U> { fn inner(value: U) -> U { return copy value } }
    module IntInner = Inner<Int>
    #Test("smoke") { expect(answer == 42) }
    #Bench("work") { expect(answer == 42) }
}
fn run() {}
"#;
        let (tokens, lex) = crate::Lexer::lex(source);
        assert!(lex.is_empty(), "{lex:?}");
        let program = crate::Parser::parse(&tokens).expect("parser-admitted generic body");
        let template = program.items.iter().find_map(|item| match item {
            Item::GenericModule(template) => Some(template),
            _ => None,
        }).expect("generic template");
        let snapshot = clone_generic_module_def(template);
        assert_eq!(snapshot.body.len(), template.body.len());
        assert_eq!(
            crate::CanonicalAST::canonical_fragment(&snapshot.body),
            crate::CanonicalAST::canonical_fragment(&template.body),
        );
    }
}
