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
    /// Parse the canonical `package.jet` root shape.
    pub fn parse(text: &str, origin: impl Into<String>) -> Result<Self, PackageParseError> {
        let facts = parse_common(text, origin.into(), false)?;
        if facts.name.is_empty() {
            return Err(PackageParseError::MissingName);
        }
        facts
            .validate_defaults()
            .map_err(|error| PackageParseError::Composition(error.to_string()))?;
        Ok(facts)
    }

    /// Load `package.jet`, falling back to the migration-era `pkg.jet`.
    pub fn load(dir: &std::path::Path) -> Option<Result<Self, PackageParseError>> {
        let canonical = dir.join("package.jet");
        let legacy = dir.join("pkg.jet");
        let path = if canonical.is_file() { canonical } else { legacy };
        let text = std::fs::read_to_string(&path).ok()?;
        Some(Self::parse(&text, path.display().to_string()).and_then(|mut facts| {
            facts.compose_configs(dir)?;
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
        for config in configs {
            merge_common(
                self,
                config.version,
                config.source,
                config.deps,
                config.services,
                config.outputs,
                config.environments,
                config.defaults,
                &config.origin,
            )?;
        }
        self.validate_defaults()?;
        Ok(())
    }

    pub fn validate_defaults(&self) -> Result<(), ComposeError> {
        for (intent, output) in &self.defaults {
            let Some(fact) = self.outputs.get(output) else {
                return Err(ComposeError::UnknownDefault {
                    intent: intent.clone(),
                    output: output.clone(),
                });
            };
            if matches!(intent.as_str(), "run" | "dev") && !fact.kind.is_runnable() {
                return Err(ComposeError::Conflict {
                    field: format!("defaults.{intent}"),
                    left_origin: self.origin.clone(),
                    right_origin: self.origin.clone(),
                    left: output.clone(),
                    right: "default must select an Executable or Service output".to_string(),
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
        let selected = explicit
            .map(str::to_string)
            .or_else(|| legacy.map(str::to_string))
            .or_else(|| {
                let mut compatible = self
                    .outputs
                    .iter()
                    .filter(|(_, output)| match intent {
                        "run" | "dev" => output.kind.is_runnable(),
                        "test" => output.kind == PackageOutputKind::Check,
                        _ => true,
                    })
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>();
                (compatible.len() == 1).then(|| compatible.pop().unwrap())
            })
            .or_else(|| self.defaults.get(intent).cloned());
        let Some(selected) = selected else {
            return Err(ComposeError::Conflict {
                field: format!("outputs.{intent}"),
                left_origin: self.origin.clone(),
                right_origin: self.origin.clone(),
                left: "none".to_string(),
                right: "no compatible output was selected".to_string(),
            });
        };
        let Some(output) = self.outputs.get(&selected) else {
            return Err(ComposeError::UnknownDefault {
                intent: intent.to_string(),
                output: selected,
            });
        };
        if matches!(intent, "run" | "dev") && !output.kind.is_runnable() {
            return Err(ComposeError::Conflict {
                field: format!("outputs.{intent}"),
                left_origin: self.origin.clone(),
                right_origin: self.origin.clone(),
                left: output.name.clone(),
                right: "selected output is not runnable".to_string(),
            });
        }
        Ok(output)
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
        let name = entry.rsplit('.').next().unwrap_or(entry);
        if !is_identifier(name) {
            return None;
        }
        let mut files = Vec::new();
        collect_jet_files(root, &mut files);
        files.sort();
        files
            .into_iter()
            .find(|path| file_has_top_level_function(path, name))
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
            if matches!(member, MemberRef::Find(_)) && !path.is_dir() {
                return Err(PackageParseError::Composition(format!(
                    "member discovery reference `{relative}` is not a directory"
                )));
            }
        }
        Ok(())
    }
}

impl ConfigFacts {
    pub fn parse(text: &str, origin: impl Into<String>) -> Result<Self, PackageParseError> {
        let origin = origin.into();
        let stripped = strip_comments(text);
        let (declared_name, body) = match config_wrapper(&stripped)? {
            Some((name, body)) => (Some(name), body),
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

fn parse_common(
    text: &str,
    origin: String,
    config: bool,
) -> Result<PackageFacts, PackageParseError> {
    let mut facts = PackageFacts {
        origin,
        ..PackageFacts::default()
    };
    let mut seen = BTreeMap::<String, String>::new();
    for entry in top_level_entries(&strip_comments(text)) {
        if !config {
            if let Some((name, _)) = config_wrapper(&entry)? {
                let contribution_origin = format!("{}::{name}", facts.origin);
                let contribution = ConfigFacts::parse(&entry, contribution_origin)?;
                merge_common(
                    &mut facts,
                    contribution.version.clone(),
                    contribution.source.clone(),
                    contribution.deps.clone(),
                    contribution.services.clone(),
                    contribution.outputs.clone(),
                    contribution.environments.clone(),
                    contribution.defaults.clone(),
                    &contribution.origin,
                )
                .map_err(|error| PackageParseError::Composition(error.to_string()))?;
                facts.inline_configs.insert(name, contribution);
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
            continue;
        }
        match field.as_str() {
            "name" => facts.name = scalar(&value),
            "version" => facts.version = Some(scalar(&value)),
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
            "configs" => facts.configs = parse_list(&value),
            "description" | "license" | "edition" | "repository" => {}
            other => return Err(PackageParseError::UnknownField(other.to_string())),
        }
    }
    Ok(facts)
}

fn merge_common(
    root: &mut PackageFacts,
    version: Option<String>,
    source: Option<String>,
    deps: BTreeMap<String, String>,
    services: BTreeMap<String, ServiceFact>,
    outputs: BTreeMap<String, OutputFact>,
    environments: BTreeMap<String, EnvironmentFact>,
    defaults: BTreeMap<String, String>,
    origin: &str,
) -> Result<(), ComposeError> {
    merge_optional("version", &mut root.version, version, &root.origin, origin)?;
    merge_optional("source", &mut root.source, source, &root.origin, origin)?;
    merge_map("deps", &mut root.deps, deps, &root.origin, origin)?;
    merge_map("services", &mut root.services, services, &root.origin, origin)?;
    merge_map("outputs", &mut root.outputs, outputs, &root.origin, origin)?;
    merge_map(
        "environments",
        &mut root.environments,
        environments,
        &root.origin,
        origin,
    )?;
    merge_map("defaults", &mut root.defaults, defaults, &root.origin, origin)
}

fn merge_optional(
    field: &str,
    current: &mut Option<String>,
    incoming: Option<String>,
    left_origin: &str,
    right_origin: &str,
) -> Result<(), ComposeError> {
    let Some(incoming) = incoming else { return Ok(()) };
    match current {
        None => *current = Some(incoming),
        Some(existing) if existing == &incoming => {}
        Some(existing) => {
            return Err(ComposeError::Conflict {
                field: field.to_string(),
                left_origin: left_origin.to_string(),
                right_origin: right_origin.to_string(),
                left: existing.clone(),
                right: incoming,
            })
        }
    }
    Ok(())
}

fn merge_map<T: PartialEq + Clone + fmt::Debug>(
    field: &str,
    current: &mut BTreeMap<String, T>,
    incoming: BTreeMap<String, T>,
    left_origin: &str,
    right_origin: &str,
) -> Result<(), ComposeError> {
    for (key, value) in incoming {
        match current.get(&key) {
            None => {
                current.insert(key, value);
            }
            Some(existing) if existing == &value => {}
            Some(existing) => {
                return Err(ComposeError::Conflict {
                    field: format!("{field}.{key}"),
                    left_origin: left_origin.to_string(),
                    right_origin: right_origin.to_string(),
                    left: format!("{existing:?}"),
                    right: format!("{value:?}"),
                })
            }
        }
    }
    Ok(())
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
    let fields = named_entries_checked(record_body(raw, "output")?, &format!("outputs.{key}"))?
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
    })
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
    fn inline_and_file_configs_compose_once_and_keep_identity() {
        let dir = temp_dir("configs");
        std::fs::write(
            dir.join("package.jet"),
            r#"
name: "demo"
configs: [dev, "release.jet"]
dev :: Config.{
    version: "1"
    outputs: .{ app: .Executable.{ entry: run } }
}
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("release.jet"),
            r#"pub release :: Config.{ defaults: .{ run: app } }"#,
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
}
