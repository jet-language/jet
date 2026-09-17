use crate::Codegen::Cx;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::struct_field_type;
use crate::Codegen::TIR::tir_recv_jet_ty;
use crate::Codegen::TIR::ListRemoveMode;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::TBuiltinOp;
use crate::Codegen::TIR::TClosureOp;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Diagnostics::Span;
use crate::AST::{Expr, IndexKind, Type};

fn invariant_expr(span: Span, construct: impl Into<String>) -> TExpr {
    TExpr {
        ty: Type::Named(crate::Syntax::TYPE_NEVER.to_string()),
        kind: TExprKind::InvariantViolation {
            construct: construct.into(),
            span,
        },
    }
}
fn base_receiver_ty(ty: &Type) -> &Type {
    match ty {
        Type::Tagged { inner, .. } => base_receiver_ty(inner),
        _ => ty,
    }
}


fn sequence_elem_ty(ty: &Type) -> Option<Type> {
    match ty {
        Type::Tagged { inner, .. } => sequence_elem_ty(inner),
        Type::List(inner) | Type::FixedList { elem: inner, .. } => Some((**inner).clone()),
        Type::Apply { name, args }
            if args.len() == 1
                && matches!(
                    name.as_str(),
                    crate::Syntax::TYPE_ITER
                        | crate::Syntax::TYPE_VIEW_ITER
                        | "View"
                        | "ViewMut"
                        | "ComputeViewMut"
                ) =>
        {
            Some(args[0].clone())
        }
        _ => None,
    }
}

fn list_receiver(ty: &Type) -> bool {
    match ty {
        Type::Tagged { inner, .. } => list_receiver(inner),
        Type::List(_) | Type::FixedList { .. } => true,
        _ => false,
    }
}

fn view_receiver(ty: &Type) -> bool {
    matches!(
        base_receiver_ty(ty),
        Type::Apply { name, args }
            if args.len() == 1
                && matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut")
    )
}
fn remove_mode_variant(variant: &str) -> Option<ListRemoveMode> {
    let variant = match variant {
        "Val" | "Slot" => variant,
        other => match other.strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX) {
            Some(short) => short,
            None => return None,
        },
    };
    match variant {
        "Val" => Some(ListRemoveMode::Value),
        "Slot" => Some(ListRemoveMode::Slot),
        _ => None,
    }
}

/// #1478: Set/Rank delegate their iterator-family surface (filter, map,
/// each, all, fold, flat_map, min, max) to the same List/Iter machinery every
/// other container already uses — insert the exact `.to_list()` conversion a
/// user would write by hand. AOT and JIT then never see a raw `HashSet`/
/// `BTreeSet` where they expect a `Vec`-backed list (I9: no second mechanism;
/// I8: one canonical iteration path). Not for `values` or collection removal,
/// which stay on the native Set API and must NOT be wrapped.
pub(crate) fn wrap_set_receiver_as_list(recv: TExpr, span: Span) -> TExpr {
    let (op, elem) = match &recv.ty {
        Type::Apply { name, args }
            if name == "Set" || name == crate::Syntax::TYPE_RANK =>
        {
            let Some(elem) = args.as_slice().first() else {
                return invariant_expr(span, "set receiver missing element type");
            };
            if args.len() != 1 {
                return invariant_expr(span, "set receiver has invalid generic arity");
            }
            (
                if name == "Set" {
                    TBuiltinOp::SetToList
                } else {
                    TBuiltinOp::SortedSetToList
                },
                elem.clone(),
            )
        }
        _ => return recv,
    };
    TExpr {
        ty: Type::List(Box::new(elem)),
        kind: TExprKind::BuiltinMethod {
            recv: Box::new(recv),
            op,
            args: vec![],
        },
    }
}

fn tuple_fields(ty: Option<&Type>) -> Option<Vec<(String, Type)>> {
    match ty {
        Some(Type::Tagged { inner, .. }) => tuple_fields(Some(inner.as_ref())),
        Some(Type::Tuple(fields)) => Some(
            fields
                .iter()
                .map(|(name, ty)| (name.clone(), (**ty).clone()))
                .collect(),
        ),
        _ => None,
    }
}

fn tuple_list_elem_fields(ty: Option<&Type>) -> Option<Vec<(String, Type)>> {
    let ty = ty?;
    let inner = sequence_elem_ty(ty)?;
    tuple_fields(Some(&inner))
}

fn option_tuple_fields(ty: Option<&Type>) -> Option<Vec<(String, Type)>> {
    match ty {
        Some(Type::Tagged { inner, .. }) => option_tuple_fields(Some(inner.as_ref())),
        Some(Type::Option(inner)) => tuple_fields(Some(inner.as_ref())),
        _ => None,
    }
}

fn result_tuple_fields(ty: Option<&Type>) -> Option<Vec<(String, Type)>> {
    match ty {
        Some(Type::Tagged { inner, .. }) => result_tuple_fields(Some(inner.as_ref())),
        Some(Type::Result { ok, .. }) => tuple_fields(Some(ok.as_ref())),
        _ => None,
    }
}

/// D-MEM1 S6: `pool[id].field`'s type — needed so a MUTATING method call on it
/// (`tree[root].children.push(child)`) is recognized as needing a real mutable
/// place (`builtin_needs_mut_receiver`), not the ordinary `jet_pool_get` value
/// clone. `tir_recv_jet_ty` has no `cx` to look up a struct field type, so this
/// is a separate small helper rather than a new arm there.
pub(crate) fn pool_field_ty_hint(e: &Expr, cx: &Cx, env: &LowerEnv) -> Option<Type> {
    let Expr::Field(base, field, _) = e else {
        return None;
    };
    let Expr::Index {
        base: pool_expr,
        kind: IndexKind::Pool,
        ..
    } = base.as_ref()
    else {
        return None;
    };
    let pool_ty = tir_recv_jet_ty(pool_expr, env)?;
    let elem_ty = match &pool_ty {
        Type::Apply { args, .. } if !args.is_empty() => args[0].clone(),
        _ => return None,
    };
    struct_field_type(cx, &elem_ty, field)
}

/// The declared type of a struct-FIELD receiver (`file.relative`,
/// `self.cfg.name`) — the one receiver shape `tir_recv_jet_ty` leaves
/// unresolved. Field reads nest, so a base that is itself a field resolves
/// through the same two steps. Sibling of `pool_field_ty_hint` above and for the
/// same reason: `tir_recv_jet_ty` has no `cx` with which to read a field's
/// declared type.
pub(crate) fn declared_field_ty(e: &Expr, cx: &Cx, env: &LowerEnv) -> Option<Type> {
    match e {
        Expr::Paren(inner, _) | Expr::Copy(inner, _) => declared_field_ty(inner, cx, env),
        Expr::Index { base, .. } => {
            let base_ty = tir_recv_jet_ty(base, env).or_else(|| declared_field_ty(base, cx, env))?;
            match base_ty {
                Type::List(inner) | Type::FixedList { elem: inner, .. } => Some(*inner),
                _ => None,
            }
        }
        Expr::Field(base, field, _) => {
            let base_ty =
                tir_recv_jet_ty(base, env).or_else(|| declared_field_ty(base, cx, env))?;
            struct_field_type(cx, &base_ty, field)
        }
        _ => None,
    }
}

