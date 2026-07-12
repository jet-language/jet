//! D-METAREFLECT1 / D-REFLECT1: build comptime reflection handles for user derives.
//!
//! `T.reflect()` in a derive body receives a `TypeInfo` value whose `.fields`,
//! `.methods`, `.type_params`, and `.markers` expose the target type's shape.

use crate::AST::{EnumDef, Field, Func, Marker, StructDef, TypeParam, VariantPayload};

use super::Value::CtValue;

#[derive(Debug, Clone, Default)]
pub struct ProgramSemanticFacts {
    pub effects: std::collections::HashMap<String, Vec<String>>,
    pub reaches_panic: std::collections::BTreeSet<String>,
}

fn identity(module: &str, symbol: &str) -> String {
    format!("{module}::{symbol}")
}

fn ct_str(s: impl Into<String>) -> CtValue {
    CtValue::Str(s.into())
}

fn ct_bool(b: bool) -> CtValue {
    CtValue::Bool(b)
}

fn ct_list(xs: Vec<CtValue>) -> CtValue {
    CtValue::List(xs)
}

fn ct_struct(type_name: &str, fields: &[(&str, CtValue)]) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: fields
            .into_iter()
            .map(|(n, v)| (n.to_string(), v.clone()))
            .collect(),
    }
}

fn marker_names(markers: &[Marker]) -> Vec<CtValue> {
    markers.iter().map(|m| ct_str(m.name.clone())).collect()
}

fn format_param(name: &str, ty: &crate::AST::Type) -> String {
    format!("{name}: {}", ty.name())
}

fn format_method_sig(method: &Func) -> String {
    let params = method
        .params
        .iter()
        .map(|p| format_param(&p.name, &p.ty))
        .collect::<Vec<_>>()
        .join(", ");
    match &method.return_type {
        Some(ret) => format!("fn {}({}) -> {}", method.name, params, ret.name()),
        None => format!("fn {}({})", method.name, params),
    }
}

/// One reflected struct field (D-METAREFLECT1).
pub fn build_field_info(field: &Field) -> CtValue {
    ct_struct(
        "FieldInfo",
        &[
            ("name", ct_str(field.name.clone())),
            ("ty", ct_str(field.ty.name())),
            ("markers", ct_list(marker_names(&field.serde_markers))),
            ("is_pub", ct_bool(field.is_pub)),
        ],
    )
}

/// One reflected inherent method (D-REFLECT1).
pub fn build_method_info(method: &Func) -> CtValue {
    let param_strs = method
        .params
        .iter()
        .map(|p| ct_str(format_param(&p.name, &p.ty)))
        .collect();
    ct_struct(
        "MethodInfo",
        &[
            ("name", ct_str(method.name.clone())),
            (
                "return_type",
                ct_str(
                    method
                        .return_type
                        .as_ref()
                        .map(|t| t.name())
                        .unwrap_or_else(|| "Unit".to_string()),
                ),
            ),
            ("params", ct_list(param_strs)),
            ("signature", ct_str(format_method_sig(method))),
            ("markers", ct_list(Vec::new())),
            ("is_pub", ct_bool(method.is_pub)),
        ],
    )
}

/// One reflected type parameter (D-REFLECT1).
pub fn build_type_param_info(param: &TypeParam) -> CtValue {
    ct_struct(
        "TypeParamInfo",
        &[
            ("name", ct_str(param.name.clone())),
            (
                "bounds",
                ct_list(param.bounds.iter().map(|b| ct_str(b.clone())).collect()),
            ),
        ],
    )
}

fn type_level_markers(s: &StructDef) -> Vec<CtValue> {
    let mut names: Vec<String> = s
        .type_markers
        .iter()
        .chain(s.serde_markers.iter())
        .map(|m| m.name.clone())
        .collect();
    for (derive, _) in &s.derives {
        names.push(derive.clone());
    }
    names.sort();
    names.dedup();
    names.into_iter().map(ct_str).collect()
}

