//! Integration tests for D-METAREFLECT1 / D-REFLECT1 rich reflection.

use jet::Comptime::{build_struct_type_info, CtValue};
use jet::Diagnostics::Span;
use jet::AST::{
    AccessConvention, Expr, Field, Func, Marker, Param, StructDef, Type, TypeParam,
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
        params: vec![Param {
            convention: AccessConvention::Read,
            name: "self".to_string(),
            name_span: span(),
            ty: Type::Named("Self".to_string()),
            ty_span: span(),
            default: None,
            variadic: false,
            variadic_bound_list: None, declared_view_from_names: None,
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
        is_inline: false,
        is_inline_always: false,
        inline_span: None,
        meta: None,
        is_task: false,
        task_span: None,
        every: None,
        inline_foreign: None,
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
    assert_eq!(list_len(struct_field(&info, "fields")), 2);
    assert_eq!(list_len(struct_field(&info, "methods")), 1);
    assert_eq!(list_len(struct_field(&info, "type_params")), 1);
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
        struct_field(&info, "marker_names"),
        CtValue::List(xs) if matches!(&xs[0], CtValue::Str(name) if name == "Debug")
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
fn marker_arguments_are_typed_and_keep_the_compatibility_name() {
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
    assert!(matches!(
        struct_field(&info, "marker_names"),
        CtValue::List(names)
            if matches!(&names[0], CtValue::Str(name) if name == "Inline")
    ));
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
