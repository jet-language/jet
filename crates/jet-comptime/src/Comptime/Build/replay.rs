use super::{BuildPlan, BuildResourcePool};
use std::collections::BTreeMap;

const HEADER: &str = "jet-build-plan-replay-v1";

/// Versioned, canonical action facts sufficient to reproduce a BuildPlan
/// without consulting mutable provider references or ambient host state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlanReplay {
    facts: BTreeMap<String, String>,
}

impl BuildPlanReplay {
    pub fn from_facts(facts: BTreeMap<String, String>) -> Result<Self, String> {
        if facts.keys().any(|key| key.is_empty()) {
            return Err("build replay contains an empty fact key".to_string());
        }
        Ok(BuildPlanReplay { facts })
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
            return Err("unsupported build replay version".to_string());
        }
        let mut facts = BTreeMap::new();
        for line in lines {
            let (key, value) = line
                .split_once('\t')
                .ok_or_else(|| "truncated build replay fact".to_string())?;
            let key = unhex(key)?;
            let value = unhex(value)?;
            if key.is_empty() {
                return Err("build replay contains an empty fact key".to_string());
            }
            if facts.insert(key.clone(), value).is_some() {
                return Err(format!("duplicate build replay fact `{key}`"));
            }
        }
        Self::from_facts(facts)
    }
}

