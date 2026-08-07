//! Exhaustive TBuiltinOp dispatch (#777).
use crate::AST::{CtFloat, Type};
use crate::AST::CtKey;
use crate::Comptime::Builtins::{apply_method, apply_mutating, apply_static_type_method};
use crate::Comptime::CollectionEval;
use crate::Comptime::{CtReport, CtValue};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Codegen::TIR::TBuiltinOp;
use crate::Syntax;
use super::unsupported;

fn eval_zip_source(value: &CtValue, span: Span) -> Result<Vec<CtValue>, Diagnostic> {
    if let Some((items, _)) = super::progress_iter_parts(value) {
        return Ok(items);
    }
    match value {
        CtValue::List(items) => Ok(items.clone()),
        _ => Err(unsupported("zip input", span)),
    }
}

fn coerce_zip_fill(value: CtValue, target: &Type) -> CtValue {
    let target = match target {
        Type::Option(inner) | Type::Tagged { inner, .. } => {
            return coerce_zip_fill(value, inner);
        }
        _ => target,
    };
    match (value, target) {
        (CtValue::Int(value), Type::Float) => CtValue::Float(CtFloat::f64(value as f64)),
        (CtValue::Int(value), Type::Float32) => CtValue::Float(CtFloat::f32(value as f32)),
        (CtValue::Float(value), Type::Float) => CtValue::Float(CtFloat::f64(value.as_f64())),
        (CtValue::Float(value), Type::Float32) => CtValue::Float(CtFloat::f32(value.as_f32())),
        (value, Type::Tagged { inner, .. }) => coerce_zip_fill(value, inner),
        (value, _) => value,
    }
}

fn zip_none_type(ty: &Type) -> Type {
    match ty {
        Type::Option(inner) => zip_none_type(inner),
        Type::Tagged { inner, .. } => zip_none_type(inner),
        _ => ty.clone(),
    }
}

fn eval_zip_family(
    recv: &CtValue,
    args: &[CtValue],
    tuple_struct: &str,
    mode: crate::Codegen::TIR::TZipMode,
    fields: &[String],
    input_count: usize,
    fill_mode: crate::Codegen::TIR::TZipFillMode,
    field_types: &[Type],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    if input_count == 0 {
        return Ok(super::progress_iter_value(Vec::new(), true));
    }
    let mut columns = Vec::with_capacity(input_count);
    columns.push(eval_zip_source(recv, span)?);
    for value in args.iter().take(input_count.saturating_sub(1)) {
        columns.push(eval_zip_source(value, span)?);
    }
    if columns.len() != input_count {
        return Err(unsupported("zip input arity", span));
    }
    let fill_args = &args[input_count.saturating_sub(1)..];
    if mode == crate::Codegen::TIR::TZipMode::Strict
        && columns.iter().any(|column| column.len() != columns[0].len())
    {
        return Err(Diagnostic::error(
            "E0128",
            "zip inputs have different lengths".to_string(),
            "strict `zip` requires every input to end on the same row".to_string(),
            "use `zip_short` or `zip_pad` when lengths may differ".to_string(),
            Some(span),
        ));
    }
    let row_count = match mode {
        crate::Codegen::TIR::TZipMode::Strict | crate::Codegen::TIR::TZipMode::Short => {
            columns.iter().map(Vec::len).min().unwrap_or(0)
        }
        crate::Codegen::TIR::TZipMode::Pad => columns.iter().map(Vec::len).max().unwrap_or(0),
    };
    let fills_for = |index: usize| -> CtValue {
        match fill_mode {
            crate::Codegen::TIR::TZipFillMode::DefaultNone => CtValue::absent(
                field_types
                    .get(index)
                    .map(zip_none_type)
                    .unwrap_or(Type::Int),
            ),
            crate::Codegen::TIR::TZipFillMode::Common => {
                fill_args
                    .first()
                    .cloned()
                    .map(|value| {
                        coerce_zip_fill(
                            value,
                            field_types.get(index).unwrap_or(&Type::Int),
                        )
                    })
                    .unwrap_or_else(|| {
                        CtValue::absent(
                            field_types
                                .get(index)
                                .map(zip_none_type)
                                .unwrap_or(Type::Int),
                        )
                    })
            }
            crate::Codegen::TIR::TZipFillMode::Columns => {
                let field = fields.get(index).map(String::as_str).unwrap_or("");
                match fill_args.first() {
                    Some(CtValue::Struct { fields, .. }) => fields
                        .iter()
                        .find(|(name, _)| {
                            name == field || name.strip_prefix("user_") == Some(field)
                        })
                        .map(|(_, value)| {
                            coerce_zip_fill(
                                value.clone(),
                                field_types.get(index).unwrap_or(&Type::Int),
                            )
                        })
                        .unwrap_or_else(|| {
                            CtValue::absent(
                                field_types
                                    .get(index)
                                    .map(zip_none_type)
                                    .unwrap_or(Type::Int),
                            )
                        }),
                    _ => CtValue::absent(
                        field_types
                            .get(index)
                            .map(zip_none_type)
                            .unwrap_or(Type::Int),
                    ),
                }
            }
        }
    };
    let mut rows = Vec::with_capacity(row_count);
    for row in 0..row_count {
        let values = columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                column
                    .get(row)
                    .cloned()
                    .unwrap_or_else(|| fills_for(index))
            })
            .collect::<Vec<_>>();
        rows.push(CtValue::Struct {
            type_name: tuple_struct.to_string(),
            fields: fields
                .iter()
                .cloned()
                .zip(values)
                .collect(),
        });
    }
    Ok(super::progress_iter_value(rows, true))
}

