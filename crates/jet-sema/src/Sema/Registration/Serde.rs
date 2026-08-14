use super::*;
use crate::AST::{
    AccessConvention, BinOp, Call, CallArg, CallArgFlags, CtReport, CtValue, EnumLitArg,
    Expr, ForKind, Func, ImplDef, IndexKind, Item, LValue, Param, PatSlot, Pattern, Stmt,
    SwitchArm, TryConvert, Type, TypeParam, VariantPayload,
};

/// D-SERDE2=A / I3: built-in codecs are ordinary Jet AST items. The builder
/// below shares the same method/body representation as hand-written codecs;
/// only the declaration data comes from the reflected struct or enum.
pub(crate) fn expand_builtin_serde_items(items: &mut Vec<Item>, _diags: &mut Vec<Diagnostic>) {
    let auto = crate::Traits::TraitRegistry::auto_derives_for_items(items);
    let mut generated_items = Vec::new();
    let snapshot = items.clone();
    for item in &snapshot {
        match item {
            Item::Struct(s)
                if has_codec(&s.derives)
                    || auto.auto_encode.contains(&s.name)
                    || auto.auto_decode.contains(&s.name) =>
            {
                let mut derived = s.clone();
                add_auto_codec_marker(
                    &mut derived.derives,
                    crate::Generics::ENCODE,
                    &auto.auto_encode,
                    &derived.name,
                    derived.name_span,
                );
                add_auto_codec_marker(
                    &mut derived.derives,
                    crate::Generics::DECODE,
                    &auto.auto_decode,
                    &derived.name,
                    derived.name_span,
                );
                generated_items.extend(struct_codec_items(&derived));
            }
            Item::Enum(e)
                if has_codec(&e.derives)
                    || auto.auto_encode.contains(&e.name)
                    || auto.auto_decode.contains(&e.name) =>
            {
                let mut derived = e.clone();
                add_auto_codec_marker(
                    &mut derived.derives,
                    crate::Generics::ENCODE,
                    &auto.auto_encode,
                    &derived.name,
                    derived.name_span,
                );
                add_auto_codec_marker(
                    &mut derived.derives,
                    crate::Generics::DECODE,
                    &auto.auto_decode,
                    &derived.name,
                    derived.name_span,
                );
                generated_items.extend(enum_codec_items(&derived));
            }
            _ => {}
        }
    }
    items.extend(generated_items);
}

fn add_auto_codec_marker(
    derives: &mut Vec<(String, Span)>,
    trait_name: &str,
    automatic: &std::collections::HashSet<String>,
    type_name: &str,
    span: Span,
) {
    if automatic.contains(type_name) && !has_derive(derives, trait_name) {
        derives.push((trait_name.to_string(), span));
    }
}

fn has_codec(derives: &[(String, Span)]) -> bool {
    derives
        .iter()
        .any(|(name, _)| matches!(name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE))
}

fn has_derive(derives: &[(String, Span)], name: &str) -> bool {
    derives.iter().any(|(derive, _)| derive == name)
}

fn codec_params(
    params: &[TypeParam],
    wire_types: impl IntoIterator<Item = Type>,
    encode: bool,
    decode: bool,
) -> Vec<TypeParam> {
    let wire_types = wire_types.into_iter().collect::<Vec<_>>();
    let mut params = params.to_vec();
    for param in &mut params {
        let reaches_wire = wire_types.iter().any(|ty| {
            crate::Generics::free_type_params(ty).contains(&param.name)
        });
        if reaches_wire && encode && !param.bounds.iter().any(|bound| bound == crate::Generics::ENCODE) {
            param.bounds.push(crate::Generics::ENCODE.to_string());
        }
        if reaches_wire && decode && !param.bounds.iter().any(|bound| bound == crate::Generics::DECODE) {
            param.bounds.push(crate::Generics::DECODE.to_string());
        }
    }
    params
}

fn struct_codec_items(s: &crate::AST::StructDef) -> Vec<Item> {
    let encode = has_derive(&s.derives, crate::Generics::ENCODE);
    let decode = has_derive(&s.derives, crate::Generics::DECODE);
    let wire_types = s
        .reflection_fields()
        .filter(|field| !has_marker(&field.serde_markers, crate::Syntax::MARKER_SKIP))
        .map(|field| field.ty.clone());
    let params = codec_params(&s.type_params, wire_types, encode, decode);
    let span = s
        .derives
        .iter()
        .find(|(name, _)| matches!(name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE))
        .map(|(_, span)| *span)
        .unwrap_or(s.name_span);
    let mut out = Vec::new();
    if encode {
        out.push(Item::Impl(serde_impl(
            &s.name,
            crate::Generics::ENCODE,
            serde_method(
                "encode",
                params.clone(),
                vec![self_param(span)],
                Some(data_tree_type()),
                struct_encode_body(s, span),
                span,
            ),
            span,
        )));
    }
    if decode {
        out.push(Item::Impl(serde_impl(
            &s.name,
            crate::Generics::DECODE,
            serde_method(
                "decode",
                params,
                vec![named_param("tree", data_tree_type(), span)],
                Some(result_type(target_type(&s.name, &s.type_params), span)),
                struct_decode_body(s, span),
                span,
            ),
            span,
        )));
    }
    out
}

fn enum_codec_items(e: &crate::AST::EnumDef) -> Vec<Item> {
    let encode = has_derive(&e.derives, crate::Generics::ENCODE);
    let decode = has_derive(&e.derives, crate::Generics::DECODE);
    let wire_types = e.variants.iter().flat_map(|variant| match &variant.payload {
        VariantPayload::Unit => Vec::new(),
        VariantPayload::Single(ty, _) => vec![ty.clone()],
        VariantPayload::Named(fields) => fields.iter().map(|field| field.ty.clone()).collect(),
    });
    let params = codec_params(&e.type_params, wire_types, encode, decode);
    let span = e
        .derives
        .iter()
        .find(|(name, _)| matches!(name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE))
        .map(|(_, span)| *span)
        .unwrap_or(e.name_span);
    let mut out = Vec::new();
    if encode {
        out.push(Item::Impl(serde_impl(
            &e.name,
            crate::Generics::ENCODE,
            serde_method(
                "encode",
                params.clone(),
                vec![self_param(span)],
                Some(data_tree_type()),
                enum_encode_body(e, span),
                span,
            ),
            span,
        )));
    }
    if decode {
        out.push(Item::Impl(serde_impl(
            &e.name,
            crate::Generics::DECODE,
            serde_method(
                "decode",
                params,
                vec![named_param("tree", data_tree_type(), span)],
                Some(result_type(target_type(&e.name, &e.type_params), span)),
                enum_decode_body(e, span),
                span,
            ),
            span,
        )));
    }
    out
}

