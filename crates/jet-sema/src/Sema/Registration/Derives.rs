use super::*;
use crate::AST::{ImplDef, Item, TraitImplBlock, Type};

use std::sync::LazyLock;

/// D-ONCE-DERIVE1=A: built-in derive providers are ordinary Prelude
/// declarations. Parse the checked-in source through the normal lexer/parser
/// instead of maintaining a second line-oriented provider language.
pub const DERIVE_SOURCE: &str =
    include_str!("../../../../jet-codegen/src/Prelude/Derives.jet");

static BUILTIN_DERIVE_PROVIDERS: LazyLock<Vec<crate::AST::DeriveDef>> =
    LazyLock::new(parse_builtin_derive_source);

fn parse_builtin_derive_source() -> Vec<crate::AST::DeriveDef> {
    let (tokens, lex_diagnostics) = crate::Lexer::lex_generated(DERIVE_SOURCE);
    if !lex_diagnostics.is_empty() {
        panic!("invalid Prelude derive declarations: {lex_diagnostics:?}");
    }
    let program = crate::Parser::parse(&tokens)
        .unwrap_or_else(|diagnostics| panic!("invalid Prelude derive declarations: {diagnostics:?}"));
    let mut names = std::collections::HashSet::new();
    let mut providers = Vec::new();
    for item in program.items {
        let Item::UserDerive(provider) = item else {
            panic!("Prelude/Derives.jet may contain only derive provider declarations");
        };
        providers.push(provider);
    }
    if providers.is_empty() {
        panic!("Prelude/Derives.jet declares no derive providers");
    }
    for provider in &providers {
        if !names.insert(provider.trait_name.clone()) {
            panic!(
                "Prelude/Derives.jet declares derive provider `{}` more than once",
                provider.trait_name
            );
        }
    }
    providers
}

fn builtin_derive_provider_enabled(trait_name: &str) -> bool {
    BUILTIN_DERIVE_PROVIDERS
        .iter()
        .any(|provider| provider.trait_name == trait_name)
}
fn builtin_derive_provider(trait_name: &str) -> Option<&'static crate::AST::DeriveDef> {
    BUILTIN_DERIVE_PROVIDERS
        .iter()
        .find(|provider| provider.trait_name == trait_name)
}

/// Expand a Prelude provider through the same typed template boundary used by
/// package-authored derives. Provider functions are converted to the selected
/// capability implementation only after template expansion, so their bodies
/// remain ordinary user-template AST all the way to the sema registry.
///
/// A failed source expansion reports its diagnostic and produces nothing;
/// the checked-in source templates are the only provider mechanism.
fn expand_builtin_provider_body(
    trait_name: &str,
    target_name: &str,
    target_span: Span,
    owner_type: &Type,
    type_info: crate::AST::CtValue,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Item> {
    let Some(provider) = builtin_derive_provider(trait_name) else {
        return Vec::new();
    };
    let expanded = match crate::Comptime::expand_derive_body(
        &provider.body,
        &provider.type_param,
        type_info,
        &std::collections::HashMap::new(),
        std::path::Path::new("."),
        None,
    ) {
        Ok(items) => items,
        Err(diagnostic) => {
            diags.push(diagnostic);
            return Vec::new();
        }
    };
    let mut ordinary = Vec::new();
    let mut methods = Vec::new();
    for item in expanded {
        match item {
            Item::Func(mut function) => {
                for parameter in &mut function.params {
                    replace_provider_type(
                        &mut parameter.ty,
                        &provider.type_param,
                        owner_type,
                    );
                }
                if let Some(return_type) = &mut function.return_type {
                    replace_provider_type(return_type, &provider.type_param, owner_type);
                }
                function.compiler_generated = true;
                methods.push(function);
            }
            other => ordinary.push(other),
        }
    }
    if !methods.is_empty() {
        ordinary.push(Item::Impl(ImplDef {
            span: target_span,
            type_name: target_name.to_string(),
            type_span: target_span,
            trait_name: Some(trait_name.to_string()),
            operator_marker: None,
            operator_rhs: None,
            trait_span: Some(target_span),
            methods,
            delegation_field: None,
            assoc_type_impls: Vec::new(),
            is_generated_serde: false,
            os_target: None,
        }));
    }
    ordinary
}

fn provider_type_info(type_info: crate::AST::CtValue, kind: &str) -> crate::AST::CtValue {
    let mut type_info = type_info;
    if let crate::AST::CtValue::Struct { fields, .. } = &mut type_info {
        if !fields.iter().any(|(name, _)| name == "kind") {
            fields.push((
                "kind".to_string(),
                crate::AST::CtValue::Str(kind.to_string()),
            ));
        }
    }
    type_info
}

fn replace_provider_type(ty: &mut Type, type_param: &str, owner_type: &Type) {
    if matches!(ty, Type::Named(name) if name == type_param) {
        *ty = owner_type.clone();
        return;
    }
    match ty {
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::FixedList { elem: inner, .. }
        | Type::InlineRange { base: inner, .. }
        | Type::Tagged { inner, .. }
        | Type::Quantity { base: inner, .. } => {
            replace_provider_type(inner, type_param, owner_type);
        }
        Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
            replace_provider_type(key, type_param, owner_type);
            replace_provider_type(value, type_param, owner_type);
        }
        Type::Fn { params, ret, .. } => {
            for param in params {
                replace_provider_type(param, type_param, owner_type);
            }
            if let Some(ret) = ret {
                replace_provider_type(ret, type_param, owner_type);
            }
        }
        Type::Apply { args, .. } | Type::Union(args) => {
            for arg in args {
                replace_provider_type(arg, type_param, owner_type);
            }
        }
        Type::Tuple(fields) => {
            for (_, field) in fields {
                replace_provider_type(field, type_param, owner_type);
            }
        }
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::Named(_)
        | Type::TraitObject(_)
        | Type::IntN { .. }
        | Type::Float32
        | Type::Measure(_) => {}
    }
}

