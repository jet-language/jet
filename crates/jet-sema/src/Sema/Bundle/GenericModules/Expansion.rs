use super::*;

pub(crate) fn expand_generic_module_aliases(
    bundle: &mut ProgramBundle,
    diags: &mut Vec<Diagnostic>,
) {
    let build_facts = bundle.build_facts.clone();
    let template_snapshots: Vec<HashMap<String, TemplateInfo>> = bundle
        .modules
        .iter()
        .enumerate()
        .map(|(source_module, module)| {
            let enums: HashMap<String, bool> = module
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Enum(def) => Some((
                        def.name.clone(),
                        def.variants
                            .iter()
                            .all(|variant| matches!(variant.payload, VariantPayload::Unit)),
                    )),
                    _ => None,
                })
                .collect();
            let funcs: HashMap<String, &Func> = module
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Func(def) => Some((def.name.clone(), def)),
                    _ => None,
                })
                .collect();
            let mut source_values = HashMap::new();
            if module
                .items
                .iter()
                .any(|item| matches!(item, Item::GenericModule(_)))
            {
                for item in &module.items {
                    if let Item::Const(def) = item {
                        if let Ok(value) = crate::Comptime::evaluate_closed_value(
                            &def.value,
                            &funcs,
                            &HashSet::new(),
                            Path::new("."),
                            &source_values,
                            &build_facts,
                        ) {
                            source_values.insert(def.name.clone(), value);
                        }
                    }
                }
            }
            let source_items = clone_definition_items(&module.items);
            module
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::GenericModule(gm) => {
                        let (package_root, dependency_name) = owning_package(bundle, &module.path);
                        let package_identity =
                            package_identity(bundle, package_root, dependency_name);
                        let module_path = module
                            .path
                            .strip_prefix(package_root)
                            .unwrap_or(&module.path)
                            .to_string_lossy()
                            .replace('\\', "/");
                        let full_key =
                            definition_full_key(&package_identity, &module_path, "", &gm.name);
                        Some((
                            gm.name.clone(),
                            TemplateInfo {
                                def: gm.clone(),
                                definition_id: crate::SHA256::sha256_hex(&full_key),
                                definition_full_key: full_key,
                                params: resolve_params(gm, &enums, diags),
                                source_module,
                                source_items: clone_definition_items(&source_items),
                                source_values: source_values.clone(),
                                source_rule_facts: module.rule_facts.clone(),
                                build_facts: build_facts.clone(),
                            },
                        ))
                    }
                    _ => None,
                })
                .collect()
        })
        .collect();
    // `use alias.Item` has its own span and therefore no direct import-target
    // entry. Resolve it through the namespace import which established
    // `alias`, exactly like the later ordinary-import registration pass.
    let import_bindings: Vec<HashMap<String, usize>> = bundle
        .modules
        .iter()
        .enumerate()
        .map(|(module_idx, module)| {
            module
                .imports
                .iter()
                .filter(|import| !matches!(import.kind, ImportKind::Unqualified { .. }))
                .filter_map(|import| {
                    bundle
                        .name_ledger
                        .import_target(module_idx, import.span)
                        .map(|target| (import.import_alias(), target))
                })
                .collect()
        })
        .collect();

    let mut bundle_instances: HashMap<ModuleInstanceKey, String> = HashMap::new();
    let mut bundle_instance_nominals: HashMap<String, Vec<String>> = HashMap::new();
    let mut fingerprint_keys: HashMap<String, Vec<u8>> = HashMap::new();
    let mut instance_applications: HashMap<String, Vec<crate::AST::ModuleInstanceApplication>> =
        HashMap::new();

    // Snapshot aliases up front — the mut loop below can't re-borrow `bundle.modules`
    // for an E0609 message that names the source module.
    let module_aliases: Vec<String> = bundle.modules.iter().map(|m| m.alias.clone()).collect();

    for (module_idx, module) in bundle.modules.iter_mut().enumerate() {
        let mut generic_module_spans = Vec::new();
        collect_generic_module_spans(&module.items, &mut generic_module_spans);
        if report_generic_module_cycles(&module.items, diags) {
            continue;
        }
        // Same disposable-registry rationale as the `template_snapshots` prepass
        // above: only used for bound resolution, never the diagnostic source of
        // truth — register the builtin hook traits and swallow its diags.
        let mut traits = TraitRegistry::default();
        traits.register_synthetic_rollback();
        traits.register_synthetic_display_debug();
        traits.register_synthetic_close();
        traits.register_synthetic_operators();
        traits.register_synthetic_iter_index();
        traits.register_synthetic_io();
        traits.register_synthetic_driver();
        traits.register_items(&module.items, &mut Vec::new());
        let enums: HashMap<String, bool> = module
            .items
            .iter()
            .filter_map(|item| {
                if let Item::Enum(def) = item {
                    Some((
                        def.name.clone(),
                        def.variants
                            .iter()
                            .all(|v| matches!(v.payload, VariantPayload::Unit)),
                    ))
                } else {
                    None
                }
            })
            .collect();
        let funcs: HashMap<String, &Func> = module
            .items
            .iter()
            .filter_map(|item| {
                if let Item::Func(f) = item {
                    Some((f.name.clone(), f))
                } else {
                    None
                }
            })
            .collect();
        let mut globals: HashMap<String, crate::AST::CtValue> = HashMap::new();
        if module
            .items
            .iter()
            .any(|item| matches!(item, Item::ModuleAlias(_)))
        {
            for item in &module.items {
                if let Item::Const(c) = item {
                    if let Ok(value) = crate::Comptime::evaluate_closed_value(
                        &c.value,
                        &funcs,
                        &HashSet::new(),
                        Path::new("."),
                        &globals,
                        &build_facts,
                    ) {
                        globals.insert(c.name.clone(), value);
                    }
                }
            }
        }
        // Parameter declarations were resolved once in the immutable prepass.
        // Reuse that result locally so invalid declarations emit one diagnostic.
        let mut templates = template_snapshots[module_idx].clone();
        let mut denied_templates = HashSet::new();

        for import in &mut module.imports {
            let ImportKind::Unqualified { module_alias, .. } = &import.kind else {
                continue;
            };
            let Some(source_idx) = import_bindings[module_idx]
                .get(module_alias.as_str())
                .copied()
            else {
                continue;
            };
            let bindings = import.walk_bindings();
            let mut consumed = HashSet::new();
            for binding in &bindings {
                let original = binding
                    .original
                    .expect("member walker returned a binding without a member");
                let Some(source) = template_snapshots[source_idx].get(original) else {
                    continue;
                };
                consumed.insert(original.to_string());
                let local = binding.local.clone();
                if !source.def.is_pub && !source.def.is_package_pub {
                    denied_templates.insert(local.clone());
                    diags.push(Diagnostic::error(
                        "E0609",
                        format!(
                            "`{original}` is private in module `{}`",
                            module_aliases[source_idx]
                        ),
                        "only `pub` items can be brought into scope with `use`".to_string(),
                        format!("add `pub` before `module {original}` in the defining file"),
                        Some(import.span),
                    ));
                    continue;
                }
                templates.insert(local, source.clone());
            }
            // Generic templates are compile-time namespace inputs, not runtime
            // values for the ordinary unqualified-import pass below.
            if let ImportKind::Unqualified { items, .. } = &mut import.kind {
                items.retain(|(original, _)| !consumed.contains(original));
            }
        }

        let aliases: HashMap<String, &ModuleAliasDef> = module
            .items
            .iter()
            .filter_map(|item| match item {
                Item::ModuleAlias(alias) => Some((alias.name.clone(), alias)),
                _ => None,
            })
            .collect();
        let type_aliases: HashMap<String, Type> = module
            .items
            .iter()
            .filter_map(|item| {
                let Item::TypeAlias(alias) = item else {
                    return None;
                };
                Some((alias.name.clone(), alias.target.clone()))
            })
            .collect();
        let mut projections = HashMap::new();
        for alias in aliases.values().copied() {
            if aliases.contains_key(&alias.target) {
                let mut terminal = aliases[&alias.target];
                while let Some(next) = aliases.get(&terminal.target).copied() {
                    terminal = next;
                }
                projections.insert(alias.name.clone(), terminal.name.clone());
            }
        }

        // Expand aliases into CodeModules, collect separately.
        let mut expansions: Vec<(usize, AliasExpansion)> = Vec::new();
        let mut ordered_aliases: Vec<(usize, &ModuleAliasDef)> = module
            .items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| match item {
                Item::ModuleAlias(alias) => Some((idx, alias)),
                _ => None,
            })
            .collect();
        ordered_aliases.sort_by_key(|(_, alias)| local_alias_depth(alias, &aliases));
        let mut invalid_aliases = HashSet::new();
        for (idx, alias) in ordered_aliases {
            if alias_chain_contains(alias, &aliases, &invalid_aliases) {
                continue;
            }
            let Some(resolved) = resolve_local_alias(alias, &aliases, &templates, diags) else {
                invalid_aliases.insert(alias.name.clone());
                continue;
            };
            if denied_templates.contains(&resolved.target) {
                invalid_aliases.insert(alias.name.clone());
                continue;
            }
            // A valid forward alias is a projection of the already-bound
            // terminal instance, not a second specialization.
            if aliases.contains_key(&alias.target) {
                continue;
            }
            let Some(info) = templates.get(&resolved.target) else {
                // The alias names a template that does not exist. This guard
                // runs before `expand_alias` (the other E0850 site), so it
                // must report the unknown target itself — otherwise the
                // alias is silently dropped and the program checks clean.
                diags.push(Diagnostic::error(
                    "E0850",
                    format!(
                        "generic module `{}` not found in this scope",
                        resolved.target
                    ),
                    "check the module template name and make sure it is defined in the same file"
                        .to_string(),
                    format!("example: `module {} :: MyTemplate<String>`", resolved.name),
                    Some(resolved.target_span),
                ));
                invalid_aliases.insert(alias.name.clone());
                continue;
            };
            let Some(args) = resolve_args(
                &resolved,
                info,
                &traits,
                &funcs,
                &globals,
                &build_facts,
                &enums,
                diags,
            ) else {
                invalid_aliases.insert(alias.name.clone());
                continue;
            };
            let key = instance_key(info, &args, &type_aliases);
            let fingerprint = crate::SHA256::sha256_hex(&key.bytes());
            instance_applications
                .entry(fingerprint.clone())
                .or_default()
                .push(crate::AST::ModuleInstanceApplication {
                    name: resolved.name.clone(),
                    source_module: module.display.clone(),
                    semantic_identity: format!("instance:{fingerprint}"),
                    span: resolved.name_span,
                });
            if let Some(canonical) = bundle_instances.get(&key) {
                projections.insert(alias.name.clone(), canonical.clone());
                if let Some(nominals) = bundle_instance_nominals.get(canonical).cloned() {
                    bundle_instance_nominals.insert(alias.name.clone(), nominals);
                }
                continue;
            }
            let identity_args = args.clone();
            if let Some(mut cm) = expand_alias(
                &resolved,
                module_idx,
                &module.display,
                &key.bytes(),
                &templates,
                diags,
                &traits,
                &funcs,
                &globals,
                &build_facts,
                &enums,
                &mut bundle_instances,
                &mut fingerprint_keys,
                &mut instance_applications,
                Some(args),
            ) {
                let identity =
                    instance_identity(&key, info, &resolved, &module.display, &identity_args);
                register_instance_fingerprint(&mut fingerprint_keys, &identity, alias.span);
                cm.module.instance_identity = Some(identity);
                bundle_instance_nominals.insert(
                    alias.name.clone(),
                    cm.declarations
                        .iter()
                        .filter_map(|item| match item {
                            Item::Struct(def) => Some(def.name.clone()),
                            Item::Enum(def) => Some(def.name.clone()),
                            _ => None,
                        })
                        .collect(),
                );
                bundle_instances.insert(key, alias.name.clone());
                expansions.push((idx, cm));
            } else {
                invalid_aliases.insert(alias.name.clone());
            }
        }

        // Replace/erase: iterate in reverse to preserve indices.
        // For each alias, replace it with the expanded CodeModule.
        // GenericModule items are erased (replaced with nothing).
        // We need to:
        // 1. Replace each ModuleAlias with its CodeModule expansion (collected above)
        // 2. Remove all GenericModule items
        let mut declarations = Vec::new();
        let mut generated_rule_facts = Vec::new();
        for (idx, expansion) in expansions {
            module.items[idx] = Item::CodeModule(expansion.module);
            declarations.extend(expansion.declarations);
            generated_rule_facts.extend(expansion.rule_facts);
        }
        module.rule_facts.retain(|application| {
            let span = application.marker.span;
            !generic_module_spans
                .iter()
                .any(|generic| span.start >= generic.start && span.end <= generic.end)
        });
        module.rule_facts.extend(generated_rule_facts);
        module
            .rule_facts
            .sort_by_key(|application| application.marker.span.start);
        // Collapse forward-alias chains through the applicative canonical
        // instance selected above.
        for alias in projections.clone().keys() {
            let mut canonical = projections[alias].clone();
            let mut seen = HashSet::new();
            while seen.insert(canonical.clone()) {
                let Some(next) = projections.get(&canonical) else {
                    break;
                };
                canonical = next.clone();
            }
            projections.insert(alias.clone(), canonical);
        }
        // Resolve projected nominal spellings before registration/codegen. No
        // duplicate declaration or zero-parameter surface alias leaks out.
        let mut projection_types = HashMap::new();
        for (alias, nominals) in &bundle_instance_nominals {
            let canonical = projections.get(alias).unwrap_or(alias);
            let prefix = module_type_prefix(canonical);
            for canonical_name in nominals {
                let Some(suffix) = canonical_name.strip_prefix(&prefix) else {
                    continue;
                };
                let resolved = Type::Named(canonical_name.clone());
                // Rewrite the source-facing member path as well as the
                // generated spelling used by specialized declarations.
                projection_types.insert(format!("{alias}.{suffix}"), resolved.clone());
                projection_types.insert(module_type_name(alias, suffix), resolved);
            }
        }
        for (alias, canonical) in &projections {
            let prefix = module_type_prefix(canonical);
            for canonical_name in bundle_instance_nominals
                .get(canonical)
                .into_iter()
                .flatten()
            {
                if let Some(suffix) = canonical_name.strip_prefix(&prefix) {
                    let resolved = Type::Named(canonical_name.clone());
                    projection_types.insert(format!("{alias}.{suffix}"), resolved.clone());
                    projection_types.insert(module_type_name(alias, suffix), resolved);
                }
            }
        }
        for (alias, canonical) in &projections {
            let names = HashSet::from([alias.clone()]);
            for item in &mut module.items {
                if let Item::Func(func) = item {
                    rewrite_inline_calls_stmts(&mut func.body, &names, canonical);
                }
            }
        }
        for item in &mut module.items {
            if let Item::Func(func) = item {
                for param in &mut func.params {
                    param.ty = crate::Generics::substitute_type(&param.ty, &projection_types);
                }
                if let Some(ret) = &mut func.return_type {
                    *ret = crate::Generics::substitute_type(ret, &projection_types);
                }
                substitute_stmts(&mut func.body, &projection_types, &HashMap::new());
            }
        }
        module
            .items
            .retain(|i| !matches!(i, Item::GenericModule(_) | Item::ModuleAlias(_)));
        module.items.extend(declarations);
        debug_assert!(!module
            .items
            .iter()
            .any(|item| matches!(item, Item::ModuleAlias(_))));
    }
    for module in &mut bundle.modules {
        for item in &mut module.items {
            let Item::CodeModule(instance) = item else {
                continue;
            };
            let Some(identity) = &mut instance.instance_identity else {
                continue;
            };
            if let Some(applications) = instance_applications.get(&identity.fingerprint) {
                identity.applications = applications.clone();
            }
        }
    }
}

