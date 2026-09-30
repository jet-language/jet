//! Comptime/MIR-eval collection ops (#722 / #777). Same CtValue shapes as
//! `Methods/dispatch/eval_method.rs` — one table for MirBridge + old helpers.

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::Type;

use super::Builtins::{as_int, cmp_for_sort};
use super::Diagnostics::{index_oob, unsupported};
use crate::AST::CtValue;
use jet_foundation::Prelude::jet_as_bytes as as_bytes;

#[allow(dead_code, non_camel_case_types, unused_imports)]
pub(crate) mod collection_semantics {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    use jet_foundation::StructuralDebug::{jet_debug_map, jet_debug_optional, jet_debug_range};

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

    /// By-value take of the map entries (`Prelude/Core.rs` spelling); this
    /// mirror owns its storage outright, so it simply drains it.
    fn jet_map_into_entries<K: Ord + Clone, V: Clone>(m: JetMap<K, V>) -> Vec<(K, V)> {
        m.0.into_iter().collect()
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

    // BTreeMap has no stable fallible reservation API. Keep this representation
    // step at the map seam; the shared Prelude owns the AllocError projection.
    fn jet_map_try_insert_storage<K: Ord + Clone, V: Clone>(
        map: &mut JetMap<K, V>,
        key: K,
        value: V,
    ) -> Result<Option<V>, ()> {
        Ok(map.insert(key, value))
    }

    fn jet_panic(_file: &str, _line: u32, message: &str) -> ! {
        panic!("{}", message);
    }
    // Collections.rs is also embedded in this generic comptime host. Use the
    // shared registry-backed renderer so rich stops retain their code, policy,
    // and source context instead of falling back to a generic E3001 panic.
    #[allow(dead_code)]
    fn jet_panic_rich_code(
        code: &'static str,
        file: &str,
        line: u32,
        fn_name: &str,
        src_line: &str,
        col: u32,
        caret_len: u32,
        message: &str,
        locals: &str,
    ) -> ! {
        let report = jet_foundation::Outcome::jet_render_runtime_stop(
            code, file, line, fn_name, src_line, col, caret_len, message, locals,
        );
        panic!("{}", report.rendered);
    }

    #[allow(dead_code)]
    fn jet_panic_rich(
        file: &str,
        line: u32,
        fn_name: &str,
        src_line: &str,
        col: u32,
        caret_len: u32,
        message: &str,
        locals: &str,
    ) -> ! {
        jet_panic_rich_code(
            "E3001", file, line, fn_name, src_line, col, caret_len, message, locals,
        )
    }

    // Comptime evaluates collection expressions outside the runtime #Test
    // harness. The runtime Prelude owns the active fault scheduler.
    fn jet_fault_should_fail_allocation() -> bool {
        false
    }
    // Values.rs is shared with the AOT Prelude; bind its unqualified Int
    // formatter to the comptime mirror before including that fragment.
    use crate::Comptime::SyncLite::jet_int_to_string;
    include!("../../../jet-codegen/src/Prelude/Core/Loadable.rs");
    include!("../../../jet-codegen/src/Prelude/Core/RangeBounds.rs");
    include!("../../../jet-codegen/src/Prelude/Core/Values.rs");
    // D-DATAFRAME1 / card #2447: comptime evaluation consumes the same typed
    // table-plan fact kernel as AOT and JIT; it owns no alternate plan shape.
    include!("../../../jet-codegen/src/Prelude/Core/LazyTablePlan.rs");
    include!("../../../jet-codegen/src/Prelude/Core/TextValues.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/JetStd/Iter.rs");
    include!("../../../jet-codegen/src/Prelude/Memo.rs");
    include!("../../../jet-codegen/src/Prelude/Core/SimdLanes.rs");
    include!("../../../jet-codegen/src/Prelude/Core/CollectionFailure.rs");
    include!("../../../jet-codegen/src/Prelude/Core/SortKernel.rs");
    include!("../../../jet-codegen/src/Prelude/Core/Collections.rs");

    pub struct LoopListCursor<T> {
        cursor: JetLoopIterCursor,
        started: bool,
        item: std::marker::PhantomData<T>,
    }

    impl<T> std::fmt::Debug for LoopListCursor<T> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.debug_struct("LoopListCursor").finish_non_exhaustive()
        }
    }

    impl<T: 'static> LoopListCursor<T> {
        pub fn new(
            values: Vec<T>,
            step: Option<i64>,
        ) -> Result<Self, &'static str> {
            jet_loop_iter_init_checked(
                values,
                step.unwrap_or(0),
                step.is_some(),
                true,
                JetLoopSourceKind::Plain,
            )
            .map(|cursor| Self {
                cursor,
                started: false,
                item: std::marker::PhantomData,
            })
        }
    }

    impl<T: 'static> Iterator for LoopListCursor<T> {
        type Item = T;

        fn next(&mut self) -> Option<Self::Item> {
            // Skip/drop subsequent items only after the current loop body.
            if self.started {
                jet_loop_iter_advance(&mut self.cursor);
            } else {
                self.started = true;
            }
            if !jet_loop_iter_has_next(&self.cursor) {
                return None;
            }
            Some(jet_loop_iter_value(&mut self.cursor))
        }
    }

    pub(super) fn list_pop<T>(values: &mut Vec<T>) -> Option<T> {
        jet_list_pop_kernel(values).ok()
    }

    pub(super) fn list_insert<T>(
        values: &mut Vec<T>,
        index: i64,
        value: T,
    ) -> Result<(), JetListInsertError> {
        jet_list_insert_kernel(values, index, value)
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

    pub(super) fn try_list_new<T>() -> JetOutcome<Vec<T>, AllocError> {
        jet_list_try_new()
    }

    pub(super) fn try_list_with_capacity<T>(capacity: i64) -> JetOutcome<Vec<T>, AllocError> {
        jet_list_try_with_capacity(capacity)
    }

    pub(super) fn try_list_with_capacity_defaulted<T>(
        capacity: i64,
        program_allocator_reserve: impl FnOnce(usize) -> bool,
        program_allocator_cancel: impl FnOnce(usize),
    ) -> JetOutcome<Vec<T>, AllocError> {
        jet_list_try_with_capacity_defaulted(
            capacity,
            program_allocator_reserve,
            program_allocator_cancel,
        )
    }

    pub(super) fn try_list_push<T>(values: &mut Vec<T>, value: T) -> JetOutcome<(), AllocError> {
        jet_list_try_push(values, value)
    }

    pub(super) fn try_list_reserve<T>(
        values: &mut Vec<T>,
        additional: i64,
    ) -> JetOutcome<(), AllocError> {
        jet_list_try_reserve(values, additional)
    }

    pub(super) fn try_map_insert<K: Ord + Clone, V: Clone>(
        map: &mut std::collections::BTreeMap<K, V>,
        key: K,
        value: V,
    ) -> JetOutcome<Option<V>, AllocError> {
        let mut native = JetMap(std::mem::take(map));
        let result = jet_map_try_insert(&mut native, key, value);
        *map = native.0;
        result
    }

    pub(super) fn try_string_push(text: &mut String, addition: &str) -> JetOutcome<(), AllocError> {
        jet_string_try_push(text, addition)
    }

    pub(super) fn iter_first<T: 'static>(values: Vec<T>) -> Option<T> {
        jet_iter_first(jet_iter_from_vec(values)).ok()
    }

    pub(super) fn iter_skip<T: 'static>(values: Vec<T>, n: i64) -> Vec<T> {
        jet_iter_skip(jet_iter_from_vec(values), n).to_list()
    }

    pub fn try_collect<T, E>(values: impl IntoIterator<Item = Result<T, E>>) -> Result<Vec<T>, E> {
        jet_list_try_collect(values)
    }

    pub(super) fn sequence_argument_message(method: &str, value: i64) -> Option<&'static str> {
        jet_sequence_argument_message(method, value)
    }

    pub(super) fn zip_row_count(lengths: &[usize], mode: u8) -> Option<usize> {
        jet_zip_row_count(lengths, mode)
    }
    pub(super) fn string_split(text: &str, separator: &str) -> Vec<String> {
        jet_iter_string_split(text, separator).to_list()
    }
}
pub fn string_split(text: &str, separator: &str) -> Vec<String> {
    collection_semantics::string_split(text, separator)
}