impl BuildPlan {
    pub fn replay_record(&self) -> Result<BuildPlanReplay, String> {
        let mut facts = BTreeMap::new();
        facts.insert(
            "plan.recipe_fingerprint".to_string(),
            self.complete_recipe_fingerprint()
                .map_err(|error| format!("cannot fingerprint build plan: {error:?}"))?,
        );
        facts.insert(
            "plan.default".to_string(),
            self.default_target()
                .map(|target| target.id().0.to_string())
                .unwrap_or_default(),
        );
        facts.insert("plan.targets".to_string(), self.targets().len().to_string());
        facts.insert("plan.actions".to_string(), self.actions().len().to_string());
        for (index, target) in self.targets().iter().enumerate() {
            let prefix = format!("target.{index}");
            facts.insert(format!("{prefix}.name"), target.name.clone());
            facts.insert(format!("{prefix}.kind"), format!("{:?}", target.kind));
            facts.insert(
                format!("{prefix}.sources"),
                target
                    .sources
                    .iter()
                    .map(|path| path.as_str())
                    .collect::<Vec<_>>()
                    .join("\0"),
            );
            facts.insert(
                format!("{prefix}.inputs"),
                target
                    .inputs
                    .iter()
                    .map(|path| path.as_str())
                    .collect::<Vec<_>>()
                    .join("\0"),
            );
            facts.insert(
                format!("{prefix}.outputs"),
                target
                    .outputs
                    .iter()
                    .map(|path| path.as_str())
                    .collect::<Vec<_>>()
                    .join("\0"),
            );
            facts.insert(
                format!("{prefix}.deps"),
                target
                    .deps
                    .iter()
                    .map(|dep| dep.id().0.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            facts.insert(
                format!("{prefix}.actions"),
                target
                    .actions
                    .iter()
                    .map(|action| action.id().0.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            facts.insert(format!("{prefix}.toolchain"), target.toolchain.id().0.to_string());
            facts.insert(format!("{prefix}.metadata"), map_debug(&target.metadata));
        }
        for (index, action) in self.actions().iter().enumerate() {
            let prefix = format!("action.{index}");
            facts.insert(format!("{prefix}.name"), action.name.clone());
            facts.insert(format!("{prefix}.kind"), action.kind.as_str().to_string());
            facts.insert(format!("{prefix}.cache"), format!("{:?}", action.cache));
            facts.insert(format!("{prefix}.argv"), action.argv.join("\0"));
            facts.insert(
                format!("{prefix}.inputs"),
                action
                    .inputs
                    .iter()
                    .map(|path| path.as_str())
                    .collect::<Vec<_>>()
                    .join("\0"),
            );
            facts.insert(
                format!("{prefix}.outputs"),
                action
                    .outputs
                    .iter()
                    .map(|path| path.as_str())
                    .collect::<Vec<_>>()
                    .join("\0"),
            );
            facts.insert(format!("{prefix}.env"), map_debug(&action.env));
            facts.insert(
                format!("{prefix}.env_allowlist"),
                action.env_allowlist.iter().cloned().collect::<Vec<_>>().join("\0"),
            );
            facts.insert(
                format!("{prefix}.caps"),
                action
                    .caps
                    .iter()
                    .map(|cap| cap.name().to_string())
                    .collect::<Vec<_>>()
                    .join("\0"),
            );
            facts.insert(format!("{prefix}.toolchain"), action.toolchain.id().0.to_string());
            facts.insert(format!("{prefix}.labels"), map_debug(&action.labels));
            facts.insert(
                format!("{prefix}.helpers"),
                map_debug(&action.helper_versions),
            );
            facts.insert(
                format!("{prefix}.pools"),
                action
                    .resource_pools
                    .iter()
                    .map(pool_name)
                    .collect::<Vec<_>>()
                    .join("\0"),
            );
            facts.insert(
                format!("{prefix}.variant"),
                action.variant_identity.clone().unwrap_or_default(),
            );
            facts.insert(
                format!("{prefix}.legacy"),
                action
                    .legacy_wrapper
                    .map(|wrapper| wrapper.as_str().to_string())
                    .unwrap_or_default(),
            );
            facts.insert(
                format!("{prefix}.action_key"),
                self.action_key(super::ActionHandle {
                    id: action.id,
                    context: self.context,
                })
                .map_err(|error| format!("cannot key replay action: {error:?}"))?
                .as_str()
                .to_string(),
            );
        }
        for (index, toolchain) in self.toolchains().iter().enumerate() {
            facts.insert(format!("toolchain.{index}"), format!("{toolchain:?}"));
        }
        for (index, identity) in self.signing_identities().iter().enumerate() {
            facts.insert(format!("signing.{index}"), format!("{identity:?}"));
        }
        for (index, probe) in self.probes().iter().enumerate() {
            facts.insert(format!("probe.{index}"), format!("{probe:?}"));
        }
        for (index, plugin) in self.plugins().iter().enumerate() {
            facts.insert(format!("plugin.{index}"), format!("{plugin:?}"));
        }
        for (index, module) in self.generated_modules().iter().enumerate() {
            facts.insert(format!("generated.{index}"), format!("{module:?}"));
        }
        BuildPlanReplay::from_facts(facts)
    }
}

fn map_debug(map: &BTreeMap<String, String>) -> String {
    map.iter()
        .map(|(key, value)| format!("{}:{}:{}:{}", key.len(), key, value.len(), value))
        .collect::<Vec<_>>()
        .join("")
}

fn pool_name(pool: &BuildResourcePool) -> String {
    match pool {
        BuildResourcePool::Custom(name) => format!("custom:{name}"),
        _ => pool.as_str().to_string(),
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
        return Err("truncated build replay field".to_string());
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        bytes.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
    }
    String::from_utf8(bytes).map_err(|_| "build replay field is not UTF-8".to_string())
}

fn nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err("build replay field is not lowercase hex".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_encoding_is_deterministic_roundtrippable_and_fail_closed() {
        let replay = BuildPlanReplay::from_facts(BTreeMap::from([
            ("action.0.argv".to_string(), "cc\0-c\0main.c".to_string()),
            ("source.main".to_string(), "sha256-source".to_string()),
        ]))
        .unwrap();
        let encoded = replay.encode();
        assert_eq!(BuildPlanReplay::decode(&encoded).unwrap(), replay);
        assert_eq!(replay.encode(), encoded);
        assert!(BuildPlanReplay::decode("jet-build-plan-replay-v1\n61\t6").is_err());
        assert!(BuildPlanReplay::decode(
            "jet-build-plan-replay-v1\n61\t62\n61\t63\n"
        )
        .is_err());
    }
}
