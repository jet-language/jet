//! M5 built-in collection and string surface (`[T]`, `[K: V]`, Char, String API).
//! M8 adds closure-powered methods (`map`, `filter`, …).
//! Sema calls into this module; codegen mirrors the same method names.

use crate::Syntax;
use crate::AST::Type;

/// Built-in type names that users cannot redefine (E0106).
pub const RESERVED_TYPES: &[&str] = &[
    Syntax::TYPE_HASH_MAP,
    Syntax::TYPE_BTREE_MAP,
    Syntax::TYPE_MAP,
    Syntax::TYPE_CHAR,
    Syntax::TYPE_BIT_SET,
    Syntax::TYPE_BYTE_BUFFER,
    Syntax::TYPE_SET,
    Syntax::TYPE_SORTED_SET,
    Syntax::TYPE_PRIORITY_QUEUE,
    Syntax::TYPE_LRU,
    Syntax::TYPE_ITER,
    Syntax::TYPE_REMOVE_BY,
    Syntax::TYPE_RANGE,
    Syntax::TYPE_SHARED_GUARD,
    Syntax::TYPE_SHARED_WEAK,
    Syntax::TYPE_CONDITION,
    "Bag",
    Syntax::TYPE_DEQUE,
    Syntax::TYPE_BIGINT,
    Syntax::TYPE_DECIMAL,
    Syntax::DURATION_TYPE,
    Syntax::DURATION_UNIT_TYPE,
    Syntax::DURATION_RANGE_ERROR_TYPE,
    Syntax::EXPIRING_VALUE_TYPE,
    // D-SOLVER-LIB1=A: `Solver` is the Core finite-solver handle. Reserving it
    // prevents a user type from being mistaken for the runtime solver handle.
    Syntax::SOLVER_TYPE,
    Syntax::TYPE_BUILD_CONTEXT,
    Syntax::TYPE_BUILD_PLAN,
    Syntax::TYPE_BUILD_ACTION,
    Syntax::TYPE_BUILD_TARGET,
    Syntax::TYPE_BUILD_TOOLCHAIN,
    Syntax::TYPE_BUILD_PROBE,
    "BuildSigningIdentity",
    Syntax::TYPE_PROGRAM_INFO,
    // D-LOCALCELL1=A: built-in local mutation and guard handles.
    "Cell",
    "CellReadGuard",
    "CellEditGuard",
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
        // #1479
        | "is_sorted_by" | "dedup_by" | "chunk_while"
        // D-PARCAPTURE1=D: explicit parallel adapters, consistently `para_`.
        | "para_map" | "para_filter" | "para_partition" | "para_fold"
        | "edit_disjoint"
    )
}

/// D-ITERTOOLS1=A: adapters that return a lazy `Iter<T>` view.
pub fn is_lazy_adapter(method: &str) -> bool {
    matches!(
        method,
        "map"
            | "filter"
            | "take"
            | "skip"
            | "step_by"
            | "dedup"
            | "dedup_by"
            | "chunks"
            | "windows"
            | "chunk_while"
            | "flatten"
            | "intersperse"
            | "indexed"
            | "indexes"
            | "zip"
            | "zip_short"
            | "zip_pad"
            | "take_while"
            | "skip_while"
            | "flat_map"
            | "filter_map"
            | "scan"
            | "cycle"
            | "repeat"
            | "drop_last"
            | "shuffle"
    )
}

/// D-ITERTOOLS1=A: terminals that consume an `Iter` (or list) and materialize
/// or reduce. `sort_by` stays in-place on lists only.
pub fn is_iter_terminal(method: &str) -> bool {
    matches!(
        method,
        "to_list"
            | "collect"
            | "each"
            | "find"
            | "any"
            | "all"
            | "reduce"
            | "fold"
            | "sum"
            | "product"
            | "min"
            | "max"
            | "min_by"
            | "max_by"
            | "position"
            | "group_by"
            | "count_by"
            | "partition"
            | "unzip"
            | "try_collect"
            | "join"
            | "is_sorted"
            | "is_sorted_by"
            | "last_index_of"
            | "average"
            | "to_set"
            | "compare"
            | "split"
    )
}

/// D-ITERTOOLS1=A: `Iter<T>` type constructor.
pub fn iter_ty(elem: Type) -> Type {
    Type::Apply {
        name: Syntax::TYPE_ITER.to_string(),
        args: vec![elem],
    }
}

pub fn is_iter_type(ty: &Type) -> bool {
    matches!(ty, Type::Apply { name, args } if name == Syntax::TYPE_ITER && args.len() == 1)
}

pub fn iter_elem(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Apply { name, args } if name == Syntax::TYPE_ITER && args.len() == 1 => {
            Some(&args[0])
        }
        _ => None,
    }
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
        // D-ITERTOOLS1=A: lazy views share the list adapter/reducer surface.
        Type::Apply { name, args } if name == Syntax::TYPE_ITER && args.len() == 1 => {
            iter_method_return(&args[0], method, arg_count)
        }
        Type::Map { key, value, .. } => map_method_return(key, value, method, arg_count),
        Type::String => string_method_return(method, arg_count),
        Type::Named(n) if n == "Stopwatch" => stopwatch_method_return(method, arg_count),
        Type::Named(n) if n == Syntax::TYPE_RANGE => match (method, arg_count) {
            ("contains", 1) => Some(Some(Type::Bool)),
            _ => None,
        },
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
        Type::Named(n) if n == crate::Syntax::TYPE_FRACTION => {
            crate::Numeric::fraction_method_return(method, arg_count)
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
        Type::Named(n) if n == "CompilerLexed" => match (method, arg_count) {
            ("source", 0) => Some(Some(Type::String)),
            ("tokens", 0) => Some(Some(Type::List(Box::new(Type::Named("CompilerToken".to_string()))))),
            ("diagnostics", 0) => Some(Some(Type::List(Box::new(Type::Named("CompilerDiagnostic".to_string()))))),
            _ => None,
        },
        Type::Named(n) if n == "CompilerSyntaxTree" => match (method, arg_count) {
            ("source", 0) => Some(Some(Type::String)),
            ("items", 0) => Some(Some(Type::List(Box::new(Type::Named("CompilerNode".to_string()))))),
            ("diagnostics", 0) => Some(Some(Type::List(Box::new(Type::Named("CompilerDiagnostic".to_string()))))),
            _ => None,
        },
        Type::Named(n) if n == "CompilerChecked" => match (method, arg_count) {
            ("source", 0) => Some(Some(Type::String)),
            ("syntax", 0) => Some(Some(Type::Named("CompilerSyntaxTree".to_string()))),
            ("functions", 0) => Some(Some(Type::List(Box::new(Type::Named("FunctionInfo".to_string()))))),
            ("effects", 0) => Some(Some(Type::List(Box::new(Type::Named("EffectInfo".to_string()))))),
            ("diagnostics", 0) => Some(Some(Type::List(Box::new(Type::Named("CompilerDiagnostic".to_string()))))),
            ("semantic_index", 0) => Some(Some(Type::Option(Box::new(Type::Named(
                "CompilerSemanticIndex".to_string(),
            ))))),
            _ => None,
        },
        Type::Named(n) if n == "CompilerSourceMap" => match (method, arg_count) {
            ("sources", 0) => Some(Some(Type::List(Box::new(Type::String)))),
            ("generated_lines", 0) => Some(Some(Type::List(Box::new(Type::Named("CompilerGeneratedLine".to_string()))))),
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
        // D-LOCALCELL1=A: one-thread cell and dynamic guard surface.
        Type::Apply { name, args } if name == "Cell" => {
            cell_method_return(args, method, arg_count)
        }
        Type::Apply { name, args } if name == "CellReadGuard" => {
            cell_guard_method_return(args, method, arg_count, false)
        }
        Type::Apply { name, args } if name == "CellEditGuard" => {
            cell_guard_method_return(args, method, arg_count, true)
        }
        // D-MEM1 S6 (D-SHARED-API1=A): `Shared<T>.read(f)`/`.edit(f)`. Same
        // placeholder-gate note as `Pool` above — `finish_shared_read`/
        // `finish_shared_edit` compute the real (closure-derived) return type.
        Type::Shared(inner) => shared_method_return(inner, method, arg_count),
        Type::Apply { name, args }
            if name == Syntax::TYPE_SHARED_WEAK && args.len() == 1 =>
        {
            shared_weak_method_return(&args[0], method, arg_count)
        }
        Type::Apply { name, args }
            if name == Syntax::TYPE_SHARED_GUARD && args.len() == 1 =>
        {
            shared_guard_method_return(&args[0], method, arg_count)
        }
        Type::Named(name) if name == Syntax::TYPE_CONDITION => {
            condition_method_return(method, arg_count)
        }
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
        Type::Named(name) if name == crate::Syntax::TYPE_EFFECT => match (method, arg_count) {
            ("unsubscribe", 0) => Some(None),
            ("is_active", 0) => Some(Some(Type::Bool)),
            _ => None,
        },
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
        Type::Named(n) if matches!(n.as_str(), "VerifyKey" | "X25519PublicKey" | "Signature" | "Sealed" | "WrappedKey" | "WrappedVaultKey" | "Digest256" | "Digest512") && method == "bytes" && arg_count == 0 => Some(Some(Type::List(Box::new(Type::IntN { signed: false, bits: 8 })))),
        Type::Named(n) if matches!(n.as_str(), "Digest256" | "Digest512") && method == "hex" && arg_count == 0 => Some(Some(Type::String)),
        Type::Named(n) if n == "X25519PublicKey" && method == "text" && arg_count == 0 => Some(Some(Type::String)),
        Type::Named(n) if n == "PasswordHash" && method == "text" && arg_count == 0 => Some(Some(Type::String)),
        // D-DYNARRAY1: `View<T>` — read-only method surface on a zero-copy window.
        Type::Apply { name, args } if matches!(name.as_str(), "View" | "ViewMut") => {
            view_method_return(args.first().unwrap_or(&Type::Int), method, arg_count)
        }
        // D-ITERTOOLS1=A: lazy `Iter<T>` — adapters chain; terminals materialize.
        Type::Apply { name, args } if name == Syntax::TYPE_ITER => {
            iter_method_return(args.first().unwrap_or(&Type::Int), method, arg_count)
        }
        // D-HOLE1: `.map` on `T?` (no general "hole"/absent-propagating value type —
        // Option composition gets library combinators instead). `.zip` is handled
        // directly in the checker dispatch (its second operand's type is independent
        // of the receiver's, which doesn't fit this table's one-fixed-placeholder-type
        // shape), so it is NOT listed here.
        Type::Option(inner) => option_method_return(inner, method, arg_count),
        Type::Result { ok, .. } => result_method_return(ok, method, arg_count),
        Type::Int | Type::Float | Type::Bool | Type::Char | Type::IntN { .. } | Type::Float32 => {
            numeric_method_return(recv_ty, method, arg_count)
        }
        _ => None,
    }
}