pub use collection_semantics::LoopListCursor;
pub use collection_semantics::{
    jet_zip_pad_step, jet_zip_short_step, jet_zip_strict_step, try_collect,
};

#[derive(Debug)]
pub struct LoopRangeCursor(collection_semantics::JetLoopRangeCursor);

impl LoopRangeCursor {
    pub fn new(
        start: i64,
        end: i64,
        step: Option<i64>,
        exclusive: bool,
    ) -> Result<Self, &'static str> {
        collection_semantics::jet_loop_range_init_checked(
            start, end, step.unwrap_or(0), step.is_some(), exclusive,
        ).map(Self)
    }
}

impl Iterator for LoopRangeCursor {
    type Item = i64;

    fn next(&mut self) -> Option<Self::Item> {
        if !collection_semantics::jet_loop_range_has_next(&self.0) {
            return None;
        }
        let value = collection_semantics::jet_loop_range_value(&self.0);
        collection_semantics::jet_loop_range_advance(&mut self.0);
        Some(value)
    }
}

pub(super) fn string_contains(text: &str, needle: &str) -> bool {
    collection_semantics::jet_string_contains(text, needle)
}

pub(super) fn iter_first<T: 'static>(values: Vec<T>) -> Option<T> {
    collection_semantics::iter_first(values)
}

pub(super) fn iter_skip<T: 'static>(values: Vec<T>, n: i64) -> Vec<T> {
    collection_semantics::iter_skip(values, n)
}

pub fn sequence_argument_message(method: &str, value: i64) -> Option<&'static str> {
    collection_semantics::sequence_argument_message(method, value)
}

/// Cross-crate MIR bridge for the shared fallible predicate-count kernel.
pub fn list_count_where_result<T, E, F>(values: &[T], predicate: F) -> Result<i64, E>
where
    F: FnMut(&T) -> Result<bool, E>,
{
    collection_semantics::jet_list_count_where_result_kernel(values, predicate)
}

/// Cross-crate MIR bridge for the shared fallible first-match update kernel.
pub fn list_update_first_result<T, E, F>(
    values: &mut Vec<T>,
    predicate: F,
    replacement: T,
) -> Result<bool, E>
where
    F: FnMut(&T) -> Result<bool, E>,
{
    collection_semantics::jet_list_update_first_result_kernel(values, predicate, replacement)
}

pub(super) fn zip_row_count(lengths: &[usize], mode: u8) -> Option<usize> {
    collection_semantics::zip_row_count(lengths, mode)
}

