
/// Structural carrier used by every map-key adapter.
#[derive(Clone, Debug)]
pub enum JetMapKey {
    Int(i64),
    /// An exact `Int` outside the `i64` range: sign plus normalized magnitude
    /// limbs (little-endian base 10^9, no high zero limb). Values that fit
    /// `i64` always use `Int`, so each value has exactly one key.
    BigInt {
        negative: bool,
        limbs: Vec<u32>,
    },
    UInt(u64),
    String(String),
    Bool(bool),
    Char(char),
    Record(Vec<JetMapKey>),
}

fn jet_map_key_kind(key: &JetMapKey) -> u8 {
    match key {
        JetMapKey::Int(_) | JetMapKey::BigInt { .. } => 0,
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
        // A `BigInt` lies outside the `i64` range, so its sign alone orders it
        // against every `Int`.
        (JetMapKey::Int(_), JetMapKey::BigInt { negative, .. }) => {
            if *negative {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Less
            }
        }
        (JetMapKey::BigInt { negative, .. }, JetMapKey::Int(_)) => {
            if *negative {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        }
        (
            JetMapKey::BigInt {
                negative: left_negative,
                limbs: left,
            },
            JetMapKey::BigInt {
                negative: right_negative,
                limbs: right,
            },
        ) => {
            if left_negative != right_negative {
                return right_negative.cmp(left_negative);
            }
            let magnitude = left
                .len()
                .cmp(&right.len())
                .then_with(|| left.iter().rev().cmp(right.iter().rev()));
            if *left_negative {
                magnitude.reverse()
            } else {
                magnitude
            }
        }
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
