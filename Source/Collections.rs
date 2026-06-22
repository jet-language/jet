//! M5 built-in collection and string surface (List, Map, Char, String API).
//! M8 adds closure-powered methods (`map`, `filter`, …).
//! Sema calls into this module; codegen mirrors the same method names.

use crate::AST::Type;
use crate::Syntax;

/// Built-in type names that users cannot redefine (E0106).
pub const RESERVED_TYPES: &[&str] = &[Syntax::TYPE_LIST, Syntax::TYPE_MAP, Syntax::TYPE_CHAR];

pub fn is_reserved_type(name: &str) -> bool {
    RESERVED_TYPES.contains(&name)
}

/// Whether `ty` may be used as a `Map` key (E0502).
pub fn is_map_key_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int | Type::Bool | Type::String | Type::Char | Type::Named(_)
    )
}

/// M8: built-in methods that take a closure argument.
pub fn is_closure_method(method: &str) -> bool {
    matches!(
        method,
        "map" | "filter" | "each" | "find" | "any" | "all" | "sort_by" | "reduce"
    )
}

/// `None` = not a built-in method; `Some(None)` = void; `Some(Some(t))` = returns `t`.
pub fn builtin_method_return(
    recv_ty: &Type,
    method: &str,
    arg_count: usize,
    is_static_on_type: bool,
) -> Option<Option<Type>> {
    if is_static_on_type {
        return builtin_static_return(recv_ty, method, arg_count);
    }
    match recv_ty {
        Type::List(inner) => list_method_return(inner, method, arg_count),
        // S76: [T#N] delegates to list methods; length-changing ops are blocked in sema (E0964).
        Type::FixedList { elem, .. } => list_method_return(elem, method, arg_count),
        Type::Map { key, value } => map_method_return(key, value, method, arg_count),
        Type::String => string_method_return(method, arg_count),
        Type::Named(n) if n == "U8" => u8_method_return(method, arg_count),
        Type::Named(n) if n == "Stopwatch" => stopwatch_method_return(method, arg_count),
        Type::Apply { name, args } if name == "Task" => task_method_return(args, method, arg_count),
        Type::Apply { name, args } if name == "Channel" => {
            channel_method_return(args, method, arg_count)
        }
        Type::Apply { name, args } if name == "Sender" => {
            sender_method_return(args, method, arg_count)
        }
        Type::Int | Type::Float | Type::Bool | Type::Char => {
            builtin_static_return(recv_ty, method, arg_count)
        }
        _ => None,
    }
}

fn builtin_static_return(ty: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (ty, method, nargs) {
        (Type::Int, "parse", 1) => Some(Some(Type::Result {
            ok: Box::new(Type::Int),
            err: Box::new(Type::Named("ParseError".to_string())),
        })),
        (Type::Float, "parse", 1) => Some(Some(Type::Result {
            ok: Box::new(Type::Float),
            err: Box::new(Type::Named("ParseError".to_string())),
        })),
        (Type::Int, "to_string", 0) => Some(Some(Type::String)),
        (Type::Int, "to_float", 0) => Some(Some(Type::Float)),
        (Type::Int, "to_u8", 0) => Some(Some(Type::Result {
            ok: Box::new(Type::Named("U8".to_string())),
            err: Box::new(Type::String),
        })),
        (Type::String, "from_bytes", 1) => Some(Some(Type::Result {
            ok: Box::new(Type::String),
            err: Box::new(Type::Named(crate::Syntax::TYPE_UTF8_ERROR.to_string())),
        })),
        (Type::Float, "to_string", 0) => Some(Some(Type::String)),
        (Type::Float, "to_int", 0) => Some(Some(Type::Int)),
        (Type::Bool, "to_string", 0) => Some(Some(Type::String)),
        (Type::Char, "to_string", 0) => Some(Some(Type::String)),
        _ => None,
    }
}

fn list_method_return(inner: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("len" | "is_empty", 0) => Some(Some(Type::Int)),
        ("push" | "insert" | "remove" | "reverse" | "sort" | "clear", _) => Some(None),
        ("pop" | "get" | "first" | "last" | "index_of", 0 | 1) => {
            Some(Some(Type::Option(Box::new(inner.clone()))))
        }
        ("contains", 1) => Some(Some(Type::Bool)),
        ("join", 1) => Some(Some(Type::String)),
        // M8 closure methods — return type depends on callback; sema fills `map` element type.
        ("map", 1) => Some(Some(Type::List(Box::new(Type::Int)))), // placeholder; sema refines
        ("filter", 1) => Some(Some(Type::List(Box::new(inner.clone())))),
        ("each", 1) => Some(None),
        ("find", 1) => Some(Some(Type::Option(Box::new(inner.clone())))),
        ("any" | "all", 1) => Some(Some(Type::Bool)),
        ("sort_by", 1) => Some(None),
        ("reduce", 2) => Some(Some(Type::Int)), // placeholder; sema refines from init arg
        _ => None,
    }
}

