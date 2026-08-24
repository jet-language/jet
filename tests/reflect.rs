//! Integration tests for D-METAREFLECT1 / D-REFLECT1 rich reflection.

mod common;

use jet::Comptime::{
    build_distinct_type_info, build_registered_fact_info, build_struct_type_info, CtReport, CtValue,
};
use jet::Diagnostics::Span;
use jet::AST::{
    AccessConvention, Dimension, DistinctDef, Expr, Field, Func, InternalTag, Marker, Param,
    ParamZone, QuantityKind, StructDef, TagMarker, Type, TypeParam,
};

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
        name: name.to_string(),
        name_span: span(),
        type_params: Vec::new(),
        head_pattern: None,
        params: vec![Param {
            convention: AccessConvention::Read,
            root: false,
            name: "self".to_string(),
            name_span: span(),
            public_label: None,
            zone: ParamZone::Either,
            ty: Type::Named("Self".to_string()),
            ty_span: span(),
            default: None,
            variadic: false,
            variadic_bound_list: None,
            declared_view_from_names: None,
        }],
        return_type: Some(Type::Named("String".to_string())),
        return_type_span: Some(span()),
        return_view_provenance: None,
        declared_return_view_provenance: None,
        gc_return: false,
        gc_scope: false,
        is_unsafe: false,
        unsafe_reason: None,
        unsafe_span: None,
        is_pure: false,
        is_sanitizer: false,
        scrub_tag: None,
        is_reactive: false,
        reactive_upgrades: Vec::new(),
        is_replayable: false,
        replayable_span: None,
        declared_effects: None,
        pre: vec![],
        post: vec![],
        effect_via: None,
        state_requires: None,
        state_transition: None,
        web_marker: None,
        is_must_use: false,
        must_use_span: None,
        maturity: None,
        maturity_span: None,
        kernel: None,
        is_inline: false,
        is_inline_always: false,
        inline_span: None,
        meta: None,
        is_job: false,
        job_span: None,
        every: None,
        job_metadata: None,
        inline_foreign: None,
        undo: None,
        markers: Vec::new(),
        compiler_generated: false,
        body: Vec::new(),
    }
}

fn list_len(v: &CtValue) -> usize {
    match v {
        CtValue::List(xs) => xs.len(),
        _ => panic!("expected list, got {v:?}"),
    }
}

fn struct_field<'a>(v: &'a CtValue, name: &str) -> &'a CtValue {
    let CtValue::Struct { fields, .. } = v else {
        panic!("expected struct");
    };
    &fields.iter().find(|(n, _)| n == name).unwrap().1
}

fn list_struct_field<'a>(v: &'a CtValue, index: usize, name: &str) -> &'a CtValue {
    let CtValue::List(values) = v else {
        panic!("expected list");
    };
    struct_field(&values[index], name)
}

#[test]
fn type_info_exposes_methods_type_params_and_markers() {
    let s = StructDef {
        span: span(),
        is_pub: true,
        is_package_pub: false,
        name: "Box".to_string(),
        name_span: span(),
        type_params: vec![TypeParam {
            name: "T".to_string(),
            name_span: span(),
            bounds: vec!["Comparable".to_string()],
        }],
        fields: vec![field("value", "T", true), field("hidden", "Int", false)],
        methods: vec![method("show", true)],
        cli_bindings: Vec::new(),
        trait_impls: Vec::new(),
        derives: vec![("Debug".to_string(), span())],
        auto_derive_default: true,
        is_published_schema: false,
        published_schema_span: None,
        is_single_use: false,
        single_use_span: None,
        layout: None,
        layout_span: None,
        serde_markers: Vec::new(),
        type_markers: Vec::new(),
        is_must_use: false,
        must_use_span: None,
        validate_block: Vec::new(),
        validate_span: None,
    };
    let info = build_struct_type_info(&s);
    assert!(matches!(
        struct_field(&info, "name"),
        CtValue::Str(name) if name.as_str() == "Box"
    ));
    assert!(matches!(
        struct_field(&info, "path"),
        CtValue::Str(path) if path.as_str() == "Box"
    ));
    assert_eq!(list_len(struct_field(&info, "fields")), 2);
    assert_eq!(list_len(struct_field(&info, "methods")), 1);
    assert_eq!(list_len(struct_field(&info, "type_params")), 1);
    assert!(matches!(
        list_struct_field(struct_field(&info, "fields"), 0, "span"),
        CtValue::Struct { fields, .. }
            if fields.iter().any(|(name, value)| name == "start" && matches!(value, CtValue::Int(0)))
    ));
    assert!(matches!(
        list_struct_field(struct_field(&info, "methods"), 0, "span"),
        CtValue::Struct { fields, .. }
            if fields.iter().any(|(name, value)| name == "end" && matches!(value, CtValue::Int(1)))
    ));
    assert!(matches!(
        list_struct_field(struct_field(&info, "type_params"), 0, "span"),
        CtValue::Struct { fields, .. }
            if fields.iter().any(|(name, value)| name == "start" && matches!(value, CtValue::Int(0)))
    ));
    let markers = struct_field(&info, "markers");
    assert_eq!(list_len(markers), 1);
    assert!(matches!(
        markers,
        CtValue::List(xs)
            if matches!(
                &xs[0],
                CtValue::Struct { fields, .. }
                    if fields.iter().any(|(name, value)|
                        name == "name" && matches!(value, CtValue::Str(value) if value == "Debug"))
            )
    ));
    assert!(matches!(
        struct_field(&info, "expanded_markers"),
        CtValue::List(xs) if xs.is_empty()
    ));
}

