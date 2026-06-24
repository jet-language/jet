//! c77 (D-HOTSWAP1=B) — type-surface stability check for `jet dev`/`jet serve`.
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
//! sema). It compares declared shapes directly.

use crate::AST::{EnumDef, Func, Item, LoadedModule, ProgramBundle, StructDef, Type, VariantPayload};
use crate::Diagnostics::Diagnostic;

/// E2210: a hot-swap edit changed a type surface, so `jet dev`/`jet serve`
/// must restart rather than swap. `what_changed` is the human summary the
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

/// Compare the struct/enum/fn type surfaces of the named module across two
/// bundles. `Ok(())` = type-stable (safe to swap); `Err` = the surface changed
/// (must restart), carrying E2210 with the first change found.
///
/// `module_name` matches a module by its `display` path or its import `alias`;
/// if neither bundle has it, the entry module is compared (the common single-
/// file dev case).
pub fn type_stable_check(
    old: &ProgramBundle,
    new: &ProgramBundle,
    module_name: &str,
) -> Result<(), Vec<Diagnostic>> {
    let old_mod = pick_module(old, module_name);
    let new_mod = pick_module(new, module_name);
    match (old_mod, new_mod) {
        (Some(o), Some(n)) => match surface_diff(o, n) {
            Some(change) => Err(vec![e2210(&change)]),
            None => Ok(()),
        },
        // A module that appeared or vanished is itself a surface change.
        (Some(_), None) | (None, Some(_)) => {
            Err(vec![e2210(&format!("the module `{}` was added or removed", module_name))])
        }
        (None, None) => Ok(()),
    }
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

/// First type-surface difference between two modules, as a human summary, or
/// `None` if the surfaces match.
fn surface_diff(old: &LoadedModule, new: &LoadedModule) -> Option<String> {
    let old_structs = structs(old);
    let new_structs = structs(new);
    for (name, s) in &old_structs {
        match new_structs.get(name) {
            None => return Some(format!("struct `{}` was removed", name)),
            Some(n) => {
                if let Some(c) = struct_diff(name, s, n) {
                    return Some(c);
                }
            }
        }
    }
    for name in new_structs.keys() {
        if !old_structs.contains_key(name) {
            return Some(format!("struct `{}` was added", name));
        }
    }

    let old_enums = enums(old);
    let new_enums = enums(new);
    for (name, e) in &old_enums {
        match new_enums.get(name) {
            None => return Some(format!("enum `{}` was removed", name)),
            Some(n) => {
                if let Some(c) = enum_diff(name, e, n) {
                    return Some(c);
                }
            }
        }
    }
    for name in new_enums.keys() {
        if !old_enums.contains_key(name) {
            return Some(format!("enum `{}` was added", name));
        }
    }

    let old_fns = funcs(old);
    let new_fns = funcs(new);
    for (name, f) in &old_fns {
        match new_fns.get(name) {
            None => return Some(format!("function `{}` was removed", name)),
            Some(n) => {
                if let Some(c) = fn_sig_diff(name, f, n) {
                    return Some(c);
                }
            }
        }
    }
    for name in new_fns.keys() {
        if !old_fns.contains_key(name) {
            return Some(format!("function `{}` was added", name));
        }
    }

    None
}

// ── collectors ──────────────────────────────────────────────────────

fn structs(m: &LoadedModule) -> std::collections::BTreeMap<&str, &StructDef> {
    let mut out = std::collections::BTreeMap::new();
    for item in &m.items {
        if let Item::Struct(s) = item {
            out.insert(s.name.as_str(), s);
        }
    }
    out
}

fn enums(m: &LoadedModule) -> std::collections::BTreeMap<&str, &EnumDef> {
    let mut out = std::collections::BTreeMap::new();
    for item in &m.items {
        if let Item::Enum(e) = item {
            out.insert(e.name.as_str(), e);
        }
    }
    out
}

fn funcs(m: &LoadedModule) -> std::collections::BTreeMap<&str, &Func> {
    let mut out = std::collections::BTreeMap::new();
    for item in &m.items {
        if let Item::Func(f) = item {
            out.insert(f.name.as_str(), f);
        }
    }
    out
}

// ── per-declaration diffs ───────────────────────────────────────────

fn struct_diff(name: &str, old: &StructDef, new: &StructDef) -> Option<String> {
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
            return Some(format!(
                "struct `{}` retyped field `{}`",
                name, o.name
            ));
        }
    }
    None
}

fn enum_diff(name: &str, old: &EnumDef, new: &EnumDef) -> Option<String> {
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
