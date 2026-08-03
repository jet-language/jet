//! Comptime/TIR-eval collection ops (#722 / #777). Same CtValue shapes as
//! `Methods/dispatch/eval_method.rs` — one table for TirBridge + old helpers.

use crate::AST::Type;
use crate::Diagnostics::{Diagnostic, Span};

use super::Builtins::{as_int, cmp};
use super::Diagnostics::unsupported;
use super::Value::CtValue;

mod set_semantics {
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

fn as_bytes(v: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
    match v {
        CtValue::Bytes(bs) => Ok(bs.clone()),
        CtValue::List(xs) => xs
            .iter()
            .map(|x| match x {
                CtValue::Int(n) if (0..=255).contains(n) => Ok(*n as u8),
                _ => Err(unsupported(
                    "a `[U8]` list with an out-of-range element",
                    span,
                )),
            })
            .collect(),
        _ => Err(unsupported("non-`[U8]` argument to ByteBuffer", span)),
    }
}

fn option_none() -> CtValue {
    CtValue::None(Type::Int)
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
    CtValue::Struct {
        type_name: crate::Syntax::TYPE_BYTE_BUFFER.to_string(),
        fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))],
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

/// `Set.from(list)` / `SortedSet.from(list)` / `PriorityQueue.from(list)` —
/// recv is the list (TIR lowering).
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
        "JetByteBuffer" => Ok(byte_buffer_struct(Vec::new())),
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
            | ("Set", "add" | "remove" | "clear")
            | ("SortedSet", "add" | "remove" | "clear")
            | ("PriorityQueue", "push" | "pop" | "clear")
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
        "first" if sorted => Ok(items
            .first()
            .cloned()
            .map_or_else(option_none, |v| CtValue::Some(Box::new(v)))),
        "last" if sorted => Ok(items
            .last()
            .cloned()
            .map_or_else(option_none, |v| CtValue::Some(Box::new(v)))),
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
            let mut merged = items;
            merged.extend(list_field(other_fields, "items"));
            let merged = if sorted {
                sorted_unique(merged, span)?
            } else {
                unique_values(merged)
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
            .map_or_else(option_none, |v| CtValue::Some(Box::new(v)))),
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
        "pop" if items.is_empty() => option_none(),
        "pop" => CtValue::Some(Box::new(items.remove(0))),
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
    _args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let items = list_field(fields, "items");
    match method {
        "len" => Ok(CtValue::Int(items.len() as i64)),
        "is_empty" => Ok(CtValue::Bool(items.is_empty())),
        "peek_front" => Ok(items
            .first()
            .cloned()
            .map_or_else(option_none, |v| CtValue::Some(Box::new(v)))),
        "peek_back" => Ok(items
            .last()
            .cloned()
            .map_or_else(option_none, |v| CtValue::Some(Box::new(v)))),
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
        "pop_front" if items.is_empty() => option_none(),
        "pop_front" => CtValue::Some(Box::new(items.remove(0))),
        "pop_back" => match items.pop() {
            Some(value) => CtValue::Some(Box::new(value)),
            None => option_none(),
        },
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
                displaced.map_or_else(option_none, |v| CtValue::Some(Box::new(v)))
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
                CtValue::Some(Box::new(value))
            }
            None => option_none(),
        },
        "remove" => match key_position(&entries, &key) {
            Some(index) => {
                let CtValue::List(pair) = entries.remove(index) else {
                    unreachable!("Cache entries are pairs")
                };
                CtValue::Some(Box::new(pair[1].clone()))
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
    _args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let bytes = fields
        .iter()
        .find(|(name, _)| name == "bytes")
        .map(|(_, value)| as_bytes(value, span))
        .transpose()?
        .unwrap_or_default();
    match method {
        "len" => Ok(CtValue::Int(bytes.len() as i64)),
        "is_empty" => Ok(CtValue::Bool(bytes.is_empty())),
        "to_bytes" => Ok(CtValue::Bytes(bytes)),
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
    match method {
        "clear" => bytes.clear(),
        "write_bytes" => bytes.extend(as_bytes(args.first().unwrap_or(&CtValue::Bytes(vec![])), span)?),
        "write_u8" => bytes.push(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as u8),
        "write_u16_le" => {
            bytes.extend_from_slice(&(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as u16).to_le_bytes())
        }
        "write_u16_be" => {
            bytes.extend_from_slice(&(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as u16).to_be_bytes())
        }
        "write_u32_le" => {
            bytes.extend_from_slice(&(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as u32).to_le_bytes())
        }
        "write_u32_be" => {
            bytes.extend_from_slice(&(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as u32).to_be_bytes())
        }
        "write_u64_le" => {
            bytes.extend_from_slice(&(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as u64).to_le_bytes())
        }
        "write_u64_be" => {
            bytes.extend_from_slice(&(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as u64).to_be_bytes())
        }
        _ => {
            return Err(unsupported(
                &format!("ByteBuffer.{} at compile time", method),
                span,
            ))
        }
    }
    *recv = byte_buffer_struct(bytes);
    Ok(CtValue::Unit)
}
