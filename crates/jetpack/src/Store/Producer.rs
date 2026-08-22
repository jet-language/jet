use super::{CacheExpectation, CacheIdentity, Closure, Roots, StoreEntry};
use crate::Comptime::Build::BuildPlanReplay;
use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet};

const HEADER: &str = "jet-producer-record-v1";
/// Build the canonical producer record shared by every store ingestion phase.
pub(crate) fn canonical_producer(
    provider: &str,
    immutable_source: &str,
    source_digest: &str,
    identity: &CacheIdentity,
    mut facts: BTreeMap<String, String>,
) -> std::io::Result<String> {
    facts.insert("action.recipe".into(), identity.recipe_fingerprint.clone());
    facts.insert("closure.authority".into(), "hangar-cas".into());
    facts.insert("cache.reproducibility".into(), "attested-v1".into());
    let plan = crate::Comptime::Build::BuildPlanReplay::from_facts(facts.clone())
        .map_err(std::io::Error::other)?;
    ProducerRecord::new(
        provider,
        immutable_source,
        source_digest,
        plan,
        "jetpack-std-provider",
        format!(
            "policy={}\nplatform={}",
            identity.policy_fingerprint, identity.platform
        ),
        facts,
    )
    .map(|record| record.encode())
    .map_err(std::io::Error::other)
}