fn serde_impl(type_name: &str, trait_name: &str, method: Func, span: Span) -> ImplDef {
    ImplDef {
        span,
        type_name: type_name.to_string(),
        type_span: span,
        trait_name: Some(trait_name.to_string()),
        trait_span: Some(span),
        methods: vec![method],
        delegation_field: None,
        assoc_type_impls: Vec::new(),
        is_generated_serde: true,
        os_target: None,
    }
}

fn serde_method(
    name: &str,
    type_params: Vec<TypeParam>,
    params: Vec<Param>,
    return_type: Option<Type>,
    body: Vec<Stmt>,
    span: Span,
) -> Func {
    let mut function = Func::implicit_run(body, span);
    function.name = name.to_string();
    function.name_span = span;
    function.type_params = type_params;
    function.params = params;
    function.return_type = return_type;
    function.return_type_span = function.return_type.as_ref().map(|_| span);
    function
}

fn struct_encode_body(s: &crate::AST::StructDef, span: Span) -> Vec<Stmt> {
    let fields = s
        .reflection_fields()
        .filter(|field| !has_marker(&field.serde_markers, crate::Syntax::MARKER_SKIP))
        .collect::<Vec<_>>();
    let has_flatten = fields
        .iter()
        .any(|field| has_marker(&field.serde_markers, crate::Syntax::MARKER_FLATTEN));
    if !has_flatten {
        return ordered_encode_fields(&s.serde_markers, &fields, 0, Vec::new(), span);
    }

    let mut body = vec![binding(
        "out",
        Some(map_type(data_tree_type(), span)),
        map_literal(Vec::new(), span),
        true,
        span,
    )];
    for field in fields {
        let key = serde_field_key(&s.serde_markers, field);
        if has_marker(&field.serde_markers, crate::Syntax::MARKER_FLATTEN) {
            let nested = format!("jet_serde_nested_{}", field.name);
            body.push(binding(
                &nested,
                None,
                method(field_read("self", &field.name, span), "encode", Vec::new(), span),
                false,
                span,
            ));
            body.push(pattern_switch(
                copy(ident(&nested, span), span),
                "Object",
                vec!["entries".to_string()],
                vec![for_each_map(
                    "key",
                    "value",
                    ident("entries", span),
                    vec![assign_index(
                        "out",
                        ident("key", span),
                        ident("value", span),
                        span,
                    )],
                    span,
                )],
                Some(Vec::new()),
                span,
            ));
        } else if matches!(field.ty, Type::Option(_)) {
            body.push(option_encode(
                copy(field_read("self", &field.name, span), span),
                &key,
                vec![field.name.clone()],
                span,
            ));
        } else {
            body.push(assign_index(
                "out",
                string_expr(&key, span),
                method(field_read("self", &field.name, span), "encode", Vec::new(), span),
                span,
            ));
        }
    }
    body.push(ret(data_tree_object_expr(ident("out", span), span), span));
    body
}

fn ordered_encode_fields(
    container_markers: &[crate::AST::Marker],
    fields: &[&crate::AST::Field],
    index: usize,
    pairs: Vec<(Expr, Expr)>,
    span: Span,
) -> Vec<Stmt> {
    let Some(field) = fields.get(index) else {
        return vec![ret(data_tree_object(pairs, span), span)];
    };
    let key = serde_field_key(container_markers, field);
    let next_pairs = |value: Expr| {
        let mut next = pairs.clone();
        next.push((string_expr(&key, span), value));
        ordered_encode_fields(container_markers, fields, index + 1, next, span)
    };
    if matches!(field.ty, Type::Option(_)) {
        let present = next_pairs(method(ident("jet_serde_option_value", span), "encode", Vec::new(), span));
        vec![option_switch(
            copy(field_read("self", &field.name, span), span),
            "jet_serde_option_value",
            present,
            ordered_encode_fields(container_markers, fields, index + 1, pairs, span),
            span,
        )]
    } else {
        next_pairs(method(field_read("self", &field.name, span), "encode", Vec::new(), span))
    }
}

