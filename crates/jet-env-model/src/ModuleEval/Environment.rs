//! Typed environment facts shared by the evaluator and Jetpack runtime.
//!
//! These are deliberately small closed records. The evaluator turns Jet
//! values into these facts; Jetpack consumes them without reparsing source or
//! inventing a second policy language.

use std::collections::BTreeMap;
use std::path::Path;

use crate::AST::CtKey;
use crate::Comptime::CtValue;

/// Return the fully-qualified name of a first-party integration call.
///
/// The parser represents `env.platform.android(...)` as a method call on the
/// field path `env.platform`, while bare calls use `Expr::Call`. Both spellings
/// are the same integration surface and must share one lowering path.
pub(super) fn qualified_call_name(expr: &crate::AST::Expr) -> Option<String> {
    match expr {
        crate::AST::Expr::Call(call) => Some(call.name.clone()),
        crate::AST::Expr::MethodCall {
            receiver, method, ..
        } => {
            let mut name = expression_path(receiver)?;
            name.push('.');
            name.push_str(method);
            Some(name)
        }
        _ => None,
    }
}

fn expression_path(expr: &crate::AST::Expr) -> Option<String> {
    match expr {
        crate::AST::Expr::Ident(name, _) => Some(name.clone()),
        crate::AST::Expr::Field(base, member, _) => {
            let mut name = expression_path(base)?;
            name.push('.');
            name.push_str(member);
            Some(name)
        }
        _ => None,
    }
}

/// D-ENV-INTEGRATIONS1=A: the closed first-party integration vocabulary. An
/// integration is a typed projection into ordinary environment facts, not a
/// second package resolver, lock, effect system, or activation engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntegrationKind {
    Android,
    Apple,
    Certificates,
    Hosts,
    CodexAgent,
    Editor,
    CloudCredentials,
    Vault,
}

impl IntegrationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Android => "android",
            Self::Apple => "apple",
            Self::Certificates => "certificates",
            Self::Hosts => "hosts",
            Self::CodexAgent => "codex-agent",
            Self::Editor => "editor",
            Self::CloudCredentials => "cloud-credentials",
            Self::Vault => "vault",
        }
    }

    pub fn from_call(name: &str) -> Option<Self> {
        match name {
            crate::Syntax::ENV_INTEGRATION_ANDROID => Some(Self::Android),
            crate::Syntax::ENV_INTEGRATION_APPLE => Some(Self::Apple),
            crate::Syntax::ENV_INTEGRATION_CERTIFICATES => Some(Self::Certificates),
            crate::Syntax::ENV_INTEGRATION_HOSTS => Some(Self::Hosts),
            crate::Syntax::ENV_INTEGRATION_CODEX => Some(Self::CodexAgent),
            crate::Syntax::ENV_INTEGRATION_EDITOR => Some(Self::Editor),
            crate::Syntax::ENV_INTEGRATION_CLOUD => Some(Self::CloudCredentials),
            crate::Syntax::ENV_INTEGRATION_VAULT => Some(Self::Vault),
            _ => None,
        }
    }
}

/// One lowered integration import. Secret-bearing inputs are represented by
/// names only; values never enter this record, fingerprints, dossiers, image
/// layers, or logs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvironmentIntegration {
    pub kind: IntegrationKind,
    pub name: String,
    pub preset: String,
    pub options: BTreeMap<String, String>,
    pub packages: Vec<String>,
    pub files: Vec<ManagedFile>,
    /// Existing lifecycle/task facts selected by the preset. These are names
    /// only; activation still belongs to the ordinary environment lifecycle.
    pub tasks: Vec<String>,
    /// Provider authorities used by package and host facts. This is metadata,
    /// not a second resolver.
    pub providers: Vec<String>,
    pub host_checks: Vec<String>,
    pub secrets: Vec<String>,
    pub grants: Vec<String>,
    pub losses: Vec<String>,
}

/// The non-package portion of integration lowering. These facts stay in the
/// ordinary environment plan so trust, inspection, and activation all consume
/// one projection instead of treating integrations as report-only annotations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationTaskFact {
    pub name: String,
    pub integration: IntegrationKind,
    pub packages: Vec<String>,
    pub secrets: Vec<String>,
    pub providers: Vec<String>,
    pub host_checks: Vec<String>,
    pub grants: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IntegrationFactProjection {
    pub tasks: Vec<String>,
    /// Typed task contracts. `tasks` is only the stable disclosure projection;
    /// realization consumes these records and their ordinary package,
    /// provider, host, and grant facts.
    pub task_facts: Vec<IntegrationTaskFact>,
    pub providers: Vec<String>,
    pub host_checks: Vec<String>,
    pub grants: Vec<String>,
    pub losses: Vec<String>,
}

impl IntegrationFactProjection {
    pub fn fingerprint(&self) -> String {
        let mut text = String::new();
        for task in &self.task_facts {
            text.push_str("task-fact=");
            text.push_str(task.integration.as_str());
            text.push('\t');
            text.push_str(&task.name);
            text.push('\t');
            text.push_str(&task.packages.join(","));
            text.push('\t');
            text.push_str(&task.secrets.join(","));
            text.push('\t');
            text.push_str(&task.providers.join(","));
            text.push('\t');
            text.push_str(&task.host_checks.join(","));
            text.push('\t');
            text.push_str(&task.grants.join(","));
            text.push('\n');
        }
        for (label, values) in [
            ("task", &self.tasks),
            ("provider", &self.providers),
            ("host-check", &self.host_checks),
            ("grant", &self.grants),
            ("loss", &self.losses),
        ] {
            for value in values {
                text.push_str(label);
                text.push('=');
                text.push_str(value);
                text.push('\n');
            }
        }
        text
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.losses.is_empty() {
            Err(format!(
                "integration lowering lost declared facts: {}",
                self.losses.join("; ")
            ))
        } else if self.task_facts.iter().any(|task| {
            task.name.trim().is_empty()
                || task.providers.iter().any(|provider| provider.trim().is_empty())
                || task.secrets.iter().any(|secret| secret.trim().is_empty())
                || task.host_checks.iter().any(|check| check.trim().is_empty())
                || task.grants.iter().any(|grant| grant.trim().is_empty())
        }) {
            Err("integration task facts contain an empty executable field".to_string())
        } else {
            Ok(())
        }
    }
}

impl EnvironmentIntegration {
    pub fn fingerprint(&self) -> String {
        let mut text = format!(
            "jet-environment-integration-v1\nkind={}\nname={}\npreset={}\n",
            self.kind.as_str(), self.name, self.preset
        );
        for (key, value) in &self.options {
            text.push_str("option=");
            text.push_str(key);
            text.push('=');
            text.push_str(value);
            text.push('\n');
        }
        for value in &self.packages {
            text.push_str("package=");
            text.push_str(value);
            text.push('\n');
        }
        for file in &self.files {
            text.push_str("file=");
            text.push_str(&file.fingerprint());
            text.push('\n');
        }
        for value in &self.host_checks {
            text.push_str("host-check=");
            text.push_str(value);
            text.push('\n');
        }
        for value in &self.tasks {
            text.push_str("task=");
            text.push_str(value);
            text.push('\n');
        }
        for value in &self.providers {
            text.push_str("provider=");
            text.push_str(value);
            text.push('\n');
        }
        for value in &self.secrets {
            text.push_str("secret-name=");
            text.push_str(value);
            text.push('\n');
        }
        for value in &self.grants {
            text.push_str("grant=");
            text.push_str(value);
            text.push('\n');
        }
        for value in &self.losses {
            text.push_str("loss=");
            text.push_str(value);
            text.push('\n');
        }
        text
    }

    /// Validate a target at the host/realization boundary. Graph evaluation
    /// keeps cross-platform modules composable, while an activation path can
    /// fail with the exact unsupported-host fact instead of silently omitting
    /// an SDK.
    pub fn validate_target(&self, target: &str) -> Result<(), String> {
        let target = target.to_ascii_lowercase();
        let supported = match self.kind {
            IntegrationKind::Android => target.contains("linux") || target.contains("android"),
            IntegrationKind::Apple => {
                target.contains("darwin") || target.contains("macos") || target.contains("ios")
            }
            IntegrationKind::Certificates
            | IntegrationKind::Hosts
            | IntegrationKind::CodexAgent
            | IntegrationKind::Editor
            | IntegrationKind::CloudCredentials
            | IntegrationKind::Vault => true,
        };
        supported.then_some(()).ok_or_else(|| {
            format!(
                "{} integration `{}` is not supported on target `{target}`",
                self.kind.as_str(), self.name
            )
        })
    }
}

impl Default for IntegrationKind {
    fn default() -> Self {
        Self::Android
    }
}

/// The one managed-file write policy. The default is a symlink to an immutable
/// Jet-owned content object; `Seed` preserves an existing destination and
/// `Copy` owns the destination after the first successful application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileMode {
    #[default]
    Symlink,
    Seed,
    Copy,
}

impl FileMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Symlink => "symlink",
            Self::Seed => "seed",
            Self::Copy => "copy",
        }
    }
}

/// Conflict handling is intentionally closed in v1. A managed file never
/// overwrites an unmanaged destination; the user must resolve that choice in
/// source before `jet env sync` can apply it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileConflict {
    #[default]
    Refuse,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ManagedFile {
    pub destination: String,
    /// A project-relative source path, when the declaration names one.
    pub source: Option<String>,
    /// Inline bytes after comptime evaluation, when the declaration supplies
    /// a value rather than a source path.
    pub content: Option<Vec<u8>>,
    pub mode: FileMode,
    pub permissions: Option<u32>,
    pub sensitive: bool,
    pub generation: Option<String>,
    /// Hash of the declarative source identity. `jetpack` re-hashes source
    /// bytes before applying a path-backed entry.
    pub source_digest: String,
    pub conflict: FileConflict,
}

impl ManagedFile {
    pub fn fingerprint(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.destination,
            self.source.as_deref().unwrap_or("<inline>"),
            self.source_digest,
            self.mode.as_str(),
            self.permissions.map_or_else(String::new, |value| value.to_string()),
            if self.sensitive { "sensitive" } else { "public" },
            self.generation.as_deref().unwrap_or(""),
            match self.conflict {
                FileConflict::Refuse => "refuse",
            },
        )
    }

}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedFileError {
    InvalidDestination(String),
    InvalidSource(String),
    InvalidEntry(String),
    InvalidMode(String),
    InvalidPermissions(String),
}

impl std::fmt::Display for ManagedFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDestination(value) => write!(f, "managed file destination `{value}` is unsafe"),
            Self::InvalidSource(value) => write!(f, "managed file source `{value}` is unsafe"),
            Self::InvalidEntry(value) => f.write_str(value),
            Self::InvalidMode(value) => write!(f, "unknown managed file mode `{value}`"),
            Self::InvalidPermissions(value) => write!(f, "managed file permissions `{value}` are invalid"),
        }
    }
}

impl std::error::Error for ManagedFileError {}

