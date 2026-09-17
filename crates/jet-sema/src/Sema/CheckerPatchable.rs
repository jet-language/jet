//! D-PATCH1 (card #181): `#[Patchable]` derive — synthetic `T.Patch` type + apply/diff/merge.

use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use crate::AST::{AccessConvention, Field, Item, StructDef, Type};

use super::{MethodSig, TypeDef, TypeRegistry};

pub(crate) fn patch_type_name(base: &str) -> String {
    format!("{base}.Patch")
}

fn has_patchable(s: &StructDef) -> bool {
    s.derives.iter().any(|(t, _)| t == Syntax::MARKER_PATCHABLE)
}

/// Append synthetic `T.Patch` struct items (Codable via Encode+Decode) before registration.
pub(crate) fn inject_patchable_types(items: &mut Vec<Item>, diags: &mut Vec<Diagnostic>) {
    let mut to_add = Vec::new();
    let mut generated_base_methods = Vec::new();
    for item in items.iter() {
        let Item::Struct(s) = item else { continue };
        if !has_patchable(s) {
            continue;
        }
        if let Some(d) = validate_patchable_struct(s) {
            diags.push(d);
            continue;
        }
        let patch_name = patch_type_name(&s.name);
        // D-FIELDPOL1: a computed field can't hold an "unchanged" sentinel —
        // it never appears in `T.Patch`, and `apply`/`diff`/`merge` skip it.
        let fields: Vec<Field> = s
            .fields
            .iter()
            .filter(|f| f.computed.is_none())
            .map(|f| Field {
                name: f.name.clone(),
                name_span: f.name_span,
                ty: Type::Option(Box::new(f.ty.clone())),
                ty_span: f.ty_span,
                is_pub: f.is_pub,
                is_package_pub: f.is_package_pub,
                serde_markers: Vec::new(),
                redact: false,
                computed: None,
                default: None,
                default_ct: None,
            })
            .collect();
        let (base_methods, patch_methods) = generated_patchable_methods(s);
        to_add.push(Item::Struct(StructDef {
            span: s.span,
            is_pub: s.is_pub,
            is_package_pub: s.is_package_pub,
            name: patch_name.clone(),
            name_span: s.name_span,
            type_params: Vec::new(),
            fields,
            state: None,
            methods: patch_methods,
            cli_bindings: Vec::new(),
            trait_impls: Vec::new(),
            derives: Vec::new(),
            // D-PATCH1: `T.Patch` serde is deferred; do not inherit the
            // source struct's package auto-derive default.
            auto_derive_default: false,
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
        }));
        generated_base_methods.push((s.name.clone(), base_methods));
    }
    for (name, methods) in generated_base_methods {
        if let Some(Item::Struct(s)) = items.iter_mut().find(|item| {
            matches!(item, Item::Struct(definition) if definition.name == name)
        }) {
            s.methods.extend(methods);
        }
    }
    items.extend(to_add);
}

