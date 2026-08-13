//! D-METAREFLECT1 / D-REFLECT1: build comptime reflection handles for user derives.
//!
//! `T.reflect()` in a derive body receives a `TypeInfo` value whose `.fields`,
//! `.methods`, `.type_params`, `.markers`, and `.expanded_markers` expose the
//! target type's shape. `.markers` preserves written markers; the expanded
//! view contains only derives lowered from them.

use crate::AST::{
    Dimension, DistinctDef, EnumDef, Expr, Field, Func, FunctionObligations, Item, KnowledgeFact, Marker,
    MaturityTag, Measure, StructDef, StructLayout, Type, TypeParam, UnitScaleProvenance,
    VariantPayload, ViewProvenance,
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
        s.reflection_fields()
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
pub fn build_dimension_info(dimension: &Dimension) -> CtValue {
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

pub fn build_range_info(start: i64, end: i64) -> CtValue {
    ct_struct(
        crate::Syntax::TYPE_RANGE,
        &[("start", CtValue::Int(start)), ("end", CtValue::Int(end))],
    )
}

fn measure_info(measure: &Measure) -> CtValue {
    let (kind, value, symbol) = match measure {
        Measure::Literal { kind, value } => (kind.clone(), Some(CtValue::Int(*value as i64)), None),
        Measure::Symbol { kind, name } => (kind.clone(), None, Some(ct_str(name.clone()))),
    };
    ct_struct(
        "MeasureInfo",
        &[
            ("kind", ct_str(kind)),
            ("value", optional_value(value, "Int")),
            ("symbol", optional_value(symbol, "String")),
        ],
    )
}

fn layout_fact_info(bytes: u8) -> CtValue {
    ct_struct("LayoutFact", &[("bytes", CtValue::Int(bytes as i64))])
}

fn classification_info(name: &str) -> CtValue {
    ct_struct("ClassificationInfo", &[("name", ct_str(name))])
}

fn nominal_info(name: &str) -> CtValue {
    ct_struct("NominalInfo", &[("name", ct_str(name))])
}

fn obligation_info(obligations: &FunctionObligations) -> CtValue {
    let params = obligations
        .param_contract
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|(name, zone)| {
            ct_struct(
                "ObligationParamInfo",
                &[
                    ("name", ct_str(name.clone())),
                    ("zone", ct_str(format!("{zone:?}"))),
                ],
            )
        })
        .collect();
    ct_struct(
        "ObligationInfo",
        &[
            (
                "effect_bound",
                ct_list(
                    obligations
                        .effect_bound
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .cloned()
                        .map(ct_str)
                        .collect(),
                ),
            ),
            ("param_contract", ct_list(params)),
            (
                "variadic",
                ct_list(
                    obligations
                        .variadic
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .copied()
                        .map(ct_bool)
                        .collect(),
                ),
            ),
        ],
    )
}

pub fn build_sendability_info(known: bool) -> CtValue {
    ct_struct("SendabilityInfo", &[("known", ct_bool(known))])
}

pub fn build_movedness_info(moved: bool) -> CtValue {
    ct_struct("MovednessInfo", &[("known", ct_bool(moved))])
}

pub fn build_attribution_info(source: &str, code: Option<&str>) -> CtValue {
    ct_struct(
        "AttributionInfo",
        &[
            ("source", ct_str(source)),
            (
                "code",
                optional_value(code.map(|code| ct_str(code)), "String"),
            ),
        ],
    )
}

pub fn build_track_origin_info(origin: Option<&str>) -> CtValue {
    ct_struct(
        "TrackOriginInfo",
        &[
            ("tracked", ct_bool(origin.is_some())),
            (
                "source",
                optional_value(origin.map(ct_str), "String"),
            ),
        ],
    )
}

pub fn build_view_provenance_info(provenance: &ViewProvenance) -> CtValue {
    ct_struct(
        "ViewProvenanceInfo",
        &[
            (
                "sources",
                ct_list(
                    provenance
                        .sources
                        .iter()
                        .map(|source| ct_str(source.canonical()))
                        .collect(),
                ),
            ),
            ("mutable", ct_bool(provenance.mutable)),
        ],
    )
}

pub fn build_unit_scale_provenance_info(provenance: &UnitScaleProvenance) -> CtValue {
    let (kind, value, source, uncertainty) = match provenance {
        UnitScaleProvenance::Rational => ("Rational", None, None, None),
        UnitScaleProvenance::SymbolicPi {
            numerator,
            denominator,
        } => (
            "SymbolicPi",
            Some(ct_str(format!("{numerator}/{denominator}"))),
            None,
            None,
        ),
        UnitScaleProvenance::Conventional { value, source } => {
            ("Conventional", Some(ct_str(value)), Some(ct_str(source)), None)
        }
        UnitScaleProvenance::Measured {
            central_value,
            standard_uncertainty,
            source,
        } => (
            "Measured",
            Some(ct_str(central_value)),
            Some(ct_str(source)),
            Some(ct_str(standard_uncertainty)),
        ),
    };
    ct_struct(
        "UnitScaleProvenanceInfo",
        &[
            ("kind", typed_enum("UnitScaleProvenanceKind", kind)),
            ("value", optional_value(value, "String")),
            ("source", optional_value(source, "String")),
            ("uncertainty", optional_value(uncertainty, "String")),
        ],
    )
}

pub fn build_maturity_info(maturity: MaturityTag) -> CtValue {
    ct_struct(
        "MaturityInfo",
        &[("level", typed_enum("Maturity", maturity.as_str()))],
    )
}

/// Names and paths remain strings because they are stable fact identities; the
/// fact's meaning is carried by the closed kind and its typed detail record.
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
            ("measure", optional_value(None, "MeasureInfo")),
            ("layout", optional_value(None, "LayoutFact")),
            (
                "classification",
                optional_value(None, "ClassificationInfo"),
            ),
            ("nominal", optional_value(None, "NominalInfo")),
            ("obligation", optional_value(None, "ObligationInfo")),
            ("state", optional_value(None, "StateRef")),
            ("sendability", optional_value(None, "SendabilityInfo")),
            (
                "view_provenance",
                optional_value(None, "ViewProvenanceInfo"),
            ),
            ("movedness", optional_value(None, "MovednessInfo")),
            ("attribution", optional_value(None, "AttributionInfo")),
            ("track_origin", optional_value(None, "TrackOriginInfo")),
            (
                "unit_scale_provenance",
                optional_value(None, "UnitScaleProvenanceInfo"),
            ),
            ("maturity", optional_value(None, "MaturityInfo")),
        ],
    )
}