pub(super) fn list_pop<T>(values: &mut Vec<T>) -> Option<T> {
    collection_semantics::list_pop(values)
}

pub(super) fn list_insert(
    values: &mut Vec<CtValue>,
    index: i64,
    value: CtValue,
    span: Span,
) -> Result<(), Diagnostic> {
    collection_semantics::list_insert(values, index, value).map_err(|error| {
        let message = error.message();
        Diagnostic::from_row(error.code(), &[("msg", message.as_str())], Some(span))
    })
}

pub(super) fn list_insert_args(
    values: &mut Vec<CtValue>,
    args: &[CtValue],
    span: Span,
) -> Result<(), Diagnostic> {
    let [index, item] = args else {
        return Err(unsupported(
            "the method `.insert` with these arguments",
            span,
        ));
    };
    list_insert(values, as_int(index, span)?, item.clone(), span)
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

pub fn try_list_new<T>() -> Result<Vec<T>, jet_foundation::Outcome::AllocError> {
    collection_semantics::try_list_new()
}

pub fn try_list_with_capacity<T>(
    capacity: i64,
) -> Result<Vec<T>, jet_foundation::Outcome::AllocError> {
    collection_semantics::try_list_with_capacity(capacity)
}

pub fn try_list_with_capacity_defaulted<T>(
    capacity: i64,
    program_allocator_reserve: impl FnOnce(usize) -> bool,
    program_allocator_cancel: impl FnOnce(usize),
) -> Result<Vec<T>, jet_foundation::Outcome::AllocError> {
    collection_semantics::try_list_with_capacity_defaulted(
        capacity,
        program_allocator_reserve,
        program_allocator_cancel,
    )
}

pub fn try_list_push<T>(
    values: &mut Vec<T>,
    value: T,
) -> Result<(), jet_foundation::Outcome::AllocError> {
    collection_semantics::try_list_push(values, value)
}

pub fn try_list_reserve<T>(
    values: &mut Vec<T>,
    additional: i64,
) -> Result<(), jet_foundation::Outcome::AllocError> {
    collection_semantics::try_list_reserve(values, additional)
}

pub fn try_map_insert<K: Ord + Clone, V: Clone>(
    map: &mut std::collections::BTreeMap<K, V>,
    key: K,
    value: V,
) -> Result<Option<V>, jet_foundation::Outcome::AllocError> {
    collection_semantics::try_map_insert(map, key, value)
}

pub fn try_string_push(
    text: &mut String,
    addition: &str,
) -> Result<(), jet_foundation::Outcome::AllocError> {
    collection_semantics::try_string_push(text, addition)
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
    fields
        .iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
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
    items.sort_by(
        |left, right| match cmp_for_sort(left.clone(), right.clone(), span) {
            Ok(order) => order,
            Err(error) => {
                sort_error.get_or_insert(error);
                std::cmp::Ordering::Equal
            }
        },
    );
    if let Some(error) = sort_error {
        return Err(error);
    }
    items.dedup();
    Ok(items)
}

fn sorted_descending(mut items: Vec<CtValue>, span: Span) -> Result<Vec<CtValue>, Diagnostic> {
    let mut sort_error = None;
    items.sort_by(
        |left, right| match cmp_for_sort(right.clone(), left.clone(), span) {
            Ok(order) => order,
            Err(error) => {
                sort_error.get_or_insert(error);
                std::cmp::Ordering::Equal
            }
        },
    );
    match sort_error {
        Some(error) => Err(error),
        None => Ok(items),
    }
}

fn as_string(v: &CtValue, span: Span) -> Result<String, Diagnostic> {
    match v {
        CtValue::Str(s) => Ok(s.clone()),
        _ => Err(unsupported("non-String argument to Bytes", span)),
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
        type_name: crate::Syntax::TYPE_BITS.to_string(),
        fields: vec![("bits".to_string(), CtValue::List(bits))],
    }
}

fn bag_struct(items: Vec<CtValue>, counts: Vec<CtValue>) -> CtValue {
    CtValue::Struct {
        type_name: crate::Syntax::TYPE_TALLY.to_string(),
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
        type_name: crate::Syntax::TYPE_BYTES.to_string(),
        fields: vec![
            ("bytes".to_string(), CtValue::Bytes(bytes)),
            ("pos".to_string(), CtValue::Int(pos as i64)),
        ],
    }
}

fn deque_struct(items: Vec<CtValue>) -> CtValue {
    CtValue::Struct {
        type_name: crate::Syntax::TYPE_QUEUE.to_string(),
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

/// `Set.from(list)` / `Rank.from(list)` / `PriorityQueue.from(list)` /
/// `Queue.init(list)` — recv is the list (TIR lowering).
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
        name if name == crate::Syntax::TYPE_RANK => {
            Ok(set_struct(name, sorted_unique(items, span)?))
        }
        name if name == crate::Syntax::TYPE_PRIORITY_QUEUE => {
            Ok(set_struct(name, sorted_descending(items, span)?))
        }
        name if name == crate::Syntax::TYPE_QUEUE => Ok(deque_struct(items)),
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
pub fn prelude_new(
    path: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
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
        // Tally.new → HashMap (Map literals use MapLit, not this path).
        "std::collections::HashMap" => Ok(bag_struct(Vec::new(), Vec::new())),
        // #1478: `Set.new()` at this tier — the tier1 native path
        // (`crates/jet-jit/.../lower_ctx.rs`) already builds an empty
        // HashSet handle; this closes the same construct for the canonical
        // MIR evaluator (comptime + `jet run` deopt), matching `BTreeSet`
        // just below (I9 — no tier left calling this an unsupported prelude
        // static once tier1 already ships it natively).
        "std::collections::HashSet" => Ok(set_struct(crate::Syntax::TYPE_SET, Vec::new())),
        "std::collections::BTreeSet" => Ok(set_struct(crate::Syntax::TYPE_RANK, Vec::new())),
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
        && matches!(
            type_name.as_str(),
            "Set" | crate::Syntax::TYPE_RANK | crate::Syntax::TYPE_BITS
        ) {
        "has"
    } else {
        method
    };

    if type_name == crate::Syntax::TYPE_TALLY {
        return Some(bag_method(fields, method, args, span));
    }
    if type_name == crate::Syntax::TYPE_SET {
        return Some(set_method(
            crate::Syntax::TYPE_SET,
            fields,
            method,
            args,
            span,
            false,
        ));
    }
    if type_name == crate::Syntax::TYPE_RANK {
        return Some(set_method(
            crate::Syntax::TYPE_RANK,
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
    if type_name == crate::Syntax::TYPE_BITS {
        return Some(bitset_method(fields, method, args, span));
    }
    if type_name == crate::Syntax::TYPE_QUEUE {
        return Some(deque_method(fields, method, args, span));
    }
    if type_name == crate::Syntax::TYPE_LRU {
        return Some(lru_method(fields, method, args, span));
    }
    if type_name == crate::Syntax::TYPE_BYTES {
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
        (crate::Syntax::TYPE_TALLY, "add" | "remove" | "clear")
            | ("Set", "add" | "remove" | "pop" | "clear")
            | (crate::Syntax::TYPE_RANK, "add" | "remove" | "clear")
            | ("PriorityQueue", "push" | "pop" | "clear" | "remove")
            | (crate::Syntax::TYPE_BITS, "add" | "remove" | "clear")
            | (
                crate::Syntax::TYPE_QUEUE,
                "push_front" | "push_back" | "pop_front" | "pop_back" | "clear"
            )
            | ("Cache", "add" | "add_new" | "get" | "remove" | "clear")
            | (
                crate::Syntax::TYPE_BYTES,
                "clear"
                    | "write_u8"
                    | "write_u16_le"
                    | "write_u16_be"
                    | "write_i8"
                    | "write_i16_le"
                    | "write_i16_be"
                    | "write_u32_le"
                    | "write_u32_be"
                    | "write_i32_le"
                    | "write_i32_be"
                    | "write_u64_le"
                    | "write_u64_be"
                    | "write_i64_le"
                    | "write_i64_be"
                    | "write_f32_le"
                    | "write_f32_be"
                    | "write_f64_le"
                    | "write_f64_be"
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
        crate::Syntax::TYPE_TALLY => bag_mutating(recv, fields, method, &args, span),
        "Set" => set_mutating(
            recv,
            crate::Syntax::TYPE_SET,
            fields,
            method,
            &args,
            span,
            false,
        ),
        crate::Syntax::TYPE_RANK => set_mutating(
            recv,
            crate::Syntax::TYPE_RANK,
            fields,
            method,
            &args,
            span,
            true,
        ),
        "PriorityQueue" => priority_queue_mutating(recv, fields, method, &args, span),
        crate::Syntax::TYPE_BITS => bitset_mutating(recv, fields, method, &args, span),
        crate::Syntax::TYPE_QUEUE => deque_mutating(recv, fields, method, &args, span),
        "Cache" => lru_mutating(recv, fields, method, &args, span),
        crate::Syntax::TYPE_BYTES => byte_buffer_mutating(recv, fields, method, &args, span),
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
        "has" => Ok(CtValue::Bool(
            items.contains(args.first().unwrap_or(&CtValue::Unit)),
        )),
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
            &format!("Tally.{} at compile time", method),
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
                &format!("Tally.{} at compile time", method),
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
                super::Builtins::cmp_for_sort(a.clone(), b.clone(), span)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            Ok(CtValue::List(sorted))
        }
        "max" => super::Builtins::apply_method(
            &CtValue::List(items),
            "max",
            args.to_vec(),
            span,
        ),
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
            let other = args
                .first()
                .ok_or_else(|| unsupported(&format!("{type_name}.equal missing argument"), span))?;
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
            Ok(CtValue::Bool(
                set_semantics::jet_set_is_subset_by(&items, &other_items, |a, b| a == b)
                    && set_semantics::jet_set_is_subset_by(&other_items, &items, |a, b| a == b),
            ))
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
            let other = args
                .first()
                .ok_or_else(|| unsupported(&format!("{type_name}.union missing argument"), span))?;
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
            let merged =
                set_semantics::jet_set_union_by(&items, &other_items, |left, right| left == right);
            let merged = if sorted {
                sorted_unique(merged, span)?
            } else {
                merged
            };
            Ok(set_struct(type_name, merged))
        }
        "intersection"
        | "difference"
        | "symmetric_difference"
        | "is_subset"
        | "is_superset"
        | "is_disjoint" => {
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
                    &items,
                    &other_items,
                    equal,
                ))),
                "is_superset" => Ok(CtValue::Bool(set_semantics::jet_set_is_superset_by(
                    &items,
                    &other_items,
                    equal,
                ))),
                "is_disjoint" => Ok(CtValue::Bool(set_semantics::jet_set_is_disjoint_by(
                    &items,
                    &other_items,
                    equal,
                ))),
                "intersection" => {
                    let values =
                        set_semantics::jet_set_intersection_by(&items, &other_items, equal);
                    Ok(set_struct(
                        type_name,
                        if sorted {
                            sorted_unique(values, span)?
                        } else {
                            values
                        },
                    ))
                }
                "difference" => {
                    let values = set_semantics::jet_set_difference_by(&items, &other_items, equal);
                    Ok(set_struct(
                        type_name,
                        if sorted {
                            sorted_unique(values, span)?
                        } else {
                            values
                        },
                    ))
                }
                "symmetric_difference" => {
                    let values =
                        set_semantics::jet_set_symmetric_difference_by(&items, &other_items, equal);
                    Ok(set_struct(
                        type_name,
                        if sorted {
                            sorted_unique(values, span)?
                        } else {
                            values
                        },
                    ))
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
                    None => CtValue::absent(
                        items
                            .first()
                            .map(|item| item.jet_type())
                            .unwrap_or(Type::Int),
                    ),
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
        "copy" => Ok(bitset_struct(bits.clone())),
        "to_list" => Ok(CtValue::List(bits)),
        _ => Err(unsupported(
            &format!("Bits.{} at compile time", method),
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
                &format!("Bits.{} at compile time", method),
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
            &format!("Queue.{} at compile time", method),
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
                &format!("Queue.{} at compile time", method),
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
                text.split(&sep)
                    .map(|s| CtValue::Str(s.to_string()))
                    .collect(),
            ))
        }
        "replace" => {
            let from = as_string(args.first().unwrap_or(&CtValue::Str(String::new())), span)?;
            let to = as_string(args.get(1).unwrap_or(&CtValue::Str(String::new())), span)?;
            Ok(byte_buffer_struct(text.replace(&from, &to).into_bytes()))
        }
        "join" => {
            let parts = match args.first() {
                Some(CtValue::List(xs)) => {
                    xs.iter()
                        .map(|x| as_string(x, span))
                        .collect::<Result<Vec<_>, _>>()?
                }
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
        "parse" => match crate::Numeric::CtBigInt::from_str(text.trim()) {
            Ok(n) => Ok(CtValue::Present(Box::new(
                super::Builtins::exact_int_value(n),
            ))),
            Err(e) => Ok(CtValue::failed(Box::new(CtValue::Str(e.to_string())))),
        },
        _ => Err(unsupported(
            &format!("Bytes.{} at compile time", method),
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
        "write_i8" => {
            bytes.push(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as i8 as u8);
            CtValue::Unit
        }
        "write_i16_le" => {
            bytes.extend_from_slice(
                &(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as i16).to_le_bytes(),
            );
            CtValue::Unit
        }
        "write_i16_be" => {
            bytes.extend_from_slice(
                &(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as i16).to_be_bytes(),
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
        "write_i32_le" => {
            bytes.extend_from_slice(
                &(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as i32).to_le_bytes(),
            );
            CtValue::Unit
        }
        "write_i32_be" => {
            bytes.extend_from_slice(
                &(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as i32).to_be_bytes(),
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
        "write_i64_le" => {
            bytes.extend_from_slice(
                &(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as i64).to_le_bytes(),
            );
            CtValue::Unit
        }
        "write_i64_be" => {
            bytes.extend_from_slice(
                &(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)? as i64).to_be_bytes(),
            );
            CtValue::Unit
        }
        "write_f32_le" | "write_f32_be" => {
            let Some(CtValue::Float(value)) = args.first() else {
                return Err(unsupported("Bytes.write_f32 expects Float", span));
            };
            let bytes_value = value.as_f32();
            if method == "write_f32_le" {
                bytes.extend_from_slice(&bytes_value.to_le_bytes());
            } else {
                bytes.extend_from_slice(&bytes_value.to_be_bytes());
            }
            CtValue::Unit
        }
        "write_f64_le" | "write_f64_be" => {
            let Some(CtValue::Float(value)) = args.first() else {
                return Err(unsupported("Bytes.write_f64 expects Float", span));
            };
            let bytes_value = value.as_f64();
            if method == "write_f64_le" {
                bytes.extend_from_slice(&bytes_value.to_le_bytes());
            } else {
                bytes.extend_from_slice(&bytes_value.to_be_bytes());
            }
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
                CtValue::absent(Type::List(Box::new(Type::IntN {
                    signed: false,
                    bits: 8,
                })))
            } else {
                let out = bytes[pos..].to_vec();
                pos = bytes.len();
                CtValue::Present(Box::new(CtValue::Bytes(out)))
            }
        }
        "read_bytes" => {
            let n = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            if n < 0 || pos + (n as usize) > bytes.len() {
                CtValue::absent(Type::List(Box::new(Type::IntN {
                    signed: false,
                    bits: 8,
                })))
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
                &format!("Bytes.{} at compile time", method),
                span,
            ));
        }
        _ => {
            return Err(unsupported(
                &format!("Bytes.{} at compile time", method),
                span,
            ))
        }
    };
    *recv = byte_buffer_struct_at(bytes, pos);
    Ok(result)
}

fn collection_type_matches(actual: &str, expected: &str) -> bool {
    actual == expected || actual == format!("Jet{expected}")
}

fn collection_struct_fields<'a>(
    value: &'a CtValue,
    expected: &str,
    what: &str,
    span: Span,
) -> Result<&'a [(String, CtValue)], Diagnostic> {
    match value {
        CtValue::Struct { type_name, fields }
            if collection_type_matches(type_name, expected) =>
        {
            Ok(fields)
        }
        _ => Err(unsupported(what, span)),
    }
}

fn collection_field<'a>(
    fields: &'a [(String, CtValue)],
    name: &str,
    what: &str,
    span: Span,
) -> Result<&'a CtValue, Diagnostic> {
    fields
        .iter()
        .find(|(field_name, _)| field_name == name)
        .map(|(_, value)| value)
        .ok_or_else(|| unsupported(what, span))
}

fn collection_string_list(
    value: &CtValue,
    what: &str,
    span: Span,
) -> Result<Vec<String>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(unsupported(what, span));
    };
    values
        .iter()
        .map(|value| match value {
            CtValue::Str(value) => Ok(value.clone()),
            _ => Err(unsupported(what, span)),
        })
        .collect()
}

fn collection_int_list(
    value: &CtValue,
    what: &str,
    span: Span,
) -> Result<Vec<i64>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(unsupported(what, span));
    };
    values
        .iter()
        .map(|value| as_int(value, span))
        .collect::<Result<Vec<_>, _>>()
}

