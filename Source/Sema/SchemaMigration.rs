//! D-MIGRATE1 (ratified 2026-06-22): sema diff pass for `#PublishedSchema` structs.
//!
//! After struct registration, for each `#PublishedSchema` struct:
//! 1. Load the prior snapshot from `.jet/cache/schema/<TypeName>.snapshot` (if any).
//! 2. Diff old vs new field layout.
//! 3. Collect declared `migration <Type> { rename a -> b }` ops for this type.
//! 4. A field removed from old without a type-matching `rename` → E0910.
//!
//! I3: all checking here; codegen sees nothing of migration state.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Publish::load_snapshot;
use crate::AST::{Item, MigrationOp};
use std::collections::HashMap;
use std::path::Path;

/// Run the schema migration diff pass over the items in a single module.
/// `project_root` is the root of the Jet project (the dir containing `.jet/`).
/// Returns any E0910 diagnostics.
pub fn check_schema_migrations(items: &[Item], project_root: &Path) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    // Collect declared migration ops: type_name → Vec<(from, to, to_type)>
    // We only need (from, to) here; type matching is done against the new field layout.
    let mut migrations: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for item in items {
        if let Item::Migration(m) = item {
            let entry = migrations.entry(m.type_name.clone()).or_default();
            for op in &m.ops {
                let MigrationOp::Rename { from, to, .. } = op;
                entry.push((from.clone(), to.clone()));
            }
        }
    }

    // Check each #PublishedSchema struct.
    for item in items {
        let Item::Struct(s) = item else {
            continue;
        };
        if !s.is_published_schema {
            continue;
        }

        let Some(prior) = load_snapshot(project_root, &s.name) else {
            // No prior snapshot → first release; nothing to diff.
            continue;
        };

        // Build new field map: name → type string
        let new_fields: HashMap<String, String> = s
            .fields
            .iter()
            .map(|f| (f.name.clone(), f.ty.name()))
            .collect();

        // Collect renames declared for this type.
        let renames = migrations.get(&s.name).cloned().unwrap_or_default();

        for old_field in &prior.fields {
            if new_fields.contains_key(&old_field.name) {
                // Field still present — OK.
                continue;
            }
            // Field removed — check for a rename declaration that bridges it.
            let bridged = renames.iter().any(|(from, to)| {
                from == &old_field.name
                    && new_fields
                        .get(to.as_str())
                        .map_or(false, |new_ty| new_ty == &old_field.ty)
            });
            if bridged {
                // rename covers this removal with matching type → unblocked.
                continue;
            }
            // E0910: breaking shape change with no migration.
            let span = s.published_schema_span.unwrap_or(s.name_span);
            diags.push(e0910(
                &s.name,
                &old_field.name,
                &prior.published_version,
                span,
            ));
        }
    }

    diags
}

/// E0910: a `#PublishedSchema` struct dropped a field since the last release,
/// with no migration to bridge it.
pub fn e0910(type_name: &str, field: &str, version: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0910",
        format!(
            "the published record `{}` dropped `{}` since version `{}`, with no migration to bridge it",
            type_name, field, version
        ),
        format!(
            "`#PublishedSchema` pins a record's saved shape at release; old data written with `{}` could no longer be read",
            field
        ),
        format!(
            "add `migration {} {{ rename {} -> <new_name> }}`, bump the major version, or deprecate the old field",
            type_name, field
        ),
        Some(span),
    )
}