fn struct_decode_body(s: &crate::AST::StructDef, span: Span) -> Vec<Stmt> {
    let mut body = vec![binding(
        "jet_serde_errors",
        Some(field_error_list_type(span)),
        list_literal(Vec::new(), span),
        true,
        span,
    )];
    body.push(binding("jet_serde_is_object", Some(Type::Bool), Expr::Bool(false, span), true, span));
    body.push(pattern_switch(
        copy(ident("tree", span), span),
        "Object",
        vec!["jet_serde_root_entries".to_string()],
        vec![assign_local("jet_serde_is_object", Expr::Bool(true, span), span)],
        Some(Vec::new()),
        span,
    ));
    body.push(if_stmt(
        unary_not(ident("jet_serde_is_object", span), span),
        vec![ret(
            err(list_literal(vec![field_error("", "expected an object", span)], span), span),
            span,
        )],
        span,
    ));

    let deny_unknown = has_marker(&s.serde_markers, crate::Syntax::MARKER_DENY_UNKNOWN_FIELDS);
    let has_flatten = s.reflection_fields().any(|field| {
        has_marker(&field.serde_markers, crate::Syntax::MARKER_FLATTEN)
    });
    if deny_unknown && !has_flatten {
        let keys = s
            .reflection_fields()
            .filter(|field| !has_marker(&field.serde_markers, crate::Syntax::MARKER_SKIP))
            .map(|field| string_expr(&serde_field_key(&s.serde_markers, field), span))
            .collect::<Vec<_>>();
        let allowed = list_literal(keys, span);
        body.push(pattern_switch(
            copy(ident("tree", span), span),
            "Object",
            vec!["entries".to_string()],
            vec![for_each_map(
                "key",
                "value",
                ident("entries", span),
                vec![if_stmt(
                    unary_not(method(allowed.clone(), "contains", vec![ident("key", span)], span), span),
                    vec![expr_stmt(method(
                        ident("jet_serde_errors", span),
                        "push",
                        vec![field_error_expr_value(
                            copy(ident("key", span), span),
                            interpolated_string(
                                "E2412: unknown field `",
                                ident("key", span),
                                "`",
                                span,
                            ),
                            span,
                        )],
                        span,
                    ))],
                    span,
                )],
                span,
            )],
            Some(Vec::new()),
            span,
        ));
    }

    let mut field_values = Vec::new();
    let mut decoded = Vec::new();
    let mut required = Vec::new();
    for field in s.reflection_fields() {
        let value = if has_marker(&field.serde_markers, crate::Syntax::MARKER_SKIP) {
            serde_default_expr(field).unwrap_or_else(|| serde_zero_expr(&field.ty, span))
        } else if has_marker(&field.serde_markers, crate::Syntax::MARKER_FLATTEN) {
            let result = format!("jet_serde_decode_{}", field.name);
            let value = format!("jet_serde_decoded_value_{}", decoded.len());
            body.push(binding(
                &result,
                None,
                method_with_type_args(ident("tree", span), "decode", vec![field.ty.clone()], span),
                false,
                span,
            ));
            decoded.push((result, value.clone(), None, None));
            ident(&value, span)
        } else {
            let key = serde_field_key(&s.serde_markers, field);
            let is_required = !matches!(field.ty, Type::Option(_)) && serde_default_expr(field).is_none();
            let missing = if is_required {
                let name = format!("jet_serde_missing_required_{}", decoded.len());
                body.push(binding(&name, Some(Type::Bool), Expr::Bool(false, span), true, span));
                required.push((name.clone(), key.clone()));
                Some(name)
            } else {
                None
            };
            let subtree = if matches!(field.ty, Type::Option(_)) {
                or_fallback(method(ident("tree", span), "field", vec![string_expr(&key, span)], span), data_tree_null(span), span)
            } else if let Some(default) = serde_default_expr(field) {
                or_fallback(
                    method(ident("tree", span), "field", vec![string_expr(&key, span)], span),
                    method(default, "encode", Vec::new(), span),
                    span,
                )
            } else {
                or_fallback(method(ident("tree", span), "field", vec![string_expr(&key, span)], span), data_tree_null(span), span)
            };
            let result = format!("jet_serde_decode_{}", field.name);
            let value = format!("jet_serde_decoded_value_{}", decoded.len());
            body.push(binding(
                &result,
                None,
                field_error_under(
                    &key,
                    method_with_type_args(subtree, "decode", vec![field.ty.clone()], span),
                    span,
                ),
                false,
                span,
            ));
            decoded.push((result, value.clone(), missing, Some(key)));
            ident(&value, span)
        };
        field_values.push((field.name.clone(), value));
    }

    if !required.is_empty() {
        let assignments = required
            .iter()
            .map(|(name, _)| assign_local(name, Expr::Bool(true, span), span))
            .collect::<Vec<_>>();
        let mut presence_body = assignments;
        presence_body.push(for_each_map(
            "jet_serde_presence_key",
            "jet_serde_presence_value",
            ident("jet_serde_presence_entries", span),
            required
                .iter()
                .map(|(missing, key)| {
                    if_stmt(
                        binary(BinOp::Eq, ident("jet_serde_presence_key", span), string_expr(key, span), span),
                        vec![assign_local(missing, Expr::Bool(false, span), span)],
                        span,
                    )
                })
                .collect(),
            span,
        ));
        body.push(pattern_switch(
            copy(ident("tree", span), span),
            "Object",
            vec!["jet_serde_presence_entries".to_string()],
            presence_body,
            Some(Vec::new()),
            span,
        ));
    }

    for (index, (result, _, missing, key)) in decoded.iter().enumerate() {
        let error_name = format!("jet_serde_field_errors_{index}");
        let mut on_error = Vec::new();
        if let (Some(missing), Some(key)) = (missing, key) {
            on_error.push(if_stmt(
                ident(missing, span),
                vec![expr_stmt(method(
                    ident("jet_serde_errors", span),
                    "push",
                    vec![field_error(key, &format!("E2410: missing required field `{key}`"), span)],
                    span,
                ))],
                span,
            ));
            on_error.push(if_stmt(
                unary_not(ident(missing, span), span),
                vec![for_each(
                    "jet_serde_field_error",
                    ident(&error_name, span),
                    vec![expr_stmt(method(
                        ident("jet_serde_errors", span),
                        "push",
                        vec![ident("jet_serde_field_error", span)],
                        span,
                    ))],
                    span,
                )],
                span,
            ));
        } else {
            on_error.push(for_each(
                "jet_serde_field_error",
                ident(&error_name, span),
                vec![expr_stmt(method(
                    ident("jet_serde_errors", span),
                    "push",
                    vec![ident("jet_serde_field_error", span)],
                    span,
                ))],
                span,
            ));
        }
        body.push(pattern_switch(
            copy(ident(result, span), span),
            "Err",
            vec![error_name],
            on_error,
            Some(Vec::new()),
            span,
        ));
    }

    let mut success = Vec::new();
    let decoded_lit = struct_literal(
        target_type_name(&s.name),
        type_args_from_params(&s.type_params, span),
        field_values,
        span,
    );
    let decoded_name = "decoded";
    success.push(binding(decoded_name, None, decoded_lit, false, span));
    if s.validate_block.is_empty() {
        success.push(ret(ok(ident(decoded_name, span), span), span));
    } else {
        success.push(ret(
            method_with_owner_args(
                ident(&s.name, span),
                "validate",
                type_args_from_params(&s.type_params, span),
                vec![ident(decoded_name, span)],
                span,
            ),
            span,
        ));
    }
    for (result, value, _, _) in decoded.iter().rev() {
        success = vec![pattern_switch(
            copy(ident(result, span), span),
            "Ok",
            vec![value.clone()],
            success,
            Some(Vec::new()),
            span,
        )];
    }
    body.push(if_stmt(method(ident("jet_serde_errors", span), "is_empty", Vec::new(), span), success, span));
    body.push(ret(err(ident("jet_serde_errors", span), span), span));
    body
}