#[test]
fn field_info_carries_visibility() {
    let s = StructDef {
        span: span(),
        is_pub: true,
        is_package_pub: false,
        name: "Secret".to_string(),
        name_span: span(),
        type_params: Vec::new(),
        fields: vec![field("visible", "Int", true), field("hidden", "Int", false)],
        methods: Vec::new(),
        cli_bindings: Vec::new(),
        trait_impls: Vec::new(),
        derives: Vec::new(),
        auto_derive_default: true,
        is_published_schema: false,
        published_schema_span: None,
        is_single_use: false,
        single_use_span: None,
        layout: None,
        layout_span: None,
        serde_markers: Vec::new(),
        type_markers: Vec::new(),
        is_must_use: false,
        must_use_span: None,
        validate_block: Vec::new(),
        validate_span: None,
    };
    let info = build_struct_type_info(&s);
    let CtValue::List(fields) = struct_field(&info, "fields") else {
        panic!("fields");
    };
    let CtValue::Struct {
        fields: hidden_fields,
        ..
    } = &fields[1]
    else {
        panic!("field struct");
    };
    let is_pub = &hidden_fields.iter().find(|(n, _)| n == "is_pub").unwrap().1;
    assert!(matches!(is_pub, CtValue::Bool(false)));
}

#[test]
fn marker_arguments_are_typed_in_the_written_view() {
    let marker = Marker {
        name: "Inline".to_string(),
        negated: false,
        name_span: span(),
        args: vec![Expr::Ident("Always".to_string(), span())],
        arg_labels: vec![None],
        span: span(),
        ct: None,
    };
    let s = StructDef {
        span: span(),
        is_pub: true,
        is_package_pub: false,
        name: "Hot".to_string(),
        name_span: span(),
        type_params: Vec::new(),
        fields: Vec::new(),
        methods: Vec::new(),
        cli_bindings: Vec::new(),
        trait_impls: Vec::new(),
        derives: Vec::new(),
        auto_derive_default: true,
        is_published_schema: false,
        published_schema_span: None,
        is_single_use: false,
        single_use_span: None,
        layout: None,
        layout_span: None,
        serde_markers: Vec::new(),
        type_markers: vec![marker],
        is_must_use: false,
        must_use_span: None,
        validate_block: Vec::new(),
        validate_span: None,
    };

    let info = build_struct_type_info(&s);
    let CtValue::List(markers) = struct_field(&info, "markers") else {
        panic!("markers");
    };
    let CtValue::Struct { fields, .. } = &markers[0] else {
        panic!("marker");
    };
    let CtValue::List(args) = &fields.iter().find(|(name, _)| name == "args").unwrap().1 else {
        panic!("args");
    };
    assert!(matches!(
        &args[0],
        CtValue::Struct { fields, .. }
            if fields.iter().any(|(name, value)|
                name == "ty" && matches!(value, CtValue::Str(value) if value == "InlineMode"))
            && fields.iter().any(|(name, value)|
                name == "value"
                    && matches!(
                        value,
                        CtValue::Enum { type_name, variant, args }
                            if type_name == "InlineMode"
                                && variant == "Always"
                                && args.is_empty()
                    ))
    ));
}

