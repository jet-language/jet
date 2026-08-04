//! Explainable semantic lock records and merge support (D-WD4 / E4-JP13).
//!
//! One live lock path: semantic records, transitive input graph, source maps,
//! and overlay invalidation facts share `.jet/lock` with machine package /
//! toolchain sections. Human rationale never enters the machine identity key.
//! Every atomic write revalidates graph satisfiability, signatures, domain
//! consistency, source authority, and offline completeness first.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum LockRecordKind {
    Package,
    SourceRef,
    WorkspaceMember,
    AdapterOutput,
    SourceBuild,
    CacheObject,
    Toolchain,
    SecretReference,
    ServicePackage,
    ImageInput,
    FleetInput,
    JetosActivationClosure,
    ReplacementOverlay,
    PackageOverlay,
    FlakeComposition,
    FlakeUnsupported,
    FlakeEvaluator,
    /// Selected typed variant domain (E4-JP15 / D-JPK-VARIANT1).
    Variant,
    Future(String),
}

impl LockRecordKind {
    pub fn as_str(&self) -> &str {
        match self {
            LockRecordKind::Package => "package",
            LockRecordKind::SourceRef => "source-ref",
            LockRecordKind::WorkspaceMember => "workspace-member",
            LockRecordKind::AdapterOutput => "adapter-output",
            LockRecordKind::SourceBuild => "source-build",
            LockRecordKind::CacheObject => "cache-object",
            LockRecordKind::Toolchain => "toolchain",
            LockRecordKind::SecretReference => "secret-reference",
            LockRecordKind::ServicePackage => "service-package",
            LockRecordKind::ImageInput => "image-input",
            LockRecordKind::FleetInput => "fleet-input",
            LockRecordKind::JetosActivationClosure => "jetos-activation-closure",
            LockRecordKind::ReplacementOverlay => "replacement-overlay",
            LockRecordKind::PackageOverlay => "package-overlay",
            LockRecordKind::FlakeComposition => "flake-composition",
            LockRecordKind::FlakeUnsupported => "flake-unsupported",
            LockRecordKind::FlakeEvaluator => "flake-evaluator",
            LockRecordKind::Variant => "variant",
            LockRecordKind::Future(s) => s.as_str(),
        }
    }

    pub fn parse(raw: &str) -> LockRecordKind {
        match raw {
            "package" => LockRecordKind::Package,
            "source-ref" => LockRecordKind::SourceRef,
            "workspace-member" => LockRecordKind::WorkspaceMember,
            "adapter-output" => LockRecordKind::AdapterOutput,
            "source-build" => LockRecordKind::SourceBuild,
            "cache-object" => LockRecordKind::CacheObject,
            "toolchain" => LockRecordKind::Toolchain,
            "secret-reference" => LockRecordKind::SecretReference,
            "service-package" => LockRecordKind::ServicePackage,
            "image-input" => LockRecordKind::ImageInput,
            "fleet-input" => LockRecordKind::FleetInput,
            "jetos-activation-closure" => LockRecordKind::JetosActivationClosure,
            "replacement-overlay" => LockRecordKind::ReplacementOverlay,
            "package-overlay" => LockRecordKind::PackageOverlay,
            "flake-composition" => LockRecordKind::FlakeComposition,
            "flake-unsupported" => LockRecordKind::FlakeUnsupported,
            "flake-evaluator" => LockRecordKind::FlakeEvaluator,
            "variant" => LockRecordKind::Variant,
            other => LockRecordKind::Future(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockIdentity {
    pub kind: LockRecordKind,
    pub key: String,
    pub exact: String,
    pub hash: String,
    pub platform: String,
}

impl LockIdentity {
    pub fn semantic_key(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.key)
    }

    pub fn machine_identity(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.kind.as_str(),
            self.key,
            self.exact,
            self.hash,
            self.platform
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LockRationale {
    pub owner_package: String,
    pub reason: String,
    pub source_ref: String,
    pub provider: String,
    pub channel_input: String,
    pub exact_output: String,
    pub policy_fingerprint: String,
    pub recipe_id: String,
    pub adapter_id: String,
    pub signature: String,
    pub cache_provenance: String,
    pub update_command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticRecord {
    pub identity: LockIdentity,
    pub rationales: Vec<LockRationale>,
    pub future_fields: BTreeMap<String, String>,
}

impl SemanticRecord {
    pub fn new(identity: LockIdentity, rationale: LockRationale) -> SemanticRecord {
        SemanticRecord {
            identity,
            rationales: vec![rationale],
            future_fields: BTreeMap::new(),
        }
    }
}

/// One flake-style lock input with optional `follows` edge (E4-JP13).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LockInput {
    pub name: String,
    pub url: String,
    pub follows: String,
}

/// The typed foreign-flake projection. This is deliberately a projection of
/// `flake.nix`, not a second resolver: source URLs and follows edges become
/// `LockInput`s, while exact revisions and output provenance become semantic
/// records in the same `.jet/lock` file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FlakeInput {
    pub name: String,
    pub url: String,
    pub revision: String,
    pub follows: String,
    pub provenance: String,
}

/// One node and its input edges from the native `flake.lock` graph.
///
/// The original and locked objects stay canonical JSON rather than being
/// reinterpreted as a second resolver model. This keeps every Nix field
/// lossless while Jet still validates the direct input projection above.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FlakeLockEdge {
    pub name: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FlakeLockNode {
    pub name: String,
    pub original: String,
    pub locked: String,
    pub inputs: Vec<FlakeLockEdge>,
}

/// An indirect flake reference resolved by the locked node. Ambient user or
/// system registries are never consulted; the lock is the authority.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FlakeRegistryEntry {
    pub alias: String,
    pub node: String,
    pub original: String,
    pub locked: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlakeOutputKind {
    Package,
    DevShell,
    App,
    Check,
    Formatter,
    Other(String),
}

impl FlakeOutputKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Package => "packages",
            Self::DevShell => "devShells",
            Self::App => "apps",
            Self::Check => "checks",
            Self::Formatter => "formatter",
            Self::Other(value) => value.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FlakeOutput {
    pub name: String,
    pub kind: FlakeOutputKind,
    pub system: String,
    pub attribute: String,
    pub provenance: String,
}

/// The declarative part of a flake-parts composition that has a direct Jet
/// graph meaning. Arbitrary evaluator functions stay behind the native Nix
/// boundary; module paths, systems, and the per-system projection marker are
/// ordinary graph facts and therefore round-trip through `.jet/lock`. Loaded
/// graphs also retain content fingerprints for each imported module so a
/// module edit cannot reuse a stale main-flake lock.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FlakeComposition {
    pub framework: String,
    pub modules: Vec<String>,
    pub module_sources: Vec<String>,
    pub systems: Vec<String>,
    pub per_system: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlakeGraphError {
    EmptyInputName,
    DuplicateInput(String),
    ConflictingInput { input: String, field: String },
    MissingFollows { input: String, follows: String },
    FollowsCycle(String),
    MissingRevision { input: String },
    InvalidAssignment(String),
    StaleSemanticLock(String),
    Io(String),
}

impl std::fmt::Display for FlakeGraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInputName => f.write_str("flake input has an empty name"),
            Self::DuplicateInput(name) => write!(f, "flake input `{name}` is declared more than once"),
            Self::ConflictingInput { input, field } => {
                write!(f, "flake input `{input}` has conflicting `{field}` assignments")
            }
            Self::MissingFollows { input, follows } => {
                write!(f, "flake input `{input}` follows unknown input `{follows}`")
            }
            Self::FollowsCycle(name) => write!(f, "flake input `{name}` participates in a follows cycle"),
            Self::MissingRevision { input } => {
                write!(f, "flake input `{input}` has no exact revision in flake.nix or flake.lock")
            }
            Self::InvalidAssignment(value) => write!(f, "unsupported flake assignment `{value}`"),
            Self::StaleSemanticLock(reason) => write!(f, "semantic flake lock is stale: {reason}"),
            Self::Io(value) => f.write_str(value),
        }
    }
}

impl std::error::Error for FlakeGraphError {}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlakeGraph {
    pub source: String,
    pub source_fingerprint: String,
    pub inputs: Vec<FlakeInput>,
    pub lock_nodes: Vec<FlakeLockNode>,
    pub registries: Vec<FlakeRegistryEntry>,
    pub outputs: Vec<FlakeOutput>,
    pub composition: Option<FlakeComposition>,
    pub unsupported: Vec<String>,
}

impl FlakeGraph {
    /// Parse the stable, declarative subset of a foreign flake. Arbitrary Nix
    /// expressions remain inside the private Nix compatibility boundary; they
    /// are reported as unsupported facts instead of becoming opaque Jet data.
    pub fn parse(source: impl Into<String>, text: &str) -> Result<Self, FlakeGraphError> {
        Self::parse_with_lock(source.into(), text, None, true)
    }

    fn parse_with_lock(
        source: String,
        text: &str,
        lock_text: Option<&str>,
        validate: bool,
    ) -> Result<Self, FlakeGraphError> {
        let source_fingerprint = crate::SHA256::sha256_hex(text.as_bytes());
        let text = strip_nix_comments(text);
        let mut inputs = BTreeMap::<String, FlakeInput>::new();
        for (path, field, value) in input_assignments(&text) {
            let name = path.trim_matches('.').to_string();
            if name.is_empty() {
                return Err(FlakeGraphError::EmptyInputName);
            }
            let entry = inputs.entry(name.clone()).or_insert_with(|| FlakeInput {
                name: name.clone(),
                url: String::new(),
                revision: String::new(),
                follows: String::new(),
                provenance: format!("{source}:inputs.{path}.{field}"),
            });
            match field.as_str() {
                "url" => {
                    if !entry.url.is_empty() && entry.url != value {
                        return Err(FlakeGraphError::ConflictingInput {
                            input: name,
                            field,
                        });
                    }
                    entry.url = value.clone();
                    entry.revision = flake_revision(&value);
                }
                "follows" => {
                    if !entry.follows.is_empty() && entry.follows != value {
                        return Err(FlakeGraphError::ConflictingInput {
                            input: name,
                            field,
                        });
                    }
                    entry.follows = value;
                }
                _ => return Err(FlakeGraphError::InvalidAssignment(field)),
            }
        }
        let mut outputs = BTreeSet::new();
        for (kind, system, attribute) in output_assignments(&text) {
            let kind = match kind.as_str() {
                "packages" | "legacyPackages" => FlakeOutputKind::Package,
                "devShells" | "devShell" => FlakeOutputKind::DevShell,
                "apps" => FlakeOutputKind::App,
                "checks" => FlakeOutputKind::Check,
                "formatter" => FlakeOutputKind::Formatter,
                other => FlakeOutputKind::Other(other.to_string()),
            };
            let name = if system.is_empty() {
                attribute.clone()
            } else {
                format!("{kind}:{system}:{attribute}", kind = kind.as_str())
            };
            outputs.insert(FlakeOutput {
                name,
                kind,
                system,
                attribute,
                provenance: source.clone(),
            });
        }
        let composition = parse_flake_parts_composition(&text);
        let mut unsupported = Vec::new();
        for field in [
            "shellHook",
            "processes",
            "services",
            "nixosConfigurations",
            "homeConfigurations",
            "darwinConfigurations",
            "checks",
        ] {
            if field == "checks" {
                continue;
            }
            if text.contains(field) {
                unsupported.push(field.to_string());
            }
        }
        if text.contains("perSystem") && composition.is_none() {
            unsupported.push("perSystem".to_string());
        }
        let mut graph = Self {
            source,
            source_fingerprint,
            inputs: inputs.into_values().collect(),
            lock_nodes: Vec::new(),
            registries: Vec::new(),
            outputs: outputs.into_iter().collect(),
            composition,
            unsupported,
        };
        if let Some(lock_text) = lock_text {
            apply_flake_lock(&mut graph, lock_text)?;
        }
        propagate_follows(&mut graph)?;
        if validate {
            graph.validate()?;
        }
        Ok(graph)
    }

