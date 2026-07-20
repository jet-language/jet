//! M5 built-in collection and string surface (`[T]`, `[K: V]`, Char, String API).
//! M8 adds closure-powered methods (`map`, `filter`, …).
//! Sema calls into this module; codegen mirrors the same method names.

use crate::Syntax;
use crate::AST::Type;

/// Built-in type names that users cannot redefine (E0106).
pub const RESERVED_TYPES: &[&str] = &[
    Syntax::TYPE_HASH_MAP,
    Syntax::TYPE_BTREE_MAP,
    Syntax::TYPE_CHAR,
    Syntax::TYPE_BIT_SET,
    Syntax::TYPE_BYTE_BUFFER,
    Syntax::TYPE_SET,
    Syntax::TYPE_SORTED_SET,
    Syntax::TYPE_PRIORITY_QUEUE,
    Syntax::TYPE_LRU,
    "Bag",
    Syntax::TYPE_DEQUE,
    Syntax::TYPE_BIGINT,
    Syntax::TYPE_DECIMAL,
    Syntax::DURATION_TYPE,
    Syntax::DURATION_UNIT_TYPE,
    Syntax::DURATION_RANGE_ERROR_TYPE,
    // D-SOLVER-LIB1=A: `Solver` is the Core finite-solver handle. Reserving it
    // prevents a user type from being mistaken for the runtime solver handle.
    Syntax::SOLVER_TYPE,
    Syntax::TYPE_BUILD_CONTEXT,
    Syntax::TYPE_BUILD_PLAN,
    Syntax::TYPE_BUILD_ACTION,
    Syntax::TYPE_BUILD_TARGET,
    Syntax::TYPE_BUILD_TOOLCHAIN,
    Syntax::TYPE_BUILD_PROBE,
    Syntax::TYPE_PROGRAM_INFO,
    // D-DYNARRAY1: `View<T>` is deliberately NOT reserved here (unlike `Set`/
    // `Deque`) — `View` is already a widely-used user type name across the
    // jetpack UI component kit (examples/features/ui/*.jet, crates/jet-driver/
    // src/Jetpack/components/*.jet). `list.view(a..b)` always types as
    // `Type::Apply{"View", [T]}`; a user's own `enum View`/`struct View` types
    // as `Type::Named("View")` — a different `Type` variant, so the two never
    // collide in the type system or in codegen (a user `View` mangles to
    // `user_View`; `.view()`'s result has no named Rust type at all — see
    // `Context::rust_type`'s `View` arm, which maps straight to `&[T]`).
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
        Type::Int | Type::Bool | Type::String | Type::Char | Type::Named(_) | Type::IntN { .. }
    )
}

/// M8 + D-ITER1: built-in methods that take a closure argument.
pub fn is_closure_method(method: &str) -> bool {
    matches!(
        method,
        "map" | "filter" | "each" | "find" | "any" | "all" | "sort_by" | "reduce"
        // D-ITER1: lazy adapter set
        | "take_while" | "skip_while" | "flat_map" | "scan"
        | "position" | "min_by" | "max_by" | "fold" | "group_by" | "count_by"
        | "partition"
        // D-FAILCOMP1: failure-aware adapters
        | "filter_map"
        // D-AUTOPAR1=A: explicit parallel adapters
        | "par_map" | "par_filter" | "par_fold"
    )
}