fn generated_patchable_methods(
    s: &StructDef,
) -> (Vec<crate::AST::Func>, Vec<crate::AST::Func>) {
    let base = s.name.as_str();
    let patch = patch_type_name(base);
    let fields = s
        .fields
        .iter()
        .filter(|field| field.computed.is_none())
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    let mut source = format!("struct __JetPatchableMethods {{\n");
    source.push_str(&format!(
        "    fn apply(self, patch: {patch}) {base} -> {{\n        return {base}{{\n"
    ));
    for (index, field) in fields.iter().enumerate() {
        if index != 0 {
            source.push_str(",\n");
        }
        source.push_str(&format!(
            "            {field}: patch.{field} ?? self.{field}"
        ));
    }
    source.push_str("\n        }\n    }\n\n");
    source.push_str(&format!(
        "    fn diff(^new: {base}, ^old: {base}) {patch} -> {{\n        return {patch}{{\n"
    ));
    for (index, field) in fields.iter().enumerate() {
        if index != 0 {
            source.push_str(",\n");
        }
        source.push_str(&format!(
            "            {field}: if new.{field} == old.{field} -> None else -> Val(new.{field})"
        ));
    }
    source.push_str("\n        }\n    }\n}\n\n");
    source.push_str("struct __JetPatchablePatchMethods {\n");
    source.push_str(&format!(
        "    fn merge(self, ^other: {patch}) {patch} -> {{\n        return {patch}{{\n"
    ));
    for (index, field) in fields.iter().enumerate() {
        if index != 0 {
            source.push_str(",\n");
        }
        source.push_str(&format!(
            "            {field}: if self.{field} == None -> other.{field} else -> self.{field}"
        ));
    }
    source.push_str("\n        }\n    }\n}\n");

    let (tokens, lex_diagnostics) = crate::Lexer::lex_generated(&source);
    if !lex_diagnostics.is_empty() {
        panic!("invalid generated Patchable methods: {lex_diagnostics:?}");
    }
    let mut program = crate::Parser::parse(&tokens)
        .unwrap_or_else(|diagnostics| panic!("invalid generated Patchable methods: {diagnostics:?}"));
    let mut base_methods = Vec::new();
    let mut patch_methods = Vec::new();
    for item in &mut program.items {
        let Item::Struct(definition) = item else {
            continue;
        };
        for method in &mut definition.methods {
            method.span = s.span;
            method.name_span = s.name_span;
            method.compiler_generated = true;
        }
        if definition.name == "__JetPatchableMethods" {
            base_methods = std::mem::take(&mut definition.methods);
        } else if definition.name == "__JetPatchablePatchMethods" {
            patch_methods = std::mem::take(&mut definition.methods);
        }
    }
    (base_methods, patch_methods)
}




fn validate_patchable_struct(s: &StructDef) -> Option<Diagnostic> {
    if !s.type_params.is_empty() {
        return Some(Diagnostic::error(
            "E0336",
            format!(
                "`#[Patchable]` on generic struct `{}` isn't supported yet",
                s.name
            ),
            "`T.Patch` codegen needs a concrete field list — generic patches are a follow-on"
                .to_string(),
            "remove the type parameters, or drop `#[Patchable]` for now".to_string(),
            Some(s.name_span),
        ));
    }
    for f in &s.fields {
        if matches!(f.ty, Type::Fn { .. }) {
            return Some(Diagnostic::error(
                "E0337",
                format!(
                    "`#[Patchable]` struct `{}` field `{}` has function type",
                    s.name, f.name
                ),
                "patches are data values — function-typed fields can't be patched".to_string(),
                "store a callable handle type instead, or drop `#[Patchable]`".to_string(),
                Some(f.name_span),
            ));
        }
    }
    None
}

