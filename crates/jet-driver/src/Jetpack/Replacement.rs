//! Compatibility-proved native replacement overlays (D-WD15).
//!
//! This is an internal data/proof layer. User-facing policy/proof source is
//! ratified separately as `policy.replacements` and `replacementProof:`.

use std::collections::{BTreeMap, BTreeSet};

use super::SemanticLock::{LockIdentity, LockRationale, LockRecordKind, SemanticRecord};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageIdentity {
    pub provider: String,
    pub name: String,
    pub version: String,
}

impl PackageIdentity {
    pub fn new(provider: &str, name: &str, version: &str) -> PackageIdentity {
        PackageIdentity {
            provider: provider.to_string(),
            name: name.to_string(),
            version: version.to_string(),
        }
    }

    pub fn ref_string(&self) -> String {
        if self.version.is_empty() {
            format!("{}:{}", self.provider, self.name)
        } else {
            format!("{}:{}@{}", self.provider, self.name, self.version)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofStatus {
    Missing,
    Failed,
    Passed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacementCandidate {
    pub foreign_identity: PackageIdentity,
    pub native_identity: PackageIdentity,
    pub covered_public_symbols: Vec<String>,
    pub unsupported_symbols: Vec<String>,
    pub license: String,
    pub platforms: Vec<String>,
    pub proof_status: ProofStatus,
    pub proof_digest: String,
}

impl ReplacementCandidate {
    pub fn visible_but_inactive(
        foreign_identity: PackageIdentity,
        native_identity: PackageIdentity,
    ) -> ReplacementCandidate {
        ReplacementCandidate {
            foreign_identity,
            native_identity,
            covered_public_symbols: Vec::new(),
            unsupported_symbols: Vec::new(),
            license: String::new(),
            platforms: Vec::new(),
            proof_status: ProofStatus::Missing,
            proof_digest: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicSymbol {
    pub name: String,
    pub signature: String,
    pub effects: Vec<String>,
    pub errors: Vec<String>,
}

impl PublicSymbol {
    pub fn new(name: &str, signature: &str) -> PublicSymbol {
        PublicSymbol {
            name: name.to_string(),
            signature: signature.to_string(),
            effects: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn with_effects(mut self, effects: &[&str]) -> PublicSymbol {
        self.effects = effects.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn with_errors(mut self, errors: &[&str]) -> PublicSymbol {
        self.errors = errors.iter().map(|s| s.to_string()).collect();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenFixture {
    pub name: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub files: Vec<(String, String)>,
    pub side_effects: Vec<String>,
}

impl GoldenFixture {
    pub fn new(name: &str, stdout: &str) -> GoldenFixture {
        GoldenFixture {
            name: name.to_string(),
            stdout: stdout.to_string(),
            stderr: String::new(),
            exit_code: 0,
            files: Vec::new(),
            side_effects: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilitySurface {
    pub identity: PackageIdentity,
    pub public_symbols: Vec<PublicSymbol>,
    pub examples: Vec<String>,
    pub goldens: Vec<GoldenFixture>,
    pub platforms: Vec<String>,
}

impl CompatibilitySurface {
    pub fn new(identity: PackageIdentity) -> CompatibilitySurface {
        CompatibilitySurface {
            identity,
            public_symbols: Vec::new(),
            examples: Vec::new(),
            goldens: Vec::new(),
            platforms: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofFailureKind {
    MissingPublicSymbol,
    SignatureMismatch,
    EffectMismatch,
    ErrorShapeMismatch,
    GoldenOutputDiff,
    PlatformMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofFailure {
    pub kind: ProofFailureKind,
    pub name: String,
    pub expected: String,
    pub found: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofReport {
    pub foreign_identity: PackageIdentity,
    pub native_identity: PackageIdentity,
    pub platform: String,
    pub inputs: Vec<String>,
    pub covered_public_symbols: Vec<String>,
    pub failures: Vec<ProofFailure>,
    pub digest: String,
}

impl ProofReport {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }

    pub fn status(&self) -> ProofStatus {
        if self.passed() {
            ProofStatus::Passed
        } else {
            ProofStatus::Failed
        }
    }

    pub fn candidate(&self, license: &str, platforms: Vec<String>) -> ReplacementCandidate {
        let unsupported = self
            .failures
            .iter()
            .filter(|f| f.kind == ProofFailureKind::MissingPublicSymbol)
            .map(|f| f.name.clone())
            .collect();
        ReplacementCandidate {
            foreign_identity: self.foreign_identity.clone(),
            native_identity: self.native_identity.clone(),
            covered_public_symbols: self.covered_public_symbols.clone(),
            unsupported_symbols: unsupported,
            license: license.to_string(),
            platforms,
            proof_status: self.status(),
            proof_digest: self.digest.clone(),
        }
    }
}

pub fn run_proof(
    foreign: &CompatibilitySurface,
    native: &CompatibilitySurface,
    platform: &str,
) -> ProofReport {
    let foreign_symbols = by_symbol(&foreign.public_symbols);
    let native_symbols = by_symbol(&native.public_symbols);
    let native_goldens = by_golden(&native.goldens);
    let mut failures = Vec::new();
    let mut covered_public_symbols = Vec::new();

    if !foreign.platforms.is_empty() && !foreign.platforms.iter().any(|p| p == platform) {
        failures.push(ProofFailure {
            kind: ProofFailureKind::PlatformMismatch,
            name: platform.to_string(),
            expected: foreign.platforms.join(","),
            found: "foreign surface unavailable".to_string(),
        });
    }
    if !native.platforms.is_empty() && !native.platforms.iter().any(|p| p == platform) {
        failures.push(ProofFailure {
            kind: ProofFailureKind::PlatformMismatch,
            name: platform.to_string(),
            expected: native.platforms.join(","),
            found: "native surface unavailable".to_string(),
        });
    }

    for (name, foreign_symbol) in &foreign_symbols {
        let Some(native_symbol) = native_symbols.get(name) else {
            failures.push(ProofFailure {
                kind: ProofFailureKind::MissingPublicSymbol,
                name: (*name).clone(),
                expected: foreign_symbol.signature.clone(),
                found: "missing".to_string(),
            });
            continue;
        };
        if foreign_symbol.signature != native_symbol.signature {
            failures.push(ProofFailure {
                kind: ProofFailureKind::SignatureMismatch,
                name: (*name).clone(),
                expected: foreign_symbol.signature.clone(),
                found: native_symbol.signature.clone(),
            });
            continue;
        }
        if sorted(&foreign_symbol.effects) != sorted(&native_symbol.effects) {
            failures.push(ProofFailure {
                kind: ProofFailureKind::EffectMismatch,
                name: (*name).clone(),
                expected: sorted(&foreign_symbol.effects).join(","),
                found: sorted(&native_symbol.effects).join(","),
            });
            continue;
        }
        if sorted(&foreign_symbol.errors) != sorted(&native_symbol.errors) {
            failures.push(ProofFailure {
                kind: ProofFailureKind::ErrorShapeMismatch,
                name: (*name).clone(),
                expected: sorted(&foreign_symbol.errors).join(","),
                found: sorted(&native_symbol.errors).join(","),
            });
            continue;
        }
        covered_public_symbols.push((*name).clone());
    }

    for foreign_golden in &foreign.goldens {
        match native_goldens.get(&foreign_golden.name) {
            Some(native_golden) if *native_golden == foreign_golden => {}
            Some(native_golden) => failures.push(ProofFailure {
                kind: ProofFailureKind::GoldenOutputDiff,
                name: foreign_golden.name.clone(),
                expected: fixture_fingerprint(foreign_golden),
                found: fixture_fingerprint(native_golden),
            }),
            None => failures.push(ProofFailure {
                kind: ProofFailureKind::GoldenOutputDiff,
                name: foreign_golden.name.clone(),
                expected: fixture_fingerprint(foreign_golden),
                found: "missing".to_string(),
            }),
        }
    }

    covered_public_symbols.sort();
    let inputs = proof_inputs(foreign, native, platform);
    let digest = digest(&inputs);
    ProofReport {
        foreign_identity: foreign.identity.clone(),
        native_identity: native.identity.clone(),
        platform: platform.to_string(),
        inputs,
        covered_public_symbols,
        failures,
        digest,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplacementPolicyMode {
    Allow,
    Deny,
    Prefer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacementPolicyRule {
    pub foreign_identity: String,
    pub native_identity: String,
    pub mode: ReplacementPolicyMode,
    pub owner_package: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacementPolicy {
    pub default: ReplacementPolicyMode,
    pub rules: Vec<ReplacementPolicyRule>,
}

impl Default for ReplacementPolicy {
    fn default() -> ReplacementPolicy {
        ReplacementPolicy {
            default: ReplacementPolicyMode::Deny,
            rules: Vec::new(),
        }
    }
}

impl ReplacementPolicy {
    pub fn allow(
        foreign: &PackageIdentity,
        native: &PackageIdentity,
        owner_package: &str,
    ) -> ReplacementPolicy {
        ReplacementPolicy {
            default: ReplacementPolicyMode::Deny,
            rules: vec![ReplacementPolicyRule {
                foreign_identity: foreign.ref_string(),
                native_identity: native.ref_string(),
                mode: ReplacementPolicyMode::Allow,
                owner_package: owner_package.to_string(),
                fingerprint: format!(
                    "policy.replacements:{}=>{}:allow",
                    foreign.ref_string(),
                    native.ref_string()
                ),
            }],
        }
    }

    pub fn prefer(
        foreign: &PackageIdentity,
        native: &PackageIdentity,
        owner_package: &str,
    ) -> ReplacementPolicy {
        ReplacementPolicy {
            default: ReplacementPolicyMode::Deny,
            rules: vec![ReplacementPolicyRule {
                foreign_identity: foreign.ref_string(),
                native_identity: native.ref_string(),
                mode: ReplacementPolicyMode::Prefer,
                owner_package: owner_package.to_string(),
                fingerprint: format!(
                    "policy.replacements:{}=>{}:prefer",
                    foreign.ref_string(),
                    native.ref_string()
                ),
            }],
        }
    }

    fn rule_for(&self, candidate: &ReplacementCandidate) -> Option<&ReplacementPolicyRule> {
        self.rules.iter().find(|rule| {
            rule.foreign_identity == candidate.foreign_identity.ref_string()
                && rule.native_identity == candidate.native_identity.ref_string()
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplacementDecision {
    Inactive { reason: String },
    Denied { reason: String },
    Active(ActiveReplacement),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveReplacement {
    pub foreign_call_site: String,
    pub native_identity: PackageIdentity,
    pub policy_mode: ReplacementPolicyMode,
    pub lock_record: SemanticRecord,
}

pub fn resolve_replacement(
    candidate: &ReplacementCandidate,
    policy: &ReplacementPolicy,
    owner_package: &str,
    platform: &str,
) -> ReplacementDecision {
    if candidate.proof_status != ProofStatus::Passed {
        return ReplacementDecision::Inactive {
            reason: "compatibility proof has not passed".to_string(),
        };
    }
    if !candidate.platforms.is_empty() && !candidate.platforms.iter().any(|p| p == platform) {
        return ReplacementDecision::Inactive {
            reason: format!("replacement proof does not cover platform `{platform}`"),
        };
    }
    let rule = policy.rule_for(candidate);
    let mode = rule
        .map(|rule| rule.mode.clone())
        .unwrap_or_else(|| policy.default.clone());
    match mode {
        ReplacementPolicyMode::Deny => ReplacementDecision::Denied {
            reason: "policy.replacements denies this native replacement".to_string(),
        },
        ReplacementPolicyMode::Allow | ReplacementPolicyMode::Prefer => {
            let policy_fingerprint = rule
                .map(|rule| rule.fingerprint.clone())
                .unwrap_or_else(|| "policy.replacements:default".to_string());
            let owner = rule
                .map(|rule| rule.owner_package.clone())
                .unwrap_or_else(|| owner_package.to_string());
            ReplacementDecision::Active(ActiveReplacement {
                foreign_call_site: candidate.foreign_identity.ref_string(),
                native_identity: candidate.native_identity.clone(),
                policy_mode: mode,
                lock_record: replacement_lock_record(
                    candidate,
                    &owner,
                    platform,
                    &policy_fingerprint,
                ),
            })
        }
    }
}

pub fn replacement_lock_record(
    candidate: &ReplacementCandidate,
    owner_package: &str,
    platform: &str,
    policy_fingerprint: &str,
) -> SemanticRecord {
    let mut future_fields = BTreeMap::new();
    future_fields.insert(
        "replacement-foreign".to_string(),
        candidate.foreign_identity.ref_string(),
    );
    future_fields.insert(
        "replacement-native".to_string(),
        candidate.native_identity.ref_string(),
    );
    future_fields.insert(
        "replacement-proof-digest".to_string(),
        candidate.proof_digest.clone(),
    );
    future_fields.insert(
        "replacement-covered-symbols".to_string(),
        candidate.covered_public_symbols.join(","),
    );
    let mut record = SemanticRecord::new(
        LockIdentity {
            kind: LockRecordKind::ReplacementOverlay,
            key: candidate.foreign_identity.ref_string(),
            exact: candidate.native_identity.ref_string(),
            hash: candidate.proof_digest.clone(),
            platform: platform.to_string(),
        },
        LockRationale {
            owner_package: owner_package.to_string(),
            reason: "native replacement overlay with passed compatibility proof".to_string(),
            source_ref: candidate.foreign_identity.ref_string(),
            provider: candidate.native_identity.provider.clone(),
            channel_input: String::new(),
            exact_output: candidate.native_identity.ref_string(),
            policy_fingerprint: policy_fingerprint.to_string(),
            recipe_id: String::new(),
            adapter_id: "replacement-overlay".to_string(),
            signature: candidate.proof_digest.clone(),
            cache_provenance: "compatibility proof inputs".to_string(),
            update_command: "policy.replacements".to_string(),
        },
    );
    record.future_fields = future_fields;
    record
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImporterReplacementStatus {
    NoCandidate,
    CandidateFound,
    ProofFailed,
    ProofPassed,
    ReplacementActive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImporterProgressFact {
    pub foreign_identity: PackageIdentity,
    pub native_identity: Option<PackageIdentity>,
    pub status: ImporterReplacementStatus,
    pub proof_digest: String,
    pub detail: String,
}

impl ImporterProgressFact {
    pub fn no_candidate(foreign_identity: PackageIdentity) -> ImporterProgressFact {
        ImporterProgressFact {
            foreign_identity,
            native_identity: None,
            status: ImporterReplacementStatus::NoCandidate,
            proof_digest: String::new(),
            detail: "no native replacement candidate found".to_string(),
        }
    }

    pub fn from_candidate(candidate: &ReplacementCandidate) -> ImporterProgressFact {
        let status = match candidate.proof_status {
            ProofStatus::Missing => ImporterReplacementStatus::CandidateFound,
            ProofStatus::Failed => ImporterReplacementStatus::ProofFailed,
            ProofStatus::Passed => ImporterReplacementStatus::ProofPassed,
        };
        ImporterProgressFact {
            foreign_identity: candidate.foreign_identity.clone(),
            native_identity: Some(candidate.native_identity.clone()),
            status,
            proof_digest: candidate.proof_digest.clone(),
            detail: candidate.covered_public_symbols.join(","),
        }
    }

    pub fn active(candidate: &ReplacementCandidate) -> ImporterProgressFact {
        ImporterProgressFact {
            foreign_identity: candidate.foreign_identity.clone(),
            native_identity: Some(candidate.native_identity.clone()),
            status: ImporterReplacementStatus::ReplacementActive,
            proof_digest: candidate.proof_digest.clone(),
            detail: "policy enabled replacement".to_string(),
        }
    }
}

fn by_symbol(symbols: &[PublicSymbol]) -> BTreeMap<String, &PublicSymbol> {
    symbols
        .iter()
        .map(|symbol| (symbol.name.clone(), symbol))
        .collect()
}

fn by_golden(goldens: &[GoldenFixture]) -> BTreeMap<String, &GoldenFixture> {
    goldens
        .iter()
        .map(|golden| (golden.name.clone(), golden))
        .collect()
}

fn sorted(values: &[String]) -> Vec<String> {
    let set: BTreeSet<String> = values.iter().cloned().collect();
    set.into_iter().collect()
}

fn proof_inputs(
    foreign: &CompatibilitySurface,
    native: &CompatibilitySurface,
    platform: &str,
) -> Vec<String> {
    let mut inputs = vec![
        format!("foreign={}", foreign.identity.ref_string()),
        format!("native={}", native.identity.ref_string()),
        format!("platform={platform}"),
    ];
    for symbol in foreign
        .public_symbols
        .iter()
        .chain(native.public_symbols.iter())
    {
        inputs.push(format!(
            "symbol={}:{}:{}:{}",
            symbol.name,
            symbol.signature,
            sorted(&symbol.effects).join(","),
            sorted(&symbol.errors).join(",")
        ));
    }
    for example in foreign.examples.iter().chain(native.examples.iter()) {
        inputs.push(format!("example={example}"));
    }
    for golden in foreign.goldens.iter().chain(native.goldens.iter()) {
        inputs.push(format!(
            "golden={}:{}",
            golden.name,
            fixture_fingerprint(golden)
        ));
    }
    inputs.sort();
    inputs
}

fn fixture_fingerprint(fixture: &GoldenFixture) -> String {
    let mut parts = vec![
        fixture.stdout.clone(),
        fixture.stderr.clone(),
        fixture.exit_code.to_string(),
    ];
    for (path, content) in &fixture.files {
        parts.push(format!("{path}={content}"));
    }
    for effect in &fixture.side_effects {
        parts.push(format!("effect={effect}"));
    }
    digest(&parts)
}

fn digest(parts: &[String]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for part in parts {
        for b in part.as_bytes() {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= b'\n' as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("proof-fnv64-{hash:016x}")
}
