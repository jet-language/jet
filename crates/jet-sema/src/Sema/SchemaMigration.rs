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
//! D-MIGRATE4 (ratified 2026-07-03): the runtime half lives in two places —
//! `desugar_migrations` below rewrites `via { … }` converters and `add`
//! defaults into synthetic top-level functions (type-checked and lowered
//! through the normal pipeline), and codegen
//! (`Codegen/Items.rs::emit_struct_migration`) emits per-block step functions
//! plus a `jet_decode_traced` chain-walker for each decodable
//! `#PublishedSchema` type with migration blocks.
//!
//! I3: all checking here; codegen only performs the mechanical lowering.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Schema::load_snapshot;
use crate::Traits::TraitRegistry;
use crate::AST::{
    AccessConvention, Expr, Func, Item, LambdaBody, MigrationOp, Param, ProgramBundle, Stmt, Type,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// D-MIGRATE4: the mangled name of the synthetic converter function a `change …
/// via { … }` op lowers to. Deterministic from the type, the migration block's
/// index (source order, per type), and the field — so codegen and this desugar
/// agree without a shared side-table.
fn converter_fn_name(type_name: &str, block_idx: usize, field: &str) -> String {
    format!("__migrate_conv_{}_{}_{}", type_name, block_idx, field)
}

/// D-MIGRATE4: the mangled name of the synthetic zero-arg default function an
/// `add f: T = val` op lowers to. Same determinism contract as
/// `converter_fn_name`.
fn add_default_fn_name(type_name: &str, block_idx: usize, field: &str) -> String {
    format!("__migrate_add_{}_{}_{}", type_name, block_idx, field)
}

/// D-MIGRATE4: rewrite each runtime-relevant migration op on a decodable
/// `#PublishedSchema` type into synthetic top-level `fn`s so the runtime step
/// functions (codegen, `Codegen/Items.rs::emit_struct_migration`) can call
/// them, and so the op expressions are type-checked and lowered through the
/// normal pipeline:
///   - `change … via { (old) => body }` → `fn __migrate_conv_<T>_<i>_<f>(old: Old) => New`
///   - `add f: T = val`                 → `fn __migrate_add_<T>_<i>_<f>() => T`
/// The op's `conv_fn`/`default_fn` is set to the synthetic name. Types that
/// never decode at runtime (no `Decode` derive, or generic) get nothing — the
/// migration stays a compile-time intent check only, and codegen emits nothing
/// (zero cost).
pub fn desugar_migrations(bundle: &mut ProgramBundle) {
    for module in &mut bundle.modules {
        // Types that have a runtime decode path: `#PublishedSchema` + `Decode`,
        // concrete (a generic published schema has no single runtime shape).
        let mut decodable_published: HashSet<String> = HashSet::new();
        for item in &module.items {
            if let Item::Struct(s) = item {
                // `#PublishedSchema struct` sets the flag; the grouped
                // `#[PublishedSchema, Codable]` spelling leaves the marker in
                // `derives` — accept both.
                let published = s.is_published_schema
                    || s.derives
                        .iter()
                        .any(|(t, _)| t == crate::Syntax::ATTR_PUBLISHED_SCHEMA);
                if published
                    && s.type_params.is_empty()
                    && s.derives.iter().any(|(t, _)| t == crate::Generics::DECODE)
                {
                    decodable_published.insert(s.name.clone());
                }
            }
        }
        if decodable_published.is_empty() {
            continue;
        }

        // Per-type migration-block counter (source order defines the chain).
        let mut block_of_type: HashMap<String, usize> = HashMap::new();
        let mut synthetic: Vec<Item> = Vec::new();
        for item in &mut module.items {
            let Item::Migration(m) = item else { continue };
            if !decodable_published.contains(&m.type_name) {
                continue;
            }
            let block_idx = {
                let c = block_of_type.entry(m.type_name.clone()).or_insert(0);
                let v = *c;
                *c += 1;
                v
            };
            for op in &mut m.ops {
                match op {
                    MigrationOp::Change {
                        field,
                        from_ty,
                        to_ty,
                        converter: Some(conv),
                        converter_span,
                        conv_fn,
                        ..
                    } => {
                        let name = converter_fn_name(&m.type_name, block_idx, field);
                        let span = converter_span.unwrap_or(m.span);
                        let f = build_converter_func(&name, from_ty, to_ty, conv, span);
                        synthetic.push(Item::Func(f));
                        *conv_fn = Some(name);
                    }
                    MigrationOp::Add {
                        field,
                        ty,
                        default,
                        default_span,
                        default_fn,
                        ..
                    } => {
                        let name = add_default_fn_name(&m.type_name, block_idx, field);
                        let f = build_default_func(&name, ty, default, *default_span);
                        synthetic.push(Item::Func(f));
                        *default_fn = Some(name);
                    }
                    _ => {}
                }
            }
        }
        module.items.extend(synthetic);
    }
}

/// Build `fn <name>() => T { return <default expr> }` for an `add` op.
fn build_default_func(name: &str, ty: &Type, default: &Expr, span: Span) -> Func {
    Func {
        span,
        is_pub: false,
        is_package_pub: false,
        external_type: None,
        name: name.to_string(),
        name_span: span,
        meta: None,
                    type_params: Vec::new(),
        params: Vec::new(),
        return_type: Some(ty.clone()),
        return_type_span: Some(span),
        return_view_provenance: None,
        declared_return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
        is_unsafe: false,
        unsafe_reason: None,
        unsafe_span: None,
        is_pure: false,
        is_reactive: false,
                reactive_upgrades: Vec::new(),
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
        scrub_tag: None,
        declared_effects: None,
        effect_via: None,
        state_requires: None,
        state_transition: None,
        web_marker: None,
        pre: Vec::new(),
        post: Vec::new(),
        inline_foreign: None,
        body: vec![Stmt::Return(Some(default.clone()), span)],
    }
}

/// Build `fn <name>(<param>: Old) => New { <converter body> }` from a `change`
/// op's inline converter. The canonical converter is a one-parameter lambda
/// `(old) => expr`; anything else is treated as a callable applied to the old
/// value.
fn build_converter_func(name: &str, old_ty: &Type, new_ty: &Type, conv: &Expr, span: Span) -> Func {
    let (param_name, body): (String, Vec<Stmt>) = match conv {
        Expr::Lambda(l) if l.params.len() == 1 => {
            let pname = l.params[0].name.clone();
            let body = match &l.body {
                LambdaBody::Expr(e) => vec![Stmt::Return(Some((**e).clone()), span)],
                LambdaBody::Block(stmts) => stmts.clone(),
            };
            (pname, body)
        }
        other => {
            // Non-lambda converter (a function value written inline): apply it
            // to the old value.
            let arg = crate::AST::CallArg {
                convention: AccessConvention::Move,
                expr: Expr::Ident("__old".to_string(), span),
                span,
                flags: crate::AST::CallArgFlags::default(),
                label: None,
                spread: false,
            };
            let call = Expr::CallValue {
                callee: Box::new(other.clone()),
                args: vec![arg],
                span,
            };
            ("__old".to_string(), vec![Stmt::Return(Some(call), span)])
        }
    };
    Func {
        span,
        is_pub: false,
        is_package_pub: false,
        external_type: None,
        name: name.to_string(),
        name_span: span,
        meta: None,
                    type_params: Vec::new(),
        params: vec![Param {
            name: param_name,
            name_span: span,
            ty: old_ty.clone(),
            ty_span: span,
            convention: AccessConvention::Move,
            default: None,
            variadic: false,
            variadic_bound_list: None,
        }],
        return_type: Some(new_ty.clone()),
        return_type_span: Some(span),
        return_view_provenance: None,
        declared_return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
        is_unsafe: false,
        unsafe_reason: None,
        unsafe_span: None,
        is_pure: false,
        is_reactive: false,
                reactive_upgrades: Vec::new(),
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
        scrub_tag: None,
        declared_effects: None,
        effect_via: None,
        state_requires: None,
        state_transition: None,
        web_marker: None,
        pre: Vec::new(),
        post: Vec::new(),
        inline_foreign: None,
        body,
    }
}

/// A declared migration op for one type, flattened for diffing.
enum DeclaredOp {
    Rename {
        from: String,
        to: String,
    },
    Add {
        field: String,
        ty: String,
    },
    Remove {
        field: String,
    },
    Change {
        field: String,
        from_ty: String,
        to_ty: String,
        has_converter: bool,
    },
}

/// True when `s` derives `Decode` but NOT `Encode` — a struct that only ever
/// deserializes (reads published data back), never serializes. Guards the
/// `add` op check: an added field on a Decode-only record must itself satisfy
/// Decode (see the call site in `check_schema_migrations`).
fn is_decode_only(s: &crate::AST::StructDef) -> bool {
    s.derives.iter().any(|(t, _)| t == crate::Generics::DECODE)
        && !s.derives.iter().any(|(t, _)| t == crate::Generics::ENCODE)
}

/// Run the schema migration diff pass over the items in a single module.
/// `project_root` is the root of the Jet project (the dir containing `.jet/`).
/// Returns any E0910 diagnostics.
pub fn check_schema_migrations(
    items: &[Item],
    project_root: &Path,
    reg: &TraitRegistry,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    // Collect declared migration ops keyed by type name.
    let mut migrations: HashMap<String, Vec<DeclaredOp>> = HashMap::new();
    for item in items {
        if let Item::Migration(m) = item {
            let entry = migrations.entry(m.type_name.clone()).or_default();
            for op in &m.ops {
                match op {
                    MigrationOp::Rename { from, to, .. } => {
                        entry.push(DeclaredOp::Rename {
                            from: from.clone(),
                            to: to.clone(),
                        });
                    }
                    MigrationOp::Add { field, ty, .. } => {
                        entry.push(DeclaredOp::Add {
                            field: field.clone(),
                            ty: ty.name(),
                        });
                    }
                    MigrationOp::Remove { field, .. } => {
                        entry.push(DeclaredOp::Remove {
                            field: field.clone(),
                        });
                    }
                    MigrationOp::Change {
                        field,
                        from_ty,
                        to_ty,
                        converter,
                        ..
                    } => {
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

    // D-MIGRATE2B: an `impl Old => New` in scope is a fallback converter source.
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
                        } else if is_decode_only(s) {
                            // Currently unreachable in any shape codegen/parsing can
                            // produce today (a Decode-only `#PublishedSchema` struct
                            // with an `add` op is not yet a shipped combination), but
                            // guard it explicitly rather than let a future shape emit
                            // rejected Rust (I2): a Decode-only record only ever
                            // DEserializes, so the field it adds must itself satisfy
                            // Decode — Encode-only would build a `decode` impl that
                            // calls a nonexistent (or half-built) decoder for the
                            // field's type.
                            if let Some(added_field) = s.fields.iter().find(|f| f.name == *field) {
                                if !super::CheckerCoreLib::is_decodable_ty(&added_field.ty, reg) {
                                    diags.push(e0910_add_field_not_decodable(
                                        &s.name, field, new_ty, span,
                                    ));
                                }
                            }
                        }
                    } else {
                        diags.push(e0910_op_unknown_field(&s.name, field, "add", span));
                    }
                }
                DeclaredOp::Change {
                    field,
                    from_ty,
                    to_ty,
                    ..
                } => {
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
                        && new_fields
                            .get(to.as_str())
                            .map_or(false, |t| t == &old_field.ty)
                }
                _ => false,
            });
            // Or by an explicit `remove old`?
            let removed = ops
                .iter()
                .any(|op| matches!(op, DeclaredOp::Remove { field } if field == &old_field.name));
            if renamed || removed {
                continue;
            }
            diags.push(e0910_dropped(
                &s.name,
                &old_field.name,
                &prior.published_version,
                span,
            ));
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
            // inline `via { … }` (D-MIGRATE2B step 1) OR an `impl Old => New`
            // in scope (step 2). Without either → ask for a converter (step 3).
            let change_op = ops.iter().find_map(|op| match op {
                DeclaredOp::Change {
                    field,
                    from_ty,
                    to_ty,
                    has_converter,
                } if field == &f.name && from_ty == old_ty && to_ty == &new_ty => {
                    Some(*has_converter)
                }
                _ => None,
            });
            match change_op {
                Some(true) => {} // inline converter present → OK
                Some(false) => {
                    // No inline `via`; look for an `impl Old => New` fallback.
                    let has_impl = conv_impls
                        .iter()
                        .any(|(from, to)| from == old_ty && to == &new_ty);
                    if !has_impl {
                        diags.push(e0910_change_no_converter(
                            &s.name, &f.name, old_ty, &new_ty, span,
                        ));
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
            let from_rename = ops
                .iter()
                .any(|op| matches!(op, DeclaredOp::Rename { to, .. } if to == &f.name));
            // Or declared `add f: T = default`?
            let added = ops
                .iter()
                .any(|op| matches!(op, DeclaredOp::Add { field, .. } if field == &f.name));
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
fn e0910_changed_type(
    type_name: &str,
    field: &str,
    old_ty: &str,
    new_ty: &str,
    span: Span,
) -> Diagnostic {
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
/// an `impl Old => New` in scope) is available.
fn e0910_change_no_converter(
    type_name: &str,
    field: &str,
    old_ty: &str,
    new_ty: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic::error(
        "E0910",
        format!(
            "the `change {}: {} -> {}` migration on `{}` has no converter",
            field, old_ty, new_ty, type_name
        ),
        "a type change needs a way to turn an old value into a new one — old data already on disk is read through it".to_string(),
        format!(
            "add an inline `via {{ (old) => … }}` to the `change` op, or declare `impl {} => {} {{ … }}` in scope",
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
fn e0910_add_type_mismatch(
    type_name: &str,
    field: &str,
    decl_ty: &str,
    real_ty: &str,
    span: Span,
) -> Diagnostic {
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

/// E0910: an `add f: T` op on a Decode-only `#PublishedSchema` record names a
/// type `T` that doesn't itself satisfy `Decode`. A Decode-only record only
/// ever deserializes; a field it can't decode would make the generated
/// `decode` impl call a decoder that doesn't exist for `T`.
fn e0910_add_field_not_decodable(type_name: &str, field: &str, ty: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0910",
        format!(
            "the `add {}: {}` migration on `{}` needs `{}` to support `Decode`",
            field, ty, type_name, ty
        ),
        format!(
            "`{}` derives `Decode` but not `Encode` — it only ever reads data back, so every field, including one added by a migration, must itself be decodable",
            type_name
        ),
        format!("derive `Decode` on `{}`, or give `{}` a decodable type", ty, field),
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

    fn zero() -> Span {
        Span::new(0, 0)
    }

    fn field(name: &str, ty: Type) -> Field {
        Field {
            is_pub: false,
            is_package_pub: false,
            name: name.to_string(),
            name_span: zero(),
            ty,
            ty_span: zero(),
            serde_markers: Vec::new(),
            redact: false,
            computed: None,
        }
    }

    fn published_struct(name: &str, fields: Vec<Field>) -> Item {
        Item::Struct(StructDef {
            span: zero(),
            is_pub: false,
            is_package_pub: false,
            name: name.to_string(),
            name_span: zero(),
            type_params: vec![],
            fields,
            methods: vec![],
            trait_impls: vec![],
            derives: vec![],
            auto_derive_default: true,
            is_published_schema: true,
            published_schema_span: Some(zero()),
            is_single_use: false,
            single_use_span: None,
            is_must_use: false,
            must_use_span: None,
            layout: None,
            layout_span: None,
            serde_markers: Vec::new(),
            type_markers: Vec::new(),
            validate_block: Vec::new(),
            validate_span: None,
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
    fn run_with_snapshot(snapshot: &str, items: &[Item]) -> Vec<String> {
        run_with_snapshot_and_registry(snapshot, items, &TraitRegistry::default())
    }

    /// Same as `run_with_snapshot`, but with a caller-supplied `TraitRegistry` —
    /// needed to exercise the Decode/Encode-aware `add`-op guard, which reads
    /// `reg.local_types`/`implements_trait`.
    fn run_with_snapshot_and_registry(
        snapshot: &str,
        items: &[Item],
        reg: &TraitRegistry,
    ) -> Vec<String> {
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
        let diags = check_schema_migrations(items, std::path::Path::new("."), reg);
        std::env::remove_var("JET_SCHEMA_CACHE_DIR");
        std::fs::remove_dir_all(&dir).ok();
        diags.into_iter().map(|d| d.code).collect()
    }

    const SNAP_ONE: &str =
        "schema_version = 1\ntype = Rec\npublished_version = 1.0.0\nfield name: String\n";

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
            migration(
                "Rec",
                vec![MigrationOp::Remove {
                    field: "name".into(),
                    field_span: zero(),
                }],
            ),
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
            published_struct(
                "Rec",
                vec![field("name", Type::String), field("extra", Type::Bool)],
            ),
            migration(
                "Rec",
                vec![MigrationOp::Add {
                    field: "extra".into(),
                    field_span: zero(),
                    ty: Type::Bool,
                    ty_span: zero(),
                    default: crate::AST::Expr::Bool(false, zero()),
                    default_span: zero(),
                    default_fn: None,
                }],
            ),
        ];
        assert!(run_with_snapshot(SNAP_ONE, &items).is_empty());
    }

    /// D-MIGRATE1 (I2 guard, bug #185/4): a Decode-only `#PublishedSchema` struct
    /// (derives `Decode`, not `Encode`) adds a field whose type is registered as
    /// local but does NOT implement `Decode` — currently unreachable in any
    /// shape parsing/codegen produce today, but guarded explicitly so a future
    /// shape can't silently emit rejected Rust (I2) instead of an E0910.
    #[test]
    fn add_op_on_decode_only_struct_needs_decodable_field_type() {
        let mut s = match published_struct(
            "Rec",
            vec![
                field("name", Type::String),
                field("payload", Type::Named("EncodeOnlyType".to_string())),
            ],
        ) {
            Item::Struct(s) => s,
            _ => unreachable!(),
        };
        s.derives = vec![(crate::Generics::DECODE.to_string(), zero())];
        let items = [
            Item::Struct(s),
            migration(
                "Rec",
                vec![MigrationOp::Add {
                    field: "payload".into(),
                    field_span: zero(),
                    ty: Type::Named("EncodeOnlyType".to_string()),
                    ty_span: zero(),
                    default: crate::AST::Expr::Bool(false, zero()),
                    default_span: zero(),
                    default_fn: None,
                }],
            ),
        ];
        // A registry where `EncodeOnlyType` is local but never implements Decode.
        let mut reg = TraitRegistry::default();
        reg.local_types.insert("EncodeOnlyType".to_string());
        assert_eq!(
            run_with_snapshot_and_registry(SNAP_ONE, &items, &reg),
            vec!["E0910"]
        );
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
                    converter: Some(crate::AST::Expr::Int(0, zero(), None, None)),
                    converter_span: Some(zero()),
                    conv_fn: None,
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
                    conv_fn: None,
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
            migration(
                "Rec",
                vec![MigrationOp::Remove {
                    field: "name".into(),
                    field_span: zero(),
                }],
            ),
        ];
        assert_eq!(run_with_snapshot(SNAP_ONE, &items), vec!["E0910"]);
    }

    #[test]
    fn no_prior_snapshot_is_clean() {
        // First release: no snapshot file → nothing to diff.
        let _guard = ENV_LOCK.lock().unwrap();
        let dir =
            std::env::temp_dir().join(format!("jet_schema_unit_empty_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("JET_SCHEMA_CACHE_DIR", &dir);
        let items = [published_struct("Brand", vec![field("x", Type::Int)])];
        let reg = TraitRegistry::default();
        let diags = check_schema_migrations(&items, std::path::Path::new("."), &reg);
        std::env::remove_var("JET_SCHEMA_CACHE_DIR");
        std::fs::remove_dir_all(&dir).ok();
        assert!(diags.is_empty());
    }
}
