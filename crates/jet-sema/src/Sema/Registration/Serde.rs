use super::*;
mod Ast;
use Ast::*;
use crate::AST::{
    AccessConvention, BinOp, CallArg, CallArgFlags, CtReport, CtValue, EnumDef, EnumLitArg, Expr,
    ForKind, Func, ImplDef, IndexKind, Item, LValue, Param, PatSlot, Pattern, Stmt, SwitchArm,
    TryConvert, Type, TypeParam, Variant, VariantPayload,
};

/// D-UNIONTYPE1=A / R11: anonymous unions are compiler-owned enum sugar, but
/// their declaration is still an ordinary sema item. The backend keeps the
/// existing representation emitter; codec methods are generated here so they
/// take the same checked Jet AST path as named structs and enums.
pub(crate) fn inject_anonymous_union_items(items: &mut Vec<Item>) {
    let unions = collect_anonymous_unions(items);
    let (encodes, decodes) = union_codec_needs(items);
    let mut existing = items
        .iter()
        .filter_map(|item| match item {
            Item::Enum(e) if e.name.starts_with("__JetUnion_") => Some(e.name.clone()),
            _ => None,
        })
        .collect::<std::collections::HashSet<_>>();
    let span = crate::Diagnostics::Span::new(0, 0);
    for (name, members) in unions {
        if !existing.insert(name.clone()) {
            continue;
        }
        let mut derives = Vec::new();
        if encodes.contains(&name) {
            derives.push((crate::Generics::ENCODE.to_string(), span));
        }
        if decodes.contains(&name) {
            derives.push((crate::Generics::DECODE.to_string(), span));
        }
        let variants = members
            .into_iter()
            .map(|member| Variant {
                name: crate::AST::union_member_tag(&member),
                name_span: span,
                payload: VariantPayload::Single(member, span),
                discriminant: None,
                discriminant_expr: None,
                serde_markers: Vec::new(),
            })
            .collect();
        items.push(Item::Enum(EnumDef {
            span,
            is_pub: false,
            is_package_pub: false,
            name,
            name_span: span,
            type_params: Vec::new(),
            variants,
            methods: Vec::new(),
            trait_impls: Vec::new(),
            derives,
            auto_derive_default: true,
            is_single_use: false,
            single_use_span: None,
            is_must_use: false,
            must_use_span: None,
            serde_markers: Vec::new(),
            type_markers: Vec::new(),
            groups: Vec::new(),
        }));
    }
}

fn collect_anonymous_unions(items: &[Item]) -> std::collections::BTreeMap<String, Vec<Type>> {
    fn walk_type(ty: &Type, unions: &mut std::collections::BTreeMap<String, Vec<Type>>) {
        match ty {
            Type::Union(members) => {
                let name = crate::AST::union_enum_name(members);
                unions.entry(name).or_insert_with(|| members.clone());
                for member in members {
                    walk_type(member, unions);
                }
            }
            Type::List(inner)
            | Type::Shared(inner)
            | Type::Option(inner)
            | Type::Tagged { inner, .. }
            | Type::FixedList { elem: inner, .. }
            | Type::InlineRange { base: inner, .. }
            | Type::Quantity { base: inner, .. } => walk_type(inner, unions),
            Type::Map { key, value, .. }
            | Type::Result {
                ok: key,
                err: value,
            } => {
                walk_type(key, unions);
                walk_type(value, unions);
            }
            Type::Fn { params, ret, .. } => {
                for param in params {
                    walk_type(param, unions);
                }
                if let Some(ret) = ret {
                    walk_type(ret, unions);
                }
            }
            Type::Apply { args, .. } => {
                for arg in args {
                    walk_type(arg, unions);
                }
            }
            Type::Tuple(fields) => {
                for (_, field) in fields {
                    walk_type(field, unions);
                }
            }
            _ => {}
        }
    }

    fn walk_item(item: &Item, unions: &mut std::collections::BTreeMap<String, Vec<Type>>) {
        match item {
            Item::Struct(s) => {
                for field in &s.fields {
                    walk_type(&field.ty, unions);
                }
            }
            Item::Enum(e) => {
                for variant in &e.variants {
                    match &variant.payload {
                        VariantPayload::Single(ty, _) => walk_type(ty, unions),
                        VariantPayload::Named(fields) => {
                            for field in fields {
                                walk_type(&field.ty, unions);
                            }
                        }
                        VariantPayload::Unit => {}
                    }
                }
            }
            Item::Func(f) => {
                for param in &f.params {
                    walk_type(&param.ty, unions);
                }
                if let Some(ret) = &f.return_type {
                    walk_type(ret, unions);
                }
            }
            Item::Impl(i) => {
                for method in &i.methods {
                    for param in &method.params {
                        walk_type(&param.ty, unions);
                    }
                    if let Some(ret) = &method.return_type {
                        walk_type(ret, unions);
                    }
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    for item in body {
                        walk_item(item, unions);
                    }
                }
            }
            _ => {}
        }
    }

    let mut unions = std::collections::BTreeMap::new();
    for item in items {
        walk_item(item, &mut unions);
    }
    unions
}