fn build_result(ok: &str) -> Option<Option<Type>> {
    Some(Some(Type::Result {
        ok: Box::new(Type::Named(ok.to_string())),
        err: Box::new(Type::Named(Syntax::TYPE_ERR.to_string())),
    }))
}

fn build_context_method_return(method: &str, arg_count: usize) -> Option<Option<Type>> {
    match (method, arg_count) {
        ("generate", 2) => Some(Some(Type::Result {
            ok: Box::new(Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string())),
            err: Box::new(Type::Named(Syntax::TYPE_ERR.to_string())),
        })),
        ("find", 1) => Some(Some(Type::List(Box::new(Type::String)))),
        ("embed", 1) => Some(Some(Type::String)),
        ("fetch", 2) => Some(Some(Type::Result {
            ok: Box::new(Type::String),
            err: Box::new(Type::Named(Syntax::TYPE_ERR.to_string())),
        })),
        ("plugin", 2) => Some(Some(Type::Result {
            ok: Box::new(Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string())),
            err: Box::new(Type::Named(Syntax::TYPE_ERR.to_string())),
        })),
        ("action", 5 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15) => build_result(Syntax::TYPE_BUILD_ACTION),
        ("legacy", 6..=17) => build_result(Syntax::TYPE_BUILD_ACTION),
        ("add_executable" | "add_library" | "add_test" | "add_bench" | "add_asset_bundle"
        | "add_doc" | "add_install" | "add_package" | "add_publish", 3 | 4 | 5 | 6 | 7) => {
            build_result(Syntax::TYPE_BUILD_TARGET)
        }
        ("toolchain", 2 | 3 | 4 | 5 | 6) => build_result(Syntax::TYPE_BUILD_TOOLCHAIN),
        ("signing", 2) => build_result("BuildSigningIdentity"),
        ("probe", 3..=6) => build_result(Syntax::TYPE_BUILD_PROBE),
        ("error", 5) => Some(None),
        ("plan", 0 | 1) => build_result(Syntax::TYPE_BUILD_PLAN),
        // D-BUILDCTX-FLAGS1=A
        ("default_profile", 1) => Some(None),
        ("default_allow", 1) => Some(None),
        _ => None,
    }
}

/// D-BUILDACTION1/D-BUILDTARGET1: build methods have one canonical base
/// shape and only add typed trailing controls. Keeping this table beside the
/// return table means the sema contract and the interpreter accept the same
/// arities; an unknown shape cannot silently become an untyped call.
pub fn build_context_method_arg_types(method: &str, arg_count: usize) -> Option<Vec<Type>> {
    let strings = || Type::List(Box::new(Type::String));
    let actions = || Type::List(Box::new(Type::Named(Syntax::TYPE_BUILD_ACTION.to_string())));
    let targets = || Type::List(Box::new(Type::Named(Syntax::TYPE_BUILD_TARGET.to_string())));
    let probes = || Type::List(Box::new(Type::Named(Syntax::TYPE_BUILD_PROBE.to_string())));
    match (method, arg_count) {
        ("generate", 2) => Some(vec![Type::String, Type::String]),
        ("plugin", 2) => Some(vec![Type::String, Type::String]),
        ("find" | "embed", 1) => Some(vec![Type::String]),
        ("fetch", 2) => Some(vec![Type::String, Type::String]),
        ("action", 5) => Some(vec![
            Type::String, strings(), strings(), strings(), strings(),
        ]),
        ("action", 7) => Some(vec![
            Type::String, strings(), strings(), strings(), strings(),
            Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()), probes(),
        ]),
        ("action", 8) => Some(vec![
            Type::String, strings(), strings(), strings(), strings(),
            Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()), probes(),
            Type::Named("BuildSigningIdentity".to_string()),
        ]),
        ("action", 9) => Some(vec![
            Type::String, strings(), strings(), strings(), strings(),
            Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()), probes(),
            Type::Named("BuildSigningIdentity".to_string()), Type::String,
        ]),
        ("action", 10) => Some(vec![
            Type::String, strings(), strings(), strings(), strings(),
            Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()), probes(),
            Type::Named("BuildSigningIdentity".to_string()), Type::String, strings(),
        ]),
        ("action", 11) => Some(vec![
            Type::String, strings(), strings(), strings(), strings(),
            Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()), probes(),
            Type::Named("BuildSigningIdentity".to_string()), Type::String, strings(), strings(),
        ]),
        ("action", 12) => Some(vec![
            Type::String, strings(), strings(), strings(), strings(),
            Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()), probes(),
            Type::Named("BuildSigningIdentity".to_string()), Type::String, strings(), strings(), strings(),
        ]),
        ("action", 13) => Some(vec![
            Type::String, strings(), strings(), strings(), strings(),
            Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()), probes(),
            Type::Named("BuildSigningIdentity".to_string()), Type::String, strings(), strings(), strings(), strings(),
        ]),
        ("action", 14) => Some(vec![
            Type::String, strings(), strings(), strings(), strings(),
            Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()), probes(),
            Type::Named("BuildSigningIdentity".to_string()), Type::String, strings(), strings(), strings(), strings(), strings(),
        ]),
        ("action", 15) => Some(vec![
            Type::String, strings(), strings(), strings(), strings(),
            Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()), probes(),
            Type::Named("BuildSigningIdentity".to_string()), Type::String, strings(), strings(), strings(), strings(), strings(), Type::String,
        ]),
        ("legacy", 6..=17) => {
            let mut args = vec![
                Type::String,
                Type::String,
                strings(),
                strings(),
                strings(),
                strings(),
            ];
            if arg_count >= 7 {
                args.push(Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()));
            }
            if arg_count >= 8 {
                args.push(probes());
            }
            if arg_count >= 9 {
                args.push(Type::Named("BuildSigningIdentity".to_string()));
            }
            if arg_count >= 10 {
                args.push(Type::String);
            }
            if arg_count >= 11 {
                args.push(strings());
            }
            if arg_count >= 12 {
                args.push(strings());
            }
            if arg_count >= 13 {
                args.push(strings());
            }
            if arg_count >= 14 {
                args.push(strings());
            }
            if arg_count >= 15 {
                args.push(strings());
            }
            if arg_count >= 16 {
                args.push(Type::String);
            }
            if arg_count >= 17 {
                args.push(Type::String);
            }
            Some(args)
        }
        ("add_executable" | "add_library" | "add_test" | "add_bench"
        | "add_asset_bundle" | "add_doc" | "add_install" | "add_package" | "add_publish", 3) => {
            Some(vec![Type::String, strings(), actions()])
        }
        ("add_executable" | "add_library" | "add_test" | "add_bench"
        | "add_asset_bundle" | "add_doc" | "add_install" | "add_package" | "add_publish", 4) => {
            Some(vec![Type::String, strings(), actions(), targets()])
        }
        ("add_executable" | "add_library" | "add_test" | "add_bench"
        | "add_asset_bundle" | "add_doc" | "add_install" | "add_package" | "add_publish", 5) => {
            Some(vec![Type::String, strings(), actions(), targets(), probes()])
        }
        ("add_executable" | "add_library" | "add_test" | "add_bench"
        | "add_asset_bundle" | "add_doc" | "add_install" | "add_package" | "add_publish", 6) => {
            Some(vec![
                Type::String, strings(), actions(), targets(), probes(),
                Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()),
            ])
        }
        ("add_executable" | "add_library" | "add_test" | "add_bench"
        | "add_asset_bundle" | "add_doc" | "add_install" | "add_package" | "add_publish", 7) => {
            Some(vec![
                Type::String, strings(), actions(), targets(), probes(),
                Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()),
                Type::Named("BuildSigningIdentity".to_string()),
            ])
        }
        ("toolchain", 2) => Some(vec![Type::String, Type::String]),
        ("toolchain", 3..=6) => {
            let mut args = vec![Type::String, Type::String];
            args.extend((0..(arg_count - 2)).map(|_| Type::String));
            Some(args)
        }
        ("signing", 2) => Some(vec![Type::String, Type::String]),
        ("probe", 3) => Some(vec![Type::String, Type::String, Type::String]),
        ("probe", 4) => Some(vec![
            Type::String,
            Type::String,
            Type::String,
            Type::Union(vec![
                Type::String,
                Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()),
            ]),
        ]),
        ("probe", 5) => Some(vec![
            Type::String,
            Type::String,
            Type::String,
            Type::Union(vec![
                Type::String,
                Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()),
            ]),
            Type::Union(vec![
                Type::String,
                Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()),
            ]),
        ]),
        ("probe", 6) => Some(vec![
            Type::String,
            Type::String,
            Type::String,
            Type::String,
            Type::String,
            Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()),
        ]),
        ("error", 5) => Some(vec![
            Type::Named(Syntax::TYPE_SOURCE_SPAN.to_string()), Type::String,
            Type::String, Type::String, Type::String,
        ]),
        ("plan", 0) => Some(Vec::new()),
        ("plan", 1) => Some(vec![Type::Named(Syntax::TYPE_BUILD_TARGET.to_string())]),
        ("default_profile", 1) => Some(vec![Type::String]),
        ("default_allow", 1) => Some(vec![strings()]),
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
        (Type::Named(n), "new", 1) if n == crate::Syntax::SOLVER_TYPE => {
            Some(Some(Type::Named(crate::Syntax::SOLVER_TYPE.to_string())))
        }
        (Type::Named(n), "new", 0) if n == crate::Syntax::TYPE_CONDITION => {
            Some(Some(Type::Named(crate::Syntax::TYPE_CONDITION.to_string())))
        }
        (Type::Named(n), "new", 1) if n == crate::Syntax::CLOCK_TYPE => {
            Some(Some(Type::Named(crate::Syntax::CLOCK_TYPE.to_string())))
        }
        // D-SHAPE-CTORVERB1=C: sema replaces Unknown with argument 1's type.
        (Type::Named(n), "new", 3) if n == crate::Syntax::EXPIRING_VALUE_TYPE => {
            Some(Some(Type::Apply {
                name: crate::Syntax::EXPIRING_VALUE_TYPE.to_string(),
                args: vec![Type::Named("Unknown".to_string())],
            }))
        }
        (Type::Named(n), "system", 0) if n == crate::Syntax::CLOCK_TYPE => {
            Some(Some(Type::Named(crate::Syntax::CLOCK_TYPE.to_string())))
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
        (Type::Named(n), "new_random", 0) if matches!(n.as_str(), "SigningKey" | "X25519SecretKey") => Some(Some(Type::Result { ok: Box::new(Type::Named(n.clone())), err: Box::new(Type::Named("CryptoError".into())) })),
        (Type::Named(n), "from_bytes", 1) if matches!(n.as_str(), "VerifyKey" | "X25519PublicKey" | "Signature" | "Sealed" | "WrappedKey") => Some(Some(Type::Result { ok: Box::new(Type::Named(n.clone())), err: Box::new(Type::Named("CryptoError".into())) })),
        (Type::Named(n), "from_bytes", 1) if n == "WrappedVaultKey" => Some(Some(Type::Result { ok: Box::new(Type::Named(n.clone())), err: Box::new(Type::Named("KeyWrapError".into())) })),
        (Type::Named(n), "Recipient", 1) if n == "KeyUnlock" => Some(Some(Type::Named("KeyUnlock".into()))),
        (Type::Named(n), "Passphrase", 1) if n == "KeyUnlock" => Some(Some(Type::Named("KeyUnlock".into()))),
        (Type::Named(n), "from_text", 1) if n == "X25519PublicKey" => Some(Some(Type::Result { ok: Box::new(Type::Named(n.clone())), err: Box::new(Type::Named("CryptoError".into())) })),
        (Type::Named(n), "parse", 1) if n == "PasswordHash" => Some(Some(Type::Result { ok: Box::new(Type::Named("PasswordHash".into())), err: Box::new(Type::Named("CryptoError".into())) })),
        (Type::Named(n), "new", 0) if n == crate::Syntax::TYPE_BYTE_BUFFER => {
            Some(Some(Type::Named(crate::Syntax::TYPE_BYTE_BUFFER.to_string())))
        }
        (Type::Named(n), "with_capacity", 1) if n == crate::Syntax::TYPE_BYTE_BUFFER => {
            Some(Some(Type::Named(crate::Syntax::TYPE_BYTE_BUFFER.to_string())))
        }
        (Type::Named(n), "from", 1) if n == crate::Syntax::TYPE_BYTE_BUFFER => {
            Some(Some(Type::Named(crate::Syntax::TYPE_BYTE_BUFFER.to_string())))
        }
        // D-COLLBREADTH1=A: Deque static constructors (ledger + static return table).
        (Type::Named(n), "new", 0) if n == "Deque" => Some(Some(Type::Apply {
            name: "Deque".to_string(),
            args: vec![Type::Int],
        })),
        (Type::Named(n), "init", 1) if n == "Deque" => Some(Some(Type::Apply {
            name: "Deque".to_string(),
            args: vec![Type::Int],
        })),
        // #1478: Set/SortedSet empty constructors (ledger + static return table).
        (Type::Named(n), "new", 0) if n == "Set" => Some(Some(Type::Apply {
            name: "Set".to_string(),
            args: vec![Type::Int],
        })),
        (Type::Named(n), "new", 0) if n == Syntax::TYPE_SORTED_SET => Some(Some(Type::Apply {
            name: Syntax::TYPE_SORTED_SET.to_string(),
            args: vec![Type::Int],
        })),
        // #1477: Map constructors.
        (Type::Named(n), "new", 0) if n == Syntax::TYPE_MAP => Some(Some(Type::Map {
            key: Box::new(Type::Int),
            key_span: None,
            value: Box::new(Type::Int),
        })),
        (Type::Named(n), "from_keys", 2) if n == Syntax::TYPE_MAP => Some(Some(Type::Map {
            key: Box::new(Type::Int),
            key_span: None,
            value: Box::new(Type::Int),
        })),
        // D-SHAPE-CONVERT1=A: destination-owned numeric conversion.
        _ => numeric_conversion_return(ty, method, nargs),
    }
}