    pub fn load(path: &Path) -> Result<Self, FlakeGraphError> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| FlakeGraphError::Io(format!("couldn't read `{}`: {error}", path.display())))?;
        let project_dir = path.parent().unwrap_or_else(|| Path::new("."));
        if let Some(lock) = crate::SemanticLock::load(project_dir) {
            let has_flake_facts = lock.inputs.iter().any(|input| input.name.starts_with("flake-"))
                || lock.records.iter().any(|record| {
                    matches!(
                        &record.identity.kind,
                        LockRecordKind::SourceRef
                            | LockRecordKind::AdapterOutput
                            | LockRecordKind::FlakeComposition
                            | LockRecordKind::FlakeUnsupported
                    ) && (record.identity.key.starts_with("flake-input:")
                        || record.identity.key.starts_with("flake-output:")
                        || record.identity.key.starts_with("flake-composition:")
                        || record.identity.key.starts_with("flake-unsupported:")
                        || record.identity.key == "flake-source")
                });
            if has_flake_facts {
                let mut source_graph = Self::parse_with_lock(
                    path.display().to_string(),
                    &text,
                    None,
                    false,
                )?;
                source_graph.attach_composition_sources(path)?;
                let locked = Self::from_semantic_lock(path.display().to_string(), &lock)?;
                if locked.source_fingerprint.is_empty() {
                    return Err(FlakeGraphError::StaleSemanticLock(
                        "it has no source fingerprint; refresh the lock".to_string(),
                    ));
                }
                if locked.source_fingerprint != source_graph.source_fingerprint {
                    return Err(FlakeGraphError::StaleSemanticLock(
                        "flake.nix changed since the lock was written".to_string(),
                    ));
                }
                Self::validate_semantic_lock_shape(&source_graph, &locked)?;
                return Ok(locked);
            }
        }
        let lock_path = path.with_file_name("flake.lock");
        let lock_text = match std::fs::read_to_string(&lock_path) {
            Ok(value) => Some(value),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(FlakeGraphError::Io(format!(
                    "couldn't read `{}`: {error}",
                    lock_path.display()
                )))
            }
        };
        let mut graph = Self::parse_with_lock(
            path.display().to_string(),
            &text,
            lock_text.as_deref(),
            true,
        )?;
        graph.attach_composition_sources(path)?;
        graph.validate()?;
        Ok(graph)
    }

    fn attach_composition_sources(&mut self, path: &Path) -> Result<(), FlakeGraphError> {
        let Some(composition) = self.composition.as_ref() else {
            return Ok(());
        };
        let composition_modules = composition.modules.clone();
        let composition_systems = composition.systems.clone();
        let composition_per_system = composition.per_system;
        let project_dir = path.parent().unwrap_or_else(|| Path::new("."));
        let project_root = project_dir.canonicalize().map_err(|error| {
            FlakeGraphError::Io(format!(
                "couldn't resolve flake-parts project root `{}`: {error}",
                project_dir.display()
            ))
        })?;
        let mut pending = composition_modules
            .into_iter()
            .map(|module| (module, project_dir.to_path_buf(), true))
            .collect::<Vec<_>>();
        let mut visited = BTreeSet::new();
        let mut module_sources = Vec::new();
        let mut imported_outputs = Vec::new();
        while let Some((module, base_dir, root_module)) = pending.pop() {
            let relative = Path::new(&module);
            if relative.is_absolute()
                || (root_module
                    && relative
                    .components()
                    .any(|component| component == std::path::Component::ParentDir))
            {
                return Err(FlakeGraphError::Io(format!(
                    "flake-parts module `{module}` must stay inside the flake project"
                )));
            }
            let module_path = base_dir.join(relative);
            let canonical = module_path.canonicalize().map_err(|error| {
                FlakeGraphError::Io(format!(
                    "couldn't read flake-parts module `{module}`: {error}"
                ))
            })?;
            if !canonical.starts_with(&project_root) || !canonical.is_file() {
                return Err(FlakeGraphError::Io(format!(
                    "flake-parts module `{module}` must be a file inside the flake project"
                )));
            }
            if !visited.insert(canonical.clone()) {
                continue;
            }
            let bytes = std::fs::read(&canonical).map_err(|error| {
                FlakeGraphError::Io(format!(
                    "couldn't read flake-parts module `{module}`: {error}"
                ))
            })?;
            let module_text = std::str::from_utf8(&bytes).map_err(|error| {
                FlakeGraphError::Io(format!(
                    "couldn't read flake-parts module `{module}` as UTF-8: {error}"
                ))
            })?;
            let source_name = if root_module {
                module.to_string()
            } else {
                format!(
                    "./{}",
                    canonical
                        .strip_prefix(&project_root)
                        .map_err(|_| {
                            FlakeGraphError::Io(format!(
                                "flake-parts module `{module}` must stay inside the flake project"
                            ))
                        })?
                        .display()
                )
            };
            module_sources.push(format!(
                "{}\t{}",
                source_name,
                crate::SHA256::sha256_hex(&bytes)
            ));
            // flake-parts places `packages.default` and
            // `devShells.default` inside `perSystem`. Those declarations are
            // still ordinary graph facts: expand an unqualified module
            // output across the declared systems and retain the module path
            // as its provenance. This keeps the lock and the bridge on the
            // same imported-module projection without evaluating arbitrary
            // Nix functions here.
            for (kind, declared_system, attribute) in output_assignments(module_text) {
                let systems = if declared_system.is_empty() && composition_per_system {
                    if composition_systems.is_empty() {
                        vec![String::new()]
                    } else {
                        composition_systems.clone()
                    }
                } else {
                    vec![declared_system]
                };
                for system in systems {
                    let kind = match kind.as_str() {
                        "packages" | "legacyPackages" => FlakeOutputKind::Package,
                        "devShells" | "devShell" => FlakeOutputKind::DevShell,
                        "apps" => FlakeOutputKind::App,
                        "checks" => FlakeOutputKind::Check,
                        "formatter" => FlakeOutputKind::Formatter,
                        other => FlakeOutputKind::Other(other.to_string()),
                    };
                    let name = if system.is_empty() {
                        attribute.clone()
                    } else {
                        format!("{}:{}:{}", kind.as_str(), system.as_str(), attribute.as_str())
                    };
                    if !imported_outputs.iter().any(|output: &FlakeOutput| {
                        output.name == name
                            && output.kind == kind
                            && output.system == system
                            && output.attribute == attribute
                    }) {
                        imported_outputs.push(FlakeOutput {
                            name,
                            kind,
                            system,
                            attribute: attribute.clone(),
                            provenance: source_name.clone(),
                        });
                    }
                }
            }
            for import in list_assignment_values(&strip_nix_comments(module_text), "imports") {
                pending.push((
                    import,
                    canonical
                        .parent()
                        .unwrap_or_else(|| project_root.as_path())
                        .to_path_buf(),
                    false,
                ));
            }
        }
        module_sources.sort();
        module_sources.dedup();
        if let Some(composition) = self.composition.as_mut() {
            composition.module_sources = module_sources;
        }
        self.outputs.extend(imported_outputs);
        self.outputs.sort();
        Ok(())
    }

    /// Reconstruct the foreign projection from the unified `.jet/lock`.
    /// `flake.lock` is accepted only as an import fallback; once Jet has a
    /// semantic lock, it is the sole identity source for this graph.
    pub fn from_semantic_lock(
        source: impl Into<String>,
        lock: &SemanticLockFile,
    ) -> Result<Self, FlakeGraphError> {
        let supplied_source = source.into();
        let source = lock
            .records
            .iter()
            .filter(|record| {
                record.identity.kind == LockRecordKind::SourceRef
                    && record.identity.key == "flake-source"
            })
            .find_map(|record| {
                record
                    .rationales
                    .first()
                    .map(|rationale| rationale.source_ref.clone())
                    .filter(|value| !value.is_empty())
            })
            .or_else(|| {
                lock.records
                    .iter()
            .filter(|record| record.identity.kind == LockRecordKind::AdapterOutput)
            .find_map(|record| {
                record
                    .rationales
                    .first()
                    .map(|rationale| rationale.source_ref.clone())
                    .filter(|value| !value.is_empty())
            })
            })
            .unwrap_or(supplied_source);
        let source_fingerprint = lock
            .records
            .iter()
            .find(|record| {
                record.identity.kind == LockRecordKind::SourceRef
                    && record.identity.key == "flake-source"
            })
            .map(|record| record.identity.exact.clone())
            .unwrap_or_default();
        let mut inputs = Vec::new();
        for input in &lock.inputs {
            let key = format!("flake-input:{}", input.name);
            let record = lock
                .records
                .iter()
                .find(|record| record.identity.kind == LockRecordKind::SourceRef && record.identity.key == key)
                .ok_or_else(|| FlakeGraphError::MissingRevision {
                    input: input.name.clone(),
                })?;
            if record.identity.exact.is_empty() {
                return Err(FlakeGraphError::MissingRevision {
                    input: input.name.clone(),
                });
            }
            let provenance = record
                .rationales
                .first()
                .map(|rationale| {
                    if rationale.reason.is_empty() {
                        source.clone()
                    } else {
                        rationale.reason.clone()
                    }
                })
                .unwrap_or_else(|| source.clone());
            inputs.push(FlakeInput {
                name: input.name.clone(),
                url: input.url.clone(),
                revision: record.identity.exact.clone(),
                follows: input.follows.clone(),
                provenance,
            });
        }
        let input_names = lock
            .inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<BTreeSet<_>>();
        for record in &lock.records {
            let Some(name) = record.identity.key.strip_prefix("flake-input:") else {
                continue;
            };
            if record.identity.kind == LockRecordKind::SourceRef
                && !input_names.contains(name)
            {
                return Err(FlakeGraphError::StaleSemanticLock(format!(
                    "lock has source record for input `{name}` without a lock_input"
                )));
            }
        }
        let mut lock_nodes = Vec::new();
        let mut lock_node_keys = BTreeSet::new();
        for record in &lock.records {
            if record.identity.kind != LockRecordKind::FlakeEvaluator {
                continue;
            }
            let Some(name) = record.identity.key.strip_prefix("flake-lock-node:") else {
                continue;
            };
            if !lock_node_keys.insert(name.to_string()) {
                return Err(FlakeGraphError::StaleSemanticLock(format!(
                    "lock has duplicate node record `{name}`"
                )));
            }
            let node = parse_flake_lock_node(&record.identity.exact)?;
            if node.name != name {
                return Err(FlakeGraphError::StaleSemanticLock(format!(
                    "lock node record `{name}` names `{}`",
                    node.name
                )));
            }
            lock_nodes.push(node);
        }
        lock_nodes.sort();
        let mut registries = Vec::new();
        let mut registry_keys = BTreeSet::new();
        for record in &lock.records {
            if record.identity.kind != LockRecordKind::FlakeEvaluator {
                continue;
            }
            let Some(alias) = record.identity.key.strip_prefix("flake-registry:") else {
                continue;
            };
            if !registry_keys.insert(alias.to_string()) {
                return Err(FlakeGraphError::StaleSemanticLock(format!(
                    "lock has duplicate registry record `{alias}`"
                )));
            }
            let entry = parse_flake_registry(&record.identity.exact)?;
            if entry.alias != alias {
                return Err(FlakeGraphError::StaleSemanticLock(format!(
                    "registry record `{alias}` names `{}`",
                    entry.alias
                )));
            }
            registries.push(entry);
        }
        registries.sort();
        let mut outputs = Vec::new();
        for record in &lock.records {
            if record.identity.kind != LockRecordKind::AdapterOutput {
                continue;
            }
            let Some(name) = record.identity.key.strip_prefix("flake-output:") else {
                continue;
            };
            let mut fields = record.identity.exact.splitn(3, ':');
            let kind = match fields.next().unwrap_or_default() {
                "packages" => FlakeOutputKind::Package,
                "devShells" => FlakeOutputKind::DevShell,
                "apps" => FlakeOutputKind::App,
                "checks" => FlakeOutputKind::Check,
                "formatter" => FlakeOutputKind::Formatter,
                other if !other.is_empty() => FlakeOutputKind::Other(other.to_string()),
                _ => return Err(FlakeGraphError::InvalidAssignment(record.identity.exact.clone())),
            };
            let system = fields.next().unwrap_or_default().to_string();
            let attribute = fields.next().unwrap_or_default().to_string();
            if attribute.is_empty() {
                return Err(FlakeGraphError::InvalidAssignment(record.identity.exact.clone()));
            }
            let provenance = record
                .rationales
                .first()
                .map(|rationale| {
                    if rationale.reason.is_empty() {
                        source.clone()
                    } else {
                        rationale.reason.clone()
                    }
                })
                .unwrap_or_else(|| source.clone());
            outputs.push(FlakeOutput {
                name: name.to_string(),
                kind,
                system,
                attribute,
                provenance,
            });
        }
        let mut unsupported = lock
            .records
            .iter()
            .filter(|record| record.identity.kind == LockRecordKind::FlakeUnsupported)
            .filter_map(|record| {
                record
                    .identity
                    .key
                    .strip_prefix("flake-unsupported:")
                    .map(str::to_string)
            })
            .collect::<Vec<_>>();
        unsupported.sort();
        unsupported.dedup();
        let composition = lock
            .records
            .iter()
            .find(|record| record.identity.kind == LockRecordKind::FlakeComposition)
            .map(|record| parse_composition_record(&record.identity.exact))
            .transpose()?;
        let graph = Self {
            source,
            source_fingerprint,
            inputs,
            lock_nodes,
            registries,
            outputs,
            composition,
            unsupported,
        };
        validate_semantic_root_inputs(&graph)?;
        graph.validate()?;
        Ok(graph)
    }

    fn validate_semantic_lock_shape(
        source: &FlakeGraph,
        locked: &FlakeGraph,
    ) -> Result<(), FlakeGraphError> {
        let source_inputs = source
            .inputs
            .iter()
            .map(|input| {
                (
                    input.name.clone(),
                    (input.url.clone(), input.follows.clone()),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let locked_inputs = locked
            .inputs
            .iter()
            .map(|input| {
                (
                    input.name.clone(),
                    (input.url.clone(), input.follows.clone()),
                )
            })
            .collect::<BTreeMap<_, _>>();
        if source_inputs != locked_inputs {
            return Err(FlakeGraphError::StaleSemanticLock(
                "flake input declarations differ from the lock".to_string(),
            ));
        }

        if source.composition != locked.composition {
            return Err(FlakeGraphError::StaleSemanticLock(
                "flake-parts composition differs from the lock".to_string(),
            ));
        }

        let output_shape = |graph: &FlakeGraph| {
            graph
                .outputs
                .iter()
                .map(|output| {
                    (
                        output.name.clone(),
                        output.kind.as_str().to_string(),
                        output.system.clone(),
                        output.attribute.clone(),
                    )
                })
                .collect::<BTreeSet<_>>()
        };
        if output_shape(source) != output_shape(locked) {
            return Err(FlakeGraphError::StaleSemanticLock(
                "flake output declarations differ from the lock".to_string(),
            ));
        }
        let source_unsupported = source
            .unsupported
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let locked_unsupported = locked
            .unsupported
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if source_unsupported != locked_unsupported {
            return Err(FlakeGraphError::StaleSemanticLock(
                "unsupported flake facts differ from the lock".to_string(),
            ));
        }
        Ok(())
    }

    /// Named devShell outputs are facts in the graph even when the private Nix
    /// oracle can only evaluate the default shell for the bridge shim.
    pub fn named_dev_shells(&self) -> Vec<&FlakeOutput> {
        self.outputs
            .iter()
            .filter(|output| output.kind == FlakeOutputKind::DevShell)
            .collect()
    }

    /// Stable lock text for callers that need a round-trip projection. The
    /// caller still owns the normal semantic-lock commit/validation gate.
    pub fn semantic_lock_text(&self) -> String {
        write(&self.semantic_lock())
    }

    pub fn validate(&self) -> Result<(), FlakeGraphError> {
        let names = self
            .inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<BTreeSet<_>>();
        if names.len() != self.inputs.len() {
            let mut seen = BTreeSet::new();
            if let Some(input) = self.inputs.iter().find(|input| !seen.insert(input.name.as_str())) {
                return Err(FlakeGraphError::DuplicateInput(input.name.clone()));
            }
        }
        let lock_names = self
            .lock_nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect::<BTreeSet<_>>();
        if lock_names.len() != self.lock_nodes.len() {
            let mut seen = BTreeSet::new();
            if let Some(node) = self
                .lock_nodes
                .iter()
                .find(|node| !seen.insert(node.name.as_str()))
            {
                return Err(FlakeGraphError::Io(format!(
                    "flake.lock has duplicate node `{}`",
                    node.name
                )));
            }
        }
        let lock_node_values = flake_lock_node_values(&self.lock_nodes)?;
        for node in &self.lock_nodes {
            for edge in &node.inputs {
                let target = crate::JSON::parse(&edge.target)
                    .map_err(|error| FlakeGraphError::Io(format!("flake.lock edge is invalid: {error}")))?;
                let target = lock_node_name(Some(&target), &lock_node_values, &node.name)?.ok_or_else(|| {
                    FlakeGraphError::Io(format!(
                        "flake.lock node `{}` input `{}` has an empty node reference",
                        node.name, edge.name
                    ))
                })?;
                if !lock_names.contains(target.as_str()) {
                    return Err(FlakeGraphError::Io(format!(
                        "flake.lock node `{}` input `{}` points to missing node `{target}`",
                        node.name, edge.name
                    )));
                }
            }
        }
        let registry_names = self
            .registries
            .iter()
            .map(|entry| entry.alias.as_str())
            .collect::<BTreeSet<_>>();
        if registry_names.len() != self.registries.len() {
            return Err(FlakeGraphError::Io(
                "flake.lock has duplicate indirect registry aliases".to_string(),
            ));
        }
        if let Some(entry) = self
            .registries
            .iter()
            .find(|entry| !lock_names.contains(entry.node.as_str()))
        {
            return Err(FlakeGraphError::Io(format!(
                "flake registry alias `{}` points to missing node `{}`",
                entry.alias, entry.node
            )));
        }
        for input in &self.inputs {
            if !input.url.is_empty() && input.revision.is_empty() {
                return Err(FlakeGraphError::MissingRevision {
                    input: input.name.clone(),
                });
            }
            if !input.follows.is_empty() && !names.contains(input.follows.as_str()) {
                return Err(FlakeGraphError::MissingFollows {
                    input: input.name.clone(),
                    follows: input.follows.clone(),
                });
            }
        }
        let edges = self
            .inputs
            .iter()
            .filter(|input| !input.follows.is_empty())
            .map(|input| (input.name.as_str(), input.follows.as_str()))
            .collect::<BTreeMap<_, _>>();
        for input in &self.inputs {
            let mut seen = BTreeSet::new();
            let mut current = input.name.as_str();
            while let Some(next) = edges.get(current) {
                if !seen.insert(current) {
                    return Err(FlakeGraphError::FollowsCycle(current.to_string()));
                }
                current = next;
            }
        }
        if let Some(input) = self.inputs.iter().find(|input| input.revision.is_empty()) {
            return Err(FlakeGraphError::MissingRevision {
                input: input.name.clone(),
            });
        }
        Ok(())
    }

    /// Project this graph into the existing semantic lock. Exact revision,
    /// source provenance, and structured outputs are records, not duplicated
    /// resolver state.
    pub fn semantic_lock(&self) -> SemanticLockFile {
        let mut records = Vec::new();
        records.push(SemanticRecord::new(
            LockIdentity {
                kind: LockRecordKind::SourceRef,
                key: "flake-source".to_string(),
                exact: self.source_fingerprint.clone(),
                hash: self.source_fingerprint.clone(),
                platform: String::new(),
            },
            LockRationale {
                source_ref: self.source.clone(),
                provider: "flake".to_string(),
                reason: "source fingerprint for semantic lock freshness".to_string(),
                ..LockRationale::default()
            },
        ));
        for input in &self.inputs {
            let identity = LockIdentity {
                kind: LockRecordKind::SourceRef,
                key: format!("flake-input:{}", input.name),
                exact: input.revision.clone(),
                hash: crate::SHA256::sha256_hex(
                    format!("{}\0{}\0{}", input.url, input.follows, input.provenance).as_bytes(),
                ),
                platform: String::new(),
            };
            records.push(SemanticRecord::new(
                identity,
                LockRationale {
                    source_ref: input.url.clone(),
                    provider: "flake".to_string(),
                    channel_input: input.follows.clone(),
                    exact_output: input.revision.clone(),
                    reason: input.provenance.clone(),
                    ..LockRationale::default()
                },
            ));
        }
        for node in &self.lock_nodes {
            let exact = flake_lock_node_json(node);
            records.push(SemanticRecord::new(
                LockIdentity {
                    kind: LockRecordKind::FlakeEvaluator,
                    key: format!("flake-lock-node:{}", node.name),
                    hash: crate::SHA256::sha256_hex(exact.as_bytes()),
                    exact,
                    platform: String::new(),
                },
                LockRationale {
                    source_ref: self.source.clone(),
                    provider: "flake-lock".to_string(),
                    reason: "exact locked node and transitive input edges".to_string(),
                    ..LockRationale::default()
                },
            ));
        }
        for registry in &self.registries {
            let exact = flake_registry_json(registry);
            records.push(SemanticRecord::new(
                LockIdentity {
                    kind: LockRecordKind::FlakeEvaluator,
                    key: format!("flake-registry:{}", registry.alias),
                    hash: crate::SHA256::sha256_hex(exact.as_bytes()),
                    exact,
                    platform: String::new(),
                },
                LockRationale {
                    source_ref: self.source.clone(),
                    provider: "flake-registry".to_string(),
                    reason: "indirect flake reference resolved by locked registry metadata".to_string(),
                    ..LockRationale::default()
                },
            ));
        }
        for output in &self.outputs {
            let exact = format!(
                "{}:{}:{}",
                output.kind.as_str(), output.system, output.attribute
            );
            records.push(SemanticRecord::new(
                LockIdentity {
                    kind: LockRecordKind::AdapterOutput,
                    key: format!("flake-output:{}", output.name),
                    exact: exact.clone(),
                    // The source path is provenance, not semantic identity;
                    // excluding it keeps a graph reconstructed from the
                    // unified lock byte-stable with its original projection.
                    hash: crate::SHA256::sha256_hex(
                        format!("{}\0{}", exact, output.provenance).as_bytes(),
                    ),
                    platform: output.system.clone(),
                },
                LockRationale {
                    source_ref: self.source.clone(),
                    provider: "flake".to_string(),
                    exact_output: exact,
                    reason: output.provenance.clone(),
                    ..LockRationale::default()
                },
            ));
        }
        if let Some(composition) = &self.composition {
            let exact = composition_json(composition);
            records.push(SemanticRecord::new(
                LockIdentity {
                    kind: LockRecordKind::FlakeComposition,
                    key: format!("flake-composition:{}", composition.framework),
                    hash: crate::SHA256::sha256_hex(exact.as_bytes()),
                    exact,
                    platform: String::new(),
                },
                LockRationale {
                    source_ref: self.source.clone(),
                    provider: "flake-parts".to_string(),
                    reason: "declarative flake-parts composition".to_string(),
                    ..LockRationale::default()
                },
            ));
        }
        for field in &self.unsupported {
            records.push(SemanticRecord::new(
                LockIdentity {
                    kind: LockRecordKind::FlakeUnsupported,
                    key: format!("flake-unsupported:{field}"),
                    exact: field.clone(),
                    hash: crate::SHA256::sha256_hex(field.as_bytes()),
                    platform: String::new(),
                },
                LockRationale {
                    source_ref: self.source.clone(),
                    provider: "flake".to_string(),
                    reason: "unsupported foreign fact retained for L0204".to_string(),
                    ..LockRationale::default()
                },
            ));
        }
        SemanticLockFile {
            records,
            inputs: self
                .inputs
                .iter()
                .map(|input| LockInput {
                    name: input.name.clone(),
                    url: input.url.clone(),
                    follows: input.follows.clone(),
                })
                .collect(),
            source_maps: Vec::new(),
        }
    }

    pub fn stable_json(&self) -> String {
        let inputs = self
            .inputs
            .iter()
            .map(|input| {
                format!(
                    "{{\"name\":{},\"url\":{},\"revision\":{},\"follows\":{},\"provenance\":{}}}",
                    crate::JSON::quote(&input.name),
                    crate::JSON::quote(&input.url),
                    crate::JSON::quote(&input.revision),
                    crate::JSON::quote(&input.follows),
                    crate::JSON::quote(&input.provenance)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let outputs = self
            .outputs
            .iter()
            .map(|output| {
                format!(
                    "{{\"name\":{},\"kind\":{},\"system\":{},\"attribute\":{},\"provenance\":{}}}",
                    crate::JSON::quote(&output.name),
                    crate::JSON::quote(output.kind.as_str()),
                    crate::JSON::quote(&output.system),
                    crate::JSON::quote(&output.attribute),
                    crate::JSON::quote(&output.provenance)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let unsupported = self
            .unsupported
            .iter()
            .map(|value| crate::JSON::quote(value))
            .collect::<Vec<_>>()
            .join(",");
        let composition = self
            .composition
            .as_ref()
            .map(composition_json)
            .unwrap_or_else(|| "null".to_string());
        let lock_nodes = self
            .lock_nodes
            .iter()
            .map(flake_lock_node_json)
            .collect::<Vec<_>>()
            .join(",");
        let registries = self
            .registries
            .iter()
            .map(flake_registry_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"source\":{},\"inputs\":[{}],\"lockNodes\":[{}],\"registries\":[{}],\"outputs\":[{}],\"composition\":{},\"unsupported\":[{}]}}",
            crate::JSON::quote(&self.source),
            inputs,
            lock_nodes,
            registries,
            outputs,
            composition,
            unsupported
        )
    }
}

fn parse_flake_parts_composition(text: &str) -> Option<FlakeComposition> {
    if !text.contains("flake-parts") || !text.contains("mkFlake") {
        return None;
    }
    let mut modules = list_assignment_values(text, "imports");
    let mut systems = list_assignment_values(text, "systems");
    modules.sort();
    modules.dedup();
    systems.sort();
    systems.dedup();
    Some(FlakeComposition {
        framework: "flake-parts".to_string(),
        modules,
        module_sources: Vec::new(),
        systems,
        per_system: text.contains("perSystem") || text.contains("per_system"),
    })
}

fn list_assignment_values(text: &str, name: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = text[cursor..].find(name) {
        let start = cursor + relative;
        let before_ok = start == 0 || !is_nix_name_char(text.as_bytes()[start - 1]);
        let after_name = start + name.len();
        let after_ok = after_name >= text.len() || !is_nix_name_char(text.as_bytes()[after_name]);
        if !before_ok || !after_ok {
            cursor = after_name;
            continue;
        }
        let mut index = after_name;
        while index < text.len() && text.as_bytes()[index].is_ascii_whitespace() {
            index += 1;
        }
        if text.as_bytes().get(index) != Some(&b'=') {
            cursor = after_name;
            continue;
        }
        index += 1;
        while index < text.len() && text.as_bytes()[index].is_ascii_whitespace() {
            index += 1;
        }
        if text.as_bytes().get(index) != Some(&b'[') {
            cursor = after_name;
            continue;
        }
        let Some(close) = matching_square(text, index) else {
            break;
        };
        let body = &text[index + 1..close];
        values.extend(quoted_values(body));
        values.extend(
            body.split_whitespace()
                .map(|value| value.trim_matches([',', ';', '(', ')']))
                .filter(|value| is_relative_module_path(value))
                .map(str::to_string),
        );
        cursor = close + 1;
    }
    values
}

fn is_relative_module_path(value: &str) -> bool {
    value == "." || value == ".." || value.starts_with("./") || value.starts_with("../")
}

fn quoted_values(text: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut cursor = 0;
    while cursor < text.len() {
        let Some(relative) = text[cursor..].find('"') else {
            break;
        };
        let start = cursor + relative;
        let end = skip_quoted(text, start);
        if end <= text.len() && end > start + 1 {
            values.push(text[start + 1..end - 1].to_string());
        }
        cursor = end.max(start + 1);
    }
    values
}

fn matching_square(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(open) != Some(&b'[') {
        return None;
    }
    let mut depth = 0_u32;
    let mut index = open;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => index = skip_quoted(text, index),
            b'[' => {
                depth += 1;
                index += 1;
            }
            b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    None
}

fn composition_json(composition: &FlakeComposition) -> String {
    let modules = composition
        .modules
        .iter()
        .map(|value| crate::JSON::quote(value))
        .collect::<Vec<_>>()
        .join(",");
    let systems = composition
        .systems
        .iter()
        .map(|value| crate::JSON::quote(value))
        .collect::<Vec<_>>()
        .join(",");
    let module_sources = composition
        .module_sources
        .iter()
        .map(|value| crate::JSON::quote(value))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"framework\":{},\"modules\":[{}],\"moduleSources\":[{}],\"perSystem\":{},\"systems\":[{}]}}",
        crate::JSON::quote(&composition.framework),
        modules,
        module_sources,
        composition.per_system,
        systems
    )
}

fn flake_lock_node_json(node: &FlakeLockNode) -> String {
    let inputs = node
        .inputs
        .iter()
        .map(|input| {
            format!(
                "{{\"name\":{},\"target\":{}}}",
                crate::JSON::quote(&input.name),
                input.target
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"inputs\":[{}],\"locked\":{},\"name\":{},\"original\":{}}}",
        inputs,
        node.locked,
        crate::JSON::quote(&node.name),
        node.original
    )
}

fn parse_flake_lock_node(raw: &str) -> Result<FlakeLockNode, FlakeGraphError> {
    let parsed = crate::JSON::parse_lenient(raw)
        .map_err(|error| FlakeGraphError::InvalidAssignment(error))?;
    let object = parsed
        .value
        .as_object()
        .map_err(FlakeGraphError::InvalidAssignment)?;
    let string_field = |key: &str| {
        object
            .get(key)
            .ok_or_else(|| FlakeGraphError::InvalidAssignment(format!("missing lock node field `{key}`")))?
            .as_str()
            .map(str::to_string)
            .map_err(FlakeGraphError::InvalidAssignment)
    };
    let inputs = object
        .get("inputs")
        .ok_or_else(|| FlakeGraphError::InvalidAssignment("missing lock node inputs".to_string()))?
        .as_array()
        .map_err(FlakeGraphError::InvalidAssignment)?
        .iter()
        .map(|value| {
            let object = value
                .as_object()
                .map_err(FlakeGraphError::InvalidAssignment)?;
            let name = object
                .get("name")
                .ok_or_else(|| FlakeGraphError::InvalidAssignment("missing lock edge name".to_string()))?
                .as_str()
                .map(str::to_string)
                .map_err(FlakeGraphError::InvalidAssignment)?;
            let target = object
                .get("target")
                .ok_or_else(|| FlakeGraphError::InvalidAssignment("missing lock edge target".to_string()))?;
            Ok(FlakeLockEdge {
                name,
                target: canonical_json(target),
            })
        })
        .collect::<Result<Vec<_>, FlakeGraphError>>()?;
    let original = object
        .get("original")
        .ok_or_else(|| FlakeGraphError::InvalidAssignment("missing lock node original".to_string()))?;
    let locked = object
        .get("locked")
        .ok_or_else(|| FlakeGraphError::InvalidAssignment("missing lock node locked".to_string()))?;
    let name = string_field("name")?;
    let original_json = canonical_json(original);
    let locked_json = canonical_json(locked);
    let original = original.as_object().map_err(|_| {
        FlakeGraphError::InvalidAssignment(
            "lock node original/locked must be objects".to_string(),
        )
    })?;
    let locked = locked.as_object().map_err(|_| {
        FlakeGraphError::InvalidAssignment(
            "lock node original/locked must be objects".to_string(),
        )
    })?;
    if name.is_empty() {
        return Err(FlakeGraphError::InvalidAssignment(
            "lock node name is empty".to_string(),
        ));
    }
    if name == "root" && (!original.is_empty() || !locked.is_empty()) {
        return Err(FlakeGraphError::InvalidAssignment(
            "root lock node original/locked must be empty objects".to_string(),
        ));
    }
    if name != "root" && (original.is_empty() || locked.is_empty()) {
        return Err(FlakeGraphError::InvalidAssignment(
            "non-root lock node original/locked must be non-empty objects".to_string(),
        ));
    }
    let mut node = FlakeLockNode {
        name,
        original: original_json,
        locked: locked_json,
        inputs,
    };
    node.inputs.sort();
    Ok(node)
}

fn flake_registry_json(entry: &FlakeRegistryEntry) -> String {
    format!(
        "{{\"alias\":{},\"locked\":{},\"node\":{},\"original\":{}}}",
        crate::JSON::quote(&entry.alias),
        entry.locked,
        crate::JSON::quote(&entry.node),
        entry.original
    )
}

fn parse_flake_registry(raw: &str) -> Result<FlakeRegistryEntry, FlakeGraphError> {
    let parsed = crate::JSON::parse_lenient(raw)
        .map_err(|error| FlakeGraphError::InvalidAssignment(error))?;
    let object = parsed
        .value
        .as_object()
        .map_err(FlakeGraphError::InvalidAssignment)?;
    let string_field = |key: &str| {
        object
            .get(key)
            .ok_or_else(|| FlakeGraphError::InvalidAssignment(format!("missing registry field `{key}`")))?
            .as_str()
            .map(str::to_string)
            .map_err(FlakeGraphError::InvalidAssignment)
    };
    let original = object
        .get("original")
        .ok_or_else(|| FlakeGraphError::InvalidAssignment("missing registry original".to_string()))?;
    let locked = object
        .get("locked")
        .ok_or_else(|| FlakeGraphError::InvalidAssignment("missing registry locked".to_string()))?;
    let original = original.as_object().map_err(|_| {
        FlakeGraphError::InvalidAssignment(
            "registry original/locked must be objects".to_string(),
        )
    })?;
    let locked = locked.as_object().map_err(|_| {
        FlakeGraphError::InvalidAssignment(
            "registry original/locked must be objects".to_string(),
        )
    })?;
    if original.is_empty() || locked.is_empty() {
        return Err(FlakeGraphError::InvalidAssignment(
            "registry original/locked must be non-empty objects".to_string(),
        ));
    }
    let alias = string_field("alias")?;
    let original = canonical_json(&crate::JSON::JSONValue::Object(original.clone()));
    let locked = canonical_json(&crate::JSON::JSONValue::Object(locked.clone()));
    if indirect_registry_alias(&original)? != Some(alias.clone()) {
        return Err(FlakeGraphError::InvalidAssignment(
            "registry record original is not the recorded indirect alias".to_string(),
        ));
    }
    Ok(FlakeRegistryEntry {
        alias,
        node: string_field("node")?,
        original,
        locked,
    })
}

fn parse_composition_record(raw: &str) -> Result<FlakeComposition, FlakeGraphError> {
    let parsed = crate::JSON::parse_lenient(raw)
        .map_err(|error| FlakeGraphError::InvalidAssignment(error))?;
    let object = parsed
        .value
        .as_object()
        .map_err(FlakeGraphError::InvalidAssignment)?;
    let string_list = |key: &str| -> Result<Vec<String>, FlakeGraphError> {
        object
            .get(key)
            .ok_or_else(|| FlakeGraphError::InvalidAssignment(format!("missing composition field `{key}`")))?
            .as_array()
            .map_err(FlakeGraphError::InvalidAssignment)?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .map_err(FlakeGraphError::InvalidAssignment)
            })
            .collect()
    };
    let framework = object
        .get("framework")
        .ok_or_else(|| FlakeGraphError::InvalidAssignment("missing composition framework".to_string()))?
        .as_str()
        .map_err(FlakeGraphError::InvalidAssignment)?
        .to_string();
    let per_system = match object.get("perSystem") {
        Some(crate::JSON::JSONValue::Bool(value)) => *value,
        _ => {
            return Err(FlakeGraphError::InvalidAssignment(
                "composition perSystem is not boolean".to_string(),
            ))
        }
    };
    let module_sources = match object.get("moduleSources") {
        Some(_) => string_list("moduleSources")?,
        None => Vec::new(),
    };
    Ok(FlakeComposition {
        framework,
        modules: string_list("modules")?,
        module_sources,
        systems: string_list("systems")?,
        per_system,
    })
}

fn strip_nix_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut quoted = false;
    let mut escaped = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
        } else if ch == '"' {
            quoted = true;
            out.push(ch);
        } else if ch == '#' {
            for comment in chars.by_ref() {
                if comment == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn input_assignments(text: &str) -> Vec<(String, String, String)> {
    let mut found = assignments_after(text, "inputs.")
        .into_iter()
        .filter(|(_, field, _)| field == "url" || field == "follows")
        .collect::<Vec<_>>();
    found.extend(
        input_record_assignments(text)
            .into_iter()
            .filter(|(_, field, _)| field == "url" || field == "follows"),
    );
    found.sort();
    found.dedup();
    found
}

/// Read the common flake form `inputs = { nixpkgs.url = "…"; }` as well as
/// the dotted `inputs.nixpkgs.url = "…"` form. The parser is intentionally
/// limited to quoted declarative assignments; arbitrary Nix stays a private
/// evaluator concern and is never guessed into the graph.
fn input_record_assignments(text: &str) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = text[cursor..].find("inputs") {
        let start = cursor + relative;
        let before_ok = start == 0
            || !text.as_bytes()[start - 1].is_ascii_alphanumeric()
                && text.as_bytes()[start - 1] != b'_';
        let after = &text[start + "inputs".len()..];
        let after_trim = after.trim_start();
        if before_ok && after_trim.starts_with('=') {
            let rhs_raw = &after_trim[1..];
            let rhs = rhs_raw.trim_start();
            if rhs.starts_with('{') {
                let trim_offset = after.len() - after_trim.len();
                let rhs_offset = rhs_raw.len() - rhs.len();
                let open = start + "inputs".len() + trim_offset + 1 + rhs_offset;
                if let Some(close) = matching_brace(text, open) {
                    found.extend(block_assignments(&text[open + 1..close], ""));
                    cursor = close + 1;
                    continue;
                }
            }
        }
        cursor = start + "inputs".len();
    }
    found
}

fn block_assignments(text: &str, parent: &str) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'"' {
            cursor = skip_quoted(text, cursor);
            continue;
        }
        if !is_nix_name_char(bytes[cursor]) {
            cursor += 1;
            continue;
        }
        let start = cursor;
        while cursor < bytes.len() && is_nix_name_char(bytes[cursor]) {
            cursor += 1;
        }
        let local = &text[start..cursor];
        let mut after = cursor;
        while after < bytes.len() && bytes[after].is_ascii_whitespace() {
            after += 1;
        }
        if after >= bytes.len() || bytes[after] != b'=' {
            continue;
        }
        after += 1;
        while after < bytes.len() && bytes[after].is_ascii_whitespace() {
            after += 1;
        }
        let path = if parent.is_empty() {
            local.to_string()
        } else {
            format!("{parent}.{local}")
        };
        if after < bytes.len() && bytes[after] == b'{' {
            if let Some(close) = matching_brace(text, after) {
                found.extend(block_assignments(&text[after + 1..close], &path));
                cursor = close + 1;
                continue;
            }
        }
        if let Some(value) = quoted_rhs(&text[after..]) {
            if let Some((path, field)) = path.rsplit_once('.') {
                found.push((path.to_string(), field.to_string(), value));
            }
        }
    }
    found
}

fn is_nix_name_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
}

fn skip_quoted(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut escaped = false;
    let mut index = start + 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            return index + 1;
        }
        index += 1;
    }
    bytes.len()
}

fn matching_brace(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(open) != Some(&b'{') {
        return None;
    }
    let mut depth = 0_u32;
    let mut index = open;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => index = skip_quoted(text, index),
            b'{' => {
                depth += 1;
                index += 1;
            }
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    None
}

fn assignments_after(text: &str, prefix: &str) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = text[cursor..].find(prefix) {
        let start = cursor + relative;
        let rest = &text[start + prefix.len()..];
        let mut end = 0;
        for (index, ch) in rest.char_indices() {
            if !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')) {
                end = index;
                break;
            }
        }
        if end == 0 {
            end = rest.len();
        }
        let path = &rest[..end];
        let Some((path, field)) = path.rsplit_once('.') else {
            cursor = start + prefix.len() + end.max(1);
            continue;
        };
        let after = &rest[end..];
        let Some(eq) = after.find('=') else {
            cursor = start + prefix.len() + end.max(1);
            continue;
        };
        if let Some(value) = quoted_rhs(&after[eq + 1..]) {
            found.push((path.to_string(), field.to_string(), value));
        }
        cursor = start + prefix.len() + end.max(1);
    }
    found
}