/// Refresh the immutable producer facts after the Nix provider publishes the
/// realization it just recorded in the project lock. The lock digest is part
/// of the Nix action key, so leaving the pre-publication digest in the closure
/// record would let a later replay accept stale provenance.
pub(crate) fn refresh_nix_lock_digest(
    roots: &Roots,
    entry: &StoreEntry,
    lock_digest: &str,
) -> std::io::Result<StoreEntry> {
    if lock_digest.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "cannot refresh a Nix producer with an empty project lock digest",
        ));
    }
    let mut producer =
        ProducerRecord::decode(&entry.producer_record).map_err(std::io::Error::other)?;
    if producer.provider != "nix" {
        return Ok(entry.clone());
    }
    producer
        .facts
        .insert("nix.lock.digest".to_string(), lock_digest.to_string());
    let mut replay_facts = producer.facts.clone();
    replay_facts.insert("nix.lock.digest".to_string(), lock_digest.to_string());
    replay_facts.remove("provider-facts");
    replay_facts.remove("provider-facts-digest");
    producer.plan = crate::Comptime::Build::BuildPlanReplay::from_facts(replay_facts)
        .map_err(std::io::Error::other)?;
    producer.bind_cache_provenance(
        &entry.reference,
        &entry.envelope.output_hash,
        &entry.cache_identity,
        &entry.references,
    );
    super::super::Provider::refresh_provider_facts(&mut producer, &entry.reference)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;

    let mut refreshed = entry.clone();
    refreshed.producer_record = producer.encode();
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        Closure::recover_closure_journal_unlocked(roots)?;
        // Producer refresh changes the canonical receipt inputs. Return the
        // digest that the registration commits, so the subsequent project
        // lock projection cannot retain the pre-refresh receipt.
        refreshed.receipt.clear();
        Closure::prepare_entry_receipt(roots, &mut refreshed)?;
        Closure::register_entry_unlocked(roots, &refreshed)?;
        Ok(refreshed)
    })
}

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
            (
                "immutable_source".to_string(),
                self.immutable_source.clone(),
            ),
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

    /// Bind the producer record to the exact cache admission facts. These
    /// values are part of the immutable record, so a cache hit can rederive
    /// the action, builder, policy, and output identity before using bytes.
    pub(crate) fn bind_cache_provenance(
        &mut self,
        reference: &str,
        output: &str,
        identity: &CacheIdentity,
        references: &[String],
    ) {
        // Providers keep the detailed recipe in the replay plan. The cache
        // admission fact is the stable recipe fingerprint shared by the
        // expectation, local proof, and signed narinfo.
        self.facts
            .insert("action.recipe".into(), identity.recipe_fingerprint.clone());
        self.facts
            .insert("cache.reference".into(), reference.into());
        self.facts
            .insert("cache.source".into(), self.immutable_source.clone());
        self.facts.insert(
            "cache.builder".into(),
            crate::TrustRoot::cache_builder_identity(
                &self.provider,
                &self.immutable_source,
                &self.source_digest,
            ),
        );
        self.facts.insert(
            "cache.action".into(),
            cache_action_identity(self, reference, identity, references),
        );
        self.facts.insert("cache.output".into(), output.into());
        self.facts
            .insert("cache.platform".into(), identity.platform.clone());
        self.facts
            .insert("cache.sandbox".into(), "sandbox:policy-bound".into());
        self.facts
            .insert("cache.policy".into(), identity.policy_fingerprint.clone());
        self.facts
            .entry("cache.reproducibility".into())
            .or_insert_with(|| "attested-v1".into());
    }

    pub fn decode(raw: &str) -> Result<Self, String> {
        let Some(raw) = raw.strip_suffix('\n') else {
            return Err("producer record is truncated".to_string());
        };
        let Some((body, checksum)) = raw.rsplit_once("checksum\t") else {
            return Err("producer record is truncated".to_string());
        };
        if checksum.len() != 64
            || !checksum
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            || SHA256::sha256_hex(body.as_bytes()) != checksum
        {
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

pub(crate) fn cache_action_identity(
    producer: &ProducerRecord,
    reference: &str,
    identity: &CacheIdentity,
    references: &[String],
) -> String {
    let mut canonical = b"jet-slsa-action-v2\0".to_vec();
    let plan = producer.plan.encode();
    for value in [
        reference,
        producer.provider.as_str(),
        producer.immutable_source.as_str(),
        producer.source_digest.as_str(),
        identity.recipe_fingerprint.as_str(),
        identity.policy_fingerprint.as_str(),
        identity.platform.as_str(),
        producer.toolchain_facts.as_str(),
        plan.as_str(),
    ] {
        canonical.extend_from_slice(&(value.len() as u64).to_be_bytes());
        canonical.extend_from_slice(value.as_bytes());
    }
    let closure = references
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    canonical.extend_from_slice(&(closure.len() as u64).to_be_bytes());
    // BTreeSet iteration is canonical. Keep the set separate from the frame
    // loop above so the signed action cannot silently change with dependency
    // ordering or duplicate input edges.
    for reference in closure {
        canonical.extend_from_slice(&(reference.len() as u64).to_be_bytes());
        canonical.extend_from_slice(reference.as_bytes());
    }
    format!("sha256:{}", SHA256::sha256_hex(&canonical))
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

fn hook_fact_mismatch(name: &str, expected: &str, actual: Option<&str>) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!(
            "adapter build hook provenance mismatch for `{name}`: expected `{expected}`, got `{}`",
            actual.unwrap_or("<missing>")
        ),
    )
}

fn validate_adapter_hook_producer(
    producer: &ProducerRecord,
    plan: &jet_env_model::ModuleEval::AdapterPlan,
    table: &jet_pkg_model::RefSpec::SourceTable,
    expectation: &CacheExpectation,
) -> std::io::Result<()> {
    let jet_env_model::ModuleEval::AdapterRecipe::Build(recipe) = &plan.recipe else {
        return Ok(());
    };
    if producer.provider != "adapter" {
        return Err(hook_fact_mismatch(
            "provider",
            "adapter",
            Some(&producer.provider),
        ));
    }
    if producer.source_digest != expectation.identity.source_fingerprint {
        return Err(hook_fact_mismatch(
            "source-digest",
            &expectation.identity.source_fingerprint,
            Some(&producer.source_digest),
        ));
    }
    if producer.facts.get("adapter.source").map(String::as_str) != Some(plan.source.as_str()) {
        return Err(hook_fact_mismatch(
            "provider-source",
            &plan.source,
            producer.facts.get("adapter.source").map(String::as_str),
        ));
    }
    let identity = crate::Provider::adapter_action_identity(
        plan,
        recipe,
        &expectation.identity.source_fingerprint,
        &expectation.identity.platform,
        table,
    );
    if producer.facts.get("build.identity").map(String::as_str) != Some(identity.as_str()) {
        return Err(hook_fact_mismatch(
            "build.identity",
            &identity,
            producer.facts.get("build.identity").map(String::as_str),
        ));
    }
    if producer
        .plan
        .facts()
        .get("adapter.build.identity")
        .map(String::as_str)
        != Some(identity.as_str())
    {
        return Err(hook_fact_mismatch(
            "adapter.build.identity",
            &identity,
            producer
                .plan
                .facts()
                .get("adapter.build.identity")
                .map(String::as_str),
        ));
    }
    let capabilities = recipe.declared_capabilities().join(",");
    if producer.facts.get("build.capabilities").map(String::as_str) != Some(capabilities.as_str()) {
        return Err(hook_fact_mismatch(
            "build.capabilities",
            &capabilities,
            producer.facts.get("build.capabilities").map(String::as_str),
        ));
    }
    let dependencies = crate::Provider::adapter_dependency_refs(plan).join(",");
    let authority = table.trust_lines().join("\n");
    if producer
        .plan
        .facts()
        .get("adapter.build.dependencies")
        .map(String::as_str)
        != Some(dependencies.as_str())
    {
        return Err(hook_fact_mismatch(
            "adapter.build.dependencies",
            &dependencies,
            producer
                .plan
                .facts()
                .get("adapter.build.dependencies")
                .map(String::as_str),
        ));
    }
    if producer
        .plan
        .facts()
        .get("adapter.build.authority")
        .map(String::as_str)
        != Some(authority.as_str())
    {
        return Err(hook_fact_mismatch(
            "adapter.build.authority",
            &authority,
            producer
                .plan
                .facts()
                .get("adapter.build.authority")
                .map(String::as_str),
        ));
    }
    if producer.facts.get("build.dependencies").map(String::as_str) != Some(dependencies.as_str()) {
        return Err(hook_fact_mismatch(
            "build.dependencies",
            &dependencies,
            producer.facts.get("build.dependencies").map(String::as_str),
        ));
    }
    Ok(())
}

pub(crate) fn validate_cached_adapter_hook(
    entry: &StoreEntry,
    plan: &jet_env_model::ModuleEval::AdapterPlan,
    table: &jet_pkg_model::RefSpec::SourceTable,
    expectation: &CacheExpectation,
) -> std::io::Result<()> {
    if !matches!(
        &plan.recipe,
        jet_env_model::ModuleEval::AdapterRecipe::Build(_)
    ) {
        return Ok(());
    }
    if entry.cache_identity != expectation.identity {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "adapter cache identity is not the exact build-hook identity",
        ));
    }
    let producer = ProducerRecord::decode(&entry.producer_record).map_err(std::io::Error::other)?;
    validate_adapter_hook_producer(&producer, plan, table, expectation)
}

