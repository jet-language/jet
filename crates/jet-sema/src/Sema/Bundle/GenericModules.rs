use super::*;

mod Substitution;

use Substitution::{substitute_expr, substitute_meta, substitute_stmts};

// ---------------------------------------------------------------------------
// D-GENMOD2=A: generic module expansion (R11 pre-pass)
// ---------------------------------------------------------------------------
//
// `module string_cache32 = lru<String, 32>` expands into a synthetic
// `CodeModule` with the same body as the generic template, with every
// TypeParam name substituted by the supplied type arg. The original
// GenericModule/ModuleAlias items are then erased. This runs before
// `mangle_inline_sibling_calls` so the expanded body is visible to that pass.


fn specialize_func(
    mut func: Func,
    params: &[ResolvedModuleParam],
    args: &[ResolvedModuleArg],
    definition_types: &HashMap<String, Type>,
    definition_values: &HashMap<String, crate::AST::CtValue>,
) -> Func {
    let mut types = definition_types.clone();
    let mut values = definition_values.clone();
    for (param, arg) in params.iter().zip(args) {
        match (param, arg) {
            (ResolvedModuleParam::Type { name, .. }, ResolvedModuleArg::Type(ty)) => {
                types.insert(name.clone(), ty.clone());
            }
            (ResolvedModuleParam::Value { name, .. }, ResolvedModuleArg::Value(value, bytes)) => {
                let _ = bytes;
                values.insert(name.clone(), value.clone());
            }
            _ => {}
        }
    }
    substitute_meta(&mut func.meta, &types, &values);
    for param in &mut func.params {
        param.ty = specialize_module_type(&param.ty, &types, &values);
        if let Some(default) = &mut param.default {
            substitute_expr(default, &types, &values);
        }
    }
    if let Some(ret) = &mut func.return_type {
        *ret = specialize_module_type(ret, &types, &values);
    }
    substitute_stmts(&mut func.body, &types, &values);
    func
}

pub fn specialize_function_types(mut func: Func, types: &HashMap<String, Type>) -> Func {
    let values = HashMap::new();
    substitute_meta(&mut func.meta, types, &values);
    for param in &mut func.params {
        param.ty = specialize_module_type(&param.ty, types, &values);
        if let Some(default) = &mut param.default {
            substitute_expr(default, types, &values);
        }
    }
    if let Some(ret) = &mut func.return_type {
        *ret = specialize_module_type(ret, types, &values);
    }
    substitute_stmts(&mut func.body, types, &values);
    func
}

fn mapped_definition_name(name: &str, types: &HashMap<String, Type>) -> String {
    match types.get(name) {
        Some(Type::Named(mapped)) => mapped.clone(),
        _ => name.to_string(),
    }
}

pub(super) fn module_type_prefix(alias: &str) -> String {
    let (visibility, body) = alias.strip_prefix('_').map_or(("", alias), |body| ("_", body));
    let encoded = body.split('_').map(|segment| {
        let segment = crate::Syntax::canonical_name_case(segment, crate::Syntax::NameCase::Pascal);
        format!("{}{segment}", segment.chars().count())
    }).collect::<String>();
    format!("{visibility}M{encoded}")
}

fn module_type_name(alias: &str, name: &str) -> String {
    format!("{}{}", module_type_prefix(alias), name.trim_start_matches('_'))
}

fn module_value_name(alias: &str, name: &str) -> String {
    format!("{}_{}", alias.trim_end_matches('_'), name.trim_start_matches('_'))
}

fn specialize_tag(source: &crate::AST::TagDef, types: &HashMap<String, Type>,
    _values: &HashMap<String, crate::AST::CtValue>) -> crate::AST::TagDef {
    let mut result = source.clone();
    result.name = mapped_definition_name(&source.name, types);
    result
}

fn specialize_test(source: &crate::AST::TestDef, alias: &str,
    types: &HashMap<String, Type>, values: &HashMap<String, crate::AST::CtValue>) -> crate::AST::TestDef {
    let mut result = source.clone();
    if let Some(expression) = &mut result.name_expr {
        substitute_expr(expression, types, values);
        result.name = None;
        result.name_prefix = Some(alias.to_string());
    } else {
        result.name = source
            .name
            .as_deref()
            .map(|name| module_value_name(alias, name));
    }
    for param in &mut result.params {
        param.ty = specialize_module_type(&param.ty, types, values);
        if let Some(default) = &mut param.default { substitute_expr(default, types, values); }
    }
    substitute_stmts(&mut result.body, types, values);
    result
}

fn specialize_bench(source: &crate::AST::BenchDef, alias: &str,
    types: &HashMap<String, Type>, values: &HashMap<String, crate::AST::CtValue>) -> crate::AST::BenchDef {
    let mut result = source.clone();
    substitute_expr(&mut result.name_expr, types, values);
    result.name = None;
    result.name_prefix = Some(alias.to_string());
    substitute_stmts(&mut result.body, types, values);
    result
}

fn specialize_trait(
    source: &crate::AST::TraitDef,
    params: &[ResolvedModuleParam],
    args: &[ResolvedModuleArg],
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) -> crate::AST::TraitDef {
    let mut result = source.clone();
    result.name = mapped_definition_name(&source.name, types);
    for method in &mut result.methods {
        for param in &mut method.params {
            param.ty = specialize_module_type(&param.ty, types, values);
            if let Some(default) = &mut param.default { substitute_expr(default, types, values); }
        }
        if let Some(ret) = &mut method.return_type {
            *ret = specialize_module_type(ret, types, values);
        }
        if let Some(body) = &mut method.default_body {
            substitute_stmts(body, types, values);
        }
    }
    let _ = (params, args);
    result
}

fn specialize_impl(
    source: &crate::AST::ImplDef,
    params: &[ResolvedModuleParam],
    args: &[ResolvedModuleArg],
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) -> crate::AST::ImplDef {
    let mut result = source.clone();
    result.type_name = mapped_definition_name(&source.type_name, types);
    result.trait_name = source.trait_name.as_ref().map(|name| mapped_definition_name(name, types));
    result.methods = source.methods.iter().cloned()
        .map(|method| specialize_func(method, params, args, types, values)).collect();
    result.assoc_type_impls = source.assoc_type_impls.iter().map(|(name, span, ty)| {
        (name.clone(), *span, specialize_module_type(ty, types, values))
    }).collect();
    result
}

fn specialize_error_conv(
    source: &crate::AST::ErrorConvDef,
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) -> crate::AST::ErrorConvDef {
    let mut result = source.clone();
    result.from_ty = mapped_definition_name(&source.from_ty, types);
    result.to_ty = mapped_definition_name(&source.to_ty, types);
    substitute_stmts(&mut result.body, types, values);
    result
}

fn specialize_module_type(
    ty: &Type,
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) -> Type {
    let mut resolved = crate::Generics::substitute_type(ty, types);
    fn lengths(ty: &mut Type, types: &HashMap<String, Type>, values: &HashMap<String, crate::AST::CtValue>) {
        match ty {
            Type::FixedList { elem, len, len_symbol } => {
                lengths(elem, types, values);
                if let Some((name, _)) = len_symbol.as_ref() {
                    if let Some(crate::AST::CtValue::Int(value)) = values.get(name) {
                        if *value >= 0 {
                            *len = *value as u64;
                            *len_symbol = None;
                        }
                    }
                }
            }
            Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => lengths(inner, types, values),
            Type::Map { key, value, .. } => { lengths(key, types, values); lengths(value, types, values); }
            Type::Result { ok, err } => { lengths(ok, types, values); lengths(err, types, values); }
            Type::Fn { params, ret, .. } => { for param in params { lengths(param, types, values); } if let Some(ret) = ret { lengths(ret, types, values); } }
            Type::Apply { args, .. } => args.iter_mut().for_each(|arg| lengths(arg, types, values)),
            Type::Tuple(fields) => fields.iter_mut().for_each(|(_, ty)| lengths(ty, types, values)),
            Type::Tagged { marker, inner } => {
                if let Some(Type::Named(mapped)) = types.get(marker) { *marker = mapped.clone(); }
                **inner = crate::Generics::substitute_type(inner, types);
                lengths(inner, types, values);
            }
            _ => {}
        }
    }
    lengths(&mut resolved, types, values);
    resolved
}

fn specialize_nested_code_module(
    module: &CodeModule,
    params: &[ResolvedModuleParam],
    args: &[ResolvedModuleArg],
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) -> CodeModule {
    let body = module.body.as_ref().map(|items| {
        items.iter().filter_map(|item| match item {
            Item::Func(def) => Some(Item::Func(specialize_func(def.clone(), params, args, types, values))),
            Item::Struct(def) => Some(Item::Struct(specialize_struct(def, "", params, args, types, values))),
            Item::Enum(def) => Some(Item::Enum(specialize_enum(def, "", params, args, types, values))),
            Item::Const(def) => {
                let mut value = def.value.clone();
                substitute_expr(&mut value, types, values);
                let mut result = def.clone();
                result.value = value;
                result.ty = def.ty.as_ref().map(|ty| specialize_module_type(ty, types, values));
                substitute_meta(&mut result.meta, types, values);
                Some(Item::Const(result))
            }
            Item::CodeModule(child) => Some(Item::CodeModule(specialize_nested_code_module(child, params, args, types, values))),
            Item::GenericModule(def) => Some(Item::GenericModule(
                specialize_nested_template_outer(def, types, values),
            )),
            Item::ModuleAlias(def) => Some(Item::ModuleAlias(
                specialize_nested_alias_outer(def, types, values),
            )),
            Item::Trait(def) => Some(Item::Trait(specialize_trait(def, params, args, types, values))),
            Item::Tag(def) => Some(Item::Tag(specialize_tag(def, types, values))),
            Item::Impl(def) => Some(Item::Impl(specialize_impl(def, params, args, types, values))),
            Item::ErrorConv(def) => Some(Item::ErrorConv(specialize_error_conv(def, types, values))),
            Item::Test(def) => Some(Item::Test(specialize_test(def, &module.name, types, values))),
            Item::Bench(def) => Some(Item::Bench(specialize_bench(def, &module.name, types, values))),
            _ => None,
        }).collect()
    });
    let mut result = module.clone();
    result.body = body;
    result
}