/// D-ITER1 / D-RANGE-EXCL1=C: named-tuple element type for `indexed` —
/// `(idx: Int, item: T)`. Fields are canonical (alpha-sorted by name): `idx` < `item`.
pub fn indexed_elem_ty(inner: &Type) -> Type {
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
    let mutable_view = || Type::Apply {
        name: "ViewMut".to_string(),
        args: vec![inner.clone()],
    };
    match (method, nargs) {
        ("len", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("push" | "insert" | "reverse" | "sort" | "clear", _) => Some(None),
        ("remove", 1 | 2) => Some(Some(Type::Option(Box::new(inner.clone())))),
        ("count", 1) => Some(Some(Type::Int)),
        ("extend", 1) => Some(None),
        ("concat", 1) => Some(Some(Type::List(Box::new(inner.clone())))),
        ("pop" | "get" | "first" | "last" | "index_of", 0 | 1) => {
            Some(Some(Type::Option(Box::new(inner.clone()))))
        }
        ("contains", 1) => Some(Some(Type::Bool)),
        ("join", 1) => Some(Some(Type::String)),
        ("sum" | "product", 0) => Some(Some(inner.clone())),
        ("min" | "max", 0) => Some(Some(Type::Option(Box::new(inner.clone())))),
        // D-LOOPMAP1=B: in-memory List adapters return collections; `.lazy()` opts into Iter.
        ("map", 1) => Some(Some(Type::List(Box::new(Type::Int)))), // placeholder; sema refines
        ("filter", 1) => Some(Some(Type::List(Box::new(inner.clone())))),
        ("lazy", 0) => Some(Some(iter_ty(inner.clone()))),
        ("each", 1) => Some(None),
        ("find", 1) => Some(Some(Type::Option(Box::new(inner.clone())))),
        ("any" | "all", 1) => Some(Some(Type::Bool)),
        ("sort_by", 1) => Some(None),
        ("reduce", 2) => Some(Some(Type::Int)), // placeholder; sema refines from init arg
        // D-ITERTOOLS1=A: non-closure lazy adapters return `Iter<T>`.
        ("take" | "skip" | "step_by", 1) => Some(Some(iter_ty(inner.clone()))),
        ("dedup", 0) => Some(Some(iter_ty(inner.clone()))),
        ("chunks" | "windows", 1) => Some(Some(iter_ty(Type::List(Box::new(inner.clone()))))),
        // #1479: List shares Iter surface (I8).
        ("repeat", 1) => Some(Some(iter_ty(inner.clone()))),
        ("compare", 1) => Some(Some(Type::Int)),
        ("is_sorted", 0) => Some(Some(Type::Bool)),
        ("is_sorted_by", 1) => Some(Some(Type::Bool)),
        ("dedup_by", 1) => Some(Some(iter_ty(inner.clone()))),
        // D-ITER1: bounded — cycles the sequence, producing exactly `n`
        // items (not `n` loops; `repeat(n)` already covers "loop n times").
        // A 0-arg infinite `cycle()` has no safe representation across
        // AOT/JIT/interpreter without unbounded materialization risk (I9).
        ("cycle", 1) => Some(Some(iter_ty(inner.clone()))),
        ("drop_last", 1) => Some(Some(iter_ty(inner.clone()))),
        ("last_index_of", 1) => Some(Some(Type::Option(Box::new(Type::Int)))),
        ("average", 0) => Some(Some(Type::Float)),
        ("to_set", 0) => Some(Some(Type::Apply {
            name: Syntax::TYPE_SET.to_string(),
            args: vec![inner.clone()],
        })),
        ("split", 1) => Some(Some(Type::Tuple(vec![
            ("left".to_string(), Box::new(Type::List(Box::new(inner.clone())))),
            ("right".to_string(), Box::new(Type::List(Box::new(inner.clone())))),
        ]))),
        ("shuffle", 0) => Some(Some(iter_ty(inner.clone()))),
        ("chunk_while", 1) => Some(Some(iter_ty(Type::List(Box::new(inner.clone()))))),
        // #1477: remaining List ledger surface.
        ("starts_with" | "ends_with" | "equal", 1) => Some(Some(Type::Bool)),
        ("copy", 0) => Some(Some(Type::List(Box::new(inner.clone())))),
        ("slice", 2) => Some(Some(Type::List(Box::new(inner.clone())))),
        ("binary_search", 1) => Some(Some(Type::Option(Box::new(Type::Int)))),
        ("binary_search_by", 1) => Some(Some(Type::Option(Box::new(Type::Int)))),
        ("union" | "intersection" | "difference", 1) => {
            Some(Some(Type::List(Box::new(inner.clone()))))
        }
        ("random", 0) => Some(Some(Type::Option(Box::new(inner.clone())))),
        ("min_max", 0) => Some(Some(Type::Option(Box::new(Type::Tuple(vec![
            ("min".to_string(), Box::new(inner.clone())),
            ("max".to_string(), Box::new(inner.clone())),
        ]))))),
        ("min_max_by", 1) => Some(Some(Type::Option(Box::new(Type::Tuple(vec![
            ("min".to_string(), Box::new(inner.clone())),
            ("max".to_string(), Box::new(inner.clone())),
        ]))))),
        // #1477: ledger `replace` — substitute every equal element.
        ("replace", 2) => Some(Some(Type::List(Box::new(inner.clone())))),
        ("flatten", 0) => match inner {
            Type::List(elem) => Some(Some(iter_ty(*elem.clone()))),
            _ => Some(Some(iter_ty(Type::Int))),
        },
        ("intersperse", 1) => Some(Some(iter_ty(inner.clone()))),
        // D-ITER1 / D-RANGE-EXCL1=C: indexed → Iter<(idx: Int, item: T)>.
        ("indexed", 0) => Some(Some(iter_ty(indexed_elem_ty(inner)))),
        // D-RANGE-EXCL1=C: every valid Int index for this sequence.
        ("indexes", 0) => Some(Some(iter_ty(Type::Int))),
        // D-ITER1: zip([U]) → Iter<(a: T, b: U)>; sema refines `b` from arg type.
        ("zip" | "zip_short" | "zip_pad", _) => {
            // placeholder element type (Int for `b`); sema will correct via resolved_ret.
            Some(Some(iter_ty(zip_elem_ty(inner, &Type::Int))))
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
        // D-ITERTOOLS1=A: closure adapters returning `Iter<T>`.
        ("take_while" | "skip_while", 1) => Some(Some(iter_ty(inner.clone()))),
        // D-ITER1: flat_map(f: T->[U]) → Iter<U>; placeholder; sema refines.
        ("flat_map", 1) => Some(Some(iter_ty(Type::Int))),
        // D-FAILCOMP1: filter_map(f: T -> V?E) → Iter<V>; keeps ok, drops err; sema refines V.
        ("filter_map", 1) => Some(Some(iter_ty(Type::Int))),
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
        // D-ITERTOOLS1=A: scan(seed, f: (acc,T)->acc) → Iter<acc>; placeholder; sema refines.
        ("scan", 2) => Some(Some(iter_ty(Type::Int))),
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
        // D-PARCAPTURE1=D: parallel adapters stay eager lists.
        ("para_map", 1) => Some(Some(Type::List(Box::new(Type::Int)))), // sema refines V
        ("para_filter", 1) => Some(Some(Type::List(Box::new(inner.clone())))),
        ("para_partition", 1) => Some(Some(partition_ret_ty(inner))),
        ("para_fold", 3) => Some(Some(Type::Int)), // sema refines from seed factory
        // D-DYNARRAY1: `list.view(a..b)` — zero-copy window constructor. Parsed
        // specially (the `..` between the two Int ends), so it always arrives
        // here as a 2-arg call; ownership tracking (E2305) happens at the
        // binding site (`CheckerCore::check_binding`), not here (I3: this
        // table is return-TYPE only).
        ("view", 2) => Some(Some(Type::Apply {
            name: "View".to_string(),
            args: vec![inner.clone()],
        })),
        // D-MEMDISJOINT1=A: runtime-proven mutable partitions use one checked
        // result family. Every successful leaf is the existing tracked ViewMut.
        ("split_write", 1) => Some(Some(Type::Result {
            ok: Box::new(Type::Tuple(vec![
                ("left".to_string(), Box::new(mutable_view())),
                ("right".to_string(), Box::new(mutable_view())),
            ])),
            err: Box::new(Type::String),
        })),
        ("get_disjoint_write", 1) => Some(Some(Type::Result {
            ok: Box::new(Type::List(Box::new(mutable_view()))),
            err: Box::new(Type::String),
        })),
        ("edit_disjoint", 2) => Some(Some(Type::Result {
            ok: Box::new(Type::Tuple(vec![])),
            err: Box::new(Type::String),
        })),
        _ => None,
    }
}

/// D-ITERTOOLS1=A: methods on a lazy `Iter<T>` view.
/// Adapters return another `Iter`; `to_list`/`collect` and reducers materialize.
fn iter_method_return(inner: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        // Explicit materialization.
        ("to_list" | "collect", 0) => Some(Some(Type::List(Box::new(inner.clone())))),
        // Lazy adapters / reducers: same surface as lists (minus in-place mutators).
        ("len", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("sum" | "product", 0) => Some(Some(inner.clone())),
        ("min" | "max", 0) => Some(Some(Type::Option(Box::new(inner.clone())))),
        ("map", 1) => Some(Some(iter_ty(Type::Int))),
        ("filter", 1) => Some(Some(iter_ty(inner.clone()))),
        ("each", 1) => Some(None),
        ("find", 1) => Some(Some(Type::Option(Box::new(inner.clone())))),
        ("any" | "all", 1) => Some(Some(Type::Bool)),
        ("reduce", 2) => Some(Some(Type::Int)),
        ("take" | "skip" | "step_by", 1) => Some(Some(iter_ty(inner.clone()))),
        ("dedup", 0) => Some(Some(iter_ty(inner.clone()))),
        ("chunks" | "windows", 1) => Some(Some(iter_ty(Type::List(Box::new(inner.clone()))))),
        ("flatten", 0) => match inner {
            Type::List(elem) => Some(Some(iter_ty(*elem.clone()))),
            _ => Some(Some(iter_ty(Type::Int))),
        },
        ("intersperse", 1) => Some(Some(iter_ty(inner.clone()))),
        ("indexed", 0) => Some(Some(iter_ty(indexed_elem_ty(inner)))),
        ("indexes", 0) => Some(Some(iter_ty(Type::Int))),
        ("zip" | "zip_short" | "zip_pad", _) => {
            Some(Some(iter_ty(zip_elem_ty(inner, &Type::Int))))
        }
        ("unzip", 0) => list_method_return(inner, "unzip", 0),
        ("partition", 1) => Some(Some(partition_ret_ty(inner))),
        ("take_while" | "skip_while", 1) => Some(Some(iter_ty(inner.clone()))),
        ("flat_map", 1) => Some(Some(iter_ty(Type::Int))),
        ("filter_map", 1) => Some(Some(iter_ty(Type::Int))),
        ("try_collect", 0) => list_method_return(inner, "try_collect", 0),
        ("scan", 2) => Some(Some(iter_ty(Type::Int))),
        ("position", 1) => Some(Some(Type::Option(Box::new(Type::Int)))),
        ("min_by" | "max_by", 1) => Some(Some(Type::Option(Box::new(inner.clone())))),
        ("fold", 2) => Some(Some(Type::Int)),
        ("group_by", 1) => Some(Some(Type::Map {
            key: Box::new(Type::String),
            key_span: None,
            value: Box::new(Type::List(Box::new(inner.clone()))),
        })),
        ("count_by", 1) => Some(Some(Type::Map {
            key: Box::new(Type::String),
            key_span: None,
            value: Box::new(Type::Int),
        })),
        ("join", 1) => Some(Some(Type::String)),
        // #1479: remaining Iter ledger surface
        ("is_sorted", 0) => Some(Some(Type::Bool)),
        ("is_sorted_by", 1) => Some(Some(Type::Bool)),
        ("dedup_by", 1) => Some(Some(iter_ty(inner.clone()))),
        // D-ITER1: bounded — cycles the sequence, producing exactly `n`
        // items (not `n` loops; `repeat(n)` already covers "loop n times").
        // A 0-arg infinite `cycle()` has no safe representation across
        // AOT/JIT/interpreter without unbounded materialization risk (I9).
        ("cycle", 1) => Some(Some(iter_ty(inner.clone()))),
        ("repeat", 1) => Some(Some(iter_ty(inner.clone()))),
        ("drop_last", 1) => Some(Some(iter_ty(inner.clone()))),
        ("last_index_of", 1) => Some(Some(Type::Option(Box::new(Type::Int)))),
        ("average", 0) => Some(Some(Type::Float)),
        ("to_set", 0) => Some(Some(Type::Apply {
            name: Syntax::TYPE_SET.to_string(),
            args: vec![inner.clone()],
        })),
        ("compare", 1) => Some(Some(Type::Int)),
        ("split", 1) => Some(Some(Type::Tuple(vec![
            ("left".to_string(), Box::new(Type::List(Box::new(inner.clone())))),
            ("right".to_string(), Box::new(Type::List(Box::new(inner.clone())))),
        ]))),
        ("shuffle", 0) => Some(Some(iter_ty(inner.clone()))),
        ("chunk_while", 1) => Some(Some(iter_ty(Type::List(Box::new(inner.clone()))))),
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
        ("join", 1) => Some(Some(Type::String)),
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
    match (method, nargs) {
        // D-FAIL-CARRIER1=A: `.or_err("why")` lifts a clean absence into a
        // failure. The payload rides through; only the report changes.
        ("or_err", 1) => Some(Some(Type::Result {
            ok: Box::new(inner.clone()),
            err: Box::new(Type::Named(crate::Syntax::TYPE_ERR.to_string())),
        })),
        ("map", 1) => Some(Some(Type::Option(Box::new(Type::Int)))),
        _ => None,
    }
}

/// D-FAIL-CARRIER1=A: the middle states of the carrier, read from the fallible
/// view. `.partial` answers `T?` — sema first proves the error type carries the
/// surviving payload under that name and at that type, so the answer is
/// derivable from the receiver alone.
fn result_method_return(ok: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("partial", 0) => Some(Some(Type::Option(Box::new(ok.clone())))),
        ("notes", 0) => Some(Some(Type::List(Box::new(Type::String)))),
        _ => None,
    }
}

fn map_method_return(key: &Type, value: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("len", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("clear", 0) => Some(None),
        ("add" | "replace", 2) => Some(Some(Type::Option(Box::new(value.clone())))),
        ("add_new", 2) => Some(Some(Type::Bool)),
        ("get" | "remove" | "pop", 1) => Some(Some(Type::Option(Box::new(value.clone())))),
        ("has_key", 1) => Some(Some(Type::Bool)),
        ("contains_value", 1) => Some(Some(Type::Bool)),
        ("pop_first", 0) => Some(Some(Type::Option(Box::new(value.clone())))),
        // D-LISTREMOVE1/F: map projections are lazy Iter views, not eager copies.
        ("keys", 0) => Some(Some(iter_ty(key.clone()))),
        ("values", 0) => Some(Some(iter_ty(value.clone()))),
        // D-MAP-MERGE1=E: merge(other) / merge(other, conflict) → same map type.
        ("merge", 1 | 2) => Some(Some(Type::Map {
            key: Box::new(key.clone()),
            key_span: None,
            value: Box::new(value.clone()),
        })),
        ("each", 1) => Some(None),
        // #1477: remaining Map ledger surface.
        ("copy", 0) => Some(Some(Type::Map {
            key: Box::new(key.clone()),
            key_span: None,
            value: Box::new(value.clone()),
        })),
        ("equal", 1) => Some(Some(Type::Bool)),
        ("first", 0) => Some(Some(Type::Option(Box::new(key.clone())))),
        ("to_list", 0) => Some(Some(Type::List(Box::new(Type::Tuple(vec![
            ("key".to_string(), Box::new(key.clone())),
            ("value".to_string(), Box::new(value.clone())),
        ]))))),
        ("any" | "all", 1) => Some(Some(Type::Bool)),
        ("map", 1) => Some(Some(Type::Map {
            key: Box::new(key.clone()),
            key_span: None,
            value: Box::new(Type::Int),
        })),
        ("filter" | "flat_map" | "intersection", 1) => Some(Some(Type::Map {
            key: Box::new(key.clone()),
            key_span: None,
            value: Box::new(value.clone()),
        })),
        ("fold", 2) => Some(Some(Type::Int)),
        ("max" | "min", 0) => Some(Some(Type::Option(Box::new(value.clone())))),
        ("slice", 1) => Some(Some(Type::Map {
            key: Box::new(key.clone()),
            key_span: None,
            value: Box::new(value.clone()),
        })),
        _ => None,
    }
}

fn string_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("len", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("contains" | "starts_with" | "ends_with", 1) => Some(Some(Type::Bool)),
        ("trim" | "trim_start" | "trim_end" | "to_upper" | "to_lower" | "to_title" | "to_string", 0) => {
            Some(Some(Type::String))
        }
        ("is_alphabetic" | "is_numeric" | "is_whitespace" | "is_ascii", 0) => {
            Some(Some(Type::Bool))
        }
        // #1476 remaining String surface.
        ("is_lower" | "is_upper", 0) => Some(Some(Type::Bool)),
        ("capitalize" | "swapcase" | "copy" | "reverse" | "normalize", 0) => {
            Some(Some(Type::String))
        }
        ("remove_prefix" | "remove_suffix", 1) => Some(Some(Type::String)),
        ("last_index_of", 1) => Some(Some(Type::Option(Box::new(Type::Int)))),
        ("compare", 1) => Some(Some(Type::Int)),
        ("equal", 1) => Some(Some(Type::Bool)),
        ("rsplit", 1) => Some(Some(iter_ty(Type::String))),
        ("bytes", 0) => Some(Some(Type::List(Box::new(u8t())))),
        ("replace" | "slice", 2) => Some(Some(Type::String)),
        ("pad_start" | "pad_end", 2) => Some(Some(Type::String)),
        ("index_of", 1) => Some(Some(Type::Option(Box::new(Type::Int)))),
        ("count", 1) => Some(Some(Type::Int)),
        ("split_once", 1) => Some(Some(Type::Option(Box::new(Type::Tuple(vec![
            ("before".to_string(), Box::new(Type::String)),
            ("after".to_string(), Box::new(Type::String)),
        ]))))),
        // D-STR-AFTER1: first-occurrence substring split; `sep` absent -> the
        // whole original string (mirrors `.replace`'s no-match-is-identity).
        ("after" | "before", 1) => Some(Some(Type::String)),
        ("split", 1) => Some(Some(iter_ty(Type::String))),
        // c97/D-STRPARSE1: split text into its lines (mirrors `split`).
        ("lines", 0) => Some(Some(Type::List(Box::new(Type::String)))),
        ("chars", 0) => Some(Some(Type::List(Box::new(Type::Char)))),
        ("repeat", 1) => Some(Some(Type::String)),
        // c97/D-STRPARSE1: fallible integer parse. Same `Int ? ParseError` result
        // `Int.parse(s)` returns, so one error type covers text→int.
        // D-STR-DECLINE1=C: `to_int`/`to_float` are direct String spellings of
        // the one parse mechanism `Int.parse`/`Float.parse` already run —
        // same `? ParseError` result, reached one call shorter from text.
        ("to_int", 0) => Some(Some(Type::Result {
            ok: Box::new(Type::Int),
            err: Box::new(Type::Named("ParseError".to_string())),
        })),
        ("to_float", 0) => Some(Some(Type::Result {
            ok: Box::new(Type::Float),
            err: Box::new(Type::Named("ParseError".to_string())),
        })),
        // D-STR-DECLINE1=C: `matches`/`match` route through the one core.regex
        // engine (`core.regex.compile` + `is_match`/`find`) — same `? String`
        // bad-pattern error shape `core.regex.compile` already returns.
        ("matches", 1) => Some(Some(Type::Result {
            ok: Box::new(Type::Bool),
            err: Box::new(Type::String),
        })),
        ("match", 1) => Some(Some(Type::Result {
            ok: Box::new(Type::Option(Box::new(Type::String))),
            err: Box::new(Type::String),
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
/// starts from the seed the caller supplied to `Clock.new(seed)` and only moves
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
        (crate::Syntax::METHOD_DURATION_IS_ZERO, 0) => Some(Some(Type::Bool)),
        (crate::Syntax::METHOD_DURATION_TOTAL_SECONDS, 0) => Some(Some(Type::Int)),
        ("difference", 1) => Some(Some(Type::Named(crate::Syntax::DURATION_TYPE.to_string()))),
        _ => None,
    }
}

fn task_method_return(args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        // D-CONC-FAIL1=A: every task wait uses the one fallible rail. The
        // runtime task handle owns the same `TaskFailure` report for joins,
        // races, and fail-fast group selection.
        ("join", 0) => Some(Some(Type::Result {
            ok: Box::new(
                args.first()
                    .cloned()
                    .unwrap_or_else(|| Type::Named("Unit".to_string())),
            ),
            err: Box::new(Type::Named(Syntax::TYPE_TASK_FAILURE.to_string())),
        })),
        // D-DETACH1: fire-and-forget — consumes the Task handle, returns unit.
        (Syntax::TASK_DETACH, 0) => Some(None),
        // D-COROUTINE1=A: task handle control-plane hooks over the internal coroutine substrate.
        (Syntax::METHOD_TASK_PAUSE, 0)
        | (Syntax::METHOD_TASK_PAUSE, 1)
        | (Syntax::METHOD_TASK_RESUME, 0)
        | (Syntax::METHOD_TASK_CANCEL, 0) => Some(None),
        ("trace", 0) => Some(Some(Type::String)),
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
        ("close", 0) => Some(None),
        _ => None,
    }
}

fn sender_method_return(_args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("send", 1) => Some(None),
        ("close", 0) => Some(None),
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
        ("guard_read", 0) => Some(Some(shared_guard_type(
            inner.clone(),
            crate::AST::InternalTag::SharedGuardRead,
        ))),
        ("guard_edit", 0) => Some(Some(shared_guard_type(
            inner.clone(),
            crate::AST::InternalTag::SharedGuardEdit,
        ))),
        // D-SHARED-CYCLE1=C: expert weak edge for intentional cycles.
        ("downgrade", 0) => Some(Some(Type::Apply {
            name: Syntax::TYPE_SHARED_WEAK.to_string(),
            args: vec![inner.clone()],
        })),
        ("strong_count", 0) => Some(Some(Type::Int)),
        _ => None,
    }
}

fn shared_weak_method_return(inner: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("upgrade", 0) => Some(Some(Type::Option(Box::new(Type::Shared(Box::new(
            inner.clone(),
        )))))),
        _ => None,
    }
}

fn shared_guard_type(inner: Type, marker: crate::AST::InternalTag) -> Type {
    Type::Tagged {
        marker: crate::AST::TagMarker::Internal(marker),
        inner: Box::new(Type::Apply {
            name: Syntax::TYPE_SHARED_GUARD.to_string(),
            args: vec![inner],
        }),
    }
}

fn shared_guard_method_return(inner: &Type, method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        // Sema refines projection types and reapplies the hidden access tag.
        ("map", 1) => Some(Some(Type::Apply {
            name: Syntax::TYPE_SHARED_GUARD.to_string(),
            args: vec![inner.clone()],
        })),
        ("split", 2) => Some(Some(Type::Tuple(vec![
            (
                "first".to_string(),
                Box::new(Type::Apply {
                    name: Syntax::TYPE_SHARED_GUARD.to_string(),
                    args: vec![inner.clone()],
                }),
            ),
            (
                "second".to_string(),
                Box::new(Type::Apply {
                    name: Syntax::TYPE_SHARED_GUARD.to_string(),
                    args: vec![inner.clone()],
                }),
            ),
        ]))),
        ("wait", 2) => Some(None),
        _ => None,
    }
}

