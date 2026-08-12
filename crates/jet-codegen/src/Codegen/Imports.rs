use super::*;
use crate::Traits;
use crate::AST::{AccessConvention, ImportDecl, ImportKind, Item, ProgramBundle, Type};
use std::collections::{HashMap, HashSet};

fn all_imports(
    module: &crate::AST::LoadedModule,
) -> impl Iterator<Item = (Option<&str>, &ImportDecl)> {
    crate::AST::walk_imports(module)
        .into_iter()
        .map(|(scope, import)| (scope, import))
}

fn foreign_imports_after_frontend(
    imp: &ImportDecl,
) -> Vec<(crate::AST::ForeignNamespace, String)> {
    imp.foreign_imports().unwrap_or_else(|error| {
        unreachable!(
            "invalid foreign import reached codegen after sema: {}",
            error.path
        )
    })
}

fn is_c_import_after_frontend(imp: &ImportDecl) -> bool {
    imp.is_c_import().unwrap_or_else(|error| {
        unreachable!(
            "invalid foreign import reached codegen after sema: {}",
            error.path
        )
    })
}

fn is_foreign_member_list(imp: &ImportDecl) -> bool {
    matches!(imp.kind, ImportKind::Unqualified { .. })
        && !foreign_imports_after_frontend(imp).is_empty()
}

/// Look up the pre-resolved module index for an import. Returns `None` for
/// core modules, C imports, and unqualified imports (they have no file target).
#[inline]
fn resolve_target(bundle: &ProgramBundle, module_idx: usize, imp: &ImportDecl) -> Option<usize> {
    bundle.import_targets.get(&(module_idx, imp.span)).copied()
}

fn required_import_target(bundle: &ProgramBundle, module_idx: usize, imp: &ImportDecl) -> usize {
    resolve_target(bundle, module_idx, imp).unwrap_or_else(|| {
        unreachable!(
            "import target missing after sema: module={} alias={} span={:?}",
            module_idx,
            imp.import_alias(),
            imp.span
        )
    })
}

fn required_c_target(
    bundle: &ProgramBundle,
    module_idx: usize,
    scope: Option<&str>,
    alias: &str,
) -> usize {
    bundle
        .cffi
        .target_for_scope(module_idx, scope, alias)
        .unwrap_or_else(|| {
            unreachable!(
                "C import target missing after sema: module={} scope={scope:?} alias={alias}",
                module_idx
            )
        })
}

fn foreign_target(
    bundle: &ProgramBundle,
    module_idx: usize,
    namespace: crate::AST::ForeignNamespace,
    alias: String,
    scope: Option<&str>,
) -> Option<(String, usize)> {
    let target = if namespace.language == crate::AST::ForeignLanguage::C {
        bundle.cffi.target_for_scope(module_idx, scope, &alias)
    } else {
        bundle
            .modules
            .iter()
            .position(|module| module.display == namespace.display())
    }?;
    Some((alias, target))
}

/// Resolve the targets named by any foreign import. The parser and AST keep
/// member lists in `ImportKind::Unqualified`; single-library imports remain
/// `Module` imports and share this resolver at inline scope.
pub(crate) fn foreign_import_targets(
    bundle: &ProgramBundle,
    module_idx: usize,
    imp: &ImportDecl,
) -> Vec<(String, usize)> {
    foreign_import_targets_in_scope(bundle, module_idx, imp, None)
}

pub(crate) fn foreign_import_targets_in_scope(
    bundle: &ProgramBundle,
    module_idx: usize,
    imp: &ImportDecl,
    scope: Option<&str>,
) -> Vec<(String, usize)> {
    foreign_imports_after_frontend(imp)
        .into_iter()
        .map(|(namespace, alias)| {
            foreign_target(bundle, module_idx, namespace, alias, scope).unwrap_or_else(|| {
                unreachable!(
                    "foreign import target missing after sema: module={} scope={scope:?}",
                    module_idx
                )
            })
        })
        .collect()
}

