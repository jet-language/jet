//! One compiler-owned opening of the readable Core prelude.

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{
    ErrorConvDef, Expr, Func, ImportDecl, ImportKind, Item, LoadedModule, ProgramBundle, Stmt,
    Program, TryConvert,
};
use jet_foundation::Prelude as CorePrelude;
use jet_foundation::Prelude::Target;
use std::collections::{BTreeSet, HashMap};

const INTERNAL_PREFIX: &str = "__jet_prelude_";

/// The original canonical declarations and conversion bodies share one parse.
/// Registration retains these AST rows; it never fabricates an Err shape.
pub(crate) fn error_prelude_program() -> Result<&'static Program, &'static [Diagnostic]> {
    static PARSED: std::sync::OnceLock<Result<Program, Vec<Diagnostic>>> = std::sync::OnceLock::new();
    match PARSED.get_or_init(|| {
        const SOURCE: &str = include_str!("../../../jet-codegen/src/Prelude/Errors.jet");
        let (tokens, mut diagnostics) = crate::Lexer::lex_generated(SOURCE);
        match crate::Parser::parse_with_source(&tokens, SOURCE) {
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
    let program = match error_prelude_program() {
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
            for item in &program.items {
                let Item::ErrorConv(conversion) = item else { continue };
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
        let modules: BTreeSet<&'static str> = CorePrelude::entries()
            .iter()
            .filter_map(|entry| match entry.target {
                Target::Core {
                    module: core_module,
                    ..
                } if source_mentions_identifier(&module.source, entry.name) => Some(core_module),
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

fn source_mentions_identifier(source: &str, name: &str) -> bool {
    source.match_indices(name).any(|(start, _)| {
        let before = source[..start].chars().next_back();
        let after = source[start + name.len()..].chars().next();
        !before.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
            && !after.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_error_supplier_retains_seven_original_declarations_and_private_default() {
        let program = error_prelude_program().expect("canonical error source must parse");
        let declarations: Vec<_> = program.items.iter().filter_map(|item| {
            if let Item::Struct(definition) = item { Some(definition) } else { None }
        }).collect();
        assert_eq!(declarations.len(), 7);
        assert!(declarations.iter().all(|definition| definition.compiler_builtin_error));
        assert!(declarations.iter().all(|definition| definition.span.end > definition.span.start));
        let error = declarations.iter().find(|definition| definition.name == "Err").unwrap();
        assert_eq!(error.fields.iter().map(|field| field.name.as_str()).collect::<Vec<_>>(),
            ["message", "code", "cause", "__jet___memo_error_metadata"]);
        let metadata = &error.fields[3];
        assert!(!metadata.is_pub && !metadata.is_package_pub);
        assert!(metadata.computed.is_none());
        assert!(matches!(&metadata.ty, crate::AST::Type::Named(name) if name == "__jet_ErrorMetadata"));
        assert!(metadata.default.is_some());
        assert!(std::ptr::eq(program, error_prelude_program().unwrap()),
            "all consumers must retain the same original parsed supplier");
    }

    #[test]
    fn ordinary_generated_source_does_not_grant_error_builtin_authority() {
        let source = "struct Example { value: Int }";
        let (tokens, diagnostics) = crate::Lexer::lex_generated(source);
        assert!(diagnostics.is_empty());
        let program = crate::Parser::parse_with_source(&tokens, source).unwrap();
        let Item::Struct(definition) = &program.items[0] else { panic!("expected struct") };
        assert!(!definition.compiler_builtin_error);
    }
}
