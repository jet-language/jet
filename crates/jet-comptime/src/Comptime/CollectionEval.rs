//! Comptime/TIR-eval collection ops (#722 / #777). Same CtValue shapes as
//! `Methods/dispatch/eval_method.rs` — one table for TirBridge + old helpers.

use crate::AST::Type;
use crate::Diagnostics::{Diagnostic, Span};

use super::Builtins::{as_int, cmp};
use super::Diagnostics::{index_oob, unsupported};
use crate::AST::CtValue;
use jet_foundation::Prelude::jet_as_bytes as as_bytes;

#[allow(dead_code, non_camel_case_types, unused_imports)]
mod collection_semantics {
    use jet_foundation::StructuralDebug::{
        jet_debug_map, jet_debug_optional, jet_debug_range,
    };
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;

    #[derive(Clone)]
    struct JetByteBuffer {
        bytes: Vec<u8>,
    }

    trait __jet_Display {
        fn display(&self) -> String;
    }

    trait __jet_Equatable: Sized {
        fn equal(&self, rhs: &Self) -> bool;
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct JetMap<K, V>(std::collections::BTreeMap<K, V>);

    impl<K, V> JetMap<K, V> {
        fn new() -> Self {
            Self(std::collections::BTreeMap::new())
        }
    }

    impl<K: Ord, V> std::iter::FromIterator<(K, V)> for JetMap<K, V> {
        fn from_iter<I: IntoIterator<Item = (K, V)>>(pairs: I) -> Self {
            Self(pairs.into_iter().collect())
        }
    }

    impl<K, V> std::ops::Deref for JetMap<K, V> {
        type Target = std::collections::BTreeMap<K, V>;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl<K, V> std::ops::DerefMut for JetMap<K, V> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }

    fn jet_panic(_file: &str, _line: u32, message: &str) -> ! {
        panic!("{}", message);
    }

    include!("../../../jet-codegen/src/Prelude/Core/Loadable.rs");
    include!("../../../jet-codegen/src/Prelude/Core/RangeBounds.rs");
    include!("../../../jet-codegen/src/Prelude/Core/Values.rs");
    include!("../../../jet-codegen/src/Prelude/Core/Collections.rs");

    pub(super) fn list_pop<T>(values: &mut Vec<T>) -> Option<T> {
        jet_list_pop_kernel(values).ok()
    }

    pub(super) fn list_replace<T: Clone>(values: &[T], index: i64, new: T) -> Vec<T> {
        jet_list_replace(values, index, new)
    }

    pub(super) fn set_pop<T: PartialEq>(values: &mut Vec<T>, value: &T) -> Option<T> {
        jet_set_pop_kernel(values, value).ok()
    }

    pub(super) fn deque_pop_front<T>(values: &mut Vec<T>) -> Option<T> {
        jet_deque_pop_front_kernel(values).ok()
    }

    pub(super) fn deque_pop_back<T>(values: &mut Vec<T>) -> Option<T> {
        jet_deque_pop_back_kernel(values).ok()
    }

    pub(super) fn priority_queue_pop<T>(values: &mut Vec<T>) -> Option<T> {
        jet_priority_queue_pop_kernel(values).ok()
    }

    pub(super) fn map_pop<K: Ord + Clone, V: Clone>(
        map: &mut std::collections::BTreeMap<K, V>,
        key: &K,
    ) -> Option<V> {
        let mut native = JetMap(std::mem::take(map));
        let result = jet_map_pop_kernel(&mut native, key);
        *map = native.0;
        result.ok()
    }
}

pub(super) fn list_pop<T>(values: &mut Vec<T>) -> Option<T> {
    collection_semantics::list_pop(values)
}

pub(super) fn list_replace<T: Clone>(values: &[T], index: i64, new: T) -> Vec<T> {
    collection_semantics::list_replace(values, index, new)
}

pub(super) fn map_pop<K: Ord + Clone, V: Clone>(
    map: &mut std::collections::BTreeMap<K, V>,
    key: &K,
) -> Option<V> {
    collection_semantics::map_pop(map, key)
}

mod set_semantics {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../../jet-codegen/src/Prelude/Core/SetAlgebra.rs");
}

fn list_field(fields: &[(String, CtValue)], wanted: &str) -> Vec<CtValue> {
    fields
        .iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            (name, CtValue::List(values)) if name == wanted => Some(values.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn int_field(fields: &[(String, CtValue)], wanted: &str) -> Option<i64> {
    fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        (name, CtValue::Int(n)) if name == wanted => Some(*n),
        _ => None,
    })
}

fn unique_values(items: Vec<CtValue>) -> Vec<CtValue> {
    // ponytail: comptime sets are small; O(n²) equality dedup.
    let mut unique = Vec::new();
    for item in items {
        if !unique.contains(&item) {
            unique.push(item);
        }
    }
    unique
}

fn sorted_unique(mut items: Vec<CtValue>, span: Span) -> Result<Vec<CtValue>, Diagnostic> {
    let mut sort_error = None;
    items.sort_by(|left, right| match cmp(left.clone(), right.clone(), span) {
        Ok(order) => order,
        Err(error) => {
            sort_error.get_or_insert(error);
            std::cmp::Ordering::Equal
        }
    });
    if let Some(error) = sort_error {
        return Err(error);
    }
    items.dedup();
    Ok(items)
}

fn sorted_descending(mut items: Vec<CtValue>, span: Span) -> Result<Vec<CtValue>, Diagnostic> {
    let mut sort_error = None;
    items.sort_by(|left, right| match cmp(right.clone(), left.clone(), span) {
        Ok(order) => order,
        Err(error) => {
            sort_error.get_or_insert(error);
            std::cmp::Ordering::Equal
        }
    });
    match sort_error {
        Some(error) => Err(error),
        None => Ok(items),
    }
}

fn as_string(v: &CtValue, span: Span) -> Result<String, Diagnostic> {
    match v {
        CtValue::Str(s) => Ok(s.clone()),
        _ => Err(unsupported("non-String argument to ByteBuffer", span)),
    }
}

fn option_none() -> CtValue {
    CtValue::absent(Type::Int)
}

fn set_struct(type_name: &str, items: Vec<CtValue>) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: vec![("items".to_string(), CtValue::List(items))],
    }
}

