use super::*;

pub(super) fn resolve_inline_module_imports(
    bundle: &mut ProgramBundle,
    states: &mut [ModuleState],
    name_ledger: &mut jet_foundation::Names::NameLedger,
    diags: &mut Vec<Diagnostic>,
) {
    // D-NAME-WALK1=A: resolve use/pub use declared inside inline-module
    // bodies. These bindings inherit the enclosing file's module aliases, but
    // are keyed by inline module so they cannot leak into sibling or top-level
    // bodies. Generic module instances are ordinary CodeModules by this pass.
    for idx in 0..bundle.modules.len() {
        let inline_imports: Vec<(String, Vec<crate::AST::ImportDecl>)> = bundle.modules[idx]
            .items
            .iter()
            .filter_map(|item| match item {
                Item::CodeModule(cm) if cm.body.is_some() && !cm.imports.is_empty() => {
                    Some((cm.name.clone(), cm.imports.clone()))
                }
                _ => None,
            })
            .collect();
        for (inline_name, imports) in inline_imports {
            let mut inline_names = HashSet::new();
            for imp in imports {
                // Qualified Core imports use the enclosing file's Core
                // namespace, but their binding remains local to this inline
                // module body.
                if let Some(module) = imp.core_module_path() {
                    if !crate::Syntax::is_known_core_module(&module) {
                        diags.push(crate::Sema::CheckerCoreLib::unknown_core_module(
                            &module,
                            imp.span,
                        ));
                        continue;
                    }
                    if let Some(mod_layer) = crate::Syntax::core_module_layer(&module) {
                        if let Some(ceiling) = bundle.layer_ceiling {
                            if mod_layer > ceiling {
                                diags.push(crate::Syntax::layer_ceiling_exceeded(
                                    &module,
                                    mod_layer,
                                    ceiling,
                                    Some(imp.span),
                                    Some(&format!("`use {module}`")),
                                ));
                                continue;
                            }
                        }
                        if mod_layer > bundle.inferred_layer {
                            bundle.inferred_layer = mod_layer;
                        }
                    }
                    let local = imp
                        .walk_bindings()
                        .into_iter()
                        .next()
                        .map(|binding| binding.local)
                        .unwrap_or_else(|| imp.import_alias());
                    if !inline_names.insert(local.clone()) {
                        diags.push(Diagnostic::error(
                            "E0105",
                            format!("the import name `{local}` is used twice"),
                            "each import needs a unique namespace name in this file".to_string(),
                            format!("rename one with `{} alias`", Syntax::KW_AS),
                            Some(imp.alias_span),
                        ));
                        continue;
                    }
                    states[idx]
                        .inline_core_imports
                        .insert((inline_name.clone(), local), module);
                    continue;
                }
                let foreign = foreign_imports_after_validation(&imp);
                if !foreign.is_empty() {
                    let mut group_failed = false;
                    let mut resolved = Vec::new();
                    let mut group_names = Vec::new();
                    for (namespace, alias) in foreign {
                        if !inline_names.insert(alias.clone()) {
                            group_failed = true;
                            diags.push(Diagnostic::error(
                                "E0105",
                                format!("the import name `{alias}` is used twice"),
                                "each import needs a unique namespace name in this file".to_string(),
                                format!("rename one with `{} alias`", Syntax::KW_AS),
                                Some(imp.alias_span),
                            ));
                            continue;
                        }
                        group_names.push(alias.clone());
                        let target = if namespace.language == crate::AST::ForeignLanguage::C {
                            bundle
                                .cffi
                                .target_for_scope(idx, Some(&inline_name), &alias)
                        } else {
                            bundle
                                .modules
                                .iter()
                                .position(|candidate| candidate.display == namespace.display())
                        };
                        if let Some(target) = target {
                            resolved.push((alias, target));
                        } else {
                            group_failed = true;
                            let root = namespace.language.root();
                            diags.push(Diagnostic::error(
                                "E1002",
                                format!("`{root}` is reserved for first-party or foreign packages"),
                                "a foreign namespace must resolve to a mounted library before it can be imported".to_string(),
                                format!("make the `{root}` binding available or rename the import"),
                                Some(imp.alias_span),
                            ));
                        }
                    }
                    if !group_failed {
                        for (alias, target) in resolved {
                            states[idx]
                                .inline_foreign_imports
                                .insert((inline_name.clone(), alias.clone()), target);
                            if imp.is_pub {
                                states[idx]
                                    .inline_reexport_foreign
                                    .insert((inline_name.clone(), alias), target);
                            }
                        }
                    } else {
                        for alias in group_names {
                            inline_names.remove(&alias);
                        }
                    }
                    continue;
                }
                let ImportKind::Unqualified {
                    module_alias,
                    module_alias_span,
                    ..
                } = &imp.kind
                else {
                    // Inline bodies inherit file/module aliases; a second
                    // module-loading declaration has no loader target.
                    continue;
                };
                let module_alias = module_alias.as_str();
                let module_alias_span = *module_alias_span;
                let bindings = imp.walk_bindings();
                let group_diagnostics = diags.len();
                let mut group_names = Vec::new();
                let mut inserted_inline = Vec::new();
                let mut inserted_file = Vec::new();
                let mut inserted_core = Vec::new();
                let mut inserted_core_items = Vec::new();
                let mut inserted_reexport_inline = Vec::new();
                let mut inserted_reexport_file = Vec::new();
                let mut inserted_reexport_core = Vec::new();
                for binding in bindings {
                    let orig = binding
                        .original
                        .expect("member walker returned a binding without a member");
                    let local = binding.local;
                    if !inline_names.insert(local.clone()) {
                        diags.push(Diagnostic::error(
                            "E0105",
                            format!("the import name `{local}` is used twice"),
                            "each import needs a unique namespace name in this file".to_string(),
                            format!("rename one with `{} alias`", Syntax::KW_AS),
                            Some(imp.alias_span),
                        ));
                        continue;
                    }
                    group_names.push(local.clone());
                    enum Target {
                        Inline { alias: String, mangled: String },
                        File { name: String, module_idx: usize },
                        Core { module: String, item: Option<String> },
                    }
                    let resolved = {
                        let st = &states[idx];
                        if let Some(canonical) = st.code_modules.get(module_alias) {
                            let mangled =
                                jet_foundation::Names::member_name(canonical, &orig);
                            if !st.funcs.contains_key(&mangled) {
                                diags.push(Diagnostic::error(
                                    "E0611",
                                    format!("{orig} is not defined in module {module_alias}"),
                                    "check the module body for the item you are importing".to_string(),
                                    "make sure the name is spelled correctly".to_string(),
                                    Some(module_alias_span),
                                ));
                                None
                            } else {
                                if !name_ledger.exported(idx, &mangled) {
                                    diags.push(Diagnostic::error(
                                        "E0609",
                                        format!("{orig} is private in module {module_alias}"),
                                        "only public items can be brought into scope with use".to_string(),
                                        format!("add pub before fn {orig} in module {module_alias}"),
                                        Some(module_alias_span),
                                    ));
                                    None
                                } else {
                                    Some(Target::Inline {
                                        alias: module_alias.to_string(),
                                        mangled,
                                    })
                                }
                            }
                        } else if let Some(core_prefix) =
                            crate::AST::core_list_prefix(&module_alias)
                        {
                            let full = format!("{core_prefix}.{orig}");
                            let target = match crate::AST::core_list_path(&module_alias, &orig) {
                                Some(crate::AST::CoreListPath::Module(module)) => {
                                    Some((module, None))
                                }
                                Some(crate::AST::CoreListPath::Item { module, item })
                                    if crate::Sema::CheckerCoreLib::core_module_items(&module)
                                        .iter()
                                        .any(|known| known == &item) =>
                                {
                                    Some((module, Some(item)))
                                }
                                _ => None,
                            };
                            match target {
                                Some((module, item)) => Some(Target::Core { module, item }),
                                None => {
                                    if crate::Syntax::is_known_core_module(&core_prefix) {
                                        diags.push(crate::Sema::CheckerCoreLib::unknown_core_item(
                                            &core_prefix,
                                            &orig,
                                            module_alias_span,
                                        ));
                                    } else {
                                        diags.push(crate::Sema::CheckerCoreLib::unknown_core_module(
                                            &full,
                                            module_alias_span,
                                        ));
                                    }
                                    None
                                }
                            }
                        } else if let Some(&target_idx) = st.imports.get(module_alias) {
                            let target = &states[target_idx];
                            let visible = name_ledger.visible(idx, target_idx, &orig);
                            if !target.funcs.contains_key(orig) {
                                diags.push(Diagnostic::error(
                                    "E0611",
                                    format!("{orig} is not defined in module {module_alias}"),
                                    "check the module for the item you are importing".to_string(),
                                    "make sure the name is spelled correctly".to_string(),
                                    Some(module_alias_span),
                                ));
                                None
                            } else if !visible {
                                diags.push(Diagnostic::error(
                                    "E0609",
                                    format!("{orig} is private in module {module_alias}"),
                                    "only public items can be brought into scope with use".to_string(),
                                    format!("add pub before fn {orig} in the imported file"),
                                    Some(module_alias_span),
                                ));
                                None
                            } else {
                                Some(Target::File {
                                    name: orig.to_string(),
                                    module_idx: target_idx,
                                })
                            }
                        } else {
                            diags.push(Diagnostic::error(
                                "E0610",
                                format!("no module named {module_alias} in scope"),
                                "the alias must refer to a module in the enclosing file".to_string(),
                                format!("import a module as {module_alias} before this use"),
                                Some(module_alias_span),
                            ));
                            None
                        }
                    };
                    let Some(target) = resolved else { continue };
                    let st = &mut states[idx];
                    match target {
                        Target::Inline { alias, mangled } => {
                            st.inline_unqualified
                                .insert((inline_name.clone(), local.clone()), mangled.clone());
                            inserted_inline.push((inline_name.clone(), local.clone()));
                            if imp.is_pub {
                                st.inline_reexport_inline.insert(
                                    (inline_name.clone(), local.clone()),
                                    (alias, mangled),
                                );
                                inserted_reexport_inline.push((inline_name.clone(), local));
                            }
                        }
                        Target::File { name, module_idx } => {
                            st.inline_unqualified_file.insert(
                                (inline_name.clone(), local.clone()),
                                (name.clone(), module_idx),
                            );
                            inserted_file.push((inline_name.clone(), local.clone()));
                            if imp.is_pub {
                                st.inline_reexport_file
                                    .insert((inline_name.clone(), local.clone()), (name, module_idx));
                                inserted_reexport_file.push((inline_name.clone(), local));
                            }
                        }
                        Target::Core { module, item } => {
                            let key = (inline_name.clone(), local.clone());
                            st.inline_core_imports
                                .insert(key.clone(), module.clone());
                            inserted_core.push(key.clone());
                            if let Some(item) = item {
                                st.inline_core_items
                                    .insert(key.clone(), item.clone());
                                inserted_core_items.push(key.clone());
                                if imp.is_pub {
                                    st.inline_reexport_core
                                        .insert(key.clone(), (module, item));
                                    inserted_reexport_core.push(key);
                                }
                            }
                        }
                    }
                }
                if diags.len() != group_diagnostics {
                    let st = &mut states[idx];
                    for key in inserted_inline {
                        st.inline_unqualified.remove(&key);
                    }
                    for key in inserted_file {
                        st.inline_unqualified_file.remove(&key);
                    }
                    for key in inserted_core {
                        st.inline_core_imports.remove(&key);
                    }
                    for key in inserted_core_items {
                        st.inline_core_items.remove(&key);
                    }
                    for key in inserted_reexport_inline {
                        st.inline_reexport_inline.remove(&key);
                    }
                    for key in inserted_reexport_file {
                        st.inline_reexport_file.remove(&key);
                    }
                    for key in inserted_reexport_core {
                        st.inline_reexport_core.remove(&key);
                    }
                    for name in group_names {
                        inline_names.remove(&name);
                    }
                }
            }
        }
    }
}
