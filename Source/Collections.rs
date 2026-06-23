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

/// M8 + D-ITER1: built-in methods that take a closure argument.
pub fn is_closure_method(method: &str) -> bool {
    matches!(
        method,
        "map" | "filter" | "each" | "find" | "any" | "all" | "sort_by" | "reduce"
        // D-ITER1: lazy adapter set
        | "take_while" | "skip_while" | "flat_map" | "scan"
        | "position" | "min_by" | "max_by" | "fold" | "group_by"
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
        Type::Named(n) if n == "Stopwatch" => stopwatch_method_return(method, arg_count),
        Type::Apply { name, args } if name == "Task" => task_method_return(args, method, arg_count),
        Type::Apply { name, args } if name == "Channel" => {
            channel_method_return(args, method, arg_count)
        }
        Type::Apply { name, args } if name == "Sender" => {
            sender_method_return(args, method, arg_count)
        }
        Type::Int | Type::Float | Type::Bool | Type::Char | Type::IntN { .. } | Type::Float32 => {
            builtin_static_return(recv_ty, method, arg_count)
        }
        _ => None,
    }
}

/// D-SG9: the byte type `U8`.
fn u8t() -> Type {
    Type::IntN { signed: false, bits: 8 }
}

/// `(signed, bits)` if `ty` is an integer type (`Int` = signed 64-bit).
fn int_kind(ty: &Type) -> Option<(bool, u8)> {
    match ty {
        Type::Int => Some((true, 64)),
        Type::IntN { signed, bits } => Some((*signed, *bits)),
        _ => None,
    }
}

/// The target type a `to_*` width-conversion method names (D-SG9/D-NUMOPS1).
fn conv_target(method: &str) -> Option<Type> {
    Some(match method {
        "to_i8" => Type::IntN { signed: true, bits: 8 },
        "to_i16" => Type::IntN { signed: true, bits: 16 },
        "to_i32" => Type::IntN { signed: true, bits: 32 },
        "to_i64" | "to_int" => Type::Int,
        "to_u8" => Type::IntN { signed: false, bits: 8 },
        "to_u16" => Type::IntN { signed: false, bits: 16 },
        "to_u32" => Type::IntN { signed: false, bits: 32 },
        "to_u64" => Type::IntN { signed: false, bits: 64 },
        "to_f32" => Type::Float32,
        "to_f64" | "to_float" => Type::Float,
        _ => return None,
    })
}

/// D-NUMOPS1: an integer width conversion is *widening* (infallible) when the
/// target range fully contains the source range; otherwise *narrowing*
/// (fallible — returns `T ? String`, with no silent truncation).
fn int_conv_widening(src: (bool, u8), dst: (bool, u8)) -> bool {
    let (slo, shi) = crate::AST::int_range(src.0, src.1);
    let (dlo, dhi) = crate::AST::int_range(dst.0, dst.1);
    dlo <= slo && shi <= dhi
}

/// D-SG9/D-NUMOPS1: width-conversion methods and `to_string` for any numeric
/// receiver (`Int`, `Float`, and the fixed widths). Returns `None` for names
/// this doesn't own so callers can keep trying other tables.
fn numeric_method_return(ty: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    if method == "to_string" && nargs == 0 {
        return Some(Some(Type::String));
    }
    // D-NUMOPS1: float predicates.
    if matches!(ty, Type::Float | Type::Float32) && nargs == 0 {
        if let "is_nan" | "is_infinite" | "is_finite" = method {
            return Some(Some(Type::Bool));
        }
    }
    // D-NUMOPS1: integer bit-population queries (count -> Int).
    if int_kind(ty).is_some() && nargs == 0 {
        if let "count_ones" | "count_zeros" | "leading_zeros" | "trailing_zeros" = method {
            return Some(Some(Type::Int));
        }
    }
    let target = conv_target(method)?;
    if nargs != 0 {
        return None;
    }
    // Integer source.
    if let Some(src) = int_kind(ty) {
        return Some(Some(match int_kind(&target) {
            // int → int: widening is infallible, narrowing is fallible.
            Some(dst) if int_conv_widening(src, dst) => target,
            Some(_) => Type::Result {
                ok: Box::new(target),
                err: Box::new(Type::String),
            },
            // int → float: always representable.
            None => target,
        }));
    }
    // Float source: float→float (explicit precision change) and float→int
    // (saturating truncation) are both infallible and explicit.
    if matches!(ty, Type::Float | Type::Float32) {
        return Some(Some(target));
    }
    None
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
        (Type::String, "from_bytes", 1) => Some(Some(Type::Result {
            ok: Box::new(Type::String),
            err: Box::new(Type::Named(crate::Syntax::TYPE_UTF8_ERROR.to_string())),
        })),
        (Type::Bool, "to_string", 0) => Some(Some(Type::String)),
        (Type::Char, "to_string", 0) => Some(Some(Type::String)),
        // D-SG9/D-NUMOPS1: width conversions + `to_string` for any numeric type.
        _ => numeric_method_return(ty, method, nargs),
    }
}

/// D-ITER1: the named-tuple element type for `enumerate` — `(idx: Int, item: T)`.
/// Fields are canonical (alpha-sorted by name): `idx` < `item`.
pub fn enumerate_elem_ty(inner: &Type) -> Type {
    Type::Tuple(vec![
        ("idx".to_string(), Box::new(Type::Int)),
        ("item".to_string(), Box::new(inner.clone())),
    ])
}

/// D-ITER1: the named-tuple element type for `zip` — `(a: T, b: U)`.
pub fn zip_elem_ty(a: &Type, b: &Type) -> Type {
    Type::Tuple(vec![
        ("a".to_string(), Box::new(a.clone())),
        ("b".to_string(), Box::new(b.clone())),
    ])
}

