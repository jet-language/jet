use std::cmp::Ordering;

/// Structural carrier used by every map-key adapter.
#[derive(Clone, Debug)]
pub enum JetMapKey {
    Int(i64),
    UInt(u64),
    String(String),
    Bool(bool),
    Char(char),
    Record(Vec<JetMapKey>),
}

/// The one codegen Prelude seam for recursively encoding key-eligible values.
pub trait JetMapKeyEncode {
    fn jet_map_key(&self) -> JetMapKey;
}

impl JetMapKeyEncode for i8 {
    fn jet_map_key(&self) -> JetMapKey {
        JetMapKey::Int(*self as i64)
    }
}

impl JetMapKeyEncode for i16 {
    fn jet_map_key(&self) -> JetMapKey {
        JetMapKey::Int(*self as i64)
    }
}

impl JetMapKeyEncode for i32 {
    fn jet_map_key(&self) -> JetMapKey {
        JetMapKey::Int(*self as i64)
    }
}

impl JetMapKeyEncode for i64 {
    fn jet_map_key(&self) -> JetMapKey {
        JetMapKey::Int(*self)
    }
}

impl JetMapKeyEncode for u8 {
    fn jet_map_key(&self) -> JetMapKey {
        JetMapKey::UInt(*self as u64)
    }
}

impl JetMapKeyEncode for u16 {
    fn jet_map_key(&self) -> JetMapKey {
        JetMapKey::UInt(*self as u64)
    }
}

impl JetMapKeyEncode for u32 {
    fn jet_map_key(&self) -> JetMapKey {
        JetMapKey::UInt(*self as u64)
    }
}

impl JetMapKeyEncode for u64 {
    fn jet_map_key(&self) -> JetMapKey {
        JetMapKey::UInt(*self)
    }
}

impl JetMapKeyEncode for String {
    fn jet_map_key(&self) -> JetMapKey {
        JetMapKey::String(self.clone())
    }
}

impl JetMapKeyEncode for bool {
    fn jet_map_key(&self) -> JetMapKey {
        JetMapKey::Bool(*self)
    }
}

impl JetMapKeyEncode for char {
    fn jet_map_key(&self) -> JetMapKey {
        JetMapKey::Char(*self)
    }
}

fn jet_map_key_kind(key: &JetMapKey) -> u8 {
    match key {
        JetMapKey::Int(_) => 0,
        JetMapKey::UInt(_) => 1,
        JetMapKey::String(_) => 2,
        JetMapKey::Bool(_) => 3,
        JetMapKey::Char(_) => 4,
        JetMapKey::Record(_) => 5,
    }
}

/// The single deep value-semantic comparison used by map-key adapters.
pub fn jet_map_key_cmp(left: &JetMapKey, right: &JetMapKey) -> Ordering {
    match (left, right) {
        (JetMapKey::Int(left), JetMapKey::Int(right)) => left.cmp(right),
        (JetMapKey::UInt(left), JetMapKey::UInt(right)) => left.cmp(right),
        (JetMapKey::String(left), JetMapKey::String(right)) => left.cmp(right),
        (JetMapKey::Bool(left), JetMapKey::Bool(right)) => left.cmp(right),
        (JetMapKey::Char(left), JetMapKey::Char(right)) => left.cmp(right),
        (JetMapKey::Record(left), JetMapKey::Record(right)) => {
            for (left, right) in left.iter().zip(right) {
                let ordering = jet_map_key_cmp(left, right);
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            left.len().cmp(&right.len())
        }
        _ => jet_map_key_kind(left).cmp(&jet_map_key_kind(right)),
    }
}

impl PartialEq for JetMapKey {
    fn eq(&self, other: &Self) -> bool {
        jet_map_key_cmp(self, other) == Ordering::Equal
    }
}

impl Eq for JetMapKey {}

impl PartialOrd for JetMapKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(jet_map_key_cmp(self, other))
    }
}

impl Ord for JetMapKey {
    fn cmp(&self, other: &Self) -> Ordering {
        jet_map_key_cmp(self, other)
    }
}
