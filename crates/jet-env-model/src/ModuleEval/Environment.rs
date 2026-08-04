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
pub struct ProfileSpec {
    pub name: String,
    pub extends: Vec<String>,
    pub packages: Vec<String>,
    pub variables: BTreeMap<String, String>,
    pub hostname: Option<String>,
    pub user: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedProfile {
    pub name: String,
    /// Ambient selection order before inheritance expansion. Explicit CLI
    /// selection contains one name; hostname and user matching can contain
    /// both names in deterministic order.
    pub selected_profiles: Vec<String>,
    pub applied: Vec<String>,
    pub packages: Vec<String>,
    pub variables: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    Missing(String),
    Cycle(Vec<String>),
    Conflict { name: String },
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(name) => write!(f, "profile '{name}' does not exist"),
            Self::Cycle(names) => write!(f, "profile inheritance cycle: {}", names.join(" -> ")),
            Self::Conflict { name } => write!(f, "profile '{name}' is declared with conflicting facts"),
        }
    }
}

impl std::error::Error for ProfileError {}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfileSet {
    pub profiles: BTreeMap<String, ProfileSpec>,
}

impl ProfileSet {
    pub fn insert(&mut self, profile: ProfileSpec) -> Result<(), ProfileError> {
        self.insert_checked(profile)
    }

    pub fn insert_checked(&mut self, profile: ProfileSpec) -> Result<(), ProfileError> {
        if let Some(existing) = self.profiles.get(&profile.name) {
            if existing != &profile {
                return Err(ProfileError::Conflict {
                    name: profile.name,
                });
            }
            return Ok(());
        }
        self.profiles.insert(profile.name.clone(), profile);
        Ok(())
    }

    pub fn resolve(&self, name: &str) -> Result<ResolvedProfile, ProfileError> {
        self.resolve_many(&[name.to_string()])
    }