/// D-ONCE-DERIVE1=A / I3: compiler-owned capability requests use the same
/// typed derive-template engine as user-authored code. Reflected enum variant
/// shape supplies the same nested field primitive as struct providers.
pub(in super::super) fn expand_builtin_derive_items(
    items: &mut Vec<Item>,
    diags: &mut Vec<Diagnostic>,
) {
    normalize_recursive_enum_payloads(items);
    let auto = crate::Traits::TraitRegistry::auto_derives_for_items(items);
    expand_builtin_derive_items_with_auto(items, &auto, diags);
}


fn normalize_recursive_enum_payloads(items: &mut [Item]) {
    for item in items {
        let Item::Enum(enum_def) = item else {
            continue;
        };
        let owner = enum_def.name.as_str();
        for variant in &mut enum_def.variants {
            normalize_variant_payload(&mut variant.payload, owner);
        }
    }
}

pub(super) fn normalize_variant_payload(payload: &mut crate::AST::VariantPayload, owner: &str) {
    match payload {
        crate::AST::VariantPayload::Unit => {}
        crate::AST::VariantPayload::Single(ty, _) => {
            normalize_recursive_payload_type(ty, owner);
        }
        crate::AST::VariantPayload::Named(fields) => {
            for field in fields {
                normalize_recursive_payload_type(&mut field.ty, owner);
            }
        }
    }
}

pub(super) fn normalize_recursive_payload_type(ty: &mut Type, owner: &str) {
    if matches!(ty, Type::Apply { name, args } if name == owner && args.is_empty()) {
        *ty = Type::Named(owner.to_string());
        return;
    }
    match ty {
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::FixedList { elem: inner, .. }
        | Type::InlineRange { base: inner, .. }
        | Type::Tagged { inner, .. } => normalize_recursive_payload_type(inner, owner),
        Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
            normalize_recursive_payload_type(key, owner);
            normalize_recursive_payload_type(value, owner);
        }
        Type::Fn { params, ret, .. } => {
            for param in params {
                normalize_recursive_payload_type(param, owner);
            }
            if let Some(ret) = ret {
                normalize_recursive_payload_type(ret, owner);
            }
        }
        Type::Apply { args, .. } | Type::Union(args) => {
            for arg in args {
                normalize_recursive_payload_type(arg, owner);
            }
        }
        Type::Tuple(fields) => {
            for (_, field) in fields {
                normalize_recursive_payload_type(field, owner);
            }
        }
        Type::Quantity { base, .. } => normalize_recursive_payload_type(base, owner),
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::Named(_)
        | Type::TraitObject(_)
        | Type::IntN { .. }
        | Type::Float32
        | Type::Measure(_) => {}
    }
}