fn enum_encode_body(e: &crate::AST::EnumDef, span: Span) -> Vec<Stmt> {
    let tag = e
        .serde_markers
        .iter()
        .find(|marker| marker.name == crate::Syntax::MARKER_TAG)
        .and_then(marker_static_string);
    let untagged = has_marker(&e.serde_markers, crate::Syntax::MARKER_UNTAGGED);
    let arms = e
        .variants
        .iter()
        .map(|variant| {
            let bindings = payload_bindings(&variant.payload, "v");
            let value = enum_wire_value(variant, tag.as_deref(), untagged, span);
            SwitchArm {
                cond: pattern_test("self", variant, bindings, span),
                body: vec![ret(value, span)],
                span,
            }
        })
        .collect();
    vec![Stmt::Switch {
        subject: ident("self", span),
        arms,
        else_body: None,
        span,
    }]
}

fn enum_wire_value(
    variant: &crate::AST::Variant,
    tag: Option<&str>,
    untagged: bool,
    span: Span,
) -> Expr {
    let wire = serde_enum_variant_key(variant);
    if !untagged && tag.is_some() {
        if let VariantPayload::Named(fields) = &variant.payload {
            let mut entries = vec![(string_expr(tag.expect("tag"), span), data_tree_text(&wire, span))];
            entries.extend(fields.iter().enumerate().map(|(index, field)| {
                (
                    string_expr(&field.name, span),
                    method(ident(&format!("v{index}"), span), "encode", Vec::new(), span),
                )
            }));
            return data_tree_object(entries, span);
        }
    }
    let payload = match &variant.payload {
        VariantPayload::Unit => data_tree_null(span),
        VariantPayload::Single(..) => method(ident("v0", span), "encode", Vec::new(), span),
        VariantPayload::Named(fields) => data_tree_object(
            fields
                .iter()
                .enumerate()
                .map(|(index, field)| {
                    (
                        string_expr(&field.name, span),
                        method(ident(&format!("v{index}"), span), "encode", Vec::new(), span),
                    )
                })
                .collect(),
            span,
        ),
    };
    if untagged {
        return payload;
    }
    if let Some(tag) = tag {
        let mut entries = vec![(string_expr(tag, span), data_tree_text(&wire, span))];
        if !matches!(variant.payload, VariantPayload::Unit) {
            if matches!(variant.payload, VariantPayload::Single(..)) {
                entries.push((string_expr("value", span), payload));
            } else if let Expr::MethodCall { args, .. } = payload {
                if let Some(crate::AST::CallArg { expr: Expr::MapLit(pairs, _), .. }) = args.into_iter().next() {
                    entries.extend(pairs);
                }
            }
        }
        return data_tree_object(entries, span);
    }
    if matches!(variant.payload, VariantPayload::Unit) {
        data_tree_text(&wire, span)
    } else {
        data_tree_object(vec![(string_expr(&wire, span), payload)], span)
    }
}

fn enum_decode_body(e: &crate::AST::EnumDef, span: Span) -> Vec<Stmt> {
    let tag = e
        .serde_markers
        .iter()
        .find(|marker| marker.name == crate::Syntax::MARKER_TAG)
        .and_then(marker_static_string);
    let untagged = has_marker(&e.serde_markers, crate::Syntax::MARKER_UNTAGGED);
    if untagged {
        let mut body = Vec::new();
        for variant in &e.variants {
            let payload_ty = enum_payload_type(variant);
            let decoded = method_with_type_args(ident("tree", span), "decode", vec![payload_ty], span);
            let binding_name = format!("jet_serde_enum_value_{}", variant.name.replace('.', "_"));
            let value = enum_constructor_from_binding(variant, &binding_name, e.name.as_str(), span);
            body.push(pattern_switch(
                decoded,
                "Ok",
                vec![binding_name],
                vec![ret(ok(value, span), span)],
                Some(Vec::new()),
                span,
            ));
        }
        body.push(no_matching_enum(span));
        return body;
    }

    if let Some(tag) = tag {
        let tag_decode = field_error_under(
            &tag,
            method_with_type_args(
                or_fallback(
                    method(ident("tree", span), "field", vec![string_expr(&tag, span)], span),
                    data_tree_null(span),
                    span,
                ),
                "decode",
                vec![Type::String],
                span,
            ),
            span,
        );
        let tag_try = try_expr(tag_decode, span);
        let mut body = vec![binding("tag_value", None, tag_try, false, span)];
        for variant in &e.variants {
            let wire = serde_enum_variant_key(variant);
            let payload_source = if matches!(variant.payload, VariantPayload::Single(..)) {
                try_expr(
                    method(ident("tree", span), "field", vec![string_expr("value", span)], span),
                    span,
                )
            } else {
                ident("tree", span)
            };
            let value = enum_decode_value(variant, &payload_source, e.name.as_str(), span);
            body.push(if_stmt(
                binary(BinOp::Eq, ident("tag_value", span), string_expr(&wire, span), span),
                enum_decode_variant_body(variant, value, &payload_source, e.name.as_str(), span),
                span,
            ));
        }
        body.push(no_matching_enum(span));
        return body;
    }

    let mut body = Vec::new();
    for variant in &e.variants {
        let wire = serde_enum_variant_key(variant);
        if matches!(variant.payload, VariantPayload::Unit) {
            body.push(pattern_switch(
                copy(ident("tree", span), span),
                "Text",
                vec!["variant_name".to_string()],
                vec![if_stmt(
                    binary(BinOp::Eq, ident("variant_name", span), string_expr(&wire, span), span),
                    vec![ret(ok(enum_constructor_unit(e.name.as_str(), variant, span), span), span)],
                    span,
                )],
                Some(Vec::new()),
                span,
            ));
        } else {
            let candidate = format!("candidate_{}", variant.name.replace('.', "_"));
            let candidate_expr = or_fallback(
                method(copy(ident("tree", span), span), "field", vec![string_expr(&wire, span)], span),
                data_tree_null(span),
                span,
            );
            body.push(binding(&candidate, None, candidate_expr, false, span));
            let source = ident(&candidate, span);
            let value = enum_decode_value(variant, &source, e.name.as_str(), span);
            if matches!(variant.payload, VariantPayload::Single(..)) {
                let decoded = method_with_type_args(source, "decode", vec![enum_payload_type(variant)], span);
                let binding_name = format!("jet_serde_enum_decoded_{}", variant.name.replace('.', "_"));
                body.push(pattern_switch(
                    decoded,
                    "Ok",
                    vec![binding_name.clone()],
                    vec![ret(ok(enum_constructor_from_binding(variant, &binding_name, e.name.as_str(), span), span), span)],
                    Some(Vec::new()),
                    span,
                ));
            } else {
                body.extend(value);
            }
        }
    }
    body.push(no_matching_enum(span));
    body
}

