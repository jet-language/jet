//! Typed environment facts shared by the evaluator and Jetpack runtime.
//!
//! These are deliberately small closed records. The evaluator turns Jet
//! values into these facts; Jetpack consumes them without reparsing source or
//! inventing a second policy language.

use std::collections::BTreeMap;
use std::path::Path;

use crate::AST::CtKey;
use crate::Comptime::CtValue;

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
        let mut resolved = ResolvedProfile {
            name: name.to_string(),
            ..Default::default()
        };
        let mut stack = Vec::new();
        self.resolve_into(name, &mut stack, &mut resolved)?;
        Ok(resolved)
    }

    pub fn auto_select(&self, hostname: &str, user: &str) -> Option<String> {
        self.profiles
            .values()
            .find(|profile| {
                profile
                    .hostname
                    .as_deref()
                    .is_some_and(|candidate| candidate == hostname)
            })
            .map(|profile| profile.name.clone())
            .or_else(|| {
                self.profiles
                    .values()
                    .find(|profile| {
                        profile
                            .user
                            .as_deref()
                            .is_some_and(|candidate| candidate == user)
                    })
                    .map(|profile| profile.name.clone())
            })
            .or_else(|| self.profiles.contains_key("default").then(|| "default".to_string()))
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
        resolved
            .variables
            .extend(profile.variables.iter().map(|(key, value)| (key.clone(), value.clone())));
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
            pack("Go", &["go@nixpkgs", "gopls@nixpkgs", "gotools@nixpkgs"]),
            pack(
                "JavaScript",
                &["nodejs@nixpkgs", "npm@nixpkgs", "yarn@nixpkgs"],
            ),
            pack("TypeScript", &["nodejs@nixpkgs", "typescript@nixpkgs"]),
            pack("Java", &["jdk@nixpkgs"]),
            pack("Kotlin", &["kotlin@nixpkgs"]),
            pack("C", &["gcc@nixpkgs"]),
            pack("CPlusPlus", &["gcc@nixpkgs", "cmake@nixpkgs"]),
            pack("Ruby", &["ruby@nixpkgs", "bundler@nixpkgs"]),
            pack("PHP", &["php@nixpkgs", "composer@nixpkgs"]),
            pack("Elixir", &["elixir@nixpkgs", "erlang@nixpkgs"]),
            pack("Haskell", &["ghc@nixpkgs", "cabal-install@nixpkgs"]),
            pack("Zig", &["zig@nixpkgs"]),
            pack("Swift", &["swift@nixpkgs"]),
            pack("CSharp", &["dotnet-sdk@nixpkgs"]),
            pack("FSharp", &["dotnet-sdk@nixpkgs"]),
            pack("Clojure", &["clojure@nixpkgs"]),
            pack("Crystal", &["crystal@nixpkgs"]),
            pack("Dart", &["dart@nixpkgs"]),
            pack("D", &["dmd@nixpkgs"]),
            pack("Fortran", &["gfortran@nixpkgs"]),
            pack("Erlang", &["erlang@nixpkgs"]),
            pack("Gleam", &["gleam@nixpkgs"]),
            pack("Julia", &["julia@nixpkgs"]),
            pack("Lua", &["lua@nixpkgs"]),
            pack("LuaJIT", &["luajit@nixpkgs"]),
            pack("Nim", &["nim@nixpkgs"]),
            pack("OCaml", &["ocaml@nixpkgs", "opam@nixpkgs"]),
            pack("Perl", &["perl@nixpkgs"]),
            pack("R", &["R@nixpkgs"]),
            pack("Scala", &["scala_3@nixpkgs"]),
            pack("Shell", &["bash@nixpkgs", "shellcheck@nixpkgs"]),
            pack("Assembly", &["nasm@nixpkgs"]),
            pack("CUDA", &["cudaPackages.cudatoolkit@nixpkgs"]),
            pack("ObjectiveC", &["clang@nixpkgs"]),
            pack("ObjectiveCPlusPlus", &["clang@nixpkgs"]),
            pack("Groovy", &["groovy@nixpkgs"]),
            pack("SQL", &["sqlite@nixpkgs"]),
            pack("Terraform", &["terraform@nixpkgs"]),
            pack("Nix", &["nix@nixpkgs"]),
            pack("Dhall", &["dhall@nixpkgs"]),
            pack("Jsonnet", &["jsonnet@nixpkgs"]),
            pack("Elm", &["elmPackages.elm@nixpkgs"]),
            pack("PureScript", &["purescript@nixpkgs"]),
            pack("Reason", &["ocaml@nixpkgs"]),
            pack("Racket", &["racket@nixpkgs"]),
            pack("CommonLisp", &["sbcl@nixpkgs"]),
            pack("Scheme", &["guile@nixpkgs"]),
            pack("Protobuf", &["protobuf@nixpkgs"]),
            pack("GraphQL", &["graphql-cli@nixpkgs"]),
            pack("Solidity", &["solc@nixpkgs"]),
            pack("Vyper", &["python@nixpkgs"]),
            pack("Move", &["move@nixpkgs"]),
            pack("WebAssembly", &["wasmtime@nixpkgs"]),
            pack("Markdown", &["pandoc@nixpkgs"]),
            pack("LaTeX", &["texliveSmall@nixpkgs"]),
            pack("MATLAB", &["matlab@nixpkgs"]),
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
            if !expansion
                .selections
                .iter()
                .any(|item| item.key() == normalized.key())
            {
                expansion.selections.push(normalized);
            }
            if !selection.enable {
                continue;
            }
            if !expansion.applied.iter().any(|item| item == &pack.name) {
                expansion.applied.push(pack.name.clone());
            }
            for package in &pack.packages {
                let package = versioned_package(package, selection.version.as_deref());
                if !expansion.packages.iter().any(|item| item == &package) {
                    expansion.packages.push(package);
                }
            }
            for package in &selection.extra_packages {
                if !expansion.packages.iter().any(|item| item == package) {
                    expansion.packages.push(package.clone());
                }
            }
            if selection.venv {
                for package in &pack.venv_packages {
                    let package = versioned_package(package, selection.version.as_deref());
                    if !expansion.packages.iter().any(|item| item == &package) {
                        expansion.packages.push(package);
                    }
                }
            }
            expansion
                .variables
                .extend(pack.variables.iter().map(|(key, value)| (key.clone(), value.clone())));
            expansion
                .commands
                .extend(pack.commands.iter().map(|(key, value)| (key.clone(), value.clone())));
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
    pub selections: Vec<LanguageSpec>,
    pub packages: Vec<String>,
    pub variables: BTreeMap<String, String>,
    pub commands: BTreeMap<String, String>,
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
            lifecycle.unset = list_strings_named(value, "unset")?;
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

fn valid_env_name(name: &str) -> bool {
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

fn string_map_named(value: &CtValue, scope: &str) -> Result<BTreeMap<String, String>, String> {
    match value {
        CtValue::Map(values) => values
            .iter()
            .map(|(key, value)| {
                let CtKey::Str(key) = key else {
                    return Err(format!("{scope} keys must be strings"));
                };
                let value = string_value(value)
                    .ok_or_else(|| format!("{scope} values must be strings"))?;
                Ok((key.clone(), value))
            })
            .collect(),
        CtValue::Struct { fields, .. } => fields
            .iter()
            .map(|(key, value)| {
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
