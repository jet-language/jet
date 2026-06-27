//! M5 built-in collection and string surface (List, Map, Char, String API).
//! M8 adds closure-powered methods (`map`, `filter`, …).
//! Sema calls into this module; codegen mirrors the same method names.

use crate::AST::Type;
use crate::Syntax;

/// Built-in type names that users cannot redefine (E0106).
pub const RESERVED_TYPES: &[&str] = &[
    Syntax::TYPE_LIST,
    Syntax::TYPE_MAP,
    Syntax::TYPE_CHAR,
    "Set",
    "Deque",
];

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

/// Whether `ty` may be used as a `Set` element — requires `Hash + Eq` (E0506).
pub fn is_hashable_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int
            | Type::Bool
            | Type::String
            | Type::Char
            | Type::Named(_)
            | Type::IntN { .. }
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
        // D-FAILCOMP1: failure-aware adapters
        | "filter_map"
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
        // D-DET1: deterministic injected Clock/Rng capability methods. Reading
        // time/randomness THROUGH the handle is reproducible (caller seeded it).
        Type::Named(n) if n == crate::Syntax::CLOCK_TYPE => clock_method_return(method, arg_count),
        Type::Named(n) if n == crate::Syntax::RNG_TYPE => rng_method_return(method, arg_count),
        // D-DET-CAPAPI: `Duration.millis()` reads the span as ms.
        Type::Named(n) if n == crate::Syntax::DURATION_TYPE => {
            duration_method_return(method, arg_count)
        }
        Type::Apply { name, args } if name == "Task" => task_method_return(args, method, arg_count),
        Type::Apply { name, args } if name == "Channel" => {
            channel_method_return(args, method, arg_count)
        }
        Type::Apply { name, args } if name == "Sender" => {
            sender_method_return(args, method, arg_count)
        }
        // D-REACT1=B: reactive handle methods. `Signal.get()/set(v)`, `Derived.get()`.
        Type::Apply { name, args } if name == crate::Syntax::TYPE_SIGNAL => {
            signal_method_return(args, method, arg_count)
        }
        Type::Apply { name, args } if name == crate::Syntax::TYPE_DERIVED => {
            derived_method_return(args, method, arg_count)
        }
        // D-COLLBREADTH1=A: Set<T> and Deque<T>.
        Type::Apply { name, args } if name == "Set" => {
            set_method_return(args.first().unwrap_or(&Type::Int), method, arg_count)
        }
        Type::Apply { name, args } if name == "Deque" => {
            deque_method_return(args.first().unwrap_or(&Type::Int), method, arg_count)
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
        // D-FAILCOMP1: filter_map(f: T -> V?E) → [V]; keeps ok, drops err; sema refines V.
        ("filter_map", 1) => Some(Some(Type::List(Box::new(Type::Int)))),
        // D-FAILCOMP1: try_collect on [Result<T,E>] → Result<[T],E>.
        ("try_collect", 0) => {
            match inner {
                Type::Result { ok, err } => Some(Some(Type::Result {
                    ok: Box::new(Type::List(ok.clone())),
                    err: err.clone(),
                })),
                _ => Some(Some(Type::Int)), // guard: only valid on [Result<T,E>]
            }
        }
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

/// D-DET1: methods on the deterministic injected `Clock` capability.
/// `clock.now()` reads the clock's current value (ms); `clock.tick(ms)` advances
/// it by a relative span and returns the new value. Reproducible — the clock
/// starts from the seed the caller supplied to `time.clock(seed)` and only moves
/// via explicit advances.
///
/// D-DET-CAPAPI widens the surface: `clock.advance(to_ms)` sets the clock to an
/// ABSOLUTE instant (returns the new value); `clock.wait(d: Duration)` advances
/// by a `Duration` (relative, returns the new value).
fn clock_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("now", 0) => Some(Some(Type::Int)),
        ("tick", 1) => Some(Some(Type::Int)),
        // D-DET-CAPAPI: absolute set + Duration-based advance.
        ("advance", 1) => Some(Some(Type::Int)),
        ("wait", 1) => Some(Some(Type::Int)),
        _ => None,
    }
}