/// `None` = not a built-in method; `Some(None)` = void; `Some(Some(t))` = returns `t`.
pub fn builtin_method_return(
    recv_ty: &Type,
    method: &str,
    arg_count: usize,
    is_static_on_type: bool,
) -> Option<Option<Type>> {
    if let Type::Tagged { inner, .. } = recv_ty {
        return builtin_method_return(inner, method, arg_count, is_static_on_type);
    }
    if is_static_on_type {
        return builtin_static_return(recv_ty, method, arg_count);
    }
    match recv_ty {
        Type::List(inner) => list_method_return(inner, method, arg_count),
        // S76: [T#N] delegates to list methods; length-changing ops are blocked in sema (E0964).
        Type::FixedList { elem, .. } => list_method_return(elem, method, arg_count),
        Type::Map { key, value, .. } => map_method_return(key, value, method, arg_count),
        Type::String => string_method_return(method, arg_count),
        Type::Named(n) if n == "Stopwatch" => stopwatch_method_return(method, arg_count),
        // D-DET1: deterministic injected Clock/Rng capability methods. Reading
        // time/randomness THROUGH the handle is reproducible (caller seeded it).
        Type::Named(n) if n == crate::Syntax::CLOCK_TYPE => clock_method_return(method, arg_count),
        Type::Named(n) if n == crate::Syntax::RNG_TYPE => rng_method_return(method, arg_count),
        // D-SOLVER-LIB1=A: explicit finite solver handle. `new(seed)` constructs
        // state; `require(Bool)` records a checked constraint; query methods read it.
        Type::Named(n) if n == crate::Syntax::SOLVER_TYPE => {
            solver_method_return(method, arg_count)
        }
        // D-SHAPE-DURATIONCONVERT1=A: checked whole-unit read.
        Type::Named(n) if n == crate::Syntax::DURATION_TYPE => {
            duration_method_return(method, arg_count)
        }
        // D-BIGINT1 / D-DECIMAL1: precise numeric methods.
        Type::Named(n) if n == crate::Syntax::TYPE_BIGINT => {
            crate::Numeric::bigint_method_return(method, arg_count)
        }
        Type::Named(n) if n == crate::Syntax::TYPE_DECIMAL => {
            crate::Numeric::decimal_method_return(method, arg_count)
        }
        Type::Named(n) if n == Syntax::TYPE_BUILD_CONTEXT => {
            build_context_method_return(method, arg_count)
        }
        Type::Named(n) if n == Syntax::TYPE_PROGRAM_INFO => match (method, arg_count) {
            ("types", 0) => Some(Some(Type::List(Box::new(Type::Named(
                Syntax::TYPE_TYPE_INFO.to_string(),
            ))))),
            ("functions", 0) => Some(Some(Type::List(Box::new(Type::Named(
                "FunctionInfo".to_string(),
            ))))),
            ("packages", 0) => Some(Some(Type::List(Box::new(Type::Named(
                "PackageInfo".to_string(),
            ))))),
            _ => None,
        },
        Type::Named(n) if n == Syntax::TYPE_TYPE_INFO => match (method, arg_count) {
            ("implements" | "has_method", 1) => Some(Some(Type::Bool)),
            _ => None,
        },
        Type::Named(n) if n == "FunctionInfo" => match (method, arg_count) {
            ("reaches_panic", 0) => Some(Some(Type::Bool)),
            _ => None,
        },
        Type::Named(n) if n == "EffectInfo" => match (method, arg_count) {
            ("has", 1) => Some(Some(Type::Bool)),
            _ => None,
        },
        Type::Apply { name, args } if name == "Task" => task_method_return(args, method, arg_count),
        Type::Apply { name, args } if name == "Receiver" => {
            receiver_method_return(args, method, arg_count)
        }
        Type::Apply { name, args } if name == "Sender" => {
            sender_method_return(args, method, arg_count)
        }
        // D-MEM1 S6 (D-POOLID-API1=A): `Pool<T>` generational-arena methods.
        // The precise return type is a placeholder here (this table only gates
        // whether the call reaches `finish_builtin_method`) — sema's
        // `finish_pool_add`/`finish_pool_remove`/the `("Pool","ids")` arm fully
        // recompute it from the receiver's real element type.
        Type::Apply { name, args } if name == "Pool" => pool_method_return(args, method, arg_count),
        // D-MEM1 S6 (D-SHARED-API1=A): `Shared<T>.read(f)`/`.edit(f)`. Same
        // placeholder-gate note as `Pool` above — `finish_shared_read`/
        // `finish_shared_edit` compute the real (closure-derived) return type.
        Type::Shared(inner) => shared_method_return(inner, method, arg_count),
        // D-REACT1=B: reactive handle methods. `Signal.get()/set(v)`, `Derived.get()`.
        Type::Apply { name, args } if name == crate::Syntax::TYPE_SIGNAL => {
            signal_method_return(args, method, arg_count)
        }
        Type::Apply { name, args } if name == crate::Syntax::TYPE_DERIVED => {
            derived_method_return(args, method, arg_count)
        }
        Type::Apply { name, args } if name == crate::Syntax::TYPE_COMPUTED => {
            derived_method_return(args, method, arg_count)
        }
        // D-EVENT1=D: compiler-known Event/Hook family; methods are ordinary
        // library calls with typed handlers/payloads.
        Type::Apply { name, args } if name == crate::Syntax::TYPE_EVENT => {
            event_method_return(args, method, arg_count)
        }
        Type::Apply { name, args } if name == crate::Syntax::TYPE_ASYNC_EVENT => {
            async_event_method_return(args, method, arg_count)
        }
        Type::Apply { name, args } if name == crate::Syntax::TYPE_DISPATCH_REPORT => {
            dispatch_report_method_return(args, method, arg_count)
        }
        Type::Apply { name, args } if name == crate::Syntax::TYPE_HOOK => {
            hook_method_return(args, method, arg_count)
        }
        Type::Apply { name, args } if name == crate::Syntax::TYPE_DECISION_HOOK => {
            decision_hook_method_return(args, method, arg_count)
        }
        Type::Named(n) if n == crate::Syntax::TYPE_SUBSCRIPTION => {
            subscription_method_return(method, arg_count)
        }
        Type::Named(n) if n == crate::Syntax::TYPE_EVENT_SCOPE => {
            event_scope_method_return(method, arg_count)
        }
        Type::Named(n) if n == crate::Syntax::TYPE_EVENT_TRACE => {
            event_trace_method_return(method, arg_count)
        }
        // D-WATCH-SCOPE1: unified file/process/port watcher handles.
        Type::Named(n) if n == crate::Syntax::TYPE_WATCH_HANDLE => {
            watch_handle_method_return(method, arg_count)
        }
        Type::Named(n) if n == crate::Syntax::TYPE_WATCH_SET => {
            watch_set_method_return(method, arg_count)
        }
        // D-COLLBREADTH1=A: Set<T> and Deque<T>.
        Type::Apply { name, args } if name == "Set" => {
            set_method_return(args.first().unwrap_or(&Type::Int), method, arg_count)
        }
        Type::Apply { name, args } if name == Syntax::TYPE_SORTED_SET => {
            sorted_set_method_return(args.first().unwrap_or(&Type::Int), method, arg_count)
        }
        Type::Apply { name, args } if name == Syntax::TYPE_PRIORITY_QUEUE => {
            priority_queue_method_return(args.first().unwrap_or(&Type::Int), method, arg_count)
        }
        Type::Apply { name, args } if name == Syntax::TYPE_LRU && args.len() >= 2 => {
            lru_method_return(&args[0], &args[1], method, arg_count)
        }
        // D-TAG1: Bag<T> counted multiset.
        Type::Apply { name, args } if name == "Bag" => {
            bag_method_return(args.first().unwrap_or(&Type::Int), method, arg_count)
        }
        Type::Apply { name, args } if name == "Deque" => {
            deque_method_return(args.first().unwrap_or(&Type::Int), method, arg_count)
        }
        Type::Named(n) if n == Syntax::TYPE_BIT_SET => bit_set_method_return(method, arg_count),
        Type::Named(n) if n == Syntax::TYPE_BYTE_BUFFER => {
            byte_buffer_method_return(method, arg_count)
        }
        Type::Named(n) if n == "SigningKey" && method == "public_key" && arg_count == 0 => Some(Some(Type::Named("VerifyKey".into()))),
        Type::Named(n) if n == "X25519SecretKey" && method == "public_key" && arg_count == 0 => Some(Some(Type::Named("X25519PublicKey".into()))),
        Type::Named(n) if matches!(n.as_str(), "VerifyKey" | "X25519PublicKey" | "Signature" | "Sealed" | "WrappedKey" | "Digest256" | "Digest512") && method == "bytes" && arg_count == 0 => Some(Some(Type::List(Box::new(Type::IntN { signed: false, bits: 8 })))),
        Type::Named(n) if matches!(n.as_str(), "Digest256" | "Digest512") && method == "hex" && arg_count == 0 => Some(Some(Type::String)),
        Type::Named(n) if n == "X25519PublicKey" && method == "text" && arg_count == 0 => Some(Some(Type::String)),
        Type::Named(n) if n == "PasswordHash" && method == "text" && arg_count == 0 => Some(Some(Type::String)),
        // D-DYNARRAY1: `View<T>` — read-only method surface on a zero-copy window.
        Type::Apply { name, args } if matches!(name.as_str(), "View" | "ViewMut") => {
            view_method_return(args.first().unwrap_or(&Type::Int), method, arg_count)
        }
        // D-HOLE1: `.map` on `T?` (no general "hole"/absent-propagating value type —
        // Option composition gets library combinators instead). `.zip` is handled
        // directly in the checker dispatch (its second operand's type is independent
        // of the receiver's, which doesn't fit this table's one-fixed-placeholder-type
        // shape), so it is NOT listed here.
        Type::Option(inner) => option_method_return(inner, method, arg_count),
        Type::Int | Type::Float | Type::Bool | Type::Char | Type::IntN { .. } | Type::Float32 => {
            numeric_method_return(recv_ty, method, arg_count)
        }
        _ => None,
    }
}

fn build_result(ok: &str) -> Option<Option<Type>> {
    Some(Some(Type::Result {
        ok: Box::new(Type::Named(ok.to_string())),
        err: Box::new(Type::Named(Syntax::TYPE_ERROR.to_string())),
    }))
}