fn bitset_struct(bits: Vec<CtValue>) -> CtValue {
    CtValue::Struct {
        type_name: crate::Syntax::TYPE_BIT_SET.to_string(),
        fields: vec![("bits".to_string(), CtValue::List(bits))],
    }
}

fn bag_struct(items: Vec<CtValue>, counts: Vec<CtValue>) -> CtValue {
    CtValue::Struct {
        type_name: "Bag".to_string(),
        fields: vec![
            ("items".to_string(), CtValue::List(items)),
            ("counts".to_string(), CtValue::List(counts)),
        ],
    }
}

fn lru_struct(capacity: i64, entries: Vec<CtValue>) -> CtValue {
    CtValue::Struct {
        type_name: crate::Syntax::TYPE_LRU.to_string(),
        fields: vec![
            ("capacity".to_string(), CtValue::Int(capacity)),
            ("entries".to_string(), CtValue::List(entries)),
        ],
    }
}

fn byte_buffer_struct(bytes: Vec<u8>) -> CtValue {
    byte_buffer_struct_at(bytes, 0)
}

fn byte_buffer_struct_at(bytes: Vec<u8>, pos: usize) -> CtValue {
    CtValue::Struct {
        type_name: crate::Syntax::TYPE_BYTE_BUFFER.to_string(),
        fields: vec![
            ("bytes".to_string(), CtValue::Bytes(bytes)),
            ("pos".to_string(), CtValue::Int(pos as i64)),
        ],
    }
}

fn deque_struct(items: Vec<CtValue>) -> CtValue {
    CtValue::Struct {
        type_name: crate::Syntax::TYPE_DEQUE.to_string(),
        fields: vec![("items".to_string(), CtValue::List(items))],
    }
}

fn pool_struct() -> CtValue {
    CtValue::Struct {
        type_name: crate::Syntax::MEM_POOL.to_string(),
        fields: vec![
            ("slots".to_string(), CtValue::List(Vec::new())),
            ("free".to_string(), CtValue::List(Vec::new())),
        ],
    }
}

/// `Set.from(list)` / `SortedSet.from(list)` / `PriorityQueue.from(list)` /
/// `Deque.init(list)` — recv is the list (TIR lowering).
pub fn from_list(type_name: &str, list: &CtValue, span: Span) -> Result<CtValue, Diagnostic> {
    let CtValue::List(items) = list else {
        return Err(unsupported(
            &format!("{type_name}.from with a non-list"),
            span,
        ));
    };
    let items = items.clone();
    match type_name {
        name if name == crate::Syntax::TYPE_SET => Ok(set_struct(name, unique_values(items))),
        name if name == crate::Syntax::TYPE_SORTED_SET => {
            Ok(set_struct(name, sorted_unique(items, span)?))
        }
        name if name == crate::Syntax::TYPE_PRIORITY_QUEUE => {
            Ok(set_struct(name, sorted_descending(items, span)?))
        }
        name if name == crate::Syntax::TYPE_DEQUE => Ok(deque_struct(items)),
        _ => Err(unsupported(
            &format!("{type_name}.from at compile time"),
            span,
        )),
    }
}

pub fn byte_buffer_from(bytes: &CtValue, span: Span) -> Result<CtValue, Diagnostic> {
    Ok(byte_buffer_struct(as_bytes(bytes, span)?))
}

/// Prelude `StaticCall` constructors lowered from `Type.new()`.
pub fn prelude_new(path: &str, args: Vec<CtValue>, span: Span) -> Option<Result<CtValue, Diagnostic>> {
    Some(match path {
        "JetBitSet" => Ok(bitset_struct(Vec::new())),
        "JetByteBuffer" => {
            let capacity = match args.into_iter().next() {
                Some(v) => match as_int(&v, span) {
                    Ok(n) => n.max(0) as usize,
                    Err(e) => return Some(Err(e)),
                },
                None => 0,
            };
            let mut bytes = Vec::new();
            bytes.reserve(capacity);
            Ok(byte_buffer_struct(bytes))
        }
        "JetCache" => {
            let capacity = match args.into_iter().next() {
                Some(v) => match as_int(&v, span) {
                    Ok(n) => n.max(0),
                    Err(e) => return Some(Err(e)),
                },
                None => 0,
            };
            Ok(lru_struct(capacity, Vec::new()))
        }
        "std::collections::VecDeque" => Ok(deque_struct(Vec::new())),
        // Bag.new → HashMap (Map literals use MapLit, not this path).
        "std::collections::HashMap" => Ok(bag_struct(Vec::new(), Vec::new())),
        // #1478: `Set.new()` at this tier — the tier1 native path
        // (`crates/jet-jit/.../lower_ctx.rs`) already builds an empty
        // HashSet handle; this closes the same construct for the canonical
        // TIR evaluator (comptime + `jet run` deopt), matching `BTreeSet`
        // just below (I9 — no tier left calling this an unsupported prelude
        // static once tier1 already ships it natively).
        "std::collections::HashSet" => Ok(set_struct(crate::Syntax::TYPE_SET, Vec::new())),
        "std::collections::BTreeSet" => Ok(set_struct(crate::Syntax::TYPE_SORTED_SET, Vec::new())),
        "std::collections::BinaryHeap" => {
            Ok(set_struct(crate::Syntax::TYPE_PRIORITY_QUEUE, Vec::new()))
        }
        "jet_std::JetPool" => Ok(pool_struct()),
        _ => return None,
    })
}

