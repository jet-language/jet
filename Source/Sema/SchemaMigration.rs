//! D-MIGRATE1 / D-MIGRATE2 (ratified 2026-06-22): sema diff pass for
//! `#PublishedSchema` structs.
//!
//! After struct registration, for each `#PublishedSchema` struct:
//! 1. Load the prior snapshot from `.jet/cache/schema/<TypeName>.snapshot` (if any).
//! 2. Diff old vs new field layout (keyed by name, so field *order* is ignored —
//!    D-MIGRATE2F: reordering is never a breaking change).
//! 3. Collect declared `migration <Type> { … }` ops for this type.
//! 4. Refuse a breaking data-shape change unless a migration declares the intent:
//!    - a field removed from old, not bridged by `rename old -> new` or `remove old`;
//!    - a field whose type changed, not bridged by a `change f: Old -> New [via …]`;
//!    - a field added in the new shape that old data can't supply (no `add` op).
//!    Every such case is the single umbrella code E0910 with case-specific text.
//! 5. Validate the declared ops against reality too (e.g. `remove f` where `f`
//!    still exists, `add f` where `f` isn't actually new), so a nonsensical
//!    migration teaches rather than silently passing.
//!
//! NOTE (Build-tier versioning library, follow-on #11): this pass checks *intent*
//! only. It never codegens the actual `from_vXXX` runtime data conversion — the
//! `via { … }` converter and `add` default are recorded but not lowered here.
//! Runtime old-data reading is a named downstream deliverable, not part of c73.
//!
//! I3: all checking here; codegen sees nothing of migration state.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Publish::load_snapshot;
use crate::AST::{Item, MigrationOp};
use std::collections::HashMap;
use std::path::Path;

/// A declared migration op for one type, flattened for diffing.
enum DeclaredOp {
    Rename { from: String, to: String },
    Add { field: String, ty: String },
    Remove { field: String },
    Change { field: String, from_ty: String, to_ty: String, has_converter: bool },
}