fn build_context_method_return(method: &str, arg_count: usize) -> Option<Option<Type>> {
    match (method, arg_count) {
        ("generate", 2) => Some(Some(Type::Result {
            ok: Box::new(Type::Named(Syntax::TYPE_VOID.to_string())),
            err: Box::new(Type::Named(Syntax::TYPE_ERROR.to_string())),
        })),
        ("find", 1) => Some(Some(Type::List(Box::new(Type::String)))),
        ("embed", 1) => Some(Some(Type::String)),
        ("fetch", 2) => Some(Some(Type::Result {
            ok: Box::new(Type::String),
            err: Box::new(Type::Named(Syntax::TYPE_ERROR.to_string())),
        })),
        ("action", 5 | 7) => build_result(Syntax::TYPE_BUILD_ACTION),
        ("add_executable" | "add_library" | "add_test" | "add_bench" | "add_asset_bundle"
        | "add_doc" | "add_install" | "add_package" | "add_publish", 3) => {
            build_result(Syntax::TYPE_BUILD_TARGET)
        }
        ("toolchain", 2) => build_result(Syntax::TYPE_BUILD_TOOLCHAIN),
        ("probe", 3) => build_result(Syntax::TYPE_BUILD_PROBE),
        ("error", 5) => Some(None),
        ("plan", 0 | 1) => build_result(Syntax::TYPE_BUILD_PLAN),
        _ => None,
    }
}

/// D-SG9: the byte type `U8`.
fn u8t() -> Type {
    Type::IntN {
        signed: false,
        bits: 8,
    }
}

/// `(signed, bits)` if `ty` is an integer type (`Int` = signed 64-bit).
fn int_kind(ty: &Type) -> Option<(bool, u8)> {
    match ty {
        Type::Int => Some((true, 64)),
        Type::IntN { signed, bits } => Some((*signed, *bits)),
        _ => None,
    }
}

/// D-NUMOPS1: an integer width conversion is *widening* (infallible) when the
/// target range fully contains the source range; otherwise *narrowing*
/// (fallible — returns `T ? String`, with no silent truncation).
fn int_conv_widening(src: (bool, u8), dst: (bool, u8)) -> bool {
    let (slo, shi) = crate::AST::int_range(src.0, src.1);
    let (dlo, dhi) = crate::AST::int_range(dst.0, dst.1);
    dlo <= slo && shi <= dhi
}

/// D-SG9/D-NUMOPS1: numeric query methods and `to_string` for any numeric
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
        if matches!(ty, Type::Float) && method == "origin" {
            return Some(Some(Type::String));
        }
    }
    // D-NUMOPS1: integer bit-population queries (count -> Int).
    if int_kind(ty).is_some() && nargs == 0 {
        if let "count_ones" | "count_zeros" | "leading_zeros" | "trailing_zeros" = method {
            return Some(Some(Type::Int));
        }
    }
    None
}

/// D-SHAPE-CONVERT1=A: `Target.from_source(value)` numeric conversions.
pub fn numeric_conversion_return(target: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    if nargs != 1 || !target.is_numeric() {
        return None;
    }
    let source = crate::Syntax::numeric_conversion_source(method)
        .and_then(crate::AST::numeric_type_from_name)?;
    Some(Some(match (int_kind(&source), int_kind(target)) {
        (Some(src), Some(dst)) if !int_conv_widening(src, dst) => Type::Result {
            ok: Box::new(target.clone()),
            err: Box::new(Type::String),
        },
        (None, Some(_)) => Type::Result {
            ok: Box::new(target.clone()),
            err: Box::new(Type::String),
        },
        (None, None) if matches!(source, Type::Float) && matches!(target, Type::Float32) => {
            Type::Result {
                ok: Box::new(target.clone()),
                err: Box::new(Type::String),
            }
        }
        _ => target.clone(),
    }))
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
        (Type::Named(n), "new", 1) if n == crate::Syntax::SOLVER_TYPE => {
            Some(Some(Type::Named(crate::Syntax::SOLVER_TYPE.to_string())))
        }
        (Type::Named(n), method, 1)
            if n == crate::Syntax::DURATION_TYPE
                && crate::Syntax::DURATION_CONSTRUCTORS.contains(&method) =>
        {
            Some(Some(Type::Result {
                ok: Box::new(Type::Named(crate::Syntax::DURATION_TYPE.to_string())),
                err: Box::new(Type::Named(
                    crate::Syntax::DURATION_RANGE_ERROR_TYPE.to_string(),
                )),
            }))
        }
        (Type::Named(n), "from_text", 1) if n == "Secret" => Some(Some(Type::Named("Secret".into()))),
        (Type::Named(n), "from_bytes", 1) if n == "Secret" => Some(Some(Type::Named("Secret".into()))),
        (Type::Named(n), "generate", 0) if matches!(n.as_str(), "SigningKey" | "X25519SecretKey") => Some(Some(Type::Result { ok: Box::new(Type::Named(n.clone())), err: Box::new(Type::Named("CryptoError".into())) })),
        (Type::Named(n), "from_bytes", 1) if matches!(n.as_str(), "VerifyKey" | "X25519PublicKey" | "Signature" | "Sealed" | "WrappedKey") => Some(Some(Type::Result { ok: Box::new(Type::Named(n.clone())), err: Box::new(Type::Named("CryptoError".into())) })),
        (Type::Named(n), "from_text", 1) if n == "X25519PublicKey" => Some(Some(Type::Result { ok: Box::new(Type::Named(n.clone())), err: Box::new(Type::Named("CryptoError".into())) })),
        (Type::Named(n), "parse", 1) if n == "PasswordHash" => Some(Some(Type::Result { ok: Box::new(Type::Named("PasswordHash".into())), err: Box::new(Type::Named("CryptoError".into())) })),
        // D-SHAPE-CONVERT1=A: destination-owned numeric conversion.
        _ => numeric_conversion_return(ty, method, nargs),
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
        ("sum" | "product", 0) => Some(Some(inner.clone())),
        ("min" | "max", 0) => Some(Some(Type::Option(Box::new(inner.clone())))),
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
        ("chunks" | "windows", 1) => Some(Some(Type::List(Box::new(Type::List(Box::new(
            inner.clone(),
        )))))),
        ("flatten", 0) => match inner {
            Type::List(elem) => Some(Some(Type::List(elem.clone()))),
            _ => Some(Some(Type::List(Box::new(Type::Int)))),
        },
        ("intersperse", 1) => Some(Some(Type::List(Box::new(inner.clone())))),
        // D-ITER1: enumerate → [(idx: Int, item: T)].
        ("enumerate", 0) => Some(Some(Type::List(Box::new(enumerate_elem_ty(inner))))),
        // D-ITER1: zip([U]) → [(a: T, b: U)]; sema refines `b` from arg type.
        ("zip", 1) => {
            // placeholder element type (Int for `b`); sema will correct via resolved_ret.
            Some(Some(Type::List(Box::new(zip_elem_ty(inner, &Type::Int)))))
        }
        ("unzip", 0) => match inner {
            Type::Tuple(fields) => {
                let a = fields
                    .iter()
                    .find(|(name, _)| name == "a")
                    .map(|(_, ty)| (**ty).clone())
                    .unwrap_or(Type::Int);
                let b = fields
                    .iter()
                    .find(|(name, _)| name == "b")
                    .map(|(_, ty)| (**ty).clone())
                    .unwrap_or(Type::Int);
                Some(Some(Type::Tuple(vec![
                    ("a".to_string(), Box::new(Type::List(Box::new(a)))),
                    ("b".to_string(), Box::new(Type::List(Box::new(b)))),
                ])))
            }
            _ => Some(Some(Type::Tuple(vec![
                ("a".to_string(), Box::new(Type::List(Box::new(Type::Int)))),
                ("b".to_string(), Box::new(Type::List(Box::new(Type::Int)))),
            ]))),
        },
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
        // D-ITER1: group_by(f: T->K) -> [K: [T]]; sema refines K.
        ("group_by", 1) => Some(Some(Type::Map {
            key: Box::new(Type::String), // placeholder; sema refines
            key_span: None,
            value: Box::new(Type::List(Box::new(inner.clone()))),
        })),
        ("count_by", 1) => Some(Some(Type::Map {
            key: Box::new(Type::String), // placeholder; sema refines
            key_span: None,
            value: Box::new(Type::Int),
        })),
        // D-AUTOPAR1=A: parallel adapters. Return types mirror their sequential equivalents;
        // sema refines V/acc from closure body (ret: None open return).
        ("par_map", 1) => Some(Some(Type::List(Box::new(Type::Int)))), // sema refines V
        ("par_filter", 1) => Some(Some(Type::List(Box::new(inner.clone())))),
        ("par_fold", 2) => Some(Some(Type::Int)), // sema refines acc
        // D-DYNARRAY1: `list.view(a..b)` — zero-copy window constructor. Parsed
        // specially (the `..` between the two Int ends), so it always arrives
        // here as a 2-arg call; ownership tracking (E2305) happens at the
        // binding site (`CheckerCore::check_binding`), not here (I3: this
        // table is return-TYPE only).
        ("view", 2) => Some(Some(Type::Apply {
            name: "View".to_string(),
            args: vec![inner.clone()],
        })),
        _ => None,
    }
}

