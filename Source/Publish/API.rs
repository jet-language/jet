// ──────────────────────────────────────────────
// Public API surface extraction
// ──────────────────────────────────────────────

/// An item in a package's public API. Two `ApiItem`s are "compatible" when they
/// have the same `kind`, `name`, and `signature` (a textual canonical form).
/// We store the signature as a string because full AST comparison is brittle;
/// the canonical form gives false-negative safety (we might miss a breaking
/// change in a complex generic; that is acceptable for v1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ApiItem {
    /// "fn", "struct", "enum", "trait", "const"
    pub kind: String,
    pub name: String,
    /// Textual canonical form of the signature (param/field types, return type).
    /// Does not include the body. Whitespace-normalised.
    pub signature: String,
}

/// Extract the public API surface from a parsed Jet source file.
/// Includes `pub` items at file scope and inside inline code modules.
pub fn extract_public_api(src: &str, file: &str) -> Vec<ApiItem> {
    let package = std::path::Path::new(file)
        .parent()
        .and_then(|parent| {
            parent
                .ancestors()
                .find_map(crate::Package::PackageFacts::load)
        })
        .and_then(Result::ok)
        .map(|manifest| manifest.name)
        .unwrap_or_else(|| "package".to_string());
    extract_public_api_for_package(src, file, &package)
}

/// Extract with the canonical package provenance already resolved by the
/// publish caller. API freeze and SemVer must receive this same identity.
pub fn extract_public_api_for_package(src: &str, file: &str, package: &str) -> Vec<ApiItem> {
    use crate::Loader;

    let mut bundle = match Loader::load_entry_with_overlay(file, None, true) {
        Ok(b) => b,
        Err(_) => return vec![],
    };
    let (_, facts) = crate::Sema::check_bundle_with_effect_facts(
        &mut bundle,
        crate::Sema::CompileMode::Check,
    );
    let _ = src; // bundle already loaded

    let mut out = Vec::new();
    // Entry file items (the main module).
    let entry = &bundle.modules[bundle.entry];
    let mut dimensions = crate::Sema::ApiFreeze::ApiUnitDimensions::new();
    crate::Sema::ApiFreeze::collect_api_unit_dimensions(&entry.items, package, &mut dimensions);
    collect_public_api(
        &entry.items,
        bundle.entry,
        facts
            .name_ledger
            .module_alias(bundle.entry)
            .unwrap_or(&entry.alias),
        None,
        &facts.solved,
        &facts.name_ledger,
        &dimensions,
        &mut out,
    );
    out.sort();
    out
}

fn collect_public_api(
    items: &[crate::AST::Item],
    module_idx: usize,
    module_alias: &str,
    code_module: Option<&str>,
    solved: &std::collections::HashMap<String, crate::Sema::EffectSet>,
    ledger: &crate::AST::NameLedger,
    dimensions: &crate::Sema::ApiFreeze::ApiUnitDimensions,
    out: &mut Vec<ApiItem>,
) {
    for item in items {
        if let Some(api) = public_api_of_item(
            item,
            module_idx,
            module_alias,
            code_module,
            solved,
            ledger,
            dimensions,
        ) {
            out.push(api);
        }
        if let crate::AST::Item::Trait(trait_def) = item {
            let trait_name = code_module
                .map(|module| crate::AST::member_name(module, &trait_def.name))
                .unwrap_or_else(|| trait_def.name.clone());
            if ledger.public(module_idx, &trait_name) {
                out.extend(
                    trait_def
                        .methods
                        .iter()
                        .filter(|method| supported_public_name(&method.name))
                        .map(|method| ApiItem {
                            kind: "fn".to_string(),
                            name: public_item_name(
                                code_module,
                                &format!("{}.{}", trait_def.name, method.name),
                            ),
                            signature: crate::Sema::ApiFreeze::qualify_api_signature(
                                code_module,
                                &crate::Sema::ApiFreeze::trait_method_signature(
                                    &trait_def.name,
                                    method,
                                    dimensions,
                                ),
                            ),
                        }),
                );
            }
        }
        if let crate::AST::Item::CodeModule(module) = item {
            if let Some(body) = &module.body {
                collect_public_api(
                    body,
                    module_idx,
                    module_alias,
                    Some(&module.name),
                    solved,
                    ledger,
                    dimensions,
                    out,
                );
            }
        }
    }
}

