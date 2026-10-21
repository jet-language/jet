use super::core_types::{decode_error_ty, u8_ty};
use crate::AST::Type;

/// A fixed data row accepts its canonical Core owner, never a same-leaf
/// user-qualified nominal. Bare names remain the bootstrap row spelling.
pub fn data_nominal_leaf(name: &str) -> Option<&str> {
    if name.starts_with("<corelib>/Core/Data::") {
        return name.rsplit("::").next();
    }
    (!name.contains("::") && !name.contains('.')).then_some(name)
}

pub(crate) fn dyn_float_literal(expr: &crate::AST::Expr) -> bool {
    use crate::AST::{Expr, UnOp};
    match expr {
        Expr::Float(_, _, false, Some(_)) => true,
        Expr::Paren(inner, _) | Expr::Unary(UnOp::Neg, inner, _) => dyn_float_literal(inner),
        _ => false,
    }
}

pub(crate) fn dyn_member(name: &str) -> bool {
    matches!(name, "get" | "has" | "keys" | "remove" | "len" | "decode" | "to_text"
        | "field" | "at" | "int" | "float" | "text" | "bool" | "equal_unordered" | "encode")
}

pub(crate) fn dyn_call(receiver: crate::AST::Expr, method: &str, values: Vec<crate::AST::Expr>, ret: Type, span: crate::Diagnostics::Span) -> crate::AST::Expr {
    use crate::AST::{AccessConvention, CallArg, CallArgFlags, Expr};
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: method.to_string(),
        method_span: span,
        owner_type_args: Vec::new(),
        type_args: Vec::new(),
        args: values.into_iter().map(|expr| CallArg {
            span: expr.span(), expr, convention: AccessConvention::Read,
            flags: CallArgFlags::default(), label: None, spread: false,
        }).collect(),
        recv_type: Some("__DynOp__".to_string()),
        resolved_ret: Some(ret),
        operator_rhs: None,
        checked_widen: false,
        checked_callee: None,
    }
}

/// D-SERDE-ACCESS=B + D-SERDE14=A: accessor methods on `Dyn`. Every read
/// yields `T ![FieldError]` so automatic propagation composes with no mapping ceremony
/// inside a hand `decode`; `.field` and `.at` fill `FieldError.path` from the
/// segment they read. Scalar readers leave the path empty, so a containing
/// decoder frames their result with `FieldError.under`.
/// `.field(name)` → `Dyn ![FieldError]`
/// `.at(i)` → `Dyn ![FieldError]`
/// `.int()` → `Int ![FieldError]`
/// `.text()` → `String ![FieldError]`
/// `.bool()` → `Bool ![FieldError]`
/// `.float()` → `Float ![FieldError]`
/// `.to_text()` → `String?`
/// `.equal_unordered(other)` → `Bool`
pub fn dyn_method_return(method: &str, n_args: usize) -> Option<Type> {
    let decode_err = || Box::new(decode_error_ty());
    match (method, n_args) {
        ("get" | "remove", 1) => Some(Type::Option(Box::new(Type::Named("Dyn".to_string())))),
        ("has", 1) => Some(Type::Bool),
        ("keys", 0) => Some(Type::List(Box::new(Type::String))),
        ("len", 0) => Some(Type::Int),
        ("field", 1) => Some(Type::Result {
            ok: Box::new(Type::Named("Dyn".to_string())),
            err: decode_err(),
        }),
        ("at", 1) => Some(Type::Result {
            ok: Box::new(Type::Named("Dyn".to_string())),
            err: decode_err(),
        }),
        ("int", 0) => Some(Type::Result {
            ok: Box::new(Type::Int),
            err: decode_err(),
        }),
        ("text", 0) => Some(Type::Result {
            ok: Box::new(Type::String),
            err: decode_err(),
        }),
        ("bool", 0) => Some(Type::Result {
            ok: Box::new(Type::Bool),
            err: decode_err(),
        }),
        ("float", 0) => Some(Type::Result {
            ok: Box::new(Type::Float),
            err: decode_err(),
        }),
        ("to_text", 0) => Some(Type::Option(Box::new(Type::String))),
        ("equal_unordered", 1) => Some(Type::Bool),
        _ => None,
    }
}

/// D-DBDRIVER1: accessor methods on `DBValue` — read back the tagged value a
/// query bound or a row column carried. Mirrors `dyn_method_return`'s
/// shape exactly (`Result<T, String>`); `int` stays 64-bit (never `Float`),
/// while `blob` returns the exact `[U8]` payload.
pub fn db_value_method_return(method: &str, n_args: usize) -> Option<Type> {
    match (method, n_args) {
        ("int", 0) => Some(Type::Result {
            ok: Box::new(Type::Int),
            err: Box::new(Type::String),
        }),
        ("float", 0) => Some(Type::Result {
            ok: Box::new(Type::Float),
            err: Box::new(Type::String),
        }),
        ("text", 0) => Some(Type::Result {
            ok: Box::new(Type::String),
            err: Box::new(Type::String),
        }),
        ("bool", 0) => Some(Type::Result {
            ok: Box::new(Type::Bool),
            err: Box::new(Type::String),
        }),
        ("blob", 0) => Some(Type::Result {
            ok: Box::new(Type::List(Box::new(u8_ty()))),
            err: Box::new(Type::String),
        }),
        ("is_null", 0) => Some(Type::Bool),
        _ => None,
    }
}

/// D-METAREFLECT1 / D-ANY-JAI1: `reflect.of(x)`'s handle types. `Value` is the
/// whole-value handle; `.fields()` returns `[Field]` (only populated for a
/// struct receiver — every other displayable shape gets an empty list). A
/// field's value is another typed `Value`; its display text is only a view.
pub fn reflect_method_return(type_name: &str, method: &str, n_args: usize) -> Option<Type> {
    match (type_name, method, n_args) {
        ("Value", "type_name", 0) => Some(Type::String),
        ("Value", "path", 0) => Some(Type::String),
        ("Value", "display", 0) => Some(Type::String),
        ("Value", "fields", 0) => Some(Type::List(Box::new(Type::Named("Field".to_string())))),
        ("Field", "name", 0) => Some(Type::String),
        ("Field", "value", 0) => Some(Type::Named("Value".to_string())),
        ("TypeInfo" | "FieldInfo" | "MethodInfo", "has_marker", 1) => Some(Type::Bool),
        _ => None,
    }
}

pub fn is_reflect_type_name(name: &str) -> bool {
    matches!(
        name,
        "Value" | "Field" | "TypeInfo" | "FieldInfo" | "MethodInfo"
    )
}

#[cfg(test)]
mod data_nominal_tests {
    use super::data_nominal_leaf;

    #[test]
    fn accepts_core_data_owners_without_erasing_user_nominals() {
        assert_eq!(data_nominal_leaf("Query"), Some("Query"));
        assert_eq!(
            data_nominal_leaf("<corelib>/Core/Data::Core/Data/Arrow.jet::DataArrowBatch"),
            Some("DataArrowBatch")
        );
        assert_eq!(data_nominal_leaf("<corelib>/Core/Data::Query"), Some("Query"));
        assert_eq!(data_nominal_leaf("user::Query"), None);
        assert_eq!(data_nominal_leaf("user.Query"), None);
        assert_eq!(data_nominal_leaf("<corelib>/Core/database::Query"), None);
    }
}