fn specialize_struct(
    source: &crate::AST::StructDef,
    alias: &str,
    params: &[ResolvedModuleParam],
    args: &[ResolvedModuleArg],
    definition_types: &HashMap<String, Type>,
    definition_values: &HashMap<String, crate::AST::CtValue>,
) -> crate::AST::StructDef {
    let mut types = definition_types.clone();
    for (param, arg) in params.iter().zip(args) {
        if let (ResolvedModuleParam::Type { name, .. }, ResolvedModuleArg::Type(ty)) = (param, arg) {
            types.insert(name.clone(), ty.clone());
        }
    }
    let mut fields = source.fields.clone();
    for field in &mut fields {
        field.ty = specialize_module_type(&field.ty, &types, definition_values);
        if let Some(computed) = &mut field.computed {
            substitute_expr(computed, &types, definition_values);
        }
        for marker in &mut field.serde_markers {
            marker
                .args
                .iter_mut()
                .for_each(|arg| substitute_expr(arg, &types, definition_values));
        }
    }
    let methods = source
        .methods
        .iter()
        .cloned()
        .map(|method| {
            specialize_func(
                method,
                params,
                args,
                definition_types,
                definition_values,
            )
        })
        .collect();
    let trait_impls = source
        .trait_impls
        .iter()
        .map(|block| crate::AST::TraitImplBlock {
            trait_name: mapped_definition_name(&block.trait_name, &types),
            trait_span: block.trait_span,
            methods: block
                .methods
                .iter()
                .cloned()
                .map(|method| {
                    specialize_func(
                        method,
                        params,
                        args,
                        definition_types,
                        definition_values,
                    )
                })
                .collect(),
            assoc_type_impls: block
                .assoc_type_impls
                .iter()
                .map(|(name, span, ty)| {
                    (
                        name.clone(),
                        *span,
                        specialize_module_type(ty, &types, definition_values),
                    )
                })
                .collect(),
        })
        .collect();
    let mut result = source.clone();
    if !alias.is_empty() {
        result.name = module_type_name(alias, &source.name);
    }
    result.fields = fields;
    result.methods = methods;
    result.trait_impls = trait_impls;
    result.derives = source.derives.iter().map(|(name, span)| {
        (mapped_definition_name(name, &types), *span)
    }).collect();
    result
}

pub(super) fn clone_struct(source: &crate::AST::StructDef) -> crate::AST::StructDef {
    source.clone()
}

fn specialize_enum(
    source: &crate::AST::EnumDef,
    alias: &str,
    params: &[ResolvedModuleParam],
    args: &[ResolvedModuleArg],
    definition_types: &HashMap<String, Type>,
    definition_values: &HashMap<String, crate::AST::CtValue>,
) -> crate::AST::EnumDef {
    let mut types = definition_types.clone();
    for (param, arg) in params.iter().zip(args) {
        if let (ResolvedModuleParam::Type { name, .. }, ResolvedModuleArg::Type(ty)) = (param, arg) {
            types.insert(name.clone(), ty.clone());
        }
    }
    let variants = source
        .variants
        .iter()
        .map(|variant| {
            let payload = match &variant.payload {
                VariantPayload::Unit => VariantPayload::Unit,
                VariantPayload::Single(ty, span) => VariantPayload::Single(
                    specialize_module_type(ty, &types, definition_values),
                    *span,
                ),
                VariantPayload::Named(fields) => VariantPayload::Named(
                    fields
                        .iter()
                        .cloned()
                        .map(|mut field| {
                            field.ty = specialize_module_type(&field.ty, &types, definition_values);
                            field
                        })
                        .collect(),
                ),
            };
            let mut serde_markers = variant.serde_markers.clone();
            for marker in &mut serde_markers {
                marker.args.iter_mut().for_each(|arg| {
                    substitute_expr(arg, &types, definition_values);
                });
            }
            crate::AST::Variant {
                name: variant.name.clone(),
                name_span: variant.name_span,
                payload,
                discriminant: variant.discriminant,
                serde_markers,
            }
        })
        .collect();
    let methods = source
        .methods
        .iter()
        .cloned()
        .map(|method| {
            specialize_func(
                method,
                params,
                args,
                definition_types,
                definition_values,
            )
        })
        .collect();
    let trait_impls = source
        .trait_impls
        .iter()
        .map(|block| crate::AST::TraitImplBlock {
            trait_name: mapped_definition_name(&block.trait_name, &types),
            trait_span: block.trait_span,
            methods: block
                .methods
                .iter()
                .cloned()
                .map(|method| {
                    specialize_func(
                        method,
                        params,
                        args,
                        definition_types,
                        definition_values,
                    )
                })
                .collect(),
            assoc_type_impls: block
                .assoc_type_impls
                .iter()
                .map(|(name, span, ty)| {
                    (
                        name.clone(),
                        *span,
                        crate::Generics::substitute_type(ty, &types),
                    )
                })
                .collect(),
        })
        .collect();
    let mut result = source.clone();
    if !alias.is_empty() {
        result.name = module_type_name(alias, &source.name);
    }
    result.variants = variants;
    result.methods = methods;
    result.trait_impls = trait_impls;
    result.derives = source.derives.iter().map(|(name, span)| {
        (mapped_definition_name(name, &types), *span)
    }).collect();
    result
}

pub(super) fn clone_enum(source: &crate::AST::EnumDef) -> crate::AST::EnumDef {
    source.clone()
}

#[derive(Clone)]
enum ResolvedModuleParam {
    Type { name: String, bound: Option<String> },
    Value { name: String, ty: Type },
    Invalid,
}

/// A cloned view of a generic module template.
#[derive(Clone)]
struct TemplateInfo {
    def: GenericModuleDef,
    definition_id: String,
    definition_full_key: Vec<u8>,
    params: Vec<ResolvedModuleParam>,
    source_module: usize,
    source_items: Vec<Item>,
    source_values: HashMap<String, crate::AST::CtValue>,
}

fn clone_definition_items(items: &[Item]) -> Vec<Item> {
    items
        .iter()
        .filter(|item| matches!(item, Item::Func(_) | Item::Struct(_) | Item::Enum(_)
            | Item::Trait(_) | Item::Tag(_) | Item::Impl(_) | Item::ErrorConv(_)
            | Item::Test(_) | Item::Bench(_) | Item::Const(_)))
        .cloned()
        .collect()
}

struct AliasExpansion {
    module: CodeModule,
    declarations: Vec<Item>,
}

fn specialize_nested_template_outer(
    source: &GenericModuleDef,
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) -> GenericModuleDef {
    let mut result = source.clone();
    result.body = result.body.into_iter().map(|item| match item {
        Item::Func(func) => Item::Func(specialize_func(func, &[], &[], types, values)),
        Item::Struct(def) => Item::Struct(specialize_struct(&def, "", &[], &[], types, values)),
        Item::Enum(def) => Item::Enum(specialize_enum(&def, "", &[], &[], types, values)),
        Item::Trait(def) => Item::Trait(specialize_trait(&def, &[], &[], types, values)),
        Item::Tag(def) => Item::Tag(specialize_tag(&def, types, values)),
        Item::Impl(def) => Item::Impl(specialize_impl(&def, &[], &[], types, values)),
        Item::ErrorConv(def) => Item::ErrorConv(specialize_error_conv(&def, types, values)),
        Item::Test(def) => Item::Test(specialize_test(&def, &source.name, types, values)),
        Item::Bench(def) => Item::Bench(specialize_bench(&def, &source.name, types, values)),
        Item::GenericModule(def) => Item::GenericModule(specialize_nested_template_outer(&def, types, values)),
        other => other,
    }).collect();
    result
}

fn specialize_nested_alias_outer(
    source: &ModuleAliasDef,
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) -> ModuleAliasDef {
    let mut args = source.args.clone();
    for arg in &mut args {
        match arg {
            ModuleArg::Type(ty, _) => *ty = specialize_module_type(ty, types, values),
            ModuleArg::Value(expr, _) => substitute_expr(expr, types, values),
        }
    }
    ModuleAliasDef { name: source.name.clone(), name_span: source.name_span,
        is_pub: source.is_pub, is_package_pub: source.is_package_pub, target: source.target.clone(),
        target_span: source.target_span, args, span: source.span }
}

