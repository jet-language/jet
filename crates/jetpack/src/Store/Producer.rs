use crate::Comptime::Build::BuildPlanReplay;
use crate::SHA256;
use std::collections::BTreeMap;

const HEADER: &str = "jet-producer-record-v1";

/// Immutable producer facts committed beside package/object relations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProducerRecord {
    pub provider: String,
    pub immutable_source: String,
    pub source_digest: String,
    pub plan: BuildPlanReplay,
    pub toolchain_facts: String,
    pub policy_facts: String,
    pub facts: BTreeMap<String, String>,
}

impl ProducerRecord {
    pub fn new(
        provider: impl Into<String>,
        immutable_source: impl Into<String>,
        source_digest: impl Into<String>,
        plan: BuildPlanReplay,
        toolchain_facts: impl Into<String>,
        policy_facts: impl Into<String>,
        facts: BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let record = Self {
            provider: provider.into(),
            immutable_source: immutable_source.into(),
            source_digest: source_digest.into(),
            plan,
            toolchain_facts: toolchain_facts.into(),
            policy_facts: policy_facts.into(),
            facts,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn encode(&self) -> String {
        let mut fields = BTreeMap::from([
            ("provider".to_string(), self.provider.clone()),
            ("immutable_source".to_string(), self.immutable_source.clone()),
            ("source_digest".to_string(), self.source_digest.clone()),
            ("plan".to_string(), self.plan.encode()),
            ("toolchain_facts".to_string(), self.toolchain_facts.clone()),
            ("policy_facts".to_string(), self.policy_facts.clone()),
        ]);
        for (key, value) in &self.facts {
            fields.insert(format!("fact.{key}"), value.clone());
        }
        let mut body = String::from(HEADER);
        body.push('\n');
        for (key, value) in fields {
            body.push_str(&hex(&key));
            body.push('\t');
            body.push_str(&hex(&value));
            body.push('\n');
        }
        let checksum = SHA256::sha256_hex(body.as_bytes());
        format!("{body}checksum\t{checksum}\n")
    }

    pub fn decode(raw: &str) -> Result<Self, String> {
        let Some((body, checksum)) = raw.rsplit_once("checksum\t") else {
            return Err("producer record is truncated".to_string());
        };
        if SHA256::sha256_hex(body.as_bytes()) != checksum.trim() {
            return Err("producer record checksum mismatch".to_string());
        }
        let mut lines = body.lines();
        if lines.next() != Some(HEADER) {
            return Err("unsupported producer record version".to_string());
        }
        let mut fields = BTreeMap::new();
        for line in lines {
            let (key, value) = line
                .split_once('\t')
                .ok_or_else(|| "producer record fact is truncated".to_string())?;
            let key = unhex(key)?;
            if fields.insert(key.clone(), unhex(value)?).is_some() {
                return Err(format!("duplicate producer record fact `{key}`"));
            }
        }
        let mut take = |key: &str| {
            fields
                .remove(key)
                .ok_or_else(|| format!("producer record misses `{key}`"))
        };
        let provider = take("provider")?;
        let immutable_source = take("immutable_source")?;
        let source_digest = take("source_digest")?;
        let plan = BuildPlanReplay::decode(&take("plan")?)?;
        let toolchain_facts = take("toolchain_facts")?;
        let policy_facts = take("policy_facts")?;
        let mut facts = BTreeMap::new();
        for (key, value) in fields {
            let Some(key) = key.strip_prefix("fact.") else {
                return Err(format!("unknown producer record field `{key}`"));
            };
            if key.is_empty() {
                return Err("producer record has empty fact name".to_string());
            }
            facts.insert(key.to_string(), value);
        }
        Self::new(
            provider,
            immutable_source,
            source_digest,
            plan,
            toolchain_facts,
            policy_facts,
            facts,
        )
    }

    fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("provider", &self.provider),
            ("immutable_source", &self.immutable_source),
            ("source_digest", &self.source_digest),
            ("toolchain_facts", &self.toolchain_facts),
            ("policy_facts", &self.policy_facts),
        ] {
            if value.trim().is_empty() {
                return Err(format!("producer record has empty `{name}`"));
            }
        }
        if self.facts.keys().any(|key| key.is_empty()) {
            return Err("producer record has empty fact name".to_string());
        }
        Ok(())
    }
}

fn hex(value: &str) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(value.len() * 2);
    for byte in value.bytes() {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}

fn unhex(value: &str) -> Result<String, String> {
    if value.len() % 2 != 0 {
        return Err("producer record field is truncated".to_string());
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        bytes.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
    }
    String::from_utf8(bytes).map_err(|_| "producer record field is not UTF-8".to_string())
}

fn nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err("producer record field is not lowercase hex".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> ProducerRecord {
        ProducerRecord::new(
            "nix",
            "/nix/store/exact.drv",
            "sha256-source",
            BuildPlanReplay::from_facts(BTreeMap::from([(
                "nix.output.out".into(),
                "/nix/store/exact-out".into(),
            )]))
            .unwrap(),
            "nix:2.30",
            "offline:false",
            BTreeMap::new(),
        )
        .unwrap()
    }

    #[test]
    fn producer_record_roundtrips_and_rejects_hostile_input() {
        let record = record();
        let encoded = record.encode();
        assert_eq!(ProducerRecord::decode(&encoded).unwrap(), record);
        assert!(ProducerRecord::decode(&encoded[..encoded.len() - 3]).is_err());
        let duplicate = encoded.replacen("checksum\t", "70726f7669646572\t78\nchecksum\t", 1);
        assert!(ProducerRecord::decode(&duplicate).is_err());
    }
}
