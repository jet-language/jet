//! D-HOTSWAP1 — type-surface stability check for `jet dev` (incl.
//! `jet dev --swap`, D-CLI-DEVSERVE1=A: was `jet serve`).
//!
//! The hot-reload unit is a MODULE. When a resident program's file changes,
//! the watch loop asks this pass whether the edit is *type-stable*: a change
//! that touches only function *bodies* / statements keeps the same type
//! surface and takes the fast swap path; a change to a struct field, an enum
//! variant, or a function signature changes the surface and forces a clean,
//! announced restart (E2210).
//!
//! This is a pure sema walk over the two parsed bundles' declared types — it
//! NEVER asks rustc whether the layout changed (I3: all checking lives in
//! sema). State retention is emitted per canonical `#Persist` binding, so an
//! unrelated module-surface change cannot reset an unchanged value.

use crate::Diagnostics::Diagnostic;
use crate::AST::{
    EnumDef, Func, Item, LoadedModule, MigrationOp, ProgramBundle, StructDef, Type,
    VariantPayload,
};
use crate::Sema::Schema::{canonical_migration_shape, load_snapshot};
use jet_foundation::HotSwap::{
    HotSwapCompatibility, HotSwapDecision, StateRetentionFact,
};
use jet_foundation::SchemaMigration::{
    SchemaMigrationOp, SchemaMigrationPlan, SchemaMigrationStep,
};


/// E2210: a hot-swap edit changed a type surface, so `jet dev` must restart
/// rather than swap. `what_changed` is the human summary the
/// caller also prints on the `[restart]` line.
pub fn e2210(what_changed: &str) -> Diagnostic {
    Diagnostic::error(
        "E2210",
        format!(
            "this edit changed a type, so `jet dev` is restarting instead of swapping — {}",
            what_changed
        ),
        "a hot swap re-applies code while the program's types stay the same; \
         changing a struct field, an enum variant, or a function signature \
         changes the shape of your data, so the running code is rebuilt cleanly \
         from the new types"
            .to_string(),
        "nothing to fix — `jet dev` restarted with the new types; this note just \
         explains why the swap became a restart. Type-stable edits (function \
         bodies, statements) swap without a restart"

            .to_string(),
        None,
    )
}
fn canonical_migration_plans(
    bundle: &ProgramBundle,
    module: &LoadedModule,
) -> Vec<SchemaMigrationPlan> {
    let mut plans = Vec::new();
    for item in &module.items {
        let Item::Struct(structure) = item else { continue };
        let published = structure.is_published_schema
            || structure
                .derives
                .iter()
                .any(|(marker, _)| marker == crate::Syntax::MARKER_PUBLISHED_SCHEMA);
        if !published {
            continue;
        }
        let Some(_snapshot) = load_snapshot(&bundle.project_root, &structure.name) else {
            continue;
        };
        let migrations = module
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Migration(m) if m.type_name == structure.name => Some(m.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if migrations.is_empty() {
            continue;
        }
        let Ok(canonical) =
            canonical_migration_shape(&module.display, structure, &migrations)
        else {
            continue;
        };
        let mut steps = Vec::with_capacity(migrations.len());
        for (block_index, migration) in migrations.iter().enumerate() {
            let ops = migration
                .ops
                .iter()
                .map(|op| match op {
                    MigrationOp::Rename { from, to, .. } => SchemaMigrationOp::Rename {
                        from_key: canonical.json_key(from),
                        to_key: canonical.json_key(to),
                    },
                    MigrationOp::Remove { field, .. } => SchemaMigrationOp::Remove {
                        key: canonical.json_key(field),
                    },
                    MigrationOp::Add {
                        field,
                        ty,
                        default_fn,
                        ..
                    } => SchemaMigrationOp::Add {
                        key: canonical.json_key(field),
                        type_name: ty.name(),
                        default_fn: default_fn.clone().unwrap_or_default(),
                    },
                    MigrationOp::Change {
                        field,
                        from_ty,
                        to_ty,
                        conv_fn,
                        ..
                    } => SchemaMigrationOp::Change {
                        key: canonical.json_key(field),
                        from_type: from_ty.name(),
                        to_type: to_ty.name(),
                        converter_fn: conv_fn.clone().unwrap_or_default(),
                    },
                })
                .collect::<Vec<_>>();
            steps.push(SchemaMigrationStep::new(
                format!("v{}", block_index + 1),
                format!("v{}", block_index + 2),
                ops,
            ));
        }
        if steps.iter().all(|step| step.ops.is_empty())
            || steps.iter().flat_map(|step| step.ops.iter()).any(|op| {
                matches!(
                    op,
                    SchemaMigrationOp::Add { default_fn, .. }
                        if default_fn.is_empty()
                ) || matches!(
                    op,
                    SchemaMigrationOp::Change { converter_fn, .. }
                        if converter_fn.is_empty()
                )
            })
        {
            continue;
        }
        plans.push(SchemaMigrationPlan::new(
            structure.name.clone(),
            canonical.historical,
            steps,
        ));
    }
    plans
}

/// Compare the type surface of the named module across two bundles and emit
/// the one backend-neutral decision every live adapter consumes.
///
/// The decision is successful even when the edit is incompatible: an
/// incompatible edit is a normal semantic verdict that carries its restart
/// reason and fresh-state facts. `Err` remains reserved for future semantic
/// failures that prevent a verdict from being produced.
///
/// `module_name` matches a module by `display` path or import `alias`; if
/// neither bundle has it, the entry module is compared (the common single-file
/// dev case).
pub fn type_stable_decision(
    old: &ProgramBundle,
    new: &ProgramBundle,
    module_name: &str,
) -> Result<HotSwapDecision, Vec<Diagnostic>> {
    let old_mod = pick_module(old, module_name);
    let new_mod = pick_module(new, module_name);
    let changed = match (old_mod, new_mod) {
        (Some(o), Some(n)) => surface_changes(o, n),
        // A module that appeared or vanished is itself a surface change.
        (Some(_), None) | (None, Some(_)) => {
            vec![format!("the module `{}` was added or removed", module_name)]
        }
        (None, None) => Vec::new(),
    };
    let changed_functions = match (old_mod, new_mod) {
        (Some(o), Some(n)) => changed_function_ids(o, n),
        (Some(o), None) => callable_functions(o).into_keys().collect(),
        (None, Some(n)) => callable_functions(n).into_keys().collect(),
        (None, None) => Vec::new(),
    };
    let compatibility = if changed.is_empty() {
        HotSwapCompatibility::Compatible
    } else {
        HotSwapCompatibility::Incompatible {
            reason: changed.join("; "),
        }
    };
    let state = persistent_state_facts(old_mod, new_mod, compatibility.reason());
    let schema_migrations = new_mod
        .map(|module| canonical_migration_plans(new, module))
        .unwrap_or_default();
    Ok(HotSwapDecision {
        module: module_name.to_string(),
        compatibility,
        changed,
        changed_functions,
        state,
        schema_migrations,
        rechecked_items: Vec::new(),
        change_facts: Vec::new(),
    })
}

/// Find the module to diff: by `display` path, else by import `alias`, else the
/// entry module.
fn pick_module<'a>(bundle: &'a ProgramBundle, name: &str) -> Option<&'a LoadedModule> {
    if let Some(m) = bundle
        .modules
        .iter()
        .find(|m| m.display == name || m.alias == name)
    {
        return Some(m);
    }
    bundle.modules.get(bundle.entry)
}