/// The receiver type the built-in table dispatches on.
///
/// `tir_recv_jet_ty` answers `None` for a struct field read, so this helper
/// recovers the declaring field type before dispatch. A recovered type is
/// accepted only when the canonical collection table has the same method row;
/// an unrecoverable or mismatched type returns `None` rather than selecting a
/// guessed List operation. This closes the old String-field/List-surface hole
/// at the dispatch boundary, once for every shared method name.
///
/// Sema already resolved this receiver — it type-checked the call against
/// `Collections::builtin_method_arg_types(String, "replace")` — it just does not
/// persist the type: a builtin's `recv_type` stays `None`. So the field type is
/// re-read from the DECLARING tables (`struct_field_type`: user structs, then
/// every core record), and is trusted ONLY when the same builtin table sema used
/// has a row for `(that type, method, arity)`. If the exact field type cannot be
/// recovered, this resolver returns `None`; it never selects the legacy List arm.
///
/// Deliberately NOT folded into `tir_recv_jet_ty`: its other callers read that
/// partiality as a fact (the for-in `lines` split recognizes `child.stdout` by
/// its BASE, the mutable-place hint, the view-owner peeks), and this is the one
/// place a receiver type becomes an op.
fn builtin_recv_ty(
    receiver: &Expr,
    method: &str,
    nargs: usize,
    cx: &Cx,
    env: &LowerEnv,
) -> Option<Type> {
    let mut receiver = receiver;
    let mut unwrap_carrier = false;
    loop {
        match receiver {
            Expr::Paren(inner, _) | Expr::Copy(inner, _) => receiver = inner,
            Expr::Try(inner, ..) => {
                unwrap_carrier = true;
                receiver = inner;
            }
            _ => break,
        }
    }
    if let Some(ty) = tir_recv_jet_ty(receiver, env) {
        return Some(if unwrap_carrier {
            match ty {
                Type::Result { ok, .. } | Type::Option(ok) => {
                    crate::Codegen::TIR::builtin_dispatch_ty(*ok)
                }
                other => other,
            }
        } else {
            ty
        });
    }
    if let Expr::MethodCall {
        receiver: call_receiver,
        method: call_method,
        resolved_ret,
        ..
    } = receiver
    {
        // Fixed Core aliases (for example `files.read(path)`) are represented
        // as MethodCall nodes. Sema intentionally leaves monomorphic fixed
        // returns off that node because lowering reads the authoritative
        // signature table. Recover the carrier here so a following builtin
        // method sees its actual success type instead of the legacy List
        // fallback.
        if let Expr::Ident(alias, _) = call_receiver.as_ref() {
            if !env.locals.contains_key(alias) {
                if let Some(module) = cx.core_import_module_for_function(&env.fn_name, alias) {
                    let ty = resolved_ret.clone().or_else(|| {
                        crate::Sema::core_fixed_sig(module, call_method)
                            .and_then(|(_, ret)| ret)
                    });
                    if let Some(ty) = ty {
                        return Some(if unwrap_carrier {
                            match ty {
                                Type::Result { ok, .. } | Type::Option(ok) => {
                                    crate::Codegen::TIR::builtin_dispatch_ty(*ok)
                                }
                                other => other,
                            }
                        } else {
                            ty
                        });
                    }
                }
            }
        }
    }
    let ty = crate::Codegen::TIR::builtin_dispatch_ty(declared_field_ty(receiver, cx, env)?);
    if crate::Collections::builtin_method_return(&ty, method, nargs, false).is_some() {
        return Some(ty);
    }
    None
}

/// D-MEM1 stage S5: lower a `Binding.string_view` init (`s.trim()` /
/// `s.after(sep)` / `s.before(sep)`) to the borrowed `&str`-returning
/// `TBuiltinOp::{TrimView,AfterView,BeforeView}` — the zero-copy sibling of
/// whatever `resolve_builtin_op` would pick for the same call written
/// somewhere the result isn't scope-tracked. The return type comes from the
/// canonical String method table; no shape or type is guessed here.
pub(super) fn lower_string_view_init(init: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    let Expr::MethodCall {
        receiver,
        method,
        method_span,
        args,
        resolved_ret,
        ..
    } = init
    else {
        return invariant_expr(init.span(), "string-view initializer shape");
    };
    let op = match method.as_str() {
        "trim" => TBuiltinOp::TrimView,
        "after" => TBuiltinOp::AfterView,
        "before" => TBuiltinOp::BeforeView,
        _ => return invariant_expr(*method_span, "string-view method"),
    };
    let Some(table_ret) =
        crate::Collections::builtin_method_return(&Type::String, method, args.len(), false)
            .flatten()
    else {
        return invariant_expr(*method_span, "string-view signature");
    };
    let ty = match resolved_ret {
        Some(ret) if ret == &table_ret => ret.clone(),
        Some(_) => return invariant_expr(*method_span, "string-view return type"),
        None => table_ret,
    };
    let recv = lower_expr(receiver, cx, env);
    if !matches!(recv.ty, Type::String) {
        return invariant_expr(*method_span, "string-view receiver type");
    }
    let targs = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
    TExpr {
        ty,
        kind: TExprKind::BuiltinMethod {
            recv: Box::new(recv),
            op,
            args: targs,
        },
    }
}