/// Convert the closed `files: { destination: value }` fact into deterministic
/// managed-file records. The evaluator owns shape and path validation; the
/// runtime only resolves path-backed bytes and applies these facts.
pub fn files_from_value(value: &CtValue) -> Result<Vec<ManagedFile>, ManagedFileError> {
    let entries = match value {
        CtValue::Map(values) => values
            .iter()
            .map(|(key, value)| match key {
                CtKey::Str(name) => Ok((name.clone(), value.clone())),
                _ => Err(ManagedFileError::InvalidEntry(
                    "managed file maps need string destination keys".to_string(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?,
        CtValue::Struct { fields, .. } => checked_unique_fields(fields, "files")
            .map_err(ManagedFileError::InvalidEntry)?
            .into_iter()
            .collect(),
        CtValue::List(values) => values
            .iter()
            .map(|value| {
                let fields = checked_managed_fields(value, "list entry")?;
                let destination = fields
                    .get("destination")
                    .and_then(string_value)
                    .ok_or_else(|| ManagedFileError::InvalidEntry(
                        "a managed file list entry needs a string `destination`".to_string(),
                    ))?;
                Ok((destination, value.clone()))
            })
            .collect::<Result<Vec<_>, ManagedFileError>>()?,
        _ => {
            return Err(ManagedFileError::InvalidEntry(
                "files must be a map or a list of managed file records".to_string(),
            ))
        }
    };
    let mut files = entries
        .into_iter()
        .map(|(destination, value)| managed_file_from_value(destination, value))
        .collect::<Result<Vec<_>, _>>()?;
    files.sort_by(|left, right| left.destination.cmp(&right.destination));
    for pair in files.windows(2) {
        if pair[0].destination == pair[1].destination {
            return Err(ManagedFileError::InvalidDestination(pair[0].destination.clone()));
        }
    }
    Ok(files)
}

fn managed_file_from_value(
    destination: String,
    value: CtValue,
) -> Result<ManagedFile, ManagedFileError> {
    validate_relative_path(&destination, false)
        .map_err(|_| ManagedFileError::InvalidDestination(destination.clone()))?;
    let mut file = ManagedFile {
        destination,
        ..Default::default()
    };
    match value {
        CtValue::Str(value) => file.content = Some(value.into_bytes()),
        CtValue::Bytes(value) => file.content = Some(value),
        value => {
            let fields = checked_managed_fields(&value, &file.destination)?;
            if fields.contains_key("source") && fields.contains_key("content") {
                return Err(ManagedFileError::InvalidEntry(format!(
                    "managed file `{}` must choose either `source` or `content`",
                    file.destination
                )));
            }
            let source_or_content = fields
                .get("content")
                .or_else(|| fields.get("source"))
                .ok_or_else(|| ManagedFileError::InvalidEntry(format!(
                    "managed file `{}` needs `source` or `content`",
                    file.destination
                )))?;
            match source_or_content {
                CtValue::Str(value) if fields.contains_key("content") => {
                    file.content = Some(value.as_bytes().to_vec());
                }
                CtValue::Str(value) => {
                    validate_relative_path(value, true)
                        .map_err(|_| ManagedFileError::InvalidSource(value.clone()))?;
                    file.source = Some(value.clone());
                }
                CtValue::Bytes(value) => file.content = Some(value.clone()),
                _ => {
                    return Err(ManagedFileError::InvalidEntry(format!(
                        "managed file `{}` source/content must be a string or bytes",
                        file.destination
                    )))
                }
            }
            if let Some(mode) = fields.get("mode") {
                file.mode = file_mode(mode)?;
            }
            if let Some(permissions) = fields.get("permissions") {
                let CtValue::Int(value) = permissions else {
                    return Err(ManagedFileError::InvalidPermissions(permissions.jet_show()));
                };
                if !(0..=0o7777).contains(value) {
                    return Err(ManagedFileError::InvalidPermissions(value.to_string()));
                }
                file.permissions = Some(*value as u32);
            }
            if let Some(sensitive) = fields.get("sensitive") {
                let CtValue::Bool(value) = sensitive else {
                    return Err(ManagedFileError::InvalidEntry(format!(
                        "managed file `{}` sensitive must be Bool",
                        file.destination
                    )));
                };
                file.sensitive = *value;
            }
            if let Some(generation) = fields.get("generation") {
                file.generation = Some(string_value(generation).ok_or_else(|| {
                    ManagedFileError::InvalidEntry(format!(
                        "managed file `{}` generation must be a string",
                        file.destination
                    ))
                })?);
            }
        }
    }
    let identity = file
        .content
        .as_deref()
        .unwrap_or_else(|| file.source.as_deref().unwrap_or("").as_bytes());
    file.source_digest = jet_pkg_model::SHA256::sha256_hex(identity);
    Ok(file)
}

fn file_mode(value: &CtValue) -> Result<FileMode, ManagedFileError> {
    let raw = match value {
        CtValue::Str(value) => value.clone(),
        CtValue::Enum { variant, .. } => variant.rsplit('.').next().unwrap_or(variant).to_string(),
        _ => return Err(ManagedFileError::InvalidMode(value.jet_show())),
    };
    match raw.to_ascii_lowercase().as_str() {
        "symlink" | "link" => Ok(FileMode::Symlink),
        "seed" => Ok(FileMode::Seed),
        "copy" => Ok(FileMode::Copy),
        _ => Err(ManagedFileError::InvalidMode(raw)),
    }
}

fn validate_relative_path(value: &str, allow_dot: bool) -> Result<(), ()> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            component == std::path::Component::ParentDir
                || (!allow_dot && component == std::path::Component::CurDir)
        })
    {
        return Err(());
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PresetSpec {
    pub name: String,
    pub extends: Vec<String>,
    pub packages: Vec<String>,
    pub variables: BTreeMap<String, String>,
    pub hostname: Option<String>,
    pub user: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedPreset {
    pub name: String,
    /// Ambient selection before inheritance expansion. Explicit CLI selection
    /// chooses one profile; ambient hostname and user matches may merge.
    pub selected_presets: Vec<String>,
    pub applied: Vec<String>,
    pub packages: Vec<String>,
    pub variables: BTreeMap<String, String>,
}

/// D-JPK-PROFILE1=D: one source-backed package-profile declaration. This is
/// deliberately separate from `PresetSpec`, which is the environment-shell
/// profile surface and may select host/user variables.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackageProfileSpec {
    pub name: String,
    pub extends: Vec<String>,
    /// Canonical package refs as written by the shared package-sugar parser.
    pub packages: Vec<String>,
    /// Exact path -> selected provider identity, from the ratified collision
    /// map. The provider is checked against realized contenders later.
    pub collisions: BTreeMap<String, String>,
    /// Source modules that contributed this equal declaration.
    pub sources: Vec<String>,
}

/// One package ref after profile composition. `declared_by` keeps source
/// profile identity when equal package facts are de-duplicated.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackageProfilePackage {
    pub raw: String,
    pub declared_by: Vec<String>,
}

/// D-JPK-PROFILE1=D: one resolved profile view. It is still source-backed;
/// generation publication and activation consume this exact object later.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedPackageProfile {
    pub name: String,
    pub selected_profiles: Vec<String>,
    pub applied: Vec<String>,
    pub packages: Vec<PackageProfilePackage>,
    pub collisions: BTreeMap<String, String>,
    pub sources: Vec<String>,
}

/// One resolved package fact with both the source spelling and the realization
/// identity that a lock, explain view, or generation writer must preserve.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackageProfileFact {
    pub raw: String,
    pub target: String,
    pub source: String,
    pub upstream: Option<String>,
    pub provider: String,
    pub channel: Option<String>,
    pub declared_by: Vec<String>,
}

/// The read-only, source-backed profile plan. Realization and activation may
/// consume this plan later; they must not reconstruct package identity from
/// the display string.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackageProfilePlan {
    pub name: String,
    pub selected_profiles: Vec<String>,
    pub applied: Vec<String>,
    pub packages: Vec<PackageProfileFact>,
    pub collisions: BTreeMap<String, String>,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetError {
    Missing(String),
    Cycle(Vec<String>),
    Conflict { name: String },
}

impl std::fmt::Display for PresetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(name) => write!(f, "preset '{name}' does not exist"),
            Self::Cycle(names) => write!(f, "preset inheritance cycle: {}", names.join(" -> ")),
            Self::Conflict { name } => write!(f, "preset '{name}' is declared with conflicting facts"),
        }
    }
}

impl std::error::Error for PresetError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageProfileError {
    Missing(String),
    Cycle(Vec<String>),
    Conflict { name: String },
}

impl std::fmt::Display for PackageProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(name) => write!(f, "package profile '{name}' does not exist"),
            Self::Cycle(names) => write!(f, "package profile inheritance cycle: {}", names.join(" -> ")),
            Self::Conflict { name } => write!(f, "package profile fact '{name}' conflicts"),
        }
    }
}

impl std::error::Error for PackageProfileError {}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PresetSet {
    pub profiles: BTreeMap<String, PresetSpec>,
}

impl PresetSet {
    pub fn insert(&mut self, profile: PresetSpec) -> Result<(), PresetError> {
        self.insert_checked(profile)
    }

    pub fn insert_checked(&mut self, profile: PresetSpec) -> Result<(), PresetError> {
        if let Some(existing) = self.profiles.get(&profile.name) {
            if existing != &profile {
                return Err(PresetError::Conflict {
                    name: profile.name,
                });
            }
            return Ok(());
        }
        self.profiles.insert(profile.name.clone(), profile);
        Ok(())
    }

    pub fn resolve(&self, name: &str) -> Result<ResolvedPreset, PresetError> {
        self.resolve_many(&[name.to_string()])
    }

    pub fn resolve_many(&self, names: &[String]) -> Result<ResolvedPreset, PresetError> {
        let mut selected_presets = Vec::new();
        for name in names {
            if !selected_presets.iter().any(|existing| existing == name) {
                selected_presets.push(name.clone());
            }
        }
        let mut resolved = ResolvedPreset {
            name: selected_presets.join("+"),
            selected_presets,
            ..Default::default()
        };
        let mut stack = Vec::new();
        for name in resolved.selected_presets.clone() {
            self.resolve_into(&name, &mut stack, &mut resolved)?;
        }
        Ok(resolved)
    }

    pub fn auto_select(&self, hostname: &str, user: &str) -> Option<String> {
        self.auto_select_many(hostname, user).into_iter().next()
    }

    /// Select ambient profiles in deterministic priority order. All hostname
    /// matches merge before all user matches; `default` is the last resort.
    pub fn auto_select_many(&self, hostname: &str, user: &str) -> Vec<String> {
        let mut selected = self
            .profiles
            .values()
            .filter(|profile| {
                profile
                    .hostname
                    .as_deref()
                    .is_some_and(|candidate| candidate == hostname)
            })
            .map(|profile| profile.name.clone())
            .collect::<Vec<_>>();
        for name in self
            .profiles
            .values()
            .filter(|profile| {
                profile
                    .user
                    .as_deref()
                    .is_some_and(|candidate| candidate == user)
            })
            .map(|profile| profile.name.clone())
        {
            if !selected.iter().any(|existing| existing == &name) {
                selected.push(name);
            }
        }
        if !selected.is_empty() {
            return selected;
        }

        if self.profiles.contains_key("default") {
            return vec!["default".to_string()];
        }
        Vec::new()
    }

    fn resolve_into(
        &self,
        name: &str,
        stack: &mut Vec<String>,
        resolved: &mut ResolvedPreset,
    ) -> Result<(), PresetError> {
        if stack.iter().any(|item| item == name) {
            let start = stack.iter().position(|item| item == name).unwrap_or(0);
            let mut cycle = stack[start..].to_vec();
            cycle.push(name.to_string());
            return Err(PresetError::Cycle(cycle));
        }
        let profile = self
            .profiles
            .get(name)
            .ok_or_else(|| PresetError::Missing(name.to_string()))?;
        stack.push(name.to_string());
        for parent in &profile.extends {
            self.resolve_into(parent, stack, resolved)?;
        }
        if !resolved.applied.iter().any(|item| item == name) {
            resolved.applied.push(name.to_string());
        }
        for package in &profile.packages {
            if !resolved.packages.iter().any(|item| item == package) {
                resolved.packages.push(package.clone());
            }
        }
        for (key, value) in &profile.variables {
            if let Some(existing) = resolved.variables.get(key) {
                if existing != value {
                    return Err(PresetError::Conflict {
                        name: format!("{name}.{key}"),
                    });
                }
            } else {
                resolved.variables.insert(key.clone(), value.clone());
            }
        }
        stack.pop();
        Ok(())
    }
}

/// One resolver for source-backed package profiles. JetOS/user composition,
/// tool projections, and package-profile commands use this same data shape.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackageProfileSet {
    pub profiles: BTreeMap<String, PackageProfileSpec>,
}

impl PackageProfileSet {
    pub fn insert_checked(&mut self, profile: PackageProfileSpec) -> Result<(), PackageProfileError> {
        if let Some(existing) = self.profiles.get_mut(&profile.name) {
            if existing.extends != profile.extends
                || existing.packages != profile.packages
                || existing.collisions != profile.collisions
            {
                return Err(PackageProfileError::Conflict {
                    name: profile.name.clone(),
                });
            }
            for source in profile.sources {
                if !existing.sources.iter().any(|item| item == &source) {
                    existing.sources.push(source);
                }
            }
            return Ok(());
        }
        self.profiles.insert(profile.name.clone(), profile);
        Ok(())
    }

    pub fn resolve(&self, name: &str) -> Result<ResolvedPackageProfile, PackageProfileError> {
        self.resolve_many(&[name.to_string()])
    }

    pub fn resolve_many(&self, names: &[String]) -> Result<ResolvedPackageProfile, PackageProfileError> {
        let mut selected_profiles = Vec::new();
        for name in names {
            if !selected_profiles.iter().any(|item| item == name) {
                selected_profiles.push(name.clone());
            }
        }
        let mut resolved = ResolvedPackageProfile {
            name: selected_profiles.join("+"),
            selected_profiles,
            ..Default::default()
        };
        let mut stack = Vec::new();
        for name in resolved.selected_profiles.clone() {
            self.resolve_into(&name, &mut stack, &mut resolved)?;
        }
        Ok(resolved)
    }