/// Non-mutating collection methods. `contains` aliases `has` for set-like types.
pub fn apply_method(
    recv: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let CtValue::Struct { type_name, fields } = recv else {
        return None;
    };
    let method = if method == "contains"
        && matches!(type_name.as_str(), "Set" | "SortedSet" | "BitSet")
    {
        "has"
    } else {
        method
    };

    if type_name == "Bag" {
        return Some(bag_method(fields, method, args, span));
    }
    if type_name == crate::Syntax::TYPE_SET {
        return Some(set_method(crate::Syntax::TYPE_SET, fields, method, args, span, false));
    }
    if type_name == crate::Syntax::TYPE_SORTED_SET {
        return Some(set_method(
            crate::Syntax::TYPE_SORTED_SET,
            fields,
            method,
            args,
            span,
            true,
        ));
    }
    if type_name == crate::Syntax::TYPE_PRIORITY_QUEUE {
        return Some(priority_queue_method(fields, method, args, span));
    }
    if type_name == crate::Syntax::TYPE_BIT_SET {
        return Some(bitset_method(fields, method, args, span));
    }
    if type_name == crate::Syntax::TYPE_DEQUE {
        return Some(deque_method(fields, method, args, span));
    }
    if type_name == crate::Syntax::TYPE_LRU {
        return Some(lru_method(fields, method, args, span));
    }
    if type_name == crate::Syntax::TYPE_BYTE_BUFFER {
        return Some(byte_buffer_method(fields, method, args, span));
    }
    None
}

/// Mutating collection methods — rewrite `recv` in place when needed.
pub fn apply_mutating(
    recv: &mut CtValue,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let CtValue::Struct { type_name, .. } = &*recv else {
        return None;
    };
    let type_name = type_name.clone();
    // PriorityQueue uses ordinary push/pop names.
    let handled = matches!(
        (type_name.as_str(), method),
        ("Bag", "add" | "remove" | "clear")
            | ("Set", "add" | "remove" | "pop" | "clear")
            | ("SortedSet", "add" | "remove" | "clear")
            | ("PriorityQueue", "push" | "pop" | "clear" | "remove")
            | ("BitSet", "add" | "remove" | "clear")
            | ("Deque", "push_front" | "push_back" | "pop_front" | "pop_back" | "clear")
            | ("Cache", "add" | "add_new" | "get" | "remove" | "clear")
            | (
                "ByteBuffer",
                "clear"
                    | "write_u8"
                    | "write_u16_le"
                    | "write_u16_be"
                    | "write_u32_le"
                    | "write_u32_be"
                    | "write_u64_le"
                    | "write_u64_be"
                    | "write_bytes"
            )
    );
    if !handled {
        return None;
    }

    let peek = recv.clone();
    let CtValue::Struct { fields, .. } = &peek else {
        return None;
    };
    let result = match type_name.as_str() {
        "Bag" => bag_mutating(recv, fields, method, &args, span),
        "Set" => set_mutating(recv, crate::Syntax::TYPE_SET, fields, method, &args, span, false),
        "SortedSet" => set_mutating(
            recv,
            crate::Syntax::TYPE_SORTED_SET,
            fields,
            method,
            &args,
            span,
            true,
        ),
        "PriorityQueue" => priority_queue_mutating(recv, fields, method, &args, span),
        "BitSet" => bitset_mutating(recv, fields, method, &args, span),
        "Deque" => deque_mutating(recv, fields, method, &args, span),
        "Cache" => lru_mutating(recv, fields, method, &args, span),
        "ByteBuffer" => byte_buffer_mutating(recv, fields, method, &args, span),
        _ => return None,
    };
    Some(result)
}

fn bag_method(
    fields: &[(String, CtValue)],
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let items = list_field(fields, "items");
    let counts = {
        let c = list_field(fields, "counts");
        if c.is_empty() {
            vec![CtValue::Int(1); items.len()]
        } else {
            c
        }
    };
    match method {
        "len" => Ok(CtValue::Int(
            counts
                .iter()
                .filter_map(|c| match c {
                    CtValue::Int(n) => Some(*n),
                    _ => None,
                })
                .sum(),
        )),
        "is_empty" => Ok(CtValue::Bool(items.is_empty())),
        "has" => Ok(CtValue::Bool(items.contains(args.first().unwrap_or(&CtValue::Unit)))),
        "count" => Ok(CtValue::Int(
            items
                .iter()
                .position(|item| item == args.first().unwrap_or(&CtValue::Unit))
                .and_then(|index| match counts.get(index) {
                    Some(CtValue::Int(count)) => Some(*count),
                    _ => None,
                })
                .unwrap_or(0),
        )),
        _ => Err(unsupported(
            &format!("Bag.{} at compile time", method),
            span,
        )),
    }
}

