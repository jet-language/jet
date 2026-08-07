use super::*;
use crate::Traits;
use crate::AST::{AccessConvention, ImportDecl, ImportKind, Item, ProgramBundle, Type};
use std::collections::HashMap;

/// Look up the pre-resolved module index for an import. Returns `None` for
/// core modules, C imports, and unqualified imports (they have no file target).
#[inline]
fn resolve_target(bundle: &ProgramBundle, module_idx: usize, imp: &ImportDecl) -> Option<usize> {
    bundle.import_targets.get(&(module_idx, imp.span)).copied()
}

fn qualify_unit_type(bundle: &ProgramBundle, target: usize, ty: &Type) -> Type {
    ty.map_named_types(&|name| {
        bundle.modules[target]
            .items
            .iter()
            .any(|item| matches!(item, Item::UnitFamily(family) if family.distinct_defs().iter().any(|member| member.name == name)))
            .then(|| format!("{}.{}", bundle.modules[target].alias, name))
    })
}
/// After `cx.foreign_types` is populated, add the foreign type names to
/// `cx.type_names` and re-run the cloneable/comparable checks for any local
/// structs or enums that reference those foreign types as fields.
pub(crate) fn update_cloneability_with_foreign_types(cx: &mut Cx, items: &[Item]) {
    for name in cx.foreign_types.keys() {
        cx.type_names.insert(name.clone());
    }
    for item in items {
        match item {
            Item::Struct(s) => {
                if !cx.cloneable.contains(&s.name) && type_is_cloneable_struct(s, &cx.type_names) {
                    cx.cloneable.insert(s.name.clone());
                }
                if !cx.comparable.contains(&s.name) && type_is_comparable_struct(s, &cx.type_names)
                {
                    cx.comparable.insert(s.name.clone());
                }
                if !cx.hashable.contains(&s.name) && type_is_hashable_struct(s, &cx.hashable) {
                    cx.hashable.insert(s.name.clone());
                }
            }
            Item::Enum(e) => {
                if !cx.cloneable.contains(&e.name) && type_is_cloneable_enum(e, &cx.type_names) {
                    cx.cloneable.insert(e.name.clone());
                }
                if !cx.comparable.contains(&e.name) && type_is_comparable_enum(e, &cx.type_names) {
                    cx.comparable.insert(e.name.clone());
                }
                if !cx.hashable.contains(&e.name) && type_is_hashable_enum(e, &cx.hashable) {
                    cx.hashable.insert(e.name.clone());
                }
            }
            _ => {}
        }
    }
}