    fn resolve_into(
        &self,
        name: &str,
        stack: &mut Vec<String>,
        resolved: &mut ResolvedPackageProfile,
    ) -> Result<(), PackageProfileError> {
        if stack.iter().any(|item| item == name) {
            let start = stack.iter().position(|item| item == name).unwrap_or(0);
            let mut cycle = stack[start..].to_vec();
            cycle.push(name.to_string());
            return Err(PackageProfileError::Cycle(cycle));
        }
        let profile = self
            .profiles
            .get(name)
            .ok_or_else(|| PackageProfileError::Missing(name.to_string()))?;
        stack.push(name.to_string());
        for parent in &profile.extends {
            self.resolve_into(parent, stack, resolved)?;
        }
        if !resolved.applied.iter().any(|item| item == name) {
            resolved.applied.push(name.to_string());
        }
        for source in &profile.sources {
            if !resolved.sources.iter().any(|item| item == source) {
                resolved.sources.push(source.clone());
            }
        }
        for raw in &profile.packages {
            if let Some(existing) = resolved.packages.iter_mut().find(|item| item.raw == *raw) {
                if !existing.declared_by.iter().any(|item| item == name) {
                    existing.declared_by.push(name.to_string());
                }
            } else {
                resolved.packages.push(PackageProfilePackage {
                    raw: raw.clone(),
                    declared_by: vec![name.to_string()],
                });
            }
        }
        for (path, provider) in &profile.collisions {
            if let Some(existing) = resolved.collisions.get(path) {
                if existing != provider {
                    return Err(PackageProfileError::Conflict {
                        name: format!("{name}.collisions.{path}"),
                    });
                }
            } else {
                resolved.collisions.insert(path.clone(), provider.clone());
            }
        }
        stack.pop();
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LanguagePack {
    pub name: String,
    pub packages: Vec<String>,
    pub venv_packages: Vec<String>,
    pub variables: BTreeMap<String, String>,
    pub commands: BTreeMap<String, String>,
    /// Execution authority for the pack. `native` means the pack is selected
    /// for the host platform recorded in `platforms`.
    pub host: String,
    pub platforms: Vec<String>,
    /// Union of the licenses declared by the catalog entries in this pack.
    pub license: String,
    /// Command names the pack promises to expose. A missing mapping is a
    /// catalog error, never an empty PATH entry.
    pub required_tools: Vec<String>,
}

impl LanguagePack {
    /// Stable identity for one catalog entry. Pack metadata is environment
    /// policy, not presentation: variables and commands must invalidate trust
    /// even when its package list is unchanged.
    pub fn fingerprint(&self) -> String {
        let mut text = String::from("jet-language-pack-v1\n");
        text.push_str("name=");
        text.push_str(&self.name);
        text.push('\n');
        for package in &self.packages {
            text.push_str("package=");
            text.push_str(package);
            text.push('\n');
        }
        for package in &self.venv_packages {
            text.push_str("venv-package=");
            text.push_str(package);
            text.push('\n');
        }
        for (name, value) in &self.variables {
            text.push_str("var=");
            text.push_str(name);
            text.push('=');
            text.push_str(value);
            text.push('\n');
        }
        for (name, command) in &self.commands {
            text.push_str("command=");
            text.push_str(name);
            text.push('=');
            text.push_str(command);
            text.push('\n');
        }
        text.push_str("host=");
        text.push_str(&self.host);
        text.push('\n');
        for platform in &self.platforms {
            text.push_str("platform=");
            text.push_str(platform);
            text.push('\n');
        }
        text.push_str("license=");
        text.push_str(&self.license);
        text.push('\n');
        for tool in &self.required_tools {
            text.push_str("required-tool=");
            text.push_str(tool);
            text.push('\n');
        }
        text
    }
}

/// One user selection from `languages`. Expansion turns enabled selections
/// into ordinary package refs; the typed selection remains in the plan for
/// trust, cache, and user-facing disclosure.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LanguageSpec {
    pub name: String,
    pub enable: bool,
    pub version: Option<String>,
    pub channel: Option<String>,
    pub venv: bool,
    pub extra_packages: Vec<String>,
}

impl LanguageSpec {
    pub fn key(&self) -> String {
        language_key(&self.name)
    }

    pub fn fingerprint(&self) -> String {
        format!(
            "{}\tenable={}\tversion={}\tchannel={}\tvenv={}\textra={}",
            self.name,
            self.enable,
            self.version.as_deref().unwrap_or(""),
            self.channel.as_deref().unwrap_or(""),
            self.venv,
            self.extra_packages.join(","),
        )
    }

    pub fn same_selection(&self, other: &Self) -> bool {
        self.key() == other.key()
            && self.enable == other.enable
            && self.version == other.version
            && self.channel == other.channel
            && self.venv == other.venv
            && self.extra_packages == other.extra_packages
    }
}

/// One typed language selection projected through its catalog entry. The
/// selection preserves author facts; `included` and `omitted` make the
/// package projection explicit for trust, image, and diagnostics consumers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LanguageProjection {
    pub selection: LanguageSpec,
    pub pack: LanguagePack,
    pub included: Vec<String>,
    pub omitted: Vec<String>,
    pub changed: Vec<String>,
    pub host: String,
    pub platform: String,
    pub license: String,
    pub missing_tools: Vec<String>,
}