/// All type-surface differences between two modules, in deterministic
/// declaration order. An empty vector means the edit is type-stable.
fn surface_changes(old: &LoadedModule, new: &LoadedModule) -> Vec<String> {
    let mut changes = Vec::new();
    let old_structs = structs(old);
    let new_structs = structs(new);
    for (name, s) in &old_structs {
        match new_structs.get(name) {
            None => changes.push(format!("struct `{}` was removed", name)),
            Some(n) => {
                if let Some(change) = struct_diff(name, s, n) {
                    changes.push(change);
                }
            }
        }
    }
    for name in new_structs.keys() {
        if !old_structs.contains_key(name) {
            changes.push(format!("struct `{}` was added", name));
        }
    }

    let old_enums = enums(old);
    let new_enums = enums(new);
    for (name, e) in &old_enums {
        match new_enums.get(name) {
            None => changes.push(format!("enum `{}` was removed", name)),
            Some(n) => {
                if let Some(change) = enum_diff(name, e, n) {
                    changes.push(change);
                }
            }
        }
    }
    for name in new_enums.keys() {
        if !old_enums.contains_key(name) {
            changes.push(format!("enum `{}` was added", name));
        }
    }

    let old_functions = callable_functions(old);
    let new_functions = callable_functions(new);
    for (identity, old_function) in &old_functions {
        match new_functions.get(identity) {
            None => changes.push(format!("function `{}` was removed", old_function.name)),
            Some(new_function) => {
                if let Some(change) = fn_sig_diff(&old_function.name, old_function, new_function) {
                    changes.push(change);
                }
            }
        }
    }
    for (identity, new_function) in &new_functions {
        if !old_functions.contains_key(identity) {
            changes.push(format!("function `{}` was added", new_function.name));
        }
    }

    changes
}

