use super::*;

mod Substitution;
mod Identity;

use Substitution::{substitute_expr, substitute_marker, substitute_markers, substitute_meta, substitute_stmts};
use Identity::{
    definition_full_key, instance_identity, parameter_bytes, register_instance_fingerprint,
    type_full_key,
};
// Budget specs and unit registration read package identity through this module,
// so the two identity helpers keep their reach at this level.
pub(in crate::Sema) use Identity::{owning_package, package_identity};

// ---------------------------------------------------------------------------
// D-CONF-GENSPELL1=A: generic module expansion (R11 pre-pass)
// ---------------------------------------------------------------------------
//
// `module string_cache32 :: lru<String>(32)` expands into a synthetic
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
    substitute_markers(&mut func.markers, &types, &values);
    for clause in func.pre.iter_mut().chain(func.post.iter_mut()) {
        substitute_expr(&mut clause.cond, &types, &values);
        substitute_expr(&mut clause.message_expr, &types, &values);
    }
    if let Some(every) = &mut func.every {
        if let crate::AST::EveryArg::Expression(expression) = &mut every.arg {
            substitute_expr(expression, &types, &values);
            every.resolved = None;
        }
    }
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
    substitute_markers(&mut func.markers, types, &values);
    for clause in func.pre.iter_mut().chain(func.post.iter_mut()) {
        substitute_expr(&mut clause.cond, types, &values);
        substitute_expr(&mut clause.message_expr, types, &values);
    }
    if let Some(every) = &mut func.every {
        if let crate::AST::EveryArg::Expression(expression) = &mut every.arg {
            substitute_expr(expression, types, &values);
            every.resolved = None;
        }
    }
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

pub(crate) fn module_type_name(alias: &str, name: &str) -> String {
    format!("{}{}", module_type_prefix(alias), name.trim_start_matches('_'))
}

fn module_value_name(alias: &str, name: &str) -> String {
    format!("{}_{}", alias.trim_end_matches('_'), name.trim_start_matches('_'))
}

/// Return the source-facing paths for declarations inside one expanded
/// instance. The AST keeps compiler-owned nominal/value names for registration
/// and codegen; the name ledger records these projections for diagnostics and
/// tooling. This is the one place that knows how generic-module specialization
/// turns a source member into its internal name.
pub(super) fn instance_display_paths(module: &CodeModule) -> Vec<(String, String)> {
    let mut paths = vec![(module.name.clone(), module.name.clone())];
    let Some(body) = &module.body else {
        return paths;
    };
    collect_instance_display_paths(
        &module.name,
        &module.name,
        &module.name,
        body,
        &mut paths,
    );
    paths
}

/// Return projections for the declarations that expansion places beside the
/// instance module in the containing module item list.
pub(super) fn top_level_instance_display_paths(
    instance: &CodeModule,
    items: &[Item],
) -> Vec<(String, String)> {
    let type_prefix = module_type_prefix(&instance.name);
    let value_prefix = format!("{}_", instance.name.trim_end_matches('_'));
    let selected: Vec<Item> = items
        .iter()
        .filter(|item| match item {
            Item::Struct(definition) => definition.name.starts_with(&type_prefix),
            Item::Enum(definition) => definition.name.starts_with(&type_prefix),
            Item::Trait(definition) => definition.name.starts_with(&type_prefix),
            Item::Tag(definition) => definition.name.starts_with(&type_prefix),
            Item::Const(definition) => definition.name.starts_with(&value_prefix),
            Item::Impl(implementation) => implementation.type_name.starts_with(&type_prefix),
            Item::CodeModule(child) => child.name.starts_with(&value_prefix),
            _ => false,
        })
        .cloned()
        .collect();
    let mut paths = Vec::new();
    collect_instance_display_paths(
        &instance.name,
        "",
        &instance.name,
        &selected,
        &mut paths,
    );
    paths
}