fn expand_nested_generics_in_code_module(
    module: &mut CodeModule,
    lexical_path: &str,
    consumer_module: usize,
    application_module: &str,
    enclosing_full_key: &[u8],
    inherited_values: &HashMap<String, crate::AST::CtValue>,
    diags: &mut Vec<Diagnostic>,
    instances: &mut HashMap<ModuleInstanceKey, String>,
    fingerprints: &mut HashMap<String, Vec<u8>>,
    applications: &mut HashMap<String, Vec<crate::AST::ModuleInstanceApplication>>,
) -> Vec<Item> {
    let Some(items) = module.body.as_mut() else { return Vec::new() };
    let enclosing_identity = crate::SHA256::sha256_hex(enclosing_full_key);
    let scope_full_key = definition_full_key("nested", "", &enclosing_identity, lexical_path);
    let mut declarations = Vec::new();

    for item in items.iter_mut() {
        let Item::CodeModule(child) = item else { continue };
        let child_local = child.name.clone();
        child.name = module_value_name(&module.name, &child_local);
        declarations.extend(expand_nested_generics_in_code_module(
            child,
            &format!("{lexical_path}.{child_local}"),
            consumer_module,
            application_module,
            &scope_full_key,
            inherited_values,
            diags,
            instances,
            fingerprints,
            applications,
        ));
    }

    let nested_defs: Vec<GenericModuleDef> = items.iter().filter_map(|item| {
        let Item::GenericModule(def) = item else { return None };
        Some(def.clone())
    }).collect();
    let alias_defs: Vec<ModuleAliasDef> = items.iter().filter_map(|item| {
        let Item::ModuleAlias(def) = item else { return None };
        Some(def.clone())
    }).collect();
    if nested_defs.is_empty() && alias_defs.is_empty() {
        return declarations;
    }

    let mut traits = TraitRegistry::default();
    traits.register_synthetic_rollback();
    traits.register_synthetic_display_debug();
    traits.register_synthetic_close();
    traits.register_synthetic_operators();
    traits.register_synthetic_iter_index();
    traits.register_synthetic_io();
    traits.register_items(items, &mut Vec::new());
    for def in &nested_defs { traits.register_items(&def.body, &mut Vec::new()); }

    let enums: HashMap<String, bool> = items.iter()
        .chain(nested_defs.iter().flat_map(|def| def.body.iter()))
        .filter_map(|item| {
            let Item::Enum(def) = item else { return None };
            Some((def.name.clone(), def.variants.iter().all(|variant| matches!(variant.payload, VariantPayload::Unit))))
        })
        .collect();
    let funcs: HashMap<String, &Func> = items.iter()
        .chain(nested_defs.iter().flat_map(|def| def.body.iter()))
        .filter_map(|item| {
            let Item::Func(def) = item else { return None };
            Some((def.name.clone(), def))
        })
        .collect();
    let mut values = inherited_values.clone();
    for item in items.iter() {
        let Item::Const(def) = item else { continue };
        if let Some(value) = def.ct.clone().or_else(|| {
            crate::Comptime::evaluate(&def.value, &funcs, &HashSet::new(), Path::new("."), &values).ok()
        }) {
            values.insert(def.name.clone(), value);
        }
    }

    let templates: HashMap<String, TemplateInfo> = nested_defs.iter().map(|def| {
        let full_key = definition_full_key("nested", "", &crate::SHA256::sha256_hex(&scope_full_key), &def.name);
        (def.name.clone(), TemplateInfo {
            def: def.clone(),
            definition_id: crate::SHA256::sha256_hex(&full_key),
            definition_full_key: full_key,
            params: resolve_params(def, &traits, &enums, diags),
            source_module: consumer_module,
            source_items: Vec::new(),
            source_values: values.clone(),
        })
    }).collect();
    let aliases: HashMap<String, &ModuleAliasDef> = alias_defs.iter()
        .map(|def| (def.name.clone(), def))
        .collect();
    let mut ordered: Vec<&ModuleAliasDef> = alias_defs.iter().collect();
    ordered.sort_by_key(|def| local_alias_depth(def, &aliases));
    let mut call_projections = HashMap::new();
    let mut type_projections = HashMap::new();
    let mut invalid_aliases = HashSet::new();

    for nested_alias in ordered {
        if alias_chain_contains(nested_alias, &aliases, &invalid_aliases) {
            continue;
        }
        let Some(mut resolved) = resolve_local_alias(nested_alias, &aliases, &templates, diags) else {
            invalid_aliases.insert(nested_alias.name.clone());
            continue;
        };
        let local_name = resolved.name.clone();
        resolved.name = module_value_name(&module.name, &local_name);
        let Some(info) = templates.get(&resolved.target) else {
            diags.push(Diagnostic::error(
                "E0850",
                format!("generic module `{}` not found in this scope", resolved.target),
                "check the module template name and make sure it is defined in the same file"
                    .to_string(),
                format!("example: `module {} = MyTemplate<String>`", local_name),
                Some(resolved.target_span),
            ));
            invalid_aliases.insert(nested_alias.name.clone());
            continue;
        };
        let Some(args) = resolve_args(&resolved, info, &traits, &funcs, &values, &enums, diags) else {
            invalid_aliases.insert(nested_alias.name.clone());
            continue;
        };
        let key = instance_key(info, &args, &HashMap::new());
        let fingerprint = crate::SHA256::sha256_hex(&key.bytes());
        applications.entry(fingerprint.clone()).or_default().push(
            crate::AST::ModuleInstanceApplication {
                name: resolved.name.clone(),
                source_module: application_module.to_string(),
                semantic_identity: format!("instance:{fingerprint}"),
                span: resolved.name_span,
            },
        );
        let canonical = if let Some(canonical) = instances.get(&key) {
            canonical.clone()
        } else {
            let Some(mut expansion) = expand_alias(
                &resolved,
                consumer_module,
                application_module,
                &key.bytes(),
                &templates,
                diags,
                &traits,
                &funcs,
                &values,
                &enums,
                instances,
                fingerprints,
                applications,
                Some(args),
            ) else { continue };
            let identity = instance_identity(&key, info, &resolved, application_module);
            register_instance_fingerprint(fingerprints, &identity, resolved.span);
            expansion.module.instance_identity = Some(identity);
            instances.insert(key, resolved.name.clone());
            declarations.push(Item::CodeModule(expansion.module));
            declarations.extend(expansion.declarations);
            resolved.name.clone()
        };
        call_projections.insert(local_name.clone(), canonical.clone());
        type_projections.insert(local_name, Type::Named(canonical.clone()));
        type_projections.insert(resolved.name, Type::Named(canonical));
    }

    items.retain(|item| !matches!(item, Item::GenericModule(_) | Item::ModuleAlias(_)));
    for item in items.iter_mut() {
        let Item::Func(func) = item else { continue };
        for (local, canonical) in &call_projections {
            rewrite_inline_calls_stmts(&mut func.body, &HashSet::from([local.clone()]), canonical);
        }
        for param in &mut func.params {
            param.ty = crate::Generics::substitute_type(&param.ty, &type_projections);
        }
        if let Some(ret) = &mut func.return_type {
            *ret = crate::Generics::substitute_type(ret, &type_projections);
        }
        substitute_stmts(&mut func.body, &type_projections, &HashMap::new());
    }
    declarations
}

#[derive(Clone)]
enum ResolvedModuleArg { Type(Type), Value(crate::AST::CtValue, Vec<u8>) }

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ModuleInstanceKey {
    definition_full_key: Vec<u8>,
    parameters: Vec<u8>,
    args: Vec<Vec<u8>>,
}

impl ModuleInstanceKey {
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        frame_bytes(&mut out, b"jet.genmod.application.v1");
        frame_bytes(&mut out, &self.definition_full_key);
        frame_bytes(&mut out, &self.parameters);
        out.extend_from_slice(&(self.args.len() as u64).to_be_bytes());
        for arg in &self.args {
            out.extend_from_slice(&(arg.len() as u64).to_be_bytes());
            out.extend_from_slice(arg);
        }
        out
    }
}

fn frame_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn frame_text(out: &mut Vec<u8>, text: &str) { frame_bytes(out, text.as_bytes()); }

fn type_full_key(ty: &Type) -> Vec<u8> {
    fn write(out: &mut Vec<u8>, ty: &Type) {
        use Type::*;
        match ty {
            Int => out.push(1), Float => out.push(2), Bool => out.push(3), String => out.push(4), Char => out.push(5),
            List(inner) => { out.push(6); write(out, inner); }
            Map { key, value, .. } => { out.push(7); write(out, key); write(out, value); }
            Shared(inner) => { out.push(8); write(out, inner); }
            Option(inner) => { out.push(9); write(out, inner); }
            Result { ok, err } => { out.push(10); write(out, ok); write(out, err); }
            Fn { params, ret, .. } => {
                out.push(11); out.extend_from_slice(&(params.len() as u64).to_be_bytes());
                for param in params { write(out, param); }
                match ret { Some(ret) => { out.push(1); write(out, ret); }, None => out.push(0) }
            }
            Named(name) => { out.push(12); frame_text(out, name); }
            Apply { name, args } => {
                out.push(13); frame_text(out, name); out.extend_from_slice(&(args.len() as u64).to_be_bytes());
                for arg in args { write(out, arg); }
            }
            TraitObject(names) => { out.push(14); out.extend_from_slice(&(names.len() as u64).to_be_bytes()); for name in names { frame_text(out, name); } }
            Tuple(fields) => { out.push(15); out.extend_from_slice(&(fields.len() as u64).to_be_bytes()); for (name, ty) in fields { frame_text(out, name); write(out, ty); } }
            FixedList { elem, len, .. } => { out.push(16); write(out, elem); out.extend_from_slice(&len.to_be_bytes()); }
            IntN { signed, bits } => { out.push(17); out.push(u8::from(*signed)); out.push(*bits); }
            Float32 => out.push(18),
            Tagged { inner, .. } => write(out, inner),
            Union(members) => {
                out.push(19);
                out.extend_from_slice(&(members.len() as u64).to_be_bytes());
                for m in members {
                    write(out, m);
                }
            }
        }
    }
    let mut out = Vec::new();
    frame_bytes(&mut out, b"jet.type.full-key.v1");
    write(&mut out, ty);
    out
}

fn parameter_bytes(params: &[ResolvedModuleParam]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(params.len() as u64).to_be_bytes());
    for param in params {
        match param {
            ResolvedModuleParam::Type { name, bound } => {
                out.push(0); frame_text(&mut out, name);
                frame_text(&mut out, bound.as_deref().unwrap_or(""));
            }
            ResolvedModuleParam::Value { name, ty } => {
                out.push(1); frame_text(&mut out, name); frame_bytes(&mut out, &type_full_key(ty));
            }
            ResolvedModuleParam::Invalid => out.push(2),
        }
    }
    out
}

fn definition_full_key(package_identity: &str, module_path: &str, lexical_path: &str, name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    frame_bytes(&mut out, b"jet.genmod.definition.v1");
    frame_text(&mut out, package_identity);
    frame_text(&mut out, module_path);
    frame_text(&mut out, lexical_path);
    frame_text(&mut out, "generic-module");
    frame_text(&mut out, name);
    out
}

fn quoted_field(text: &str, field: &str) -> Option<String> {
    let needle = format!("{field}:");
    text.match_indices(&needle).find_map(|(offset, _)| {
        let boundary = text[..offset].chars().next_back();
        if boundary.is_some_and(|ch| ch.is_alphanumeric() || ch == '_') { return None; }
        let rest = text[offset + needle.len()..].trim_start().strip_prefix('"')?;
        Some(rest.split('"').next()?.to_string())
    })
}

fn canonical_semver(version: &str) -> String {
    let (core_pre, build) = version.split_once('+').map_or((version, None), |(core, build)| (core, Some(build)));
    let (core, pre) = core_pre.split_once('-').map_or((core_pre, None), |(core, pre)| (core, Some(pre)));
    let parts: Vec<_> = core.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|part| part.is_empty()
        || !part.bytes().all(|byte| byte.is_ascii_digit())
        || (part.len() > 1 && part.starts_with('0')))
    {
        return version.trim().to_string();
    }
    let mut canonical = parts.iter().map(|part| part.parse::<u64>().unwrap_or(0).to_string()).collect::<Vec<_>>().join(".");
    if let Some(pre) = pre { canonical.push('-'); canonical.push_str(pre); }
    if let Some(build) = build { canonical.push('+'); canonical.push_str(build); }
    canonical
}

fn lock_value(line: &str, field: &str) -> Option<String> {
    let (key, value) = line.split_once('=')?;
    (key.trim() == field).then(|| value.trim().trim_matches('"').to_string())
}

fn inline_lock_value(table: &str, field: &str) -> Option<String> {
    let table = table.trim().trim_start_matches('{').trim_end_matches('}');
    table.split(',').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        (key.trim() == field).then(|| value.trim().trim_matches('"').to_string())
    })
}

fn credential_free_git_url(url: &str) -> String {
    let without_fragment = url.split(['?', '#']).next().unwrap_or(url);
    let Some((scheme, authority_path)) = without_fragment.split_once("://") else {
        return without_fragment.to_string();
    };
    let (authority, path) = authority_path.split_once('/').unwrap_or((authority_path, ""));
    let clean_authority = authority.rsplit_once('@').map_or(authority, |(_, clean)| clean);
    format!("{}://{}{}{}", scheme.to_ascii_lowercase(), clean_authority, if path.is_empty() { "" } else { "/" }, path)
}