fn structs(m: &LoadedModule) -> std::collections::BTreeMap<&str, &StructDef> {
    let mut out = std::collections::BTreeMap::new();
    for item in &m.items {
        if let Item::Struct(definition) = item {
            out.insert(definition.name.as_str(), definition);
        }
    }
    out
}

fn enums(m: &LoadedModule) -> std::collections::BTreeMap<&str, &EnumDef> {
    let mut out = std::collections::BTreeMap::new();
    for item in &m.items {
        if let Item::Enum(definition) = item {
            out.insert(definition.name.as_str(), definition);
        }
    }
    out
}

fn changed_function_ids(old: &LoadedModule, new: &LoadedModule) -> Vec<String> {
    let old_functions = callable_functions(old);
    let new_functions = callable_functions(new);
    let mut changed = Vec::new();
    for (identity, old_function) in &old_functions {
        match new_functions.get(identity) {
            None => changed.push(identity.clone()),
            Some(new_function)
                if fn_sig_diff(&old_function.name, old_function, new_function).is_some()
                    || function_body_changed(old, old_function, new, new_function) =>
            {
                changed.push(identity.clone());
            }
            Some(_) => {}
        }
    }
    for identity in new_functions.keys() {
        if !old_functions.contains_key(identity) {
            changed.push(identity.clone());
        }
    }
    changed
}

fn function_body_changed(
    old_module: &LoadedModule,
    old: &Func,
    new_module: &LoadedModule,
    new: &Func,
) -> bool {
    let old_source = old_module.source.get(old.span.start..old.span.end);
    let new_source = new_module.source.get(new.span.start..new.span.end);
    old_source != new_source || format!("{:?}", old.body) != format!("{:?}", new.body)
}


// ── callable identity collection ────────────────────────────────────

fn callable_functions(m: &LoadedModule) -> std::collections::BTreeMap<String, &Func> {
    let module = module_identity(m);
    let mut out = std::collections::BTreeMap::new();
    for item in &m.items {
        match item {
            Item::Func(function) => {
                out.insert(format!("{module}::fn:{}", function.name), function);
            }
            Item::Struct(definition) => {
                for function in &definition.methods {
                    out.insert(
                        format!("{module}::struct:{}::method:{}", definition.name, function.name),
                        function,
                    );
                }
                for implementation in &definition.trait_impls {
                    for function in &implementation.methods {
                        out.insert(
                            format!(
                                "{module}::struct:{}::trait:{}::method:{}",
                                definition.name, implementation.trait_name, function.name
                            ),
                            function,
                        );
                    }
                }
            }
            Item::Enum(definition) => {
                for function in &definition.methods {
                    out.insert(
                        format!("{module}::enum:{}::method:{}", definition.name, function.name),
                        function,
                    );
                }
                for implementation in &definition.trait_impls {
                    for function in &implementation.methods {
                        out.insert(
                            format!(
                                "{module}::enum:{}::trait:{}::method:{}",
                                definition.name, implementation.trait_name, function.name
                            ),
                            function,
                        );
                    }
                }
            }
            Item::Impl(implementation) => {
                for function in &implementation.methods {
                    out.insert(
                        format!(
                            "{module}::impl:{}::{}::method:{}",
                            implementation.type_name,
                            implementation.trait_name.as_deref().unwrap_or("inherent"),
                            function.name
                        ),
                        function,
                    );
                }
            }
            _ => {}
        }
    }
    out
}

fn persistent_state_facts(
    old: Option<&LoadedModule>,
    new: Option<&LoadedModule>,
    reason: Option<&str>,
) -> Vec<StateRetentionFact> {
    let old_bindings = persistent_bindings(old);
    let new_bindings = persistent_bindings(new);
    let mut keys = old_bindings
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    keys.extend(new_bindings.keys().cloned());
    let fresh_reason = reason.unwrap_or("persistent state key was added or removed");
    keys.into_iter()
        .map(|key| {
            let old_present = old_bindings.contains_key(&key);
            let new_present = new_bindings.contains_key(&key);
            let old_ty = old_bindings.get(&key).copied().flatten();
            let new_ty = new_bindings.get(&key).copied().flatten();
            match (old_present, new_present, old_ty, new_ty) {
                (true, true, Some(old_ty), Some(new_ty)) if types_eq(old_ty, new_ty) => {
                    StateRetentionFact::preserve(key)
                }
                (true, true, Some(_), Some(_)) => StateRetentionFact::fresh(
                    key,
                    "persistent binding type changed; reset to the new declaration",
                ),
                (true, true, _, _) => {
                    StateRetentionFact::fresh(key, "persistent binding type is unavailable")
                }
                (true, false, _, _) | (false, true, _, _) => {
                    StateRetentionFact::fresh(key, fresh_reason)
                }
                (false, false, _, _) => unreachable!("persistent key came from neither bundle"),
            }
        })
        .collect()
}