fn fact_value_with_detail(
    kind: &str,
    name: &str,
    members: impl IntoIterator<Item = String>,
    detail: CtValue,
    detail_field: &str,
) -> CtValue {
    let mut value = fact_value(kind, name, members, None, None);
    if let CtValue::Struct { fields, .. } = &mut value {
        if let Some((_, field)) = fields
            .iter_mut()
            .find(|(field_name, _)| field_name == detail_field)
        {
            *field = CtValue::Present(Box::new(detail));
        }
    }
    value
}

fn fact_kind_for(plane: &str, fact: &KnowledgeFact) -> &'static str {
    let registered = jet_foundation::Registry::row(plane)
        .unwrap_or_else(|| panic!("reflection found unregistered plane `{plane}`"));
    assert_eq!(registered.kind(), jet_foundation::Registry::RowKind::Plane);
    match fact {
        KnowledgeFact::Interval { .. } => "Range",
        KnowledgeFact::Layout { .. } => "Layout",
        KnowledgeFact::Measure(_) => "Measure",
        KnowledgeFact::Dimension(_) => "Dimension",
        KnowledgeFact::Classification(_) => "Classification",
        KnowledgeFact::Nominal(_) => "Nominal",
        KnowledgeFact::Obligation(_) => "Obligation",
    }
}