/// Run the schema migration diff pass over the items in a single module.
/// `project_root` is the root of the Jet project (the dir containing `.jet/`).
/// Returns any E0910 diagnostics.
pub fn check_schema_migrations(items: &[Item], project_root: &Path) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    // Collect declared migration ops keyed by type name.
    let mut migrations: HashMap<String, Vec<DeclaredOp>> = HashMap::new();
    for item in items {
        if let Item::Migration(m) = item {
            let entry = migrations.entry(m.type_name.clone()).or_default();
            for op in &m.ops {
                match op {
                    MigrationOp::Rename { from, to, .. } => {
                        entry.push(DeclaredOp::Rename { from: from.clone(), to: to.clone() });
                    }
                    MigrationOp::Add { field, ty, .. } => {
                        entry.push(DeclaredOp::Add { field: field.clone(), ty: ty.name() });
                    }
                    MigrationOp::Remove { field, .. } => {
                        entry.push(DeclaredOp::Remove { field: field.clone() });
                    }
                    MigrationOp::Change { field, from_ty, to_ty, converter, .. } => {
                        entry.push(DeclaredOp::Change {
                            field: field.clone(),
                            from_ty: from_ty.name(),
                            to_ty: to_ty.name(),
                            has_converter: converter.is_some(),
                        });
                    }
                }
            }
        }
    }

    // D-MIGRATE2B: an `impl Old -> New` in scope is a fallback converter source.
    // Collect declared error-conversion-style impls as (from, to) type-name pairs.
    let mut conv_impls: Vec<(String, String)> = Vec::new();
    for item in items {
        if let Item::ErrorConv(ec) = item {
            conv_impls.push((ec.from_ty.clone(), ec.to_ty.clone()));
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

        let span = s.published_schema_span.unwrap_or(s.name_span);

        // New field map: name → type string.
        let new_fields: HashMap<String, String> = s
            .fields
            .iter()
            .map(|f| (f.name.clone(), f.ty.name()))
            .collect();
        // Old field map: name → type string.
        let old_fields: HashMap<String, String> = prior
            .fields
            .iter()
            .map(|f| (f.name.clone(), f.ty.clone()))
            .collect();

        let ops = migrations.get(&s.name).map(|v| v.as_slice()).unwrap_or(&[]);

        // ---- validate declared ops against reality -----------------------
        for op in ops {
            match op {
                DeclaredOp::Remove { field } => {
                    if new_fields.contains_key(field) {
                        diags.push(e0910_remove_still_present(&s.name, field, span));
                    } else if !old_fields.contains_key(field) {
                        diags.push(e0910_op_unknown_field(&s.name, field, "remove", span));
                    }
                }
                DeclaredOp::Add { field, ty } => {
                    if old_fields.contains_key(field) {
                        diags.push(e0910_add_not_new(&s.name, field, span));
                    } else if let Some(new_ty) = new_fields.get(field) {
                        // The declared `add f: T` type must match the struct field's type.
                        if new_ty != ty {
                            diags.push(e0910_add_type_mismatch(&s.name, field, ty, new_ty, span));
                        }
                    } else {
                        diags.push(e0910_op_unknown_field(&s.name, field, "add", span));
                    }
                }
                DeclaredOp::Change { field, from_ty, to_ty, .. } => {
                    // The field must exist in both shapes, and the declared
                    // from/to must match the snapshot/current types.
                    match (old_fields.get(field), new_fields.get(field)) {
                        (Some(old_ty), Some(new_ty)) => {
                            if old_ty != from_ty || new_ty != to_ty {
                                diags.push(e0910_change_type_mismatch(
                                    &s.name, field, from_ty, to_ty, old_ty, new_ty, span,
                                ));
                            }
                        }
                        _ => diags.push(e0910_op_unknown_field(&s.name, field, "change", span)),
                    }
                }
                DeclaredOp::Rename { .. } => {}
            }
        }

        // ---- removed fields ---------------------------------------------
        for old_field in &prior.fields {
            if new_fields.contains_key(&old_field.name) {
                continue; // still present (handled by type-change check below)
            }
            // Bridged by a type-matching `rename old -> new`?
            let renamed = ops.iter().any(|op| match op {
                DeclaredOp::Rename { from, to } => {
                    from == &old_field.name
                        && new_fields.get(to.as_str()).map_or(false, |t| t == &old_field.ty)
                }
                _ => false,
            });
            // Or by an explicit `remove old`?
            let removed = ops.iter().any(|op| matches!(op, DeclaredOp::Remove { field } if field == &old_field.name));
            if renamed || removed {
                continue;
            }
            diags.push(e0910_dropped(&s.name, &old_field.name, &prior.published_version, span));
        }

        // ---- type-changed fields ----------------------------------------
        for f in &s.fields {
            let Some(old_ty) = old_fields.get(&f.name) else {
                continue; // not in old shape — handled by added-field check
            };
            let new_ty = f.ty.name();
            if old_ty == &new_ty {
                continue; // unchanged
            }
            // A `change f: old -> new` op declares intent. The converter is the
            // inline `via { … }` (D-MIGRATE2B step 1) OR an `impl Old -> New`
            // in scope (step 2). Without either → ask for a converter (step 3).
            let change_op = ops.iter().find_map(|op| match op {
                DeclaredOp::Change { field, from_ty, to_ty, has_converter }
                    if field == &f.name && from_ty == old_ty && to_ty == &new_ty =>
                {
                    Some(*has_converter)
                }
                _ => None,
            });
            match change_op {
                Some(true) => {} // inline converter present → OK
                Some(false) => {
                    // No inline `via`; look for an `impl Old -> New` fallback.
                    let has_impl = conv_impls.iter().any(|(from, to)| from == old_ty && to == &new_ty);
                    if !has_impl {
                        diags.push(e0910_change_no_converter(&s.name, &f.name, old_ty, &new_ty, span));
                    }
                }
                None => {
                    diags.push(e0910_changed_type(&s.name, &f.name, old_ty, &new_ty, span));
                }
            }
        }

        // ---- added fields -----------------------------------------------
        // A field in the new shape absent from old data needs a default to read
        // old records. Struct-field defaults don't exist in this AST yet, so the
        // bridge is an `add f: T = default` migration op (D-MIGRATE2A).
        for f in &s.fields {
            if old_fields.contains_key(&f.name) {
                continue; // existed before
            }
            // Bridged by `rename old -> f` (the new name of a renamed field)?
            let from_rename = ops.iter().any(|op| matches!(op, DeclaredOp::Rename { to, .. } if to == &f.name));
            // Or declared `add f: T = default`?
            let added = ops.iter().any(|op| matches!(op, DeclaredOp::Add { field, .. } if field == &f.name));
            if from_rename || added {
                continue;
            }
            diags.push(e0910_added_required(&s.name, &f.name, span));
        }
    }

    diags
}