fn collection_counter(
    value: &CtValue,
    span: Span,
) -> Result<collection_semantics::JetCounter, Diagnostic> {
    let fields = collection_struct_fields(value, "Counter", "Counter value", span)?;
    let keys = collection_string_list(
        collection_field(fields, "keys", "Counter keys", span)?,
        "Counter keys",
        span,
    )?;
    let counts = collection_int_list(
        collection_field(fields, "counts", "Counter counts", span)?,
        "Counter counts",
        span,
    )?;
    if keys.len() != counts.len() {
        return Err(unsupported("Counter keys/counts", span));
    }
    Ok(collection_semantics::JetCounter { keys, counts })
}

fn collection_counter_value(counter: &collection_semantics::JetCounter) -> CtValue {
    CtValue::Struct {
        type_name: "Counter".to_string(),
        fields: vec![
            (
                "keys".to_string(),
                CtValue::List(counter.keys.iter().cloned().map(CtValue::Str).collect()),
            ),
            (
                "counts".to_string(),
                CtValue::List(counter.counts.iter().copied().map(CtValue::Int).collect()),
            ),
        ],
    }
}

fn collection_optional_string(value: Option<String>) -> CtValue {
    value
        .map(|value| CtValue::Present(Box::new(CtValue::Str(value))))
        .unwrap_or_else(|| CtValue::absent(Type::String))
}