fn union_codec_needs(
    items: &[Item],
) -> (
    std::collections::BTreeSet<String>,
    std::collections::BTreeSet<String>,
) {
    fn collect_type(
        ty: &Type,
        encode: bool,
        decode: bool,
        encodes: &mut std::collections::BTreeSet<String>,
        decodes: &mut std::collections::BTreeSet<String>,
    ) {
        if let Type::Union(members) = ty {
            let name = crate::AST::union_enum_name(members);
            if encode {
                encodes.insert(name.clone());
            }
            if decode {
                decodes.insert(name);
            }
        }
        match ty {
            Type::List(inner)
            | Type::Shared(inner)
            | Type::Option(inner)
            | Type::Tagged { inner, .. }
            | Type::FixedList { elem: inner, .. }
            | Type::InlineRange { base: inner, .. }
            | Type::Quantity { base: inner, .. } => {
                collect_type(inner, encode, decode, encodes, decodes)
            }
            Type::Map { key, value, .. }
            | Type::Result {
                ok: key,
                err: value,
            } => {
                collect_type(key, encode, decode, encodes, decodes);
                collect_type(value, encode, decode, encodes, decodes);
            }
            Type::Apply { args, .. } | Type::Union(args) => {
                for arg in args {
                    collect_type(arg, encode, decode, encodes, decodes);
                }
            }
            Type::Tuple(fields) => {
                for (_, field) in fields {
                    collect_type(field, encode, decode, encodes, decodes);
                }
            }
            Type::Fn { params, ret, .. } => {
                for param in params {
                    collect_type(param, encode, decode, encodes, decodes);
                }
                if let Some(ret) = ret {
                    collect_type(ret, encode, decode, encodes, decodes);
                }
            }
            _ => {}
        }
    }

    fn walk_items(
        items: &[Item],
        auto: &crate::Traits::TraitRegistry,
        encodes: &mut std::collections::BTreeSet<String>,
        decodes: &mut std::collections::BTreeSet<String>,
    ) {
        for item in items {
            match item {
                Item::Struct(s) => {
                    let encode = s.derives.iter().any(|(name, _)| {
                        matches!(name.as_str(), crate::Generics::ENCODE | "Codable")
                    }) || auto.auto_encode.contains(&s.name);
                    let decode = s.derives.iter().any(|(name, _)| {
                        matches!(name.as_str(), crate::Generics::DECODE | "Codable")
                    }) || auto.auto_decode.contains(&s.name);
                    for field in &s.fields {
                        if !field
                            .serde_markers
                            .iter()
                            .any(|marker| marker.name == crate::Syntax::MARKER_SKIP)
                        {
                            collect_type(&field.ty, encode, decode, encodes, decodes);
                        }
                    }
                }
                Item::Enum(e) => {
                    let encode = e.derives.iter().any(|(name, _)| {
                        matches!(name.as_str(), crate::Generics::ENCODE | "Codable")
                    }) || auto.auto_encode.contains(&e.name);
                    let decode = e.derives.iter().any(|(name, _)| {
                        matches!(name.as_str(), crate::Generics::DECODE | "Codable")
                    }) || auto.auto_decode.contains(&e.name);
                    for variant in &e.variants {
                        match &variant.payload {
                            VariantPayload::Single(ty, _) => {
                                collect_type(ty, encode, decode, encodes, decodes)
                            }
                            VariantPayload::Named(fields) => {
                                for field in fields {
                                    collect_type(&field.ty, encode, decode, encodes, decodes);
                                }
                            }
                            VariantPayload::Unit => {}
                        }
                    }
                }
                Item::CodeModule(module) => {
                    if let Some(body) = &module.body {
                        walk_items(body, auto, encodes, decodes);
                    }
                }
                _ => {}
            }
        }
    }

    let mut encodes = std::collections::BTreeSet::new();
    let mut decodes = std::collections::BTreeSet::new();
    let auto = crate::Traits::TraitRegistry::auto_derives_for_items(items);
    walk_items(items, &auto, &mut encodes, &mut decodes);
    (encodes, decodes)
}

/// D-SERDE2=A / I3: built-in codecs are ordinary Jet AST items. The builder
/// below shares the same method/body representation as hand-written codecs;
/// only the declaration data comes from the reflected struct or enum. The
/// finished items enter the same typed template expander as user derives.
pub(crate) fn expand_builtin_serde_items(items: &mut Vec<Item>, diags: &mut Vec<Diagnostic>) {
    for item in items.iter_mut() {
        if let Item::CodeModule(module) = item {
            if let Some(body) = &mut module.body {
                expand_builtin_serde_items(body, diags);
            }
        }
    }
    // Comptime evaluation expands a private clone before the bundle pipeline
    // registers synthetic union enums. Materialize those declarations here as
    // well so the clone receives the same checked codec bodies as production.
    inject_anonymous_union_items(items);
    let auto = crate::Traits::TraitRegistry::auto_derives_for_items(items);
    expand_builtin_serde_items_with_auto_here(items, &auto, diags);
}

