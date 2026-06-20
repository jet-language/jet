use super::*;
use crate::AST::{
    AccessConvention, ImportKind, Item, ProgramBundle, Type,
};
use crate::Loader;
use crate::M9;
use std::collections::HashMap;
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
                if !cx.comparable.contains(&s.name)
                    && type_is_comparable_struct(s, &cx.type_names)
                {
                    cx.comparable.insert(s.name.clone());
                }
            }
            Item::Enum(e) => {
                if !cx.cloneable.contains(&e.name) && type_is_cloneable_enum(e, &cx.type_names) {
                    cx.cloneable.insert(e.name.clone());
                }
                if !cx.comparable.contains(&e.name) && type_is_comparable_enum(e, &cx.type_names)
                {
                    cx.comparable.insert(e.name.clone());
                }
            }
            _ => {}
        }
    }
}

/// Build a map from pub type name → Rust module path for all types defined in
/// imported file-modules of `module_idx`. Used by codegen to qualify cross-module
/// type references (e.g. `Note` → `user_note::user_Note`).
pub(crate) fn foreign_type_map(bundle: &ProgramBundle, module_idx: usize) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let module = &bundle.modules[module_idx];
    for imp in &module.imports {
        if crate::CFFI::is_c_import(imp) {
            continue;
        }
        if let Ok(target) = Loader::resolve_import_target(bundle, module_idx, imp) {
            let rust_mod = format!("user_{}", bundle.modules[target].alias);
            for item in &bundle.modules[target].items {
                match item {
                    Item::Struct(s) if s.is_pub => {
                        map.insert(s.name.clone(), rust_mod.clone());
                    }
                    Item::Enum(e) if e.is_pub => {
                        map.insert(e.name.clone(), rust_mod.clone());
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
pub(crate) fn register_foreign_enum_variants(cx: &mut Cx, bundle: &ProgramBundle, module_idx: usize) {
    let module = &bundle.modules[module_idx];
    for imp in &module.imports {
        if crate::CFFI::is_c_import(imp) {
            continue;
        }
        if let Ok(target) = Loader::resolve_import_target(bundle, module_idx, imp) {
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
        let alias = Loader::import_alias(imp);
        // S59: C `use` forms target a synthetic merged module by alias.
        if crate::CFFI::is_c_import(imp) {
            if let Some(target) = bundle.cffi.target_for(module_idx, &alias) {
                let stem = &bundle.modules[target].alias;
                map.insert(alias, format!("user_{}", stem));
            }
            continue;
        }
        if let Ok(target) = Loader::resolve_import_target(bundle, module_idx, imp) {
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
        let alias = Loader::import_alias(imp);
        let Ok(target_idx) = Loader::resolve_import_target(bundle, module_idx, imp) else {
            continue;
        };
        let target = &bundle.modules[target_idx];
        for reimp in &target.imports {
            let ImportKind::Unqualified { module_alias, items, .. } = &reimp.kind else {
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
                .filter(|i2| Loader::import_alias(i2) == *module_alias)
                .find_map(|i2| Loader::resolve_import_target(bundle, target_idx, i2).ok());
            if let Some(real_idx) = real_idx {
                let real_mod = format!("user_{}", bundle.modules[real_idx].alias);
                for item in items {
                    map.insert(
                        (alias.clone(), item.clone()),
                        (real_mod.clone(), item.clone()),
                    );
                }
            }
        }
    }
    map
}

pub(crate) fn std_import_map(bundle: &ProgramBundle, module_idx: usize) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let module = &bundle.modules[module_idx];
    for imp in &module.imports {
        if let Some(std_module) = Loader::std_module_path(imp) {
            map.insert(Loader::import_alias(imp), std_module);
            continue;
        }
        // D-MOD3: `use core.item` / `use core.{a,b}` — bind each item name to its
        // full std path so that `item.method(...)` resolves the same way as
        // `import core.item as item` would.
        if let ImportKind::Unqualified { module_alias, items, .. } = &imp.kind {
            if module_alias == "core" || module_alias == "jet" {
                for item in items {
                    let full = format!("core.{}", item);
                    if Loader::is_known_std_module(&full) {
                        map.insert(item.clone(), full);
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
        let ImportKind::Unqualified { module_alias, items, .. } = &imp.kind else {
            continue;
        };
        if module_alias == "core" || module_alias == "jet" {
            // Std namespace — handled separately by std_import_map.
            continue;
        }
        if code_mod_aliases.contains(module_alias.as_str()) {
            // Inline code module.
            for item in items {
                let key = format!("{}__{}", module_alias, item);
                inline_map.insert(item.clone(), key);
            }
        } else if let Some(target) = module
            .imports
            .iter()
            .filter(|i2| !matches!(i2.kind, ImportKind::Unqualified { .. }))
            .filter(|i2| Loader::import_alias(i2) == *module_alias)
            .find_map(|i2| Loader::resolve_import_target(bundle, module_idx, i2).ok())
        {
            // File module: resolve the file-import whose alias matches, then point
            // each unqualified item at that Rust module.
            let rust_mod = format!("user_{}", bundle.modules[target].alias);
            for item in items {
                file_map.insert(item.clone(), (rust_mod.clone(), item.clone()));
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
        let alias = Loader::import_alias(imp);
        // S59: C `use` forms — pull boundary fn sigs from the synthetic module.
        let target = if crate::CFFI::is_c_import(imp) {
            match bundle.cffi.target_for(module_idx, &alias) {
                Some(t) => t,
                None => continue,
            }
        } else {
            match Loader::resolve_import_target(bundle, module_idx, imp) {
                Ok(t) => t,
                Err(_) => continue,
            }
        };
        for item in &bundle.modules[target].items {
            match item {
                Item::Func(f) if f.is_pub => {
                    map.insert(
                        (alias.clone(), f.name.clone()),
                        f.params
                            .iter()
                            .map(|p| (p.convention, p.ty.clone()))
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
        if let Some(real) = bundle.modules.iter().find(|m| m.alias == stem) {
            for it in &real.items {
                if let Item::Func(f) = it {
                    if f.is_pub && f.name == real_fn {
                        map.insert(
                            (alias.clone(), item.clone()),
                            f.params
                                .iter()
                                .map(|p| (p.convention, p.ty.clone()))
                                .collect(),
                        );
                    }
                }
            }
        }
    }
    map
}

pub(crate) fn emit_program_items(cx: &Cx, items: &[Item], out: &mut String, include_main: bool) {
    let tuple_shapes = collect_tuple_shapes(items);
    emit_tuple_structs(cx, &tuple_shapes, out);
    for item in items {
        match item {
            Item::Trait(t) => M9::emit_trait_def(t, out),
            Item::Struct(s) => emit_struct(cx, s, out),
            Item::Enum(e) => emit_enum(cx, e, out),
            Item::Const(c) => emit_const(c, out),
            Item::CModule(cm) => emit_c_module(cm, out),
            Item::Distinct(d) => emit_distinct(cx, d, out),
            Item::Func(_) | Item::Impl(_) | Item::Test(_) | Item::ExternRust(_)
            | Item::Module(_) | Item::CodeModule(_) => {}
        }
    }
    for item in items {
        match item {
            Item::Struct(s) => {
                emit_type_impl(cx, &s.name, &s.type_params, &s.methods, out);
                for block in &s.trait_impls {
                    emit_trait_impl(cx, &s.name, &s.type_params, block, out);
                }
            }
            Item::Enum(e) => {
                emit_type_impl(cx, &e.name, &e.type_params, &e.methods, out);
                for block in &e.trait_impls {
                    emit_trait_impl(cx, &e.name, &e.type_params, block, out);
                }
            }
            Item::Impl(i) => {
                if i.trait_name.is_some() {
                    emit_external_trait_impl(cx, i, out);
                } else {
                    emit_type_impl(cx, &i.type_name, &[], &i.methods, out);
                }
            }
            _ => {}
        }
    }
    for item in items {
        if let Item::Func(f) = item {
            if f.name == "main" && !include_main {
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
                        let mut mangled_f = f.clone();
                        mangled_f.name = format!("{}__{}", cm.name, f.name);
                        emit_func(cx, &mangled_f, out);
                    }
                }
            }
        }
    }
}