/// Give an inline module's member TYPES the same lifted member
/// identity its member functions already have.
///
/// Registration and every engine only ever learn TOP-LEVEL type declarations:
/// `Bundle/Pipeline.rs`'s `Item::CodeModule` arm registers member `Item::Func`
/// rows only, and codegen's item loops list `Item::CodeModule` among the items
/// that declare no type. So a `struct` written inside `module bank { … }` was
/// invisible everywhere — E0119 "there's no type called `Account`" inside its
/// own module (card #2054), while a generic module's member struct worked
/// because `expand_alias` renames and lifts it.
///
/// This is that same mechanism, with no arguments to substitute: one member
/// naming scheme (`module_type_name`), one lifting step, one display
/// projection (`plain_inline_module_display_paths`), no engine change. Generic
/// instances are skipped — `expand_alias` already lifted their members.
///
/// Visibility is preserved: the bare name and the qualified `bank.Account`
/// spelling resolve inside the module body, but only a `pub` member's
/// qualified spelling resolves in the rest of the file, so a private member
/// stays unreachable from outside exactly as `pub` promises.
pub(crate) fn hoist_inline_module_member_types(bundle: &mut ProgramBundle) {
    for module in bundle.modules.iter_mut() {
        let mut declarations: Vec<Item> = Vec::new();
        let mut exported: HashMap<String, Type> = HashMap::new();
        for item in module.items.iter_mut() {
            let Item::CodeModule(code_module) = item else {
                continue;
            };
            let source_path = code_module.name.clone();
            hoist_inline_module_types(code_module, &source_path, &mut declarations, &mut exported);
        }
        if declarations.is_empty() {
            continue;
        }
        // The rest of the file reaches a lifted member only through its
        // qualified `module.Type` spelling, and only when it is `pub` — the
        // same consumer rewrite generic-module instances get.
        if !exported.is_empty() {
            rewrite_exported_inline_types(&mut module.items, &exported);
        }
        module.items.extend(declarations);
    }
}