pub(crate) fn expand_builtin_serde_items_with_auto(
    items: &mut Vec<Item>,
    auto: &crate::Traits::TraitRegistry,
    diags: &mut Vec<Diagnostic>,
) {
    for item in items.iter_mut() {
        if let Item::CodeModule(module) = item {
            if let Some(body) = &mut module.body {
                expand_builtin_serde_items_with_auto(body, auto, diags);
            }
        }
    }
    inject_anonymous_union_items(items);
    expand_builtin_serde_items_with_auto_here(items, auto, diags);
}

fn expand_builtin_serde_items_with_auto_here(
    items: &mut Vec<Item>,
    auto: &crate::Traits::TraitRegistry,
    diags: &mut Vec<Diagnostic>,
) {
    let existing_generated = items
        .iter()
        .filter_map(|item| {
            let Item::Impl(implementation) = item else {
                return None;
            };
            implementation.is_generated_serde.then(|| {
                (
                    implementation.type_name.clone(),
                    implementation.trait_name.clone(),
                )
            })
        })
        .collect::<std::collections::HashSet<_>>();
    let mut generated_items = Vec::new();
    let snapshot = items.clone();
    for item in &snapshot {
        match item {
            Item::Enum(e) if e.name.starts_with("__JetUnion_") => {
                generated_items.extend(union_codec_items(e, &snapshot));
            }
            Item::Struct(s) => {
                let explicit_encode =
                    has_explicit_codec(&snapshot, &s.name, crate::Generics::ENCODE);
                let explicit_decode =
                    has_explicit_codec(&snapshot, &s.name, crate::Generics::DECODE);
                // An explicit marker is a request to materialize the compiler
                // candidate even when a source impl already exists. The normal
                // trait-registration pass then reports the duplicate method and
                // impl, while an unmarked hand codec remains the sole candidate.
                let encode = has_derive(&s.derives, crate::Generics::ENCODE)
                    || (!explicit_encode && auto.auto_encode.contains(&s.name));
                let decode = has_derive(&s.derives, crate::Generics::DECODE)
                    || (!explicit_decode && auto.auto_decode.contains(&s.name));
                if encode || decode {
                    let mut derived = s.clone();
                    if explicit_encode && !has_derive(&s.derives, crate::Generics::ENCODE) {
                        derived
                            .derives
                            .retain(|(name, _)| name != crate::Generics::ENCODE);
                    }
                    if explicit_decode && !has_derive(&s.derives, crate::Generics::DECODE) {
                        derived
                            .derives
                            .retain(|(name, _)| name != crate::Generics::DECODE);
                    }
                    if !explicit_encode {
                        add_auto_codec_marker(
                            &mut derived.derives,
                            crate::Generics::ENCODE,
                            &auto.auto_encode,
                            &derived.name,
                            derived.name_span,
                        );
                    }
                    if !explicit_decode {
                        add_auto_codec_marker(
                            &mut derived.derives,
                            crate::Generics::DECODE,
                            &auto.auto_decode,
                            &derived.name,
                            derived.name_span,
                        );
                    }
                    generated_items.extend(struct_codec_items(&derived));
                }
            }
            Item::Enum(e) => {
                let explicit_encode =
                    has_explicit_codec(&snapshot, &e.name, crate::Generics::ENCODE);
                let explicit_decode =
                    has_explicit_codec(&snapshot, &e.name, crate::Generics::DECODE);
                let encode = has_derive(&e.derives, crate::Generics::ENCODE)
                    || (!explicit_encode && auto.auto_encode.contains(&e.name));
                let decode = has_derive(&e.derives, crate::Generics::DECODE)
                    || (!explicit_decode && auto.auto_decode.contains(&e.name));
                if encode || decode {
                    let mut derived = e.clone();
                    if explicit_encode && !has_derive(&e.derives, crate::Generics::ENCODE) {
                        derived
                            .derives
                            .retain(|(name, _)| name != crate::Generics::ENCODE);
                    }
                    if explicit_decode && !has_derive(&e.derives, crate::Generics::DECODE) {
                        derived
                            .derives
                            .retain(|(name, _)| name != crate::Generics::DECODE);
                    }
                    if !explicit_encode {
                        add_auto_codec_marker(
                            &mut derived.derives,
                            crate::Generics::ENCODE,
                            &auto.auto_encode,
                            &derived.name,
                            derived.name_span,
                        );
                    }
                    if !explicit_decode {
                        add_auto_codec_marker(
                            &mut derived.derives,
                            crate::Generics::DECODE,
                            &auto.auto_decode,
                            &derived.name,
                            derived.name_span,
                        );
                    }
                    generated_items.extend(enum_codec_items(&derived));
                }
            }
            _ => {}
        }
    }
    generated_items.retain(|item| {
        let Item::Impl(implementation) = item else {
            return true;
        };
        !implementation.is_generated_serde
            || !existing_generated.contains(&(
                implementation.type_name.clone(),
                implementation.trait_name.clone(),
            ))
    });
    let generated_items = match crate::Comptime::expand_generated_items(generated_items) {
        Ok(items) => items,
        Err(diagnostic) => {
            diags.push(diagnostic);
            return;
        }
    };
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
fn has_explicit_codec(items: &[Item], type_name: &str, trait_name: &str) -> bool {
    items.iter().any(|item| match item {
        Item::Impl(implementation) => {
            implementation.type_name == type_name
                && implementation.trait_name.as_deref() == Some(trait_name)
                && !implementation.is_generated_serde
        }
        Item::Struct(definition) if definition.name == type_name => definition
            .trait_impls
            .iter()
            .any(|block| block.trait_name == trait_name && !block.compiler_generated),
        Item::Enum(definition) if definition.name == type_name => definition
            .trait_impls
            .iter()
            .any(|block| block.trait_name == trait_name && !block.compiler_generated),
        _ => false,
    })
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
        let reaches_wire = wire_types
            .iter()
            .any(|ty| crate::Generics::free_type_params(ty).contains(&param.name));
        if reaches_wire
            && encode
            && !param
                .bounds
                .iter()
                .any(|bound| bound == crate::Generics::ENCODE)
        {
            param.bounds.push(crate::Generics::ENCODE.to_string());
        }
        if reaches_wire
            && decode
            && !param
                .bounds
                .iter()
                .any(|bound| bound == crate::Generics::DECODE)
        {
            param.bounds.push(crate::Generics::DECODE.to_string());
        }
    }
    params
}