/// Build the `TypeInfo` handle passed into a user derive body for `struct` targets.
pub fn build_struct_type_info(s: &StructDef) -> CtValue {
    let fields_info: Vec<CtValue> = s.fields.iter().map(build_field_info).collect();
    let methods_info: Vec<CtValue> = s.methods.iter().map(build_method_info).collect();
    let type_params_info: Vec<CtValue> = s.type_params.iter().map(build_type_param_info).collect();
    ct_struct(
        "TypeInfo",
        &[
            ("name", ct_str(s.name.clone())),
            (
                "span",
                ct_struct(
                    crate::Syntax::TYPE_SOURCE_SPAN,
                    &[
                        ("start", CtValue::Int(s.name_span.start as i64)),
                        ("end", CtValue::Int(s.name_span.end as i64)),
                    ],
                ),
            ),
            ("fields", ct_list(fields_info)),
            ("methods", ct_list(methods_info)),
            ("type_params", ct_list(type_params_info)),
            ("markers", ct_list(type_level_markers(s))),
            (
                "implements",
                ct_list(
                    s.trait_impls
                        .iter()
                        .map(|implementation| ct_str(implementation.trait_name.clone()))
                        .collect(),
                ),
            ),
        ],
    )
}

fn qualify_info(mut info: CtValue, module: &str, symbol: &str, kind: &str) -> CtValue {
    if let CtValue::Struct { fields, .. } = &mut info {
        fields.push(("module".to_string(), ct_str(module)));
        fields.push(("identity".to_string(), ct_str(identity(module, symbol))));
        fields.push(("kind".to_string(), ct_str(kind)));
    }
    info
}

fn qualified_method_info(method: &Func, module: &str, owner: &str) -> CtValue {
    let mut info = build_method_info(method);
    if let CtValue::Struct { fields, .. } = &mut info {
        fields.push(("module".to_string(), ct_str(module)));
        fields.push(("identity".to_string(), ct_str(format!("{module}::{owner}.{}", method.name))));
    }
    info
}

fn build_enum_type_info(def: &EnumDef, module: &str) -> CtValue {
    let variants = def.variants.iter().map(|variant| {
        let ty = match &variant.payload {
            VariantPayload::Unit => "Unit".to_string(),
            VariantPayload::Single(ty, _) => ty.name(),
            VariantPayload::Named(fields) => format!("{{{}}}", fields.iter().map(|field| format!("{}: {}", field.name, field.ty.name())).collect::<Vec<_>>().join(", ")),
        };
        ct_struct("FieldInfo", &[
            ("name", ct_str(variant.name.clone())),
            ("ty", ct_str(ty)),
            ("markers", ct_list(marker_names(&variant.serde_markers))),
            ("is_pub", ct_bool(def.is_pub)),
        ])
    }).collect();
    let methods = def.methods.iter().map(|method| qualified_method_info(method, module, &def.name)).collect();
    let params = def.type_params.iter().map(build_type_param_info).collect();
    let mut markers = def.type_markers.iter().chain(def.serde_markers.iter()).map(|marker| ct_str(marker.name.clone())).collect::<Vec<_>>();
    markers.extend(def.derives.iter().map(|(name, _)| ct_str(name.clone())));
    qualify_info(ct_struct("TypeInfo", &[
        ("name", ct_str(def.name.clone())),
        ("span", ct_struct(crate::Syntax::TYPE_SOURCE_SPAN, &[("start", CtValue::Int(def.name_span.start as i64)), ("end", CtValue::Int(def.name_span.end as i64))])),
        ("fields", ct_list(variants)),
        ("methods", ct_list(methods)),
        ("type_params", ct_list(params)),
        ("markers", ct_list(markers)),
        ("implements", ct_list(def.trait_impls.iter().map(|implementation| ct_str(implementation.trait_name.clone())).collect())),
    ]), module, &def.name, "enum")
}