/// D-ITER1: the named-tuple return type for `partition` — `(false_: [T], true_: [T])`.
/// Fields are alpha-sorted: `false_` < `true_`.
pub fn partition_ret_ty(inner: &Type) -> Type {
    let list = Type::List(Box::new(inner.clone()));
    Type::Tuple(vec![
        ("false_".to_string(), Box::new(list.clone())),
        ("true_".to_string(), Box::new(list)),
    ])
}

fn list_method_return(inner: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("len", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
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
        // D-ITER1: non-closure lazy adapters (return [T]).
        ("take" | "skip" | "step_by", 1) => Some(Some(Type::List(Box::new(inner.clone())))),
        ("dedup", 0) => Some(Some(Type::List(Box::new(inner.clone())))),
        ("chunks" | "windows", 1) => {
            Some(Some(Type::List(Box::new(Type::List(Box::new(inner.clone()))))))
        }
        // D-ITER1: enumerate → [(idx: Int, item: T)].
        ("enumerate", 0) => Some(Some(Type::List(Box::new(enumerate_elem_ty(inner))))),
        // D-ITER1: zip([U]) → [(a: T, b: U)]; sema refines `b` from arg type.
        ("zip", 1) => {
            // placeholder element type (Int for `b`); sema will correct via resolved_ret.
            Some(Some(Type::List(Box::new(zip_elem_ty(inner, &Type::Int)))))
        }
        // D-ITER1: partition(f) → (false_: [T], true_: [T]).
        ("partition", 1) => Some(Some(partition_ret_ty(inner))),
        // D-ITER1: closure adapters returning [T].
        ("take_while" | "skip_while", 1) => Some(Some(Type::List(Box::new(inner.clone())))),
        // D-ITER1: flat_map(f: T->[U]) → [U]; placeholder; sema refines.
        ("flat_map", 1) => Some(Some(Type::List(Box::new(Type::Int)))),
        // D-ITER1: scan(seed, f: (acc,T)->acc) → [acc]; placeholder; sema refines.
        ("scan", 2) => Some(Some(Type::List(Box::new(Type::Int)))),
        // D-ITER1: position(f) → Int?
        ("position", 1) => Some(Some(Type::Option(Box::new(Type::Int)))),
        // D-ITER1: min_by/max_by(f: T->K) → T?
        ("min_by" | "max_by", 1) => Some(Some(Type::Option(Box::new(inner.clone())))),
        // D-ITER1: fold(init, f: (acc,T)->acc) → acc; placeholder; sema refines from init.
        ("fold", 2) => Some(Some(Type::Int)),
        // D-ITER1: group_by(f: T->K) → [K, [T]] (Map<K, List<T>>); sema refines K.
        ("group_by", 1) => Some(Some(Type::Map {
            key: Box::new(Type::String), // placeholder; sema refines
            value: Box::new(Type::List(Box::new(inner.clone()))),
        })),
        _ => None,
    }
}

fn map_method_return(key: &Type, value: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("len", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
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
        ("len", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("contains" | "starts_with" | "ends_with", 1) => Some(Some(Type::Bool)),
        ("trim" | "to_upper" | "to_lower" | "to_string", 0) => Some(Some(Type::String)),
        ("bytes", 0) => Some(Some(Type::List(Box::new(u8t())))),
        ("replace" | "slice", 2) => Some(Some(Type::String)),
        ("split", 1) => Some(Some(Type::List(Box::new(Type::String)))),
        // c97/D-STRPARSE1: split text into its lines (mirrors `split`).
        ("lines", 0) => Some(Some(Type::List(Box::new(Type::String)))),
        ("chars", 0) => Some(Some(Type::List(Box::new(Type::Char)))),
        ("repeat", 1) => Some(Some(Type::String)),
        // c97/D-STRPARSE1: fallible integer parse. Same `Int ? ParseError` result
        // `Int.parse(s)` returns, so one error type covers text→int.
        ("to_int", 0) => Some(Some(Type::Result {
            ok: Box::new(Type::Int),
            err: Box::new(Type::Named("ParseError".to_string())),
        })),
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
            "filter" | "find" | "any" | "all"
            // D-ITER1: closure bool predicates.
            | "take_while" | "skip_while" | "position" | "partition" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: Some(Box::new(Type::Bool)),
            }]),
            "each" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None,
            }]),
            // D-ITER1: key-extracting closure methods.
            "sort_by" | "min_by" | "max_by" | "group_by" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: Some(Box::new(Type::Int)), // sema refines key type
            }]),
            "reduce" | "fold" => Some(vec![
                Type::Int, // init — sema refines
                Type::Fn {
                    params: vec![Type::Int, (**inner).clone()],
                    ret: Some(Box::new(Type::Int)),
                },
            ]),
            "scan" => Some(vec![
                Type::Int, // seed — sema refines
                Type::Fn {
                    params: vec![Type::Int, (**inner).clone()],
                    ret: Some(Box::new(Type::Int)), // sema refines
                },
            ]),
            "flat_map" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: Some(Box::new(Type::List(Box::new(Type::Int)))), // sema refines
            }]),
            // D-ITER1: non-closure adapters.
            "take" | "skip" | "step_by" | "chunks" | "windows" => Some(vec![Type::Int]),
            "zip" => Some(vec![Type::List(Box::new((**inner).clone()))]),
            "dedup" | "enumerate" => Some(vec![]),
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
            "from_bytes" => Some(vec![Type::List(Box::new(u8t()))]),
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