/// Resolve only a foreign member-list import. This keeps the list branch
/// explicit for file-wide maps that still handle the single `Module` form in
/// their existing path.
pub(crate) fn foreign_list_targets(
    bundle: &ProgramBundle,
    module_idx: usize,
    imp: &ImportDecl,
) -> Vec<(String, usize)> {
    foreign_list_targets_in_scope(bundle, module_idx, imp, None)
}

pub(crate) fn foreign_list_targets_in_scope(
    bundle: &ProgramBundle,
    module_idx: usize,
    imp: &ImportDecl,
    scope: Option<&str>,
) -> Vec<(String, usize)> {
    matches!(imp.kind, ImportKind::Unqualified { .. })
        .then(|| foreign_import_targets_in_scope(bundle, module_idx, imp, scope))
        .unwrap_or_default()
}

fn file_import_target(
    bundle: &ProgramBundle,
    module_idx: usize,
    alias: &str,
) -> Option<usize> {
    bundle.modules[module_idx]
        .imports
        .iter()
        .filter(|imp| !matches!(imp.kind, ImportKind::Unqualified { .. }))
        .filter(|imp| imp.import_alias() == alias)
        .find_map(|imp| resolve_target(bundle, module_idx, imp))
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
/// `cx.type_names` and re-run the cloneability/hashability checks for any local
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
                if !cx.hashable.contains(&s.name) && type_is_hashable_struct(s, &cx.hashable) {
                    cx.hashable.insert(s.name.clone());
                }
            }
            Item::Enum(e) => {
                if !cx.cloneable.contains(&e.name) && type_is_cloneable_enum(e, &cx.type_names) {
                    cx.cloneable.insert(e.name.clone());
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
/// type references (e.g. `Note` → `__jet_note::__jet_Note`).
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
    for (scope, imp) in all_imports(module) {
        for (_, target) in foreign_list_targets_in_scope(bundle, module_idx, imp, scope) {
            let rust_mod = mangle(&bundle.modules[target].alias);
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
        if is_foreign_member_list(imp) {
            continue;
        }
        if is_c_import_after_frontend(imp) {
            continue;
        }
        if matches!(imp.kind, ImportKind::Unqualified { .. })
            || imp.core_module_path().is_some()
        {
            continue;
        }
        let target = required_import_target(bundle, module_idx, imp);
        let rust_mod = mangle(&bundle.modules[target].alias);
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
    map
}

/// Populate `cx.variant_owner` and `cx.enum_variants` with pub enum variants
/// from imported file-modules, so cross-module pattern matching works in codegen.
fn register_foreign_comparable_layout(cx: &mut Cx, bundle: &ProgramBundle, target: usize) {
    for item in &bundle.modules[target].items {
        let Item::Struct(structure) = item else {
            continue;
        };
        if structure.is_pub {
            cx.struct_fields
                .entry(structure.name.clone())
                .or_insert_with(|| {
                    structure
                        .fields
                        .iter()
                        .map(|field| (field.name.clone(), field.ty.clone()))
                        .collect()
                });
        }
    }
}

pub(crate) fn register_foreign_enum_variants(
    cx: &mut Cx,
    bundle: &ProgramBundle,
    module_idx: usize,
) {
    let module = &bundle.modules[module_idx];
    for (scope, imp) in all_imports(module) {
        for (_, target) in foreign_list_targets_in_scope(bundle, module_idx, imp, scope) {
            register_foreign_comparable_layout(cx, bundle, target);
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
        if is_foreign_member_list(imp) {
            continue;
        }
        if is_c_import_after_frontend(imp) {
            continue;
        }
        if matches!(imp.kind, ImportKind::Unqualified { .. })
            || imp.core_module_path().is_some()
        {
            continue;
        }
        let target = required_import_target(bundle, module_idx, imp);
        register_foreign_comparable_layout(cx, bundle, target);
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

pub(crate) fn import_mod_map(bundle: &ProgramBundle, module_idx: usize) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let module = &bundle.modules[module_idx];
    for imp in &module.imports {
        for (alias, target) in foreign_list_targets(bundle, module_idx, imp) {
            let stem = &bundle.modules[target].alias;
            map.insert(alias, mangle(stem));
        }
        if is_foreign_member_list(imp) {
            continue;
        }
        let alias = imp.import_alias();
        // S59: C `use` forms target a synthetic merged module by alias.
        if is_c_import_after_frontend(imp) {
            let target = required_c_target(bundle, module_idx, None, &alias);
            let stem = &bundle.modules[target].alias;
            map.insert(alias, mangle(stem));
            continue;
        }
        if matches!(imp.kind, ImportKind::Unqualified { .. })
            || imp.core_module_path().is_some()
        {
            continue;
        }
        let target = required_import_target(bundle, module_idx, imp);
        let stem = &bundle.modules[target].alias;
        map.insert(alias, mangle(stem));
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
        if imp.core_module_path().is_some() || is_c_import_after_frontend(imp) {
            continue;
        }
        let alias = imp.import_alias();
        let target_idx = required_import_target(bundle, module_idx, imp);
        let target = &bundle.modules[target_idx];
        for reimp in &target.imports {
            let ImportKind::Unqualified {
                module_alias,
                ..
            } = &reimp.kind
            else {
                continue;
            };
            if !reimp.is_pub {
                continue;
            }
            // Resolve `module_alias` within the target module's own imports.
            if is_foreign_member_list(reimp)
                || crate::AST::core_list_prefix(module_alias).is_some()
            {
                continue;
            }
            let real_idx = file_import_target(bundle, target_idx, module_alias)
                .unwrap_or_else(|| {
                    unreachable!(
                        "re-export target missing after sema: module={} alias={module_alias}",
                        target_idx
                    )
                });
            let real_mod = mangle(&bundle.modules[real_idx].alias);
            for binding in reimp.walk_bindings() {
                let orig = binding
                    .original
                    .expect("member walker returned a binding without a member");
                let local = binding.local;
                map.insert(
                    (alias.clone(), local),
                    (real_mod.clone(), orig.to_string()),
                );
            }
        }
    }
    // D-NAME-WALK1=A: an inline module can re-export a file-module item from
    // the enclosing file. It lowers through the same qualified map as a
    // top-level file-module re-export; inline-to-inline re-exports use the
    // separate InlineMangled map below.
    for item in &module.items {
        let Item::CodeModule(cm) = item else {
            continue;
        };
        for reimp in &cm.imports {
            let ImportKind::Unqualified {
                module_alias,
                ..
            } = &reimp.kind
            else {
                continue;
            };
            if !reimp.is_pub {
                continue;
            }
            if is_foreign_member_list(reimp)
                || crate::AST::core_list_prefix(module_alias).is_some()
            {
                continue;
            }
            let real_idx = file_import_target(bundle, module_idx, module_alias)
                .unwrap_or_else(|| {
                    unreachable!(
                        "inline re-export target missing after sema: module={} alias={module_alias}",
                        module_idx
                    )
                });
            let real_mod = mangle(&bundle.modules[real_idx].alias);
            for binding in reimp.walk_bindings() {
                let orig = binding
                    .original
                    .expect("member walker returned a binding without a member");
                let local = binding.local;
                map.insert(
                    (cm.name.clone(), local),
                    (real_mod.clone(), orig.to_string()),
                );
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
        // D-MOD3: `use core.item` / `use core.[a, b]` — bind each item name to its
        // full std path so that `item.method(...)` resolves the same way as
        // `use core.item as item` would.
        if let ImportKind::Unqualified {
            module_alias,
            ..
        } = &imp.kind
        {
            if crate::AST::core_list_prefix(module_alias).is_some() {
                for binding in imp.walk_bindings() {
                    let orig = binding
                        .original
                        .expect("member walker returned a binding without a member");
                    let local = binding.local;
                    match crate::AST::core_list_path(module_alias, orig) {
                        Some(crate::AST::CoreListPath::Module(module)) => {
                            map.insert(local.to_string(), module);
                        }
                        Some(crate::AST::CoreListPath::Item { module, .. }) => {
                            // A bare imported Core item lowers as
                            // `module.item`; keep the module in the ordinary
                            // core-import map for every backend.
                            map.insert(local.to_string(), module);
                        }
                        None => unreachable!(
                            "invalid Core member import reached codegen after sema: {module_alias}.{orig}"
                        ),
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
            ..
        } = &imp.kind
        else {
            continue;
        };
        if is_foreign_member_list(imp) {
            // Foreign member lists are mounted namespaces, not ordinary
            // unqualified functions; `foreign_list_targets` handles them.
            continue;
        }
        if crate::AST::core_list_prefix(module_alias).is_some() {
            // Std namespace — handled separately by core_import_map.
            continue;
        }
        if code_mod_aliases.contains(module_alias.as_str()) {
            // Inline code module.
            for binding in imp.walk_bindings() {
                let orig = binding
                    .original
                    .expect("member walker returned a binding without a member");
                let local = binding.local;
                let key = format!("{}__{}", module_alias, orig);
                inline_map.insert(local, key);
            }
        } else {
            let target = file_import_target(bundle, module_idx, module_alias)
                .unwrap_or_else(|| {
                    unreachable!(
                        "unqualified import target missing after sema: module={} alias={module_alias}",
                        module_idx
                    )
                });
            // File module: resolve the file-import whose alias matches, then point
            // each unqualified item at that Rust module.
            let rust_mod = mangle(&bundle.modules[target].alias);
            for binding in imp.walk_bindings() {
                let orig = binding
                    .original
                    .expect("member walker returned a binding without a member");
                let local = binding.local;
                file_map.insert(local, (rust_mod.clone(), orig.to_string()));
            }
        }
    }
    (inline_map, file_map)
}

/// D-NAME-WALK1=A: build the import scopes for functions inside inline
/// modules. The enclosing file's unqualified maps remain in `Cx`; these maps
/// contain only body-local bindings and are selected by the emitted mangled
/// function name.
pub(crate) fn inline_import_maps(
    bundle: &ProgramBundle,
    module_idx: usize,
) -> (
    HashMap<String, HashMap<String, String>>,
    HashMap<String, HashMap<String, (String, String)>>,
    HashSet<String>,
    HashMap<(String, String), String>,
) {
    let module = &bundle.modules[module_idx];
    let code_module_names: std::collections::HashSet<String> = module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::CodeModule(cm) if cm.body.is_some() => Some(cm.name.clone()),
            _ => None,
        })
        .collect();
    let mut inline_scopes = HashMap::new();
    let mut file_scopes = HashMap::new();
    let mut names = std::collections::HashSet::new();
    let mut inline_reexports = HashMap::new();
    for item in &module.items {
        let Item::CodeModule(cm) = item else {
            continue;
        };
        let Some(body) = &cm.body else {
            continue;
        };
        let mut inline_scope = HashMap::new();
        let mut file_scope = HashMap::new();
        for imp in &cm.imports {
            let ImportKind::Unqualified {
                module_alias,
                ..
            } = &imp.kind
            else {
                continue;
            };
            if is_foreign_member_list(imp) {
                continue;
            }
            if code_module_names.contains(module_alias) {
                for binding in imp.walk_bindings() {
                    let orig = binding
                        .original
                        .expect("member walker returned a binding without a member");
                    let local = binding.local;
                    inline_scope.insert(local.clone(), format!("{module_alias}__{orig}"));
                    names.insert(local.clone());
                    if imp.is_pub {
                        inline_reexports.insert(
                            (cm.name.clone(), local),
                            format!("{module_alias}__{orig}"),
                        );
                    }
                }
            } else if crate::AST::core_list_prefix(module_alias).is_none() {
                let target = file_import_target(bundle, module_idx, module_alias);
                let target = target.unwrap_or_else(|| {
                    unreachable!(
                        "inline unqualified import target missing after sema: module={} alias={module_alias}",
                        module_idx
                    )
                });
                let rust_mod = mangle(&bundle.modules[target].alias);
                for binding in imp.walk_bindings() {
                    let orig = binding
                        .original
                        .expect("member walker returned a binding without a member");
                    let local = binding.local;
                    file_scope.insert(local.clone(), (rust_mod.clone(), orig.to_string()));
                    names.insert(local);
                }
            }
        }
        for inner in body {
            let Item::Func(function) = inner else {
                continue;
            };
            let key = format!("{}__{}", cm.name, function.name);
            if !inline_scope.is_empty() {
                inline_scopes.insert(key.clone(), inline_scope.clone());
            }
            if !file_scope.is_empty() {
                file_scopes.insert(key, file_scope.clone());
            }
        }
    }
    (inline_scopes, file_scopes, names, inline_reexports)
}

/// D-NAME-WALK1=A / D-VERDICT-1867-1: foreign namespace aliases imported
/// inside an inline module body. They use the same foreign member-list
/// resolver as file-level imports, but remain scoped to each emitted function.
pub(crate) fn inline_foreign_import_maps(
    bundle: &ProgramBundle,
    module_idx: usize,
) -> HashMap<String, HashMap<String, String>> {
    let module = &bundle.modules[module_idx];
    let mut scopes = HashMap::new();
    for item in &module.items {
        let Item::CodeModule(cm) = item else {
            continue;
        };
        let Some(body) = &cm.body else {
            continue;
        };
        let mut scope = HashMap::new();
        for imp in &cm.imports {
            for (alias, target) in
                foreign_import_targets_in_scope(bundle, module_idx, imp, Some(&cm.name))
            {
                scope.insert(alias, mangle(&bundle.modules[target].alias));
            }
        }
        if scope.is_empty() {
            continue;
        }
        for inner in body {
            let Item::Func(function) = inner else {
                continue;
            };
            scopes.insert(format!("{}__{}", cm.name, function.name), scope.clone());
        }
    }
    scopes
}

/// D-NAME-WALK1=A / D-VERDICT-1867-1: build the foreign call facts for each
/// emitted inline function. Signature and return facts are deliberately not
/// merged into the file-wide `(alias, method)` maps: sibling inline modules
/// may reuse an alias without sharing its target library.
pub(crate) fn inline_foreign_import_signature_maps(
    bundle: &ProgramBundle,
    module_idx: usize,
) -> (
    HashMap<String, HashMap<(String, String), Vec<(AccessConvention, Type)>>>,
    HashMap<String, HashMap<(String, String), Option<Type>>>,
) {
    let module = &bundle.modules[module_idx];
    let mut signatures = HashMap::new();
    let mut returns = HashMap::new();
    for item in &module.items {
        let Item::CodeModule(cm) = item else {
            continue;
        };
        let Some(body) = &cm.body else {
            continue;
        };
        let mut sig_scope = HashMap::new();
        let mut ret_scope = HashMap::new();
        for imp in &cm.imports {
            for (alias, target) in
                foreign_import_targets_in_scope(bundle, module_idx, imp, Some(&cm.name))
            {
                for item in &bundle.modules[target].items {
                    match item {
                        Item::Func(function) if function.is_pub => {
                            sig_scope.insert(
                                (alias.clone(), function.name.clone()),
                                function
                                    .params
                                    .iter()
                                    .map(|param| {
                                        (
                                            param.convention,
                                            qualify_unit_type(bundle, target, &param.ty),
                                        )
                                    })
                                    .collect(),
                            );
                            ret_scope.insert(
                                (alias.clone(), function.name.clone()),
                                function
                                    .return_type
                                    .as_ref()
                                    .map(|ty| qualify_unit_type(bundle, target, ty)),
                            );
                        }
                        Item::CModule(c_module) => {
                            for function in &c_module.functions {
                                sig_scope.insert(
                                    (alias.clone(), function.name.clone()),
                                    function
                                        .params
                                        .iter()
                                        .map(|param| (param.convention, param.ty.clone()))
                                        .collect(),
                                );
                                ret_scope.insert(
                                    (alias.clone(), function.name.clone()),
                                    function.return_type.clone(),
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if sig_scope.is_empty() && ret_scope.is_empty() {
            continue;
        }
        for inner in body {
            let Item::Func(function) = inner else {
                continue;
            };
            let key = format!("{}__{}", cm.name, function.name);
            signatures.insert(key.clone(), sig_scope.clone());
            returns.insert(key, ret_scope.clone());
        }
    }
    (signatures, returns)
}

/// D-VERDICT-1867-1: an inline module may publicly re-export a foreign
/// namespace. The exported namespace is still just the same mounted Rust
/// module; callers reach it as `inline_alias.exported_alias.method(...)`.
pub(crate) fn inline_foreign_reexport_maps(
    bundle: &ProgramBundle,
    module_idx: usize,
) -> HashMap<(String, String), String> {
    let module = &bundle.modules[module_idx];
    let mut map = HashMap::new();
    for item in &module.items {
        let Item::CodeModule(cm) = item else {
            continue;
        };
        if cm.body.is_none() {
            continue;
        }
        for imp in &cm.imports {
            if !imp.is_pub {
                continue;
            }
            for (alias, target) in
                foreign_import_targets_in_scope(bundle, module_idx, imp, Some(&cm.name))
            {
                map.insert(
                    (cm.name.clone(), alias),
                    mangle(&bundle.modules[target].alias),
                );
            }
        }
    }
    map
}

/// Build signature and return facts for calls through an inline module's
/// public foreign namespace re-export. These calls occur in the enclosing
/// module, so their facts are keyed by the exporting inline module rather than
/// by the caller's emitted function.
pub(crate) fn inline_foreign_reexport_signature_maps(
    bundle: &ProgramBundle,
    module_idx: usize,
) -> (
    HashMap<(String, String, String), Vec<(AccessConvention, Type)>>,
    HashMap<(String, String, String), Option<Type>>,
) {
    let module = &bundle.modules[module_idx];
    let mut signatures = HashMap::new();
    let mut returns = HashMap::new();
    for item in &module.items {
        let Item::CodeModule(cm) = item else {
            continue;
        };
        if cm.body.is_none() {
            continue;
        }
        for imp in &cm.imports {
            if !imp.is_pub {
                continue;
            }
            for (alias, target) in
                foreign_import_targets_in_scope(bundle, module_idx, imp, Some(&cm.name))
            {
                for item in &bundle.modules[target].items {
                    match item {
                        Item::Func(function) if function.is_pub => {
                            signatures.insert(
                                (cm.name.clone(), alias.clone(), function.name.clone()),
                                function
                                    .params
                                    .iter()
                                    .map(|param| {
                                        (
                                            param.convention,
                                            qualify_unit_type(bundle, target, &param.ty),
                                        )
                                    })
                                    .collect(),
                            );
                            returns.insert(
                                (cm.name.clone(), alias.clone(), function.name.clone()),
                                function
                                    .return_type
                                    .as_ref()
                                    .map(|ty| qualify_unit_type(bundle, target, ty)),
                            );
                        }
                        Item::CModule(c_module) => {
                            for function in &c_module.functions {
                                signatures.insert(
                                    (cm.name.clone(), alias.clone(), function.name.clone()),
                                    function
                                        .params
                                        .iter()
                                        .map(|param| (param.convention, param.ty.clone()))
                                        .collect(),
                                );
                                returns.insert(
                                    (cm.name.clone(), alias.clone(), function.name.clone()),
                                    function.return_type.clone(),
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    (signatures, returns)
}

/// D-NAME-WALK1=A: build the Core portion of every inline-module import
/// scope, plus Core items re-exported by that inline module. Core aliases are
/// keyed by the emitted `module__function` name so a body-local import cannot
/// leak into a sibling or enclosing function.
pub(crate) fn inline_core_import_maps(
    bundle: &ProgramBundle,
    module_idx: usize,
) -> (
    HashMap<String, HashMap<String, String>>,
    HashMap<(String, String), (String, String)>,
) {
    let module = &bundle.modules[module_idx];
    let mut scopes = HashMap::new();
    let mut reexports = HashMap::new();
    for item in &module.items {
        let Item::CodeModule(cm) = item else {
            continue;
        };
        let Some(body) = &cm.body else {
            continue;
        };
        let mut scope = HashMap::new();
        for imp in &cm.imports {
            if let Some(core_module) = imp.core_module_path() {
                scope.insert(imp.import_alias(), core_module);
                continue;
            }
            let ImportKind::Unqualified {
                module_alias,
                ..
            } = &imp.kind
            else {
                continue;
            };
            if crate::AST::core_list_prefix(module_alias).is_none() {
                continue;
            }
            for binding in imp.walk_bindings() {
                let orig = binding
                    .original
                    .expect("member walker returned a binding without a member");
                let local = binding.local;
                let path = crate::AST::core_list_path(module_alias, orig).unwrap_or_else(|| {
                    unreachable!(
                        "invalid Core member import reached codegen after sema: {module_alias}.{orig}"
                    )
                });
                match path {
                    crate::AST::CoreListPath::Module(core_module) => {
                        scope.insert(local, core_module);
                    }
                    crate::AST::CoreListPath::Item { module, item } => {
                        scope.insert(local.clone(), module.clone());
                        if imp.is_pub {
                            reexports.insert((cm.name.clone(), local), (module, item));
                        }
                    }
                }
            }
        }
        if scope.is_empty() {
            continue;
        }
        for inner in body {
            let Item::Func(function) = inner else {
                continue;
            };
            scopes.insert(
                format!("{}__{}", cm.name, function.name),
                scope.clone(),
            );
        }
    }
    (scopes, reexports)
}

/// Build signature/return entries for selective imports whose target is a file
/// module. The local key is the spelling used in the body; the second key is
/// the target's declared function name. This is needed for both top-level and
/// inline-module `use mod.[item]` calls.
fn unqualified_file_function_entries(
    bundle: &ProgramBundle,
    module_idx: usize,
    imports: &[ImportDecl],
) -> Vec<(
    String,
    String,
    Vec<(AccessConvention, Type)>,
    Option<Type>,
)> {
    let mut entries = Vec::new();
    for imp in imports {
        let ImportKind::Unqualified {
            module_alias,
            ..
        } = &imp.kind
        else {
            continue;
        };
        let Some(target) = file_import_target(bundle, module_idx, module_alias) else {
            if is_foreign_member_list(imp)
                || crate::AST::core_list_prefix(module_alias).is_some()
            {
                continue;
            }
            unreachable!(
                "file import target missing after sema: module={module_idx} alias={module_alias}"
            );
        };
        for binding in imp.walk_bindings() {
            let orig = binding
                .original
                .expect("member walker returned a binding without a member");
            let local = binding.local;
            let Some(item) = bundle.modules[target].items.iter().find(|item| match item {
                Item::Func(f) => f.name == orig && f.is_pub,
                _ => false,
            }) else {
                if let Some(Item::CModule(cm)) = bundle.modules[target].items.iter().find(|item| {
                    matches!(item, Item::CModule(_))
                }) {
                    if let Some(function) = cm.functions.iter().find(|function| function.name == orig) {
                        entries.push((
                            local,
                            orig.to_string(),
                            function
                                .params
                                .iter()
                                .map(|param| (param.convention, param.ty.clone()))
                                .collect(),
                            function.return_type.clone(),
                        ));
                        continue;
                    }
                }
                unreachable!(
                    "imported member missing after sema: module={module_idx} alias={module_alias} member={orig}"
                );
            };
            let Item::Func(function) = item else {
                unreachable!(
                    "imported member is not a function after sema: module={module_idx} alias={module_alias} member={orig}"
                );
            };
            entries.push((
                local,
                orig.to_string(),
                function
                    .params
                    .iter()
                    .map(|param| {
                        (
                            param.convention,
                            qualify_unit_type(bundle, target, &param.ty),
                        )
                    })
                    .collect(),
                function
                    .return_type
                    .as_ref()
                    .map(|ty| qualify_unit_type(bundle, target, ty)),
            ));
        }
    }
    entries
}

pub(crate) fn import_sig_map(
    bundle: &ProgramBundle,
    module_idx: usize,
) -> HashMap<(String, String), Vec<(AccessConvention, Type)>> {
    let mut map = HashMap::new();
    let module = &bundle.modules[module_idx];
    for imp in &module.imports {
        for (alias, target) in foreign_list_targets(bundle, module_idx, imp) {
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
        if is_foreign_member_list(imp) {
            continue;
        }
        let alias = imp.import_alias();
        // S59: C `use` forms — pull boundary fn sigs from the synthetic module.
        let target = if is_c_import_after_frontend(imp) {
            required_c_target(bundle, module_idx, None, &alias)
        } else if matches!(imp.kind, ImportKind::Unqualified { .. })
            || imp.core_module_path().is_some()
        {
            continue;
        } else {
            required_import_target(bundle, module_idx, imp)
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
    for (local, original, params, _) in
        unqualified_file_function_entries(bundle, module_idx, &module.imports)
    {
        map.insert((local, original), params);
    }
    for item in &module.items {
        if let Item::CodeModule(cm) = item {
            for (local, original, params, _) in
                unqualified_file_function_entries(bundle, module_idx, &cm.imports)
            {
                map.insert((local, original), params);
            }
        }
    }
    // D-MOD4: re-exported items (`pub use sub.Item`) must carry the *real*
    // function's parameter conventions under the re-exporting alias, or calls
    // through the re-export would pass by value where a borrow is expected.
    for ((alias, item), (real_mod, real_fn)) in reexport_call_map(bundle, module_idx) {
        let stem = crate::Syntax::generated_suffix(&real_mod);
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
        for (alias, target) in foreign_list_targets(bundle, module_idx, imp) {
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
        if is_foreign_member_list(imp) {
            continue;
        }
        let alias = imp.import_alias();
        let target = if is_c_import_after_frontend(imp) {
            required_c_target(bundle, module_idx, None, &alias)
        } else if matches!(imp.kind, ImportKind::Unqualified { .. })
            || imp.core_module_path().is_some()
        {
            continue;
        } else {
            required_import_target(bundle, module_idx, imp)
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
    for (local, original, _, ret) in
        unqualified_file_function_entries(bundle, module_idx, &module.imports)
    {
        map.insert((local, original), ret);
    }
    for item in &module.items {
        if let Item::CodeModule(cm) = item {
            for (local, original, _, ret) in
                unqualified_file_function_entries(bundle, module_idx, &cm.imports)
            {
                map.insert((local, original), ret);
            }
        }
    }
    for ((alias, item), (real_mod, real_fn)) in reexport_call_map(bundle, module_idx) {
        let stem = crate::Syntax::generated_suffix(&real_mod);
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

pub(crate) fn emit_program_items(
    cx: &Cx,
    items: &[Item],
    out: &mut String,
    include_main: bool,
    include_runtime_owned_traits: bool,
) {
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
        out.push_str("use super::{__jet_Encode, __jet_Decode, jet_std};\n\n");
    }
    let tuple_shapes = collect_tuple_shapes(items);
    emit_tuple_structs(cx, &tuple_shapes, out);
    emit_anonymous_unions(cx, items, out);
    emit_synthetic_display_trait(out, include_runtime_owned_traits);
    emit_synthetic_operator_traits(out, include_runtime_owned_traits);
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
            | Item::FactDecl(_)
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