fn struct_codec_items(s: &crate::AST::StructDef) -> Vec<Item> {
    let encode = has_derive(&s.derives, crate::Generics::ENCODE);
    let decode = has_derive(&s.derives, crate::Generics::DECODE);
    let span = s
        .derives
        .iter()
        .find(|(name, _)| {
            matches!(
                name.as_str(),
                crate::Generics::ENCODE | crate::Generics::DECODE
            )
        })
        .map(|(_, span)| *span)
        .unwrap_or(s.name_span);
    let mut out = Vec::new();
    if encode {
        let params = codec_params(
            &s.type_params,
            s.fields
                .iter()
                .filter(|field| {
                    field.computed.is_none()
                        && !has_marker(&field.serde_markers, crate::Syntax::MARKER_SKIP)
                })
                .map(|field| field.ty.clone()),
            true,
            false,
        );
        out.push(Item::Impl(serde_impl(
            &s.name,
            crate::Generics::ENCODE,
            serde_method(
                "encode",
                params,
                vec![self_param(span)],
                Some(data_tree_type()),
                struct_encode_body(s, span),
                span,
            ),
            span,
        )));
    }
    if decode {
        let params = codec_params(
            &s.type_params,
            s.reflection_fields()
                .filter(|field| !has_marker(&field.serde_markers, crate::Syntax::MARKER_SKIP))
                .map(|field| field.ty.clone()),
            false,
            true,
        );
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
    let wire_types = e
        .variants
        .iter()
        .flat_map(|variant| match &variant.payload {
            VariantPayload::Unit => Vec::new(),
            VariantPayload::Single(ty, _) => vec![ty.clone()],
            VariantPayload::Named(fields) => fields.iter().map(|field| field.ty.clone()).collect(),
        });
    let params = codec_params(&e.type_params, wire_types, encode, decode);
    let span = e
        .derives
        .iter()
        .find(|(name, _)| {
            matches!(
                name.as_str(),
                crate::Generics::ENCODE | crate::Generics::DECODE
            )
        })
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

fn union_codec_items(e: &crate::AST::EnumDef, items: &[Item]) -> Vec<Item> {
    let encode = has_derive(&e.derives, crate::Generics::ENCODE);
    let decode = has_derive(&e.derives, crate::Generics::DECODE);
    let span = e
        .derives
        .iter()
        .find(|(name, _)| {
            matches!(
                name.as_str(),
                crate::Generics::ENCODE | crate::Generics::DECODE
            )
        })
        .map(|(_, span)| *span)
        .unwrap_or(e.name_span);
    let mut out = Vec::new();
    if encode {
        out.push(Item::Impl(serde_impl(
            &e.name,
            crate::Generics::ENCODE,
            serde_method(
                "encode",
                Vec::new(),
                vec![self_param(span)],
                Some(data_tree_type()),
                union_encode_body(e, span),
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
                Vec::new(),
                vec![named_param("tree", data_tree_type(), span)],
                Some(result_type(target_type(&e.name, &e.type_params), span)),
                union_decode_body(e, items, span),
                span,
            ),
            span,
        )));
    }
    out
}

fn union_encode_body(e: &crate::AST::EnumDef, span: Span) -> Vec<Stmt> {
    let arms = e
        .variants
        .iter()
        .map(|variant| {
            let binding = format!("jet_serde_union_{}", variant.name.replace('.', "_"));
            SwitchArm {
                cond: pattern_test("self", variant, vec![binding.clone()], span),
                body: vec![ret(
                    method(ident(&binding, span), "encode", Vec::new(), span),
                    span,
                )],
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

fn union_decode_body(e: &crate::AST::EnumDef, items: &[Item], span: Span) -> Vec<Stmt> {
    let mut body = Vec::new();
    for (index, variant) in e.variants.iter().enumerate() {
        let VariantPayload::Single(member, _) = &variant.payload else {
            continue;
        };
        let Some(shapes) = crate::AST::resolved_decode_wire_shapes(items, member) else {
            continue;
        };
        // Typed JSON keeps numeric lexemes in DataTree::Number so fixed-width
        // decoders can validate them without a lossy float round-trip. A
        // numeric member therefore accepts its native DataTree variant and
        // the compiler-only Number carrier; keep each runtime variant once.
        let mut wire_variants = Vec::new();
        for shape in shapes {
            match shape {
                crate::AST::SerdeWireShape::Int => {
                    if !wire_variants.contains(&"Int") {
                        wire_variants.push("Int");
                    }
                    if !wire_variants.contains(&"Number") {
                        wire_variants.push("Number");
                    }
                }
                crate::AST::SerdeWireShape::Float => {
                    if !wire_variants.contains(&"Float") {
                        wire_variants.push("Float");
                    }
                    if !wire_variants.contains(&"Number") {
                        wire_variants.push("Number");
                    }
                }
                shape => {
                    let name = shape.name();
                    if !wire_variants.contains(&name) {
                        wire_variants.push(name);
                    }
                }
            }
        }
        for wire_variant in wire_variants {
            let value_name = format!("jet_serde_union_value_{index}");
            let decode = try_expr(
                method_with_type_args(
                    copy(ident("tree", span), span),
                    "decode",
                    vec![member.clone()],
                    span,
                ),
                span,
            );
            let branch = vec![
                binding(&value_name, None, decode, false, span),
                ret(
                    ok(
                        enum_constructor_from_binding(variant, &value_name, &e.name, span),
                        span,
                    ),
                    span,
                ),
            ];
            let bindings = if wire_variant == "Null" {
                Vec::new()
            } else {
                vec![format!("jet_serde_union_wire_{index}")]
            };
            body.push(pattern_switch(
                copy(ident("tree", span), span),
                wire_variant,
                bindings,
                branch,
                Some(Vec::new()),
                span,
            ));
        }
    }
    body.push(no_matching_union(span));
    body
}

fn serde_impl(type_name: &str, trait_name: &str, method: Func, span: Span) -> ImplDef {
    ImplDef {
        span,
        type_name: type_name.to_string(),
        type_span: span,
        trait_name: Some(trait_name.to_string()),
        operator_rhs: None,
        operator_marker: None,
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
    function.compiler_generated = true;
    function
}

fn struct_encode_body(s: &crate::AST::StructDef, span: Span) -> Vec<Stmt> {
    let fields = s
        .fields
        .iter()
        .filter(|field| {
            field.computed.is_none()
                && !has_marker(&field.serde_markers, crate::Syntax::MARKER_SKIP)
        })
        .collect::<Vec<_>>();
    let has_flatten = fields
        .iter()
        .any(|field| has_marker(&field.serde_markers, crate::Syntax::MARKER_FLATTEN));
    if !has_flatten {
        return ordered_encode_fields(s, &fields, 0, Vec::new(), span);
    }

    let mut body = vec![binding(
        "out",
        Some(map_type(data_tree_type(), span)),
        map_literal(Vec::new(), span),
        true,
        span,
    )];
    for field in fields {
        let key = serde_field_key(s, field);
        if has_marker(&field.serde_markers, crate::Syntax::MARKER_FLATTEN) {
            let nested = format!("jet_serde_nested_{}", field.name);
            body.push(binding(
                &nested,
                None,
                method(field_value(field, span), "encode", Vec::new(), span),
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
                copy(field_value(field, span), span),
                &key,
                vec![field.name.clone()],
                span,
            ));
        } else {
            body.push(assign_index(
                "out",
                string_expr(&key, span),
                method(field_value(field, span), "encode", Vec::new(), span),
                span,
            ));
        }
    }
    body.push(ret(data_tree_object_expr(ident("out", span), span), span));
    body
}

fn ordered_encode_fields(
    structure: &crate::AST::StructDef,
    fields: &[&crate::AST::Field],
    index: usize,
    pairs: Vec<(Expr, Expr)>,
    span: Span,
) -> Vec<Stmt> {
    let Some(field) = fields.get(index) else {
        return vec![ret(data_tree_object(pairs, span), span)];
    };
    let key = serde_field_key(structure, field);
    let next_pairs = |value: Expr| {
        let mut next = pairs.clone();
        next.push((string_expr(&key, span), value));
        ordered_encode_fields(structure, fields, index + 1, next, span)
    };
    if matches!(field.ty, Type::Option(_)) {
        let binding_name = format!("jet_serde_option_value_{}", field.name.replace('.', "_"));
        let present = next_pairs(method(
            ident(&binding_name, span),
            "encode",
            Vec::new(),
            span,
        ));
        vec![option_switch(
            copy(field_value(field, span), span),
            &binding_name,
            present,
            ordered_encode_fields(structure, fields, index + 1, pairs, span),
            span,
        )]
    } else {
        next_pairs(method(field_value(field, span), "encode", Vec::new(), span))
    }
}

fn serde_decode_error_body(
    error_name: &str,
    missing: Option<&str>,
    key: Option<&str>,
    span: Span,
) -> Vec<Stmt> {
    let mut body = Vec::new();
    if let (Some(missing), Some(key)) = (missing, key) {
        body.push(if_stmt(
            ident(missing, span),
            vec![expr_stmt(method(
                ident("jet_serde_errors", span),
                "push",
                vec![field_error(
                    key,
                    &format!("E2410: missing required field `{key}`"),
                    span,
                )],
                span,
            ))],
            span,
        ));
        body.push(if_stmt(
            unary_not(ident(missing, span), span),
            vec![for_each(
                "jet_serde_field_error",
                ident(error_name, span),
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
        body.push(for_each(
            "jet_serde_field_error",
            ident(error_name, span),
            vec![expr_stmt(method(
                ident("jet_serde_errors", span),
                "push",
                vec![ident("jet_serde_field_error", span)],
                span,
            ))],
            span,
        ));
    }
    body
}

fn struct_decode_body(s: &crate::AST::StructDef, span: Span) -> Vec<Stmt> {
    let mut body = vec![binding(
        "jet_serde_errors",
        Some(field_error_list_type(span)),
        list_literal(Vec::new(), span),
        true,
        span,
    )];
    body.push(binding(
        "jet_serde_is_object",
        Some(Type::Bool),
        Expr::Bool(false, span),
        true,
        span,
    ));
    body.push(pattern_switch(
        copy(ident("tree", span), span),
        "Object",
        vec!["jet_serde_root_entries".to_string()],
        vec![assign_local(
            "jet_serde_is_object",
            Expr::Bool(true, span),
            span,
        )],
        Some(Vec::new()),
        span,
    ));
    body.push(if_stmt(
        unary_not(ident("jet_serde_is_object", span), span),
        vec![ret(
            err(
                list_literal(vec![field_error("", "expected an object", span)], span),
                span,
            ),
            span,
        )],
        span,
    ));

    let deny_unknown = has_marker(&s.serde_markers, crate::Syntax::MARKER_DENY_UNKNOWN_FIELDS);
    let has_flatten = s
        .reflection_fields()
        .any(|field| has_marker(&field.serde_markers, crate::Syntax::MARKER_FLATTEN));
    if deny_unknown && !has_flatten {
        let keys = s
            .reflection_fields()
            .filter(|field| !has_marker(&field.serde_markers, crate::Syntax::MARKER_SKIP))
            .map(|field| string_expr(&serde_field_key(s, field), span))
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
                    unary_not(
                        method(allowed.clone(), "contains", vec![ident("key", span)], span),
                        span,
                    ),
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
            let index = field_values.len();
            let slot = format!("jet_serde_field_value_{index}");
            let value = format!("jet_serde_decoded_value_{index}");
            let ok_value = format!("jet_serde_ok_value_{index}");
            body.push(binding(
                &slot,
                Some(Type::Option(Box::new(field.ty.clone()))),
                Expr::Absent(span),
                true,
                span,
            ));
            body.push(binding(
                &result,
                None,
                method_with_type_args(ident("tree", span), "decode", vec![field.ty.clone()], span),
                false,
                span,
            ));
            let error_name = format!("jet_serde_field_errors_{index}");
            body.push(result_switch(
                ident(&result, span),
                &ok_value,
                vec![assign_local(
                    &slot,
                    Expr::Present(Box::new(ident(&ok_value, span)), span),
                    span,
                )],
                &error_name,
                serde_decode_error_body(&error_name, None, None, span),
                span,
            ));
            decoded.push((slot, value.clone(), None, None));
            ident(&value, span)
        } else {
            let key = serde_field_key(s, field);
            let is_required =
                !matches!(field.ty, Type::Option(_)) && serde_default_expr(field).is_none();
            let missing = if is_required {
                let name = format!("jet_serde_missing_required_{}", decoded.len());
                body.push(binding(
                    &name,
                    Some(Type::Bool),
                    Expr::Bool(false, span),
                    true,
                    span,
                ));
                required.push((name.clone(), key.clone()));
                Some(name)
            } else {
                None
            };
            let subtree = if matches!(field.ty, Type::Option(_)) {
                or_fallback(
                    method(
                        ident("tree", span),
                        "field",
                        vec![string_expr(&key, span)],
                        span,
                    ),
                    data_tree_null(span),
                    span,
                )
            } else if let Some(default) = serde_default_expr(field) {
                or_fallback(
                    method(
                        ident("tree", span),
                        "field",
                        vec![string_expr(&key, span)],
                        span,
                    ),
                    method(default, "encode", Vec::new(), span),
                    span,
                )
            } else {
                or_fallback(
                    method(
                        ident("tree", span),
                        "field",
                        vec![string_expr(&key, span)],
                        span,
                    ),
                    data_tree_null(span),
                    span,
                )
            };
            let index = field_values.len();
            let result = format!("jet_serde_decode_{}", field.name);
            let slot = format!("jet_serde_field_value_{index}");
            let value = format!("jet_serde_decoded_value_{index}");
            let ok_value = format!("jet_serde_ok_value_{index}");
            let optional = matches!(field.ty, Type::Option(_));
            body.push(binding(
                &slot,
                Some(if optional {
                    field.ty.clone()
                } else {
                    Type::Option(Box::new(field.ty.clone()))
                }),
                Expr::Absent(span),
                true,
                span,
            ));
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
            let error_name = format!("jet_serde_field_errors_{index}");
            let ok_body = if optional {
                vec![assign_local(&slot, ident(&ok_value, span), span)]
            } else {
                vec![assign_local(
                    &slot,
                    Expr::Present(Box::new(ident(&ok_value, span)), span),
                    span,
                )]
            };
            body.push(result_switch(
                ident(&result, span),
                &ok_value,
                ok_body,
                &error_name,
                serde_decode_error_body(&error_name, missing.as_deref(), Some(&key), span),
                span,
            ));
            if optional {
                ident(&slot, span)
            } else {
                decoded.push((slot, value.clone(), missing, Some(key)));
                ident(&value, span)
            }
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
                        binary(
                            BinOp::Eq,
                            ident("jet_serde_presence_key", span),
                            string_expr(key, span),
                            span,
                        ),
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
    for (slot, value, _, _) in decoded.iter().rev() {
        success = vec![pattern_switch(
            ident(slot, span),
            "Val",
            vec![value.clone()],
            success,
            Some(Vec::new()),
            span,
        )];
    }
    body.push(if_stmt(
        method(
            ident("jet_serde_errors", span),
            "is_empty",
            Vec::new(),
            span,
        ),
        success,
        span,
    ));
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
    let style = serde_rename_all_style(&e.serde_markers);
    let arms = e
        .variants
        .iter()
        .map(|variant| {
            let bindings = payload_bindings(&variant.payload, "v");
            let value = enum_wire_value(variant, tag.as_deref(), untagged, style, span);
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
    style: Option<&str>,
    span: Span,
) -> Expr {
    let wire = serde_enum_variant_key(variant, style);
    if !untagged && tag.is_some() {
        if let VariantPayload::Named(fields) = &variant.payload {
            let mut entries = vec![(
                string_expr(tag.expect("tag"), span),
                data_tree_text(&wire, span),
            )];
            entries.extend(fields.iter().enumerate().map(|(index, field)| {
                (
                    string_expr(&field.name, span),
                    method(
                        ident(&format!("v{index}"), span),
                        "encode",
                        Vec::new(),
                        span,
                    ),
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
                        method(
                            ident(&format!("v{index}"), span),
                            "encode",
                            Vec::new(),
                            span,
                        ),
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
                if let Some(crate::AST::CallArg {
                    expr: Expr::MapLit(pairs, _),
                    ..
                }) = args.into_iter().next()
                {
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
    let style = serde_rename_all_style(&e.serde_markers);
    if untagged {
        let mut body = Vec::new();
        for variant in &e.variants {
            let payload_ty = enum_payload_type(variant);
            let decoded =
                method_with_type_args(ident("tree", span), "decode", vec![payload_ty], span);
            let binding_name = format!("jet_serde_enum_value_{}", variant.name.replace('.', "_"));
            let value =
                enum_constructor_from_binding(variant, &binding_name, e.name.as_str(), span);
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
                    method(
                        ident("tree", span),
                        "field",
                        vec![string_expr(&tag, span)],
                        span,
                    ),
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
            let wire = serde_enum_variant_key(variant, style);
            let payload_source = if matches!(variant.payload, VariantPayload::Single(..)) {
                try_expr(
                    method(
                        ident("tree", span),
                        "field",
                        vec![string_expr("value", span)],
                        span,
                    ),
                    span,
                )
            } else {
                ident("tree", span)
            };
            let value = enum_decode_value(variant, &payload_source, e.name.as_str(), span);
            body.push(if_stmt(
                binary(
                    BinOp::Eq,
                    ident("tag_value", span),
                    string_expr(&wire, span),
                    span,
                ),
                enum_decode_variant_body(variant, value, &payload_source, e.name.as_str(), span),
                span,
            ));
        }
        body.push(no_matching_enum(span));
        return body;
    }

    let mut body = Vec::new();
    for variant in &e.variants {
        let wire = serde_enum_variant_key(variant, style);
        if matches!(variant.payload, VariantPayload::Unit) {
            let binding_name = format!(
                "jet_serde_enum_variant_name_{}",
                variant.name.replace('.', "_")
            );
            let decoded = method_with_type_args(
                copy(ident("tree", span), span),
                "decode",
                vec![Type::String],
                span,
            );
            body.push(pattern_switch(
                decoded,
                "Ok",
                vec![binding_name.clone()],
                vec![if_stmt(
                    binary(
                        BinOp::Eq,
                        ident(&binding_name, span),
                        string_expr(&wire, span),
                        span,
                    ),
                    vec![ret(
                        ok(enum_constructor_unit(e.name.as_str(), variant, span), span),
                        span,
                    )],
                    span,
                )],
                Some(Vec::new()),
                span,
            ));
        } else {
            let candidate = format!("candidate_{}", variant.name.replace('.', "_"));
            let candidate_expr = or_fallback(
                method(
                    copy(ident("tree", span), span),
                    "field",
                    vec![string_expr(&wire, span)],
                    span,
                ),
                data_tree_null(span),
                span,
            );
            body.push(binding(&candidate, None, candidate_expr, false, span));
            let source = ident(&candidate, span);
            let value = enum_decode_value(variant, &source, e.name.as_str(), span);
            if matches!(variant.payload, VariantPayload::Single(..)) {
                let decoded =
                    method_with_type_args(source, "decode", vec![enum_payload_type(variant)], span);
                let binding_name =
                    format!("jet_serde_enum_decoded_{}", variant.name.replace('.', "_"));
                body.push(pattern_switch(
                    decoded,
                    "Ok",
                    vec![binding_name.clone()],
                    vec![ret(
                        ok(
                            enum_constructor_from_binding(
                                variant,
                                &binding_name,
                                e.name.as_str(),
                                span,
                            ),
                            span,
                        ),
                        span,
                    )],
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
        VariantPayload::Unit => vec![ret(
            ok(enum_constructor_unit(type_name, variant, span), span),
            span,
        )],
        VariantPayload::Single(ty, _) => {
            let decoded = method_with_type_args(source.clone(), "decode", vec![ty.clone()], span);
            let binding_name = format!("jet_serde_enum_decoded_{}", variant.name.replace('.', "_"));
            vec![pattern_switch(
                decoded,
                "Ok",
                vec![binding_name.clone()],
                vec![ret(
                    ok(
                        enum_constructor_from_binding(variant, &binding_name, type_name, span),
                        span,
                    ),
                    span,
                )],
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
            method(
                copy(source.clone(), span),
                "field",
                vec![string_expr(&field.name, span)],
                span,
            ),
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
        let error_name = format!(
            "jet_serde_enum_{}_errors_{index}",
            variant.name.replace('.', "_")
        );
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
        .map(|(_index, field)| {
            (
                field.name.clone(),
                ident(
                    &format!(
                        "jet_serde_enum_{}_value_{}",
                        variant.name.replace('.', "_"),
                        field.name
                    ),
                    span,
                ),
            )
        })
        .collect::<Vec<_>>();
    success.push(ret(
        ok(
            enum_constructor_named(type_name, variant, value_fields, span),
            span,
        ),
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
    body.push(if_stmt(
        method(ident(&errors_name, span), "is_empty", Vec::new(), span),
        success,
        span,
    ));
    body
}

fn enum_constructor_unit(type_name: &str, variant: &crate::AST::Variant, span: Span) -> Expr {
    Expr::EnumLit {
        type_name: type_name.to_string(),
        variant: variant.name.clone(),
        variant_span: None,
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
            variant_span: None,
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
        variant_span: None,
        args: fields
            .into_iter()
            .map(|(label, expr)| EnumLitArg::Named { label, expr })
            .collect(),
        leading_dot: false,
        span,
    }
}