fn canonical_lock_source(
    project_root: &Path,
    package_root: &Path,
    dependency_name: Option<&str>,
    package_name: &str,
) -> String {
    if dependency_name.is_none() { return "workspace".into(); }
    let raw = std::fs::read_to_string(project_root.join(crate::Syntax::UNIFIED_LOCK_FILE)).unwrap_or_default();
    let wanted = dependency_name.unwrap_or(package_name);
    let mut current = false;
    let mut name = String::new();
    let mut version = String::new();
    let mut source = String::new();
    let mut locked = String::new();
    let mut content_hash = String::new();
    let mut records = Vec::new();
    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
        if line.starts_with('[') {
            if current { records.push((std::mem::take(&mut name), std::mem::take(&mut version), std::mem::take(&mut source), std::mem::take(&mut locked), std::mem::take(&mut content_hash))); }
            current = line == "[[package]]";
            continue;
        }
        if !current { continue; }
        if let Some(value) = lock_value(line, "name") { name = value; }
        else if let Some(value) = lock_value(line, "version") { version = value; }
        else if let Some(value) = lock_value(line, "source") { source = value; }
        else if let Some(value) = lock_value(line, "locked") { locked = value; }
        else if let Some(value) = lock_value(line, "content-hash") { content_hash = value; }
    }
    if current { records.push((name, version, source, locked, content_hash)); }
    if let Some((_, locked_version, source, locked, content_hash)) = records.into_iter()
        .find(|(name, ..)| name == wanted || name == package_name)
    {
        if let Some(path) = inline_lock_value(&source, "path") {
            if let Some(registry) = path.strip_prefix("registry:") {
                return format!("registry:{registry}@{}#{content_hash}", canonical_semver(&locked_version));
            }
            let canonical = if Path::new(&path).is_absolute() {
                wanted.to_string()
            } else {
                path.replace('\\', "/").split('/').filter(|part| !part.is_empty() && *part != ".").collect::<Vec<_>>().join("/")
            };
            let content = if !content_hash.is_empty() {
                content_hash
            } else {
                inline_lock_value(&locked, "tree-hash").unwrap_or_else(|| "unlocked".into())
            };
            return format!("path:{canonical}#{content}");
        }
        if let Some(url) = inline_lock_value(&source, "git") {
            let rev = inline_lock_value(&locked, "rev").unwrap_or_default();
            let tree = inline_lock_value(&locked, "tree-hash").unwrap_or(content_hash);
            return format!("git:{}@{rev}#{tree}", credential_free_git_url(&url));
        }
    }
    let relative = package_root.strip_prefix(project_root).ok()
        .and_then(|path| path.to_str()).filter(|path| !path.is_empty())
        .map(|path| path.replace('\\', "/"))
        .unwrap_or_else(|| wanted.to_string());
    format!("path:{relative}")
}

fn owning_package<'a>(bundle: &'a ProgramBundle, module_path: &Path) -> (&'a Path, Option<&'a str>) {
    bundle.dep_roots.iter()
        .filter(|(_, root)| module_path.starts_with(root))
        .max_by_key(|(_, root)| root.components().count())
        .map(|(name, root)| (root.as_path(), Some(name.as_str())))
        .unwrap_or((bundle.project_root.as_path(), None))
}

fn package_identity(bundle: &ProgramBundle, root: &Path, dependency_name: Option<&str>) -> String {
    let manifest = std::fs::read_to_string(root.join(crate::Syntax::PAYLOAD_FILE)).unwrap_or_default();
    let name = quoted_field(&manifest, "name").or_else(|| dependency_name.map(str::to_string)).unwrap_or_else(|| "workspace".into());
    let version = canonical_semver(&quoted_field(&manifest, "version").unwrap_or_else(|| "0.0.0+workspace".into()));
    let source = canonical_lock_source(&bundle.project_root, root, dependency_name, &name);
    let mut bytes = Vec::new();
    frame_bytes(&mut bytes, b"jet.package.identity.v2");
    frame_text(&mut bytes, &name);
    frame_text(&mut bytes, &version);
    frame_text(&mut bytes, &source);
    crate::SHA256::sha256_hex(&bytes)
}

fn instance_identity(
    key: &ModuleInstanceKey,
    template: &TemplateInfo,
    alias: &ModuleAliasDef,
    source_module: &str,
) -> crate::AST::ModuleInstanceIdentity {
    let full_key = key.bytes();
    let fingerprint = crate::SHA256::sha256_hex(&full_key);
    crate::AST::ModuleInstanceIdentity {
        fingerprint: fingerprint.clone(),
        full_key,
        definition_id: template.definition_id.clone(),
        argument_keys: key.args.clone(),
        template_span: template.def.span,
        applications: vec![crate::AST::ModuleInstanceApplication {
            name: alias.name.clone(),
            source_module: source_module.to_string(),
            semantic_identity: format!("instance:{fingerprint}"),
            span: alias.name_span,
        }],
    }
}

fn register_instance_fingerprint(
    registry: &mut HashMap<String, Vec<u8>>,
    identity: &crate::AST::ModuleInstanceIdentity,
    span: Span,
) {
    if let Some(previous) = registry.get(&identity.fingerprint) {
        if previous != &identity.full_key {
            let hex = |bytes: &[u8]| bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
            jet_foundation::ice!(
                Some(span),
                "E0859 generic module instance fingerprint collision: digest={} first-full-key={} second-full-key={}; compilation stopped before codegen",
                identity.fingerprint,
                hex(previous),
                hex(&identity.full_key),
            );
        }
    } else {
        registry.insert(identity.fingerprint.clone(), identity.full_key.clone());
    }
}

fn normalized_instance_type(ty: &Type, aliases: &HashMap<String, Type>) -> Type {
    fn go(ty: &Type, aliases: &HashMap<String, Type>, seen: &mut HashSet<String>) -> Type {
        if let Type::Named(name) = ty {
            if seen.insert(name.clone()) {
                if let Some(target) = aliases.get(name) {
                    let result = go(target, aliases, seen);
                    seen.remove(name);
                    return result;
                }
                seen.remove(name);
            }
        }
        crate::Generics::substitute_type(ty, &aliases.iter().map(|(name, target)| (name.clone(), target.clone())).collect())
    }
    go(ty, aliases, &mut HashSet::new())
}

fn instance_key(
    info: &TemplateInfo,
    args: &[ResolvedModuleArg],
    type_aliases: &HashMap<String, Type>,
) -> ModuleInstanceKey {
    let args = args.iter().map(|arg| match arg {
        ResolvedModuleArg::Type(ty) => {
            let key = type_full_key(&normalized_instance_type(ty, type_aliases));
            let mut bytes = vec![0];
            frame_bytes(&mut bytes, &key);
            bytes
        }
        ResolvedModuleArg::Value(_, normalized) => {
            let mut bytes = vec![1];
            bytes.extend_from_slice(&(normalized.len() as u64).to_be_bytes());
            bytes.extend_from_slice(normalized);
            bytes
        }
    }).collect();
    ModuleInstanceKey { definition_full_key: info.definition_full_key.clone(), parameters: parameter_bytes(&info.params), args }
}