fn output_assignments(text: &str) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    for prefix in ["packages.", "legacyPackages.", "devShells.", "devShell.", "apps.", "checks.", "formatter."] {
        for path in assignment_paths(text, prefix) {
            let parts = path.split('.').collect::<Vec<_>>();
            if parts.is_empty() {
                continue;
            }
            let (system, attribute) = if parts.len() > 1 {
                (parts[0].to_string(), parts[1..].join("."))
            } else {
                (String::new(), parts[0].to_string())
            };
            found.push((prefix.trim_end_matches('.').to_string(), system, attribute));
        }
    }
    found.sort();
    found.dedup();
    found
}

fn assignment_paths(text: &str, prefix: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = text[cursor..].find(prefix) {
        let start = cursor + relative;
        let rest = &text[start + prefix.len()..];
        let mut end = 0;
        for (index, ch) in rest.char_indices() {
            if !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')) {
                end = index;
                break;
            }
        }
        if end == 0 {
            end = rest.len();
        }
        let path = rest[..end].trim_end_matches('.');
        if let Some(eq) = rest[end..].find('=') {
            if !path.is_empty() {
                found.push(path.to_string());
            }
            cursor = start + prefix.len() + end + eq + 1;
        } else {
            cursor = start + prefix.len() + end.max(1);
        }
    }
    found
}