fn enum_decode_variant_body(
    variant: &crate::AST::Variant,
    value: Vec<Stmt>,
    source: &Expr,
    type_name: &str,
    span: Span,
) -> Vec<Stmt> {
    match &variant.payload {
        VariantPayload::Unit => vec![ret(ok(enum_constructor_unit(type_name, variant, span), span), span)],
        VariantPayload::Single(ty, _) => {
            let decoded = method_with_type_args(source.clone(), "decode", vec![ty.clone()], span);
            let binding_name = format!("jet_serde_enum_decoded_{}", variant.name.replace('.', "_"));
            vec![pattern_switch(
                decoded,
                "Ok",
                vec![binding_name.clone()],
                vec![ret(ok(enum_constructor_from_binding(variant, &binding_name, type_name, span), span), span)],
                Some(Vec::new()),
                span,
            )]
        }
        VariantPayload::Named(_) => value,
    }
}

fn enum_decode_value(
    variant: &crate::AST::Variant,
    source: &Expr,
    type_name: &str,
    span: Span,
) -> Vec<Stmt> {
    let VariantPayload::Named(fields) = &variant.payload else {
        return Vec::new();
    };
    let errors_name = format!("jet_serde_enum_{}_errors", variant.name.replace('.', "_"));
    let mut body = vec![binding(
        &errors_name,
        Some(field_error_list_type(span)),
        list_literal(Vec::new(), span),
        true,
        span,
    )];
    let mut results = Vec::new();
    for field in fields {
        let result = format!(
            "jet_serde_enum_{}_decode_{}",
            variant.name.replace('.', "_"),
            field.name
        );
        let subtree = or_fallback(
            method(copy(source.clone(), span), "field", vec![string_expr(&field.name, span)], span),
            data_tree_null(span),
            span,
        );
        body.push(binding(
            &result,
            None,
            field_error_under(
                &field.name,
                method_with_type_args(subtree, "decode", vec![field.ty.clone()], span),
                span,
            ),
            false,
            span,
        ));
        results.push(result);
    }
    for (index, result) in results.iter().enumerate() {
        let error_name = format!("jet_serde_enum_{}_errors_{index}", variant.name.replace('.', "_"));
        body.push(pattern_switch(
            copy(ident(result, span), span),
            "Err",
            vec![error_name.clone()],
            vec![for_each(
                "jet_serde_enum_error",
                ident(&error_name, span),
                vec![expr_stmt(method(
                    ident(&errors_name, span),
                    "push",
                    vec![ident("jet_serde_enum_error", span)],
                    span,
                ))],
                span,
            )],
            Some(Vec::new()),
            span,
        ));
    }
    let mut success = Vec::new();
    let value_fields = fields
        .iter()
        .enumerate()
        .map(|(_index, field)| (field.name.clone(), ident(&format!("jet_serde_enum_{}_value_{}", variant.name.replace('.', "_"), field.name), span)))
        .collect::<Vec<_>>();
    success.push(ret(
        ok(enum_constructor_named(type_name, variant, value_fields, span), span),
        span,
    ));
    for result in results.iter().rev() {
        let value_name = format!(
            "jet_serde_enum_{}_value_{}",
            variant.name.replace('.', "_"),
            fields[results.iter().position(|item| item == result).unwrap()].name
        );
        success = vec![pattern_switch(
            copy(ident(result, span), span),
            "Ok",
            vec![value_name],
            success,
            Some(Vec::new()),
            span,
        )];
    }
    body.push(if_stmt(method(ident(&errors_name, span), "is_empty", Vec::new(), span), success, span));
    body
}

fn enum_constructor_unit(type_name: &str, variant: &crate::AST::Variant, span: Span) -> Expr {
    Expr::EnumLit {
        type_name: type_name.to_string(),
        variant: variant.name.clone(),
        args: Vec::new(),
        leading_dot: false,
        span,
    }
}

fn enum_constructor_from_binding(
    variant: &crate::AST::Variant,
    binding: &str,
    type_name: &str,
    span: Span,
) -> Expr {
    match variant.payload {
        VariantPayload::Unit => enum_constructor_unit(type_name, variant, span),
        VariantPayload::Single(..) => Expr::EnumLit {
            type_name: type_name.to_string(),
            variant: variant.name.clone(),
            args: vec![EnumLitArg::Positional(ident(binding, span))],
            leading_dot: false,
            span,
        },
        VariantPayload::Named(_) => enum_constructor_unit(type_name, variant, span),
    }
}