/// c109 Phase 9: resolve the built-in method op from the method name, arg count, and
/// the receiver's resolved type. The Map-vs-List branch (`insert`/`remove`/`get`)
/// and the String-vs-List branch (`len`/`replace`/…) both come from `rty`, so the
/// receiver type has to be the real one: see `builtin_recv_ty` above for why a
/// struct-field receiver used to arrive here untyped and take the List surface.
/// `receiver_ty` is the already-lowered receiver type when the AST shape (notably
/// a list literal) does not retain one. Returns `None` until another analytical
/// lowerer handles an alternate shape, or when a checked shape is inconsistent.
pub(crate) fn resolve_builtin_op(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    args: &[crate::AST::CallArg],
    resolved_ret: Option<&Type>,
    receiver_ty: Option<&Type>,
    env: &LowerEnv,
    cx: &Cx,
) -> Option<TBuiltinOp> {
    let rty = receiver_ty
        .cloned()
        .map(crate::Codegen::TIR::builtin_dispatch_ty)
        .or_else(|| builtin_recv_ty(receiver, method, args.len(), cx, env));
    if rty.is_none() {
        return None;
    }
    if rty.as_ref().is_some_and(|ty| {
        matches!(
            ty,
            Type::Apply {
                name,
                args: generic_args
            } if name == "Atomic"
                && generic_args.len() == 1
                && crate::Collections::builtin_method_return(ty, method, args.len(), false)
                    .is_some()
        )
    }) {
        return Some(TBuiltinOp::AtomicMethod {
            method: method.to_string(),
        });
    }
    if matches!(&rty, Some(Type::Named(name)) if name == crate::Syntax::TYPE_ORDERING) {
        return match (method, args.len()) {
            ("then", 1) => Some(TBuiltinOp::OrderingThen),
            ("reverse", 0) => Some(TBuiltinOp::OrderingReverse),
            _ => None,
        };
    }
    let is_byte_buffer =
        matches!(&rty, Some(Type::Named(name)) if name == crate::Syntax::TYPE_BYTES);
    // Bytes.position() is a 0-arg cursor read — must win before the
    // Iter.position(pred) closure-method early-out.
    if is_byte_buffer {
        let op = match (method, args.len()) {
            (
                "write_u8" | "write_byte" | "write_i8" | "write_u16_le" | "write_u16_be"
                | "write_i16_le" | "write_i16_be" | "write_u32_le" | "write_u32_be"
                | "write_i32_le" | "write_i32_be" | "write_u64_le" | "write_u64_be"
                | "write_i64_le" | "write_i64_be" | "write_f32_le" | "write_f32_be"
                | "write_f64_le" | "write_f64_be" | "write_bytes" | "write",
                1,
            ) => TBuiltinOp::ByteBufferWrite {
                method: method.to_string(),
            },
            ("to_bytes", 0) => TBuiltinOp::ByteBufferToBytes,
            (
                "len" | "is_empty" | "clear" | "capacity" | "position" | "eof" | "rewind" | "flush"
                | "close" | "shutdown" | "get_buffer" | "buffer" | "to_string" | "string" | "trim"
                | "trim_start" | "trim_end" | "to_lower" | "to_upper" | "to_title" | "title"
                | "clone" | "copy" | "lines" | "first" | "next" | "read_byte" | "read" | "is_ascii"
                | "parse",
                0,
            )
            | (
                "get" | "seek" | "read_bytes" | "read_string" | "contains" | "starts_with"
                | "ends_with" | "index_of" | "last_index_of" | "split" | "join" | "equal"
                | "compare" | "copy_to" | "write_to",
                1,
            )
            | ("replace", 2) => TBuiltinOp::ByteBufferMethod {
                method: method.to_string(),
            },
            _ => return None,
        };
        let emitted_borrow = match &op {
            TBuiltinOp::ByteBufferWrite { .. } => {
                crate::Collections::BuiltinReceiverBorrow::TwoPhaseWrite
            }
            TBuiltinOp::ByteBufferMethod { method }
                if matches!(
                    method.as_str(),
                    "clear"
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
                        | "write_to"
                ) =>
            {
                crate::Collections::BuiltinReceiverBorrow::TwoPhaseWrite
            }
            TBuiltinOp::ByteBufferToBytes => crate::Collections::BuiltinReceiverBorrow::Read,
            TBuiltinOp::ByteBufferMethod { .. } => crate::Collections::BuiltinReceiverBorrow::Read,
            _ => return None,
        };
        if let Some(receiver_borrow) = rty
            .as_ref()
            .map(|ty| crate::Collections::builtin_receiver_borrow(ty, method))
        {
            if receiver_borrow != emitted_borrow {
                return None;
            }
        }
        return Some(op);
    }
    let is_option = matches!(rty, Some(Type::Option(_)));
    if rty.as_ref().is_some_and(|ty| {
        crate::Collections::builtin_method_return(ty, method, args.len(), false).is_none()
    }) && !(is_option && method == "zip" && args.len() == 1)
    {
        return None;
    }
    if crate::Collections::is_closure_method(method) {
        return None;
    }
    let is_string = matches!(rty, Some(Type::String));
    let is_list = rty.as_ref().is_some_and(list_receiver);
    let is_map = matches!(rty, Some(Type::Map { .. }));
    let is_set = matches!(&rty, Some(Type::Apply { name, .. }) if name == "Set");
    let is_sorted_set =
        matches!(&rty, Some(Type::Apply { name, .. }) if name == crate::Syntax::TYPE_RANK);
    let is_priority_queue = matches!(&rty, Some(Type::Apply { name, .. }) if name == crate::Syntax::TYPE_PRIORITY_QUEUE);
    let is_lru = matches!(&rty, Some(Type::Apply { name, .. }) if name == crate::Syntax::TYPE_LRU);
    let is_bag =
        matches!(&rty, Some(Type::Apply { name, .. }) if name == crate::Syntax::TYPE_TALLY);
    let is_bit_set = matches!(&rty, Some(Type::Named(name)) if name == crate::Syntax::TYPE_BITS);
    let is_deque =
        matches!(&rty, Some(Type::Apply { name, .. }) if name == crate::Syntax::TYPE_QUEUE);
    let is_iter = matches!(
        &rty,
        Some(ty) if crate::Collections::is_iter_type(ty)
    );
    let is_float_sequence = matches!(
        &rty,
        Some(Type::List(elem) | Type::FixedList { elem, .. })
            if matches!(elem.as_ref(), Type::Float | Type::Float32)
    ) || matches!(
        &rty,
        Some(Type::Apply { name, args })
            if name == crate::Syntax::TYPE_ITER
                && args.len() == 1
                && matches!(args[0], Type::Float | Type::Float32)
    ) || matches!(
        resolved_ret,
        Some(Type::Option(elem)) if matches!(elem.as_ref(), Type::Float | Type::Float32)
    );
    // D-HOLE1: `.zip` on `T?` (vs. `[T].zip`).
    let receiver_borrow = rty
        .as_ref()
        .map(|ty| crate::Collections::builtin_receiver_borrow(ty, method));
    let list_remove_mode = match (method, args.len()) {
        ("remove", 1) => Some(ListRemoveMode::Value),
        ("remove", 2) => match args.get(1).map(|arg| &arg.expr) {
            Some(Expr::EnumLit { variant, .. }) => remove_mode_variant(variant),
            Some(Expr::Ident(name, _)) => match cx.const_values.get(name) {
                Some(crate::AST::CtValue::Enum {
                    type_name, variant, ..
                }) if type_name == crate::Syntax::TYPE_REMOVE_BY => {
                    remove_mode_variant(variant)
                }
                _ => match &rty {
                    Some(Type::List(inner)) if **inner == Type::Int => {
                        Some(ListRemoveMode::Dynamic)
                    }
                    _ => None,
                },
            },
            _ => match &rty {
                Some(Type::List(inner)) if **inner == Type::Int => {
                    Some(ListRemoveMode::Dynamic)
                }
                _ => None,
            },
        },
        _ => None,
    };
    let op = match (method, args.len()) {
        ("len", 0) => {
            if is_string {
                TBuiltinOp::LenString
            } else if is_bag {
                TBuiltinOp::BagLen
            } else {
                TBuiltinOp::LenList
            }
        }
        ("is_empty", 0) => TBuiltinOp::IsEmpty,
        ("try_push", 1) if is_string => TBuiltinOp::TryStringPush,
        ("try_push", 1) if is_list => TBuiltinOp::TryPush,
        ("try_reserve", 1) if is_list => TBuiltinOp::TryReserve,
        ("try_insert", 2) if is_map => TBuiltinOp::TryInsertMap,
        ("push", 1) => TBuiltinOp::Push,
        ("pop", 0) if is_priority_queue => TBuiltinOp::PriorityQueuePop,
        ("pop", 0) => TBuiltinOp::Pop,
        ("insert", 2) => TBuiltinOp::InsertList,
        ("add", 2) if is_map => TBuiltinOp::InsertMap,
        ("add_new", 2) if is_map => TBuiltinOp::AddNewMap,
        ("merge", 1) if is_map => TBuiltinOp::MapMerge,
        ("merge", 2) if is_map => TBuiltinOp::MapMergeWith,
        ("pop", 1) if is_set => TBuiltinOp::SetPop,
        ("pop", 1) if is_map || is_lru => TBuiltinOp::RemoveMap,
        ("pop_first", 0) if is_map => TBuiltinOp::MapPopFirst,
        ("contains_value", 1) if is_map => TBuiltinOp::MapContainsValue,
        ("remove", 1 | 2) => {
            if is_set {
                TBuiltinOp::SetRemove
            } else if is_sorted_set {
                TBuiltinOp::SortedSetRemove
            } else if is_bit_set {
                TBuiltinOp::BitSetRemove
            } else if is_bag {
                TBuiltinOp::BagRemove
            } else if is_map {
                TBuiltinOp::RemoveMap
            } else if is_lru {
                TBuiltinOp::RemoveMap
            } else if is_priority_queue {
                let Some(mode) = list_remove_mode else {
                    return None;
                };
                // D-LISTREMOVE1/F: PriorityQueue reuses List's exact selector
                // shape and panic-line convention (criterion c6 on #1481).
                let line = crate::Diagnostics::span_line_col(&cx.src, method_span.start).0;
                TBuiltinOp::PriorityQueueRemove { line, mode }
            } else if is_list {
                let Some(mode) = list_remove_mode else {
                    return None;
                };
                // The list form embeds the *method-span* line for its bounds panic,
                // exactly as `emit_builtin_method` reads `span_line_col(method_span.start)`.
                let line = crate::Diagnostics::span_line_col(&cx.src, method_span.start).0;
                TBuiltinOp::RemoveList { line, mode }
            } else {
                return None;
            }
        }
        ("get", 1) => {
            if is_deque {
                TBuiltinOp::DequeGet
            } else if is_lru {
                TBuiltinOp::LruGet
            } else if is_map {
                TBuiltinOp::GetMap
            } else {
                TBuiltinOp::GetList
            }
        }
        ("first", 0) if is_map => TBuiltinOp::MapFirst,
        ("first", 0) if is_set => TBuiltinOp::SetFirst,
        ("first", 0) if is_sorted_set => TBuiltinOp::First,
        ("last", 0) if is_sorted_set => TBuiltinOp::Last,
        ("first", 0) => TBuiltinOp::First,
        ("last", 0) => TBuiltinOp::Last,
        ("contains", 1) if is_deque => TBuiltinOp::DequeContains,
        ("contains", 1) => TBuiltinOp::Contains,
        ("has", 1) if is_set || is_sorted_set || is_bit_set => TBuiltinOp::Contains,
        ("index_of", 1) if is_string => TBuiltinOp::StringIndexOf,
        ("index_of", 1) => TBuiltinOp::IndexOf,
        ("reverse", 0) if is_deque => TBuiltinOp::DequeReverse,
        ("reverse", 0) if is_string => TBuiltinOp::StringMethod {
            method: "reverse".to_string(),
        },
        ("reverse", 0) => TBuiltinOp::Reverse,
        // D-SET-DECLINE1=C: guard ahead of the unconditional List `sort` arm
        // below — Set.sort() returns a fresh List instead of mutating in place.
        ("sort", 0) if is_set => TBuiltinOp::SetSort,
        ("sort", 0) => TBuiltinOp::Sort,
        ("sort_desc", 0) => TBuiltinOp::SortDesc,
        ("join", 1) if is_deque => TBuiltinOp::DequeJoin,
        ("join", 1) => TBuiltinOp::JoinSep,
        ("split", 1) if is_deque => TBuiltinOp::DequeSplit,
        ("split", 1) if is_string => TBuiltinOp::Split,
        ("split", 1) => {
            let fields = match resolved_ret {
                Some(ret) => tuple_fields(Some(ret))?,
                None => {
                    let elem = sequence_elem_ty(rty.as_ref()?)?;
                    let list_ty = Type::List(Box::new(elem));
                    vec![
                        ("left".to_string(), list_ty.clone()),
                        ("right".to_string(), list_ty),
                    ]
                }
            };
            TBuiltinOp::IterSplit {
                tuple_struct: crate::Codegen::Tuples::tuple_struct_name(&fields),
            }
        }
        ("repeat", 1) if is_string => TBuiltinOp::Repeat,
        // List literals leave `rty` None — same pattern as Take/Dedup.
        ("repeat", 1) => TBuiltinOp::IterRepeat,
        ("cycle", 1) => TBuiltinOp::IterCycle,
        ("drop_last", 1) => TBuiltinOp::IterDropLast,
        // D-SET-DECLINE1=C: guard ahead of the unconditional Iter `shuffle`
        // arm below — Set.shuffle() returns a fresh List, same as Set.sort().
        ("shuffle", 0) if is_set => TBuiltinOp::SetShuffle,
        ("shuffle", 0) => TBuiltinOp::IterShuffle,
        ("is_sorted", 0) => TBuiltinOp::IterIsSorted,
        ("last_index_of", 1) if is_string => TBuiltinOp::StringMethod {
            method: "last_index_of".to_string(),
        },
        ("last_index_of", 1) => TBuiltinOp::IterLastIndexOf,
        ("average", 0) => TBuiltinOp::IterAverage {
            float: is_float_sequence,
        },
        ("compare", 1) if is_string => TBuiltinOp::StringMethod {
            method: "compare".to_string(),
        },
        ("compare", 1) => TBuiltinOp::IterCompare,
        ("to_set", 0) if is_set => TBuiltinOp::SetCopy,
        ("to_set", 0) => TBuiltinOp::SetFrom,
        ("sum", 0) => TBuiltinOp::Sum {
            float: matches!(resolved_ret, Some(Type::Float | Type::Float32)),
            f32: matches!(resolved_ret, Some(Type::Float32)),
        },
        ("product", 0) => TBuiltinOp::Product {
            float: matches!(resolved_ret, Some(Type::Float | Type::Float32)),
        },
        ("min", 0) if is_map => TBuiltinOp::MapMin,
        ("max", 0) if is_map => TBuiltinOp::MapMax,
        ("min", 0) => TBuiltinOp::Min {
            float: is_float_sequence,
        },
        ("max", 0) => TBuiltinOp::Max {
            float: is_float_sequence,
        },
        // D-CORE-EAGER2=A: emit/eval split this op by List vs Iter receiver.
        ("flatten", 0) => TBuiltinOp::Flatten,
        ("intersperse", 1) => TBuiltinOp::Intersperse,
        ("clear", 0) => TBuiltinOp::Clear,
        ("chars", 0) => TBuiltinOp::Chars,
        ("bytes", 0) => TBuiltinOp::Bytes { owned: false },
        ("trim", 0) => TBuiltinOp::Trim,
        ("trim_start", 0) => TBuiltinOp::TrimStart,
        ("trim_end", 0) => TBuiltinOp::TrimEnd,
        // c97/D-STRPARSE1: String-only `lines`; parsing stays `Type.parse`.
        ("lines", 0) => TBuiltinOp::Lines,
        ("starts_with", 1) => TBuiltinOp::StartsWith,
        ("ends_with", 1) => TBuiltinOp::EndsWith,
        ("replace", 2) if is_string => TBuiltinOp::StringMethod {
            method: "replace".to_string(),
        },
        ("replace", 2) if is_list || rty.is_none() => TBuiltinOp::ListReplace,
        ("replace", 2) => TBuiltinOp::Replace,
        ("pad_start", 2) => TBuiltinOp::PadStart,
        ("pad_end", 2) => TBuiltinOp::PadEnd,
        ("count", 1) if is_list => TBuiltinOp::CountList,
        ("count", 1) if is_string => TBuiltinOp::StringCount,
        ("counts", 0) if is_list || matches!(&rty, Some(Type::FixedList { .. })) || is_iter => {
            TBuiltinOp::Counts
        }
        ("extend", 1) if is_list => TBuiltinOp::ExtendList,
        // D-TYPE2-MEASURE1=A: fixed lists join through the same builtin; emit
        // picks the const-generic fixed shape from the FixedList types.
        ("concat", 1) if is_list || matches!(&rty, Some(Type::FixedList { .. })) => {
            TBuiltinOp::ConcatList
        }
        ("is_alphabetic", 0) if is_string => TBuiltinOp::StringIsAlphabetic,
        ("is_numeric", 0) if is_string => TBuiltinOp::StringIsNumeric,
        ("is_whitespace", 0) if is_string => TBuiltinOp::StringIsWhitespace,
        ("is_ascii", 0) if is_string => TBuiltinOp::StringIsAscii,
        ("to_title", 0) if is_string => TBuiltinOp::StringToTitle,
        (
            "count_bytes" | "is_lower" | "is_upper" | "capitalize" | "swapcase" | "copy"
            | "normalize",
            0,
        ) if is_string =>
        {
            TBuiltinOp::StringMethod {
                method: method.to_string(),
            }
        }
        ("remove_prefix" | "remove_suffix" | "equal" | "rsplit" | "matches" | "match", 1)
            if is_string =>
        {
            TBuiltinOp::StringMethod {
                method: method.to_string(),
            }
        }
        // D-STR-DECLINE1=C: `s.to_int()`/`s.to_float()` are the same builtin
        // `Int.parse(s)`/`Float.parse(s)` already lower to (D-STRPARSE1) — the
        // string is the receiver either way, so the op is reused verbatim.
        ("to_int", 0) if is_string => TBuiltinOp::ParseInt,
        ("to_float", 0) if is_string => TBuiltinOp::ParseFloat,
        ("split_once", 1) if is_string => {
            let fields = if let Some(ret) = resolved_ret {
                option_tuple_fields(Some(ret))?
            } else {
                let ret = crate::Collections::builtin_method_return(
                    &Type::String,
                    method,
                    args.len(),
                    false,
                )
                .flatten()?;
                option_tuple_fields(Some(&ret))?
            };
            TBuiltinOp::StringSplitOnce {
                tuple_struct: crate::Codegen::Tuples::tuple_struct_name(&fields),
            }
        }
        ("cut_last", 1) if is_string => {
            let fields = if let Some(ret) = resolved_ret {
                option_tuple_fields(Some(ret))?
            } else {
                let ret = crate::Collections::builtin_method_return(
                    &Type::String,
                    method,
                    args.len(),
                    false,
                )
                .flatten()?;
                option_tuple_fields(Some(&ret))?
            };
            TBuiltinOp::StringCutLast {
                tuple_struct: crate::Codegen::Tuples::tuple_struct_name(&fields),
            }
        }
        // D-STR-AFTER1: `.after(sep)`/`.before(sep)` — first-occurrence substring split.
        ("after", 1) => TBuiltinOp::After,
        ("before", 1) => TBuiltinOp::Before,
        ("to_upper", 0) => TBuiltinOp::ToUpper,
        ("to_lower", 0) => TBuiltinOp::ToLower,
        ("to_ascii_upper", 0) if is_string => TBuiltinOp::ToAsciiUpper,
        ("to_ascii_lower", 0) if is_string => TBuiltinOp::ToAsciiLower,
        ("slice", 2) if is_string => {
            let line = crate::Diagnostics::span_line_col(&cx.src, receiver.span().start).0;
            TBuiltinOp::Slice { line }
        }
        ("slice", 2) => TBuiltinOp::ListSlice,
        ("slice", 1) if is_map => TBuiltinOp::MapSliceKeys,
        ("copy", 0) if is_map => TBuiltinOp::MapCopy,
        ("copy", 0) if is_set => TBuiltinOp::SetCopy,
        ("copy", 0) if is_bit_set => TBuiltinOp::BitSetCopy,
        ("copy", 0) => TBuiltinOp::ListCopy,
        ("equal", 1) if is_map => TBuiltinOp::MapEqual,
        ("equal", 1) if is_set => TBuiltinOp::SetEqual,
        ("equal", 1) => TBuiltinOp::ListEqual,
        ("binary_search", 1) => TBuiltinOp::ListBinarySearch,
        ("union", 1) if is_sorted_set => TBuiltinOp::SortedSetUnion,
        ("union", 1) if is_set => TBuiltinOp::SetUnion,
        ("union", 1) => TBuiltinOp::ListUnion,
        ("intersection", 1) if is_sorted_set => TBuiltinOp::SortedSetIntersection,
        ("intersection", 1) if is_set => TBuiltinOp::SetIntersection,
        ("intersection", 1) if is_map => TBuiltinOp::MapIntersection,
        ("intersection", 1) => TBuiltinOp::ListIntersection,
        ("difference", 1) if is_sorted_set => TBuiltinOp::SortedSetDifference,
        ("difference", 1) if is_set => TBuiltinOp::SetDifference,
        ("difference", 1) => TBuiltinOp::ListDifference,
        ("random", 0) => TBuiltinOp::ListRandom,
        ("min_max", 0) => {
            let fields = match resolved_ret {
                Some(ret) => option_tuple_fields(Some(ret))?,
                None => {
                    let elem = sequence_elem_ty(rty.as_ref()?)?;
                    vec![("min".to_string(), elem.clone()), ("max".to_string(), elem)]
                }
            };
            TBuiltinOp::ListMinMax {
                tuple_struct: crate::Codegen::Tuples::tuple_struct_name(&fields),
            }
        }
        ("to_list", 0) if is_map => {
            let fields = match resolved_ret {
                Some(ret) => tuple_list_elem_fields(Some(ret))?,
                None => {
                    let Some(Type::Map { key, value, .. }) = rty.as_ref() else {
                        return None;
                    };
                    vec![
                        ("key".to_string(), (**key).clone()),
                        ("value".to_string(), (**value).clone()),
                    ]
                }
            };
            TBuiltinOp::MapToList {
                tuple_struct: crate::Codegen::Tuples::tuple_struct_name(&fields),
            }
        }
        ("top_n", 1) if is_map => {
            let fields = match resolved_ret {
                Some(ret) => tuple_list_elem_fields(Some(ret))?,
                None => {
                    let Some(Type::Map { key, value, .. }) = rty.as_ref() else {
                        return None;
                    };
                    vec![
                        ("key".to_string(), (**key).clone()),
                        ("value".to_string(), (**value).clone()),
                    ]
                }
            };
            TBuiltinOp::MapTopN {
                tuple_struct: crate::Codegen::Tuples::tuple_struct_name(&fields),
            }
        }
        // D-DYNARRAY1: `list.view(a..b)`
        ("view", 2) => {
            let line = crate::Diagnostics::span_line_col(&cx.src, receiver.span().start).0;
            TBuiltinOp::ViewNew { line }
        }
        ("split_write", 1) => {
            let fields = match resolved_ret {
                Some(ret) => result_tuple_fields(Some(ret))?,
                None => {
                    let elem = sequence_elem_ty(rty.as_ref()?)?;
                    let view = Type::Apply {
                        name: "ViewMut".to_string(),
                        args: vec![elem],
                    };
                    vec![
                        ("left".to_string(), view.clone()),
                        ("right".to_string(), view),
                    ]
                }
            };
            TBuiltinOp::SplitWrite {
                tuple_struct: crate::Codegen::Tuples::tuple_struct_name(&fields),
            }
        }
        ("get_disjoint_write", 1) => TBuiltinOp::GetDisjointWrite,
        ("keys", 0) if is_lru => TBuiltinOp::LruKeys,
        ("keys", 0) => TBuiltinOp::Keys,
        // #1478: `Set.values()` is the lazy Set-native alias of `to_list`;
        // must precede the Map-generic `Values` fallback below.
        ("values", 0) if is_set => TBuiltinOp::SetValues,
        ("values", 0) => TBuiltinOp::Values,
        ("has_key", 1) => TBuiltinOp::ContainsKey,
        ("to_string", 0) => TBuiltinOp::ToString,
        // D-ITER1: non-closure list adapters.
        ("take", 1) => TBuiltinOp::Take,
        ("skip", 1) => TBuiltinOp::Skip,
        ("step_by", 1) => TBuiltinOp::StepBy,
        ("dedup", 0) => TBuiltinOp::Dedup,
        ("chunks", 1) => TBuiltinOp::Chunks,
        ("windows", 1) => TBuiltinOp::Windows,
        ("indexed", 0) => {
            let fields = match resolved_ret {
                Some(ret) => tuple_list_elem_fields(Some(ret))?,
                None => {
                    let elem_ty = sequence_elem_ty(rty.as_ref()?)?;
                    vec![
                        ("idx".to_string(), Type::Int),
                        ("item".to_string(), elem_ty),
                    ]
                }
            };
            let ts = crate::Codegen::Tuples::tuple_struct_name(&fields);
            TBuiltinOp::Indexed { tuple_struct: ts }
        }
        ("indexes", 0) => TBuiltinOp::Indexes,
        ("zip", 1) if is_option => {
            // D-HOLE1: `a: T?`.zip(`b: U?`) -> `(a: T, b: U)?`.
            let fields = option_tuple_fields(resolved_ret)?;
            let ts = crate::Codegen::Tuples::tuple_struct_name(&fields);
            let elem_ty = Type::Tuple(
                fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), Box::new(ty.clone())))
                    .collect(),
            );
            TBuiltinOp::OptionZip {
                tuple_struct: ts,
                elem_ty,
            }
        }
        ("zip", 1) => {
            // Build the tuple struct name for `(a: T, b: U)`. Sema has already
            // refined both element types in the resolved return.
            let fields = tuple_list_elem_fields(resolved_ret)?;
            let ts = crate::Codegen::Tuples::tuple_struct_name(&fields);
            let field_types = fields.iter().map(|(_, ty)| ty.clone()).collect();
            TBuiltinOp::Zip {
                tuple_struct: ts,
                mode: crate::Codegen::TIR::TZipMode::Strict,
                fields: fields.into_iter().map(|(name, _)| name).collect(),
                flatten: false,
                input_count: 2,
                fill_mode: crate::Codegen::TIR::TZipFillMode::DefaultNone,
                field_types,
            }
        }
        ("unzip", 0) => {
            let fields = match resolved_ret {
                Some(ret) => tuple_fields(Some(ret))?,
                None => {
                    let pair = sequence_elem_ty(rty.as_ref()?)?;
                    let pair_fields = tuple_fields(Some(&pair))?;
                    let a_ty = pair_fields
                        .iter()
                        .find(|(name, _)| name == "a")
                        .map(|(_, ty)| ty.clone())?;
                    let b_ty = pair_fields
                        .iter()
                        .find(|(name, _)| name == "b")
                        .map(|(_, ty)| ty.clone())?;
                    vec![
                        ("a".to_string(), Type::List(Box::new(a_ty))),
                        ("b".to_string(), Type::List(Box::new(b_ty))),
                    ]
                }
            };
            let ts = crate::Codegen::Tuples::tuple_struct_name(&fields);
            TBuiltinOp::Unzip { tuple_struct: ts }
        }
        // D-FAILCOMP1: try_collect on [Result<T,E>] → Result<[T],E>.
        ("try_collect", 0) => TBuiltinOp::TryCollect,
        // D-COLLBREADTH1=A: Set<T> instance methods.
        ("add", 1) if is_set => TBuiltinOp::SetInsert,
        ("add", 1) if is_sorted_set => TBuiltinOp::SortedSetInsert,
        ("add", 1) if is_bit_set => TBuiltinOp::BitSetAdd,
        ("add", 1) if is_bag => TBuiltinOp::BagAdd,
        ("peek", 0) if is_priority_queue => TBuiltinOp::PriorityQueuePeek,
        ("to_sorted_list", 0) if is_priority_queue => TBuiltinOp::PriorityQueueToSortedList,
        ("to_list", 0) if is_iter => TBuiltinOp::IterToList,
        ("collect", 0) if is_iter => TBuiltinOp::IterCollect,
        ("lazy", 0) if is_list || matches!(rty, Some(Type::FixedList { .. })) => {
            TBuiltinOp::ListLazy
        }
        ("to_list", 0) if is_deque => TBuiltinOp::DequeToList,
        ("to_list", 0) if is_sorted_set => TBuiltinOp::SortedSetToList,
        ("to_list", 0) if is_bit_set => TBuiltinOp::BitSetToList,
        ("to_list", 0) => TBuiltinOp::SetToList,
        // removed duplicate #1477
        ("symmetric_difference", 1) if is_sorted_set => TBuiltinOp::SortedSetSymmetricDifference,
        ("is_subset", 1) if is_sorted_set => TBuiltinOp::SortedSetIsSubset,
        ("is_superset", 1) if is_sorted_set => TBuiltinOp::SortedSetIsSuperset,
        ("is_disjoint", 1) if is_sorted_set => TBuiltinOp::SortedSetIsDisjoint,
        // removed duplicate #1477
        ("symmetric_difference", 1) if is_set => TBuiltinOp::SetSymmetricDifference,
        ("is_subset", 1) if is_set => TBuiltinOp::SetIsSubset,
        ("is_superset", 1) if is_set => TBuiltinOp::SetIsSuperset,
        ("is_disjoint", 1) if is_set => TBuiltinOp::SetIsDisjoint,
        // removed duplicate #1477
        ("capacity", 0) if is_set => TBuiltinOp::SetCapacity,
        // D-ONCE-VERB1=A: Set has one remove-and-return spelling.
        ("add", 2) if is_lru => TBuiltinOp::LruPut,
        ("add_new", 2) if is_lru => TBuiltinOp::LruAddNew,
        ("capacity", 0) if is_lru => TBuiltinOp::LruCapacity,
        ("count", 0) if is_bit_set => TBuiltinOp::BitSetCount,
        (
            "write_u8" | "write_i8" | "write_u16_le" | "write_u16_be" | "write_i16_le"
            | "write_i16_be" | "write_u32_le" | "write_u32_be" | "write_i32_le" | "write_i32_be"
            | "write_u64_le" | "write_u64_be" | "write_i64_le" | "write_i64_be" | "write_f32_le"
            | "write_f32_be" | "write_f64_le" | "write_f64_be" | "write_bytes",
            1,
        ) if is_byte_buffer => TBuiltinOp::ByteBufferWrite {
            method: method.to_string(),
        },
        ("to_bytes", 0) if is_byte_buffer => TBuiltinOp::ByteBufferToBytes,
        ("has", 1) if is_bag => TBuiltinOp::BagHas,
        ("count", 1) if is_bag => TBuiltinOp::BagCount,
        // D-COLLBREADTH1=A: Queue<T> instance methods.
        ("push_front", 1) if is_deque => TBuiltinOp::DequePushFront,
        ("push_back", 1) if is_deque => TBuiltinOp::DequePushBack,
        ("pop_front", 0) if is_deque => TBuiltinOp::DequePopFront,
        ("pop_back", 0) if is_deque => TBuiltinOp::DequePopBack,
        ("peek_front", 0) if is_deque => TBuiltinOp::DequePeekFront,
        ("peek_back", 0) if is_deque => TBuiltinOp::DequePeekBack,
        ("capacity", 0) if is_deque => TBuiltinOp::DequeCapacity,
        ("delete", 1) if is_deque => TBuiltinOp::DequeDelete,
        _ => return None,
    };
    let emitted_borrow = if is_iter {
        crate::Collections::BuiltinReceiverBorrow::Move
    } else {
        match &op {
            // Explicit helper call: `jet_list_remove(&mut receiver, ...)`.
            TBuiltinOp::RemoveList { .. } => crate::Collections::BuiltinReceiverBorrow::EagerWrite,
            TBuiltinOp::SortDesc => crate::Collections::BuiltinReceiverBorrow::EagerWrite,
            // Native method syntax receives Rust's two-phase `&mut self`.
            TBuiltinOp::Push
            | TBuiltinOp::TryPush
            | TBuiltinOp::TryReserve
            | TBuiltinOp::TryInsertMap
            | TBuiltinOp::TryStringPush
            | TBuiltinOp::Pop
            | TBuiltinOp::InsertMap
            | TBuiltinOp::AddNewMap
            | TBuiltinOp::InsertList
            | TBuiltinOp::RemoveMap
            | TBuiltinOp::MapPopFirst
            | TBuiltinOp::ExtendList
            | TBuiltinOp::Reverse
            | TBuiltinOp::Sort
            | TBuiltinOp::Clear
            | TBuiltinOp::SetInsert
            | TBuiltinOp::SetRemove
            | TBuiltinOp::SetPop
            | TBuiltinOp::PriorityQueuePop
            | TBuiltinOp::SortedSetInsert
            | TBuiltinOp::SortedSetRemove
            | TBuiltinOp::BitSetAdd
            | TBuiltinOp::BitSetRemove
            | TBuiltinOp::BagAdd
            | TBuiltinOp::BagRemove
            | TBuiltinOp::LruPut
            | TBuiltinOp::LruAddNew
            | TBuiltinOp::LruGet
            | TBuiltinOp::ByteBufferWrite { .. }
            | TBuiltinOp::DequePushFront
            | TBuiltinOp::DequePushBack
            | TBuiltinOp::DequePopFront
            | TBuiltinOp::DequePopBack
            | TBuiltinOp::DequeDelete
            | TBuiltinOp::DequeReverse
            | TBuiltinOp::DequeSplit
            | TBuiltinOp::SplitWrite { .. }
            | TBuiltinOp::GetDisjointWrite => {
                crate::Collections::BuiltinReceiverBorrow::TwoPhaseWrite
            }
            _ => crate::Collections::BuiltinReceiverBorrow::Read,
        }
    };
    if let Some(receiver_borrow) = receiver_borrow {
        if receiver_borrow != emitted_borrow {
            return None;
        }
    }
    Some(op)
}