fn condition_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    match (method, nargs) {
        ("notify_one" | "notify_all", 0) => Some(None),
        _ => None,
    }
}

fn cell_method_return(args: &[Type], method: &str, nargs: usize) -> Option<Option<Type>> {
    let t = args.first().cloned().unwrap_or(Type::Int);
    match (method, nargs) {
        ("get", 0) | ("replace", 1) => Some(Some(t.clone())),
        ("set", 1) => Some(None),
        ("get_or_set", 1) => match t {
            Type::Option(inner) => Some(Some(*inner)),
            _ => Some(Some(t)),
        },
        ("read", 1) | ("edit", 1) => Some(Some(t.clone())),
        ("guard_read", 0) => Some(Some(Type::Apply {
            name: "CellReadGuard".to_string(),
            args: vec![t],
        })),
        ("guard_edit", 0) => Some(Some(Type::Apply {
            name: "CellEditGuard".to_string(),
            args: vec![t],
        })),
        _ => None,
    }
}

fn cell_guard_method_return(
    args: &[Type],
    method: &str,
    nargs: usize,
    editable: bool,
) -> Option<Option<Type>> {
    let t = args.first().cloned().unwrap_or(Type::Int);
    match (method, nargs) {
        ("get", 0) => Some(Some(t.clone())),
        ("set", 1) if editable => Some(None),
        ("read", 1) => Some(Some(t.clone())),
        ("edit", 1) if editable => Some(Some(t.clone())),
        ("map", 1) => Some(Some(Type::Apply {
            name: if editable {
                "CellEditGuard".to_string()
            } else {
                "CellReadGuard".to_string()
            },
            args: vec![t],
        })),
        ("split", 2) => Some(Some(Type::Tuple(vec![
            (
                "first".to_string(),
                Box::new(Type::Apply {
                    name: if editable {
                        "CellEditGuard".to_string()
                    } else {
                        "CellReadGuard".to_string()
                    },
                    args: vec![t.clone()],
                }),
            ),
            (
                "second".to_string(),
                Box::new(Type::Apply {
                    name: if editable {
                        "CellEditGuard".to_string()
                    } else {
                        "CellReadGuard".to_string()
                    },
                    args: vec![t],
                }),
            ),
        ]))),
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
        ("intersection" | "difference" | "symmetric_difference", 1) => {
            Some(Some(set_of_elem()))
        }
        ("is_subset" | "is_superset" | "is_disjoint", 1) => Some(Some(Type::Bool)),
        ("to_list", 0) => Some(Some(Type::List(Box::new(elem.clone())))),
        // #1478: remaining Set surface (non-closure).
        ("copy" | "to_set", 0) => Some(Some(set_of_elem())),
        ("equal", 1) => Some(Some(Type::Bool)),
        ("capacity", 0) => Some(Some(Type::Int)),
        ("first", 0) => Some(Some(Type::Option(Box::new(elem.clone())))),
        // #1478: ledger closes on Set's remaining order-agnostic surface.
        // `values` is the lazy alias of `to_list` (I8, mirrors `Map.values`).
        ("values", 0) => Some(Some(iter_ty(elem.clone()))),
        // Native swap-in / remove-and-return — Rust's own HashSet contract.
        ("replace" | "take", 1) => Some(Some(Type::Option(Box::new(elem.clone())))),
        ("all", 1) => Some(Some(Type::Bool)),
        ("each", 1) => Some(None),
        ("filter", 1) => Some(Some(Type::List(Box::new(elem.clone())))),
        ("min" | "max", 0) => Some(Some(Type::Option(Box::new(elem.clone())))),
        // `map`/`fold`/`flat_map` placeholders — sema refines from the
        // closure body's actual return type, same as the List surface.
        ("map", 1) => Some(Some(Type::List(Box::new(Type::Int)))),
        ("fold", 2) => Some(Some(Type::Int)),
        ("flat_map", 1) => Some(Some(iter_ty(Type::Int))),
        // D-SET-DECLINE1=C: `sort`/`shuffle` turn an unordered Set into an
        // ordered `List`, the same to-list-then-List machinery `filter`/`map`/
        // `fold`/`each`/`all`/`min`/`max` already run. Neither mutates the Set.
        ("sort" | "shuffle", 0) => Some(Some(Type::List(Box::new(elem.clone())))),
        // #1478: `flatten` declined — E0506 forbids a Set element type that
        // is itself a List/Set (Set elements must be Hash+Eq), so no legal
        // `Set<T>` can ever satisfy `flatten`'s nested-container precondition.
        // `indexof`/`indexed` also declined — a hash Set keeps no stable
        // position for `index_of`/`indexed` to answer. See the D-SET-DECLINE1
        // ballot for the full declined-name rationale.
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
        ("intersection" | "difference" | "symmetric_difference", 1) => {
            Some(Some(set_of_elem()))
        }
        ("is_subset" | "is_superset" | "is_disjoint", 1) => Some(Some(Type::Bool)),
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
        // D-LISTREMOVE1/F (criterion c6 on #1481): same value/slot shape as List.remove.
        ("remove", 1 | 2) => Some(Some(Type::Option(Box::new(elem.clone())))),
        ("to_sorted_list", 0) => Some(Some(Type::List(Box::new(elem.clone())))),
        _ => None,
    }
}