/// D-METADEPTH2: read-only, post-sema whole-program snapshot handed only to
/// selected root `fn build`. Existing TypeInfo builders remain canonical.
pub fn build_program_info(
    bundle: &crate::AST::ProgramBundle,
    facts: &ProgramSemanticFacts,
) -> CtValue {
    let mut external_impls = std::collections::HashMap::<(String, String), (Vec<String>, Vec<CtValue>)>::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let crate::AST::Item::Impl(implementation) = item {
                let entry = external_impls.entry((module.alias.clone(), implementation.type_name.clone())).or_default();
                if let Some(trait_name) = &implementation.trait_name {
                    entry.0.push(trait_name.clone());
                }
                entry.1.extend(implementation.methods.iter().map(|method| qualified_method_info(method, &module.alias, &implementation.type_name)));
            }
        }
    }
    let mut types = Vec::new();
    let mut functions = Vec::new();
    let mut packages = Vec::new();
    for module in &bundle.modules {
        let mut package_types = Vec::new();
        let mut package_functions = Vec::new();
        for item in &module.items {
            match item {
                crate::AST::Item::Struct(def) => {
                    let mut info = qualify_info(build_struct_type_info(def), &module.alias, &def.name, "struct");
                    if let CtValue::Struct { fields, .. } = &mut info {
                        if let Some((_, CtValue::List(methods))) = fields.iter_mut().find(|(name, _)| name == "methods") {
                            *methods = def.methods.iter().map(|method| qualified_method_info(method, &module.alias, &def.name)).collect();
                        }
                    }
                    if let Some((traits, methods)) = external_impls.get(&(module.alias.clone(), def.name.clone())) {
                        if let CtValue::Struct { fields, .. } = &mut info {
                            if let Some((_, CtValue::List(values))) = fields.iter_mut().find(|(name, _)| name == "implements") {
                                values.extend(traits.iter().cloned().map(ct_str));
                            }
                            if let Some((_, CtValue::List(values))) = fields.iter_mut().find(|(name, _)| name == "methods") {
                                values.extend(methods.iter().cloned());
                            }
                        }
                    }
                    types.push(info.clone());
                    package_types.push(info);
                }
                crate::AST::Item::Enum(def) => {
                    let mut info = build_enum_type_info(def, &module.alias);
                    if let Some((traits, methods)) = external_impls.get(&(module.alias.clone(), def.name.clone())) {
                        if let CtValue::Struct { fields, .. } = &mut info {
                            if let Some((_, CtValue::List(values))) = fields.iter_mut().find(|(name, _)| name == "implements") { values.extend(traits.iter().cloned().map(ct_str)); }
                            if let Some((_, CtValue::List(values))) = fields.iter_mut().find(|(name, _)| name == "methods") { values.extend(methods.iter().cloned()); }
                        }
                    }
                    types.push(info.clone());
                    package_types.push(info);
                }
                crate::AST::Item::Func(func) if func.name != "build" => {
                    let info = build_function_info(func, &module.alias, facts);
                    functions.push(info.clone());
                    package_functions.push(info);
                }
                _ => {}
            }
        }
        packages.push(ct_struct(
            "PackageInfo",
            &[
                ("name", ct_str(module.alias.clone())),
                ("identity", ct_str(module.alias.clone())),
                ("types", ct_list(package_types)),
                ("functions", ct_list(package_functions)),
            ],
        ));
    }
    ct_struct(
        crate::Syntax::TYPE_PROGRAM_INFO,
        &[
            ("packages", ct_list(packages)),
            ("types", ct_list(types)),
            ("functions", ct_list(functions)),
        ],
    )
}