/// E0910: a published field was dropped with no migration to bridge it.
fn e0910_dropped(type_name: &str, field: &str, version: &str, span: Span) -> Diagnostic {
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
            "add `migration {} {{ remove {} }}` to delete it, `migration {} {{ rename {} -> <new> }}` if you renamed it, or bump the major version",
            type_name, field, type_name, field
        ),
        Some(span),
    )
}

/// E0910: a published field changed type, with no `change` op declaring intent.
fn e0910_changed_type(type_name: &str, field: &str, old_ty: &str, new_ty: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0910",
        format!(
            "the published record `{}` changed `{}` from `{}` to `{}`, with no migration to bridge it",
            type_name, field, old_ty, new_ty
        ),
        "`#PublishedSchema` pins a record's saved shape at release; old data stored at the previous type could no longer be read".to_string(),
        format!(
            "add `migration {} {{ change {}: {} -> {} via {{ (old) => … }} }}`, or bump the major version",
            type_name, field, old_ty, new_ty
        ),
        Some(span),
    )
}

/// E0910: a `change` op is declared but no converter (neither inline `via` nor
/// an `impl Old -> New` in scope) is available.
fn e0910_change_no_converter(type_name: &str, field: &str, old_ty: &str, new_ty: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0910",
        format!(
            "the `change {}: {} -> {}` migration on `{}` has no converter",
            field, old_ty, new_ty, type_name
        ),
        "a type change needs a way to turn an old value into a new one — old data already on disk is read through it".to_string(),
        format!(
            "add an inline `via {{ (old) => … }}` to the `change` op, or declare `impl {} -> {} {{ … }}` in scope",
            old_ty, new_ty
        ),
        Some(span),
    )
}

/// E0910: a new field was added with nothing for old data to supply.
fn e0910_added_required(type_name: &str, field: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0910",
        format!(
            "the published record `{}` added `{}`, but old data has no value for it",
            type_name, field
        ),
        "`#PublishedSchema` records already written without this field can't be read unless there's a default to fill it in".to_string(),
        format!(
            "add `migration {} {{ add {}: <Type> = <default> }}` to supply the value, or bump the major version",
            type_name, field
        ),
        Some(span),
    )
}

/// E0910: `remove f` declared, but `f` still exists in the current struct.
fn e0910_remove_still_present(type_name: &str, field: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0910",
        format!(
            "the migration removes `{}`, but `{}` still has that field",
            field, type_name
        ),
        "a `remove` op declares a field is gone, but the current `#PublishedSchema` struct still defines it".to_string(),
        format!("delete the field from `struct {}`, or drop the `remove {}` line", type_name, field),
        Some(span),
    )
}

/// E0910: `add f` declared, but `f` already existed in the prior snapshot.
fn e0910_add_not_new(type_name: &str, field: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0910",
        format!(
            "the migration adds `{}`, but `{}` already had that field",
            field, type_name
        ),
        "an `add` op declares a brand-new field, but the prior published snapshot already records this one".to_string(),
        format!("drop the `add {}` line; if its type changed, use `change` instead", field),
        Some(span),
    )
}

/// E0910: a `change` op's declared from/to types don't match the real shapes.
fn e0910_change_type_mismatch(
    type_name: &str,
    field: &str,
    decl_from: &str,
    decl_to: &str,
    real_from: &str,
    real_to: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic::error(
        "E0910",
        format!(
            "the `change {}: {} -> {}` migration on `{}` doesn't match the real shapes (`{}` -> `{}`)",
            field, decl_from, decl_to, type_name, real_from, real_to
        ),
        "the `from` type must be the field's published type and the `to` type must be its current type".to_string(),
        format!("write `change {}: {} -> {} …`", field, real_from, real_to),
        Some(span),
    )
}