fn knowledge_fact_value(kind: &str, fact: &KnowledgeFact) -> CtValue {
    match fact {
        KnowledgeFact::Interval { lo, hi } => fact_value(
            kind,
            "range",
            std::iter::empty::<String>(),
            Some(build_range_info(
                i64::try_from(*lo).expect("reflected interval lower bound fits Int"),
                i64::try_from(*hi).expect("reflected interval upper bound fits Int"),
            )),
            None,
        ),
        KnowledgeFact::Dimension(dimension) => fact_value(
            kind,
            "dimension",
            std::iter::empty::<String>(),
            None,
            Some(build_dimension_info(dimension)),
        ),
        KnowledgeFact::Measure(measure) => fact_value_with_detail(
            kind,
            "measure",
            std::iter::empty::<String>(),
            measure_info(measure),
            "measure",
        ),
        KnowledgeFact::Layout { bytes } => fact_value_with_detail(
            kind,
            "layout",
            std::iter::empty::<String>(),
            layout_fact_info(*bytes),
            "layout",
        ),
        KnowledgeFact::Classification(name) => fact_value_with_detail(
            kind,
            "classification",
            [name.clone()],
            classification_info(name),
            "classification",
        ),
        KnowledgeFact::Nominal(name) => fact_value_with_detail(
            kind,
            "nominal",
            [name.clone()],
            nominal_info(name),
            "nominal",
        ),
        KnowledgeFact::Obligation(obligations) => fact_value_with_detail(
            kind,
            "obligation",
            std::iter::empty::<String>(),
            obligation_info(obligations),
            "obligation",
        ),
    }
}

fn registered_fact_kind(name: &str) -> Option<&'static str> {
    jet_foundation::Registry::reflection_kind(name)
}

/// Reflect one row from the shared registration table. A row with no current
/// producer still has a typed, absent payload; it never falls back to a
/// display string or a private menu.
pub fn build_registered_fact_info(name: &str) -> Option<CtValue> {
    let row = jet_foundation::Registry::row(name)?;
    let kind = registered_fact_kind(row.name)?;
    let value = fact_value(
        kind,
        row.name,
        std::iter::empty::<String>(),
        None,
        None,
    );
    let path = if row.name == "Attribution" {
        "report.$attribution".to_string()
    } else {
        row.name.to_string()
    };
    Some(fact_info(kind, row.name, path, value))
}

pub fn build_registered_fact_infos() -> Vec<CtValue> {
    jet_foundation::Registry::fact_rows()
        .filter_map(|row| build_registered_fact_info(row.name))
        .collect()
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

fn declared_function_facts(
    path: &str,
    maturity: Option<MaturityTag>,
    view_provenance: Option<&crate::AST::ViewProvenanceMap>,
) -> Vec<CtValue> {
    let mut facts = Vec::new();
    if let Some(maturity) = maturity {
        facts.push(fact_info(
            "Maturity",
            "maturity",
            format!("{path}.$maturity"),
            fact_value_with_detail(
                "Maturity",
                "maturity",
                std::iter::empty::<String>(),
                build_maturity_info(maturity),
                "maturity",
            ),
        ));
    }
    if let Some(view_provenance) = view_provenance {
        for (slot, provenance) in view_provenance {
            let slot_name = if slot.is_empty() {
                "return".to_string()
            } else {
                slot.join(".")
            };
            facts.push(fact_info(
                "ViewProvenance",
                "view_provenance",
                format!("{path}.$view_provenance.{slot_name}"),
                fact_value_with_detail(
                    "ViewProvenance",
                    "view_provenance",
                    [slot_name],
                    build_view_provenance_info(provenance),
                    "view_provenance",
                ),
            ));
        }
    }
    facts
}

fn type_dimensions(ty: &Type) -> Vec<CtValue> {
    match ty {
        Type::Quantity { dimension, .. } => vec![build_dimension_info(dimension)],
        Type::Tagged { inner, .. } => type_dimensions(inner),
        _ => Vec::new(),
    }
}

fn type_fact_rows(path: &str, ty: &Type) -> Vec<CtValue> {
    ty.knowledge_vector()
        .iter()
        .map(|entry| {
            let kind = fact_kind_for(entry.plane, &entry.fact);
            let name = kind.to_ascii_lowercase();
            let suffix = if entry.path.is_empty() {
                format!("@{name}")
            } else {
                format!("{}.@{name}", entry.path.join("."))
            };
            fact_info(
                kind,
                &name,
                format!("{path}.{suffix}"),
                knowledge_fact_value(kind, &entry.fact),
            )
        })
        .collect()
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
                Some(build_range_info(start, end)),
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
                Some(build_dimension_info(dimension)),
            ),
        ));
    }
    facts
}