fn collection_ordered_map(
    value: &CtValue,
    span: Span,
) -> Result<collection_semantics::JetOrderedMap, Diagnostic> {
    let fields = collection_struct_fields(value, "OrderedMap", "OrderedMap value", span)?;
    let keys = collection_string_list(
        collection_field(fields, "keys", "OrderedMap keys", span)?,
        "OrderedMap keys",
        span,
    )?;
    let values = collection_string_list(
        collection_field(fields, "values", "OrderedMap values", span)?,
        "OrderedMap values",
        span,
    )?;
    if keys.len() != values.len() {
        return Err(unsupported("OrderedMap keys/values", span));
    }
    Ok(collection_semantics::JetOrderedMap { keys, values })
}

fn collection_ordered_map_value(map: &collection_semantics::JetOrderedMap) -> CtValue {
    CtValue::Struct {
        type_name: "OrderedMap".to_string(),
        fields: vec![
            (
                "keys".to_string(),
                CtValue::List(map.keys.iter().cloned().map(CtValue::Str).collect()),
            ),
            (
                "values".to_string(),
                CtValue::List(map.values.iter().cloned().map(CtValue::Str).collect()),
            ),
        ],
    }
}

fn collection_layer(
    value: &CtValue,
    span: Span,
) -> Result<collection_semantics::JetLayer, Diagnostic> {
    let fields = collection_struct_fields(value, "Layer", "Chain layer", span)?;
    let keys = collection_string_list(
        collection_field(fields, "keys", "Chain layer keys", span)?,
        "Chain layer keys",
        span,
    )?;
    let values = collection_string_list(
        collection_field(fields, "values", "Chain layer values", span)?,
        "Chain layer values",
        span,
    )?;
    if keys.len() != values.len() {
        return Err(unsupported("Chain layer keys/values", span));
    }
    Ok(collection_semantics::JetLayer { keys, values })
}