pub(super) fn eval_builtin(
    op: &TBuiltinOp,
    recv: &mut CtValue,
    args: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    match op {
        TBuiltinOp::LenString => apply_method(recv, "len", args, span),
        TBuiltinOp::LenList => apply_method(recv, "len", args, span),
        TBuiltinOp::IsEmpty => apply_method(recv, "is_empty", args, span),
        TBuiltinOp::Push => apply_mutating(recv, "push", args, span),
        TBuiltinOp::Pop => apply_mutating(recv, "pop", args, span),
        TBuiltinOp::InsertMap => apply_mutating(recv, "add", args, span),
        TBuiltinOp::AddNewMap => apply_mutating(recv, "add_new", args, span),
        TBuiltinOp::MapMerge | TBuiltinOp::MapMergeWith => {
            apply_method(recv, "merge", args, span)
        }
        TBuiltinOp::InsertList => apply_mutating(recv, "insert", args, span),
        TBuiltinOp::RemoveMap => apply_mutating(recv, "remove", args, span),
        TBuiltinOp::RemoveList { .. } => apply_mutating(recv, "remove", args, span),
        TBuiltinOp::CountList => apply_method(recv, "count", args, span),
        TBuiltinOp::ExtendList => apply_mutating(recv, "extend", args, span),
        TBuiltinOp::ConcatList => apply_method(recv, "concat", args, span),
        TBuiltinOp::GetMap => apply_method(recv, "get", args, span),
        TBuiltinOp::GetList => apply_method(recv, "get", args, span),
        TBuiltinOp::First => apply_method(recv, "first", args, span),
        TBuiltinOp::Last => apply_method(recv, "last", args, span),
        TBuiltinOp::Contains
            if matches!(recv, CtValue::Struct { type_name, .. } if type_name == Syntax::TYPE_RANGE) =>
        {
            let needle = args.first().unwrap_or(&CtValue::Int(0));
            super::range_contains(recv, needle, span).map(CtValue::Bool)
        }
        TBuiltinOp::Contains => apply_method(recv, "contains", args, span),
        TBuiltinOp::IndexOf => apply_method(recv, "index_of", args, span),
        TBuiltinOp::Reverse => apply_mutating(recv, "reverse", args, span),
        TBuiltinOp::Sort => apply_mutating(recv, "sort", args, span),
        TBuiltinOp::JoinSep => apply_method(recv, "join", args, span),
        TBuiltinOp::Sum { .. } => apply_method(recv, "sum", args, span),
        TBuiltinOp::Product { .. } => apply_method(recv, "product", args, span),
        TBuiltinOp::Min { .. } => apply_method(recv, "min", args, span),
        TBuiltinOp::Max { .. } => apply_method(recv, "max", args, span),
        TBuiltinOp::Flatten => apply_method(recv, "flatten", args, span),
        TBuiltinOp::Intersperse => apply_method(recv, "intersperse", args, span),
        TBuiltinOp::Unzip { .. } => apply_method(recv, "unzip", args, span),
        TBuiltinOp::Clear => apply_mutating(recv, "clear", args, span),
        TBuiltinOp::Chars => apply_method(recv, "chars", args, span),
        TBuiltinOp::Bytes => apply_method(recv, "bytes", args, span),
        TBuiltinOp::Trim => apply_method(recv, "trim", args, span),
        TBuiltinOp::TrimStart => apply_method(recv, "trim_start", args, span),
        TBuiltinOp::TrimEnd => apply_method(recv, "trim_end", args, span),
        TBuiltinOp::Split => apply_method(recv, "split", args, span),
        TBuiltinOp::Lines => apply_method(recv, "lines", args, span),
        // D-STRPARSE1: text is the builtin receiver; parse is static on Int/Float.
        TBuiltinOp::ParseInt => apply_static_type_method("Int", "parse", vec![recv.clone()], span)
            .unwrap_or_else(|| Err(unsupported("Int.parse", span))),
        TBuiltinOp::ParseFloat => {
            apply_static_type_method("Float", "parse", vec![recv.clone()], span)
                .unwrap_or_else(|| Err(unsupported("Float.parse", span)))
        }
        TBuiltinOp::StartsWith => apply_method(recv, "starts_with", args, span),
        TBuiltinOp::EndsWith => apply_method(recv, "ends_with", args, span),
        TBuiltinOp::Replace => apply_method(recv, "replace", args, span),
        TBuiltinOp::PadStart => apply_method(recv, "pad_start", args, span),
        TBuiltinOp::PadEnd => apply_method(recv, "pad_end", args, span),
        TBuiltinOp::StringIndexOf => apply_method(recv, "index_of", args, span),
        TBuiltinOp::StringCount => apply_method(recv, "count", args, span),
        TBuiltinOp::StringIsAlphabetic => apply_method(recv, "is_alphabetic", args, span),
        TBuiltinOp::StringIsNumeric => apply_method(recv, "is_numeric", args, span),
        TBuiltinOp::StringIsWhitespace => apply_method(recv, "is_whitespace", args, span),
        TBuiltinOp::StringIsAscii => apply_method(recv, "is_ascii", args, span),
        TBuiltinOp::StringToTitle => apply_method(recv, "to_title", args, span),
        TBuiltinOp::StringMethod { method } => apply_method(recv, method, args, span),
        TBuiltinOp::StringSplitOnce { .. } => apply_method(recv, "split_once", args, span),
        TBuiltinOp::ToUpper => apply_method(recv, "to_upper", args, span),
        TBuiltinOp::ToLower => apply_method(recv, "to_lower", args, span),
        TBuiltinOp::Repeat => apply_method(recv, "repeat", args, span),
        TBuiltinOp::Slice { .. } => apply_method(recv, "slice", args, span),
        TBuiltinOp::After => apply_method(recv, "after", args, span),
        TBuiltinOp::Before => apply_method(recv, "before", args, span),
        TBuiltinOp::TrimView => apply_method(recv, "trim", args, span),
        TBuiltinOp::AfterView => apply_method(recv, "after", args, span),
        TBuiltinOp::BeforeView => apply_method(recv, "before", args, span),
        TBuiltinOp::Keys => apply_method(recv, "keys", args, span),
        TBuiltinOp::Values => apply_method(recv, "values", args, span),
        // Surface is `.has_key`; emit spells `contains_key` on the Rust map API.
        TBuiltinOp::ContainsKey => apply_method(recv, "has_key", args, span),
        TBuiltinOp::ToString => apply_method(recv, "to_string", args, span),
        TBuiltinOp::MatchGroup => apply_method(recv, "group", args, span),
        TBuiltinOp::Take => apply_method(recv, "take", args, span),
        TBuiltinOp::Skip => apply_method(recv, "skip", args, span),
        TBuiltinOp::StepBy => apply_method(recv, "step_by", args, span),
        TBuiltinOp::Dedup => apply_method(recv, "dedup", args, span),
        TBuiltinOp::Chunks => apply_method(recv, "chunks", args, span),
        TBuiltinOp::Windows => apply_method(recv, "windows", args, span),
        TBuiltinOp::IterRepeat => apply_method(recv, "repeat", args, span),
        TBuiltinOp::IterCycle => apply_method(recv, "cycle", args, span),
        TBuiltinOp::IterDropLast => apply_method(recv, "drop_last", args, span),
        TBuiltinOp::IterShuffle => apply_method(recv, "shuffle", args, span),
        TBuiltinOp::IterIsSorted => apply_method(recv, "is_sorted", args, span),
        TBuiltinOp::IterLastIndexOf => apply_method(recv, "last_index_of", args, span),
        TBuiltinOp::IterAverage { .. } => apply_method(recv, "average", args, span),
        TBuiltinOp::IterCompare => apply_method(recv, "compare", args, span),
        TBuiltinOp::IterSplit { .. } => apply_method(recv, "split", args, span),
        TBuiltinOp::ListSlice => apply_method(recv, "slice", args, span),
        TBuiltinOp::ListCopy => apply_method(recv, "copy", args, span),
        TBuiltinOp::ListEqual => apply_method(recv, "equal", args, span),
        TBuiltinOp::ListBinarySearch => apply_method(recv, "binary_search", args, span),
        TBuiltinOp::ListUnion => apply_method(recv, "union", args, span),
        TBuiltinOp::ListIntersection => apply_method(recv, "intersection", args, span),
        TBuiltinOp::ListDifference => apply_method(recv, "difference", args, span),
        TBuiltinOp::ListRandom => apply_method(recv, "random", args, span),
        TBuiltinOp::ListMinMax { .. } => apply_method(recv, "min_max", args, span),
        TBuiltinOp::MapCopy => apply_method(recv, "copy", args, span),
        TBuiltinOp::MapEqual => apply_method(recv, "equal", args, span),
        TBuiltinOp::MapFirst => apply_method(recv, "first", args, span),
        TBuiltinOp::MapToList { .. } => apply_method(recv, "to_list", args, span),
        TBuiltinOp::MapMin => apply_method(recv, "min", args, span),
        TBuiltinOp::MapMax => apply_method(recv, "max", args, span),
        TBuiltinOp::MapIntersection => apply_method(recv, "intersection", args, span),
        TBuiltinOp::MapSliceKeys => apply_method(recv, "slice", args, span),
        TBuiltinOp::MapNew => Ok(CtValue::Map(Default::default())),
        TBuiltinOp::MapFromKeys => {
            let keys = match recv {
                CtValue::List(xs) => xs.clone(),
                _ => return Err(unsupported("Map.from_keys keys", span)),
            };
            let default = args.into_iter().next().unwrap_or(CtValue::Unit);
            let mut out = std::collections::BTreeMap::new();
            for k in keys {
                let key = CtKey::from_value(k).ok_or_else(|| unsupported("map key", span))?;
                out.insert(key, default.clone());
            }
            Ok(CtValue::Map(out))
        }
        TBuiltinOp::MapContainsValue => apply_method(recv, "contains_value", args, span),
        TBuiltinOp::MapPopFirst => apply_method(recv, "pop_first", args, span),
        TBuiltinOp::ListReplace => apply_method(recv, "replace", args, span),
        TBuiltinOp::Indexed { .. } => apply_method(recv, "indexed", args, span),
        TBuiltinOp::Indexes => apply_method(recv, "indexes", args, span),
        TBuiltinOp::Zip {
            tuple_struct,
            mode,
            fields,
            input_count,
            fill_mode,
            field_types,
            ..
        } => eval_zip_family(
            recv,
            &args,
            tuple_struct,
            *mode,
            fields,
            *input_count,
            *fill_mode,
            field_types,
            span,
        ),
        TBuiltinOp::OptionZip { .. } => apply_method(recv, "zip", args, span),
        TBuiltinOp::IterToList => apply_method(recv, "to_list", args, span),
        TBuiltinOp::ListLazy => {
            let value = apply_method(recv, "lazy", args, span)?;
            let CtValue::List(items) = value else {
                return Err(unsupported("List.lazy result", span));
            };
            Ok(super::progress_iter_value(items, true))
        }
        TBuiltinOp::IterCollect => apply_method(recv, "collect", args, span),
        // From-ctors: recv is the source list/bytes (see method_calls.rs).
        TBuiltinOp::SetFrom => CollectionEval::from_list(Syntax::TYPE_SET, recv, span),
        TBuiltinOp::SetInsert => apply_mutating(recv, "add", args, span),
        TBuiltinOp::SetRemove => apply_mutating(recv, "remove", args, span),
        TBuiltinOp::SetToList => apply_method(recv, "to_list", args, span),
        TBuiltinOp::SetUnion => apply_method(recv, "union", args, span),
        TBuiltinOp::SetIntersection => apply_method(recv, "intersection", args, span),
        TBuiltinOp::SetDifference => apply_method(recv, "difference", args, span),
        TBuiltinOp::SetSymmetricDifference => apply_method(recv, "symmetric_difference", args, span),
        TBuiltinOp::SetIsSubset => apply_method(recv, "is_subset", args, span),
        TBuiltinOp::SetIsSuperset => apply_method(recv, "is_superset", args, span),
        TBuiltinOp::SetIsDisjoint => apply_method(recv, "is_disjoint", args, span),
        TBuiltinOp::SetCopy => apply_method(recv, "copy", args, span),
        TBuiltinOp::SetEqual => apply_method(recv, "equal", args, span),
        TBuiltinOp::SetCapacity => apply_method(recv, "capacity", args, span),
        TBuiltinOp::SetFirst => apply_method(recv, "first", args, span),
        // #1478: values is a read; replace/take are native `&mut self` ops.
        TBuiltinOp::SetValues => apply_method(recv, "values", args, span),
        TBuiltinOp::SetReplace => apply_mutating(recv, "replace", args, span),
        TBuiltinOp::SetTake => apply_mutating(recv, "take", args, span),
        TBuiltinOp::SortedSetFrom => {
            CollectionEval::from_list(Syntax::TYPE_SORTED_SET, recv, span)
        }
        TBuiltinOp::SortedSetInsert => apply_mutating(recv, "add", args, span),
        TBuiltinOp::SortedSetRemove => apply_mutating(recv, "remove", args, span),
        TBuiltinOp::SortedSetToList => apply_method(recv, "to_list", args, span),
        TBuiltinOp::SortedSetUnion => apply_method(recv, "union", args, span),
        TBuiltinOp::SortedSetIntersection => apply_method(recv, "intersection", args, span),
        TBuiltinOp::SortedSetDifference => apply_method(recv, "difference", args, span),
        TBuiltinOp::SortedSetSymmetricDifference => apply_method(recv, "symmetric_difference", args, span),
        TBuiltinOp::SortedSetIsSubset => apply_method(recv, "is_subset", args, span),
        TBuiltinOp::SortedSetIsSuperset => apply_method(recv, "is_superset", args, span),
        TBuiltinOp::SortedSetIsDisjoint => apply_method(recv, "is_disjoint", args, span),
        TBuiltinOp::PriorityQueueFrom => {
            CollectionEval::from_list(Syntax::TYPE_PRIORITY_QUEUE, recv, span)
        }
        TBuiltinOp::PriorityQueuePeek => apply_method(recv, "peek", args, span),
        TBuiltinOp::PriorityQueueToSortedList => apply_method(recv, "to_sorted_list", args, span),
        TBuiltinOp::LruPut => apply_mutating(recv, "add", args, span),
        TBuiltinOp::LruAddNew => apply_mutating(recv, "add_new", args, span),
        TBuiltinOp::LruGet => apply_mutating(recv, "get", args, span),
        TBuiltinOp::LruCapacity => apply_method(recv, "capacity", args, span),
        TBuiltinOp::LruKeys => apply_method(recv, "keys", args, span),
        TBuiltinOp::BitSetAdd => apply_mutating(recv, "add", args, span),
        TBuiltinOp::BitSetRemove => apply_mutating(recv, "remove", args, span),
        TBuiltinOp::BitSetCount => apply_method(recv, "count", args, span),
        TBuiltinOp::BitSetToList => apply_method(recv, "to_list", args, span),
        TBuiltinOp::BitSetNew => CollectionEval::prelude_new("JetBitSet", vec![], span)
            .unwrap_or_else(|| Err(unsupported("BitSet.new", span))),
        TBuiltinOp::ByteBufferNew => CollectionEval::prelude_new("JetByteBuffer", vec![], span)
            .unwrap_or_else(|| Err(unsupported("ByteBuffer.new", span))),
        TBuiltinOp::ByteBufferWithCapacity => {
            CollectionEval::prelude_new("JetByteBuffer", vec![recv.clone()], span)
                .unwrap_or_else(|| Err(unsupported("ByteBuffer.with_capacity", span)))
        }
        TBuiltinOp::ByteBufferFrom => CollectionEval::byte_buffer_from(recv, span),
        TBuiltinOp::ByteBufferWrite { method } => {
            apply_mutating(recv, method.as_str(), args, span)
        }
        TBuiltinOp::ByteBufferToBytes => apply_method(recv, "to_bytes", args, span),
        TBuiltinOp::ByteBufferMethod { method } => {
            if crate::Collections::builtin_method_mutates(
                &Type::Named(crate::Syntax::TYPE_BYTE_BUFFER.to_string()),
                method.as_str(),
            ) {
                apply_mutating(recv, method.as_str(), args, span)
            } else {
                apply_method(recv, method.as_str(), args, span)
            }
        }
        TBuiltinOp::BagAdd => apply_mutating(recv, "add", args, span),
        TBuiltinOp::BagRemove => apply_mutating(recv, "remove", args, span),
        TBuiltinOp::BagHas => apply_method(recv, "has", args, span),
        TBuiltinOp::BagCount => apply_method(recv, "count", args, span),
        TBuiltinOp::BagLen => apply_method(recv, "len", args, span),
        TBuiltinOp::DequePushFront => apply_mutating(recv, "push_front", args, span),
        TBuiltinOp::DequePushBack => apply_mutating(recv, "push_back", args, span),
        TBuiltinOp::DequePopFront => apply_mutating(recv, "pop_front", args, span),
        TBuiltinOp::DequePopBack => apply_mutating(recv, "pop_back", args, span),
        TBuiltinOp::DequePeekFront => apply_method(recv, "peek_front", args, span),
        TBuiltinOp::DequePeekBack => apply_method(recv, "peek_back", args, span),
        TBuiltinOp::DequeCapacity => apply_method(recv, "capacity", args, span),
        TBuiltinOp::DequeContains => apply_method(recv, "contains", args, span),
        TBuiltinOp::DequeGet => apply_method(recv, "get", args, span),
        TBuiltinOp::DequeDelete => apply_mutating(recv, "delete", args, span),
        TBuiltinOp::DequeToList => apply_method(recv, "to_list", args, span),
        TBuiltinOp::DequeJoin => apply_method(recv, "join", args, span),
        TBuiltinOp::DequeReverse => apply_mutating(recv, "reverse", args, span),
        TBuiltinOp::DequeSplit => apply_mutating(recv, "split", args, span),
        TBuiltinOp::DequeFrom => CollectionEval::from_list(Syntax::TYPE_DEQUE, recv, span),
        // D-FAILCOMP1 / D-ITERTOOLS1: materialize Iter<T?E> / [T?E] → T?E.
        TBuiltinOp::TryCollect => {
            let CtValue::List(xs) = recv else {
                return Err(unsupported("try_collect receiver", span));
            };
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                match x {
                    CtValue::Present(value) => out.push((**value).clone()),
                    CtValue::Failed(CtReport::Told(error)) => {
                        return Ok(CtValue::failed(Box::new((**error).clone())))
                    }
                    _ => return Err(unsupported("try_collect on a non-Result list", span)),
                }
            }
            Ok(CtValue::Present(Box::new(CtValue::List(out))))
        }
        // CtValue has no distinct View type — materialize the inclusive window as a List.
        TBuiltinOp::ViewNew { .. } | TBuiltinOp::ViewMutNew { .. } => {
            let CtValue::List(xs) = recv else {
                return Err(unsupported("view receiver", span));
            };
            let mut it = args.into_iter();
            let first = it.next().ok_or_else(|| unsupported("view start", span))?;
            let (a, end_exclusive) = if let Some(second) = it.next() {
                let CtValue::Int(a) = first else {
                    return Err(unsupported("view start", span));
                };
                let CtValue::Int(z) = second else {
                    return Err(unsupported("view end", span));
                };
                if a < 0 || z < a || z as usize >= xs.len() {
                    return Err(super::view_bounds_diagnostic(
                        xs.len(),
                        a,
                        z,
                        false,
                        span,
                    ));
                }
                (a, z + 1)
            } else {
                super::range_window(&first, xs.len(), span)?
            };
            Ok(CtValue::List(
                xs[a as usize..end_exclusive as usize].to_vec(),
            ))
        }
        TBuiltinOp::ComputeViewNew { .. } => {
            crate::Comptime::ComputeLite::tensor_view_list(recv, &args, span)
        }
        TBuiltinOp::ComputeViewMutNew { .. } => {
            Err(unsupported("Tensor mutable view builtin", span))
        }
        TBuiltinOp::SplitWrite { .. } | TBuiltinOp::GetDisjointWrite => {
            Err(unsupported("disjoint mutable view builtin", span))
        }
    }
}
