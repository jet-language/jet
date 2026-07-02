//! D-METAREFLECT1 / D-REFLECT1: build comptime reflection handles for user derives.
//!
//! `T.reflect()` in a derive body receives a `TypeInfo` value whose `.fields`,
//! `.methods`, `.type_params`, and `.markers` expose the target type's shape.

use crate::AST::{Field, Func, Marker, StructDef, TypeParam};

use super::Value::CtValue;

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
                ct_list(
                    param
                        .bounds
                        .iter()
                        .map(|b| ct_str(b.clone()))
                        .collect(),
                ),
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
    let type_params_info: Vec<CtValue> = s
        .type_params
        .iter()
        .map(build_type_param_info)
        .collect();
    ct_struct(
        "TypeInfo",
        &[
            ("name", ct_str(s.name.clone())),
            ("fields", ct_list(fields_info)),
            ("methods", ct_list(methods_info)),
            ("type_params", ct_list(type_params_info)),
            ("markers", ct_list(type_level_markers(s))),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AST::Type, Diagnostics::Span};

    fn span() -> Span {
        Span::new(0, 1)
    }

    fn field(name: &str, ty: &str, is_pub: bool) -> Field {
        Field {
            is_pub,
            is_package_pub: false,
            is_stored_ref: false,
            stored_ref_label: None,
            name: name.to_string(),
            name_span: span(),
            ty: Type::Named(ty.to_string()),
            ty_span: span(),
            serde_markers: Vec::new(),
            redact: false,
        }
    }

    fn method(name: &str, is_pub: bool) -> Func {
        Func {
            is_pub,
            is_package_pub: false,
            external_type: None,
            name: name.to_string(),
            name_span: span(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Some(Type::Named("String".to_string())),
            is_view_return: false,
            is_unsafe: false,
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
            body: Vec::new(),
        }
    }

    #[test]
    fn type_info_includes_methods_and_type_params() {
        let s = StructDef {
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
        assert!(markers.iter().any(|m| matches!(m, CtValue::Str(s) if s == "Debug")));
    }
}