fn bag_mutating(
    recv: &mut CtValue,
    fields: &[(String, CtValue)],
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let mut items = list_field(fields, "items");
    let mut counts = {
        let c = list_field(fields, "counts");
        if c.is_empty() {
            vec![CtValue::Int(1); items.len()]
        } else {
            c
        }
    };
    let result = match method {
        "clear" => {
            items.clear();
            counts.clear();
            CtValue::Unit
        }
        "add" => {
            let value = args.first().cloned().unwrap_or(CtValue::Unit);
            if let Some(index) = items.iter().position(|item| item == &value) {
                if let Some(CtValue::Int(count)) = counts.get_mut(index) {
                    *count += 1;
                }
            } else {
                items.push(value);
                counts.push(CtValue::Int(1));
            }
            CtValue::Bool(true)
        }
        "remove" => {
            let value = args.first().unwrap_or(&CtValue::Unit);
            if let Some(index) = items.iter().position(|item| item == value) {
                let last = matches!(counts.get(index), Some(CtValue::Int(1)));
                if last {
                    items.remove(index);
                    counts.remove(index);
                } else if let Some(CtValue::Int(count)) = counts.get_mut(index) {
                    *count -= 1;
                }
            }
            CtValue::Unit
        }
        _ => {
            return Err(unsupported(
                &format!("Bag.{} at compile time", method),
                span,
            ))
        }
    };
    *recv = bag_struct(items, counts);
    Ok(result)
}