fn quoted_rhs(value: &str) -> Option<String> {
    let value = value.trim_start();
    let value = value.strip_prefix('"')?;
    let mut escaped = false;
    let mut out = String::new();
    for ch in value.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

fn flake_lock_node_values(
    nodes: &[FlakeLockNode],
) -> Result<BTreeMap<String, crate::JSON::JSONValue>, FlakeGraphError> {
    nodes
        .iter()
        .map(|node| {
            let inputs = node
                .inputs
                .iter()
                .map(|edge| {
                    crate::JSON::parse(&edge.target)
                        .map(|target| (edge.name.clone(), target))
                        .map_err(|error| {
                            FlakeGraphError::Io(format!(
                                "flake.lock node `{}` input `{}` is invalid: {error}",
                                node.name, edge.name
                            ))
                        })
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            Ok((
                node.name.clone(),
                crate::JSON::JSONValue::Object(BTreeMap::from([(
                    "inputs".to_string(),
                    crate::JSON::JSONValue::Object(inputs),
                )])),
            ))
        })
        .collect()
}

fn flake_revision(url: &str) -> String {
    if let Some(revision) = url
        .split_once('?')
        .and_then(|(_, query)| flake_query_fields(query))
        .and_then(|query| query.get("rev").cloned())
    {
        return revision;
    }
    let candidate = url
        .split_once('?')
        .map_or(url, |(base, _)| base)
        .rsplit('/')
        .next()
        .unwrap_or_default();
    if is_flake_revision(candidate) {
        candidate.to_string()
    } else {
        String::new()
    }
}

fn is_flake_revision(value: &str) -> bool {
    value.len() >= 7 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn flake_query_fields(query: &str) -> Option<BTreeMap<String, String>> {
    let mut fields = BTreeMap::new();
    if query.is_empty() {
        return Some(fields);
    }
    for field in query.split('&') {
        let (key, value) = field.split_once('=')?;
        if key.is_empty() || fields.insert(key.to_string(), value.to_string()).is_some() {
            return None;
        }
    }
    Some(fields)
}

fn canonical_json(value: &crate::JSON::JSONValue) -> String {
    match value {
        crate::JSON::JSONValue::Null => "null".to_string(),
        crate::JSON::JSONValue::Bool(value) => value.to_string(),
        crate::JSON::JSONValue::Num(value) => {
            if value.is_finite() && value.fract() == 0.0 {
                format!("{value:.0}")
            } else {
                value.to_string()
            }
        }
        crate::JSON::JSONValue::Str(value) => crate::JSON::quote(value),
        crate::JSON::JSONValue::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        crate::JSON::JSONValue::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| {
                    format!("{}:{}", crate::JSON::quote(key), canonical_json(value))
                })
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn canonical_object(
    object: &std::collections::BTreeMap<String, crate::JSON::JSONValue>,
    key: &str,
) -> String {
    object
        .get(key)
        .map(canonical_json)
        .unwrap_or_else(|| "{}".to_string())
}

fn indirect_registry_alias(raw: &str) -> Result<Option<String>, FlakeGraphError> {
    let value = crate::JSON::parse(raw)
        .map_err(|error| FlakeGraphError::Io(format!("flake.lock original metadata is invalid: {error}")))?;
    let object = value.as_object().map_err(|error| {
        FlakeGraphError::Io(format!("flake.lock original metadata is invalid: {error}"))
    })?;
    let Some(kind) = object.get("type") else {
        return Ok(None);
    };
    let kind = kind.as_str().map_err(|error| {
        FlakeGraphError::Io(format!("flake.lock original type is invalid: {error}"))
    })?;
    if kind != "indirect" {
        return Ok(None);
    }
    let alias = object
        .get("id")
        .ok_or_else(|| FlakeGraphError::Io("flake.lock indirect input has no id".to_string()))?
        .as_str()
        .map_err(|error| FlakeGraphError::Io(format!("flake.lock indirect id is invalid: {error}")))?;
    if alias.is_empty() {
        return Err(FlakeGraphError::Io(
            "flake.lock indirect input has an empty id".to_string(),
        ));
    }
    Ok(Some(alias.to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FlakeSourceIdentity {
    Direct {
        kind: String,
        owner: String,
        repo: String,
        reference: Option<String>,
        revision: Option<String>,
        query: BTreeMap<String, String>,
    },
    Indirect {
        alias: String,
        reference: Option<String>,
        revision: Option<String>,
        query: BTreeMap<String, String>,
    },
}

fn flake_source_identity(url: &str) -> Option<FlakeSourceIdentity> {
    let (base, query) = url.split_once('?').unwrap_or((url, ""));
    let query = flake_query_fields(query)?;
    let (kind, path) = if let Some(path) = base.strip_prefix("github:") {
        ("github", path)
    } else if let Some(path) = base.strip_prefix("gitlab:") {
        ("gitlab", path)
    } else if let Some(path) = base.strip_prefix("sourcehut:") {
        ("sourcehut", path)
    } else if let Some(path) = base.strip_prefix("https://github.com/") {
        ("github", path)
    } else if let Some(path) = base.strip_prefix("https://gitlab.com/") {
        ("gitlab", path)
    } else {
        let path = base.strip_prefix("flake:").unwrap_or(base);
        if path.is_empty() || path.starts_with('.') || path.starts_with('/') || path.contains(':') {
            return None;
        }
        let mut parts = path.split('/');
        let alias = parts.next()?.to_string();
        if alias.is_empty() {
            return None;
        }
        let tail = parts.collect::<Vec<_>>();
        if tail.len() > 2 || tail.iter().any(|part| part.is_empty()) {
            return None;
        }
        let (mut reference, mut revision) = match tail.as_slice() {
            [] => (None, None),
            [value] if is_flake_revision(value) => (None, Some((*value).to_string())),
            [value] => (Some((*value).to_string()), None),
            [reference, revision] if is_flake_revision(revision) => {
                (Some((*reference).to_string()), Some((*revision).to_string()))
            }
            _ => return None,
        };
        if let Some(value) = query.get("ref") {
            if reference.as_deref().is_some_and(|current| current != value) {
                return None;
            }
            reference = Some(value.clone());
        }
        if let Some(value) = query.get("rev") {
            if revision.as_deref().is_some_and(|current| current != value) {
                return None;
            }
            revision = Some(value.clone());
        }
        return Some(FlakeSourceIdentity::Indirect {
            alias,
            reference,
            revision,
            query,
        });
    };
    let mut parts = path.split('/').filter(|part| !part.is_empty());
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.to_string();
    let tail = parts.collect::<Vec<_>>();
    let (mut reference, mut revision) = match tail.as_slice() {
        [] => (None, None),
        [value] if is_flake_revision(value) => (None, Some((*value).to_string())),
        values => {
            let last = values.last().copied().unwrap_or_default();
            if is_flake_revision(last) {
                (
                    Some(values[..values.len() - 1].join("/")),
                    Some(last.to_string()),
                )
            } else {
                (Some(values.join("/")), None)
            }
        }
    };
    if let Some(value) = query.get("ref") {
        if reference.as_deref().is_some_and(|current| current != value) {
            return None;
        }
        reference = Some(value.clone());
    }
    if let Some(value) = query.get("rev") {
        if revision.as_deref().is_some_and(|current| current != value) {
            return None;
        }
        revision = Some(value.clone());
    };
    Some(FlakeSourceIdentity::Direct {
        kind: kind.to_string(),
        owner,
        repo,
        reference,
        revision,
        query,
    })
}

fn json_string_field<'a>(
    object: &'a BTreeMap<String, crate::JSON::JSONValue>,
    key: &str,
) -> Option<&'a str> {
    object.get(key).and_then(|value| value.as_str().ok())
}

fn source_query_matches(
    query: &BTreeMap<String, String>,
    original: &BTreeMap<String, crate::JSON::JSONValue>,
    locked: &BTreeMap<String, crate::JSON::JSONValue>,
    validate_revision: bool,
) -> bool {
    query.iter().all(|(key, expected)| {
        if key == "rev" && !validate_revision {
            return true;
        }
        let actual = if key == "rev" {
            json_string_field(locked, key)
        } else {
            original
                .get(key)
                .or_else(|| locked.get(key))
                .and_then(|value| value.as_str().ok())
        };
        actual == Some(expected.as_str())
    })
}

fn source_revision_matches(
    identity_revision: Option<&String>,
    input: &FlakeInput,
    locked: &BTreeMap<String, crate::JSON::JSONValue>,
    validate_revision: bool,
) -> bool {
    if !validate_revision {
        return true;
    }
    let locked_revision = json_string_field(locked, "rev");
    identity_revision
        .map(|revision| locked_revision == Some(revision.as_str()))
        .unwrap_or(true)
        && (input.revision.is_empty()
            || locked_revision == Some(input.revision.as_str()))
}

fn validate_root_input_identity(
    input: &FlakeInput,
    node_name: &str,
    node: &FlakeLockNode,
    root_inputs: &BTreeMap<String, crate::JSON::JSONValue>,
    nodes: &BTreeMap<String, crate::JSON::JSONValue>,
    validate_revision: bool,
) -> Result<(), FlakeGraphError> {
    let original = crate::JSON::parse(&node.original)
        .map_err(|error| FlakeGraphError::Io(format!("flake.lock node `{node_name}` is invalid: {error}")))?;
    let original = original.as_object().map_err(|error| {
        FlakeGraphError::Io(format!("flake.lock node `{node_name}` is invalid: {error}"))
    })?;
    let locked = crate::JSON::parse(&node.locked)
        .map_err(|error| FlakeGraphError::Io(format!("flake.lock node `{node_name}` is invalid: {error}")))?;
    let locked = locked.as_object().map_err(|error| {
        FlakeGraphError::Io(format!("flake.lock node `{node_name}` is invalid: {error}"))
    })?;

    if !input.follows.is_empty() {
        let follows = root_inputs.get(&input.follows).ok_or_else(|| {
            FlakeGraphError::Io(format!(
                "flake.lock root input `{}` follows missing root input `{}`",
                input.name, input.follows
            ))
        })?;
        let follows = lock_node_name(Some(follows), nodes, "root")?.ok_or_else(|| {
            FlakeGraphError::Io(format!(
                "flake.lock root input `{}` follows an empty root input `{}`",
                input.name, input.follows
            ))
        })?;
        if follows != node_name {
            return Err(FlakeGraphError::Io(format!(
                "flake.lock root input `{}` follows `{}` but maps to node `{node_name}`",
                input.name, input.follows
            )));
        }
        if validate_revision
            && !input.revision.is_empty()
            && json_string_field(locked, "rev") != Some(input.revision.as_str())
        {
            return Err(FlakeGraphError::Io(format!(
                "flake.lock root input `{}` has a different locked revision",
                input.name
            )));
        }
        return Ok(());
    }

    // Keep malformed indirect metadata fail-closed even when source identity
    // parsing falls back to the generic URL form.
    let _ = indirect_registry_alias(&node.original)?;
    let original_field = |key: &str| json_string_field(original, key);
    let matches = match flake_source_identity(&input.url) {
        Some(FlakeSourceIdentity::Direct {
            kind,
            owner,
            repo,
            reference,
            revision,
            query,
        }) => {
            original_field("type") == Some(kind.as_str())
                && original_field("owner") == Some(owner.as_str())
                && original_field("repo") == Some(repo.as_str())
                && original_field("ref") == reference.as_deref()
                && source_query_matches(&query, original, locked, validate_revision)
                && source_revision_matches(
                    revision.as_ref(),
                    input,
                    locked,
                    validate_revision,
                )
        }
        Some(FlakeSourceIdentity::Indirect {
            alias,
            reference,
            revision,
            query,
        }) => {
            original_field("type") == Some("indirect")
                && original_field("id") == Some(alias.as_str())
                && original_field("ref") == reference.as_deref()
                && source_query_matches(&query, original, locked, validate_revision)
                && source_revision_matches(
                    revision.as_ref(),
                    input,
                    locked,
                    validate_revision,
                )
        }
        None => {
            let source_url = input.url.split_once('?').unwrap_or((input.url.as_str(), ""));
            let original_url = original_field("url")
                .and_then(|url| Some(url.split_once('?').unwrap_or((url, ""))));
            original_url
                .map(|(original_base, original_query)| {
                    original_base == source_url.0
                        && flake_query_fields(original_query)
                            == flake_query_fields(source_url.1)
                })
                .unwrap_or(false)
        }
    };
    if matches {
        Ok(())
    } else {
        Err(FlakeGraphError::Io(format!(
            "flake.lock root input `{}` maps to node `{node_name}` with a different source URL or indirect alias",
            input.name
        )))
    }
}

fn validate_semantic_root_inputs(graph: &FlakeGraph) -> Result<(), FlakeGraphError> {
    let Some(root) = graph.lock_nodes.iter().find(|node| node.name == "root") else {
        return Ok(());
    };
    let nodes = flake_lock_node_values(&graph.lock_nodes)?;
    let root_inputs = root
        .inputs
        .iter()
        .map(|edge| {
            crate::JSON::parse(&edge.target)
                .map(|target| (edge.name.clone(), target))
                .map_err(|error| {
                    FlakeGraphError::StaleSemanticLock(format!(
                        "root input `{}` is invalid: {error}",
                        edge.name
                    ))
                })
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let source_names = graph
        .inputs
        .iter()
        .map(|input| input.name.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(input) = root_inputs
        .keys()
        .find(|input| !source_names.contains(input.as_str()))
    {
        return Err(FlakeGraphError::StaleSemanticLock(format!(
            "root input `{input}` is not declared by flake.nix"
        )));
    }
    for input in &graph.inputs {
        let Some(reference) = root_inputs.get(&input.name) else {
            return Err(FlakeGraphError::StaleSemanticLock(format!(
                "root input `{}` is missing from the lock",
                input.name
            )));
        };
        let Some(node_name) = lock_node_name(Some(reference), &nodes, "root")? else {
            return Err(FlakeGraphError::StaleSemanticLock(format!(
                "root input `{}` has no node reference",
                input.name
            )));
        };
        let Some(node) = graph.lock_nodes.iter().find(|node| node.name == node_name) else {
            return Err(FlakeGraphError::StaleSemanticLock(format!(
                "root input `{}` points to missing node `{node_name}`",
                input.name
            )));
        };
        validate_root_input_identity(
            input,
            &node_name,
            node,
            &root_inputs,
            &nodes,
            true,
        )?;
    }
    Ok(())
}

/// Fill floating input URLs from the sibling `flake.lock`. The source flake
/// remains the authority for URI and follows spelling; the lock contributes
/// only the exact node identity. Missing or malformed lock data stays a hard
/// error instead of turning a floating input into an apparently exact lock.
fn apply_flake_lock(graph: &mut FlakeGraph, text: &str) -> Result<(), FlakeGraphError> {
    let parsed = crate::JSON::parse_lenient(text)
        .map_err(|error| FlakeGraphError::Io(format!("flake.lock is invalid: {error}")))?;
    let root_inputs = parsed
        .value
        .get("nodes")
        .ok()
        .and_then(|value| value.as_object().ok())
        .and_then(|nodes| nodes.get("root"))
        .and_then(|value| value.get("inputs").ok())
        .and_then(|value| value.as_object().ok())
        .ok_or_else(|| {
            FlakeGraphError::Io("flake.lock has no nodes.root.inputs map".to_string())
        })?;
    let nodes = parsed
        .value
        .get("nodes")
        .ok()
        .and_then(|value| value.as_object().ok())
        .ok_or_else(|| FlakeGraphError::Io("flake.lock has no nodes map".to_string()))?;

    graph.lock_nodes = nodes
        .iter()
        .map(|(name, value)| {
            let object = value.as_object().map_err(|error| {
                FlakeGraphError::Io(format!("flake.lock node `{name}` is invalid: {error}"))
            })?;
            if name.is_empty() {
                return Err(FlakeGraphError::Io(
                    "flake.lock node name is empty".to_string(),
                ));
            }
            let has_original = object.contains_key("original");
            let has_locked = object.contains_key("locked");
            if name == "root" {
                if has_original || has_locked {
                    return Err(FlakeGraphError::Io(
                        "flake.lock root node must not carry original/locked metadata".to_string(),
                    ));
                }
            } else if !has_original || !has_locked {
                let missing = if !has_original { "original" } else { "locked" };
                return Err(FlakeGraphError::Io(format!(
                    "flake.lock node `{name}` is missing `{missing}` metadata"
                )));
            }
            if name != "root" {
                let original = object
                    .get("original")
                    .and_then(|value| value.as_object().ok())
                    .ok_or_else(|| {
                        FlakeGraphError::Io(format!(
                            "flake.lock node `{name}` original metadata is not an object"
                        ))
                    })?;
                let locked = object
                    .get("locked")
                    .and_then(|value| value.as_object().ok())
                    .ok_or_else(|| {
                        FlakeGraphError::Io(format!(
                            "flake.lock node `{name}` locked metadata is not an object"
                        ))
                    })?;
                if original.is_empty() || locked.is_empty() {
                    return Err(FlakeGraphError::Io(format!(
                        "flake.lock node `{name}` has empty original/locked metadata"
                    )));
                }
            }
            let inputs = match object.get("inputs") {
                None => Vec::new(),
                Some(value) => value
                    .as_object()
                    .map_err(|error| {
                        FlakeGraphError::Io(format!(
                            "flake.lock node `{name}` inputs are invalid: {error}"
                        ))
                    })?
                    .iter()
                    .map(|(input, target)| FlakeLockEdge {
                        name: input.clone(),
                        target: canonical_json(target),
                    })
                    .collect(),
            };
            Ok(FlakeLockNode {
                name: name.clone(),
                original: canonical_object(object, "original"),
                locked: canonical_object(object, "locked"),
                inputs,
            })
        })
        .collect::<Result<Vec<_>, FlakeGraphError>>()?;
    graph.lock_nodes.sort();

    let lock_names = graph
        .lock_nodes
        .iter()
        .map(|node| node.name.as_str())
        .collect::<BTreeSet<_>>();
    for (input, reference) in root_inputs {
        let Some(node) = lock_node_name(Some(reference), nodes, "root")? else {
            return Err(FlakeGraphError::Io(format!(
                "flake.lock root input `{input}` has no node reference"
            )));
        };
        if !lock_names.contains(node.as_str()) {
            return Err(FlakeGraphError::Io(format!(
                "flake.lock root input points to missing node `{node}`"
            )));
        }
    }
    let source_names = graph
        .inputs
        .iter()
        .map(|input| input.name.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(input) = root_inputs.keys().find(|input| !source_names.contains(input.as_str())) {
        return Err(FlakeGraphError::Io(format!(
            "flake.lock root input `{input}` is not declared by flake.nix"
        )));
    }
    let mut registries = Vec::new();
    for node in &graph.lock_nodes {
        let Some(alias) = indirect_registry_alias(&node.original)? else {
            continue;
        };
        registries.push(FlakeRegistryEntry {
            alias,
            node: node.name.clone(),
            original: node.original.clone(),
            locked: node.locked.clone(),
        });
    }
    graph.registries = registries;
    graph.registries.sort();

    for input in &mut graph.inputs {
        let Some(reference) = root_inputs.get(&input.name) else {
            return Err(FlakeGraphError::Io(format!(
                "flake.lock has no root input `{}`",
                input.name
            )));
        };
        let Some(node_name) = lock_node_name(Some(reference), nodes, "root")? else {
            return Err(FlakeGraphError::Io(format!(
                "flake.lock root input `{}` has no node reference",
                input.name
            )));
        };
        let Some(node) = graph.lock_nodes.iter().find(|node| node.name == node_name) else {
            return Err(FlakeGraphError::Io(format!(
                "flake.lock root input `{}` points to missing node `{node_name}`",
                input.name
            )));
        };
        validate_root_input_identity(input, &node_name, node, root_inputs, nodes, false)?;
        let locked = crate::JSON::parse(&node.locked)
            .map_err(|error| FlakeGraphError::Io(format!("flake.lock node `{node_name}` is invalid: {error}")))?;
        let locked = locked.as_object().map_err(|error| {
            FlakeGraphError::Io(format!("flake.lock node `{node_name}` is invalid: {error}"))
        })?;
        let Some(exact) = locked.get("rev").and_then(|value| value.as_str().ok()) else {
            if !input.revision.is_empty() || !input.follows.is_empty() {
                continue;
            }
            return Err(FlakeGraphError::MissingRevision {
                input: input.name.clone(),
            });
        };
        if input.revision.is_empty() {
            input.revision = exact.to_string();
        } else if input.revision != exact {
            return Err(FlakeGraphError::ConflictingInput {
                input: input.name.clone(),
                field: "revision".to_string(),
            });
        }
    }
    Ok(())
}

fn lock_node_name(
    value: Option<&crate::JSON::JSONValue>,
    nodes: &BTreeMap<String, crate::JSON::JSONValue>,
    start_node: &str,
) -> Result<Option<String>, FlakeGraphError> {
    let Some(value) = value else {
        return Ok(None);
    };
    fn resolve(
        value: &crate::JSON::JSONValue,
        nodes: &BTreeMap<String, crate::JSON::JSONValue>,
        path: &mut BTreeSet<String>,
        start_node: &str,
    ) -> Result<Option<String>, FlakeGraphError> {
        match value {
            crate::JSON::JSONValue::Str(value) if !value.is_empty() => Ok(Some(value.clone())),
            crate::JSON::JSONValue::Str(_) => Err(FlakeGraphError::Io(
                "flake.lock node reference is empty".to_string(),
            )),
            crate::JSON::JSONValue::Array(values) => {
                if values.is_empty() {
                    return Ok(None);
                }
                // Nix uses a one-element array as a direct node reference
                // when an input follows a node outside the current input
                // map. Resolve that form before treating the array as an
                // input path; otherwise `flake-utils.inputs.systems =
                // ["systems"]` asks for `systems` twice and looks like a
                // cycle.
                if values.len() == 1 {
                    if let Some(node_name) = values[0].as_str().ok().filter(|name| nodes.contains_key(*name)) {
                        return Ok(Some(node_name.to_string()));
                    }
                }
                let mut current = start_node.to_string();
                for value in values {
                    let segment = value.as_str().map_err(|error| {
                        FlakeGraphError::Io(format!(
                            "flake.lock node reference array must contain strings: {error}"
                        ))
                    })?;
                    if segment.is_empty() {
                        return Err(FlakeGraphError::Io(
                            "flake.lock node reference array must contain non-empty strings"
                                .to_string(),
                        ));
                    }
                    let path_key = format!("{current}\0{segment}");
                    if !path.insert(path_key.clone()) {
                        return Err(FlakeGraphError::Io(format!(
                            "flake.lock follow path cycles at `{segment}`"
                        )));
                    }
                    let node = nodes.get(&current).ok_or_else(|| {
                        FlakeGraphError::Io(format!(
                            "flake.lock follow path starts at missing node `{current}`"
                        ))
                    })?;
                    let inputs = node
                        .get("inputs")
                        .ok()
                        .and_then(|value| value.as_object().ok())
                        .ok_or_else(|| {
                            FlakeGraphError::Io(format!(
                                "flake.lock follow path node `{current}` has no inputs map"
                            ))
                        })?;
                    let target = inputs.get(segment).ok_or_else(|| {
                        FlakeGraphError::Io(format!(
                            "flake.lock follow path `{segment}` is missing from node `{current}`"
                        ))
                    })?;
                    current = resolve(target, nodes, path, &current)?.ok_or_else(|| {
                        FlakeGraphError::Io(format!(
                            "flake.lock follow path `{segment}` resolves to an empty node reference"
                        ))
                    })?;
                    path.remove(&path_key);
                }
                Ok(Some(current))
            }
            _ => Err(FlakeGraphError::Io(
                "flake.lock node reference must be a node name or node list".to_string(),
            )),
        }
    }

    resolve(value, nodes, &mut BTreeSet::new(), start_node)
}

fn propagate_follows(graph: &mut FlakeGraph) -> Result<(), FlakeGraphError> {
    let by_name = graph
        .inputs
        .iter()
        .map(|input| (input.name.clone(), (input.revision.clone(), input.follows.clone())))
        .collect::<BTreeMap<_, _>>();

    fn resolve(
        name: &str,
        facts: &BTreeMap<String, (String, String)>,
        stack: &mut BTreeSet<String>,
    ) -> Result<Option<String>, FlakeGraphError> {
        let Some((revision, follows)) = facts.get(name) else {
            return Ok(None);
        };
        if !revision.is_empty() {
            return Ok(Some(revision.clone()));
        }
        if follows.is_empty() {
            return Ok(None);
        }
        if !stack.insert(name.to_string()) {
            return Err(FlakeGraphError::FollowsCycle(name.to_string()));
        }
        let result = resolve(follows, facts, stack);
        stack.remove(name);
        result
    }

    for input in &mut graph.inputs {
        if !input.follows.is_empty() {
            if let Some(revision) = resolve(&input.follows, &by_name, &mut BTreeSet::new())? {
                input.revision = revision;
            }
        }
    }
    Ok(())
}

/// Package-pattern → allowed source authorities (dependency-confusion guard).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceMapEntry {
    pub pattern: String,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SemanticLockFile {
    pub records: Vec<SemanticRecord>,
    pub inputs: Vec<LockInput>,
    pub source_maps: Vec<SourceMapEntry>,
}

impl SemanticLockFile {
    pub fn with_records(records: Vec<SemanticRecord>) -> SemanticLockFile {
        SemanticLockFile {
            records,
            ..Default::default()
        }
    }
}

pub fn write(lock: &SemanticLockFile) -> String {
    let mut records = lock.records.clone();
    records.sort_by(|a, b| a.identity.semantic_key().cmp(&b.identity.semantic_key()));
    let mut inputs = lock.inputs.clone();
    inputs.sort();
    let mut source_maps = lock.source_maps.clone();
    source_maps.sort();
    let mut out = String::from("semantic-lock-version = 1\n");
    for input in inputs {
        out.push_str("\n[[lock_input]]\n");
        out.push_str(&line("name", &input.name));
        out.push_str(&line("url", &input.url));
        out.push_str(&line("follows", &input.follows));
    }
    for map in source_maps {
        out.push_str("\n[[source_map]]\n");
        out.push_str(&line("pattern", &map.pattern));
        out.push_str(&line("sources", &format!("[{}]", map.sources.iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(", "))));
    }
    for rec in records {
        out.push_str("\n[[semantic_record]]\n");
        out.push_str(&line("kind", rec.identity.kind.as_str()));
        out.push_str(&line("key", &rec.identity.key));
        out.push_str(&line("exact", &rec.identity.exact));
        out.push_str(&line("hash", &rec.identity.hash));
        out.push_str(&line("platform", &rec.identity.platform));
        for (k, v) in rec.future_fields {
            out.push_str(&line(&k, &v));
        }
        for rationale in rec.rationales {
            out.push_str("[[semantic_record.rationale]]\n");
            out.push_str(&line("owner-package", &rationale.owner_package));
            out.push_str(&line("reason", &rationale.reason));
            out.push_str(&line("source-ref", &rationale.source_ref));
            out.push_str(&line("provider", &rationale.provider));
            out.push_str(&line("channel-input", &rationale.channel_input));
            out.push_str(&line("exact-output", &rationale.exact_output));
            out.push_str(&line("policy-fingerprint", &rationale.policy_fingerprint));
            out.push_str(&line("recipe-id", &rationale.recipe_id));
            out.push_str(&line("adapter-id", &rationale.adapter_id));
            out.push_str(&line("signature", &rationale.signature));
            out.push_str(&line("cache-provenance", &rationale.cache_provenance));
            out.push_str(&line("update-command", &rationale.update_command));
        }
    }
    out
}

fn line(key: &str, value: &str) -> String {
    format!(
        "{key} = \"{}\"\n",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

pub fn parse(raw: &str) -> SemanticLockFile {
    let mut records = Vec::new();
    let mut inputs = Vec::new();
    let mut source_maps = Vec::new();
    let mut current: Option<PartialRecord> = None;
    let mut current_rationale: Option<LockRationale> = None;
    let mut current_input: Option<PartialInput> = None;
    let mut current_source_map: Option<PartialSourceMap> = None;

    for raw_line in raw.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line {
            "[[semantic_record]]" => {
                flush_rationale(&mut current, &mut current_rationale);
                flush_record(&mut records, &mut current);
                flush_input(&mut inputs, &mut current_input);
                flush_source_map(&mut source_maps, &mut current_source_map);
                current = Some(PartialRecord::default());
                continue;
            }
            "[[semantic_record.rationale]]" => {
                flush_rationale(&mut current, &mut current_rationale);
                current_rationale = Some(LockRationale::default());
                continue;
            }
            "[[lock_input]]" => {
                flush_rationale(&mut current, &mut current_rationale);
                flush_record(&mut records, &mut current);
                flush_input(&mut inputs, &mut current_input);
                flush_source_map(&mut source_maps, &mut current_source_map);
                current_input = Some(PartialInput::default());
                continue;
            }
            "[[source_map]]" => {
                flush_rationale(&mut current, &mut current_rationale);
                flush_record(&mut records, &mut current);
                flush_input(&mut inputs, &mut current_input);
                flush_source_map(&mut source_maps, &mut current_source_map);
                current_source_map = Some(PartialSourceMap::default());
                continue;
            }
            _ => {}
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let val = val
            .trim()
            .trim_matches('"')
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
        if let Some(rationale) = &mut current_rationale {
            set_rationale(rationale, key, val);
        } else if let Some(input) = &mut current_input {
            input.set(key, val);
        } else if let Some(map) = &mut current_source_map {
            map.set(key, val);
        } else if let Some(record) = &mut current {
            record.set(key, val);
        }
    }
    flush_rationale(&mut current, &mut current_rationale);
    flush_record(&mut records, &mut current);
    flush_input(&mut inputs, &mut current_input);
    flush_source_map(&mut source_maps, &mut current_source_map);
    SemanticLockFile {
        records,
        inputs,
        source_maps,
    }
}

fn flush_rationale(
    current: &mut Option<PartialRecord>,
    current_rationale: &mut Option<LockRationale>,
) {
    if let (Some(record), Some(rationale)) = (current, current_rationale.take()) {
        record.rationales.push(rationale);
    }
}

fn flush_record(records: &mut Vec<SemanticRecord>, current: &mut Option<PartialRecord>) {
    if let Some(record) = current.take().and_then(PartialRecord::finish) {
        records.push(record);
    }
}

fn flush_input(inputs: &mut Vec<LockInput>, current: &mut Option<PartialInput>) {
    if let Some(input) = current.take().and_then(PartialInput::finish) {
        inputs.push(input);
    }
}

fn flush_source_map(maps: &mut Vec<SourceMapEntry>, current: &mut Option<PartialSourceMap>) {
    if let Some(map) = current.take().and_then(PartialSourceMap::finish) {
        maps.push(map);
    }
}

fn set_rationale(r: &mut LockRationale, key: &str, value: String) {
    match key {
        "owner-package" => r.owner_package = value,
        "reason" => r.reason = value,
        "source-ref" => r.source_ref = value,
        "provider" => r.provider = value,
        "channel-input" => r.channel_input = value,
        "exact-output" => r.exact_output = value,
        "policy-fingerprint" => r.policy_fingerprint = value,
        "recipe-id" => r.recipe_id = value,
        "adapter-id" => r.adapter_id = value,
        "signature" => r.signature = value,
        "cache-provenance" => r.cache_provenance = value,
        "update-command" => r.update_command = value,
        _ => {}
    }
}

#[derive(Default)]
struct PartialInput {
    name: Option<String>,
    url: Option<String>,
    follows: String,
}

impl PartialInput {
    fn set(&mut self, key: &str, value: String) {
        match key {
            "name" => self.name = Some(value),
            "url" => self.url = Some(value),
            "follows" => self.follows = value,
            _ => {}
        }
    }

    fn finish(self) -> Option<LockInput> {
        Some(LockInput {
            name: self.name?,
            url: self.url.unwrap_or_default(),
            follows: self.follows,
        })
    }
}

#[derive(Default)]
struct PartialSourceMap {
    pattern: Option<String>,
    sources: Vec<String>,
}

impl PartialSourceMap {
    fn set(&mut self, key: &str, value: String) {
        match key {
            "pattern" => self.pattern = Some(value),
            "sources" => self.sources = parse_string_list(&value),
            "source" => self.sources.push(value),
            _ => {}
        }
    }

    fn finish(self) -> Option<SourceMapEntry> {
        Some(SourceMapEntry {
            pattern: self.pattern?,
            sources: self.sources,
        })
    }
}

fn parse_string_list(raw: &str) -> Vec<String> {
    let raw = raw.trim().trim_start_matches('[').trim_end_matches(']');
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[derive(Default)]
struct PartialRecord {
    kind: Option<LockRecordKind>,
    key: Option<String>,
    exact: Option<String>,
    hash: Option<String>,
    platform: Option<String>,
    rationales: Vec<LockRationale>,
    future_fields: BTreeMap<String, String>,
}

impl PartialRecord {
    fn set(&mut self, key: &str, value: String) {
        match key {
            "kind" => self.kind = Some(LockRecordKind::parse(&value)),
            "key" => self.key = Some(value),
            "exact" => self.exact = Some(value),
            "hash" => self.hash = Some(value),
            "platform" => self.platform = Some(value),
            other if other != "semantic-lock-version" => {
                self.future_fields.insert(other.to_string(), value);
            }
            _ => {}
        }
    }

    fn finish(self) -> Option<SemanticRecord> {
        Some(SemanticRecord {
            identity: LockIdentity {
                kind: self.kind?,
                key: self.key?,
                exact: self.exact.unwrap_or_default(),
                hash: self.hash.unwrap_or_default(),
                platform: self.platform.unwrap_or_default(),
            },
            rationales: self.rationales,
            future_fields: self.future_fields,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockConflict {
    pub semantic_key: String,
    pub owner_package: String,
    pub left_identity: String,
    pub right_identity: String,
    pub left_reason: String,
    pub right_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeOutcome {
    pub merged: SemanticLockFile,
    pub conflicts: Vec<LockConflict>,
}

pub fn merge(
    base: &SemanticLockFile,
    left: &SemanticLockFile,
    right: &SemanticLockFile,
) -> MergeOutcome {
    let base_by_key = records_by_key(base);
    let left_by_key = records_by_key(left);
    let right_by_key = records_by_key(right);
    let mut keys = BTreeSet::new();
    keys.extend(left_by_key.keys().cloned());
    keys.extend(right_by_key.keys().cloned());

    let mut by_key = BTreeMap::new();
    let mut conflicts = Vec::new();
    for key in keys {
        match (left_by_key.get(&key), right_by_key.get(&key)) {
            (Some(left_rec), None) => {
                by_key.insert(key, (*left_rec).clone());
            }
            (None, Some(right_rec)) => {
                by_key.insert(key, (*right_rec).clone());
            }
            (Some(left_rec), Some(right_rec)) => {
                if left_rec.identity.machine_identity() == right_rec.identity.machine_identity() {
                    let mut merged = (*left_rec).clone();
                    merge_rationales(&mut merged, &right_rec.rationales);
                    by_key.insert(key, merged);
                    continue;
                }
                let base_rec = base_by_key.get(&key).copied();
                if base_rec.is_some_and(|base_rec| {
                    left_rec.identity.machine_identity() == base_rec.identity.machine_identity()
                }) {
                    by_key.insert(key, (*right_rec).clone());
                    continue;
                }
                if base_rec.is_some_and(|base_rec| {
                    right_rec.identity.machine_identity() == base_rec.identity.machine_identity()
                }) {
                    by_key.insert(key, (*left_rec).clone());
                    continue;
                }
                if let Some(conflict) = conflict_for(left_rec, right_rec) {
                    conflicts.push(conflict);
                } else {
                    let mut merged = (*left_rec).clone();
                    merge_rationales(&mut merged, &right_rec.rationales);
                    by_key.insert(key, merged);
                }
            }
            (None, None) => {}
        }
    }
    MergeOutcome {
        merged: SemanticLockFile {
            records: by_key.into_values().collect(),
            inputs: merge_inputs(base, left, right),
            source_maps: merge_source_maps(base, left, right),
        },
        conflicts,
    }
}

fn merge_inputs(
    base: &SemanticLockFile,
    left: &SemanticLockFile,
    right: &SemanticLockFile,
) -> Vec<LockInput> {
    let mut by_name: BTreeMap<String, LockInput> = BTreeMap::new();
    for input in base
        .inputs
        .iter()
        .chain(left.inputs.iter())
        .chain(right.inputs.iter())
    {
        by_name.insert(input.name.clone(), input.clone());
    }
    // Prefer right over left over base for same name when urls differ.
    for input in left.inputs.iter().chain(right.inputs.iter()) {
        by_name.insert(input.name.clone(), input.clone());
    }
    by_name.into_values().collect()
}

fn merge_source_maps(
    base: &SemanticLockFile,
    left: &SemanticLockFile,
    right: &SemanticLockFile,
) -> Vec<SourceMapEntry> {
    let mut by_pattern: BTreeMap<String, SourceMapEntry> = BTreeMap::new();
    for map in base
        .source_maps
        .iter()
        .chain(left.source_maps.iter())
        .chain(right.source_maps.iter())
    {
        by_pattern.insert(map.pattern.clone(), map.clone());
    }
    for map in left.source_maps.iter().chain(right.source_maps.iter()) {
        by_pattern.insert(map.pattern.clone(), map.clone());
    }
    by_pattern.into_values().collect()
}

fn records_by_key(lock: &SemanticLockFile) -> BTreeMap<String, &SemanticRecord> {
    lock.records
        .iter()
        .map(|record| (record.identity.semantic_key(), record))
        .collect()
}

fn merge_rationales(existing: &mut SemanticRecord, incoming: &[LockRationale]) {
    let mut seen: BTreeSet<(String, String)> = existing
        .rationales
        .iter()
        .map(|r| (r.owner_package.clone(), r.reason.clone()))
        .collect();
    for r in incoming {
        if seen.insert((r.owner_package.clone(), r.reason.clone())) {
            existing.rationales.push(r.clone());
        }
    }
}

fn conflict_for(left: &SemanticRecord, right: &SemanticRecord) -> Option<LockConflict> {
    for l in &left.rationales {
        for r in &right.rationales {
            if l.owner_package == r.owner_package {
                return Some(LockConflict {
                    semantic_key: left.identity.semantic_key(),
                    owner_package: l.owner_package.clone(),
                    left_identity: left.identity.machine_identity(),
                    right_identity: right.identity.machine_identity(),
                    left_reason: l.reason.clone(),
                    right_reason: r.reason.clone(),
                });
            }
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainFact {
    pub semantic_key: String,
    pub owners: Vec<String>,
    pub provider: String,
    pub platform: String,
    pub exact_artifact: String,
    pub policy_fingerprint: String,
    pub update_command: String,
    pub offline_satisfied: bool,
}

pub fn explain(lock: &SemanticLockFile, key: &str) -> Option<ExplainFact> {
    let rec = lock
        .records
        .iter()
        .find(|r| r.identity.semantic_key() == key)?;
    let mut owners: Vec<String> = rec
        .rationales
        .iter()
        .map(|r| r.owner_package.clone())
        .filter(|s| !s.is_empty())
        .collect();
    owners.sort();
    owners.dedup();
    let first = rec.rationales.first().cloned().unwrap_or_default();
    Some(ExplainFact {
        semantic_key: key.to_string(),
        owners,
        provider: first.provider,
        platform: rec.identity.platform.clone(),
        exact_artifact: rec.identity.exact.clone(),
        policy_fingerprint: first.policy_fingerprint,
        update_command: first.update_command,
        offline_satisfied: !rec.identity.hash.is_empty(),
    })
}

/// Record a selected typed variant into the semantic lock (E4-JP15).
/// `platform` holds the canonical `PackageVariant::identity_key()`.
pub fn record_selected_variant(
    lock: &mut SemanticLockFile,
    package: &str,
    variant_identity: &str,
    hash: &str,
    reason: &str,
) {
    let identity = LockIdentity {
        kind: LockRecordKind::Variant,
        key: package.to_string(),
        exact: variant_identity.to_string(),
        hash: hash.to_string(),
        platform: variant_identity.to_string(),
    };
    let rationale = LockRationale {
        owner_package: package.to_string(),
        reason: reason.to_string(),
        source_ref: String::new(),
        provider: "native".to_string(),
        channel_input: String::new(),
        exact_output: variant_identity.to_string(),
        policy_fingerprint: String::new(),
        recipe_id: String::new(),
        adapter_id: String::new(),
        signature: String::new(),
        cache_provenance: String::new(),
        update_command: String::new(),
    };
    // Replace existing variant record for the same package key.
    lock.records
        .retain(|r| !(r.identity.kind == LockRecordKind::Variant && r.identity.key == package));
    lock.records
        .push(SemanticRecord::new(identity, rationale));
}

/// Identity keys of every locked variant domain (universal lock coverage).
pub fn locked_variant_domains(lock: &SemanticLockFile) -> BTreeSet<String> {
    lock.records
        .iter()
        .filter(|r| r.identity.kind == LockRecordKind::Variant)
        .map(|r| r.identity.exact.clone())
        .collect()
}

/// Record a workspace catalog selection into the semantic lock (E4-JP13).
pub fn record_catalog_selection(
    lock: &mut SemanticLockFile,
    owner: &str,
    logical_name: &str,
    provider_ref: &str,
    hash: &str,
    platform: &str,
    rationale: &str,
) {
    let identity = LockIdentity {
        kind: LockRecordKind::Package,
        key: logical_name.to_string(),
        exact: provider_ref.to_string(),
        hash: hash.to_string(),
        platform: platform.to_string(),
    };
    let rationale = LockRationale {
        owner_package: owner.to_string(),
        reason: rationale.to_string(),
        source_ref: format!("catalog:{logical_name}"),
        provider: provider_ref
            .split(':')
            .next()
            .unwrap_or("catalog")
            .to_string(),
        channel_input: String::new(),
        exact_output: provider_ref.to_string(),
        policy_fingerprint: String::new(),
        recipe_id: String::new(),
        adapter_id: "workspace.catalog".to_string(),
        signature: String::new(),
        cache_provenance: String::new(),
        update_command: format!("jet update {logical_name}"),
    };
    lock.records
        .retain(|r| !(r.identity.kind == LockRecordKind::Package && r.identity.key == logical_name));
    lock.records
        .push(SemanticRecord::new(identity, rationale));
}

// ── E4-JP13 live path, selective update, revalidation ───────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationIssue {
    InputCycle(String),
    MissingFollows { input: String, follows: String },
    MissingHash { semantic_key: String },
    MissingSignature { semantic_key: String },
    DomainConflict { key: String, left: String, right: String },
    SourceAuthority { package: String, source: String, allowed: Vec<String> },
    OfflineIncomplete { semantic_key: String },
}

impl ValidationIssue {
    pub fn message(&self) -> String {
        match self {
            ValidationIssue::InputCycle(name) => {
                format!("lock input `{name}` participates in a follows cycle")
            }
            ValidationIssue::MissingFollows { input, follows } => {
                format!("lock input `{input}` follows unknown input `{follows}`")
            }
            ValidationIssue::MissingHash { semantic_key } => {
                format!("`{semantic_key}` has empty content hash — offline realize cannot run")
            }
            ValidationIssue::MissingSignature { semantic_key } => {
                format!("`{semantic_key}` requires a signature but the slot is empty")
            }
            ValidationIssue::DomainConflict { key, left, right } => {
                format!("platform domain conflict for `{key}`: `{left}` vs `{right}`")
            }
            ValidationIssue::SourceAuthority {
                package,
                source,
                allowed,
            } => format!(
                "package `{package}` source `{source}` is not in source map [{}]",
                allowed.join(", ")
            ),
            ValidationIssue::OfflineIncomplete { semantic_key } => {
                format!("`{semantic_key}` is not offline-complete")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockCommitError {
    pub issues: Vec<ValidationIssue>,
    pub io: Option<String>,
}

impl LockCommitError {
    pub fn message(&self) -> String {
        if let Some(io) = &self.io {
            return io.clone();
        }
        self.issues
            .iter()
            .map(ValidationIssue::message)
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Revalidate before any atomic write (E4-JP13).
pub fn revalidate(lock: &SemanticLockFile) -> Result<(), Vec<ValidationIssue>> {
    let mut issues = Vec::new();
    issues.extend(validate_input_graph(&lock.inputs));
    issues.extend(validate_records(lock));
    issues.extend(validate_source_maps(lock));
    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn validate_input_graph(inputs: &[LockInput]) -> Vec<ValidationIssue> {
    let names: BTreeSet<String> = inputs.iter().map(|i| i.name.clone()).collect();
    let mut issues = Vec::new();
    for input in inputs {
        if !input.follows.is_empty() && !names.contains(&input.follows) {
            issues.push(ValidationIssue::MissingFollows {
                input: input.name.clone(),
                follows: input.follows.clone(),
            });
        }
    }
    let mut adj: BTreeMap<String, String> = BTreeMap::new();
    for input in inputs {
        if !input.follows.is_empty() && names.contains(&input.follows) {
            adj.insert(input.name.clone(), input.follows.clone());
        }
    }
    let mut reported = BTreeSet::new();
    for name in &names {
        if let Some(cycle_node) = find_cycle_from(name, &adj) {
            if reported.insert(cycle_node.clone()) {
                issues.push(ValidationIssue::InputCycle(cycle_node));
            }
        }
    }
    issues
}

fn find_cycle_from(start: &str, adj: &BTreeMap<String, String>) -> Option<String> {
    let mut slow = start.to_string();
    let mut fast = start.to_string();
    loop {
        let Some(next_slow) = adj.get(&slow) else {
            return None;
        };
        slow = next_slow.clone();
        let Some(next_fast) = adj.get(&fast).and_then(|n| adj.get(n)) else {
            return None;
        };
        fast = next_fast.clone();
        if slow == fast {
            return Some(slow);
        }
    }
}

fn validate_records(lock: &SemanticLockFile) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let mut seen_platform: BTreeMap<String, String> = BTreeMap::new();
    for rec in &lock.records {
        let key = rec.identity.semantic_key();
        if rec.identity.hash.is_empty() {
            issues.push(ValidationIssue::MissingHash {
                semantic_key: key.clone(),
            });
            issues.push(ValidationIssue::OfflineIncomplete {
                semantic_key: key.clone(),
            });
        }
        for rationale in &rec.rationales {
            if rationale.cache_provenance.contains("signed") && rationale.signature.is_empty() {
                issues.push(ValidationIssue::MissingSignature {
                    semantic_key: key.clone(),
                });
            }
        }
        if !rec.identity.platform.is_empty() {
            if let Some(prev) = seen_platform.get(&key) {
                if prev != &rec.identity.platform {
                    issues.push(ValidationIssue::DomainConflict {
                        key: key.clone(),
                        left: prev.clone(),
                        right: rec.identity.platform.clone(),
                    });
                }
            } else {
                seen_platform.insert(key, rec.identity.platform.clone());
            }
        }
    }
    issues
}

fn validate_source_maps(lock: &SemanticLockFile) -> Vec<ValidationIssue> {
    if lock.source_maps.is_empty() {
        return Vec::new();
    }
    let mut issues = Vec::new();
    for rec in &lock.records {
        if rec.identity.kind != LockRecordKind::Package {
            continue;
        }
        let Some(map) = matching_source_map(lock, &rec.identity.key) else {
            continue;
        };
        let source = rec
            .rationales
            .first()
            .map(|r| {
                if !r.source_ref.is_empty() {
                    r.source_ref.clone()
                } else {
                    r.provider.clone()
                }
            })
            .unwrap_or_default();
        if source.is_empty() {
            continue;
        }
        let allowed = &map.sources;
        let ok = allowed.iter().any(|a| source_matches(a, &source));
        if !ok {
            issues.push(ValidationIssue::SourceAuthority {
                package: rec.identity.key.clone(),
                source,
                allowed: allowed.clone(),
            });
        }
    }
    issues
}

fn matching_source_map<'a>(
    lock: &'a SemanticLockFile,
    package: &str,
) -> Option<&'a SourceMapEntry> {
    lock.source_maps.iter().find(|m| pattern_matches(&m.pattern, package))
}

fn pattern_matches(pattern: &str, package: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        package.starts_with(prefix)
    } else {
        pattern == package
    }
}

fn source_matches(allowed: &str, source: &str) -> bool {
    source == allowed || source.starts_with(allowed) || allowed.starts_with(source)
}

/// Selective update: replace one semantic key; unrelated records stay stable.
pub fn selective_update(
    lock: &mut SemanticLockFile,
    new_record: SemanticRecord,
) -> Option<SemanticRecord> {
    let key = new_record.identity.semantic_key();
    let previous = lock
        .records
        .iter()
        .position(|r| r.identity.semantic_key() == key)
        .map(|idx| lock.records.remove(idx));
    lock.records.push(new_record);
    previous
}

/// Exact overlay/patch invalidation facts (E4-JP13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayInvalidation {
    pub overlay: String,
    pub package: String,
    pub policy_fingerprint_before: String,
    pub policy_fingerprint_after: String,
    pub affected_action_keys: Vec<String>,
    pub reason: String,
}

/// Diff two overlay policies and name exactly which action keys must rebuild.
pub fn overlay_invalidations(
    before_fingerprints: &BTreeMap<(String, String), String>,
    after_fingerprints: &BTreeMap<(String, String), String>,
    actions_by_package: &BTreeMap<String, Vec<String>>,
) -> Vec<OverlayInvalidation> {
    let mut keys: BTreeSet<(String, String)> = BTreeSet::new();
    keys.extend(before_fingerprints.keys().cloned());
    keys.extend(after_fingerprints.keys().cloned());
    let mut out = Vec::new();
    for (overlay, package) in keys {
        let before = before_fingerprints
            .get(&(overlay.clone(), package.clone()))
            .cloned()
            .unwrap_or_default();
        let after = after_fingerprints
            .get(&(overlay.clone(), package.clone()))
            .cloned()
            .unwrap_or_default();
        if before == after {
            continue;
        }
        let affected = actions_by_package
            .get(&package)
            .cloned()
            .unwrap_or_else(|| vec![format!("action:{package}")]);
        let reason = if before.is_empty() {
            format!("overlay `{overlay}` added package `{package}`")
        } else if after.is_empty() {
            format!("overlay `{overlay}` removed package `{package}`")
        } else {
            format!(
                "overlay `{overlay}` changed package `{package}` policy ({before} → {after})"
            )
        };
        out.push(OverlayInvalidation {
            overlay,
            package,
            policy_fingerprint_before: before,
            policy_fingerprint_after: after,
            affected_action_keys: affected,
            reason,
        });
    }
    out
}

/// Apply overlay invalidations: drop stale overlay records; clear hashes of
/// affected packages so offline realize fails until rebuild.
pub fn apply_overlay_invalidations(
    lock: &mut SemanticLockFile,
    invalidations: &[OverlayInvalidation],
) {
    for inv in invalidations {
        let overlay_key = format!("{}:{}", inv.overlay, inv.package);
        lock.records.retain(|r| {
            !(r.identity.kind == LockRecordKind::PackageOverlay
                && r.identity.key == overlay_key)
        });
        for rec in &mut lock.records {
            if rec.identity.key == inv.package
                || inv.affected_action_keys.iter().any(|a| {
                    a == &rec.identity.key || a.ends_with(&format!(":{}", rec.identity.key))
                })
            {
                rec.identity.hash.clear();
            }
        }
    }
}

/// Project lock path (`.jet/lock`).
pub fn live_path(project: &Path) -> PathBuf {
    crate::Store::lock_path(project)
}

/// Load semantic sections from the live unified lock (ignores machine-only
/// `[[package]]` / `[[toolchain]]` blocks — Lock::parse still owns those).
pub fn load(project: &Path) -> Option<SemanticLockFile> {
    let raw = std::fs::read_to_string(live_path(project)).ok()?;
    let lock = parse(&raw);
    if lock.records.is_empty() && lock.inputs.is_empty() && lock.source_maps.is_empty() {
        // Distinguish "no semantic content" from "file missing".
        if raw.contains("[[semantic_record]]")
            || raw.contains("[[lock_input]]")
            || raw.contains("[[source_map]]")
            || raw.contains("semantic-lock-version")
        {
            return Some(lock);
        }
        return Some(SemanticLockFile::default());
    }
    Some(lock)
}

/// Strip semantic sections so machine Lock::parse/write keep their bytes.
pub fn strip_semantic_sections(raw: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            skipping = matches!(
                trimmed,
                "[[semantic_record]]"
                    | "[[semantic_record.rationale]]"
                    | "[[lock_input]]"
                    | "[[source_map]]"
            );
            if skipping {
                continue;
            }
        }
        if skipping {
            continue;
        }
        if trimmed.starts_with("semantic-lock-version") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Merge three-way, revalidate, then atomically write into `.jet/lock`.
pub fn merge_revalidate_commit(
    project: &Path,
    base: &SemanticLockFile,
    left: &SemanticLockFile,
    right: &SemanticLockFile,
) -> Result<SemanticLockFile, LockCommitError> {
    let outcome = merge(base, left, right);
    if !outcome.conflicts.is_empty() {
        return Err(LockCommitError {
            issues: outcome
                .conflicts
                .iter()
                .map(|c| ValidationIssue::DomainConflict {
                    key: c.semantic_key.clone(),
                    left: c.left_identity.clone(),
                    right: c.right_identity.clone(),
                })
                .collect(),
            io: None,
        });
    }
    atomic_commit(project, &outcome.merged)?;
    Ok(outcome.merged)
}

/// Revalidate then atomically splice semantic sections into `.jet/lock`.
pub fn atomic_commit(project: &Path, lock: &SemanticLockFile) -> Result<(), LockCommitError> {
    if let Err(issues) = revalidate(lock) {
        return Err(LockCommitError { issues, io: None });
    }
    let path = live_path(project);
    let project = project.to_path_buf();
    let lock = lock.clone();
    crate::RuntimePolicy::with_project_lock(&project, "semantic-lock", move || {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let machine = strip_semantic_sections(&existing);
        // Machine half must remain Lock-parseable when present.
        if !machine.trim().is_empty() {
            crate::Lock::parse(&machine).map_err(std::io::Error::other)?;
        }
        let semantic = write(&lock);
        let body = if machine.trim().is_empty() {
            format!("version = 1\n\n{semantic}")
        } else {
            format!("{}\n\n{semantic}", machine.trim_end())
        };
        let tmp = path
            .parent()
            .map(|p| p.join("lock.tmp"))
            .unwrap_or_else(|| PathBuf::from("lock.tmp"));
        std::fs::write(&tmp, &body)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    })
    .map_err(|e| LockCommitError {
        issues: Vec::new(),
        io: Some(e.to_string()),
    })
}

#[cfg(test)]
mod flake_tests {
    use super::*;

    #[test]
    fn parses_follows_outputs_and_round_trips_through_semantic_lock() {
        let source = r#"
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs?rev=0123456789abcdef0123456789abcdef01234567";
    tools.url = "github:example/tools?rev=89abcdef0123456789abcdef0123456789abcdef";
    tools.follows = "nixpkgs";
  };
  outputs = { self, nixpkgs, ... }: {
    packages.x86_64-linux.app = 1;
    devShells.x86_64-linux.default = 2;
  };
}
"#;
        let graph = FlakeGraph::parse("flake.nix", source).unwrap();
        assert_eq!(graph.inputs.len(), 2);
        assert_eq!(graph.inputs[1].follows, "nixpkgs");
        assert_eq!(graph.inputs[1].revision, graph.inputs[0].revision);
        assert_eq!(graph.named_dev_shells().len(), 1);

        let lock = graph.semantic_lock();
        let restored = FlakeGraph::from_semantic_lock(".jet/lock", &lock).unwrap();
        assert_eq!(restored.inputs, graph.inputs);
        assert_eq!(restored.outputs, graph.outputs);
        assert_eq!(restored.semantic_lock_text(), graph.semantic_lock_text());
    }

    #[test]
    fn pinned_flake_lock_fixture_preserves_nodes_edges_and_registry_resolution() {
        let source = include_str!("../../../tests/fixtures/nix-compat/stage-a-flake.nix");
        let lock = include_str!("../../../tests/fixtures/nix-compat/stage-a-flake.lock");
        let expected = crate::JSON::parse(include_str!(
            "../../../tests/fixtures/nix-compat/stage-a-flake.expected.json"
        ))
        .unwrap();
        let expected = expected.as_object().unwrap();
        let graph = FlakeGraph::parse_with_lock(
            "tests/fixtures/nix-compat/stage-a-flake.nix".to_string(),
            source,
            Some(lock),
            true,
        )
        .unwrap();

        let expected_revisions = expected.get("input_revisions").unwrap().as_object().unwrap();
        for input in &graph.inputs {
            assert_eq!(
                input.revision,
                expected_revisions
                    .get(&input.name)
                    .unwrap()
                    .as_str()
                    .unwrap()
            );
        }
        let expected_nodes = expected
            .get("lock_nodes")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            graph
                .lock_nodes
                .iter()
                .map(|node| node.name.as_str())
                .collect::<Vec<_>>(),
            expected_nodes
        );
        let systems = graph
            .lock_nodes
            .iter()
            .find(|node| node.name == "flake-utils")
            .unwrap()
            .inputs
            .first()
            .unwrap();
        assert_eq!(systems.name, "systems");
        assert_eq!(systems.target, "[\"systems\"]");

        let registry = graph.registries.first().unwrap();
        let expected_registry = expected.get("registry").unwrap().as_object().unwrap();
        assert_eq!(registry.alias, expected_registry.get("alias").unwrap().as_str().unwrap());
        assert_eq!(registry.node, expected_registry.get("node").unwrap().as_str().unwrap());
        let locked = crate::JSON::parse(&registry.locked).unwrap();
        let locked = locked.as_object().unwrap();
        for field in ["type", "owner", "repo"] {
            assert_eq!(
                locked.get(field).unwrap().as_str().unwrap(),
                expected_registry
                    .get(&format!("locked_{field}"))
                    .unwrap()
                    .as_str()
                    .unwrap()
            );
        }

        let restored = FlakeGraph::from_semantic_lock(".jet/lock", &graph.semantic_lock()).unwrap();
        assert_eq!(restored, graph);
        assert_eq!(restored.semantic_lock_text(), graph.semantic_lock_text());
    }

    #[test]
    fn transitive_indirect_registry_resolution_round_trips() {
        let source = r#"
{
  inputs.tools.url = "github:example/tools?rev=0123456789abcdef0123456789abcdef01234567";
  outputs = { tools, ... }: {
    packages.x86_64-linux.default = tools;
  };
}
"#;
        let lock = r#"
{
  "nodes": {
    "root": { "inputs": { "tools": "tools" } },
    "tools": {
      "inputs": { "nixpkgs": "nixpkgs" },
      "locked": { "owner": "example", "repo": "tools", "rev": "0123456789abcdef0123456789abcdef01234567", "type": "github" },
      "original": { "owner": "example", "repo": "tools", "type": "github" }
    },
    "nixpkgs": {
      "locked": { "owner": "NixOS", "repo": "nixpkgs", "rev": "89abcdef0123456789abcdef0123456789abcdef", "type": "github" },
      "original": { "id": "nixpkgs", "type": "indirect" }
    }
  }
}
"#;
        let graph = FlakeGraph::parse_with_lock(
            "flake.nix".to_string(),
            source,
            Some(lock),
            true,
        )
        .unwrap();
        let registry = graph
            .registries
            .iter()
            .find(|entry| entry.alias == "nixpkgs")
            .unwrap();
        assert_eq!(registry.node, "nixpkgs");
        assert!(registry.locked.contains("\"rev\":\"89abcdef"));

        let restored = FlakeGraph::from_semantic_lock(".jet/lock", &graph.semantic_lock()).unwrap();
        assert_eq!(restored.registries, graph.registries);
        assert_eq!(restored.lock_nodes, graph.lock_nodes);
    }

    #[test]
    fn indirect_registry_alias_ref_and_query_identity_round_trips_and_rejects_drift() {
        let source =
            r#"{ inputs.nixpkgs.url = "nixpkgs/nixos-unstable?ref=nixos-unstable&dir=lib"; }"#;
        let lock = r#"
{
  "nodes": {
    "root": { "inputs": { "nixpkgs": "nixpkgs" } },
    "nixpkgs": {
      "locked": {
        "owner": "NixOS",
        "repo": "nixpkgs",
        "rev": "0123456789abcdef0123456789abcdef01234567",
        "type": "github"
      },
      "original": {
        "dir": "lib",
        "id": "nixpkgs",
        "ref": "nixos-unstable",
        "type": "indirect"
      }
    }
  }
}
"#;
        let graph = FlakeGraph::parse_with_lock(
            "flake.nix".to_string(),
            source,
            Some(lock),
            true,
        )
        .unwrap();
        assert_eq!(graph.inputs[0].revision, "0123456789abcdef0123456789abcdef01234567");
        let registry = graph.registries.first().unwrap();
        assert_eq!(registry.alias, "nixpkgs");
        assert_eq!(registry.node, "nixpkgs");

        let restored = FlakeGraph::from_semantic_lock(".jet/lock", &graph.semantic_lock()).unwrap();
        assert_eq!(restored, graph);

        let wrong_ref = lock.replace(
            "\"ref\": \"nixos-unstable\"",
            "\"ref\": \"nixos-24.11\"",
        );
        let error = FlakeGraph::parse_with_lock(
            "flake.nix".to_string(),
            source,
            Some(&wrong_ref),
            true,
        )
        .unwrap_err();
        assert!(matches!(error, FlakeGraphError::Io(reason) if reason.contains("different source URL")));

        let wrong_query = lock.replace("\"dir\": \"lib\"", "\"dir\": \"share\"");
        let error = FlakeGraph::parse_with_lock(
            "flake.nix".to_string(),
            source,
            Some(&wrong_query),
            true,
        )
        .unwrap_err();
        assert!(matches!(error, FlakeGraphError::Io(reason) if reason.contains("different source URL")));
    }

    #[test]
    fn flake_lock_follow_paths_resolve_to_terminal_node() {
        let nodes = crate::JSON::parse(
            r#"{
  "root": { "inputs": { "flake-utils": "flake-utils" } },
  "flake-utils": { "inputs": { "systems": "systems" } },
  "systems": { "inputs": {} }
}"#,
        )
        .unwrap()
        .as_object()
        .unwrap()
        .clone();
        let reference = crate::JSON::parse(r#"["flake-utils", "systems"]"#).unwrap();
        assert_eq!(
            lock_node_name(Some(&reference), &nodes, "root").unwrap(),
            Some("systems".to_string())
        );
        let reference = crate::JSON::parse(r#"["systems"]"#).unwrap();
        assert_eq!(
            lock_node_name(Some(&reference), &nodes, "flake-utils").unwrap(),
            Some("systems".to_string())
        );
    }

    #[test]
    fn flake_lock_revision_drift_fails_closed() {
        let source =
            "{ inputs.nixpkgs.url = \"github:NixOS/nixpkgs?rev=aaaaaaaa\"; }";
        let lock = r#"
{
  "nodes": {
    "root": { "inputs": { "nixpkgs": "nixpkgs" } },
    "nixpkgs": {
      "locked": { "rev": "bbbbbbbb" },
      "original": { "type": "github", "owner": "NixOS", "repo": "nixpkgs" }
    }
  }
}
"#;
        let error = FlakeGraph::parse_with_lock("flake.nix".to_string(), source, Some(lock), true)
            .unwrap_err();
        assert!(matches!(
            error,
            FlakeGraphError::ConflictingInput { input, field }
                if input == "nixpkgs" && field == "revision"
        ));

        let wrong_repo = r#"
{
  "nodes": {
    "root": { "inputs": { "nixpkgs": "nixpkgs" } },
    "nixpkgs": {
      "locked": { "rev": "aaaaaaaa" },
      "original": { "type": "github", "owner": "other", "repo": "repo" }
    }
  }
}
"#;
        let error = FlakeGraph::parse_with_lock(
            "flake.nix".to_string(),
            "{ inputs.nixpkgs.url = \"github:NixOS/nixpkgs?rev=aaaaaaaa\"; }",
            Some(wrong_repo),
            true,
        )
        .unwrap_err();
        assert!(matches!(error, FlakeGraphError::Io(reason) if reason.contains("different source URL")));
    }

    #[test]
    fn flake_lock_metadata_and_indirect_alias_fail_closed() {
        let source = "{ inputs.nixpkgs.url = \"nixpkgs\"; }";
        let missing_original = r#"
{
  "nodes": {
    "root": { "inputs": { "nixpkgs": "nixpkgs" } },
    "nixpkgs": { "locked": { "rev": "aaaaaaaa" } }
  }
}
"#;
        let error = FlakeGraph::parse_with_lock(
            "flake.nix".to_string(),
            source,
            Some(missing_original),
            true,
        )
        .unwrap_err();
        assert!(matches!(error, FlakeGraphError::Io(reason) if reason.contains("missing `original`")));

        let root_metadata = r#"
{
  "nodes": {
    "root": {
      "inputs": { "nixpkgs": "nixpkgs" },
      "original": { "type": "github" }
    },
    "nixpkgs": {
      "locked": { "rev": "aaaaaaaa" },
      "original": { "type": "github", "owner": "NixOS", "repo": "nixpkgs" }
    }
  }
}
"#;
        let error = FlakeGraph::parse_with_lock(
            "flake.nix".to_string(),
            source,
            Some(root_metadata),
            true,
        )
        .unwrap_err();
        assert!(matches!(error, FlakeGraphError::Io(reason) if reason.contains("root node must not carry")));

        let empty_node_name = r#"
{
  "nodes": {
    "root": { "inputs": { "nixpkgs": "nixpkgs" } },
    "": {
      "locked": { "rev": "aaaaaaaa" },
      "original": { "type": "github", "owner": "NixOS", "repo": "nixpkgs" }
    },
    "nixpkgs": {
      "locked": { "rev": "aaaaaaaa" },
      "original": { "type": "github", "owner": "NixOS", "repo": "nixpkgs" }
    }
  }
}
"#;
        let error = FlakeGraph::parse_with_lock(
            "flake.nix".to_string(),
            source,
            Some(empty_node_name),
            true,
        )
        .unwrap_err();
        assert!(matches!(error, FlakeGraphError::Io(reason) if reason.contains("node name is empty")));

        let malformed_indirect = r#"
{
  "nodes": {
    "root": { "inputs": { "nixpkgs": "nixpkgs" } },
    "nixpkgs": {
      "locked": { "rev": "aaaaaaaa" },
      "original": { "type": "indirect" }
    }
  }
}
"#;
        let error = FlakeGraph::parse_with_lock(
            "flake.nix".to_string(),
            source,
            Some(malformed_indirect),
            true,
        )
        .unwrap_err();
        assert!(matches!(error, FlakeGraphError::Io(reason) if reason.contains("indirect input has no id")));
    }

    #[test]
    fn floating_or_conflicting_foreign_inputs_fail_closed() {
        let floating = FlakeGraph::parse(
            "flake.nix",
            "{ inputs.nixpkgs.url = \"github:NixOS/nixpkgs\"; }",
        )
        .unwrap_err();
        assert!(matches!(floating, FlakeGraphError::MissingRevision { .. }));

        let conflicting = FlakeGraph::parse(
            "flake.nix",
            "{ inputs.nixpkgs.url = \"github:NixOS/nixpkgs?rev=aaaaaaa\"; inputs.nixpkgs.url = \"github:NixOS/nixpkgs?rev=bbbbbbb\"; }",
        )
        .unwrap_err();
        assert!(matches!(conflicting, FlakeGraphError::ConflictingInput { .. }));
    }

    #[test]
    fn lock_input_without_revision_is_not_promoted_from_nar_hash() {
        let source = "{ inputs.nixpkgs.url = \"github:NixOS/nixpkgs\"; }";
        let lock = r#"
{
  "nodes": {
    "root": { "inputs": { "nixpkgs": "nixpkgs" } },
    "nixpkgs": {
      "locked": { "narHash": "sha256-deadbeef" },
      "original": { "type": "github", "owner": "NixOS", "repo": "nixpkgs" }
    }
  }
}
"#;
        let error = FlakeGraph::parse_with_lock("flake.nix".to_string(), source, Some(lock), true)
            .expect_err("narHash is not an exact source revision");
        assert!(matches!(error, FlakeGraphError::MissingRevision { input } if input == "nixpkgs"));
    }

    #[test]
    fn semantic_lock_requires_every_flake_input_record() {
        let lock = SemanticLockFile {
            inputs: vec![LockInput {
                name: "nixpkgs".to_string(),
                url: "github:NixOS/nixpkgs".to_string(),
                follows: String::new(),
            }],
            ..SemanticLockFile::default()
        };
        let error = FlakeGraph::from_semantic_lock("flake.nix", &lock).unwrap_err();
        assert!(matches!(error, FlakeGraphError::MissingRevision { input } if input == "nixpkgs"));
    }

    #[test]
    fn loading_a_changed_flake_rejects_a_stale_semantic_lock() {
        let root = std::env::temp_dir().join(format!(
            "jet-flake-stale-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".jet")).unwrap();
        let flake = root.join("flake.nix");
        let source = "{ inputs.nixpkgs.url = \"github:NixOS/nixpkgs?rev=0123456789abcdef0123456789abcdef01234567\"; }";
        std::fs::write(&flake, source).unwrap();
        let graph = FlakeGraph::parse(flake.display().to_string(), source).unwrap();
        let lock_body = write(&graph.semantic_lock());
        std::fs::write(live_path(&root), &lock_body).unwrap();

        assert!(FlakeGraph::load(&flake).is_ok());
        std::fs::write(&flake, format!("{source}\n# changed\n")).unwrap();
        let error = FlakeGraph::load(&flake).unwrap_err();
        assert!(matches!(error, FlakeGraphError::StaleSemanticLock(_)), "{error}");
        assert_eq!(std::fs::read(live_path(&root)).unwrap(), lock_body.as_bytes());
        let prior = FlakeGraph::from_semantic_lock("flake.nix", &parse(&lock_body))
            .expect("the previous semantic plan must remain readable after source drift");
        assert_eq!(prior.source_fingerprint, graph.source_fingerprint);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn flake_parts_composition_is_graph_data_and_round_trips() {
        let source = r#"
{
  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts?rev=0123456789abcdef0123456789abcdef01234567";
    nixpkgs.url = "github:NixOS/nixpkgs?rev=89abcdef0123456789abcdef0123456789abcdef";
  };
  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ ./parts/dev.nix ];
      systems = [ "x86_64-linux" "aarch64-darwin" ];
      perSystem = { pkgs, ... }: {
        packages.default = pkgs.hello;
        devShells.default = pkgs.mkShell { };
      };
    };
}
"#;
        let graph = FlakeGraph::parse("flake.nix", source).unwrap();
        assert_eq!(
            graph.composition,
            Some(FlakeComposition {
                framework: "flake-parts".to_string(),
                modules: vec!["./parts/dev.nix".to_string()],
                module_sources: Vec::new(),
                systems: vec!["aarch64-darwin".to_string(), "x86_64-linux".to_string()],
                per_system: true,
            })
        );
        assert!(!graph.unsupported.iter().any(|item| item == "flake-parts"));
        assert!(!graph.unsupported.iter().any(|item| item == "perSystem"));
        let restored = FlakeGraph::from_semantic_lock(".jet/lock", &graph.semantic_lock()).unwrap();
        assert_eq!(restored.composition, graph.composition);
        assert_eq!(restored.stable_json(), graph.stable_json());
    }

    #[test]
    fn loaded_flake_parts_modules_are_fingerprinted_in_the_same_lock() {
        let root = std::env::temp_dir().join(format!(
            "jet-flake-parts-source-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".jet")).unwrap();
        std::fs::create_dir_all(root.join("parts")).unwrap();
        let flake = root.join("flake.nix");
        let module = root.join("parts/dev.nix");
        let shared = root.join("shared.nix");
        let source = r#"
{
  inputs.flake-parts.url = "github:hercules-ci/flake-parts?rev=0123456789abcdef0123456789abcdef01234567";
  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ ./parts/dev.nix ];
      systems = [ "x86_64-linux" ];
      perSystem = { pkgs, ... }: { devShells.default = pkgs.mkShell { }; };
    };
}
"#;
        let module_source =
            "{ imports = [ ../shared.nix ]; perSystem = { pkgs, ... }: { devShells.default = pkgs.mkShell { }; }; }\n";
        let shared_source = "{ perSystem = { pkgs, ... }: { packages.default = pkgs.hello; }; }\n";
        std::fs::write(&flake, source).unwrap();
        std::fs::write(&module, module_source).unwrap();
        std::fs::write(&shared, shared_source).unwrap();

        let graph = FlakeGraph::load(&flake).unwrap();
        let module_sources = &graph.composition.as_ref().unwrap().module_sources;
        assert_eq!(
            module_sources,
            &vec![
                format!(
                    "./parts/dev.nix\t{}",
                    crate::SHA256::sha256_hex(module_source.as_bytes())
                ),
                format!(
                    "./shared.nix\t{}",
                    crate::SHA256::sha256_hex(shared_source.as_bytes())
                ),
            ]
        );
        std::fs::write(live_path(&root), write(&graph.semantic_lock())).unwrap();
        assert_eq!(FlakeGraph::load(&flake).unwrap(), graph);

        std::fs::write(&shared, "{ perSystem = { }; }\n").unwrap();
        let error = FlakeGraph::load(&flake).unwrap_err();
        assert!(matches!(error, FlakeGraphError::StaleSemanticLock(reason) if reason.contains("composition")));
        let _ = std::fs::remove_dir_all(root);
    }
}
