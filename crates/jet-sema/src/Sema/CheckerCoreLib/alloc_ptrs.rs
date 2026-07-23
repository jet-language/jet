use crate::AST::Type;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use super::core_types::unit_ty;

/// D-ALLOC1/D-ALLOC-C/D-ALLOC-D (ratified 2026-06-19): method calls on the four
/// allocator opaque types (Arena, Bump, Pool, Fixed).
/// Returns `Some(Some(T))` for a valid method with return type T, `Some(None)` for
/// a void method, `None` if the type_name is not an allocator type.
/// D-ALLOC1/D-ALLOC2: is `ty` one of the four allocator handle types?
pub(crate) fn is_allocator_type(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if matches!(n.as_str(), "Arena" | "Bump" | "Pool" | "Fixed"))
}

pub(crate) fn alloc_method_return(
    type_name: &str,
    method: &str,
    args: &[crate::AST::CallArg],
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    if !matches!(type_name, "Arena" | "Bump" | "Pool" | "Fixed") {
        return None;
    }
    let unit = unit_ty();
    match method {
        // D-ALLOC1: `new()` is a static constructor — handled via `infer_core_field`
        // returning a Named sentinel, then `.new()` dispatched here as an instance method
        // on the sentinel. Return the same Named type (the allocator handle).
        "new" => {
            // Optional capacity/slots/size arg.
            if args.len() > 1 {
                diags.push(Diagnostic::error(
                    "E0103",
                    format!("`{}.new` takes at most one optional argument", type_name),
                    "the only optional argument is `capacity:` / `slots:` / `size:`".to_string(),
                    format!(
                        "write `mem.{}.new()` or `mem.{}.new(capacity: N)`",
                        type_name, type_name
                    ),
                    Some(span),
                ));
            }
            Some(Some(Type::Named(type_name.to_string())))
        }
        "over" if type_name == "Fixed" => Some(Some(Type::Named(type_name.to_string()))),
        // D-ALLOC1: `alloc(value)` — allocates a value into the arena.
        // Returns the value's type; we infer it from the argument.
        "alloc" => {
            if args.len() != 1 {
                diags.push(Diagnostic::error(
                    "E0103",
                    format!("`{}.alloc` takes exactly one argument", type_name),
                    "pass the value you want to store in the allocator".to_string(),
                    format!("write `arena.alloc(my_value)`"),
                    Some(span),
                ));
                return Some(None);
            }
            // Return type is inferred from argument — caller handles inference.
            // We return a sentinel; actual type inference is done in the caller.
            Some(Some(Type::Named("__alloc_infer__".to_string())))
        }
        // D-ALLOC-D: `reset()` — keeps the backing buffer, marks all allocations invalid.
        "reset" => {
            if !args.is_empty() {
                diags.push(Diagnostic::error(
                    "E0103",
                    format!("`{}.reset` takes no arguments", type_name),
                    "reset clears all allocations, keeping the backing buffer".to_string(),
                    "write `arena.reset()`".to_string(),
                    Some(span),
                ));
            }
            Some(Some(unit))
        }
        _ => {
            let (why, fix) = if method == "free" {
                (
                    "allocator terminal release uses the same nominal `Close` protocol as every resource".to_string(),
                    "write `close(^allocator)`; use `allocator.reset()` only when reusing its backing storage".to_string(),
                )
            } else {
                let methods = if type_name == "Fixed" {
                    "`new`, `over`, `alloc`, `reset`"
                } else {
                    "`new`, `alloc`, `reset`"
                };
                (
                    format!("`{}` supports: {}", type_name, methods),
                    format!("check the method name — valid methods are {methods}"),
                )
            };
            diags.push(Diagnostic::error(
                "E0102",
                format!("`{}` has no method `{}`", type_name, method),
                why,
                fix,
                Some(span),
            ));
            None
        }
    }
}

pub(crate) fn io_error_ty() -> Type {
    Type::Named(Syntax::TYPE_IO_ERROR.to_string())
}

/// D-DBDRIVER1: the error type `.query`/`.query_one`/`.execute` fail with.
pub(crate) fn db_error_ty() -> Type {
    Type::Named("DbError".to_string())
}

/// D-DBDRIVER1: a `Row` is `Map<String, DbValue>` — the built-in `Map` already
/// gives `.get`/`.keys`/`.values`/`.contains_key`, so no separate nominal `Row`
/// type is registered (I8: reuse the existing collection instead of inventing one).
pub(crate) fn db_row_ty() -> Type {
    Type::Map {
        key: Box::new(Type::String),
        key_span: None,
        value: Box::new(Type::Named(Syntax::TYPE_DB_VALUE.to_string())),
    }
}

pub(crate) fn result_ty(ok: Type, err: Type) -> Type {
    Type::Result {
        ok: Box::new(ok),
        err: Box::new(err),
    }
}

/// S58 (E2-M13): `Ptr<T>`.
pub fn ptr_type(elem: Type) -> Type {
    Type::Apply {
        name: Syntax::TYPE_PTR.to_string(),
        args: vec![elem],
    }
}

/// S58 (E2-M13): the element type of a `Ptr<T>`, if `t` is one.
pub fn ptr_elem(t: &Type) -> Option<Type> {
    match t {
        Type::Apply { name, args } if name == Syntax::TYPE_PTR && args.len() == 1 => {
            Some(args[0].clone())
        }
        _ => None,
    }
}

/// E3101: a low-level memory operation used outside an `#Unsafe` block.
pub(crate) fn e3101(op: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3101",
        format!("`{}` can only run inside an `#Unsafe` block", op),
        "this operation can violate memory safety, so it must sit in an audited region".to_string(),
        format!(
            "wrap it: #{}(\"why this is safe\") {{ … }}",
            Syntax::KW_UNSAFE
        ),
        Some(span),
    )
}