/// D-DYNARRAY1: `View<T>` — the read-only method surface a `.view(a..b)`
/// window supports (indexing is handled separately, in `infer_index`; a
/// `View` never mutates its owner — out of the ratified scope). Mirrors
/// `list_method_return`'s equivalent entries exactly (I8: one behavior for
/// the same method name, whether the receiver owns its storage or borrows
/// it) — `index_of` intentionally matches the list table's existing
/// `Option<T>` shape rather than `Option<Int>`, for the same reason.
fn view_method_return(elem: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("len", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("get" | "first" | "last" | "index_of", 0 | 1) => {
            Some(Some(Type::Option(Box::new(elem.clone()))))
        }
        ("contains", 1) => Some(Some(Type::Bool)),
        // D-ITER1-style closure surface, read-only subset only (D-DYNARRAY1 §2
        // scope: indexing, iteration, `.fold`, `.map`-to-owned).
        ("fold", 2) => Some(Some(Type::Int)), // placeholder; sema refines from init arg
        ("map", 1) => Some(Some(Type::List(Box::new(Type::Int)))), // placeholder; sema refines; owned [R]
        _ => None,
    }
}

/// D-HOLE1: `opt.map(f: T -> R) -> R?` — the return element type `R` is a
/// placeholder (refined from the closure's actual return type in
/// `finish_builtin_method`, the same "sema refines" convention `list_method_return`'s
/// `map` uses).
fn option_method_return(inner: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    let _ = inner;
    match (method, nargs) {
        ("map", 1) => Some(Some(Type::Option(Box::new(Type::Int)))),
        _ => None,
    }
}

fn map_method_return(key: &Type, value: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("len", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("clear", 0) => Some(None),
        ("add", 2) => Some(Some(Type::Option(Box::new(value.clone())))),
        ("add_new", 2) => Some(Some(Type::Bool)),
        ("get" | "remove", 1) => Some(Some(Type::Option(Box::new(value.clone())))),
        ("has_key", 1) => Some(Some(Type::Bool)),
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
        // D-STR-AFTER1: first-occurrence substring split; `sep` absent -> the
        // whole original string (mirrors `.replace`'s no-match-is-identity).
        ("after" | "before", 1) => Some(Some(Type::String)),
        ("split", 1) => Some(Some(Type::List(Box::new(Type::String)))),
        // c97/D-STRPARSE1: split text into its lines (mirrors `split`).
        ("lines", 0) => Some(Some(Type::List(Box::new(Type::String)))),
        ("chars", 0) => Some(Some(Type::List(Box::new(Type::Char)))),
        ("repeat", 1) => Some(Some(Type::String)),
        // c97/D-STRPARSE1: fallible integer parse. Same `Int ? ParseError` result
        // `Int.parse(s)` returns, so one error type covers text→int.
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
/// D-DET-CAPAPI/D-RANDOMDIST1 widens the surface to mirror the ambient
/// `random.*` set: distributions, bytes, sample/weighted choice, and in-place
/// shuffle. Generic element-aware return types are resolved in the checker
/// dispatch (Source/Sema/CheckerInfer/calls.rs) — placeholders here keep the
/// codegen totality path (lower.rs) honest.
fn rng_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("int", 2) => Some(Some(Type::Int)),
        ("float", 0) => Some(Some(Type::Float)),
        ("float_range", 2) => Some(Some(Type::Float)),
        // D-DET-CAPAPI: coin draw; D-RANDOMDIST1: probability draw.
        ("bool", 0) => Some(Some(Type::Bool)),
        ("bool", 1) => Some(Some(Type::Bool)),
        ("normal", 2) => Some(Some(Type::Float)),
        ("exponential", 1) => Some(Some(Type::Float)),
        ("bytes", 1) => Some(Some(Type::List(Box::new(Type::IntN {
            signed: false,
            bits: 8,
        })))),
        ("split", 0) => Some(Some(Type::Named(crate::Syntax::RNG_TYPE.to_string()))),
        // Generic — element type refined in sema. Placeholders (Int) here,
        // refined by the dispatch.
        ("pick", 1) => Some(Some(Type::Option(Box::new(Type::Int)))),
        ("weighted_pick", 2) => Some(Some(Type::Option(Box::new(Type::Int)))),
        ("sample", 2) => Some(Some(Type::List(Box::new(Type::Int)))),
        ("shuffle", 1) => Some(None),
        _ => None,
    }
}

/// D-SOLVER-LIB1=A: explicit finite solver state. This first Core slice admits
/// ordinary Bool constraints only; richer domains stay future library work.
fn solver_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("require", 1) => Some(None),
        ("failure_count", 0) => Some(Some(Type::Int)),
        ("status", 0) => Some(Some(Type::String)),
        _ => None,
    }
}