    pub fn resolve_many(&self, names: &[String]) -> Result<ResolvedProfile, ProfileError> {
        let mut selected_profiles = Vec::new();
        for name in names {
            if !selected_profiles.iter().any(|existing| existing == name) {
                selected_profiles.push(name.clone());
            }
        }
        let mut resolved = ResolvedProfile {
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

    pub fn auto_select(&self, hostname: &str, user: &str) -> Option<String> {
        self.auto_select_many(hostname, user).into_iter().next()
    }

    /// Select all matching ambient profiles. Hostname matches are applied
    /// before user matches; the BTreeMap keeps each group deterministic. The
    /// default profile is a fallback only when neither ambient selector matches.
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
        if selected.is_empty() && self.profiles.contains_key("default") {
            selected.push("default".to_string());
        }
        selected
    }

    fn resolve_into(
        &self,
        name: &str,
        stack: &mut Vec<String>,
        resolved: &mut ResolvedProfile,
    ) -> Result<(), ProfileError> {
        if stack.iter().any(|item| item == name) {
            let start = stack.iter().position(|item| item == name).unwrap_or(0);
            let mut cycle = stack[start..].to_vec();
            cycle.push(name.to_string());
            return Err(ProfileError::Cycle(cycle));
        }
        let profile = self
            .profiles
            .get(name)
            .ok_or_else(|| ProfileError::Missing(name.to_string()))?;
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
                    return Err(ProfileError::Conflict {
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LanguagePack {
    pub name: String,
    pub packages: Vec<String>,
    pub venv_packages: Vec<String>,
    pub variables: BTreeMap<String, String>,
    pub commands: BTreeMap<String, String>,
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
        ] {
            catalog.register(pack).expect("built-in language pack names are unique");
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

    pub fn expand(&self, selections: &[LanguageSpec]) -> Result<LanguageExpansion, String> {
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
                ..Default::default()
            };
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
            expansion
                .variables
                .extend(pack.variables.iter().map(|(key, value)| (key.clone(), value.clone())));
            expansion
                .commands
                .extend(pack.commands.iter().map(|(key, value)| (key.clone(), value.clone())));
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
    LanguagePack {
        name: name.to_string(),
        packages: packages.iter().map(|item| (*item).to_string()).collect(),
        venv_packages: (name == "Python")
            .then(|| vec!["pythonPackages.virtualenv@nixpkgs".to_string()])
            .unwrap_or_default(),
        ..Default::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HookSpec {
    pub name: String,
    pub command: String,
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
            text.push_str(&hook.command);
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

pub fn profiles_from_value(value: &CtValue) -> Result<Vec<ProfileSpec>, String> {
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

fn profile_from_value(name: String, value: CtValue) -> Result<ProfileSpec, String> {
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
    Ok(ProfileSpec {
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
                if command.trim().is_empty() {
                    return Err(format!("{prefix}[{index}] command cannot be empty"));
                }
                return Ok(HookSpec {
                    name: format!("{prefix}.{index}"),
                    command,
                    trusted: true,
                    ..Default::default()
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
            Ok(HookSpec { name, command, cwd, trusted })
        })
        .collect()
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
        let mut set = ProfileSet::default();
        set.insert(ProfileSpec {
            name: "base".to_string(),
            packages: vec!["git@nixpkgs".to_string()],
            ..Default::default()
        }).unwrap();
        set.insert(ProfileSpec {
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
    fn ambient_hostname_and_user_profiles_merge_without_overrides() {
        let mut set = ProfileSet::default();
        set.insert(ProfileSpec {
            name: "host".to_string(),
            hostname: Some("build-01".to_string()),
            packages: vec!["git@nixpkgs".to_string()],
            variables: BTreeMap::from([("HOST_MODE".to_string(), "host".to_string())]),
            ..Default::default()
        })
        .unwrap();
        set.insert(ProfileSpec {
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
        assert_eq!(resolved.selected_profiles, selected);
        assert_eq!(resolved.packages, vec!["git@nixpkgs", "ripgrep@nixpkgs"]);
        assert_eq!(resolved.variables.get("HOST_MODE"), Some(&"host".to_string()));
        assert_eq!(resolved.variables.get("USER_MODE"), Some(&"user".to_string()));
    }

    #[test]
    fn ambient_profile_variable_conflicts_are_rejected() {
        let mut set = ProfileSet::default();
        set.insert(ProfileSpec {
            name: "host".to_string(),
            hostname: Some("build-01".to_string()),
            variables: BTreeMap::from([("MODE".to_string(), "host".to_string())]),
            ..Default::default()
        })
        .unwrap();
        set.insert(ProfileSpec {
            name: "sam".to_string(),
            user: Some("sam".to_string()),
            variables: BTreeMap::from([("MODE".to_string(), "user".to_string())]),
            ..Default::default()
        })
        .unwrap();

        let selected = set.auto_select_many("build-01", "sam");
        assert!(matches!(
            set.resolve_many(&selected),
            Err(ProfileError::Conflict { .. })
        ));
    }

    #[test]
    fn profile_cycles_are_rejected() {
        let mut set = ProfileSet::default();
        set.insert(ProfileSpec {
            name: "a".to_string(),
            extends: vec!["b".to_string()],
            ..Default::default()
        }).unwrap();
        set.insert(ProfileSpec {
            name: "b".to_string(),
            extends: vec!["a".to_string()],
            ..Default::default()
        }).unwrap();
        assert!(matches!(set.resolve("a"), Err(ProfileError::Cycle(_))));
    }

    #[test]
    fn catalog_has_the_core_language_families() {
        let catalog = LanguagePackCatalog::builtin();
        assert_eq!(catalog.names().len(), 2);
        for name in ["Rust", "Python"] {
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
    fn catalog_accepts_a_valid_contribution_and_rejects_duplicate_names() {
        let mut catalog = LanguagePackCatalog::builtin();
        catalog
            .register(LanguagePack {
                name: "JetExperimental".to_string(),
                packages: vec!["jet-tool@nixpkgs".to_string()],
                ..Default::default()
            })
            .unwrap();
        assert!(catalog.get("jet-experimental").is_some());
        assert!(catalog
            .register(LanguagePack {
                name: "jetexperimental".to_string(),
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
        };
        let fingerprint = pack.fingerprint();
        assert!(fingerprint.contains("venv-package=pythonPackages.virtualenv@nixpkgs"));
        assert!(fingerprint.contains("var=PYTHONUTF8=1"));
        assert!(fingerprint.contains("command=python=python3"));
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
