//! One compiler-owned opening of the readable Core prelude.

use crate::AST::{
    ErrorConvDef, Expr, Func, ImportDecl, ImportKind, Item, LoadedModule, ProgramBundle, Stmt,
    TryConvert,
};
use crate::Diagnostics::{Diagnostic, Span};
use jet_foundation::Prelude as CorePrelude;
use jet_foundation::Prelude::Target;
use std::collections::{BTreeSet, HashMap, HashSet};

const INTERNAL_PREFIX: &str = "__jet_prelude_";

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
    let (shipped, diagnostics) = shipped_error_conversions();
    if shipped.is_empty() {
        return diagnostics;
    }
    let mut needed: HashSet<(String, String)> = HashSet::new();
    for item in &mut module.items {
        walk_item_exprs(item, &mut |expr| {
            let Expr::Try(_, _, TryConvert::Typed(fn_name), _) = expr else {
                return;
            };
            for conversion in &shipped {
                if fn_name == &super::error_conv_fn_name(&conversion.from_ty, &conversion.to_ty) {
                    needed.insert((conversion.from_ty.clone(), conversion.to_ty.clone()));
                }
            }
        });
    }
    for conversion in shipped {
        let key = (conversion.from_ty.clone(), conversion.to_ty.clone());
        if !needed.contains(&key) {
            continue;
        }
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
    diagnostics
}

fn shipped_error_conversions() -> (Vec<ErrorConvDef>, Vec<Diagnostic>) {
    const SOURCE: &str = include_str!("../../../jet-codegen/src/Prelude/Errors.jet");
    let (tokens, mut diagnostics) = crate::Lexer::lex_generated(SOURCE);
    match crate::Parser::parse(&tokens) {
        Ok(program) => {
            let conversions = program
                .items
                .into_iter()
                .filter_map(|item| match item {
                    Item::ErrorConv(conversion) => Some(conversion),
                    _ => None,
                })
                .collect();
            (conversions, diagnostics)
        }
        Err(mut parse_diagnostics) => {
            diagnostics.append(&mut parse_diagnostics);
            (Vec::new(), diagnostics)
        }
    }
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
        Item::Bench(bench) => walk_stmts(&mut bench.body, visit),
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
        let modules: BTreeSet<&'static str> = CorePrelude::entries()
            .iter()
            .filter_map(|entry| match entry.target {
                Target::Core {
                    module: core_module,
                    ..
                } if source_mentions_identifier(&module.source, entry.name) => {
                    Some(core_module)
                }
                _ => None,
            })
            .collect();
        for core_module in modules {
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

fn source_mentions_identifier(source: &str, name: &str) -> bool {
    source.match_indices(name).any(|(start, _)| {
        let before = source[..start].chars().next_back();
        let after = source[start + name.len()..].chars().next();
        !before.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
            && !after.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
    })
}
