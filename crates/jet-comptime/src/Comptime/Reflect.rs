//! D-METAREFLECT1 / D-REFLECT1: build comptime reflection handles for user derives.
//!
//! `T.reflect()` in a derive body receives a `TypeInfo` value whose `.fields`,
//! `.methods`, `.type_params`, `.markers`, and `.expanded_markers` expose the
//! target type's shape. `.markers` preserves written markers; the expanded
//! view contains only derives lowered from them.

use crate::AST::{
    Dimension, DistinctDef, EnumDef, Field, Func, Marker, StructDef, StructLayout, Type, TypeParam,
    VariantPayload,
};

use crate::AST::CtValue;

#[derive(Debug, Clone, Default)]
pub struct ProgramSemanticFacts {
    pub effects: std::collections::HashMap<String, Vec<String>>,
    pub reaches_panic: std::collections::BTreeSet<String>,
    pub fact_registry: jet_foundation::Facts::FactRegistry,
    pub name_ledger: jet_foundation::Names::NameLedger,
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
/// wider `TypeInfo` wrapper. Tooling uses this same value as `T.@layout`.
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
/// wider `TypeInfo` wrapper. Tooling uses this same value as `T.@layout`.
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

fn typed_enum(type_name: &str, variant: &str) -> CtValue {
    CtValue::Enum {
        type_name: type_name.to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn optional_value(value: Option<CtValue>, ty: &str) -> CtValue {
    value.map_or_else(
        || CtValue::absent(Type::Named(ty.to_string())),
        |value| CtValue::Present(Box::new(value)),
    )
}

/// One typed dimension fact. Axes remain records, so `Length^1` is not a
/// display-only string: callers can inspect the axis and exponent.
fn dimension_info(dimension: &Dimension) -> CtValue {
    let axes = dimension
        .axes()
        .map(|(name, exponent)| {
            ct_struct(
                "DimensionAxis",
                &[
                    ("name", ct_str(name)),
                    ("exponent", CtValue::Int(exponent as i64)),
                ],
            )
        })
        .collect();
    ct_struct(
        "DimensionInfo",
        &[
            ("axes", ct_list(axes)),
            ("identity", ct_str(dimension.identity())),
            ("display", ct_str(dimension.display_name())),
        ],
    )
}

fn range_info(start: i64, end: i64) -> CtValue {
    ct_struct(
        crate::Syntax::TYPE_RANGE,
        &[("start", CtValue::Int(start)), ("end", CtValue::Int(end))],
    )
}

/// Names and paths remain strings because they are stable fact identities; the
/// fact's meaning is carried by typed kind, members, range, and dimension.
fn fact_value(
    kind: &str,
    name: &str,
    members: impl IntoIterator<Item = String>,
    range: Option<CtValue>,
    dimension: Option<CtValue>,
) -> CtValue {
    ct_struct(
        "FactValue",
        &[
            ("kind", typed_enum("FactKind", kind)),
            ("name", ct_str(name)),
            (
                "members",
                ct_list(members.into_iter().map(ct_str).collect()),
            ),
            ("range", optional_value(range, crate::Syntax::TYPE_RANGE)),
            (
                "dimension",
                optional_value(dimension, "DimensionInfo"),
            ),
        ],
    )
}

fn fact_info(kind: &str, name: &str, path: String, value: CtValue) -> CtValue {
    ct_struct(
        "FactInfo",
        &[
            ("kind", typed_enum("FactKind", kind)),
            ("name", ct_str(name)),
            ("path", ct_str(path)),
            ("value", value),
        ],
    )
}

fn type_dimensions(ty: &Type) -> Vec<CtValue> {
    match ty {
        Type::Quantity { dimension, .. } => vec![dimension_info(dimension)],
        Type::Tagged { inner, .. } => type_dimensions(inner),
        _ => Vec::new(),
    }
}

fn type_range(ty: &Type) -> Option<(i64, i64)> {
    match ty {
        Type::IntN { signed, bits } => {
            let (start, end) = crate::AST::int_range(*signed, *bits);
            Some((i64::try_from(start).ok()?, i64::try_from(end).ok()?))
        }
        Type::Tagged { inner, .. } => type_range(inner),
        _ => None,
    }
}

fn type_fact_rows(path: &str, ty: &Type) -> Vec<CtValue> {
    let mut facts = Vec::new();
    if let Some((start, end)) = type_range(ty) {
        facts.push(fact_info(
            "Range",
            "range",
            format!("{path}.@range"),
            fact_value(
                "Range",
                "range",
                std::iter::empty::<String>(),
                Some(range_info(start, end)),
                None,
            ),
        ));
    }
    if let Some((_, dimension)) = ty.quantity_parts() {
        facts.push(fact_info(
            "Dimension",
            "dimension",
            format!("{path}.@dimension"),
            fact_value(
                "Dimension",
                "dimension",
                std::iter::empty::<String>(),
                None,
                Some(dimension_info(&dimension)),
            ),
        ));
    }
    facts
}

fn distinct_fact_rows(definition: &DistinctDef) -> Vec<CtValue> {
    let mut facts = Vec::new();
    if let Some((start, end, _)) = definition.range {
        facts.push(fact_info(
            "Range",
            "range",
            format!("{}.@range", definition.name),
            fact_value(
                "Range",
                "range",
                std::iter::empty::<String>(),
                Some(range_info(start, end)),
                None,
            ),
        ));
    }
    if let Some((dimension, kind)) = &definition.quantity {
        facts.push(fact_info(
            "Dimension",
            "dimension",
            format!("{}.@dimension", definition.name),
            fact_value(
                "Dimension",
                "dimension",
                [kind.name().to_string()],
                None,
                Some(dimension_info(dimension)),
            ),
        ));
    }
    facts
}

/// D-TYPE2-PLANE1=A: a marker reflects as the typed record its registry row
/// describes, at every position it may be written — the type, a field, a
/// method, a variant. Nothing reflects a marker as a bare name.
fn marker_infos(markers: &[Marker]) -> Vec<CtValue> {
    markers.iter().map(marker_info).collect()
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
    // D-META-REG1=A: reflection reads the one registration table, not a
    // marker-only table beside it. A marker is the row whose target is written
    // code, so its signature rides on the row.
    let row = jet_foundation::Registry::row(&marker.name).and_then(|row| row.rule);
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
    marker_info_value(&marker.name, args)
}

fn marker_info_value(name: &str, args: Vec<CtValue>) -> CtValue {
    ct_struct(
        "MarkerInfo",
        &[("name", ct_str(name)), ("args", ct_list(args))],
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
            ("markers", ct_list(marker_infos(&field.serde_markers))),
            ("dimensions", ct_list(type_dimensions(&field.ty))),
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
    let dimensions = method
        .params
        .iter()
        .flat_map(|param| type_dimensions(&param.ty))
        .chain(
            method
                .return_type
                .iter()
                .flat_map(|ty| type_dimensions(ty)),
        )
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
            ("dimensions", ct_list(dimensions)),
            // D-REFLECT1: the retained marker nodes, same source as every other
            // consumer. This was hardcoded empty, so reflection reported that a
            // method carried no markers no matter what was written on it.
            ("markers", ct_list(marker_infos(&method.markers))),
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

fn derived_marker_info(name: &str) -> CtValue {
    let name = jet_foundation::Registry::row(name)
        .map(|row| row.name)
        .unwrap_or(name);
    marker_info_value(name, Vec::new())
}

/// Keep user-written markers separate from derives lowered from them. The
/// parser gives lowered derives the source marker's name span, so source
/// markers stay in `markers` and their lowered rows appear in `expanded_markers`.
fn type_level_marker_views(
    type_markers: &[Marker],
    serde_markers: &[Marker],
    derives: &[(String, crate::Diagnostics::Span)],
) -> (Vec<CtValue>, Vec<CtValue>) {
    let written_source = if type_markers.is_empty() {
        serde_markers
    } else {
        type_markers
    };
    let mut written = marker_infos(written_source);
    let mut expanded = Vec::new();
    for (name, span) in derives {
        if written_source
            .iter()
            .any(|marker| marker.name_span == *span)
        {
            expanded.push(derived_marker_info(name));
        } else {
            written.push(derived_marker_info(name));
        }
    }
    (written, expanded)
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

fn reflected_state_fact(owner: &str, state: &str) -> CtValue {
    let path = state_path(owner, state);
    fact_info(
        "State",
        state,
        path.clone(),
        fact_value("State", state, [path], None, None),
    )
}

fn reflected_facts(registry: &jet_foundation::Facts::FactRegistry) -> Vec<CtValue> {
    registry
        .iter()
        .flat_map(|fact| {
            if fact.members.is_empty() {
                vec![fact_info(
                    fact.kind.name(),
                    &fact.name,
                    fact.name.clone(),
                    fact_value(
                        fact.kind.name(),
                        &fact.name,
                        std::iter::empty::<String>(),
                        None,
                        None,
                    ),
                )]
            } else {
                fact.members
                    .iter()
                    .map(|member| {
                        let path = format!("{}.{}", fact.name, member);
                        fact_info(
                            fact.kind.name(),
                            member,
                            path.clone(),
                            fact_value(
                                fact.kind.name(),
                                member,
                                [path],
                                None,
                                None,
                            ),
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
    build_struct_type_info_with_path(s, states, &s.name)
}

pub fn build_struct_type_info_with_path(
    s: &StructDef,
    states: &[String],
    path: &str,
) -> CtValue {
    let fields_info: Vec<CtValue> = s.fields.iter().map(build_field_info).collect();
    let dimensions = s
        .fields
        .iter()
        .flat_map(|field| type_dimensions(&field.ty))
        .collect::<Vec<_>>();
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
    let mut facts = states
        .iter()
        .map(|state| reflected_state_fact(&s.name, state))
        .collect::<Vec<_>>();
    for field in &s.fields {
        facts.extend(type_fact_rows(&format!("{}.{}", s.name, field.name), &field.ty));
    }
    let (markers, expanded_markers) =
        type_level_marker_views(&s.type_markers, &s.serde_markers, &s.derives);
    ct_struct(
        "TypeInfo",
        &[
            ("name", ct_str(s.name.clone())),
            ("path", ct_str(path)),
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
            ("markers", ct_list(markers)),
            ("expanded_markers", ct_list(expanded_markers)),
            ("states", ct_list(state_info)),
            ("transitions", ct_list(transition_info)),
            ("facts", ct_list(facts)),
            ("dimensions", ct_list(dimensions)),
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

/// Build the same reflection handle for a nominal distinct type. Distinct
/// capability rows are kept in `derives`, so the written `#Comparable` marker
/// remains visible in `.markers` while its lowered row is also available in
/// `.expanded_markers`.
pub fn build_distinct_type_info(d: &DistinctDef, module: &str) -> CtValue {
    let path = format!("{module}.{}", d.name);
    build_distinct_type_info_with_path(d, module, &path)
}

pub fn build_distinct_type_info_with_path(
    d: &DistinctDef,
    module: &str,
    path: &str,
) -> CtValue {
    let layout = layout_info("default", "physical layout unspecified", "distinct declaration", Vec::<(String, String)>::new());
    let dimensions = d
        .quantity
        .as_ref()
        .map(|(dimension, _)| vec![dimension_info(dimension)])
        .unwrap_or_default();
    let (markers, expanded_markers) =
        type_level_marker_views(&d.type_markers, &[], &d.derives);
    qualify_info(
        ct_struct(
            "TypeInfo",
            &[
                ("name", ct_str(d.name.clone())),
                ("layout", layout),
                (
                    "span",
                    ct_struct(
                        crate::Syntax::TYPE_SOURCE_SPAN,
                        &[
                            ("start", CtValue::Int(d.name_span.start as i64)),
                            ("end", CtValue::Int(d.name_span.end as i64)),
                        ],
                    ),
                ),
                ("fields", ct_list(Vec::new())),
                ("methods", ct_list(Vec::new())),
                ("type_params", ct_list(Vec::new())),
                ("markers", ct_list(markers)),
                ("expanded_markers", ct_list(expanded_markers)),
                ("states", ct_list(Vec::new())),
                ("transitions", ct_list(Vec::new())),
                ("facts", ct_list(distinct_fact_rows(d))),
                ("dimensions", ct_list(dimensions)),
                ("implements", ct_list(Vec::new())),
            ],
        ),
        module,
        &d.name,
        "distinct",
        path,
    )
}

fn qualify_info(
    mut info: CtValue,
    module: &str,
    identity: &str,
    kind: &str,
    path: &str,
) -> CtValue {
    if let CtValue::Struct { fields, .. } = &mut info {
        if let Some((_, value)) = fields.iter_mut().find(|(name, _)| name == "path") {
            *value = ct_str(path);
        } else {
            fields.push(("path".to_string(), ct_str(path)));
        }
        fields.push(("module".to_string(), ct_str(module)));
        fields.push(("identity".to_string(), ct_str(identity)));
        fields.push(("kind".to_string(), ct_str(kind)));
    }
    info
}

fn qualified_method_info(method: &Func, module: &str, identity: &str) -> CtValue {
    let mut info = build_method_info(method);
    if let CtValue::Struct { fields, .. } = &mut info {
        fields.push(("module".to_string(), ct_str(module)));
        fields.push((
            "identity".to_string(),
            ct_str(format!("{identity}.{}", method.name)),
        ));
    }
    info
}

fn build_enum_type_info(def: &EnumDef, module: &str, identity: &str, path: &str) -> CtValue {
    let layout = build_enum_layout_info(def);
    let mut dimensions = Vec::new();
    let mut facts = Vec::new();
    for variant in &def.variants {
        match &variant.payload {
            VariantPayload::Unit => {}
            VariantPayload::Single(ty, _) => {
                dimensions.extend(type_dimensions(ty));
                facts.extend(type_fact_rows(&format!("{}.{}", def.name, variant.name), ty));
            }
            VariantPayload::Named(fields) => {
                for field in fields {
                    dimensions.extend(type_dimensions(&field.ty));
                    facts.extend(type_fact_rows(
                        &format!("{}.{}.{}", def.name, variant.name, field.name),
                        &field.ty,
                    ));
                }
            }
        }
    }
    let variants = def.variants.iter().map(|variant| {
        let ty = match &variant.payload {
            VariantPayload::Unit => "Unit".to_string(),
            VariantPayload::Single(ty, _) => ty.name(),
            VariantPayload::Named(fields) => format!("{{{}}}", fields.iter().map(|field| format!("{}: {}", field.name, field.ty.name())).collect::<Vec<_>>().join(", ")),
        };
        let variant_dimensions = match &variant.payload {
            VariantPayload::Unit => Vec::new(),
            VariantPayload::Single(ty, _) => type_dimensions(ty),
            VariantPayload::Named(fields) => fields
                .iter()
                .flat_map(|field| type_dimensions(&field.ty))
                .collect(),
        };
        ct_struct("FieldInfo", &[
            ("name", ct_str(variant.name.clone())),
            ("ty", ct_str(ty)),
            ("markers", ct_list(marker_infos(&variant.serde_markers))),
            ("dimensions", ct_list(variant_dimensions)),
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
        .map(|method| qualified_method_info(method, module, identity))
        .collect();
    let params = def.type_params.iter().map(build_type_param_info).collect();
    let (markers, expanded_markers) =
        type_level_marker_views(&def.type_markers, &def.serde_markers, &def.derives);
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
        ("expanded_markers", ct_list(expanded_markers)),
        ("states", ct_list(Vec::new())),
        ("transitions", ct_list(transitions)),
        ("facts", ct_list(facts)),
        ("dimensions", ct_list(dimensions)),
        ("implements", ct_list(def.trait_impls.iter().map(|implementation| ct_str(implementation.trait_name.clone())).collect())),
    ]), module, identity, "enum", path)
}

fn ledger_path(
    ledger: &jet_foundation::Names::NameLedger,
    module: usize,
    module_name: &str,
    symbol: &str,
) -> String {
    ledger
        .canonical_path(module, symbol)
        .unwrap_or_else(|| format!("{module_name}.{symbol}"))
}

fn ledger_module_name(
    ledger: &jet_foundation::Names::NameLedger,
    module: usize,
    fallback: &str,
) -> String {
    ledger
        .module_alias(module)
        .map(str::to_string)
        .unwrap_or_else(|| fallback.to_string())
}

fn ledger_identity(
    ledger: &jet_foundation::Names::NameLedger,
    module: usize,
    module_name: &str,
    symbol: &str,
) -> String {
    ledger
        .semantic_identity(module, symbol)
        .unwrap_or_else(|| identity(module_name, symbol))
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
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        let module_name = ledger_module_name(&facts.name_ledger, module_idx, &module.alias);
        for item in &module.items {
            if let crate::AST::Item::Impl(implementation) = item {
                let entry = external_impls
                    .entry((module_name.clone(), implementation.type_name.clone()))
                    .or_default();
                if let Some(trait_name) = &implementation.trait_name {
                    entry.0.push(trait_name.clone());
                }
                entry.1.extend(implementation.methods.iter().map(|method| {
                    qualified_method_info(
                        method,
                        &module_name,
                        &ledger_identity(
                            &facts.name_ledger,
                            module_idx,
                            &module_name,
                            &implementation.type_name,
                        ),
                    )
                }));
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
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        let module_name = ledger_module_name(&facts.name_ledger, module_idx, &module.alias);
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
                        &module_name,
                        &ledger_identity(&facts.name_ledger, module_idx, &module_name, &def.name),
                        "struct",
                        &ledger_path(&facts.name_ledger, module_idx, &module_name, &def.name),
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
                                .map(|method| {
                                    qualified_method_info(
                                        method,
                                        &module_name,
                                        &ledger_identity(
                                            &facts.name_ledger,
                                            module_idx,
                                            &module_name,
                                            &def.name,
                                        ),
                                    )
                                })
                                .collect();
                        }
                        if let Some((_, CtValue::List(values))) =
                            fields.iter_mut().find(|(name, _)| name == "facts")
                        {
                            values.extend(reflected_facts(&facts.fact_registry));
                        }
                    }
                    if let Some((traits, methods, transitions)) =
                        external_impls.get(&(module_name.clone(), def.name.clone()))
                    {
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
                    let mut info = build_enum_type_info(
                        def,
                        &module_name,
                        &ledger_identity(&facts.name_ledger, module_idx, &module_name, &def.name),
                        &ledger_path(&facts.name_ledger, module_idx, &module_name, &def.name),
                    );
                    if let CtValue::Struct { fields, .. } = &mut info {
                        if let Some((_, CtValue::List(values))) =
                            fields.iter_mut().find(|(name, _)| name == "facts")
                        {
                            values.extend(reflected_facts(&facts.fact_registry));
                        }
                    }
                    if let Some((traits, methods, transitions)) =
                        external_impls.get(&(module_name.clone(), def.name.clone()))
                    {
                        if let CtValue::Struct { fields, .. } = &mut info {
                            if let Some((_, CtValue::List(values))) = fields.iter_mut().find(|(name, _)| name == "implements") { values.extend(traits.iter().cloned().map(ct_str)); }
                            if let Some((_, CtValue::List(values))) = fields.iter_mut().find(|(name, _)| name == "methods") { values.extend(methods.iter().cloned()); }
                            if let Some((_, CtValue::List(values))) = fields.iter_mut().find(|(name, _)| name == "transitions") { values.extend(transitions.iter().cloned()); }
                        }
                    }
                    types.push(info.clone());
                    package_types.push(info);
                }
                crate::AST::Item::Distinct(def) => {
                    let mut info = build_distinct_type_info_with_path(
                        def,
                        &module_name,
                        &ledger_path(&facts.name_ledger, module_idx, &module_name, &def.name),
                    );
                    if let CtValue::Struct { fields, .. } = &mut info {
                        if let Some((_, CtValue::List(values))) =
                            fields.iter_mut().find(|(name, _)| name == "facts")
                        {
                            values.extend(reflected_facts(&facts.fact_registry));
                        }
                    }
                    if let Some((traits, methods, transitions)) =
                        external_impls.get(&(module_name.clone(), def.name.clone()))
                    {
                        if let CtValue::Struct { fields, .. } = &mut info {
                            if let Some((_, CtValue::List(values))) =
                                fields.iter_mut().find(|(name, _)| name == "implements")
                            {
                                values.extend(traits.iter().cloned().map(ct_str));
                            }
                            if let Some((_, CtValue::List(values))) =
                                fields.iter_mut().find(|(name, _)| name == "methods")
                            {
                                values.extend(methods.iter().cloned());
                            }
                            if let Some((_, CtValue::List(values))) =
                                fields.iter_mut().find(|(name, _)| name == "transitions")
                            {
                                values.extend(transitions.iter().cloned());
                            }
                        }
                    }
                    types.push(info.clone());
                    package_types.push(info);
                }
                crate::AST::Item::Func(func) if func.name != "build" => {
                    let info = build_function_info(func, module_idx, &module_name, facts);
                    functions.push(info.clone());
                    package_functions.push(info);
                }
                _ => {}
            }
        }
        packages.push(ct_struct(
            "PackageInfo",
            &[
                ("name", ct_str(module_name.clone())),
                ("identity", ct_str(module_name)),
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

fn build_function_info(
    func: &Func,
    module_idx: usize,
    module: &str,
    facts: &ProgramSemanticFacts,
) -> CtValue {
    let qualified = ledger_identity(&facts.name_ledger, module_idx, module, &func.name);
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
    use crate::{Diagnostics::Span, AST::{Dimension, QuantityKind, Type}};

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
            head_pattern: None,
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
            undo: None,
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

    fn struct_field<'a>(value: &'a CtValue, name: &str) -> &'a CtValue {
        let CtValue::Struct { fields, .. } = value else {
            panic!("expected struct, got {value:?}");
        };
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
            .unwrap_or_else(|| panic!("missing field `{name}` in {value:?}"))
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
        assert!(matches!(get("path"), CtValue::Str(ref path) if path == "Point"));
        let qualified = build_struct_type_info_with_path(&s, &[], "app.Point");
        assert!(matches!(
            struct_field(&qualified, "name"),
            CtValue::Str(ref name) if name == "Point"
        ));
        assert!(matches!(
            struct_field(&qualified, "path"),
            CtValue::Str(ref path) if path == "app.Point"
        ));
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
        assert!(matches!(
            get("expanded_markers"),
            CtValue::List(expanded) if expanded.is_empty()
        ));
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

    #[test]
    fn range_and_dimension_facts_are_typed_records() {
        let info = build_distinct_type_info(
            &DistinctDef {
                is_pub: true,
                is_package_pub: false,
                type_markers: Vec::new(),
                derives: Vec::new(),
                quantity: Some((Dimension::base("Length"), QuantityKind::Linear)),
                name: "Severity".to_string(),
                name_span: span(),
                base: Type::Int,
                base_span: span(),
                range: Some((0, 10, span())),
                invariant: None,
                span: span(),
            },
            "main",
        );
        let CtValue::List(facts) = struct_field(&info, "facts") else {
            panic!("facts");
        };
        let is_kind = |fact: &CtValue, expected: &str| match struct_field(fact, "kind") {
            CtValue::Enum { variant, .. } => variant == expected,
            other => panic!("expected typed fact kind, got {other:?}"),
        };

        let range = facts
            .iter()
            .find(|fact| is_kind(fact, "Range"))
            .expect("range fact");
        let CtValue::Present(range_value) = struct_field(struct_field(range, "value"), "range")
        else {
            panic!("range fact must carry a present Range");
        };
        assert!(matches!(struct_field(range_value, "start"), CtValue::Int(0)));
        assert!(matches!(struct_field(range_value, "end"), CtValue::Int(10)));

        let dimension = facts
            .iter()
            .find(|fact| is_kind(fact, "Dimension"))
            .expect("dimension fact");
        let CtValue::Present(dimension_value) =
            struct_field(struct_field(dimension, "value"), "dimension")
        else {
            panic!("dimension fact must carry a present DimensionInfo");
        };
        let CtValue::List(axes) = struct_field(dimension_value, "axes") else {
            panic!("dimension axes");
        };
        assert!(axes.iter().any(|axis| {
            matches!(
                (struct_field(axis, "name"), struct_field(axis, "exponent")),
                (CtValue::Str(name), CtValue::Int(1)) if name == "Length"
            )
        }));
    }
}
