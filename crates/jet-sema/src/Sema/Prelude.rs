//! One compiler-owned opening of the readable Core prelude.

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{
    ErrorConvDef, Expr, Func, ImportDecl, ImportKind, Item, LoadedModule, Program, ProgramBundle,
    Stmt, TryConvert,
};
use jet_foundation::Prelude as CorePrelude;
use std::collections::HashMap;

const INTERNAL_PREFIX: &str = "__jet_prelude_";

pub(crate) const ERROR_PRELUDE_MODULE_PATH: &str = "Prelude/Errors.jet";
pub(crate) const ERROR_PRELUDE_SOURCE_PATH: &str = "jet:canonical-prelude/Prelude/Errors.jet";
const ERROR_PRELUDE_SOURCE: &str = include_str!("../../../jet-codegen/src/Prelude/Errors.jet");

/// Original canonical nominal declarations and conversions share one parse.
/// Registration retains these source rows and their declared private defaults.
pub(crate) fn error_prelude_program() -> Result<&'static Program, &'static [Diagnostic]> {
    static PARSED: std::sync::OnceLock<Result<Program, Vec<Diagnostic>>> = std::sync::OnceLock::new();
    match PARSED.get_or_init(|| {
        let (tokens, mut diagnostics) = crate::Lexer::lex_generated(ERROR_PRELUDE_SOURCE);
        match crate::Parser::parse_with_source(&tokens, ERROR_PRELUDE_SOURCE) {
            Ok(mut program) if diagnostics.is_empty() => {
                for item in &mut program.items {
                    if let Item::Struct(definition) = item {
                        definition.compiler_builtin_error = true;
                    }
                }
                Ok(program)
            }
            Ok(_) => Err(diagnostics),
            Err(mut parse_diagnostics) => {
                diagnostics.append(&mut parse_diagnostics);
                Err(diagnostics)
            }
        }
    }) {
        Ok(program) => Ok(program),
        Err(diagnostics) => Err(diagnostics.as_slice()),
    }
}

/// Retain the original declarations in their own ordinary source context.
/// The original parser mints the provenance; paths and aliases never select it.
pub(crate) fn retain_error_prelude_module(bundle: &mut ProgramBundle) -> Result<usize, Vec<Diagnostic>> {
    let program = error_prelude_program().map_err(<[Diagnostic]>::to_vec)?;
    if !program.items.iter().any(|item| matches!(item,
        Item::Struct(definition) if definition.compiler_builtin_error
            && definition.name == crate::Syntax::TYPE_ERR))
    {
        return Err(vec![error_supplier_diagnostic("the original Prelude has no default-error declaration")]);
    }
    let mut retained = bundle.modules.iter().enumerate().filter_map(|(owner, module)|
        module.items.iter().any(|item| matches!(item,
            Item::Struct(definition) if definition.compiler_builtin_error)).then_some(owner));
    if let Some(owner) = retained.next() {
        if retained.next().is_some() {
            return Err(vec![error_supplier_diagnostic("original Prelude declarations have multiple owners")]);
        }
        let module = &bundle.modules[owner];
        if module.path != std::path::Path::new(ERROR_PRELUDE_SOURCE_PATH)
            || module.display != ERROR_PRELUDE_MODULE_PATH
            || module.source != ERROR_PRELUDE_SOURCE
            || module.alias != crate::Syntax::generated_path(ERROR_PRELUDE_SOURCE_PATH)
        {
            return Err(vec![error_supplier_diagnostic("original Prelude provenance lost its source context")]);
        }
        return Ok(owner);
    }
    let alias = crate::Syntax::generated_path(ERROR_PRELUDE_SOURCE_PATH);
    if bundle.modules.iter().any(|module|
        module.path == std::path::Path::new(ERROR_PRELUDE_SOURCE_PATH) || module.alias == alias)
    {
        return Err(vec![error_supplier_diagnostic("an existing source collides with the original Prelude identity")]);
    }
    let owner = bundle.modules.len();
    bundle.modules.push(LoadedModule {
        path: std::path::PathBuf::from(ERROR_PRELUDE_SOURCE_PATH),
        display: ERROR_PRELUDE_MODULE_PATH.to_owned(),
        source: ERROR_PRELUDE_SOURCE.to_owned(),
        core_identifier_demand: crate::Lexer::core_identifier_demand(
            &crate::Lexer::lex_generated(ERROR_PRELUDE_SOURCE).0,
        ),
        alias,
        imports: program.imports.clone(),
        items: program.items.iter().filter(|item| matches!(item,
            Item::Struct(definition) if definition.compiler_builtin_error)).cloned().collect(),
        script_body: Vec::new(),
        block_spans: program.block_spans.clone(),
        web_target_ceiling: program.web_target_ceiling,
        no_prelude: program.no_prelude,
        default_target: program.default_target.clone(),
        html_path: program.html_path.clone(),
        policy_declarations: program.policy_declarations.clone(),
        user_policy_declarations: program.user_policy_declarations.clone(),
        rule_facts: program.rule_facts.clone(),
    });
    Ok(owner)
}

fn error_supplier_diagnostic(detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E0003",
        detail.to_owned(),
        "default-error declarations retain their original compiler-private source owner".to_owned(),
        "retain the canonical Prelude source and its parsed declaration provenance".to_owned(),
        None,
    )
}