fn enum_constructor_named(
    type_name: &str,
    variant: &crate::AST::Variant,
    fields: Vec<(String, Expr)>,
    span: Span,
) -> Expr {
    Expr::EnumLit {
        type_name: type_name.to_string(),
        variant: variant.name.clone(),
        args: fields
            .into_iter()
            .map(|(label, expr)| EnumLitArg::Named { label, expr })
            .collect(),
        leading_dot: false,
        span,
    }
}

fn no_matching_enum(span: Span) -> Stmt {
    ret(
        err(list_literal(vec![field_error("", "no matching enum variant", span)], span), span),
        span,
    )
}

fn enum_payload_type(variant: &crate::AST::Variant) -> Type {
    match &variant.payload {
        VariantPayload::Unit | VariantPayload::Named(_) => data_tree_type(),
        VariantPayload::Single(ty, _) => ty.clone(),
    }
}

fn payload_bindings(payload: &VariantPayload, prefix: &str) -> Vec<String> {
    let count = match payload {
        VariantPayload::Unit => 0,
        VariantPayload::Single(..) => 1,
        VariantPayload::Named(fields) => fields.len(),
    };
    (0..count).map(|index| format!("{prefix}{index}")).collect()
}

fn pattern_test(subject: &str, variant: &crate::AST::Variant, bindings: Vec<String>, span: Span) -> Expr {
    Expr::PatternTest {
        subject: Box::new(ident(subject, span)),
        pattern: Pattern::Variant {
            variant: variant.name.clone(),
            bindings: bindings.into_iter().map(|name| PatSlot::Bind { name, span }).collect(),
            leading_dot: true,
            span,
        },
        span,
    }
}

fn pattern_switch(
    subject: Expr,
    variant: &str,
    bindings: Vec<String>,
    body: Vec<Stmt>,
    else_body: Option<Vec<Stmt>>,
    span: Span,
) -> Stmt {
    Stmt::Switch {
        subject,
        arms: vec![SwitchArm {
            cond: Expr::PatternTest {
                subject: Box::new(ident("it", span)),
                pattern: Pattern::Variant {
                    variant: variant.to_string(),
                    bindings: bindings.into_iter().map(|name| PatSlot::Bind { name, span }).collect(),
                    leading_dot: true,
                    span,
                },
                span,
            },
            body,
            span,
        }],
        else_body,
        span,
    }
}

fn option_switch(subject: Expr, binding_name: &str, body: Vec<Stmt>, else_body: Vec<Stmt>, span: Span) -> Stmt {
    pattern_switch(subject, "Val", vec![binding_name.to_string()], body, Some(else_body), span)
}

fn option_encode(subject: Expr, key: &str, field_names: Vec<String>, span: Span) -> Stmt {
    let _ = field_names.first().expect("one field for option encode");
    option_switch(
        subject,
        "jet_serde_option_value",
        vec![assign_index(
            "out",
            string_expr(key, span),
            method(ident("jet_serde_option_value", span), "encode", Vec::new(), span),
            span,
        )],
        Vec::new(),
        span,
    )
}

fn for_each_map(var: &str, var2: &str, collection: Expr, body: Vec<Stmt>, span: Span) -> Stmt {
    Stmt::For {
        var: var.to_string(),
        var_span: span,
        var2: Some((var2.to_string(), span)),
        kind: ForKind::In { collection, step: None },
        body,
        span,
        arrow_body: false,
        label: None,
    }
}

fn for_each(var: &str, collection: Expr, body: Vec<Stmt>, span: Span) -> Stmt {
    Stmt::For {
        var: var.to_string(),
        var_span: span,
        var2: None,
        kind: ForKind::In { collection, step: None },
        body,
        span,
        arrow_body: false,
        label: None,
    }
}

fn binding(name: &str, ty: Option<Type>, init: Expr, mutable: bool, span: Span) -> Stmt {
    Stmt::Val(crate::AST::Binding {
        mutable,
        markers: Vec::new(),
        reactive_upgrade: false,
        meta: None,
        name: name.to_string(),
        name_span: span,
        sigil_span: None,
        pattern: None,
        ty,
        ty_span: Some(span),
        init,
        is_comptime: false,
        ct: None,
        uninit: false,
        arena_view: false,
        string_view: false,
        gc_promotion: None,
        gc_transferred: false,
    })
}

fn assign_local(name: &str, value: Expr, span: Span) -> Stmt {
    Stmt::Assign {
        target: LValue::Local { name: name.to_string(), name_span: span },
        op: None,
        op_span: span,
        value,
    }
}

fn assign_index(base: &str, index: Expr, value: Expr, span: Span) -> Stmt {
    Stmt::Assign {
        target: LValue::Index {
            base: Box::new(ident(base, span)),
            index: Box::new(index),
            span,
            kind: IndexKind::Unknown,
        },
        op: None,
        op_span: span,
        value,
    }
}

fn expr_stmt(expr: Expr) -> Stmt { Stmt::Expr(expr) }
fn ret(expr: Expr, span: Span) -> Stmt { Stmt::Return(Some(expr), span) }
fn ok(expr: Expr, span: Span) -> Expr { Expr::Ok(Box::new(expr), span) }
fn err(expr: Expr, span: Span) -> Expr { Expr::Err(Box::new(expr), span) }
fn copy(expr: Expr, span: Span) -> Expr { Expr::Copy(Box::new(expr), span) }
fn unary_not(expr: Expr, span: Span) -> Expr { Expr::Unary(crate::AST::UnOp::Not, Box::new(expr), span) }

fn ident(name: &str, span: Span) -> Expr { Expr::Ident(name.to_string(), span) }

fn binary(op: BinOp, left: Expr, right: Expr, span: Span) -> Expr {
    Expr::Binary(op, Box::new(left), Box::new(right), span)
}

fn if_stmt(cond: Expr, body: Vec<Stmt>, span: Span) -> Stmt {
    Stmt::Switch {
        subject: Expr::Bool(true, span),
        arms: vec![SwitchArm { cond, body, span }],
        else_body: Some(Vec::new()),
        span,
    }
}

