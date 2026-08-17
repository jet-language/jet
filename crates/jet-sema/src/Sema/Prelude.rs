//! One compiler-owned opening of the readable Core prelude.

use crate::AST::{ImportDecl, ImportKind, Item, ProgramBundle};
use crate::Diagnostics::{Diagnostic, Span};
use jet_foundation::Prelude as CorePrelude;
use jet_foundation::Prelude::Target;
use std::collections::{BTreeSet, HashMap};

const INTERNAL_PREFIX: &str = "__jet_prelude_";

/// D-FAIL-CONV2=A: open the standard library's own error family onto the default
/// `Err`. The declarations are ordinary Jet source in
/// `Prelude/Errors.jet`, loaded the same way `Prelude/Units.jet` ships the
/// dimension catalog, so sema, codegen, the Cranelift hosts, and the interpreter
/// all receive one `impl <CoreError> => Err` per family member and lower it
/// through the existing D-ERR-CONV rail (I8/I9 — no second conversion
/// mechanism, no per-engine rule).
///
/// A module that declares its own conversion for the same pair keeps it: the
/// shipped one is skipped rather than duplicated, so a package can still say how
/// its program reports a Core failure and E2405 never fires on the injection.
///
/// Two files receive nothing. A `#NoPrelude` file opted out of every readable
/// Core name (D-PRELUDEX1). A `policy no_alloc` file cannot use the default `Err`
/// at all, because that report carries an owned message, so there is no target to
/// open the family onto.
pub(crate) fn inject_error_conversions(bundle: &mut ProgramBundle) -> Vec<Diagnostic> {
    const SOURCE: &str = include_str!("../../../jet-codegen/src/Prelude/Errors.jet");
    let (tokens, mut diagnostics) = crate::Lexer::lex_generated(SOURCE);
    let shipped = match crate::Parser::parse(&tokens) {
        Ok(program) => program
            .items
            .into_iter()
            .filter_map(|item| match item {
                Item::ErrorConv(conversion) => Some(conversion),
                _ => None,
            })
            .collect::<Vec<_>>(),
        Err(mut parse_diagnostics) => {
            diagnostics.append(&mut parse_diagnostics);
            return diagnostics;
        }
    };
    for module in &mut bundle.modules {
        if module.no_prelude || module.no_alloc_policy.is_some() {
            continue;
        }
        for conversion in &shipped {
            let declared_locally = module.items.iter().any(|item| {
                matches!(
                    item,
                    Item::ErrorConv(local)
                        if local.from_ty == conversion.from_ty
                            && local.to_ty == conversion.to_ty
                )
            });
            if declared_locally {
                continue;
            }
            module.items.push(Item::ErrorConv(conversion.clone()));
        }
    }
    diagnostics
}

/// Inject the prelude's Core module aliases once at file scope. The import
/// nodes are compiler-owned so codegen, JIT, and interpreter all receive the
/// same core-import map; no package source can manufacture this opening.
pub(crate) fn inject(bundle: &mut ProgramBundle) {
    for module in &mut bundle.modules {
        if module.no_prelude {
            continue;
        }
        // Keep the opening readable without turning every file's runtime layer
        // into hosted just because the prelude has hosted aliases. Import only
        // the Core module that a source name can actually select; unused
        // aliases remain semantically available but do not affect layer or
        // Core reachability.
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