fn public_item_name(code_module: Option<&str>, name: &str) -> String {
    code_module
        .map(|module| format!("{module}.{name}"))
        .unwrap_or_else(|| name.to_string())
}

/// Build an `ApiItem` for a single AST item, or `None` if it is private.
fn public_api_of_item(
    item: &crate::AST::Item,
    module_idx: usize,
    module_alias: &str,
    code_module: Option<&str>,
    solved: &std::collections::HashMap<String, crate::Sema::EffectSet>,
    ledger: &crate::AST::NameLedger,
    dimensions: &crate::Sema::ApiFreeze::ApiUnitDimensions,
) -> Option<ApiItem> {
    use crate::AST::Item;
    match item {
        Item::Func(f) if supported_public_name(&f.name) => {
            let ledger_name = code_module
                .map(|module| crate::AST::member_name(module, &f.name))
                .unwrap_or_else(|| f.name.clone());
            if !ledger.public(module_idx, &ledger_name) {
                return None;
            }
            let signature = format_fn_sig(
                f,
                solved.get(&format!(
                    "{module_alias}::{}",
                    code_module
                        .map(|module| crate::AST::member_name(module, &f.name))
                        .unwrap_or_else(|| f.name.clone())
                ))
                .or_else(|| solved.get(&f.name)),
                dimensions,
            );
            Some(ApiItem {
                kind: "fn".into(),
                name: public_item_name(code_module, &f.name),
                signature: crate::Sema::ApiFreeze::qualify_api_signature(
                    code_module,
                    &signature,
                ),
            })
        }
        Item::Struct(s) if supported_public_name(&s.name) => {
            let ledger_name = code_module
                .map(|module| crate::AST::member_name(module, &s.name))
                .unwrap_or_else(|| s.name.clone());
            ledger.public(module_idx, &ledger_name).then(|| ApiItem {
                kind: "struct".into(),
                name: public_item_name(code_module, &s.name),
                signature: format_struct_sig(s, dimensions),
            })
        }
        Item::Enum(e) if supported_public_name(&e.name) => {
            let ledger_name = code_module
                .map(|module| crate::AST::member_name(module, &e.name))
                .unwrap_or_else(|| e.name.clone());
            ledger.public(module_idx, &ledger_name).then(|| ApiItem {
                kind: "enum".into(),
                name: public_item_name(code_module, &e.name),
                signature: format_enum_sig(e),
            })
        }
        Item::Trait(t) if supported_public_name(&t.name) => {
            let ledger_name = code_module
                .map(|module| crate::AST::member_name(module, &t.name))
                .unwrap_or_else(|| t.name.clone());
            ledger.public(module_idx, &ledger_name).then(|| ApiItem {
                kind: "trait".into(),
                name: public_item_name(code_module, &t.name),
                signature: format_trait_sig(t, dimensions),
            })
        }
        // ConstDef does not carry is_pub in v1 — consts are accessible by name
        // and the pub distinction is enforced at use sites by sema. Skip from
        // public API for now; revisit when const visibility is added to the AST.
        Item::Const(_c) => None,
        _ => None,
    }
}

fn supported_public_name(name: &str) -> bool {
    crate::Syntax::classify_identifier(name) == crate::Syntax::IdentifierClass::Ordinary
}

fn format_type(
    ty: &crate::AST::Type,
    dimensions: &crate::Sema::ApiFreeze::ApiUnitDimensions,
) -> String {
    crate::Sema::ApiFreeze::canonical_api_type_name(ty, dimensions)
}