fn resolve_params(
    def: &GenericModuleDef,
    traits: &TraitRegistry,
    enums: &HashMap<String, bool>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<ResolvedModuleParam> {
    def.params
        .iter()
        .map(|param| match param {
            GenericModuleParam::Bare { name, .. } => ResolvedModuleParam::Type {
                name: name.clone(),
                bound: None,
            },
            GenericModuleParam::Annotated {
                name,
                name_span,
                annotation,
            } => {
                if let Type::Named(bound) = annotation {
                    if traits.traits.contains_key(bound) {
                        return ResolvedModuleParam::Type {
                            name: name.clone(),
                            bound: Some(bound.clone()),
                        };
                    }
                }
                let allowed = matches!(annotation, Type::Bool | Type::Int | Type::Char | Type::String)
                    || matches!(annotation, Type::Named(n) if enums.get(n).copied() == Some(true));
                if allowed {
                    ResolvedModuleParam::Value {
                        name: name.clone(),
                        ty: annotation.clone(),
                    }
                } else {
                    diags.push(Diagnostic::error(
                        "E0856",
                        format!(
                            "generic module value parameter `{name}` uses unsupported type `{}`",
                            type_name(annotation)
                        ),
                        "value parameters admit only Bool, Int, Char, String, or a fieldless enum"
                            .to_string(),
                        "use a Tier-0 value type, or make this an unannotated type parameter"
                            .to_string(),
                        Some(*name_span),
                    ));
                    ResolvedModuleParam::Invalid
                }
            }
        })
        .collect()
}

fn type_name(ty:&Type)->String{match ty{Type::Int=>"Int".into(),Type::Bool=>"Bool".into(),Type::Char=>"Char".into(),Type::String=>"String".into(),Type::Named(n)=>n.clone(),Type::Apply{name,..}=>name.clone(),other=>format!("{other:?}")}}
fn value_type(value:&crate::AST::CtValue)->Option<Type>{match value{crate::AST::CtValue::Bool(_)=>Some(Type::Bool),crate::AST::CtValue::Int(_)=>Some(Type::Int),crate::AST::CtValue::Char(_)=>Some(Type::Char),crate::AST::CtValue::Str(_)=>Some(Type::String),crate::AST::CtValue::Enum{type_name,args,..}if args.is_empty()=>Some(Type::Named(type_name.clone())),_=>None}}
fn normalized_value(value:&crate::AST::CtValue)->Option<Vec<u8>>{let mut out=Vec::new();match value{crate::AST::CtValue::Bool(v)=>{out.extend_from_slice(&[1,u8::from(*v)]);},crate::AST::CtValue::Int(v)=>{out.push(2);out.extend_from_slice(&v.to_be_bytes());},crate::AST::CtValue::Char(v)=>{out.push(3);out.extend_from_slice(&(*v as u32).to_be_bytes());},crate::AST::CtValue::Str(v)=>{out.push(4);out.extend_from_slice(&(v.len() as u64).to_be_bytes());out.extend_from_slice(v.as_bytes());},crate::AST::CtValue::Enum{type_name,variant,args}if args.is_empty()=>{out.push(5);for text in [type_name,variant]{out.extend_from_slice(&(text.len() as u64).to_be_bytes());out.extend_from_slice(text.as_bytes());}},_=>return None}Some(out)}

fn module_arg_expr(arg:&ModuleArg)->Option<Expr>{match arg{ModuleArg::Value(expr,_)=>Some(expr.clone()),ModuleArg::Type(Type::Named(name),span)=>Some(Expr::Ident(name.clone(),*span)),_=>None}}

fn resolve_args(
    alias: &ModuleAliasDef,
    template: &TemplateInfo,
    traits: &TraitRegistry,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, crate::AST::CtValue>,
    enums: &HashMap<String, bool>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Vec<ResolvedModuleArg>> {
    let mut out = Vec::new();
    for (param, arg) in template.params.iter().zip(&alias.args) {
        match param {
            ResolvedModuleParam::Invalid => return None,
            ResolvedModuleParam::Type { name, bound } => {
                let ty = match arg {
                    ModuleArg::Type(ty, _) => ty.clone(),
                    ModuleArg::Value(Expr::Ident(n, _), _) => Type::Named(n.clone()),
                    _ => {
                        diags.push(Diagnostic::error(
                            "E0852",
                            format!("type argument for `{name}` does not satisfy its module bound"),
                            "this slot resolves to a type parameter, but the argument is a value expression".into(),
                            "pass a type that satisfies the declared bound".into(),
                            Some(arg.span()),
                        ));
                        return None;
                    }
                };
                if let Some(bound) = bound {
                    let identity = type_name(&ty);
                    if !traits.implements_trait(&identity, bound) {
                        diags.push(Diagnostic::error(
                            "E0852",
                            format!("type argument `{identity}` does not satisfy `{bound}`"),
                            format!("generic module parameter `{name}` requires the `{bound}` bound"),
                            format!("pass a type that implements `{bound}`"),
                            Some(arg.span()),
                        ));
                        return None;
                    }
                }
                out.push(ResolvedModuleArg::Type(ty));
            }
            ResolvedModuleParam::Value { name, ty } => {
                let Some(expr) = module_arg_expr(arg) else {
                    diags.push(Diagnostic::error(
                        "E0853",
                        format!("value argument for `{name}` has the wrong type"),
                        format!("this slot requires an exact `{}` Tier-0 value", type_name(ty)),
                        format!("pass a compile-time `{}` value without conversion", type_name(ty)),
                        Some(arg.span()),
                    ));
                    return None;
                };
                let value = match crate::Comptime::evaluate(
                    &expr,
                    funcs,
                    &HashSet::new(),
                    Path::new("."),
                    globals,
                ) {
                    Ok(value) => value,
                    Err(_) => {
                        diags.push(Diagnostic::error(
                            "E0857",
                            format!("value argument for `{name}` is not known at compile time"),
                            "generic module instances need one closed, deterministic Tier-0 value"
                                .to_string(),
                            format!("pass a literal or comptime `{}` value", type_name(ty)),
                            Some(arg.span()),
                        ));
                        return None;
                    }
                };
                let actual = value_type(&value);
                let allowed = matches!(ty, Type::Bool | Type::Int | Type::Char | Type::String)
                    || matches!(ty, Type::Named(n) if enums.get(n).copied() == Some(true));
                if !allowed || actual.as_ref() != Some(ty) {
                    diags.push(Diagnostic::error(
                        "E0853",
                        format!("value argument for `{name}` has the wrong type"),
                        format!(
                            "expected exact `{}`, found `{}`",
                            type_name(ty),
                            actual
                                .as_ref()
                                .map(type_name)
                                .unwrap_or_else(|| "non-Tier-0 value".into())
                        ),
                        format!("pass a compile-time `{}` value without conversion", type_name(ty)),
                        Some(arg.span()),
                    ));
                    return None;
                }
                let bytes = normalized_value(&value).expect("allowed Tier-0 value normalizes");
                out.push(ResolvedModuleArg::Value(value, bytes));
            }
        }
    }
    Some(out)
}

trait ModuleArgSpan{fn span(&self)->crate::Diagnostics::Span;}impl ModuleArgSpan for ModuleArg{fn span(&self)->crate::Diagnostics::Span{match self{ModuleArg::Type(_,s)|ModuleArg::Value(_,s)=>*s}}}

fn expand_alias(
    alias: &ModuleAliasDef,
    consumer_module: usize,
    application_module: &str,
    instance_full_key: &[u8],
    templates: &std::collections::HashMap<String, TemplateInfo>,
    diags: &mut Vec<Diagnostic>,
    traits:&TraitRegistry,
    funcs:&HashMap<String,&Func>,
    globals:&HashMap<String,crate::AST::CtValue>,
    enums:&HashMap<String,bool>,
    instances: &mut HashMap<ModuleInstanceKey, String>,
    fingerprints: &mut HashMap<String, Vec<u8>>,
    applications: &mut HashMap<String, Vec<crate::AST::ModuleInstanceApplication>>,
    resolved_args: Option<Vec<ResolvedModuleArg>>,
) -> Option<AliasExpansion> {
    let info = match templates.get(&alias.target) {
        Some(t) => t,
        None => {
            diags.push(Diagnostic::error(
                "E0850",
                format!("generic module `{}` not found in this scope", alias.target),
                "check the module template name and make sure it is defined in the same file"
                    .to_string(),
                format!("example: `module {} = MyTemplate<String>`", alias.name),
                Some(alias.target_span),
            ));
            return None;
        }
    };
    let template = &info.def;
    let source_items: &[Item] = if info.source_module == consumer_module {
        &[]
    } else {
        &info.source_items
    };
    if alias.args.len() != template.params.len() {
        diags.push(Diagnostic::error(
            "E0851",
            format!(
                "module alias `{}` passes {} argument(s) but `{}` expects {}",
                alias.name,
                alias.args.len(),
                alias.target,
                template.params.len(),
            ),
            "the number of type/value arguments must match the template parameter list".to_string(),
            format!(
                "example: `module {} = {}<{}>` with {} arg(s)",
                alias.name,
                alias.target,
                template
                    .params
                    .iter()
                    .map(GenericModuleParam::name)
                    .collect::<Vec<_>>()
                    .join(", "),
                template.params.len(),
            ),
            Some(alias.span),
        ));
        return None;
    }
    let resolved_args = match resolved_args {
        Some(args) => args,
        None => resolve_args(alias, info, traits, funcs, globals, enums, diags)?,
    };
    let mut type_args = HashMap::new();
    let mut value_args = HashMap::new();
    for (param, arg) in info.params.iter().zip(&resolved_args) {
        match (param, arg) {
            (ResolvedModuleParam::Type { name, .. }, ResolvedModuleArg::Type(ty)) => {
                type_args.insert(name.clone(), ty.clone());
            }
            (ResolvedModuleParam::Value { name, .. }, ResolvedModuleArg::Value(value, _)) => {
                value_args.insert(name.clone(), value.clone());
            }
            _ => {}
        }
    }
    let mut definition_types = HashMap::new();
    for item in source_items {
        let name = match item {
            Item::Struct(def) => Some(&def.name),
            Item::Enum(def) => Some(&def.name),
            Item::Trait(def) => Some(&def.name),
            Item::Tag(def) => Some(&def.name),
            _ => None,
        };
        if let Some(name) = name {
            definition_types.insert(name.clone(), Type::Named(module_type_name(&alias.name, name)));
        }
    }
    for item in &template.body {
        let name = match item {
            Item::Struct(def) => Some(&def.name),
            Item::Enum(def) => Some(&def.name),
            Item::Trait(def) => Some(&def.name),
            Item::Tag(def) => Some(&def.name),
            _ => None,
        };
        if let Some(name) = name {
            definition_types.insert(
                name.clone(),
                Type::Named(module_type_name(&alias.name, name)),
            );
        }
    }
    definition_types.extend(type_args.clone());
    for item in &template.body {
        let local = match item {
            Item::CodeModule(module) => Some(module.name.as_str()),
            Item::ModuleAlias(module) => Some(module.name.as_str()),
            _ => None,
        };
        if let Some(local) = local {
            definition_types.insert(
                local.to_string(),
                Type::Named(module_value_name(&alias.name, local)),
            );
        }
    }

    // Constants specialize in declaration order. Their evaluated definition-site
    // values are then available to later constants and every template function.
    let mut definition_values = if info.source_module == consumer_module {
        HashMap::new()
    } else {
        info.source_values.clone()
    };
    definition_values.extend(value_args);
    let mut declarations = Vec::new();
    for item in source_items {
        match item {
            Item::Struct(def) => declarations.push(Item::Struct(specialize_struct(
                def,
                &alias.name,
                &[],
                &[],
                &definition_types,
                &definition_values,
            ))),
            Item::Enum(def) => declarations.push(Item::Enum(specialize_enum(
                def,
                &alias.name,
                &[],
                &[],
                &definition_types,
                &definition_values,
            ))),
            Item::Trait(def) => declarations.push(Item::Trait(specialize_trait(
                def, &[], &[], &definition_types, &definition_values,
            ))),
            Item::Tag(def) => declarations.push(Item::Tag(specialize_tag(
                def, &definition_types, &definition_values,
            ))),
            Item::Impl(def) => declarations.push(Item::Impl(specialize_impl(
                def, &[], &[], &definition_types, &definition_values,
            ))),
            Item::ErrorConv(def) => declarations.push(Item::ErrorConv(specialize_error_conv(
                def, &definition_types, &definition_values,
            ))),
            _ => {}
        }
    }
    for item in &template.body {
        let Item::Const(source) = item else { continue };
        let mut value = source.value.clone();
        substitute_expr(&mut value, &definition_types, &definition_values);
        let evaluated = crate::Comptime::evaluate(
            &value,
            funcs,
            &HashSet::new(),
            Path::new("."),
            &definition_values,
        );
        if let Ok(evaluated) = evaluated {
            definition_values.insert(source.name.clone(), evaluated);
        }
        let mut meta = source.meta.clone();
        substitute_meta(&mut meta, &definition_types, &definition_values);
        declarations.push(Item::Const(crate::AST::ConstDef {
            span: source.span,
            name: module_value_name(&alias.name, &source.name),
            name_span: source.name_span,
            value,
            meta,
            attrs: source.attrs.clone(),
            rust_kind: source.rust_kind,
            is_comptime: source.is_comptime,
            ct: source.ct.clone(),
            ty: source.ty.as_ref().map(|ty| specialize_module_type(ty, &definition_types, &definition_values)),
            is_persist: source.is_persist,
            persist_span: source.persist_span,
            mutable: source.mutable,
            resolved_output: source.resolved_output.clone(),
        }));
    }
    for item in &template.body {
        if let Item::Struct(def) = item {
            declarations.push(Item::Struct(specialize_struct(
                def,
                &alias.name,
                &info.params,
                &resolved_args,
                &definition_types,
                &definition_values,
            )));
        }
        if let Item::Enum(def) = item {
            declarations.push(Item::Enum(specialize_enum(
                def,
                &alias.name,
                &info.params,
                &resolved_args,
                &definition_types,
                &definition_values,
            )));
        }
        if let Item::Trait(def) = item {
            declarations.push(Item::Trait(specialize_trait(
                def, &info.params, &resolved_args, &definition_types, &definition_values,
            )));
        }
        if let Item::Tag(def) = item {
            declarations.push(Item::Tag(specialize_tag(
                def, &definition_types, &definition_values,
            )));
        }
        if let Item::Impl(def) = item {
            declarations.push(Item::Impl(specialize_impl(
                def, &info.params, &resolved_args, &definition_types, &definition_values,
            )));
        }
        if let Item::ErrorConv(def) = item {
            declarations.push(Item::ErrorConv(specialize_error_conv(
                def, &definition_types, &definition_values,
            )));
        }
        if let Item::Test(def) = item {
            declarations.push(Item::Test(specialize_test(
                def, &alias.name, &definition_types, &definition_values,
            )));
        }
        if let Item::Bench(def) = item {
            declarations.push(Item::Bench(specialize_bench(
                def, &alias.name, &definition_types, &definition_values,
            )));
        }
        if let Item::CodeModule(module) = item {
            let lexical_path = module.name.clone();
            let mut module = specialize_nested_code_module(
                module,
                &info.params,
                &resolved_args,
                &definition_types,
                &definition_values,
            );
            module.name = module_value_name(&alias.name, &module.name);
            let nested_declarations = expand_nested_generics_in_code_module(
                &mut module,
                &lexical_path,
                consumer_module,
                application_module,
                instance_full_key,
                &definition_values,
                diags,
                instances,
                fingerprints,
                applications,
            );
            declarations.push(Item::CodeModule(module));
            declarations.extend(nested_declarations);
        }
    }

    let mut body: Vec<Item> = source_items
        .iter()
        .filter_map(|item| match item {
            Item::Func(func) => Some(Item::Func(specialize_func(
                func.clone(),
                &[],
                &[],
                &definition_types,
                &definition_values,
            ))),
            _ => None,
        })
        .collect();
    body.extend(template
        .body
        .iter()
        .filter_map(|item| match item {
            Item::Func(func) => Some(Item::Func(specialize_func(
                func.clone(),
                &info.params,
                &resolved_args,
                &definition_types,
                &definition_values,
            ))),
            Item::Const(_) => None,
            _ => None,
        })
        .collect::<Vec<_>>());

    // BODY1: nested generic templates close over the outer instance. Resolve
    // their aliases now, while the outer type/value environment is concrete.
    let nested_defs: Vec<GenericModuleDef> = template.body.iter().filter_map(|item| {
        let Item::GenericModule(def) = item else { return None };
        Some(specialize_nested_template_outer(def, &definition_types, &definition_values))
    }).collect();
    if !nested_defs.is_empty() {
        // Disposable bound-resolution registry, same rationale as
        // `expand_generic_module_aliases` above: register builtin hook
        // traits and swallow diags — the canonical per-module pass
        // re-validates every impl once these nested templates are expanded
        // into real module items.
        let mut nested_traits = TraitRegistry::default();
        nested_traits.register_synthetic_rollback();
        nested_traits.register_synthetic_display_debug();
        nested_traits.register_synthetic_close();
        nested_traits.register_synthetic_operators();
        nested_traits.register_synthetic_iter_index();
        nested_traits.register_synthetic_io();
        for def in &nested_defs { nested_traits.register_items(&def.body, &mut Vec::new()); }
        let nested_enums: HashMap<String, bool> = nested_defs.iter().flat_map(|def| def.body.iter()).filter_map(|item| {
            let Item::Enum(def) = item else { return None };
            Some((def.name.clone(), def.variants.iter().all(|v| matches!(v.payload, VariantPayload::Unit))))
        }).collect();
        let nested_funcs: HashMap<String, &Func> = nested_defs.iter().flat_map(|def| def.body.iter()).filter_map(|item| {
            let Item::Func(def) = item else { return None }; Some((def.name.clone(), def))
        }).collect();
        let enclosing_identity = crate::SHA256::sha256_hex(instance_full_key);
        let nested_templates: HashMap<String, TemplateInfo> = nested_defs.iter().map(|def| {
            let full_key = definition_full_key("nested", "", &enclosing_identity, &def.name);
            (def.name.clone(), TemplateInfo { def: def.clone(), definition_id: crate::SHA256::sha256_hex(&full_key), definition_full_key: full_key,
                params: resolve_params(def, &nested_traits, &nested_enums, diags),
                source_module: consumer_module, source_items: Vec::new(), source_values: definition_values.clone() })
        }).collect();
        let nested_alias_defs: Vec<ModuleAliasDef> = template.body.iter().filter_map(|item| {
            let Item::ModuleAlias(def) = item else { return None };
            Some(specialize_nested_alias_outer(def, &definition_types, &definition_values))
        }).collect();
        let nested_aliases: HashMap<String, &ModuleAliasDef> = nested_alias_defs.iter()
            .map(|def| (def.name.clone(), def)).collect();
        let mut ordered: Vec<&ModuleAliasDef> = nested_alias_defs.iter().collect();
        ordered.sort_by_key(|def| local_alias_depth(def, &nested_aliases));
        let mut nested_projections = HashMap::new();
        for nested_alias in ordered {
            let Some(mut resolved_alias) = resolve_local_alias(nested_alias, &nested_aliases, &nested_templates, diags) else { continue };
            resolved_alias.name = module_value_name(&alias.name, &nested_alias.name);
            let Some(nested_info) = nested_templates.get(&resolved_alias.target) else { continue };
            let Some(args) = resolve_args(
                &resolved_alias,
                nested_info,
                &nested_traits,
                &nested_funcs,
                &definition_values,
                &nested_enums,
                diags,
            ) else { continue };
            let key = instance_key(nested_info, &args, &HashMap::new());
            let fingerprint = crate::SHA256::sha256_hex(&key.bytes());
            let application = crate::AST::ModuleInstanceApplication {
                name: resolved_alias.name.clone(),
                source_module: application_module.to_string(),
                semantic_identity: format!("instance:{fingerprint}"),
                span: resolved_alias.name_span,
            };
            applications
                .entry(fingerprint.clone())
                .or_default()
                .push(application);
            if let Some(canonical) = instances.get(&key) {
                nested_projections.insert(
                    resolved_alias.name.clone(),
                    Type::Named(canonical.clone()),
                );
                continue;
            }
            if let Some(mut expansion) = expand_alias(&resolved_alias, consumer_module, application_module, &key.bytes(), &nested_templates,
                diags, &nested_traits, &nested_funcs, &definition_values, &nested_enums,
                instances, fingerprints, applications, Some(args)) {
                let identity = instance_identity(
                    &key,
                    nested_info,
                    &resolved_alias,
                    application_module,
                );
                register_instance_fingerprint(
                    fingerprints,
                    &identity,
                    resolved_alias.span,
                );
                expansion.module.instance_identity = Some(identity);
                instances.insert(key, resolved_alias.name.clone());
                declarations.push(Item::CodeModule(expansion.module));
                declarations.extend(expansion.declarations);
            }
        }
        if !nested_projections.is_empty() {
            for item in &mut body {
                let Item::Func(func) = item else { continue };
                for param in &mut func.params {
                    param.ty = crate::Generics::substitute_type(&param.ty, &nested_projections);
                }
                if let Some(ret) = &mut func.return_type {
                    *ret = crate::Generics::substitute_type(ret, &nested_projections);
                }
                substitute_stmts(&mut func.body, &nested_projections, &HashMap::new());
            }
        }
    }
    Some(AliasExpansion {
        module: CodeModule {
            name: alias.name.clone(),
            name_span: alias.name_span,
            is_pub: alias.is_pub,
            is_package_pub: alias.is_package_pub,
            body: Some(body),
            web_target: None,
            instance_identity: None,
            span: alias.span,
        },
        declarations,
    })
}

fn report_generic_module_cycles(items: &[Item], diags: &mut Vec<Diagnostic>) -> bool {
    fn collect_alias_edges(items: &[Item], edges: &mut Vec<(String, crate::Diagnostics::Span)>) {
        for item in items {
            match item {
                Item::ModuleAlias(alias) => {
                    edges.push((alias.target.clone(), alias.target_span));
                }
                Item::GenericModule(module) => collect_alias_edges(&module.body, edges),
                _ => {}
            }
        }
    }

    let mut graph: HashMap<String, Vec<(String, crate::Diagnostics::Span)>> = HashMap::new();
    for item in items {
        match item {
            Item::ModuleAlias(alias) => {
                graph
                    .entry(alias.name.clone())
                    .or_default()
                    .push((alias.target.clone(), alias.target_span));
            }
            Item::GenericModule(module) => {
                let mut edges = Vec::new();
                collect_alias_edges(&module.body, &mut edges);
                graph.insert(module.name.clone(), edges);
            }
            _ => {}
        }
    }
    for edges in graph.values_mut() {
        edges.sort_by(|a, b| a.0.cmp(&b.0));
    }

    fn visit(
        node: &str,
        graph: &HashMap<String, Vec<(String, crate::Diagnostics::Span)>>,
        state: &mut HashMap<String, u8>,
        stack: &mut Vec<String>,
        reported: &mut HashSet<String>,
        diags: &mut Vec<Diagnostic>,
    ) {
        state.insert(node.to_string(), 1);
        stack.push(node.to_string());
        if let Some(edges) = graph.get(node) {
            for (target, span) in edges {
                if !graph.contains_key(target) {
                    continue;
                }
                match state.get(target).copied().unwrap_or(0) {
                    0 => visit(target, graph, state, stack, reported, diags),
                    1 => {
                        let start = stack.iter().position(|name| name == target).unwrap_or(0);
                        let mut chain = stack[start..].to_vec();
                        chain.push(target.clone());
                        let text = chain.join(" -> ");
                        if reported.insert(text.clone()) {
                            diags.push(Diagnostic::error(
                                "E0855",
                                format!("generic module instantiation forms a cycle: {text}"),
                                "module aliases must form an acyclic dependency graph so specialization reaches one stable result".to_string(),
                                format!("break the cycle: {text}"),
                                Some(*span),
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        stack.pop();
        state.insert(node.to_string(), 2);
    }

    let mut nodes: Vec<String> = graph.keys().cloned().collect();
    nodes.sort();
    let mut state = HashMap::new();
    let mut stack = Vec::new();
    let mut reported = HashSet::new();
    for node in nodes {
        if state.get(&node).copied().unwrap_or(0) == 0 {
            visit(
                &node,
                &graph,
                &mut state,
                &mut stack,
                &mut reported,
                diags,
            );
        }
    }
    !reported.is_empty()
}

fn resolve_local_alias(
    alias: &ModuleAliasDef,
    aliases: &HashMap<String, &ModuleAliasDef>,
    templates: &HashMap<String, TemplateInfo>,
    diags: &mut Vec<Diagnostic>,
) -> Option<ModuleAliasDef> {
    let mut current = alias;
    while let Some(next) = aliases.get(&current.target).copied() {
        if !current.args.is_empty() {
            diags.push(Diagnostic::error(
                "E0851",
                format!(
                    "module alias `{}` passes {} argument(s) but alias `{}` expects 0",
                    current.name,
                    current.args.len(),
                    current.target
                ),
                "an alias-to-alias link reuses an already-bound module instance".to_string(),
                format!("remove the arguments and write `module {} = {}`", current.name, current.target),
                Some(current.span),
            ));
            return None;
        }
        current = next;
    }
    if !templates.contains_key(&current.target) {
        return Some(ModuleAliasDef {
            name: alias.name.clone(),
            name_span: alias.name_span,
            is_pub: alias.is_pub,
            is_package_pub: alias.is_package_pub,
            target: current.target.clone(),
            target_span: current.target_span,
            args: current.args.clone(),
            span: alias.span,
        });
    }
    Some(ModuleAliasDef {
        name: alias.name.clone(),
        name_span: alias.name_span,
        is_pub: alias.is_pub,
        is_package_pub: alias.is_package_pub,
        target: current.target.clone(),
        target_span: current.target_span,
        args: current.args.clone(),
        span: alias.span,
    })
}

fn local_alias_depth(alias: &ModuleAliasDef, aliases: &HashMap<String, &ModuleAliasDef>) -> usize {
    let mut depth = 0;
    let mut current = alias;
    while let Some(next) = aliases.get(&current.target).copied() {
        depth += 1;
        current = next;
    }
    depth
}

fn alias_chain_contains(
    alias: &ModuleAliasDef,
    aliases: &HashMap<String, &ModuleAliasDef>,
    names: &HashSet<String>,
) -> bool {
    let mut current = alias;
    while let Some(next) = aliases.get(&current.target).copied() {
        if names.contains(&next.name) {
            return true;
        }
        current = next;
    }
    false
}

/// D-GENMOD2=A: expand every `ModuleAlias` in each module's item list into a
/// concrete `CodeModule` using the corresponding `GenericModule` template.
/// Templates and aliases are removed from the item list after expansion.
pub(crate) fn expand_generic_module_aliases(
    bundle: &mut ProgramBundle,
    diags: &mut Vec<Diagnostic>,
) {
    let template_snapshots: Vec<HashMap<String, TemplateInfo>> = bundle
        .modules
        .iter()
        .enumerate()
        .map(|(source_module, module)| {
            // This registry only feeds `resolve_params`/`resolve_args` (does a
            // bound name exist, does a type implement it?) — it is NOT the
            // canonical per-module trait pass (that runs later, once, in the
            // main loop below with synthetic hooks pre-registered). Register
            // the same builtin hook traits here so a generic-module bound
            // like `T: Index` resolves, and throw the diagnostics away: the
            // canonical pass re-validates every impl block and is the only
            // place user-facing E0119/E0906/… should be reported. Without
            // this, every `impl T.Index`/`.Iterable`/`.Rollback`/`.Display`/
            // `.Debug` in the bundle spuriously fails E0119 here (empty trait
            // table) before the real pass ever runs.
            let mut traits = TraitRegistry::default();
            traits.register_synthetic_rollback();
            traits.register_synthetic_display_debug();
            traits.register_synthetic_close();
            traits.register_synthetic_operators();
            traits.register_synthetic_iter_index();
            traits.register_synthetic_io();
            traits.register_items(&module.items, &mut Vec::new());
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
                        if let Ok(value) = crate::Comptime::evaluate(
                            &def.value,
                            &funcs,
                            &HashSet::new(),
                            Path::new("."),
                            &source_values,
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
                        let package_identity = package_identity(bundle, package_root, dependency_name);
                        let module_path = module.path.strip_prefix(package_root).unwrap_or(&module.path).to_string_lossy().replace('\\', "/");
                        let full_key = definition_full_key(&package_identity, &module_path, "", &gm.name);
                        Some((
                            gm.name.clone(),
                            TemplateInfo {
                                def: gm.clone(),
                                definition_id: crate::SHA256::sha256_hex(&full_key),
                                definition_full_key: full_key,
                                params: resolve_params(gm, &traits, &enums, diags),
                                source_module,
                                source_items: clone_definition_items(&source_items),
                                source_values: source_values.clone(),
                            },
                        ))
                    }
                    _ => None,
                })
                .collect()
        })
        .collect();
    // `use alias.Item` has its own span and therefore no `import_targets`
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
                        .import_targets
                        .get(&(module_idx, import.span))
                        .copied()
                        .map(|target| (import.import_alias(), target))
                })
                .collect()
        })
        .collect();

    let mut bundle_instances: HashMap<ModuleInstanceKey, String> = HashMap::new();
    let mut bundle_instance_nominals: HashMap<String, Vec<String>> = HashMap::new();
    let mut fingerprint_keys: HashMap<String, Vec<u8>> = HashMap::new();
    let mut instance_applications: HashMap<String, Vec<crate::AST::ModuleInstanceApplication>> = HashMap::new();

    // Snapshot aliases up front — the mut loop below can't re-borrow `bundle.modules`
    // for an E0609 message that names the source module.
    let module_aliases: Vec<String> = bundle.modules.iter().map(|m| m.alias.clone()).collect();

    for (module_idx, module) in bundle.modules.iter_mut().enumerate() {
        if report_generic_module_cycles(&module.items, diags) {
            continue;
        }
        // Same disposable-registry rationale as the `template_snapshots` prepass
        // above: only used for bound resolution, never the diagnostic source of
        // truth — register the builtin hook traits and swallow its diags.
        let mut traits=TraitRegistry::default();
        traits.register_synthetic_rollback();
        traits.register_synthetic_display_debug();
        traits.register_synthetic_close();
        traits.register_synthetic_operators();
        traits.register_synthetic_iter_index();
        traits.register_synthetic_io();
        traits.register_items(&module.items,&mut Vec::new());
        let enums:HashMap<String,bool>=module.items.iter().filter_map(|item|if let Item::Enum(def)=item{Some((def.name.clone(),def.variants.iter().all(|v|matches!(v.payload,VariantPayload::Unit))))}else{None}).collect();
        let funcs:HashMap<String,&Func>=module.items.iter().filter_map(|item|if let Item::Func(f)=item{Some((f.name.clone(),f))}else{None}).collect();
        let mut globals:HashMap<String,crate::AST::CtValue>=HashMap::new();
        if module.items.iter().any(|item| matches!(item, Item::ModuleAlias(_))) {
            for item in &module.items {
                if let Item::Const(c)=item {
                    if let Ok(value)=crate::Comptime::evaluate(&c.value,&funcs,&HashSet::new(),Path::new("."),&globals) {
                        globals.insert(c.name.clone(),value);
                    }
                }
            }
        }
        // Parameter declarations were resolved once in the immutable prepass.
        // Reuse that result locally so invalid declarations emit one diagnostic.
        let mut templates = template_snapshots[module_idx].clone();
        let mut denied_templates = HashSet::new();

        for import in &mut module.imports {
            let ImportKind::Unqualified {
                module_alias,
                items,
                ..
            } = &mut import.kind
            else {
                continue;
            };
            let Some(source_idx) = import_bindings[module_idx].get(module_alias).copied() else {
                continue;
            };
            let mut consumed = HashSet::new();
            for (original, alias) in items.iter() {
                let Some(source) = template_snapshots[source_idx].get(original) else {
                    continue;
                };
                consumed.insert(original.clone());
                let local = alias.as_deref().unwrap_or(original);
                if !source.def.is_pub && !source.def.is_package_pub {
                    denied_templates.insert(local.to_string());
                    diags.push(Diagnostic::error(
                        "E0609",
                        format!("`{original}` is private in module `{}`", module_aliases[source_idx]),
                        "only `pub` items can be brought into scope with `use`".to_string(),
                        format!("add `pub` before `module {original}` in the defining file"),
                        Some(import.span),
                    ));
                    continue;
                }
                templates.insert(local.to_string(), source.clone());
            }
            // Generic templates are compile-time namespace inputs, not runtime
            // values for the ordinary unqualified-import pass below.
            items.retain(|(original, _)| !consumed.contains(original));
        }

        let aliases: HashMap<String, &ModuleAliasDef> = module
            .items
            .iter()
            .filter_map(|item| match item {
                Item::ModuleAlias(alias) => Some((alias.name.clone(), alias)),
                _ => None,
            })
            .collect();
        let type_aliases: HashMap<String, Type> = module.items.iter().filter_map(|item| {
            let Item::TypeAlias(alias) = item else { return None };
            Some((alias.name.clone(), alias.target.clone()))
        }).collect();
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
                        format!("generic module `{}` not found in this scope", resolved.target),
                        "check the module template name and make sure it is defined in the same file"
                            .to_string(),
                        format!("example: `module {} = MyTemplate<String>`", resolved.name),
                        Some(resolved.target_span),
                    ));
                    invalid_aliases.insert(alias.name.clone());
                    continue;
                };
                let Some(args) = resolve_args(&resolved, info, &traits, &funcs, &globals, &enums, diags) else {
                    invalid_aliases.insert(alias.name.clone());
                    continue;
                };
                let key = instance_key(info, &args, &type_aliases);
                let fingerprint = crate::SHA256::sha256_hex(&key.bytes());
                instance_applications.entry(fingerprint.clone())
                    .or_default().push(crate::AST::ModuleInstanceApplication {
                        name: resolved.name.clone(),
                        source_module: module.display.clone(),
                        semantic_identity: format!("instance:{fingerprint}"),
                        span: resolved.name_span,
                    });
                if let Some(canonical) = bundle_instances.get(&key) {
                    projections.insert(alias.name.clone(), canonical.clone());
                    continue;
                }
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
                    &enums,
                    &mut bundle_instances,
                    &mut fingerprint_keys,
                    &mut instance_applications,
                    Some(args),
                ) {
                    let identity = instance_identity(&key, info, &resolved, &module.display);
                    register_instance_fingerprint(&mut fingerprint_keys, &identity, alias.span);
                    cm.module.instance_identity = Some(identity);
                    bundle_instance_nominals.insert(alias.name.clone(), cm.declarations.iter().filter_map(|item| match item {
                        Item::Struct(def) => Some(def.name.clone()),
                        Item::Enum(def) => Some(def.name.clone()),
                        _ => None,
                    }).collect());
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
        for (idx, expansion) in expansions {
            module.items[idx] = Item::CodeModule(expansion.module);
            declarations.extend(expansion.declarations);
        }
        // Collapse forward-alias chains through the applicative canonical
        // instance selected above.
        for alias in projections.clone().keys() {
            let mut canonical = projections[alias].clone();
            let mut seen = HashSet::new();
            while seen.insert(canonical.clone()) {
                let Some(next) = projections.get(&canonical) else { break };
                canonical = next.clone();
            }
            projections.insert(alias.clone(), canonical);
        }
        // Resolve projected nominal spellings before registration/codegen. No
        // duplicate declaration or zero-parameter surface alias leaks out.
        let projection_types: HashMap<String, Type> = projections.iter().flat_map(|(alias, canonical)| {
            let prefix = module_type_prefix(canonical);
            bundle_instance_nominals.get(canonical).into_iter().flatten().filter_map(move |canonical_name| {
                canonical_name.strip_prefix(&prefix).map(|suffix| {
                    (module_type_name(alias, suffix), Type::Named(canonical_name.clone()))
                })
            })
        }).collect();
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
        debug_assert!(!module.items.iter().any(|item| matches!(item, Item::ModuleAlias(_))));
    }
    for module in &mut bundle.modules {
        for item in &mut module.items {
            let Item::CodeModule(instance) = item else { continue };
            let Some(identity) = &mut instance.instance_identity else { continue };
            if let Some(applications) = instance_applications.get(&identity.fingerprint) {
                identity.applications = applications.clone();
            }
        }
    }
}

