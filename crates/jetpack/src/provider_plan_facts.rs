use std::collections::BTreeMap;

const HEADER: &str = "jet-build-plan-replay-v1";

/// Versioned, canonical provider facts carried with a realized producer plan.
/// The codec is independent from compiler build-plan internals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPlanFacts {
    facts: BTreeMap<String, String>,
}

impl ProviderPlanFacts {
    pub fn from_facts(facts: BTreeMap<String, String>) -> Result<Self, String> {
        if facts.keys().any(|key| key.is_empty()) {
            return Err("provider plan facts contain an empty fact key".to_string());
        }
        Ok(Self { facts })
    }

    pub fn facts(&self) -> &BTreeMap<String, String> {
        &self.facts
    }

    pub fn encode(&self) -> String {
        let mut out = String::from(HEADER);
        out.push('\n');
        for (key, value) in &self.facts {
            out.push_str(&hex(key));
            out.push('\t');
            out.push_str(&hex(value));
            out.push('\n');
        }
        out
    }

    pub fn decode(raw: &str) -> Result<Self, String> {
        let mut lines = raw.lines();
        if lines.next() != Some(HEADER) {
            return Err("unsupported provider plan facts version".to_string());
        }
        let mut facts = BTreeMap::new();
        for line in lines {
            let (key, value) = line
                .split_once('\t')
                .ok_or_else(|| "truncated provider plan fact".to_string())?;
            let key = unhex(key)?;
            let value = unhex(value)?;
            if key.is_empty() {
                return Err("provider plan facts contain an empty fact key".to_string());
            }
            if facts.insert(key.clone(), value).is_some() {
                return Err(format!("duplicate provider plan fact `{key}`"));
            }
        }
        Self::from_facts(facts)
    }
}

fn hex(value: &str) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn unhex(value: &str) -> Result<String, String> {
    if value.len() % 2 != 0 {
        return Err("truncated provider plan fact field".to_string());
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        bytes.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
    }
    String::from_utf8(bytes).map_err(|_| "provider plan fact field is not UTF-8".to_string())
}

fn nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err("provider plan fact field is not lowercase hex".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_is_deterministic_roundtrippable_and_fail_closed() {
        let facts = ProviderPlanFacts::from_facts(BTreeMap::from([
            ("action.0.argv".to_string(), "cc\0-c\0main.c".to_string()),
            ("source.main".to_string(), "sha256-source".to_string()),
        ]))
        .unwrap();
        let encoded = facts.encode();
        assert_eq!(ProviderPlanFacts::decode(&encoded).unwrap(), facts);
        assert_eq!(facts.encode(), encoded);
        assert!(ProviderPlanFacts::decode("jet-build-plan-replay-v1\n61\t6").is_err());
        assert!(ProviderPlanFacts::decode(
            "jet-build-plan-replay-v1\n61\t62\n61\t63\n"
        )
        .is_err());
    }
}