fn build_function_info(func: &Func, module: &str, facts: &ProgramSemanticFacts) -> CtValue {
    let qualified = identity(module, &func.name);
    let effects = facts.effects.get(&qualified).cloned().unwrap_or_default();
    ct_struct(
        "FunctionInfo",
        &[
            ("name", ct_str(func.name.clone())),
            ("module", ct_str(module)),
            ("identity", ct_str(qualified.clone())),
            (
                "params",
                ct_list(func.params.iter().map(|param| ct_str(param.name.clone())).collect()),
            ),
            (
                "span",
                ct_struct(
                    crate::Syntax::TYPE_SOURCE_SPAN,
                    &[
                        ("start", CtValue::Int(func.name_span.start as i64)),
                        ("end", CtValue::Int(func.name_span.end as i64)),
                    ],
                ),
            ),
            (
                "effects",
                ct_struct(
                    "EffectInfo",
                    &[("values", ct_list(effects.into_iter().map(ct_str).collect()))],
                ),
            ),
            (
                "reaches_panic",
                ct_bool(facts.reaches_panic.contains(&qualified)),
            ),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Diagnostics::Span, AST::Type};

    fn span() -> Span {
        Span::new(0, 1)
    }

    fn field(name: &str, ty: &str, is_pub: bool) -> Field {
        Field {
            is_pub,
            is_package_pub: false,
            name: name.to_string(),
            name_span: span(),
            ty: Type::Named(ty.to_string()),
            ty_span: span(),
            serde_markers: Vec::new(),
            redact: false,
            computed: None,
        }
    }

    fn method(name: &str, is_pub: bool) -> Func {
        Func {
            span: span(),
            is_pub,
            is_package_pub: false,
            external_type: None,
            meta: None,
            name: name.to_string(),
            name_span: span(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Some(Type::Named("String".to_string())),
            return_type_span: Some(span()),
            is_unsafe: false,
            unsafe_reason: None,
            unsafe_span: None,
            is_pure: false,
            is_sanitizer: false,
            is_reactive: false,
            declared_effects: None,
            effect_via: None,
            state_requires: None,
            state_transition: None,
            web_marker: None,
            pre: Vec::new(),
            post: Vec::new(),
            is_must_use: false,
            must_use_span: None,
            maturity: None,
            maturity_span: None,
            is_inline: false,
            is_inline_always: false,
            is_replayable: false,
            replayable_span: None,
            is_task: false,
            task_span: None,
            every: None,
            inline_foreign: None,
            inline_span: None,
            body: Vec::new(),
        }
    }

    #[test]
    fn type_info_includes_methods_and_type_params() {
        let s = StructDef {
            span: span(),
            is_pub: true,
            is_package_pub: false,
            name: "Point".to_string(),
            name_span: span(),
            type_params: vec![TypeParam {
                name: "T".to_string(),
                name_span: span(),
                bounds: vec!["Comparable".to_string()],
            }],
            fields: vec![field("x", "T", true), field("secret", "Int", false)],
            methods: vec![method("tag", true)],
            trait_impls: Vec::new(),
            derives: vec![("Debug".to_string(), span())],
            is_published_schema: false,
            published_schema_span: None,
            is_single_use: false,
            single_use_span: None,
            is_must_use: false,
            must_use_span: None,
            layout: None,
            layout_span: None,
            serde_markers: Vec::new(),
            type_markers: Vec::new(),
            validate_block: Vec::new(),
            validate_span: None,
        };
        let info = build_struct_type_info(&s);
        let CtValue::Struct { fields, .. } = info else {
            panic!("expected struct");
        };
        let get = |name: &str| {
            fields
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        assert!(matches!(get("name"), CtValue::Str(ref n) if n == "Point"));
        let CtValue::List(fields) = get("fields") else {
            panic!("fields");
        };
        assert_eq!(fields.len(), 2);
        let CtValue::List(methods) = get("methods") else {
            panic!("methods");
        };
        assert_eq!(methods.len(), 1);
        let CtValue::List(type_params) = get("type_params") else {
            panic!("type_params");
        };
        assert_eq!(type_params.len(), 1);
        let CtValue::List(markers) = get("markers") else {
            panic!("markers");
        };
        assert!(markers
            .iter()
            .any(|m| matches!(m, CtValue::Str(s) if s == "Debug")));
    }
}