fn collection_layer_value(layer: &collection_semantics::JetLayer) -> CtValue {
    CtValue::Struct {
        type_name: "Layer".to_string(),
        fields: vec![
            (
                "keys".to_string(),
                CtValue::List(layer.keys.iter().cloned().map(CtValue::Str).collect()),
            ),
            (
                "values".to_string(),
                CtValue::List(layer.values.iter().cloned().map(CtValue::Str).collect()),
            ),
        ],
    }
}

fn collection_chain(
    value: &CtValue,
    span: Span,
) -> Result<collection_semantics::JetChain, Diagnostic> {
    let fields = collection_struct_fields(value, "Chain", "Chain value", span)?;
    let CtValue::List(layers) =
        collection_field(fields, "layers", "Chain layers", span)?
    else {
        return Err(unsupported("Chain layers", span));
    };
    Ok(collection_semantics::JetChain {
        layers: layers
            .iter()
            .map(|layer| collection_layer(layer, span))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn collection_chain_value(chain: &collection_semantics::JetChain) -> CtValue {
    CtValue::Struct {
        type_name: "Chain".to_string(),
        fields: vec![(
            "layers".to_string(),
            CtValue::List(chain.layers.iter().map(collection_layer_value).collect()),
        )],
    }
}
fn collection_string_set(
    value: &CtValue,
    span: Span,
) -> Result<collection_semantics::JetStringSet, Diagnostic> {
    let fields = collection_struct_fields(value, "StringSet", "StringSet value", span)?;
    Ok(collection_semantics::JetStringSet {
        items: collection_string_list(
            collection_field(fields, "items", "StringSet items", span)?,
            "StringSet items",
            span,
        )?,
    })
}

fn collection_string_set_value(set: &collection_semantics::JetStringSet) -> CtValue {
    CtValue::Struct {
        type_name: "StringSet".to_string(),
        fields: vec![(
            "items".to_string(),
            CtValue::List(set.items.iter().cloned().map(CtValue::Str).collect()),
        )],
    }
}

fn collection_optional_string_set(
    value: Option<collection_semantics::JetStringSet>,
) -> CtValue {
    value
        .map(|value| CtValue::Present(Box::new(collection_string_set_value(&value))))
        .unwrap_or_else(|| CtValue::absent(Type::Named("StringSet".to_string())))
}

fn collection_arg<'a>(
    args: &'a [CtValue],
    index: usize,
    method: &str,
    span: Span,
) -> Result<&'a CtValue, Diagnostic> {
    args.get(index)
        .ok_or_else(|| unsupported(&format!("core.collections.{method} argument"), span))
}

/// Evaluate the nominal string-container surface through the same kernel used
/// by generated AOT code.  This is deliberately outside the generic list
/// evaluator: the carrier fields are part of the public Core type contract.
pub fn apply_core_collections(
    module: &str,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    use collection_semantics as native;

    match method {
        "counter" => Ok(collection_counter_value(&native::jet_coll_counter())),
        "counter_from" => {
            let keys = collection_string_list(
                collection_arg(args, 0, method, span)?,
                "core.collections.counter_from keys",
                span,
            )?;
            Ok(collection_counter_value(&native::jet_coll_counter_from(&keys)))
        }
        "add" if module == "core.collections" => {
            let counter = collection_counter(collection_arg(args, 0, method, span)?, span)?;
            let key = as_string(collection_arg(args, 1, method, span)?, span)?;
            let amount = as_int(collection_arg(args, 2, method, span)?, span)?;
            Ok(collection_counter_value(&native::jet_coll_counter_add(
                &counter, &key, amount,
            )))
        }
        "inc" | "dec" => {
            let counter = collection_counter(collection_arg(args, 0, method, span)?, span)?;
            let key = as_string(collection_arg(args, 1, method, span)?, span)?;
            let value = if method == "inc" {
                native::jet_coll_counter_inc(&counter, &key)
            } else {
                native::jet_coll_counter_dec(&counter, &key)
            };
            Ok(collection_counter_value(&value))
        }
        "get" => {
            let counter = collection_counter(collection_arg(args, 0, method, span)?, span)?;
            let key = as_string(collection_arg(args, 1, method, span)?, span)?;
            Ok(CtValue::Int(native::jet_coll_counter_get(&counter, &key)))
        }
        "set_count" => {
            let counter = collection_counter(collection_arg(args, 0, method, span)?, span)?;
            let key = as_string(collection_arg(args, 1, method, span)?, span)?;
            let count = as_int(collection_arg(args, 2, method, span)?, span)?;
            Ok(collection_counter_value(&native::jet_coll_counter_set_count(
                &counter, &key, count,
            )))
        }
        "total" => {
            let counter = collection_counter(collection_arg(args, 0, method, span)?, span)?;
            Ok(CtValue::Int(native::jet_coll_counter_total(&counter)))
        }
        "names" | "elements" => {
            let counter = collection_counter(collection_arg(args, 0, method, span)?, span)?;
            let values = if method == "names" {
                native::jet_coll_counter_names(&counter)
            } else {
                native::jet_coll_counter_elements(&counter)
            };
            Ok(CtValue::List(values.into_iter().map(CtValue::Str).collect()))
        }
        "most_common" => {
            let counter = collection_counter(collection_arg(args, 0, method, span)?, span)?;
            let count = as_int(collection_arg(args, 1, method, span)?, span)?;
            Ok(collection_counter_value(&native::jet_coll_counter_most_common(
                &counter, count,
            )))
        }
        "subtract" | "merge_add" => {
            let left = collection_counter(collection_arg(args, 0, method, span)?, span)?;
            let right = collection_counter(collection_arg(args, 1, method, span)?, span)?;
            let value = if method == "subtract" {
                native::jet_coll_counter_subtract(&left, &right)
            } else {
                native::jet_coll_counter_merge_add(&left, &right)
            };
            Ok(collection_counter_value(&value))
        }
        "clear_counter" => {
            let counter = collection_counter(collection_arg(args, 0, method, span)?, span)?;
            Ok(collection_counter_value(&native::jet_coll_counter_clear(
                &counter,
            )))
        }
        "ordered_map" => Ok(collection_ordered_map_value(&native::jet_coll_ordered_map())),
        "map_get" | "map_set" | "map_remove" | "map_contains" => {
            let map = collection_ordered_map(collection_arg(args, 0, method, span)?, span)?;
            let key = as_string(collection_arg(args, 1, method, span)?, span)?;
            match method {
                "map_get" => Ok(collection_optional_string(
                    native::jet_coll_map_get(&map, &key),
                )),
                "map_set" => {
                    let value = as_string(collection_arg(args, 2, method, span)?, span)?;
                    Ok(collection_ordered_map_value(&native::jet_coll_map_set(
                        &map, &key, &value,
                    )))
                }
                "map_remove" => Ok(collection_ordered_map_value(
                    &native::jet_coll_map_remove(&map, &key),
                )),
                _ => Ok(CtValue::Bool(native::jet_coll_map_contains(&map, &key))),
            }
        }
        "map_keys" | "map_values" | "map_len" => {
            let map = collection_ordered_map(collection_arg(args, 0, method, span)?, span)?;
            match method {
                "map_keys" => Ok(CtValue::List(
                    native::jet_coll_map_keys(&map)
                        .into_iter()
                        .map(CtValue::Str)
                        .collect(),
                )),
                "map_values" => Ok(CtValue::List(
                    native::jet_coll_map_values(&map)
                        .into_iter()
                        .map(CtValue::Str)
                        .collect(),
                )),
                _ => Ok(CtValue::Int(native::jet_coll_map_len(&map))),
            }
        }
        "chain" => Ok(collection_chain_value(&native::jet_coll_chain())),
        "chain_push" => {
            let chain = collection_chain(collection_arg(args, 0, method, span)?, span)?;
            let keys = collection_string_list(
                collection_arg(args, 1, method, span)?,
                "core.collections.chain_push keys",
                span,
            )?;
            let values = collection_string_list(
                collection_arg(args, 2, method, span)?,
                "core.collections.chain_push values",
                span,
            )?;
            Ok(collection_chain_value(&native::jet_coll_chain_push(
                &chain, &keys, &values,
            )))
        }
        "chain_get" | "chain_contains" => {
            let chain = collection_chain(collection_arg(args, 0, method, span)?, span)?;
            let key = as_string(collection_arg(args, 1, method, span)?, span)?;
            if method == "chain_get" {
                Ok(collection_optional_string(
                    native::jet_coll_chain_get(&chain, &key),
                ))
            } else {
                Ok(CtValue::Bool(native::jet_coll_chain_contains(&chain, &key)))
            }
        }
        "new" => Ok(collection_string_set_value(&native::jet_coll_set())),
        "from_list" => {
            let items = collection_string_list(
                collection_arg(args, 0, method, span)?,
                "core.collections.set.from_list items",
                span,
            )?;
            Ok(collection_string_set_value(
                &native::jet_coll_set_from_list(&items),
            ))
        }
        "add" | "discard" => {
            let set = collection_string_set(collection_arg(args, 0, method, span)?, span)?;
            let item = as_string(collection_arg(args, 1, method, span)?, span)?;
            let next = if method == "add" {
                native::jet_coll_set_add(&set, &item)
            } else {
                native::jet_coll_set_discard(&set, &item)
            };
            Ok(collection_string_set_value(&next))
        }
        "remove" => {
            let set = collection_string_set(collection_arg(args, 0, method, span)?, span)?;
            let item = as_string(collection_arg(args, 1, method, span)?, span)?;
            Ok(collection_optional_string_set(
                native::jet_coll_set_remove(&set, &item),
            ))
        }
        "contains" => {
            let set = collection_string_set(collection_arg(args, 0, method, span)?, span)?;
            let item = as_string(collection_arg(args, 1, method, span)?, span)?;
            Ok(CtValue::Bool(native::jet_coll_set_contains(&set, &item)))
        }
        "len" | "is_empty" | "to_list" | "clear" | "clone_set" => {
            let set = collection_string_set(collection_arg(args, 0, method, span)?, span)?;
            match method {
                "len" => Ok(CtValue::Int(native::jet_coll_set_len(&set))),
                "is_empty" => Ok(CtValue::Bool(native::jet_coll_set_is_empty(&set))),
                "to_list" => Ok(CtValue::List(
                    native::jet_coll_set_items(&set)
                        .into_iter()
                        .map(CtValue::Str)
                        .collect(),
                )),
                "clear" => Ok(collection_string_set_value(
                    &native::jet_coll_set_clear(&set),
                )),
                _ => Ok(collection_string_set_value(
                    &native::jet_coll_set_clone(&set),
                )),
            }
        }
        "union" | "intersection" | "difference" | "symmetric_difference" => {
            let left = collection_string_set(collection_arg(args, 0, method, span)?, span)?;
            let right = collection_string_set(collection_arg(args, 1, method, span)?, span)?;
            let next = match method {
                "union" => native::jet_coll_set_union(&left, &right),
                "intersection" => native::jet_coll_set_intersection(&left, &right),
                "difference" => native::jet_coll_set_difference(&left, &right),
                _ => native::jet_coll_set_symmetric_difference(&left, &right),
            };
            Ok(collection_string_set_value(&next))
        }
        "issubset" | "issuperset" | "isdisjoint" => {
            let left = collection_string_set(collection_arg(args, 0, method, span)?, span)?;
            let right = collection_string_set(collection_arg(args, 1, method, span)?, span)?;
            let result = match method {
                "issubset" => native::jet_coll_set_issubset(&left, &right),
                "issuperset" => native::jet_coll_set_issuperset(&left, &right),
                _ => native::jet_coll_set_isdisjoint(&left, &right),
            };
            Ok(CtValue::Bool(result))
        }
        _ => Err(unsupported(
            &format!("core.collections.{method}()"),
            span,
        )),
    }
}
