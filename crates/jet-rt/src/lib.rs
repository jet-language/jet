#![deny(warnings)]

use std::collections::BTreeMap;

/// The arena only marshals resident values into the shared Prelude carrier.
mod map_key_semantics {
    include!("../../jet-codegen/src/Prelude/Core/MapKey.rs");
}
pub use map_key_semantics::{jet_map_key_cmp, JetMapKey, JetMapKeyEncode};

#[allow(dead_code)]
mod uninit_semantics {
    include!("../../jet-codegen/src/Prelude/Uninit.rs");
}
#[doc(hidden)]
pub(crate) mod jet_mem {
    pub(crate) use jet_foundation::MemSentry::{jet_memory_ledger_record, MemoryLedgerWitness};
}

/// Compiler/runtime-only traced heap. Jet source reaches this through codegen.
#[doc(hidden)]
pub mod __gc;

pub fn display_f32(v: f32) -> String {
    format!("{v:?}")
}

pub fn display_f64(v: f64) -> String {
    format!("{v:?}")
}

pub fn string_len_chars(s: &str) -> i64 {
    s.chars().count() as i64
}

pub fn string_trim(s: &str) -> String {
    jet_foundation::generated::UnicodeString::jet_unicode_trim(s)
}

pub fn string_to_upper(s: &str) -> String {
    jet_foundation::generated::UnicodeString::jet_unicode_upper(s)
}

pub fn string_to_lower(s: &str) -> String {
    jet_foundation::generated::UnicodeString::jet_unicode_lower(s)
}

pub fn string_replace(s: &str, from: &str, to: &str) -> String {
    s.replace(from, to)
}

pub fn string_after(s: &str, sep: &str) -> String {
    match s.find(sep) {
        Some(i) => s[i + sep.len()..].to_string(),
        None => s.to_string(),
    }
}