/// D-DET1: methods on the deterministic injected `Rng` capability.
/// `rng.int(lo, hi)` draws an Int in `[lo, hi]`; `rng.float()` draws a Float in
/// `[0, 1)`. Reproducible — the stream is fixed by the seed passed to
/// `random.rng(seed)`, so the same seed yields the same draws on every machine.
///
/// D-DET-CAPAPI widens the surface to mirror the ambient `random.*` set:
/// `rng.bool()` draws a coin; `rng.pick(list)` returns a uniform `T?`;
/// `rng.shuffle(~list)` shuffles in place (Fisher–Yates). `pick`/`shuffle` are
/// generic, so their element-aware return types are resolved in the checker
/// dispatch (Source/Sema/CheckerInfer/calls.rs) — the placeholders here keep the
/// codegen totality path (lower.rs) honest.
fn rng_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("int", 2) => Some(Some(Type::Int)),
        ("float", 0) => Some(Some(Type::Float)),
        // D-DET-CAPAPI: coin draw.
        ("bool", 0) => Some(Some(Type::Bool)),
        // D-DET-CAPAPI: generic — element type refined in sema. `pick` → `T?`,
        // `shuffle` → void. Placeholders (Int) here, refined by the dispatch.
        ("pick", 1) => Some(Some(Type::Option(Box::new(Type::Int)))),
        ("shuffle", 1) => Some(None),
        _ => None,
    }
}

/// D-DET-CAPAPI: methods on the deterministic `Duration` value. `millis()` reads
/// the span back as an Int (the ms the `time.ms`/`time.secs` constructor minted).
fn duration_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("millis", 0) => Some(Some(Type::Int)),
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

/// D-REACT1=B: `Signal<T>` methods. `.get()` reads the current value (and, inside a
/// derived/effect body, subscribes); `.set(v)` writes a new value and notifies.
fn signal_method_return(args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    let t = args.first().cloned().unwrap_or(Type::Int);
    match (method, nargs) {
        ("get", 0) => Some(Some(t)),
        ("set", 1) => Some(None),
        _ => None,
    }
}

/// D-REACT1=B: `Derived<T>` methods. `.get()` reads the latest computed value,
/// recomputing it from its signals when one of them has changed.
fn derived_method_return(args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    let t = args.first().cloned().unwrap_or(Type::Int);
    match (method, nargs) {
        ("get", 0) => Some(Some(t)),
        _ => None,
    }
}

