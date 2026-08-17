//! One compiler-owned opening of the readable Core prelude.

use crate::AST::{ImportDecl, ImportKind, ProgramBundle};
use crate::Diagnostics::{Diagnostic, Span};
use jet_foundation::Prelude as CorePrelude;
use jet_foundation::Prelude::Target;
use std::collections::{BTreeSet, HashMap};

const INTERNAL_PREFIX: &str = "__jet_prelude_";

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