/// D-ITERTOOLS1=A: `Cache<K,V>` bounded cache methods.
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

/// D-ITERTOOLS1=A / #1467: `ByteBuffer` builder + read cursor + string-like.
fn byte_buffer_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    let bytes = || Type::List(Box::new(u8t()));
    let buf = || Type::Named(Syntax::TYPE_BYTE_BUFFER.to_string());
    match (method, nargs) {
        ("len" | "capacity" | "position", 0) => Some(Some(Type::Int)),
        ("is_empty" | "eof" | "is_ascii", 0) => Some(Some(Type::Bool)),
        ("clear" | "rewind" | "flush" | "close" | "shutdown", 0) => Some(None),
        ("to_bytes" | "get_buffer" | "buffer", 0) => Some(Some(bytes())),
        ("to_string" | "string", 0) => Some(Some(Type::String)),
        ("trim" | "trim_start" | "trim_end" | "to_lower" | "to_upper" | "to_title" | "title"
        | "clone" | "copy", 0) => Some(Some(buf())),
        ("lines", 0) => Some(Some(Type::List(Box::new(Type::String)))),
        ("first" | "next" | "read_byte", 0) => Some(Some(Type::Option(Box::new(u8t())))),
        ("read", 0) => Some(Some(Type::Option(Box::new(bytes())))),
        ("get", 1) => Some(Some(Type::Option(Box::new(u8t())))),
        ("seek", 1) => Some(None),
        ("read_bytes", 1) => Some(Some(Type::Option(Box::new(bytes())))),
        ("read_string", 1) => Some(Some(Type::Option(Box::new(Type::String)))),
        ("contains" | "starts_with" | "ends_with", 1) => Some(Some(Type::Bool)),
        ("index_of" | "last_index_of", 1) => Some(Some(Type::Option(Box::new(Type::Int)))),
        ("split", 1) => Some(Some(Type::List(Box::new(Type::String)))),
        ("join", 1) => Some(Some(buf())),
        ("replace", 2) => Some(Some(buf())),
        ("equal" | "compare", 1) => Some(Some(if method == "equal" {
            Type::Bool
        } else {
            Type::Int
        })),
        ("copy_to" | "write_to", 1) => Some(None),
        ("parse", 0) => Some(Some(Type::Result {
            ok: Box::new(Type::Int),
            err: Box::new(Type::String),
        })),
        (
            "write_u8" | "write_byte" | "write_u16_le" | "write_u16_be" | "write_u32_le"
            | "write_u32_be" | "write_u64_le" | "write_u64_be" | "write_bytes" | "write",
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
    let deque = || Type::Apply {
        name: "Deque".to_string(),
        args: vec![elem.clone()],
    };
    match (method, nargs) {
        ("len" | "capacity", 0) => Some(Some(Type::Int)),
        ("is_empty", 0) => Some(Some(Type::Bool)),
        ("push_front" | "push_back" | "clear" | "delete" | "reverse", _) => Some(None),
        ("pop_front" | "pop_back" | "peek_front" | "peek_back", 0) => {
            Some(Some(Type::Option(Box::new(elem.clone()))))
        }
        ("get", 1) => Some(Some(Type::Option(Box::new(elem.clone())))),
        ("contains", 1) => Some(Some(Type::Bool)),
        ("to_list", 0) => Some(Some(Type::List(Box::new(elem.clone())))),
        ("join", 1) => Some(Some(Type::String)),
        ("split", 1) => Some(Some(deque())),
        _ => None,
    }
}

/// Whether a built-in method mutates its receiver (needs `var` binding).
pub fn builtin_method_mutates(recv_ty: &Type, method: &str) -> bool {
    if let Type::Tagged { inner, .. } = recv_ty {
        return builtin_method_mutates(inner, method);
    }
    match recv_ty {
        Type::List(_) => matches!(
            method,
            "push"
                | "pop"
                | "insert"
                | "remove"
                | "extend"
                | "reverse"
                | "sort"
                | "sort_by"
                | "clear"
                | "split_write"
                | "get_disjoint_write"
                | "edit_disjoint"
        ),
        Type::Map { .. } => matches!(
            method,
            "add" | "add_new" | "replace" | "remove" | "pop" | "pop_first" | "clear"
        ),
        // D-COLLBREADTH1=A: Set mutating methods.
        Type::Apply { name, .. } if name == "Set" => {
            // #1478: `.replace`/`.take` are native HashSet `&mut self` methods.
            matches!(method, "add" | "remove" | "clear" | "replace" | "take")
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
                | "write_byte"
                | "write_u16_le"
                | "write_u16_be"
                | "write_u32_le"
                | "write_u32_be"
                | "write_u64_le"
                | "write_u64_be"
                | "write_bytes"
                | "write"
                | "write_to"
                | "clear"
                | "seek"
                | "rewind"
                | "next"
                | "read"
                | "read_byte"
                | "read_bytes"
                | "read_string"
                | "flush"
                | "close"
                | "shutdown"
                | "copy_to"
        ),
        // D-TAG1: Bag mutating methods.
        Type::Apply { name, .. } if name == "Bag" => {
            matches!(method, "add" | "remove" | "clear")
        }
        // D-COLLBREADTH1=A: Deque mutating methods.
        Type::Apply { name, .. } if name == "Deque" => {
            matches!(
                method,
                "push_front"
                    | "push_back"
                    | "pop_front"
                    | "pop_back"
                    | "clear"
                    | "delete"
                    | "reverse"
                    | "split"
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
        Type::Named(n) if n == Syntax::TYPE_RANGE && method == "contains" => {
            Some(vec![Type::Int])
        }
        Type::Named(n) if n == "Secret" && method == "from_text" => Some(vec![Type::String]),
        Type::Named(n) if matches!(n.as_str(), "Secret" | "VerifyKey" | "X25519PublicKey" | "Signature" | "Sealed" | "WrappedKey" | "WrappedVaultKey") && method == "from_bytes" => Some(vec![Type::List(Box::new(Type::IntN { signed: false, bits: 8 }))]),
        Type::Named(n) if n == "KeyUnlock" && method == "Recipient" => Some(vec![Type::Named("X25519SecretKey".into())]),
        Type::Named(n) if n == "KeyUnlock" && method == "Passphrase" => Some(vec![Type::Named("Secret".into())]),
        Type::Named(n) if n == "PasswordHash" && method == "parse" => Some(vec![Type::String]),
        Type::Named(n)
            if n == crate::Syntax::DURATION_TYPE
                && method == crate::Syntax::METHOD_DURATION_IN =>
        {
            Some(vec![Type::Named(
                crate::Syntax::DURATION_UNIT_TYPE.to_string(),
            )])
        }
        Type::Named(n)
            if n == crate::Syntax::DURATION_TYPE && method == "difference" =>
        {
            Some(vec![Type::Named(crate::Syntax::DURATION_TYPE.to_string())])
        }
        Type::Named(n) if n == "X25519PublicKey" && method == "from_text" => Some(vec![Type::String]),
        Type::Named(n) if matches!(n.as_str(), "SigningKey" | "X25519SecretKey") && method == "new_random" => Some(vec![]),
        Type::Named(n) if matches!(n.as_str(), "SigningKey" | "X25519SecretKey" | "VerifyKey" | "X25519PublicKey" | "Signature" | "Sealed" | "WrappedKey" | "WrappedVaultKey" | "Digest256" | "Digest512" | "PasswordHash") => Some(vec![]),
        Type::Named(n) if n == Syntax::TYPE_BUILD_CONTEXT => match method {
            "generate" => Some(vec![Type::String, Type::String]),
            "find" | "embed" => Some(vec![Type::String]),
            "fetch" => Some(vec![Type::String, Type::String]),
            "plugin" => Some(vec![Type::String, Type::String]),
            "action" => Some(vec![
                Type::String,
                Type::List(Box::new(Type::String)),
                Type::List(Box::new(Type::String)),
                Type::List(Box::new(Type::String)),
                Type::List(Box::new(Type::String)),
                Type::Named(Syntax::TYPE_BUILD_TOOLCHAIN.to_string()),
                Type::List(Box::new(Type::Named(Syntax::TYPE_BUILD_PROBE.to_string()))),
            ]),
            "legacy" => Some(vec![
                Type::String,
                Type::String,
                Type::List(Box::new(Type::String)),
                Type::List(Box::new(Type::String)),
                Type::List(Box::new(Type::String)),
                Type::List(Box::new(Type::String)),
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
        Type::Named(n) if n == "CompilerLexed" && matches!(method, "source" | "tokens" | "diagnostics") => Some(vec![]),
        Type::Named(n) if n == "CompilerSyntaxTree" && matches!(method, "source" | "items" | "diagnostics") => Some(vec![]),
        Type::Named(n) if n == "CompilerChecked" && matches!(method, "source" | "syntax" | "functions" | "effects" | "diagnostics" | "semantic_index") => Some(vec![]),
        Type::Named(n) if n == "CompilerSourceMap" && matches!(method, "sources" | "generated_lines") => Some(vec![]),
        Type::Named(n) if n == "FunctionInfo" && method == "reaches_panic" => Some(vec![]),
        Type::Named(n) if n == "EffectInfo" && method == "has" => Some(vec![Type::String]),
        // D-ITERTOOLS1=A: Iter shares list adapter arg types; materializers take none.
        Type::Apply { name, args }
            if name == Syntax::TYPE_ITER && args.len() == 1 && matches!(method, "to_list" | "collect") =>
        {
            Some(vec![])
        }
        Type::Apply { name, args } if name == Syntax::TYPE_ITER && args.len() == 1 => {
            builtin_method_arg_types(&Type::List(Box::new(args[0].clone())), method)
        }
        Type::List(inner) => match method {
            "push" | "contains" => Some(vec![(**inner).clone()]),
            "insert" => Some(vec![Type::Int, (**inner).clone()]),
            "get" | "index_of" => Some(vec![Type::Int]),
            "remove" => Some(vec![
                (**inner).clone(),
                Type::Named(Syntax::TYPE_REMOVE_BY.to_string()),
            ]),
            "count" => Some(vec![(**inner).clone()]),
            "extend" | "concat" => Some(vec![Type::List(Box::new((**inner).clone()))]),
            "split_write" => Some(vec![Type::Int]),
            "get_disjoint_write" => Some(vec![Type::List(Box::new(Type::Int))]),
            "edit_disjoint" => Some(vec![
                Type::List(Box::new(Type::Int)),
                Type::Fn {
                    params: vec![
                        Type::Apply {
                            name: "ViewMut".to_string(),
                            args: vec![(**inner).clone()],
                        },
                        Type::Apply {
                            name: "ViewMut".to_string(),
                            args: vec![(**inner).clone()],
                        },
                    ],
                    ret: None,
                    effect_bound: None,
                    param_contract: None,
                call_metadata: None,
                    return_view_provenance: None,
                },
            ]),
            "join" => Some(vec![Type::String]),
            "map" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None, // sema refines V from closure's actual return
                effect_bound: None, return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            }]),
            "filter" | "find" | "any" | "all"
            // D-ITER1: closure bool predicates.
            | "take_while" | "skip_while" | "position" | "partition" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: Some(Box::new(Type::Bool)),
                effect_bound: None, return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            }]),
            "each" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None,
                effect_bound: None, return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            }]),
            // D-ITER1: key-extracting closure methods.
            "sort_by" | "min_by" | "max_by" | "group_by" | "count_by" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None, // sema refines key type
                effect_bound: None, return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            }]),
            "reduce" | "fold" => Some(vec![
                Type::Int, // init — sema refines
                Type::Fn {
                    params: vec![Type::Int, (**inner).clone()],
                    ret: Some(Box::new(Type::Int)),
                    effect_bound: None, return_view_provenance: None,
                    param_contract: None,
                call_metadata: None,
                },
            ]),
            "scan" => Some(vec![
                Type::Int, // seed — sema refines
                Type::Fn {
                    params: vec![Type::Int, (**inner).clone()],
                    ret: Some(Box::new(Type::Int)), // sema refines
                    effect_bound: None, return_view_provenance: None,
                    param_contract: None,
                call_metadata: None,
                },
            ]),
            "flat_map" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None, // sema refines the returned list's element type
                effect_bound: None, return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            }]),
            // D-FAILCOMP1: filter_map(f: T -> V?E) → [V]; keeps ok, drops err.
            // ret: None so any Result return is accepted; sema refines V via calls.rs.
            "filter_map" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None,
                effect_bound: None, return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            }]),
            // D-PARCAPTURE1=D: parallel adapters. `para_fold` separates fresh
            // worker state, per-item stepping, and deterministic merging.
            "para_map" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None, // sema refines V from closure body
                effect_bound: None, return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            }]),
            "para_filter" | "para_partition" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: Some(Box::new(Type::Bool)),
                effect_bound: None, return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            }]),
            "para_fold" => Some(vec![
                Type::Fn {
                    params: vec![],
                    ret: None, // sema refines accumulator from seed factory
                    effect_bound: None, return_view_provenance: None,
                    param_contract: None,
                call_metadata: None,
                },
                Type::Fn {
                    params: vec![Type::Int, (**inner).clone()],
                    ret: Some(Box::new(Type::Int)), // sema refines
                    effect_bound: None, return_view_provenance: None,
                    param_contract: None,
                call_metadata: None,
                },
                Type::Fn {
                    params: vec![Type::Int, Type::Int],
                    ret: Some(Box::new(Type::Int)),
                    effect_bound: None, return_view_provenance: None,
                    param_contract: None,
                call_metadata: None,
                },
            ]),
            // D-ITER1: non-closure adapters.
            "take" | "skip" | "step_by" | "chunks" | "windows" | "repeat" | "drop_last" | "split"
            | "cycle" => {
                Some(vec![Type::Int])
            }
            "intersperse" | "last_index_of" | "binary_search" => Some(vec![(**inner).clone()]),
            "replace" => Some(vec![(**inner).clone(), (**inner).clone()]),
            "compare" | "starts_with" | "ends_with" | "equal" | "union" | "intersection"
            | "difference" => Some(vec![Type::List(Box::new((**inner).clone()))]),
            "slice" => Some(vec![Type::Int, Type::Int]),
            "zip" => Some(vec![]),
            "dedup"
            | "indexed"
            | "indexes"
            | "sum"
            | "product"
            | "min"
            | "max"
            | "flatten"
            | "unzip"
            | "is_sorted"
            | "shuffle"
            | "average"
            | "to_set" | "copy" | "random" | "min_max" => Some(vec![]),
            "dedup_by" | "is_sorted_by" | "binary_search_by" | "min_max_by" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None,
                effect_bound: None,
                return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            }]),
            "chunk_while" => Some(vec![Type::Fn {
                params: vec![(**inner).clone(), (**inner).clone()],
                ret: Some(Box::new(Type::Bool)),
                effect_bound: None,
                return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            }]),
            // D-DYNARRAY1: `.view(a..b)` — both range ends are Int (parsed
            // specially; always arrives as exactly 2 args).
            "view" => Some(vec![Type::Int, Type::Int]),
            _ => Some(vec![]),
        },
        Type::Map { key, value, .. } => match method {
            "add" | "add_new" | "replace" => Some(vec![(**key).clone(), (**value).clone()]),
            "get" | "remove" | "pop" | "has_key" => Some(vec![(**key).clone()]),
            "contains_value" => Some(vec![(**value).clone()]),
            "merge" | "equal" | "intersection" => Some(vec![Type::Map {
                key: Box::new((**key).clone()),
                key_span: None,
                value: Box::new((**value).clone()),
            }]),
            "slice" => Some(vec![Type::List(Box::new((**key).clone()))]),
            "any" | "all" | "filter" => Some(vec![Type::Fn {
                params: vec![(**key).clone(), (**value).clone()],
                ret: Some(Box::new(Type::Bool)),
                effect_bound: None,
                return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            }]),
            "map" | "flat_map" => Some(vec![Type::Fn {
                params: vec![(**key).clone(), (**value).clone()],
                ret: None,
                effect_bound: None,
                return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            }]),
            "fold" => Some(vec![
                Type::Int,
                Type::Fn {
                    params: vec![Type::Int, (**key).clone(), (**value).clone()],
                    ret: Some(Box::new(Type::Int)),
                    effect_bound: None,
                    return_view_provenance: None,
                    param_contract: None,
                call_metadata: None,
                },
            ]),
            "each" => Some(vec![Type::Fn {
                params: vec![(**key).clone(), (**value).clone()],
                ret: None,
                effect_bound: None, return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            }]),
            _ => Some(vec![]),
        },
        Type::String => match method {
            "contains" | "starts_with" | "ends_with" | "split" | "index_of" | "count"
            | "split_once" => Some(vec![Type::String]),
            "from_bytes" => Some(vec![Type::List(Box::new(u8t()))]),
            "replace" => Some(vec![Type::String, Type::String]),
            "pad_start" | "pad_end" => Some(vec![Type::Int, Type::String]),
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
            // D-FAIL-CARRIER1=A: the reason a clean absence becomes a failure,
            // and the note an outcome collects on its way.
            "or_err" => Some(vec![Type::String]),
            "map" => Some(vec![Type::Fn {
                params: vec![(**inner).clone()],
                ret: None, // sema refines R from the closure's actual return
                effect_bound: None, return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            }]),
            _ => Some(vec![]),
        },
        Type::Apply { name, args } if name == "Sender" => match method {
            "send" => Some(vec![args.first().cloned().unwrap_or(Type::Int)]),
            _ => Some(vec![]),
        },
        Type::Apply { name, .. } if name == "Task" || name == "Channel" => Some(vec![]),
        Type::Apply { name, args } if name == "Cell" => {
            let t = args.first().cloned().unwrap_or(Type::Int);
            match method {
                "set" | "replace" => Some(vec![t.clone()]),
                "get_or_set" => {
                    let value = match t {
                        Type::Option(inner) => *inner,
                        other => other,
                    };
                    Some(vec![Type::Fn {
                        params: vec![],
                        ret: Some(Box::new(value)),
                        effect_bound: None,
                        param_contract: None,
                call_metadata: None,
                        return_view_provenance: None,
                    }])
                }
                "read" | "edit" => Some(vec![Type::Fn {
                    params: vec![t],
                    ret: None,
                    effect_bound: None,
                    param_contract: None,
                call_metadata: None,
                    return_view_provenance: None,
                }]),
                _ => Some(vec![]),
            }
        }
        Type::Apply { name, args }
            if matches!(name.as_str(), "CellReadGuard" | "CellEditGuard") =>
        {
            let t = args.first().cloned().unwrap_or(Type::Int);
            match method {
                "set" => Some(vec![t.clone()]),
                "read" | "edit" | "map" => Some(vec![Type::Fn {
                    params: vec![t],
                    ret: None,
                    effect_bound: None,
                    param_contract: None,
                call_metadata: None,
                    return_view_provenance: None,
                }]),
                "split" => Some(vec![
                    Type::Fn {
                        params: vec![t.clone()],
                        ret: None,
                        effect_bound: None,
                        param_contract: None,
                call_metadata: None,
                        return_view_provenance: None,
                    },
                    Type::Fn {
                        params: vec![t],
                        ret: None,
                        effect_bound: None,
                        param_contract: None,
                call_metadata: None,
                        return_view_provenance: None,
                    },
                ]),
                _ => Some(vec![]),
            }
        }
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
            "remove" => Some(vec![
                args.first().cloned().unwrap_or(Type::Int),
                Type::Named(Syntax::TYPE_REMOVE_BY.to_string()),
            ]),
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
            "write_bytes" | "write" => Some(vec![Type::List(Box::new(u8t()))]),
            "write_u8" | "write_byte" => Some(vec![u8t()]),
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
            "seek" | "read_bytes" | "read_string" | "get" => Some(vec![Type::Int]),
            "contains" | "starts_with" | "ends_with" | "split" | "index_of" | "last_index_of" => {
                Some(vec![Type::String])
            }
            "replace" => Some(vec![Type::String, Type::String]),
            "join" => Some(vec![Type::List(Box::new(Type::String))]),
            "equal" | "compare" | "copy_to" | "write_to" => {
                Some(vec![Type::Named(Syntax::TYPE_BYTE_BUFFER.to_string())])
            }
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
                        effect_bound: None, return_view_provenance: None,
                        param_contract: None,
                call_metadata: None,
                    },
                ]),
                "on_priority" => Some(vec![
                    Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()),
                    Type::Int,
                    Type::Fn {
                        params: vec![payload],
                        ret: None,
                        effect_bound: None, return_view_provenance: None,
                        param_contract: None,
                call_metadata: None,
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
                ok: Box::new(Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string())),
                err: Box::new(error),
            };
            match method {
                "on" | "once" => Some(vec![
                    Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()),
                    Type::Fn { params: vec![payload], ret: Some(Box::new(handler_ret)), effect_bound: None, param_contract: None,
                call_metadata: None, return_view_provenance: None },
                ]),
                "on_priority" => Some(vec![
                    Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()),
                    Type::Int,
                    Type::Fn { params: vec![payload], ret: Some(Box::new(handler_ret)), effect_bound: None, param_contract: None,
                call_metadata: None, return_view_provenance: None },
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
                        effect_bound: None, return_view_provenance: None,
                        param_contract: None,
                call_metadata: None,
                    },
                ]),
                "on_priority" => Some(vec![
                    Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()),
                    Type::Int,
                    Type::Fn {
                        params: vec![payload],
                        ret: Some(Box::new(result)),
                        effect_bound: None, return_view_provenance: None,
                        param_contract: None,
                call_metadata: None,
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
                        effect_bound: None, return_view_provenance: None,
                        param_contract: None,
                call_metadata: None,
                    },
                ]),
                "on_priority" => Some(vec![
                    Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()),
                    Type::Int,
                    Type::Fn {
                        params: vec![payload],
                        ret: Some(Box::new(decision)),
                        effect_bound: None, return_view_provenance: None,
                        param_contract: None,
                call_metadata: None,
                    },
                ]),
                "run" => Some(vec![payload]),
                _ => Some(vec![]),
            }
        }
        Type::Named(n)
            if n == crate::Syntax::TYPE_EFFECT
                || n == crate::Syntax::TYPE_SUBSCRIPTION
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
                    effect_bound: None, return_view_provenance: None,
                    param_contract: None,
                call_metadata: None,
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
                "add" | "has" | "remove" => Some(vec![elem.clone()]),
                "equal" => Some(vec![Type::Apply {
                    name: "Set".to_string(),
                    args: vec![elem.clone()],
                }]),
                "union" | "intersection" | "difference" | "symmetric_difference"
                | "is_subset" | "is_superset" | "is_disjoint" => Some(vec![Type::Apply {
                    name: "Set".to_string(),
                    args: vec![elem.clone()],
                }]),
                // #1478: replace(v)/take(v) stay on the native single-value form.
                "replace" | "take" => Some(vec![elem.clone()]),
                "filter" | "all" => Some(vec![Type::Fn {
                    params: vec![elem.clone()],
                    ret: Some(Box::new(Type::Bool)),
                    effect_bound: None,
                    return_view_provenance: None,
                    param_contract: None,
                call_metadata: None,
                }]),
                "each" => Some(vec![Type::Fn {
                    params: vec![elem.clone()],
                    ret: None,
                    effect_bound: None,
                    return_view_provenance: None,
                    param_contract: None,
                call_metadata: None,
                }]),
                "map" | "flat_map" => Some(vec![Type::Fn {
                    params: vec![elem],
                    ret: None, // sema refines R from the closure's actual return
                    effect_bound: None,
                    return_view_provenance: None,
                    param_contract: None,
                call_metadata: None,
                }]),
                "fold" => Some(vec![
                    Type::Int, // init — sema refines
                    Type::Fn {
                        params: vec![Type::Int, elem],
                        ret: Some(Box::new(Type::Int)),
                        effect_bound: None,
                        return_view_provenance: None,
                        param_contract: None,
                call_metadata: None,
                    },
                ]),
                _ => Some(vec![]),
            }
        }
        Type::Apply { name, args } if name == Syntax::TYPE_SORTED_SET => {
            let elem = args.first().cloned().unwrap_or(Type::Int);
            match method {
                "add" | "has" | "remove" => Some(vec![elem.clone()]),
                "union" | "intersection" | "difference" | "symmetric_difference"
                | "is_subset" | "is_superset" | "is_disjoint" => Some(vec![Type::Apply {
                    name: Syntax::TYPE_SORTED_SET.to_string(),
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
                    effect_bound: None, return_view_provenance: None,
                    param_contract: None,
                call_metadata: None,
                }]),
                _ => Some(vec![]),
            }
        }
        // D-COLLBREADTH1=A: Deque<T> arg types.
        Type::Apply { name, args } if name == "Deque" => {
            let elem = args.first().cloned().unwrap_or(Type::Int);
            match method {
                "push_front" | "push_back" | "contains" | "delete" => Some(vec![elem]),
                "get" | "split" => Some(vec![Type::Int]),
                "join" => Some(vec![Type::String]),
                _ => Some(vec![]),
            }
        }
        // D-DYNARRAY1: `View<T>` arg types — read-only subset, mirrors `Type::List`.
        Type::Apply { name, args } if matches!(name.as_str(), "View" | "ViewMut") => {
            let elem = args.first().cloned().unwrap_or(Type::Int);
            match method {
                "get" | "contains" => Some(vec![elem]),
                "join" => Some(vec![Type::String]),
                "fold" => Some(vec![
                    Type::Int, // init — sema refines
                    Type::Fn {
                        params: vec![Type::Int, elem],
                        ret: Some(Box::new(Type::Int)),
                        effect_bound: None, return_view_provenance: None,
                        param_contract: None,
                call_metadata: None,
                    },
                ]),
                "map" => Some(vec![Type::Fn {
                    params: vec![elem],
                    ret: None, // sema refines R from the closure's actual return
                    effect_bound: None, return_view_provenance: None,
                    param_contract: None,
                call_metadata: None,
                }]),
                _ => Some(vec![]),
            }
        }
        // D-DET1/D-DET-CAPAPI: injected capability method args. `pick`/`shuffle` are
        // generic ([T]) and handled element-aware in the checker dispatch — they are
        // NOT routed here.
        Type::Named(n) if n == crate::Syntax::CLOCK_TYPE => match method {
            "new" => Some(vec![Type::Int]),
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

/// Generated Rust receiver-borrow form for a builtin call. This is shared by
/// sema's call-loan timing and TIR lowering so the two cannot drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinReceiverBorrow {
    Read,
    TwoPhaseWrite,
    EagerWrite,
    Move,
}

pub fn builtin_receiver_borrow(recv_ty: &Type, method: &str) -> BuiltinReceiverBorrow {
    if is_iter_type(recv_ty) {
        BuiltinReceiverBorrow::Move
    } else if !builtin_method_mutates(recv_ty, method) {
        BuiltinReceiverBorrow::Read
    } else if (matches!(recv_ty, Type::List(_))
        && matches!(method, "remove" | "sort_by" | "edit_disjoint"))
        || matches!(
            recv_ty,
            Type::Named(name)
                if name == Syntax::CLOCK_TYPE
                    || name == Syntax::RNG_TYPE
                    || name == Syntax::SOLVER_TYPE
        )
    {
        // TIR lowers these through helpers whose first argument is an explicit
        // `&mut receiver`. Unlike Rust method-call syntax, that borrow is eager:
        // it must reject receiver reads while later arguments are evaluated.
        BuiltinReceiverBorrow::EagerWrite
    } else {
        // Ordinary Rust method-call syntax reserves `&mut self`, evaluates
        // arguments, then activates the exclusive borrow.
        BuiltinReceiverBorrow::TwoPhaseWrite
    }
}