#[test]
fn distinct_capability_marker_is_visible_in_reflection() {
    let marker = Marker {
        name: "Comparable".to_string(),
        negated: false,
        name_span: span(),
        args: Vec::new(),
        arg_labels: Vec::new(),
        span: span(),
        ct: None,
    };
    let info = build_distinct_type_info(
        &DistinctDef {
            is_pub: true,
            is_package_pub: false,
            type_markers: vec![marker],
            derives: vec![("Comparable".to_string(), span())],
            quantity: None,
            name: "CustomerId".to_string(),
            name_span: span(),
            base: Type::Int,
            base_span: span(),
            range: None,
            span: span(),
        },
        "main",
    );
    assert!(matches!(
        struct_field(&info, "markers"),
        CtValue::List(values)
            if values.iter().any(|value| matches!(
                value,
                CtValue::Struct { fields, .. }
                    if fields.iter().any(|(name, value)|
                        name == "name" && matches!(value, CtValue::Str(value) if value == "Comparable"))
            ))
    ));
    assert!(matches!(
        struct_field(&info, "expanded_markers"),
        CtValue::List(values) if values.len() == 1
    ));
}

#[test]
fn registered_type_planes_reflect_as_typed_values() {
    let mut fields = Vec::new();
    for (name, ty) in [
        (
            "bounded",
            Type::IntN {
                signed: true,
                bits: 8,
            },
        ),
        (
            "shape",
            Type::FixedList {
                elem: Box::new(Type::Int),
                len: jet::AST::Measure::literal("length", 4),
            },
        ),
        (
            "quantity",
            Type::Quantity {
                base: Box::new(Type::Int),
                dimension: Dimension::base("Length"),
            },
        ),
        (
            "classified",
            Type::Tagged {
                marker: TagMarker::User("Audited".to_string()),
                inner: Box::new(Type::Int),
            },
        ),
        (
            "nominal",
            Type::Tagged {
                marker: TagMarker::Internal(InternalTag::CoreCryptoNominal),
                inner: Box::new(Type::Named("Secret".to_string())),
            },
        ),
        (
            "callable",
            Type::Fn {
                params: vec![Type::Int],
                ret: Some(Box::new(Type::Int)),
                effect_bound: None,
                param_contract: Some(vec![("value".to_string(), ParamZone::Either)]),
                call_metadata: None,
                return_view_provenance: None,
            },
        ),
        ("approximate", Type::Float32),
    ] {
        let mut field = field(name, "Int", true);
        field.ty = ty;
        fields.push(field);
    }
    let info = build_struct_type_info(&StructDef {
        span: span(),
        is_pub: true,
        is_package_pub: false,
        name: "Planes".to_string(),
        name_span: span(),
        type_params: Vec::new(),
        fields,
        methods: Vec::new(),
        cli_bindings: Vec::new(),
        trait_impls: Vec::new(),
        derives: Vec::new(),
        auto_derive_default: true,
        is_published_schema: false,
        published_schema_span: None,
        is_single_use: false,
        single_use_span: None,
        layout: None,
        layout_span: None,
        serde_markers: Vec::new(),
        type_markers: Vec::new(),
        is_must_use: false,
        must_use_span: None,
        validate_block: Vec::new(),
        validate_span: None,
    });
    let CtValue::List(facts) = struct_field(&info, "facts") else {
        panic!("facts");
    };
    for expected in [
        "Range",
        "Layout",
        "Measure",
        "Exactness",
        "Dimension",
        "Classification",
        "Nominal",
        "Obligation",
    ] {
        let fact = facts
            .iter()
            .find(|fact| {
                matches!(
                    struct_field(fact, "kind"),
                    CtValue::Enum { variant, .. } if variant == expected
                )
            })
            .unwrap_or_else(|| panic!("missing typed `{expected}` fact in {facts:?}"));
        assert!(matches!(
            struct_field(fact, "value"),
            CtValue::Struct { .. }
        ));
    }

    let fact_of_kind = |kind: &str| {
        facts
            .iter()
            .find(|fact| matches!(struct_field(fact, "kind"), CtValue::Enum { variant, .. } if variant == kind))
            .unwrap_or_else(|| panic!("missing `{kind}` payload in {facts:?}"))
    };
    let range = struct_field(struct_field(fact_of_kind("Range"), "value"), "range");
    let CtValue::Present(range) = range else {
        panic!("range payload is not present");
    };
    assert!(matches!(struct_field(range, "start"), CtValue::Int(-128)));
    assert!(matches!(struct_field(range, "end"), CtValue::Int(127)));

    let layout = struct_field(struct_field(fact_of_kind("Layout"), "value"), "layout");
    let CtValue::Present(layout) = layout else {
        panic!("layout payload is not present");
    };
    assert!(matches!(struct_field(layout, "bytes"), CtValue::Int(1)));

    let measure = struct_field(struct_field(fact_of_kind("Measure"), "value"), "measure");
    let CtValue::Present(measure) = measure else {
        panic!("measure payload is not present");
    };
    assert!(matches!(
        (struct_field(measure, "kind"), struct_field(measure, "value")),
        (CtValue::Str(kind), CtValue::Present(value))
            if kind == "length" && matches!(value.as_ref(), CtValue::Int(4))
    ));

    let exactness = facts
        .iter()
        .filter(|fact| matches!(struct_field(fact, "kind"), CtValue::Enum { variant, .. } if variant == "Exactness"))
        .find(|fact| {
            matches!(
                struct_field(struct_field(fact, "value"), "exactness"),
                CtValue::Present(value)
                    if matches!(
                        struct_field(value, "kind"),
                        CtValue::Enum { variant, .. } if variant == "Approximate"
                    )
            )
        })
        .expect("approximate exactness payload");
    let exactness = struct_field(struct_field(exactness, "value"), "exactness");
    let CtValue::Present(exactness) = exactness else {
        panic!("exactness payload is not present");
    };
    assert!(matches!(
        struct_field(exactness, "precision"),
        CtValue::Present(value) if matches!(value.as_ref(), CtValue::Int(24))
    ));

    let dimension = struct_field(
        struct_field(fact_of_kind("Dimension"), "value"),
        "dimension",
    );
    let CtValue::Present(dimension) = dimension else {
        panic!("dimension payload is not present");
    };
    assert!(matches!(
        struct_field(dimension, "axes"),
        CtValue::List(axes) if axes.iter().any(|axis|
            matches!(
                (struct_field(axis, "name"), struct_field(axis, "exponent")),
                (CtValue::Str(name), CtValue::Int(1)) if name == "Length"
            ))
    ));

    let classification = struct_field(
        struct_field(fact_of_kind("Classification"), "value"),
        "classification",
    );
    let CtValue::Present(classification) = classification else {
        panic!("classification payload is not present");
    };
    assert!(
        matches!(struct_field(classification, "name"), CtValue::Str(name) if name == "Audited")
    );

    let nominal = struct_field(struct_field(fact_of_kind("Nominal"), "value"), "nominal");
    let CtValue::Present(nominal) = nominal else {
        panic!("nominal payload is not present");
    };
    assert!(matches!(struct_field(nominal, "name"), CtValue::Str(name) if name == "core.crypto"));

    let obligation = struct_field(
        struct_field(fact_of_kind("Obligation"), "value"),
        "obligation",
    );
    let CtValue::Present(obligation) = obligation else {
        panic!("obligation payload is not present");
    };
    let CtValue::List(params) = struct_field(obligation, "param_contract") else {
        panic!("obligation parameter contract");
    };
    assert!(matches!(
        params.first().map(|param| (struct_field(param, "name"), struct_field(param, "zone"))),
        Some((CtValue::Str(name), CtValue::Enum { type_name, variant, .. }))
            if name == "value" && type_name == "ParamZone" && variant == "Either"
    ));
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
            span: span(),
        },
        "main",
    );
    assert!(matches!(
        struct_field(&info, "name"),
        CtValue::Str(name) if name.as_str() == "Severity"
    ));
    assert!(matches!(
        struct_field(&info, "path"),
        CtValue::Str(path) if path.as_str() == "main.Severity"
    ));
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
    let CtValue::Present(range_value) = struct_field(struct_field(range, "value"), "range") else {
        panic!("range fact must carry a present Range");
    };
    assert!(matches!(
        struct_field(range_value, "start"),
        CtValue::Int(0)
    ));
    assert!(matches!(struct_field(range_value, "end"), CtValue::Int(10)));
    assert!(matches!(
        struct_field(struct_field(range, "value"), "dimension"),
        CtValue::Failed(CtReport::Clean(_))
    ));

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
    assert!(matches!(
        struct_field(struct_field(dimension, "value"), "range"),
        CtValue::Failed(CtReport::Clean(_))
    ));
}

#[test]
fn orphan_fact_rows_are_typed_and_readable() {
    for (name, kind) in [
        ("Sendability", "Sendability"),
        ("Attribution", "Attribution"),
        ("TrackOrigin", "TrackOrigin"),
        ("ViewProvenance", "ViewProvenance"),
        ("UnitScaleProvenance", "UnitScaleProvenance"),
        ("Maturity", "Maturity"),
    ] {
        let info = build_registered_fact_info(name).expect("registered orphan fact");
        assert!(matches!(
            struct_field(&info, "kind"),
            CtValue::Enum { variant, .. } if variant == kind
        ));
        assert!(matches!(
            struct_field(&info, "value"),
            CtValue::Struct { .. }
        ));
        if name == "Attribution" {
            assert!(matches!(
                struct_field(&info, "path"),
                CtValue::Str(path) if path == "report.@attribution"
            ));
        }
    }
}
