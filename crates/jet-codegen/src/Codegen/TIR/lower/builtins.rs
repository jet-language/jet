use super::*;

fn tuple_fields(ty: Option<&Type>) -> Option<Vec<(String, Type)>> {
    match ty {
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
    match ty {
        Some(Type::List(inner)) => tuple_fields(Some(inner.as_ref())),
        _ => None,
    }
}

fn option_tuple_fields(ty: Option<&Type>) -> Option<Vec<(String, Type)>> {
    match ty {
        Some(Type::Option(inner)) => tuple_fields(Some(inner.as_ref())),
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

/// D-MEM1 stage S5: lower a `Binding.string_view` init (`s.trim()` /
/// `s.after(sep)` / `s.before(sep)`) to the borrowed `&str`-returning
/// `TBuiltinOp::{TrimView,AfterView,BeforeView}` — the zero-copy sibling of
/// whatever `resolve_builtin_op` would pick for the same call written
/// somewhere the result isn't scope-tracked. `ty` stays `Type::String` (Jet
/// has one string type end to end — D-MEM1 gallery); only the generated Rust
/// text is a borrow, not the Jet-level type.
fn lower_string_view_init(init: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    let Expr::MethodCall {
        receiver,
        method,
        args,
        ..
    } = init
    else {
        unreachable!("sema sets `string_view` only for a trim/after/before MethodCall init");
    };
    let op = match method.as_str() {
        "trim" => TBuiltinOp::TrimView,
        "after" => TBuiltinOp::AfterView,
        "before" => TBuiltinOp::BeforeView,
        _ => unreachable!("sema sets `string_view` only for trim/after/before"),
    };
    let recv = lower_expr(receiver, cx, env);
    let targs = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
    TExpr {
        ty: Type::String,
        kind: TExprKind::BuiltinMethod {
            recv: Box::new(recv),
            op,
            args: targs,
        },
    }
}

/// c109 Phase 9: resolve the built-in method op from the method name, arg count, and
/// the receiver's resolved type — reproducing `emit_builtin_method`'s name+`rty`
/// dispatch (Source/Codegen/Expression.rs) exactly. The Map-vs-List branch
/// (`insert`/`remove`/`get`) and the String-vs-list branch (`len`) come from
/// `tir_recv_jet_ty` (matching the AST's `rty`); a `None` or non-Map/non-String
/// receiver falls to the list/else branch, byte-for-byte the AST default. Returns
/// `None` for any name/shape the TIR does not lower (the caller stays on the AST
/// path — the gate already excluded these, so this is a defensive belt).
pub(crate) fn resolve_builtin_op(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    args: &[crate::AST::CallArg],
    resolved_ret: Option<&Type>,
    env: &LowerEnv,
    cx: &Cx,
) -> Option<TBuiltinOp> {
    if crate::Collections::is_closure_method(method) {
        return None;
    }
    let rty = tir_recv_jet_ty(receiver, env);
    let is_string = matches!(rty, Some(Type::String));
    let is_map = matches!(rty, Some(Type::Map { .. }));
    let is_set = matches!(&rty, Some(Type::Apply { name, .. }) if name == "Set");
    let is_sorted_set =
        matches!(&rty, Some(Type::Apply { name, .. }) if name == crate::Syntax::TYPE_SORTED_SET);
    let is_priority_queue = matches!(&rty, Some(Type::Apply { name, .. }) if name == crate::Syntax::TYPE_PRIORITY_QUEUE);
    let is_lru = matches!(&rty, Some(Type::Apply { name, .. }) if name == crate::Syntax::TYPE_LRU);
    let is_bag = matches!(&rty, Some(Type::Apply { name, .. }) if name == "Bag");
    let is_bit_set = matches!(&rty, Some(Type::Named(name)) if name == crate::Syntax::TYPE_BIT_SET);
    let is_byte_buffer =
        matches!(&rty, Some(Type::Named(name)) if name == crate::Syntax::TYPE_BYTE_BUFFER);
    // D-HOLE1: `.zip` on `T?` (vs. `[T].zip`).
    let is_option = matches!(rty, Some(Type::Option(_)));
    Some(match (method, args.len()) {
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
        ("push", 1) => TBuiltinOp::Push,
        ("pop", 0) => TBuiltinOp::Pop,
        ("insert", 2) => {
            if is_map {
                TBuiltinOp::InsertMap
            } else {
                TBuiltinOp::InsertList
            }
        }
        ("remove", 1) => {
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
            } else {
                // The list form embeds the *method-span* line for its bounds panic,
                // exactly as `emit_builtin_method` reads `span_line_col(method_span.start)`.
                let line = crate::Diagnostics::span_line_col(&cx.src, method_span.start).0;
                TBuiltinOp::RemoveList { line }
            }
        }
        ("get", 1) => {
            if is_lru {
                TBuiltinOp::LruGet
            } else if is_map {
                TBuiltinOp::GetMap
            } else {
                TBuiltinOp::GetList
            }
        }
        ("first", 0) if is_sorted_set => TBuiltinOp::First,
        ("last", 0) if is_sorted_set => TBuiltinOp::Last,
        ("first", 0) => TBuiltinOp::First,
        ("last", 0) => TBuiltinOp::Last,
        ("contains", 1) => TBuiltinOp::Contains,
        ("index_of", 1) => TBuiltinOp::IndexOf,
        ("reverse", 0) => TBuiltinOp::Reverse,
        ("sort", 0) => TBuiltinOp::Sort,
        ("join", 1) => TBuiltinOp::JoinSep,
        ("sum", 0) => TBuiltinOp::Sum,
        ("product", 0) => TBuiltinOp::Product,
        ("min", 0) => TBuiltinOp::Min,
        ("max", 0) => TBuiltinOp::Max,
        ("flatten", 0) => TBuiltinOp::Flatten,
        ("intersperse", 1) => TBuiltinOp::Intersperse,
        ("clear", 0) => TBuiltinOp::Clear,
        ("chars", 0) => TBuiltinOp::Chars,
        ("bytes", 0) => TBuiltinOp::Bytes,
        ("trim", 0) => TBuiltinOp::Trim,
        ("split", 1) => TBuiltinOp::Split,
        // c97/D-STRPARSE1: String-only builtins. The numeric `to_int` is a `MethodCall`
        // with a numeric `recv_type` (→ `NumericMethod`) and never reaches this path;
        // the stream-handle `lines` carries a handle `recv_type` and is likewise routed
        // elsewhere. There is no list/map `to_int`/`lines`, so an unguarded match here is
        // unambiguous — no `is_string` test (the loop-var receiver carries `jet_ty: None`,
        // so it would spuriously fail one).
        ("lines", 0) => TBuiltinOp::Lines,
        ("to_int", 0) => TBuiltinOp::ToIntString,
        ("starts_with", 1) => TBuiltinOp::StartsWith,
        ("ends_with", 1) => TBuiltinOp::EndsWith,
        ("replace", 2) => TBuiltinOp::Replace,
        // D-STR-AFTER1: `.after(sep)`/`.before(sep)` — first-occurrence substring split.
        ("after", 1) => TBuiltinOp::After,
        ("before", 1) => TBuiltinOp::Before,
        ("to_upper", 0) => TBuiltinOp::ToUpper,
        ("to_lower", 0) => TBuiltinOp::ToLower,
        ("repeat", 1) => TBuiltinOp::Repeat,
        ("slice", 2) => {
            // The string-slice form embeds the *receiver-span* line for its bounds panic.
            let line = crate::Diagnostics::span_line_col(&cx.src, receiver.span().start).0;
            TBuiltinOp::Slice { line }
        }
        // D-DYNARRAY1: `list.view(a..b)` — the string-slice line-number
        // convention (`("slice", 2)` above) doesn't apply here: the receiver
        // is always a plain value (never a chained view — `resolve_builtin_op`
        // only fires when `recv_type.is_none()`), so the receiver-span line
        // matches `emit_builtin_method`'s own panic-site convention.
        ("view", 2) => {
            let line = crate::Diagnostics::span_line_col(&cx.src, receiver.span().start).0;
            TBuiltinOp::ViewNew { line }
        }
        ("keys", 0) if is_lru => TBuiltinOp::LruKeys,
        ("keys", 0) => TBuiltinOp::Keys,
        ("values", 0) => TBuiltinOp::Values,
        ("contains_key", 1) => TBuiltinOp::ContainsKey,
        ("to_string", 0) => TBuiltinOp::ToString,
        // D-ITER1: non-closure list adapters.
        ("take", 1) => TBuiltinOp::Take,
        ("skip", 1) => TBuiltinOp::Skip,
        ("step_by", 1) => TBuiltinOp::StepBy,
        ("dedup", 0) => TBuiltinOp::Dedup,
        ("chunks", 1) => TBuiltinOp::Chunks,
        ("windows", 1) => TBuiltinOp::Windows,
        ("enumerate", 0) => {
            // Build the tuple struct name for `(idx: Int, item: T)`.
            // Fields are alpha-sorted: idx < item.
            let fields = tuple_list_elem_fields(resolved_ret).unwrap_or_else(|| {
                let elem_ty = match &rty {
                    Some(Type::List(inner)) => *inner.clone(),
                    _ => Type::Int,
                };
                vec![
                    ("idx".to_string(), Type::Int),
                    ("item".to_string(), elem_ty),
                ]
            });
            let ts = crate::Codegen::Tuples::tuple_struct_name(&fields);
            TBuiltinOp::Enumerate { tuple_struct: ts }
        }
        ("zip", 1) if is_option => {
            // D-HOLE1: `a: T?`.zip(`b: U?`) -> `(a: T, b: U)?` — heterogeneous, so
            // (unlike list zip below) the real `b` type is read from the argument.
            let fields = option_tuple_fields(resolved_ret).unwrap_or_else(|| {
                let a_ty = match &rty {
                    Some(Type::Option(inner)) => (**inner).clone(),
                    _ => Type::Int,
                };
                let b_ty = match tir_recv_jet_ty(&args[0].expr, env) {
                    Some(Type::Option(inner)) => *inner,
                    _ => Type::Int,
                };
                vec![("a".to_string(), a_ty), ("b".to_string(), b_ty)]
            });
            let ts = crate::Codegen::Tuples::tuple_struct_name(&fields);
            TBuiltinOp::OptionZip {
                tuple_struct: ts,
                elem_ty: Type::Tuple(
                    fields
                        .into_iter()
                        .map(|(name, ty)| (name, Box::new(ty)))
                        .collect(),
                ),
            }
        }
        ("zip", 1) => {
            // Build the tuple struct name for `(a: T, b: U)`.
            let fields = tuple_list_elem_fields(resolved_ret).unwrap_or_else(|| {
                let a_ty = match &rty {
                    Some(Type::List(inner)) => *inner.clone(),
                    _ => Type::Int,
                };
                let b_ty = match tir_recv_jet_ty(&args[0].expr, env) {
                    Some(Type::List(inner)) => *inner,
                    _ => Type::Int,
                };
                vec![("a".to_string(), a_ty), ("b".to_string(), b_ty)]
            });
            let ts = crate::Codegen::Tuples::tuple_struct_name(&fields);
            TBuiltinOp::Zip { tuple_struct: ts }
        }
        ("unzip", 0) => {
            let fields = tuple_fields(resolved_ret).unwrap_or_else(|| {
                let (a_ty, b_ty) = match &rty {
                    Some(Type::List(inner)) => match inner.as_ref() {
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
                            (a, b)
                        }
                        _ => (Type::Int, Type::Int),
                    },
                    _ => (Type::Int, Type::Int),
                };
                vec![
                    ("a".to_string(), Type::List(Box::new(a_ty))),
                    ("b".to_string(), Type::List(Box::new(b_ty))),
                ]
            });
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
        ("to_list", 0) if is_sorted_set => TBuiltinOp::SortedSetToList,
        ("to_list", 0) if is_bit_set => TBuiltinOp::BitSetToList,
        ("to_list", 0) => TBuiltinOp::SetToList,
        ("union", 1) if is_sorted_set => TBuiltinOp::SortedSetUnion,
        ("union", 1) => TBuiltinOp::SetUnion,
        ("put", 2) if is_lru => TBuiltinOp::LruPut,
        ("capacity", 0) if is_lru => TBuiltinOp::LruCapacity,
        ("count", 0) if is_bit_set => TBuiltinOp::BitSetCount,
        (
            "write_u8" | "write_u16_le" | "write_u16_be" | "write_u32_le" | "write_u32_be"
            | "write_u64_le" | "write_u64_be" | "write_bytes",
            1,
        ) if is_byte_buffer => TBuiltinOp::ByteBufferWrite {
            method: method.to_string(),
        },
        ("to_bytes", 0) if is_byte_buffer => TBuiltinOp::ByteBufferToBytes,
        ("has", 1) if is_bag => TBuiltinOp::BagHas,
        ("count", 1) if is_bag => TBuiltinOp::BagCount,
        // D-COLLBREADTH1=A: Deque<T> instance methods.
        ("push_front", 1) => TBuiltinOp::DequePushFront,
        ("push_back", 1) => TBuiltinOp::DequePushBack,
        ("pop_front", 0) => TBuiltinOp::DequePopFront,
        ("pop_back", 0) => TBuiltinOp::DequePopBack,
        ("peek_front", 0) => TBuiltinOp::DequePeekFront,
        ("peek_back", 0) => TBuiltinOp::DequePeekBack,
        _ => return None,
    })
}

/// c109 Phase 9: the resolved return type of a built-in collection/string method,
/// from `Collections::builtin_method_return` (the sema table). Kept total per the
/// design principle; rarely load-bearing in emit (a binding carries sema's `b.ty`),
/// but resolved here so the TIR never guesses. Falls back to `Unit` for a void
/// method or an unresolved receiver type (impossible for a covered call — sema
/// validated it).
pub(crate) fn builtin_result_ty(method: &str, nargs: usize, recv_ty: Option<&Type>) -> Type {
    match recv_ty.and_then(|rt| crate::Collections::builtin_method_return(rt, method, nargs, false))
    {
        Some(Some(t)) => t,
        _ => unit_type(),
    }
}

/// c109 Phase 11: resolve a closure-taking collection method into a total
/// `TClosureOp`, reproducing `emit_builtin_method`'s closure arms
/// (Source/Codegen/Expression.rs) exactly. The receiver-type branch
/// (`rty = expr_jet_ty(receiver)`) picks Map (`EachMap`) vs trait-object list
/// (`EachRef`) vs plain list; the Fn-vs-FnMut branch reads the lambda arg's
/// `needs_fn_mut` meta. All decisions made HERE, never in emit (I3). The gate
/// proved a literal lambda arg, so `needs_fn_mut` is always readable; a non-lambda
/// arg defaults to the non-mut form, matching the AST `else` branch.
pub(crate) fn resolve_closure_op(
    receiver: &Expr,
    method: &str,
    args: &[crate::AST::CallArg],
    env: &LowerEnv,
    cx: &Cx,
) -> TClosureOp {
    let rty = tir_recv_jet_ty(receiver, env);
    // The lambda arg's FnMut fact (the AST checks `args[0]` for map/each).
    let fn_mut =
        matches!(args.first().map(|a| &a.expr), Some(Expr::Lambda(l)) if l.meta.needs_fn_mut);
    match method {
        "map" => {
            // D-HOLE1: `.map` on `T?` uses Rust's native `Option::map` directly —
            // never the mutable-list form.
            if matches!(rty, Some(Type::Option(_))) {
                TClosureOp::OptionMap
            } else if matches!(&rty, Some(Type::Apply { name, .. }) if name == "View") {
                // D-DYNARRAY1: map-to-owned — never the `.clone()`-into-Vec form
                // the other list ops use (`recv` is already a borrow, not owned).
                TClosureOp::ViewMap
            } else if fn_mut {
                TClosureOp::MapMut
            } else {
                TClosureOp::Map
            }
        }
        "filter" => TClosureOp::Filter,
        "each" => {
            // The AST: `match rty { Map => jet_map_each, _ => list_each }`, where
            // `list_each` checks trait-object-list FIRST, then lambda FnMut.
            match &rty {
                Some(Type::Map { .. }) => TClosureOp::EachMap,
                Some(Type::List(inner)) if list_carries_trait(cx, inner) => TClosureOp::EachRef,
                _ if fn_mut => TClosureOp::EachMut,
                _ => TClosureOp::Each,
            }
        }
        "find" => TClosureOp::Find,
        "any" => TClosureOp::Any,
        "all" => TClosureOp::All,
        "sort_by" => TClosureOp::SortBy,
        "reduce" => TClosureOp::Reduce,
        // D-ITER1: new closure adapters.
        "take_while" => TClosureOp::TakeWhile,
        "skip_while" => TClosureOp::SkipWhile,
        "flat_map" => TClosureOp::FlatMap,
        "filter_map" => TClosureOp::FilterMap,
        "par_map" => TClosureOp::ParMap,
        "par_filter" => TClosureOp::ParFilter,
        "par_fold" => TClosureOp::ParFold,
        "scan" => TClosureOp::Scan,
        "fold" => {
            // D-DYNARRAY1: a View's `recv` is already `&[T]` — fold it directly,
            // no `.clone()`-to-owned-Vec step.
            if matches!(&rty, Some(Type::Apply { name, .. }) if name == "View") {
                TClosureOp::ViewFold
            } else {
                TClosureOp::Fold
            }
        }
        "position" => TClosureOp::Position,
        "min_by" => TClosureOp::MinBy,
        "max_by" => TClosureOp::MaxBy,
        "group_by" => TClosureOp::GroupBy,
        "count_by" => TClosureOp::CountBy,
        "partition" => {
            // Compute the tuple struct name from the receiver element type.
            // recv = List<T>; partition returns (false_: [T], true_: [T]).
            let elem_ty = match tir_recv_jet_ty(receiver, env) {
                Some(Type::List(inner)) => *inner,
                _ => Type::Int,
            };
            let list_ty = Type::List(Box::new(elem_ty.clone()));
            let fields = vec![
                ("false_".to_string(), list_ty.clone()),
                ("true_".to_string(), list_ty),
            ];
            let ts = crate::Codegen::Tuples::tuple_struct_name(&fields);
            TClosureOp::Partition { tuple_struct: ts }
        }
        // The gate (`is_closure_method`) admits only the names above.
        _ => unreachable!("non-closure method in resolve_closure_op (gate)"),
    }
}

/// c109 Phase 11: TIR-local reproduction of codegen's `list_carries_trait`
/// (Source/Codegen/Expression.rs) — a list element type that is a trait object or a
/// named trait. Used by the `each`-on-trait-object-list emit branch (`jet_list_each_ref`).
/// In the covered collection subset a trait-object element type is excluded, so this
/// is always false for a covered receiver; reproduced for exactness regardless.
pub(crate) fn list_carries_trait(cx: &Cx, inner: &Type) -> bool {
    matches!(inner, Type::TraitObject(_))
        || matches!(inner, Type::Named(n) if cx.trait_names.contains(n))
}