/// D-COLLBREADTH1=A: `Set<T>` methods (hash-backed unordered set).
fn set_method_return(elem: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    let set_of_elem = || Type::Apply {
        name: "Set".to_string(),
        args: vec![elem.clone()],
    };
    match (method, nargs) {
        ("len", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("add" | "remove" | "clear", _) => Some(None),
        ("contains", 1) => Some(Some(Type::Bool)),
        ("union", 1) => Some(Some(set_of_elem())),
        ("to_list", 0) => Some(Some(Type::List(Box::new(elem.clone())))),
        _ => None,
    }
}

/// D-COLLBREADTH1=A: `Deque<T>` methods (ring-buffer double-ended queue).
fn deque_method_return(elem: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("len", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("push_front" | "push_back" | "clear", _) => Some(None),
        ("pop_front" | "pop_back" | "peek_front" | "peek_back", 0) => {
            Some(Some(Type::Option(Box::new(elem.clone()))))
        }
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
        // D-COLLBREADTH1=A: Set mutating methods.
        Type::Apply { name, .. } if name == "Set" => {
            matches!(method, "add" | "remove" | "clear")
        }
        // D-COLLBREADTH1=A: Deque mutating methods.
        Type::Apply { name, .. } if name == "Deque" => {
            matches!(method, "push_front" | "push_back" | "pop_front" | "pop_back" | "clear")
        }
        // D-DET1/D-DET-CAPAPI: `clock.tick`/`advance`/`wait` move the clock; every
        // `rng` draw advances the PRNG stream — these need an edit-access (`~`)
        // receiver. `clock.now()` / `duration.millis()` are pure reads (no `~`).
        Type::Named(n) if n == crate::Syntax::CLOCK_TYPE => {
            matches!(method, "tick" | "advance" | "wait")
        }
        Type::Named(n) if n == crate::Syntax::RNG_TYPE => {
            matches!(method, "int" | "float" | "bool" | "pick" | "shuffle")
        }
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
                ret: None, // sema refines V from closure's actual return
                effect_bound: None,
            }]),
            "filter" | "find" | "any" | "all"
            // D-ITER1: closure bool predicates.
            | "take_while" | "skip_while" | "position" | "partition" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: Some(Box::new(Type::Bool)),
                effect_bound: None,
            }]),
            "each" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None,
                effect_bound: None,
            }]),
            // D-ITER1: key-extracting closure methods.
            "sort_by" | "min_by" | "max_by" | "group_by" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: Some(Box::new(Type::Int)), // sema refines key type
                effect_bound: None,
            }]),
            "reduce" | "fold" => Some(vec![
                Type::Int, // init — sema refines
                Type::Fn {
                    params: vec![Type::Int, (**inner).clone()],
                    ret: Some(Box::new(Type::Int)),
                    effect_bound: None,
                },
            ]),
            "scan" => Some(vec![
                Type::Int, // seed — sema refines
                Type::Fn {
                    params: vec![Type::Int, (**inner).clone()],
                    ret: Some(Box::new(Type::Int)), // sema refines
                    effect_bound: None,
                },
            ]),
            "flat_map" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: Some(Box::new(Type::List(Box::new(Type::Int)))), // sema refines
                effect_bound: None,
            }]),
            // D-FAILCOMP1: filter_map(f: T -> V?E) → [V]; keeps ok, drops err.
            // ret: None so any Result return is accepted; sema refines V via calls.rs.
            "filter_map" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None,
                effect_bound: None,
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
                effect_bound: None,
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
        // D-REACT1=B: `Signal.set(v)` expects a value of the signal's element type.
        Type::Apply { name, args } if name == crate::Syntax::TYPE_SIGNAL => match method {
            "set" => Some(vec![args.first().cloned().unwrap_or(Type::Int)]),
            _ => Some(vec![]),
        },
        Type::Apply { name, .. } if name == crate::Syntax::TYPE_DERIVED => Some(vec![]),
        // D-COLLBREADTH1=A: Set<T> arg types.
        Type::Apply { name, args } if name == "Set" => {
            let elem = args.first().cloned().unwrap_or(Type::Int);
            match method {
                "add" | "contains" | "remove" => Some(vec![elem]),
                "union" => Some(vec![Type::Apply {
                    name: "Set".to_string(),
                    args: vec![elem],
                }]),
                _ => Some(vec![]),
            }
        }
        // D-COLLBREADTH1=A: Deque<T> arg types.
        Type::Apply { name, args } if name == "Deque" => {
            let elem = args.first().cloned().unwrap_or(Type::Int);
            match method {
                "push_front" | "push_back" => Some(vec![elem]),
                _ => Some(vec![]),
            }
        }
        // D-DET1/D-DET-CAPAPI: injected capability method args. `pick`/`shuffle` are
        // generic ([T]) and handled element-aware in the checker dispatch — they are
        // NOT routed here.
        Type::Named(n) if n == crate::Syntax::CLOCK_TYPE => match method {
            "tick" | "advance" => Some(vec![Type::Int]),
            "wait" => Some(vec![Type::Named(crate::Syntax::DURATION_TYPE.to_string())]),
            _ => Some(vec![]),
        },
        Type::Named(n) if n == crate::Syntax::RNG_TYPE => match method {
            "int" => Some(vec![Type::Int, Type::Int]),
            _ => Some(vec![]),
        },
        Type::Named(n) if n == crate::Syntax::DURATION_TYPE => Some(vec![]),
        _ => None,
    }
}

/// Whether receiver convention for a built-in call must be `mut`.
pub fn builtin_needs_mut_receiver(recv_ty: &Type, method: &str) -> bool {
    builtin_method_mutates(recv_ty, method)
}
