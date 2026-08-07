//! D-METAREFLECT1 / D-REFLECT1: build comptime reflection handles for user derives.
//!
//! `T.reflect()` in a derive body receives a `TypeInfo` value whose `.fields`,
//! `.methods`, `.type_params`, and `.markers` expose the target type's shape.

use crate::AST::{EnumDef, Field, Func, Marker, StructDef, StructLayout, TypeParam, VariantPayload};

use super::Value::CtValue;

#[derive(Debug, Clone, Default)]
pub struct ProgramSemanticFacts {
    pub effects: std::collections::HashMap<String, Vec<String>>,
    pub reaches_panic: std::collections::BTreeSet<String>,
    pub fact_registry: jet_foundation::Facts::FactRegistry,
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

fn unknown_layout_bytes() -> CtValue {
    // D-LAYOUT-FACTS1=B: byte facts stay absent until a canonical target
    // layout engine exists. `None(Int)` is the typed optional value used by
    // the public `LayoutInfo`/`LayoutField` model.
    CtValue::absent(crate::AST::Type::Int)
}

fn layout_field_info(
    name: impl Into<String>,
    ty: impl Into<String>,
    guarantee: &str,
    source: &str,
) -> CtValue {
    ct_struct(
        crate::Syntax::TYPE_LAYOUT_FIELD,
        &[
            ("name", ct_str(name)),
            ("ty", ct_str(ty)),
            ("offset", unknown_layout_bytes()),
            ("size", unknown_layout_bytes()),
            ("target", ct_str("unknown")),
            ("guarantee", ct_str(guarantee)),
            ("source", ct_str(source)),
        ],
    )
}

fn layout_info(
    kind: &str,
    guarantee: &str,
    source: &str,
    fields: impl IntoIterator<Item = (String, String)>,
) -> CtValue {
    ct_struct(
        crate::Syntax::TYPE_LAYOUT_INFO,
        &[
            ("kind", ct_str(kind)),
            ("size", unknown_layout_bytes()),
            ("alignment", unknown_layout_bytes()),
            ("stride", unknown_layout_bytes()),
            ("target", ct_str("unknown")),
            ("guarantee", ct_str(guarantee)),
            ("source", ct_str(source)),
            (
                "fields",
                ct_list(
                    fields
                        .into_iter()
                        .map(|(name, ty)| layout_field_info(name, ty, guarantee, source))
                        .collect(),
                ),
            ),
        ],
    )
}

fn layout_info_for_struct(s: &StructDef) -> CtValue {
    let (kind, guarantee) = match s.layout.as_ref() {
        Some(StructLayout::C) => ("c", "repr(C) declaration"),
        Some(StructLayout::Columnar) => ("columnar", "columnar storage declaration"),
        None => ("default", "physical layout unspecified"),
    };
    layout_info(
        kind,
        guarantee,
        "struct declaration",
        s.fields
            .iter()
            .map(|field| (field.name.clone(), field.ty.name())),
    )
}

/// Build the focused layout projection for a struct without exposing the
/// wider `TypeInfo` wrapper. Tooling uses this same value as `T.$layout`.
pub fn build_struct_layout_info(s: &StructDef) -> CtValue {
    layout_info_for_struct(s)
}

fn layout_info_for_enum(def: &EnumDef) -> CtValue {
    let fields = def.variants.iter().map(|variant| {
        let ty = match &variant.payload {
            VariantPayload::Unit => "Unit".to_string(),
            VariantPayload::Single(ty, _) => ty.name(),
            VariantPayload::Named(fields) => format!(
                "{{{}}}",
                fields
                    .iter()
                    .map(|field| format!("{}: {}", field.name, field.ty.name()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        (variant.name.clone(), ty)
    });
    layout_info(
        "default",
        "enum physical layout unspecified",
        "enum declaration",
        fields,
    )
}

/// Build the focused layout projection for an enum without exposing the
/// wider `TypeInfo` wrapper. Tooling uses this same value as `T.$layout`.
pub fn build_enum_layout_info(def: &EnumDef) -> CtValue {
    layout_info_for_enum(def)
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

fn marker_arg_path(expression: &crate::AST::Expr) -> Option<String> {
    match expression {
        crate::AST::Expr::Ident(name, _) => Some(name.clone()),
        crate::AST::Expr::Field(base, member, _) => {
            Some(format!("{}.{}", marker_arg_path(base)?, member))
        }
        _ => None,
    }
}

fn marker_arg_value(expression: &crate::AST::Expr, source_type: &str) -> CtValue {
    if jet_foundation::Policy::rule_arg_declaration(source_type).is_some() {
        if let Some(path) = marker_arg_path(expression) {
            return CtValue::Enum {
                type_name: source_type.to_string(),
                variant: path.rsplit('.').next().unwrap_or(&path).to_string(),
                args: Vec::new(),
            };
        }
    }
    match expression {
        crate::AST::Expr::Str(parts, _) => CtValue::Str(
            parts
                .iter()
                .filter_map(|part| match part {
                    crate::AST::StrPart::Lit(text) => Some(text.clone()),
                    _ => None,
                })
                .collect(),
        ),
        crate::AST::Expr::Int(value, ..) => CtValue::Int(*value),
        crate::AST::Expr::Bool(value, _) => CtValue::Bool(*value),
        crate::AST::Expr::Char(value, _) => CtValue::Char(*value),
        _ => CtValue::Unit,
    }
}

fn marker_info(marker: &Marker) -> CtValue {
    let row = jet_foundation::Policy::applied_rule(&marker.name);
    let bindings = row.and_then(|row| row.signature.marker_argument_bindings(marker));
    let args = marker
        .args
        .iter()
        .enumerate()
        .map(|(index, argument)| {
            let binding = bindings
                .as_ref()
                .and_then(|bindings| bindings.iter().find(|binding| binding.source_index == index));
            let parameter = binding
                .and_then(|binding| binding.parameter_index)
                .and_then(|parameter| row.and_then(|row| row.signature.params.get(parameter)));
            let source_type = parameter
                .map(|parameter| parameter.source_type)
                .unwrap_or("Value");
            ct_struct(
                "MarkerArgInfo",
                &[
                    (
                        "name",
                        ct_str(parameter.map(|parameter| parameter.name).unwrap_or("value")),
                    ),
                    (
                        "ty",
                        ct_str(
                            source_type,
                        ),
                    ),
                    ("value", marker_arg_value(argument, source_type)),
                ],
            )
        })
        .collect();
    ct_struct(
        "MarkerInfo",
        &[("name", ct_str(marker.name.clone())), ("args", ct_list(args))],
    )
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
        Some(ret) => format!("fn {}({}) => {}", method.name, params, ret.name()),
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
            (
                "span",
                ct_struct(
                    crate::Syntax::TYPE_SOURCE_SPAN,
                    &[
                        ("start", CtValue::Int(field.name_span.start as i64)),
                        ("end", CtValue::Int(field.name_span.end as i64)),
                    ],
                ),
            ),
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
            // D-REFLECT1: the retained marker nodes, same source as every other
            // consumer. This was hardcoded empty, so reflection reported that a
            // method carried no markers no matter what was written on it.
            ("markers", ct_list(marker_names(&method.markers))),
            ("is_pub", ct_bool(method.is_pub)),
            (
                "span",
                ct_struct(
                    crate::Syntax::TYPE_SOURCE_SPAN,
                    &[
                        ("start", CtValue::Int(method.name_span.start as i64)),
                        ("end", CtValue::Int(method.name_span.end as i64)),
                    ],
                ),
            ),
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
            (
                "span",
                ct_struct(
                    crate::Syntax::TYPE_SOURCE_SPAN,
                    &[
                        ("start", CtValue::Int(param.name_span.start as i64)),
                        ("end", CtValue::Int(param.name_span.end as i64)),
                    ],
                ),
            ),
        ],
    )
}

fn type_level_marker_names(s: &StructDef) -> Vec<String> {
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
    names
}

fn type_level_markers(s: &StructDef) -> Vec<CtValue> {
    let mut markers = s
        .type_markers
        .iter()
        .chain(s.serde_markers.iter())
        .map(marker_info)
        .collect::<Vec<_>>();
    markers.extend(s.derives.iter().map(|(name, _)| {
        ct_struct(
            "MarkerInfo",
            &[("name", ct_str(name.clone())), ("args", ct_list(Vec::new()))],
        )
    }));
    markers
}

fn state_path(owner: &str, state: &str) -> String {
    if state == "_" || state.contains(".State.") {
        state.to_string()
    } else {
        format!("{owner}.State.{state}")
    }
}

fn transition_info(owner: &str, method: &Func) -> Option<CtValue> {
    let transition = method.state_transition.as_ref()?;
    Some(ct_struct(
        "TransitionInfo",
        &[
            ("operation", ct_str(method.name.clone())),
            (
                "from",
                ct_str(
                    transition
                        .from
                        .as_deref()
                        .map(|state| state_path(owner, state))
                        .unwrap_or_else(|| "_".to_string()),
                ),
            ),
            ("to", ct_str(state_path(owner, &transition.to))),
        ],
    ))
}

fn reflected_facts(registry: &jet_foundation::Facts::FactRegistry) -> Vec<CtValue> {
    registry
        .iter()
        .flat_map(|fact| {
            if fact.members.is_empty() {
                vec![ct_struct(
                    "FactInfo",
                    &[
                        ("kind", ct_str(fact.kind.name())),
                        ("name", ct_str(fact.name.clone())),
                        ("path", ct_str(fact.name.clone())),
                    ],
                )]
            } else {
                fact.members
                    .iter()
                    .map(|member| {
                        ct_struct(
                            "FactInfo",
                            &[
                                ("kind", ct_str(fact.kind.name())),
                                ("name", ct_str(member)),
                                ("path", ct_str(format!("{}.{}", fact.name, member))),
                            ],
                        )
                    })
                    .collect()
            }
        })
        .collect()
}

/// Build the `TypeInfo` handle passed into a user derive body for `struct` targets.
pub fn build_struct_type_info(s: &StructDef) -> CtValue {
    build_struct_type_info_with_states(s, &[])
}

pub fn build_struct_type_info_with_states(s: &StructDef, states: &[String]) -> CtValue {
    let fields_info: Vec<CtValue> = s.fields.iter().map(build_field_info).collect();
    let layout = build_struct_layout_info(s);
    let methods_info: Vec<CtValue> = s.methods.iter().map(build_method_info).collect();
    let type_params_info: Vec<CtValue> = s.type_params.iter().map(build_type_param_info).collect();
    let state_info = states
        .iter()
        .map(|state| {
            ct_struct(
                "StateInfo",
                &[
                    ("name", ct_str(state)),
                    ("path", ct_str(format!("{}.State.{state}", s.name))),
                ],
            )
        })
        .collect::<Vec<_>>();
    let transition_info = s
        .methods
        .iter()
        .chain(
            s.trait_impls
                .iter()
                .flat_map(|implementation| implementation.methods.iter()),
        )
        .filter_map(|method| transition_info(&s.name, method))
        .collect::<Vec<_>>();
    let facts = states
        .iter()
        .map(|state| {
            ct_struct(
                "FactInfo",
                &[
                    ("kind", ct_str("State")),
                    ("name", ct_str(state)),
                    ("path", ct_str(format!("{}.State.{state}", s.name))),
                ],
            )
        })
        .collect::<Vec<_>>();
    let marker_names = type_level_marker_names(s);
    ct_struct(
        "TypeInfo",
        &[
            ("name", ct_str(s.name.clone())),
            ("layout", layout),
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
                "marker_names",
                ct_list(marker_names.into_iter().map(ct_str).collect()),
            ),
            ("states", ct_list(state_info)),
            ("transitions", ct_list(transition_info)),
            ("facts", ct_list(facts)),
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
    let layout = build_enum_layout_info(def);
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
            ("span", ct_struct(crate::Syntax::TYPE_SOURCE_SPAN, &[
                ("start", CtValue::Int(variant.name_span.start as i64)),
                ("end", CtValue::Int(variant.name_span.end as i64)),
            ])),
        ])
    }).collect();
    let methods = def
        .methods
        .iter()
        .chain(
            def.trait_impls
                .iter()
                .flat_map(|implementation| implementation.methods.iter()),
        )
        .map(|method| qualified_method_info(method, module, &def.name))
        .collect();
    let params = def.type_params.iter().map(build_type_param_info).collect();
    let mut marker_names = def.type_markers.iter().chain(def.serde_markers.iter()).map(|marker| ct_str(marker.name.clone())).collect::<Vec<_>>();
    marker_names.extend(def.derives.iter().map(|(name, _)| ct_str(name.clone())));
    let mut markers = def.type_markers.iter().chain(def.serde_markers.iter()).map(marker_info).collect::<Vec<_>>();
    markers.extend(def.derives.iter().map(|(name, _)| ct_struct("MarkerInfo", &[("name", ct_str(name.clone())), ("args", ct_list(Vec::new()))])));
    let transitions = def
        .methods
        .iter()
        .chain(
            def.trait_impls
                .iter()
                .flat_map(|implementation| implementation.methods.iter()),
        )
        .filter_map(|method| transition_info(&def.name, method))
        .collect();
    qualify_info(ct_struct("TypeInfo", &[
        ("name", ct_str(def.name.clone())),
        ("layout", layout),
        ("span", ct_struct(crate::Syntax::TYPE_SOURCE_SPAN, &[("start", CtValue::Int(def.name_span.start as i64)), ("end", CtValue::Int(def.name_span.end as i64))])),
        ("fields", ct_list(variants)),
        ("methods", ct_list(methods)),
        ("type_params", ct_list(params)),
        ("markers", ct_list(markers)),
        ("marker_names", ct_list(marker_names)),
        ("states", ct_list(Vec::new())),
        ("transitions", ct_list(transitions)),
        ("facts", ct_list(Vec::new())),
        ("implements", ct_list(def.trait_impls.iter().map(|implementation| ct_str(implementation.trait_name.clone())).collect())),
    ]), module, &def.name, "enum")
}

/// D-METADEPTH2: read-only, post-sema whole-program snapshot handed only to
/// selected root `fn build`. Existing TypeInfo builders remain canonical.
pub fn build_program_info(
    bundle: &crate::AST::ProgramBundle,
    facts: &ProgramSemanticFacts,
) -> CtValue {
    let mut external_impls = std::collections::HashMap::<
        (String, String),
        (Vec<String>, Vec<CtValue>, Vec<CtValue>),
    >::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let crate::AST::Item::Impl(implementation) = item {
                let entry = external_impls.entry((module.alias.clone(), implementation.type_name.clone())).or_default();
                if let Some(trait_name) = &implementation.trait_name {
                    entry.0.push(trait_name.clone());
                }
                entry.1.extend(implementation.methods.iter().map(|method| qualified_method_info(method, &module.alias, &implementation.type_name)));
                entry.2.extend(
                    implementation
                        .methods
                        .iter()
                        .filter_map(|method| transition_info(&implementation.type_name, method)),
                );
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
                    let states = module
                        .items
                        .iter()
                        .find_map(|item| match item {
                            crate::AST::Item::StateDecl(state)
                                if state.type_name == def.name =>
                            {
                                Some(
                                    state
                                        .states
                                        .iter()
                                        .map(|(name, _)| name.clone())
                                        .collect::<Vec<_>>(),
                                )
                            }
                            _ => None,
                        })
                        .unwrap_or_default();
                    let mut info = qualify_info(
                        build_struct_type_info_with_states(def, &states),
                        &module.alias,
                        &def.name,
                        "struct",
                    );
                    if let CtValue::Struct { fields, .. } = &mut info {
                        if let Some((_, CtValue::List(methods))) = fields.iter_mut().find(|(name, _)| name == "methods") {
                            *methods = def
                                .methods
                                .iter()
                                .chain(
                                    def.trait_impls
                                        .iter()
                                        .flat_map(|implementation| implementation.methods.iter()),
                                )
                                .map(|method| qualified_method_info(method, &module.alias, &def.name))
                                .collect();
                        }
                        if let Some((_, CtValue::List(values))) =
                            fields.iter_mut().find(|(name, _)| name == "facts")
                        {
                            *values = reflected_facts(&facts.fact_registry);
                        }
                    }
                    if let Some((traits, methods, transitions)) = external_impls.get(&(module.alias.clone(), def.name.clone())) {
                        if let CtValue::Struct { fields, .. } = &mut info {
                            if let Some((_, CtValue::List(values))) = fields.iter_mut().find(|(name, _)| name == "implements") {
                                values.extend(traits.iter().cloned().map(ct_str));
                            }
                            if let Some((_, CtValue::List(values))) = fields.iter_mut().find(|(name, _)| name == "methods") {
                                values.extend(methods.iter().cloned());
                            }
                            if let Some((_, CtValue::List(values))) = fields.iter_mut().find(|(name, _)| name == "transitions") {
                                values.extend(transitions.iter().cloned());
                            }
                        }
                    }
                    types.push(info.clone());
                    package_types.push(info);
                }
                crate::AST::Item::Enum(def) => {
                    let mut info = build_enum_type_info(def, &module.alias);
                    if let CtValue::Struct { fields, .. } = &mut info {
                        if let Some((_, CtValue::List(values))) =
                            fields.iter_mut().find(|(name, _)| name == "facts")
                        {
                            *values = reflected_facts(&facts.fact_registry);
                        }
                    }
                    if let Some((traits, methods, transitions)) = external_impls.get(&(module.alias.clone(), def.name.clone())) {
                        if let CtValue::Struct { fields, .. } = &mut info {
                            if let Some((_, CtValue::List(values))) = fields.iter_mut().find(|(name, _)| name == "implements") { values.extend(traits.iter().cloned().map(ct_str)); }
                            if let Some((_, CtValue::List(values))) = fields.iter_mut().find(|(name, _)| name == "methods") { values.extend(methods.iter().cloned()); }
                            if let Some((_, CtValue::List(values))) = fields.iter_mut().find(|(name, _)| name == "transitions") { values.extend(transitions.iter().cloned()); }
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
            default: None,
            default_ct: None,
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
            scrub_tag: None,
            is_reactive: false,
                reactive_upgrades: Vec::new(),
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
            task_metadata: None,
            inline_foreign: None,
            inline_span: None,
            return_view_provenance: None,
            declared_return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
            kernel: None,
            markers: Vec::new(),
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
            auto_derive_default: true,
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
            .any(|marker| matches!(
                marker,
                CtValue::Struct { fields, .. }
                    if fields.iter().any(|(name, value)|
                        name == "name" && matches!(value, CtValue::Str(value) if value == "Debug"))
            )));
        let marker_names = get("marker_names");
        assert!(matches!(marker_names, CtValue::List(_)));
        let CtValue::List(marker_names) = marker_names else {
            return;
        };
        assert!(marker_names
            .iter()
            .any(|name| matches!(name, CtValue::Str(name) if name == "Debug")));
    }

    #[test]
    fn transition_reflection_normalizes_state_paths() {
        let mut transition = method("open", true);
        transition.state_transition = Some(crate::AST::StateTransition {
            from: Some("Closed".to_string()),
            to: "Door.State.Open".to_string(),
            span: span(),
        });
        let info = transition_info("Door", &transition).expect("transition");
        assert!(matches!(info, CtValue::Struct { .. }));
        let CtValue::Struct { fields, .. } = info else {
            return;
        };
        assert!(fields.iter().any(
            |(name, value)| name == "from"
                && matches!(value, CtValue::Str(path) if path == "Door.State.Closed")
        ));
        assert!(fields.iter().any(
            |(name, value)| name == "to"
                && matches!(value, CtValue::Str(path) if path == "Door.State.Open")
        ));
    }

    #[test]
    fn unified_fact_reflection_contains_effect_tag_and_state_rows() {
        let mut registry = jet_foundation::Facts::FactRegistry::default();
        registry.declare(
            jet_foundation::Facts::FactKind::Effect,
            "Exec",
            std::iter::empty(),
        );
        registry.declare_with_rules(
            jet_foundation::Facts::FactKind::Tag,
            "PII",
            std::iter::empty(),
            ["Exec".to_string()],
            std::iter::empty(),
        );
        registry.declare(
            jet_foundation::Facts::FactKind::State,
            "Door.State",
            ["Open".to_string()],
        );
        let rows = format!("{:?}", reflected_facts(&registry));
        assert!(rows.contains("Effect") && rows.contains("Exec"), "{rows}");
        assert!(rows.contains("Tag") && rows.contains("PII"), "{rows}");
        assert!(rows.contains("State") && rows.contains("Door.State.Open"), "{rows}");
    }
}