/// c109 Phase 9: the resolved return type of a built-in collection/string method,
/// from `Collections::builtin_method_return` (the sema table). `Some(Unit)` means
/// the canonical row is a genuinely void method; `None` means no canonical row.


/// c109 Phase 11: resolve a closure-taking collection method into a total
/// `TClosureOp`. Every receiver and callback-dispatch fact is explicit; a missing
/// fact returns `None` so the caller can emit an invariant violation for a checked
/// call or try another analytical lowering form.
pub(crate) fn resolve_closure_op(
    recv_ty: &Type,
    method: &str,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    fallible_callback: bool,
    callback_needs_fn_mut: Option<bool>,
    callback_param_count: Option<usize>,
) -> Option<TClosureOp> {
    let recv_ty = base_receiver_ty(recv_ty);
    let elem_ty = sequence_elem_ty(recv_ty);
    let is_sequence = elem_ty.is_some();
    let is_list = list_receiver(recv_ty);
    let is_iter = matches!(
        recv_ty,
        Type::Apply { name, args }
            if args.len() == 1
                && matches!(
                    name.as_str(),
                    crate::Syntax::TYPE_ITER | crate::Syntax::TYPE_VIEW_ITER
                )
    );
    let is_map = matches!(recv_ty, Type::Map { .. });
    let is_option = matches!(recv_ty, Type::Option(_));
    let is_bag =
        matches!(recv_ty, Type::Apply { name, .. } if name == crate::Syntax::TYPE_TALLY);
    let is_view = view_receiver(recv_ty);
    let has_args = |count: usize| args.len() == count;
    let op = match method {
        "edit_disjoint" if is_list && has_args(2) => TClosureOp::EditDisjoint,
        "map" if is_option && has_args(1) => TClosureOp::OptionMap,
        "map" if is_map && has_args(1) => TClosureOp::MapMap,
        "map" if !is_sequence || !has_args(1) => return None,
        "map" if fallible_callback => TClosureOp::TryMap,
        "map" if is_view => TClosureOp::ViewMap,
        "map" => match callback_needs_fn_mut {
            Some(true) => TClosureOp::MapMut,
            Some(false) => TClosureOp::Map,
            None => return None,
        },
        "filter" if is_map && has_args(1) => TClosureOp::MapFilter,
        "filter" if !is_sequence || !has_args(1) => return None,
        "filter" if fallible_callback => TClosureOp::TryFilter,
        "filter" => TClosureOp::Filter,
        "each" if is_map && has_args(1) => TClosureOp::EachMap,
        "each" if !is_sequence || !has_args(1) => return None,
        "each" if is_list
            && elem_ty
                .as_ref()
                .is_some_and(|elem| list_carries_trait(cx, elem)) =>
        {
            TClosureOp::EachRef
        }
        "each" => match callback_needs_fn_mut {
            Some(true) => TClosureOp::EachMut,
            Some(false) => TClosureOp::Each,
            None => return None,
        },
        "find" if is_sequence && has_args(1) => TClosureOp::Find,
        "any" if is_bag && has_args(1) => TClosureOp::BagAny,
        "any" if is_map && has_args(1) => TClosureOp::MapAny,
        "any" if is_sequence && has_args(1) => TClosureOp::Any,
        "all" if is_map && has_args(1) => TClosureOp::MapAll,
        "all" if is_sequence && has_args(1) => TClosureOp::All,
        "count_where" if is_sequence && has_args(1) => TClosureOp::CountWhere,
        "sort_by" if !is_list || !has_args(1) => return None,
        "sort_by" => match (callback_param_count, fallible_callback) {
            (Some(1), true) => TClosureOp::TrySortBy,
            (Some(1), false) => TClosureOp::SortBy,
            (Some(2), false) => TClosureOp::SortByCompare,
            _ => return None,
        },
        "sort_by_desc" if !is_list || !has_args(1) => return None,
        "sort_by_desc" => match (callback_param_count, fallible_callback) {
            (Some(1), true) => TClosureOp::TrySortByDesc,
            (Some(1), false) => TClosureOp::SortByDesc,
            _ => return None,
        },
        "reduce" if (is_list || is_iter) && has_args(2) => TClosureOp::Reduce,
        "take_while" if is_sequence && has_args(1) => TClosureOp::TakeWhile,
        "skip_while" if is_sequence && has_args(1) => TClosureOp::SkipWhile,
        "flat_map" if is_map && has_args(1) => TClosureOp::MapFlatMap,
        "flat_map" if is_sequence && has_args(1) => TClosureOp::FlatMap,
        "binary_search_by" if is_list && has_args(1) => TClosureOp::ListBinarySearchBy,
        "min_max_by" if is_list && has_args(1) => {
            let elem = elem_ty.as_ref()?;
            let fields = vec![
                ("min".to_string(), elem.clone()),
                ("max".to_string(), elem.clone()),
            ];
            TClosureOp::ListMinMaxBy {
                tuple_struct: crate::Codegen::Tuples::tuple_struct_name(&fields),
            }
        }
        "filter_map" if is_sequence && has_args(1) => TClosureOp::FilterMap,
        "para_map" if is_list && matches!(args.len(), 1 | 2) => TClosureOp::ParaMap,
        "para_filter" if is_list && has_args(1) => TClosureOp::ParaFilter,
        "para_fold" if is_list && has_args(3) => TClosureOp::ParaFold,
        "para_partition" if is_list && has_args(1) => {
            let elem = elem_ty.as_ref()?;
            let list_ty = Type::List(Box::new(elem.clone()));
            let fields = vec![
                ("false_".to_string(), list_ty.clone()),
                ("true_".to_string(), list_ty),
            ];
            TClosureOp::ParaPartition {
                tuple_struct: crate::Codegen::Tuples::tuple_struct_name(&fields),
            }
        }
        "scan" if is_sequence && has_args(2) => TClosureOp::Scan,
        "fold" if is_map && has_args(2) => TClosureOp::MapFold,
        "fold" if is_view && has_args(2) => TClosureOp::ViewFold,
        "fold" if (is_list || is_iter) && has_args(2) => TClosureOp::Fold,
        "position" if is_sequence && has_args(1) => TClosureOp::Position,
        "min_by" if is_sequence && has_args(1) => TClosureOp::MinBy,
        "max_by" if is_sequence && has_args(1) => TClosureOp::MaxBy,
        "group_by" if is_sequence && has_args(1) => TClosureOp::GroupBy,
        "count_by" if is_sequence && has_args(1) => TClosureOp::CountBy,
        "update_first" if is_list && has_args(2) => TClosureOp::UpdateFirst,
        "dedup_by" if is_sequence && has_args(1) => TClosureOp::DedupBy,
        "is_sorted_by" if is_sequence && has_args(1) => TClosureOp::IsSortedBy,
        "chunk_while" if is_sequence && has_args(1) => TClosureOp::ChunkWhile,
        "partition" if is_sequence && has_args(1) => {
            let elem = elem_ty.as_ref()?;
            let list_ty = Type::List(Box::new(elem.clone()));
            let fields = vec![
                ("false_".to_string(), list_ty.clone()),
                ("true_".to_string(), list_ty),
            ];
            TClosureOp::Partition {
                tuple_struct: crate::Codegen::Tuples::tuple_struct_name(&fields),
            }
        }
        _ => return None,
    };
    if matches!(
        op,
        TClosureOp::SortBy
            | TClosureOp::SortByDesc
            | TClosureOp::TrySortBy
            | TClosureOp::TrySortByDesc
            | TClosureOp::SortByCompare
    ) && crate::Collections::builtin_receiver_borrow(recv_ty, method)
        != crate::Collections::BuiltinReceiverBorrow::EagerWrite
    {
        return None;
    }
    Some(op)
}

/// c109 Phase 11: TIR-local reproduction of codegen's `list_carries_trait` — a list
/// element type that is a trait object or a
/// named trait. Used by the `each`-on-trait-object-list emit branch (`jet_list_each_ref`).
/// In the covered collection subset a trait-object element type is excluded, so this
/// is always false for a covered receiver; reproduced for exactness regardless.
pub(crate) fn list_carries_trait(cx: &Cx, inner: &Type) -> bool {
    matches!(inner, Type::TraitObject(_))
        || matches!(inner, Type::Named(n) if cx.trait_names.contains(n))
}
