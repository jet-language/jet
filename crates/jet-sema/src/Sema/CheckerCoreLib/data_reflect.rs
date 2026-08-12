use crate::AST::Type;
use super::core_types::decode_error_ty;

/// D-SERDE-ACCESS=B + D-SERDE14=A: accessor methods on `DataTree`. Every read
/// yields `T ? [FieldError]` so a `?` chain composes with no mapping ceremony
/// inside a hand `decode`; `.field` and `.at` fill `FieldError.path` from the
/// segment they read. Scalar readers leave the path empty, so a containing
/// decoder frames their result with `FieldError.under`.
/// `.field(name)` → `DataTree ? [FieldError]`
/// `.at(i)` → `DataTree ? [FieldError]`
/// `.int()` → `Int ? [FieldError]`
/// `.text()` → `String ? [FieldError]`
/// `.bool()` → `Bool ? [FieldError]`
/// `.float()` → `Float ? [FieldError]`
pub fn datatree_method_return(method: &str, n_args: usize) -> Option<Type> {
    let decode_err = || Box::new(decode_error_ty());
    match (method, n_args) {
        ("field", 1) => Some(Type::Result {
            ok: Box::new(Type::Named("DataTree".to_string())),
            err: decode_err(),
        }),
        ("at", 1) => Some(Type::Result {
            ok: Box::new(Type::Named("DataTree".to_string())),
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
        _ => None,
    }
}

/// D-DBDRIVER1: accessor methods on `DBValue` — read back the tagged value a
/// query bound or a row column carried. Mirrors `datatree_method_return`'s
/// shape exactly (`Result<T, String>`); `int` stays 64-bit (never `Float`).
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
        ("is_null", 0) => Some(Type::Bool),
        _ => None,
    }
}

/// D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)`'s handle types. `Value` is the
/// whole-value handle; `.fields()` returns `[Field]` (only populated for a
/// struct receiver — every other displayable shape gets an empty list,
/// resolved at codegen from `cx.struct_fields`, I3: sema only decides the
/// TYPES here, codegen does the per-call-site enumeration).
pub fn reflect_method_return(type_name: &str, method: &str, n_args: usize) -> Option<Type> {
    match (type_name, method, n_args) {
        ("Value", "type_name", 0) => Some(Type::String),
        ("Value", "path", 0) => Some(Type::String),
        ("Value", "display", 0) => Some(Type::String),
        ("Value", "fields", 0) => Some(Type::List(Box::new(Type::Named("Field".to_string())))),
        ("Field", "name", 0) => Some(Type::String),
        ("Field", "value", 0) => Some(Type::String),
        _ => None,
    }
}

pub fn is_reflect_type_name(name: &str) -> bool {
    matches!(name, "Value" | "Field")
}