fn format_fn_sig(
    f: &crate::AST::Func,
    inferred: Option<&crate::Sema::EffectSet>,
    dimensions: &crate::Sema::ApiFreeze::ApiUnitDimensions,
) -> String {
    let type_params = crate::Sema::ApiFreeze::canonical_type_params(&f.type_params);
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            // c129: freeze the resolved capability sigil (D-CAP7) onto the
            // published type. By the time API metadata is emitted, sema
            // (D-CAP8) has resolved every `Infer` to a concrete convention, so
            // the published surface carries the sigil the caller must honor.
            // Plain read is the unmarked default and emits no sigil.
            format!(
                "{}: {}{}",
                p.name,
                p.convention.sigil(),
                format_type(&p.ty, dimensions)
            )
        })
        .collect();
    let ret = match inferred {
        Some(row) => {
            let row = crate::Sema::ApiFreeze::normalized_public_effect_row(f, row);
            format!(
                " =[{}]=>{}",
                row.iter().cloned().collect::<Vec<_>>().join(", "),
                f.return_type
                    .as_ref()
                    .map(|t| format!(" {}", format_type(t, dimensions)))
                    .unwrap_or_default()
            )
        }
        None => f
            .return_type
            .as_ref()
            .map(|t| format!(" => {}", format_type(t, dimensions)))
            .unwrap_or_default(),
    };
    format!("fn {}{}({}){}", f.name, type_params, params.join(", "), ret)
}

fn format_struct_sig(s: &crate::AST::StructDef, dimensions: &crate::Sema::ApiFreeze::ApiUnitDimensions) -> String {
    let is_cli = s
        .derives
        .iter()
        .any(|(name, _)| name == crate::Syntax::MARKER_CLI);
    let mut positional_order = 0u16;
    let fields: Vec<String> = s
        .fields
        .iter()
        .filter(|f| supported_public_name(&f.name))
        .map(|f| {
            let ty = crate::Sema::ApiFreeze::canonical_api_type_name(&f.ty, dimensions);
            if !is_cli {
                return format!("{}: {}", f.name, ty);
            }
            // D-CLI-POS1: publish API diff sees positional order and #[Flag]
            // opt-outs as part of the public command shape.
            let flag_only = f
                .serde_markers
                .iter()
                .any(|m| m.name == crate::Syntax::MARKER_FLAG);
            let has_default = f
                .serde_markers
                .iter()
                .any(|m| m.name == crate::Syntax::MARKER_DEFAULT);
            let is_bool = matches!(f.ty, crate::AST::Type::Bool);
            let is_optional = matches!(f.ty, crate::AST::Type::Option(_));
            if !is_bool && !is_optional && !has_default && !flag_only {
                let order = positional_order;
                positional_order = positional_order.saturating_add(1);
                format!("{}: {} [positional {order}]", f.name, ty)
            } else if flag_only {
                format!("{}: {} [flag]", f.name, ty)
            } else {
                format!("{}: {}", f.name, ty)
            }
        })
        .collect();
    format!("struct {} {{ {} }}", s.name, fields.join("; "))
}

fn format_enum_sig(e: &crate::AST::EnumDef) -> String {
    let variants: Vec<String> = e.variants.iter().filter(|v| supported_public_name(&v.name)).map(|v| v.name.clone()).collect();
    format!("enum {} {{ {} }}", e.name, variants.join(", "))
}

fn format_trait_sig(
    t: &crate::AST::TraitDef,
    dimensions: &crate::Sema::ApiFreeze::ApiUnitDimensions,
) -> String {
    let methods: Vec<String> = t
        .methods
        .iter()
        .filter(|method| supported_public_name(&method.name))
        .map(|method| {
            crate::Sema::ApiFreeze::trait_method_signature(&t.name, method, dimensions)
        })
        .collect();
    format!("trait {} {{ {} }}", t.name, methods.join(", "))
}