fn join_member_path(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

fn collect_instance_display_paths(
    scope_name: &str,
    internal_module_path: &str,
    display_module_path: &str,
    body: &[Item],
    paths: &mut Vec<(String, String)>,
) {
    let add = |paths: &mut Vec<(String, String)>, internal: String, display: String| {
        paths.push((internal, display));
    };
    let add_method = |paths: &mut Vec<(String, String)>, internal_owner: &str, display_owner: &str, name: &str| {
        add(
            paths,
            format!("{internal_owner}.{name}"),
            format!("{display_owner}.{name}"),
        );
    };

    for item in body {
        match item {
            Item::Struct(definition) => {
                let display_name = definition
                    .name
                    .strip_prefix(&module_type_prefix(scope_name))
                    .unwrap_or(&definition.name);
                let internal = join_member_path(internal_module_path, &definition.name);
                let display = format!("{display_module_path}.{display_name}");
                add(paths, internal.clone(), display.clone());
                for field in &definition.fields {
                    add(
                        paths,
                        format!("{internal}.{}", field.name),
                        format!("{display}.{}", field.name),
                    );
                }
                for method in &definition.methods {
                    add_method(paths, &internal, &display, &method.name);
                }
                for implementation in &definition.trait_impls {
                    for method in &implementation.methods {
                        add_method(paths, &internal, &display, &method.name);
                    }
                }
            }
            Item::Enum(definition) => {
                let display_name = definition
                    .name
                    .strip_prefix(&module_type_prefix(scope_name))
                    .unwrap_or(&definition.name);
                let internal = join_member_path(internal_module_path, &definition.name);
                let display = format!("{display_module_path}.{display_name}");
                add(paths, internal.clone(), display.clone());
                for variant in &definition.variants {
                    add(
                        paths,
                        format!("{internal}.{}", variant.name),
                        format!("{display}.{}", variant.name),
                    );
                }
                for method in &definition.methods {
                    add_method(paths, &internal, &display, &method.name);
                }
                for implementation in &definition.trait_impls {
                    for method in &implementation.methods {
                        add_method(paths, &internal, &display, &method.name);
                    }
                }
            }
            Item::Trait(definition) => {
                let display_name = definition
                    .name
                    .strip_prefix(&module_type_prefix(scope_name))
                    .unwrap_or(&definition.name);
                let internal = join_member_path(internal_module_path, &definition.name);
                let display = format!("{display_module_path}.{display_name}");
                add(paths, internal.clone(), display.clone());
                for method in &definition.methods {
                    add_method(paths, &internal, &display, &method.name);
                }
            }
            Item::Tag(definition) => {
                let display_name = definition
                    .name
                    .strip_prefix(&module_type_prefix(scope_name))
                    .unwrap_or(&definition.name);
                add(
                    paths,
                    join_member_path(internal_module_path, &definition.name),
                    format!("{display_module_path}.{display_name}"),
                );
            }
            Item::Const(definition) => {
                let display_name = definition
                    .name
                    .strip_prefix(&format!("{}_", scope_name.trim_end_matches('_')))
                    .unwrap_or(&definition.name);
                add(
                    paths,
                    join_member_path(internal_module_path, &definition.name),
                    format!("{display_module_path}.{display_name}"),
                );
            }
            Item::Func(function) => add(
                paths,
                join_member_path(internal_module_path, &function.name),
                format!("{display_module_path}.{}", function.name),
            ),
            Item::Impl(implementation) => {
                let internal = join_member_path(internal_module_path, &implementation.type_name);
                let display_name = implementation
                    .type_name
                    .strip_prefix(&module_type_prefix(scope_name))
                    .unwrap_or(&implementation.type_name);
                let display = format!("{display_module_path}.{display_name}");
                add(paths, internal.clone(), display.clone());
                for method in &implementation.methods {
                    add_method(paths, &internal, &display, &method.name);
                }
            }
            Item::CodeModule(child) => {
                let display_name = child
                    .name
                    .strip_prefix(&format!("{}_", scope_name.trim_end_matches('_')))
                    .unwrap_or(&child.name);
                let internal = join_member_path(internal_module_path, &child.name);
                let display = format!("{display_module_path}.{display_name}");
                add(paths, internal.clone(), display.clone());
                if let Some(body) = &child.body {
                    collect_instance_display_paths(
                        &child.name,
                        &internal,
                        &display,
                        body,
                        paths,
                    );
                }
            }
            _ => {}
        }
    }
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
    if let Some(expression) = &mut result.faults_expr {
        substitute_expr(expression, types, values);
    }
    if let Some(expression) = &mut result.expected_fail_expr {
        substitute_expr(expression, types, values);
    }
    result.expected_fail = false;
    result.faults.clear();
    for param in &mut result.params {
        param.ty = specialize_module_type(&param.ty, types, values);
        if let Some(default) = &mut param.default { substitute_expr(default, types, values); }
    }
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
            Type::FixedList { elem, len } => {
                lengths(elem, types, values);
                *len = len.resolve_symbols(&|name| {
                    values.get(name).and_then(|value| match value {
                        crate::AST::CtValue::Int(value) => u64::try_from(*value).ok(),
                        _ => None,
                    })
                });
            }
            Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => lengths(inner, types, values),
            Type::Map { key, value, .. } => { lengths(key, types, values); lengths(value, types, values); }
            Type::Result { ok, err } => { lengths(ok, types, values); lengths(err, types, values); }
            Type::Fn { params, ret, .. } => { for param in params { lengths(param, types, values); } if let Some(ret) = ret { lengths(ret, types, values); } }
            Type::Apply { name, args } => {
                if matches!(name.as_str(), "Vec" | "Matrix") {
                    for argument in args.iter_mut() {
                        if let Type::Measure(measure) = argument {
                            *measure = measure.resolve_symbols(&|symbol| {
                                values.get(symbol).and_then(|value| match value {
                                    crate::AST::CtValue::Int(value) => u64::try_from(*value).ok(),
                                    _ => None,
                                })
                            });
                        }
                    }
                }
                args.iter_mut().for_each(|arg| lengths(arg, types, values));
            }
            Type::Tuple(fields) => fields.iter_mut().for_each(|(_, ty)| lengths(ty, types, values)),
            Type::Tagged { marker, inner } => {
                // Only a user-written tag name (D-QUAL4) can coincide with a
                // generic type-parameter name; an `Internal` fact never is one.
                if let crate::AST::TagMarker::User(name) = marker {
                    if let Some(Type::Named(mapped)) = types.get(name) {
                        *marker = crate::AST::TagMarker::User(mapped.clone());
                    }
                }
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
            substitute_marker(marker, &types, definition_values);
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
            compiler_generated: block.compiler_generated,
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
    substitute_markers(&mut result.serde_markers, &types, definition_values);
    substitute_markers(&mut result.type_markers, &types, definition_values);
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
            substitute_markers(&mut serde_markers, &types, definition_values);
            crate::AST::Variant {
                name: variant.name.clone(),
                name_span: variant.name_span,
                payload,
                discriminant: variant.discriminant,
                discriminant_expr: variant.discriminant_expr.clone().map(|mut expr| {
                    substitute_expr(&mut expr, &types, definition_values);
                    expr
                }),
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
            compiler_generated: block.compiler_generated,
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
    substitute_markers(&mut result.serde_markers, &types, definition_values);
    substitute_markers(&mut result.type_markers, &types, definition_values);
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
    source_rule_facts: Vec<crate::AST::AppliedRuleApplication>,
    build_facts: jet_foundation::Facts::BuildFactSnapshot,
}

fn collect_generic_module_spans(
    items: &[Item],
    spans: &mut Vec<crate::Diagnostics::Span>,
) {
    for item in items {
        match item {
            Item::GenericModule(def) => {
                spans.push(def.span);
                collect_generic_module_spans(&def.body, spans);
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    collect_generic_module_spans(body, spans);
                }
            }
            _ => {}
        }
    }
}

fn specialize_rule_facts(
    facts: &[crate::AST::AppliedRuleApplication],
    template: &GenericModuleDef,
    types: &HashMap<String, Type>,
    values: &HashMap<String, crate::AST::CtValue>,
) -> Vec<crate::AST::AppliedRuleApplication> {
    let mut nested_spans = Vec::new();
    collect_generic_module_spans(&template.body, &mut nested_spans);
    facts
        .iter()
        .filter(|application| {
            let span = application.marker.span;
            span.start >= template.span.start
                && span.end <= template.span.end
                && !nested_spans
                    .iter()
                    .any(|nested| span.start >= nested.start && span.end <= nested.end)
        })
        .cloned()
        .map(|mut application| {
            substitute_marker(&mut application.marker, types, values);
            application
        })
        .collect()
}

fn clone_definition_items(items: &[Item]) -> Vec<Item> {
    items
        .iter()
        .filter(|item| matches!(item, Item::Func(_) | Item::Struct(_) | Item::Enum(_)
            | Item::Trait(_) | Item::Tag(_) | Item::Impl(_) | Item::ErrorConv(_)
            | Item::Test(_) | Item::Const(_)))
        .cloned()
        .collect()
}

struct AliasExpansion {
    module: CodeModule,
    declarations: Vec<Item>,
    rule_facts: Vec<crate::AST::AppliedRuleApplication>,
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
    build_facts: &jet_foundation::Facts::BuildFactSnapshot,
    source_rule_facts: &[crate::AST::AppliedRuleApplication],
    generated_rule_facts: &mut Vec<crate::AST::AppliedRuleApplication>,
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
            build_facts,
            source_rule_facts,
            generated_rule_facts,
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
    traits.register_synthetic_driver();
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
            crate::Comptime::evaluate_closed_value(
                &def.value,
                &funcs,
                &HashSet::new(),
                Path::new("."),
                &values,
                build_facts,
            )
            .ok()
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
            params: resolve_params(def, &enums, diags),
            source_module: consumer_module,
            source_items: Vec::new(),
            source_values: values.clone(),
            source_rule_facts: source_rule_facts.to_vec(),
            build_facts: build_facts.clone(),
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
                format!("example: `module {} :: MyTemplate<String>`", local_name),
                Some(resolved.target_span),
            ));
            invalid_aliases.insert(nested_alias.name.clone());
            continue;
        };
        let Some(args) = resolve_args(&resolved, info, &traits, &funcs, &values, build_facts, &enums, diags) else {
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
            let identity_args = args.clone();
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
                build_facts,
                &enums,
                instances,
                fingerprints,
                applications,
                Some(args),
            ) else { continue };
            let identity = instance_identity(&key, info, &resolved, application_module, &identity_args);
            register_instance_fingerprint(fingerprints, &identity, resolved.span);
            expansion.module.instance_identity = Some(identity);
            instances.insert(key, resolved.name.clone());
            declarations.push(Item::CodeModule(expansion.module));
            declarations.extend(expansion.declarations);
            resolved.name.clone()
        };
        call_projections.insert(local_name.clone(), canonical.clone());
        type_projections.insert(local_name.clone(), Type::Named(canonical.clone()));
        type_projections.insert(resolved.name.clone(), Type::Named(canonical.clone()));
        // Nested aliases expose the same source-facing nominal-member paths
        // as top-level instances (`closed.Item`). Their declarations are
        // generated from this template under the canonical alias prefix.
        for item in &info.def.body {
            let Some(name) = (match item {
                Item::Struct(def) => Some(&def.name),
                Item::Enum(def) => Some(&def.name),
                _ => None,
            }) else {
                continue;
            };
            let generated = module_type_name(&canonical, name);
            let resolved_type = Type::Named(generated);
            type_projections.insert(format!("{local_name}.{name}"), resolved_type.clone());
            type_projections.insert(format!("{}.{name}", resolved.name), resolved_type);
        }
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
    enums: &HashMap<String, bool>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<ResolvedModuleParam> {
    def.params
        .iter()
        .map(|param| match param {
            GenericModuleParam::Type { name, bound, .. } => ResolvedModuleParam::Type {
                name: name.clone(),
                bound: match bound {
                    Some(Type::Named(bound)) => Some(bound.clone()),
                    _ => None,
                },
            },
            GenericModuleParam::Value {
                name,
                name_span,
                ty,
            } => {
                let allowed = matches!(ty, Type::Bool | Type::Int | Type::Char | Type::String)
                    || matches!(ty, Type::Named(n) if enums.get(n).copied() == Some(true));
                if allowed {
                    ResolvedModuleParam::Value {
                        name: name.clone(),
                        ty: ty.clone(),
                    }
                } else {
                    diags.push(Diagnostic::error(
                        "E0856",
                        format!(
                            "generic module value parameter `{name}` uses unsupported type `{}`",
                            type_name(ty)
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
    build_facts: &jet_foundation::Facts::BuildFactSnapshot,
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
                let value = match crate::Comptime::evaluate_closed_value(
                    &expr,
                    funcs,
                    &HashSet::new(),
                    Path::new("."),
                    globals,
                    build_facts,
                ) {
                    Ok(value) => value,
                    Err(_) => {
                        diags.push(Diagnostic::error(
                            "E0857",
                            format!("value argument for `{name}` is not known at compile time"),
                            "generic module instances need one closed, deterministic Tier-0 value"
                                .to_string(),
                            format!("pass a literal, a `@build.*` fact, or a comptime `{}` value", type_name(ty)),
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
    build_facts:&jet_foundation::Facts::BuildFactSnapshot,
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
                format!("example: `module {} :: MyTemplate<String>`", alias.name),
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
                "example: `module {} :: {}<{}>` with {} arg(s)",
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
        None => resolve_args(alias, info, traits, funcs, globals, build_facts, enums, diags)?,
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
    // Definition-site comptime bindings belong to the template, including
    // same-file templates. Keep them alive through expansion; registration
    // must not be the first phase that can see an earlier `@` binding.
    let mut definition_values = info.source_values.clone();
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
        // Keep the folded value on the generated declaration. Expansion runs
        // before ordinary item registration, so dropping this result leaves
        // the later comptime context unable to resolve an earlier `@` binding.
        let evaluated = source.ct.clone().or_else(|| {
            crate::Comptime::evaluate_closed_value(
                &value,
                funcs,
                &HashSet::new(),
                Path::new("."),
                &definition_values,
                build_facts,
            )
            .ok()
        });
        if let Some(evaluated) = &evaluated {
            definition_values.insert(source.name.clone(), evaluated.clone());
        }
        let mut meta = source.meta.clone();
        substitute_meta(&mut meta, &definition_types, &definition_values);
        let ty = source
            .ty
            .as_ref()
            .map(|ty| specialize_module_type(ty, &definition_types, &definition_values))
            .or_else(|| evaluated.as_ref().map(crate::AST::CtValue::jet_type));
        declarations.push(Item::Const(crate::AST::ConstDef {
            span: source.span,
            name: module_value_name(&alias.name, &source.name),
            name_span: source.name_span,
            value,
            meta,
            attrs: source.attrs.clone(),
            rust_kind: source.rust_kind,
            is_comptime: source.is_comptime,
            ct: evaluated,
            ty,
            is_persist: source.is_persist,
            persist_span: source.persist_span,
            mutable: source.mutable,
            resolved_output: source.resolved_output.clone(),
        }));
    }
    let mut rule_facts = specialize_rule_facts(
        &info.source_rule_facts,
        template,
        &definition_types,
        &definition_values,
    );
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
                &info.build_facts,
                &info.source_rule_facts,
                &mut rule_facts,
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
        nested_traits.register_synthetic_driver();
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
                                params: resolve_params(def, &nested_enums, diags),
                source_module: consumer_module, source_items: Vec::new(), source_values: definition_values.clone(),
                source_rule_facts: info.source_rule_facts.clone(),
                build_facts: info.build_facts.clone() })
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
                &info.build_facts,
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
            let identity_args = args.clone();
            if let Some(mut expansion) = expand_alias(&resolved_alias, consumer_module, application_module, &key.bytes(), &nested_templates,
                diags, &nested_traits, &nested_funcs, &definition_values, &info.build_facts, &nested_enums,
                instances, fingerprints, applications, Some(args)) {
                let identity = instance_identity(
                    &key,
                    nested_info,
                    &resolved_alias,
                    application_module,
                    &identity_args,
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
                rule_facts.extend(expansion.rule_facts);
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
            imports: template.imports.clone(),
            web_target: None,
            instance_identity: None,
            span: alias.span,
        },
        declarations,
        rule_facts,
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
                format!("remove the arguments and write `module {} :: {}`", current.name, current.target),
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

/// D-CONF-GENSPELL1=A: expand every `ModuleAlias` in each module's item list into a
/// concrete `CodeModule` using the corresponding `GenericModule` template.
/// Templates and aliases are removed from the item list after expansion.
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
                        let package_identity = package_identity(bundle, package_root, dependency_name);
                        let module_path = module.path.strip_prefix(package_root).unwrap_or(&module.path).to_string_lossy().replace('\\', "/");
                        let full_key = definition_full_key(&package_identity, &module_path, "", &gm.name);
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
    let mut instance_applications: HashMap<String, Vec<crate::AST::ModuleInstanceApplication>> = HashMap::new();

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
        let mut traits=TraitRegistry::default();
        traits.register_synthetic_rollback();
        traits.register_synthetic_display_debug();
        traits.register_synthetic_close();
        traits.register_synthetic_operators();
        traits.register_synthetic_iter_index();
        traits.register_synthetic_io();
        traits.register_synthetic_driver();
        traits.register_items(&module.items,&mut Vec::new());
        let enums:HashMap<String,bool>=module.items.iter().filter_map(|item|if let Item::Enum(def)=item{Some((def.name.clone(),def.variants.iter().all(|v|matches!(v.payload,VariantPayload::Unit))))}else{None}).collect();
        let funcs:HashMap<String,&Func>=module.items.iter().filter_map(|item|if let Item::Func(f)=item{Some((f.name.clone(),f))}else{None}).collect();
        let mut globals:HashMap<String,crate::AST::CtValue>=HashMap::new();
        if module.items.iter().any(|item| matches!(item, Item::ModuleAlias(_))) {
            for item in &module.items {
                if let Item::Const(c)=item {
                    if let Ok(value)=crate::Comptime::evaluate_closed_value(
                        &c.value,
                        &funcs,
                        &HashSet::new(),
                        Path::new("."),
                        &globals,
                        &build_facts,
                    ) {
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
                ..
            } = &import.kind
            else {
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
                        format!("`{original}` is private in module `{}`", module_aliases[source_idx]),
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
                        format!("example: `module {} :: MyTemplate<String>`", resolved.name),
                        Some(resolved.target_span),
                    ));
                    invalid_aliases.insert(alias.name.clone());
                    continue;
                };
                let Some(args) = resolve_args(&resolved, info, &traits, &funcs, &globals, &build_facts, &enums, diags) else {
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
                    let identity = instance_identity(&key, info, &resolved, &module.display, &identity_args);
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
                let Some(next) = projections.get(&canonical) else { break };
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
/// projection (`top_level_instance_display_paths`), no engine change. Generic
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
            if code_module.instance_identity.is_some() {
                continue;
            }
            let module_name = code_module.name.clone();
            let Some(body) = &mut code_module.body else {
                continue;
            };
            let mut member_types: HashMap<String, Type> = HashMap::new();
            for inner in body.iter() {
                let (name, is_pub) = match inner {
                    Item::Struct(def) => (&def.name, def.is_pub || def.is_package_pub),
                    Item::Enum(def) => (&def.name, def.is_pub || def.is_package_pub),
                    Item::Trait(def) => (&def.name, def.is_pub || def.is_package_pub),
                    Item::Tag(def) => (&def.name, def.is_pub || def.is_package_pub),
                    _ => continue,
                };
                let resolved = Type::Named(module_type_name(&module_name, name));
                member_types.insert(name.clone(), resolved.clone());
                member_types.insert(format!("{module_name}.{name}"), resolved.clone());
                if is_pub {
                    exported.insert(format!("{module_name}.{name}"), resolved);
                }
            }
            if member_types.is_empty() {
                continue;
            }
            let values: HashMap<String, crate::AST::CtValue> = HashMap::new();
            let mut kept = Vec::with_capacity(body.len());
            for inner in std::mem::take(body) {
                match inner {
                    Item::Struct(def) => declarations.push(Item::Struct(specialize_struct(
                        &def,
                        &module_name,
                        &[],
                        &[],
                        &member_types,
                        &values,
                    ))),
                    Item::Enum(def) => declarations.push(Item::Enum(specialize_enum(
                        &def,
                        &module_name,
                        &[],
                        &[],
                        &member_types,
                        &values,
                    ))),
                    Item::Trait(def) => declarations.push(Item::Trait(specialize_trait(
                        &def,
                        &[],
                        &[],
                        &member_types,
                        &values,
                    ))),
                    Item::Tag(def) => {
                        declarations.push(Item::Tag(specialize_tag(&def, &member_types, &values)))
                    }
                    // An `impl` block belongs beside the type it implements.
                    Item::Impl(def) => declarations.push(Item::Impl(specialize_impl(
                        &def,
                        &[],
                        &[],
                        &member_types,
                        &values,
                    ))),
                    // Member callables keep their own name (mangling happens at
                    // registration); only the member type spellings inside them move.
                    Item::Func(def) => kept.push(Item::Func(specialize_func(
                        def,
                        &[],
                        &[],
                        &member_types,
                        &values,
                    ))),
                    Item::Const(mut def) => {
                        substitute_expr(&mut def.value, &member_types, &values);
                        def.ty = def
                            .ty
                            .as_ref()
                            .map(|ty| specialize_module_type(ty, &member_types, &values));
                        kept.push(Item::Const(def));
                    }
                    other => kept.push(other),
                }
            }
            *body = kept;
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

#[cfg(test)]
mod InstanceCollisionTests;