/// Register apply / diff / merge after base + Patch types exist in the registry.
pub(crate) fn register_patchable_methods(items: &[Item], registry: &mut TypeRegistry) {
    for item in items {
        let Item::Struct(s) = item else { continue };
        if !has_patchable(s) {
            continue;
        }
        let patch = patch_type_name(&s.name);
        if !registry.contains(&patch) {
            continue;
        }
        let base = s.name.clone();
        let generated = items.iter().any(|item| {
            matches!(
                item,
                Item::Struct(definition)
                    if definition.name == base
                        && definition.methods.iter().any(|method| {
                            method.compiler_generated && method.name == "apply"
                        })
                        && definition.methods.iter().any(|method| {
                            method.compiler_generated && method.name == "diff"
                        })
            )
        }) && items.iter().any(|item| {
            matches!(
                item,
                Item::Struct(definition)
                    if definition.name == patch
                        && definition.methods.iter().any(|method| {
                            method.compiler_generated && method.name == "merge"
                        })
            )
        });
        if generated {
            continue;
        }
        let base_ty = Type::Named(base.clone());
        let patch_ty = Type::Named(patch.clone());

        if let Some(TypeDef::Struct { methods, .. }) = registry.types.get_mut(&base) {
            methods.insert(
                "apply".to_string(),
                MethodSig {
                    params: vec![
                        (AccessConvention::Read, base_ty.clone()),
                        (AccessConvention::Move, patch_ty.clone()),
                    ],
                    return_type: Some(base_ty.clone()),
                    deprecation: None,
                    type_params: Vec::new(),
                    is_static: false,
                    self_conv: Some(AccessConvention::Read),
                    param_info: vec![("patch".to_string(), false)],
                    param_call: vec![("patch".to_string(), crate::AST::ParamZone::Either)],
                    param_variadic: vec![false],
                    defaults: vec![None],
                    must_use: false,
                    return_view_provenance: Default::default(),
                },
            );
            methods.insert(
                "diff".to_string(),
                MethodSig {
                    params: vec![
                        (AccessConvention::Move, base_ty.clone()),
                        (AccessConvention::Move, base_ty),
                    ],
                    return_type: Some(patch_ty.clone()),
                    deprecation: None,
                    type_params: Vec::new(),
                    is_static: true,
                    self_conv: None,
                    param_info: vec![("new".to_string(), false), ("old".to_string(), false)],
                    param_call: vec![
                        ("new".to_string(), crate::AST::ParamZone::Either),
                        ("old".to_string(), crate::AST::ParamZone::Either),
                    ],
                    param_variadic: vec![false, false],
                    defaults: vec![None, None],
                    must_use: false,
                    return_view_provenance: Default::default(),
                },
            );
        }
        if let Some(TypeDef::Struct { methods, .. }) = registry.types.get_mut(&patch) {
            methods.insert(
                "merge".to_string(),
                MethodSig {
                    params: vec![
                        (AccessConvention::Read, patch_ty.clone()),
                        (AccessConvention::Move, patch_ty),
                    ],
                    return_type: Some(Type::Named(patch)),
                    deprecation: None,
                    type_params: Vec::new(),
                    is_static: false,
                    self_conv: Some(AccessConvention::Read),
                    param_info: vec![("other".to_string(), false)],
                    param_call: vec![("other".to_string(), crate::AST::ParamZone::Either)],
                    param_variadic: vec![false],
                    defaults: vec![None],
                    must_use: false,
                    return_view_provenance: Default::default(),
                },
            );
        }
    }
}

/// D-SHAPE-PROJECT1=A: every `#CLI` struct owns the precedence combinator
/// `T.merge(flags, settings) T ![FieldError]`: an explicit flag wins, then an
/// environment value, then the field default. A present invalid value is a
/// `FieldError` from the layer that decoded it, never absence.
pub(crate) fn register_cli_merge_methods(items: &[Item], registry: &mut TypeRegistry) {
    for item in items {
        let Item::Struct(s) = item else { continue };
        if !s.derives.iter().any(|(name, _)| name == crate::Syntax::MARKER_CLI) {
            continue;
        }
        let Some(TypeDef::Struct { methods, .. }) = registry.types.get_mut(&s.name) else {
            continue;
        };
        if methods.contains_key("merge") {
            continue;
        }
        let ty = Type::Named(s.name.clone());
        methods.insert(
            "merge".to_string(),
            MethodSig {
                params: vec![
                    (AccessConvention::Read, ty.clone()),
                    (AccessConvention::Read, ty.clone()),
                ],
                return_type: Some(Type::Result {
                    ok: Box::new(ty),
                    err: Box::new(Type::List(Box::new(Type::Named("FieldError".to_string())))),
                }),
                deprecation: None,
                type_params: Vec::new(),
                is_static: true,
                self_conv: None,
                param_info: vec![("flags".to_string(), false), ("settings".to_string(), false)],
                param_call: vec![
                    ("flags".to_string(), crate::AST::ParamZone::Either),
                    ("settings".to_string(), crate::AST::ParamZone::Either),
                ],
                param_variadic: vec![false, false],
                defaults: vec![None, None],
                must_use: true,
                return_view_provenance: Default::default(),
            },
        );
    }
}