fn map_method_return(key: &Type, value: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("len" | "is_empty", 0) => Some(Some(Type::Int)),
        ("insert" | "clear", _) => Some(None),
        ("get" | "remove", 1) => Some(Some(Type::Option(Box::new(value.clone())))),
        ("contains_key", 1) => Some(Some(Type::Bool)),
        ("keys", 0) => Some(Some(Type::List(Box::new(key.clone())))),
        ("values", 0) => Some(Some(Type::List(Box::new(value.clone())))),
        ("each", 1) => Some(None),
        _ => None,
    }
}

fn string_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("len" | "is_empty", 0) => Some(Some(Type::Int)),
        ("contains" | "starts_with" | "ends_with", 1) => Some(Some(Type::Bool)),
        ("trim" | "to_upper" | "to_lower" | "to_string", 0) => Some(Some(Type::String)),
        ("bytes", 0) => Some(Some(Type::List(Box::new(Type::Named("U8".to_string()))))),
        ("replace" | "slice", 2) => Some(Some(Type::String)),
        ("split", 1) => Some(Some(Type::List(Box::new(Type::String)))),
        ("chars", 0) => Some(Some(Type::List(Box::new(Type::Char)))),
        ("repeat", 1) => Some(Some(Type::String)),
        _ => None,
    }
}

fn u8_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("to_int", 0) => Some(Some(Type::Int)),
        _ => None,
    }
}

fn stopwatch_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("elapsed_millis", 0) => Some(Some(Type::Int)),
        _ => None,
    }
}

fn task_method_return(args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("join", 0) => Some(args.first().cloned()),
        // D-DETACH1: fire-and-forget — consumes the Task handle, returns unit.
        ("detach", 0) => Some(None),
        _ => None,
    }
}

fn channel_method_return(args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    let t = args.first().cloned().unwrap_or(Type::Int);
    match (method, nargs) {
        ("receive", 0) => Some(Some(Type::Result {
            ok: Box::new(t),
            err: Box::new(Type::Named("Closed".to_string())),
        })),
        ("sender", 0) => Some(Some(Type::Apply {
            name: "Sender".to_string(),
            args: vec![t],
        })),
        _ => None,
    }
}

fn sender_method_return(_args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("send", 1) => Some(None),
        _ => None,
    }
}

/// Whether a built-in method mutates its receiver (needs `var` binding).
pub fn builtin_method_mutates(recv_ty: &Type, method: &str) -> bool {
    match recv_ty {
        Type::List(_) => matches!(
            method,
            "push" | "pop" | "insert" | "remove" | "reverse" | "sort" | "sort_by" | "clear"
        ),
        Type::Map { .. } => matches!(method, "insert" | "remove" | "clear"),
        _ => false,
    }
}

/// Expected argument types for built-in methods (excluding receiver).
pub fn builtin_method_arg_types(recv_ty: &Type, method: &str) -> Option<Vec<Type>> {
    match recv_ty {
        Type::List(inner) => match method {
            "push" | "contains" => Some(vec![(**inner).clone()]),
            "insert" => Some(vec![Type::Int, (**inner).clone()]),
            "get" | "index_of" | "remove" => Some(vec![Type::Int]),
            "join" => Some(vec![Type::String]),
            "map" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: Some(Box::new(Type::Int)), // sema refines via expected_type
            }]),
            "filter" | "find" | "any" | "all" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: Some(Box::new(Type::Bool)),
            }]),
            "each" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None,
            }]),
            "sort_by" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: Some(Box::new(Type::Int)), // sema refines key type
            }]),
            "reduce" => Some(vec![
                Type::Int, // init — sema refines
                Type::Fn {
                    params: vec![Type::Int, (**inner).clone()],
                    ret: Some(Box::new(Type::Int)),
                },
            ]),
            _ => Some(vec![]),
        },
        Type::Map { key, value } => match method {
            "insert" => Some(vec![(**key).clone(), (**value).clone()]),
            "get" | "remove" | "contains_key" => Some(vec![(**key).clone()]),
            "each" => Some(vec![Type::Fn {
                params: vec![(**key).clone(), (**value).clone()],
                ret: None,
            }]),
            _ => Some(vec![]),
        },
        Type::String => match method {
            "contains" | "starts_with" | "ends_with" | "split" => Some(vec![Type::String]),
            "from_bytes" => Some(vec![Type::List(Box::new(Type::Named("U8".to_string())))]),
            "replace" => Some(vec![Type::String, Type::String]),
            "slice" => Some(vec![Type::Int, Type::Int]),
            "repeat" => Some(vec![Type::Int]),
            _ => Some(vec![]),
        },
        Type::Int | Type::Float => match method {
            "parse" => Some(vec![Type::String]),
            _ => None,
        },
        Type::Apply { name, args } if name == "Sender" => match method {
            "send" => Some(vec![args.first().cloned().unwrap_or(Type::Int)]),
            _ => Some(vec![]),
        },
        Type::Apply { name, .. } if name == "Task" || name == "Channel" => Some(vec![]),
        _ => None,
    }
}

/// Whether receiver convention for a built-in call must be `mut`.
pub fn builtin_needs_mut_receiver(recv_ty: &Type, method: &str) -> bool {
    builtin_method_mutates(recv_ty, method)
}