/// Rewrite public inline-module type projections in every function scope in
/// one file. A sibling inline module is outside the declaring module's body,
/// but it still uses the enclosing file's public module surface. Private
/// members never enter `exported`, so this cannot widen their reach.
fn rewrite_exported_inline_types(items: &mut [Item], exported: &HashMap<String, Type>) {
    for item in items {
        match item {
            Item::Func(func) => {
                *func = specialize_function_types(func.clone(), exported);
            }
            Item::CodeModule(module) => {
                if let Some(body) = &mut module.body {
                    rewrite_exported_inline_types(body, exported);
                }
            }
            _ => {}
        }
    }
}

pub(super) fn inline_type_alias(source_path: &str) -> String {
    source_path.replace('.', "_")
}

fn hoist_inline_module_types(
    code_module: &mut CodeModule,
    source_path: &str,
    declarations: &mut Vec<Item>,
    exported: &mut HashMap<String, Type>,
) -> HashMap<String, Type> {
    if code_module.instance_identity.is_some() {
        return HashMap::new();
    }
    let Some(body) = &mut code_module.body else {
        return HashMap::new();
    };

    let module_alias = inline_type_alias(source_path);
    let module_name = code_module.name.clone();
    let mut direct_types = HashMap::new();
    let mut direct_names = Vec::new();
    let display_alias = inline_type_alias(&module_name);
    for inner in body.iter() {
        let (name, is_pub) = match inner {
            Item::Struct(def) => (&def.name, def.is_pub || def.is_package_pub),
            Item::Enum(def) => (&def.name, def.is_pub || def.is_package_pub),
            Item::Trait(def) => (&def.name, def.is_pub || def.is_package_pub),
            Item::Tag(def) => (&def.name, def.is_pub || def.is_package_pub),
            _ => continue,
        };
        let resolved = Type::Named(module_type_name(&module_alias, name));
        direct_types.insert(name.clone(), resolved.clone());
        direct_types.insert(format!("{module_name}.{name}"), resolved.clone());
        direct_types.insert(format!("{source_path}.{name}"), resolved.clone());
        // A file imported under an alias can still carry a compiler-owned
        // nominal spelling from its defining module. Normalize that spelling
        // to the wrapper's canonical identity before body checking.
        direct_types.insert(module_type_name(&display_alias, name), resolved.clone());
        direct_names.push(name.clone());
        if is_pub {
            exported.insert(format!("{source_path}.{name}"), resolved.clone());
            exported.insert(format!("{module_name}.{name}"), resolved);
        }
    }

    let mut kept = Vec::with_capacity(body.len());
    let mut lifted = Vec::new();
    let mut descendant_types = HashMap::new();
    for inner in std::mem::take(body) {
        match inner {
            Item::CodeModule(mut child) => {
                let child_path = format!("{source_path}.{}", child.name);
                descendant_types.extend(hoist_inline_module_types(
                    &mut child,
                    &child_path,
                    declarations,
                    exported,
                ));
                kept.push(Item::CodeModule(child));
            }
            Item::Struct(_) | Item::Enum(_) | Item::Trait(_) | Item::Tag(_) | Item::Impl(_) => {
                lifted.push(inner)
            }
            other => kept.push(other),
        }
    }

    let mut visible_types = direct_types.clone();
    for (full_path, ty) in &descendant_types {
        visible_types.insert(full_path.clone(), ty.clone());
        if let Some(relative) = full_path.strip_prefix(&format!("{source_path}.")) {
            visible_types.insert(relative.to_string(), ty.clone());
        }
    }
    let values: HashMap<String, crate::AST::CtValue> = HashMap::new();
    for inner in &mut kept {
        match inner {
            Item::Func(def) => {
                *def = specialize_func(def.clone(), &[], &[], &visible_types, &values)
            }
            Item::Const(def) => {
                substitute_expr(&mut def.value, &visible_types, &values);
                def.ty = def
                    .ty
                    .as_ref()
                    .map(|ty| specialize_module_type(ty, &visible_types, &values));
            }
            _ => {}
        }
    }
    for inner in lifted {
        match inner {
            Item::Struct(def) => declarations.push(Item::Struct(specialize_struct(
                &def,
                &module_alias,
                &[],
                &[],
                &visible_types,
                &values,
            ))),
            Item::Enum(def) => declarations.push(Item::Enum(specialize_enum(
                &def,
                &module_alias,
                &[],
                &[],
                &visible_types,
                &values,
            ))),
            Item::Trait(def) => declarations.push(Item::Trait(specialize_trait(
                &def,
                &[],
                &[],
                &visible_types,
                &values,
            ))),
            Item::Tag(def) => {
                declarations.push(Item::Tag(specialize_tag(&def, &visible_types, &values)))
            }
            Item::Impl(def) => declarations.push(Item::Impl(specialize_impl(
                &def,
                &[],
                &[],
                &visible_types,
                &values,
            ))),
            _ => unreachable!("only type declarations and impls are lifted"),
        }
    }
    *body = kept;

    let mut all_types = HashMap::new();
    for name in direct_names {
        if let Some(ty) = direct_types.get(&name) {
            all_types.insert(format!("{source_path}.{name}"), ty.clone());
        }
    }
    all_types.extend(descendant_types);
    all_types
}