#[cfg(test)]
mod instance_collision_tests {
    use super::*;

    fn identity_bundle(project_root: PathBuf) -> ProgramBundle {
        ProgramBundle {
            entry: 0,
            project_root,
            modules: Vec::new(),
            parse_teaching: Vec::new(),
            used_core: HashSet::new(),
            ffi_callback_fns: HashSet::new(),
            cffi: crate::AST::CFfi::default(),
            comptime_inputs: Vec::new(),
            import_targets: HashMap::new(),
            layer_ceiling: None,
            inferred_layer: crate::Syntax::RuntimeLayer::Core,
            web_partitions: HashMap::new(),
            web_partition_enforced: false,
            web_partition_report: None,
            dep_roots: HashMap::new(),
            active_os: crate::Syntax::OSTarget::host(),
            edition: "2027".to_string(),
        }
    }

    #[test]
    fn package_identity_uses_canonical_source_not_credentials_paths_or_formatting() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let base = std::env::temp_dir().join(format!("jet_package_identity_{nonce}"));
        let project_a = base.join("checkout-a/project");
        let project_b = base.join("checkout-b/project");
        let dep_a = base.join("private-a/dependency");
        let dep_b = base.join("private-b/dependency");
        for path in [&project_a, &project_b, &dep_a, &dep_b] { std::fs::create_dir_all(path).unwrap(); }
        std::fs::create_dir_all(project_a.join(".jet")).unwrap();
        std::fs::create_dir_all(project_b.join(".jet")).unwrap();
        std::fs::write(dep_a.join(crate::Syntax::PAYLOAD_FILE), "payload: { name: \"demo\", version: \"1.2.3\" }").unwrap();
        std::fs::write(dep_b.join(crate::Syntax::PAYLOAD_FILE), "payload: {\n  version: \"1.2.3\",\n  name: \"demo\"\n}\n").unwrap();
        std::fs::write(project_a.join(crate::Syntax::UNIFIED_LOCK_FILE), "version=1\n[[package]]\nname=\"demo\"\nversion=\"1.2.3\"\nsource={ git=\"https://alice:secret@example.com/acme/demo.git?token=one\", rev=\"main\" }\nlocked={ rev=\"abc\", tree-hash=\"tree\", last-modified=1 }\n").unwrap();
        std::fs::write(project_b.join(crate::Syntax::UNIFIED_LOCK_FILE), "version = 1\n\n[[package]]\nsource = { git = \"https://bob:other@example.com/acme/demo.git#credential\", rev = \"main\" }\nname = \"demo\"\nlocked = { tree-hash = \"tree\", rev = \"abc\", last-modified = 99 }\nversion = \"1.2.3\"\n").unwrap();
        let a = package_identity(&identity_bundle(project_a.clone()), &dep_a, Some("demo"));
        let b = package_identity(&identity_bundle(project_b.clone()), &dep_b, Some("demo"));
        assert_eq!(a, b, "formatting, credentials, timestamps, and host paths are non-semantic");
        std::fs::write(project_b.join(crate::Syntax::UNIFIED_LOCK_FILE), "version=1\n[[package]]\nname=\"demo\"\nversion=\"1.2.3\"\nsource={ git=\"https://example.com/acme/demo.git\", rev=\"main\" }\nlocked={ rev=\"different\", tree-hash=\"tree\" }\n").unwrap();
        let changed = package_identity(&identity_bundle(project_b), &dep_b, Some("demo"));
        assert_ne!(a, changed, "locked git revision is semantic package source identity");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn same_template_path_in_different_packages_has_distinct_definition_identity() {
        let root = std::env::temp_dir().join(format!("jet_package_nominal_{}", std::process::id()));
        let first = root.join("first");
        let second = root.join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join(crate::Syntax::PAYLOAD_FILE), "payload: { name: \"first\", version: \"1.0.0\" }").unwrap();
        std::fs::write(second.join(crate::Syntax::PAYLOAD_FILE), "payload: { name: \"second\", version: \"1.0.0\" }").unwrap();
        let bundle = identity_bundle(root.clone());
        let a = definition_full_key(&package_identity(&bundle, &first, Some("first")), "src/template.jet", "", "Boxed");
        let b = definition_full_key(&package_identity(&bundle, &second, Some("second")), "src/template.jet", "", "Boxed");
        assert_ne!(a, b);
        assert!(!String::from_utf8_lossy(&a).contains(&root.to_string_lossy().as_ref()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn path_lock_content_changes_definition_and_instance_identity_but_host_path_does_not() {
        let root = std::env::temp_dir().join(format!("jet_path_lock_identity_{}", std::process::id()));
        let project = root.join("project");
        let dependency = root.join("dependency");
        std::fs::create_dir_all(project.join(".jet")).unwrap();
        std::fs::create_dir_all(&dependency).unwrap();
        std::fs::write(dependency.join(crate::Syntax::PAYLOAD_FILE), "payload: { name: \"dep\", version: \"1.2.3\" }").unwrap();
        let lock = |path: &str, content: &str| format!("version=1\n[[package]]\nname=\"dep\"\nversion=\"1.2.3\"\nsource={{path=\"{path}\"}}\ncontent-hash=\"{content}\"\n");
        let bundle = identity_bundle(project.clone());
        std::fs::write(project.join(crate::Syntax::UNIFIED_LOCK_FILE), lock("/host/a/dep", "tree-a")).unwrap();
        let package_a = package_identity(&bundle, &dependency, Some("dep"));
        std::fs::write(project.join(crate::Syntax::UNIFIED_LOCK_FILE), lock("/host/b/dep", "tree-a")).unwrap();
        assert_eq!(package_a, package_identity(&bundle, &dependency, Some("dep")));
        std::fs::write(project.join(crate::Syntax::UNIFIED_LOCK_FILE), lock("/host/b/dep", "tree-b")).unwrap();
        let package_b = package_identity(&bundle, &dependency, Some("dep"));
        let definition_a = definition_full_key(&package_a, "template.jet", "", "Boxed");
        let definition_b = definition_full_key(&package_b, "template.jet", "", "Boxed");
        assert_ne!(crate::SHA256::sha256_hex(&definition_a), crate::SHA256::sha256_hex(&definition_b));
        let instance = |definition_full_key| ModuleInstanceKey { definition_full_key, parameters: vec![1], args: vec![vec![2]] };
        assert_ne!(crate::SHA256::sha256_hex(&instance(definition_a).bytes()), crate::SHA256::sha256_hex(&instance(definition_b).bytes()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[should_panic(expected = "internal compiler error: E0859 generic module instance fingerprint collision")]
    fn different_full_keys_with_same_digest_fail_closed_before_codegen() {
        let mut registry = HashMap::new();
        let make = |full_key| crate::AST::ModuleInstanceIdentity { full_key, fingerprint: "forced-digest".into(), definition_id: "def".into(), argument_keys: Vec::new(), template_span: Span::new(0, 0), applications: Vec::new() };
        let first = make(vec![1]);
        let second = make(vec![2]);
        register_instance_fingerprint(&mut registry, &first, Span::new(1, 2));
        register_instance_fingerprint(&mut registry, &second, Span::new(3, 4));
    }

    #[test]
    fn generated_nominal_names_encode_module_alias_boundaries() {
        assert_ne!(module_type_name("foo", "BarBaz"), module_type_name("foo_bar", "Baz"));
        assert_ne!(module_type_name("foo_bar", "Baz"), module_type_name("fo_obar", "Baz"));
        assert_eq!(module_type_name("_cache", "Item"), "_M5CacheItem");
    }

    #[test]
    fn generic_template_snapshot_never_filters_parser_admitted_items() {
        let source = r#"
module everything<T> {
    const answer = 42
    tag Marked { deny: [Net] }
    trait Show { fn show(self) => T }
    struct Boxed { value: T }
    enum Maybe { Empty Value(T) }
    impl Boxed.Show { fn show(self) => T { return self.value } }
    fn id(value: T) => T { return ~value }
    module nested { fn nested() {} }
    module inner<U> { fn inner(value: U) => U { return ~value } }
    module int_inner = inner<Int>
    #Test("smoke") { expect(answer == 42) }
    #Bench("work") { expect(answer == 42) }
}
fn run() {}
"#;
        let (tokens, lex) = crate::Lexer::lex(source);
        assert!(lex.is_empty(), "{lex:?}");
        let program = crate::Parser::parse(&tokens).expect("parser-admitted generic body");
        let template = program.items.iter().find_map(|item| match item {
            Item::GenericModule(template) => Some(template),
            _ => None,
        }).expect("generic template");
        let snapshot = template.clone();
        assert_eq!(snapshot.body.len(), template.body.len());
        assert_eq!(
            crate::CanonicalAST::canonical_fragment(&snapshot.body),
            crate::CanonicalAST::canonical_fragment(&template.body),
        );
    }
}
