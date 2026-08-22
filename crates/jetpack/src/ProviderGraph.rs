//! Federated provider facts under Jetpack authority (D-WD6).
//!
//! External provider prefixes and trust-root config remain owner-gated. This
//! module models provider metadata/fetch/lock/sandbox/signature/audit facts.

pub use super::Replacement::ReplacementCandidate as ReplacementOverlay;
use super::JSON::{self, JSONValue};
use jet_pkg_model::ProviderFacts::{ProviderFactValue, ProviderFacts, ProviderSelector};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderFamily {
    Core,
    Nix,
    Path,
    Github,
    JetRegistry,
    Npm,
    PyPI,
    Cargo,
    SwiftPM,
    Maven,
    NuGet,
    Conan,
    Vcpkg,
    Homebrew,
    Binary,
}

impl ProviderFamily {
    pub fn label(&self) -> &'static str {
        match self {
            ProviderFamily::Core => "core",
            ProviderFamily::Nix => "nix",
            ProviderFamily::Path => "path",
            ProviderFamily::Github => "github",
            ProviderFamily::JetRegistry => "jet-registry",
            ProviderFamily::Npm => "npm",
            ProviderFamily::PyPI => "pypi",
            ProviderFamily::Cargo => "cargo",
            ProviderFamily::SwiftPM => "swiftpm",
            ProviderFamily::Maven => "maven",
            ProviderFamily::NuGet => "nuget",
            ProviderFamily::Conan => "conan",
            ProviderFamily::Vcpkg => "vcpkg",
            ProviderFamily::Homebrew => "homebrew",
            ProviderFamily::Binary => "binary",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderContract {
    pub family: ProviderFamily,
    pub parses_refs: bool,
    pub probes_metadata: bool,
    pub resolves_channels: bool,
    pub fetches_bytes: bool,
    pub verifies_hash_signature: bool,
    pub exposes_audit_facts: bool,
    pub reports_offline_satisfiability: bool,
}

impl ProviderContract {
    /// Describe the surface that this build actually owns. Metadata
    /// normalization does not imply that Jet can fetch or lock bytes from a
    /// foreign ecosystem, so those capabilities stay false until a real
    /// provider adapter supplies them.
    pub fn for_family(family: ProviderFamily) -> ProviderContract {
        let local_or_first_party = matches!(
            family,
            ProviderFamily::Core
                | ProviderFamily::Nix
                | ProviderFamily::Path
                | ProviderFamily::JetRegistry
        );
        let binary = matches!(family, ProviderFamily::Binary);
        ProviderContract {
            family,
            parses_refs: true,
            probes_metadata: true,
            resolves_channels: local_or_first_party,
            fetches_bytes: local_or_first_party || binary,
            verifies_hash_signature: local_or_first_party || binary,
            exposes_audit_facts: true,
            reports_offline_satisfiability: local_or_first_party || binary,
        }
    }

    /// Kept as a descriptive constructor for callers that already have a
    /// family. It now returns the honest family contract rather than claiming
    /// unimplemented foreign network support.
    pub fn full(family: ProviderFamily) -> ProviderContract {
        Self::for_family(family)
    }
}

pub fn built_in_contracts() -> Vec<ProviderContract> {
    vec![
        ProviderContract::full(ProviderFamily::Core),
        ProviderContract::full(ProviderFamily::Nix),
        ProviderContract::full(ProviderFamily::Path),
        ProviderContract::full(ProviderFamily::Github),
        ProviderContract::full(ProviderFamily::JetRegistry),
        ProviderContract::full(ProviderFamily::Npm),
        ProviderContract::full(ProviderFamily::PyPI),
        ProviderContract::full(ProviderFamily::Cargo),
        ProviderContract::full(ProviderFamily::SwiftPM),
        ProviderContract::full(ProviderFamily::Maven),
        ProviderContract::full(ProviderFamily::NuGet),
        ProviderContract::full(ProviderFamily::Conan),
        ProviderContract::full(ProviderFamily::Vcpkg),
        ProviderContract::full(ProviderFamily::Homebrew),
        ProviderContract::full(ProviderFamily::Binary),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderFactReport {
    pub facts: MetadataFacts,
    pub losses: Vec<String>,
    pub conflicts: Vec<String>,
    pub native_format: String,
    pub native_document: String,
}

impl ProviderFactReport {
    pub fn is_lossless(&self) -> bool {
        self.losses.is_empty() && self.conflicts.is_empty() && self.shared_facts().is_lossless()
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.conflicts.is_empty() {
            return Err(format!(
                "provider facts conflict: {}",
                self.conflicts.join("; ")
            ));
        }
        if !self.losses.is_empty() {
            return Err(format!(
                "provider facts are lossy: {}",
                self.losses.join("; ")
            ));
        }
        if self.facts.name.is_empty() {
            return Err("provider facts need a package name".to_string());
        }
        if metadata_identity_selector(&self.facts).1.is_empty() {
            return Err("provider facts need an exact version, revision, or digest".to_string());
        }
        if self.facts.source_identity.is_empty() {
            return Err("provider facts need a resolved source identity".to_string());
        }
        if self.native_format.is_empty() || self.native_document.is_empty() {
            return Err("provider facts need the native document and its format".to_string());
        }
        self.shared_facts().validate()?;
        Ok(())
    }

    /// Lower the provider report into the one carrier used by plan, lock, and
    /// explain. The native document remains byte-for-byte available on that
    /// carrier; the typed projection is additive and never a replacement.
    pub fn shared_facts_for(&self, reference: &str) -> ProviderFacts {
        let mut shared = ProviderFacts::for_reference(self.facts.family.label(), reference);
        shared.set_resolved_source(&self.facts.source_identity);
        shared.set_native_document(&self.native_format, &self.native_document);
        add_metadata_facts(&mut shared, &self.facts);
        let (selector_key, expected_identity) = metadata_identity_selector(&self.facts);
        if !shared.selector.is_exact() && !expected_identity.is_empty() {
            shared.set_resolved_selector(
                &format!("#{selector_key}={expected_identity}"),
                "provider.metadata",
            );
        }
        let selector = shared.effective_selector();
        let selector_identity = if !selector.version.is_empty() {
            Some(selector.version)
        } else if !selector.revision.is_empty() {
            Some(selector.revision)
        } else if !selector.digest.is_empty() {
            Some(selector.digest)
        } else {
            None
        };
        if selector_identity.as_deref() != Some(expected_identity.as_str()) {
            shared.add_conflict(
                "provider.selector.identity",
                &expected_identity,
                selector_identity.as_deref().unwrap_or("<missing>"),
                "provider.metadata",
            );
        }
        for (index, loss) in self.losses.iter().enumerate() {
            shared.add_loss(
                &format!("provider.loss.{index}"),
                loss,
                "provider.native_document",
            );
        }
        for (index, conflict) in self.conflicts.iter().enumerate() {
            shared.add_conflict(
                &format!("provider.conflict.{index}"),
                "<provider-native-left-unavailable>",
                conflict,
                "provider.native_document",
            );
        }
        shared
    }

    pub fn shared_facts(&self) -> ProviderFacts {
        let (selector_key, selector_value) = metadata_identity_selector(&self.facts);
        let reference = if selector_value.is_empty() {
            format!("{}@{}", self.facts.name, self.facts.family.label())
        } else {
            format!(
                "{}#{}={}@{}",
                self.facts.name,
                selector_key,
                selector_value,
                self.facts.family.label()
            )
        };
        self.shared_facts_for(&reference)
    }

    pub fn export_json(&self) -> String {
        self.shared_facts().to_json()
    }

    /// Make a semantic-lock record without inventing a second provider fact
    /// schema. A lossy report is refused before it can enter the lock.
    pub fn lock_record(
        &self,
        owner_package: &str,
        reference: &str,
        platform: &str,
    ) -> Result<crate::SemanticLock::SemanticRecord, String> {
        let requested = self.shared_facts_for(reference);
        requested.validate()?;
        let shared = self.shared_facts();
        shared.validate()?;
        let preserve_source_authority = requested.facts.contains_key("provider.authority");
        if !preserve_source_authority
            && requested.qualified_reference() != shared.qualified_reference()
        {
            return Err(format!(
                "provider lock reference `{reference}` disagrees with metadata identity `{}`",
                shared.qualified_reference()
            ));
        }
        let locked = if preserve_source_authority {
            &requested
        } else {
            &shared
        };
        let qualified_reference = locked.qualified_reference();
        let mut record = crate::SemanticLock::SemanticRecord::new(
            crate::SemanticLock::LockIdentity {
                kind: crate::SemanticLock::LockRecordKind::Package,
                key: format!("provider:{qualified_reference}"),
                exact: qualified_reference.clone(),
                hash: locked.digest(),
                platform: platform.to_string(),
            },
            crate::SemanticLock::LockRationale {
                owner_package: owner_package.to_string(),
                reason: "provider-native facts lowered through the shared carrier".to_string(),
                source_ref: reference.to_string(),
                provider: self.facts.family.label().to_string(),
                exact_output: self.facts.source_identity.clone(),
                ..Default::default()
            },
        );
        record
            .future_fields
            .insert("provider-facts".to_string(), locked.to_json());
        record
            .future_fields
            .insert("provider-facts-digest".to_string(), locked.digest());
        Ok(record)
    }
}

fn add_metadata_facts(shared: &mut ProviderFacts, facts: &MetadataFacts) {
    for (key, value) in [
        ("package.name", facts.name.as_str()),
        ("package.version", facts.version.as_str()),
        ("package.source", facts.source_identity.as_str()),
        ("package.integrity", facts.integrity_hash.as_str()),
        ("package.license", facts.license.as_str()),
    ] {
        if !value.is_empty() {
            shared.add_fact(
                key,
                ProviderFactValue::Text(value.to_string()),
                "provider.metadata",
            );
        }
    }
    for (key, values) in [
        ("package.dependencies", &facts.dependencies),
        ("package.dev_dependencies", &facts.dev_dependencies),
        ("package.build_dependencies", &facts.build_dependencies),
        ("package.scripts", &facts.scripts),
        ("package.platforms", &facts.platforms),
        ("package.bins", &facts.bins),
        ("package.trust_roots", &facts.trust_roots),
        ("package.todos", &facts.todos),
    ] {
        if !values.is_empty() {
            shared.add_fact(
                key,
                ProviderFactValue::List(
                    values
                        .iter()
                        .map(|value| ProviderFactValue::Text(value.clone()))
                        .collect(),
                ),
                "provider.metadata",
            );
        }
    }
    for (key, values) in &facts.typed {
        shared.add_fact(
            key,
            ProviderFactValue::List(
                values
                    .iter()
                    .map(|value| typed_projection_value(value))
                    .collect(),
            ),
            "provider.native_projection",
        );
    }
    if !facts.replacement_candidates.is_empty() {
        let candidates = facts
            .replacement_candidates
            .iter()
            .map(|candidate| {
                let mut fields = std::collections::BTreeMap::new();
                fields.insert(
                    "foreign_identity".to_string(),
                    ProviderFactValue::Text(candidate.foreign_identity.ref_string()),
                );
                fields.insert(
                    "native_identity".to_string(),
                    ProviderFactValue::Text(candidate.native_identity.ref_string()),
                );
                fields.insert(
                    "license".to_string(),
                    ProviderFactValue::Text(candidate.license.clone()),
                );
                fields.insert(
                    "proof_status".to_string(),
                    ProviderFactValue::Text(format!("{:?}", candidate.proof_status)),
                );
                fields.insert(
                    "proof_digest".to_string(),
                    ProviderFactValue::Text(candidate.proof_digest.clone()),
                );
                for (key, values) in [
                    ("covered_public_symbols", &candidate.covered_public_symbols),
                    ("unsupported_symbols", &candidate.unsupported_symbols),
                    ("platforms", &candidate.platforms),
                    ("proof_inputs", &candidate.proof_inputs),
                ] {
                    fields.insert(
                        key.to_string(),
                        ProviderFactValue::List(
                            values
                                .iter()
                                .map(|value| ProviderFactValue::Text(value.clone()))
                                .collect(),
                        ),
                    );
                }
                ProviderFactValue::Map(fields)
            })
            .collect();
        shared.add_fact(
            "package.replacement_candidates",
            ProviderFactValue::List(candidates),
            "provider.replacement",
        );
    }
}

fn typed_projection_value(value: &str) -> ProviderFactValue {
    JSON::parse(value)
        .ok()
        .and_then(|value| ProviderFactValue::from_json_value(&value).ok())
        .unwrap_or_else(|| ProviderFactValue::Text(value.to_string()))
}

fn add_typed_json_fact(facts: &mut MetadataFacts, key: impl Into<String>, value: &JSONValue) {
    facts.typed.insert(key.into(), vec![json_value_json(value)]);
}

/// Keep every native top-level field in the typed projection as well as in the
/// byte-for-byte native document. Provider-specific consumers can use the
/// namespaced projection without forcing the shared model to grow a field for
/// every ecosystem release.
fn add_json_projection(
    facts: &mut MetadataFacts,
    namespace: &str,
    object: &std::collections::BTreeMap<String, JSONValue>,
) {
    for (key, value) in object {
        add_typed_json_fact(facts, format!("{namespace}.{key}"), value);
    }
}

fn add_typed_text_fact(facts: &mut MetadataFacts, key: impl Into<String>, value: &str) {
    facts.typed.insert(key.into(), vec![value.to_string()]);
}

fn json_string_list(value: &JSONValue) -> Option<Vec<String>> {
    match value {
        JSONValue::String(value) => Some(vec![value.clone()]),
        JSONValue::Array(values) => values
            .iter()
            .map(|value| match value {
                JSONValue::String(value) => Some(value.clone()),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn json_bool(value: Option<&JSONValue>) -> Option<bool> {
    match value {
        Some(JSONValue::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn metadata_identity_selector(facts: &MetadataFacts) -> (&'static str, String) {
    if facts.family == ProviderFamily::Binary {
        return ("digest", facts.integrity_hash.clone());
    }
    if facts.family == ProviderFamily::SwiftPM {
        if let Some(revision) = facts
            .typed
            .get("provider.revision")
            .and_then(|values| values.first())
            .filter(|revision| !revision.is_empty())
        {
            return ("revision", revision.clone());
        }
    }
    if facts.family == ProviderFamily::Github && facts.version.is_empty() {
        if let Some(revision) = facts
            .typed
            .get("provider.github.revision")
            .and_then(|values| values.first())
            .filter(|revision| ProviderSelector::parse(&format!("#revision={revision}")).is_exact())
        {
            return ("revision", revision.clone());
        }
    }
    if !facts.version.is_empty() {
        return ("version", facts.version.clone());
    }
    if !facts.integrity_hash.is_empty() {
        return ("digest", facts.integrity_hash.clone());
    }
    ("version", String::new())
}

fn exact_provider_version(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.contains("$(") || value.contains("${") {
        return false;
    }
    // NuGet's `[1.2.3]` is an exact version range. Every other bracketed
    // range remains a request, not a package identity.
    if let Some(exact) = value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    {
        return !exact
            .chars()
            .any(|character| matches!(character, '[' | ']' | '(' | ')' | ','))
            && exact_provider_version(exact);
    }
    if value
        .chars()
        .any(|character| matches!(character, '[' | ']' | '(' | ')' | ','))
    {
        return false;
    }
    ProviderSelector::parse(&format!("#version={value}")).is_exact()
}

/// Normalize one provider-native metadata document into the shared fact model.
/// The report is intentionally separate from `MetadataFacts`: unsupported or
/// ambiguous fields stay visible instead of becoming silent defaults.
pub fn normalize_provider_document(family: ProviderFamily, document: &str) -> ProviderFactReport {
    // Compute the format first: the match below consumes `family` (the
    // Core/Nix/Path arm moves it into `MetadataFacts::empty`).
    let native_format = provider_document_format(&family, document);
    let mut report = match family {
        ProviderFamily::Npm => npm_report(document),
        ProviderFamily::Cargo => cargo_report(document),
        ProviderFamily::PyPI => pypi_report(document),
        ProviderFamily::SwiftPM => swiftpm_report(document),
        ProviderFamily::Maven => maven_report(document),
        ProviderFamily::NuGet => nuget_report(document),
        ProviderFamily::Conan => conan_report(document),
        ProviderFamily::Vcpkg => vcpkg_report(document),
        ProviderFamily::Homebrew => homebrew_report(document),
        ProviderFamily::JetRegistry => jet_registry_report(document),
        ProviderFamily::Github => github_report(document),
        ProviderFamily::Binary => binary_report(document),
        ProviderFamily::Nix => nix_report(document),
        ProviderFamily::Core | ProviderFamily::Path => {
            let mut facts = MetadataFacts::empty(family, "");
            facts.source_identity = format!("{}:document", facts.family.label());
            ProviderFactReport {
                facts,
                losses: vec!["this provider has no foreign metadata document shape".to_string()],
                conflicts: Vec::new(),
                native_format: String::new(),
                native_document: String::new(),
            }
        }
    };
    report.native_format = native_format;
    report.native_document = document.to_string();
    report
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataFacts {
    pub family: ProviderFamily,
    pub name: String,
    pub version: String,
    pub source_identity: String,
    pub integrity_hash: String,
    pub dependencies: Vec<String>,
    pub dev_dependencies: Vec<String>,
    pub build_dependencies: Vec<String>,
    pub scripts: Vec<String>,
    pub platforms: Vec<String>,
    pub license: String,
    pub bins: Vec<String>,
    pub trust_roots: Vec<String>,
    pub todos: Vec<String>,
    pub typed: std::collections::BTreeMap<String, Vec<String>>,
    pub replacement_candidates: Vec<ReplacementOverlay>,
}

impl MetadataFacts {
    pub fn empty(family: ProviderFamily, name: impl Into<String>) -> MetadataFacts {
        MetadataFacts {
            family,
            name: name.into(),
            version: String::new(),
            source_identity: String::new(),
            integrity_hash: String::new(),
            dependencies: Vec::new(),
            dev_dependencies: Vec::new(),
            build_dependencies: Vec::new(),
            scripts: Vec::new(),
            platforms: Vec::new(),
            license: String::new(),
            bins: Vec::new(),
            trust_roots: Vec::new(),
            todos: Vec::new(),
            typed: std::collections::BTreeMap::new(),
            replacement_candidates: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderObject {
    pub family: ProviderFamily,
    pub ref_key: String,
    pub exact_identity: String,
    pub hash: String,
    pub platform: String,
    pub signature: String,
    pub audit: Vec<String>,
    pub sandbox_effects: Vec<String>,
    pub build_effects: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorityGraph {
    pub locked_objects: Vec<ProviderObject>,
}

impl AuthorityGraph {
    pub fn add_locked(&mut self, object: ProviderObject) {
        self.locked_objects.push(object);
    }

    pub fn fetch_allowed(&self, request: &ProviderRequest) -> FetchDecision {
        if request.offline {
            if self.locked_objects.iter().any(|obj| {
                obj.family == request.family
                    && obj.ref_key == request.ref_key
                    && obj.exact_identity == request.exact_identity
                    && obj.hash == request.hash
                    && obj.platform == request.platform
            }) {
                FetchDecision::AllowedOfflineSatisfied
            } else {
                FetchDecision::DeniedOfflineMissingLock
            }
        } else {
            FetchDecision::AllowedNetworkWithGrant {
                authority: format!("provider.fetch.{}", request.family.label()),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRequest {
    pub family: ProviderFamily,
    pub ref_key: String,
    pub exact_identity: String,
    pub hash: String,
    pub platform: String,
    pub offline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchDecision {
    AllowedOfflineSatisfied,
    AllowedNetworkWithGrant { authority: String },
    DeniedOfflineMissingLock,
}

pub fn normalize_npm(package_json: &str) -> MetadataFacts {
    let parsed = JSON::parse(package_json).ok();
    let obj = parsed.as_ref().and_then(|j| j.as_object().ok());
    let name = obj
        .and_then(|m| m.get("name"))
        .and_then(|v| v.as_str().ok())
        .unwrap_or_default()
        .to_string();
    let mut facts = MetadataFacts::empty(ProviderFamily::Npm, name);
    facts.version = obj
        .and_then(|m| m.get("version"))
        .and_then(|v| v.as_str().ok())
        .unwrap_or("")
        .to_string();
    facts.license = obj
        .and_then(|m| m.get("license"))
        .and_then(|v| v.as_str().ok())
        .unwrap_or("")
        .to_string();
    if let Some(JSONValue::Object(deps)) = obj.and_then(|m| m.get("dependencies")) {
        facts.dependencies = deps.keys().cloned().collect();
    }
    if let Some(JSONValue::Object(scripts)) = obj.and_then(|m| m.get("scripts")) {
        facts.scripts = scripts.keys().cloned().collect();
    }
    if let Some(JSONValue::Object(bin)) = obj.and_then(|m| m.get("bin")) {
        facts.bins = bin.keys().cloned().collect();
    }
    facts.source_identity = format!("npm:{}@{}", facts.name, facts.version);
    facts
}

pub fn normalize_cargo(cargo_toml: &str) -> MetadataFacts {
    let name = toml_string(cargo_toml, "name").unwrap_or_default();
    let mut facts = MetadataFacts::empty(ProviderFamily::Cargo, name);
    facts.version = toml_string(cargo_toml, "version").unwrap_or_default();
    facts.license = toml_string(cargo_toml, "license").unwrap_or_default();
    facts.dependencies = dependency_keys(cargo_toml, "[dependencies]");
    facts.build_dependencies = dependency_keys(cargo_toml, "[build-dependencies]");
    if cargo_toml.contains("build =") {
        facts.scripts.push("build.rs".to_string());
    }
    facts.source_identity = format!("cargo:{}@{}", facts.name, facts.version);
    facts
}

pub fn normalize_pypi(name: &str, version: &str, dynamic_metadata: bool) -> MetadataFacts {
    let mut facts = MetadataFacts::empty(ProviderFamily::PyPI, name);
    facts.version = version.to_string();
    facts.source_identity = format!("pypi:{name}@{version}");
    if dynamic_metadata {
        facts
            .todos
            .push("dynamic metadata must be resolved before build execution".to_string());
    }
    facts
}

pub fn normalize_swiftpm(name: &str, revision: &str) -> MetadataFacts {
    let mut facts = MetadataFacts::empty(ProviderFamily::SwiftPM, name);
    facts.version = revision.to_string();
    facts.source_identity = format!("swiftpm:{name}@{revision}");
    facts.integrity_hash = revision.to_string();
    facts
        .typed
        .insert("provider.revision".to_string(), vec![revision.to_string()]);
    facts
}

pub fn binary_object(
    name: &str,
    hash: &str,
    platform: &str,
    signature: &str,
) -> Result<ProviderObject, String> {
    if hash.trim().is_empty() {
        return Err("binary provider requires a hash".to_string());
    }
    if platform.trim().is_empty() {
        return Err("binary provider requires a platform".to_string());
    }
    Ok(ProviderObject {
        family: ProviderFamily::Binary,
        ref_key: name.to_string(),
        exact_identity: format!("binary:{name}:{platform}:{hash}"),
        hash: hash.to_string(),
        platform: platform.to_string(),
        signature: signature.to_string(),
        audit: vec!["hash verified before realization".to_string()],
        sandbox_effects: vec!["no build execution".to_string()],
        build_effects: Vec::new(),
    })
}

fn npm_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(ProviderFamily::Npm, "npm", "package.json is not valid JSON");
    };
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Npm,
        json_string(&object, "name").unwrap_or_default(),
    );
    let mut metadata = object.clone();
    let mut losses = Vec::new();
    let mut conflicts = Vec::new();
    if facts.name.is_empty() {
        losses.push("npm metadata has no package name".to_string());
    }
    facts.version = json_string(&object, "version").unwrap_or_default();
    if object.contains_key("version") && facts.version.is_empty() {
        losses.push("npm metadata `version` must be a non-empty string".to_string());
    }
    if facts.version.is_empty() {
        match object.get("versions") {
            Some(JSONValue::Object(versions)) if versions.len() == 1 => {
                if let Some((version, JSONValue::Object(package))) = versions.iter().next() {
                    facts.version = version.clone();
                    metadata = package.clone();
                    add_json_projection(
                        &mut facts,
                        &format!("provider.npm.version.{version}"),
                        package,
                    );
                }
            }
            Some(JSONValue::Object(versions)) if versions.len() > 1 => losses.push(
                "npm packument contains multiple versions; select one exact version before realization"
                    .to_string(),
            ),
            Some(_) => losses.push("npm `versions` must be an object".to_string()),
            None => {}
        }
    }
    facts.license = json_string(&metadata, "license").unwrap_or_default();
    facts.dependencies = json_keys(&metadata, "dependencies");
    facts.dev_dependencies = json_keys(&metadata, "devDependencies");
    facts.scripts = json_keys(&metadata, "scripts");
    facts.bins = match metadata.get("bin") {
        Some(JSONValue::Object(values)) => {
            if values.values().any(|value| !matches!(value, JSONValue::String(_))) {
                losses.push("npm `bin` object values must be strings".to_string());
            }
            values.keys().cloned().collect()
        }
        Some(JSONValue::String(_)) => vec![facts.name.clone()],
        Some(_) => {
            losses.push("npm `bin` must be a string or object".to_string());
            Vec::new()
        }
        None => Vec::new(),
    };
    if facts.name.is_empty() {
        facts.name = json_string(&metadata, "name").unwrap_or_default();
    }
    if metadata != object {
        if let (Some(top_name), Some(package_name)) = (
            json_string(&object, "name"),
            json_string(&metadata, "name"),
        ) {
            if top_name != package_name {
                conflicts.push(format!(
                    "npm packument name `{top_name}` conflicts with package name `{package_name}`"
                ));
            }
        }
        match metadata.get("version") {
            Some(JSONValue::String(version)) if version != &facts.version => conflicts.push(
                format!(
                    "npm packument version `{}` conflicts with package version `{version}`",
                    facts.version
                ),
            ),
            Some(_) => losses.push("npm package `version` must be a string".to_string()),
            None => losses.push("npm packument package has no version".to_string()),
        }
    }
    facts.source_identity = format!("npm:{}@{}", facts.name, facts.version);
    add_json_projection(&mut facts, "provider.npm.native", &object);
    if metadata != object {
        add_json_projection(&mut facts, "provider.npm.package", &metadata);
    }

    let dependency_sets = [
        ("dependencies", "runtime"),
        ("devDependencies", "dev"),
        ("optionalDependencies", "optional"),
        ("peerDependencies", "peer"),
        ("bundledDependencies", "bundled"),
    ];
    for (field, kind) in dependency_sets {
        let Some(value) = metadata.get(field) else {
            continue;
        };
        match value {
            JSONValue::Object(values) if field != "bundledDependencies" => {
                for (name, requirement) in values {
                    add_typed_json_fact(
                        &mut facts,
                        format!("provider.npm.dependency.{kind}.{name}"),
                        requirement,
                    );
                    if !matches!(requirement, JSONValue::String(_)) {
                        losses.push(format!(
                            "npm `{field}.{name}` must retain a string requirement"
                        ));
                    }
                }
            }
            JSONValue::Array(values) if field == "bundledDependencies" => {
                for (index, value) in values.iter().enumerate() {
                    add_typed_json_fact(
                        &mut facts,
                        format!("provider.npm.dependency.bundled.{index}"),
                        value,
                    );
                    if !matches!(value, JSONValue::String(_)) {
                        losses.push(format!(
                            "npm `bundledDependencies[{index}]` must be a package name"
                        ));
                    }
                }
            }
            _ => losses.push(format!("npm `{field}` must be an object")),
        }
    }
    if let (Some(JSONValue::Object(dependencies)), Some(JSONValue::Object(optional))) = (
        metadata.get("dependencies"),
        metadata.get("optionalDependencies"),
    ) {
        for (name, dependency) in dependencies {
            if let Some(optional_requirement) = optional.get(name) {
                if dependency != optional_requirement {
                    losses.push(format!(
                        "npm dependency `{name}` has different runtime and optional requirements"
                    ));
                }
            }
        }
    }
    if let Some(JSONValue::Object(scripts)) = metadata.get("scripts") {
        for (name, command) in scripts {
            add_typed_json_fact(&mut facts, format!("provider.npm.hook.{name}"), command);
            if !matches!(command, JSONValue::String(_)) {
                losses.push(format!("npm script `{name}` must be a string command"));
            }
        }
    } else if metadata.contains_key("scripts") {
        losses.push("npm `scripts` must be an object".to_string());
    }
    for key in ["os", "cpu"] {
        if let Some(value) = metadata.get(key) {
            match json_string_list(value) {
                Some(values) => {
                    facts
                        .platforms
                        .extend(values.iter().map(|value| format!("{key}:{value}")));
                    add_typed_json_fact(&mut facts, format!("provider.npm.platform.{key}"), value);
                }
                None => losses.push(format!("npm `{key}` must be a string or string array")),
            }
        }
    }
    if let Some(JSONValue::Object(engines)) = metadata.get("engines") {
        for (engine, requirement) in engines {
            add_typed_json_fact(
                &mut facts,
                format!("provider.npm.variant.engine.{engine}"),
                requirement,
            );
            if !matches!(requirement, JSONValue::String(_)) {
                losses.push(format!("npm engine `{engine}` must be a string requirement"));
            }
        }
    } else if metadata.contains_key("engines") {
        losses.push("npm `engines` must be an object".to_string());
    }
    for (field, target) in [
        ("deprecated", "provider.npm.advisory.deprecated"),
        ("repository", "provider.npm.source.repository"),
        ("publishConfig", "provider.npm.source.publish_config"),
        ("author", "provider.npm.source.author"),
        ("maintainers", "provider.npm.source.maintainers"),
        ("dist-tags", "provider.npm.channel.dist_tags"),
    ] {
        if let Some(value) = metadata.get(field) {
            add_typed_json_fact(&mut facts, target, value);
        }
    }
    if let Some(value) = metadata.get("yanked") {
        if json_bool(Some(value)).is_some() {
            add_typed_json_fact(&mut facts, "provider.npm.yanked", value);
        } else {
            losses.push("npm `yanked` must be a boolean".to_string());
        }
    }
    if let Some(JSONValue::Object(dist)) = metadata.get("dist") {
        for key in ["integrity", "shasum", "tarball"] {
            if let Some(value) = dist.get(key) {
                add_typed_json_fact(&mut facts, format!("provider.npm.dist.{key}"), value);
                if !matches!(value, JSONValue::String(value) if !value.trim().is_empty()) {
                    losses.push(format!("npm dist field `{key}` must be a non-empty string"));
                }
            }
        }
        facts.integrity_hash = json_string(dist, "integrity")
            .or_else(|| json_string(dist, "shasum"))
            .unwrap_or_default();
    } else if metadata.contains_key("dist") {
        losses.push("npm dist metadata must be an object".to_string());
    }
    let mut report = report_with_identity(facts);
    report.losses.extend(losses);
    report.conflicts.extend(conflicts);
    report
}

fn cargo_report(document: &str) -> ProviderFactReport {
    let assignments = toml_assignments(document);
    let package_values = assignments
        .iter()
        .filter(|entry| !entry.array_table && entry.section == "package")
        .collect::<Vec<_>>();
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Cargo,
        first_toml_value(&package_values, "name").unwrap_or_default(),
    );
    facts.version = first_toml_value(&package_values, "version").unwrap_or_default();
    facts.license = first_toml_value(&package_values, "license").unwrap_or_default();
    let mut conflicts = Vec::new();
    for key in ["name", "version", "license"] {
        let values = package_values
            .iter()
            .filter(|entry| entry.key == key)
            .map(|entry| entry.value.clone())
            .collect::<Vec<_>>();
        if values.windows(2).any(|pair| pair[0] != pair[1]) {
            conflicts.push(format!("Cargo package declares conflicting `{key}` values"));
        }
    }
    let mut losses = Vec::new();
    let mut native_values = std::collections::BTreeMap::new();
    for entry in &package_values {
        if matches!(entry.key.as_str(), "name" | "version" | "license")
            && !toml_string_value(&entry.value)
        {
            losses.push(format!(
                "Cargo package field `{}` must be a TOML string",
                entry.key
            ));
        }
    }
    for (assignment_index, entry) in assignments.iter().enumerate() {
        let native_namespace = if entry.array_table {
            "lock"
        } else {
            "manifest"
        };
        let native_key = if entry.array_table {
            format!(
                "provider.cargo.native.{native_namespace}.{}.{}.{}",
                entry.section, assignment_index, entry.key
            )
        } else {
            format!(
                "provider.cargo.native.{native_namespace}.{}.{}",
                entry.section, entry.key
            )
        };
        if let Some(previous) = native_values.insert(native_key.clone(), entry.value.clone()) {
            if previous != entry.value {
                conflicts.push(format!(
                    "Cargo native field {} has conflicting values",
                    native_key
                ));
            }
        }
        add_typed_text_fact(&mut facts, native_key, &entry.value);
        if let Some(kind) = cargo_dependency_kind(&entry.section) {
            if !toml_dependency_value(&entry.value) {
                losses.push(format!(
                    "Cargo dependency `{}` must be a string or inline table",
                    entry.key
                ));
            }
            add_typed_text_fact(
                &mut facts,
                format!("provider.cargo.dependency.{kind}.{}", entry.key),
                &entry.value,
            );
            match kind {
                "runtime" => facts.dependencies.push(entry.key.clone()),
                "dev" => facts.dev_dependencies.push(entry.key.clone()),
                "build" => facts.build_dependencies.push(entry.key.clone()),
                _ => {}
            }
            if let Some(target) = entry.section.strip_prefix("target.") {
                let target = target
                    .strip_suffix(".dependencies")
                    .or_else(|| target.strip_suffix(".dev-dependencies"))
                    .or_else(|| target.strip_suffix(".build-dependencies"))
                    .unwrap_or(target);
                facts.platforms.push(target.to_string());
            }
        }
        if entry.section == "features" {
            add_typed_text_fact(
                &mut facts,
                format!("provider.cargo.variant.feature.{}", entry.key),
                &entry.value,
            );
        }
        if entry.section == "package" && entry.key == "build" {
            let build_value = toml_value_text(&entry.value);
            if build_value != "false" && !toml_string_value(&entry.value) {
                losses.push("Cargo package field `build` must be a string or false".to_string());
            } else if build_value != "false" {
                facts.scripts.push(toml_value_text(&entry.value));
                add_typed_text_fact(&mut facts, "provider.cargo.hook.build", &entry.value);
            }
        }
        if entry.section == "package"
            && matches!(
                entry.key.as_str(),
                "repository" | "homepage" | "authors" | "publish" | "links"
            )
        {
            add_typed_text_fact(
                &mut facts,
                format!("provider.cargo.source.{}", entry.key),
                &entry.value,
            );
        }
    }
    facts.dependencies.sort();
    facts.dependencies.dedup();
    facts.dev_dependencies.sort();
    facts.dev_dependencies.dedup();
    facts.build_dependencies.sort();
    facts.build_dependencies.dedup();
    facts.platforms.sort();
    facts.platforms.dedup();
    let lock_records = cargo_lock_records(&assignments);
    if !package_values.iter().any(|entry| entry.key == "name") && !lock_records.is_empty() {
        if lock_records.len() == 1 {
            facts.name = lock_records[0]
                .get("name")
                .map(|value| toml_value_text(value))
                .unwrap_or_default();
            facts.version = lock_records[0]
                .get("version")
                .map(|value| toml_value_text(value))
                .unwrap_or_default();
        } else {
            facts.name = "cargo-lock".to_string();
            facts.version = "set".to_string();
            facts.dependencies = lock_records
                .iter()
                .filter_map(|record| record.get("name").map(|value| toml_value_text(value)))
                .collect();
            losses.push(
                "Cargo.lock contains multiple package identities; lock each package separately"
                    .to_string(),
            );
        }
    }
    for record in &lock_records {
        let Some(name) = record.get("name").map(|value| toml_value_text(value)) else {
            losses.push("Cargo.lock package has no name".to_string());
            continue;
        };
        let record_version = record
            .get("version")
            .map(|value| toml_value_text(value))
            .unwrap_or_default();
        for key in ["name", "version", "source", "checksum"] {
            if let Some(value) = record.get(key) {
                if !toml_string_value(value) {
                    losses.push(format!(
                        "Cargo.lock field `{key}` for package `{name}` must be a TOML string"
                    ));
                }
            }
        }
        if name == facts.name
            && !facts.version.is_empty()
            && !record_version.is_empty()
            && record_version != facts.version
        {
            conflicts.push(format!(
                "Cargo.lock package `{name}` version `{record_version}` conflicts with manifest version `{}`",
                facts.version
            ));
        }
        for key in ["version", "source", "checksum", "dependencies"] {
            if let Some(value) = record.get(key) {
                let projection_key = format!("provider.cargo.lock.{name}.{key}");
                if let Some(previous) = facts
                    .typed
                    .get(&projection_key)
                    .and_then(|values| values.first())
                {
                    if previous != value {
                        conflicts.push(format!(
                            "Cargo.lock field {} has conflicting values",
                            projection_key
                        ));
                    }
                }
                add_typed_text_fact(&mut facts, projection_key, value);
            }
        }
        if record
            .get("source")
            .map(|value| toml_value_text(value).starts_with("registry+"))
            .unwrap_or(false)
            && !record.contains_key("checksum")
        {
            losses.push(format!(
                "Cargo.lock registry package `{name}` has no checksum"
            ));
        }
        if name == facts.name {
            if let Some(checksum) = record.get("checksum") {
                facts.integrity_hash = toml_value_text(checksum);
            }
        }
    }
    facts.source_identity = format!("cargo:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    report.losses.extend(losses);
    report.conflicts.extend(conflicts);
    report
}

#[derive(Debug, Clone)]
struct TomlAssignment {
    section: String,
    array_table: bool,
    key: String,
    value: String,
}

fn toml_assignments(document: &str) -> Vec<TomlAssignment> {
    let mut section = String::new();
    let mut array_table = false;
    let mut assignments = Vec::new();
    for line in document.lines() {
        let line = line.trim();
        if line.starts_with("[[") && line.ends_with("]]") {
            section = line[2..line.len() - 2].trim().to_string();
            array_table = true;
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_string();
            array_table = false;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        assignments.push(TomlAssignment {
            section: section.clone(),
            array_table,
            key: key.to_string(),
            value: value.trim().to_string(),
        });
    }
    assignments
}

fn first_toml_value(entries: &[&TomlAssignment], key: &str) -> Option<String> {
    entries
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| toml_value_text(&entry.value))
}

fn toml_value_text(value: &str) -> String {
    let value = toml_value_without_comment(value);
    let value = value.trim();
    if value.len() >= 2
        && matches!(
            (value.as_bytes().first(), value.as_bytes().last()),
            (Some(b'"'), Some(b'"')) | (Some(b'\''), Some(b'\''))
        )
    {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

fn toml_value_without_comment(value: &str) -> String {
    let mut quoted = None;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
        match quoted {
            Some('"') if character == '"' && !escaped => quoted = None,
            Some('\'') if character == '\'' => quoted = None,
            None if character == '"' || character == '\'' => quoted = Some(character),
            None if character == '#' => return value[..index].to_string(),
            _ => {}
        }
        escaped = character == '\\' && !escaped;
        if character != '\\' {
            escaped = false;
        }
    }
    value.to_string()
}

fn toml_string_value(value: &str) -> bool {
    let value = toml_value_without_comment(value);
    let value = value.trim();
    value.len() >= 2
        && matches!(
            (value.as_bytes().first(), value.as_bytes().last()),
            (Some(b'"'), Some(b'"')) | (Some(b'\''), Some(b'\''))
        )
}

fn toml_dependency_value(value: &str) -> bool {
    let value = toml_value_without_comment(value);
    let value = value.trim();
    toml_string_value(value)
        || (value.starts_with('{') && value.ends_with('}') && value.len() >= 2)
}

fn cargo_dependency_kind(section: &str) -> Option<&'static str> {
    let section = section.strip_prefix("target.").unwrap_or(section);
    match section {
        "dependencies" => Some("runtime"),
        "dev-dependencies" => Some("dev"),
        "build-dependencies" => Some("build"),
        value if value.ends_with(".dependencies") => Some("runtime"),
        value if value.ends_with(".dev-dependencies") => Some("dev"),
        value if value.ends_with(".build-dependencies") => Some("build"),
        _ => None,
    }
}

fn cargo_lock_records(
    assignments: &[TomlAssignment],
) -> Vec<std::collections::BTreeMap<String, String>> {
    let mut records = Vec::new();
    let mut current = None;
    for entry in assignments {
        if !entry.array_table || entry.section != "package" {
            continue;
        }
        if current.is_none() {
            current = Some(std::collections::BTreeMap::new());
        }
        if entry.key == "name"
            && current
                .as_ref()
                .is_some_and(|record| record.contains_key("name"))
        {
            records.push(current.take().unwrap_or_default());
            current = Some(std::collections::BTreeMap::new());
        }
        if let Some(record) = current.as_mut() {
            record.insert(entry.key.clone(), entry.value.clone());
        }
    }
    if let Some(record) = current {
        if !record.is_empty() {
            records.push(record);
        }
    }
    records
}

fn pypi_report(document: &str) -> ProviderFactReport {
    if let Ok(JSONValue::Object(object)) = JSON::parse(document) {
        return pypi_json_report(&object);
    }

    let fields = metadata_fields(document);
    let names = metadata_values(&fields, "name");
    let versions = metadata_values(&fields, "version");
    let mut facts = MetadataFacts::empty(
        ProviderFamily::PyPI,
        names
            .first()
            .cloned()
            .or_else(|| toml_string(document, "name"))
            .unwrap_or_default(),
    );
    facts.version = versions
        .first()
        .cloned()
        .or_else(|| toml_string(document, "version"))
        .unwrap_or_default();
    facts.dependencies = metadata_values(&fields, "requires-dist");
    facts.license = metadata_values(&fields, "license")
        .first()
        .cloned()
        .unwrap_or_default();

    for (index, (key, value)) in fields.iter().enumerate() {
        let normalized = key.to_ascii_lowercase().replace('-', "_");
        add_typed_text_fact(
            &mut facts,
            format!("provider.pypi.native.{normalized}.{index}"),
            value,
        );
        match normalized.as_str() {
            "requires_python" => {
                add_typed_text_fact(&mut facts, "provider.pypi.variant.requires_python", value)
            }
            "project_url" | "home_page" | "download_url" | "author" | "author_email"
            | "maintainer" | "maintainer_email" => add_typed_text_fact(
                &mut facts,
                format!("provider.pypi.source.{normalized}.{index}"),
                value,
            ),
            "classifier" => {
                if value.starts_with("Operating System ::") || value.starts_with("Platform ::") {
                    facts.platforms.push(value.clone());
                }
                if facts.license.is_empty() && value.starts_with("License ::") {
                    facts.license = value.clone();
                }
                add_typed_text_fact(
                    &mut facts,
                    format!("provider.pypi.variant.classifier.{index}"),
                    value,
                );
            }
            "provides_extra" => add_typed_text_fact(
                &mut facts,
                format!("provider.pypi.variant.extra.{index}"),
                value,
            ),
            "dynamic" => {
                add_typed_text_fact(&mut facts, format!("provider.pypi.dynamic.{index}"), value)
            }
            "yanked" | "yanked_reason" | "signature" | "gpg_signature" => {
                add_typed_text_fact(
                    &mut facts,
                    format!("provider.pypi.advisory.{normalized}.{index}"),
                    value,
                );
            }
            _ => {}
        }
    }
    facts.platforms.sort();
    facts.platforms.dedup();
    facts.source_identity = format!("pypi:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    for (field, values) in [("Name", names), ("Version", versions)] {
        let distinct = distinct_values(&values);
        if distinct.len() > 1 {
            report.conflicts.push(format!(
                "PyPI Core Metadata declares conflicting {field} values: {}",
                distinct.join(", ")
            ));
        }
    }
    if !metadata_values(&fields, "dynamic").is_empty()
        || document.contains("dynamic =")
        || document.contains("dynamic:")
    {
        report
            .losses
            .push("dynamic Python metadata must be resolved to an exact lock".to_string());
    }
    if document.lines().any(|line| {
        !line.trim().is_empty()
            && !line
                .chars()
                .next()
                .is_some_and(|character| character.is_whitespace())
            && !line.contains(':')
            && !line.contains('=')
    }) {
        report
            .losses
            .push("PyPI metadata contains an unrecognized non-field line".to_string());
    }
    report
}

fn pypi_json_report(object: &std::collections::BTreeMap<String, JSONValue>) -> ProviderFactReport {
    let info = object
        .get("info")
        .and_then(|value| value.as_object().ok())
        .unwrap_or(object);
    let mut facts = MetadataFacts::empty(
        ProviderFamily::PyPI,
        json_string(info, "name")
            .or_else(|| json_string(object, "name"))
            .unwrap_or_default(),
    );
    facts.version = json_string(info, "version")
        .or_else(|| json_string(object, "version"))
        .unwrap_or_default();
    facts.license = json_string(info, "license").unwrap_or_default();
    add_json_projection(&mut facts, "provider.pypi.native", object);
    if info != object {
        add_json_projection(&mut facts, "provider.pypi.info", info);
    }

    let mut losses = Vec::new();
    let mut conflicts = Vec::new();
    if !object.contains_key("info") {
        losses.push("PyPI JSON has no `info` object".to_string());
    }
    if object
        .get("info")
        .is_some_and(|value| value.as_object().is_err())
    {
        losses.push("PyPI `info` must be an object".to_string());
    }
    if info != object {
        for field in ["name", "version"] {
            if let Some(value) = object.get(field) {
                if !matches!(value, JSONValue::String(_)) {
                    losses.push(format!("PyPI top-level `{field}` must be a string"));
                }
            }
        }
    }
    for field in [
        "name",
        "version",
        "license",
        "summary",
        "home_page",
        "download_url",
        "author",
        "author_email",
        "maintainer",
        "maintainer_email",
        "requires_python",
        "description_content_type",
    ] {
        if let Some(value) = info.get(field) {
            if !matches!(value, JSONValue::String(_)) {
                losses.push(format!("PyPI `info.{field}` must be a string"));
            }
        }
    }
    if let (Some(info_name), Some(root_name)) = (
        json_string(info, "name"),
        json_string(object, "name"),
    ) {
        if info != object && info_name != root_name {
            conflicts.push(format!(
                "PyPI metadata declares conflicting name values: {info_name}, {root_name}"
            ));
        }
    }
    if let (Some(info_version), Some(root_version)) = (
        json_string(info, "version"),
        json_string(object, "version"),
    ) {
        if info != object && info_version != root_version {
            conflicts.push(format!(
                "PyPI metadata declares conflicting version values: {info_version}, {root_version}"
            ));
        }
    }
    for (key, value) in [
        ("summary", "summary"),
        ("home_page", "home_page"),
        ("download_url", "download_url"),
        ("author", "author"),
        ("author_email", "author_email"),
        ("maintainer", "maintainer"),
        ("maintainer_email", "maintainer_email"),
        ("requires_python", "requires_python"),
        ("description_content_type", "description_content_type"),
    ] {
        if let Some(value) = json_string(info, value) {
            let namespace = if matches!(
                key,
                "home_page"
                    | "download_url"
                    | "author"
                    | "author_email"
                    | "maintainer"
                    | "maintainer_email"
            ) {
                "provider.pypi.source"
            } else if key == "requires_python" {
                "provider.pypi.variant"
            } else {
                "provider.pypi.metadata"
            };
            add_typed_text_fact(&mut facts, format!("{namespace}.{key}"), &value);
        }
    }
    for (field, kind) in [
        ("requires_dist", "runtime"),
        ("provides_extra", "extra"),
        ("classifiers", "classifier"),
        ("project_urls", "project_url"),
    ] {
        let Some(value) = info.get(field) else {
            continue;
        };
        match value {
            JSONValue::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    if let JSONValue::String(value) = value {
                        if field == "requires_dist" {
                            facts.dependencies.push(value.clone());
                            add_typed_text_fact(
                                &mut facts,
                                format!("provider.pypi.dependency.{kind}.{index}"),
                                value,
                            );
                        } else if field == "classifiers" {
                            add_typed_text_fact(
                                &mut facts,
                                format!("provider.pypi.variant.{kind}.{index}"),
                                value,
                            );
                            if value.starts_with("Operating System ::")
                                || value.starts_with("Platform ::")
                            {
                                facts.platforms.push(value.clone());
                            }
                            if facts.license.is_empty() && value.starts_with("License ::") {
                                facts.license = value.clone();
                            }
                        } else {
                            add_typed_text_fact(
                                &mut facts,
                                format!("provider.pypi.variant.{kind}.{index}"),
                                value,
                            );
                        }
                    } else {
                        losses.push(format!("PyPI `{field}[{index}]` must be a string"));
                    }
                }
            }
            JSONValue::Object(values) if field == "project_urls" => {
                for (name, value) in values {
                    if let JSONValue::String(value) = value {
                        add_typed_text_fact(
                            &mut facts,
                            format!("provider.pypi.source.project_url.{name}"),
                            value,
                        );
                    } else {
                        losses.push(format!("PyPI project URL `{name}` must be a string"));
                    }
                }
            }
            _ => losses.push(format!("PyPI `{field}` has an unsupported shape")),
        }
    }
    if let Some(JSONValue::Array(values)) = info.get("dynamic") {
        for (index, value) in values.iter().enumerate() {
            add_typed_json_fact(&mut facts, format!("provider.pypi.dynamic.{index}"), value);
            if !matches!(value, JSONValue::String(_)) {
                losses.push(format!("PyPI `dynamic[{index}]` must be a field name"));
            }
        }
        if !values.is_empty() {
            losses.push("dynamic Python metadata must be resolved to an exact lock".to_string());
        }
    } else if info.contains_key("dynamic") {
        losses.push("PyPI `dynamic` must be an array of fields".to_string());
    }
    for (field, target) in [
        ("obsoletes_dist", "provider.pypi.advisory.obsoletes"),
        ("provides_dist", "provider.pypi.metadata.provides"),
        ("license_files", "provider.pypi.metadata.license_files"),
        ("import_name", "provider.pypi.metadata.import_name"),
        (
            "import_namespaces",
            "provider.pypi.metadata.import_namespaces",
        ),
    ] {
        if let Some(value) = info.get(field) {
            add_typed_json_fact(&mut facts, target, value);
        }
    }

    let mut artifact_hashes = Vec::new();
    if let Some(JSONValue::Array(urls)) = object.get("urls") {
        for (index, value) in urls.iter().enumerate() {
            let JSONValue::Object(url) = value else {
                losses.push(format!("PyPI `urls[{index}]` must be an object"));
                continue;
            };
            add_json_projection(&mut facts, &format!("provider.pypi.artifact.{index}"), url);
            let filename = json_string(url, "filename").unwrap_or_else(|| {
                losses.push(format!("PyPI artifact {index} has no filename"));
                index.to_string()
            });
            if let Some(JSONValue::Object(digests)) = url.get("digests") {
                for algorithm in ["md5", "sha256", "sha384", "sha512"] {
                    let Some(value) = digests.get(algorithm) else {
                        continue;
                    };
                    let JSONValue::String(value) = value else {
                        losses.push(format!(
                            "PyPI artifact `{filename}` digest `{algorithm}` must be a string"
                        ));
                        continue;
                    };
                    if value.trim().is_empty() {
                        losses.push(format!(
                            "PyPI artifact `{filename}` digest `{algorithm}` must not be empty"
                        ));
                        continue;
                    }
                    if algorithm == "sha256" {
                        artifact_hashes.push((filename.clone(), value.clone()));
                        add_typed_text_fact(
                            &mut facts,
                            format!("provider.pypi.artifact.{index}.sha256"),
                            value,
                        );
                    }
                    add_typed_text_fact(
                        &mut facts,
                        format!("provider.pypi.signature.{index}.{algorithm}"),
                        value,
                    );
                }
            } else if url.contains_key("digests") {
                losses.push(format!(
                    "PyPI artifact `{filename}` digests must be an object"
                ));
            }
            if let Some(value) = url.get("yanked") {
                if matches!(value, JSONValue::Bool(_)) {
                    add_typed_json_fact(
                        &mut facts,
                        format!("provider.pypi.advisory.yanked.{index}"),
                        value,
                    );
                } else {
                    losses.push(format!("PyPI artifact `{filename}` yanked must be boolean"));
                }
            }
            if let Some(reason) = json_string(url, "yanked_reason") {
                add_typed_text_fact(
                    &mut facts,
                    format!("provider.pypi.advisory.yanked_reason.{index}"),
                    &reason,
                );
            } else if url.contains_key("yanked_reason") {
                losses.push(format!(
                    "PyPI artifact `{filename}` yanked_reason must be a string"
                ));
            }
        }
    }
    artifact_hashes.sort();
    for pair in artifact_hashes.windows(2) {
        if pair[0].0 == pair[1].0 && pair[0].1 != pair[1].1 {
            conflicts.push(format!(
                "PyPI artifact `{}` has conflicting sha256 digests",
                pair[0].0
            ));
        }
    }
    if artifact_hashes.len() == 1 {
        facts.integrity_hash = artifact_hashes[0].1.clone();
    }
    if let Some(JSONValue::Array(vulnerabilities)) = object.get("vulnerabilities") {
        for (index, vulnerability) in vulnerabilities.iter().enumerate() {
            add_typed_json_fact(
                &mut facts,
                format!("provider.pypi.advisory.vulnerability.{index}"),
                vulnerability,
            );
            if !matches!(vulnerability, JSONValue::Object(_)) {
                losses.push(format!(
                    "PyPI vulnerability {index} must be an object"
                ));
            }
        }
    }
    if object
        .get("urls")
        .is_some_and(|value| !matches!(value, JSONValue::Array(_)))
    {
        losses.push("PyPI `urls` must be an array of artifacts".to_string());
    }
    if object
        .get("vulnerabilities")
        .is_some_and(|value| !matches!(value, JSONValue::Array(_)))
    {
        losses.push("PyPI `vulnerabilities` must be an array".to_string());
    }
    facts.source_identity = format!("pypi:{}@{}", facts.name, facts.version);
    facts.dependencies.sort();
    facts.dependencies.dedup();
    facts.platforms.sort();
    facts.platforms.dedup();
    let mut report = report_with_identity(facts);
    report.losses.extend(losses);
    report.conflicts.extend(conflicts);
    report
}

fn swiftpm_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let Some(JSONValue::Object(root)) = parsed else {
        return empty_report(
            ProviderFamily::SwiftPM,
            "swiftpm",
            "Package.resolved is not valid JSON",
        );
    };
    let root_pins = root.get("pins");
    let object_pins = root
        .get("object")
        .and_then(|value| value.as_object().ok())
        .and_then(|object| object.get("pins"));
    if let (Some(root_pins), Some(object_pins)) = (root_pins, object_pins) {
        if root_pins != object_pins {
            return empty_report(
                ProviderFamily::SwiftPM,
                "swiftpm",
                "SwiftPM lock has conflicting top-level and object `pins` arrays",
            );
        }
    }
    let pins = match root_pins.or(object_pins) {
        Some(JSONValue::Array(pins)) => pins,
        Some(_) => {
            return empty_report(
                ProviderFamily::SwiftPM,
                "swiftpm",
                "Package.resolved `pins` must be an array",
            )
        }
        None => {
            return empty_report(
                ProviderFamily::SwiftPM,
                "swiftpm",
                "Package.resolved has no `pins` array",
            )
        }
    };

    let mut facts = MetadataFacts::empty(ProviderFamily::SwiftPM, String::new());
    let mut losses = Vec::new();
    let mut conflicts = Vec::new();
    add_json_projection(&mut facts, "provider.swiftpm.native", &root);
    match root.get("version") {
        Some(JSONValue::Number(version)) if matches!(*version, 1..=3) => {
            add_typed_text_fact(
                &mut facts,
                "provider.swiftpm.lock.version",
                &version.to_string(),
            );
        }
        Some(JSONValue::Number(version)) => losses.push(format!(
            "SwiftPM lock version `{version}` is unsupported; expected v1, v2, or v3"
        )),
        Some(_) => losses.push("SwiftPM lock `version` must be numeric".to_string()),
        None => losses.push("SwiftPM lock has no numeric `version`".to_string()),
    }

    if pins.len() == 1 {
        let Some(JSONValue::Object(pin)) = pins.first() else {
            return empty_report(
                ProviderFamily::SwiftPM,
                "swiftpm",
                "Package.resolved pin is not an object",
            );
        };
        add_json_projection(&mut facts, "provider.swiftpm.pin", pin);
        let identity = json_string(pin, "identity");
        let package = json_string(pin, "package");
        if let (Some(identity), Some(package)) = (&identity, &package) {
            if identity != package {
                conflicts.push(format!(
                    "SwiftPM pin declares conflicting identity values: {identity}, {package}"
                ));
            }
        }
        for field in ["identity", "package", "location", "repositoryURL", "kind"] {
            if let Some(value) = pin.get(field) {
                if !matches!(value, JSONValue::String(_)) {
                    losses.push(format!("SwiftPM pin `{field}` must be a string"));
                }
            }
        }
        facts.name = identity.or(package).unwrap_or_default();
        let state = match pin.get("state") {
            Some(JSONValue::Object(state)) => Some(state),
            Some(_) => {
                losses.push(format!("SwiftPM pin `{}` state must be an object", facts.name));
                None
            }
            None => None,
        };
        if let Some(state) = state {
            for field in ["version", "revision", "branch"] {
                if let Some(value) = state.get(field) {
                    if !matches!(value, JSONValue::String(_) | JSONValue::Null) {
                        losses.push(format!(
                            "SwiftPM pin `{}` state field `{field}` must be a string or null",
                            facts.name
                        ));
                    }
                }
            }
        }
        let version = state
            .and_then(|state| json_string(state, "version"))
            .or_else(|| json_string(pin, "version"));
        let revision = state
            .and_then(|state| json_string(state, "revision"))
            .or_else(|| json_string(pin, "revision"));
        let branch = state
            .and_then(|state| json_string(state, "branch"))
            .or_else(|| json_string(pin, "branch"));
        facts.version = version
            .clone()
            .or_else(|| revision.clone())
            .unwrap_or_default();
        if let Some(revision) = revision {
            facts.integrity_hash = revision.clone();
            add_typed_text_fact(&mut facts, "provider.revision", &revision);
        }
        if let Some(branch) = branch {
            add_typed_text_fact(&mut facts, "provider.swiftpm.variant.branch", &branch);
            if facts.integrity_hash.is_empty() {
                losses.push(format!(
                    "SwiftPM pin `{}` has branch `{branch}` but no exact revision",
                    facts.name
                ));
            }
        }
        if let Some(kind) = json_string(pin, "kind") {
            add_typed_text_fact(&mut facts, "provider.swiftpm.variant.kind", &kind);
        }
        if let Some(location) =
            json_string(pin, "location").or_else(|| json_string(pin, "repositoryURL"))
        {
            add_typed_text_fact(&mut facts, "provider.swiftpm.source.location", &location);
        }
        if state.is_none() && !pin.contains_key("state") {
            losses.push(format!("SwiftPM pin `{}` has no state object", facts.name));
        }
        let source_identity = facts
            .typed
            .get("provider.revision")
            .and_then(|values| values.first())
            .filter(|revision| !revision.is_empty())
            .cloned()
            .unwrap_or_else(|| facts.version.clone());
        facts.source_identity = format!("swiftpm:{}@{}", facts.name, source_identity);
    } else {
        facts.name = "swiftpm-lock".to_string();
        facts.dependencies = pins
            .iter()
            .enumerate()
            .filter_map(|(index, pin)| {
                let JSONValue::Object(pin) = pin else {
                    losses.push(format!("SwiftPM pin {index} is not an object"));
                    return None;
                };
                let identity = json_string(pin, "identity").or_else(|| json_string(pin, "package"));
                if identity.is_none() {
                    losses.push(format!("SwiftPM pin {index} has no package identity"));
                }
                identity
            })
            .collect();
        facts.source_identity = "swiftpm:lock".to_string();
        if pins.len() > 1 {
            losses.push(
                "Package.resolved contains multiple pins; normalize each pin before realization"
                    .to_string(),
            );
            for (left_index, left) in pins.iter().enumerate() {
                let JSONValue::Object(left) = left else {
                    continue;
                };
                let left_identity =
                    json_string(left, "identity").or_else(|| json_string(left, "package"));
                for right in pins.iter().skip(left_index + 1) {
                    let JSONValue::Object(right) = right else {
                        continue;
                    };
                    let right_identity =
                        json_string(right, "identity").or_else(|| json_string(right, "package"));
                    if left_identity.is_some() && left_identity == right_identity && left != right {
                        conflicts.push(format!(
                            "SwiftPM pin `{}` has conflicting native states",
                            left_identity.clone().unwrap_or_default()
                        ));
                    }
                }
            }
        }
    }
    let mut report = report_with_identity(facts);
    report.losses.extend(losses);
    report.conflicts.extend(conflicts);
    report
}

fn maven_exact_value(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.contains("${")
        && !value.contains(',')
        && !value.contains('[')
        && !value.contains(']')
        && !value.contains('(')
        && !value.contains(')')
        && !value.eq_ignore_ascii_case("latest")
        && !value.eq_ignore_ascii_case("release")
}

fn maven_xml_loss(document: &str) -> Option<String> {
    const MAVEN_NAMESPACE: &str = "http://maven.apache.org/POM/4.0.0";
    const XML_SCHEMA_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema-instance";

    let mut stack = Vec::<String>::new();
    let mut cursor = 0usize;
    let mut root_seen = false;
    while cursor < document.len() {
        let Some(relative_start) = document[cursor..].find('<') else {
            if !document[cursor..].trim().is_empty() && root_seen && stack.is_empty() {
                return Some("Maven POM has non-whitespace text outside the project element".to_string());
            }
            break;
        };
        let start = cursor + relative_start;
        if !document[cursor..start].trim().is_empty() && (!root_seen || stack.is_empty()) {
            return Some("Maven POM has text outside the project element".to_string());
        }
        if document[start..].starts_with("<!--") {
            let Some(end) = document[start + 4..].find("-->") else {
                return Some("Maven POM has an unterminated XML comment".to_string());
            };
            cursor = start + 4 + end + 3;
            continue;
        }
        if document[start..].starts_with("<![CDATA[") {
            let Some(end) = document[start + 9..].find("]]>") else {
                return Some("Maven POM has an unterminated CDATA section".to_string());
            };
            cursor = start + 9 + end + 3;
            continue;
        }
        if document[start..].starts_with("<?") {
            let Some(end) = document[start + 2..].find("?>") else {
                return Some("Maven POM has an unterminated processing instruction".to_string());
            };
            cursor = start + 2 + end + 2;
            continue;
        }
        if document[start..].starts_with("<!") {
            let Some(end) = xml_opening_tag_end(&document[start..]) else {
                return Some("Maven POM has an unterminated declaration".to_string());
            };
            cursor = start + end + 1;
            continue;
        }
        let Some(relative_end) = xml_opening_tag_end(&document[start..]) else {
            return Some("Maven POM has an unterminated XML element".to_string());
        };
        let end = start + relative_end;
        let opening = &document[start..=end];
        let name = xml_opening_name(opening);
        if name.is_empty() {
            return Some("Maven POM has an XML element with no name".to_string());
        }
        if name.contains(':') {
            return Some(
                "Maven POM uses a namespaced XML vocabulary that this provider cannot normalize"
                    .to_string(),
            );
        }
        if !stack.is_empty() && (opening.contains("xmlns=") || opening.contains("xmlns:")) {
            return Some(
                "Maven POM uses a nested XML namespace that this provider cannot normalize"
                    .to_string(),
            );
        }
        let closing = opening.starts_with("</");
        let self_closing = opening.trim_end().ends_with("/>");
        if closing {
            if self_closing {
                return Some("Maven POM has an invalid self-closing end element".to_string());
            }
            if stack.pop().as_deref() != Some(name) {
                return Some(format!("Maven POM has mismatched closing element `{name}`"));
            }
        } else if stack.is_empty() {
            if root_seen {
                return Some("Maven POM has multiple root elements".to_string());
            }
            if name != "project" {
                return Some("Maven POM has no project root element".to_string());
            }
            if let Some(namespace) = xml_attribute(opening, "xmlns") {
                if namespace != MAVEN_NAMESPACE {
                    return Some(format!(
                        "Maven POM uses unsupported XML namespace `{namespace}`"
                    ));
                }
            }
            if let Some(namespace) = xml_attribute(opening, "xmlns:xsi") {
                if namespace != XML_SCHEMA_NAMESPACE {
                    return Some(format!(
                        "Maven POM uses unsupported XML namespace `{namespace}`"
                    ));
                }
            }
            if opening.contains("xmlns:") && !opening.contains("xmlns:xsi=") {
                return Some(
                    "Maven POM uses a namespaced XML vocabulary that this provider cannot normalize"
                        .to_string(),
                );
            }
            root_seen = true;
            if !self_closing {
                stack.push(name.to_string());
            }
        } else if !self_closing {
            stack.push(name.to_string());
        }
        cursor = end + 1;
    }
    if !root_seen {
        return Some("Maven POM has no project root element".to_string());
    }
    if !stack.is_empty() {
        return Some("Maven POM has an unterminated XML element".to_string());
    }
    None
}

fn maven_report(document: &str) -> ProviderFactReport {
    if let Some(loss) = maven_xml_loss(document) {
        return empty_report(ProviderFamily::Maven, "maven", &loss);
    }

    let project_group = xml_direct_child_values(document, "project", "groupId");
    let project_artifact = xml_direct_child_values(document, "project", "artifactId");
    let project_version = xml_direct_child_values(document, "project", "version");
    let parent = xml_blocks(document, "parent")
        .into_iter()
        .next()
        .map(|(_, block)| block);
    let group = project_group
        .first()
        .cloned()
        .or_else(|| {
            parent
                .as_deref()
                .and_then(|parent| xml_tag(parent, "groupId"))
        })
        .unwrap_or_default();
    let name = project_artifact.first().cloned().unwrap_or_default();
    let version = project_version.first().cloned().unwrap_or_default();
    let mut facts = MetadataFacts::empty(ProviderFamily::Maven, name);
    facts.version = version;
    facts.license = xml_blocks(document, "license")
        .into_iter()
        .filter_map(|(_, block)| xml_tag(&block, "name"))
        .next()
        .unwrap_or_default();
    facts.source_identity = format!("maven:{}:{}@{}", group, facts.name, facts.version);
    let artifact_name = facts.name.clone();
    let gav = facts.source_identity.clone();
    add_typed_text_fact(&mut facts, "provider.maven.coordinate.group", &group);
    add_typed_text_fact(
        &mut facts,
        "provider.maven.coordinate.artifact",
        &artifact_name,
    );
    add_typed_text_fact(&mut facts, "provider.maven.coordinate.gav", &gav);
    for (key, values) in [
        (
            "packaging",
            xml_direct_child_values(document, "project", "packaging"),
        ),
        ("name", xml_direct_child_values(document, "project", "name")),
        (
            "description",
            xml_direct_child_values(document, "project", "description"),
        ),
        ("url", xml_direct_child_values(document, "project", "url")),
    ] {
        for (index, value) in values.iter().enumerate() {
            add_typed_text_fact(
                &mut facts,
                format!("provider.maven.project.{key}.{index}"),
                value,
            );
        }
    }
    for (index, value) in xml_tag_values(document, "license").iter().enumerate() {
        add_typed_text_fact(
            &mut facts,
            format!("provider.maven.license.raw.{index}"),
            value,
        );
    }
    for (index, (_, block)) in xml_blocks(document, "license").iter().enumerate() {
        for field in ["name", "url", "distribution"] {
            if let Some(value) = xml_tag(block, field) {
                add_typed_text_fact(
                    &mut facts,
                    format!("provider.maven.license.{field}.{index}"),
                    &value,
                );
            }
        }
    }

    let mut losses = Vec::new();
    let mut conflicts = Vec::new();
    for (field, values) in [
        ("groupId", project_group),
        ("artifactId", project_artifact),
        ("version", project_version),
    ] {
        let distinct = distinct_values(&values);
        if distinct.len() > 1 {
            conflicts.push(format!(
                "Maven project declares conflicting {field} values: {}",
                distinct.join(", ")
            ));
        }
    }
    if group.is_empty() {
        losses.push("Maven project has no exact groupId".to_string());
    }
    if facts.name.is_empty() {
        losses.push("Maven project has no exact artifactId".to_string());
    }
    if !maven_exact_value(&facts.version) {
        losses.push(format!(
            "Maven project version `{}` is not an exact version",
            facts.version
        ));
    }
    if !group.is_empty() && !maven_exact_value(&group) {
        losses.push(format!(
            "Maven project groupId `{group}` is not an exact identity"
        ));
    }

    let mut dependency_versions = std::collections::BTreeMap::<String, String>::new();
    for (index, (_, block)) in xml_blocks(document, "dependency").iter().enumerate() {
        let artifact = xml_tag(block, "artifactId").unwrap_or_default();
        let dependency_group = xml_tag(block, "groupId").unwrap_or_default();
        let dependency_version = xml_tag(block, "version").unwrap_or_default();
        let declared_versions = distinct_values(&xml_tag_values(block, "version"));
        if declared_versions.len() > 1 {
            conflicts.push(format!(
                "Maven dependency `{dependency_group}:{artifact}` declares conflicting version values: {}",
                declared_versions.join(", ")
            ));
        }
        let scope_defaulted = xml_tag(block, "scope").is_none();
        let scope = xml_tag(block, "scope").unwrap_or_else(|| "compile".to_string());
        let coordinate = format_dependency(
            &format!("{dependency_group}:{artifact}"),
            &dependency_version,
        );
        if artifact.is_empty() || dependency_group.is_empty() {
            losses.push(format!(
                "Maven dependency {index} lacks an exact groupId or artifactId"
            ));
        }
        if dependency_version.is_empty() {
            losses.push(format!(
                "Maven dependency `{coordinate}` has no exact version"
            ));
        } else if !maven_exact_value(&dependency_version) {
            losses.push(format!(
                "Maven dependency `{coordinate}` has a non-exact version"
            ));
        }
        let dependency_key = format!("{dependency_group}:{artifact}");
        if let Some(previous) =
            dependency_versions.insert(dependency_key.clone(), dependency_version.clone())
        {
            if previous != dependency_version {
                conflicts.push(format!(
                    "Maven dependency `{dependency_key}` declares conflicting version values: {previous}, {dependency_version}"
                ));
            }
        }
        facts.dependencies.push(coordinate.clone());
        add_typed_text_fact(
            &mut facts,
            format!("provider.maven.dependency.{index}.coordinate"),
            &coordinate,
        );
        add_typed_text_fact(
            &mut facts,
            format!("provider.maven.dependency.{index}.kind"),
            &scope,
        );
        if scope_defaulted {
            add_typed_text_fact(
                &mut facts,
                format!("provider.maven.dependency.{index}.scope.defaulted"),
                "true",
            );
        }
        for field in ["optional", "type", "classifier", "systemPath"] {
            if let Some(value) = xml_tag(block, field) {
                add_typed_text_fact(
                    &mut facts,
                    format!("provider.maven.dependency.{index}.{field}"),
                    &value,
                );
            }
        }
        match scope.as_str() {
            "test" => facts.dev_dependencies.push(coordinate),
            "provided" | "system" => facts.build_dependencies.push(coordinate),
            _ => {}
        }
    }
    for (index, (_, block)) in xml_blocks(document, "plugin").iter().enumerate() {
        let artifact = xml_tag(block, "artifactId").unwrap_or_default();
        let plugin_group_defaulted = xml_tag(block, "groupId").is_none();
        let plugin_group =
            xml_tag(block, "groupId").unwrap_or_else(|| "org.apache.maven.plugins".to_string());
        let plugin_version = xml_tag(block, "version").unwrap_or_default();
        let plugin = format_dependency(&format!("{plugin_group}:{artifact}"), &plugin_version);
        if artifact.is_empty() {
            losses.push(format!("Maven build plugin {index} has no artifactId"));
        }
        if !maven_exact_value(&plugin_version) {
            losses.push(format!(
                "Maven build plugin `{plugin}` has no exact version"
            ));
        }
        facts.scripts.push(plugin.clone());
        add_typed_text_fact(&mut facts, format!("provider.maven.hook.{index}"), &plugin);
        add_typed_text_fact(
            &mut facts,
            format!("provider.maven.hook.{index}.group"),
            &plugin_group,
        );
        if plugin_group_defaulted {
            add_typed_text_fact(
                &mut facts,
                format!("provider.maven.hook.{index}.group.defaulted"),
                "true",
            );
        }
        for (goal_index, goal) in xml_tag_values(block, "goal").iter().enumerate() {
            add_typed_text_fact(
                &mut facts,
                format!("provider.maven.hook.{index}.goal.{goal_index}"),
                goal,
            );
        }
    }
    for (index, (_, block)) in xml_blocks(document, "profile").iter().enumerate() {
        if let Some(id) = xml_tag(block, "id") {
            add_typed_text_fact(
                &mut facts,
                format!("provider.maven.variant.profile.{index}.id"),
                &id,
            );
        }
        for field in ["os", "arch", "jdk", "activeByDefault"] {
            if let Some(value) = xml_tag(block, field) {
                let value = if field == "os" {
                    xml_tag(&value, "name").unwrap_or(value)
                } else {
                    value
                };
                add_typed_text_fact(
                    &mut facts,
                    format!("provider.maven.variant.profile.{index}.{field}"),
                    &value,
                );
                if field == "os" || field == "arch" {
                    facts.platforms.push(value);
                }
            }
        }
    }
    for (index, (_, block)) in xml_blocks(document, "scm").iter().enumerate() {
        for field in ["url", "connection", "developerConnection", "tag"] {
            if let Some(value) = xml_tag(block, field) {
                add_typed_text_fact(
                    &mut facts,
                    format!("provider.maven.source.scm.{field}.{index}"),
                    &value,
                );
            }
        }
    }
    for (index, (_, block)) in xml_blocks(document, "repository").iter().enumerate() {
        if let Some(url) = xml_tag(block, "url") {
            add_typed_text_fact(
                &mut facts,
                format!("provider.maven.source.repository.{index}"),
                &url,
            );
        }
    }
    for tag in [
        "signature",
        "sha1",
        "sha256",
        "yanked",
        "vulnerability",
        "advisory",
    ] {
        for (index, value) in xml_tag_values(document, tag).iter().enumerate() {
            add_typed_text_fact(
                &mut facts,
                format!("provider.maven.audit.{tag}.{index}"),
                value,
            );
        }
    }
    facts.dependencies.sort();
    facts.dependencies.dedup();
    facts.dev_dependencies.sort();
    facts.dev_dependencies.dedup();
    facts.build_dependencies.sort();
    facts.build_dependencies.dedup();
    facts.platforms.sort();
    facts.platforms.dedup();
    let mut report = report_with_identity(facts);
    report.losses.extend(losses);
    report.conflicts.extend(conflicts);
    report
}

fn nuget_report(document: &str) -> ProviderFactReport {
    if let Ok(JSONValue::Object(object)) = JSON::parse(document) {
        return nuget_json_report(&object);
    }
    if xml_has_namespace(document) {
        return empty_report(
            ProviderFamily::NuGet,
            "nuget",
            "NuGet XML uses a namespaced vocabulary that this provider cannot normalize",
        );
    }
    let mut facts = MetadataFacts::empty(ProviderFamily::NuGet, String::new());
    let root_package = match (
        xml_tag(document, "id").or_else(|| xml_tag(document, "Id")),
        xml_tag(document, "version").or_else(|| xml_tag(document, "Version")),
    ) {
        (Some(name), Some(version)) => Some((name, version)),
        _ => None,
    };
    let package_references = xml_opening_tags(document, "PackageReference")
        .into_iter()
        .filter_map(|tag| {
            let name = xml_attribute(&tag, "Include").or_else(|| xml_attribute(&tag, "Update"))?;
            let version = xml_attribute(&tag, "Version").unwrap_or_default();
            Some((name, version, tag))
        })
        .collect::<Vec<_>>();
    let package_config_references = xml_opening_tags(document, "package")
        .into_iter()
        .filter_map(|tag| {
            let name = xml_attribute(&tag, "id")?;
            let version = xml_attribute(&tag, "version").unwrap_or_default();
            Some((name, version, tag))
        })
        .collect::<Vec<_>>();
    let mut losses = Vec::new();
    for tag in xml_opening_tags(document, "PackageReference") {
        let Some(name) = xml_attribute(&tag, "Include").or_else(|| xml_attribute(&tag, "Update"))
        else {
            losses.push("NuGet `PackageReference` has no Include or Update identity".to_string());
            continue;
        };
        let version = xml_attribute(&tag, "Version").unwrap_or_default();
        if version.is_empty() || !exact_provider_version(&version) {
            losses.push(format!(
                "NuGet PackageReference `{name}` has no resolved exact version"
            ));
        }
    }
    for tag in xml_opening_tags(document, "package") {
        let Some(name) = xml_attribute(&tag, "id") else {
            continue;
        };
        let version = xml_attribute(&tag, "version").unwrap_or_default();
        if version.is_empty() || !exact_provider_version(&version) {
            losses.push(format!(
                "NuGet package `{name}` has no resolved exact version"
            ));
        }
    }
    let dependency_values = xml_opening_tags(document, "dependency")
        .into_iter()
        .filter_map(|tag| {
            let name = xml_attribute(&tag, "id").or_else(|| xml_attribute(&tag, "name"))?;
            let version = xml_attribute(&tag, "version").unwrap_or_default();
            let dependency = format_dependency(&name, &version);
            Some(dependency)
        })
        .collect::<Vec<_>>();
    for tag in xml_opening_tags(document, "dependency") {
        let Some(name) = xml_attribute(&tag, "id").or_else(|| xml_attribute(&tag, "name")) else {
            losses.push("NuGet dependency has no id or name".to_string());
            continue;
        };
        let version = xml_attribute(&tag, "version").unwrap_or_default();
        if version.is_empty() || !exact_provider_version(&version) {
            losses.push(format!(
                "NuGet dependency `{name}` has no resolved exact version"
            ));
        }
    }
    let mut package_references = package_references;
    package_references.extend(package_config_references);
    if let Some((name, version)) = root_package.clone() {
        facts.name = name;
        facts.version = version;
    } else if package_references.len() == 1 {
        facts.name = package_references[0].0.clone();
        facts.version = package_references[0].1.clone();
    } else {
        facts.name = "nuget-lock".to_string();
        facts.version = "set".to_string();
        facts.dependencies = package_references
            .iter()
            .map(|(name, version, _)| format_dependency(name, version))
            .collect();
        if package_references.is_empty() {
            losses.push("NuGet metadata has no package identity".to_string());
        }
    }
    facts.license = xml_tag(document, "license")
        .or_else(|| xml_tag(document, "licenseExpression"))
        .or_else(|| xml_tag(document, "licenseUrl"))
        .unwrap_or_default();
    facts.platforms = xml_opening_tags(document, "group")
        .into_iter()
        .filter_map(|tag| xml_attribute(&tag, "targetFramework"))
        .collect();
    facts.platforms.extend(
        package_references
            .iter()
            .filter_map(|(_, _, tag)| xml_attribute(tag, "targetFramework")),
    );
    facts.platforms.sort();
    facts.platforms.dedup();
    facts.dependencies = if !dependency_values.is_empty() {
        dependency_values
    } else if root_package.is_none() && package_references.len() > 1 {
        package_references
            .iter()
            .map(|(name, version, _)| format_dependency(name, version))
            .collect()
    } else {
        Vec::new()
    };
    for (index, (name, version, tag)) in package_references.iter().enumerate() {
        add_typed_text_fact(
            &mut facts,
            format!("provider.nuget.dependency.reference.{index}"),
            &format_dependency(name, version),
        );
        for key in [
            "Condition",
            "PrivateAssets",
            "IncludeAssets",
            "ExcludeAssets",
        ] {
            if let Some(value) = xml_attribute(tag, key) {
                add_typed_text_fact(
                    &mut facts,
                    format!("provider.nuget.dependency.reference.{index}.{key}"),
                    &value,
                );
            }
        }
    }
    for (index, dependency) in xml_opening_tags(document, "dependency")
        .into_iter()
        .enumerate()
    {
        for key in ["id", "name", "version", "exclude"] {
            if let Some(value) = xml_attribute(&dependency, key) {
                add_typed_text_fact(
                    &mut facts,
                    format!("provider.nuget.dependency.nuspec.{index}.{key}"),
                    &value,
                );
            }
        }
    }
    for (tag, key) in [
        ("authors", "authors"),
        ("owners", "owners"),
        ("license", "license"),
        ("licenseExpression", "license_expression"),
        ("licenseUrl", "license_url"),
        ("projectUrl", "project_url"),
        ("repositoryUrl", "repository_url"),
        ("description", "description"),
        ("summary", "summary"),
        ("releaseNotes", "release_notes"),
        ("copyright", "copyright"),
        ("tags", "tags"),
        ("contentHash", "content_hash"),
        ("signature", "signature"),
        ("signatures", "signatures"),
        ("signatureValidation", "signature_validation"),
        ("deprecated", "deprecated"),
        ("deprecation", "deprecation"),
        ("vulnerability", "vulnerability"),
        ("vulnerabilities", "vulnerabilities"),
        ("advisory", "advisory"),
        ("advisories", "advisories"),
    ] {
        for (index, value) in xml_tag_values(document, tag).into_iter().enumerate() {
            add_typed_text_fact(
                &mut facts,
                format!("provider.nuget.metadata.{key}.{index}"),
                &value,
            );
        }
    }
    for (index, tag) in xml_opening_tags(document, "group").into_iter().enumerate() {
        if let Some(target) = xml_attribute(&tag, "targetFramework") {
            add_typed_text_fact(
                &mut facts,
                format!("provider.nuget.platform.group.{index}"),
                &target,
            );
        }
    }
    for (index, tag) in xml_opening_tags(document, "repository")
        .into_iter()
        .enumerate()
    {
        for key in ["type", "url", "commit"] {
            if let Some(value) = xml_attribute(&tag, key) {
                add_typed_text_fact(
                    &mut facts,
                    format!("provider.nuget.source.repository.{index}.{key}"),
                    &value,
                );
            }
        }
    }
    for (tag_name, fact_name) in [
        ("signature", "signature"),
        ("signatures", "signatures"),
        ("deprecation", "deprecation"),
        ("vulnerability", "vulnerability"),
        ("advisory", "advisory"),
    ] {
        for (index, tag) in xml_opening_tags(document, tag_name).into_iter().enumerate() {
            add_typed_text_fact(
                &mut facts,
                format!("provider.nuget.metadata.{fact_name}.opening.{index}"),
                &tag,
            );
        }
    }
    for (index, tag) in xml_opening_tags(document, "file").into_iter().enumerate() {
        if let Some(source) = xml_attribute(&tag, "src") {
            add_typed_text_fact(
                &mut facts,
                format!("provider.nuget.hook.file.{index}"),
                &source,
            );
            if source.contains("tools/") || source.contains("tools\\") {
                facts.scripts.push(source);
            }
        }
    }
    if let Some(repository) = xml_opening_tags(document, "repository")
        .into_iter()
        .find_map(|tag| xml_attribute(&tag, "url"))
    {
        facts
            .typed
            .insert("provider.repository".to_string(), vec![repository]);
    }
    facts.integrity_hash = xml_tag(document, "contentHash").unwrap_or_default();
    facts.source_identity = format!("nuget:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    report.losses.append(&mut losses);
    if root_package.is_none() && package_references.len() > 1 {
        report.losses.push(
            "NuGet metadata contains multiple packages; lock each package identity separately"
                .to_string(),
        );
    }
    if report.facts.version != "set"
        && !report.facts.version.is_empty()
        && !exact_provider_version(&report.facts.version)
    {
        report.losses.push(
            "NuGet package version is a range or mutable selector, not an exact identity"
                .to_string(),
        );
    }
    report
}

fn nuget_json_report(object: &std::collections::BTreeMap<String, JSONValue>) -> ProviderFactReport {
    let mut packages = Vec::new();
    let mut dependency_values = Vec::new();
    let mut losses = Vec::new();
    let mut typed_projection = Vec::new();
    let mut target_frameworks = Vec::new();
    let mut library_hashes = Vec::new();
    if let Some(JSONValue::Object(dependencies)) = object.get("dependencies") {
        for (framework, packages_for_framework) in dependencies {
            target_frameworks.push(format!("framework:{framework}"));
            if let JSONValue::Object(entries) = packages_for_framework {
                for (name, value) in entries {
                    if let JSONValue::Object(entry) = value {
                        let requested = json_string(entry, "requested").unwrap_or_default();
                        let resolved = json_string(entry, "resolved")
                            .or_else(|| json_string(entry, "version"))
                            .unwrap_or_default();
                        let selected = if resolved.is_empty() {
                            requested.clone()
                        } else {
                            resolved.clone()
                        };
                        if !selected.is_empty() {
                            packages.push((name.clone(), selected.clone()));
                            dependency_values.push(format_dependency(name, &selected));
                        }
                        if let Some(requested) = entry.get("requested") {
                            typed_projection.push((
                                format!("provider.nuget.request.{framework}.{name}"),
                                json_value_json(requested),
                            ));
                        }
                        if let Some(resolved) =
                            entry.get("resolved").or_else(|| entry.get("version"))
                        {
                            typed_projection.push((
                                format!("provider.nuget.resolution.{framework}.{name}"),
                                json_value_json(resolved),
                            ));
                        }
                        if resolved.is_empty() || !exact_provider_version(&selected) {
                            losses.push(format!(
                                "NuGet dependency `{name}` on framework `{framework}` has no resolved exact version"
                            ));
                        }
                    } else {
                        losses.push(format!(
                            "NuGet dependency `{name}` on framework `{framework}` is not an object"
                        ));
                    }
                }
            } else {
                losses.push(format!(
                    "NuGet dependency group `{framework}` is not an object"
                ));
            }
        }
    } else if object.contains_key("dependencies") {
        losses.push("NuGet `dependencies` must be an object keyed by target framework".to_string());
    }
    if let Some(JSONValue::Object(libraries)) = object.get("libraries") {
        for (identity, value) in libraries {
            let Some((name, version)) = identity.rsplit_once('/') else {
                losses.push(format!(
                    "NuGet library identity `{identity}` is not `name/version`"
                ));
                continue;
            };
            if name.trim().is_empty() || version.trim().is_empty() {
                losses.push(format!(
                    "NuGet library identity `{identity}` has an empty name or version"
                ));
                continue;
            }
            packages.push((name.to_string(), version.to_string()));
            dependency_values.push(format_dependency(name, version));
            if !exact_provider_version(version) {
                losses.push(format!(
                    "NuGet library identity `{identity}` is not an exact package identity"
                ));
            }
            if let JSONValue::Object(record) = value {
                if let Some(hash) =
                    json_string(record, "sha512").or_else(|| json_string(record, "contentHash"))
                {
                    library_hashes.push((identity.clone(), hash));
                }
            } else {
                losses.push(format!(
                    "NuGet library `{identity}` metadata is not an object"
                ));
            }
        }
    } else if object.contains_key("libraries") {
        losses.push("NuGet `libraries` must be an object keyed by name/version".to_string());
    }
    if let Some(JSONValue::Object(targets)) = object.get("targets") {
        for (framework, packages_for_framework) in targets {
            target_frameworks.push(format!("framework:{framework}"));
            let Some(entries) = packages_for_framework.as_object().ok() else {
                losses.push(format!(
                    "NuGet target framework `{framework}` is not an object"
                ));
                continue;
            };
            for (identity, value) in entries {
                let Some((name, version)) = identity.rsplit_once('/') else {
                    losses.push(format!(
                        "NuGet target identity `{identity}` is not `name/version`"
                    ));
                    continue;
                };
                packages.push((name.to_string(), version.to_string()));
                if !exact_provider_version(version) {
                    losses.push(format!(
                        "NuGet target identity `{identity}` is not an exact package identity"
                    ));
                }
                let Some(record) = value.as_object().ok() else {
                    losses.push(format!(
                        "NuGet target package `{identity}` metadata is not an object"
                    ));
                    continue;
                };
                if let Some(JSONValue::Object(dependencies)) = record.get("dependencies") {
                    for (dependency, resolved) in dependencies {
                        typed_projection.push((
                            format!("provider.nuget.target.{framework}.{identity}.{dependency}"),
                            json_value_json(resolved),
                        ));
                        let version = match resolved {
                            JSONValue::String(value) => value.clone(),
                            JSONValue::Object(value) => {
                                json_string(value, "version").unwrap_or_default()
                            }
                            _ => String::new(),
                        };
                        if version.is_empty() || !exact_provider_version(&version) {
                            losses.push(format!(
                                "NuGet target dependency `{dependency}` on framework `{framework}` has no resolved exact version"
                            ));
                        } else {
                            dependency_values.push(format_dependency(dependency, &version));
                        }
                    }
                } else if record.contains_key("dependencies") {
                    losses.push(format!(
                        "NuGet target package `{identity}` dependencies are not an object"
                    ));
                }
            }
        }
    } else if object.contains_key("targets") {
        losses.push("NuGet `targets` must be an object keyed by target framework".to_string());
    }
    if let Some(JSONValue::Object(groups)) = object.get("projectFileDependencyGroups") {
        for (framework, requests) in groups {
            target_frameworks.push(format!("framework:{framework}"));
            let Some(requests) = requests.as_array().ok() else {
                losses.push(format!(
                    "NuGet project dependency group `{framework}` is not an array"
                ));
                continue;
            };
            for (index, request) in requests.iter().enumerate() {
                if !matches!(request, JSONValue::String(_)) {
                    losses.push(format!(
                        "NuGet project dependency `{framework}[{index}]` is not a string"
                    ));
                    continue;
                }
                typed_projection.push((
                    format!("provider.nuget.project.request.{framework}.{index}"),
                    json_value_json(request),
                ));
            }
        }
    } else if object.contains_key("projectFileDependencyGroups") {
        losses.push(
            "NuGet `projectFileDependencyGroups` must be an object keyed by target framework"
                .to_string(),
        );
    }
    let root_identity = match (
        json_string(object, "id").or_else(|| json_string(object, "name")),
        json_string(object, "version"),
    ) {
        (Some(name), Some(version)) => Some((name, version)),
        _ => object.get("package").and_then(|value| match value {
            JSONValue::Object(package) => Some((
                json_string(package, "id").or_else(|| json_string(package, "name"))?,
                json_string(package, "version")?,
            )),
            _ => None,
        }),
    };
    let has_root_identity = root_identity.is_some();
    packages.sort();
    packages.dedup();
    let mut facts = MetadataFacts::empty(
        ProviderFamily::NuGet,
        root_identity
            .as_ref()
            .map(|(name, _)| name.clone())
            .unwrap_or_default(),
    );
    if let Some((name, version)) = root_identity {
        facts.name = name;
        facts.version = version;
    } else if packages.len() == 1 {
        facts.name = packages[0].0.clone();
        facts.version = packages[0].1.clone();
    } else if packages.len() > 1 {
        facts.name = "nuget-lock".to_string();
        facts.version = "set".to_string();
        facts.dependencies = packages
            .iter()
            .map(|(name, version)| format_dependency(name, version))
            .collect();
    } else {
        facts.version = json_string(object, "version").unwrap_or_default();
    }
    facts.dependencies = if dependency_values.is_empty() && facts.version == "set" {
        packages
            .iter()
            .map(|(name, version)| format_dependency(name, version))
            .collect()
    } else {
        dependency_values
    };
    target_frameworks.sort();
    target_frameworks.dedup();
    facts.platforms = target_frameworks;
    facts.integrity_hash = json_string(object, "contentHash")
        .or_else(|| json_string(object, "hash"))
        .unwrap_or_default();
    if facts.integrity_hash.is_empty() {
        let identity = format!("{}/{}", facts.name, facts.version);
        facts.integrity_hash = library_hashes
            .iter()
            .find(|(library, _)| library == &identity)
            .map(|(_, hash)| hash.clone())
            .unwrap_or_default();
    }
    facts.license = json_string(object, "licenseExpression")
        .or_else(|| json_string(object, "license"))
        .or_else(|| json_string(object, "licenseUrl"))
        .unwrap_or_default();
    if let Some(JSONValue::Object(dependencies)) = object.get("dependencies") {
        facts.platforms = dependencies
            .keys()
            .map(|framework| format!("framework:{framework}"))
            .collect();
    }
    add_json_projection(&mut facts, "provider.nuget.native", object);
    for key in [
        "licenseExpression",
        "licenseUrl",
        "authors",
        "owners",
        "repository",
        "listed",
        "isListed",
        "deprecated",
        "vulnerabilities",
        "signature",
        "signatures",
        "contentHash",
    ] {
        if let Some(value) = object.get(key) {
            add_typed_json_fact(&mut facts, format!("provider.nuget.{key}"), value);
        }
    }
    for (key, value) in typed_projection {
        add_typed_text_fact(&mut facts, key, &value);
    }
    facts.source_identity = format!("nuget:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    report.losses.append(&mut losses);
    if packages.len() > 1 && !has_root_identity {
        report.losses.push(
            "NuGet lock metadata contains multiple package identities; lock each package separately"
                .to_string(),
        );
    }
    if report.facts.version != "set"
        && !report.facts.version.is_empty()
        && !exact_provider_version(&report.facts.version)
    {
        report.losses.push(
            "NuGet package version is a range or mutable selector, not an exact identity"
                .to_string(),
        );
    }
    report
}

fn conan_report(document: &str) -> ProviderFactReport {
    if let Ok(JSONValue::Object(object)) = JSON::parse(document) {
        return conan_json_report(&object);
    }
    let names = assignment_values(document, "name");
    let versions = assignment_values(document, "version");
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Conan,
        names.first().cloned().unwrap_or_default(),
    );
    facts.version = versions.first().cloned().unwrap_or_default();
    facts.license = assignment_values(document, "license")
        .first()
        .cloned()
        .unwrap_or_default();
    facts.dependencies = quoted_values_after_all(document, "requires");
    facts.build_dependencies = quoted_values_after_all(document, "tool_requires");
    facts
        .build_dependencies
        .extend(quoted_values_after_all(document, "build_requires"));
    facts.platforms = quoted_values_after_all(document, "settings");
    for key in [
        "options",
        "generators",
        "settings",
        "revision",
        "rrev",
        "package_id",
        "prev",
    ] {
        let values = quoted_values_after_all(document, key);
        if !values.is_empty() {
            facts.typed.insert(format!("conan.{key}"), values);
        }
    }
    for (index, line) in document.lines().enumerate() {
        let line = line.trim();
        if !line.is_empty() {
            add_typed_text_fact(
                &mut facts,
                format!("provider.conan.native.line.{index}"),
                line,
            );
        }
    }
    facts.source_identity = format!("conan:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if names.windows(2).any(|values| values[0] != values[1]) {
        report
            .conflicts
            .push("Conan recipe declares conflicting package names".to_string());
    }
    if versions.windows(2).any(|values| values[0] != values[1]) {
        report
            .conflicts
            .push("Conan recipe declares conflicting package versions".to_string());
    }
    if !report.facts.version.is_empty() && !exact_provider_version(&report.facts.version) {
        report
            .losses
            .push("Conan package version is not an exact identity".to_string());
    }
    for (field, values) in [
        ("requires", &report.facts.dependencies),
        ("tool_requires", &report.facts.build_dependencies),
    ] {
        if document.lines().any(|line| {
            let trimmed = line.trim_start();
            (trimmed.starts_with(field) || trimmed.starts_with(&format!("self.{field}")))
                && !trimmed.contains('"')
                && !trimmed.contains('\'')
        }) && values.is_empty()
        {
            report.losses.push(format!(
                "Conan `{field}` contains no parseable dependency identity"
            ));
        }
    }
    if document.lines().any(|line| {
        let line = line.trim();
        line.starts_with("def source")
            || line.starts_with("def generate")
            || line.starts_with("def build")
            || line.starts_with("def package")
            || line.starts_with("def validate")
            || line.starts_with("def configure")
            || line.starts_with("def layout")
            || line.contains("self.run(")
            || line.starts_with("python_requires")
    }) {
        report.losses.push(
            "Conan recipe contains executable build or Python hook semantics; transport must be verified before realization"
                .to_string(),
        );
    }
    report
}

fn conan_json_report(object: &std::collections::BTreeMap<String, JSONValue>) -> ProviderFactReport {
    let mut losses = Vec::new();
    let mut conflicts = Vec::new();
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Conan,
        json_string(object, "name").unwrap_or_default(),
    );
    add_json_projection(&mut facts, "provider.conan.native", object);

    let top_level_ref = json_string(object, "ref");
    let ref_name = top_level_ref
        .as_deref()
        .and_then(|value| conan_ref_part(value, 0));
    let ref_version = top_level_ref
        .as_deref()
        .and_then(|value| conan_ref_part(value, 1));
    if facts.name.is_empty() {
        facts.name = ref_name.clone().unwrap_or_default();
    } else if let Some(ref_name) = &ref_name {
        if ref_name != &facts.name {
            conflicts.push(format!(
                "Conan package name `{}` conflicts with ref name `{ref_name}`",
                facts.name
            ));
        }
    }
    facts.version = json_string(object, "version")
        .or(ref_version.clone())
        .unwrap_or_default();
    if let (Some(version), Some(ref_version)) = (json_string(object, "version"), ref_version) {
        if version != ref_version {
            conflicts.push(format!(
                "Conan package version `{version}` conflicts with ref version `{ref_version}`"
            ));
        }
    }
    match object.get("license") {
        Some(JSONValue::String(license)) => facts.license = license.clone(),
        Some(JSONValue::Array(values)) => {
            for (index, value) in values.iter().enumerate() {
                if let JSONValue::String(value) = value {
                    add_typed_text_fact(
                        &mut facts,
                        format!("provider.conan.metadata.license.{index}"),
                        value,
                    );
                } else {
                    losses.push(format!("Conan `license[{index}]` must be a string"));
                }
            }
            facts.license = values
                .iter()
                .find_map(|value| match value {
                    JSONValue::String(value) => Some(value.clone()),
                    _ => None,
                })
                .unwrap_or_default();
        }
        Some(_) => losses.push("Conan `license` must be a string or array of strings".to_string()),
        None => {}
    }
    let dependencies =
        conan_json_dependencies(&mut facts, object.get("requires"), "runtime", &mut losses);
    facts.dependencies = dependencies;
    let mut build_dependencies =
        conan_json_dependencies(&mut facts, object.get("tool_requires"), "tool", &mut losses);
    build_dependencies.extend(conan_json_dependencies(
        &mut facts,
        object.get("build_requires"),
        "build",
        &mut losses,
    ));
    facts.build_dependencies = build_dependencies;

    let graph_value = object.get("graph_lock").or_else(|| object.get("graph"));
    let graph_nodes = graph_value
        .and_then(|value| value.as_object().ok())
        .and_then(|graph| graph.get("nodes"))
        .and_then(|value| value.as_object().ok())
        .or_else(|| object.get("nodes").and_then(|value| value.as_object().ok()));
    if let Some(nodes) = graph_nodes {
        let mut node_identities = Vec::new();
        for (node_id, node_value) in nodes {
            let Some(node) = node_value.as_object().ok() else {
                losses.push(format!("Conan graph node `{node_id}` must be an object"));
                continue;
            };
            add_json_projection(
                &mut facts,
                &format!("provider.conan.graph.node.{node_id}"),
                node,
            );
            add_typed_json_fact(
                &mut facts,
                format!("provider.conan.graph.node.{node_id}"),
                node_value,
            );
            if let Some(reference) = json_string(node, "ref") {
                let name = conan_ref_part(&reference, 0).unwrap_or_default();
                let version = conan_ref_part(&reference, 1).unwrap_or_default();
                if !name.is_empty() && !version.is_empty() {
                    node_identities.push((name, version, reference));
                } else {
                    losses.push(format!(
                        "Conan graph node `{node_id}` has an incomplete package ref"
                    ));
                }
            } else {
                losses.push(format!("Conan graph node `{node_id}` has no package ref"));
            }
            if let Some(requires) = node.get("requires") {
                let dependencies = conan_json_dependencies(
                    &mut facts,
                    Some(requires),
                    &format!("graph.{node_id}"),
                    &mut losses,
                );
                facts.dependencies.extend(dependencies);
            }
            if let Some(tool_requires) = node.get("tool_requires") {
                let dependencies = conan_json_dependencies(
                    &mut facts,
                    Some(tool_requires),
                    &format!("graph.{node_id}.tool"),
                    &mut losses,
                );
                facts.build_dependencies.extend(dependencies);
            }
            if let Some(build_requires) = node.get("build_requires") {
                let dependencies = conan_json_dependencies(
                    &mut facts,
                    Some(build_requires),
                    &format!("graph.{node_id}.build"),
                    &mut losses,
                );
                facts.build_dependencies.extend(dependencies);
            }
        }
        node_identities.sort();
        node_identities.dedup();
        if facts.name.is_empty() {
            if node_identities.len() == 1 {
                facts.name = node_identities[0].0.clone();
                facts.version = node_identities[0].1.clone();
                facts.typed.insert(
                    "provider.conan.source.ref".to_string(),
                    vec![node_identities[0].2.clone()],
                );
            } else if node_identities.len() > 1 {
                losses.push(
                    "Conan graph lock contains multiple package identities; lock each node separately"
                        .to_string(),
                );
            }
        }
    } else if graph_value.is_some() || object.contains_key("nodes") {
        losses.push("Conan graph lock `nodes` must be an object".to_string());
    }

    for key in [
        "settings",
        "options",
        "generators",
        "package_id",
        "rrev",
        "prev",
    ] {
        if let Some(value) = object.get(key) {
            add_typed_json_fact(&mut facts, format!("provider.conan.variant.{key}"), value);
        }
    }
    if let Some(reference) = top_level_ref {
        add_typed_text_fact(&mut facts, "provider.conan.source.ref", &reference);
    }
    facts.source_identity = format!("conan:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    report.losses.append(&mut losses);
    report.conflicts.append(&mut conflicts);
    if !report.facts.version.is_empty() && !exact_provider_version(&report.facts.version) {
        report
            .losses
            .push("Conan package version is not an exact identity".to_string());
    }
    report
}

fn conan_json_dependencies(
    facts: &mut MetadataFacts,
    value: Option<&JSONValue>,
    kind: &str,
    losses: &mut Vec<String>,
) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(values) = value.as_array().ok() else {
        losses.push(format!("Conan `{kind}` dependencies must be an array"));
        return Vec::new();
    };
    let mut dependencies = Vec::new();
    for (index, value) in values.iter().enumerate() {
        match value {
            JSONValue::String(reference) if !reference.trim().is_empty() => {
                dependencies.push(reference.clone());
                add_typed_text_fact(
                    facts,
                    format!("provider.conan.dependency.{kind}.{index}"),
                    reference,
                );
            }
            JSONValue::String(_) => {
                losses.push(format!(
                    "Conan `{kind}[{index}]` has an empty dependency ref"
                ));
            }
            JSONValue::Object(dependency) => {
                let reference =
                    json_string(dependency, "ref").or_else(|| json_string(dependency, "name"));
                let Some(reference) = reference else {
                    losses.push(format!(
                        "Conan `{kind}[{index}]` has no dependency ref or name"
                    ));
                    continue;
                };
                let version = json_string(dependency, "version")
                    .or_else(|| conan_ref_part(&reference, 1))
                    .unwrap_or_default();
                let name = conan_ref_part(&reference, 0).unwrap_or(reference);
                dependencies.push(format_dependency(&name, &version));
                add_typed_json_fact(
                    facts,
                    format!("provider.conan.dependency.{kind}.{index}"),
                    value,
                );
            }
            _ => losses.push(format!(
                "Conan `{kind}[{index}]` must be a string or object"
            )),
        }
    }
    dependencies
}

fn vcpkg_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(
            ProviderFamily::Vcpkg,
            "vcpkg",
            "vcpkg.json is not valid JSON",
        );
    };
    let mut losses = Vec::new();
    let mut conflicts = Vec::new();
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Vcpkg,
        json_string(&object, "name").unwrap_or_default(),
    );
    add_json_projection(&mut facts, "provider.vcpkg.native", &object);
    if facts.name.is_empty() {
        losses.push("vcpkg manifest has no package name".to_string());
    }
    if let Some(value) = object.get("name") {
        if !matches!(value, JSONValue::String(value) if !value.trim().is_empty()) {
            losses.push("vcpkg `name` must be a non-empty string".to_string());
        }
    }
    let version_fields = [
        "version",
        "version-string",
        "version-semver",
        "version-date",
    ];
    let mut versions = Vec::new();
    for key in version_fields {
        if let Some(value) = object.get(key) {
            match value {
                JSONValue::String(value) if !value.trim().is_empty() => {
                    versions.push((key, value.clone()));
                }
                JSONValue::String(_) => {
                    losses.push(format!("vcpkg `{key}` must not be empty"));
                }
                _ => losses.push(format!("vcpkg `{key}` must be a string")),
            }
        }
    }
    facts.version = versions
        .first()
        .map(|(_, version)| version.clone())
        .unwrap_or_default();
    for (key, value) in &versions {
        add_typed_text_fact(&mut facts, format!("provider.vcpkg.variant.{key}"), value);
    }
    facts.dependencies = vcpkg_dependencies(&object, &mut facts.typed, &mut losses);
    if let Some(value) = object.get("supports") {
        match value {
            JSONValue::String(value) if !value.trim().is_empty() => {
                facts.platforms.push(value.clone());
            }
            JSONValue::String(_) => losses.push("vcpkg `supports` must not be empty".to_string()),
            _ => losses.push("vcpkg `supports` must be a string".to_string()),
        }
    }
    if let Some(value) = object.get("license") {
        match value {
            JSONValue::String(value) => facts.license = value.clone(),
            _ => losses.push("vcpkg `license` must be a string".to_string()),
        }
    }
    for key in ["builtin-baseline", "overrides", "features"] {
        if let Some(value) = object.get(key) {
            add_typed_json_fact(&mut facts, format!("vcpkg.{key}"), value);
        }
    }
    if let Some(value) = object.get("port-version") {
        add_typed_json_fact(&mut facts, "provider.vcpkg.variant.port-version", value);
        if !matches!(value, JSONValue::Number(value) if *value >= 0) {
            losses.push("vcpkg `port-version` must be a non-negative integer".to_string());
        }
    }
    if let Some(value) = object.get("builtin-baseline") {
        if !matches!(value, JSONValue::String(value) if !value.trim().is_empty()) {
            losses.push("vcpkg `builtin-baseline` must be a non-empty string".to_string());
        } else {
            add_typed_text_fact(
                &mut facts,
                "provider.vcpkg.variant.baseline",
                value.as_str().unwrap_or_default(),
            );
        }
    }
    if let Some(JSONValue::Object(features)) = object.get("features") {
        for (name, value) in features {
            if !matches!(value, JSONValue::Array(values) if values.iter().all(|item| matches!(item, JSONValue::String(_))))
            {
                losses.push(format!(
                    "vcpkg feature `{name}` must be an array of strings"
                ));
            }
        }
    } else if object.contains_key("features") {
        losses.push("vcpkg `features` must be an object".to_string());
    }
    if let Some(value) = object.get("overrides") {
        if let Some(overrides) = value.as_array().ok() {
            for (index, value) in overrides.iter().enumerate() {
                let Some(override_value) = value.as_object().ok() else {
                    losses.push(format!("vcpkg override {index} must be an object"));
                    continue;
                };
                if json_string(override_value, "name").is_none() {
                    losses.push(format!("vcpkg override {index} has no non-empty `name`"));
                }
                for key in [
                    "version",
                    "version-string",
                    "version-semver",
                    "version-date",
                ] {
                    if let Some(value) = override_value.get(key) {
                        if !matches!(value, JSONValue::String(value) if !value.trim().is_empty()) {
                            losses.push(format!(
                                "vcpkg override {index} field `{key}` must be a non-empty string"
                            ));
                        }
                    }
                }
                if let Some(value) = override_value.get("port-version") {
                    if !matches!(value, JSONValue::Number(value) if *value >= 0) {
                        losses.push(format!(
                            "vcpkg override {index} `port-version` must be a non-negative integer"
                        ));
                    }
                }
            }
        } else {
            losses.push("vcpkg `overrides` must be an array of objects".to_string());
        }
    }
    facts.source_identity = format!("vcpkg:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if versions.windows(2).any(|values| values[0].1 != values[1].1) {
        conflicts.push("vcpkg manifest declares conflicting version fields".to_string());
    }
    report.losses.append(&mut losses);
    report.conflicts.append(&mut conflicts);
    if !report.facts.version.is_empty() && !exact_provider_version(&report.facts.version) {
        report
            .losses
            .push("vcpkg package version is not an exact identity".to_string());
    }
    report
}

fn homebrew_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(
            ProviderFamily::Homebrew,
            "homebrew",
            "formula metadata is not valid JSON",
        );
    };
    let mut losses = Vec::new();
    let mut conflicts = Vec::new();
    let name = json_string(&object, "name")
        .or_else(|| json_string(&object, "full_name"))
        .unwrap_or_default();
    if name.is_empty() {
        losses.push("Homebrew formula has no name or full_name".to_string());
    }
    let mut facts = MetadataFacts::empty(ProviderFamily::Homebrew, name);
    add_json_projection(&mut facts, "provider.homebrew.native", &object);

    let mut versions = Vec::new();
    if let Some(value) = object.get("version") {
        match value {
            JSONValue::String(version) if !version.trim().is_empty() => {
                versions.push(("version", version.clone()));
            }
            JSONValue::String(_) => losses.push("Homebrew formula version is empty".to_string()),
            _ => losses.push("Homebrew formula `version` must be a string".to_string()),
        }
    }
    if let Some(value) = object.get("versions") {
        let Some(versions_object) = value.as_object().ok() else {
            losses.push("Homebrew formula `versions` must be an object".to_string());
            facts.version = String::new();
            facts.source_identity = format!("homebrew:{}@", facts.name);
            let mut report = report_with_identity(facts);
            report.losses.extend(losses);
            report.conflicts.extend(conflicts);
            return report;
        };
        if let Some(stable) = versions_object.get("stable") {
            match stable {
                JSONValue::String(version) if !version.trim().is_empty() => {
                    versions.push(("versions.stable", version.clone()));
                }
                JSONValue::String(_) => {
                    losses.push("Homebrew `versions.stable` is empty".to_string())
                }
                _ => losses.push("Homebrew `versions.stable` must be a string".to_string()),
            }
        }
        for (key, value) in versions_object {
            add_typed_json_fact(
                &mut facts,
                format!("provider.homebrew.variant.version.{key}"),
                value,
            );
        }
    }
    if versions.windows(2).any(|pair| pair[0].1 != pair[1].1) {
        conflicts.push("Homebrew formula declares conflicting version facts".to_string());
    }
    facts.version = versions
        .first()
        .map(|(_, version)| version.clone())
        .unwrap_or_default();

    if let Some(value) = object.get("license") {
        match value {
            JSONValue::String(license) => facts.license = license.clone(),
            _ => losses.push("Homebrew formula `license` must be a string".to_string()),
        }
    }
    facts.dependencies = provider_dependency_field(
        &mut facts,
        &object,
        "dependencies",
        "runtime",
        "homebrew",
        &mut losses,
    );
    facts.build_dependencies = provider_dependency_field(
        &mut facts,
        &object,
        "build_dependencies",
        "build",
        "homebrew",
        &mut losses,
    );
    facts.dev_dependencies = provider_dependency_field(
        &mut facts,
        &object,
        "test_dependencies",
        "test",
        "homebrew",
        &mut losses,
    );
    for (field, kind) in [
        ("recommended_dependencies", "recommended"),
        ("optional_dependencies", "optional"),
        ("uses_from_macos", "platform"),
        ("requirements", "requirement"),
        ("conflicts", "conflict"),
    ] {
        provider_dependency_field(&mut facts, &object, field, kind, "homebrew", &mut losses);
    }
    if let Some(value) = object.get("platforms") {
        match json_string_list(value) {
            Some(platforms) => facts.platforms = platforms,
            None => losses.push("Homebrew formula `platforms` must be strings".to_string()),
        }
    }
    if let Some(value) = object.get("bottle") {
        homebrew_bottle_facts(&mut facts, value, &mut losses);
    }
    facts.integrity_hash = homebrew_source_hash(&object, &mut losses).unwrap_or_default();
    add_homebrew_named_facts(&mut facts, &object);
    facts.source_identity = format!("homebrew:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    report.losses.extend(losses);
    report.conflicts.extend(conflicts);
    report
}

fn jet_registry_report(document: &str) -> ProviderFactReport {
    let lines = document
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let (object, duplicate_lines) = match JSON::parse(document) {
        Ok(JSONValue::Object(object)) => (object, Vec::new()),
        Ok(_) => {
            return empty_report(
                ProviderFamily::JetRegistry,
                "jet-registry",
                "registry metadata must be a JSON object or newline-delimited JSON objects",
            )
        }
        Err(_) => {
            let Some(line) = lines.first() else {
                return empty_report(
                    ProviderFamily::JetRegistry,
                    "jet-registry",
                    "registry metadata is not valid JSON",
                );
            };
            let Some(JSONValue::Object(object)) = JSON::parse(line).ok() else {
                return empty_report(
                    ProviderFamily::JetRegistry,
                    "jet-registry",
                    "registry metadata is not valid JSON",
                );
            };
            (object, lines.iter().skip(1).copied().collect())
        }
    };
    let mut facts = MetadataFacts::empty(
        ProviderFamily::JetRegistry,
        json_string(&object, "name").unwrap_or_default(),
    );
    facts.version = json_string(&object, "version").unwrap_or_default();
    facts.integrity_hash = json_string(&object, "content_hash")
        .or_else(|| json_string(&object, "sha256"))
        .unwrap_or_default();
    facts.license = json_string(&object, "license").unwrap_or_default();
    facts.dependencies = json_keys(&object, "dependencies");
    facts.platforms = json_array_strings(&object, "platforms");
    add_json_projection(&mut facts, "provider.registry.native", &object);
    let mut losses = Vec::new();
    if let Some(value) = object.get("dependencies") {
        if let JSONValue::Object(dependencies) = value {
            for (name, requirement) in dependencies {
                add_typed_json_fact(
                    &mut facts,
                    format!("provider.registry.dependency.{name}"),
                    requirement,
                );
            }
        } else {
            losses.push("registry dependencies must be an object".to_string());
        }
    }
    for (field, role) in [
        ("build_dependencies", "build"),
        ("tool_dependencies", "tool"),
        ("dev_dependencies", "dev"),
        ("test_dependencies", "test"),
        ("optional_dependencies", "optional"),
        ("peer_dependencies", "peer"),
        ("plugin_dependencies", "plugin"),
        ("target_dependencies", "target"),
    ] {
        if let Some(value) = object.get(field) {
            if let JSONValue::Object(dependencies) = value {
                add_typed_json_fact(
                    &mut facts,
                    format!("provider.registry.dependency-role.{role}"),
                    value,
                );
                for (name, requirement) in dependencies {
                    add_typed_json_fact(
                        &mut facts,
                        format!("provider.registry.dependency.{role}.{name}"),
                        requirement,
                    );
                }
            } else {
                losses.push(format!("registry `{field}` dependencies must be an object"));
            }
        }
    }
    if let Some(value) = object.get("features") {
        if let JSONValue::Object(features) = value {
            if features.values().all(|value| {
                matches!(
                    value,
                    JSONValue::Array(values)
                        if values.iter().all(|item| matches!(item, JSONValue::String(_)))
                )
            }) {
                add_typed_json_fact(&mut facts, "provider.registry.features", value);
            } else {
                losses.push("registry feature values must be arrays of strings".to_string());
            }
        } else {
            losses.push("registry `features` must be an object".to_string());
        }
    }
    if let Some(value) = object.get("constraints") {
        if matches!(value, JSONValue::Object(_)) {
            add_typed_json_fact(&mut facts, "provider.registry.constraints", value);
        } else {
            losses.push("registry `constraints` must be an object".to_string());
        }
    }
    if let Some(value) = object.get("platforms") {
        if !matches!(
            value,
            JSONValue::Array(values)
                if values.iter().all(|item| matches!(item, JSONValue::String(_)))
        ) {
            losses.push("registry platforms must be an array of strings".to_string());
        }
    }
    for (field, valid) in [
        (
            "license",
            matches!(
                object.get("license"),
                Some(JSONValue::String(_)) | Some(JSONValue::Object(_)) | None
            ),
        ),
        (
            "source",
            matches!(
                object.get("source"),
                Some(JSONValue::String(_)) | Some(JSONValue::Object(_)) | None
            ),
        ),
        (
            "advisories",
            matches!(
                object.get("advisories"),
                Some(JSONValue::Array(_)) | Some(JSONValue::Object(_)) | None
            ),
        ),
        (
            "variants",
            matches!(object.get("variants"), Some(JSONValue::Object(_)) | None),
        ),
        (
            "hooks",
            matches!(object.get("hooks"), Some(JSONValue::Object(_)) | None),
        ),
    ] {
        if !valid {
            losses.push(format!("registry `{field}` has an unsupported shape"));
        }
    }
    for (field, target) in [
        ("content_hash", "provider.registry.content_hash"),
        ("fingerprint", "provider.registry.fingerprint"),
        ("yanked", "provider.registry.yanked"),
        ("tier", "provider.registry.tier"),
        ("gate_status", "provider.registry.gate_status"),
        ("public_key", "provider.registry.publisher_key"),
        ("signature", "provider.registry.signature"),
        ("owner", "provider.registry.owner"),
        ("source", "provider.registry.source"),
        ("advisories", "provider.registry.advisories"),
        ("variants", "provider.registry.variants"),
        ("hooks", "provider.registry.hooks"),
    ] {
        if let Some(value) = object.get(field) {
            add_typed_json_fact(&mut facts, target, value);
        }
    }
    facts.source_identity = format!("jet-registry:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    report.losses.append(&mut losses);
    if report.facts.integrity_hash.is_empty() {
        report
            .losses
            .push("registry entry has no content hash".to_string());
    }
    if let Some(value) = object.get("yanked") {
        if json_bool(Some(value)).is_none() {
            report
                .losses
                .push("registry `yanked` must be a boolean".to_string());
        }
    }
    for (index, line) in duplicate_lines.iter().enumerate() {
        match JSON::parse(line) {
            Ok(JSONValue::Object(other))
                if json_string(&other, "name") == json_string(&object, "name")
                    && json_string(&other, "version") == json_string(&object, "version") =>
            {
                if other != object {
                    report.conflicts.push(format!(
                        "registry identity on line {} has conflicting native facts",
                        index + 2
                    ));
                }
            }
            Ok(JSONValue::Object(_)) => report.losses.push(format!(
                "registry metadata contains multiple package identities; line {} needs its own lock record",
                index + 2
            )),
            Ok(_) | Err(_) => report.losses.push(format!(
                "registry metadata line {} is not a JSON object",
                index + 2
            )),
        }
    }
    report
}

fn github_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(
            ProviderFamily::Github,
            "github",
            "release metadata is not valid JSON",
        );
    };
    let mut losses = Vec::new();
    let mut conflicts = Vec::new();
    let name = json_string(&object, "name")
        .or_else(|| json_string(&object, "full_name"))
        .or_else(|| {
            object
                .get("repository")
                .and_then(|value| value.as_object().ok())
                .and_then(|repository| json_string(repository, "full_name"))
        })
        .unwrap_or_default();
    if name.is_empty() {
        losses.push("GitHub release has no package or repository name".to_string());
    }
    let mut facts = MetadataFacts::empty(ProviderFamily::Github, name);
    add_json_projection(&mut facts, "provider.github.native", &object);

    let tag = json_string(&object, "tag_name");
    let version = json_string(&object, "version");
    if let (Some(tag), Some(version)) = (&tag, &version) {
        if tag != version {
            conflicts.push(format!(
                "GitHub release declares conflicting tag/version facts: {tag} vs {version}"
            ));
        }
    }
    facts.version = tag.or(version).unwrap_or_default();
    if let Some(value) = object.get("target_commitish") {
        match value {
            JSONValue::String(revision) if !revision.trim().is_empty() => {
                add_typed_text_fact(
                    &mut facts,
                    "provider.github.source.target_commitish",
                    revision,
                );
                if ProviderSelector::parse(&format!("#revision={revision}")).is_exact() {
                    add_typed_text_fact(&mut facts, "provider.github.revision", revision);
                }
            }
            JSONValue::String(_) => losses.push("GitHub `target_commitish` is empty".to_string()),
            _ => losses.push("GitHub `target_commitish` must be a string".to_string()),
        }
    }
    if let Some(value) = object.get("license") {
        facts.license = match value {
            JSONValue::String(license) => license.clone(),
            JSONValue::Object(license) => json_string(license, "spdx_id").unwrap_or_default(),
            _ => {
                losses.push("GitHub `license` must be a string or object".to_string());
                String::new()
            }
        };
        if matches!(value, JSONValue::Object(_)) && facts.license.is_empty() {
            losses.push("GitHub license object has no spdx_id".to_string());
        }
    }
    facts.dependencies = provider_dependency_field(
        &mut facts,
        &object,
        "dependencies",
        "runtime",
        "github",
        &mut losses,
    );
    facts.build_dependencies = provider_dependency_field(
        &mut facts,
        &object,
        "build_dependencies",
        "build",
        "github",
        &mut losses,
    );
    facts.dev_dependencies = provider_dependency_field(
        &mut facts,
        &object,
        "dev_dependencies",
        "dev",
        "github",
        &mut losses,
    );
    for key in ["platforms", "os", "architectures"] {
        if let Some(value) = object.get(key) {
            match json_string_list(value) {
                Some(values) => facts.platforms.extend(values),
                None => losses.push(format!("GitHub `{key}` must be strings")),
            }
        }
    }
    if let Some(value) = object.get("assets") {
        github_asset_facts(&mut facts, value, &mut losses);
    }
    for (field, target) in [
        ("html_url", "provider.github.source.html_url"),
        ("tarball_url", "provider.github.source.tarball_url"),
        ("zipball_url", "provider.github.source.zipball_url"),
        ("repository", "provider.github.source.repository"),
        ("owner", "provider.github.source.owner"),
        ("url", "provider.github.source.api_url"),
        ("node_id", "provider.github.identity.node_id"),
        ("id", "provider.github.identity.id"),
        ("draft", "provider.github.release.draft"),
        ("prerelease", "provider.github.release.prerelease"),
        ("immutable", "provider.github.release.immutable"),
        ("created_at", "provider.github.release.created_at"),
        ("published_at", "provider.github.release.published_at"),
        ("body", "provider.github.release.notes"),
        ("signature", "provider.github.signature"),
        ("verification", "provider.github.signature.verification"),
        ("provenance", "provider.github.provenance"),
        ("advisories", "provider.github.advisories"),
        ("yanked", "provider.github.yanked"),
        ("hooks", "provider.github.hooks"),
        ("variants", "provider.github.variants"),
    ] {
        if let Some(value) = object.get(field) {
            add_typed_json_fact(&mut facts, target, value);
        }
    }
    for (key, target) in [
        ("sha256", "provider.github.digest"),
        ("digest", "provider.github.digest"),
        ("hash", "provider.github.digest"),
    ] {
        if let Some(value) = object.get(key) {
            add_typed_json_fact(&mut facts, target, value);
            if let Some(digest) = json_string(&object, key) {
                if !ProviderSelector::parse(&format!("#digest={digest}")).is_exact() {
                    losses.push(format!("GitHub `{key}` is not an exact digest"));
                } else if facts.integrity_hash.is_empty() {
                    facts.integrity_hash = digest;
                } else if facts.integrity_hash != digest {
                    conflicts
                        .push("GitHub release declares conflicting content digests".to_string());
                }
            } else {
                losses.push(format!("GitHub `{key}` must be a string"));
            }
        }
    }
    facts.source_identity = format!("github:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    report.losses.extend(losses);
    report.conflicts.extend(conflicts);
    report
}

fn binary_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(
            ProviderFamily::Binary,
            "binary",
            "binary metadata is not valid JSON",
        );
    };
    let mut losses = Vec::new();
    let mut conflicts = Vec::new();
    let name = json_string(&object, "name").unwrap_or_default();
    if name.is_empty() {
        losses.push("binary metadata has no package name".to_string());
    }
    let mut facts = MetadataFacts::empty(ProviderFamily::Binary, name);
    add_json_projection(&mut facts, "provider.binary.native", &object);
    if let Some(value) = object.get("version") {
        match value {
            JSONValue::String(version) => facts.version = version.clone(),
            _ => losses.push("binary metadata `version` must be a string".to_string()),
        }
    }
    let mut digests = Vec::new();
    for key in ["hash", "sha256", "digest"] {
        if let Some(value) = object.get(key) {
            match value {
                JSONValue::String(digest) if !digest.trim().is_empty() => {
                    digests.push((key, digest.clone()));
                    add_typed_text_fact(
                        &mut facts,
                        format!("provider.binary.identity.{key}"),
                        digest,
                    );
                }
                JSONValue::String(_) => losses.push(format!("binary metadata `{key}` is empty")),
                _ => losses.push(format!("binary metadata `{key}` must be a string")),
            }
        }
    }
    if digests.windows(2).any(|pair| pair[0].1 != pair[1].1) {
        conflicts.push("binary metadata declares conflicting content hashes".to_string());
    }
    facts.integrity_hash = digests
        .first()
        .map(|(_, digest)| digest.clone())
        .unwrap_or_default();
    if facts.integrity_hash.is_empty() {
        losses.push("binary metadata has no content hash".to_string());
    } else if !ProviderSelector::parse(&format!("#digest={}", facts.integrity_hash)).is_exact() {
        losses.push("binary metadata content hash is not an exact digest".to_string());
    }
    let mut platforms = Vec::new();
    for key in ["platform", "target"] {
        if let Some(value) = object.get(key) {
            match value {
                JSONValue::String(platform) if !platform.trim().is_empty() => {
                    platforms.push(platform.clone());
                }
                JSONValue::String(_) => losses.push(format!("binary metadata `{key}` is empty")),
                _ => losses.push(format!("binary metadata `{key}` must be a string")),
            }
        }
    }
    if let Some(value) = object.get("platforms") {
        match json_string_list(value) {
            Some(values) => platforms.extend(values),
            None => losses.push("binary metadata `platforms` must be strings".to_string()),
        }
    }
    platforms.sort();
    platforms.dedup();
    facts.platforms = platforms;
    if facts.platforms.is_empty() {
        losses.push("binary metadata has no target platform".to_string());
    }
    facts.dependencies = provider_dependency_field(
        &mut facts,
        &object,
        "dependencies",
        "runtime",
        "binary",
        &mut losses,
    );
    facts.build_dependencies = provider_dependency_field(
        &mut facts,
        &object,
        "build_dependencies",
        "build",
        "binary",
        &mut losses,
    );
    facts.dev_dependencies = provider_dependency_field(
        &mut facts,
        &object,
        "dev_dependencies",
        "dev",
        "binary",
        &mut losses,
    );
    if let Some(value) = object.get("bins") {
        match json_string_list(value) {
            Some(values) => facts.bins = values,
            None => losses.push("binary metadata `bins` must be strings".to_string()),
        }
    }
    for (field, target) in [
        ("license", "provider.binary.license"),
        ("url", "provider.binary.source.url"),
        ("urls", "provider.binary.source.urls"),
        ("source", "provider.binary.source.identity"),
        ("owner", "provider.binary.source.owner"),
        ("signature", "provider.binary.signature"),
        ("signatures", "provider.binary.signature.set"),
        ("provenance", "provider.binary.provenance"),
        ("sbom", "provider.binary.sbom"),
        ("advisories", "provider.binary.advisories"),
        ("yanked", "provider.binary.yanked"),
        ("revoked", "provider.binary.revoked"),
        ("variants", "provider.binary.variants"),
        ("hooks", "provider.binary.hooks"),
    ] {
        if let Some(value) = object.get(field) {
            add_typed_json_fact(&mut facts, target, value);
        }
    }
    if let Some(value) = object.get("artifacts") {
        binary_artifact_facts(&mut facts, value, &mut losses);
    }
    facts.source_identity = format!("binary:{}@{}", facts.name, facts.integrity_hash);
    let mut report = report_with_identity(facts);
    report.losses.extend(losses);
    report.conflicts.extend(conflicts);
    report
}

fn nix_report(document: &str) -> ProviderFactReport {
    let parsed = match JSON::parse(document) {
        Ok(value) => value,
        Err(_) => {
            return empty_report(
                ProviderFamily::Nix,
                "nix",
                "Nix provider metadata is not valid JSON",
            )
        }
    };
    match parsed {
        JSONValue::Object(object) => nix_object_report(&object),
        JSONValue::Array(entries) => {
            let Some(JSONValue::Object(first)) = entries.first() else {
                return empty_report(
                    ProviderFamily::Nix,
                    "nix",
                    "Nix provider metadata entries must be JSON objects",
                );
            };
            let mut report = nix_object_report(first);
            for (index, entry) in entries.iter().enumerate().skip(1) {
                let JSONValue::Object(other) = entry else {
                    report.losses.push(format!(
                        "Nix provider metadata entry {} is not a JSON object",
                        index + 1
                    ));
                    continue;
                };
                if nix_identity_key(first) != nix_identity_key(other) {
                    report.losses.push(format!(
                        "Nix provider metadata contains multiple package identities; entry {} needs its own lock record",
                        index + 1
                    ));
                } else if other != first {
                    report.conflicts.push(format!(
                        "Nix provider metadata entry {} conflicts with the first native record",
                        index + 1
                    ));
                }
            }
            report
        }
        _ => empty_report(
            ProviderFamily::Nix,
            "nix",
            "Nix provider metadata must be a JSON object or realization array",
        ),
    }
}

fn nix_object_report(object: &std::collections::BTreeMap<String, JSONValue>) -> ProviderFactReport {
    let meta = object.get("meta").and_then(|value| value.as_object().ok());
    let locked = object
        .get("locked")
        .and_then(|value| value.as_object().ok());
    let mut losses = Vec::new();
    let mut conflicts = Vec::new();
    let mut facts = MetadataFacts::empty(ProviderFamily::Nix, "");
    add_json_projection(&mut facts, "provider.nix.native", object);

    if let Some(meta) = meta {
        add_json_projection(&mut facts, "provider.nix.native.meta", meta);
    } else if object.contains_key("meta") {
        losses.push("Nix `meta` must be a JSON object".to_string());
    }
    if let Some(locked) = locked {
        add_json_projection(&mut facts, "provider.nix.locked", locked);
        if locked_source_identity(Some(locked)).is_none() {
            losses.push(
                "Nix `locked` source facts have no complete immutable owner/revision identity"
                    .to_string(),
            );
        }
    } else if object.contains_key("locked") {
        losses.push("Nix `locked` source facts must be a JSON object".to_string());
    }

    let top_name = nix_string_field(object, "name", "name", &mut losses);
    let pname = nix_string_field(object, "pname", "pname", &mut losses);
    let package = nix_string_field(object, "package", "package", &mut losses);
    let meta_name =
        meta.and_then(|value| nix_string_field(value, "name", "meta.name", &mut losses));
    nix_conflicting_values(
        &mut conflicts,
        "Nix metadata declares conflicting package names",
        [
            ("pname", pname.clone()),
            ("package", package.clone()),
            ("meta.name", meta_name.clone()),
        ],
    );
    if let (Some(top_name), Some(pname)) = (top_name.as_deref(), pname.as_deref()) {
        if top_name != pname && nix_version_suffix(top_name, pname).is_none() {
            conflicts.push(format!(
                "Nix metadata declares conflicting package names: name={top_name} disagrees with pname={pname}"
            ));
        }
    }

    let top_version = nix_string_field(object, "version", "version", &mut losses);
    let meta_version =
        meta.and_then(|value| nix_string_field(value, "version", "meta.version", &mut losses));
    nix_conflicting_values(
        &mut conflicts,
        "Nix metadata declares conflicting package versions",
        [
            ("version", top_version.clone()),
            ("meta.version", meta_version.clone()),
        ],
    );

    let drv_path = nix_string_field(object, "drvPath", "drvPath", &mut losses)
        .or_else(|| nix_string_field(object, "derivationPath", "derivationPath", &mut losses));
    let output_path = nix_output_path(object, &mut losses);
    let path_identity = output_path
        .as_deref()
        .or(drv_path.as_deref())
        .and_then(nix_path_identity);
    let raw_name = pname
        .clone()
        .or(meta_name.clone())
        .or(package.clone())
        .or(top_name.clone());
    let (path_name, path_version) = path_identity.unwrap_or_default();
    let path_name = (!path_name.is_empty()).then_some(path_name);
    let path_version = (!path_version.is_empty()).then_some(path_version);
    let selected_name = if pname.is_none() && meta_name.is_none() && package.is_none() {
        path_name.clone().or(raw_name)
    } else {
        raw_name.or(path_name)
    };
    facts.name = selected_name
        .unwrap_or_default()
        .trim_end_matches(".drv")
        .to_string();
    facts.version = top_version
        .or(meta_version)
        .or(path_version)
        .or_else(|| {
            top_name
                .as_deref()
                .and_then(|name| nix_version_suffix(name, facts.name.as_str()))
        })
        .unwrap_or_default();

    let top_hash = nix_hash_field(object, &mut losses);
    let output_hash = object
        .get("outputs")
        .and_then(|value| value.as_object().ok())
        .and_then(|outputs| outputs.get("out").or_else(|| outputs.get("bin")))
        .and_then(|value| value.as_object().ok())
        .and_then(|output| {
            ["narHash", "outputHash", "hash", "sha256"]
                .iter()
                .find_map(|key| {
                    nix_string_field(output, key, &format!("outputs.out.{key}"), &mut losses)
                })
        });
    nix_conflicting_values(
        &mut conflicts,
        "Nix metadata declares conflicting integrity hashes",
        [
            ("native", top_hash.clone()),
            ("outputs.out", output_hash.clone()),
        ],
    );
    facts.integrity_hash = top_hash.or(output_hash).unwrap_or_default();

    nix_dependencies(
        &mut facts,
        object,
        &mut losses,
        &[
            ("dependencies", "runtime"),
            ("runtimeInputs", "runtime"),
            ("propagatedBuildInputs", "propagated"),
            ("devDependencies", "dev"),
            ("devInputs", "dev"),
            ("buildInputs", "build"),
            ("nativeBuildInputs", "native-build"),
            ("checkInputs", "check"),
            ("inputDrvs", "input"),
        ],
    );
    nix_hooks(&mut facts, object, &mut losses);
    nix_variants(&mut facts, object, meta, &mut losses);
    nix_license(&mut facts, object, meta, &mut losses);
    nix_signatures(&mut facts, object, &mut losses);
    nix_advisories(&mut facts, object, meta, &mut losses);
    nix_sources(&mut facts, object, locked, drv_path.as_deref(), &mut losses);

    if let Some(JSONValue::Object(nodes)) = object.get("nodes") {
        add_json_projection(&mut facts, "provider.nix.lock.nodes", nodes);
        for (name, node) in nodes {
            add_typed_json_fact(&mut facts, format!("provider.nix.lock.node.{name}"), node);
            let JSONValue::Object(node) = node else {
                losses.push(format!(
                    "Nix flake lock node `{name}` must be a JSON object"
                ));
                continue;
            };
            let Some(locked) = node.get("locked") else {
                continue;
            };
            let Ok(locked) = locked.as_object() else {
                losses.push(format!(
                    "Nix flake lock node `{name}` `locked` facts must be a JSON object"
                ));
                continue;
            };
            if locked_source_identity(Some(locked)).is_none() {
                losses.push(format!(
                    "Nix flake lock node `{name}` has no complete immutable owner/revision identity"
                ));
            }
            add_json_projection(
                &mut facts,
                &format!("provider.nix.source.locked.{name}"),
                locked,
            );
        }
        if nodes.len() > 1 && (facts.name.is_empty() || facts.version.is_empty()) {
            facts.name = "nix-lock".to_string();
            facts.version = "set".to_string();
            losses.push(
                "Nix flake lock contains multiple source nodes; select one exact package identity"
                    .to_string(),
            );
        }
    } else if object.contains_key("nodes") {
        losses.push("Nix flake lock `nodes` must be a JSON object".to_string());
    }
    if let Some(value) = object.get("packages") {
        add_typed_json_fact(&mut facts, "provider.nix.packages", value);
        match value {
            JSONValue::Array(packages) => {
                for package in packages {
                    match package {
                        JSONValue::String(name) => facts.dependencies.push(name.clone()),
                        JSONValue::Object(package) => {
                            if let Some(name) = json_string(package, "name") {
                                facts.dependencies.push(name.clone());
                                add_typed_json_fact(
                                    &mut facts,
                                    format!("provider.nix.package.{name}"),
                                    package.get("version").unwrap_or(value),
                                );
                            } else {
                                losses.push(
                                    "Nix package entry has no exact package name".to_string(),
                                );
                            }
                        }
                        _ => losses.push(
                            "Nix package entries must be strings or JSON objects".to_string(),
                        ),
                    }
                }
            }
            _ => losses.push("Nix `packages` must be an array".to_string()),
        }
    }

    let source_identity = nix_source_identity(object, locked, drv_path.as_deref());
    let has_immutable_source = source_identity.is_some();
    facts.source_identity = source_identity.unwrap_or_else(|| {
        if facts.name.is_empty() || facts.version.is_empty() {
            String::new()
        } else {
            format!("nix:{}@{}", facts.name, facts.version)
        }
    });
    if !has_immutable_source
        && (facts.name.is_empty()
            || facts.version.is_empty()
            || pname.is_some()
            || package.is_some()
            || meta_name.is_some())
    {
        losses.push("Nix metadata has no immutable source identity".to_string());
    }
    let mut report = report_with_identity(facts);
    report.losses.append(&mut losses);
    report.conflicts.append(&mut conflicts);
    report
}

fn nix_identity_key(
    object: &std::collections::BTreeMap<String, JSONValue>,
) -> (Option<String>, Option<String>, Option<String>) {
    (
        object
            .get("pname")
            .and_then(|value| value.as_str().ok())
            .map(str::to_string)
            .or_else(|| {
                object
                    .get("name")
                    .and_then(|value| value.as_str().ok())
                    .map(str::to_string)
            }),
        object
            .get("version")
            .and_then(|value| value.as_str().ok())
            .map(str::to_string),
        object
            .get("drvPath")
            .and_then(|value| value.as_str().ok())
            .map(str::to_string)
            .or_else(|| {
                object
                    .get("narHash")
                    .and_then(|value| value.as_str().ok())
                    .map(str::to_string)
            }),
    )
}

fn nix_string_field(
    object: &std::collections::BTreeMap<String, JSONValue>,
    key: &str,
    label: &str,
    losses: &mut Vec<String>,
) -> Option<String> {
    match object.get(key) {
        None => None,
        Some(JSONValue::String(value)) if !value.trim().is_empty() => Some(value.clone()),
        Some(JSONValue::String(_)) => {
            losses.push(format!("Nix `{label}` must not be empty"));
            None
        }
        Some(_) => {
            losses.push(format!("Nix `{label}` must be a string"));
            None
        }
    }
}

fn nix_conflicting_values<const N: usize>(
    conflicts: &mut Vec<String>,
    message: &str,
    values: [(&str, Option<String>); N],
) {
    let mut selected: Option<(&str, String)> = None;
    for (label, value) in values
        .into_iter()
        .filter_map(|(label, value)| value.map(|value| (label, value)))
    {
        if let Some((selected_label, selected_value)) = selected.as_ref() {
            if selected_value != &value {
                conflicts.push(format!(
                    "{message}: {selected_label}={selected_value} disagrees with {label}={value}"
                ));
            }
        } else {
            selected = Some((label, value));
        }
    }
}

fn nix_output_path(
    object: &std::collections::BTreeMap<String, JSONValue>,
    losses: &mut Vec<String>,
) -> Option<String> {
    let Some(outputs) = object.get("outputs") else {
        return None;
    };
    let Ok(outputs) = outputs.as_object() else {
        losses.push("Nix `outputs` must be a JSON object".to_string());
        return None;
    };
    let key = if outputs.contains_key("out") {
        "out"
    } else {
        "bin"
    };
    nix_string_field(outputs, key, &format!("outputs.{key}"), losses)
}

fn nix_hash_field(
    object: &std::collections::BTreeMap<String, JSONValue>,
    losses: &mut Vec<String>,
) -> Option<String> {
    for key in ["narHash", "outputHash", "hash", "sha256", "contentHash"] {
        if object.contains_key(key) {
            let hash = nix_string_field(object, key, key, losses);
            if let Some(hash) = &hash {
                if !ProviderSelector::parse(&format!("#digest={hash}")).is_exact() {
                    losses.push(format!("Nix `{key}` is not an exact digest"));
                }
            }
            return hash;
        }
    }
    None
}

fn nix_path_identity(path: &str) -> Option<(String, String)> {
    let base = path.rsplit('/').next()?.trim_end_matches(".drv");
    let payload = base.split_once('-')?.1;
    let payload = payload.strip_suffix("-bin").unwrap_or(payload);
    let mut candidate = None;
    for (index, _) in payload.match_indices('-') {
        let suffix = &payload[index + 1..];
        if suffix
            .chars()
            .next()
            .is_some_and(|value| value.is_ascii_digit())
        {
            candidate = Some((payload[..index].to_string(), suffix.to_string()));
            break;
        }
    }
    candidate.or_else(|| Some((payload.to_string(), String::new())))
}

fn nix_version_suffix(name: &str, package: &str) -> Option<String> {
    name.strip_prefix(&format!("{package}-"))
        .filter(|value| {
            value
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_digit())
        })
        .map(str::to_string)
}

fn nix_dependency_values(value: &JSONValue, losses: &mut Vec<String>, label: &str) -> Vec<String> {
    match value {
        JSONValue::String(value) => vec![value.clone()],
        JSONValue::Array(values) => values
            .iter()
            .filter_map(|value| match value {
                JSONValue::String(value) => Some(value.clone()),
                JSONValue::Object(object) => json_string(object, "name")
                    .or_else(|| json_string(object, "drvPath"))
                    .or_else(|| {
                        losses.push(format!(
                            "Nix `{label}` contains an object without a name or drvPath"
                        ));
                        None
                    }),
                _ => {
                    losses.push(format!("Nix `{label}` entries must be strings or objects"));
                    None
                }
            })
            .collect(),
        JSONValue::Object(values) => {
            if label == "inputDrvs"
                && values.values().any(|value| {
                    !matches!(
                        value,
                        JSONValue::Array(outputs)
                            if outputs
                                .iter()
                                .all(|output| matches!(output, JSONValue::String(_)))
                    )
                })
            {
                losses.push("Nix `inputDrvs` values must be arrays of output names".to_string());
            }
            values.keys().cloned().collect()
        }
        _ => {
            losses.push(format!("Nix `{label}` must be a string, array, or object"));
            Vec::new()
        }
    }
}

fn nix_dependencies(
    facts: &mut MetadataFacts,
    object: &std::collections::BTreeMap<String, JSONValue>,
    losses: &mut Vec<String>,
    fields: &[(&str, &str)],
) {
    for (field, kind) in fields {
        let Some(value) = object.get(*field) else {
            continue;
        };
        add_typed_json_fact(facts, format!("provider.nix.dependency.{kind}"), value);
        let values = nix_dependency_values(value, losses, field);
        match *kind {
            "runtime" | "propagated" => facts.dependencies.extend(values),
            "dev" => facts.dev_dependencies.extend(values),
            _ => facts.build_dependencies.extend(values),
        }
    }
    for values in [
        &mut facts.dependencies,
        &mut facts.dev_dependencies,
        &mut facts.build_dependencies,
    ] {
        values.sort();
        values.dedup();
    }
}

fn nix_hooks(
    facts: &mut MetadataFacts,
    object: &std::collections::BTreeMap<String, JSONValue>,
    losses: &mut Vec<String>,
) {
    for key in [
        "builder",
        "args",
        "buildCommand",
        "shellHook",
        "setupHook",
        "hooks",
        "configurePhase",
        "buildPhase",
        "checkPhase",
        "installPhase",
        "postInstall",
    ] {
        if let Some(value) = object.get(key) {
            add_typed_json_fact(facts, format!("provider.nix.hook.{key}"), value);
            let valid = match key {
                "args" => match value {
                    JSONValue::String(_) => true,
                    JSONValue::Array(values) => values
                        .iter()
                        .all(|value| matches!(value, JSONValue::String(_))),
                    _ => false,
                },
                "hooks" => matches!(
                    value,
                    JSONValue::String(_)
                        | JSONValue::Array(_)
                        | JSONValue::Object(_)
                ),
                _ => matches!(value, JSONValue::String(_)),
            };
            if !valid {
                losses.push(format!("Nix `{key}` hook has an unsupported shape"));
            }
            facts.scripts.push(key.to_string());
        }
    }
    facts.scripts.sort();
    facts.scripts.dedup();
}

fn nix_variants(
    facts: &mut MetadataFacts,
    object: &std::collections::BTreeMap<String, JSONValue>,
    meta: Option<&std::collections::BTreeMap<String, JSONValue>>,
    losses: &mut Vec<String>,
) {
    for key in [
        "system",
        "platform",
        "hostPlatform",
        "buildPlatform",
        "targetPlatform",
        "crossSystem",
        "features",
        "platforms",
        "outputs",
        "variants",
    ] {
        if let Some(value) = object.get(key) {
            add_typed_json_fact(facts, format!("provider.nix.variant.{key}"), value);
            let values = nix_string_list_value(value, key, losses);
            facts.platforms.extend(values);
        }
    }
    if let Some(meta) = meta {
        if let Some(value) = meta.get("platforms") {
            add_typed_json_fact(facts, "provider.nix.variant.meta.platforms", value);
            facts
                .platforms
                .extend(nix_string_list_value(value, "meta.platforms", losses));
        }
    }
    facts.platforms.sort();
    facts.platforms.dedup();
}

fn nix_string_list_value(value: &JSONValue, label: &str, losses: &mut Vec<String>) -> Vec<String> {
    match value {
        JSONValue::String(value) => vec![value.clone()],
        JSONValue::Array(values) => values
            .iter()
            .filter_map(|value| match value {
                JSONValue::String(value) => Some(value.clone()),
                _ => {
                    losses.push(format!("Nix `{label}` entries must be strings"));
                    None
                }
            })
            .collect(),
        JSONValue::Object(_)
            if matches!(
                label,
                "crossSystem"
                    | "features"
                    | "outputs"
                    | "variants"
                    | "signature"
                    | "signatures"
                    | "publicKey"
                    | "public_key"
                    | "signingKey"
                    | "signedBy"
                    | "trustedKeys"
            ) =>
        {
            Vec::new()
        }
        _ => {
            losses.push(format!(
                "Nix `{label}` must be a string or array of strings"
            ));
            Vec::new()
        }
    }
}

fn nix_license(
    facts: &mut MetadataFacts,
    object: &std::collections::BTreeMap<String, JSONValue>,
    meta: Option<&std::collections::BTreeMap<String, JSONValue>>,
    losses: &mut Vec<String>,
) {
    let license = object
        .get("license")
        .or_else(|| meta.and_then(|value| value.get("license")));
    if let Some(value) = license {
        add_typed_json_fact(facts, "provider.nix.license", value);
        if !matches!(
            value,
            JSONValue::String(_) | JSONValue::Object(_) | JSONValue::Array(_)
        ) {
            losses.push("Nix `license` must be a string, object, or array".to_string());
        }
        facts.license = json_value_text(value);
    }
}

fn nix_signatures(
    facts: &mut MetadataFacts,
    object: &std::collections::BTreeMap<String, JSONValue>,
    losses: &mut Vec<String>,
) {
    for key in [
        "signature",
        "signatures",
        "publicKey",
        "public_key",
        "signingKey",
        "signedBy",
        "trustedKeys",
    ] {
        if let Some(value) = object.get(key) {
            add_typed_json_fact(facts, format!("provider.nix.signature.{key}"), value);
            facts
                .trust_roots
                .extend(nix_string_list_value(value, key, losses));
        }
    }
    facts.trust_roots.sort();
    facts.trust_roots.dedup();
}

fn nix_advisories(
    facts: &mut MetadataFacts,
    object: &std::collections::BTreeMap<String, JSONValue>,
    meta: Option<&std::collections::BTreeMap<String, JSONValue>>,
    losses: &mut Vec<String>,
) {
    for key in ["yanked", "broken", "insecure", "unfree", "available"] {
        if let Some(value) = object.get(key).or_else(|| meta.and_then(|m| m.get(key))) {
            add_typed_json_fact(facts, format!("provider.nix.advisory.{key}"), value);
            if !matches!(value, JSONValue::Bool(_)) {
                losses.push(format!("Nix `{key}` advisory flag must be a boolean"));
            }
        }
    }
    for key in ["advisories", "knownVulnerabilities", "vulnerabilities"] {
        if let Some(value) = object.get(key).or_else(|| meta.and_then(|m| m.get(key))) {
            add_typed_json_fact(facts, format!("provider.nix.advisory.{key}"), value);
            if !matches!(value, JSONValue::String(_) | JSONValue::Array(_)) {
                losses.push(format!("Nix `{key}` advisories must be a string or array"));
            }
            if let Some(values) = json_string_list(value) {
                facts.todos.extend(values);
            }
        }
    }
}

fn nix_sources(
    facts: &mut MetadataFacts,
    object: &std::collections::BTreeMap<String, JSONValue>,
    locked: Option<&std::collections::BTreeMap<String, JSONValue>>,
    drv_path: Option<&str>,
    _losses: &mut Vec<String>,
) {
    for key in [
        "source",
        "sourceInfo",
        "sourceProvenance",
        "origin",
        "owner",
        "repository",
        "repo",
        "url",
        "homepage",
        "downloadPage",
    ] {
        if let Some(value) = object.get(key) {
            add_typed_json_fact(facts, format!("provider.nix.source.{key}"), value);
        }
    }
    if let Some(locked) = locked {
        for key in ["type", "owner", "repo", "rev", "narHash", "lastModified"] {
            if let Some(value) = locked.get(key) {
                add_typed_json_fact(facts, format!("provider.nix.source.locked.{key}"), value);
            }
        }
    }
    if let Some(drv_path) = drv_path {
        add_typed_text_fact(facts, "provider.nix.source.drv_path", drv_path);
    }
}

fn nix_source_identity(
    object: &std::collections::BTreeMap<String, JSONValue>,
    locked: Option<&std::collections::BTreeMap<String, JSONValue>>,
    drv_path: Option<&str>,
) -> Option<String> {
    for key in ["sourceIdentity", "immutableSource", "sourcePath", "flake"] {
        if let Some(value) = object.get(key).and_then(|value| value.as_str().ok()) {
            if !value.trim().is_empty() {
                return Some(value.to_string());
            }
        }
    }
    if let Some(identity) = locked_source_identity(locked) {
        return Some(identity);
    }
    let root = object
        .get("root")
        .and_then(|value| value.as_str().ok())
        .unwrap_or("root");
    if let Some(JSONValue::Object(nodes)) = object.get("nodes") {
        if let Some(JSONValue::Object(node)) = nodes.get(root) {
            if let Some(locked) = node.get("locked").and_then(|value| value.as_object().ok()) {
                if let Some(identity) = locked_source_identity(Some(locked)) {
                    return Some(identity);
                }
            }
        }
    }
    drv_path.map(|path| format!("nix:drv:{path}"))
}

fn locked_source_identity(
    locked: Option<&std::collections::BTreeMap<String, JSONValue>>,
) -> Option<String> {
    let locked = locked?;
    let kind = json_string(locked, "type").unwrap_or_else(|| "flake".to_string());
    let owner = json_string(locked, "owner");
    let repo = json_string(locked, "repo");
    let revision = if let Some(revision) = json_string(locked, "rev") {
        ProviderSelector::parse(&format!("#revision={revision}"))
            .is_exact()
            .then_some(revision)
    } else if let Some(hash) = json_string(locked, "narHash") {
        ProviderSelector::parse(&format!("#digest={hash}"))
            .is_exact()
            .then_some(hash)
    } else {
        None
    };
    let name = match (owner, repo) {
        (Some(owner), Some(repo)) => format!("{owner}/{repo}"),
        (Some(owner), None) | (None, Some(owner)) => owner,
        (None, None) => String::new(),
    };
    match (name.is_empty(), revision) {
        (false, Some(revision)) if !revision.trim().is_empty() => {
            Some(format!("{kind}:{name}@{revision}"))
        }
        _ => None,
    }
}

fn provider_dependency_field(
    facts: &mut MetadataFacts,
    object: &std::collections::BTreeMap<String, JSONValue>,
    field: &str,
    kind: &str,
    namespace: &str,
    losses: &mut Vec<String>,
) -> Vec<String> {
    let Some(value) = object.get(field) else {
        return Vec::new();
    };
    let Some(values) = value.as_array().ok() else {
        losses.push(format!("{namespace} `{field}` must be an array"));
        return Vec::new();
    };
    let mut dependencies = Vec::new();
    for (index, value) in values.iter().enumerate() {
        add_typed_json_fact(
            facts,
            format!("provider.{namespace}.dependency.{kind}.{index}"),
            value,
        );
        match value {
            JSONValue::String(name) if !name.trim().is_empty() => {
                dependencies.push(name.clone());
                add_typed_json_fact(
                    facts,
                    format!("provider.{namespace}.dependency.{kind}.{name}"),
                    value,
                );
            }
            JSONValue::Object(dependency) => {
                let Some(name) =
                    json_string(dependency, "name").or_else(|| json_string(dependency, "id"))
                else {
                    losses.push(format!(
                        "{namespace} `{field}[{index}]` has no dependency name"
                    ));
                    continue;
                };
                let requirement = ["version", "version_requirement", "requirement"]
                    .iter()
                    .find_map(|key| json_string(dependency, key))
                    .unwrap_or_default();
                dependencies.push(format_dependency(&name, &requirement));
                add_typed_json_fact(
                    facts,
                    format!("provider.{namespace}.dependency.{kind}.{name}"),
                    value,
                );
            }
            _ => losses.push(format!(
                "{namespace} `{field}[{index}]` must be a dependency name or object"
            )),
        }
    }
    dependencies
}

fn add_homebrew_named_facts(
    facts: &mut MetadataFacts,
    object: &std::collections::BTreeMap<String, JSONValue>,
) {
    for (field, category) in [
        ("homepage", "source"),
        ("tap", "source"),
        ("full_name", "source"),
        ("urls", "source"),
        ("source", "source"),
        ("head", "source"),
        ("bottle", "variant"),
        ("variations", "variant"),
        ("keg_only", "variant"),
        ("relocatable", "variant"),
        ("cellar", "variant"),
        ("prefix", "variant"),
        ("install", "hook"),
        ("post_install", "hook"),
        ("service", "hook"),
        ("test", "hook"),
        ("caveats", "hook"),
        ("deprecated", "advisory"),
        ("disabled", "advisory"),
        ("deprecation_date", "advisory"),
        ("disable_date", "advisory"),
        ("replacement", "advisory"),
    ] {
        if let Some(value) = object.get(field) {
            add_typed_json_fact(
                facts,
                format!("provider.homebrew.{category}.{field}"),
                value,
            );
        }
    }
}

fn homebrew_bottle_facts(facts: &mut MetadataFacts, value: &JSONValue, losses: &mut Vec<String>) {
    let Some(bottle) = value.as_object().ok() else {
        losses.push("Homebrew `bottle` must be an object".to_string());
        return;
    };
    let Some(stable) = bottle.get("stable") else {
        losses.push("Homebrew `bottle` has no stable artifact set".to_string());
        return;
    };
    let Some(stable) = stable.as_object().ok() else {
        losses.push("Homebrew `bottle.stable` must be an object".to_string());
        return;
    };
    let files = stable
        .get("files")
        .and_then(|value| value.as_object().ok())
        .unwrap_or(stable);
    for (platform, artifact) in files {
        let Some(artifact) = artifact.as_object().ok() else {
            if platform != "rebuild" {
                losses.push(format!(
                    "Homebrew bottle entry `{platform}` must be an object"
                ));
            }
            continue;
        };
        add_typed_json_fact(
            facts,
            format!("provider.homebrew.bottle.{platform}"),
            &JSONValue::Object(artifact.clone()),
        );
        let Some(hash) = json_string(artifact, "sha256") else {
            losses.push(format!(
                "Homebrew bottle entry `{platform}` has no content hash"
            ));
            continue;
        };
        if !ProviderSelector::parse(&format!("#digest={hash}")).is_exact() {
            losses.push(format!(
                "Homebrew bottle entry `{platform}` has a non-exact digest"
            ));
        }
        add_typed_text_fact(
            facts,
            format!("provider.homebrew.bottle.{platform}.sha256"),
            &hash,
        );
    }
}

fn homebrew_source_hash(
    object: &std::collections::BTreeMap<String, JSONValue>,
    losses: &mut Vec<String>,
) -> Option<String> {
    let mut hashes = Vec::new();
    for (field, value) in [
        ("sha256", object.get("sha256")),
        ("source", object.get("source")),
    ] {
        let Some(value) = value else {
            continue;
        };
        let hash = if field == "sha256" {
            match value {
                JSONValue::String(hash) if !hash.trim().is_empty() => Some(hash.clone()),
                JSONValue::String(_) => {
                    losses.push("Homebrew `sha256` must not be empty".to_string());
                    None
                }
                _ => {
                    losses.push("Homebrew `sha256` must be a string".to_string());
                    None
                }
            }
        } else {
            let Some(source) = value.as_object().ok() else {
                losses.push("Homebrew `source` must be an object".to_string());
                continue;
            };
            match source.get("sha256") {
                Some(JSONValue::String(hash)) if !hash.trim().is_empty() => Some(hash.clone()),
                Some(JSONValue::String(_)) => {
                    losses.push("Homebrew `source.sha256` must not be empty".to_string());
                    None
                }
                Some(_) => {
                    losses.push("Homebrew `source.sha256` must be a string".to_string());
                    None
                }
                None => None,
            }
        };
        if let Some(hash) = hash {
            if !ProviderSelector::parse(&format!("#digest={hash}")).is_exact() {
                losses.push(format!("Homebrew {field} hash is not an exact digest"));
            }
            hashes.push(hash);
        }
    }
    if let Some(urls_value) = object.get("urls") {
        let Some(urls) = urls_value.as_object().ok() else {
            losses.push("Homebrew `urls` must be an object".to_string());
            return hashes.into_iter().next();
        };
        if let Some(stable_value) = urls.get("stable") {
            let Some(stable) = stable_value.as_object().ok() else {
                losses.push("Homebrew `urls.stable` must be an object".to_string());
                return hashes.into_iter().next();
            };
            if let Some(hash) = json_string(stable, "sha256") {
                if !ProviderSelector::parse(&format!("#digest={hash}")).is_exact() {
                    losses.push("Homebrew `urls.stable.sha256` is not an exact digest".to_string());
                }
                hashes.push(hash);
            } else if stable.contains_key("sha256") {
                losses.push("Homebrew `urls.stable.sha256` must be a string".to_string());
            }
        }
    }
    if hashes.windows(2).any(|pair| pair[0] != pair[1]) {
        losses.push("Homebrew source metadata declares conflicting hashes".to_string());
    }
    hashes.into_iter().next()
}

fn github_asset_facts(facts: &mut MetadataFacts, value: &JSONValue, losses: &mut Vec<String>) {
    let Some(assets) = value.as_array().ok() else {
        losses.push("GitHub `assets` must be an array".to_string());
        return;
    };
    for (index, value) in assets.iter().enumerate() {
        let Some(asset) = value.as_object().ok() else {
            losses.push(format!("GitHub asset {index} must be an object"));
            continue;
        };
        let Some(name) = json_string(asset, "name") else {
            losses.push(format!("GitHub asset {index} has no name"));
            continue;
        };
        add_json_projection(facts, &format!("provider.github.asset.{name}"), asset);
        if let Some(platform) = json_string(asset, "platform") {
            facts.platforms.push(platform);
        }
        let digest = asset
            .get("digest")
            .or_else(|| asset.get("sha256"))
            .or_else(|| asset.get("hash"));
        match digest {
            Some(JSONValue::String(digest))
                if ProviderSelector::parse(&format!("#digest={digest}")).is_exact() => {}
            Some(JSONValue::String(_)) => {
                losses.push(format!("GitHub asset `{name}` has a non-exact digest"))
            }
            Some(_) => losses.push(format!("GitHub asset `{name}` digest must be a string")),
            None => losses.push(format!("GitHub asset `{name}` has no content digest")),
        }
    }
}

fn binary_artifact_facts(facts: &mut MetadataFacts, value: &JSONValue, losses: &mut Vec<String>) {
    let Some(artifacts) = value.as_object().ok() else {
        losses.push("binary metadata `artifacts` must be an object".to_string());
        return;
    };
    for (platform, artifact) in artifacts {
        facts.platforms.push(platform.clone());
        let Some(artifact) = artifact.as_object().ok() else {
            losses.push(format!("binary artifact `{platform}` must be an object"));
            continue;
        };
        add_json_projection(
            facts,
            &format!("provider.binary.artifact.{platform}"),
            artifact,
        );
        if let Some(digest) = json_string(artifact, "sha256")
            .or_else(|| json_string(artifact, "hash"))
            .or_else(|| json_string(artifact, "digest"))
        {
            if !ProviderSelector::parse(&format!("#digest={digest}")).is_exact() {
                losses.push(format!(
                    "binary artifact `{platform}` has a non-exact digest"
                ));
            }
        } else {
            losses.push(format!("binary artifact `{platform}` has no content hash"));
        }
    }
}

fn report_with_identity(facts: MetadataFacts) -> ProviderFactReport {
    let mut losses = Vec::new();
    if facts.name.is_empty() {
        losses.push("provider metadata has no package name".to_string());
    }
    if metadata_identity_selector(&facts).1.is_empty() {
        losses.push("provider metadata has no exact version, revision, or digest".to_string());
    }
    ProviderFactReport {
        facts,
        losses,
        conflicts: Vec::new(),
        native_format: String::new(),
        native_document: String::new(),
    }
}

fn empty_report(family: ProviderFamily, name: &str, loss: &str) -> ProviderFactReport {
    let facts = MetadataFacts::empty(family, name);
    ProviderFactReport {
        facts,
        losses: vec![loss.to_string()],
        conflicts: Vec::new(),
        native_format: String::new(),
        native_document: String::new(),
    }
}

fn provider_document_format(family: &ProviderFamily, document: &str) -> String {
    match family {
        ProviderFamily::Npm
        | ProviderFamily::Vcpkg
        | ProviderFamily::Homebrew
        | ProviderFamily::JetRegistry
        | ProviderFamily::Github
        | ProviderFamily::Binary => "json".to_string(),
        ProviderFamily::SwiftPM => "json".to_string(),
        ProviderFamily::PyPI if JSON::parse(document).is_ok() => "json".to_string(),
        ProviderFamily::NuGet if JSON::parse(document).is_ok() => "json".to_string(),
        ProviderFamily::NuGet => "xml".to_string(),
        ProviderFamily::Conan if JSON::parse(document).is_ok() => "json".to_string(),
        ProviderFamily::Conan => "conan".to_string(),
        ProviderFamily::Nix if JSON::parse(document).is_ok() => "json".to_string(),
        ProviderFamily::Nix => "nix".to_string(),
        ProviderFamily::Cargo => "toml".to_string(),
        ProviderFamily::PyPI => "python-metadata".to_string(),
        ProviderFamily::Maven => "xml".to_string(),
        ProviderFamily::Core | ProviderFamily::Path => "provider".to_string(),
    }
}

fn json_string(
    object: &std::collections::BTreeMap<String, JSONValue>,
    key: &str,
) -> Option<String> {
    match object.get(key) {
        Some(JSONValue::String(value)) if !value.trim().is_empty() => Some(value.clone()),
        _ => None,
    }
}

fn json_keys(object: &std::collections::BTreeMap<String, JSONValue>, key: &str) -> Vec<String> {
    match object.get(key) {
        Some(JSONValue::Object(values)) => values.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

fn json_array_strings(
    object: &std::collections::BTreeMap<String, JSONValue>,
    key: &str,
) -> Vec<String> {
    match object.get(key) {
        Some(JSONValue::Array(values)) => values
            .iter()
            .filter_map(|value| match value {
                JSONValue::String(value) => Some(value.clone()),
                JSONValue::Object(value) => value.get("name").and_then(|value| match value {
                    JSONValue::String(value) => Some(value.clone()),
                    _ => None,
                }),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn vcpkg_dependencies(
    object: &std::collections::BTreeMap<String, JSONValue>,
    typed: &mut std::collections::BTreeMap<String, Vec<String>>,
    losses: &mut Vec<String>,
) -> Vec<String> {
    let Some(value) = object.get("dependencies") else {
        return Vec::new();
    };
    let Some(values) = value.as_array().ok() else {
        losses.push("vcpkg `dependencies` must be an array".to_string());
        return Vec::new();
    };
    let mut dependencies = Vec::new();
    for (index, value) in values.iter().enumerate() {
        match value {
            JSONValue::String(name) if !name.trim().is_empty() => dependencies.push(name.clone()),
            JSONValue::String(_) => {
                losses.push(format!("vcpkg dependency {index} has an empty name"));
            }
            JSONValue::Object(dependency) => {
                let Some(name) = json_string(dependency, "name") else {
                    losses.push(format!("vcpkg dependency {index} has no non-empty `name`"));
                    continue;
                };
                let version = ["version>=", "version>", "version"]
                    .iter()
                    .find_map(|key| json_string(dependency, key))
                    .unwrap_or_default();
                dependencies.push(format_dependency(&name, &version));
                for key in ["features", "platform", "host", "default-features"] {
                    if let Some(value) = dependency.get(key) {
                        typed.insert(
                            format!("vcpkg.dependency.{name}.{key}"),
                            vec![json_value_text(value)],
                        );
                        let valid = match key {
                            "features" => matches!(
                                value,
                                JSONValue::Array(values)
                                    if values.iter().all(|item| matches!(item, JSONValue::String(_)))
                            ),
                            "platform" => {
                                matches!(value, JSONValue::String(value) if !value.trim().is_empty())
                            }
                            "host" | "default-features" => matches!(value, JSONValue::Bool(_)),
                            _ => true,
                        };
                        if !valid {
                            losses.push(format!(
                                "vcpkg dependency `{name}` field `{key}` has an invalid shape"
                            ));
                        }
                    }
                }
                for key in ["version>=", "version>", "version"] {
                    if let Some(value) = dependency.get(key) {
                        if !matches!(value, JSONValue::String(_)) {
                            losses.push(format!(
                                "vcpkg dependency `{name}` field `{key}` must be a string"
                            ));
                        }
                    }
                }
            }
            _ => losses.push(format!(
                "vcpkg dependency {index} must be a string or object"
            )),
        }
    }
    dependencies
}

fn json_value_text(value: &JSONValue) -> String {
    match value {
        JSONValue::Null => "null".to_string(),
        JSONValue::Bool(value) => value.to_string(),
        JSONValue::Number(value) => value.to_string(),
        JSONValue::Flt(value) => value.to_string(),
        JSONValue::String(value) => value.clone(),
        JSONValue::Array(_) | JSONValue::Object(_) => json_value_json(value),
    }
}

fn json_value_json(value: &JSONValue) -> String {
    match value {
        JSONValue::Null => "null".to_string(),
        JSONValue::Bool(value) => value.to_string(),
        JSONValue::Number(value) => value.to_string(),
        JSONValue::Flt(value) => value.to_string(),
        JSONValue::String(value) => JSON::quote(value),
        JSONValue::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(json_value_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        JSONValue::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!("{}:{}", JSON::quote(key), json_value_json(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

// ponytail: bounded tag/attribute scan; add an XML parser if namespaces or
// entity decoding become part of provider identity.
fn xml_blocks(document: &str, tag: &str) -> Vec<(usize, String)> {
    let closing = format!("</{tag}>");
    xml_opening_tag_ranges(document, tag)
        .into_iter()
        .filter_map(|(start, end, opening)| {
            if opening.trim_end().ends_with("/>") {
                return None;
            }
            let body_start = end + 1;
            let close = document.get(body_start..)?.find(&closing)?;
            let body_end = body_start + close + closing.len();
            Some((start, document.get(start..body_end)?.to_string()))
        })
        .collect()
}

fn xml_direct_child_values(document: &str, parent: &str, child: &str) -> Vec<String> {
    let Some((_, parent_end, _)) = xml_opening_tag_ranges(document, parent).into_iter().next()
    else {
        return Vec::new();
    };
    let closing = format!("</{parent}>");
    let body_start = parent_end + 1;
    let Some(close) = document
        .get(body_start..)
        .and_then(|body| body.find(&closing))
    else {
        return Vec::new();
    };
    let body_end = body_start + close;
    let mut values = Vec::new();
    let mut depth = 0usize;
    let mut cursor = body_start;
    while cursor < body_end {
        let Some(relative_start) = document[cursor..body_end].find('<') else {
            break;
        };
        let start = cursor + relative_start;
        if document[start..].starts_with("<!--") {
            cursor = document[start + 4..body_end]
                .find("-->")
                .map(|end| start + 4 + end + 3)
                .unwrap_or(body_end);
            continue;
        }
        if document[start..].starts_with("<![CDATA[") {
            cursor = document[start + 9..body_end]
                .find("]]>")
                .map(|end| start + 9 + end + 3)
                .unwrap_or(body_end);
            continue;
        }
        let Some(relative_end) = xml_opening_tag_end(&document[start..body_end]) else {
            break;
        };
        let end = start + relative_end;
        let opening = &document[start..=end];
        if opening.starts_with("</") {
            depth = depth.saturating_sub(1);
            cursor = end + 1;
            continue;
        }
        if opening.starts_with("<?") || opening.starts_with("<!") {
            cursor = end + 1;
            continue;
        }
        let name = xml_opening_name(opening);
        let self_closing = opening.trim_end().ends_with("/>");
        if depth == 0 && name == child && !self_closing {
            let child_closing = format!("</{child}>");
            if let Some(relative_close) = document[end + 1..body_end].find(&child_closing) {
                let value_start = end + 1;
                values.push(xml_unescape(
                    document[value_start..value_start + relative_close].trim(),
                ));
                cursor = value_start + relative_close + child_closing.len();
                continue;
            }
        }
        if !self_closing {
            depth += 1;
        }
        cursor = end + 1;
    }
    values
}

fn xml_opening_name(opening: &str) -> &str {
    opening
        .strip_prefix('<')
        .unwrap_or(opening)
        .trim_start_matches('/')
        .split(|character: char| character.is_whitespace() || matches!(character, '>' | '/'))
        .next()
        .unwrap_or_default()
}

fn xml_has_namespace(document: &str) -> bool {
    if document.contains("xmlns:") || document.contains("xmlns=") {
        return true;
    }
    let mut cursor = 0;
    while cursor < document.len() {
        let Some(relative_start) = document[cursor..].find('<') else {
            break;
        };
        let start = cursor + relative_start;
        let Some(relative_end) = xml_opening_tag_end(&document[start..]) else {
            break;
        };
        let end = start + relative_end;
        let opening = &document[start..=end];
        if !opening.starts_with("<!--")
            && !opening.starts_with("<?")
            && !opening.starts_with("<!")
            && xml_opening_name(opening).contains(':')
        {
            return true;
        }
        cursor = end + 1;
    }
    false
}

fn xml_tag(document: &str, tag: &str) -> Option<String> {
    xml_tag_values(document, tag).into_iter().next()
}

fn xml_tag_values(document: &str, tag: &str) -> Vec<String> {
    let closing = format!("</{tag}>");
    xml_opening_tag_ranges(document, tag)
        .into_iter()
        .filter_map(|(_, end, opening)| {
            if opening.trim_end().ends_with("/>") {
                return None;
            }
            let value_start = end + 1;
            document
                .get(value_start..)?
                .split_once(&closing)
                .map(|(value, _)| xml_unescape(value.trim()))
        })
        .collect()
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        // Decode ampersands last so `&amp;lt;` remains the literal text `&lt;`.
        .replace("&amp;", "&")
}

fn xml_opening_tags(document: &str, tag: &str) -> Vec<String> {
    xml_opening_tag_ranges(document, tag)
        .into_iter()
        .map(|(_, _, opening)| opening)
        .collect()
}

fn xml_opening_tag_ranges(document: &str, tag: &str) -> Vec<(usize, usize, String)> {
    let needle = format!("<{tag}");
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while cursor < document.len() {
        let Some(relative_start) = document[cursor..].find(&needle) else {
            break;
        };
        let start = cursor + relative_start;
        let name_end = start + 1 + tag.len();
        let boundary = document[name_end..].chars().next();
        if !matches!(boundary, Some(value) if value.is_whitespace() || value == '>' || value == '/')
        {
            cursor = name_end;
            continue;
        }
        let Some(relative_end) = xml_opening_tag_end(&document[start..]) else {
            break;
        };
        let end = start + relative_end;
        ranges.push((start, end, document[start..=end].to_string()));
        cursor = end + 1;
    }
    ranges
}

fn xml_opening_tag_end(opening: &str) -> Option<usize> {
    let mut quote = None;
    for (index, character) in opening.char_indices() {
        match (quote, character) {
            (Some(expected), value) if expected == value => quote = None,
            (None, '\'' | '"') => quote = Some(character),
            (None, '>') => return Some(index),
            _ => {}
        }
    }
    None
}

fn xml_attribute(opening: &str, key: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let marker = format!("{key}={quote}");
        let mut cursor = 0;
        while cursor < opening.len() {
            let Some(relative) = opening[cursor..].find(&marker) else {
                break;
            };
            let start = cursor + relative;
            let valid_boundary = start == 0
                || opening[..start]
                    .chars()
                    .next_back()
                    .is_some_and(|value| value.is_whitespace() || value == '<' || value == '/');
            if valid_boundary {
                let value_start = start + marker.len();
                if let Some(value) = opening[value_start..].split_once(quote) {
                    return Some(value.0.to_string());
                }
            }
            cursor = start + marker.len();
        }
    }
    None
}

fn metadata_fields(document: &str) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in document.lines() {
        if line
            .chars()
            .next()
            .is_some_and(|character| character.is_whitespace())
        {
            if let Some((_, value)) = fields.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        let Some((left, right)) = line.split_once(':') else {
            continue;
        };
        let key = left.trim();
        if !key.is_empty() {
            fields.push((key.to_string(), right.trim().to_string()));
        }
    }
    fields
}

fn metadata_values(fields: &[(String, String)], key: &str) -> Vec<String> {
    fields
        .iter()
        .filter(|(field, _)| field.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.clone())
        .collect()
}

fn distinct_values(values: &[String]) -> Vec<String> {
    let mut distinct = Vec::new();
    for value in values {
        if !distinct.contains(value) {
            distinct.push(value.clone());
        }
    }
    distinct
}

fn format_dependency(name: &str, version: &str) -> String {
    let name = name.trim();
    let version = version.trim();
    match (name.is_empty(), version.is_empty()) {
        (true, _) => version.to_string(),
        (_, true) => name.to_string(),
        (false, false) => format!("{name}@{version}"),
    }
}

fn assignment_values(document: &str, key: &str) -> Vec<String> {
    document
        .lines()
        .filter_map(|line| {
            let (left, right) = line.split_once('=')?;
            let left = left.trim().strip_prefix("self.").unwrap_or(left.trim());
            if left != key {
                return None;
            }
            quoted_values_in(right).into_iter().next().or_else(|| {
                let value = right
                    .split('#')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .trim_end_matches(',');
                (!value.is_empty()).then(|| value.to_string())
            })
        })
        .collect()
}

fn quoted_values_after_all(document: &str, key: &str) -> Vec<String> {
    let self_key = format!("self.{key}");
    let mut values = Vec::new();
    for line in document.lines() {
        let line = line.trim_start();
        let rest = line
            .strip_prefix(key)
            .or_else(|| line.strip_prefix(&self_key));
        let Some(rest) = rest else {
            continue;
        };
        if !matches!(
            rest.chars().next(),
            None | Some(' ' | '\t' | '=' | '(' | ':')
        ) {
            continue;
        }
        values.extend(quoted_values_in(line));
    }
    values
}

fn quoted_values_in(text: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut quote = None;
    let mut value_start = 0;
    for (index, character) in text.char_indices() {
        match (quote, character) {
            (Some(expected), value) if expected == value => {
                values.push(text[value_start..index].to_string());
                quote = None;
            }
            (None, '\'' | '"') => {
                quote = Some(character);
                value_start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    values
}

fn conan_ref_part(reference: &str, index: usize) -> Option<String> {
    let value = reference.split('@').next()?.split('/').nth(index)?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn toml_string(raw: &str, key: &str) -> Option<String> {
    raw.lines().find_map(|line| {
        let line = line.trim();
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

fn dependency_keys(raw: &str, section: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_section = false;
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == section;
            continue;
        }
        if in_section {
            if let Some((key, _)) = line.split_once('=') {
                out.push(key.trim().to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{normalize_provider_document, ProviderFamily};
    use jet_pkg_model::ProviderFacts::{ProviderFactValue, ProviderFacts};

    #[test]
    fn lock_uses_canonical_carrier_and_retains_raw_reference() {
        let native = r#"{"name":"web","version":"1.0.0","dependencies":{"vite":"5"}}"#;
        let report = normalize_provider_document(ProviderFamily::Npm, native);
        report.validate().expect("lossless provider report");
        let shared = report.shared_facts();
        let lock = report
            .lock_record("app", "web#1.0.0@npm", "any")
            .expect("canonical provider lock");
        let locked = ProviderFacts::from_json(
            lock.future_fields
                .get("provider-facts")
                .expect("provider facts in lock"),
        )
        .expect("locked provider facts JSON");
        assert_eq!(locked, shared);
        assert_eq!(lock.rationales[0].source_ref, "web#1.0.0@npm".to_string());
    }

    #[test]
    fn unpinned_alias_lowers_resolved_selector_and_typed_native_facts() {
        let native = r#"{"name":"web","version":"1.0.0","yanked":false}"#;
        let report = normalize_provider_document(ProviderFamily::Npm, native);
        let shared = report.shared_facts_for("web@catalog");
        shared.validate().expect("provider alias remains lossless");
        assert_eq!(shared.qualified_reference(), "web#version=1.0.0@catalog");
        assert_eq!(
            shared.facts.get("provider.resolved_selector"),
            Some(&ProviderFactValue::Text("#version=1.0.0".to_string()))
        );
        assert_eq!(
            shared.facts.get("provider.npm.native.yanked"),
            Some(&ProviderFactValue::List(vec![ProviderFactValue::Bool(
                false
            )]))
        );

        let lock = report
            .lock_record("app", "web@catalog", "any")
            .expect("provider alias lock");
        assert_eq!(lock.identity.exact, "web#version=1.0.0@catalog");
        let locked = ProviderFacts::from_json(
            lock.future_fields
                .get("provider-facts")
                .expect("provider facts in lock"),
        )
        .expect("locked provider facts");
        locked.validate().expect("locked alias provider facts");
        assert_eq!(locked.reference, "web@catalog");
    }

    #[test]
    fn npm_conformance_retains_native_and_typed_provider_facts() {
        let native = r#"{"name":"web","version":"2.0.0","license":"MIT","dependencies":{"vite":"5.4.0"},"devDependencies":{"typescript":"5.5.0"},"optionalDependencies":{"fsevents":"2.3.3"},"peerDependencies":{"react":"18.3.1"},"scripts":{"build":"vite build"},"bin":{"web":"bin/web.js"},"os":["linux","darwin"],"cpu":"x64","engines":{"node":">=20"},"repository":{"type":"git","url":"https://example.invalid/web.git"},"dist":{"integrity":"sha512-abc"}}"#;
        let report = normalize_provider_document(ProviderFamily::Npm, native);
        report.validate().expect("npm provider facts are lossless");
        assert_eq!(report.native_format, "json");
        assert_eq!(report.native_document, native);
        assert_eq!(report.facts.dependencies, vec!["vite".to_string()]);
        assert_eq!(
            report.facts.dev_dependencies,
            vec!["typescript".to_string()]
        );
        assert!(report.facts.platforms.contains(&"os:linux".to_string()));
        assert!(report.facts.typed.contains_key("provider.npm.hook.build"));
        assert!(report
            .facts
            .typed
            .contains_key("provider.npm.variant.engine.node"));
        assert!(report
            .shared_facts()
            .facts
            .contains_key("provider.npm.native.repository"));
    }

    #[test]
    fn npm_pretty_documents_and_malformed_typed_fields_stay_explicit() {
        let pretty = "{\n  \"name\": \"web\",\n  \"version\": \"1.0.0\",\n  \"dist\": {\"integrity\": \"sha512-abc\"}\n}";
        let report = normalize_provider_document(ProviderFamily::Npm, pretty);
        report
            .validate()
            .expect("pretty npm JSON remains lossless");
        assert_eq!(report.native_document, pretty);

        let malformed = normalize_provider_document(
            ProviderFamily::Npm,
            r#"{"name":"web","version":"1.0.0","bin":{"web":1},"engines":{"node":20},"dist":{"integrity":false}}"#,
        );
        assert!(malformed
            .losses
            .iter()
            .any(|loss| loss.contains("bin") && loss.contains("strings")));
        assert!(malformed
            .losses
            .iter()
            .any(|loss| loss.contains("engine") && loss.contains("string")));
        assert!(malformed
            .losses
            .iter()
            .any(|loss| loss.contains("dist field `integrity`")));
        assert!(malformed.validate().is_err());
    }

    #[test]
    fn cargo_conformance_retains_lock_variants_hooks_and_source_ownership() {
        let native = r#"[package]
name = "app"
version = "1.0.0"
license = "MIT"
repository = "https://example.invalid/app.git"
build = "build.rs"

[dependencies]
serde = "1.0"

[target.x86_64-unknown-linux-gnu.dependencies]
cc = "1.0"

[features]
default = ["serde"]

[[package]]
name = "app"
version = "1.0.0"

[[package]]
name = "serde"
version = "1.0.200"
source = "registry+https://example.invalid"
checksum = "abc123"
"#;
        let report = normalize_provider_document(ProviderFamily::Cargo, native);
        report
            .validate()
            .expect("Cargo provider facts are lossless");
        assert!(report.facts.dependencies.contains(&"serde".to_string()));
        assert!(report.facts.dependencies.contains(&"cc".to_string()));
        assert!(report.facts.scripts.contains(&"build.rs".to_string()));
        assert!(report
            .facts
            .typed
            .contains_key("provider.cargo.variant.feature.default"));
        assert!(report
            .facts
            .typed
            .contains_key("provider.cargo.lock.serde.checksum"));
        assert!(report
            .shared_facts()
            .facts
            .contains_key("provider.cargo.source.repository"));
    }

    #[test]
    fn cargo_comments_keep_identity_and_malformed_fields_report_loss() {
        let commented = r#"[package]
name = "app" # package name
version = "1.0.0" # package version
"#;
        let report = normalize_provider_document(ProviderFamily::Cargo, commented);
        report
            .validate()
            .expect("Cargo inline comments do not change identity");
        assert_eq!(report.facts.name, "app");
        assert_eq!(report.facts.version, "1.0.0");

        let malformed = normalize_provider_document(
            ProviderFamily::Cargo,
            "[package]\nname = 7\nversion = \"1.0.0\"\n[dependencies]\nserde = false\n",
        );
        assert!(malformed
            .losses
            .iter()
            .any(|loss| loss.contains("package field `name`")));
        assert!(malformed
            .losses
            .iter()
            .any(|loss| loss.contains("dependency `serde`")));
        assert!(malformed.validate().is_err());

        let missing_checksum = normalize_provider_document(
            ProviderFamily::Cargo,
            "[package]\nname = \"app\"\nversion = \"1.0.0\"\n[[package]]\nname = \"serde\"\nversion = \"1.0.200\"\nsource = \"registry+https://example.invalid\"\n",
        );
        assert!(missing_checksum
            .losses
            .iter()
            .any(|loss| loss.contains("registry package") && loss.contains("checksum")));
        assert!(missing_checksum.validate().is_err());
    }

    #[test]
    fn nix_conformance_retains_native_and_typed_provider_facts() {
        let native = r#"{"pname":"ripgrep","version":"14.1.1","drvPath":"/nix/store/hash-ripgrep-14.1.1.drv","narHash":"sha256-0123456789012345678901234567890123456789012=","inputDrvs":{"/nix/store/hash-openssl.drv":["out"]},"buildInputs":["openssl"],"nativeBuildInputs":["pkg-config"],"system":"x86_64-linux","platforms":["linux"],"license":"BSD-3-Clause","builder":"/bin/bash","signatures":["cache.nixos.org"],"owner":"nixos","repository":"NixOS/nixpkgs","advisories":["CVE-0000-0000"],"meta":{"license":"BSD-3-Clause","platforms":["x86_64-linux"]}}"#;
        let report = normalize_provider_document(ProviderFamily::Nix, native);
        report.validate().expect("Nix provider facts are lossless");
        assert_eq!(report.native_format, "json");
        assert_eq!(report.native_document, native);
        assert_eq!(report.facts.name, "ripgrep");
        assert_eq!(report.facts.version, "14.1.1");
        assert!(report
            .facts
            .build_dependencies
            .contains(&"openssl".to_string()));
        assert!(report
            .facts
            .typed
            .contains_key("provider.nix.dependency.build"));
        assert!(report.facts.typed.contains_key("provider.nix.hook.builder"));
        assert!(report
            .facts
            .typed
            .contains_key("provider.nix.variant.system"));
        assert!(report
            .facts
            .typed
            .contains_key("provider.nix.signature.signatures"));
        assert!(report
            .facts
            .typed
            .contains_key("provider.nix.source.repository"));
        assert!(report
            .shared_facts()
            .facts
            .contains_key("provider.nix.native.meta"));

        let flake_lock = concat!(
            r#"{"name":"nixpkgs","version":"24.05","root":"root","nodes":{"#,
            r#""root":{"locked":{"type":"github","owner":"NixOS","repo":"nixpkgs","#,
            r#""rev":"0123456789abcdef0123456789abcdef01234567"}}}}"#
        );
        let flake_report = normalize_provider_document(ProviderFamily::Nix, flake_lock);
        flake_report
            .validate()
            .expect("Nix flake lock source facts are lossless");
        assert_eq!(
            flake_report.facts.source_identity,
            "github:NixOS/nixpkgs@0123456789abcdef0123456789abcdef01234567"
        );
        assert!(flake_report
            .shared_facts()
            .facts
            .contains_key("provider.nix.source.locked.root.rev"));

        let shared = report.shared_facts();
        let exported = ProviderFacts::from_json(&report.export_json())
            .expect("Nix provider export uses the shared carrier");
        assert_eq!(exported, shared);
        assert!(shared
            .explain_lines()
            .iter()
            .any(|line| line == "native json: retained"));
        let locked = report
            .lock_record("app", "ripgrep#version=14.1.1@nix", "x86_64-linux")
            .expect("Nix provider lock uses the shared carrier");
        let locked_facts = ProviderFacts::from_json(
            locked
                .future_fields
                .get("provider-facts")
                .expect("Nix provider facts in lock"),
        )
        .expect("locked Nix provider facts");
        assert_eq!(locked_facts, shared);
    }

    #[test]
    fn nix_conformance_reports_loss_and_conflict_in_native_facts() {
        let missing_source = normalize_provider_document(
            ProviderFamily::Nix,
            r#"{"pname":"ripgrep","version":"14.1.1","buildInputs":["openssl"]}"#,
        );
        assert!(missing_source.validate().is_err());
        assert!(missing_source
            .losses
            .iter()
            .any(|loss| loss.contains("immutable source identity")));

        let mutable_lock = normalize_provider_document(
            ProviderFamily::Nix,
            r#"{"pname":"ripgrep","version":"14.1.1","locked":{"type":"github","owner":"NixOS","repo":"nixpkgs","rev":"main"}}"#,
        );
        assert!(mutable_lock.validate().is_err());
        assert!(mutable_lock
            .losses
            .iter()
            .any(|loss| loss.contains("complete immutable owner/revision identity")));

        let lossy = normalize_provider_document(
            ProviderFamily::Nix,
            r#"{"pname":"ripgrep","version":"14.1.1","drvPath":"/nix/store/hash-ripgrep-14.1.1.drv","platforms":1,"yanked":"unknown","inputDrvs":[1]}"#,
        );
        assert!(lossy.validate().is_err());
        assert!(lossy.losses.iter().any(|loss| loss.contains("platforms")));
        assert!(lossy.losses.iter().any(|loss| loss.contains("yanked")));
        assert!(lossy.losses.iter().any(|loss| loss.contains("inputDrvs")));

        let conflicting = normalize_provider_document(
            ProviderFamily::Nix,
            concat!(
                r#"[{"pname":"ripgrep","version":"14.1.1","drvPath":"/nix/store/hash-ripgrep-14.1.1.drv","owner":"nixos"},"#,
                r#"{"pname":"ripgrep","version":"14.1.1","drvPath":"/nix/store/hash-ripgrep-14.1.1.drv","owner":"other"}]"#
            ),
        );
        assert!(conflicting.validate().is_err());
        assert!(conflicting
            .conflicts
            .iter()
            .any(|conflict| conflict.contains("conflicts with the first native record")));
    }

    #[test]
    fn jet_registry_conformance_retains_attestation_and_yank_facts() {
        let native = r#"{"name":"web","version":"1.0.0","content_hash":"sha256-web","license":"MIT","platforms":["linux"],"yanked":true,"tier":"trusted","gate_status":"passed","public_key":"pk","signature":"sig","owner":"team-web","source":{"kind":"git","url":"https://example.invalid/web.git"},"advisories":["CVE-0000-0000"],"variants":{"debug":{"features":["trace"]}},"hooks":{"build":{"digest":"hook-digest"}}}"#;
        let report = normalize_provider_document(ProviderFamily::JetRegistry, native);
        report.validate().expect("Jet registry facts are lossless");
        assert_eq!(report.facts.integrity_hash, "sha256-web");
        assert!(report
            .facts
            .typed
            .contains_key("provider.registry.signature"));
        assert!(report.facts.typed.contains_key("provider.registry.yanked"));
        assert!(report
            .shared_facts()
            .facts
            .contains_key("provider.registry.native.source"));
    }

    #[test]
    fn jet_registry_pretty_json_retains_native_document() {
        let native = "{\n  \"name\": \"web\",\n  \"version\": \"1.0.0\",\n  \"content_hash\": \"sha256-web\"\n}";
        let report = normalize_provider_document(ProviderFamily::JetRegistry, native);
        report
            .validate()
            .expect("pretty registry JSON remains lossless");
        assert_eq!(report.native_document, native);
        assert_eq!(report.facts.name, "web");
        assert_eq!(report.facts.version, "1.0.0");

        let malformed = normalize_provider_document(
            ProviderFamily::JetRegistry,
            r#"{"name":"web","version":"1.0.0","content_hash":"sha256-web","hooks":[],"source":false}"#,
        );
        assert!(malformed
            .losses
            .iter()
            .any(|loss| loss.contains("hooks") && loss.contains("shape")));
        assert!(malformed
            .losses
            .iter()
            .any(|loss| loss.contains("source") && loss.contains("shape")));
        assert!(malformed.validate().is_err());
    }

    #[test]
    fn provider_conformance_reports_ambiguous_and_conflicting_native_facts() {
        let packument = r#"{"name":"web","versions":{"1.0.0":{"name":"web","version":"1.0.0"},"2.0.0":{"name":"web","version":"2.0.0"}}}"#;
        let report = normalize_provider_document(ProviderFamily::Npm, packument);
        assert!(report.validate().is_err());
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.contains("multiple versions")));

        let registry = concat!(
            r#"{"name":"web","version":"1.0.0","content_hash":"sha256-a"}"#,
            "\n",
            r#"{"name":"web","version":"2.0.0","content_hash":"sha256-b"}"#
        );
        let report = normalize_provider_document(ProviderFamily::JetRegistry, registry);
        assert!(report.validate().is_err());
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.contains("multiple package identities")));

        let conflicting_registry = concat!(
            r#"{"name":"web","version":"1.0.0","content_hash":"sha256-a"}"#,
            "\n",
            r#"{"name":"web","version":"1.0.0","content_hash":"sha256-b"}"#
        );
        let report = normalize_provider_document(ProviderFamily::JetRegistry, conflicting_registry);
        assert!(report.validate().is_err());
        assert!(report
            .conflicts
            .iter()
            .any(|conflict| conflict.contains("conflicting native facts")));
    }

    #[test]
    fn pypi_swiftpm_maven_reports_round_trip_through_lock_and_export() {
        let revision = "0123456789abcdef0123456789abcdef01234567";
        let pypi = r#"{"info":{"name":"sample","version":"1.2.3","license":"MIT","requires_dist":["httpx>=0.27"]},"urls":[{"filename":"sample.whl","digests":{"sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"},"yanked":false}]}"#;
        let swiftpm = format!(
            r#"{{"version":1,"pins":[{{"package":"swift-log","state":{{"revision":"{revision}","version":"1.5.4"}}}}]}}"#
        );
        let maven = r#"<project><groupId>com.example</groupId><artifactId>sample</artifactId><version>1.2.3</version><dependencies><dependency><groupId>org.example</groupId><artifactId>dep</artifactId><version>3.0.0</version></dependency></dependencies></project>"#;
        for (family, document) in [
            (ProviderFamily::PyPI, pypi),
            (ProviderFamily::SwiftPM, swiftpm.as_str()),
            (ProviderFamily::Maven, maven),
        ] {
            let report = normalize_provider_document(family, document);
            report.validate().expect("provider report is lossless");
            let shared = report.shared_facts();
            assert_eq!(shared.native_document, document);
            assert_eq!(
                ProviderFacts::from_json(&report.export_json()).unwrap(),
                shared
            );
            let lock = report
                .lock_record("app", &shared.reference, "x86_64-linux")
                .expect("provider lock retains exact identity");
            let locked =
                ProviderFacts::from_json(lock.future_fields.get("provider-facts").unwrap())
                    .unwrap();
            assert_eq!(locked, shared);
        }
    }

    #[test]
    fn pypi_swiftpm_maven_reports_surface_loss_and_conflict() {
        let pypi = normalize_provider_document(
            ProviderFamily::PyPI,
            r#"{"info":{"name":"sample"},"urls":[]}"#,
        );
        assert!(pypi.validate().is_err());
        assert!(pypi
            .losses
            .iter()
            .any(|loss| loss.contains("exact version")));

        let swiftpm = normalize_provider_document(
            ProviderFamily::SwiftPM,
            r#"{"version":1,"pins":[{"package":"swift-log","state":{"branch":"main"}}]}"#,
        );
        assert!(swiftpm.validate().is_err());
        assert!(swiftpm
            .losses
            .iter()
            .any(|loss| loss.contains("exact revision")));

        let maven = normalize_provider_document(
            ProviderFamily::Maven,
            r#"<project><groupId>com.example</groupId><artifactId>sample</artifactId><version>1.0.0</version><version>2.0.0</version></project>"#,
        );
        assert!(maven.validate().is_err());
        assert!(maven
            .conflicts
            .iter()
            .any(|conflict| conflict.contains("conflicting version")));
    }

    #[test]
    fn pypi_swiftpm_maven_reject_malformed_native_shapes_without_defaults() {
        let pypi = normalize_provider_document(
            ProviderFamily::PyPI,
            r#"{"info":{"name":"sample","version":"1.0.0"},"urls":[{"filename":"sample.whl","digests":{"sha256":false}}]}"#,
        );
        assert!(pypi
            .losses
            .iter()
            .any(|loss| loss.contains("digest `sha256` must be a string")));
        assert!(pypi.validate().is_err());

        let swiftpm = normalize_provider_document(
            ProviderFamily::SwiftPM,
            r#"{"version":9,"pins":[{"identity":"swift-log","state":{"revision":1}}]}"#,
        );
        assert!(swiftpm
            .losses
            .iter()
            .any(|loss| loss.contains("unsupported") && loss.contains("version")));
        assert!(swiftpm
            .losses
            .iter()
            .any(|loss| loss.contains("revision") && loss.contains("string")));
        assert!(swiftpm.validate().is_err());

        let maven_namespace = normalize_provider_document(
            ProviderFamily::Maven,
            r#"<project xmlns="urn:unsupported"><groupId>com.example</groupId><artifactId>sample</artifactId><version>1.0.0</version></project>"#,
        );
        assert!(maven_namespace
            .losses
            .iter()
            .any(|loss| loss.contains("unsupported XML namespace")));
        assert!(maven_namespace.validate().is_err());

        let duplicate_dependency = normalize_provider_document(
            ProviderFamily::Maven,
            r#"<project><groupId>com.example</groupId><artifactId>sample</artifactId><version>1.0.0</version><dependencies><dependency><groupId>org.example</groupId><artifactId>dep</artifactId><version>1.0.0</version></dependency><dependency><groupId>org.example</groupId><artifactId>dep</artifactId><version>2.0.0</version></dependency></dependencies></project>"#,
        );
        assert!(duplicate_dependency
            .conflicts
            .iter()
            .any(|conflict| conflict.contains("org.example:dep")
                && conflict.contains("conflicting version")));
        assert!(duplicate_dependency.validate().is_err());
    }

    #[test]
    fn nuget_conan_and_vcpkg_conformance_retains_native_facts_and_lock_identity() {
        let nuget = r#"<package>
  <metadata>
    <id>widget</id>
    <version>1.2.3</version>
    <licenseExpression>MIT</licenseExpression>
    <repository type="git" url="https://example.invalid/widget" commit="abc" />
    <dependencies><group targetFramework="net8.0"><dependency id="serde" version="[1.0.0]" /></group></dependencies>
  </metadata>
</package>"#;
        let conan = r#"from conan import ConanFile
class Widget(ConanFile):
    name = "widget"
    version = "1.2.3"
    license = "MIT"
    settings = "os", "arch", "compiler", "build_type"
    options = {"shared": [True, False]}
    generators = "CMakeDeps"
    def requirements(self):
        self.requires("zlib/1.3.1")
"#;
        let vcpkg = r#"{
  "name": "widget",
  "version-string": "1.2.3",
  "license": "MIT",
  "builtin-baseline": "0123456789abcdef0123456789abcdef01234567",
  "supports": "!uwp",
  "dependencies": [{"name":"zlib","version>=":"1.3.0","features":["core"]}],
  "features": {"tools": ["fmt"]}
}"#;

        for (family, document, reference) in [
            (ProviderFamily::NuGet, nuget, "widget#version=1.2.3@nuget"),
            (ProviderFamily::Conan, conan, "widget#version=1.2.3@conan"),
            (ProviderFamily::Vcpkg, vcpkg, "widget#version=1.2.3@vcpkg"),
        ] {
            let report = normalize_provider_document(family, document);
            report.validate().expect("provider report is lossless");
            assert_eq!(report.native_document, document);
            assert!(!report.export_json().is_empty());
            let lock = report
                .lock_record("app", reference, "x86_64-linux")
                .expect("provider lock retains exact identity");
            let locked = ProviderFacts::from_json(
                lock.future_fields
                    .get("provider-facts")
                    .expect("provider facts in lock"),
            )
            .expect("provider facts JSON");
            assert_eq!(locked.native_document, document);
            assert_eq!(locked.qualified_reference(), reference);
        }

        let conan_lock = r#"{"ref":"widget/1.2.3@acme/stable#rrev","requires":[{"ref":"zlib/1.3.1"}],"settings":{"os":"Linux"},"options":{"shared":false},"package_id":"pkgid"}"#;
        let conan_report = normalize_provider_document(ProviderFamily::Conan, conan_lock);
        conan_report
            .validate()
            .expect("Conan JSON provider facts are lossless");
        assert!(conan_report
            .facts
            .typed
            .contains_key("provider.conan.variant.package_id"));
        assert!(conan_report
            .facts
            .dependencies
            .contains(&"zlib@1.3.1".to_string()));
    }

    #[test]
    fn nuget_conan_and_vcpkg_conformance_reports_loss_and_conflict() {
        let nuget = r#"{"dependencies":{"net8.0":{"widget":{"requested":"[1.0.0,2.0.0)"}}}}"#;
        let conan =
            r#"{"name":"widget","version":"1.0.0","requires":[{"package_id":"missing-ref"}]}"#;
        let vcpkg = r#"{"name":"widget","version":"1.0.0","version-string":"1.1.0","dependencies":[{"features":["core"]}]}"#;

        let nuget_report = normalize_provider_document(ProviderFamily::NuGet, nuget);
        assert!(nuget_report.validate().is_err());
        assert!(nuget_report
            .losses
            .iter()
            .any(|loss| loss.contains("resolved exact version") || loss.contains("range")));

        let conan_report = normalize_provider_document(ProviderFamily::Conan, conan);
        assert!(conan_report.validate().is_err());
        assert!(conan_report
            .losses
            .iter()
            .any(|loss| loss.contains("no dependency ref")));

        let vcpkg_report = normalize_provider_document(ProviderFamily::Vcpkg, vcpkg);
        assert!(vcpkg_report.validate().is_err());
        assert!(vcpkg_report
            .conflicts
            .iter()
            .any(|conflict| conflict.contains("conflicting version fields")));
        assert!(vcpkg_report
            .losses
            .iter()
            .any(|loss| loss.contains("no non-empty `name`")));
    }

    #[test]
    fn provider_conformance_retains_real_lock_shapes_and_rejects_hooks() {
        let nuget = r#"{
  "targets":{"net8.0":{"widget/1.2.3":{"type":"package","dependencies":{"serde":"1.0.0"}}}},
  "libraries":{"widget/1.2.3":{"type":"package","sha512":"sha512-widget"}},
  "projectFileDependencyGroups":{"net8.0":["widget >= 1.0.0"]}
        }"#;
        let report = normalize_provider_document(ProviderFamily::NuGet, nuget);
        report
            .validate()
            .expect("single NuGet lock identity is lossless");
        assert_eq!(report.facts.platforms, vec!["framework:net8.0"]);
        assert_eq!(report.facts.integrity_hash, "sha512-widget");
        assert!(report
            .facts
            .typed
            .contains_key("provider.nuget.target.net8.0.widget/1.2.3.serde"));
        assert!(report
            .facts
            .typed
            .contains_key("provider.nuget.project.request.net8.0.0"));

        let ranged_xml = r#"<package><metadata><id>widget</id><version>1.2.3</version><dependencies><dependency id="serde" version="[1.0.0,2.0.0)" /></dependencies></metadata></package>"#;
        let ranged = normalize_provider_document(ProviderFamily::NuGet, ranged_xml);
        assert!(ranged.validate().is_err());
        assert!(ranged
            .losses
            .iter()
            .any(|loss| loss.contains("serde") && loss.contains("exact version")));

        let graph = r#"{"ref":"widget/1.2.3","graph":{"nodes":{"0":{"ref":"widget/1.2.3","requires":["1"],"tool_requires":["2"],"package_id":"pkgid"},"1":{"ref":"zlib/1.3.1"},"2":{"ref":"cmake/3.29.0"}}}}"#;
        let graph_report = normalize_provider_document(ProviderFamily::Conan, graph);
        graph_report
            .validate()
            .expect("Conan graph root identity is lossless");
        assert!(graph_report
            .facts
            .typed
            .contains_key("provider.conan.graph.node.0"));
        assert!(graph_report
            .facts
            .build_dependencies
            .iter()
            .any(|dependency| dependency == "2"));

        let hook = normalize_provider_document(
            ProviderFamily::Conan,
            "name = \"widget\"\nversion = \"1.2.3\"\ndef generate(self):\n    self.run(\"cmake\")\n",
        );
        assert!(hook.validate().is_err());
        assert!(hook
            .losses
            .iter()
            .any(|loss| loss.contains("executable build or Python hook")));

        let malformed_vcpkg = normalize_provider_document(
            ProviderFamily::Vcpkg,
            r#"{"name":"widget","version":"1.0.0","dependencies":[{"name":"zlib","features":"core"}],"overrides":[{"version":"1.0.0"}]}"#,
        );
        assert!(malformed_vcpkg.validate().is_err());
        assert!(malformed_vcpkg
            .losses
            .iter()
            .any(|loss| loss.contains("features") && loss.contains("invalid shape")));
        assert!(malformed_vcpkg
            .losses
            .iter()
            .any(|loss| loss.contains("override 0") && loss.contains("name")));
    }
}