pub(crate) fn bind_adapter_hook_identity(
    realized: &mut crate::Provider::Realized,
    plan: &jet_env_model::ModuleEval::AdapterPlan,
    table: &jet_pkg_model::RefSpec::SourceTable,
    expectation: &CacheExpectation,
    ctx: &crate::Provider::Ctx<'_>,
) -> std::io::Result<()> {
    if !matches!(
        &plan.recipe,
        jet_env_model::ModuleEval::AdapterRecipe::Build(_)
    ) {
        return Ok(());
    }
    validate_adapter_hook_producer(&realized.producer, plan, table, expectation)?;
    let expected = crate::Provider::adapter_cache_identity(
        &expectation.identity.source_fingerprint,
        realized
            .producer
            .facts
            .get("build.identity")
            .ok_or_else(|| hook_fact_mismatch("build.identity", "exact subject", None))?,
        ctx,
    );
    if expected != expectation.identity {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "adapter cache expectation is not derived from the exact build-hook subject",
        ));
    }
    for (name, expected, actual) in [
        (
            "source-fingerprint",
            &expectation.identity.source_fingerprint,
            &realized.cache_identity.source_fingerprint,
        ),
        (
            "policy-fingerprint",
            &expectation.identity.policy_fingerprint,
            &realized.cache_identity.policy_fingerprint,
        ),
        (
            "platform",
            &expectation.identity.platform,
            &realized.cache_identity.platform,
        ),
    ] {
        if expected != actual {
            return Err(hook_fact_mismatch(
                name,
                expected.as_str(),
                Some(actual.as_str()),
            ));
        }
    }
    realized.cache_identity = expected;
    Ok(())
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
        assert!(ProducerRecord::decode(encoded.trim_end()).is_err());
        assert!(ProducerRecord::decode(&format!("{encoded} ")).is_err());
        assert!(ProducerRecord::decode(&format!("{encoded}\n")).is_err());
        let duplicate = encoded.replacen("checksum\t", "70726f7669646572\t78\nchecksum\t", 1);
        assert!(ProducerRecord::decode(&duplicate).is_err());
    }

    #[test]
    fn cache_action_identity_binds_a_canonical_realized_closure() {
        let producer = record();
        let identity = CacheIdentity {
            source_fingerprint: "source".into(),
            recipe_fingerprint: "recipe".into(),
            policy_fingerprint: "policy".into(),
            platform: "x86_64-linux".into(),
        };
        let unordered = vec![
            "sha256:dependency-b".into(),
            "sha256:dependency-a".into(),
            "sha256:dependency-a".into(),
        ];
        let sorted = vec!["sha256:dependency-a".into(), "sha256:dependency-b".into()];
        let without_closure = cache_action_identity(&producer, "ref", &identity, &[]);
        let with_closure = cache_action_identity(&producer, "ref", &identity, &unordered);

        assert_ne!(without_closure, with_closure);
        assert_eq!(
            with_closure,
            cache_action_identity(&producer, "ref", &identity, &sorted)
        );
    }
}