fn set_method(
    type_name: &str,
    fields: &[(String, CtValue)],
    method: &str,
    args: &[CtValue],
    span: Span,
    sorted: bool,
) -> Result<CtValue, Diagnostic> {
    let items = list_field(fields, "items");
    match method {
        "len" => Ok(CtValue::Int(items.len() as i64)),
        "is_empty" => Ok(CtValue::Bool(items.is_empty())),
        "has" => Ok(CtValue::Bool(
            items.contains(args.first().unwrap_or(&CtValue::Unit)),
        )),
        "to_list" => Ok(CtValue::List(items)),
        // #1478: `Set.values()` — Iter is List-shaped at this eval tier
        // (see the `take`/`dedup` note in Builtins.rs), so this is `to_list`.
        "values" => Ok(CtValue::List(items)),
        // D-SET-DECLINE1=C: `sort`/`shuffle` — same to-list-then-List
        // machinery as `to_list`/`values` above, never mutating the Set.
        "sort" => {
            let mut sorted = items;
            sorted.sort_by(|a, b| {
                super::Builtins::cmp(a.clone(), b.clone(), span)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            Ok(CtValue::List(sorted))
        }
        // Same Fisher-Yates + fixed-seed PCG stream `List.shuffle()` runs
        // (`(CtValue::List(xs), "shuffle")` in Builtins.rs) — deterministic
        // and uniform regardless of the Set's internal walk order.
        "shuffle" => {
            let mut out = items;
            let mut state: u64 = 0xC0FF_EE42;
            for i in (1..out.len()).rev() {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                let j = ((state >> 33) as usize) % (i + 1);
                out.swap(i, j);
            }
            Ok(CtValue::List(out))
        }
        "copy" | "to_set" => Ok(set_struct(type_name, items)),
        "capacity" => Ok(CtValue::Int(items.len() as i64)),
        "equal" => {
            let other = args.first().ok_or_else(|| {
                unsupported(&format!("{type_name}.equal missing argument"), span)
            })?;
            let CtValue::Struct {
                type_name: other_type,
                fields: other_fields,
            } = other
            else {
                return Err(unsupported(
                    &format!("{type_name}.equal with a non-set"),
                    span,
                ));
            };
            if other_type != type_name {
                return Ok(CtValue::Bool(false));
            }
            let other_items = list_field(other_fields, "items");
            Ok(CtValue::Bool(set_semantics::jet_set_is_subset_by(
                &items, &other_items, |a, b| a == b,
            ) && set_semantics::jet_set_is_subset_by(
                &other_items, &items, |a, b| a == b,
            )))
        }
        "first" => Ok(items
            .first()
            .cloned()
            .map_or_else(option_none, |v| CtValue::Present(Box::new(v)))),
        "last" if sorted => Ok(items
            .last()
            .cloned()
            .map_or_else(option_none, |v| CtValue::Present(Box::new(v)))),
        "union" => {
            let other = args.first().ok_or_else(|| {
                unsupported(&format!("{type_name}.union missing argument"), span)
            })?;
            let CtValue::Struct {
                type_name: other_type,
                fields: other_fields,
            } = other
            else {
                return Err(unsupported(
                    &format!("{type_name}.union with a non-set"),
                    span,
                ));
            };
            if other_type != type_name {
                return Err(unsupported(
                    &format!("{type_name}.union with a non-set"),
                    span,
                ));
            }
            let other_items = list_field(other_fields, "items");
            let merged = set_semantics::jet_set_union_by(&items, &other_items, |left, right| left == right);
            let merged = if sorted {
                sorted_unique(merged, span)?
            } else {
                merged
            };
            Ok(set_struct(type_name, merged))
        }
        "intersection" | "difference" | "symmetric_difference" | "is_subset"
        | "is_superset" | "is_disjoint" => {
            let other = args.first().ok_or_else(|| {
                unsupported(&format!("{type_name}.{method} missing argument"), span)
            })?;
            let CtValue::Struct {
                type_name: other_type,
                fields: other_fields,
            } = other
            else {
                return Err(unsupported(
                    &format!("{type_name}.{method} with a non-set"),
                    span,
                ));
            };
            if other_type != type_name {
                return Err(unsupported(
                    &format!("{type_name}.{method} with a non-set"),
                    span,
                ));
            }
            let other_items = list_field(other_fields, "items");
            let equal = |left: &CtValue, right: &CtValue| left == right;
            match method {
                "is_subset" => Ok(CtValue::Bool(set_semantics::jet_set_is_subset_by(
                    &items, &other_items, equal,
                ))),
                "is_superset" => Ok(CtValue::Bool(set_semantics::jet_set_is_superset_by(
                    &items, &other_items, equal,
                ))),
                "is_disjoint" => Ok(CtValue::Bool(set_semantics::jet_set_is_disjoint_by(
                    &items, &other_items, equal,
                ))),
                "intersection" => {
                    let values = set_semantics::jet_set_intersection_by(&items, &other_items, equal);
                    Ok(set_struct(type_name, if sorted { sorted_unique(values, span)? } else { values }))
                }
                "difference" => {
                    let values = set_semantics::jet_set_difference_by(&items, &other_items, equal);
                    Ok(set_struct(type_name, if sorted { sorted_unique(values, span)? } else { values }))
                }
                "symmetric_difference" => {
                    let values = set_semantics::jet_set_symmetric_difference_by(&items, &other_items, equal);
                    Ok(set_struct(type_name, if sorted { sorted_unique(values, span)? } else { values }))
                }
                _ => unreachable!(),
            }
        }
        _ => Err(unsupported(
            &format!("{type_name}.{} at compile time", method),
            span,
        )),
    }
}

fn set_mutating(
    recv: &mut CtValue,
    type_name: &str,
    fields: &[(String, CtValue)],
    method: &str,
    args: &[CtValue],
    span: Span,
    sorted: bool,
) -> Result<CtValue, Diagnostic> {
    let mut items = list_field(fields, "items");
    let result = match method {
        "clear" => {
            items.clear();
            CtValue::Unit
        }
        "add" => {
            let value = args.first().cloned().unwrap_or(CtValue::Unit);
            let added = !items.contains(&value);
            if added {
                items.push(value);
                if sorted {
                    items = sorted_unique(items, span)?;
                }
            }
            CtValue::Bool(added)
        }
        "remove" => {
            let value = args.first().unwrap_or(&CtValue::Unit);
            if let Some(index) = items.iter().position(|item| item == value) {
                items.remove(index);
            }
            CtValue::Unit
        }
        // D-ONCE-VERB1=A: remove-and-return is `pop` on every collection.
        "pop" => {
            let value = args.first().unwrap_or(&CtValue::Unit);
            match collection_semantics::set_pop(&mut items, value) {
                Some(value) => CtValue::Present(Box::new(value)),
                None => option_none(),
            }
        }
        _ => {
            return Err(unsupported(
                &format!("{type_name}.{} at compile time", method),
                span,
            ))
        }
    };
    *recv = set_struct(type_name, items);
    Ok(result)
}

fn priority_queue_method(
    fields: &[(String, CtValue)],
    method: &str,
    _args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let items = list_field(fields, "items");
    match method {
        "len" => Ok(CtValue::Int(items.len() as i64)),
        "is_empty" => Ok(CtValue::Bool(items.is_empty())),
        "peek" => Ok(items
            .first()
            .cloned()
            .map_or_else(option_none, |v| CtValue::Present(Box::new(v)))),
        "to_sorted_list" => Ok(CtValue::List(items)),
        _ => Err(unsupported(
            &format!("PriorityQueue.{} at compile time", method),
            span,
        )),
    }
}

fn priority_queue_mutating(
    recv: &mut CtValue,
    fields: &[(String, CtValue)],
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let mut items = list_field(fields, "items");
    let result = match method {
        "clear" => {
            items.clear();
            CtValue::Unit
        }
        "push" => {
            items.push(args.first().cloned().unwrap_or(CtValue::Unit));
            items = sorted_descending(items, span)?;
            CtValue::Unit
        }
        "pop" => match collection_semantics::priority_queue_pop(&mut items) {
            Some(value) => CtValue::Present(Box::new(value)),
            None => option_none(),
        },
        // D-LISTREMOVE1/F (criterion c6 on #1481): same value/slot selector
        // as `List.remove`, over the same highest-first order `push` already
        // maintains — matches the AOT/JIT `BinaryHeap::into_sorted_vec().rev()`
        // order so `.Slot` means the same position on every tier (I9).
        "remove" => {
            let by_slot = matches!(
                args.get(1),
                Some(CtValue::Enum { variant, .. }) if variant == "Slot"
            );
            if by_slot {
                let i = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
                if i < 0 || i as usize >= items.len() {
                    return Err(index_oob(items.len(), i, span));
                }
                CtValue::Present(Box::new(items.remove(i as usize)))
            } else {
                let value = args.first().cloned().unwrap_or(CtValue::Unit);
                match items.iter().position(|item| *item == value) {
                    Some(index) => CtValue::Present(Box::new(items.remove(index))),
                    None => CtValue::absent(items.first().map(|item| item.jet_type()).unwrap_or(Type::Int)),
                }
            }
        }
        _ => {
            return Err(unsupported(
                &format!("PriorityQueue.{} at compile time", method),
                span,
            ))
        }
    };
    *recv = set_struct(crate::Syntax::TYPE_PRIORITY_QUEUE, items);
    Ok(result)
}

fn bitset_method(
    fields: &[(String, CtValue)],
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let bits = list_field(fields, "bits");
    match method {
        "count" => Ok(CtValue::Int(bits.len() as i64)),
        "len" => Ok(CtValue::Int(match bits.last() {
            Some(CtValue::Int(bit)) => bit + 1,
            _ => 0,
        })),
        "is_empty" => Ok(CtValue::Bool(bits.is_empty())),
        "has" => {
            let bit = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            Ok(CtValue::Bool(bits.contains(&CtValue::Int(bit))))
        }
        "to_list" => Ok(CtValue::List(bits)),
        _ => Err(unsupported(
            &format!("BitSet.{} at compile time", method),
            span,
        )),
    }
}

fn bitset_mutating(
    recv: &mut CtValue,
    fields: &[(String, CtValue)],
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let mut bits = list_field(fields, "bits");
    let result = match method {
        "clear" => {
            bits.clear();
            CtValue::Unit
        }
        "add" => {
            let bit = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            let added = bit >= 0 && !bits.contains(&CtValue::Int(bit));
            if added {
                bits.push(CtValue::Int(bit));
                bits = sorted_unique(bits, span)?;
            }
            CtValue::Bool(added)
        }
        "remove" => {
            let bit = CtValue::Int(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?);
            if let Some(index) = bits.iter().position(|value| value == &bit) {
                bits.remove(index);
            }
            CtValue::Unit
        }
        _ => {
            return Err(unsupported(
                &format!("BitSet.{} at compile time", method),
                span,
            ))
        }
    };
    *recv = bitset_struct(bits);
    Ok(result)
}

fn deque_method(
    fields: &[(String, CtValue)],
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let items = list_field(fields, "items");
    match method {
        "len" | "capacity" => Ok(CtValue::Int(items.len() as i64)),
        "is_empty" => Ok(CtValue::Bool(items.is_empty())),
        "peek_front" => Ok(items
            .first()
            .cloned()
            .map_or_else(option_none, |v| CtValue::Present(Box::new(v)))),
        "peek_back" => Ok(items
            .last()
            .cloned()
            .map_or_else(option_none, |v| CtValue::Present(Box::new(v)))),
        "get" => {
            let idx = match args.first() {
                Some(CtValue::Int(i)) if *i >= 0 => *i as usize,
                _ => return Ok(option_none()),
            };
            Ok(items
                .get(idx)
                .cloned()
                .map_or_else(option_none, |v| CtValue::Present(Box::new(v))))
        }
        "contains" => {
            let needle = args.first().cloned().unwrap_or(CtValue::Unit);
            Ok(CtValue::Bool(items.iter().any(|x| x == &needle)))
        }
        "to_list" => Ok(CtValue::List(items)),
        "join" => {
            let sep = match args.first() {
                Some(CtValue::Str(s)) => s.as_str(),
                _ => "",
            };
            let parts: Vec<String> = items.iter().map(|x| x.jet_show()).collect();
            Ok(CtValue::Str(parts.join(sep)))
        }
        _ => Err(unsupported(
            &format!("Deque.{} at compile time", method),
            span,
        )),
    }
}

fn deque_mutating(
    recv: &mut CtValue,
    fields: &[(String, CtValue)],
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let mut items = list_field(fields, "items");
    let result = match method {
        "clear" => {
            items.clear();
            CtValue::Unit
        }
        "push_front" => {
            items.insert(0, args.first().cloned().unwrap_or(CtValue::Unit));
            CtValue::Unit
        }
        "push_back" => {
            items.push(args.first().cloned().unwrap_or(CtValue::Unit));
            CtValue::Unit
        }
        "pop_front" => match collection_semantics::deque_pop_front(&mut items) {
            Some(value) => CtValue::Present(Box::new(value)),
            None => option_none(),
        },
        "pop_back" => match collection_semantics::deque_pop_back(&mut items) {
            Some(value) => CtValue::Present(Box::new(value)),
            None => option_none(),
        },
        "delete" => {
            let needle = args.first().cloned().unwrap_or(CtValue::Unit);
            if let Some(i) = items.iter().position(|x| x == &needle) {
                items.remove(i);
            }
            CtValue::Unit
        }
        "reverse" => {
            items.reverse();
            CtValue::Unit
        }
        "split" => {
            let idx = match args.first() {
                Some(CtValue::Int(i)) if *i >= 0 => (*i as usize).min(items.len()),
                _ => items.len(),
            };
            let rest = items.split_off(idx);
            *recv = deque_struct(items);
            return Ok(deque_struct(rest));
        }
        _ => {
            return Err(unsupported(
                &format!("Deque.{} at compile time", method),
                span,
            ))
        }
    };
    *recv = deque_struct(items);
    Ok(result)
}

fn lru_method(
    fields: &[(String, CtValue)],
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let capacity = int_field(fields, "capacity").unwrap_or(0) as usize;
    let entries = list_field(fields, "entries");
    let key_position = |entries: &[CtValue], key: &CtValue| {
        entries
            .iter()
            .position(|entry| matches!(entry, CtValue::List(pair) if pair.first() == Some(key)))
    };
    match method {
        "len" => Ok(CtValue::Int(entries.len() as i64)),
        "is_empty" => Ok(CtValue::Bool(entries.is_empty())),
        "capacity" => Ok(CtValue::Int(capacity as i64)),
        "has_key" | "contains_key" => Ok(CtValue::Bool(
            key_position(&entries, args.first().unwrap_or(&CtValue::Unit)).is_some(),
        )),
        "keys" => Ok(CtValue::List(
            entries
                .iter()
                .filter_map(|entry| match entry {
                    CtValue::List(pair) => pair.first().cloned(),
                    _ => None,
                })
                .collect(),
        )),
        _ => Err(unsupported(
            &format!("Cache.{} at compile time", method),
            span,
        )),
    }
}

fn lru_mutating(
    recv: &mut CtValue,
    fields: &[(String, CtValue)],
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let capacity = int_field(fields, "capacity").unwrap_or(0).max(0) as usize;
    let mut entries = list_field(fields, "entries");
    let key_position = |entries: &[CtValue], key: &CtValue| {
        entries
            .iter()
            .position(|entry| matches!(entry, CtValue::List(pair) if pair.first() == Some(key)))
    };
    let key = args.first().cloned().unwrap_or(CtValue::Unit);
    let value = args.get(1).cloned().unwrap_or(CtValue::Unit);
    let result = match method {
        "clear" => {
            entries.clear();
            CtValue::Unit
        }
        "add_new" => {
            let added = capacity > 0 && key_position(&entries, &key).is_none();
            if added {
                entries.insert(0, CtValue::List(vec![key, value]));
                if entries.len() > capacity {
                    entries.pop();
                }
            }
            CtValue::Bool(added)
        }
        "add" => {
            if capacity == 0 {
                option_none()
            } else {
                let displaced = key_position(&entries, &key).map(|index| {
                    let CtValue::List(pair) = entries.remove(index) else {
                        unreachable!("Cache entries are pairs")
                    };
                    pair[1].clone()
                });
                entries.insert(0, CtValue::List(vec![key, value]));
                if entries.len() > capacity {
                    entries.pop();
                }
                displaced.map_or_else(option_none, |v| CtValue::Present(Box::new(v)))
            }
        }
        "get" => match key_position(&entries, &key) {
            Some(index) => {
                let entry = entries.remove(index);
                let CtValue::List(pair) = &entry else {
                    unreachable!("Cache entries are pairs")
                };
                let value = pair[1].clone();
                entries.insert(0, entry);
                CtValue::Present(Box::new(value))
            }
            None => option_none(),
        },
        "remove" => match key_position(&entries, &key) {
            Some(index) => {
                let CtValue::List(pair) = entries.remove(index) else {
                    unreachable!("Cache entries are pairs")
                };
                CtValue::Present(Box::new(pair[1].clone()))
            }
            None => option_none(),
        },
        _ => {
            return Err(unsupported(
                &format!("Cache.{} at compile time", method),
                span,
            ))
        }
    };
    *recv = lru_struct(capacity as i64, entries);
    Ok(result)
}

fn byte_buffer_method(
    fields: &[(String, CtValue)],
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let bytes = fields
        .iter()
        .find(|(name, _)| name == "bytes")
        .map(|(_, value)| as_bytes(value, span))
        .transpose()?
        .unwrap_or_default();
    let pos = fields
        .iter()
        .find(|(name, _)| name == "pos")
        .map(|(_, value)| as_int(value, span))
        .transpose()?
        .unwrap_or(0)
        .max(0) as usize;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    match method {
        "len" => Ok(CtValue::Int(bytes.len() as i64)),
        "capacity" => Ok(CtValue::Int(bytes.capacity() as i64)),
        "is_empty" => Ok(CtValue::Bool(bytes.is_empty())),
        "to_bytes" | "get_buffer" | "buffer" => Ok(CtValue::Bytes(bytes)),
        "position" => Ok(CtValue::Int(pos as i64)),
        "eof" => Ok(CtValue::Bool(pos >= bytes.len())),
        "to_string" | "string" => Ok(CtValue::Str(text)),
        "is_ascii" => Ok(CtValue::Bool(bytes.is_ascii())),
        "first" => Ok(match bytes.first() {
            Some(b) => CtValue::Present(Box::new(CtValue::Int(*b as i64))),
            None => CtValue::absent(Type::Int),
        }),
        "get" => {
            let index = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            Ok(if index < 0 {
                CtValue::absent(Type::Int)
            } else {
                match bytes.get(index as usize) {
                    Some(b) => CtValue::Present(Box::new(CtValue::Int(*b as i64))),
                    None => CtValue::absent(Type::Int),
                }
            })
        }
        "contains" => {
            let needle = as_string(args.first().unwrap_or(&CtValue::Str(String::new())), span)?;
            Ok(CtValue::Bool(text.contains(&needle)))
        }
        "starts_with" => {
            let prefix = as_string(args.first().unwrap_or(&CtValue::Str(String::new())), span)?;
            Ok(CtValue::Bool(text.starts_with(&prefix)))
        }
        "ends_with" => {
            let suffix = as_string(args.first().unwrap_or(&CtValue::Str(String::new())), span)?;
            Ok(CtValue::Bool(text.ends_with(&suffix)))
        }
        "trim" => Ok(byte_buffer_struct(text.trim().as_bytes().to_vec())),
        "trim_start" => Ok(byte_buffer_struct(text.trim_start().as_bytes().to_vec())),
        "trim_end" => Ok(byte_buffer_struct(text.trim_end().as_bytes().to_vec())),
        "to_lower" => Ok(byte_buffer_struct(text.to_lowercase().into_bytes())),
        "to_upper" => Ok(byte_buffer_struct(text.to_uppercase().into_bytes())),
        "to_title" | "title" => {
            let mut out = String::with_capacity(text.len());
            let mut start = true;
            for ch in text.chars() {
                if ch.is_whitespace() {
                    start = true;
                    out.push(ch);
                } else if start {
                    for c in ch.to_uppercase() {
                        out.push(c);
                    }
                    start = false;
                } else {
                    for c in ch.to_lowercase() {
                        out.push(c);
                    }
                }
            }
            Ok(byte_buffer_struct(out.into_bytes()))
        }
        "clone" | "copy" => Ok(byte_buffer_struct_at(bytes, pos)),
        "lines" => Ok(CtValue::List(
            text.lines().map(|s| CtValue::Str(s.to_string())).collect(),
        )),
        "index_of" => {
            let needle = as_string(args.first().unwrap_or(&CtValue::Str(String::new())), span)?;
            Ok(match text.find(&needle) {
                Some(i) => CtValue::Present(Box::new(CtValue::Int(i as i64))),
                None => CtValue::absent(Type::Int),
            })
        }
        "last_index_of" => {
            let needle = as_string(args.first().unwrap_or(&CtValue::Str(String::new())), span)?;
            Ok(match text.rfind(&needle) {
                Some(i) => CtValue::Present(Box::new(CtValue::Int(i as i64))),
                None => CtValue::absent(Type::Int),
            })
        }
        "split" => {
            let sep = as_string(args.first().unwrap_or(&CtValue::Str(String::new())), span)?;
            Ok(CtValue::List(
                text.split(&sep).map(|s| CtValue::Str(s.to_string())).collect(),
            ))
        }
        "replace" => {
            let from = as_string(args.first().unwrap_or(&CtValue::Str(String::new())), span)?;
            let to = as_string(args.get(1).unwrap_or(&CtValue::Str(String::new())), span)?;
            Ok(byte_buffer_struct(text.replace(&from, &to).into_bytes()))
        }
        "join" => {
            let parts = match args.first() {
                Some(CtValue::List(xs)) => xs
                    .iter()
                    .map(|x| as_string(x, span))
                    .collect::<Result<Vec<_>, _>>()?,
                _ => Vec::new(),
            };
            Ok(byte_buffer_struct(parts.join(&text).into_bytes()))
        }
        "equal" => {
            let other = match args.first() {
                Some(CtValue::Struct { fields, .. }) => fields
                    .iter()
                    .find(|(n, _)| n == "bytes")
                    .map(|(_, v)| as_bytes(v, span))
                    .transpose()?
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            Ok(CtValue::Bool(bytes == other))
        }
        "compare" => {
            let other = match args.first() {
                Some(CtValue::Struct { fields, .. }) => fields
                    .iter()
                    .find(|(n, _)| n == "bytes")
                    .map(|(_, v)| as_bytes(v, span))
                    .transpose()?
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            Ok(CtValue::Int(match bytes.cmp(&other) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }))
        }
        "parse" => match text.trim().parse::<i64>() {
            Ok(n) => Ok(CtValue::Present(Box::new(CtValue::Int(n)))),
            Err(e) => Ok(CtValue::failed(Box::new(CtValue::Str(e.to_string())))),
        },
        _ => Err(unsupported(
            &format!("ByteBuffer.{} at compile time", method),
            span,
        )),
    }
}

fn byte_buffer_mutating(
    recv: &mut CtValue,
    fields: &[(String, CtValue)],
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let mut bytes = fields
        .iter()
        .find(|(name, _)| name == "bytes")
        .map(|(_, value)| as_bytes(value, span))
        .transpose()?
        .unwrap_or_default();
    let mut pos = fields
        .iter()
        .find(|(name, _)| name == "pos")
        .map(|(_, value)| as_int(value, span))
        .transpose()?
        .unwrap_or(0)
        .max(0) as usize;
    let result = match method {
        "clear" | "close" | "shutdown" => {
            bytes.clear();
            pos = 0;
            CtValue::Unit
        }
        "flush" => CtValue::Unit,
        "rewind" => {
            pos = 0;
            CtValue::Unit
        }
        "seek" => {
            let index = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            pos = if index <= 0 {
                0
            } else if (index as usize) > bytes.len() {
                bytes.len()
            } else {
                index as usize
            };
            CtValue::Unit
        }
        "write_bytes" | "write" => {
            bytes.extend(as_bytes(
                args.first().unwrap_or(&CtValue::Bytes(vec![])),
                span,
            )?);
            CtValue::Unit
        }
        "write_u8" | "write_byte" => {
            bytes.push(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as u8);
            CtValue::Unit
        }
        "write_u16_le" => {
            bytes.extend_from_slice(
                &(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as u16).to_le_bytes(),
            );
            CtValue::Unit
        }
        "write_u16_be" => {
            bytes.extend_from_slice(
                &(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as u16).to_be_bytes(),
            );
            CtValue::Unit
        }
        "write_u32_le" => {
            bytes.extend_from_slice(
                &(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as u32).to_le_bytes(),
            );
            CtValue::Unit
        }
        "write_u32_be" => {
            bytes.extend_from_slice(
                &(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as u32).to_be_bytes(),
            );
            CtValue::Unit
        }
        "write_u64_le" => {
            bytes.extend_from_slice(
                &(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as u64).to_le_bytes(),
            );
            CtValue::Unit
        }
        "write_u64_be" => {
            bytes.extend_from_slice(
                &(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as u64).to_be_bytes(),
            );
            CtValue::Unit
        }
        "read_byte" | "next" => {
            if pos >= bytes.len() {
                CtValue::absent(Type::Int)
            } else {
                let b = bytes[pos];
                pos += 1;
                CtValue::Present(Box::new(CtValue::Int(b as i64)))
            }
        }
        "read" => {
            if pos >= bytes.len() {
                CtValue::absent(Type::List(Box::new(Type::IntN { signed: false, bits: 8 })))
            } else {
                let out = bytes[pos..].to_vec();
                pos = bytes.len();
                CtValue::Present(Box::new(CtValue::Bytes(out)))
            }
        }
        "read_bytes" => {
            let n = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            if n < 0 || pos + (n as usize) > bytes.len() {
                CtValue::absent(Type::List(Box::new(Type::IntN { signed: false, bits: 8 })))
            } else {
                let out = bytes[pos..pos + n as usize].to_vec();
                pos += n as usize;
                CtValue::Present(Box::new(CtValue::Bytes(out)))
            }
        }
        "read_string" => {
            let n = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            if n < 0 || pos + (n as usize) > bytes.len() {
                CtValue::absent(Type::String)
            } else {
                let out = bytes[pos..pos + n as usize].to_vec();
                pos += n as usize;
                CtValue::Present(Box::new(CtValue::Str(
                    String::from_utf8_lossy(&out).into_owned(),
                )))
            }
        }
        "copy_to" | "write_to" => {
            // Mutating methods with a second buffer are runtime-only.
            return Err(unsupported(
                &format!("ByteBuffer.{} at compile time", method),
                span,
            ));
        }
        _ => {
            return Err(unsupported(
                &format!("ByteBuffer.{} at compile time", method),
                span,
            ))
        }
    };
    *recv = byte_buffer_struct_at(bytes, pos);
    Ok(result)
}