pub(in super::super) fn expand_builtin_derive_items_with_auto(
    items: &mut Vec<Item>,
    auto: &crate::Traits::TraitRegistry,
    diags: &mut Vec<Diagnostic>,
) {
    normalize_recursive_enum_payloads(items);
    let struct_equatable = builtin_derive_provider_enabled(crate::Generics::EQUATABLE);
    let struct_comparable = builtin_derive_provider_enabled(crate::Generics::COMPARABLE);
    let enum_equatable = builtin_derive_provider_enabled(crate::Generics::EQUATABLE);
    let enum_comparable = builtin_derive_provider_enabled(crate::Generics::COMPARABLE);
    let distinct_equatable = builtin_derive_provider_enabled(crate::Generics::EQUATABLE);
    let distinct_comparable = builtin_derive_provider_enabled(crate::Generics::COMPARABLE);

    let invalid_distinct_names: std::collections::HashSet<String> = items
        .iter()
        .filter_map(|item| {
            let Item::Distinct(d) = item else { return None };
            let Type::Named(base) = &d.base else {
                return None;
            };
            items
                .iter()
                .any(|item| matches!(item, Item::Distinct(other) if other.name == *base))
                .then(|| d.name.clone())
        })
        .collect();

    let mut generated = Vec::new();
    let snapshot = items.clone();

    for item in &snapshot {
        match item {
            Item::Struct(s) => {
                let comparable = struct_comparable
                    && (has_derive(&s.derives, crate::Generics::COMPARABLE)
                        || auto.auto_comparable.contains(&s.name));
                let equatable = struct_equatable
                    && (has_derive(&s.derives, crate::Generics::EQUATABLE)
                        || auto.auto_equatable.contains(&s.name));
                let owner_type = applied_owner_type(&s.name, &s.type_params);
                if equatable {
                    generated.extend(expand_builtin_provider_body(
                        crate::Generics::EQUATABLE,
                        &s.name,
                        s.name_span,
                        &owner_type,
                        provider_type_info(crate::Comptime::build_struct_type_info(s), "struct"),
                        diags,
                    ));
                }
                if comparable {
                    generated.extend(expand_builtin_provider_body(
                        crate::Generics::COMPARABLE,
                        &s.name,
                        s.name_span,
                        &owner_type,
                        provider_type_info(crate::Comptime::build_struct_type_info(s), "struct"),
                        diags,
                    ));
                }
            }
            Item::Enum(e) => {
                let comparable = enum_comparable
                    && (has_derive(&e.derives, crate::Generics::COMPARABLE)
                        || auto.auto_comparable.contains(&e.name));
                let equatable = enum_equatable
                    && (has_derive(&e.derives, crate::Generics::EQUATABLE)
                        || auto.auto_equatable.contains(&e.name));
                let owner_type = applied_owner_type(&e.name, &e.type_params);
                if equatable {
                    generated.extend(expand_builtin_provider_body(
                        crate::Generics::EQUATABLE,
                        &e.name,
                        e.name_span,
                        &owner_type,
                        provider_type_info(crate::Comptime::build_enum_type_info(e), "enum"),
                        diags,
                    ));
                }
                if comparable {
                    generated.extend(expand_builtin_provider_body(
                        crate::Generics::COMPARABLE,
                        &e.name,
                        e.name_span,
                        &owner_type,
                        provider_type_info(crate::Comptime::build_enum_type_info(e), "enum"),
                        diags,
                    ));
                }
            }
            Item::Distinct(d) if !invalid_distinct_names.contains(&d.name) => {
                let owner_type = Type::Named(d.name.clone());
                let type_info = provider_type_info(
                    crate::Comptime::build_distinct_type_info(d, ""),
                    "distinct",
                );
                if distinct_equatable {
                    generated.extend(expand_builtin_provider_body(
                        crate::Generics::EQUATABLE,
                        &d.name,
                        d.name_span,
                        &owner_type,
                        type_info.clone(),
                        diags,
                    ));
                }
                if distinct_comparable
                    && (has_derive(&d.derives, crate::Generics::COMPARABLE)
                        || auto.auto_comparable.contains(&d.name))
                {
                    generated.extend(expand_builtin_provider_body(
                        crate::Generics::COMPARABLE,
                        &d.name,
                        d.name_span,
                        &owner_type,
                        type_info,
                        diags,
                    ));
                }
            }
            Item::UnitFamily(family) => {
                for d in family.distinct_defs() {
                    let owner_type = Type::Named(d.name.clone());
                    let type_info = provider_type_info(
                        crate::Comptime::build_distinct_type_info(&d, ""),
                        "distinct",
                    );
                    if distinct_equatable {
                        generated.extend(expand_builtin_provider_body(
                            crate::Generics::EQUATABLE,
                            &d.name,
                            d.name_span,
                            &owner_type,
                            type_info.clone(),
                            diags,
                        ));
                    }
                    if distinct_comparable
                        && (has_derive(&d.derives, crate::Generics::COMPARABLE)
                            || auto.auto_comparable.contains(&d.name))
                    {
                        generated.extend(expand_builtin_provider_body(
                            crate::Generics::COMPARABLE,
                            &d.name,
                            d.name_span,
                            &owner_type,
                            type_info,
                            diags,
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    for item in generated {
        attach_generated_derive_item(items, item);
    }
}

fn has_derive(derives: &[(String, Span)], name: &str) -> bool {
    derives.iter().any(|(derive, _)| derive == name)
}

fn attach_generated_derive_item(items: &mut Vec<Item>, item: Item) {
    let Item::Impl(implementation) = item else {
        if let Item::Func(function) = item {
            if !items
                .iter()
                .any(|existing| matches!(existing, Item::Func(old) if old.name == function.name))
            {
                items.push(Item::Func(function));
            }
        }
        return;
    };
    let Some(trait_name) = implementation.trait_name.clone() else {
        return;
    };
    if has_trait_impl(items, &implementation.type_name, &trait_name) {
        return;
    }
    if let Some(target) = items.iter_mut().find_map(|item| match item {
        Item::Struct(s) if s.name == implementation.type_name => Some(&mut s.trait_impls),
        Item::Enum(e) if e.name == implementation.type_name => Some(&mut e.trait_impls),
        _ => None,
    }) {
        target.push(TraitImplBlock {
            trait_name,
            trait_span: implementation
                .trait_span
                .unwrap_or(implementation.type_span),
            operator_rhs: implementation.operator_rhs,
            operator_marker: implementation.operator_marker,
            methods: implementation.methods,
            compiler_generated: true,
            assoc_type_impls: implementation.assoc_type_impls,
        });
    } else {
        items.push(Item::Impl(implementation));
    }
}

fn has_trait_impl(items: &[Item], type_name: &str, trait_name: &str) -> bool {
    items.iter().any(|item| match item {
        Item::Impl(i) => i.type_name == type_name && i.trait_name.as_deref() == Some(trait_name),
        Item::Struct(s) => {
            s.name == type_name && s.trait_impls.iter().any(|i| i.trait_name == trait_name)
        }
        Item::Enum(e) => {
            e.name == type_name && e.trait_impls.iter().any(|i| i.trait_name == trait_name)
        }
        _ => false,
    })
}

fn applied_owner_type(name: &str, type_params: &[crate::AST::TypeParam]) -> Type {
    if type_params.is_empty() {
        Type::Named(name.to_string())
    } else {
        Type::Apply {
            name: name.to_string(),
            args: type_params
                .iter()
                .map(|param| Type::Named(param.name.clone()))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursive_enum_derive_uses_cartesian_payload_bindings() {
        jet_codegen::Codegen::MIREval::install_mir_bridge();
        let src = "enum Expr { Num(Int)\nWrap(Expr) }";
        let (tokens, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty());
        let mut program = crate::Parser::parse(&tokens).expect("source parses");
        let mut diags = Vec::new();
        expand_builtin_derive_items(&mut program.items, &mut diags);
        assert!(diags.is_empty(), "built-in derive diagnostics: {diags:?}");
        let rendered = crate::Formatter::format_synthetic_program(&program);
        assert!(
            rendered.contains("left_Num_value != right_Num_Num_value"),
            "Int payload should compare with !=, got:\n{rendered}"
        );
        assert!(
            rendered.contains("left_Wrap_value.equal(right_Wrap_Wrap_value)"),
            "recursive payload should call equal, got:\n{rendered}"
        );
        assert!(
            rendered.contains(".Wrap(right_Num_Wrap_value)"),
            "rhs bindings must include both variants, got:\n{rendered}"
        );
        assert!(
            !rendered.contains("left_Num_value.equal"),
            "Int payload must not call equal, got:\n{rendered}"
        );
    }

    #[test]
    fn builtin_derive_source_expands_to_ast_item() {
        jet_codegen::Codegen::MIREval::install_mir_bridge();
        let (tokens, lex_diags) = crate::Lexer::lex("#Comparable struct Point { value: Int }");
        assert!(lex_diags.is_empty());
        let mut program = crate::Parser::parse(&tokens).expect("source parses");
        let mut diags = Vec::new();
        expand_builtin_derive_items(&mut program.items, &mut diags);
        assert!(diags.is_empty(), "built-in derive diagnostics: {diags:?}");
        assert!(program.items.iter().any(|item| matches!(
            item,
            Item::Struct(s)
                if s.trait_impls
                    .iter()
                    .any(|implementation| implementation.trait_name == Syntax::MARKER_COMPARABLE)
        )));
    }

    #[test]
    fn register_enum_canonicalizes_recursive_payloads() {
        let src = "enum Expr { Num(Int)\nWrap(Expr) }";
        let (tokens, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty());
        let program = crate::Parser::parse(&tokens).expect("source parses");
        let Item::Enum(enum_def) = &program.items[0] else {
            panic!("expected enum");
        };
        let mut registry = TypeRegistry {
            types: std::collections::HashMap::new(),
            error_types: std::collections::HashSet::new(),
            unit_types: std::collections::HashSet::new(),
            unit_facts: std::collections::HashMap::new(),
            literal_facts: std::collections::HashMap::new(),
            computed_fields: std::collections::HashMap::new(),
            field_defaults: std::collections::HashMap::new(),
            receipt_sections: std::collections::HashMap::new(),
            devtools_publications: std::cell::RefCell::new(Vec::new()),
        };
        let mut diags = Vec::new();
        register_enum(
            enum_def,
            &mut registry,
            &mut diags,
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
        );
        assert!(diags.is_empty(), "{diags:?}");
        let variants = registry.enum_variants("Expr").expect("enum registered");
        let (_, payload) = variants.get("Wrap").expect("Wrap variant");
        match payload {
            crate::AST::VariantPayload::Single(ty, _) => {
                assert_eq!(*ty, Type::Named("Expr".to_string()));
            }
            other => panic!("expected single payload, got {other:?}"),
        }
    }

    #[test]
    fn nested_derive_methods_register_on_the_type() {
        jet_codegen::Codegen::MIREval::install_mir_bridge();
        let src = "enum Expr { Num(Int)\nWrap(Expr) }";
        let (tokens, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty());
        let mut program = crate::Parser::parse(&tokens).expect("source parses");
        let mut diags = Vec::new();
        expand_builtin_derive_items(&mut program.items, &mut diags);
        assert!(diags.is_empty(), "built-in derive diagnostics: {diags:?}");
        let mut registry = TypeRegistry {
            types: std::collections::HashMap::new(),
            error_types: std::collections::HashSet::new(),
            unit_types: std::collections::HashSet::new(),
            unit_facts: std::collections::HashMap::new(),
            literal_facts: std::collections::HashMap::new(),
            computed_fields: std::collections::HashMap::new(),
            field_defaults: std::collections::HashMap::new(),
            receipt_sections: std::collections::HashMap::new(),
            devtools_publications: std::cell::RefCell::new(Vec::new()),
        };
        for item in &program.items {
            if let Item::Enum(enum_def) = item {
                register_enum(
                    enum_def,
                    &mut registry,
                    &mut diags,
                    &std::collections::HashMap::new(),
                    &std::collections::HashMap::new(),
                );
            }
        }
        register_type_methods(&program.items, &mut registry, &mut diags);
        assert!(
            registry.method("Expr", "equal").is_some(),
            "nested Equatable.equal must be visible on the enum"
        );
    }
}