fn string_expr(value: &str, span: Span) -> Expr {
    Expr::Str(vec![crate::AST::StrPart::Lit(value.to_string())], span)
}

fn data_tree_type() -> Type { Type::Named("DataTree".to_string()) }
fn field_error_type() -> Type { Type::Named("FieldError".to_string()) }
fn field_error_list_type(_span: Span) -> Type { Type::List(Box::new(field_error_type())) }
fn result_type(ok: Type, _span: Span) -> Type {
    Type::Result { ok: Box::new(ok), err: Box::new(field_error_list_type(_span)) }
}

fn map_type(value: Type, _span: Span) -> Type {
    Type::Map { key: Box::new(Type::String), key_span: None, value: Box::new(value) }
}

fn target_type(name: &str, params: &[TypeParam]) -> Type {
    if params.is_empty() {
        Type::Named(name.to_string())
    } else {
        Type::Apply { name: name.to_string(), args: params.iter().map(|param| Type::Named(param.name.clone())).collect() }
    }
}

fn target_type_name(name: &str) -> String { name.to_string() }

fn type_args_from_params(params: &[TypeParam], _span: Span) -> Vec<Type> {
    params.iter().map(|param| Type::Named(param.name.clone())).collect()
}

fn list_literal(values: Vec<Expr>, span: Span) -> Expr { Expr::ListLit(values, span) }
fn map_literal(values: Vec<(Expr, Expr)>, span: Span) -> Expr { Expr::MapLit(values, span) }

fn data_tree_object(entries: Vec<(Expr, Expr)>, span: Span) -> Expr {
    data_tree_variant("Object", vec![map_literal(entries, span)], span)
}

fn data_tree_object_expr(map: Expr, span: Span) -> Expr {
    data_tree_variant("Object", vec![map], span)
}

fn data_tree_null(span: Span) -> Expr { data_tree_variant("Null", Vec::new(), span) }
fn data_tree_text(value: &str, span: Span) -> Expr { data_tree_variant("Text", vec![string_expr(value, span)], span) }

fn data_tree_variant(variant: &str, args: Vec<Expr>, span: Span) -> Expr {
    if args.is_empty() {
        Expr::Field(Box::new(ident("DataTree", span)), variant.to_string(), span)
    } else {
        Expr::MethodCall {
            receiver: Box::new(ident("DataTree", span)),
            method: variant.to_string(),
            method_span: span,
            owner_type_args: Vec::new(),
            type_args: Vec::new(),
            args: args.into_iter().map(|expr| call_arg(expr, span)).collect(),
            recv_type: None,
            resolved_ret: None,
            checked_widen: false,
        }
    }
}

fn struct_literal(name: String, type_args: Vec<Type>, fields: Vec<(String, Expr)>, span: Span) -> Expr {
    Expr::StructLit {
        type_name: name,
        type_args,
        import_ns: None,
        as_trait: None,
        fields: fields.into_iter().map(|(name, expr)| (name, span, expr)).collect(),
        inferred: false,
        span,
    }
}

fn field_error(path: &str, reason: &str, span: Span) -> Expr {
    field_error_expr_value(string_expr(path, span), string_expr(reason, span), span)
}

fn field_error_expr_value(path: Expr, reason: Expr, span: Span) -> Expr {
    struct_literal(
        "FieldError".to_string(),
        Vec::new(),
        vec![("path".to_string(), path), ("reason".to_string(), reason)],
        span,
    )
}

fn interpolated_string(prefix: &str, value: Expr, suffix: &str, span: Span) -> Expr {
    Expr::Str(
        vec![
            crate::AST::StrPart::Lit(prefix.to_string()),
            crate::AST::StrPart::Interp(Box::new(value), crate::AST::StrFormat::default()),
            crate::AST::StrPart::Lit(suffix.to_string()),
        ],
        span,
    )
}

fn method(receiver: Expr, name: &str, args: Vec<Expr>, span: Span) -> Expr {
    method_with_owner_args(receiver, name, Vec::new(), args, span)
}

fn method_with_owner_args(receiver: Expr, name: &str, owner_type_args: Vec<Type>, args: Vec<Expr>, span: Span) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: name.to_string(),
        method_span: span,
        owner_type_args,
        type_args: Vec::new(),
        args: args.into_iter().map(|expr| call_arg(expr, span)).collect(),
        recv_type: None,
        resolved_ret: None,
        checked_widen: false,
    }
}

fn method_with_type_args(receiver: Expr, name: &str, type_args: Vec<Type>, span: Span) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: name.to_string(),
        method_span: span,
        owner_type_args: Vec::new(),
        type_args,
        args: Vec::new(),
        recv_type: None,
        resolved_ret: None,
        checked_widen: false,
    }
}

fn field_read(base: &str, field: &str, span: Span) -> Expr {
    Expr::Field(Box::new(ident(base, span)), field.to_string(), span)
}

fn call_arg(expr: Expr, span: Span) -> CallArg {
    CallArg { convention: AccessConvention::Read, expr, span, flags: CallArgFlags::default(), label: None, spread: false }
}

fn self_param(span: Span) -> Param { named_param("self", Type::Named(String::new()), span) }

fn named_param(name: &str, ty: Type, span: Span) -> Param {
    Param {
        convention: AccessConvention::Read,
        root: false,
        name: name.to_string(),
        name_span: span,
        public_label: None,
        zone: crate::AST::ParamZone::Either,
        ty,
        ty_span: span,
        default: None,
        variadic: false,
        variadic_bound_list: None,
        declared_view_from_names: None,
    }
}

fn try_expr(expr: Expr, span: Span) -> Expr { Expr::Try(Box::new(expr), span, TryConvert::None, None) }

fn or_fallback(value: Expr, fallback: Expr, span: Span) -> Expr {
    Expr::OrFallback { value: Box::new(value), fallback: crate::AST::OrFallback::Value(Box::new(fallback)), is_option: false, span }
}