/// D-SHAPE-DURATIONCONVERT1=A: one checked whole-unit reader.
fn duration_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        (crate::Syntax::METHOD_DURATION_IN, 1) => Some(Some(Type::Result {
            ok: Box::new(Type::Int),
            err: Box::new(Type::Named(
                crate::Syntax::DURATION_RANGE_ERROR_TYPE.to_string(),
            )),
        })),
        _ => None,
    }
}

fn task_method_return(args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("join", 0) | (Syntax::METHOD_TASK_WAIT, 0) => Some(args.first().cloned()),
        // D-DETACH1: fire-and-forget — consumes the Task handle, returns unit.
        (Syntax::TASK_DETACH, 0) => Some(None),
        // D-COROUTINE1=A: task handle control-plane hooks over the internal coroutine substrate.
        (Syntax::METHOD_TASK_PAUSE, 0)
        | (Syntax::METHOD_TASK_RESUME, 0)
        | (Syntax::METHOD_TASK_CANCEL, 0) => Some(None),
        (Syntax::METHOD_TASK_TRACE, 0) => Some(Some(Type::String)),
        _ => None,
    }
}

/// D-TUPLE-DESTRUCT1: `Receiver<T>.receive()` — the receive half returned
/// alongside `Sender<T>` by `tasks.channel<T>()`. No `.sender()` method here
/// (there's no combined "Channel" handle to fetch a sender off of — a second
/// sender comes from `tx.clone()`, same as any other `Sender<T>`).
fn receiver_method_return(args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    let t = args.first().cloned().unwrap_or(Type::Int);
    match (method, nargs) {
        ("receive", 0) => Some(Some(Type::Result {
            ok: Box::new(t),
            err: Box::new(Type::Named("Closed".to_string())),
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

/// D-MEM1 S6 (D-POOLID-API1=A): `Pool<T>` placeholder gate (see call site note).
fn pool_method_return(args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    let t = args.first().cloned().unwrap_or(Type::Int);
    match (method, nargs) {
        ("add", 1) => Some(Some(Type::Apply {
            name: "Id".to_string(),
            args: vec![t],
        })),
        ("remove", 1) => Some(Some(Type::Option(Box::new(t)))),
        ("ids", 0) => Some(Some(Type::List(Box::new(Type::Apply {
            name: "Id".to_string(),
            args: vec![t],
        })))),
        _ => None,
    }
}

/// D-MEM1 S6 (D-SHARED-API1=A): `Shared<T>` placeholder gate (see call site note).
fn shared_method_return(inner: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("read", 1) | ("edit", 1) => Some(Some(inner.clone())),
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

fn event_method_return(args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    let payload = args.first().cloned().unwrap_or(Type::Int);
    match (method, nargs) {
        ("on" | "once", 2) | ("on_priority", 3) => Some(Some(Type::Named(
            crate::Syntax::TYPE_SUBSCRIPTION.to_string(),
        ))),
        ("emit", 1) => Some(Some(Type::Named(
            crate::Syntax::TYPE_EVENT_TRACE.to_string(),
        ))),
        ("trace", 0) => Some(Some(Type::String)),
        ("listener_count", 0) => Some(Some(Type::Int)),
        _ => {
            let _ = payload;
            None
        }
    }
}

fn async_event_method_return(args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    let error = args.get(1).cloned().unwrap_or(Type::String);
    match (method, nargs) {
        ("on" | "once", 2) | ("on_priority", 3) => Some(Some(Type::Named(
            crate::Syntax::TYPE_SUBSCRIPTION.to_string(),
        ))),
        ("emit_async", 1) => Some(Some(Type::Apply {
            name: "Task".to_string(),
            args: vec![Type::Apply {
                name: crate::Syntax::TYPE_DISPATCH_REPORT.to_string(),
                args: vec![error],
            }],
        })),
        ("close", 0) => Some(None),
        ("listener_count" | "queued_count" | "running_count" | "blocked_count", 0) => Some(Some(Type::Int)),
        _ => None,
    }
}

fn hook_method_return(args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    let _payload = args.first().cloned().unwrap_or(Type::Int);
    let result = args.get(1).cloned().unwrap_or(Type::Int);
    match (method, nargs) {
        ("on" | "once", 2) | ("on_priority", 3) => Some(Some(Type::Named(
            crate::Syntax::TYPE_SUBSCRIPTION.to_string(),
        ))),
        ("run", 2) => Some(Some(result)),
        ("trace", 0) => Some(Some(Type::String)),
        ("listener_count", 0) => Some(Some(Type::Int)),
        _ => None,
    }
}

fn decision_hook_method_return(args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    let payload = args.first().cloned().unwrap_or(Type::Int);
    let error = args.get(1).cloned().unwrap_or(Type::String);
    match (method, nargs) {
        ("on" | "once", 2) | ("on_priority", 3) => Some(Some(Type::Named(
            crate::Syntax::TYPE_SUBSCRIPTION.to_string(),
        ))),
        ("run", 1) => Some(Some(Type::Apply {
            name: crate::Syntax::TYPE_HOOK_OUTCOME.to_string(),
            args: vec![payload, error],
        })),
        ("listener_count", 0) => Some(Some(Type::Int)),
        _ => None,
    }
}

fn subscription_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("unsubscribe", 0) => Some(None),
        ("is_active", 0) => Some(Some(Type::Bool)),
        _ => None,
    }
}

fn event_scope_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("cancel", 0) => Some(None),
        ("active_count", 0) => Some(Some(Type::Int)),
        _ => None,
    }
}

fn event_trace_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("summary", 0) => Some(Some(Type::String)),
        ("delivered", 0) | ("queued", 0) | ("dropped", 0) => Some(Some(Type::Int)),
        _ => None,
    }
}

fn dispatch_report_method_return(args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    let error = args.first().cloned().unwrap_or(Type::String);
    match (method, nargs) {
        ("accepted", 0) => Some(Some(Type::Bool)),
        ("delivered_handlers", 0) => Some(Some(Type::Int)),
        ("state", 0) => Some(Some(Type::Named(crate::Syntax::TYPE_DISPATCH_STATE.to_string()))),
        ("failures", 0) => Some(Some(Type::List(Box::new(Type::Apply {
            name: crate::Syntax::TYPE_DISPATCH_FAILURE.to_string(),
            args: vec![error],
        })))),
        ("trace", 0) => Some(Some(Type::Named(crate::Syntax::TYPE_EVENT_TRACE.to_string()))),
        _ => None,
    }
}

fn watch_handle_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("poll", 0) | ("events", 0) => Some(Some(Type::List(Box::new(Type::Named(
            crate::Syntax::TYPE_WATCH_EVENT.to_string(),
        ))))),
        ("on" | "once", 2) => Some(Some(Type::Named(
            crate::Syntax::TYPE_SUBSCRIPTION.to_string(),
        ))),
        ("summary", 0) => Some(Some(Type::String)),
        ("is_active", 0) => Some(Some(Type::Bool)),
        ("cancel", 0) => Some(None),
        _ => None,
    }
}