/// E0910: an `add f: T` op's declared type doesn't match the struct field's type.
fn e0910_add_type_mismatch(type_name: &str, field: &str, decl_ty: &str, real_ty: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0910",
        format!(
            "the `add {}: {}` migration on `{}` declares a type the field doesn't have (`{}`)",
            field, decl_ty, type_name, real_ty
        ),
        "the type in an `add` op must match the field's type in the struct".to_string(),
        format!("write `add {}: {} = <default>`", field, real_ty),
        Some(span),
    )
}

/// E0910: an `add`/`remove`/`change` op names a field that's in neither shape.
fn e0910_op_unknown_field(type_name: &str, field: &str, verb: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0910",
        format!(
            "the `{} {}` migration on `{}` names a field that doesn't exist",
            verb, field, type_name
        ),
        "a migration op must reference a field of this record — either its published shape or its current one".to_string(),
        format!("check the spelling of `{}`, or remove the `{}` op", field, verb),
        Some(span),
    )
}

// ──────────────────────────────────────────────
// Unit tests — exercise the diff against a temp snapshot via JET_SCHEMA_CACHE_DIR.
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AST::{Field, MigrationDecl, StructDef, Type};
    use std::sync::Mutex;

    // The diff reads a process-global env var; serialize tests that set it.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn zero() -> Span { Span::new(0, 0) }

    fn field(name: &str, ty: Type) -> Field {
        Field {
            is_pub: false,
            is_stored_ref: false,
            stored_ref_label: None,
            name: name.to_string(),
            name_span: zero(),
            ty,
            ty_span: zero(),
            serde_markers: Vec::new(),
        }
    }

    fn published_struct(name: &str, fields: Vec<Field>) -> Item {
        Item::Struct(StructDef {
            is_pub: false,
            name: name.to_string(),
            name_span: zero(),
            type_params: vec![],
            fields,
            methods: vec![],
            trait_impls: vec![],
            derives: vec![],
            is_published_schema: true,
            published_schema_span: Some(zero()),
            is_single_use: false,
            single_use_span: None,
            layout: None,
            layout_span: None,
            serde_markers: Vec::new(),
            type_markers: Vec::new(),
        })
    }

    fn migration(type_name: &str, ops: Vec<MigrationOp>) -> Item {
        Item::Migration(MigrationDecl {
            type_name: type_name.to_string(),
            type_span: zero(),
            ops,
            span: zero(),
        })
    }

    /// Run the diff with a snapshot written into a temp dir pointed at by the
    /// env override. Returns the diagnostic codes produced.
    fn run_with_snapshot(snapshot: &str, items: &[Item]) -> Vec<&'static str> {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "jet_schema_unit_{}_{}",
            std::process::id(),
            // a cheap unique-ish suffix
            snapshot.len()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // The snapshot's type name keys the file.
        let type_name = snapshot
            .lines()
            .find_map(|l| l.strip_prefix("type = "))
            .unwrap()
            .trim();
        std::fs::write(dir.join(format!("{}.snapshot", type_name)), snapshot).unwrap();
        std::env::set_var("JET_SCHEMA_CACHE_DIR", &dir);
        let diags = check_schema_migrations(items, std::path::Path::new("."));
        std::env::remove_var("JET_SCHEMA_CACHE_DIR");
        std::fs::remove_dir_all(&dir).ok();
        diags.iter().map(|d| d.code).collect()
    }

    const SNAP_ONE: &str = "schema_version = 1\ntype = Rec\npublished_version = 1.0.0\nfield name: String\n";

    #[test]
    fn unchanged_struct_is_clean() {
        let items = [published_struct("Rec", vec![field("name", Type::String)])];
        assert!(run_with_snapshot(SNAP_ONE, &items).is_empty());
    }

    #[test]
    fn reorder_is_not_a_break() {
        // Snapshot is name then age; struct is age then name. Order ignored.
        let snap = "schema_version = 1\ntype = Rec\npublished_version = 1.0.0\nfield name: String\nfield age: Int\n";
        let items = [published_struct(
            "Rec",
            vec![field("age", Type::Int), field("name", Type::String)],
        )];
        assert!(run_with_snapshot(snap, &items).is_empty());
    }

    #[test]
    fn dropped_field_without_migration_is_e0910() {
        // Struct drops `name`.
        let items = [published_struct("Rec", vec![field("kept", Type::Int)])];
        let snap = "schema_version = 1\ntype = Rec\npublished_version = 1.0.0\nfield name: String\nfield kept: Int\n";
        assert_eq!(run_with_snapshot(snap, &items), vec!["E0910"]);
    }

    #[test]
    fn remove_op_bridges_a_drop() {
        let items = [
            published_struct("Rec", vec![]),
            migration("Rec", vec![MigrationOp::Remove { field: "name".into(), field_span: zero() }]),
        ];
        assert!(run_with_snapshot(SNAP_ONE, &items).is_empty());
    }

    #[test]
    fn added_field_needs_add_op() {
        // Struct adds `extra` not in the snapshot.
        let items = [published_struct(
            "Rec",
            vec![field("name", Type::String), field("extra", Type::Bool)],
        )];
        assert_eq!(run_with_snapshot(SNAP_ONE, &items), vec!["E0910"]);
    }

    #[test]
    fn add_op_bridges_a_new_field() {
        let items = [
            published_struct("Rec", vec![field("name", Type::String), field("extra", Type::Bool)]),
            migration(
                "Rec",
                vec![MigrationOp::Add {
                    field: "extra".into(),
                    field_span: zero(),
                    ty: Type::Bool,
                    ty_span: zero(),
                    default: crate::AST::Expr::Bool(false, zero()),
                    default_span: zero(),
                }],
            ),
        ];
        assert!(run_with_snapshot(SNAP_ONE, &items).is_empty());
    }

    #[test]
    fn type_change_without_op_is_e0910() {
        // `name` goes String -> Int with no change op.
        let items = [published_struct("Rec", vec![field("name", Type::Int)])];
        assert_eq!(run_with_snapshot(SNAP_ONE, &items), vec!["E0910"]);
    }

    #[test]
    fn change_op_with_inline_via_bridges() {
        let items = [
            published_struct("Rec", vec![field("name", Type::Int)]),
            migration(
                "Rec",
                vec![MigrationOp::Change {
                    field: "name".into(),
                    field_span: zero(),
                    from_ty: Type::String,
                    from_span: zero(),
                    to_ty: Type::Int,
                    to_span: zero(),
                    converter: Some(crate::AST::Expr::Int(0, zero(), None)),
                    converter_span: Some(zero()),
                }],
            ),
        ];
        assert!(run_with_snapshot(SNAP_ONE, &items).is_empty());
    }

    #[test]
    fn change_op_without_converter_is_e0910() {
        // change op present, no inline via, no impl in scope.
        let items = [
            published_struct("Rec", vec![field("name", Type::Int)]),
            migration(
                "Rec",
                vec![MigrationOp::Change {
                    field: "name".into(),
                    field_span: zero(),
                    from_ty: Type::String,
                    from_span: zero(),
                    to_ty: Type::Int,
                    to_span: zero(),
                    converter: None,
                    converter_span: None,
                }],
            ),
        ];
        assert_eq!(run_with_snapshot(SNAP_ONE, &items), vec!["E0910"]);
    }

    #[test]
    fn remove_op_on_present_field_is_e0910() {
        // remove `name` but the struct still has it.
        let items = [
            published_struct("Rec", vec![field("name", Type::String)]),
            migration("Rec", vec![MigrationOp::Remove { field: "name".into(), field_span: zero() }]),
        ];
        assert_eq!(run_with_snapshot(SNAP_ONE, &items), vec!["E0910"]);
    }

    #[test]
    fn no_prior_snapshot_is_clean() {
        // First release: no snapshot file → nothing to diff.
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("jet_schema_unit_empty_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("JET_SCHEMA_CACHE_DIR", &dir);
        let items = [published_struct("Brand", vec![field("x", Type::Int)])];
        let diags = check_schema_migrations(&items, std::path::Path::new("."));
        std::env::remove_var("JET_SCHEMA_CACHE_DIR");
        std::fs::remove_dir_all(&dir).ok();
        assert!(diags.is_empty());
    }
}