pub fn string_before(s: &str, sep: &str) -> String {
    match s.find(sep) {
        Some(i) => s[..i].to_string(),
        None => s.to_string(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetVal {
    /// JIT value carrier for sema-proved uninitialized fixed-list storage.
    /// Initialization policy comes from the canonical Prelude source above.
    UninitList {
        values: Vec<JetVal>,
        initialized: Vec<bool>,
    },
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    String(String),
    StringView {
        owner: i64,
        start: usize,
        end: usize,
    },
    /// Dense resident carrier for integer-backed lists. String and opaque
    /// handles use the same `i64` word representation.
    IntList(Vec<i64>),
    List(Vec<JetVal>),
    /// D-RANGE-VALUE1: inline list element, not an arena record handle.
    Range {
        start: i64,
        end: i64,
        exclusive: bool,
    },
    /// Ordered map; each entry keeps the raw key handle for `keys()` and the
    /// packed value word used by the JIT ABI.
    Map(BTreeMap<JetMapKey, (i64, i64)>),
    Record(Vec<JetVal>),
    /// A record handle stored in another record. Keeping the distinction from
    /// `Int` lets recursive map-key marshalling follow the declared field
    /// shape without mistaking an integer that happens to equal an arena index
    /// for a nested record.
    RecordRef(i64),
    // D-INTBIG1: exact Int spill carrier. Reuses `CtBigInt` (jet-foundation)
    // limb-for-limb so a JIT-computed spilled Int prints byte-identical to the
    // AOT and comptime exact integer paths (R12 parity).
    ExactInt(jet_foundation::Numeric::CtBigInt),
}

#[derive(Clone, Debug, Default)]
pub struct JetArena {
    values: Vec<JetVal>,
}

impl JetArena {
    pub fn alloc_empty_string(&mut self) -> i64 {
        self.alloc_string(String::new())
    }

    pub fn alloc_string(&mut self, text: impl Into<String>) -> i64 {
        let id = self.values.len() as i64;
        self.values.push(JetVal::String(text.into()));
        id
    }

    pub fn get_string(&self, id: i64) -> Option<&str> {
        match self.values.get(id as usize) {
            Some(JetVal::String(value)) => Some(value.as_str()),
            Some(JetVal::StringView { owner, start, end }) => {
                let Some(JetVal::String(value)) = self.values.get(*owner as usize) else {
                    return None;
                };
                value.get(*start..*end)
            }
            _ => None,
        }
    }

    pub fn alloc_string_view(&mut self, owner: i64, start: usize, end: usize) -> Option<i64> {
        let (owner, base, len) = match self.values.get(owner as usize)? {
            JetVal::String(value) => (owner, 0, value.len()),
            JetVal::StringView { owner, start, end } => (*owner, *start, end.checked_sub(*start)?),
            _ => return None,
        };
        if start > end || end > len {
            return None;
        }
        let start = base.checked_add(start)?;
        let end = base.checked_add(end)?;
        let JetVal::String(value) = self.values.get(owner as usize)? else {
            return None;
        };
        value.get(start..end)?;
        let id = self.values.len() as i64;
        self.values.push(JetVal::StringView { owner, start, end });
        Some(id)
    }

    pub fn get_string_mut(&mut self, id: i64) -> Option<&mut String> {
        match self.values.get_mut(id as usize) {
            Some(JetVal::String(value)) => Some(value),
            _ => None,
        }
    }

    pub fn clone_string(&self, id: i64) -> Option<String> {
        self.get_string(id).map(str::to_string)
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }

    /// Indices of `String` values allocated during JIT lowering (baked into code as handles).
    pub fn string_slots(&self) -> Vec<(usize, String)> {
        self.values
            .iter()
            .enumerate()
            .filter_map(|(i, value)| match value {
                JetVal::String(text) => Some((i, text.clone())),
                _ => None,
            })
            .collect()
    }

    /// Restore compile-time string handles so cached machine code sees the same ids.
    pub fn install_string_slots(&mut self, slots: &[(usize, String)]) {
        if slots.is_empty() {
            return;
        }
        let max = slots.iter().map(|(i, _)| *i).max().unwrap_or(0);
        while self.values.len() <= max {
            self.values.push(JetVal::Int(0));
        }
        for (i, text) in slots {
            self.values[*i] = JetVal::String(text.clone());
        }
    }

    pub fn alloc_int_list(&mut self, values: Vec<i64>) -> i64 {
        let id = self.values.len() as i64;
        self.values.push(JetVal::IntList(values));
        id
    }

    pub fn alloc_empty_list(&mut self) -> i64 {
        let id = self.values.len() as i64;
        self.values.push(JetVal::List(Vec::new()));
        id
    }

    pub fn alloc_uninit_list(&mut self, len: usize) -> i64 {
        let id = self.values.len() as i64;
        self.values.push(JetVal::UninitList {
            values: vec![JetVal::Int(0); len],
            initialized: uninit_semantics::jet_uninit_bitmap(len),
        });
        id
    }

    pub fn alloc_empty_map(&mut self) -> i64 {
        let id = self.values.len() as i64;
        self.values.push(JetVal::Map(BTreeMap::new()));
        id
    }

    pub fn map_insert(&mut self, map: i64, key_id: i64, value: i64) -> Option<()> {
        let key = self.clone_string(key_id)?;
        match self.values.get_mut(map as usize) {
            Some(JetVal::Map(entries)) => {
                entries.insert(JetMapKey::String(key), (key_id, value));
                Some(())
            }
            _ => None,
        }
    }

    pub fn map_get(&self, map: i64, key_id: i64) -> Option<i64> {
        let key = self.get_string(key_id)?;
        match self.values.get(map as usize) {
            Some(JetVal::Map(entries)) => entries
                .get(&JetMapKey::String(key.to_string()))
                .map(|(_, value)| *value),
            _ => None,
        }
    }

    pub fn map_remove(&mut self, map: i64, key_id: i64) -> Option<i64> {
        let key = self.clone_string(key_id)?;
        match self.values.get_mut(map as usize) {
            Some(JetVal::Map(entries)) => entries
                .remove(&JetMapKey::String(key))
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn map_insert_int(&mut self, map: i64, key: i64, value: i64) -> Option<()> {
        match self.values.get_mut(map as usize) {
            Some(JetVal::Map(entries)) => {
                entries.insert(JetMapKey::Int(key), (key, value));
                Some(())
            }
            _ => None,
        }
    }

    pub fn map_get_int(&self, map: i64, key: i64) -> Option<i64> {
        match self.values.get(map as usize) {
            Some(JetVal::Map(entries)) => entries
                .get(&JetMapKey::Int(key))
                .map(|(_, value)| *value),
            _ => None,
        }
    }

    pub fn map_remove_int(&mut self, map: i64, key: i64) -> Option<i64> {
        match self.values.get_mut(map as usize) {
            Some(JetVal::Map(entries)) => entries
                .remove(&JetMapKey::Int(key))
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// Convert a generated tuple/struct record into the structural key used by
    /// the JIT map adapter. Record references recurse through the same
    /// structural carrier; the shared Prelude `jet_map_key_cmp` supplies the
    /// ordered value comparison without a second map policy.
    fn composite_key(&self, key_id: i64) -> Option<JetMapKey> {
        let JetVal::Record(fields) = self.values.get(key_id as usize)? else {
            return None;
        };
        fields
            .iter()
            .map(|field| self.composite_key_field(field))
            .collect::<Option<Vec<_>>>()
            .map(JetMapKey::Record)
    }

    fn composite_key_field(&self, field: &JetVal) -> Option<JetMapKey> {
        let fields = match field {
            JetVal::Int(value) => return Some(JetMapKey::Int(*value)),
            JetVal::String(value) => return Some(JetMapKey::String(value.clone())),
            JetVal::Bool(value) => return Some(JetMapKey::Bool(*value)),
            JetVal::Char(value) => return Some(JetMapKey::Char(*value)),
            JetVal::RecordRef(handle) => match self.values.get(*handle as usize)? {
                JetVal::Record(fields) => fields,
                _ => return None,
            },
            JetVal::Record(fields) => fields,
            _ => return None,
        };
        fields
            .iter()
            .map(|field| self.composite_key_field(field))
            .collect::<Option<Vec<_>>>()
            .map(JetMapKey::Record)
    }

    pub fn map_insert_composite(&mut self, map: i64, key_id: i64, value: i64) -> Option<()> {
        let key = self.composite_key(key_id)?;
        match self.values.get_mut(map as usize) {
            Some(JetVal::Map(entries)) => {
                entries.insert(key, (key_id, value));
                Some(())
            }
            _ => None,
        }
    }

    pub fn map_get_composite(&self, map: i64, key_id: i64) -> Option<i64> {
        let key = self.composite_key(key_id)?;
        match self.values.get(map as usize) {
            Some(JetVal::Map(entries)) => entries.get(&key).map(|(_, value)| *value),
            _ => None,
        }
    }

    pub fn map_remove_composite(&mut self, map: i64, key_id: i64) -> Option<i64> {
        let key = self.composite_key(key_id)?;
        match self.values.get_mut(map as usize) {
            Some(JetVal::Map(entries)) => entries.remove(&key).map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn map_len(&self, map: i64) -> Option<i64> {
        match self.values.get(map as usize) {
            Some(JetVal::Map(entries)) => Some(entries.len() as i64),
            _ => None,
        }
    }

    pub fn map_key_at(&mut self, map: i64, index: i64) -> Option<i64> {
        if index < 0 {
            return None;
        }
        let key = match self.values.get(map as usize) {
            Some(JetVal::Map(entries)) => entries.values().nth(index as usize)?.0,
            _ => return None,
        };
        Some(key)
    }

    pub fn map_value_at(&self, map: i64, index: i64) -> Option<i64> {
        if index < 0 {
            return None;
        }
        match self.values.get(map as usize) {
            Some(JetVal::Map(entries)) => entries
                .values()
                .nth(index as usize)
                .map(|(_, value)| *value),
            _ => None,
        }
    }

    pub fn list_push_int(&mut self, list: i64, value: i64) -> Option<()> {
        match self.values.get_mut(list as usize) {
            Some(JetVal::IntList(values)) => {
                values.push(value);
                Some(())
            }
            // An empty untyped list promotes to the dense carrier on first
            // push; a populated one keeps its boxed shape. Read emptiness
            // through a borrow that ends before the slot is overwritten.
            Some(slot) => {
                let empty = match &*slot {
                    JetVal::List(values) => values.is_empty(),
                    _ => return None,
                };
                if empty {
                    *slot = JetVal::IntList(vec![value]);
                } else if let JetVal::List(values) = slot {
                    values.push(JetVal::Int(value));
                }
                Some(())
            }
            _ => None,
        }
    }

    pub fn list_push_float(&mut self, list: i64, value: f64) -> Option<()> {
        match self.values.get_mut(list as usize) {
            Some(JetVal::List(values)) => {
                values.push(JetVal::Float(value));
                Some(())
            }
            _ => None,
        }
    }

    pub fn replace_int_list(&mut self, list: i64, values: Vec<i64>) -> Option<()> {
        match self.values.get_mut(list as usize) {
            Some(slot @ JetVal::List(_)) => {
                *slot = JetVal::IntList(values);
                Some(())
            }
            Some(JetVal::IntList(target)) => {
                *target = values;
                Some(())
            }
            _ => None,
        }
    }

    pub fn replace_float_list(&mut self, list: i64, values: Vec<f64>) -> Option<()> {
        match self.values.get_mut(list as usize) {
            Some(JetVal::List(target)) => {
                *target = values.into_iter().map(JetVal::Float).collect();
                Some(())
            }
            _ => None,
        }
    }

    pub fn list_push_range(
        &mut self,
        list: i64,
        start: i64,
        end: i64,
        exclusive: bool,
    ) -> Option<()> {
        match self.values.get_mut(list as usize) {
            Some(JetVal::List(values)) => {
                values.push(JetVal::Range {
                    start,
                    end,
                    exclusive,
                });
                Some(())
            }
            _ => None,
        }
    }

    pub fn list_len(&self, list: i64) -> Option<i64> {
        match self.values.get(list as usize) {
            Some(JetVal::IntList(values)) => Some(values.len() as i64),
            Some(JetVal::List(values)) => Some(values.len() as i64),
            Some(JetVal::UninitList { values, .. }) => Some(values.len() as i64),
            _ => None,
        }
    }

    /// Mutable erased view for collection operations whose shared kernels use
    /// `JetVal` values. Converting a dense integer carrier here is a cold
    /// mutation boundary; native read-only reductions keep the dense carrier.
    pub fn list_values_mut(&mut self, list: i64) -> Option<&mut Vec<JetVal>> {
        if matches!(self.values.get(list as usize), Some(JetVal::IntList(_))) {
            let dense = match self.values.get_mut(list as usize)? {
                JetVal::IntList(values) => std::mem::take(values),
                _ => unreachable!(),
            };
            self.values[list as usize] = JetVal::List(dense.into_iter().map(JetVal::Int).collect());
        }
        match self.values.get_mut(list as usize)? {
            JetVal::List(values) => Some(values),
            _ => None,
        }
    }

    pub fn list_get_int(&self, list: i64, index: i64) -> Option<i64> {
        if index < 0 {
            return None;
        }
        match self.values.get(list as usize) {
            Some(JetVal::IntList(values)) => values.get(index as usize).copied(),
            Some(JetVal::List(values)) => match values.get(index as usize) {
                Some(JetVal::Int(value)) => Some(*value),
                _ => None,
            },
            Some(JetVal::UninitList {
                values,
                initialized,
            }) => {
                let index = uninit_semantics::jet_uninit_read(initialized, index as usize).ok()?;
                match values.get(index) {
                    Some(JetVal::Int(value)) => Some(*value),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Read a sema-proven fixed-list integer slot. This path has no user-facing
    /// bounds check; an invalid carrier is an internal compiler/runtime fault.
    pub fn list_get_int_proven(&self, list: i64, index: i64) -> i64 {
        let value = match self.values.get(list as usize) {
            Some(JetVal::IntList(values)) => return values[index as usize],
            Some(JetVal::List(values)) => &values[index as usize],
            Some(JetVal::UninitList {
                values,
                initialized,
            }) => {
                let index = uninit_semantics::jet_uninit_read(initialized, index as usize)
                    .expect("proven fixed-list read of uninitialized slot");
                &values[index]
            }
            _ => jet_foundation::ice!(None, "proven fixed-list read of a non-list carrier"),
        };
        match value {
            JetVal::Int(value) => *value,
            _ => jet_foundation::ice!(None, "proven fixed-list integer read of a non-integer slot"),
        }
    }

    /// Clone one string handle from a homogeneous `[String]` list. JIT host
    /// adapters use this to marshal lists into shared Prelude functions;
    /// numeric list storage remains unchanged because string handles are the
    /// established erased `Int` representation.
    pub fn list_get_string(&self, list: i64, index: i64) -> Option<String> {
        if index < 0 {
            return None;
        }
        let handle = match self.values.get(list as usize)? {
            JetVal::IntList(values) => Some(*values.get(index as usize)?),
            JetVal::List(values) => match values.get(index as usize)? {
                JetVal::Int(handle) => Some(*handle),
                JetVal::String(value) => return Some(value.clone()),
                _ => None,
            },
            _ => None,
        }?;
        self.clone_string(handle)
    }

    pub fn list_get_float(&self, list: i64, index: i64) -> Option<f64> {
        if index < 0 {
            return None;
        }
        match self.values.get(list as usize) {
            Some(JetVal::List(values)) => match values.get(index as usize) {
                Some(JetVal::Float(value)) => Some(*value),
                _ => None,
            },
            Some(JetVal::UninitList {
                values,
                initialized,
            }) => {
                let index = uninit_semantics::jet_uninit_read(initialized, index as usize).ok()?;
                match values.get(index) {
                    Some(JetVal::Float(value)) => Some(*value),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Read a sema-proven fixed-list float slot without a language bounds check.
    pub fn list_get_float_proven(&self, list: i64, index: i64) -> f64 {
        let value = match self.values.get(list as usize) {
            Some(JetVal::List(values)) => &values[index as usize],
            Some(JetVal::UninitList {
                values,
                initialized,
            }) => {
                let index = uninit_semantics::jet_uninit_read(initialized, index as usize)
                    .expect("proven fixed-list read of uninitialized slot");
                &values[index]
            }
            _ => jet_foundation::ice!(None, "proven fixed-list read of a non-list carrier"),
        };
        match value {
            JetVal::Float(value) => *value,
            _ => jet_foundation::ice!(None, "proven fixed-list float read of a non-float slot"),
        }
    }

    pub fn list_get_range(&self, list: i64, index: i64) -> Option<(i64, i64, bool)> {
        if index < 0 {
            return None;
        }
        match self.values.get(list as usize) {
            Some(JetVal::List(values)) => match values.get(index as usize) {
                Some(JetVal::Range {
                    start,
                    end,
                    exclusive,
                }) => Some((*start, *end, *exclusive)),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn list_set_int(&mut self, list: i64, index: i64, value: i64) -> Option<()> {
        if index < 0 {
            return None;
        }
        match self.values.get_mut(list as usize) {
            Some(JetVal::IntList(values)) => {
                *values.get_mut(index as usize)? = value;
                Some(())
            }
            Some(JetVal::List(values)) => match values.get_mut(index as usize) {
                Some(slot @ JetVal::Int(_)) => {
                    *slot = JetVal::Int(value);
                    Some(())
                }
                _ => None,
            },
            Some(JetVal::UninitList {
                values,
                initialized,
            }) => {
                let (index, _) =
                    uninit_semantics::jet_uninit_write(initialized, index as usize).ok()?;
                values[index] = JetVal::Int(value);
                Some(())
            }
            _ => None,
        }
    }

    pub fn list_set_float(&mut self, list: i64, index: i64, value: f64) -> Option<()> {
        if index < 0 {
            return None;
        }
        match self.values.get_mut(list as usize) {
            Some(JetVal::List(values)) => match values.get_mut(index as usize) {
                Some(slot @ JetVal::Float(_)) => {
                    *slot = JetVal::Float(value);
                    Some(())
                }
                _ => None,
            },
            Some(JetVal::UninitList {
                values,
                initialized,
            }) => {
                let (index, _) =
                    uninit_semantics::jet_uninit_write(initialized, index as usize).ok()?;
                values[index] = JetVal::Float(value);
                Some(())
            }
            _ => None,
        }
    }

    pub fn list_sort_int(&mut self, list: i64) -> Option<()> {
        match self.values.get_mut(list as usize) {
            Some(JetVal::IntList(values)) => {
                values.sort_unstable();
                Some(())
            }
            Some(JetVal::List(values)) => {
                let mut ints = Vec::with_capacity(values.len());
                for value in values.iter() {
                    let JetVal::Int(value) = value else {
                        return None;
                    };
                    ints.push(*value);
                }
                ints.sort_unstable();
                *values = ints.into_iter().map(JetVal::Int).collect();
                Some(())
            }
            _ => None,
        }
    }

    pub fn list_slice(&mut self, list: i64, start: i64, end: i64) -> Option<i64> {
        if start < 0 || end < start {
            return None;
        }
        let slice = match self.values.get(list as usize) {
            Some(JetVal::IntList(values)) if end <= values.len() as i64 => {
                JetVal::IntList(values[start as usize..end as usize].to_vec())
            }
            Some(JetVal::List(values)) if end <= values.len() as i64 => {
                JetVal::List(values[start as usize..end as usize].to_vec())
            }
            _ => return None,
        };
        let id = self.values.len() as i64;
        self.values.push(slice);
        Some(id)
    }

    pub fn clone_int_list(&self, list: i64) -> Option<Vec<i64>> {
        match self.values.get(list as usize) {
            Some(JetVal::IntList(values)) => Some(values.clone()),
            Some(JetVal::List(values)) => {
                let mut out = Vec::with_capacity(values.len());
                for value in values {
                    let JetVal::Int(value) = value else {
                        return None;
                    };
                    out.push(*value);
                }
                Some(out)
            }
            Some(JetVal::UninitList {
                values,
                initialized,
            }) => {
                uninit_semantics::jet_uninit_all(initialized).ok()?;
                let mut out = Vec::with_capacity(values.len());
                for value in values {
                    let JetVal::Int(value) = value else {
                        return None;
                    };
                    out.push(*value);
                }
                Some(out)
            }
            _ => None,
        }
    }

    /// Borrow the dense carrier used by the resident JIT's proven native loop.
    /// The caller must keep this arena alive and must not mutate this list while
    /// using the pointer.
    pub fn int_list_ptr(&self, list: i64) -> Option<*const i64> {
        match self.values.get(list as usize) {
            Some(JetVal::IntList(values)) => Some(values.as_ptr()),
            _ => None,
        }
    }

    pub fn clone_list(&mut self, list: i64) -> Option<i64> {
        let value = match self.values.get(list as usize) {
            Some(JetVal::IntList(values)) => JetVal::IntList(values.clone()),
            Some(JetVal::List(values)) => JetVal::List(values.clone()),
            Some(JetVal::UninitList {
                values,
                initialized,
            }) => JetVal::UninitList {
                values: values.clone(),
                initialized: initialized.clone(),
            },
            _ => return None,
        };
        let id = self.values.len() as i64;
        self.values.push(value);
        Some(id)
    }

    pub fn alloc_record(&mut self, fields: usize) -> i64 {
        let id = self.values.len() as i64;
        self.values
            .push(JetVal::Record(vec![JetVal::Int(0); fields]));
        id
    }

    fn record_set(&mut self, record: i64, index: i64, value: JetVal) -> Option<()> {
        if index < 0 {
            return None;
        }
        match self.values.get_mut(record as usize) {
            Some(JetVal::Record(fields)) => {
                let slot = fields.get_mut(index as usize)?;
                *slot = value;
                Some(())
            }
            _ => None,
        }
    }

    fn record_get(&self, record: i64, index: i64) -> Option<&JetVal> {
        if index < 0 {
            return None;
        }
        match self.values.get(record as usize) {
            Some(JetVal::Record(fields)) => fields.get(index as usize),
            _ => None,
        }
    }

    pub fn record_set_int(&mut self, record: i64, index: i64, value: i64) -> Option<()> {
        self.record_set(record, index, JetVal::Int(value))
    }

    pub fn record_set_float(&mut self, record: i64, index: i64, value: f64) -> Option<()> {
        self.record_set(record, index, JetVal::Float(value))
    }

    pub fn record_set_bool(&mut self, record: i64, index: i64, value: bool) -> Option<()> {
        self.record_set(record, index, JetVal::Bool(value))
    }

    pub fn record_set_char(&mut self, record: i64, index: i64, value: char) -> Option<()> {
        self.record_set(record, index, JetVal::Char(value))
    }

    pub fn record_set_string(&mut self, record: i64, index: i64, value_handle: i64) -> Option<()> {
        let value = self.clone_string(value_handle)?;
        self.record_set(record, index, JetVal::String(value))
    }

    pub fn record_set_record(&mut self, record: i64, index: i64, value: i64) -> Option<()> {
        if !matches!(self.values.get(value as usize), Some(JetVal::Record(_))) {
            return None;
        }
        self.record_set(record, index, JetVal::RecordRef(value))
    }

    /// Whole-record write-through for `(*self) = …` / D-MUTSELF1 (keeps the
    /// caller's handle identity; replaces field slots from `src`).
    pub fn record_assign_from(&mut self, dst: i64, src: i64) -> Option<()> {
        let src_fields = match self.values.get(src as usize)? {
            JetVal::Record(fields) => fields.clone(),
            _ => return None,
        };
        match self.values.get_mut(dst as usize)? {
            JetVal::Record(dst_fields) => {
                *dst_fields = src_fields;
                Some(())
            }
            _ => None,
        }
    }

    /// D-SOA-TIER1=A: the logical rows of a list whose elements are records,
    /// as cell vectors in stored-field order.
    ///
    /// The Cranelift tier holds a `#Layout(columnar)` list as its rows, exactly
    /// as the interpreter ambient does, and marshals them into THE shared
    /// Prelude column store for the two reads the layout defines. This is the
    /// read side of that marshalling: it hands the rows over as cells so the
    /// host never has to know the arena's private shape, and the store — not an
    /// engine — owns the layout, the row bookkeeping and the bounds policy (I9).
    pub fn record_rows(&self, list: i64) -> Option<Vec<Vec<JetVal>>> {
        let values = match self.values.get(list as usize)? {
            JetVal::IntList(values) => values.iter().copied().map(JetVal::Int).collect(),
            JetVal::List(values) => values.clone(),
            _ => return None,
        };
        values
            .iter()
            .map(|value| match value {
                // A struct element is an arena record reached through its
                // handle; `Record` in place is the same row already inline.
                JetVal::Int(handle) | JetVal::RecordRef(handle) => {
                    match self.values.get(*handle as usize) {
                        Some(JetVal::Record(fields)) => Some(fields.clone()),
                        _ => None,
                    }
                }
                JetVal::Record(fields) => Some(fields.clone()),
                _ => None,
            })
            .collect()
    }

    /// The write side of the same marshalling: one record straight from the
    /// cells a gather returned. Cell order is column order, which is the slot
    /// order the tier already lays a record of that struct out in, so the row is
    /// rebuilt slot-for-slot with no second numbering.
    pub fn alloc_record_cells(&mut self, cells: Vec<JetVal>) -> i64 {
        let id = self.values.len() as i64;
        self.values.push(JetVal::Record(cells));
        id
    }

    pub fn record_get_int(&self, record: i64, index: i64) -> Option<i64> {
        match self.record_get(record, index) {
            Some(JetVal::Int(value)) => Some(*value),
            Some(JetVal::RecordRef(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn record_get_float(&self, record: i64, index: i64) -> Option<f64> {
        match self.record_get(record, index) {
            Some(JetVal::Float(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn record_get_bool(&self, record: i64, index: i64) -> Option<bool> {
        match self.record_get(record, index) {
            Some(JetVal::Bool(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn record_get_char(&self, record: i64, index: i64) -> Option<char> {
        match self.record_get(record, index) {
            Some(JetVal::Char(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn record_clone_string(&self, record: i64, index: i64) -> Option<String> {
        match self.record_get(record, index) {
            Some(JetVal::String(value)) => Some(value.clone()),
            _ => None,
        }
    }

    pub fn record_get_string(&mut self, record: i64, index: i64) -> Option<i64> {
        let value = match self.record_get(record, index)? {
            JetVal::String(value) => value.clone(),
            _ => return None,
        };
        Some(self.alloc_string(value))
    }

    // ── D-INTBIG1: packed default `Int` ───────────────────────────────────
    // A resident signed 63-bit payload is its own value. Larger values use
    // the same arena and CtBigInt limbs as the spill carrier; the public
    // language type is still only `Int`.
    pub const INT_SMALL_MIN: i64 = -(1i64 << 62);
    pub const INT_SMALL_MAX: i64 = (1i64 << 62) - 1;
    const INT_BIG_TAG: i64 = i64::MIN + 1;

    fn int_is_tagged(value: i64) -> bool {
        (Self::INT_BIG_TAG..Self::INT_SMALL_MIN).contains(&value)
    }

    fn int_big_value(&self, value: i64) -> Option<jet_foundation::Numeric::CtBigInt> {
        if !Self::int_is_tagged(value) {
            return None;
        }
        let id = value.wrapping_sub(Self::INT_BIG_TAG) as usize;
        match self.values.get(id) {
            Some(JetVal::ExactInt(value)) => Some(value.clone()),
            _ => None,
        }
    }

    fn int_value(&self, value: i64) -> jet_foundation::Numeric::CtBigInt {
        self.int_big_value(value)
            .unwrap_or_else(|| jet_foundation::Numeric::CtBigInt::from_int(value))
    }

    fn int_pack(&mut self, value: jet_foundation::Numeric::CtBigInt) -> i64 {
        if let Some(small) = value.try_i64() {
            if (Self::INT_SMALL_MIN..=Self::INT_SMALL_MAX).contains(&small) {
                return small;
            }
        }
        if let Some(id) = self.values.iter().position(|existing| {
            matches!(existing, JetVal::ExactInt(existing) if existing == &value)
        }) {
            return Self::INT_BIG_TAG.wrapping_add(id as i64);
        }
        let id = self.values.len() as i64;
        self.values.push(JetVal::ExactInt(value));
        Self::INT_BIG_TAG.wrapping_add(id)
    }

    pub fn int_from_i64(&mut self, value: i64) -> i64 {
        if (Self::INT_SMALL_MIN..=Self::INT_SMALL_MAX).contains(&value) {
            value
        } else {
            self.int_pack(jet_foundation::Numeric::CtBigInt::from_int(value))
        }
    }

    pub fn int_from_u64(&mut self, value: u64) -> i64 {
        if value <= Self::INT_SMALL_MAX as u64 {
            value as i64
        } else {
            self.int_pack(jet_foundation::Numeric::CtBigInt::from_u64(value))
        }
    }

    pub fn int_from_str(&mut self, value: &str) -> Result<i64, String> {
        Ok(self.int_pack(jet_foundation::Numeric::CtBigInt::from_str(value)?))
    }

    pub fn int_to_i64(&self, value: i64) -> Option<i64> {
        match self.int_big_value(value) {
            Some(value) => value.try_i64(),
            None => Some(value),
        }
    }

    pub fn int_to_i128(&self, value: i64) -> Option<i128> {
        self.int_big_value(value)
            .map(|value| value.try_i128())
            .unwrap_or_else(|| Some(i128::from(value)))
    }

    pub fn int_is_zero(&self, value: i64) -> bool {
        self.int_big_value(value)
            .map(|value| value.is_zero())
            .unwrap_or(value == 0)
    }

    pub fn int_is_negative(&self, value: i64) -> bool {
        self.int_big_value(value)
            .map(|value| value.negative)
            .unwrap_or(value < 0)
    }

    pub fn int_to_string(&self, value: i64) -> String {
        self.int_big_value(value)
            .map(|value| value.to_string_rep())
            .unwrap_or_else(|| value.to_string())
    }

    pub fn int_to_f64(&self, value: i64) -> f64 {
        self.int_to_string(value)
            .parse::<f64>()
            .unwrap_or_else(|_| {
                if self.int_is_negative(value) {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                }
            })
    }

    pub fn int_checked_widen(&self, value: i64, target_f32: bool) -> Option<f64> {
        self.int_value(value).checked_widen(target_f32)
    }

    pub fn int_bit_count(&self, value: i64, width: u32, method: &str) -> Option<i64> {
        self.int_value(value).bit_count(width, method)
    }

    pub fn int_compare(&self, left: i64, right: i64) -> i64 {
        if !Self::int_is_tagged(left) && !Self::int_is_tagged(right) {
            return match left.cmp(&right) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            };
        }
        match self.int_value(left).compare(&self.int_value(right)) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }

    pub fn int_add(&mut self, left: i64, right: i64) -> i64 {
        if !Self::int_is_tagged(left) && !Self::int_is_tagged(right) {
            if let Some(value) = left.checked_add(right) {
                if (Self::INT_SMALL_MIN..=Self::INT_SMALL_MAX).contains(&value) {
                    return value;
                }
            }
        }
        self.int_pack(self.int_value(left).add(&self.int_value(right)))
    }

    pub fn int_sub(&mut self, left: i64, right: i64) -> i64 {
        if !Self::int_is_tagged(left) && !Self::int_is_tagged(right) {
            if let Some(value) = left.checked_sub(right) {
                if (Self::INT_SMALL_MIN..=Self::INT_SMALL_MAX).contains(&value) {
                    return value;
                }
            }
        }
        self.int_pack(self.int_value(left).sub(&self.int_value(right)))
    }

    pub fn int_mul(&mut self, left: i64, right: i64) -> i64 {
        if !Self::int_is_tagged(left) && !Self::int_is_tagged(right) {
            if let Some(value) = left.checked_mul(right) {
                if (Self::INT_SMALL_MIN..=Self::INT_SMALL_MAX).contains(&value) {
                    return value;
                }
            }
        }
        self.int_pack(self.int_value(left).mul(&self.int_value(right)))
    }

    pub fn int_bit_and(&mut self, left: i64, right: i64) -> i64 {
        let left = self.int_value(left);
        let right = self.int_value(right);
        self.int_pack(left.bit_and(&right))
    }

    pub fn int_bit_or(&mut self, left: i64, right: i64) -> i64 {
        let left = self.int_value(left);
        let right = self.int_value(right);
        self.int_pack(left.bit_or(&right))
    }

    pub fn int_bit_xor(&mut self, left: i64, right: i64) -> i64 {
        let left = self.int_value(left);
        let right = self.int_value(right);
        self.int_pack(left.bit_xor(&right))
    }

    pub fn int_neg(&mut self, value: i64) -> i64 {
        if !Self::int_is_tagged(value) {
            if let Some(value) = value.checked_neg() {
                if (Self::INT_SMALL_MIN..=Self::INT_SMALL_MAX).contains(&value) {
                    return value;
                }
            }
        }
        self.int_pack(self.int_value(value).neg())
    }

    pub fn int_abs(&mut self, value: i64) -> i64 {
        if self.int_is_negative(value) {
            self.int_neg(value)
        } else {
            value
        }
    }

    pub fn int_try_from(&self, value: i64, kind: i64) -> Option<i128> {
        let value = self.int_to_i128(value)?;
        let (lo, hi) = match kind {
            0 => (i8::MIN as i128, i8::MAX as i128),
            1 => (i16::MIN as i128, i16::MAX as i128),
            2 => (i32::MIN as i128, i32::MAX as i128),
            3 => (i64::MIN as i128, i64::MAX as i128),
            4 => (u8::MIN as i128, u8::MAX as i128),
            5 => (u16::MIN as i128, u16::MAX as i128),
            6 => (u32::MIN as i128, u32::MAX as i128),
            7 => (u64::MIN as i128, u64::MAX as i128),
            _ => return None,
        };
        (lo..=hi).contains(&value).then_some(value)
    }

    pub fn int_not(&mut self, value: i64) -> i64 {
        let negated = self.int_neg(value);
        let one = self.int_from_i64(1);
        self.int_sub(negated, one)
    }

    pub fn int_shl(&mut self, value: i64, count: i64) -> Option<i64> {
        let value = self.int_value(value);
        let count = self.int_value(count);
        Some(self.int_pack(value.shl(&count)?))
    }

    pub fn int_shr(&mut self, value: i64, count: i64) -> Option<i64> {
        let value = self.int_value(value);
        let count = self.int_value(count);
        Some(self.int_pack(value.shr(&count)?))
    }

    pub fn int_div_rem(&mut self, value: i64, divisor: i64) -> Option<(i64, i64)> {
        if self.int_is_zero(divisor) {
            return None;
        }
        if !Self::int_is_tagged(value) && !Self::int_is_tagged(divisor) {
            if let (Some(quotient), Some(remainder)) =
                (value.checked_div(divisor), value.checked_rem(divisor))
            {
                return Some((quotient, remainder));
            }
        }
        let (quotient, remainder) = self.int_value(value).div_rem(&self.int_value(divisor))?;
        Some((self.int_pack(quotient), self.int_pack(remainder)))
    }

    pub fn int_div(&mut self, value: i64, divisor: i64) -> Option<i64> {
        Some(self.int_div_rem(value, divisor)?.0)
    }

    pub fn int_rem(&mut self, value: i64, divisor: i64) -> Option<i64> {
        Some(self.int_div_rem(value, divisor)?.1)
    }

    pub fn int_div_rem_euclid(&mut self, value: i64, divisor: i64) -> Option<(i64, i64)> {
        let (quotient, remainder) = self
            .int_value(value)
            .div_rem_euclid(&self.int_value(divisor))?;
        Some((self.int_pack(quotient), self.int_pack(remainder)))
    }

    pub fn int_div_euclid(&mut self, value: i64, divisor: i64) -> Option<i64> {
        Some(self.int_div_rem_euclid(value, divisor)?.0)
    }

    pub fn int_rem_euclid(&mut self, value: i64, divisor: i64) -> Option<i64> {
        Some(self.int_div_rem_euclid(value, divisor)?.1)
    }

    pub fn int_floor_div(&mut self, value: i64, divisor: i64) -> Option<i64> {
        let (quotient, remainder) = self.int_div_rem(value, divisor)?;
        if !self.int_is_zero(remainder)
            && self.int_is_negative(value) != self.int_is_negative(divisor)
        {
            let one = self.int_from_i64(1);
            Some(self.int_sub(quotient, one))
        } else {
            Some(quotient)
        }
    }

    pub fn int_mod(&mut self, value: i64, divisor: i64) -> Option<i64> {
        let (quotient, remainder) = self.int_div_rem(value, divisor)?;
        if !self.int_is_zero(remainder)
            && self.int_is_negative(value) != self.int_is_negative(divisor)
        {
            Some(self.int_add(remainder, divisor))
        } else {
            let _ = quotient;
            Some(remainder)
        }
    }

    pub fn int_pow(&mut self, value: i64, exponent: i64) -> Option<i64> {
        self.int_value(value)
            .pow(&self.int_value(exponent))
            .map(|result| self.int_pack(result))
    }

    pub fn int_factorial(&mut self, value: i64) -> Option<i64> {
        if self.int_is_negative(value) {
            return None;
        }
        let mut current = self.int_from_i64(2);
        let mut result = self.int_from_i64(1);
        while self.int_compare(current, value) <= 0 {
            result = self.int_mul(result, current);
            let one = self.int_from_i64(1);
            current = self.int_add(current, one);
        }
        Some(result)
    }

    pub fn int_is_even(&self, value: i64) -> bool {
        self.int_value(value).is_even()
    }

    pub fn int_is_odd(&self, value: i64) -> bool {
        self.int_value(value).is_odd()
    }

    pub fn int_isqrt(&mut self, value: i64) -> Option<i64> {
        self.int_value(value)
            .isqrt()
            .map(|result| self.int_pack(result))
    }

    pub fn int_binomial(&mut self, n: i64, k: i64) -> Option<i64> {
        let n = self.int_value(n);
        let k = self.int_value(k);
        jet_foundation::Numeric::CtBigInt::binomial(&n, &k).map(|result| self.int_pack(result))
    }

    pub fn int_digits(&self, value: i64) -> i64 {
        self.int_value(value).digits()
    }

    pub fn int_leading_ones(&self, value: i64) -> i64 {
        self.int_value(value).leading_ones()
    }

    pub fn int_trailing_ones(&self, value: i64) -> i64 {
        self.int_value(value).trailing_ones()
    }

    pub fn int_checked_abs(&mut self, value: i64) -> Option<i64> {
        Some(self.int_abs(value))
    }

    pub fn int_checked_neg(&mut self, value: i64) -> Option<i64> {
        Some(self.int_neg(value))
    }

    pub fn int_checked_add(&mut self, left: i64, right: i64) -> Option<i64> {
        Some(self.int_add(left, right))
    }

    pub fn int_checked_sub(&mut self, left: i64, right: i64) -> Option<i64> {
        Some(self.int_sub(left, right))
    }

    pub fn int_checked_mul(&mut self, left: i64, right: i64) -> Option<i64> {
        Some(self.int_mul(left, right))
    }

    pub fn int_checked_div(&mut self, left: i64, right: i64) -> Option<i64> {
        self.int_div(left, right)
    }

    pub fn int_checked_rem(&mut self, left: i64, right: i64) -> Option<i64> {
        self.int_rem(left, right)
    }

    pub fn int_checked_pow(&mut self, left: i64, right: i64) -> Option<i64> {
        self.int_pow(left, right)
    }

    pub fn int_saturating_add(&mut self, left: i64, right: i64) -> i64 {
        self.int_add(left, right)
    }

    pub fn int_saturating_sub(&mut self, left: i64, right: i64) -> i64 {
        self.int_sub(left, right)
    }

    pub fn int_saturating_mul(&mut self, left: i64, right: i64) -> i64 {
        self.int_mul(left, right)
    }

    pub fn int_int_pow(&mut self, left: i64, right: i64) -> i64 {
        self.int_pow(left, right)
            .unwrap_or_else(|| self.int_from_i64(0))
    }

    pub fn int_gcd(&mut self, left: i64, right: i64) -> i64 {
        let left = self.int_value(left);
        let right = self.int_value(right);
        self.int_pack(jet_foundation::Numeric::CtBigInt::gcd(&left, &right))
    }

    pub fn int_lcm(&mut self, left: i64, right: i64) -> i64 {
        let left = self.int_value(left);
        let right = self.int_value(right);
        self.int_pack(jet_foundation::Numeric::CtBigInt::lcm(&left, &right))
    }

    pub fn int_div_mod(&mut self, value: i64, divisor: i64) -> Option<(i64, i64)> {
        let (quotient, remainder) = self.int_div_rem(value, divisor)?;
        if !self.int_is_zero(remainder)
            && self.int_is_negative(value) != self.int_is_negative(divisor)
        {
            let one = self.int_from_i64(1);
            let quotient = self.int_sub(quotient, one);
            let remainder = self.int_add(remainder, divisor);
            Some((quotient, remainder))
        } else {
            Some((quotient, remainder))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_matches_aot_float_prelude() {
        assert_eq!(display_f32(1.0), "1.0");
        assert_eq!(display_f64(-5.0), "-5.0");
        assert_eq!(display_f64(2.5), "2.5");
    }

    #[test]
    fn exact_int_small_path_and_spill_boundary() {
        let mut arena = JetArena::default();
        let before = arena.values.len();

        let inline = arena.int_add(JetArena::INT_SMALL_MAX - 1, 1);
        assert_eq!(inline, JetArena::INT_SMALL_MAX);
        assert_eq!(
            arena.values.len(),
            before,
            "small exact Int arithmetic must not allocate a spill value"
        );

        let positive_spill = arena.int_add(JetArena::INT_SMALL_MAX, 1);
        assert_eq!(arena.values.len(), before + 1);
        assert_eq!(arena.int_to_string(positive_spill), "4611686018427387904");

        let negative_spill = arena.int_sub(JetArena::INT_SMALL_MIN, 1);
        assert_eq!(arena.values.len(), before + 2);
        assert_eq!(arena.int_to_string(negative_spill), "-4611686018427387905");
    }

    #[test]
    fn string_helpers_match_prelude_semantics() {
        assert_eq!(string_len_chars("aé日"), 3);
        assert_eq!(string_trim("  jet\n"), "jet");
        assert_eq!(string_to_upper("Jet"), "JET");
        assert_eq!(string_to_lower("Jet"), "jet");
        assert_eq!(string_to_lower("\u{A7CE}"), "\u{A7CE}");
        assert_eq!(string_to_upper("\u{A7CF}"), "\u{A7CF}");
        assert_eq!(string_trim("\u{2003}jet\u{2003}"), "jet");
        assert_eq!(string_replace("one two one", "one", "1"), "1 two 1");
        assert_eq!(string_after("nate@jet-lang.dev", "@"), "jet-lang.dev");
        assert_eq!(string_before("nate@jet-lang.dev", "@"), "nate");
        assert_eq!(string_after("no-at-sign", "@"), "no-at-sign");
        assert_eq!(string_before("no-at-sign", "@"), "no-at-sign");
    }

    #[test]
    fn arena_allocates_string_handles() {
        let mut arena = JetArena::default();
        let first = arena.alloc_empty_string();
        let second = arena.alloc_string("jet");

        arena.get_string_mut(first).unwrap().push_str("dev");

        assert_eq!(first, 0);
        assert_eq!(second, 1);
        assert_eq!(arena.get_string(first), Some("dev"));
        assert_eq!(arena.clone_string(second), Some("jet".to_string()));

        arena.clear();
        assert_eq!(arena.get_string(first), None);
    }

    #[test]
    fn arena_allocates_int_lists() {
        let mut arena = JetArena::default();
        let list = arena.alloc_int_list(vec![3, 1, 2]);

        arena.list_push_int(list, 4).unwrap();
        arena.list_sort_int(list).unwrap();
        assert_eq!(arena.list_len(list), Some(4));
        assert_eq!(arena.list_get_int(list, 2), Some(3));
        arena.list_set_int(list, 2, 9).unwrap();
        assert_eq!(arena.clone_int_list(list), Some(vec![1, 2, 9, 4]));

        let slice = arena.list_slice(list, 1, 3).unwrap();
        assert_eq!(arena.clone_int_list(slice), Some(vec![2, 9]));
    }

    #[test]
    fn arena_allocates_float_lists() {
        let mut arena = JetArena::default();
        let list = arena.alloc_empty_list();

        arena.list_push_float(list, 1.5).unwrap();
        arena.list_push_float(list, 2.5).unwrap();
        assert_eq!(arena.list_get_float(list, 1), Some(2.5));
        arena.list_set_float(list, 1, 4.5).unwrap();
        assert_eq!(arena.list_get_float(list, 1), Some(4.5));

        let slice = arena.list_slice(list, 0, 1).unwrap();
        assert_eq!(arena.list_get_float(slice, 0), Some(1.5));
    }

    #[test]
    fn arena_stores_ranges_inline_in_lists() {
        let mut arena = JetArena::default();
        let list = arena.alloc_empty_list();

        arena.list_push_range(list, 2, 5, true).unwrap();

        assert_eq!(arena.list_get_range(list, 0), Some((2, 5, true)));
        assert_eq!(
            arena.values.len(),
            1,
            "a Range list element must not allocate an arena record"
        );
    }

    #[test]
    fn arena_allocates_records() {
        let mut arena = JetArena::default();
        let name = arena.alloc_string("jet");
        let record = arena.alloc_record(4);

        arena.record_set_string(record, 0, name).unwrap();
        arena.record_set_float(record, 1, 2.5).unwrap();
        arena.record_set_bool(record, 2, true).unwrap();
        arena.record_set_char(record, 3, 'J').unwrap();

        let cloned_name = arena.record_get_string(record, 0).unwrap();
        assert_eq!(arena.get_string(cloned_name), Some("jet"));
        assert_eq!(arena.record_get_float(record, 1), Some(2.5));
        assert_eq!(arena.record_get_bool(record, 2), Some(true));
        assert_eq!(arena.record_get_char(record, 3), Some('J'));
    }

    #[test]
    fn string_views_borrow_owned_storage_and_validate_bounds() {
        let mut arena = JetArena::default();
        let owner = arena.alloc_string("αbeta");
        let view = arena.alloc_string_view(owner, 2, 6).unwrap();
        let nested = arena.alloc_string_view(view, 1, 4).unwrap();
        assert_eq!(arena.get_string(view), Some("beta"));
        assert_eq!(arena.get_string(nested), Some("eta"));
        assert_eq!(arena.string_slots(), vec![(0, "αbeta".to_string())]);
        assert_eq!(arena.alloc_string_view(owner, 1, 3), None);
        assert_eq!(arena.alloc_string_view(999, 0, 0), None);
    }
}
