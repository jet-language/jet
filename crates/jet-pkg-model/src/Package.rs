//! Closed Package and Config facts for the unified ecosystem surface.
//!
//! `Package` is the checked meaning of one source tree. `Config` is a typed
//! contribution to that Package. Both are plain data: parsing is structural,
//! composition is deterministic, and realization stays in `jetpack`.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageOutputKind {
    Library,
    Executable,
    Service,
    Check,
    Environment,
    Image,
    Bundle,
    System,
    Fleet,
}

impl PackageOutputKind {
    pub fn is_runnable(&self) -> bool {
        matches!(self, Self::Executable | Self::Service)
    }

    fn parse(value: &str) -> Result<Self, PackageParseError> {
        match value.trim().trim_start_matches('.') {
            "Library" => Ok(Self::Library),
            "Executable" => Ok(Self::Executable),
            "Service" => Ok(Self::Service),
            "Check" => Ok(Self::Check),
            "Environment" => Ok(Self::Environment),
            "Image" => Ok(Self::Image),
            "Bundle" => Ok(Self::Bundle),
            "System" => Ok(Self::System),
            "Fleet" => Ok(Self::Fleet),
            other => Err(PackageParseError::UnknownOutputKind(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputFact {
    pub kind: PackageOutputKind,
    pub name: String,
    pub entry: Option<String>,
    pub fields: BTreeMap<String, String>,
    /// The checked output payload. `fields` remains the compatibility view;
    /// Canvas and other structured consumers must use this value so arrays,
    /// objects, booleans, and numbers are not flattened into strings.
    pub payload: OutputPayload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputPayload {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<OutputPayload>),
    Object(BTreeMap<String, OutputPayload>),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServiceFact {
    pub enable: bool,
    pub ports: Vec<i64>,
    pub ready: Option<String>,
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvironmentFact {
    pub name: String,
    pub tools: Vec<String>,
    pub services: BTreeMap<String, ServiceFact>,
    pub secrets: BTreeMap<String, String>,
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberRef {
    Path(String),
    Find(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackageFacts {
    pub name: String,
    pub version: Option<String>,
    /// The optional Jet self-toolchain pin. It is a Package identity fact,
    /// not a Config contribution (D-JPK-TOOLCHAIN1, D-ECO-FILEROOT1).
    pub jet: Option<String>,
    pub source: Option<String>,
    pub deps: BTreeMap<String, String>,
    pub services: BTreeMap<String, ServiceFact>,
    pub outputs: BTreeMap<String, OutputFact>,
    pub environments: BTreeMap<String, EnvironmentFact>,
    pub defaults: BTreeMap<String, String>,
    pub configs: Vec<String>,
    /// Inline `name :: Config.{ ... }` contributions. The merged fields stay
    /// in the parent facts; this map preserves the declaration identity so a
    /// `configs: [...]` list can refer to either inline or file-backed Configs.
    pub inline_configs: BTreeMap<String, ConfigFacts>,
    pub members: Vec<MemberRef>,
    /// Every successful contributor to a field, in declaration/composition
    /// order. The fact values remain the authority; this is their audit view.
    pub provenance: BTreeMap<String, Vec<String>>,
    pub origin: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigFacts {
    pub name: Option<String>,
    pub version: Option<String>,
    pub source: Option<String>,
    pub deps: BTreeMap<String, String>,
    pub services: BTreeMap<String, ServiceFact>,
    pub outputs: BTreeMap<String, OutputFact>,
    pub environments: BTreeMap<String, EnvironmentFact>,
    pub defaults: BTreeMap<String, String>,
    pub origin: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeError {
    MembersInConfig { origin: String },
    Conflict {
        field: String,
        left_origin: String,
        right_origin: String,
        left: String,
        right: String,
    },
    UnknownDefault { intent: String, output: String },
    UnknownIntent { intent: String },
}

impl fmt::Display for ComposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MembersInConfig { origin } => {
                write!(f, "Config `{origin}` cannot declare Package members")
            }
            Self::Conflict {
                field,
                left_origin,
                right_origin,
                left,
                right,
            } => write!(
                f,
                "conflicting `{field}` from `{left_origin}` ({left}) and `{right_origin}` ({right})"
            ),
            Self::UnknownDefault { intent, output } => {
                write!(f, "default `{intent}` names unknown output `{output}`")
            }
            Self::UnknownIntent { intent } => {
                write!(f, "unknown Output intent `{intent}`")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageParseError {
    MissingName,
    MissingRecord(String),
    MalformedField(String),
    UnknownField(String),
    UnknownOutputKind(String),
    InvalidValue { field: String, value: String },
    ConfigMembers,
    Composition(String),
}

impl fmt::Display for PackageParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingName => f.write_str("Package needs a `name` field"),
            Self::MissingRecord(name) => write!(f, "`{name}` needs a record value"),
            Self::MalformedField(value) => write!(f, "malformed Package field `{value}`"),
            Self::UnknownField(name) => write!(f, "unknown Package field `{name}`"),
            Self::UnknownOutputKind(kind) => write!(f, "unknown Output kind `{kind}`"),
            Self::InvalidValue { field, value } => {
                write!(f, "invalid value for `{field}`: `{value}`")
            }
            Self::ConfigMembers => f.write_str("Config cannot declare `members`"),
            Self::Composition(value) => f.write_str(value),
        }
    }
}

impl std::error::Error for ComposeError {}
impl std::error::Error for PackageParseError {}

impl PackageFacts {
    /// Stable digest of the fully composed typed facts used by workspace
    /// locks. Source origins remain in the digest because a moved Config is a
    /// different provenance fact even when its values happen to match.
    pub fn semantic_digest(&self) -> String {
        crate::SHA256::sha256_hex(format!("{self:?}").as_bytes())
    }

    /// Parse the canonical `package.jet` root shape.
    pub fn parse(text: &str, origin: impl Into<String>) -> Result<Self, PackageParseError> {
        let facts = Self::parse_uncomposed(text, origin)?;
        facts
            .validate_defaults()
            .map_err(|error| PackageParseError::Composition(error.to_string()))?;
        Ok(facts)
    }

    /// Parse the root's own declarations without validating references that
    /// may be supplied by a later file-backed Config.  `load` is the complete
    /// path; this split keeps member identity checks structural while letting
    /// the package loader compose every declared Config before validating
    /// defaults.
    pub fn parse_uncomposed(
        text: &str,
        origin: impl Into<String>,
    ) -> Result<Self, PackageParseError> {
        let facts = parse_common(text, origin.into(), false)?;
        if facts.name.is_empty() {
            return Err(PackageParseError::MissingName);
        }
        Ok(facts)
    }

    /// Load `package.jet`, falling back only when the migration-era `pkg.jet`
    /// is the sole role file. Both names together are ambiguous and fail
    /// closed before any composition or member discovery occurs.
    pub fn load(dir: &std::path::Path) -> Option<Result<Self, PackageParseError>> {
        let canonical = dir.join("package.jet");
        let legacy = dir.join("pkg.jet");
        if canonical.is_file() && legacy.is_file() {
            return Some(Err(PackageParseError::Composition(
                "both `package.jet` and migration-era `pkg.jet` exist; keep one Package root"
                    .to_string(),
            )));
        }
        let path = if canonical.is_file() { canonical } else { legacy };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if path.is_file() => {
                return Some(Err(PackageParseError::Composition(format!(
                    "couldn't read Package `{}`: {error}",
                    path.display()
                ))))
            }
            Err(_) => return None,
        };
        let parsed = Self::parse_uncomposed(&text, path.display().to_string())
            .or_else(|error| {
                if path.file_name().and_then(|name| name.to_str()) == Some("pkg.jet") {
                    legacy_package_facts(&text, &path, error)
                } else {
                    Err(error)
                }
            });
        Some(parsed.and_then(|mut facts| {
            facts.compose_configs(dir)?;
            facts
                .validate_defaults()
                .map_err(|error| PackageParseError::Composition(error.to_string()))?;
            facts.validate_members_in(dir)?;
            Ok(facts)
        }))
    }

    /// Load and merge the root's declared Config files. Config paths are
    /// project-relative and are resolved before any facts are changed.
    pub fn compose_configs(&mut self, dir: &std::path::Path) -> Result<(), PackageParseError> {
        let configs = self.configs.clone();
        let mut parsed = Vec::with_capacity(configs.len());
        for relative in configs {
            if self.inline_configs.contains_key(&relative) {
                continue;
            }
            let path = std::path::Path::new(&relative);
            if path.is_absolute()
                || path
                    .components()
                    .any(|component| component == std::path::Component::ParentDir)
            {
                return Err(PackageParseError::InvalidValue {
                    field: "configs".to_string(),
                    value: relative,
                });
            }
            let path = dir.join(path);
            let path = if path.is_file() {
                path
            } else if path.extension().is_none() && path.with_extension("jet").is_file() {
                path.with_extension("jet")
            } else {
                discover_config_path(dir, &relative)?
            };
            let root = dir.canonicalize().map_err(|error| {
                PackageParseError::Composition(format!(
                    "couldn't resolve Package root `{}`: {error}",
                    dir.display()
                ))
            })?;
            let path = path.canonicalize().map_err(|error| {
                PackageParseError::Composition(format!(
                    "couldn't resolve Config `{}`: {error}",
                    path.display()
                ))
            })?;
            if !path.starts_with(&root) {
                return Err(PackageParseError::InvalidValue {
                    field: "configs".to_string(),
                    value: path.display().to_string(),
                });
            }
            let text = std::fs::read_to_string(&path).map_err(|error| {
                PackageParseError::Composition(format!(
                    "couldn't read Config `{}`: {error}",
                    path.display()
                ))
            })?;
            parsed.push(ConfigFacts::parse(&text, path.display().to_string())?);
        }
        self.compose(parsed)
            .map_err(|error| PackageParseError::Composition(error.to_string()))?;
        Ok(())
    }

    /// Compose one root Package with Config contributions. Equal facts
    /// coalesce; unequal scalar facts retain both source origins in the error.
    pub fn compose<I>(&mut self, configs: I) -> Result<(), ComposeError>
    where
        I: IntoIterator<Item = ConfigFacts>,
    {
        let mut candidate = self.clone();
        for config in configs {
            merge_config(&mut candidate, &config)?;
        }
        candidate.validate_defaults()?;
        *self = candidate;
        Ok(())
    }

    /// Return the ordered sources that contributed to one typed field.
    pub fn field_provenance(&self, field: &str) -> &[String] {
        self.provenance
            .get(field)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn validate_defaults(&self) -> Result<(), ComposeError> {
        for (intent, output) in &self.defaults {
            validate_intent(intent)?;
            let Some(fact) = self.outputs.get(output) else {
                return Err(ComposeError::UnknownDefault {
                    intent: intent.clone(),
                    output: output.clone(),
                });
            };
            if !intent_accepts_kind(intent, fact.kind) {
                return Err(ComposeError::Conflict {
                    field: format!("defaults.{intent}"),
                    left_origin: self.origin.clone(),
                    right_origin: self.origin.clone(),
                    left: output.clone(),
                    right: format!(
                        "default must select a {} output",
                        intent_kind_description(intent)
                    ),
                });
            }
        }
        Ok(())
    }

    /// Select one compatible output. Plural intents remain the caller's job;
    /// this method implements the singular explicit/legacy/sole/default law.
    pub fn select_output(
        &self,
        intent: &str,
        explicit: Option<&str>,
        legacy: Option<&str>,
    ) -> Result<&OutputFact, ComposeError> {
        self.validate_defaults()?;
        validate_intent(intent)?;
        let selected = explicit
            .map(str::to_string)
            .or_else(|| legacy.map(str::to_string));
        let selected = if let Some(selected) = selected {
            selected
        } else {
            let mut compatible = self
                .outputs
                .iter()
                .filter(|(_, output)| intent_accepts_kind(intent, output.kind))
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            compatible.sort();
            if compatible.len() == 1 {
                compatible.pop().unwrap()
            } else if let Some(default) = self.defaults.get(intent) {
                default.clone()
            } else {
                let candidates = if compatible.is_empty() {
                    "none".to_string()
                } else {
                    compatible.join(", ")
                };
                return Err(ComposeError::Conflict {
                    field: format!("outputs.{intent}"),
                    left_origin: self.origin.clone(),
                    right_origin: self.origin.clone(),
                    left: candidates,
                    right: "no unambiguous compatible output was selected".to_string(),
                });
            }
        };
        let Some(output) = self.outputs.get(&selected) else {
            return Err(ComposeError::UnknownDefault {
                intent: intent.to_string(),
                output: selected,
            });
        };
        if !intent_accepts_kind(intent, output.kind) {
            return Err(ComposeError::Conflict {
                field: format!("outputs.{intent}"),
                left_origin: self.origin.clone(),
                right_origin: self.origin.clone(),
                left: output.name.clone(),
                right: format!(
                    "selected output is incompatible with the {} intent",
                    intent_kind_description(intent)
                ),
            });
        }
        Ok(output)
    }

    /// Resolve the selected runnable output without allowing the migration
    /// filename convention to hide a broken typed declaration.
    pub fn resolve_run_entry(
        &self,
        root: &std::path::Path,
    ) -> Result<Option<std::path::PathBuf>, String> {
        self.validate_defaults()
            .map_err(|error| format!("{}: {error}", self.origin))?;
        if self
            .outputs
            .values()
            .any(|output| intent_accepts_kind("run", output.kind))
        {
            let output = self
                .select_output("run", None, None)
                .map_err(|error| format!("{}: {error}", self.origin))?;
            let Some(entry) = self.entry_path(root, output) else {
                return Err(format!(
                    "{}: typed output `{}` has no unique source entry for `{}`",
                    self.origin,
                    output.name,
                    output.entry.as_deref().unwrap_or("<missing>")
                ));
            };
            return Ok(Some(entry));
        }
        Ok(self.legacy_run_entry(root))
    }

    fn legacy_run_entry(&self, root: &std::path::Path) -> Option<std::path::PathBuf> {
        let matches = self
            .source_files(root)
            .into_iter()
            .filter(|path| path.parent() == Some(root))
            .filter(|path| file_has_top_level_function(path, "run"))
            .collect::<Vec<_>>();
        (matches.len() == 1).then(|| matches.into_iter().next().unwrap())
    }

    /// Resolve a runnable Output's checked-reference spelling to a source
    /// file for legacy command entry points. The compiler still performs the
    /// callable/type check when it loads that file; this helper only chooses a
    /// deterministic file from the Package tree and never treats an arbitrary
    /// path as an Output reference.
    pub fn entry_path(
        &self,
        root: &std::path::Path,
        output: &OutputFact,
    ) -> Option<std::path::PathBuf> {
        let entry = output.entry.as_deref()?;
        let parts = entry.split('.').collect::<Vec<_>>();
        if (parts.len() != 1 && parts.len() != 2)
            || parts.iter().any(|part| !is_identifier(part))
        {
            return None;
        }
        let files = self.source_files(root);
        if parts.len() == 1 {
            let matches = files
                .into_iter()
                .filter(|path| path.parent() == Some(root))
                .filter(|path| file_has_top_level_function(path, parts[0]))
                .collect::<Vec<_>>();
            return (matches.len() == 1).then(|| matches.into_iter().next().unwrap());
        }
        let sources = parse_sources(&files)?;
        let mut targets = sources
            .iter()
            .filter(|source| source.path.parent() == Some(root))
            .flat_map(|source| imported_module_targets(root, source, parts[0], &sources))
            .collect::<Vec<_>>();
        targets.sort();
        targets.dedup();
        let matches = if targets.len() == 1 {
            sources
                .iter()
                .filter(|source| source.path == targets[0])
                .filter(|source| {
                    unique_top_level_function(&source.program, parts[1])
                        .is_some_and(|function| function.is_pub)
                })
                .map(|source| source.path.clone())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        (matches.len() == 1).then(|| matches.into_iter().next().unwrap())
    }

    fn source_files(&self, root: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut files = Vec::new();
        collect_jet_files(root, &mut files);
        files.retain(|path| {
            let reserved = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "package.jet" || name == "pkg.jet");
            let config = self.configs.iter().any(|name| {
                let candidate = root.join(name);
                path == &candidate
                    || (candidate.extension().is_none()
                        && path == &candidate.with_extension("jet"))
            });
            !reserved && !config
        });
        files.sort();
        files
    }

    pub fn validate_members(&self) -> Result<(), ComposeError> {
        for member in &self.members {
            let path = match member {
                MemberRef::Path(path) | MemberRef::Find(path) => path,
            };
            let candidate = std::path::Path::new(path);
            if path.is_empty()
                || candidate.is_absolute()
                || candidate.components().any(|c| c == std::path::Component::ParentDir)
            {
                return Err(ComposeError::Conflict {
                    field: "members".to_string(),
                    left_origin: self.origin.clone(),
                    right_origin: self.origin.clone(),
                    left: path.clone(),
                    right: "member path escapes its Package root".to_string(),
                });
            }
        }
        Ok(())
    }

    /// Validate member paths against the physical Package root. The structural
    /// validator above catches lexical escapes; this variant also rejects an
    /// escaping symlink before a workspace can realize the member.
    pub fn validate_members_in(&self, dir: &std::path::Path) -> Result<(), PackageParseError> {
        self.validate_members()
            .map_err(|error| PackageParseError::Composition(error.to_string()))?;
        let root = dir.canonicalize().map_err(|error| {
            PackageParseError::Composition(format!(
                "couldn't resolve Package root `{}`: {error}",
                dir.display()
            ))
        })?;
        let mut physical = Vec::new();
        let mut names = Vec::new();
        for member in &self.members {
            let relative = match member {
                MemberRef::Path(relative) | MemberRef::Find(relative) => relative,
            };
            let path = dir.join(relative).canonicalize().map_err(|error| {
                PackageParseError::Composition(format!(
                    "couldn't resolve member reference `{relative}`: {error}"
                ))
            })?;
            if !path.starts_with(&root) {
                return Err(PackageParseError::Composition(format!(
                    "member reference `{relative}` resolves outside Package root `{}`",
                    dir.display()
                )));
            }
            if path == root {
                return Err(PackageParseError::Composition(format!(
                    "member reference `{relative}` resolves to its Package root"
                )));
            }
            let candidates = if matches!(member, MemberRef::Find(_)) {
                let entries = std::fs::read_dir(&path).map_err(|error| {
                    PackageParseError::Composition(format!(
                        "couldn't scan member discovery directory `{relative}`: {error}"
                    ))
                })?;
                let mut children = Vec::new();
                for entry in entries {
                    let entry = entry.map_err(|error| {
                        PackageParseError::Composition(format!(
                            "couldn't read member discovery directory `{relative}`: {error}"
                        ))
                    })?;
                    let child = entry.path();
                    let file_type = entry.file_type().map_err(|error| {
                        PackageParseError::Composition(format!(
                            "couldn't inspect discovered member `{}`: {error}",
                            child.display()
                        ))
                    })?;
                    if !(file_type.is_dir() || (file_type.is_symlink() && child.is_dir())) {
                        continue;
                    }
                    if package_manifest_path(&child).is_some() {
                        children.push(child);
                    }
                }
                children.sort();
                children
            } else {
                vec![path]
            };
            for candidate in candidates {
                let candidate = candidate.canonicalize().map_err(|error| {
                    PackageParseError::Composition(format!(
                        "couldn't resolve member Package `{}`: {error}",
                        candidate.display()
                    ))
                })?;
                if !candidate.starts_with(&root) || candidate == root {
                    return Err(PackageParseError::Composition(format!(
                        "member reference `{relative}` resolves outside its Package root"
                    )));
                }
                if candidate.join("package.jet").is_file() && candidate.join("pkg.jet").is_file() {
                    return Err(PackageParseError::Composition(format!(
                        "member Package `{relative}` contains both `package.jet` and migration-era `pkg.jet`"
                    )));
                }
                if physical.iter().any(|existing| existing == &candidate) {
                    return Err(PackageParseError::Composition(format!(
                        "member reference `{relative}` has the same physical identity as another member"
                    )));
                }
                let (name, nested) = package_member_identity(&candidate)?;
                if nested {
                    let manifest = package_manifest_path(&candidate)
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| candidate.display().to_string());
                    return Err(PackageParseError::Composition(format!(
                        "member Package `{relative}` at `{manifest}` declares members"
                    )));
                }
                if names.iter().any(|existing| existing == &name) {
                    return Err(PackageParseError::Composition(format!(
                        "member Package name `{name}` is declared more than once"
                    )));
                }
                physical.push(candidate);
                names.push(name);
            }
        }
        Ok(())
    }
}

fn intent_accepts_kind(intent: &str, kind: PackageOutputKind) -> bool {
    match intent {
        "run" => matches!(kind, PackageOutputKind::Executable | PackageOutputKind::Service),
        "test" | "check" => kind == PackageOutputKind::Check,
        "dev" | "enter" => kind == PackageOutputKind::Environment,
        "publish" => matches!(
            kind,
            PackageOutputKind::Library
                | PackageOutputKind::Executable
                | PackageOutputKind::Service
        ),
        "deploy" | "fleet" => kind == PackageOutputKind::Fleet,
        "activate" | "activation" => {
            matches!(kind, PackageOutputKind::System | PackageOutputKind::Fleet)
        }
        _ => false,
    }
}

fn intent_kind_description(intent: &str) -> &'static str {
    match intent {
        "run" => "Executable or Service",
        "test" | "check" => "Check",
        "dev" | "enter" => "Environment",
        "publish" => "Library, Executable, or Service",
        "deploy" | "fleet" => "Fleet",
        "activate" | "activation" => "System or Fleet",
        _ => "a compatible",
    }
}

fn validate_intent(intent: &str) -> Result<(), ComposeError> {
    if matches!(
        intent,
        "run"
            | "test"
            | "check"
            | "dev"
            | "enter"
            | "publish"
            | "deploy"
            | "fleet"
            | "activate"
            | "activation"
    ) {
        Ok(())
    } else {
        Err(ComposeError::UnknownIntent {
            intent: intent.to_string(),
        })
    }
}

impl ConfigFacts {
    pub fn parse(text: &str, origin: impl Into<String>) -> Result<Self, PackageParseError> {
        let origin = origin.into();
        let stripped = strip_comments(text);
        let (declared_name, body) = match config_wrapper(&stripped)? {
            Some((name, body)) => (Some(name), body),
            None if stripped.trim_start().starts_with("Config") => {
                (None, record_body(stripped.trim(), "Config")?)
            }
            None => (None, stripped.as_str()),
        };
        let facts = parse_common(body, origin, true)?;
        Ok(ConfigFacts {
            name: declared_name.or_else(|| (!facts.name.is_empty()).then_some(facts.name)),
            version: facts.version,
            source: facts.source,
            deps: facts.deps,
            services: facts.services,
            outputs: facts.outputs,
            environments: facts.environments,
            defaults: facts.defaults,
            origin: facts.origin,
        })
    }
}

fn package_manifest_path(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let canonical = dir.join("package.jet");
    if canonical.is_file() {
        Some(canonical)
    } else {
        let legacy = dir.join("pkg.jet");
        legacy.is_file().then_some(legacy)
    }
}

fn legacy_package_facts(
    text: &str,
    path: &std::path::Path,
    canonical_error: PackageParseError,
) -> Result<PackageFacts, PackageParseError> {
    let manifest = crate::PackageManifest::parse(text).map_err(|error| {
        PackageParseError::Composition(format!(
            "migration-era Package `{}` is not a valid Package ({canonical_error}): {error:?}",
            path.display()
        ))
    })?;
    let mut facts = PackageFacts {
        name: manifest.package.name,
        version: Some(manifest.package.version),
        jet: manifest.package.jet_constraint,
        origin: path.display().to_string(),
        ..PackageFacts::default()
    };
    for dependency in manifest.deps {
        facts
            .deps
            .insert(dependency.name, legacy_dependency_value(&dependency.source));
    }
    let origin = facts.origin.clone();
    record_provenance(&mut facts.provenance, "name", &origin);
    record_provenance(&mut facts.provenance, "version", &origin);
    if facts.jet.is_some() {
        record_provenance(&mut facts.provenance, "jet", &origin);
    }
    for name in facts.deps.keys() {
        record_provenance(&mut facts.provenance, &format!("deps.{name}"), &origin);
    }
    Ok(facts)
}

fn legacy_dependency_value(source: &crate::PackageManifest::DepSource) -> String {
    match source {
        crate::PackageManifest::DepSource::Version(value) => value.clone(),
        crate::PackageManifest::DepSource::Provider { provider, target } => {
            if matches!(provider, crate::RefSpec::Source::Path) {
                target.clone()
            } else {
                format!("{target}@{}", provider.label())
            }
        }
        crate::PackageManifest::DepSource::Git { url, selector } => {
            let (field, value) = match selector {
                crate::Manifest::GitSelector::Tag(value) => ("tag", value),
                crate::Manifest::GitSelector::Branch(value) => ("branch", value),
                crate::Manifest::GitSelector::Rev(value) => ("rev", value),
            };
            format!("{{ git: {url:?}, {field}: {value:?} }}")
        }
        crate::PackageManifest::DepSource::CLib { target } => format!("lib: {target}"),
    }
}

fn legacy_package_name(text: &str, dir: &std::path::Path) -> String {
    text.lines()
        .find_map(|line| {
            let rest = line.trim().strip_prefix("name:")?;
            let value = rest.trim().trim_end_matches(',').trim().trim_matches('"');
            (!value.is_empty() && !value.contains('{')).then(|| value.to_string())
        })
        .or_else(|| dir.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

fn package_member_identity(
    dir: &std::path::Path,
) -> Result<(String, bool), PackageParseError> {
    let Some(manifest) = package_manifest_path(dir) else {
        return Err(PackageParseError::Composition(format!(
            "member directory `{}` is not a Package directory",
            dir.display()
        )));
    };
    let text = std::fs::read_to_string(&manifest).map_err(|error| {
        PackageParseError::Composition(format!(
            "couldn't read member Package `{}`: {error}",
            manifest.display()
        ))
    })?;
    if manifest.file_name().and_then(|name| name.to_str()) == Some("package.jet") {
        let facts = PackageFacts::parse_uncomposed(&text, manifest.display().to_string())?;
        Ok((facts.name, !facts.members.is_empty()))
    } else {
        Ok((
            legacy_package_name(&text, dir),
            text.lines()
                .any(|line| line.trim_start().starts_with("members:")),
        ))
    }
}

fn parse_common(
    text: &str,
    origin: String,
    config: bool,
) -> Result<PackageFacts, PackageParseError> {
    let mut facts = PackageFacts {
        origin,
        ..PackageFacts::default()
    };
    let mut inline = Vec::new();
    let mut seen = BTreeMap::<String, String>::new();
    for entry in top_level_entries(&strip_comments(text)) {
        if !config {
            if let Some((name, _)) = config_wrapper(&entry)? {
                let contribution_origin = format!("{}::{name}", facts.origin);
                let contribution = ConfigFacts::parse(&entry, contribution_origin)?;
                inline.push((name, contribution));
                continue;
            }
        }
        let Some((field, value)) = split_field(&entry) else {
            return Err(PackageParseError::MalformedField(entry));
        };
        if let Some(previous) = seen.get(&field) {
            if previous.trim() != value.trim() {
                return Err(PackageParseError::Composition(format!(
                    "`{field}` is declared with conflicting values"
                )));
            }
            continue;
        };
        seen.insert(field.clone(), value.clone());
        if let Some(output_value) = value.strip_prefix("Output ::") {
            let output = parse_output_value(&field, output_value.trim())?;
            if facts.outputs.insert(field.clone(), output).is_some() {
                return Err(PackageParseError::Composition(format!(
                    "output `{field}` is declared more than once"
                )));
            }
            let origin = facts.origin.clone();
            let output = facts.outputs.get(&field).cloned().expect("inserted output");
            record_output_provenance(
                &mut facts.provenance,
                &format!("outputs.{field}"),
                &origin,
                &output,
            );
            continue;
        }
        match field.as_str() {
            "name" => facts.name = scalar(&value),
            "version" => facts.version = Some(scalar(&value)),
            "jet" if !config => facts.jet = Some(scalar(&value)),
            "jet" => return Err(PackageParseError::UnknownField(field.clone())),
            "source" => facts.source = Some(scalar(&value)),
            "deps" => facts.deps = parse_string_map("deps", &value)?,
            "services" => facts.services = parse_services(&value)?,
            "outputs" => {
                for (name, output) in parse_outputs(&value)? {
                    match facts.outputs.get(&name) {
                        None => {
                            facts.outputs.insert(name, output);
                        }
                        Some(existing) if existing == &output => {}
                        Some(existing) => {
                            return Err(PackageParseError::Composition(format!(
                                "output `{name}` is declared with conflicting values: {existing:?} and {output:?}"
                            )));
                        }
                    }
                }
            }
            "environments" => facts.environments = parse_environments(&value)?,
            "defaults" => facts.defaults = parse_string_map("defaults", &value)?,
            "members" if config => return Err(PackageParseError::ConfigMembers),
            "members" => facts.members = parse_members(&value)?,
            "configs" if config => {
                return Err(PackageParseError::UnknownField(field.clone()))
            }
            "configs" => facts.configs = parse_list(&value),
            "description" | "license" | "edition" | "repository" => {}
            other => return Err(PackageParseError::UnknownField(other.to_string())),
        }
        let origin = facts.origin.clone();
        record_declared_provenance(&mut facts, &field, &origin);
    }
    for (name, contribution) in inline {
        merge_config(&mut facts, &contribution)
            .map_err(|error| PackageParseError::Composition(error.to_string()))?;
        facts.inline_configs.insert(name, contribution);
    }
    Ok(facts)
}

fn merge_config(root: &mut PackageFacts, config: &ConfigFacts) -> Result<(), ComposeError> {
    let fallback = root.origin.clone();
    merge_optional_field(
        &mut root.version,
        config.version.as_ref(),
        "version",
        &fallback,
        &config.origin,
        &mut root.provenance,
    )?;
    merge_optional_field(
        &mut root.source,
        config.source.as_ref(),
        "source",
        &fallback,
        &config.origin,
        &mut root.provenance,
    )?;
    merge_string_map(
        &mut root.deps,
        &config.deps,
        "deps",
        &fallback,
        &config.origin,
        &mut root.provenance,
    )?;
    merge_services(
        &mut root.services,
        &config.services,
        "services",
        &fallback,
        &config.origin,
        &mut root.provenance,
    )?;
    merge_outputs(
        &mut root.outputs,
        &config.outputs,
        "outputs",
        &fallback,
        &config.origin,
        &mut root.provenance,
    )?;
    merge_environments(
        &mut root.environments,
        &config.environments,
        "environments",
        &fallback,
        &config.origin,
        &mut root.provenance,
    )?;
    merge_string_map(
        &mut root.defaults,
        &config.defaults,
        "defaults",
        &fallback,
        &config.origin,
        &mut root.provenance,
    )
}

fn record_declared_provenance(facts: &mut PackageFacts, field: &str, origin: &str) {
    record_provenance(&mut facts.provenance, field, origin);
    match field {
        "services" => {
            for (key, service) in &facts.services {
                record_service_provenance(
                    &mut facts.provenance,
                    &format!("services.{key}"),
                    origin,
                    service,
                );
            }
        }
        "outputs" => {
            for (key, output) in &facts.outputs {
                record_output_provenance(
                    &mut facts.provenance,
                    &format!("outputs.{key}"),
                    origin,
                    output,
                );
            }
        }
        "environments" => {
            for (key, environment) in &facts.environments {
                record_environment_provenance(
                    &mut facts.provenance,
                    &format!("environments.{key}"),
                    origin,
                    environment,
                );
            }
        }
        _ => {
            let keys = match field {
                "deps" => facts.deps.keys().cloned().collect::<Vec<_>>(),
                "defaults" => facts.defaults.keys().cloned().collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            for key in keys {
                record_provenance(&mut facts.provenance, &format!("{field}.{key}"), origin);
            }
        }
    }
}

fn merge_optional_field(
    current: &mut Option<String>,
    incoming: Option<&String>,
    field: &str,
    fallback: &str,
    origin: &str,
    provenance: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), ComposeError> {
    let Some(incoming) = incoming else { return Ok(()) };
    let left_origin = provenance_origin(provenance, field, fallback);
    match current {
        None => *current = Some(incoming.clone()),
        Some(existing) if existing == incoming => {}
        Some(existing) => {
            return Err(ComposeError::Conflict {
                field: field.to_string(),
                left_origin,
                right_origin: origin.to_string(),
                left: existing.clone(),
                right: incoming.clone(),
            })
        }
    }
    record_provenance(provenance, field, origin);
    Ok(())
}

fn merge_string_map(
    current: &mut BTreeMap<String, String>,
    incoming: &BTreeMap<String, String>,
    field: &str,
    fallback: &str,
    origin: &str,
    provenance: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), ComposeError> {
    for (key, value) in incoming {
        let path = format!("{field}.{key}");
        let left_origin = provenance_origin(provenance, &path, fallback);
        match current.get(key) {
            None => {
                current.insert(key.clone(), value.clone());
            }
            Some(existing) if existing == value => {}
            Some(existing) => {
                return Err(ComposeError::Conflict {
                    field: path,
                    left_origin,
                    right_origin: origin.to_string(),
                    left: existing.clone(),
                    right: value.clone(),
                })
            }
        }
        record_provenance(provenance, &format!("{field}.{key}"), origin);
    }
    Ok(())
}

fn merge_services(
    current: &mut BTreeMap<String, ServiceFact>,
    incoming: &BTreeMap<String, ServiceFact>,
    field: &str,
    fallback: &str,
    origin: &str,
    provenance: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), ComposeError> {
    for (key, value) in incoming {
        let path = format!("{field}.{key}");
        if let Some(existing) = current.get(key) {
            let mut merged = existing.clone();
            merge_service_fact(&mut merged, value, &path, fallback, origin, provenance)?;
            current.insert(key.clone(), merged);
        } else {
            current.insert(key.clone(), value.clone());
            record_service_provenance(provenance, &path, origin, value);
        }
    }
    Ok(())
}

fn merge_service_fact(
    current: &mut ServiceFact,
    incoming: &ServiceFact,
    path: &str,
    fallback: &str,
    origin: &str,
    provenance: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), ComposeError> {
    merge_string_fields(
        &mut current.fields,
        &incoming.fields,
        path,
        fallback,
        origin,
        provenance,
    )?;
    if incoming.fields.contains_key("enable") {
        current.enable = incoming.enable;
    }
    if incoming.fields.contains_key("ports") {
        current.ports = incoming.ports.clone();
    }
    if incoming.fields.contains_key("ready") {
        current.ready = incoming.ready.clone();
    }
    record_provenance(provenance, path, origin);
    Ok(())
}

fn merge_outputs(
    current: &mut BTreeMap<String, OutputFact>,
    incoming: &BTreeMap<String, OutputFact>,
    field: &str,
    fallback: &str,
    origin: &str,
    provenance: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), ComposeError> {
    for (key, value) in incoming {
        let path = format!("{field}.{key}");
        let Some(existing) = current.get(key) else {
            current.insert(key.clone(), value.clone());
            record_output_provenance(provenance, &path, origin, value);
            continue;
        };
        if existing.kind != value.kind {
            return Err(ComposeError::Conflict {
                field: format!("{path}.kind"),
                left_origin: provenance_origin(
                    provenance,
                    &format!("{path}.kind"),
                    fallback,
                ),
                right_origin: origin.to_string(),
                left: format!("{:?}", existing.kind),
                right: format!("{:?}", value.kind),
            });
        }
        let mut merged = existing.clone();
        merge_output_fields(
            &mut merged.fields,
            &value.fields,
            &path,
            fallback,
            origin,
            provenance,
        )?;
        merge_output_payload(
            &mut merged.payload,
            &value.payload,
            &path,
            fallback,
            origin,
            provenance,
        )?;
        merged.name = merged
            .fields
            .get("name")
            .map(|value| scalar(value))
            .unwrap_or_else(|| key.clone());
        merged.entry = merged.fields.get("entry").map(|value| scalar(value));
        current.insert(key.clone(), merged);
        record_provenance(provenance, &path, origin);
    }
    Ok(())
}

fn merge_output_fields(
    current: &mut BTreeMap<String, String>,
    incoming: &BTreeMap<String, String>,
    path: &str,
    fallback: &str,
    origin: &str,
    provenance: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), ComposeError> {
    for (key, value) in incoming {
        let field = format!("{path}.{key}");
        let left_origin = provenance_origin(provenance, &field, fallback);
        match current.get(key) {
            None => {
                current.insert(key.clone(), value.clone());
            }
            Some(existing) if existing == value => {}
            Some(_) if output_structured_field(key) => {}
            Some(existing) => {
                return Err(ComposeError::Conflict {
                    field,
                    left_origin,
                    right_origin: origin.to_string(),
                    left: existing.clone(),
                    right: value.clone(),
                })
            }
        }
        record_provenance(provenance, &format!("{path}.{key}"), origin);
    }
    Ok(())
}

fn output_structured_field(field: &str) -> bool {
    matches!(
        field,
        "modules"
            | "tools"
            | "services"
            | "secrets"
            | "from"
            | "kind"
            | "environment"
            | "members"
            | "packages"
            | "options"
            | "hosts"
    )
}

fn merge_output_payload(
    current: &mut OutputPayload,
    incoming: &OutputPayload,
    path: &str,
    fallback: &str,
    origin: &str,
    provenance: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), ComposeError> {
    match (current, incoming) {
        (OutputPayload::Object(current), OutputPayload::Object(incoming)) => {
            for (key, value) in incoming {
                let field = format!("{path}.{key}");
                match current.get_mut(key) {
                    None => {
                        current.insert(key.clone(), value.clone());
                    }
                    Some(existing) => {
                        merge_output_payload(existing, value, &field, fallback, origin, provenance)?;
                    }
                }
                record_provenance(provenance, &field, origin);
            }
            Ok(())
        }
        (OutputPayload::Array(current), OutputPayload::Array(incoming)) => {
            for value in incoming {
                if !current.iter().any(|existing| existing == value) {
                    current.push(value.clone());
                }
            }
            record_provenance(provenance, path, origin);
            Ok(())
        }
        (current, incoming) if current == incoming => {
            record_provenance(provenance, path, origin);
            Ok(())
        }
        (current, incoming) => Err(ComposeError::Conflict {
            field: path.to_string(),
            left_origin: provenance_origin(provenance, path, fallback),
            right_origin: origin.to_string(),
            left: format!("{current:?}"),
            right: format!("{incoming:?}"),
        }),
    }
}

fn merge_environments(
    current: &mut BTreeMap<String, EnvironmentFact>,
    incoming: &BTreeMap<String, EnvironmentFact>,
    field: &str,
    fallback: &str,
    origin: &str,
    provenance: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), ComposeError> {
    for (key, value) in incoming {
        let path = format!("{field}.{key}");
        let Some(existing) = current.get(key) else {
            current.insert(key.clone(), value.clone());
            record_environment_provenance(provenance, &path, origin, value);
            continue;
        };
        let mut merged = existing.clone();
        let scalar_fields = value
            .fields
            .iter()
            .filter(|(name, _)| !matches!(name.as_str(), "name" | "tools" | "services" | "secrets"))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        merge_string_fields(
            &mut merged.fields,
            &scalar_fields,
            &path,
            fallback,
            origin,
            provenance,
        )?;
        if let Some(name) = value.fields.get("name") {
            let mut names = BTreeMap::new();
            names.insert("name".to_string(), name.clone());
            merge_string_fields(&mut merged.fields, &names, &path, fallback, origin, provenance)?;
        }
        merge_string_list_field(
            &mut merged.tools,
            &value.tools,
            &format!("{path}.tools"),
            origin,
            provenance,
        );
        merge_string_map(
            &mut merged.secrets,
            &value.secrets,
            &format!("{path}.secrets"),
            fallback,
            origin,
            provenance,
        )?;
        merge_services(
            &mut merged.services,
            &value.services,
            &format!("{path}.services"),
            fallback,
            origin,
            provenance,
        )?;
        merged.name = merged
            .fields
            .get("name")
            .map(|value| scalar(value))
            .unwrap_or_else(|| key.clone());
        current.insert(key.clone(), merged);
        record_provenance(provenance, &path, origin);
    }
    Ok(())
}

fn merge_string_fields(
    current: &mut BTreeMap<String, String>,
    incoming: &BTreeMap<String, String>,
    path: &str,
    fallback: &str,
    origin: &str,
    provenance: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), ComposeError> {
    for (key, value) in incoming {
        let field = format!("{path}.{key}");
        let left_origin = provenance_origin(provenance, &field, fallback);
        match current.get(key) {
            None => {
                current.insert(key.clone(), value.clone());
            }
            Some(existing) if existing == value => {}
            Some(existing) => {
                return Err(ComposeError::Conflict {
                    field,
                    left_origin,
                    right_origin: origin.to_string(),
                    left: existing.clone(),
                    right: value.clone(),
                })
            }
        }
        record_provenance(provenance, &format!("{path}.{key}"), origin);
    }
    Ok(())
}

fn merge_string_list_field(
    current: &mut Vec<String>,
    incoming: &[String],
    field: &str,
    origin: &str,
    provenance: &mut BTreeMap<String, Vec<String>>,
) {
    for value in incoming {
        if !current.iter().any(|existing| existing == value) {
            current.push(value.clone());
        }
    }
    current.sort();
    record_provenance(provenance, field, origin);
}

fn record_service_provenance(
    provenance: &mut BTreeMap<String, Vec<String>>,
    path: &str,
    origin: &str,
    service: &ServiceFact,
) {
    record_provenance(provenance, path, origin);
    for field in service.fields.keys() {
        record_provenance(provenance, &format!("{path}.{field}"), origin);
    }
}

fn record_output_provenance(
    provenance: &mut BTreeMap<String, Vec<String>>,
    path: &str,
    origin: &str,
    output: &OutputFact,
) {
    record_provenance(provenance, path, origin);
    record_provenance(provenance, &format!("{path}.kind"), origin);
    record_provenance(provenance, &format!("{path}.name"), origin);
    for field in output.fields.keys() {
        record_provenance(provenance, &format!("{path}.{field}"), origin);
    }
    record_payload_provenance(provenance, path, origin, &output.payload);
}

fn record_payload_provenance(
    provenance: &mut BTreeMap<String, Vec<String>>,
    path: &str,
    origin: &str,
    payload: &OutputPayload,
) {
    if let OutputPayload::Object(fields) = payload {
        for (field, value) in fields {
            let path = format!("{path}.{field}");
            record_provenance(provenance, &path, origin);
            record_payload_provenance(provenance, &path, origin, value);
        }
    }
}

fn record_environment_provenance(
    provenance: &mut BTreeMap<String, Vec<String>>,
    path: &str,
    origin: &str,
    environment: &EnvironmentFact,
) {
    record_provenance(provenance, path, origin);
    for field in environment.fields.keys() {
        record_provenance(provenance, &format!("{path}.{field}"), origin);
    }
    for (name, service) in &environment.services {
        record_service_provenance(provenance, &format!("{path}.services.{name}"), origin, service);
    }
    for name in environment.secrets.keys() {
        record_provenance(provenance, &format!("{path}.secrets.{name}"), origin);
    }
}

fn record_provenance(provenance: &mut BTreeMap<String, Vec<String>>, field: &str, origin: &str) {
    let origins = provenance.entry(field.to_string()).or_default();
    if !origins.iter().any(|existing| existing == origin) {
        origins.push(origin.to_string());
    }
}

fn provenance_origin(
    provenance: &BTreeMap<String, Vec<String>>,
    field: &str,
    fallback: &str,
) -> String {
    provenance
        .get(field)
        .and_then(|origins| origins.last())
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}

fn parse_outputs(value: &str) -> Result<BTreeMap<String, OutputFact>, PackageParseError> {
    let mut outputs = BTreeMap::new();
    for (key, raw) in record_entries(value, "outputs")? {
        let output = parse_output_value(&key, &raw)?;
        if outputs.insert(key.clone(), output).is_some() {
            return Err(PackageParseError::Composition(format!(
                "output `{key}` is declared more than once"
            )));
        }
    }
    Ok(outputs)
}

fn parse_output_value(key: &str, raw: &str) -> Result<OutputFact, PackageParseError> {
    let kind_text = raw
        .split('{')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_start_matches('.')
        .trim_end_matches('.');
    let kind = PackageOutputKind::parse(kind_text)?;
    let raw_fields = named_entries_checked(record_body(raw, "output")?, &format!("outputs.{key}"))?;
    let payload = OutputPayload::Object(
        raw_fields
            .iter()
            .map(|(field, value)| Ok((field.clone(), parse_output_payload(value)?)))
            .collect::<Result<BTreeMap<_, _>, PackageParseError>>()?,
    );
    let fields = raw_fields
        .into_iter()
        .map(|(field, value)| {
            if !output_field_allowed(kind, &field) {
                return Err(PackageParseError::UnknownField(format!(
                    "outputs.{key}.{field}"
                )));
            }
            if field == "entry" && value.trim().starts_with('"') {
                return Err(PackageParseError::InvalidValue {
                    field: format!("outputs.{key}.entry"),
                    value,
                });
            }
            if field == "entry" && !is_identifier(&scalar(&value)) {
                return Err(PackageParseError::InvalidValue {
                    field: format!("outputs.{key}.entry"),
                    value,
                });
            }
            Ok((field, scalar(&value)))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let name = fields
        .get("name")
        .cloned()
        .unwrap_or_else(|| key.to_string());
    let entry = fields.get("entry").cloned();
    Ok(OutputFact {
        kind,
        name,
        entry,
        fields,
        payload,
    })
}

fn parse_output_payload(value: &str) -> Result<OutputPayload, PackageParseError> {
    let value = value.trim().trim_end_matches(',').trim();
    if value.is_empty() {
        return Ok(OutputPayload::Null);
    }
    if value == "true" {
        return Ok(OutputPayload::Bool(true));
    }
    if value == "false" {
        return Ok(OutputPayload::Bool(false));
    }
    if value == "null" {
        return Ok(OutputPayload::Null);
    }
    if value.starts_with('"') && value.ends_with('"') {
        return Ok(OutputPayload::String(scalar(value)));
    }
    if value.starts_with('[') && value.ends_with(']') {
        let body = &value[1..value.len() - 1];
        return Ok(OutputPayload::Array(
            top_level_entries(body)
                .into_iter()
                .map(|item| parse_output_payload(&item))
                .collect::<Result<Vec<_>, _>>()?,
        ));
    }
    if value.starts_with('{') || value.starts_with(".{") {
        let body = record_body(value, "output payload")?;
        let entries = named_entries_checked(body, "output payload")?;
        return Ok(OutputPayload::Object(
            entries
                .into_iter()
                .map(|(key, value)| Ok((key.trim_matches('"').to_string(), parse_output_payload(&value)?)))
                .collect::<Result<BTreeMap<_, _>, PackageParseError>>()?,
        ));
    }
    if value.parse::<i64>().is_ok() || value.parse::<f64>().is_ok() {
        return Ok(OutputPayload::Number(value.to_string()));
    }
    Ok(OutputPayload::String(scalar(value)))
}

fn parse_environments(
    value: &str,
) -> Result<BTreeMap<String, EnvironmentFact>, PackageParseError> {
    let mut environments = BTreeMap::new();
    for (key, raw) in record_entries(value, "environments")? {
        let body = record_body(&raw, "environment")?;
        let fields = named_entries_checked(body, &format!("environments.{key}"))?
            .into_iter()
            .map(|(field, value)| {
                if !matches!(field.as_str(), "name" | "tools" | "services" | "secrets") {
                    return Err(PackageParseError::UnknownField(format!(
                        "environments.{key}.{field}"
                    )));
                }
                Ok((field, value))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let name = fields
            .get("name")
            .map(|v| scalar(v))
            .unwrap_or_else(|| key.clone());
        let tools = fields
            .get("tools")
            .map(|v| parse_list(v))
            .unwrap_or_default();
        let services = fields
            .get("services")
            .map(|v| parse_services(v))
            .transpose()?
            .unwrap_or_default();
        let secrets = fields
            .get("secrets")
            .map(|v| parse_string_map("secrets", v))
            .transpose()?
            .unwrap_or_default();
        let environment = EnvironmentFact {
                name,
                tools,
                services,
                secrets,
                fields,
            };
        if environments.insert(key.clone(), environment).is_some() {
            return Err(PackageParseError::Composition(format!(
                "environment `{key}` is declared more than once"
            )));
        }
    }
    Ok(environments)
}

fn parse_services(value: &str) -> Result<BTreeMap<String, ServiceFact>, PackageParseError> {
    let mut services = BTreeMap::new();
    for (key, raw) in record_entries(value, "services")? {
        let fields = named_entries_checked(record_body(&raw, "service")?, &format!("services.{key}"))?
            .into_iter()
            .map(|(field, value)| {
                if !service_field_allowed(&field) {
                    return Err(PackageParseError::UnknownField(format!(
                        "services.{key}.{field}"
                    )));
                }
                Ok((field, value))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let enable = match fields.get("enable") {
            None => false,
            Some(value) if scalar(value) == "true" => true,
            Some(value) if scalar(value) == "false" => false,
            Some(value) => {
                return Err(PackageParseError::InvalidValue {
                    field: format!("services.{key}.enable"),
                    value: value.clone(),
                })
            }
        };
        let ports = match fields.get("ports") {
            None => Vec::new(),
            Some(value) => parse_list(value)
                .into_iter()
                .map(|port| {
                    port.parse::<i64>().map_err(|_| PackageParseError::InvalidValue {
                        field: format!("services.{key}.ports"),
                        value: port,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        let ready = fields.get("ready").map(|v| scalar(v));
        let service = ServiceFact {
                enable,
                ports,
                ready,
                fields,
            };
        if services.insert(key.clone(), service).is_some() {
            return Err(PackageParseError::Composition(format!(
                "service `{key}` is declared more than once"
            )));
        }
    }
    Ok(services)
}

fn parse_members(value: &str) -> Result<Vec<MemberRef>, PackageParseError> {
    let value = value.trim();
    if value.starts_with("find(") {
        return Ok(vec![MemberRef::Find(scalar(
            value.trim_start_matches("find(").trim_end_matches(')'),
        ))]);
    }
    if !value.starts_with('[') {
        return Err(PackageParseError::InvalidValue {
            field: "members".to_string(),
            value: value.to_string(),
        });
    }
    Ok(parse_list(value)
        .into_iter()
        .map(MemberRef::Path)
        .collect())
}

fn output_field_allowed(kind: PackageOutputKind, field: &str) -> bool {
    match kind {
        PackageOutputKind::Library => matches!(field, "name" | "modules"),
        PackageOutputKind::Executable | PackageOutputKind::Service | PackageOutputKind::Check => {
            matches!(field, "name" | "entry")
        }
        PackageOutputKind::Environment => {
            matches!(field, "name" | "tools" | "services" | "secrets")
        }
        PackageOutputKind::Image => matches!(field, "name" | "from" | "kind" | "environment"),
        PackageOutputKind::Bundle => matches!(field, "name" | "members"),
        PackageOutputKind::System => {
            matches!(field, "name" | "packages" | "services" | "options")
        }
        PackageOutputKind::Fleet => matches!(field, "name" | "hosts"),
    }
}

fn service_field_allowed(field: &str) -> bool {
    matches!(
        field,
        "enable"
            | "from"
            | "ports"
            | "ready"
            | "run"
            | "restart"
            | "watch"
            | "after"
            | "before_start"
            | "sockets"
            | "shutdown"
            | "depends_on"
            | "health"
            | "limits"
            | "logs"
    )
}

fn parse_string_map(
    field: &str,
    value: &str,
) -> Result<BTreeMap<String, String>, PackageParseError> {
    let mut map = BTreeMap::new();
    for (key, raw) in record_entries(value, field)? {
        let key = key.trim_matches('"').to_string();
        let value = scalar(&raw);
        if let Some(existing) = map.insert(key.clone(), value.clone()) {
            if existing != value {
                return Err(PackageParseError::Composition(format!(
                    "`{field}.{key}` is declared with conflicting values"
                )));
            }
        }
    }
    Ok(map)
}

fn record_entries(value: &str, field: &str) -> Result<Vec<(String, String)>, PackageParseError> {
    named_entries_checked(record_body(value, field)?, field)
}

fn record_body<'a>(value: &'a str, field: &str) -> Result<&'a str, PackageParseError> {
    let Some(open) = value.find('{') else {
        return Err(PackageParseError::MissingRecord(field.to_string()));
    };
    let Some(close) = value.rfind('}') else {
        return Err(PackageParseError::MissingRecord(field.to_string()));
    };
    Ok(&value[open + 1..close])
}

fn named_entries_checked(
    body: &str,
    scope: &str,
) -> Result<Vec<(String, String)>, PackageParseError> {
    let mut entries = Vec::new();
    let mut seen = BTreeMap::<String, String>::new();
    for entry in top_level_entries(body) {
        let Some((field, value)) = split_field(&entry) else {
            return Err(PackageParseError::MalformedField(format!("{scope}: {entry}")));
        };
        if let Some(previous) = seen.get(&field) {
            if previous.trim() != value.trim() {
                return Err(PackageParseError::Composition(format!(
                    "`{scope}.{field}` is declared with conflicting values"
                )));
            }
            continue;
        }
        seen.insert(field.clone(), value.clone());
        entries.push((field, value));
    }
    Ok(entries)
}

/// Parse the file form `pub name :: Config.{ ... }` while keeping the file
/// itself layout-neutral. The same helper also recognizes an inline Config
/// declaration in `package.jet`.
fn config_wrapper<'a>(text: &'a str) -> Result<Option<(String, &'a str)>, PackageParseError> {
    let Some(separator) = text.find("::") else {
        return Ok(None);
    };
    let name = text[..separator]
        .trim()
        .strip_prefix("pub ")
        .unwrap_or(text[..separator].trim())
        .trim();
    if name.is_empty() {
        return Err(PackageParseError::MalformedField(text.trim().to_string()));
    }
    let mut value = text[separator + 2..].trim();
    let Some(rest) = value.strip_prefix("Config") else {
        return Ok(None);
    };
    value = rest.trim_start();
    if let Some(rest) = value.strip_prefix('.') {
        value = rest.trim_start();
    }
    if !value.starts_with('{') {
        return Err(PackageParseError::MissingRecord("Config".to_string()));
    }
    let body = record_body(value, "Config")?;
    Ok(Some((name.to_string(), body)))
}

fn discover_config_path(
    dir: &std::path::Path,
    name: &str,
) -> Result<std::path::PathBuf, PackageParseError> {
    let mut matches = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|error| {
        PackageParseError::Composition(format!(
            "couldn't discover Config `{name}` in `{}`: {error}",
            dir.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            PackageParseError::Composition(format!(
                "couldn't discover Config `{name}` in `{}`: {error}",
                dir.display()
            ))
        })?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("jet") {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|error| {
            PackageParseError::Composition(format!("couldn't read Config `{}`: {error}", path.display()))
        })?;
        match ConfigFacts::parse(&text, path.display().to_string()) {
            Ok(facts) if facts.name.as_deref() == Some(name) => matches.push(path),
            Ok(_) => {}
            Err(error) if text.contains("Config") => return Err(error),
            Err(_) => {}
        }
    }
    matches.sort();
    match matches.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err(PackageParseError::Composition(format!(
            "Config `{name}` was not found under `{}`",
            dir.display()
        ))),
        _ => Err(PackageParseError::Composition(format!(
            "Config `{name}` is ambiguous under `{}`",
            dir.display()
        ))),
    }
}

fn parse_list(value: &str) -> Vec<String> {
    let value = value.trim();
    let body = value
        .strip_prefix('[')
        .and_then(|rest| rest.rfind(']').map(|end| &rest[..end]))
        .unwrap_or(value);
    top_level_entries(body)
        .into_iter()
        .map(|entry| scalar(&entry))
        .filter(|value| !value.is_empty())
        .collect()
}

fn scalar(value: &str) -> String {
    let value = value.trim().trim_end_matches(',').trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        return value[1..value.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
    }
    value.to_string()
}

fn split_field(value: &str) -> Option<(String, String)> {
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        match ch {
            '"' => quoted = true,
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth -= 1,
            ':' if depth == 0 => {
                return Some((
                    value[..index].trim().to_string(),
                    value[index + 1..].trim().to_string(),
                ))
            }
            _ => {}
        }
    }
    None
}

fn top_level_entries(value: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    for ch in value.chars() {
        if quoted {
            current.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        match ch {
            '"' => {
                quoted = true;
                current.push(ch);
            }
            '{' | '[' | '(' => {
                depth += 1;
                current.push(ch);
            }
            '}' | ']' | ')' => {
                depth -= 1;
                current.push(ch);
            }
            ',' | '\n' if depth == 0 => {
                if !current.trim().is_empty() {
                    entries.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        entries.push(current);
    }
    entries
}

fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|part| {
            let mut chars = part.chars();
            matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
                && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
        })
}

fn collect_jet_files(root: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "target" || name == "build" {
            continue;
        }
        if path.is_dir() {
            collect_jet_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jet") {
            files.push(path);
        }
    }
}

struct ParsedSource {
    path: std::path::PathBuf,
    program: crate::AST::Program,
}

fn parse_sources(files: &[std::path::PathBuf]) -> Option<Vec<ParsedSource>> {
    files
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(path).ok()?;
            let (tokens, lex_diags) = crate::Lexer::lex(&source);
            if !lex_diags.is_empty() {
                return None;
            }
            Some(ParsedSource {
                path: path.clone(),
                program: crate::Parser::parse(&tokens).ok()?,
            })
        })
        .collect()
}

fn unique_top_level_function<'a>(
    program: &'a crate::AST::Program,
    wanted: &str,
) -> Option<&'a crate::AST::Func> {
    let mut found = None;
    for item in &program.items {
        let crate::AST::Item::Func(function) = item else {
            continue;
        };
        if function.name != wanted {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(function);
    }
    found
}

fn imported_module_targets(
    root: &std::path::Path,
    source: &ParsedSource,
    wanted: &str,
    sources: &[ParsedSource],
) -> Vec<std::path::PathBuf> {
    let mut targets = Vec::new();
    for import in &source.program.imports {
        if import.import_alias() != wanted
            || matches!(&import.kind, crate::AST::ImportKind::Unqualified { .. })
        {
            continue;
        }
        for candidate in import_target_paths(root, &source.path, &import.kind) {
            if sources.iter().any(|other| other.path == candidate) {
                targets.push(candidate);
            }
        }
    }
    targets
}

fn import_target_paths(
    root: &std::path::Path,
    importer: &std::path::Path,
    kind: &crate::AST::ImportKind,
) -> Vec<std::path::PathBuf> {
    let base = match kind {
        crate::AST::ImportKind::File(path, _) => importer
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(path),
        crate::AST::ImportKind::Module(name, _) => name.split('.').fold(
            root.to_path_buf(),
            |mut path, component| {
                path.push(component);
                path
            },
        ),
        crate::AST::ImportKind::Unqualified { .. } => return Vec::new(),
    };
    let mut paths = vec![base.clone()];
    if base.extension().and_then(|ext| ext.to_str()) != Some("jet") {
        paths.push(base.with_extension("jet"));
    }
    paths.push(base.join("module.jet"));
    paths.sort();
    paths.dedup();
    paths
}

fn file_has_top_level_function(path: &std::path::Path, wanted: &str) -> bool {
    let Ok(source) = std::fs::read_to_string(path) else { return false };
    let mut depth = 0i32;
    let mut token = String::new();
    let mut saw_fn = false;
    let mut in_string = false;
    let mut escaped = false;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut previous = String::new();
    for ch in source.chars().chain(std::iter::once(' ')) {
        if line_comment {
            if ch == '\n' {
                line_comment = false;
            }
            continue;
        }
        if block_comment {
            if previous == "/" && ch == '*' {
                previous.clear();
            } else if previous == "*" && ch == '/' {
                block_comment = false;
                previous.clear();
            } else {
                previous = ch.to_string();
            }
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if previous == "/" && ch == '/' {
            line_comment = true;
            previous.clear();
            continue;
        }
        if previous == "/" && ch == '*' {
            block_comment = true;
            previous.clear();
            continue;
        }
        if ch == '"' {
            in_string = true;
            token.clear();
            previous.clear();
            continue;
        }
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
        }
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
            continue;
        }
        if !token.is_empty() {
            if depth == 0 && saw_fn && token == wanted {
                return true;
            }
            saw_fn = depth == 0 && token == "fn";
            token.clear();
        }
        previous = ch.to_string();
    }
    false
}

fn strip_comments(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut quoted = false;
    let mut escaped = false;
    let mut chars = value.chars().peekable();
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
            continue;
        }
        if ch == '"' {
            quoted = true;
            out.push(ch);
        } else if ch == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for comment_ch in chars.by_ref() {
                if comment_ch == '\n' {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("jet-package-{tag}-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_closed_output_kinds_and_selects_defaults() {
        let facts = PackageFacts::parse(
            r#"
name: "demo"
outputs: .{
    app: .Executable.{ name: "app", entry: run }
    check: .Check.{ name: "check", entry: check }
}
defaults: .{ run: app, test: check }
"#,
            "package.jet",
        )
        .unwrap();
        assert_eq!(facts.outputs["app"].kind, PackageOutputKind::Executable);
        assert_eq!(facts.select_output("run", None, None).unwrap().name, "app");
        assert_eq!(facts.select_output("test", None, None).unwrap().name, "check");
    }

    #[test]
    fn output_payload_keeps_json_shapes() {
        let facts = PackageFacts::parse(
            r#"
name: "demo"
outputs: .{
    app: .Environment.{ name: "dev", tools: ["a", "b"], services: .{ db: .{ enable: true, ports: [5432] } }, secrets: .{ token: "x" } }
}
"#,
            "package.jet",
        )
        .unwrap();
        let OutputPayload::Object(payload) = &facts.outputs["app"].payload else {
            panic!("output payload must remain an object");
        };
        assert!(matches!(payload["tools"], OutputPayload::Array(_)));
        assert!(matches!(payload["services"], OutputPayload::Object(_)));
        assert!(matches!(payload["secrets"], OutputPayload::Object(_)));
    }

    #[test]
    fn inline_and_file_configs_compose_once_and_keep_identity() {
        let dir = temp_dir("configs");
        std::fs::write(
            dir.join("package.jet"),
            r#"
name: "demo"
configs: [dev, "release.jet"]
defaults: .{ run: app }
dev :: Config.{
    version: "1"
}
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("release.jet"),
            r#"pub release :: Config.{ outputs: .{ app: .Executable.{ entry: run } } }"#,
        )
        .unwrap();
        let facts = PackageFacts::load(&dir).unwrap().unwrap();
        assert_eq!(facts.version.as_deref(), Some("1"));
        assert_eq!(facts.inline_configs["dev"].name.as_deref(), Some("dev"));
        assert_eq!(facts.defaults["run"], "app");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn conflicting_nested_fields_fail_closed() {
        let error = PackageFacts::parse(
            r#"name: "demo"
outputs: .{ app: .Executable.{ entry: run, entry: other } }"#,
            "package.jet",
        )
        .unwrap_err();
        assert!(error.to_string().contains("outputs.app.entry"));
    }

    #[test]
    fn scalar_config_conflict_names_both_sources_and_does_not_mutate() {
        let mut facts = PackageFacts::parse_uncomposed("name: \"demo\"\n", "package.jet")
            .unwrap();
        let first = ConfigFacts::parse("Config.{ version: \"1\" }", "configs/one.jet")
            .unwrap();
        let second = ConfigFacts::parse("Config.{ version: \"2\" }", "configs/two.jet")
            .unwrap();

        let error = facts.compose([first, second]).unwrap_err();
        match error {
            ComposeError::Conflict {
                field,
                left_origin,
                right_origin,
                left,
                right,
            } => {
                assert_eq!(field, "version");
                assert_eq!(left_origin, "configs/one.jet");
                assert_eq!(right_origin, "configs/two.jet");
                assert_eq!(left, "1");
                assert_eq!(right, "2");
            }
            other => panic!("expected scalar conflict, got {other:?}"),
        }
        assert_eq!(facts.version, None);
        assert!(facts.field_provenance("version").is_empty());
    }

    #[test]
    fn equal_scalar_config_contributors_keep_ordered_provenance() {
        let mut facts = PackageFacts::parse_uncomposed("name: \"demo\"\n", "package.jet")
            .unwrap();
        let first = ConfigFacts::parse("Config.{ version: \"1\" }", "configs/one.jet")
            .unwrap();
        let second = ConfigFacts::parse("Config.{ version: \"1\" }", "configs/two.jet")
            .unwrap();

        facts.compose([first, second]).unwrap();
        assert_eq!(facts.version.as_deref(), Some("1"));
        let origins = facts
            .field_provenance("version")
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(origins, ["configs/one.jet", "configs/two.jet"]);
    }

    #[test]
    fn entry_path_only_accepts_declared_top_level_function() {
        let dir = temp_dir("entry");
        std::fs::write(dir.join("main.jet"), "fn run() { print(1) }\n").unwrap();
        let facts = PackageFacts::parse(
            r#"name: "demo"
outputs: .{ app: .Executable.{ entry: run } }"#,
            "package.jet",
        )
        .unwrap();
        let output = facts.outputs.get("app").unwrap();
        assert_eq!(facts.entry_path(&dir, output), Some(dir.join("main.jet")));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn typed_output_precedes_a_legacy_run_entry() {
        let dir = temp_dir("legacy-run");
        std::fs::write(dir.join("main.jet"), "fn run() { print(1) }\n").unwrap();
        std::fs::write(dir.join("serve.jet"), "fn serve() { print(2) }\n").unwrap();
        let facts = PackageFacts::parse(
            r#"name: "demo"
outputs: .{ app: .Executable.{ entry: serve } }"#,
            "package.jet",
        )
        .unwrap();
        assert_eq!(
            facts.resolve_run_entry(&dir).unwrap(),
            Some(dir.join("serve.jet"))
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn typed_entry_ambiguity_reports_package_origin() {
        let dir = temp_dir("entry-ambiguous");
        std::fs::write(dir.join("one.jet"), "fn run() { print(1) }\n").unwrap();
        std::fs::write(dir.join("two.jet"), "fn run() { print(2) }\n").unwrap();
        let facts = PackageFacts::parse(
            r#"name: "demo"
outputs: .{ app: .Executable.{ entry: run } }"#,
            "package.jet",
        )
        .unwrap();
        let error = facts.resolve_run_entry(&dir).unwrap_err();
        assert!(error.contains("package.jet"));
        assert!(error.contains("no unique source entry"));
        std::fs::remove_dir_all(dir).ok();
    }
}