fn watch_set_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("add", 1) => Some(None),
        ("poll", 0) | ("events", 0) => Some(Some(Type::List(Box::new(Type::Named(
            crate::Syntax::TYPE_WATCH_EVENT.to_string(),
        ))))),
        ("summary", 0) => Some(Some(Type::String)),
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
        ("add", 1) => Some(Some(Type::Bool)),
        ("remove" | "clear", _) => Some(None),
        ("has", 1) => Some(Some(Type::Bool)),
        ("union", 1) => Some(Some(set_of_elem())),
        ("to_list", 0) => Some(Some(Type::List(Box::new(elem.clone())))),
        _ => None,
    }
}

/// D-ITERTOOLS1=A: `SortedSet<T>` methods (BTreeSet-backed ordered set).
fn sorted_set_method_return(elem: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    let set_of_elem = || Type::Apply {
        name: Syntax::TYPE_SORTED_SET.to_string(),
        args: vec![elem.clone()],
    };
    match (method, nargs) {
        ("len", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("add", 1) => Some(Some(Type::Bool)),
        ("remove" | "clear", _) => Some(None),
        ("has", 1) => Some(Some(Type::Bool)),
        ("union", 1) => Some(Some(set_of_elem())),
        ("to_list", 0) => Some(Some(Type::List(Box::new(elem.clone())))),
        ("first" | "last", 0) => Some(Some(Type::Option(Box::new(elem.clone())))),
        _ => None,
    }
}

/// D-ITERTOOLS1=A: `PriorityQueue<T>` methods (BinaryHeap-backed max-heap).
fn priority_queue_method_return(elem: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("len", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("push" | "clear", _) => Some(None),
        ("pop" | "peek", 0) => Some(Some(Type::Option(Box::new(elem.clone())))),
        ("to_sorted_list", 0) => Some(Some(Type::List(Box::new(elem.clone())))),
        _ => None,
    }
}

/// D-ITERTOOLS1=A: `Lru<K,V>` bounded cache methods.
fn lru_method_return(key: &Type, value: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("len" | "capacity", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("clear", 0) => Some(None),
        ("add", 2) => Some(Some(Type::Option(Box::new(value.clone())))),
        ("add_new", 2) => Some(Some(Type::Bool)),
        ("get" | "remove", 1) => Some(Some(Type::Option(Box::new(value.clone())))),
        ("has_key", 1) => Some(Some(Type::Bool)),
        ("keys", 0) => Some(Some(Type::List(Box::new(key.clone())))),
        _ => None,
    }
}

/// D-ITERTOOLS1=A: `BitSet` methods.
fn bit_set_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("len" | "count", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("add", 1) => Some(Some(Type::Bool)),
        ("remove" | "clear", _) => Some(None),
        ("has", 1) => Some(Some(Type::Bool)),
        ("to_list", 0) => Some(Some(Type::List(Box::new(Type::Int)))),
        _ => None,
    }
}

/// D-ITERTOOLS1=A: `ByteBuffer` builder methods.
fn byte_buffer_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("len", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("clear", 0) => Some(None),
        ("to_bytes", 0) => Some(Some(Type::List(Box::new(u8t())))),
        (
            "write_u8" | "write_u16_le" | "write_u16_be" | "write_u32_le" | "write_u32_be"
            | "write_u64_le" | "write_u64_be" | "write_bytes",
            1,
        ) => Some(None),
        _ => None,
    }
}

/// D-TAG1: `Bag<T>` methods (counted multiset backed by `HashMap<T, usize>`).
fn bag_method_return(_elem: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("len", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("add", 1) => Some(Some(Type::Bool)),
        ("remove", 1) => Some(None),
        ("has", 1) => Some(Some(Type::Bool)),
        ("count", 1) => Some(Some(Type::Int)),
        ("any", 1) => Some(Some(Type::Bool)),
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
        Type::Map { .. } => matches!(method, "add" | "add_new" | "remove" | "clear"),
        // D-COLLBREADTH1=A: Set mutating methods.
        Type::Apply { name, .. } if name == "Set" => {
            matches!(method, "add" | "remove" | "clear")
        }
        Type::Apply { name, .. } if name == Syntax::TYPE_SORTED_SET => {
            matches!(method, "add" | "remove" | "clear")
        }
        Type::Apply { name, .. } if name == Syntax::TYPE_PRIORITY_QUEUE => {
            matches!(method, "push" | "pop" | "clear")
        }
        Type::Apply { name, .. } if name == Syntax::TYPE_LRU => {
            matches!(method, "add" | "add_new" | "get" | "remove" | "clear")
        }
        Type::Named(n) if n == Syntax::TYPE_BIT_SET => {
            matches!(method, "add" | "remove" | "clear")
        }
        Type::Named(n) if n == Syntax::TYPE_BYTE_BUFFER => matches!(
            method,
            "write_u8"
                | "write_u16_le"
                | "write_u16_be"
                | "write_u32_le"
                | "write_u32_be"
                | "write_u64_le"
                | "write_u64_be"
                | "write_bytes"
                | "clear"
        ),
        // D-TAG1: Bag mutating methods.
        Type::Apply { name, .. } if name == "Bag" => {
            matches!(method, "add" | "remove" | "clear")
        }
        // D-COLLBREADTH1=A: Deque mutating methods.
        Type::Apply { name, .. } if name == "Deque" => {
            matches!(
                method,
                "push_front" | "push_back" | "pop_front" | "pop_back" | "clear"
            )
        }
        // D-DET1/D-DET-CAPAPI: `clock.tick`/`advance`/`wait` move the clock; every
        // `rng` draw advances the PRNG stream — these need an edit-access (`&`)
        // receiver. `clock.now()` / `duration.in(unit)` are pure reads (no `&`).
        Type::Named(n) if n == crate::Syntax::CLOCK_TYPE => {
            matches!(method, "tick" | "advance" | "wait")
        }
        Type::Named(n) if n == crate::Syntax::RNG_TYPE => {
            matches!(
                method,
                "int"
                    | "float"
                    | "float_range"
                    | "bool"
                    | "normal"
                    | "exponential"
                    | "bytes"
                    | "split"
                    | "pick"
                    | "weighted_pick"
                    | "sample"
                    | "shuffle"
            )
        }
        Type::Named(n) if n == crate::Syntax::SOLVER_TYPE => matches!(method, "require"),
        _ => false,
    }
}