/// D-TYPE2-PLANE1=A: a marker reflects as the typed record its registry row
/// describes, at every position it may be written — the type, a field, a
/// method, a variant. Nothing reflects a marker as a bare name.
fn marker_infos(
    markers: &[Marker],
    vocabulary: Option<&jet_foundation::Policy::MarkerVocabulary>,
) -> Vec<CtValue> {
    markers
        .iter()
        .map(|marker| marker_info(marker, vocabulary))
        .collect()
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

fn marker_info(
    marker: &Marker,
    vocabulary: Option<&jet_foundation::Policy::MarkerVocabulary>,
) -> CtValue {
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
            let declaration_parameter = vocabulary
                .and_then(|vocabulary| vocabulary.declaration(&marker.name))
                .and_then(|declaration| {
                    marker
                        .arg_labels
                        .get(index)
                        .and_then(|label| label.as_ref())
                        .and_then(|(name, _)| {
                            declaration
                                .params
                                .iter()
                                .find(|parameter| parameter.name == *name)
                        })
                        .or_else(|| {
                            declaration
                                .params
                                .iter()
                                .filter(|parameter| !parameter.name.starts_with('$'))
                                .nth(index)
                        })
                });
            let binding = bindings
                .as_ref()
                .and_then(|bindings| bindings.iter().find(|binding| binding.source_index == index));
            let parameter = binding
                .and_then(|binding| binding.parameter_index)
                .and_then(|parameter| row.and_then(|row| row.signature.params.get(parameter)));
            let (argument_name, source_type) = if let Some(parameter) = declaration_parameter {
                (
                    parameter.name.clone(),
                    parameter
                        .ty
                        .as_ref()
                        .map(Type::name)
                        .unwrap_or_else(|| "Value".to_string()),
                )
            } else {
                (
                    parameter
                        .map(|parameter| parameter.name.to_string())
                        .unwrap_or_else(|| "value".to_string()),
                    parameter
                        .map(|parameter| parameter.source_type.to_string())
                        .unwrap_or_else(|| "Value".to_string()),
                )
            };
            ct_struct(
                "MarkerArgInfo",
                &[
                    ("name", ct_str(argument_name)),
                    ("ty", ct_str(source_type.clone())),
                    ("value", marker_arg_value(argument, &source_type)),
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
    build_field_info_with_vocabulary(field, None)
}

fn build_field_info_with_vocabulary(
    field: &Field,
    vocabulary: Option<&jet_foundation::Policy::MarkerVocabulary>,
) -> CtValue {
    ct_struct(
        "FieldInfo",
        &[
            ("name", ct_str(field.name.clone())),
            ("ty", ct_str(field.ty.name())),
            (
                "markers",
                ct_list(marker_infos(&field.serde_markers, vocabulary)),
            ),
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
    build_method_info_with_vocabulary(method, None)
}

fn build_method_info_with_vocabulary(
    method: &Func,
    vocabulary: Option<&jet_foundation::Policy::MarkerVocabulary>,
) -> CtValue {
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
    let facts = declared_function_facts(
        &method.name,
        method.maturity,
        method.return_view_provenance.as_ref(),
    );
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
            ("facts", ct_list(facts)),
            // D-REFLECT1: the retained marker nodes, same source as every other
            // consumer. This was hardcoded empty, so reflection reported that a
            // method carried no markers no matter what was written on it.
            (
                "markers",
                ct_list(marker_infos(&method.markers, vocabulary)),
            ),
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
    type_level_marker_views_with_vocabulary(type_markers, serde_markers, derives, None)
}

fn type_level_marker_views_with_vocabulary(
    type_markers: &[Marker],
    serde_markers: &[Marker],
    derives: &[(String, crate::Diagnostics::Span)],
    vocabulary: Option<&jet_foundation::Policy::MarkerVocabulary>,
) -> (Vec<CtValue>, Vec<CtValue>) {
    let written_source = if type_markers.is_empty() {
        serde_markers
    } else {
        type_markers
    };
    let mut written = marker_infos(written_source, vocabulary);
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

/// A state identity is not a display-only path. Keep the canonical path for
/// diagnostics, but expose owner and state as fields of a typed record.
pub fn build_state_ref(owner: &str, state: &str) -> CtValue {
    ct_struct(
        "StateRef",
        &[
            ("owner", ct_str(owner)),
            ("name", ct_str(state)),
            ("path", ct_str(state_path(owner, state))),
        ],
    )
}

/// Build the typed state values returned by `Type.@states`.
pub fn build_state_refs(owner: &str, states: &[String]) -> CtValue {
    ct_list(
        states
            .iter()
            .map(|state| build_state_ref(owner, state))
            .collect(),
    )
}

/// Build the same state rows that the aggregate `TypeInfo.states` view uses.
pub fn build_state_infos(owner: &str, states: &[String]) -> CtValue {
    ct_list(
        states
            .iter()
            .map(|state| {
                ct_struct(
                    "StateInfo",
                    &[
                        ("name", ct_str(state)),
                        ("path", build_state_ref(owner, state)),
                    ],
                )
            })
            .collect(),
    )
}

/// Build the typed effect values returned by `fn.@effects`.
pub fn build_effect_info(effects: &[String]) -> CtValue {
    ct_struct(
        "EffectInfo",
        &[("values", ct_list(effects.iter().cloned().map(ct_str).collect()))],
    )
}

/// D-FACT-READ1=A: resolve a direct fact read while top-level comptime
/// bindings are evaluated. This pass runs before sema has built a module
/// `TypeRegistry`, so it reads the same registered plane through the source
/// declarations that will be registered moments later.
pub fn fact_read_value(
    expr: &Expr,
    items: &[Item],
    build_facts: &jet_foundation::Facts::BuildFactSnapshot,
) -> Option<CtValue> {
    let Expr::Field(subject, member, _) = expr else {
        return None;
    };
    if let Some(path) = fact_path(expr) {
        if let Some(key) = jet_foundation::Registry::build_setting_key(&path) {
            return build_setting_value(build_facts, key);
        }
        if let Some(read) = jet_foundation::Registry::build_fact_read(&path) {
            return build_fact_value(build_facts, read);
        }
        if let Some(name) = path.strip_prefix(crate::Syntax::COMPILER_BUILD_FACT_SETTINGS_PREFIX) {
            return build_setting_value(build_facts, name);
        }
    }
    let is_build_subject = matches!(subject.as_ref(), Expr::ComptimeName { name, .. } if name == "@build");
    let read = if member == crate::Syntax::BUILD_INFO_PROFILE {
        if !is_build_subject {
            return None;
        }
        jet_foundation::Registry::fact_read(crate::Syntax::COMPILER_BUILD_FACT_PROFILE)?
    } else {
        jet_foundation::Registry::fact_read(member)?
    };
    let subject_name = match subject.as_ref() {
        Expr::Ident(name, _) => name.as_str(),
        _ => return None,
    };
    match read {
        jet_foundation::Registry::FactRead::Range => items.iter().find_map(|item| match item {
            Item::Distinct(def) if def.name == subject_name => def
                .range
                .map(|(start, end, _)| build_range_info(start, end)),
            Item::UnitFamily(family) => family
                .distinct_defs()
                .into_iter()
                .find(|def| def.name == subject_name)
                .and_then(|def| def.range)
                .map(|(start, end, _)| build_range_info(start, end)),
            _ => None,
        }),
        jet_foundation::Registry::FactRead::Dimension => items.iter().find_map(|item| match item {
            Item::Distinct(def) if def.name == subject_name => def
                .quantity
                .as_ref()
                .map(|(dimension, _)| build_dimension_info(dimension)),
            Item::UnitFamily(family) => family
                .distinct_defs()
                .into_iter()
                .find(|def| def.name == subject_name)
                .and_then(|def| def.quantity)
                .map(|(dimension, _)| build_dimension_info(&dimension)),
            _ => None,
        }),
        jet_foundation::Registry::FactRead::States => items.iter().find_map(|item| match item {
            Item::StateDecl(decl) if decl.type_name == subject_name => Some(build_state_infos(
                subject_name,
                &decl.states.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>(),
            )),
            _ => None,
        }),
        jet_foundation::Registry::FactRead::Effects => items.iter().find_map(|item| match item {
            Item::Func(function) if function.name == subject_name => Some(build_effect_info(
                &function
                    .declared_effects
                    .as_ref()
                    .map(|effects| {
                        effects
                            .iter()
                            .map(|(name, _)| name.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            )),
            _ => None,
        }),
        jet_foundation::Registry::FactRead::BuildProfile
        | jet_foundation::Registry::FactRead::BuildPackageName
        | jet_foundation::Registry::FactRead::BuildPackageVersion
        | jet_foundation::Registry::FactRead::BuildOS
        | jet_foundation::Registry::FactRead::BuildStampGit
        | jet_foundation::Registry::FactRead::BuildStampDirty
        | jet_foundation::Registry::FactRead::BuildStampToolchain
        | jet_foundation::Registry::FactRead::BuildStampAt => {
            build_fact_value(build_facts, read)
        }
        jet_foundation::Registry::FactRead::Layout
        | jet_foundation::Registry::FactRead::Name
        | jet_foundation::Registry::FactRead::Fields => None,
    }
}

fn fact_path(expr: &Expr) -> Option<String> {
    match expr {
        Expr::ComptimeName { name, .. } | Expr::Ident(name, _) => Some(name.clone()),
        Expr::Field(base, member, _) => Some(format!("{}.{}", fact_path(base)?, member)),
        _ => None,
    }
}

/// D-FACT-READ1=A: convert one registered build row to the shared compile-time
/// value carrier. Sema and comptime use this same conversion before codegen.
pub fn build_fact_value(
    snapshot: &jet_foundation::Facts::BuildFactSnapshot,
    read: jet_foundation::Registry::FactRead,
) -> Option<CtValue> {
    build_fact_scalar(snapshot.value(read)?)
}

/// Resolve one declared setting from the already-seeded build snapshot.
/// Settings and the fixed build facts share this conversion so the top-level
/// comptime evaluator and ordinary sema inference cannot disagree.
pub fn build_setting_value(
    snapshot: &jet_foundation::Facts::BuildFactSnapshot,
    key: &str,
) -> Option<CtValue> {
    build_fact_scalar(snapshot.setting(key)?.value.clone())
}

fn build_fact_scalar(value: jet_foundation::Facts::BuildFactValue) -> Option<CtValue> {
    match value {
        jet_foundation::Facts::BuildFactValue::Text(value) => Some(CtValue::Str(value)),
        jet_foundation::Facts::BuildFactValue::OptionalText(Some(value)) => {
            Some(CtValue::Present(Box::new(CtValue::Str(value))))
        }
        jet_foundation::Facts::BuildFactValue::OptionalText(None) => {
            Some(CtValue::absent(crate::AST::Type::String))
        }
        jet_foundation::Facts::BuildFactValue::Bool(value) => Some(CtValue::Bool(value)),
        jet_foundation::Facts::BuildFactValue::Int(value) => Some(CtValue::Int(value)),
        jet_foundation::Facts::BuildFactValue::Char(value) => Some(CtValue::Char(value)),
        jet_foundation::Facts::BuildFactValue::Enum { type_name, variant } => Some(CtValue::Enum {
            type_name,
            variant,
            args: Vec::new(),
        }),
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
                build_state_ref(
                    owner,
                    transition
                        .from
                        .as_deref()
                        .unwrap_or("_"),
                ),
            ),
            ("to", build_state_ref(owner, &transition.to)),
        ],
    ))
}

fn reflected_state_fact(owner: &str, state: &str) -> CtValue {
    let path = state_path(owner, state);
    fact_info(
        "State",
        state,
        path.clone(),
        {
            let mut value = fact_value("State", state, [path], None, None);
            if let CtValue::Struct { fields, .. } = &mut value {
                if let Some((_, field)) = fields.iter_mut().find(|(name, _)| name == "state") {
                    *field = CtValue::Present(Box::new(build_state_ref(owner, state)));
                }
            }
            value
        },
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
    build_struct_type_info_with_path_and_vocabulary(s, states, path, None)
}

/// Build the struct reflection handle against the bundle's one marker
/// vocabulary. Source-declared marker rows are therefore reflected with their
/// declared argument types, not as an untyped fallback beside Prelude rows.
pub fn build_struct_type_info_with_path_and_vocabulary(
    s: &StructDef,
    states: &[String],
    path: &str,
    vocabulary: Option<&jet_foundation::Policy::MarkerVocabulary>,
) -> CtValue {
    let fields_info: Vec<CtValue> = s
        .reflection_fields()
        .map(|field| build_field_info_with_vocabulary(field, vocabulary))
        .collect();
    let dimensions = s
        .reflection_fields()
        .flat_map(|field| type_dimensions(&field.ty))
        .collect::<Vec<_>>();
    let layout = build_struct_layout_info(s);
    let methods_info: Vec<CtValue> = s
        .methods
        .iter()
        .map(|method| build_method_info_with_vocabulary(method, vocabulary))
        .collect();
    let type_params_info: Vec<CtValue> = s.type_params.iter().map(build_type_param_info).collect();
    let state_info = states
        .iter()
        .map(|state| {
            ct_struct(
                "StateInfo",
                &[
                    ("name", ct_str(state)),
                    ("path", build_state_ref(&s.name, state)),
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
    for field in s.reflection_fields() {
        facts.extend(type_fact_rows(&format!("{}.{}", s.name, field.name), &field.ty));
    }
    let (markers, expanded_markers) = type_level_marker_views_with_vocabulary(
        &s.type_markers,
        &s.serde_markers,
        &s.derives,
        vocabulary,
    );
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
        .map(|(dimension, _)| vec![build_dimension_info(dimension)])
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
            (
                "markers",
                ct_list(marker_infos(&variant.serde_markers, None)),
            ),
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
    let declared_facts = declared_function_facts(
        &qualified,
        func.maturity,
        func.return_view_provenance.as_ref(),
    );
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
            ("facts", ct_list(declared_facts)),
        ],
    )
}

/// Build the function reflection handle passed into a source rule body. The
/// function body is checked in the bundle that owns the rule, so the target's
/// stable source shape is the useful projection here; program-wide effect
/// facts remain a separate reflection input.
pub fn build_function_type_info(func: &Func) -> CtValue {
    build_function_info(func, 0, "", &ProgramSemanticFacts::default())
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
        let state_path = |value: &CtValue, expected: &str| {
            matches!(
                value,
                CtValue::Struct { type_name, fields }
                    if type_name == "StateRef"
                        && fields.iter().any(|(name, value)|
                            name == "path" && matches!(value, CtValue::Str(path) if path == expected))
            )
        };
        assert!(fields
            .iter()
            .any(|(name, value)| name == "from" && state_path(value, "Door.State.Closed")));
        assert!(fields
            .iter()
            .any(|(name, value)| name == "to" && state_path(value, "Door.State.Open")));
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