/// Build a map from pub type name → Rust module path for all types defined in
/// imported file-modules of `module_idx`. Used by codegen to qualify cross-module
/// type references (e.g. `Note` → `user_note::user_Note`).
pub(crate) fn foreign_type_map(
    bundle: &ProgramBundle,
    module_idx: usize,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let module = &bundle.modules[module_idx];
    let is_local = |name: &str| {
        module.items.iter().any(|item| match item {
            Item::Struct(structure) => structure.name == name,
            Item::Enum(enumeration) => enumeration.name == name,
            _ => false,
        })
    };
    for imp in &module.imports {
        if imp.is_c_import() {
            continue;
        }
        if let Some(target) = resolve_target(bundle, module_idx, imp) {
            let rust_mod = format!("user_{}", bundle.modules[target].alias);
            for item in &bundle.modules[target].items {
                match item {
                    Item::Struct(s) if s.is_pub && !is_local(&s.name) => {
                        map.insert(s.name.clone(), rust_mod.clone());
                    }
                    Item::Enum(e) if e.is_pub && !is_local(&e.name) => {
                        map.insert(e.name.clone(), rust_mod.clone());
                    }
                    Item::UnitFamily(family) if family.is_pub => {
                        for member in family.distinct_defs() {
                            map.insert(
                                format!("{}.{}", bundle.modules[target].alias, member.name),
                                rust_mod.clone(),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    map
}

/// Populate `cx.variant_owner` and `cx.enum_variants` with pub enum variants
/// from imported file-modules, so cross-module pattern matching works in codegen.
pub(crate) fn register_foreign_enum_variants(
    cx: &mut Cx,
    bundle: &ProgramBundle,
    module_idx: usize,
) {
    let module = &bundle.modules[module_idx];
    for imp in &module.imports {
        if imp.is_c_import() {
            continue;
        }
        if let Some(target) = resolve_target(bundle, module_idx, imp) {
            for item in &bundle.modules[target].items {
                if let Item::Enum(e) = item {
                    if e.is_pub {
                        cx.enum_variants.entry(e.name.clone()).or_insert_with(|| {
                            e.variants
                                .iter()
                                .map(|v| (v.name.clone(), v.payload.clone()))
                                .collect()
                        });
                        for v in &e.variants {
                            cx.variant_owner
                                .entry(v.name.clone())
                                .or_insert_with(|| e.name.clone());
                        }
                    }
                }
            }
        }
    }
}

pub(crate) fn import_mod_map(bundle: &ProgramBundle, module_idx: usize) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let module = &bundle.modules[module_idx];
    for imp in &module.imports {
        let alias = imp.import_alias();
        // S59: C `use` forms target a synthetic merged module by alias.
        if imp.is_c_import() {
            if let Some(target) = bundle.cffi.target_for(module_idx, &alias) {
                let stem = &bundle.modules[target].alias;
                map.insert(alias, format!("user_{}", stem));
            }
            continue;
        }
        if let Some(target) = resolve_target(bundle, module_idx, imp) {
            let stem = &bundle.modules[target].alias;
            map.insert(alias, format!("user_{}", stem));
        }
    }
    map
}

/// D-MOD4: build the `pub use` re-export call map for `module_idx`. For each
/// imported module `A` (e.g. a directory module `text`), scan its own imports for
/// `pub use sub.Item` and map `(A, Item)` to the Rust module that really defines
/// it (`user_wrap`, `wrap`). Resolution is one level (matches sema's `reexports`).
pub(crate) fn reexport_call_map(
    bundle: &ProgramBundle,
    module_idx: usize,
) -> HashMap<(String, String), (String, String)> {
    let mut map = HashMap::new();
    let module = &bundle.modules[module_idx];
    for imp in &module.imports {
        if matches!(imp.kind, ImportKind::Unqualified { .. }) {
            continue;
        }
        let alias = imp.import_alias();
        let Some(target_idx) = resolve_target(bundle, module_idx, imp) else {
            continue;
        };
        let target = &bundle.modules[target_idx];
        for reimp in &target.imports {
            let ImportKind::Unqualified {
                module_alias,
                items,
                ..
            } = &reimp.kind
            else {
                continue;
            };
            if !reimp.is_pub {
                continue;
            }
            // Resolve `module_alias` within the target module's own imports.
            let real_idx = target
                .imports
                .iter()
                .filter(|i2| !matches!(i2.kind, ImportKind::Unqualified { .. }))
                .filter(|i2| i2.import_alias() == *module_alias)
                .find_map(|i2| resolve_target(bundle, target_idx, i2));
            if let Some(real_idx) = real_idx {
                let real_mod = format!("user_{}", bundle.modules[real_idx].alias);
                for (orig, alias_opt) in items {
                    let local = alias_opt.as_deref().unwrap_or(orig.as_str());
                    map.insert(
                        (alias.clone(), local.to_string()),
                        (real_mod.clone(), orig.clone()),
                    );
                }
            }
        }
    }
    map
}

pub(crate) fn core_import_map(
    bundle: &ProgramBundle,
    module_idx: usize,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let module = &bundle.modules[module_idx];
    for imp in &module.imports {
        if let Some(core_module) = imp.core_module_path() {
            map.insert(imp.import_alias(), core_module);
            continue;
        }
        // D-MOD3: `use core.item` / `use core.{a,b}` — bind each item name to its
        // full std path so that `item.method(...)` resolves the same way as
        // `import core.item as item` would.
        if let ImportKind::Unqualified {
            module_alias,
            items,
            ..
        } = &imp.kind
        {
            if module_alias == "core" || module_alias == "jet" {
                for (orig, alias_opt) in items {
                    let local = alias_opt.as_deref().unwrap_or(orig.as_str());
                    let full = format!("core.{}", orig);
                    if crate::Syntax::is_known_core_module(&full) {
                        map.insert(local.to_string(), full);
                    }
                }
            }
        }
    }
    map
}

/// D-MOD3: build unqualified-import maps for codegen.
/// Returns (inline_map, file_map) where:
///   inline_map: unqualified name → "alias__method" (for inline code modules)
///   file_map:   unqualified name → (rust_mod_name, fn_name) (for file modules)
pub(crate) fn unqualified_import_maps(
    bundle: &ProgramBundle,
    module_idx: usize,
) -> (HashMap<String, String>, HashMap<String, (String, String)>) {
    let mut inline_map: HashMap<String, String> = HashMap::new();
    let mut file_map: HashMap<String, (String, String)> = HashMap::new();
    let module = &bundle.modules[module_idx];
    // Build a set of inline code module aliases for this module.
    let code_mod_aliases: std::collections::HashSet<String> = module
        .items
        .iter()
        .filter_map(|item| {
            if let crate::AST::Item::CodeModule(cm) = item {
                if cm.body.is_some() {
                    return Some(cm.name.clone());
                }
            }
            None
        })
        .collect();
    for imp in &module.imports {
        let ImportKind::Unqualified {
            module_alias,
            items,
            ..
        } = &imp.kind
        else {
            continue;
        };
        if module_alias == "core" || module_alias == "jet" {
            // Std namespace — handled separately by core_import_map.
            continue;
        }
        if code_mod_aliases.contains(module_alias.as_str()) {
            // Inline code module.
            for (orig, alias_opt) in items {
                let local = alias_opt.as_deref().unwrap_or(orig.as_str());
                let key = format!("{}__{}", module_alias, orig);
                inline_map.insert(local.to_string(), key);
            }
        } else if let Some(target) = module
            .imports
            .iter()
            .filter(|i2| !matches!(i2.kind, ImportKind::Unqualified { .. }))
            .filter(|i2| i2.import_alias() == *module_alias)
            .find_map(|i2| resolve_target(bundle, module_idx, i2))
        {
            // File module: resolve the file-import whose alias matches, then point
            // each unqualified item at that Rust module.
            let rust_mod = format!("user_{}", bundle.modules[target].alias);
            for (orig, alias_opt) in items {
                let local = alias_opt.as_deref().unwrap_or(orig.as_str());
                file_map.insert(local.to_string(), (rust_mod.clone(), orig.clone()));
            }
        }
    }
    (inline_map, file_map)
}

pub(crate) fn import_sig_map(
    bundle: &ProgramBundle,
    module_idx: usize,
) -> HashMap<(String, String), Vec<(AccessConvention, Type)>> {
    let mut map = HashMap::new();
    let module = &bundle.modules[module_idx];
    for imp in &module.imports {
        let alias = imp.import_alias();
        // S59: C `use` forms — pull boundary fn sigs from the synthetic module.
        let target = if imp.is_c_import() {
            match bundle.cffi.target_for(module_idx, &alias) {
                Some(t) => t,
                None => continue,
            }
        } else {
            match resolve_target(bundle, module_idx, imp) {
                Some(t) => t,
                None => continue,
            }
        };
        for item in &bundle.modules[target].items {
            match item {
                Item::Func(f) if f.is_pub => {
                    map.insert(
                        (alias.clone(), f.name.clone()),
                        f.params
                            .iter()
                            .map(|p| (p.convention, qualify_unit_type(bundle, target, &p.ty)))
                            .collect(),
                    );
                }
                Item::CModule(cm) => {
                    for ef in &cm.functions {
                        map.insert(
                            (alias.clone(), ef.name.clone()),
                            ef.params
                                .iter()
                                .map(|p| (p.convention, p.ty.clone()))
                                .collect(),
                        );
                    }
                }
                _ => {}
            }
        }
    }
    // D-MOD4: re-exported items (`pub use sub.Item`) must carry the *real*
    // function's parameter conventions under the re-exporting alias, or calls
    // through the re-export would pass by value where a borrow is expected.
    for ((alias, item), (real_mod, real_fn)) in reexport_call_map(bundle, module_idx) {
        let stem = real_mod.strip_prefix("user_").unwrap_or(&real_mod);
        if let Some((real_idx, real)) = bundle.modules.iter().enumerate().find(|(_, m)| m.alias == stem) {
            for it in &real.items {
                if let Item::Func(f) = it {
                    if f.is_pub && f.name == real_fn {
                        map.insert(
                            (alias.clone(), item.clone()),
                            f.params
                                .iter()
                                .map(|p| (p.convention, qualify_unit_type(bundle, real_idx, &p.ty)))
                                .collect(),
                        );
                    }
                }
            }
        }
    }
    map
}

/// c109 Phase 14: build a map from `(import alias, function)` → the function's return
/// type, mirroring `import_sig_map`. The TIR carries this as the total result type of
/// a cross-module call. Covers file-module pub funcs, C-boundary funcs, and `pub use`
/// re-exports (the same shapes `import_sig_map` covers).
pub(crate) fn import_ret_map(
    bundle: &ProgramBundle,
    module_idx: usize,
) -> HashMap<(String, String), Option<Type>> {
    let mut map = HashMap::new();
    let module = &bundle.modules[module_idx];
    for imp in &module.imports {
        let alias = imp.import_alias();
        let target = if imp.is_c_import() {
            match bundle.cffi.target_for(module_idx, &alias) {
                Some(t) => t,
                None => continue,
            }
        } else {
            match resolve_target(bundle, module_idx, imp) {
                Some(t) => t,
                None => continue,
            }
        };
        for item in &bundle.modules[target].items {
            match item {
                Item::Func(f) if f.is_pub => {
                    let ret = f
                        .return_type
                        .as_ref()
                        .map(|ty| qualify_unit_type(bundle, target, ty));
                    map.insert((alias.clone(), f.name.clone()), ret);
                }
                Item::CModule(cm) => {
                    for ef in &cm.functions {
                        map.insert((alias.clone(), ef.name.clone()), ef.return_type.clone());
                    }
                }
                _ => {}
            }
        }
    }
    for ((alias, item), (real_mod, real_fn)) in reexport_call_map(bundle, module_idx) {
        let stem = real_mod.strip_prefix("user_").unwrap_or(&real_mod);
        if let Some((real_idx, real)) = bundle.modules.iter().enumerate().find(|(_, m)| m.alias == stem) {
            for it in &real.items {
                if let Item::Func(f) = it {
                    if f.is_pub && f.name == real_fn {
                        map.insert(
                            (alias.clone(), item.clone()),
                            f.return_type
                                .as_ref()
                                .map(|ty| qualify_unit_type(bundle, real_idx, ty)),
                        );
                    }
                }
            }
        }
    }
    map
}

pub(crate) fn emit_program_items(cx: &Cx, items: &[Item], out: &mut String, include_main: bool) {
    let has_serde_protocol_impl = items.iter().any(|item| match item {
        Item::Func(f) => f.type_params.iter().any(|param| param.bounds.iter().any(|bound| {
            matches!(bound.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE)
        })),
        Item::Struct(s) => s.trait_impls.iter().any(|block| {
            matches!(block.trait_name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE)
        }),
        Item::Enum(e) => e.trait_impls.iter().any(|block| {
            matches!(block.trait_name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE)
        }),
        Item::Impl(i) => i.trait_name.as_deref().is_some_and(|name| {
            matches!(name, crate::Generics::ENCODE | crate::Generics::DECODE)
        }),
        _ => false,
    });
    if !cx.root_prefix.is_empty() && has_serde_protocol_impl {
        out.push_str("use super::{user_Encode, user_Decode, jet_std};\n\n");
    }
    let tuple_shapes = collect_tuple_shapes(items);
    emit_tuple_structs(cx, &tuple_shapes, out);
    emit_anonymous_unions(cx, items, out);
    emit_synthetic_display_trait(out);
    emit_synthetic_operator_traits(out);
    emit_synthetic_close_trait(out);
    emit_synthetic_close_builtin_impls(cx, items, out);
    let (hi, hj, hk, hm) = program_iter_index_usage(items);
    emit_synthetic_iter_index_traits(out, hi, hj, hk, hm);
    // D-TXN-ROLLBACK layer 2: emit the synthetic Rollback trait iff this module has one.
    if program_has_rollback_impl(items) {
        emit_synthetic_rollback_trait(out);
    }
    for item in items {
        match item {
            Item::Trait(t) => Traits::emit_trait_def(t, out, |ty, assoc| {
                cx.rust_type_with_view_lifetime_assoc(ty, assoc)
            }),
            Item::Struct(s) => emit_struct(cx, s, out),
            Item::Enum(e) => emit_enum(cx, e, out),
            Item::Const(c) => emit_const(c, out),
            Item::CModule(cm) => emit_c_module(cx, cm, out),
            Item::Distinct(d) => emit_distinct(cx, d, out),
            // D-QUAL3: emit one distinct newtype per unit-family member.
            Item::UnitFamily(uf) => {
                for d in uf.distinct_defs() {
                    emit_distinct(cx, &d, out);
                }
            }
            Item::EffectDecl(_)
            | Item::MarkerDecl(_)
            | Item::Func(_) | Item::Impl(_) | Item::Test(_) | Item::Bench(_) | Item::ExternRust(_)
            | Item::Module(_) | Item::CodeModule(_) | Item::ErrorConv(_)
            | Item::Tag(_) // D-QUAL2: tags erase
            | Item::TypeAlias(_) // D-TYPEALIAS1: erases
            | Item::Migration(_) // D-MIGRATE1
            | Item::StateDecl(_) // D-STATE-DECL: erases
            | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2: erases
            | Item::UserDerive(_) // D-METADERIVE1=A: erase (expanded in sema)
            | Item::GenericModule(_) // D-GENMOD2=A: template — erases
            | Item::ModuleAlias(_) => {} // D-GENMOD2=A: alias — erases after expansion
        }
    }
    for item in items {
        match item {
            Item::Struct(s) => {
                emit_type_impl(cx, &s.name, &s.type_params, &s.methods, out);
                for block in &s.trait_impls {
                    emit_trait_impl(cx, &s.name, &s.type_params, block, Some(s), out);
                }
            }
            Item::Enum(e) => {
                emit_type_impl(cx, &e.name, &e.type_params, &e.methods, out);
                for block in &e.trait_impls {
                    emit_trait_impl(cx, &e.name, &e.type_params, block, None, out);
                }
            }
            Item::Impl(i) => {
                // D-OSTARGET1=A: an `impl` gated to a different native OS than
                // this build's active target is skipped entirely — mirrors how
                // `Codegen/Web.rs` filters function membership by `WebBucket`.
                if i.os_target.is_some_and(|os| os != cx.active_os) {
                    continue;
                }
                if i.trait_name.is_some() {
                    let struct_def = items.iter().find_map(|item| match item {
                        Item::Struct(s) if s.name == i.type_name => Some(s),
                        _ => None,
                    });
                    emit_external_trait_impl(cx, i, struct_def, out);
                } else {
                    emit_type_impl(
                        cx,
                        &i.type_name,
                        type_params_for_name(items, &i.type_name),
                        &i.methods,
                        out,
                    );
                }
            }
            // D-ERR-CONV: emit the conversion function.
            Item::ErrorConv(ec) => {
                emit_error_conv(cx, ec, out);
            }
            _ => {}
        }
    }
    for item in items {
        if let Item::Func(f) = item {
            if f.name == "run" && !include_main {
                continue;
            }
            // D-ANY-JAI1/D-VARARGBOUND1 (c7jaiany): a trait-bounded variadic
            // (`...Trait` / `...[A, B]`) has no single Rust signature — it's
            // emitted per call-site arity below instead (`VariadicBound.rs`).
            if cx.variadic_bound_fns.contains_key(&f.name) {
                continue;
            }
            emit_func(cx, f, out);
        }
    }
    // D-MOD2: emit inline code module functions with mangled names.
    for item in items {
        if let Item::CodeModule(cm) = item {
            if let Some(body) = &cm.body {
                for inner in body {
                    if let Item::Func(f) = inner {
                        if cx.variadic_bound_fns.contains_key(&f.name) {
                            continue;
                        }
                        let mut mangled_f = f.clone();
                        mangled_f.name = format!("{}__{}", cm.name, f.name);
                        emit_func(cx, &mangled_f, out);
                    }
                }
            }
        }
    }
    // D-ANY-JAI1: emit exactly the per-arity specializations call sites above
    // actually needed (`Cx::needed_variadic_arities`, populated while lowering
    // those call sites — see `TIR/lower.rs::lower_variadic_bound_call`).
    crate::Codegen::VariadicBound::emit_variadic_bound_specializations(cx, items, out);
}
