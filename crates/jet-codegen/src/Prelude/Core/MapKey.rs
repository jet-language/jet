
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
pub fn jet_map_key_cmp(left: &JetMapKey, right: &JetMapKey) -> std::cmp::Ordering {
    match (left, right) {
        (JetMapKey::Int(left), JetMapKey::Int(right)) => left.cmp(right),
        (JetMapKey::UInt(left), JetMapKey::UInt(right)) => left.cmp(right),
        (JetMapKey::String(left), JetMapKey::String(right)) => left.cmp(right),
        (JetMapKey::Bool(left), JetMapKey::Bool(right)) => left.cmp(right),
        (JetMapKey::Char(left), JetMapKey::Char(right)) => left.cmp(right),
        (JetMapKey::Record(left), JetMapKey::Record(right)) => {
            for (left, right) in left.iter().zip(right) {
                let ordering = jet_map_key_cmp(left, right);
                if ordering != std::cmp::Ordering::Equal {
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
        jet_map_key_cmp(self, other) == std::cmp::Ordering::Equal
    }
}

impl Eq for JetMapKey {}

impl PartialOrd for JetMapKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(jet_map_key_cmp(self, other))
    }
}

impl Ord for JetMapKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        jet_map_key_cmp(self, other)
    }
}
