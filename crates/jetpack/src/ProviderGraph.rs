//! Federated provider facts under Jetpack authority (D-WD6).
//!
//! External provider prefixes and trust-root config remain owner-gated. This
//! module models provider metadata/fetch/lock/sandbox/signature/audit facts.

pub use super::Replacement::ReplacementCandidate as ReplacementOverlay;
use super::JSON::{self, JSONValue};
use jet_pkg_model::ProviderFacts::{ProviderFactValue, ProviderFacts};

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
        let selector_identity = if !shared.selector.version.is_empty() {
            Some(shared.selector.version.clone())
        } else if !shared.selector.revision.is_empty() {
            Some(shared.selector.revision.clone())
        } else if !shared.selector.digest.is_empty() {
            Some(shared.selector.digest.clone())
        } else {
            None
        };
        let (_, expected_identity) = metadata_identity_selector(&self.facts);
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
        let reference = format!(
            "{}#{}={}@{}",
            self.facts.name,
            selector_key,
            selector_value,
            self.facts.family.label()
        );
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
        if requested.qualified_reference() != shared.qualified_reference() {
            return Err(format!(
                "provider lock reference `{reference}` disagrees with metadata identity `{}`",
                shared.qualified_reference()
            ));
        }
        let qualified_reference = shared.qualified_reference();
        let mut record = crate::SemanticLock::SemanticRecord::new(
            crate::SemanticLock::LockIdentity {
                kind: crate::SemanticLock::LockRecordKind::Package,
                key: format!("provider:{qualified_reference}"),
                exact: qualified_reference.clone(),
                hash: shared.digest(),
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
            .insert("provider-facts".to_string(), shared.to_json());
        record
            .future_fields
            .insert("provider-facts-digest".to_string(), shared.digest());
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
                    .map(|value| ProviderFactValue::Text(value.clone()))
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
    if !facts.version.is_empty() {
        return ("version", facts.version.clone());
    }
    if !facts.integrity_hash.is_empty() {
        return ("digest", facts.integrity_hash.clone());
    }
    ("version", String::new())
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
        ProviderFamily::Core | ProviderFamily::Nix | ProviderFamily::Path => {
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
    if facts.name.is_empty() {
        losses.push("npm metadata has no package name".to_string());
    }
    facts.version = json_string(&object, "version").unwrap_or_default();
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
        Some(JSONValue::Object(_)) => json_keys(&metadata, "bin"),
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
    for entry in &assignments {
        let native_namespace = if entry.array_table {
            "lock"
        } else {
            "manifest"
        };
        let native_key = format!(
            "provider.cargo.native.{native_namespace}.{}.{}",
            entry.section, entry.key
        );
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
            if entry.value != "false" {
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
    let name = metadata_line(document, "name").or_else(|| toml_string(document, "name"));
    let version = metadata_line(document, "version").or_else(|| toml_string(document, "version"));
    let mut facts = MetadataFacts::empty(ProviderFamily::PyPI, name.unwrap_or_default());
    facts.version = version.unwrap_or_default();
    facts.dependencies = metadata_list(document, "requires-dist");
    facts.source_identity = format!("pypi:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if document.contains("dynamic =") || document.contains("dynamic:") {
        report
            .losses
            .push("dynamic Python metadata must be resolved to an exact lock".to_string());
    }
    report
}

fn swiftpm_report(document: &str) -> ProviderFactReport {
    let parsed = JSON::parse(document).ok();
    let mut facts = MetadataFacts::empty(ProviderFamily::SwiftPM, String::new());
    if let Some(JSONValue::Object(root)) = parsed {
        if let Some(JSONValue::Array(pins)) = root.get("pins") {
            if pins.len() == 1 {
                if let Some(JSONValue::Object(pin)) = pins.first() {
                    facts.name = json_string(pin, "identity").unwrap_or_default();
                    let (version, revision) = match pin.get("state") {
                        Some(JSONValue::Object(state)) => (
                            json_string(state, "version"),
                            json_string(state, "revision"),
                        ),
                        _ => (None, json_string(pin, "revision")),
                    };
                    facts.version = version
                        .clone()
                        .or_else(|| revision.clone())
                        .unwrap_or_default();
                    if let Some(revision) = revision {
                        facts.integrity_hash = revision.clone();
                        facts
                            .typed
                            .insert("provider.revision".to_string(), vec![revision]);
                    } else {
                        facts.integrity_hash = facts.version.clone();
                    }
                }
            } else if pins.len() > 1 {
                facts.name = "swiftpm-lock".to_string();
                facts.version = "set".to_string();
                facts.dependencies = pins
                    .iter()
                    .filter_map(|pin| match pin {
                        JSONValue::Object(pin) => json_string(pin, "identity"),
                        _ => None,
                    })
                    .collect();
            }
        }
    }
    let source_revision = facts
        .typed
        .get("provider.revision")
        .and_then(|values| values.first())
        .filter(|revision| !revision.is_empty())
        .cloned()
        .unwrap_or_else(|| facts.version.clone());
    facts.source_identity = format!("swiftpm:{}@{}", facts.name, source_revision);
    let mut report = report_with_identity(facts);
    if report.facts.version == "set" {
        report.losses.push(
            "Package.resolved contains multiple pins; normalize each pin before realization"
                .to_string(),
        );
    }
    report
}

fn maven_report(document: &str) -> ProviderFactReport {
    let name = xml_tag(document, "artifactId").unwrap_or_default();
    let group = xml_tag(document, "groupId").unwrap_or_default();
    let version = xml_tag(document, "version").unwrap_or_default();
    let mut facts = MetadataFacts::empty(ProviderFamily::Maven, name);
    facts.version = version;
    facts.source_identity = format!("maven:{}:{}@{}", group, facts.name, facts.version);
    facts.dependencies = xml_tag_values(document, "artifactId")
        .into_iter()
        .filter(|dependency| dependency != &facts.name)
        .collect();
    report_with_identity(facts)
}

fn nuget_report(document: &str) -> ProviderFactReport {
    if let Ok(JSONValue::Object(object)) = JSON::parse(document) {
        return nuget_json_report(&object);
    }
    let mut facts = MetadataFacts::empty(ProviderFamily::NuGet, String::new());
    let mut packages = Vec::new();
    let root_package = match (xml_tag(document, "id"), xml_tag(document, "version")) {
        (Some(name), Some(version)) => {
            let package = (name, version);
            packages.push(package.clone());
            Some(package)
        }
        _ => None,
    };
    for tag in xml_opening_tags(document, "PackageReference") {
        if let Some(name) = xml_attribute(&tag, "Include") {
            packages.push((name, xml_attribute(&tag, "Version").unwrap_or_default()));
        }
    }
    let dependency_values = xml_opening_tags(document, "dependency")
        .into_iter()
        .filter_map(|tag| {
            let name = xml_attribute(&tag, "id")?;
            let version = xml_attribute(&tag, "version").unwrap_or_default();
            let dependency = format_dependency(&name, &version);
            packages.push((name, version));
            Some(dependency)
        })
        .collect::<Vec<_>>();
    packages.sort();
    packages.dedup();
    if packages.len() == 1 {
        let (name, version) = packages.remove(0);
        facts.name = name;
        facts.version = version;
    } else {
        facts.name = "nuget-lock".to_string();
        facts.version = "set".to_string();
        facts.dependencies = packages.iter().map(|(name, _)| name.clone()).collect();
    }
    facts.license = xml_tag(document, "license")
        .or_else(|| xml_tag(document, "licenseUrl"))
        .unwrap_or_default();
    facts.platforms = xml_opening_tags(document, "group")
        .into_iter()
        .filter_map(|tag| xml_attribute(&tag, "targetFramework"))
        .collect();
    facts.dependencies = if !dependency_values.is_empty() {
        dependency_values
    } else if root_package.is_none() && packages.len() > 1 {
        packages
            .iter()
            .map(|(name, version)| format_dependency(name, version))
            .collect()
    } else {
        Vec::new()
    };
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
    if packages.len() > 1 || report.facts.version == "set" {
        report.losses.push(
            "NuGet metadata contains multiple packages; lock each package identity separately"
                .to_string(),
        );
    }
    report
}

fn nuget_json_report(object: &std::collections::BTreeMap<String, JSONValue>) -> ProviderFactReport {
    let mut packages = Vec::new();
    if let Some(JSONValue::Object(dependencies)) = object.get("dependencies") {
        for packages_for_framework in dependencies.values() {
            if let JSONValue::Object(entries) = packages_for_framework {
                for (name, value) in entries {
                    if let JSONValue::Object(entry) = value {
                        let version = json_string(entry, "resolved")
                            .or_else(|| json_string(entry, "version"))
                            .or_else(|| json_string(entry, "requested"))
                            .unwrap_or_default();
                        packages.push((name.clone(), version));
                    }
                }
            }
        }
    }
    if let Some(JSONValue::Object(package)) = object.get("package") {
        if let (Some(name), Some(version)) = (
            json_string(package, "id").or_else(|| json_string(package, "name")),
            json_string(package, "version"),
        ) {
            packages.push((name, version));
        }
    }
    packages.sort();
    packages.dedup();
    let mut facts = MetadataFacts::empty(
        ProviderFamily::NuGet,
        json_string(object, "id")
            .or_else(|| json_string(object, "name"))
            .unwrap_or_default(),
    );
    if packages.len() == 1 {
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
    facts.integrity_hash = json_string(object, "contentHash")
        .or_else(|| json_string(object, "hash"))
        .unwrap_or_default();
    facts.source_identity = format!("nuget:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if packages.len() > 1 {
        report.losses.push(
            "NuGet lock metadata contains multiple package identities; lock each package separately"
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
    for key in ["options", "generators", "settings"] {
        let values = quoted_values_after_all(document, key);
        if !values.is_empty() {
            facts.typed.insert(format!("conan.{key}"), values);
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
    if document.lines().any(|line| {
        let line = line.trim();
        line.starts_with("def build")
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
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Conan,
        json_string(object, "name").unwrap_or_default(),
    );
    facts.version = json_string(object, "version")
        .or_else(|| json_string(object, "ref").and_then(|value| conan_ref_part(&value, 1)))
        .unwrap_or_default();
    facts.license = json_string(object, "license").unwrap_or_default();
    facts.dependencies = json_array_strings(object, "requires");
    facts.build_dependencies = json_array_strings(object, "tool_requires");
    facts.source_identity = format!("conan:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if object.contains_key("requires") && report.facts.dependencies.is_empty() {
        report
            .losses
            .push("Conan lock requires are not a string array".to_string());
    }
    report
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
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Vcpkg,
        json_string(&object, "name").unwrap_or_default(),
    );
    let version_fields = [
        "version",
        "version-string",
        "version-semver",
        "version-date",
    ];
    let versions = version_fields
        .iter()
        .filter_map(|key| json_string(&object, key).map(|value| (*key, value)))
        .collect::<Vec<_>>();
    facts.version = versions
        .first()
        .map(|(_, version)| version.clone())
        .unwrap_or_default();
    facts.dependencies = vcpkg_dependencies(&object, &mut facts.typed);
    facts.platforms = json_string(&object, "supports")
        .map(|value| vec![value])
        .unwrap_or_default();
    facts.license = json_string(&object, "license").unwrap_or_default();
    for key in ["builtin-baseline", "overrides", "features"] {
        if let Some(value) = object.get(key) {
            facts
                .typed
                .insert(format!("vcpkg.{key}"), vec![json_value_text(value)]);
        }
    }
    facts.source_identity = format!("vcpkg:{}@{}", facts.name, facts.version);
    let mut report = report_with_identity(facts);
    if versions.windows(2).any(|values| values[0].1 != values[1].1) {
        report
            .conflicts
            .push("vcpkg manifest declares conflicting version fields".to_string());
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
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Homebrew,
        json_string(&object, "name").unwrap_or_default(),
    );
    facts.version = json_string(&object, "version")
        .or_else(|| {
            object
                .get("versions")
                .and_then(|value| value.as_object().ok())
                .and_then(|versions| json_string(versions, "stable"))
        })
        .unwrap_or_default();
    facts.license = json_string(&object, "license").unwrap_or_default();
    facts.dependencies = json_array_strings(&object, "dependencies");
    facts.source_identity = format!("homebrew:{}@{}", facts.name, facts.version);
    report_with_identity(facts)
}

fn jet_registry_report(document: &str) -> ProviderFactReport {
    let lines = document
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let Some(line) = lines.first() else {
        return empty_report(
            ProviderFamily::JetRegistry,
            "jet-registry",
            "registry metadata is not valid JSON",
        );
    };
    let parsed = JSON::parse(line).ok();
    let Some(JSONValue::Object(object)) = parsed else {
        return empty_report(
            ProviderFamily::JetRegistry,
            "jet-registry",
            "registry metadata is not valid JSON",
        );
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
    if let Some(value) = object.get("platforms") {
        if !matches!(
            value,
            JSONValue::Array(values)
                if values.iter().all(|item| matches!(item, JSONValue::String(_)))
        ) {
            losses.push("registry platforms must be an array of strings".to_string());
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
    for (index, line) in lines.iter().enumerate().skip(1) {
        match JSON::parse(line) {
            Ok(JSONValue::Object(other))
                if json_string(&other, "name") == json_string(&object, "name")
                    && json_string(&other, "version") == json_string(&object, "version") =>
            {
                if other != object {
                    report.conflicts.push(format!(
                        "registry identity on line {} has conflicting native facts",
                        index + 1
                    ));
                }
            }
            Ok(JSONValue::Object(_)) => report.losses.push(format!(
                "registry metadata contains multiple package identities; line {} needs its own lock record",
                index + 1
            )),
            Ok(_) | Err(_) => report.losses.push(format!(
                "registry metadata line {} is not a JSON object",
                index + 1
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
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Github,
        json_string(&object, "name").unwrap_or_default(),
    );
    facts.version = json_string(&object, "tag_name")
        .or_else(|| json_string(&object, "version"))
        .unwrap_or_default();
    facts.source_identity = format!("github:{}@{}", facts.name, facts.version);
    report_with_identity(facts)
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
    let mut facts = MetadataFacts::empty(
        ProviderFamily::Binary,
        json_string(&object, "name").unwrap_or_default(),
    );
    facts.version = json_string(&object, "version").unwrap_or_default();
    facts.integrity_hash = json_string(&object, "hash")
        .or_else(|| json_string(&object, "sha256"))
        .unwrap_or_default();
    facts.platforms = json_array_strings(&object, "platforms");
    let identity = if facts.version.is_empty() {
        facts.integrity_hash.clone()
    } else {
        facts.version.clone()
    };
    facts.source_identity = format!("binary:{}@{}", facts.name, identity);
    let mut report = report_with_identity(facts);
    if report.facts.integrity_hash.is_empty() {
        report
            .losses
            .push("binary metadata has no content hash".to_string());
    }
    report
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
        | ProviderFamily::SwiftPM
        | ProviderFamily::Vcpkg
        | ProviderFamily::Homebrew
        | ProviderFamily::JetRegistry
        | ProviderFamily::Github
        | ProviderFamily::Binary => "json".to_string(),
        ProviderFamily::NuGet if JSON::parse(document).is_ok() => "json".to_string(),
        ProviderFamily::NuGet => "xml".to_string(),
        ProviderFamily::Conan if JSON::parse(document).is_ok() => "json".to_string(),
        ProviderFamily::Conan => "conan".to_string(),
        ProviderFamily::Cargo => "toml".to_string(),
        ProviderFamily::PyPI => "python-metadata".to_string(),
        ProviderFamily::Maven => "xml".to_string(),
        ProviderFamily::Core | ProviderFamily::Nix | ProviderFamily::Path => "provider".to_string(),
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
) -> Vec<String> {
    let Some(JSONValue::Array(values)) = object.get("dependencies") else {
        return Vec::new();
    };
    let mut dependencies = Vec::new();
    for value in values {
        match value {
            JSONValue::String(name) => dependencies.push(name.clone()),
            JSONValue::Object(dependency) => {
                let Some(name) = json_string(dependency, "name") else {
                    continue;
                };
                let version = ["version>=", "version>", "version"]
                    .iter()
                    .find_map(|key| json_string(dependency, key))
                    .unwrap_or_default();
                dependencies.push(format_dependency(&name, &version));
                for key in ["features", "platform", "host"] {
                    if let Some(value) = dependency.get(key) {
                        typed.insert(
                            format!("vcpkg.dependency.{name}.{key}"),
                            vec![json_value_text(value)],
                        );
                    }
                }
            }
            _ => {}
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
                .map(|(value, _)| value.trim().to_string())
        })
        .collect()
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

fn metadata_line(document: &str, key: &str) -> Option<String> {
    document.lines().find_map(|line| {
        let (left, right) = line.split_once(':')?;
        left.trim()
            .eq_ignore_ascii_case(key)
            .then(|| right.trim().to_string())
    })
}

fn metadata_list(document: &str, key: &str) -> Vec<String> {
    document
        .lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(':')?;
            left.trim()
                .eq_ignore_ascii_case(key)
                .then(|| right.trim().to_string())
        })
        .collect()
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
    use jet_pkg_model::ProviderFacts::ProviderFacts;

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
}