fn persistent_bindings<'a>(
    module: Option<&'a LoadedModule>,
) -> std::collections::BTreeMap<String, Option<&'a Type>> {
    let Some(module) = module else {
        return std::collections::BTreeMap::new();
    };
    module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Const(constant) if constant.is_persist => Some((
                format!("{}::{}", module.alias, constant.name),
                constant.ty.as_ref(),
            )),
            _ => None,
        })
        .collect()
}

fn module_identity(module: &LoadedModule) -> &str {
    if module.display.is_empty() {
        module.alias.as_str()
    } else {
        module.display.as_str()
    }
}

// ── per-declaration diffs ───────────────────────────────────────────

fn struct_diff(name: &str, old: &StructDef, new: &StructDef) -> Option<String> {
    if !type_params_eq(&old.type_params, &new.type_params) {
        return Some(format!("struct `{}` changed its type parameters", name));
    }
    if old.layout != new.layout {
        return Some(format!("struct `{}` changed its layout", name));
    }
    if old.fields.len() != new.fields.len() {
        return Some(format!("struct `{}` changed its fields", name));
    }
    for (o, n) in old.fields.iter().zip(&new.fields) {
        if o.name != n.name {
            return Some(format!(
                "struct `{}` renamed field `{}` to `{}`",
                name, o.name, n.name
            ));
        }
        if !types_eq(&o.ty, &n.ty) {
            return Some(format!("struct `{}` retyped field `{}`", name, o.name));
        }
    }
    None
}

fn enum_diff(name: &str, old: &EnumDef, new: &EnumDef) -> Option<String> {
    if !type_params_eq(&old.type_params, &new.type_params) {
        return Some(format!("enum `{}` changed its type parameters", name));
    }
    if old.c_layout_tag() != new.c_layout_tag() {
        return Some(format!("enum `{}` changed its layout", name));
    }
    if old.variants.len() != new.variants.len() {
        return Some(format!("enum `{}` changed its variants", name));
    }
    for (o, n) in old.variants.iter().zip(&new.variants) {
        if o.name != n.name {
            return Some(format!(
                "enum `{}` renamed variant `{}` to `{}`",
                name, o.name, n.name
            ));
        }
        if o.discriminant != n.discriminant {
            return Some(format!(
                "enum `{}` changed the discriminant of variant `{}`",
                name, o.name
            ));
        }
        if !payload_eq(&o.payload, &n.payload) {
            return Some(format!(
                "enum `{}` changed the payload of variant `{}`",
                name, o.name
            ));
        }
    }
    None
}

fn fn_sig_diff(name: &str, old: &Func, new: &Func) -> Option<String> {
    if !type_params_eq(&old.type_params, &new.type_params) {
        return Some(format!("function `{}` changed its type parameters", name));
    }
    if old.params.len() != new.params.len() {
        return Some(format!("function `{}` changed its parameters", name));
    }
    for (o, n) in old.params.iter().zip(&new.params) {
        if !types_eq(&o.ty, &n.ty) || o.convention != n.convention {
            return Some(format!("function `{}` changed a parameter type", name));
        }
    }
    if !opt_types_eq(&old.return_type, &new.return_type) {
        return Some(format!("function `{}` changed its return type", name));
    }
    None
}

fn type_params_eq(old: &[crate::AST::TypeParam], new: &[crate::AST::TypeParam]) -> bool {
    old.len() == new.len()
        && old.iter().zip(new).all(|(left, right)| {
            left.name == right.name && left.bounds == right.bounds
        })
}

// ── structural type equality ────────────────────────────────────────

fn types_eq(a: &Type, b: &Type) -> bool {
    a == b
}

fn opt_types_eq(a: &Option<Type>, b: &Option<Type>) -> bool {
    a == b
}

fn payload_eq(a: &VariantPayload, b: &VariantPayload) -> bool {
    match (a, b) {
        (VariantPayload::Unit, VariantPayload::Unit) => true,
        (VariantPayload::Single(ta, _), VariantPayload::Single(tb, _)) => types_eq(ta, tb),
        (VariantPayload::Named(fa), VariantPayload::Named(fb)) => {
            fa.len() == fb.len()
                && fa
                    .iter()
                    .zip(fb)
                    .all(|(x, y)| x.name == y.name && types_eq(&x.ty, &y.ty))
        }
        _ => false,
    }
}