impl LanguageProjection {
    pub fn fingerprint(&self) -> String {
        let mut text = String::new();
        text.push_str("selection=");
        text.push_str(&self.selection.fingerprint());
        text.push('\n');
        text.push_str("pack=");
        text.push_str(&self.pack.fingerprint());
        text.push_str("included=");
        text.push_str(&self.included.join(","));
        text.push('\n');
        text.push_str("omitted=");
        text.push_str(&self.omitted.join(","));
        text.push('\n');
        text.push_str("changed=");
        text.push_str(&self.changed.join(","));
        text.push('\n');
        text.push_str("host=");
        text.push_str(&self.host);
        text.push('\n');
        text.push_str("platform=");
        text.push_str(&self.platform);
        text.push('\n');
        text.push_str("license=");
        text.push_str(&self.license);
        text.push('\n');
        text.push_str("missing-tools=");
        text.push_str(&self.missing_tools.join(","));
        text.push('\n');
        text
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LanguagePackCatalog {
    packs: BTreeMap<String, LanguagePack>,
}

impl LanguagePackCatalog {
    pub fn builtin() -> Self {
        let mut catalog = Self::default();
        for pack in [
            pack(
                "Rust",
                &[
                    "rustc@nixpkgs",
                    "cargo@nixpkgs",
                    "rust-analyzer@nixpkgs",
                    "rustfmt@nixpkgs",
                    "clippy@nixpkgs",
                ],
            ),
            pack(
                "Python",
                &[
                    "python@nixpkgs",
                    "pip@nixpkgs",
                ],
            ),
            pack(
                "Go",
                &[
                    "go@nixpkgs",
                    "gopls@nixpkgs",
                ],
            ),
            pack(
                "JavaScript",
                &[
                    "nodejs@nixpkgs",
                ],
            ),
        ] {
            catalog.register(pack).expect("built-in language pack names are unique");
        }
        for name in extended_language_names() {
            catalog
                .register(extended_language_pack(name))
                .expect("extended built-in language pack names are unique");
        }
        catalog
    }

    pub fn register(&mut self, pack: LanguagePack) -> Result<(), String> {
        if pack.name.trim().is_empty() {
            return Err("language pack name cannot be empty".to_string());
        }
        if self
            .packs
            .keys()
            .any(|name| language_key(name) == language_key(&pack.name))
        {
            return Err(format!("language pack '{}' is already registered", pack.name));
        }
        validate_language_pack(&pack)?;
        self.packs.insert(pack.name.clone(), pack);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&LanguagePack> {
        self.packs.get(name).or_else(|| {
            let key = language_key(name);
            self.packs.values().find(|pack| language_key(&pack.name) == key)
        })
    }

    pub fn names(&self) -> Vec<String> {
        self.packs.keys().cloned().collect()
    }

    /// Stable catalog identity disclosed by `jet env info`; changing a pack's
    /// packages or host variables changes this fingerprint.
    pub fn fingerprint(&self) -> String {
        let mut text = String::from("jet-language-catalog-v1\n");
        for pack in self.packs.values() {
            text.push_str(&pack.fingerprint());
            text.push('\n');
        }
        jet_pkg_model::SHA256::sha256_hex(text.as_bytes())
    }

    pub fn expand(&self, selections: &[LanguageSpec]) -> Result<LanguageExpansion, String> {
        self.expand_for_platform(selections, &jet_pkg_model::Platform::host_key())
    }

    /// Expand selections for one explicit host platform. Keeping the target
    /// parameter here makes unsupported-host behavior testable without
    /// pretending the host is something it is not.
    pub fn expand_for_platform(
        &self,
        selections: &[LanguageSpec],
        platform: &str,
    ) -> Result<LanguageExpansion, String> {
        let mut expansion = LanguageExpansion::default();
        for selection in selections {
            let name = selection.name.as_str();
            let pack = self
                .get(name)
                .ok_or_else(|| format!("language pack '{}' is not in the catalog", name))?;
            let mut normalized = selection.clone();
            normalized.name = pack.name.clone();
            if let Some(existing) = expansion
                .selections
                .iter()
                .find(|item| item.key() == normalized.key())
            {
                if !existing.same_selection(&normalized) {
                    return Err(format!(
                        "language pack `{}` is declared with conflicting selection facts",
                        normalized.name
                    ));
                }
                continue;
            }
            expansion.selections.push(normalized.clone());
            let mut projection = LanguageProjection {
                selection: normalized,
                pack: pack.clone(),
                host: pack.host.clone(),
                platform: platform.to_string(),
                license: pack.license.clone(),
                ..Default::default()
            };
            if selection.enable && pack.license.trim().is_empty() {
                return Err(format!(
                    "language pack `{}` has no license fact",
                    pack.name
                ));
            }
            if selection.enable
                && !pack.platforms.is_empty()
                && !pack.platforms.iter().any(|item| item == platform)
            {
                return Err(format!(
                    "language pack `{}` does not support host platform `{platform}`",
                    pack.name
                ));
            }
            let missing_tools = pack
                .required_tools
                .iter()
                .filter(|tool| !pack.commands.contains_key(*tool))
                .cloned()
                .collect::<Vec<_>>();
            if selection.enable && !missing_tools.is_empty() {
                return Err(format!(
                    "language pack `{}` is missing catalog tools: {}",
                    pack.name,
                    missing_tools.join(", ")
                ));
            }
            projection.missing_tools = missing_tools;
            projection.changed.push(format!("host={}", pack.host));
            projection.changed.push(format!("platform={platform}"));
            projection.changed.push(format!("license={}", pack.license));
            if let Some(version) = selection.version.as_deref() {
                projection.changed.push(format!("version={version}"));
            }
            if let Some(channel) = selection.channel.as_deref() {
                projection.changed.push(format!("channel={channel}"));
            }
            if selection.venv {
                projection.changed.push("venv=true".to_string());
            }
            if !selection.extra_packages.is_empty() {
                projection
                    .changed
                    .push(format!("extra={}", selection.extra_packages.join(",")));
            }
            if !selection.enable {
                projection.omitted.extend(pack.packages.iter().cloned());
                projection
                    .omitted
                    .extend(pack.venv_packages.iter().cloned());
                projection
                    .omitted
                    .extend(selection.extra_packages.iter().cloned());
                projection.changed.push("enable=false".to_string());
                expansion.projections.push(projection);
                continue;
            }
            if !expansion.applied.iter().any(|item| item == &pack.name) {
                expansion.applied.push(pack.name.clone());
                expansion.packs.push(pack.clone());
            }
            for package in &pack.packages {
                let package = versioned_package(package, selection.version.as_deref());
                if !expansion.packages.iter().any(|item| item == &package) {
                    expansion.packages.push(package.clone());
                }
                projection.included.push(package);
            }
            for package in &selection.extra_packages {
                if !expansion.packages.iter().any(|item| item == package) {
                    expansion.packages.push(package.clone());
                }
                projection.included.push(package.clone());
            }
            if selection.venv {
                for package in &pack.venv_packages {
                    let package = versioned_package(package, selection.version.as_deref());
                    if !expansion.packages.iter().any(|item| item == &package) {
                        expansion.packages.push(package.clone());
                    }
                    projection.included.push(package);
                }
            } else {
                projection.omitted.extend(pack.venv_packages.iter().cloned());
            }
            for (key, value) in &pack.variables {
                merge_language_fact(&mut expansion.variables, &pack.name, "variable", key, value)?;
            }
            for (key, value) in &pack.commands {
                merge_language_fact(&mut expansion.commands, &pack.name, "command", key, value)?;
            }
            expansion.projections.push(projection);
        }
        Ok(expansion)
    }

    pub fn expand_names(&self, names: &[String]) -> Result<LanguageExpansion, String> {
        let selections = names
            .iter()
            .map(|name| LanguageSpec {
                name: name.clone(),
                enable: true,
                ..Default::default()
            })
            .collect::<Vec<_>>();
        self.expand(&selections)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LanguageExpansion {
    pub applied: Vec<String>,
    /// The exact catalog entries used by this expansion. Keeping these with
    /// the derived package list makes the expansion the one runtime fact
    /// shared by planning, trust, activation, and disclosure.
    pub packs: Vec<LanguagePack>,
    pub selections: Vec<LanguageSpec>,
    pub packages: Vec<String>,
    pub variables: BTreeMap<String, String>,
    pub commands: BTreeMap<String, String>,
    pub projections: Vec<LanguageProjection>,
}

impl LanguageExpansion {
    pub fn fingerprint(&self) -> String {
        let mut text = String::from("jet-language-expansion-v1\n");
        for selection in &self.selections {
            text.push_str("selection=");
            text.push_str(&selection.fingerprint());
            text.push('\n');
        }
        for pack in &self.packs {
            text.push_str("pack=");
            text.push_str(&pack.fingerprint());
        }
        for package in &self.packages {
            text.push_str("expanded-package=");
            text.push_str(package);
            text.push('\n');
        }
        for (name, value) in &self.variables {
            text.push_str("expanded-var=");
            text.push_str(name);
            text.push('=');
            text.push_str(value);
            text.push('\n');
        }
        for (name, command) in &self.commands {
            text.push_str("expanded-command=");
            text.push_str(name);
            text.push('=');
            text.push_str(command);
            text.push('\n');
        }
        for projection in &self.projections {
            text.push_str("projection=");
            text.push_str(&projection.fingerprint());
        }
        text
    }
}

fn language_key(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect()
}

fn validate_language_pack(pack: &LanguagePack) -> Result<(), String> {
    if pack.packages.is_empty() || pack.packages.iter().any(|package| package.trim().is_empty()) {
        return Err(format!(
            "language pack '{}' must declare at least one non-empty package",
            pack.name
        ));
    }
    if pack.venv_packages.iter().any(|package| package.trim().is_empty()) {
        return Err(format!(
            "language pack '{}' has an empty venv package",
            pack.name
        ));
    }
    if pack.variables.keys().any(|name| !valid_env_name(name)) {
        return Err(format!(
            "language pack '{}' has an invalid environment variable name",
            pack.name
        ));
    }
    if pack.host.trim().is_empty() {
        return Err(format!("language pack '{}' must declare a host", pack.name));
    }
    if pack.platforms.is_empty() || pack.platforms.iter().any(|platform| platform.trim().is_empty()) {
        return Err(format!(
            "language pack '{}' must declare at least one non-empty platform",
            pack.name
        ));
    }
    if pack.license.trim().is_empty() {
        return Err(format!("language pack '{}' must declare a license", pack.name));
    }
    if pack.commands.is_empty() {
        return Err(format!(
            "language pack '{}' must declare at least one command",
            pack.name
        ));
    }
    if pack.commands.iter().any(|(name, command)| {
        name.trim().is_empty() || command.trim().is_empty()
    }) {
        return Err(format!(
            "language pack '{}' has an empty command name or mapping",
            pack.name
        ));
    }
    if pack.required_tools.is_empty() || pack.required_tools.iter().any(|tool| tool.trim().is_empty()) {
        return Err(format!(
            "language pack '{}' must declare non-empty required tools",
            pack.name
        ));
    }
    for tool in &pack.required_tools {
        if !pack.commands.contains_key(tool) {
            return Err(format!(
                "language pack '{}' required tool '{}' has no command mapping",
                pack.name, tool
            ));
        }
    }
    Ok(())
}

fn merge_language_fact(
    facts: &mut BTreeMap<String, String>,
    pack_name: &str,
    kind: &str,
    key: &str,
    value: &str,
) -> Result<(), String> {
    if let Some(existing) = facts.get(key) {
        if existing != value {
            return Err(format!(
                "language pack '{}' conflicts with existing {} '{}'",
                pack_name, kind, key
            ));
        }
    } else {
        facts.insert(key.to_string(), value.to_string());
    }
    Ok(())
}

fn versioned_package(package: &str, version: Option<&str>) -> String {
    let Some(version) = version else {
        return package.to_string();
    };
    let Some((name, source)) = package.rsplit_once('@') else {
        return package.to_string();
    };
    if name.contains("#version=") {
        package.to_string()
    } else {
        format!("{name}#version={version}@{source}")
    }
}

fn pack(name: &str, packages: &[&str]) -> LanguagePack {
    let (commands, license, required_tools) = match name {
        "Rust" => (
            BTreeMap::from([
                ("rustc".to_string(), "rustc".to_string()),
                ("cargo".to_string(), "cargo".to_string()),
                ("rust-analyzer".to_string(), "rust-analyzer".to_string()),
                ("rustfmt".to_string(), "rustfmt".to_string()),
                ("clippy".to_string(), "clippy-driver".to_string()),
            ]),
            "Apache-2.0 OR MIT".to_string(),
            vec![
                "rustc".to_string(),
                "cargo".to_string(),
                "rust-analyzer".to_string(),
                "rustfmt".to_string(),
                "clippy".to_string(),
            ],
        ),
        "Python" => (
            BTreeMap::from([
                ("python".to_string(), "python3".to_string()),
                ("pip".to_string(), "pip".to_string()),
            ]),
            "MIT OR PSF-2.0".to_string(),
            vec!["python".to_string(), "pip".to_string()],
        ),
        "Go" => (
            BTreeMap::from([
                ("go".to_string(), "go".to_string()),
                ("gofmt".to_string(), "gofmt".to_string()),
                ("gopls".to_string(), "gopls".to_string()),
            ]),
            "BSD-3-Clause".to_string(),
            vec!["go".to_string(), "gofmt".to_string(), "gopls".to_string()],
        ),
        "JavaScript" => (
            BTreeMap::from([
                ("node".to_string(), "node".to_string()),
                ("npm".to_string(), "npm".to_string()),
                ("npx".to_string(), "npx".to_string()),
            ]),
            "MIT".to_string(),
            vec!["node".to_string(), "npm".to_string(), "npx".to_string()],
        ),
        _ => (BTreeMap::new(), String::new(), Vec::new()),
    };
    LanguagePack {
        name: name.to_string(),
        packages: packages.iter().map(|item| (*item).to_string()).collect(),
        venv_packages: (name == "Python")
            .then(|| vec!["pythonPackages.virtualenv@nixpkgs".to_string()])
            .unwrap_or_default(),
        commands,
        host: "native".to_string(),
        platforms: vec![
            "aarch64-macos".to_string(),
            "aarch64-linux".to_string(),
            "x86_64-macos".to_string(),
            "x86_64-linux".to_string(),
        ],
        license,
        required_tools,
        ..Default::default()
    }
}

fn extended_language_names() -> &'static [&'static str] {
    &[
        "Ansible",
        "C",
        "Clojure",
        "Cplusplus",
        "Crystal",
        "Cue",
        "Dart",
        "Deno",
        "Dotnet",
        "Elixir",
        "Elm",
        "Erlang",
        "Fortran",
        "Gawk",
        "Gleam",
        "Hare",
        "Haskell",
        "Helm",
        "Idris",
        "Java",
        "Jsonnet",
        "Julia",
        "Kotlin",
        "Lean4",
        "Lobster",
        "Lua",
        "Nim",
        "Nix",
        "Ocaml",
        "Odin",
        "Opentofu",
        "Pascal",
        "Perl",
        "Php",
        "Pkl",
        "Purescript",
        "R",
        "Racket",
        "Raku",
        "Robotframework",
        "Ruby",
        "Scala",
        "Shell",
        "Solidity",
        "Standardml",
        "Swift",
        "Terraform",
        "Texlive",
        "Typescript",
        "Typst",
        "Unison",
        "V",
        "Vala",
        "Zig",
    ]
}

fn extended_language_pack(name: &str) -> LanguagePack {
    let (packages, tools, license) = match name {
        "Ansible" => (vec!["ansible@nixpkgs"], vec!["ansible", "ansible-playbook"], "GPL-3.0-or-later"),
        "C" => (vec!["gcc@nixpkgs", "gnumake@nixpkgs"], vec!["gcc", "make"], "GPL-3.0-or-later"),
        "Clojure" => (vec!["clojure@nixpkgs"], vec!["clojure"], "EPL-1.0"),
        "Cplusplus" => (vec!["gcc@nixpkgs", "cmake@nixpkgs"], vec!["g++", "cmake"], "GPL-3.0-or-later"),
        "Crystal" => (vec!["crystal@nixpkgs"], vec!["crystal"], "Apache-2.0"),
        "Cue" => (vec!["cue@nixpkgs"], vec!["cue"], "Apache-2.0"),
        "Dart" => (vec!["dart@nixpkgs"], vec!["dart"], "BSD-3-Clause"),
        "Deno" => (vec!["deno@nixpkgs"], vec!["deno"], "MIT"),
        "Dotnet" => (vec!["dotnet-sdk@nixpkgs"], vec!["dotnet"], "MIT"),
        "Elixir" => (vec!["elixir@nixpkgs"], vec!["elixir", "mix"], "Apache-2.0"),
        "Elm" => (vec!["elm@nixpkgs"], vec!["elm"], "BSD-3-Clause"),
        "Erlang" => (vec!["erlang@nixpkgs", "rebar3@nixpkgs"], vec!["erl", "rebar3"], "Apache-2.0"),
        "Fortran" => (vec!["gfortran@nixpkgs"], vec!["gfortran"], "GPL-3.0-or-later"),
        "Gawk" => (vec!["gawk@nixpkgs"], vec!["gawk"], "GPL-3.0-or-later"),
        "Gleam" => (vec!["gleam@nixpkgs"], vec!["gleam"], "Apache-2.0"),
        "Hare" => (vec!["hare@nixpkgs"], vec!["hare"], "GPL-3.0-or-later"),
        "Haskell" => (vec!["ghc@nixpkgs", "cabal-install@nixpkgs"], vec!["ghc", "cabal"], "BSD-3-Clause"),
        "Helm" => (vec!["kubernetes-helm@nixpkgs"], vec!["helm"], "Apache-2.0"),
        "Idris" => (vec!["idris2@nixpkgs"], vec!["idris2"], "BSD-3-Clause"),
        "Java" => (vec!["jdk@nixpkgs", "maven@nixpkgs"], vec!["java", "javac", "mvn"], "GPL-2.0-with-classpath-exception"),
        "Jsonnet" => (vec!["jsonnet@nixpkgs"], vec!["jsonnet"], "Apache-2.0"),
        "Julia" => (vec!["julia@nixpkgs"], vec!["julia"], "MIT"),
        "Kotlin" => (vec!["kotlin@nixpkgs"], vec!["kotlinc"], "Apache-2.0"),
        "Lean4" => (vec!["lean4@nixpkgs"], vec!["lean", "lake"], "Apache-2.0"),
        "Lobster" => (vec!["lobster@nixpkgs"], vec!["lobster"], "MIT"),
        "Lua" => (vec!["lua@nixpkgs", "luarocks@nixpkgs"], vec!["lua", "luarocks"], "MIT"),
        "Nim" => (vec!["nim@nixpkgs"], vec!["nim", "nimble"], "MIT"),
        "Nix" => (vec!["nix@nixpkgs"], vec!["nix"], "LGPL-2.1-or-later"),
        "Ocaml" => (vec!["ocaml@nixpkgs", "opam@nixpkgs"], vec!["ocaml", "opam"], "LGPL-2.1-with-linking-exception"),
        "Odin" => (vec!["odin@nixpkgs"], vec!["odin"], "BSD-3-Clause"),
        "Opentofu" => (vec!["opentofu@nixpkgs"], vec!["tofu"], "MPL-2.0"),
        "Pascal" => (vec!["fpc@nixpkgs"], vec!["fpc"], "GPL-2.0-or-later"),
        "Perl" => (vec!["perl@nixpkgs"], vec!["perl"], "Artistic-1.0 OR GPL-1.0-or-later"),
        "Php" => (vec!["php@nixpkgs", "composer@nixpkgs"], vec!["php", "composer"], "PHP-3.01"),
        "Pkl" => (vec!["pkl@nixpkgs"], vec!["pkl"], "Apache-2.0"),
        "Purescript" => (vec!["purescript@nixpkgs", "spago@nixpkgs"], vec!["purs", "spago"], "BSD-3-Clause"),
        "R" => (vec!["R@nixpkgs"], vec!["R"], "GPL-2.0-or-later"),
        "Racket" => (vec!["racket@nixpkgs"], vec!["racket"], "LGPL-3.0-or-later"),
        "Raku" => (vec!["rakudo@nixpkgs"], vec!["raku"], "Artistic-2.0"),
        "Robotframework" => (vec!["robotframework@nixpkgs"], vec!["robot"], "Apache-2.0"),
        "Ruby" => (vec!["ruby@nixpkgs", "bundler@nixpkgs"], vec!["ruby", "bundle"], "BSD-2-Clause"),
        "Scala" => (vec!["scala@nixpkgs", "sbt@nixpkgs"], vec!["scala", "sbt"], "Apache-2.0"),
        "Shell" => (vec!["bash@nixpkgs", "shellcheck@nixpkgs"], vec!["bash", "shellcheck"], "GPL-3.0-or-later"),
        "Solidity" => (vec!["solc@nixpkgs"], vec!["solc"], "GPL-3.0-or-later"),
        "Standardml" => (vec!["smlnj@nixpkgs"], vec!["sml"], "BSD-3-Clause"),
        "Swift" => (vec!["swift@nixpkgs"], vec!["swiftc"], "Apache-2.0"),
        "Terraform" => (vec!["terraform@nixpkgs"], vec!["terraform"], "MPL-2.0"),
        "Texlive" => (vec!["texlive@nixpkgs"], vec!["pdflatex"], "GPL-3.0-or-later"),
        "Typescript" => (vec!["nodejs@nixpkgs", "nodePackages.typescript@nixpkgs"], vec!["node", "npm", "tsc"], "Apache-2.0"),
        "Typst" => (vec!["typst@nixpkgs"], vec!["typst"], "Apache-2.0"),
        "Unison" => (vec!["unison-language@nixpkgs"], vec!["unison"], "MIT"),
        "V" => (vec!["vlang@nixpkgs"], vec!["v"], "MIT"),
        "Vala" => (vec!["vala@nixpkgs"], vec!["valac"], "LGPL-2.1-or-later"),
        "Zig" => (vec!["zig@nixpkgs"], vec!["zig"], "MIT"),
        _ => panic!("unknown extended language pack: {name}"),
    };
    catalog_pack(name, &packages, &tools, license)
}

fn catalog_pack(
    name: &str,
    packages: &[&str],
    tools: &[&str],
    license: &str,
) -> LanguagePack {
    LanguagePack {
        name: name.to_string(),
        packages: packages.iter().map(|package| (*package).to_string()).collect(),
        commands: tools
            .iter()
            .map(|tool| ((*tool).to_string(), (*tool).to_string()))
            .collect(),
        host: "native".to_string(),
        platforms: vec![
            "aarch64-macos".to_string(),
            "aarch64-linux".to_string(),
            "x86_64-macos".to_string(),
            "x86_64-linux".to_string(),
        ],
        license: license.to_string(),
        required_tools: tools.iter().map(|tool| (*tool).to_string()).collect(),
        ..Default::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookAction {
    /// The normal lifecycle spelling names one checked `#Job fn`.
    Task(String),
    /// The explicit expert escape remains a trust-gated command record.
    Command(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSpec {
    pub name: String,
    pub action: HookAction,
    pub cwd: Option<String>,
    pub trusted: bool,
}

/// One dotenv source and its disclosure policy. A bare string uses the safe
/// beginner default: load the file's ordinary variables. The record form adds
/// an explicit allowlist and names which allowed variables are secrets. Secret
/// names are facts only; values never enter plan fingerprints or dossiers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DotenvSpec {
    pub file: String,
    pub allow: Vec<String>,
    pub secrets: Vec<String>,
}

impl DotenvSpec {
    pub fn fingerprint(&self) -> String {
        format!(
            "{}\t{}\t{}",
            self.file,
            self.allow.join(","),
            self.secrets.join(",")
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadPolicy {
    Never,
    Prompt,
    Watch { paths: Vec<String>, debounce_ms: u64 },
}

impl Default for ReloadPolicy {
    fn default() -> Self {
        Self::Prompt
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvironmentLifecycle {
    pub dotenv: Vec<DotenvSpec>,
    pub unset: Vec<String>,
    pub on_enter: Vec<HookSpec>,
    pub checks: Vec<HookSpec>,
    pub reload: ReloadPolicy,
    /// Distinguish the default policy from an author-provided policy when
    /// several imported modules are merged.
    pub reload_explicit: bool,
}

impl EnvironmentLifecycle {
    pub fn fingerprint(&self) -> String {
        let mut text = String::new();
        for dotenv in &self.dotenv {
            text.push_str("dotenv\t");
            text.push_str(&dotenv.fingerprint());
            text.push('\n');
        }
        for name in &self.unset {
            text.push_str("unset\t");
            text.push_str(name);
            text.push('\n');
        }
        for hook in self.on_enter.iter().chain(self.checks.iter()) {
            text.push_str("hook\t");
            text.push_str(&hook.name);
            text.push('\t');
            match &hook.action {
                HookAction::Task(task) => {
                    text.push_str("task:");
                    text.push_str(task);
                }
                HookAction::Command(command) => {
                    text.push_str("command:");
                    text.push_str(command);
                }
            }
            text.push('\t');
            text.push_str(hook.cwd.as_deref().unwrap_or(""));
            text.push('\t');
            text.push_str(if hook.trusted { "trusted" } else { "untrusted" });
            text.push('\n');
        }
        match &self.reload {
            ReloadPolicy::Never => text.push_str("reload\tnever\n"),
            ReloadPolicy::Prompt => text.push_str("reload\tprompt\n"),
            ReloadPolicy::Watch { paths, debounce_ms } => {
                text.push_str("reload\twatch\t");
                text.push_str(&paths.join(","));
                text.push('\t');
                text.push_str(&debounce_ms.to_string());
                text.push('\n');
            }
        }
        text.push_str("reload-explicit=");
        text.push_str(if self.reload_explicit { "true\n" } else { "false\n" });
        text
    }
}

pub fn presets_from_value(value: &CtValue) -> Result<Vec<PresetSpec>, String> {
    profile_entries(value)?
        .into_iter()
        .map(|(name, value)| profile_from_value(name, value))
        .collect()
}

pub fn languages_from_value(value: &CtValue) -> Result<Vec<LanguageSpec>, String> {
    let mut selections = match value {
        CtValue::Map(values) => values
            .iter()
            .map(|(key, value)| {
                let CtKey::Str(name) = key else {
                    return Err("language maps need string language names".to_string());
                };
                language_record(Some(name), value)
            })
            .collect::<Result<Vec<_>, _>>()?,
        CtValue::List(values) => values
            .iter()
            .map(|value| match value {
                CtValue::Str(name) => language_name(name),
                CtValue::Enum { variant, args, .. } if args.is_empty() => {
                    language_name(variant.rsplit('.').next().unwrap_or(variant))
                }
                CtValue::Struct { .. } => language_record(None, value),
                _ => Err(
                    "language lists need names or typed Lang records".to_string(),
                ),
            })
            .collect::<Result<Vec<_>, _>>()?,
        CtValue::Struct { type_name, .. }
            if type_name.rsplit('.').next().unwrap_or(type_name) == "Lang" => {
            vec![language_record(None, value)?]
        }
        _ => {
            return Err(
                "languages must be a map of Lang records or a list of names/ Lang records"
                    .to_string(),
            )
        }
    };

    let mut unique = Vec::with_capacity(selections.len());
    for selection in selections.drain(..) {
        if let Some(existing) = unique.iter().find(|item: &&LanguageSpec| item.key() == selection.key()) {
            if *existing != selection {
                return Err(format!(
                    "language pack `{}` is declared with conflicting selection facts",
                    selection.name
                ));
            }
            continue;
        }
        unique.push(selection);
    }
    Ok(unique)
}

fn language_name(name: &str) -> Result<LanguageSpec, String> {
    if name.trim().is_empty() || language_key(name).is_empty() {
        return Err("language pack names cannot be empty".to_string());
    }
    Ok(LanguageSpec {
        name: name.to_string(),
        enable: true,
        ..Default::default()
    })
}

fn language_record(name_hint: Option<&str>, value: &CtValue) -> Result<LanguageSpec, String> {
    let CtValue::Struct { type_name, .. } = value else {
        return Err("language selections must use typed Lang records".to_string());
    };
    let kind = type_name.rsplit('.').next().unwrap_or(type_name);
    if kind != "Lang" {
        return Err(format!("language selection must be a Lang record, not `{type_name}`"));
    }
    let fields = checked_unique_fields_named(
        value,
        "language selection",
        &["name", "enable", "version", "channel", "venv", "extra", "extras", "packages"],
    )?;
    let field_name = fields
        .get("name")
        .map(|value| {
            string_value(value)
                .ok_or_else(|| "language selection `name` must be a string".to_string())
        })
        .transpose()?;
    let name = match (name_hint, field_name.as_deref()) {
        (Some(key), Some(field)) if language_key(key) != language_key(field) => {
            return Err(format!(
                "language map key `{key}` conflicts with Lang.name `{field}`"
            ));
        }
        (Some(key), _) => key.to_string(),
        (None, Some(field)) => field.to_string(),
        (None, None) => {
            return Err("a Lang record needs a language name or a map key".to_string())
        }
    };
    let name = language_name(&name)?.name;
    let enable = fields
        .get("enable")
        .ok_or_else(|| "a Lang record needs a Bool `enable` field".to_string())
        .and_then(|value| match value {
            CtValue::Bool(enable) => Ok(*enable),
            _ => Err("Lang.enable must be Bool".to_string()),
        })?;
    let version = fields
        .get("version")
        .map(|value| language_token(value, "Lang.version"))
        .transpose()?;
    let channel = fields
        .get("channel")
        .map(|value| language_token(value, "Lang.channel"))
        .transpose()?;
    let venv = fields
        .get("venv")
        .map(|value| match value {
            CtValue::Bool(enabled) => Ok(*enabled),
            _ => Err("Lang.venv must be Bool".to_string()),
        })
        .transpose()?
        .unwrap_or(false);
    let mut extra_packages = None;
    for alias in ["extra", "extras", "packages"] {
        if let Some(value) = fields.get(alias) {
            let packages = list_strings_named(value, &format!("Lang.{alias}"))?;
            if packages.iter().any(|package| {
                package.trim().is_empty() || package.chars().any(|character| character.is_whitespace())
            }) {
                return Err(format!("Lang.{alias} must contain non-empty package refs"));
            }
            if let Some(existing) = &extra_packages {
                if existing != &packages {
                    return Err("Lang extra, extras, and packages fields conflict".to_string());
                }
            } else {
                extra_packages = Some(packages);
            }
        }
    }
    Ok(LanguageSpec {
        name,
        enable,
        version,
        channel,
        venv,
        extra_packages: extra_packages.unwrap_or_default(),
    })
}

fn language_token(value: &CtValue, field: &str) -> Result<String, String> {
    let token = match value {
        CtValue::Str(value) => value.clone(),
        CtValue::Enum { variant, args, .. } if args.is_empty() => {
            variant.rsplit('.').next().unwrap_or(variant).to_string()
        }
        _ => return Err(format!("{field} must be a string or a parameterless enum")),
    };
    if token.trim().is_empty() || token.chars().any(char::is_whitespace) {
        return Err(format!("{field} must be a non-empty token"));
    }
    Ok(token)
}

pub fn lifecycle_from_field(
    lifecycle: &mut EnvironmentLifecycle,
    name: &str,
    value: &CtValue,
) -> Result<bool, String> {
    match name {
        "dotenv" => {
            lifecycle.dotenv = dotenv_from_value(value)?;
            Ok(true)
        }
        "unset" => {
            lifecycle.unset = env_names_named(value, "unset")?;
            Ok(true)
        }
        "on_enter" => {
            lifecycle.on_enter = hooks_from_value("on_enter", value)?;
            Ok(true)
        }
        "checks" => {
            lifecycle.checks = hooks_from_value("check", value)?;
            Ok(true)
        }
        "reload" => {
            lifecycle.reload = reload_from_value(value)?;
            lifecycle.reload_explicit = true;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Parse both `dotenv: [".env"]` and the expert record form
/// `dotenv: Dotenv.{ file: ".env", allow: ["PORT"], secrets: ["TOKEN"] }`.
/// The shape is closed and validated before the lifecycle plan is returned.
pub fn dotenv_from_value(value: &CtValue) -> Result<Vec<DotenvSpec>, String> {
    let values = match value {
        CtValue::Str(_) | CtValue::Struct { .. } => vec![value.clone()],
        CtValue::List(values) => values.clone(),
        _ => return Err("dotenv must be a list of files or Dotenv records".to_string()),
    };
    let mut specs = Vec::with_capacity(values.len());
    for value in values {
        let spec = match value {
            CtValue::Str(file) => DotenvSpec {
                file,
                ..Default::default()
            },
            value => {
                let fields = checked_struct_fields(&value)?;
                let file = fields
                    .get("file")
                    .and_then(string_value)
                    .ok_or_else(|| "a Dotenv record needs a string `file`".to_string())?;
                let allow = fields
                    .get("allow")
                    .map(list_strings_checked)
                    .transpose()?
                    .unwrap_or_default();
                let secrets = fields
                    .get("secrets")
                    .map(list_strings_checked)
                    .transpose()?
                    .unwrap_or_default();
                DotenvSpec { file, allow, secrets }
            }
        };
        validate_dotenv_spec(&spec)?;
        specs.push(spec);
    }
    let mut seen = BTreeMap::<String, DotenvSpec>::new();
    for spec in specs {
        if let Some(existing) = seen.get(&spec.file) {
            if existing != &spec {
                return Err(format!(
                    "dotenv file `{}` has conflicting policies",
                    spec.file
                ));
            }
            continue;
        }
        seen.insert(spec.file.clone(), spec);
    }
    Ok(seen.into_values().collect())
}

fn validate_dotenv_spec(spec: &DotenvSpec) -> Result<(), String> {
    if spec.file.is_empty()
        || Path::new(&spec.file).is_absolute()
        || Path::new(&spec.file)
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(format!(
            "dotenv file `{}` must stay inside the project",
            spec.file
        ));
    }
    for name in spec.allow.iter().chain(spec.secrets.iter()) {
        if !valid_env_name(name) {
            return Err(format!("dotenv variable `{name}` is not a valid environment name"));
        }
    }
    if !spec.allow.is_empty() && spec.secrets.iter().any(|name| !spec.allow.contains(name)) {
        return Err(format!(
            "dotenv secrets must be included in the allowlist for `{}`",
            spec.file
        ));
    }
    Ok(())
}

pub fn valid_env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn checked_struct_fields(value: &CtValue) -> Result<BTreeMap<String, CtValue>, String> {
    let CtValue::Struct { fields, .. } = value else {
        return Err("dotenv entries must be strings or Dotenv records".to_string());
    };
    let mut result = BTreeMap::new();
    for (name, value) in fields {
        if let Some(existing) = result.get(name) {
            if existing != value {
                return Err(format!("Dotenv field `{name}` is declared with conflicting values"));
            }
            continue;
        }
        result.insert(name.clone(), value.clone());
    }
    for name in result.keys() {
        if !matches!(name.as_str(), "file" | "allow" | "secrets") {
            return Err(format!("unknown Dotenv field `{name}`"));
        }
    }
    Ok(result)
}

fn list_strings_checked(value: &CtValue) -> Result<Vec<String>, String> {
    let CtValue::List(values) = value else {
        return Err("Dotenv `allow` and `secrets` must be lists of strings".to_string());
    };
    values
        .iter()
        .map(|value| {
            string_value(value).ok_or_else(|| "Dotenv variable names must be strings".to_string())
        })
        .collect()
}

fn profile_entries(value: &CtValue) -> Result<Vec<(String, CtValue)>, String> {
    match value {
        CtValue::Map(values) => values
            .iter()
            .map(|(key, value)| match key {
                CtKey::Str(name) if !name.trim().is_empty() => Ok((name.clone(), value.clone())),
                _ => Err("profile maps need non-empty string names".to_string()),
            })
            .collect(),
        CtValue::Struct { fields, .. } => fields
            .iter()
            .map(|(name, value)| {
                if name.trim().is_empty() {
                    Err("profile records need non-empty names".to_string())
                } else {
                    Ok((name.clone(), value.clone()))
                }
            })
            .collect(),
        CtValue::List(values) => values
            .iter()
            .map(|value| {
                let fields = checked_struct_fields_named(value, "profile")?;
                let name = fields
                    .get("name")
                    .and_then(string_value)
                    .filter(|name| !name.trim().is_empty())
                    .ok_or_else(|| "profile list entries need a non-empty `name`".to_string())?;
                Ok((name, value.clone()))
            })
            .collect(),
        _ => Err("profiles must be a map, record, or list of named Profile records".to_string()),
    }
}

fn profile_from_value(name: String, value: CtValue) -> Result<PresetSpec, String> {
    let fields = checked_struct_fields_named(&value, "profile")?;
    let declared_name = fields.get("name").and_then(string_value);
    if let Some(declared_name) = declared_name.as_deref() {
        if declared_name != name {
            return Err(format!(
                "profile map key `{name}` conflicts with its `name: {declared_name}` field"
            ));
        }
    }
    let extends = fields
        .get("extends")
        .map(|value| list_strings_named(value, "profile.extends"))
        .transpose()?
        .unwrap_or_default();
    let packages = fields
        .get("packages")
        .map(|value| list_strings_named(value, "profile.packages"))
        .transpose()?
        .unwrap_or_default();
    let variables = fields
        .get("variables")
        .map(|value| string_map_named(value, "profile.variables"))
        .transpose()?
        .unwrap_or_default();
    let hostname = optional_string_named(&fields, "hostname", "profile.hostname")?;
    let user = optional_string_named(&fields, "user", "profile.user")?;
    Ok(PresetSpec {
        name,
        extends,
        packages,
        variables,
        hostname,
        user,
    })
}

fn checked_struct_fields_named(
    value: &CtValue,
    scope: &str,
) -> Result<BTreeMap<String, CtValue>, String> {
    let CtValue::Struct { fields, .. } = value else {
        return Err(format!("{scope} entries must be typed records"));
    };
    let mut result = BTreeMap::new();
    for (name, value) in fields {
        if let Some(existing) = result.get(name) {
            if existing != value {
                return Err(format!("{scope}.{name} is declared with conflicting values"));
            }
            continue;
        }
        result.insert(name.clone(), value.clone());
    }
    for name in result.keys() {
        if !matches!(name.as_str(), "name" | "extends" | "packages" | "variables" | "hostname" | "user") {
            return Err(format!("unknown {scope} field `{name}`"));
        }
    }
    Ok(result)
}

fn list_strings_named(value: &CtValue, scope: &str) -> Result<Vec<String>, String> {
    let CtValue::List(values) = value else {
        return Err(format!("{scope} must be a list of strings"));
    };
    values
        .iter()
        .map(|value| string_value(value).ok_or_else(|| format!("{scope} must contain only strings")))
        .collect()
}

fn env_names_named(value: &CtValue, scope: &str) -> Result<Vec<String>, String> {
    let names = list_strings_named(value, scope)?;
    for name in &names {
        if !valid_env_name(name) {
            return Err(format!("{scope} variable '{name}' is not a valid environment name"));
        }
    }
    Ok(names)
}

fn string_map_named(value: &CtValue, scope: &str) -> Result<BTreeMap<String, String>, String> {
    match value {
        CtValue::Map(values) => values
            .iter()
            .map(|(key, value)| {
                let CtKey::Str(key) = key else {
                    return Err(format!("{scope} keys must be strings"));
                };
                if !valid_env_name(key) {
                    return Err(format!("{scope} variable '{key}' is not a valid environment name"));
                }
                let value = string_value(value)
                    .ok_or_else(|| format!("{scope} values must be strings"))?;
                Ok((key.clone(), value))
            })
            .collect(),
        CtValue::Struct { fields, .. } => fields
            .iter()
            .map(|(key, value)| {
                if !valid_env_name(key) {
                    return Err(format!("{scope} variable '{key}' is not a valid environment name"));
                }
                let value = string_value(value)
                    .ok_or_else(|| format!("{scope} values must be strings"))?;
                Ok((key.clone(), value))
            })
            .collect(),
        _ => Err(format!("{scope} must be a record of string values")),
    }
}

fn optional_string_named(
    fields: &BTreeMap<String, CtValue>,
    name: &str,
    scope: &str,
) -> Result<Option<String>, String> {
    fields
        .get(name)
        .map(|value| string_value(value).ok_or_else(|| format!("{scope} must be a string")))
        .transpose()
}

fn hooks_from_value(prefix: &str, value: &CtValue) -> Result<Vec<HookSpec>, String> {
    let CtValue::List(values) = value else {
        return Err(format!("{prefix} must be a list of commands or hook records"));
    };
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if let Some(command) = string_value(value) {
                if !valid_task_name(&command) {
                    return Err(format!(
                        "{prefix}[{index}] must name a declared #Job task; arbitrary commands need a trusted hook record"
                    ));
                }
                return Ok(HookSpec {
                    name: format!("{prefix}.{index}"),
                    action: HookAction::Task(command),
                    cwd: None,
                    trusted: false,
                });
            }
            let fields = checked_unique_fields_named(value, &format!("{prefix}[{index}]"), &["name", "command", "cwd", "trusted"])?;
            let command = fields
                .get("command")
                .and_then(string_value)
                .filter(|command| !command.trim().is_empty())
                .ok_or_else(|| format!("{prefix}[{index}] needs a non-empty string `command`"))?;
            let name = fields
                .get("name")
                .map(|value| string_value(value).ok_or_else(|| format!("{prefix}[{index}].name must be a string")))
                .transpose()?
                .unwrap_or_else(|| format!("{prefix}.{index}"));
            let cwd = fields
                .get("cwd")
                .map(|value| string_value(value).ok_or_else(|| format!("{prefix}[{index}].cwd must be a string")))
                .transpose()?;
            if let Some(cwd) = &cwd {
                let path = Path::new(cwd);
                if cwd.is_empty()
                    || path.is_absolute()
                    || path.components().any(|component| component == std::path::Component::ParentDir)
                {
                    return Err(format!("{prefix}[{index}].cwd must stay inside the project"));
                }
            }
            let trusted = fields
                .get("trusted")
                .map(|value| match value {
                    CtValue::Bool(trusted) => Ok(*trusted),
                    _ => Err(format!("{prefix}[{index}].trusted must be Bool")),
                })
                .transpose()?
                .unwrap_or(false);
            Ok(HookSpec {
                name,
                action: HookAction::Command(command),
                cwd,
                trusted,
            })
        })
        .collect()
}

fn valid_task_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn reload_from_value(value: &CtValue) -> Result<ReloadPolicy, String> {
    match value {
        CtValue::Enum { variant, args, .. } => {
            let name = variant.rsplit('.').next().unwrap_or(variant);
            match name {
                "Never" if args.is_empty() => Ok(ReloadPolicy::Never),
                "Prompt" if args.is_empty() => Ok(ReloadPolicy::Prompt),
                "Watch" => reload_watch_from_args(args),
                _ => Err(format!("unknown or malformed reload policy `{variant}`")),
            }
        }
        CtValue::Struct { .. } => {
            let fields = checked_unique_fields_named(value, "reload", &["watch", "debounce"])?;
            let paths = fields
                .get("watch")
                .map(|value| reload_paths(value))
                .transpose()?
                .unwrap_or_default();
            let debounce_ms = fields
                .get("debounce")
                .map(duration_ms)
                .transpose()?
                .unwrap_or(250);
            Ok(ReloadPolicy::Watch { paths, debounce_ms })
        }
        CtValue::Str(value) if value.eq_ignore_ascii_case("never") => Ok(ReloadPolicy::Never),
        CtValue::Str(value) if value.eq_ignore_ascii_case("prompt") => Ok(ReloadPolicy::Prompt),
        CtValue::Str(value) if value.eq_ignore_ascii_case("watch") => Ok(ReloadPolicy::Watch {
            paths: Vec::new(),
            debounce_ms: 250,
        }),
        CtValue::Str(value) => Err(format!("unknown reload policy `{value}`")),
        _ => Err("reload must be Never, Prompt, Watch, or a Reload record".to_string()),
    }
}

fn reload_watch_from_args(args: &[(Option<String>, CtValue)]) -> Result<ReloadPolicy, String> {
    let mut paths = Vec::new();
    let mut debounce_ms = 250;
    for (name, value) in args {
        match name.as_deref() {
            Some("watch") => paths = reload_paths(value)?,
            Some("debounce") => debounce_ms = duration_ms(value)?,
            None if paths.is_empty() && matches!(value, CtValue::List(_)) => {
                paths = reload_paths(value)?
            }
            None => debounce_ms = duration_ms(value)?,
            Some(other) => return Err(format!("unknown Reload.Watch field `{other}`")),
        }
    }
    Ok(ReloadPolicy::Watch { paths, debounce_ms })
}

fn reload_paths(value: &CtValue) -> Result<Vec<String>, String> {
    let paths = list_strings_named(value, "reload.watch")?;
    for path in &paths {
        let path_ref = Path::new(path);
        if path.is_empty()
            || path_ref.is_absolute()
            || path_ref.components().any(|component| component == std::path::Component::ParentDir)
        {
            return Err(format!("reload.watch path `{path}` must stay inside the project"));
        }
    }
    Ok(paths)
}

fn duration_ms(value: &CtValue) -> Result<u64, String> {
    let raw = match value {
        CtValue::Int(value) if *value > 0 => u64::try_from(*value).ok(),
        CtValue::Struct { type_name, fields } => {
            let unit = type_name.rsplit('.').next().unwrap_or(type_name);
            let fields = checked_unique_fields(fields, "duration")?;
            let amount = fields
                .get("value")
                .or_else(|| fields.get("milliseconds"))
                .and_then(|value| match value {
                    CtValue::Int(value) if *value > 0 => u64::try_from(*value).ok(),
                    _ => None,
                });
            amount.map(|amount| match unit {
                "Seconds" | "Second" => amount.saturating_mul(1_000),
                "Minutes" | "Minute" => amount.saturating_mul(60_000),
                "Hours" | "Hour" => amount.saturating_mul(3_600_000),
                _ => amount,
            })
        }
        CtValue::Enum { variant, args, .. } => {
            let amount = args.first().and_then(|(_, value)| match value {
                CtValue::Int(value) if *value > 0 => u64::try_from(*value).ok(),
                _ => None,
            });
            amount.map(|amount| match variant.rsplit('.').next().unwrap_or(variant) {
                "Seconds" | "Second" => amount.saturating_mul(1_000),
                "Minutes" | "Minute" => amount.saturating_mul(60_000),
                "Hours" | "Hour" => amount.saturating_mul(3_600_000),
                _ => amount,
            })
        }
        _ => None,
    };
    raw.filter(|value| *value > 0)
        .ok_or_else(|| "reload debounce must be a positive duration".to_string())
}

fn checked_unique_fields_named(
    value: &CtValue,
    scope: &str,
    allowed: &[&str],
) -> Result<BTreeMap<String, CtValue>, String> {
    let CtValue::Struct { fields, .. } = value else {
        return Err(format!("{scope} must be a typed record"));
    };
    let fields = checked_unique_fields(fields, scope)?;
    for name in fields.keys() {
        if !allowed.iter().any(|allowed| *allowed == name) {
            return Err(format!("unknown {scope} field `{name}`"));
        }
    }
    Ok(fields)
}

fn checked_unique_fields(
    fields: &[(String, CtValue)],
    scope: &str,
) -> Result<BTreeMap<String, CtValue>, String> {
    let mut result = BTreeMap::new();
    for (name, value) in fields {
        if let Some(existing) = result.get(name) {
            if existing != value {
                return Err(format!("{scope}.{name} is declared with conflicting values"));
            }
            continue;
        }
        result.insert(name.clone(), value.clone());
    }
    Ok(result)
}

fn checked_managed_fields(
    value: &CtValue,
    scope: &str,
) -> Result<BTreeMap<String, CtValue>, ManagedFileError> {
    let CtValue::Struct { fields, .. } = value else {
        return Err(ManagedFileError::InvalidEntry(format!(
            "managed file {scope} must be a typed record"
        )));
    };
    let fields = checked_unique_fields(fields, &format!("managed file {scope}"))
        .map_err(ManagedFileError::InvalidEntry)?;
    for name in fields.keys() {
        if !matches!(name.as_str(), "destination" | "source" | "content" | "mode" | "permissions" | "sensitive" | "generation") {
            return Err(ManagedFileError::InvalidEntry(format!(
                "unknown managed file field `{name}`"
            )));
        }
    }
    Ok(fields)
}

fn string_value(value: &CtValue) -> Option<String> {
    match value {
        CtValue::Str(value) => Some(value.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_resolution_is_parent_first_and_deduplicated() {
        let mut set = PresetSet::default();
        set.insert(PresetSpec {
            name: "base".to_string(),
            packages: vec!["git@nixpkgs".to_string()],
            ..Default::default()
        }).unwrap();
        set.insert(PresetSpec {
            name: "dev".to_string(),
            extends: vec!["base".to_string()],
            packages: vec!["git@nixpkgs".to_string(), "rustc@nixpkgs".to_string()],
            ..Default::default()
        }).unwrap();
        let resolved = set.resolve("dev").unwrap();
        assert_eq!(resolved.applied, vec!["base", "dev"]);
        assert_eq!(resolved.packages, vec!["git@nixpkgs", "rustc@nixpkgs"]);
    }

    #[test]
    fn ambient_hostname_profiles_merge_before_user_profiles() {
        let mut set = PresetSet::default();
        set.insert(PresetSpec {
            name: "host".to_string(),
            hostname: Some("build-01".to_string()),
            packages: vec!["git@nixpkgs".to_string()],
            variables: BTreeMap::from([("HOST_MODE".to_string(), "host".to_string())]),
            ..Default::default()
        })
        .unwrap();
        set.insert(PresetSpec {
            name: "sam".to_string(),
            user: Some("sam".to_string()),
            packages: vec!["ripgrep@nixpkgs".to_string()],
            variables: BTreeMap::from([("USER_MODE".to_string(), "user".to_string())]),
            ..Default::default()
        })
        .unwrap();

        let selected = set.auto_select_many("build-01", "sam");
        assert_eq!(selected, vec!["host", "sam"]);
        let resolved = set.resolve_many(&selected).unwrap();
        assert_eq!(resolved.name, "host+sam");
        assert_eq!(resolved.selected_presets, selected);
        assert_eq!(resolved.packages, vec!["git@nixpkgs", "ripgrep@nixpkgs"]);
        assert_eq!(resolved.variables.get("HOST_MODE"), Some(&"host".to_string()));
        assert_eq!(resolved.variables.get("USER_MODE"), Some(&"user".to_string()));
    }

    #[test]
    fn ambient_profile_variable_conflicts_are_rejected() {
        let mut set = PresetSet::default();
        set.insert(PresetSpec {
            name: "host".to_string(),
            hostname: Some("build-01".to_string()),
            variables: BTreeMap::from([("MODE".to_string(), "host".to_string())]),
            ..Default::default()
        })
        .unwrap();
        set.insert(PresetSpec {
            name: "sam".to_string(),
            user: Some("sam".to_string()),
            variables: BTreeMap::from([("MODE".to_string(), "user".to_string())]),
            ..Default::default()
        })
        .unwrap();

        let selected = set.auto_select_many("build-01", "sam");
        assert_eq!(selected, vec!["host", "sam"]);
        assert!(matches!(
            set.resolve_many(&selected),
            Err(PresetError::Conflict { name }) if name == "sam.MODE"
        ));
    }

    #[test]
    fn ambient_user_precedes_default_and_default_is_last_resort() {
        let mut set = PresetSet::default();
        set.insert(PresetSpec {
            name: "default".to_string(),
            packages: vec!["default@nixpkgs".to_string()],
            ..Default::default()
        })
        .unwrap();
        set.insert(PresetSpec {
            name: "sam".to_string(),
            user: Some("sam".to_string()),
            packages: vec!["user@nixpkgs".to_string()],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(set.auto_select_many("other", "sam"), vec!["sam"]);
        assert_eq!(set.auto_select_many("other", "other"), vec!["default"]);
    }

    #[test]
    fn profile_cycles_are_rejected() {
        let mut set = PresetSet::default();
        set.insert(PresetSpec {
            name: "a".to_string(),
            extends: vec!["b".to_string()],
            ..Default::default()
        }).unwrap();
        set.insert(PresetSpec {
            name: "b".to_string(),
            extends: vec!["a".to_string()],
            ..Default::default()
        }).unwrap();
        assert!(matches!(set.resolve("a"), Err(PresetError::Cycle(_))));
    }

    #[test]
    fn catalog_has_the_core_language_families() {
        let catalog = LanguagePackCatalog::builtin();
        assert_eq!(catalog.names().len(), 58);
        for name in ["Rust", "Python", "Go", "JavaScript"] {
            assert!(catalog.get(name).is_some());
        }
        let expanded = catalog
            .expand(&[
                LanguageSpec {
                    name: "Rust".to_string(),
                    enable: true,
                    ..Default::default()
                },
                LanguageSpec {
                    name: "Python".to_string(),
                    enable: true,
                    ..Default::default()
                },
            ])
            .unwrap();
        assert!(expanded.packages.contains(&"rustc@nixpkgs".to_string()));
        assert!(expanded.packages.contains(&"python@nixpkgs".to_string()));
    }

    #[test]
    fn catalog_covers_the_extended_language_families_with_tool_facts() {
        let catalog = LanguagePackCatalog::builtin();
        assert_eq!(extended_language_names().len(), 54);
        for name in extended_language_names() {
            let pack = catalog.get(name).unwrap_or_else(|| panic!("missing pack {name}"));
            assert!(!pack.packages.is_empty(), "{name} has no package facts");
            assert!(!pack.commands.is_empty(), "{name} has no command facts");
            assert!(!pack.license.is_empty(), "{name} has no license fact");
            assert_eq!(pack.required_tools.len(), pack.commands.len(), "{name} tool facts drift");
        }
        let selections = catalog
            .names()
            .into_iter()
            .map(|name| LanguageSpec {
                name,
                enable: true,
                ..Default::default()
            })
            .collect::<Vec<_>>();
        let expansion = catalog.expand(&selections).unwrap();
        assert_eq!(expansion.applied.len(), 58);
        assert!(expansion
            .projections
            .iter()
            .all(|projection| projection.missing_tools.is_empty()));
    }

    #[test]
    fn catalog_expands_go_and_javascript_with_tool_projection() {
        let catalog = LanguagePackCatalog::builtin();
        let expanded = catalog
            .expand(&[
                LanguageSpec {
                    name: "go".to_string(),
                    enable: true,
                    ..Default::default()
                },
                LanguageSpec {
                    name: "javascript".to_string(),
                    enable: true,
                    ..Default::default()
                },
            ])
            .unwrap();

        assert_eq!(expanded.applied, ["Go", "JavaScript"]);
        assert!(expanded.packages.contains(&"go@nixpkgs".to_string()));
        assert!(expanded.packages.contains(&"gopls@nixpkgs".to_string()));
        assert!(expanded.packages.contains(&"nodejs@nixpkgs".to_string()));
        assert_eq!(expanded.commands.get("gofmt").map(String::as_str), Some("gofmt"));
        assert_eq!(expanded.commands.get("npx").map(String::as_str), Some("npx"));
        assert!(expanded
            .projections
            .iter()
            .all(|projection| projection.missing_tools.is_empty()));
    }

    #[test]
    fn catalog_accepts_a_valid_contribution_and_rejects_duplicate_names() {
        let mut catalog = LanguagePackCatalog::builtin();
        catalog
            .register(LanguagePack {
                name: "JetExperimental".to_string(),
                packages: vec!["jet-tool@nixpkgs".to_string()],
                venv_packages: vec!["jet-tool-venv@nixpkgs".to_string()],
                variables: BTreeMap::from([("JET_EXPERIMENTAL".to_string(), "1".to_string())]),
                commands: BTreeMap::from([("jet".to_string(), "jet".to_string())]),
                host: "native".to_string(),
                platforms: vec![jet_pkg_model::Platform::host_key()],
                license: "MIT".to_string(),
                required_tools: vec!["jet".to_string()],
                ..Default::default()
            })
            .unwrap();
        assert!(catalog.get("jet-experimental").is_some());
        assert!(catalog
            .register(LanguagePack {
                name: "jet-experimental".to_string(),
                packages: vec!["other@nixpkgs".to_string()],
                commands: BTreeMap::from([("other".to_string(), "other".to_string())]),
                host: "native".to_string(),
                platforms: vec![jet_pkg_model::Platform::host_key()],
                license: "MIT".to_string(),
                required_tools: vec!["other".to_string()],
                ..Default::default()
            })
            .is_err());
        let expansion = catalog
            .expand(&[LanguageSpec {
                name: "JetExperimental".to_string(),
                enable: true,
                ..Default::default()
            }])
            .unwrap();
        assert_eq!(expansion.packages, vec!["jet-tool@nixpkgs"]);
    }

    #[test]
    fn catalog_contribution_projects_every_declared_fact_and_fingerprints_it() {
        let mut catalog = LanguagePackCatalog::default();
        let pack = LanguagePack {
            name: "Contributed".to_string(),
            packages: vec!["compiler@nixpkgs".to_string()],
            venv_packages: vec!["compiler-tools@nixpkgs".to_string()],
            variables: BTreeMap::from([("COMPILER_MODE".to_string(), "strict".to_string())]),
            commands: BTreeMap::from([
                ("compiler".to_string(), "compiler".to_string()),
                ("compiler-fmt".to_string(), "compiler-fmt".to_string()),
            ]),
            host: "native".to_string(),
            platforms: vec![jet_pkg_model::Platform::host_key()],
            license: "MIT".to_string(),
            required_tools: vec!["compiler".to_string(), "compiler-fmt".to_string()],
        };
        let original_fingerprint = pack.fingerprint();
        catalog.register(pack).unwrap();

        let expansion = catalog
            .expand(&[LanguageSpec {
                name: "contributed".to_string(),
                enable: true,
                venv: true,
                ..Default::default()
            }])
            .unwrap();
        assert_eq!(expansion.applied, vec!["Contributed"]);
        assert_eq!(
            expansion.packages,
            vec!["compiler@nixpkgs", "compiler-tools@nixpkgs"]
        );
        assert_eq!(
            expansion.variables.get("COMPILER_MODE").map(String::as_str),
            Some("strict")
        );
        assert_eq!(
            expansion.commands.get("compiler-fmt").map(String::as_str),
            Some("compiler-fmt")
        );
        let projection = &expansion.projections[0];
        assert_eq!(projection.host, "native");
        assert_eq!(projection.platform, jet_pkg_model::Platform::host_key());
        assert_eq!(projection.license, "MIT");
        assert!(projection.missing_tools.is_empty());
        assert!(projection
            .included
            .contains(&"compiler-tools@nixpkgs".to_string()));
        assert!(expansion.fingerprint().contains(&original_fingerprint));

        let mut changed = catalog.get("Contributed").unwrap().clone();
        changed.variables.insert("COMPILER_MODE".to_string(), "fast".to_string());
        assert_ne!(changed.fingerprint(), original_fingerprint);
    }

    #[test]
    fn catalog_rejects_conflicting_contributed_variables_and_commands() {
        let mut catalog = LanguagePackCatalog::default();
        for (name, variable, command) in [
            ("First", "strict", "compiler"),
            ("Second", "fast", "other-compiler"),
        ] {
            catalog
                .register(LanguagePack {
                    name: name.to_string(),
                    packages: vec![format!("{name}@nixpkgs")],
                    variables: BTreeMap::from([(
                        "COMPILER_MODE".to_string(),
                        variable.to_string(),
                    )]),
                    commands: BTreeMap::from([(
                        "compiler".to_string(),
                        command.to_string(),
                    )]),
                    host: "native".to_string(),
                    platforms: vec![jet_pkg_model::Platform::host_key()],
                    license: "MIT".to_string(),
                    required_tools: vec!["compiler".to_string()],
                    ..Default::default()
                })
                .unwrap();
        }
        let error = catalog
            .expand_names(&["First".to_string(), "Second".to_string()])
            .unwrap_err();
        assert!(error.contains("conflicts with existing variable"), "{error}");

        let mut commands = LanguagePackCatalog::default();
        for (name, command) in [("One", "compiler"), ("Two", "other-compiler")] {
            commands
                .register(LanguagePack {
                    name: name.to_string(),
                    packages: vec![format!("{name}@nixpkgs")],
                    commands: BTreeMap::from([(
                        "compiler".to_string(),
                        command.to_string(),
                    )]),
                    host: "native".to_string(),
                    platforms: vec![jet_pkg_model::Platform::host_key()],
                    license: "MIT".to_string(),
                    required_tools: vec!["compiler".to_string()],
                    ..Default::default()
                })
                .unwrap();
        }
        let error = commands
            .expand_names(&["One".to_string(), "Two".to_string()])
            .unwrap_err();
        assert!(error.contains("conflicts with existing command"), "{error}");
    }

    #[test]
    fn language_pack_rejects_unsupported_platform_and_missing_tool_facts() {
        let mut catalog = LanguagePackCatalog::default();
        catalog
            .register(LanguagePack {
                name: "Narrow".to_string(),
                packages: vec!["narrow@nixpkgs".to_string()],
                platforms: vec!["x86_64-linux".to_string()],
                host: "native".to_string(),
                license: "MIT".to_string(),
                required_tools: vec!["narrow".to_string()],
                commands: BTreeMap::from([("narrow".to_string(), "narrow".to_string())]),
                ..Default::default()
            })
            .unwrap();
        let unsupported = catalog
            .expand_for_platform(
                &[LanguageSpec {
                    name: "Narrow".to_string(),
                    enable: true,
                    ..Default::default()
                }],
                "aarch64-darwin",
            )
            .unwrap_err();
        assert!(unsupported.contains("does not support host platform"), "{unsupported}");

        let mut missing = LanguagePackCatalog::default();
        missing.packs.insert("Missing".to_string(), LanguagePack {
                name: "Missing".to_string(),
                packages: vec!["missing@nixpkgs".to_string()],
                host: "native".to_string(),
                platforms: vec!["x86_64-linux".to_string()],
                license: "MIT".to_string(),
                required_tools: vec!["missing".to_string()],
                commands: BTreeMap::new(),
                ..Default::default()
            });
        let error = missing
            .expand_for_platform(
                &[LanguageSpec {
                    name: "Missing".to_string(),
                    enable: true,
                    ..Default::default()
                }],
                "x86_64-linux",
            )
            .unwrap_err();
        assert!(error.contains("missing catalog tools: missing"), "{error}");
        let disabled = missing
            .expand_for_platform(
                &[LanguageSpec {
                    name: "Missing".to_string(),
                    enable: false,
                    ..Default::default()
                }],
                "x86_64-linux",
            )
            .unwrap();
        assert!(disabled.packages.is_empty());
        assert_eq!(
            disabled.projections[0].missing_tools,
            vec!["missing".to_string()]
        );

        let mut unlicensed = LanguagePackCatalog::default();
        let unlicensed_error = unlicensed
            .register(LanguagePack {
                name: "Unlicensed".to_string(),
                packages: vec!["unlicensed@nixpkgs".to_string()],
                commands: BTreeMap::from([("unlicensed".to_string(), "unlicensed".to_string())]),
                host: "native".to_string(),
                platforms: vec!["x86_64-linux".to_string()],
                required_tools: vec!["unlicensed".to_string()],
                ..Default::default()
            })
            .unwrap_err();
        assert!(unlicensed_error.contains("must declare a license"), "{unlicensed_error}");

        let mut malformed = LanguagePackCatalog::default();
        let invalid_venv = malformed
            .register(LanguagePack {
                name: "InvalidVenv".to_string(),
                packages: vec!["tool@nixpkgs".to_string()],
                venv_packages: vec!["".to_string()],
                commands: BTreeMap::from([("tool".to_string(), "tool".to_string())]),
                host: "native".to_string(),
                platforms: vec!["x86_64-linux".to_string()],
                license: "MIT".to_string(),
                required_tools: vec!["tool".to_string()],
                ..Default::default()
            })
            .unwrap_err();
        assert!(invalid_venv.contains("empty venv package"), "{invalid_venv}");
        let invalid_variable = malformed
            .register(LanguagePack {
                name: "InvalidVariable".to_string(),
                packages: vec!["tool@nixpkgs".to_string()],
                variables: BTreeMap::from([("not-valid".to_string(), "1".to_string())]),
                commands: BTreeMap::from([("tool".to_string(), "tool".to_string())]),
                host: "native".to_string(),
                platforms: vec!["x86_64-linux".to_string()],
                license: "MIT".to_string(),
                required_tools: vec!["tool".to_string()],
                ..Default::default()
            })
            .unwrap_err();
        assert!(
            invalid_variable.contains("invalid environment variable name"),
            "{invalid_variable}"
        );
    }

    #[test]
    fn typed_language_map_preserves_options_and_expands_enabled_tools() {
        let value = CtValue::Map(BTreeMap::from([
            (
                CtKey::Str("rust".to_string()),
                CtValue::Struct {
                    type_name: "Lang".to_string(),
                    fields: vec![
                        ("enable".to_string(), CtValue::Bool(true)),
                        ("channel".to_string(), CtValue::Enum {
                            type_name: "Channel".to_string(),
                            variant: "Stable".to_string(),
                            args: Vec::new(),
                        }),
                        ("version".to_string(), CtValue::Str("1.78".to_string())),
                        ("extra".to_string(), CtValue::List(vec![CtValue::Str(
                            "rust-analyzer@nixpkgs".to_string(),
                        )])),
                    ],
                },
            ),
            (
                CtKey::Str("python".to_string()),
                CtValue::Struct {
                    type_name: "Lang".to_string(),
                    fields: vec![
                        ("enable".to_string(), CtValue::Bool(false)),
                        ("venv".to_string(), CtValue::Bool(true)),
                    ],
                },
            ),
        ]));
        let selections = languages_from_value(&value).unwrap();
        assert_eq!(selections.len(), 2);
        let rust = selections.iter().find(|selection| selection.name == "rust").unwrap();
        let python = selections.iter().find(|selection| selection.name == "python").unwrap();
        assert_eq!(rust.channel.as_deref(), Some("Stable"));
        assert!(python.venv);

        let expansion = LanguagePackCatalog::builtin().expand(&selections).unwrap();
        assert_eq!(expansion.applied, vec!["Rust"]);
        assert!(expansion
            .packages
            .contains(&"rustc#version=1.78@nixpkgs".to_string()));
        assert!(expansion
            .packages
            .contains(&"rust-analyzer@nixpkgs".to_string()));
        assert_eq!(expansion.selections.len(), 2);
        let rust_projection = expansion
            .projections
            .iter()
            .find(|projection| projection.selection.name == "Rust")
            .unwrap();
        assert!(rust_projection
            .included
            .contains(&"rustc#version=1.78@nixpkgs".to_string()));
        assert!(rust_projection.changed.contains(&"channel=Stable".to_string()));
        assert_eq!(rust_projection.host, "native");
        assert_eq!(rust_projection.platform, jet_pkg_model::Platform::host_key());
        assert_eq!(rust_projection.license, "Apache-2.0 OR MIT");
        assert!(rust_projection.missing_tools.is_empty());
        let python_projection = expansion
            .projections
            .iter()
            .find(|projection| projection.selection.name == "Python")
            .unwrap();
        assert!(python_projection
            .omitted
            .contains(&"python@nixpkgs".to_string()));
        assert!(python_projection.changed.contains(&"enable=false".to_string()));
    }

    #[test]
    fn language_pack_fingerprint_keeps_venv_and_environment_facts() {
        let pack = LanguagePack {
            name: "Python".to_string(),
            packages: vec!["python@nixpkgs".to_string()],
            venv_packages: vec!["pythonPackages.virtualenv@nixpkgs".to_string()],
            variables: BTreeMap::from([("PYTHONUTF8".to_string(), "1".to_string())]),
            commands: BTreeMap::from([("python".to_string(), "python3".to_string())]),
            host: "native".to_string(),
            license: "PSF-2.0".to_string(),
            required_tools: vec!["python".to_string()],
            ..Default::default()
        };
        let fingerprint = pack.fingerprint();
        assert!(fingerprint.contains("venv-package=pythonPackages.virtualenv@nixpkgs"));
        assert!(fingerprint.contains("var=PYTHONUTF8=1"));
        assert!(fingerprint.contains("command=python=python3"));
        assert!(fingerprint.contains("host=native"));
        assert!(fingerprint.contains("license="));
        assert!(fingerprint.contains("required-tool="));
    }

    #[test]
    fn typed_language_records_reject_unknown_and_malformed_fields() {
        let unknown = CtValue::Map(BTreeMap::from([(
            CtKey::Str("rust".to_string()),
            CtValue::Struct {
                type_name: "Lang".to_string(),
                fields: vec![
                    ("enable".to_string(), CtValue::Bool(true)),
                    ("surprise".to_string(), CtValue::Bool(true)),
                ],
            },
        )]));
        assert!(languages_from_value(&unknown)
            .unwrap_err()
            .contains("unknown language selection field"));

        let malformed = CtValue::Map(BTreeMap::from([(
            CtKey::Str("rust".to_string()),
            CtValue::Struct {
                type_name: "Lang".to_string(),
                fields: vec![("enable".to_string(), CtValue::Str("yes".to_string()))],
            },
        )]));
        assert!(languages_from_value(&malformed)
            .unwrap_err()
            .contains("Lang.enable must be Bool"));
    }
}