/// Expected argument types for built-in methods (excluding receiver).
pub fn builtin_method_arg_types(recv_ty: &Type, method: &str) -> Option<Vec<Type>> {
    if let Type::Tagged { inner, .. } = recv_ty {
        return builtin_method_arg_types(inner, method);
    }
    if recv_ty.is_numeric() {
        if let Some(source) = crate::Syntax::numeric_conversion_source(method)
            .and_then(crate::AST::numeric_type_from_name)
        {
            return Some(vec![source]);
        }
    }
    match recv_ty {
        Type::Named(n) if n == "Secret" && method == "from_text" => Some(vec![Type::String]),
        Type::Named(n) if matches!(n.as_str(), "Secret" | "VerifyKey" | "X25519PublicKey" | "Signature" | "Sealed" | "WrappedKey") && method == "from_bytes" => Some(vec![Type::List(Box::new(Type::IntN { signed: false, bits: 8 }))]),
        Type::Named(n) if n == "PasswordHash" && method == "parse" => Some(vec![Type::String]),
        Type::Named(n)
            if n == crate::Syntax::DURATION_TYPE
                && method == crate::Syntax::METHOD_DURATION_IN =>
        {
            Some(vec![Type::Named(
                crate::Syntax::DURATION_UNIT_TYPE.to_string(),
            )])
        }
        Type::Named(n) if n == "X25519PublicKey" && method == "from_text" => Some(vec![Type::String]),
        Type::Named(n) if matches!(n.as_str(), "SigningKey" | "X25519SecretKey") && method == "generate" => Some(vec![]),
        Type::Named(n) if matches!(n.as_str(), "SigningKey" | "X25519SecretKey" | "VerifyKey" | "X25519PublicKey" | "Signature" | "Sealed" | "WrappedKey" | "Digest256" | "Digest512" | "PasswordHash") => Some(vec![]),
        Type::Named(n) if n == Syntax::TYPE_BUILD_CONTEXT => match method {
            "generate" => Some(vec![Type::String, Type::String]),
            "find" | "embed" => Some(vec![Type::String]),
            "fetch" => Some(vec![Type::String, Type::String]),
            "action" => Some(vec![
                Type::String,
                Type::List(Box::new(Type::String)),
                Type::List(Box::new(Type::String)),
                Type::List(Box::new(Type::String)),
                Type::List(Box::new(Type::String)),
                Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()),
                Type::List(Box::new(Type::Named(Syntax::TYPE_BUILD_PROBE.to_string()))),
            ]),
            "add_executable" | "add_library" | "add_test" | "add_bench"
            | "add_asset_bundle" | "add_doc" | "add_install" | "add_package"
            | "add_publish" => Some(vec![
                Type::String,
                Type::List(Box::new(Type::String)),
                Type::List(Box::new(Type::Named(Syntax::TYPE_BUILD_ACTION.to_string()))),
            ]),
            "toolchain" => Some(vec![Type::String, Type::String]),
            "probe" => Some(vec![Type::String, Type::String, Type::String]),
            "error" => Some(vec![
                Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string()),
                Type::String,
                Type::String,
                Type::String,
                Type::String,
            ]),
            "plan" => Some(vec![Type::Named(Syntax::TYPE_BUILD_TARGET.to_string())]),
            _ => None,
        },
        Type::Named(n) if n == Syntax::TYPE_PROGRAM_INFO => Some(vec![]),
        Type::Named(n) if n == Syntax::TYPE_TYPE_INFO && matches!(method, "implements" | "has_method") => Some(vec![Type::String]),
        Type::Named(n) if n == "FunctionInfo" && method == "reaches_panic" => Some(vec![]),
        Type::Named(n) if n == "EffectInfo" && method == "has" => Some(vec![Type::String]),
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
            "sort_by" | "min_by" | "max_by" | "group_by" | "count_by" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None, // sema refines key type
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
                ret: None, // sema refines the returned list's element type
                effect_bound: None,
            }]),
            // D-FAILCOMP1: filter_map(f: T -> V?E) → [V]; keeps ok, drops err.
            // ret: None so any Result return is accepted; sema refines V via calls.rs.
            "filter_map" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None,
                effect_bound: None,
            }]),
            // D-AUTOPAR1=A: parallel adapters — same closure shapes as sequential equivalents.
            "par_map" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None, // sema refines V from closure body
                effect_bound: None,
            }]),
            "par_filter" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: Some(Box::new(Type::Bool)),
                effect_bound: None,
            }]),
            "par_fold" => Some(vec![
                Type::Int, // init — sema refines acc type
                Type::Fn {
                    params: vec![Type::Int, (**inner).clone()],
                    ret: Some(Box::new(Type::Int)), // sema refines
                    effect_bound: None,
                },
            ]),
            // D-ITER1: non-closure adapters.
            "take" | "skip" | "step_by" | "chunks" | "windows" => Some(vec![Type::Int]),
            "intersperse" => Some(vec![(**inner).clone()]),
            "zip" => Some(vec![]),
            "dedup" | "enumerate" | "sum" | "product" | "min" | "max" | "flatten" | "unzip" => {
                Some(vec![])
            }
            // D-DYNARRAY1: `.view(a..b)` — both range ends are Int (parsed
            // specially; always arrives as exactly 2 args).
            "view" => Some(vec![Type::Int, Type::Int]),
            _ => Some(vec![]),
        },
        Type::Map { key, value, .. } => match method {
            "add" | "add_new" => Some(vec![(**key).clone(), (**value).clone()]),
            "get" | "remove" | "has_key" => Some(vec![(**key).clone()]),
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
        // D-HOLE1: `opt.map(f: T -> R)`. `.zip` isn't listed — it's checked directly
        // (see `Collections::builtin_method_return`'s `Type::Option` arm comment).
        Type::Option(inner) => match method {
            "map" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None, // sema refines R from the closure's actual return
                effect_bound: None,
            }]),
            _ => Some(vec![]),
        },
        Type::Apply { name, args } if name == "Sender" => match method {
            "send" => Some(vec![args.first().cloned().unwrap_or(Type::Int)]),
            _ => Some(vec![]),
        },
        Type::Apply { name, .. } if name == "Task" || name == "Channel" => Some(vec![]),
        Type::Apply { name, args } if name == Syntax::TYPE_SORTED_SET => match method {
            "add" | "remove" | "has" => Some(vec![args.first().cloned().unwrap_or(Type::Int)]),
            "union" => Some(vec![Type::Apply {
                name: Syntax::TYPE_SORTED_SET.to_string(),
                args: vec![args.first().cloned().unwrap_or(Type::Int)],
            }]),
            _ => Some(vec![]),
        },
        Type::Apply { name, args } if name == Syntax::TYPE_PRIORITY_QUEUE => match method {
            "push" => Some(vec![args.first().cloned().unwrap_or(Type::Int)]),
            _ => Some(vec![]),
        },
        Type::Apply { name, args } if name == Syntax::TYPE_LRU && args.len() >= 2 => match method {
            "add" | "add_new" => Some(vec![args[0].clone(), args[1].clone()]),
            "get" | "remove" | "has_key" => Some(vec![args[0].clone()]),
            _ => Some(vec![]),
        },
        Type::Named(n) if n == Syntax::TYPE_BIT_SET => match method {
            "add" | "remove" | "has" => Some(vec![Type::Int]),
            _ => Some(vec![]),
        },
        Type::Named(n) if n == Syntax::TYPE_BYTE_BUFFER => match method {
            "write_bytes" => Some(vec![Type::List(Box::new(u8t()))]),
            "write_u8" => Some(vec![u8t()]),
            "write_u16_le" | "write_u16_be" => Some(vec![Type::IntN {
                signed: false,
                bits: 16,
            }]),
            "write_u32_le" | "write_u32_be" => Some(vec![Type::IntN {
                signed: false,
                bits: 32,
            }]),
            "write_u64_le" | "write_u64_be" => Some(vec![Type::IntN {
                signed: false,
                bits: 64,
            }]),
            _ => Some(vec![]),
        },
        // D-REACT1=B: `Signal.set(v)` expects a value of the signal's element type.
        Type::Apply { name, args } if name == crate::Syntax::TYPE_SIGNAL => match method {
            "set" => Some(vec![args.first().cloned().unwrap_or(Type::Int)]),
            _ => Some(vec![]),
        },
        Type::Apply { name, .. } if name == crate::Syntax::TYPE_DERIVED => Some(vec![]),
        Type::Apply { name, .. } if name == crate::Syntax::TYPE_COMPUTED => Some(vec![]),
        Type::Apply { name, args } if name == crate::Syntax::TYPE_EVENT => {
            let payload = args.first().cloned().unwrap_or(Type::Int);
            match method {
                "on" | "once" => Some(vec![
                    Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()),
                    Type::Fn {
                        params: vec![payload],
                        ret: None,
                        effect_bound: None,
                    },
                ]),
                "on_priority" => Some(vec![
                    Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()),
                    Type::Int,
                    Type::Fn {
                        params: vec![payload],
                        ret: None,
                        effect_bound: None,
                    },
                ]),
                "emit" => Some(vec![payload]),
                _ => Some(vec![]),
            }
        }
        Type::Apply { name, args } if name == crate::Syntax::TYPE_ASYNC_EVENT => {
            let payload = args.first().cloned().unwrap_or(Type::Int);
            let error = args.get(1).cloned().unwrap_or(Type::String);
            let handler_ret = Type::Result {
                ok: Box::new(Type::Named("Void".to_string())),
                err: Box::new(error),
            };
            match method {
                "on" | "once" => Some(vec![
                    Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()),
                    Type::Fn { params: vec![payload], ret: Some(Box::new(handler_ret)), effect_bound: None },
                ]),
                "on_priority" => Some(vec![
                    Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()),
                    Type::Int,
                    Type::Fn { params: vec![payload], ret: Some(Box::new(handler_ret)), effect_bound: None },
                ]),
                "emit_async" => Some(vec![payload]),
                _ => Some(vec![]),
            }
        }
        Type::Apply { name, args } if name == crate::Syntax::TYPE_HOOK => {
            let payload = args.first().cloned().unwrap_or(Type::Int);
            let result = args.get(1).cloned().unwrap_or(Type::Int);
            match method {
                "on" | "once" => Some(vec![
                    Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()),
                    Type::Fn {
                        params: vec![payload],
                        ret: Some(Box::new(result)),
                        effect_bound: None,
                    },
                ]),
                "on_priority" => Some(vec![
                    Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()),
                    Type::Int,
                    Type::Fn {
                        params: vec![payload],
                        ret: Some(Box::new(result)),
                        effect_bound: None,
                    },
                ]),
                "run" => Some(vec![payload, result]),
                _ => Some(vec![]),
            }
        }
        Type::Apply { name, args } if name == crate::Syntax::TYPE_DECISION_HOOK => {
            let payload = args.first().cloned().unwrap_or(Type::Int);
            let error = args.get(1).cloned().unwrap_or(Type::String);
            let decision = Type::Apply {
                name: crate::Syntax::TYPE_HOOK_DECISION.to_string(),
                args: vec![payload.clone(), error],
            };
            match method {
                "on" | "once" => Some(vec![
                    Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()),
                    Type::Fn {
                        params: vec![payload],
                        ret: Some(Box::new(decision)),
                        effect_bound: None,
                    },
                ]),
                "on_priority" => Some(vec![
                    Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()),
                    Type::Int,
                    Type::Fn {
                        params: vec![payload],
                        ret: Some(Box::new(decision)),
                        effect_bound: None,
                    },
                ]),
                "run" => Some(vec![payload]),
                _ => Some(vec![]),
            }
        }
        Type::Named(n)
            if n == crate::Syntax::TYPE_SUBSCRIPTION
                || n == crate::Syntax::TYPE_EVENT_SCOPE
                || n == crate::Syntax::TYPE_EVENT_TRACE =>
        {
            Some(vec![])
        }
        Type::Named(n) if n == crate::Syntax::TYPE_WATCH_HANDLE => match method {
            "on" | "once" => Some(vec![
                Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()),
                Type::Fn {
                    params: vec![Type::Named(crate::Syntax::TYPE_WATCH_EVENT.to_string())],
                    ret: None,
                    effect_bound: None,
                },
            ]),
            _ => Some(vec![]),
        },
        Type::Named(n) if n == crate::Syntax::TYPE_WATCH_SET => match method {
            "add" => Some(vec![Type::Named(
                crate::Syntax::TYPE_WATCH_HANDLE.to_string(),
            )]),
            _ => Some(vec![]),
        },
        // D-COLLBREADTH1=A: Set<T> arg types.
        Type::Apply { name, args } if name == "Set" => {
            let elem = args.first().cloned().unwrap_or(Type::Int);
            match method {
                "add" | "has" | "remove" => Some(vec![elem]),
                "union" => Some(vec![Type::Apply {
                    name: "Set".to_string(),
                    args: vec![elem],
                }]),
                _ => Some(vec![]),
            }
        }
        // D-TAG1: Bag<T> arg types.
        Type::Apply { name, args } if name == "Bag" => {
            let elem = args.first().cloned().unwrap_or(Type::Int);
            match method {
                "add" | "remove" | "has" | "count" => Some(vec![elem]),
                "any" => Some(vec![Type::Fn {
                    params: vec![elem],
                    ret: Some(Box::new(Type::Bool)),
                    effect_bound: None,
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
        // D-DYNARRAY1: `View<T>` arg types — read-only subset, mirrors `Type::List`.
        Type::Apply { name, args } if matches!(name.as_str(), "View" | "ViewMut") => {
            let elem = args.first().cloned().unwrap_or(Type::Int);
            match method {
                "get" | "contains" => Some(vec![elem]),
                "fold" => Some(vec![
                    Type::Int, // init — sema refines
                    Type::Fn {
                        params: vec![Type::Int, elem],
                        ret: Some(Box::new(Type::Int)),
                        effect_bound: None,
                    },
                ]),
                "map" => Some(vec![Type::Fn {
                    params: vec![elem],
                    ret: None, // sema refines R from the closure's actual return
                    effect_bound: None,
                }]),
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
            "float_range" => Some(vec![Type::Float, Type::Float]),
            "bool" => Some(vec![Type::Float]),
            "normal" => Some(vec![Type::Float, Type::Float]),
            "exponential" => Some(vec![Type::Float]),
            "bytes" => Some(vec![Type::Int]),
            _ => Some(vec![]),
        },
        Type::Named(n) if n == crate::Syntax::SOLVER_TYPE => match method {
            "new" => Some(vec![Type::Int]),
            "require" => Some(vec![Type::Bool]),
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