/// D-FAIL-CONV2=A: open the standard library's own error family onto the default
/// `Err`. The declarations are ordinary Jet source in `Prelude/Errors.jet`.
///
/// Injection is demand-driven. A module receives only the conversions its own
/// `?` operators actually exercise (recorded as `TryConvert::Typed` during
/// checking). Unused members stay out of the module, so blast radius scales
/// with use instead of being total.
///
/// A `#NoPrelude` file receives nothing. A memory denial is an effect contract
/// and does not disable the readable Core prelude.
pub(crate) fn inject_exercised_error_conversions(module: &mut LoadedModule) -> Vec<Diagnostic> {
    if module.no_prelude {
        return Vec::new();
    }
    let original = match error_prelude_program() {
        Ok(program) => program,
        Err(diagnostics) => return diagnostics.to_vec(),
    };
    // Keyed by the exercised source name. A source-owned Core error arrives
    // under its canonical identity; its conversion is the shipped declaration
    // for its leaf, bound to that identity so sema and codegen name one
    // conversion function.
    let mut needed: HashMap<(String, String), ErrorConvDef> = HashMap::new();
    for item in &mut module.items {
        walk_item_exprs(item, &mut |expr| {
            let Expr::Try(_, _, TryConvert::Typed { source, target, .. }, _) = expr else {
                return;
            };
            let source = source.name();
            let target = target.name();
            let leaf = crate::Sema::Diagnostics::core_error_family_leaf(&source);
            for conversion in original.items.iter().filter_map(|item| match item {
                Item::ErrorConv(conversion) => Some(conversion),
                _ => None,
            }) {
                if leaf == conversion.from_ty && target == conversion.to_ty {
                    let mut bound = conversion.clone();
                    bound.from_ty = source.clone();
                    needed.insert((source.clone(), target.clone()), bound);
                }
            }
        });
    }
    let mut needed: Vec<_> = needed.into_values().collect();
    needed.sort_by(|a, b| (&a.from_ty, &a.to_ty).cmp(&(&b.from_ty, &b.to_ty)));
    for conversion in needed {
        let declared_locally = module.items.iter().any(|item| {
            matches!(
                item,
                Item::ErrorConv(local)
                    if local.from_ty == conversion.from_ty && local.to_ty == conversion.to_ty
            )
        });
        if declared_locally {
            continue;
        }
        module.items.push(Item::ErrorConv(conversion));
    }
    Vec::new()
}


fn walk_item_exprs(item: &mut Item, visit: &mut impl FnMut(&Expr)) {
    match item {
        Item::Func(function) => walk_func(function, visit),
        Item::Struct(definition) => {
            for method in &mut definition.methods {
                walk_func(method, visit);
            }
            for implementation in &mut definition.trait_impls {
                for method in &mut implementation.methods {
                    walk_func(method, visit);
                }
            }
        }
        Item::Impl(implementation) => {
            for method in &mut implementation.methods {
                walk_func(method, visit);
            }
        }
        Item::Test(test) => walk_stmts(&mut test.body, visit),
        Item::Const(constant) => walk_expr(&mut constant.value, visit),
        Item::ErrorConv(conversion) => walk_stmts(&mut conversion.body, visit),
        Item::CodeModule(module) => {
            if let Some(body) = &mut module.body {
                for inner in body {
                    walk_item_exprs(inner, visit);
                }
            }
        }
        _ => {}
    }
}

fn walk_func(function: &mut Func, visit: &mut impl FnMut(&Expr)) {
    walk_stmts(&mut function.body, visit);
}

fn walk_stmts(body: &mut [Stmt], visit: &mut impl FnMut(&Expr)) {
    for stmt in body {
        stmt.for_each_expr_mut(|expr| walk_expr(expr, visit));
    }
}

fn walk_expr(expr: &mut Expr, visit: &mut impl FnMut(&Expr)) {
    visit(expr);
}

/// Inject the prelude's Core module aliases once at file scope. The import
/// nodes are compiler-owned so codegen, JIT, and interpreter all receive the
/// same core-import map; no package source can manufacture this opening.
pub(crate) fn inject(bundle: &mut ProgramBundle) {
    for module in &mut bundle.modules {
        if module.no_prelude {
            continue;
        }
        for &core_module in &module.core_identifier_demand.prelude_modules {
            if module.imports.iter().any(|import| {
                import.import_alias().starts_with(INTERNAL_PREFIX)
                    && import.core_module_path().as_deref() == Some(core_module)
            }) {
                continue;
            }
            let stem = core_module.replace('.', "_");
            let base = format!("{INTERNAL_PREFIX}{stem}");
            let mut alias = base.clone();
            let mut suffix = 0usize;
            while module
                .imports
                .iter()
                .any(|import| import.import_alias() == alias)
            {
                suffix += 1;
                alias = format!("{base}_{suffix}");
            }
            let zero = Span::new(0, 0);
            module.imports.push(ImportDecl {
                kind: ImportKind::Module(core_module.to_string(), zero),
                alias,
                alias_span: zero,
                span: zero,
                item_spans: Vec::new(),
                local_spans: Vec::new(),
                is_pub: false,
                is_package_pub: false,
                inline_version: None,
            });
        }
    }
}

/// Find the compiler-owned alias for one Core module after injection.
pub(crate) fn core_alias_for<'a>(
    core_imports: &'a HashMap<String, String>,
    module: &str,
) -> Option<&'a str> {
    core_imports.iter().find_map(|(alias, target)| {
        (alias.starts_with(INTERNAL_PREFIX) && target == module).then_some(alias.as_str())
    })
}

pub(crate) fn shadow_warning(name: &str, span: Span) -> Diagnostic {
    Diagnostic::lint(
        "L0510",
        format!("declaration {name} replaces a Core prelude alias"),
        "the readable Core prelude opens this name automatically, and a user declaration wins in its file".to_string(),
        "keep the declaration to accept the shadow, or rename it to use the prelude alias".to_string(),
        Some(span),
    )
}

pub(crate) fn is_prelude_name(name: &str) -> bool {
    CorePrelude::entry(name).is_some()
}