fn field_error_under(path: &str, value: Expr, span: Span) -> Expr {
    method(
        ident("FieldError", span),
        "under",
        vec![string_expr(path, span), value],
        span,
    )
}

fn serde_zero_expr(ty: &Type, span: Span) -> Expr {
    match ty {
        Type::Int | Type::IntN { .. } => Expr::Int(0, span, None, None),
        Type::Float | Type::Float32 => Expr::Float(0.0, span, matches!(ty, Type::Float32)),
        Type::Bool => Expr::Bool(false, span),
        Type::String => string_expr("", span),
        Type::Option(_) => Expr::Absent(span),
        Type::List(_) | Type::Map { .. } => list_literal(Vec::new(), span),
        Type::Apply { name, args } => struct_literal(name.clone(), args.clone(), Vec::new(), span),
        Type::Named(name) => struct_literal(name.clone(), Vec::new(), Vec::new(), span),
        _ => struct_literal(ty.name(), Vec::new(), Vec::new(), span),
    }
}

fn serde_default_expr(field: &crate::AST::Field) -> Option<Expr> {
    if let Some(expr) = &field.default {
        return Some(expr.as_ref().clone());
    }
    let marker = field
        .serde_markers
        .iter()
        .find(|marker| marker.name == crate::Syntax::MARKER_DEFAULT)?;
    if let Some(expr) = marker.args.first() {
        if let Some(value) = marker.ct.as_ref() {
            return serde_ct_expr(value, expr.span());
        }
        return Some(expr.clone());
    }
    marker.ct.as_ref().and_then(|value| serde_ct_expr(value, marker.span))
}

fn serde_ct_expr(value: &CtValue, span: Span) -> Option<Expr> {
    Some(match value {
        CtValue::Int(value) => Expr::Int(*value, span, None, None),
        CtValue::Float(value) => Expr::Float(value.as_f64(), span, false),
        CtValue::Bool(value) => Expr::Bool(*value, span),
        CtValue::Char(value) => Expr::Char(*value, span),
        CtValue::Str(value) => string_expr(value, span),
        CtValue::BigInt(value) => Expr::Call(Call {
            name: "BigInt".to_string(), name_span: span, type_args: Vec::new(),
            args: vec![call_arg(string_expr(&value.to_string_rep(), span), span)],
            resolved_ret: None, range_checked: false, widen_approx: false,
        }),
        CtValue::Bytes(values) => list_literal(values.iter().map(|value| Expr::Int(i64::from(*value), span, None, None)).collect(), span),
        CtValue::List(values) => list_literal(values.iter().map(|value| serde_ct_expr(value, span)).collect::<Option<Vec<_>>>()?, span),
        CtValue::Map(values) => map_literal(values.iter().map(|(key, value)| Some((serde_ct_expr(&key.to_value(), span)?, serde_ct_expr(value, span)?))).collect::<Option<Vec<_>>>()?, span),
        CtValue::Struct { type_name, fields } => struct_literal(type_name.clone(), Vec::new(), fields.iter().map(|(name, value)| Some((name.clone(), serde_ct_expr(value, span)?))).collect::<Option<Vec<_>>>()?, span),
        CtValue::Enum { type_name, variant, args } => Expr::EnumLit {
            type_name: type_name.clone(), variant: variant.clone(),
            args: args.iter().map(|(label, value)| Some(match label {
                Some(label) => EnumLitArg::Named { label: label.clone(), expr: serde_ct_expr(value, span)? },
                None => EnumLitArg::Positional(serde_ct_expr(value, span)?),
            })).collect::<Option<Vec<_>>>()?, leading_dot: false, span,
        },
        CtValue::Present(value) => Expr::Present(Box::new(serde_ct_expr(value, span)?), span),
        CtValue::Failed(CtReport::Clean(_)) => Expr::Absent(span),
        CtValue::Failed(CtReport::Told(value)) => Expr::Err(Box::new(serde_ct_expr(value, span)?), span),
        CtValue::Unit | CtValue::Closure(_) => return None,
    })
}

fn has_marker(markers: &[crate::AST::Marker], name: &str) -> bool {
    markers.iter().any(|marker| marker.name == name)
}

fn serde_enum_variant_key(v: &crate::AST::Variant) -> String {
    v.serde_markers
        .iter()
        .find(|marker| marker.name == crate::Syntax::MARKER_RENAME)
        .and_then(marker_static_string)
        .unwrap_or_else(|| v.name.clone())
}

fn marker_static_string(marker: &crate::AST::Marker) -> Option<String> {
    if let Some(CtValue::Str(value)) = &marker.ct {
        return Some(value.clone());
    }
    marker.args.first().and_then(|expression| match expression {
        Expr::Str(parts, _) => parts.first().and_then(|part| match part {
            crate::AST::StrPart::Lit(value) => Some(value.clone()),
            crate::AST::StrPart::Interp(..) => None,
        }),
        _ => None,
    })
}

fn serde_field_key(container: &[crate::AST::Marker], field: &crate::AST::Field) -> String {
    if let Some(marker) = field
        .serde_markers
        .iter()
        .find(|marker| marker.name == crate::Syntax::MARKER_RENAME)
    {
        if let Some(value) = marker_static_string(marker) {
            return value;
        }
    }
    let style = container
        .iter()
        .find(|marker| marker.name == crate::Syntax::MARKER_RENAME_ALL)
        .and_then(|marker| marker.args.first())
        .and_then(|expression| match expression {
            Expr::Ident(name, _) => Some(name.as_str()),
            _ => None,
        });
    match style {
        Some("camel") => crate::Syntax::to_camel_acronym(&field.name),
        Some("kebab") => crate::Syntax::to_snake_acronym(&field.name).replace('_', "-"),
        Some("screaming") => crate::Syntax::to_shouty_acronym(&field.name),
        Some("pascal") => crate::Syntax::to_pascal_acronym(&field.name),
        Some("snake") => crate::Syntax::to_snake_acronym(&field.name),
        _ => field.name.clone(),
    }
}
